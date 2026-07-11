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
use uppercut_core::{
    mix_timeline_audio_range_to_file, mix_timeline_audio_segment, timeline_duration, DecodeOptions,
    ExportSettings, FrameRenderer, Project,
};

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

const TICK_INTERVAL: Duration = Duration::from_millis(33);

struct PlaySession {
    stop: Arc<AtomicUsize>, // 0 = running, 1 = stop requested
    seek: Arc<Mutex<Option<f64>>>,
    current_time: Arc<Mutex<f64>>,
    thread: JoinHandle<()>,
}

struct ScrubRequest {
    project: Project,
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
        }
    }

    /// Preview-panel height in pixels; playback decodes video layers downscaled to this
    /// instead of full source resolution. `0` means "use native resolution".
    pub fn set_target_size(&self, height: u32) {
        self.target_height.store(height, Ordering::SeqCst);
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

    pub fn play(&self, app: AppHandle, project: Project, start_secs: f64) {
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
        };
        // target_height is deliberately NOT used for continuous playback (unlike
        // paused-state scrub, see `submit_scrub`): a decoder held open across many
        // sequential reads needs `frame_bytes` (computed from our own predicted scaled
        // dimensions) to exactly match what ffmpeg's `-vf scale=-2:h` actually emits — if
        // our rounding and ffmpeg's ever disagree by even a couple of pixels, every frame
        // after the first reads mis-aligned bytes from the raw pipe, which free-runs
        // uncorrected for the rest of the stream and looks exactly like the video
        // glitching/corrupting progressively during playback. A single scrub-rendered
        // frame reopens the decoder fresh each time, so it can never accumulate that
        // drift — full source resolution here trades some CPU for correctness.
        let decode_opts = DecodeOptions {
            target_height: None,
            output_fps: Some(fps),
        };

        let stop = Arc::new(AtomicUsize::new(0));
        let seek = Arc::new(Mutex::new(None::<f64>));
        let current_time = Arc::new(Mutex::new(start_secs.min(duration.max(0.0))));

        let stop_clone = Arc::clone(&stop);
        let seek_clone = Arc::clone(&seek);
        let current_time_clone = Arc::clone(&current_time);
        let start_secs = start_secs.max(0.0);

        let thread = thread::spawn(move || {
            run_playback_loop(
                app,
                project,
                start_secs,
                duration,
                settings,
                decode_opts,
                stop_clone,
                seek_clone,
                current_time_clone,
            );
        });

        *self.session.lock() = Some(PlaySession {
            stop,
            seek,
            current_time,
            thread,
        });
    }

    /// Render a single frame at `time_secs` (paused-state seek); coalesced with any
    /// other pending scrub/seek request.
    pub fn request_preview(&self, app: AppHandle, project: Project, time_secs: f64) {
        self.submit_scrub(app, project, time_secs, false);
    }

    /// Render a single frame at `time_secs` and play a short audio blip; coalesced.
    pub fn request_scrub_audio(&self, app: AppHandle, project: Project, time_secs: f64) {
        self.submit_scrub(app, project, time_secs, true);
    }

    fn submit_scrub(
        &self,
        app: AppHandle,
        project: Project,
        time_secs: f64,
        with_audio_blip: bool,
    ) {
        let fps = project.settings.fps.max(1.0);
        let settings = ExportSettings {
            width: project.settings.width,
            height: project.settings.height,
            fps,
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
    project: Project,
    mut start_secs: f64,
    duration: f64,
    settings: ExportSettings,
    decode_opts: DecodeOptions,
    stop: Arc<AtomicUsize>,
    seek: Arc<Mutex<Option<f64>>>,
    current_time: Arc<Mutex<f64>>,
) {
    let mut renderer = match FrameRenderer::new(settings, decode_opts) {
        Ok(r) => r,
        Err(e) => {
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
            return;
        }
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

    'restart: loop {
        if stop.load(Ordering::SeqCst) != 0 {
            finish(start_secs);
            return;
        }
        if start_secs >= duration {
            finish(duration.max(0.0));
            return;
        }

        // Pre-mix timeline audio ONCE for [start_secs, duration) — a single ffmpeg
        // filtergraph, not one spawn per playback chunk — so the loop below only ever
        // reads from an already-rendered file via the audio clock.
        let audio_dir =
            std::env::temp_dir().join(format!("uppercut-playback-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&audio_dir);
        // RAII cleanup instead of a manual `remove_dir_all` at every exit point below: it
        // also covers a panic unwinding through this scope (e.g. a wgpu validation panic
        // inside `renderer.render`), which none of the manual call sites could.
        let _audio_dir_guard = TempDirGuard(audio_dir.clone());
        let audio_wav = audio_dir.join("audio.wav");
        let has_audio = match mix_timeline_audio_range_to_file(
            &project,
            start_secs,
            duration - start_secs,
            &audio_wav,
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
                if let Ok(file) = File::open(&audio_wav) {
                    if let Ok(decoder) = Decoder::new_wav(BufReader::new(file)) {
                        sink.append(decoder);
                    }
                }
            }
        }

        let wall_clock_start = Instant::now();
        let mut last_tick_emit = Instant::now() - TICK_INTERVAL;
        let mut last_error_emit = Instant::now() - Duration::from_secs(2);
        let frame_period = Duration::from_secs_f64(1.0 / settings.fps);

        loop {
            if stop.load(Ordering::SeqCst) != 0 {
                if let Some(sink) = &sink {
                    sink.stop();
                }
                finish(*current_time.lock());
                return;
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
                finish(duration.max(0.0));
                return;
            }

            *current_time.lock() = t;

            match renderer.render(&project, t) {
                Ok(pixels) => {
                    let state = app.state::<crate::AppState>();
                    let result =
                        state
                            .preview
                            .lock()
                            .present_rgba(&pixels, settings.width, settings.height);
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
    let mut cached: Option<(ExportSettings, DecodeOptions, FrameRenderer)> = None;

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
            Some((s, d, _)) => *s != req.settings || *d != req.decode_opts,
            None => true,
        };
        if needs_new {
            match FrameRenderer::new(req.settings, req.decode_opts) {
                Ok(r) => cached = Some((req.settings, req.decode_opts, r)),
                Err(e) => {
                    eprintln!("scrub: frame renderer init failed: {e}");
                    continue;
                }
            }
        }

        let (_, _, renderer) = cached.as_mut().expect("just populated above");
        match renderer.render(&req.project, req.time_secs) {
            Ok(pixels) => {
                // Re-check: `render` above is a real decode, not instantaneous — `play()`
                // can start while it was in flight. Presenting after that would overwrite
                // a live playback frame with this now-stale scrub frame.
                if req.epoch != play_epoch.load(Ordering::SeqCst) {
                    continue;
                }
                let state = req.app.state::<crate::AppState>();
                let result = state.preview.lock().present_rgba(
                    &pixels,
                    req.settings.width,
                    req.settings.height,
                );
                if let Err(e) = result {
                    eprintln!("scrub: present failed at {:.3}s: {e}", req.time_secs);
                }
            }
            Err(e) => eprintln!("scrub: render failed at {:.3}s: {e}", req.time_secs),
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
