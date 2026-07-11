use super::gfx::GfxState;
use super::{PreviewBounds, PreviewError};
use raw_window_handle::{HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowsDisplayHandle};
use std::sync::{Once, OnceLock};

static PREVIEW_CLASS: OnceLock<Vec<u16>> = OnceLock::new();
static REGISTER_CLASS: Once = Once::new();

fn preview_class_name() -> &'static [u16] {
    PREVIEW_CLASS.get_or_init(|| wide("RenderlyPreviewPanel"))
}

fn register_preview_class() {
    REGISTER_CLASS.call_once(|| {
        use windows::core::PCWSTR;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            RegisterClassW, CS_HREDRAW, CS_VREDRAW, WNDCLASSW,
        };

        let class_name = preview_class_name();
        unsafe {
            let hinstance = GetModuleHandleW(None).expect("module handle");
            let class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(def_preview_wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassW(&class);
        }
    });
}

#[derive(Clone, Copy, Debug)]
pub struct NativeWindow {
    pub hwnd: isize,
}

pub struct PlatformPreview {
    parent: Option<isize>,
    child: Option<isize>,
    gfx: Option<GfxState>,
}

impl PlatformPreview {
    pub fn new() -> Self {
        Self {
            parent: None,
            child: None,
            gfx: None,
        }
    }

    pub fn attach_parent(&mut self, parent: NativeWindow) {
        self.parent = Some(parent.hwnd);
    }

    pub fn set_bounds(&mut self, bounds: PreviewBounds) -> Result<(), PreviewError> {
        let parent = self.parent.ok_or(PreviewError::NotInitialized)?;
        if bounds.width == 0 || bounds.height == 0 {
            eprintln!(
                "preview: set_bounds got a zero dimension ({}x{} at {},{}), skipping — \
                 present_rgba will report NotInitialized until a non-zero call arrives",
                bounds.width, bounds.height, bounds.x, bounds.y
            );
            return Ok(());
        }

        // Bounds come from the webview's getBoundingClientRect — already in the
        // parent client coordinate space. Do not run ScreenToClient on them.
        let x = bounds.x;
        let y = bounds.y;
        let child = ensure_child_window(self.child, parent, x, y, bounds.width, bounds.height)?;
        self.child = Some(child);

        if self.gfx.is_none() {
            match preview_gfx_state(child, bounds.width, bounds.height) {
                Ok(gfx) => self.gfx = Some(gfx),
                Err(e) => {
                    eprintln!("preview: GfxState::new failed: {e}");
                    return Err(e);
                }
            }
        } else if let Some(gfx) = &mut self.gfx {
            if let Err(e) = gfx.resize(bounds.width, bounds.height) {
                eprintln!("preview: resize failed ({e}), recreating GfxState");
                self.gfx = Some(preview_gfx_state(child, bounds.width, bounds.height)?);
            }
        }
        Ok(())
    }

    pub fn present_rgba(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), PreviewError> {
        let gfx = self.gfx.as_mut().ok_or(PreviewError::NotInitialized)?;
        gfx.present_rgba(pixels, width, height)
    }
}

fn preview_gfx_state(hwnd: isize, width: u32, height: u32) -> Result<GfxState, PreviewError> {
    let window_handle = PreviewWindowHandle(hwnd)
        .window_handle()
        .map_err(|e| PreviewError::Wgpu(e.to_string()))?
        .as_raw();
    GfxState::new(
        RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
        window_handle,
        width,
        height,
    )
}

struct PreviewWindowHandle(isize);

impl raw_window_handle::HasWindowHandle for PreviewWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::{Win32WindowHandle, WindowHandle};
        use std::num::NonZeroIsize;

        let hwnd = NonZeroIsize::new(self.0).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = Win32WindowHandle::new(hwnd);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

fn ensure_child_window(
    existing: Option<isize>,
    parent: isize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<isize, PreviewError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        GWL_EXSTYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOW,
        WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT, WS_VISIBLE,
    };

    register_preview_class();
    let class_name = preview_class_name();

    unsafe {
        if let Some(hwnd) = existing {
            let child = HWND(hwnd as *mut _);
            let ex = GetWindowLongPtrW(child, GWL_EXSTYLE) as u32;
            let want = ex | WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0;
            if ex != want {
                SetWindowLongPtrW(child, GWL_EXSTYLE, want as isize);
            }
            let _ = SetWindowPos(
                child,
                Some(HWND_TOP),
                x,
                y,
                width as i32,
                height as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
            );
            return Ok(hwnd);
        }

        let hinstance = GetModuleHandleW(None).map_err(|e| PreviewError::Wgpu(e.to_string()))?;
        // WS_EX_TRANSPARENT: preview is display-only; clicks pass through to the webview.
        let child = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_TRANSPARENT.0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(wide("Preview").as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            x,
            y,
            width as i32,
            height as i32,
            Some(HWND(parent as *mut _)),
            None,
            Some(hinstance.into()),
            None,
        )
        .map_err(|e| PreviewError::Wgpu(e.to_string()))?;

        let _ = ShowWindow(child, SW_SHOW);
        Ok(child.0 as isize)
    }
}

unsafe extern "system" fn def_preview_wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_DESTROY};

    if msg == WM_DESTROY {
        return windows::Win32::Foundation::LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}
