//! Persistent playback engine.
//!
//! Replaces the old per-frame path (`update_preview` calling `render_frame_at`, which
//! created a fresh wgpu Instance+Adapter+Device and respawned ffprobe/ffmpeg per video
//! layer on *every* JS `setInterval` tick — see docs/architecture.md "Playback engine"
//! for the full trace). Playback now runs on a dedicated worker thread that holds one
//! `FrameRenderer` (persistent wgpu device + open decoders) for the whole play session
//! and drives it from a monotonic audio/wall clock, so decoders stream forward instead of
//! being torn down and respawned every frame.
//!
//! A second, always-on worker (`ScrubWorker`) serves paused-state seeks/scrubs: requests
//! are coalesced (only the latest survives), and it also caches its `FrameRenderer` across
//! calls, rebuilding only when output settings actually change.

use parking_lot::Mutex;
use renderly_core::{
    mix_timeline_audio_range_to_file, mix_timeline_audio_segment, timeline_duration, DecodeOptions,
    ExportSettings, FrameRenderer, Project,
};
use rodio::{Decoder, OutputStreamBuilder, Sink};
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::sync::{
    atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Emitted ~30 Hz while playing, and once on seek/pause/EOF.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackTick {
    pub time_secs: f64,
    pub playing: bool,
}

/// Emitted on play start/stop and end-of-timeline.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackStateEvent {
    pub playing: bool,
    pub time_secs: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackErrorEvent {
    pub message: String,
}

/// Emitted ~1 Hz while playing (see improvement-plan A0 — "measure from day one" before
/// A1-A8 touch this hot path). Averaged over the emit window, not instantaneous, so a
/// single slow frame (GC-style hiccup, disk stall) doesn't make the HUD unreadable.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct PlaybackPerfEvent {
    pub decode_ms: f64,
    pub compose_ms: f64,
    pub present_ms: f64,
    pub frame_ms: f64,
    pub fps: f64,
}

/// Accumulates per-frame timing and periodically drains itself into a `PlaybackPerfEvent`
/// — a plain averaging accumulator, not a ring buffer: A0 only needs "what's it been
/// like for the last second", not a full history.
#[derive(Default)]
struct PerfAccumulator {
    frames: u32,
    decode_ms: f64,
    compose_ms: f64,
    present_ms: f64,
    frame_ms: f64,
    window_start: Option<Instant>,
}

impl PerfAccumulator {
    fn record(&mut self, timing: renderly_core::FrameTiming, present_ms: f64, frame_ms: f64) {
        if self.window_start.is_none() {
            self.window_start = Some(Instant::now());
        }
        self.frames += 1;
        self.decode_ms += timing.decode_ms;
        self.compose_ms += timing.compose_ms;
        self.present_ms += present_ms;
        self.frame_ms += frame_ms;
    }

    /// Drains and returns an averaged event once `PERF_EMIT_INTERVAL` has elapsed since
    /// the window started; otherwise leaves the accumulator untouched.
    fn maybe_drain(&mut self) -> Option<PlaybackPerfEvent> {
        let elapsed = self.window_start?.elapsed();
        if elapsed < PERF_EMIT_INTERVAL || self.frames == 0 {
            return None;
        }
        let n = self.frames as f64;
        let event = PlaybackPerfEvent {
            decode_ms: self.decode_ms / n,
            compose_ms: self.compose_ms / n,
            present_ms: self.present_ms / n,
            frame_ms: self.frame_ms / n,
            fps: n / elapsed.as_secs_f64(),
        };
        *self = Self::default();
        Some(event)
    }
}

const TICK_INTERVAL: Duration = Duration::from_millis(33);
const PERF_EMIT_INTERVAL: Duration = Duration::from_secs(1);

