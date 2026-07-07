//! Timeline audio scrub/playback via rodio (WAV chunks from uppercut-core).

use parking_lot::Mutex;
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink};
use std::io::Cursor;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use uppercut_core::{mix_timeline_audio_segment, Project};

const CHUNK_SECS: f64 = 0.12;

pub struct AudioScrubEngine {
    inner: Mutex<EngineInner>,
}

struct EngineInner {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    stream: Option<OutputStream>,
}

impl AudioScrubEngine {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(EngineInner {
                stop: Arc::new(AtomicBool::new(true)),
                thread: None,
                stream: None,
            }),
        }
    }

    pub fn stop(&self) {
        let mut inner = self.inner.lock();
        inner.stop.store(true, Ordering::SeqCst);
        if let Some(t) = inner.thread.take() {
            let _ = t.join();
        }
        inner.stream = None;
    }

    pub fn play_once(&self, project: &Project, time_secs: f64) -> Result<(), String> {
        let wav = mix_timeline_audio_segment(project, time_secs, 0.08).map_err(|e| e.to_string())?;
        if wav.is_empty() {
            return Ok(());
        }
        let stream = OutputStreamBuilder::open_default_stream().map_err(|e| format!("audio: {e}"))?;
        let sink = Sink::connect_new(stream.mixer());
        let decoder =
            Decoder::new_wav(Cursor::new(wav)).map_err(|e| format!("wav decode: {e}"))?;
        sink.append(decoder);
        sink.sleep_until_end();
        Ok(())
    }

    pub fn start_playback(
        &self,
        project: Project,
        mut time_secs: f64,
        fps: f64,
    ) -> Result<(), String> {
        self.stop();
        let stream = OutputStreamBuilder::open_default_stream().map_err(|e| format!("audio: {e}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let sink = Sink::connect_new(stream.mixer());
        let thread = thread::spawn(move || {
            let frame_dt = 1.0 / fps.max(1.0);
            while !stop_clone.load(Ordering::SeqCst) {
                let duration = timeline_end(&project).unwrap_or(0.0);
                if time_secs >= duration {
                    break;
                }
                match mix_timeline_audio_segment(&project, time_secs, CHUNK_SECS) {
                    Ok(wav) if !wav.is_empty() => {
                        if let Ok(decoder) = Decoder::new_wav(Cursor::new(wav)) {
                            sink.append(decoder);
                        }
                    }
                    Ok(_) => thread::sleep(Duration::from_millis(20)),
                    Err(_) => break,
                }
                time_secs += CHUNK_SECS.min(frame_dt * 4.0);
                thread::sleep(Duration::from_millis((CHUNK_SECS * 1000.0 * 0.85) as u64));
            }
            sink.stop();
        });

        let mut inner = self.inner.lock();
        inner.stop = stop;
        inner.stream = Some(stream);
        inner.thread = Some(thread);
        Ok(())
    }
}

fn timeline_end(project: &Project) -> Option<f64> {
    let end = uppercut_core::timeline_duration(project);
    if end > 0.0 {
        Some(end)
    } else {
        None
    }
}
