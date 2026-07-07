mod audio_scrub;
mod preview;

use audio_scrub::AudioScrubEngine;
use parking_lot::Mutex;
use preview::{NativeWindow, PreviewBounds, PreviewPanel};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};
use uppercut_core::{
    apply_command as apply_core_command, commands::ExportPreset, export::render_frame_at,
    project::Project, Command, ExportSettings,
};

struct Session {
    path: PathBuf,
    project: Project,
}

pub struct AppState {
    session: Mutex<Option<Session>>,
    preview: Mutex<PreviewPanel>,
    parent_attached: Mutex<bool>,
    audio: AudioScrubEngine,
}

impl AppState {
    fn new() -> Self {
        Self {
            session: Mutex::new(None),
            preview: Mutex::new(PreviewPanel::new()),
            parent_attached: Mutex::new(false),
            audio: AudioScrubEngine::new(),
        }
    }

    fn with_session<F, T>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce(&mut Session) -> Result<T, String>,
    {
        let mut guard = self.session.lock();
        let session = guard
            .as_mut()
            .ok_or_else(|| "no project open".to_string())?;
        f(session)
    }

    fn save(session: &Session) -> Result<(), String> {
        let data = serde_json::to_string_pretty(&session.project).map_err(|e| e.to_string())?;
        std::fs::write(&session.path, data).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
fn native_window_from_app(app: &AppHandle) -> Result<NativeWindow, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    let handle = window
        .window_handle()
        .map_err(|e| format!("window handle: {e}"))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(NativeWindow { hwnd: h.hwnd.get() }),
        other => Err(format!("unsupported window handle: {other:?}")),
    }
}

#[cfg(not(windows))]
fn native_window_from_app(_app: &AppHandle) -> Result<NativeWindow, String> {
    Err("native preview requires Windows in Phase 2 v1".into())
}

fn ensure_preview_parent(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let mut attached = state.parent_attached.lock();
    if *attached {
        return Ok(());
    }
    let parent = native_window_from_app(app)?;
    state.preview.lock().attach_parent(parent);
    *attached = true;
    Ok(())
}

fn default_projects_dir() -> Result<PathBuf, String> {
    let home = if cfg!(windows) {
        std::env::var("USERPROFILE")
    } else {
        std::env::var("HOME")
    }
    .map_err(|e| format!("home directory: {e}"))?;
    Ok(PathBuf::from(home).join("Documents").join("Uppercut"))
}

#[tauri::command]
fn quick_start_project(app: AppHandle, state: State<AppState>) -> Result<String, String> {
    use uppercut_core::project::Settings;

    let dir = default_projects_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let path_buf = dir.join(format!("Untitled {ts}.uppercut.json"));
    let project = Project::new(
        "Untitled edit",
        Settings {
            fps: 60.0,
            width: 1080,
            height: 1920,
            sample_rate: 48000,
            duck_db: -12.0,
        },
    );
    let data = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
    std::fs::write(&path_buf, data).map_err(|e| e.to_string())?;
    *state.session.lock() = Some(Session {
        path: path_buf.clone(),
        project,
    });
    ensure_preview_parent(&app, &state)?;
    Ok(path_buf.to_string_lossy().into_owned())
}

#[tauri::command]
fn new_project(
    app: AppHandle,
    state: State<AppState>,
    path: String,
    name: String,
) -> Result<(), String> {
    use uppercut_core::project::Settings;

    let path_buf = PathBuf::from(&path);
    let project = Project::new(
        name,
        Settings {
            fps: 60.0,
            width: 1080,
            height: 1920,
            sample_rate: 48000,
            duck_db: -12.0,
        },
    );
    let data = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
    std::fs::write(&path_buf, data).map_err(|e| e.to_string())?;
    *state.session.lock() = Some(Session {
        path: path_buf,
        project,
    });
    ensure_preview_parent(&app, &state)?;
    Ok(())
}

#[tauri::command]
fn open_project(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);
    let data = std::fs::read_to_string(&path_buf).map_err(|e| e.to_string())?;
    let project: Project = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    *state.session.lock() = Some(Session {
        path: path_buf,
        project,
    });
    ensure_preview_parent(&app, &state)?;
    Ok(())
}

#[tauri::command]
fn save_project(state: State<AppState>) -> Result<(), String> {
    state.with_session(|session| AppState::save(session))
}

#[tauri::command]
fn get_project(state: State<AppState>) -> Result<Project, String> {
    state.with_session(|session| Ok(session.project.clone()))
}

#[tauri::command]
fn apply_command(state: State<AppState>, command: serde_json::Value) -> Result<String, String> {
    let cmd: Command =
        serde_json::from_value(command).map_err(|e| format!("invalid command: {e}"))?;
    state.with_session(|session| {
        let outcome = apply_core_command(&mut session.project, cmd).map_err(|e| e.to_string())?;
        AppState::save(session)?;
        Ok(format!("{outcome:?}"))
    })
}

#[tauri::command]
fn export_project(
    state: State<AppState>,
    output_path: String,
    preset: String,
) -> Result<(), String> {
    let preset = match preset.as_str() {
        "tiktok" => ExportPreset::TikTok9x16,
        "youtube" => ExportPreset::Youtube16x9,
        other => return Err(format!("unknown preset '{other}'")),
    };
    state.with_session(|session| {
        apply_core_command(
            &mut session.project,
            Command::Export {
                output_path: output_path.clone(),
                preset,
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
fn set_preview_bounds(
    app: AppHandle,
    state: State<AppState>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    ensure_preview_parent(&app, &state)?;
    state
        .preview
        .lock()
        .set_bounds(PreviewBounds {
            x,
            y,
            width,
            height,
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_preview(app: AppHandle, state: State<AppState>, time_secs: f64) -> Result<(), String> {
    ensure_preview_parent(&app, &state)?;
    let (rgba, width, height) = state.with_session(|session| {
        let settings = ExportSettings {
            width: session.project.settings.width,
            height: session.project.settings.height,
            fps: session.project.settings.fps,
        };
        let pixels =
            render_frame_at(&session.project, time_secs, settings).map_err(|e| e.to_string())?;
        Ok((pixels, settings.width, settings.height))
    })?;

    state
        .preview
        .lock()
        .present_rgba(&rgba, width, height)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn start_playback(state: State<AppState>, time_secs: f64) -> Result<(), String> {
    state.with_session(|session| {
        state.audio.start_playback(
            session.project.clone(),
            time_secs,
            session.project.settings.fps,
        )
    })
}

#[tauri::command]
fn stop_playback(state: State<AppState>) {
    state.audio.stop();
}

#[tauri::command]
fn scrub_audio(state: State<AppState>, time_secs: f64) -> Result<(), String> {
    state.with_session(|session| state.audio.play_once(&session.project, time_secs))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            quick_start_project,
            new_project,
            open_project,
            save_project,
            get_project,
            apply_command,
            export_project,
            set_preview_bounds,
            update_preview,
            start_playback,
            stop_playback,
            scrub_audio,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Uppercut");
}