struct PlaySession {
    stop: Arc<AtomicUsize>, // 0 = running, 1 = stop requested
    seek: Arc<Mutex<Option<f64>>>,
    current_time: Arc<Mutex<f64>>,
    /// Shared with the playback thread; `update_live_project` swaps its contents so an
    /// edit made while playing shows up on the next rendered frame without stopping
    /// audio or reopening decoders — see `update_live_project`'s doc comment.
    /// Outer `Arc`/`Mutex` for sharing; inner `Arc<Project>` so live swaps and
    /// `project.lock().clone()` snapshots are cheap (B3 — no full Project clone).
    project: Arc<Mutex<Arc<Project>>>,
    thread: JoinHandle<()>,
}

struct ScrubRequest {
    project: Arc<Project>,
    time_secs: f64,
    settings: ExportSettings,
    decode_opts: DecodeOptions,
    with_audio_blip: bool,
    app: AppHandle,
    /// `play_epoch` at submission time — lets the scrub worker detect a `play()` call
    /// that started after this request was queued (or while it was mid-render) and skip
    /// presenting a now-stale frame instead of it landing after, and overwriting, a live
    /// playback frame. See the two checks in `run_scrub_worker`.
    epoch: u64,
}

pub struct PlaybackEngine {
    session: Mutex<Option<PlaySession>>,
    target_height: AtomicU32,
    scrub_pending: Arc<Mutex<Option<ScrubRequest>>>,
    _scrub_thread: JoinHandle<()>,
    play_epoch: Arc<AtomicU64>,
    /// P1 webview preview migration (docs/preview-webview.md item 10): when set, the play
    /// loop does audio-only work (premix + `playback:tick`) and skips all video decode/
    /// compose/present — the webview renders video itself via `<video>` elements, and the
    /// native preview surface may not even exist in this mode. Set once at startup by the
    /// frontend's `set_preview_mode` call, mirrored from `isWebviewPreview()`.
    webview_mode: std::sync::atomic::AtomicBool,
}

