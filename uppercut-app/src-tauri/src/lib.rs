mod playback;
mod preview;

use parking_lot::Mutex;
use playback::PlaybackEngine;
use preview::{NativeWindow, PreviewBounds, PreviewPanel};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};
use uppercut_core::{
    apply_command as apply_core_command, commands::ExportPreset, project::Project, Command,
};

struct Session {
    path: PathBuf,
    project: Project,
}

/// Undo/redo stack over full `Project` snapshots — session-layer state management, not a
/// second edit path: every entry is a project state that only ever arose from a successful
/// `apply_command` call (or a prior undo/redo), so `apply_command` remains the sole way a
/// project's *contents* change. See docs/architecture.md "Undo/redo" for the full
/// rationale required by AGENTS.md.
struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
}

const HISTORY_CAP: usize = 100;

impl History {
    fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Push onto the bounded undo stack, evicting the oldest entry past `HISTORY_CAP`.
    fn push_undo_bounded(&mut self, project: Project) {
        self.undo.push(project);
        if self.undo.len() > HISTORY_CAP {
            self.undo.remove(0);
        }
    }

    /// Push a pre-mutation snapshot and drop the (now-stale) redo branch — call this for
    /// a genuinely new edit, not for `redo()`'s own bookkeeping (see `push_undo_bounded`).
    fn push_undo(&mut self, project: Project) {
        self.push_undo_bounded(project);
        self.redo.clear();
    }

    fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn status(&self) -> HistoryStatus {
        HistoryStatus {
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
struct HistoryStatus {
    can_undo: bool,
    can_redo: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ProjectChanged {
    revision: u64,
    can_undo: bool,
    can_redo: bool,
}

pub struct AppState {
    session: Mutex<Option<Session>>,
    preview: Mutex<PreviewPanel>,
    parent_attached: Mutex<bool>,
    playback: PlaybackEngine,
    history: Mutex<History>,
    revision: AtomicU64,
    /// Serializes every whole-project mutation (`apply_command`, `apply_commands`, `undo`,
    /// `redo`, and project open/create/quick-start) end-to-end — snapshot, compute,
    /// history push, session write-back, and save all happen while this is held. Without
    /// it, two overlapping calls (e.g. a double-tapped Ctrl+Z firing two `undo` invokes
    /// before the first resolves) could each read the same pre-mutation project, race on
    /// which write-back lands last, and silently corrupt the undo/redo stacks. A
    /// `tokio`-backed async mutex (via `tauri::async_runtime`) rather than `parking_lot`,
    /// since it must be held across `.await` points (the `spawn_blocking` compute step).
    edit_lock: tauri::async_runtime::Mutex<()>,
}

impl AppState {
    fn new() -> Self {
        Self {
            session: Mutex::new(None),
            preview: Mutex::new(PreviewPanel::new()),
            parent_attached: Mutex::new(false),
            playback: PlaybackEngine::new(),
            history: Mutex::new(History::new()),
            revision: AtomicU64::new(0),
            edit_lock: tauri::async_runtime::Mutex::new(()),
        }
    }

    /// Clone/mutate the project under a short-lived lock only — never hold this across
    /// file I/O, media decode, or other blocking work (see docs/architecture.md
    /// "Playback engine" for why the old sync-command design froze the UI thread).
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

    /// Write `project` into the session and persist it to disk. The `session` lock is
    /// held only long enough to swap in the new project and read the save path — the
    /// actual (blocking) serialize+write runs in `spawn_blocking`, outside any lock. This
    /// used to run `std::fs::write` synchronously while still holding `session`'s lock
    /// (directly contradicting `with_session`'s own documented invariant above), which
    /// stalled every other session-locking command (`play`/`seek`/`scrub_audio`/
    /// `get_project`) behind a disk write on every single edit.
    async fn commit_project(&self, project: Project) -> Result<(), String> {
        let path = {
            let mut guard = self.session.lock();
            let session = guard
                .as_mut()
                .ok_or_else(|| "no project open".to_string())?;
            let path = session.path.clone();
            session.project = project.clone();
            path
        };
        tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
            let data = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
            std::fs::write(&path, data).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// Bump the revision counter, emit `project:changed`, and return the current
    /// undo/redo availability (every mutating command, undo, and redo call this once).
    fn emit_project_changed(&self, app: &AppHandle) -> HistoryStatus {
        let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        let status = self.history.lock().status();
        let _ = app.emit(
            "project:changed",
            ProjectChanged {
                revision,
                can_undo: status.can_undo,
                can_redo: status.can_redo,
            },
        );
        status
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
    let parent = native_window_from_app(app).inspect_err(|e| {
        eprintln!("preview: failed to attach parent window: {e}");
    })?;
    eprintln!("preview: attached parent hwnd {}", parent.hwnd);
    state.preview.lock().attach_parent(parent);
    *attached = true;
    Ok(())
}

/// Stops any active playback session, joining its worker thread from `spawn_blocking`
/// rather than the calling async command's own worker thread. `PlaybackEngine::stop()`
/// can block for as long as an in-flight audio premix takes (multi-second, on a long
/// timeline) if called right after `play()` starts — running that wait inline in an async
/// command handler stalls whichever tokio worker thread is executing it, and everything
/// else queued behind that same worker.
async fn stop_playback_blocking(app: &AppHandle) {
    let app = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().playback.stop();
    })
    .await;
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
async fn quick_start_project(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    use uppercut_core::project::Settings;

    let _edit_guard = state.edit_lock.lock().await;
    stop_playback_blocking(&app).await;
    // Checked before any session/file mutation below: on a failure here (guaranteed on
    // non-Windows builds, possible transiently on Windows if the main window isn't ready
    // yet), we must not have already written a project file or set `state.session` — the
    // frontend sees this error and assumes no project is open, so the backend can't be
    // left holding one anyway (or, on the create-project commands, a real file already
    // sitting on disk that the project that "failed to open" never gets to reference).
    ensure_preview_parent(&app, &state)?;

    let dir = default_projects_dir()?;
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

    let write_path = path_buf.clone();
    let write_project = project.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let data = serde_json::to_string_pretty(&write_project).map_err(|e| e.to_string())?;
        std::fs::write(&write_path, data).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    *state.session.lock() = Some(Session {
        path: path_buf.clone(),
        project,
    });
    state.history.lock().clear();
    state.emit_project_changed(&app);
    Ok(path_buf.to_string_lossy().into_owned())
}

#[tauri::command]
async fn new_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<(), String> {
    use uppercut_core::project::Settings;

    let _edit_guard = state.edit_lock.lock().await;
    stop_playback_blocking(&app).await;
    ensure_preview_parent(&app, &state)?;

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

    let write_path = path_buf.clone();
    let write_project = project.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let data = serde_json::to_string_pretty(&write_project).map_err(|e| e.to_string())?;
        std::fs::write(&write_path, data).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    *state.session.lock() = Some(Session {
        path: path_buf,
        project,
    });
    state.history.lock().clear();
    state.emit_project_changed(&app);
    Ok(())
}

#[tauri::command]
async fn open_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let _edit_guard = state.edit_lock.lock().await;
    stop_playback_blocking(&app).await;
    ensure_preview_parent(&app, &state)?;

    let path_buf = PathBuf::from(&path);
    let read_path = path_buf.clone();
    let project: Project =
        tauri::async_runtime::spawn_blocking(move || -> Result<Project, String> {
            let data = std::fs::read_to_string(&read_path).map_err(|e| e.to_string())?;
            serde_json::from_str(&data).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;

    *state.session.lock() = Some(Session {
        path: path_buf,
        project,
    });
    state.history.lock().clear();
    state.emit_project_changed(&app);
    Ok(())
}

#[tauri::command]
async fn save_project(state: State<'_, AppState>) -> Result<(), String> {
    let (path, project) = state.with_session(|s| Ok((s.path.clone(), s.project.clone())))?;
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let data = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
        std::fs::write(&path, data).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_project(state: State<'_, AppState>) -> Result<Project, String> {
    state.with_session(|session| Ok(session.project.clone()))
}

#[tauri::command]
async fn apply_command(
    app: AppHandle,
    state: State<'_, AppState>,
    command: serde_json::Value,
) -> Result<String, String> {
    let _edit_guard = state.edit_lock.lock().await;
    let cmd: Command =
        serde_json::from_value(command).map_err(|e| format!("invalid command: {e}"))?;
    let before = state.with_session(|s| Ok(s.project.clone()))?;
    let mut project = before.clone();

    let (outcome, project) = tauri::async_runtime::spawn_blocking(move || {
        let outcome = apply_core_command(&mut project, cmd);
        (outcome, project)
    })
    .await
    .map_err(|e| e.to_string())?;
    let outcome = outcome.map_err(|e| e.to_string())?;

    state.history.lock().push_undo(before);
    state.commit_project(project).await?;
    state.emit_project_changed(&app);
    Ok(format!("{outcome:?}"))
}

/// Apply a batch of commands atomically: all-or-nothing against a single project clone,
/// one undo snapshot, one save, one `project:changed` emit — used for gestures that are
/// logically a single edit but need more than one `Command` (e.g. CapCut-style
/// auto-track-on-drop: `AddTrack` + `AddClip` should undo together, not as two steps).
#[tauri::command]
async fn apply_commands(
    app: AppHandle,
    state: State<'_, AppState>,
    commands: Vec<serde_json::Value>,
) -> Result<Vec<String>, String> {
    let _edit_guard = state.edit_lock.lock().await;
    let cmds: Vec<Command> = commands
        .into_iter()
        .map(|c| serde_json::from_value(c).map_err(|e| format!("invalid command: {e}")))
        .collect::<Result<_, _>>()?;

    // `GenerateVoiceover` writes a real audio file as a side effect of mutating the
    // in-memory `Project` clone — that write can't be undone by discarding the clone. If
    // it succeeds but a *later* command in this batch fails, the whole batch reports
    // failure and the project mutation is discarded, but without this the synthesized WAV
    // would stay on disk forever with no project ever referencing it. Track it here (by
    // index, matching `cmds`) so it can be deleted if the batch doesn't make it all the
    // way through.
    let voiceover_paths: Vec<Option<PathBuf>> = cmds
        .iter()
        .map(|c| match c {
            Command::GenerateVoiceover { output_path, .. } => Some(PathBuf::from(output_path)),
            _ => None,
        })
        .collect();

    let before = state.with_session(|s| Ok(s.project.clone()))?;
    let mut project = before.clone();

    let (result, project) = tauri::async_runtime::spawn_blocking(move || {
        let mut outcomes = Vec::with_capacity(cmds.len());
        for cmd in cmds {
            match apply_core_command(&mut project, cmd) {
                Ok(outcome) => outcomes.push(outcome),
                // Carry how many commands succeeded before this failure, so the caller
                // knows exactly which earlier side-effecting commands (if any) need
                // cleanup — not every command in the batch, only the ones that actually ran.
                Err(e) => return (Err((outcomes.len(), e.to_string())), project),
            }
        }
        (Ok(outcomes), project)
    })
    .await
    .map_err(|e| e.to_string())?;

    // Discard the whole batch on any failure — `project` here is the partially-mutated
    // clone, never written back, so a mid-batch error leaves the session untouched.
    let outcomes = match result {
        Ok(outcomes) => outcomes,
        Err((succeeded, message)) => {
            for path in voiceover_paths.iter().take(succeeded).flatten() {
                let _ = std::fs::remove_file(path);
            }
            return Err(message);
        }
    };

    state.history.lock().push_undo(before);
    state.commit_project(project).await?;
    state.emit_project_changed(&app);
    Ok(outcomes.into_iter().map(|o| format!("{o:?}")).collect())
}

/// Restore the previous project snapshot, moving the current one onto the redo stack.
#[tauri::command]
async fn undo(app: AppHandle, state: State<'_, AppState>) -> Result<HistoryStatus, String> {
    let _edit_guard = state.edit_lock.lock().await;
    let popped = state.history.lock().undo.pop();
    let Some(prev) = popped else {
        return Ok(state.history.lock().status());
    };

    let current = state.with_session(|s| Ok(s.project.clone()))?;
    state.history.lock().redo.push(current);
    state.commit_project(prev).await?;
    Ok(state.emit_project_changed(&app))
}

/// Re-apply the most recently undone snapshot, moving the current one onto the undo stack.
#[tauri::command]
async fn redo(app: AppHandle, state: State<'_, AppState>) -> Result<HistoryStatus, String> {
    let _edit_guard = state.edit_lock.lock().await;
    let popped = state.history.lock().redo.pop();
    let Some(next) = popped else {
        return Ok(state.history.lock().status());
    };

    // `push_undo_bounded`, not `push_undo` — a redo is not a new edit, so any
    // further-forward entries already sitting below `next` on the redo stack must stay
    // redo-able (plain `push_undo` would clear them as a new-edit side effect).
    let current = state.with_session(|s| Ok(s.project.clone()))?;
    state.history.lock().push_undo_bounded(current);
    state.commit_project(next).await?;
    Ok(state.emit_project_changed(&app))
}

#[tauri::command]
async fn export_project(
    state: State<'_, AppState>,
    output_path: String,
    preset: String,
) -> Result<(), String> {
    let preset = match preset.as_str() {
        "tiktok" => ExportPreset::TikTok9x16,
        "youtube" => ExportPreset::Youtube16x9,
        other => return Err(format!("unknown preset '{other}'")),
    };
    let mut project = state.with_session(|s| Ok(s.project.clone()))?;

    tauri::async_runtime::spawn_blocking(move || {
        apply_core_command(
            &mut project,
            Command::Export {
                output_path,
                preset,
            },
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Deliberately a *sync* command, not `async fn`. Tauri dispatches sync commands on the
/// main thread, which is required here: this is the only call site that creates the
/// native preview child HWND and its wgpu swapchain (`PreviewPanel::set_bounds` ->
/// `ensure_child_window` / `GfxState::new`), and Win32 windows must be created on a
/// thread that pumps messages for them — creating one from an async command's background
/// worker thread hangs (see docs/architecture.md "Playback engine" risk notes). Frame
/// *presentation* from the playback/scrub worker threads onto this already-created
/// surface is fine and unaffected by this.
///
/// `x`/`y`/`width`/`height` arrive as CSS logical pixels (`getBoundingClientRect()`).
/// Win32 window APIs for a DPI-aware process expect *physical* pixels, so on any monitor
/// scaled above 100% (125%/150%/etc. — the common case, not the exception), passing the
/// logical values straight through undersizes and mispositions the child HWND, which is
/// why the preview can render "successfully" (no errors) while showing nothing visible.
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
    let scale = app
        .get_webview_window("main")
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);
    let to_px = |v: i32| -> i32 { (v as f64 * scale).round() as i32 };
    let to_pu = |v: u32| -> u32 { (v as f64 * scale).round() as u32 };
    let width_px = to_pu(width);
    let height_px = to_pu(height);

    // Even-round so scale=-2:h in the decoder never has to round again.
    state.playback.set_target_size((height_px / 2) * 2);
    state
        .preview
        .lock()
        .set_bounds(PreviewBounds {
            x: to_px(x),
            y: to_px(y),
            width: width_px,
            height: height_px,
        })
        .map_err(|e| e.to_string())
}

/// Start (or resume) playback from `time_secs`. Non-blocking: hands a cloned `Project`
/// off to the playback worker thread and returns immediately — see playback.rs.
#[tauri::command]
async fn play(app: AppHandle, state: State<'_, AppState>, time_secs: f64) -> Result<(), String> {
    ensure_preview_parent(&app, &state)?;
    let project = state.with_session(|s| Ok(s.project.clone()))?;
    state.playback.play(app, project, time_secs);
    Ok(())
}

/// Stop playback and return the time to resume from. Joins the playback thread from
/// `spawn_blocking` rather than inline — see `stop_playback_blocking`'s doc comment; the
/// same in-flight-premix blocking risk applies here since `pause()` can be called the
/// instant after `play()` starts.
#[tauri::command]
async fn pause(app: AppHandle) -> Result<f64, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().playback.pause())
        .await
        .map_err(|e| e.to_string())
}

/// Jump the playhead to `time_secs`. While playing, this coalesces into the running
/// playback loop (audio/decoders restart from the new position without a pause/resume
/// round trip). While paused, it renders one frame via the scrub worker.
#[tauri::command]
async fn seek(app: AppHandle, state: State<'_, AppState>, time_secs: f64) -> Result<(), String> {
    if state.playback.seek_while_playing(time_secs) {
        return Ok(());
    }
    ensure_preview_parent(&app, &state)?;
    let project = state.with_session(|s| Ok(s.project.clone()))?;
    state.playback.request_preview(app, project, time_secs);
    Ok(())
}

/// Render a frame + play a short audio blip at `time_secs` (timeline scrub feedback).
/// Non-blocking and coalesced — safe to call on every pointermove during a drag.
#[tauri::command]
async fn scrub_audio(
    app: AppHandle,
    state: State<'_, AppState>,
    time_secs: f64,
) -> Result<(), String> {
    ensure_preview_parent(&app, &state)?;
    let project = state.with_session(|s| Ok(s.project.clone()))?;
    state.playback.request_scrub_audio(app, project, time_secs);
    Ok(())
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
            apply_commands,
            undo,
            redo,
            export_project,
            set_preview_bounds,
            play,
            pause,
            seek,
            scrub_audio,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Uppercut");
}

#[cfg(test)]
mod history_tests {
    use super::*;
    use uppercut_core::project::Settings;

    fn dummy_project(tag: &str) -> Project {
        Project::new(tag, Settings::default())
    }

    #[test]
    fn push_undo_bounds_stack_at_history_cap() {
        let mut history = History::new();
        for i in 0..(HISTORY_CAP + 20) {
            history.push_undo(dummy_project(&format!("edit-{i}")));
        }
        assert_eq!(history.undo.len(), HISTORY_CAP);
        // Oldest entries should have been evicted — the surviving bottom entry is the
        // 21st push (edits 0..19 evicted), not edit-0.
        assert_eq!(history.undo.first().unwrap().name, "edit-20");
        assert_eq!(history.undo.last().unwrap().name, "edit-119");
    }

    #[test]
    fn push_undo_clears_redo_but_push_undo_bounded_does_not() {
        let mut history = History::new();
        history.push_undo(dummy_project("a"));
        history.redo.push(dummy_project("stale-redo"));

        // A genuinely new edit invalidates the redo branch.
        history.push_undo(dummy_project("b"));
        assert!(history.redo.is_empty());

        // Redo's own bookkeeping push must NOT clear a redo branch that still has
        // further-forward entries below the one just popped.
        history.redo.push(dummy_project("still-redoable"));
        history.push_undo_bounded(dummy_project("c"));
        assert_eq!(history.redo.len(), 1);
    }

    #[test]
    fn clear_empties_both_stacks() {
        let mut history = History::new();
        history.push_undo(dummy_project("a"));
        history.redo.push(dummy_project("b"));
        history.clear();
        assert!(!history.status().can_undo);
        assert!(!history.status().can_redo);
    }
}
