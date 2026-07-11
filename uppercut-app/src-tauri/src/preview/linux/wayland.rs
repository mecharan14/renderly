//! Wayland native preview via `wl_subsurface`.
//!
//! Creates our own `wl_surface`, attaches it as a desync subsurface of the GTK/webview
//! parent surface, positions it at the letterboxed preview rect, sets an empty input
//! region (click-through), and presents into it with the shared wgpu `GfxState`.
//!
//! Caveat: `wl_subsurface.set_position` is parent-synchronized — it applies on the parent
//! surface's next commit (owned by GTK/webkit). Child *content* updates immediately via
//! `set_desync`, but repositioning during window/panel resize may lag by a GTK frame.
//! We deliberately do not commit the parent surface ourselves.

use super::super::gfx::GfxState;
use super::super::{PreviewBounds, PreviewError};
use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle,
    WaylandWindowHandle,
};
use std::os::raw::c_void;
use std::ptr::NonNull;
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::protocol::{
    wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry,
    wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface,
};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    Connection, Dispatch, Proxy, QueueHandle,
};

#[derive(Clone, Copy, Debug)]
pub struct Parent {
    pub display: *mut c_void,
    pub surface: *mut c_void,
}

/// Event-queue state for Wayland setup. All events are no-ops; we only need the
/// `Dispatch` impls so `create_surface` / `get_subsurface` / `create_region` can run.
struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlCompositor,
        _: <WlCompositor as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSubcompositor, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSubcompositor,
        _: <WlSubcompositor as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSurface,
        _: <WlSurface as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSubsurface, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSubsurface,
        _: <WlSubsurface as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlRegion, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlRegion,
        _: <WlRegion as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

pub struct Preview {
    parent: Parent,
    conn: Option<Connection>,
    child: Option<WlSurface>,
    subsurface: Option<WlSubsurface>,
    /// Kept alive so the empty input region stays valid for the child surface.
    _input_region: Option<WlRegion>,
    /// Held so the queue handle backing our proxies stays valid.
    _event_queue: Option<wayland_client::EventQueue<State>>,
    gfx: Option<GfxState>,
}

impl Preview {
    pub fn new(parent: Parent) -> Self {
        Self {
            parent,
            conn: None,
            child: None,
            subsurface: None,
            _input_region: None,
            _event_queue: None,
            gfx: None,
        }
    }

    pub fn set_bounds(&mut self, bounds: PreviewBounds) -> Result<(), PreviewError> {
        if bounds.width == 0 || bounds.height == 0 {
            eprintln!(
                "preview: set_bounds got a zero dimension ({}x{} at {},{}), skipping — \
                 present_rgba will report NotInitialized until a non-zero call arrives",
                bounds.width, bounds.height, bounds.x, bounds.y
            );
            return Ok(());
        }

        if self.child.is_none() {
            self.create_subsurface(bounds)?;
        } else {
            self.reposition(bounds)?;
        }
        Ok(())
    }

    fn create_subsurface(&mut self, bounds: PreviewBounds) -> Result<(), PreviewError> {
        if self.parent.display.is_null() || self.parent.surface.is_null() {
            return Err(PreviewError::Wgpu(
                "wayland parent display/surface is null".into(),
            ));
        }

        // Guest connection into GTK's existing wl_display — we must not close it on drop.
        let backend = unsafe { Backend::from_foreign_display(self.parent.display.cast()) };
        let conn = Connection::from_backend(backend);

        let (globals, mut event_queue) = registry_queue_init::<State>(&conn)
            .map_err(|e| PreviewError::Wgpu(format!("wayland registry: {e}")))?;
        let qh = event_queue.handle();

        let compositor: WlCompositor = globals
            .bind(&qh, 1..=6, ())
            .map_err(|e| PreviewError::Wgpu(format!("bind wl_compositor: {e}")))?;
        let subcompositor: WlSubcompositor = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| PreviewError::Wgpu(format!("bind wl_subcompositor: {e}")))?;