impl PlaybackEngine {
    pub fn new() -> Self {
        let scrub_pending: Arc<Mutex<Option<ScrubRequest>>> = Arc::new(Mutex::new(None));
        let worker_pending = Arc::clone(&scrub_pending);
        let play_epoch = Arc::new(AtomicU64::new(0));
        let worker_epoch = Arc::clone(&play_epoch);
        let scrub_thread = thread::spawn(move || run_scrub_worker(worker_pending, worker_epoch));

        Self {
            session: Mutex::new(None),
            target_height: AtomicU32::new(0),
            scrub_pending,
            _scrub_thread: scrub_thread,
            play_epoch,
            webview_mode: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn set_webview_mode(&self, webview: bool) {
        self.webview_mode.store(webview, Ordering::SeqCst);
    }

    pub fn is_webview_mode(&self) -> bool {
        self.webview_mode.load(Ordering::SeqCst)
    }

    /// Preview-panel height in pixels; playback decodes video layers downscaled to this
    /// instead of full source resolution. `0` means "use native resolution".
    pub fn set_target_size(&self, height: u32) {
        self.target_height.store(height, Ordering::SeqCst);
    }

    /// Current preview target height in pixels (`0` = unset / full-res fallback).
    pub fn target_height(&self) -> u32 {
        self.target_height.load(Ordering::SeqCst)
    }

    fn decode_opts(&self, output_fps: f64) -> DecodeOptions {
        let h = self.target_height.load(Ordering::SeqCst);
        DecodeOptions {
            target_height: if h > 0 { Some(h) } else { None },
            output_fps: Some(output_fps),
        }
    }

    /// Stop playback without reporting a resume time (used on session teardown).
    pub fn stop(&self) {
        // `let session = ...;` as its own statement, NOT `if let Some(session) =
        // self.session.lock().take() { ... }` — Rust extends a temporary created in a
        // match/if-let scrutinee to live for the whole block (see the Reference's
        // "temporary scopes" rules), so the `MutexGuard` from `self.session.lock()` would
        // otherwise stay held for the entire body below, including the blocking
        // `thread.join()` — self-deadlocking any other call (e.g. `play()`'s own
        // `self.stop()`) that needs this same lock while a session is still joining.
        let session = self.session.lock().take();
        if let Some(session) = session {
            session.stop.store(1, Ordering::SeqCst);
            let _ = session.thread.join();
        }
    }

    /// Stop playback and return the time to resume from.
    pub fn pause(&self) -> f64 {
        // See the identical note in `stop()` above — this early `let` is load-bearing,
        // not stylistic.
        let session = self.session.lock().take();
        if let Some(session) = session {
            session.stop.store(1, Ordering::SeqCst);
            let t = *session.current_time.lock();
            let _ = session.thread.join();
            t
        } else {
            0.0
        }
    }

    /// Whether a play session is currently active (bridge status / HUD).
    pub fn is_playing(&self) -> bool {
        self.session.lock().is_some()
    }

    /// If a play session is active, coalesce a seek into it (the loop picks up the
    /// latest value on its next iteration and restarts audio/decoders from there — a
    /// newer seek arriving before the loop observes an older one simply replaces it).
    /// Returns `false` if nothing is playing, so the caller can fall back to a
    /// paused-state preview render.
    pub fn seek_while_playing(&self, time_secs: f64) -> bool {
        let guard = self.session.lock();
        if let Some(session) = guard.as_ref() {
            *session.seek.lock() = Some(time_secs);
            true
        } else {
            false
        }
    }

    /// Live-swap the project for the active play session so the *next* rendered frame
    /// reflects an edit — no pause, no audio-sink teardown, no decoder respawn. Safe for
    /// both property edits (opacity/gain/effects) and structural ones (split/move/delete):
    /// `FrameRenderer::render` takes the project by reference on every call and its
    /// decoder cache is keyed by track id, reopening ffmpeg whenever the active clip's
    /// source path at that track changes — exactly the same recovery path an ordinary
    /// forward seek already exercises. Returns `false` if nothing is playing, so the
    /// caller can fall back to a paused-state scrub render instead.
    ///
    /// One caveat: the audio track was pre-mixed once for the whole remaining play range
    /// at the last restart (see `run_playback_loop`), so gain/mute/audio-clip edits are
    /// picked up visually next frame but won't be audible until the next natural restart
    /// (seek, loop end). Segmented premix (planned separately) removes this caveat.
    pub fn update_live_project(&self, project: Arc<Project>) -> bool {
        let guard = self.session.lock();
        if let Some(session) = guard.as_ref() {
            *session.project.lock() = project;
            true
        } else {
            false
        }
    }

    pub fn play(&self, app: AppHandle, project: Arc<Project>, start_secs: f64) {
        self.stop();
        // Claims the preview surface for this playback generation — see `ScrubRequest`'s
        // `epoch` doc comment.
        self.play_epoch.fetch_add(1, Ordering::SeqCst);

        let duration = timeline_duration(&project);
        let fps = project.settings.fps.max(1.0);
        let settings = ExportSettings {
            width: project.settings.width,
            height: project.settings.height,
            fps,
            ..Default::default()
        };
        // A2: decode at panel resolution instead of full source resolution during
        // continuous playback too (previously only the paused-state scrub path did this
        // via `decode_opts()`, see `submit_scrub`). This is safe *only* because
        // `scaled_dimensions`/`ffalign_even` in `renderly-core::media::ffmpeg_cli` now
        // exactly mirror ffmpeg's own `FFALIGN(lrint(x), 2)` resolution of a `scale=-2:h`
        // filter (verified against real ffmpeg output for several odd-dimension sources,
        // not just derived on paper) — a decoder held open across many sequential reads
        // needs our predicted `frame_bytes` to exactly match what ffmpeg emits, or every
        // frame after the first reads mis-aligned bytes from the raw pipe and free-runs
        // uncorrected, which looks exactly like the video corrupting/glitching
        // progressively during playback. Decoding a 1080p/4K source at panel height
        // (typically a few hundred px) is a several-times reduction in decode + memcpy
        // work per frame — the single biggest felt playback-performance win here.
        let decode_opts = self.decode_opts(fps);

        let stop = Arc::new(AtomicUsize::new(0));
        let seek = Arc::new(Mutex::new(None::<f64>));
        let current_time = Arc::new(Mutex::new(start_secs.min(duration.max(0.0))));
        let project = Arc::new(Mutex::new(project));

        let stop_clone = Arc::clone(&stop);
        let seek_clone = Arc::clone(&seek);
        let current_time_clone = Arc::clone(&current_time);
        let project_clone = Arc::clone(&project);
        let start_secs = start_secs.max(0.0);
        let webview_mode = self.is_webview_mode();

        let thread = thread::spawn(move || {
            run_playback_loop(
                app,
                project_clone,
                start_secs,
                duration,
                settings,
                decode_opts,
                stop_clone,
                seek_clone,
                current_time_clone,
                webview_mode,
            );
        });

        *self.session.lock() = Some(PlaySession {
            stop,
            seek,
            current_time,
            project,
            thread,
        });
    }

    /// Render a single frame at `time_secs` (paused-state seek); coalesced with any
    /// other pending scrub/seek request.
    pub fn request_preview(&self, app: AppHandle, project: Arc<Project>, time_secs: f64) {
        self.submit_scrub(app, project, time_secs, false);
    }

    /// Render a single frame at `time_secs` and play a short audio blip; coalesced.
    pub fn request_scrub_audio(&self, app: AppHandle, project: Arc<Project>, time_secs: f64) {
        self.submit_scrub(app, project, time_secs, true);
    }

    fn submit_scrub(
        &self,
        app: AppHandle,
        project: Arc<Project>,
        time_secs: f64,
        with_audio_blip: bool,
    ) {
        let fps = project.settings.fps.max(1.0);
        let settings = ExportSettings {
            width: project.settings.width,
            height: project.settings.height,
            fps,
            ..Default::default()
        };
        // Paused-state preview decodes a single frame; no output-fps pacing needed.
        let decode_opts = DecodeOptions {
            target_height: self.decode_opts(fps).target_height,
            output_fps: None,
        };
        *self.scrub_pending.lock() = Some(ScrubRequest {
            project,
            time_secs,
            settings,
            decode_opts,
            with_audio_blip,
            app,
            epoch: self.play_epoch.load(Ordering::SeqCst),
        });
    }
}

/// Removes its directory on drop — covers every exit path from the scope it's declared
/// in (early `return`, `continue 'restart`, *and* a panic unwinding through), unlike a
/// manual `remove_dir_all` call repeated at each individual exit point.
struct TempDirGuard(std::path::PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

#[allow(clippy::too_many_arguments)]
fn run_playback_loop(
    app: AppHandle,
    project: Arc<Mutex<Arc<Project>>>,
    mut start_secs: f64,
    duration: f64,
    settings: ExportSettings,
    decode_opts: DecodeOptions,
    stop: Arc<AtomicUsize>,
    seek: Arc<Mutex<Option<f64>>>,
    current_time: Arc<Mutex<f64>>,
    webview_mode: bool,
) {
    // P1 webview preview migration: in webview mode the browser decodes/presents video
    // itself, so this loop does audio-only work (premix + tick emission) and never
    // touches a `FrameRenderer` or the native preview surface at all — which may not even
    // be initialized in this mode (see `AppState`/`ensure_preview_parent` call sites in
    // lib.rs, which are skipped for `play`/`seek`/`refresh_frame` when webview mode is on).
    let mut renderer: Option<(FrameRenderer, bool)> = if webview_mode {
        None
    } else {
        let shared = app
            .state::<crate::AppState>()
            .preview
            .lock()
            .shared_device();
        let built = match shared {
            Some((device, queue)) => {
                match FrameRenderer::with_device(device, queue, settings, decode_opts) {
                    Ok(r) => Some((r, true)),
                    Err(e) => {
                        eprintln!(
                            "playback: shared-device renderer init failed ({e}); falling back"
                        );
                        FrameRenderer::new(settings, decode_opts)
                            .map(|r| Some((r, false)))
                            .unwrap_or_else(|e| {
                                eprintln!("playback: frame renderer init failed: {e}");
                                let _ = app.emit(
                                    "playback:state",
                                    PlaybackStateEvent {
                                        playing: false,
                                        time_secs: start_secs,
                                    },
                                );
                                let _ = app.emit(
                                    "playback:error",
                                    PlaybackErrorEvent {
                                        message: format!("Could not start playback: {e}"),
                                    },
                                );
                                None
                            })
                    }
                }
            }
            None => FrameRenderer::new(settings, decode_opts)
                .map(|r| Some((r, false)))
                .unwrap_or_else(|e| {
                    eprintln!("playback: frame renderer init failed: {e}");
                    let _ = app.emit(
                        "playback:state",
                        PlaybackStateEvent {
                            playing: false,
                            time_secs: start_secs,
                        },
                    );
                    let _ = app.emit(
                        "playback:error",
                        PlaybackErrorEvent {
                            message: format!("Could not start playback: {e}"),
                        },
                    );
                    None
                }),
        };
        // Renderer init failure is fatal to a *native* play session (nothing to present),
        // matching the original early-`return` behavior.
        if built.is_none() {
            return;
        }
        built
    };

    let finish = |t: f64| {
        *current_time.lock() = t;
        let _ = app.emit(
            "playback:tick",
            PlaybackTick {
                time_secs: t,
                playing: false,
            },
        );
        let _ = app.emit(
            "playback:state",
            PlaybackStateEvent {
                playing: false,
                time_secs: t,
            },
        );
    };

    let mut perf = PerfAccumulator::default();

    'restart: loop {
        if stop.load(Ordering::SeqCst) != 0 {
            finish(start_secs);
            return;
        }
        if start_secs >= duration {
            finish(duration.max(0.0));
            return;
        }

        // A6: premix audio in ~5s chunks so playback can start after the first chunk
        // (~100–300 ms) instead of waiting on the entire remaining timeline. Chunk 0 is
        // mixed on this thread; later chunks are mixed on a background worker and
        // appended to the same rodio Sink (still the master clock via get_pos()).
        const AUDIO_CHUNK_SECS: f64 = 5.0;
        let audio_dir =
            std::env::temp_dir().join(format!("renderly-playback-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&audio_dir);
        // RAII cleanup instead of a manual `remove_dir_all` at every exit point below: it
        // also covers a panic unwinding through this scope (e.g. a wgpu validation panic
        // inside `renderer.render`), which none of the manual call sites could.
        let _audio_dir_guard = TempDirGuard(audio_dir.clone());

        let remaining = (duration - start_secs).max(0.0);
        let first_chunk = remaining.min(AUDIO_CHUNK_SECS);
        let first_wav = audio_dir.join("audio_0.wav");
        let project_snap = project.lock().clone();
        let has_audio = match mix_timeline_audio_range_to_file(
            &project_snap,
            start_secs,
            first_chunk,
            &first_wav,
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("playback: audio pre-mix failed: {e}");
                false
            }
        };

        let stream = OutputStreamBuilder::open_default_stream().ok();
        let sink = stream.as_ref().map(|s| Sink::connect_new(s.mixer()));
        if has_audio {
            if let Some(sink) = &sink {
                if let Ok(file) = File::open(&first_wav) {
                    if let Ok(decoder) = Decoder::new_wav(BufReader::new(file)) {
                        sink.append(decoder);
                    }
                }
            }
        }

        // Background premix for chunks after the first. Cancelled when this restart
        // scope ends (seek / stop / end) by dropping `audio_rx` so `send` fails.
        let (audio_tx, audio_rx) = std::sync::mpsc::channel::<std::path::PathBuf>();
        let audio_worker = if has_audio && remaining > first_chunk {
            let dir = audio_dir.clone();
            let stop_flag = Arc::clone(&stop);
            Some(thread::spawn(move || {
                let mut chunk_start = start_secs + first_chunk;
                let mut idx = 1u32;
                while chunk_start < duration - 1e-6 {
                    if stop_flag.load(Ordering::SeqCst) != 0 {
                        break;
                    }
                    let chunk_len = (duration - chunk_start).min(AUDIO_CHUNK_SECS);
                    let path = dir.join(format!("audio_{idx}.wav"));
                    match mix_timeline_audio_range_to_file(
                        &project_snap,
                        chunk_start,
                        chunk_len,
                        &path,
                    ) {
                        Ok(true) => {
                            if audio_tx.send(path).is_err() {
                                break; // playback restarted or stopped
                            }
                        }
                        Ok(false) => break,
                        Err(e) => {
                            eprintln!("playback: audio chunk {idx} premix failed: {e}");
                            break;
                        }
                    }
                    chunk_start += chunk_len;
                    idx += 1;
                }
            }))
        } else {
            None
        };

        let wall_clock_start = Instant::now();
        let mut last_tick_emit = Instant::now() - TICK_INTERVAL;
        let mut last_error_emit = Instant::now() - Duration::from_secs(2);
        let frame_period = Duration::from_secs_f64(1.0 / settings.fps);

        loop {
            if stop.load(Ordering::SeqCst) != 0 {
                if let Some(sink) = &sink {
                    sink.stop();
                }
                drop(audio_rx);
                if let Some(h) = audio_worker {
                    let _ = h.join();
                }
                finish(*current_time.lock());
                return;
            }

            // Drain any finished background audio chunks onto the sink.
            if let Some(sink) = &sink {
                while let Ok(path) = audio_rx.try_recv() {
                    if let Ok(file) = File::open(&path) {
                        if let Ok(decoder) = Decoder::new_wav(BufReader::new(file)) {
                            sink.append(decoder);
                        }
                    }
                }
            }

            // `seek.lock()` as its own statement, NOT the `if let` scrutinee directly —
            // same reasoning as `stop()`/`pause()`'s identically-shaped fix above: an
            // if-let scrutinee's temporary is extended to live for the whole block, which
            // would hold this `MutexGuard` across `sink.stop()`/`app.emit` (blocking I/O)
            // for no reason. Milder than the original bug (no reverse-lock-order deadlock
            // here — confirmed no other holder of `seek` ever tries to reacquire anything
            // this loop holds) but the same footgun shape.
            let pending_seek = seek.lock().take();
            if let Some(new_time) = pending_seek {
                if let Some(sink) = &sink {
                    sink.stop();
                }
                drop(audio_rx);
                if let Some(h) = audio_worker {
                    let _ = h.join();
                }
                start_secs = new_time.clamp(0.0, duration.max(0.0));
                let _ = app.emit(
                    "playback:tick",
                    PlaybackTick {
                        time_secs: start_secs,
                        playing: true,
                    },
                );
                continue 'restart;
            }

            // rodio's `Sink::get_pos()` quantizes to buffer boundaries; the wall-clock
            // fallback (no audio) is a plain monotonic clock instead.
            let elapsed = if let (Some(sink), true) = (&sink, has_audio) {
                sink.get_pos().as_secs_f64()
            } else {
                wall_clock_start.elapsed().as_secs_f64()
            };
            let t = start_secs + elapsed;

            if t >= duration {
                if let Some(sink) = &sink {
                    sink.stop();
                }
                drop(audio_rx);
                if let Some(h) = audio_worker {
                    let _ = h.join();
                }
                finish(duration.max(0.0));
                return;
            }

            *current_time.lock() = t;

            // Webview mode: no `FrameRenderer`, no native surface — this loop's only job
            // is audio pacing + tick emission (see `renderer`'s doc comment above).
            if let Some((renderer, gpu_present)) = renderer.as_mut() {
                let frame_start = Instant::now();
                if *gpu_present {
                    match renderer.render_to_texture(&project.lock(), t) {
                        Ok(timing) => {
                            let state = app.state::<crate::AppState>();
                            let present_start = Instant::now();
                            let result = state
                                .preview
                                .lock()
                                .present_texture_view(renderer.output_view());
                            let present_ms = present_start.elapsed().as_secs_f64() * 1000.0;
                            if let Err(e) = result {
                                eprintln!("playback: present failed at {t:.3}s: {e}");
                                if last_error_emit.elapsed() >= Duration::from_secs(2) {
                                    let _ = app.emit(
                                        "playback:error",
                                        PlaybackErrorEvent {
                                            message: format!("Preview present failed: {e}"),
                                        },
                                    );
                                    last_error_emit = Instant::now();
                                }
                            }
                            let frame_ms = frame_start.elapsed().as_secs_f64() * 1000.0;
                            perf.record(timing, present_ms, frame_ms);
                        }
                        Err(e) => {
                            eprintln!("playback: render failed at {t:.3}s: {e}");
                            if last_error_emit.elapsed() >= Duration::from_secs(2) {
                                let _ = app.emit(
                                    "playback:error",
                                    PlaybackErrorEvent {
                                        message: format!("Playback render failed: {e}"),
                                    },
                                );
                                last_error_emit = Instant::now();
                            }
                        }
                    }
                } else {
                    match renderer.render_timed(&project.lock(), t) {
                        Ok((pixels, timing)) => {
                            let state = app.state::<crate::AppState>();
                            let present_start = Instant::now();
                            let result = state.preview.lock().present_rgba(
                                &pixels,
                                settings.width,
                                settings.height,
                            );
                            let present_ms = present_start.elapsed().as_secs_f64() * 1000.0;
                            if let Err(e) = result {
                                eprintln!("playback: present failed at {t:.3}s: {e}");
                                if last_error_emit.elapsed() >= Duration::from_secs(2) {
                                    let _ = app.emit(
                                        "playback:error",
                                        PlaybackErrorEvent {
                                            message: format!("Preview present failed: {e}"),
                                        },
                                    );
                                    last_error_emit = Instant::now();
                                }
                            }
                            let frame_ms = frame_start.elapsed().as_secs_f64() * 1000.0;
                            perf.record(timing, present_ms, frame_ms);
                        }
                        Err(e) => {
                            eprintln!("playback: render failed at {t:.3}s: {e}");
                            if last_error_emit.elapsed() >= Duration::from_secs(2) {
                                let _ = app.emit(
                                    "playback:error",
                                    PlaybackErrorEvent {
                                        message: format!("Playback render failed: {e}"),
                                    },
                                );
                                last_error_emit = Instant::now();
                            }
                        }
                    }
                }

                if let Some(event) = perf.maybe_drain() {
                    let _ = app.emit("playback:perf", event);
                }
            }

            if last_tick_emit.elapsed() >= TICK_INTERVAL {
                let _ = app.emit(
                    "playback:tick",
                    PlaybackTick {
                        time_secs: t,
                        playing: true,
                    },
                );
                last_tick_emit = Instant::now();
            }

            thread::sleep(frame_period);
        }
    }
}

fn run_scrub_worker(pending: Arc<Mutex<Option<ScrubRequest>>>, play_epoch: Arc<AtomicU64>) {
    // (settings, decode_opts, renderer, gpu_present)
    let mut cached: Option<(ExportSettings, DecodeOptions, FrameRenderer, bool)> = None;

    loop {
        let request = pending.lock().take();
        let Some(req) = request else {
            thread::sleep(Duration::from_millis(8));
            continue;
        };

        // A `play()` call landed after this request was submitted — skip it entirely
        // rather than decoding a frame nobody wants; playback owns the preview surface now.
        if req.epoch != play_epoch.load(Ordering::SeqCst) {
            continue;
        }

        let needs_new = match &cached {
            Some((s, d, _, _)) => *s != req.settings || *d != req.decode_opts,
            None => true,
        };
        if needs_new {
            let shared = req
                .app
                .state::<crate::AppState>()
                .preview
                .lock()
                .shared_device();
            let built = match shared {
                Some((device, queue)) => {
                    FrameRenderer::with_device(device, queue, req.settings, req.decode_opts)
                        .map(|r| (r, true))
                        .or_else(|e| {
                            eprintln!(
                                "scrub: shared-device renderer init failed ({e}); falling back"
                            );
                            FrameRenderer::new(req.settings, req.decode_opts).map(|r| (r, false))
                        })
                }
                None => FrameRenderer::new(req.settings, req.decode_opts).map(|r| (r, false)),
            };
            match built {
                Ok((r, gpu)) => cached = Some((req.settings, req.decode_opts, r, gpu)),
                Err(e) => {
                    eprintln!("scrub: frame renderer init failed: {e}");
                    continue;
                }
            }
        }

        let (_, _, renderer, gpu_present) = cached.as_mut().expect("just populated above");
        let present_ok = if *gpu_present {
            match renderer.render_to_texture(&req.project, req.time_secs) {
                Ok(_) => {
                    if req.epoch != play_epoch.load(Ordering::SeqCst) {
                        continue;
                    }
                    let state = req.app.state::<crate::AppState>();
                    let result = state
                        .preview
                        .lock()
                        .present_texture_view(renderer.output_view());
                    result
                }
                Err(e) => {
                    eprintln!("scrub: render failed at {:.3}s: {e}", req.time_secs);
                    continue;
                }
            }
        } else {
            match renderer.render(&req.project, req.time_secs) {
                Ok(pixels) => {
                    if req.epoch != play_epoch.load(Ordering::SeqCst) {
                        continue;
                    }
                    let state = req.app.state::<crate::AppState>();
                    let result = state.preview.lock().present_rgba(
                        &pixels,
                        req.settings.width,
                        req.settings.height,
                    );
                    result
                }
                Err(e) => {
                    eprintln!("scrub: render failed at {:.3}s: {e}", req.time_secs);
                    continue;
                }
            }
        };
        if let Err(e) = present_ok {
            eprintln!("scrub: present failed at {:.3}s: {e}", req.time_secs);
        } else if pending.lock().is_none() && req.epoch == play_epoch.load(Ordering::SeqCst) {
            // A5: warm decode LRU ±0.5s while the user is paused on this scrub
            // position — next nearby scrub (especially reverse) hits cache.
            let (_, _, renderer, _) = cached.as_mut().expect("renderer still cached");
            if let Err(e) = renderer.prefetch_around(&req.project, req.time_secs, 0.5) {
                eprintln!("scrub: prefetch failed at {:.3}s: {e}", req.time_secs);
            }
        }

        if req.with_audio_blip {
            if let Ok(wav) = mix_timeline_audio_segment(&req.project, req.time_secs, 0.08) {
                if !wav.is_empty() {
                    // Detached: plays out on its own thread so the scrub worker is free
                    // to pick up the next coalesced request immediately.
                    thread::spawn(move || {
                        if let Ok(stream) = OutputStreamBuilder::open_default_stream() {
                            let sink = Sink::connect_new(stream.mixer());
                            if let Ok(decoder) = Decoder::new_wav(Cursor::new(wav)) {
                                sink.append(decoder);
                                sink.sleep_until_end();
                            }
                        }
                    });
                }
            }
        }
    }
}