        // Wrap the foreign GTK parent surface without taking ownership.
        let parent_id = unsafe {
            ObjectId::from_ptr(WlSurface::interface(), self.parent.surface.cast())
                .map_err(|e| PreviewError::Wgpu(format!("parent surface ObjectId: {e}")))?
        };
        let parent_surface = WlSurface::from_id(&conn, parent_id)
            .map_err(|e| PreviewError::Wgpu(format!("parent WlSurface: {e}")))?;

        let child = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&child, &parent_surface, &qh, ());
        subsurface.set_position(bounds.x, bounds.y);
        subsurface.set_desync();

        // Empty input region → clicks pass through to the webview chrome.
        let region = compositor.create_region(&qh, ());
        child.set_input_region(Some(&region));
        child.commit();

        let mut state = State;
        let _ = event_queue.roundtrip(&mut state);
        conn.flush()
            .map_err(|e| PreviewError::Wgpu(format!("wayland flush: {e}")))?;

        let gfx = preview_gfx_state(
            self.parent.display,
            child.id().as_ptr().cast(),
            bounds.width,
            bounds.height,
        )?;

        self.conn = Some(conn);
        self.child = Some(child);
        self.subsurface = Some(subsurface);
        self._input_region = Some(region);
        self._event_queue = Some(event_queue);
        self.gfx = Some(gfx);
        Ok(())
    }

    fn reposition(&mut self, bounds: PreviewBounds) -> Result<(), PreviewError> {
        if let Some(subsurface) = &self.subsurface {
            subsurface.set_position(bounds.x, bounds.y);
        }
        if let Some(gfx) = &mut self.gfx {
            if let Err(e) = gfx.resize(bounds.width, bounds.height) {
                eprintln!("preview: wayland resize failed ({e}), recreating GfxState");
                let child_ptr = self
                    .child
                    .as_ref()
                    .ok_or(PreviewError::NotInitialized)?
                    .id()
                    .as_ptr()
                    .cast();
                self.gfx = Some(preview_gfx_state(
                    self.parent.display,
                    child_ptr,
                    bounds.width,
                    bounds.height,
                )?);
            }
        }
        if let Some(conn) = &self.conn {
            let _ = conn.flush();
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
        gfx.present_rgba(pixels, width, height)?;
        if let Some(conn) = &self.conn {
            let _ = conn.flush();
        }
        Ok(())
    }
}

fn preview_gfx_state(
    display: *mut c_void,
    surface: *mut c_void,
    width: u32,
    height: u32,
) -> Result<GfxState, PreviewError> {
    let handle = WaylandPreviewHandle { display, surface };
    let window_handle = handle
        .window_handle()
        .map_err(|e| PreviewError::Wgpu(e.to_string()))?
        .as_raw();
    let display_handle = handle
        .display_handle()
        .map_err(|e| PreviewError::Wgpu(e.to_string()))?
        .as_raw();
    GfxState::new(display_handle, window_handle, width, height)
}

struct WaylandPreviewHandle {
    display: *mut c_void,
    surface: *mut c_void,
}

impl HasWindowHandle for WaylandPreviewHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let surface =
            NonNull::new(self.surface).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = WaylandWindowHandle::new(surface);
        Ok(
            unsafe {
                raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Wayland(handle))
            },
        )
    }
}

impl HasDisplayHandle for WaylandPreviewHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let display =
            NonNull::new(self.display).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = WaylandDisplayHandle::new(display);
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::Wayland(handle))
        })
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        // Destroy role object before the surface (protocol requirement).
        if let Some(subsurface) = self.subsurface.take() {
            subsurface.destroy();
        }
        if let Some(child) = self.child.take() {
            child.destroy();
        }
        if let Some(region) = self._input_region.take() {
            region.destroy();
        }
        if let Some(conn) = &self.conn {
            let _ = conn.flush();
        }
        // Drop gfx before connection so the wgpu surface is released first.
        self.gfx = None;
        self._event_queue = None;
        self.conn = None;
    }
}
