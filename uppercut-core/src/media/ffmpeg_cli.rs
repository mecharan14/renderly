//! Invoke `ffmpeg` / `ffprobe` as subprocesses. Phase 0 uses the user's installed FFmpeg
//! binaries (no link-time dependency on libav); linked decode/encode via `ffmpeg-the-third`
//! lands once vcpkg/FFMPEG_DIR is wired up for all dev/CI environments.

use std::io::{BufReader, Read, Write};

use crate::project::TrackAudioRole;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FfmpegCliError {
    #[error("ffmpeg/ffprobe not found on PATH; install FFmpeg to use media I/O")]
    NotFound,
    #[error("failed to run {tool}: {message}")]
    SpawnFailed { tool: &'static str, message: String },
    #[error("ffmpeg exited with status {0}")]
    NonZeroExit(i32),
    #[error("unexpected ffmpeg output: {0}")]
    BadOutput(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

static FFMPEG: OnceLock<PathBuf> = OnceLock::new();
static FFPROBE: OnceLock<PathBuf> = OnceLock::new();

fn resolve_tool(name: &str, cache: &OnceLock<PathBuf>) -> Result<PathBuf, FfmpegCliError> {
    if let Some(path) = cache.get() {
        return Ok(path.clone());
    }
    let found = which_tool(name).ok_or(FfmpegCliError::NotFound)?;
    let _ = cache.set(found.clone());
    Ok(found)
}

fn which_tool(name: &str) -> Option<PathBuf> {
    let with_exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(&with_exe);
            candidate.is_file().then_some(candidate)
        })
    })
}

pub fn ffmpeg_path() -> Result<PathBuf, FfmpegCliError> {
    resolve_tool("ffmpeg", &FFMPEG)
}

pub fn ffprobe_path() -> Result<PathBuf, FfmpegCliError> {
    resolve_tool("ffprobe", &FFPROBE)
}

/// Returns true when both `ffmpeg` and `ffprobe` are discoverable on PATH.
pub fn is_available() -> bool {
    ffmpeg_path().is_ok() && ffprobe_path().is_ok()
}

#[derive(Debug, Clone)]
pub struct ProbedVideo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_secs: f64,
}

pub fn probe_video(path: &Path) -> Result<ProbedVideo, FfmpegCliError> {
    let output = Command::new(ffprobe_path()?)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate,duration",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| FfmpegCliError::SpawnFailed {
            tool: "ffprobe",
            message: e.to_string(),
        })?;

    if !output.status.success() {
        return Err(FfmpegCliError::NonZeroExit(
            output.status.code().unwrap_or(-1),
        ));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| FfmpegCliError::BadOutput(e.to_string()))?;

    let stream = json
        .get("streams")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| FfmpegCliError::BadOutput("no video stream".into()))?;

    let width = stream
        .get("width")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| FfmpegCliError::BadOutput("missing width".into()))? as u32;
    let height = stream
        .get("height")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| FfmpegCliError::BadOutput("missing height".into()))? as u32;

    let fps = stream
        .get("r_frame_rate")
        .and_then(|v| v.as_str())
        .map(parse_rational)
        .transpose()?
        .unwrap_or(30.0);

    let duration_secs = stream
        .get("duration")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            json.get("format")
                .and_then(|f| f.get("duration"))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
        })
        .ok_or_else(|| FfmpegCliError::BadOutput("missing duration".into()))?;

    Ok(ProbedVideo {
        width,
        height,
        fps,
        duration_secs,
    })
}

fn parse_rational(s: &str) -> Result<f64, FfmpegCliError> {
    if let Some((num, den)) = s.split_once('/') {
        let num: f64 = num
            .parse()
            .map_err(|e: std::num::ParseFloatError| FfmpegCliError::BadOutput(e.to_string()))?;
        let den: f64 = den
            .parse()
            .map_err(|e: std::num::ParseFloatError| FfmpegCliError::BadOutput(e.to_string()))?;
        if den == 0.0 {
            return Err(FfmpegCliError::BadOutput(format!(
                "zero denominator in {s}"
            )));
        }
        Ok(num / den)
    } else {
        s.parse::<f64>()
            .map_err(|e: std::num::ParseFloatError| FfmpegCliError::BadOutput(e.to_string()))
    }
}

/// Continuously drains a child process's stderr pipe on a background thread for the life
/// of the process, discarding the bytes. `VideoReader`/`VideoEncoder` pipe stderr *and*
/// separately block synchronously reading stdout / writing stdin — an unread stderr pipe
/// fills its OS buffer once ffmpeg writes enough (a handful of KB of warnings is plenty),
/// at which point ffmpeg blocks trying to write more, while we're blocked reading/writing
/// the other pipe, deadlocking the whole decode/encode. Draining it independently makes
/// that structurally impossible.
fn drain_stderr(stderr: std::process::ChildStderr) {
    std::thread::spawn(move || {
        let mut sink = Vec::new();
        let _ = BufReader::new(stderr).read_to_end(&mut sink);
    });
}

#[derive(Debug, Clone)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Decode-time scaling/pacing knobs for `VideoReader`. Used by the playback engine to
/// decode at panel resolution (instead of full source resolution) and at the project's
/// output fps (instead of the source's native fps), rather than downscaling/retiming
/// full-resolution frames after the fact.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReaderOptions {
    /// Decode scaled so the output height matches this (even-rounded); width follows the
    /// source aspect ratio (also even-rounded). `None` or `>=` source height keeps native
    /// resolution.
    pub target_height: Option<u32>,
    /// Force this output frame rate (ffmpeg duplicates/drops frames to match). `None`
    /// keeps the source's native frame rate.
    pub output_fps: Option<f64>,
}

/// Round to the nearest integer, then align *up* to the next even number — mirrors
/// ffmpeg's `FFALIGN(lrint(x), 2)`, which is what libavfilter's `scale` filter actually
/// uses to resolve a `-2` dimension. Our previous implementation rounded down (integer
/// division truncation) instead of up, which mismatched ffmpeg's real output dimensions
/// and desynced the raw RGBA byte stream from our declared frame size.
fn ffalign_even(x: f64) -> u32 {
    let rounded = x.round() as i64;
    (((rounded + 1) & !1).max(2)) as u32
}

/// Scaled (width, height) for `target_height`, matching ffmpeg's `scale=-2:h` behavior:
/// height is even-aligned, width follows source aspect ratio and is also even-aligned
/// (both rounded up, not down — see `ffalign_even`).
fn scaled_dimensions(src_width: u32, src_height: u32, target_height: Option<u32>) -> (u32, u32) {
    match target_height {
        Some(h) if h > 0 && h < src_height => {
            let height = ffalign_even(h as f64);
            let width_f = src_width as f64 * height as f64 / src_height as f64;
            let width = ffalign_even(width_f);
            (width, height)
        }
        _ => (src_width, src_height),
    }
}

/// Sequential RGBA frame reader backed by a long-lived `ffmpeg` decode pipe.
pub struct VideoReader {
    child: Child,
    stdout: BufReader<ChildStdout>,
    width: u32,
    height: u32,
    frame_bytes: usize,
}

type ChildStdout = std::process::ChildStdout;

impl VideoReader {
    pub fn open(path: &Path, start_secs: f64) -> Result<Self, FfmpegCliError> {
        Self::open_with(path, start_secs, &ReaderOptions::default())
    }

    pub fn open_with(
        path: &Path,
        start_secs: f64,
        opts: &ReaderOptions,
    ) -> Result<Self, FfmpegCliError> {
        let probed = probe_video(path)?;
        let (width, height) = scaled_dimensions(probed.width, probed.height, opts.target_height);

        let mut cmd = Command::new(ffmpeg_path()?);
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{start_secs:.6}"),
            "-i",
        ])
        .arg(path);
        if let Some(fps) = opts.output_fps {
            cmd.args(["-r", &format!("{fps:.6}")]);
        }
        if width != probed.width || height != probed.height {
            cmd.args(["-vf", &format!("scale=-2:{height}")]);
        }
        cmd.args(["-an", "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| FfmpegCliError::SpawnFailed {
            tool: "ffmpeg",
            message: e.to_string(),
        })?;

        if let Some(stderr) = child.stderr.take() {
            drain_stderr(stderr);
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| FfmpegCliError::BadOutput("no stdout".into()))?;
        let frame_bytes = (width * height * 4) as usize;

        Ok(Self {
            child,
            stdout: BufReader::new(stdout),
            width,
            height,
            frame_bytes,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn read_frame(&mut self) -> Result<Option<RgbaFrame>, FfmpegCliError> {
        let mut buf = vec![0u8; self.frame_bytes];
        match self.stdout.read_exact(&mut buf) {
            Ok(()) => Ok(Some(RgbaFrame {
                width: self.width,
                height: self.height,
                pixels: buf,
            })),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

impl Drop for VideoReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// H.264 MP4 encoder that accepts raw RGBA frames on stdin.
pub struct VideoEncoder {
    child: Child,
    frame_bytes: usize,
}

impl VideoEncoder {
    pub fn open(
        output_path: &Path,
        width: u32,
        height: u32,
        fps: f64,
    ) -> Result<Self, FfmpegCliError> {
        let mut child = Command::new(ffmpeg_path()?)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &format!("{width}x{height}"),
                "-r",
                &format!("{fps:.6}"),
                "-i",
                "pipe:0",
                "-an",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
            ])
            .arg(output_path)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| FfmpegCliError::SpawnFailed {
                tool: "ffmpeg",
                message: e.to_string(),
            })?;

        if let Some(stderr) = child.stderr.take() {
            drain_stderr(stderr);
        }

        Ok(Self {
            child,
            frame_bytes: (width * height * 4) as usize,
        })
    }

    pub fn write_frame(&mut self, pixels: &[u8]) -> Result<(), FfmpegCliError> {
        if pixels.len() != self.frame_bytes {
            return Err(FfmpegCliError::BadOutput(format!(
                "expected {} bytes, got {}",
                self.frame_bytes,
                pixels.len()
            )));
        }
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| FfmpegCliError::BadOutput("encoder stdin closed".into()))?;
        stdin.write_all(pixels)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), FfmpegCliError> {
        drop(self.child.stdin.take());
        let status = self.child.wait()?;
        if !status.success() {
            return Err(FfmpegCliError::NonZeroExit(status.code().unwrap_or(-1)));
        }
        Ok(())
    }
}

/// Mux a video-only MP4 with a WAV/MP4 audio track.
pub fn mux_video_audio(
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
) -> Result<(), FfmpegCliError> {
    let status = Command::new(ffmpeg_path()?)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(video_path)
        .args(["-i"])
        .arg(audio_path)
        .args([
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            "-movflags",
            "+faststart",
        ])
        .arg(output_path)
        .status()
        .map_err(|e| FfmpegCliError::SpawnFailed {
            tool: "ffmpeg",
            message: e.to_string(),
        })?;
    if !status.success() {
        return Err(FfmpegCliError::NonZeroExit(status.code().unwrap_or(-1)));
    }
    Ok(())
}

/// Sidechain ducking applied when voice and music buses are both present.
#[derive(Debug, Clone, Copy)]
pub struct DuckSettings {
    pub duck_db: f64,
}

/// Mix enabled audio clips onto a timeline-length WAV file.
pub fn mix_timeline_audio(
    clips: &[AudioMixClip],
    sample_rate: u32,
    duration_secs: f64,
    output_wav: &Path,
    duck: Option<DuckSettings>,
) -> Result<(), FfmpegCliError> {
    if clips.is_empty() {
        return Err(FfmpegCliError::BadOutput("no audio clips".into()));
    }

    if let Some(duck_cfg) = duck {
        let (voice, music, other) = partition_clips(clips);
        if !voice.is_empty() && !music.is_empty() {
            let temp_dir =
                std::env::temp_dir().join(format!("uppercut-audio-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&temp_dir).map_err(FfmpegCliError::Io)?;

            let voice_wav = temp_dir.join("voice.wav");
            let music_wav = temp_dir.join("music.wav");
            let ducked_wav = temp_dir.join("ducked_music.wav");

            let voice_clips: Vec<AudioMixClip> = voice.into_iter().cloned().collect();
            let music_clips: Vec<AudioMixClip> = music.into_iter().cloned().collect();
            mix_clip_bus(&voice_clips, sample_rate, duration_secs, &voice_wav)?;
            mix_clip_bus(&music_clips, sample_rate, duration_secs, &music_wav)?;

            // `sidechaincompress` alone already produces the gain reduction ("ducking")
            // when the voice/dialog sidechain is active; no extra gain stage is applied
            // after it. An earlier version chained `volume={db_to_linear(-duck_db)}` here,
            // which for the default duck_db=-12 computed a +12dB *boost* on the whole
            // compressed signal (right sign flipped: -(-12) = +12) — that boost applied
            // uniformly to both ducked and non-ducked passages, undoing the compressor's
            // reduction during dialogue and amplifying the music beyond its original level
            // the rest of the time. `duck_cfg.duck_db`'s magnitude isn't yet mapped to a
            // precise output dB target (only its sign gates ducking on/off, see
            // `duck_settings` in export/mod.rs) — a follow-up could tune ratio/threshold
            // from it, but leaving gain at unity here is the safe, clearly-correct default.
            let _ = duck_cfg.duck_db;
            let filter =
                "[0:a][1:a]sidechaincompress=threshold=0.02:ratio=8:attack=200:release=1000[out]"
                    .to_string();
            let status = Command::new(ffmpeg_path()?)
                .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                .arg(&music_wav)
                .args(["-i"])
                .arg(&voice_wav)
                .args(["-filter_complex", &filter, "-map", "[out]"])
                .arg(&ducked_wav)
                .status()
                .map_err(|e| FfmpegCliError::SpawnFailed {
                    tool: "ffmpeg",
                    message: e.to_string(),
                })?;
            if !status.success() {
                std::fs::remove_dir_all(&temp_dir).ok();
                return Err(FfmpegCliError::NonZeroExit(status.code().unwrap_or(-1)));
            }

            let mut final_clips = Vec::new();
            final_clips.push(AudioMixClip {
                path: voice_wav.clone(),
                position_secs: 0.0,
                source_in_secs: 0.0,
                source_out_secs: duration_secs,
                gain_db: 0.0,
                fade_in_secs: 0.0,
                fade_out_secs: 0.0,
                role: None,
            });
            final_clips.push(AudioMixClip {
                path: ducked_wav.clone(),
                position_secs: 0.0,
                source_in_secs: 0.0,
                source_out_secs: duration_secs,
                gain_db: 0.0,
                fade_in_secs: 0.0,
                fade_out_secs: 0.0,
                role: None,
            });
            for c in other {
                final_clips.push(c.clone());
            }

            let result = mix_clip_bus(&final_clips, sample_rate, duration_secs, output_wav);
            std::fs::remove_dir_all(&temp_dir).ok();
            return result;
        }
    }

    mix_clip_bus(clips, sample_rate, duration_secs, output_wav)
}

fn mix_clip_bus(
    clips: &[AudioMixClip],
    sample_rate: u32,
    duration_secs: f64,
    output_wav: &Path,
) -> Result<(), FfmpegCliError> {
    if clips.is_empty() {
        return Err(FfmpegCliError::BadOutput("no audio clips".into()));
    }

    let temp_dir = std::env::temp_dir().join(format!("uppercut-audio-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).map_err(FfmpegCliError::Io)?;

    let mut segment_paths = Vec::new();
    let mut filter_parts = Vec::new();

    for (i, clip) in clips.iter().enumerate() {
        let seg = temp_dir.join(format!("seg_{i}.wav"));
        let seg_duration = clip.source_out_secs - clip.source_in_secs;
        let af = build_audio_filter(clip, seg_duration);

        let status = Command::new(ffmpeg_path()?)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-ss",
                &format!("{:.6}", clip.source_in_secs),
                "-i",
            ])
            .arg(&clip.path)
            .args([
                "-t",
                &format!("{seg_duration:.6}"),
                "-af",
                &af,
                "-ar",
                &sample_rate.to_string(),
                "-ac",
                "2",
            ])
            .arg(&seg)
            .status()
            .map_err(|e| FfmpegCliError::SpawnFailed {
                tool: "ffmpeg",
                message: e.to_string(),
            })?;
        if !status.success() {
            std::fs::remove_dir_all(&temp_dir).ok();
            return Err(FfmpegCliError::NonZeroExit(status.code().unwrap_or(-1)));
        }
        segment_paths.push(seg);

        let delay_ms = (clip.position_secs * 1000.0).round() as u64;
        filter_parts.push(format!("[{i}:a]adelay={delay_ms}|{delay_ms}[a{i}]"));
    }

    let n = clips.len();
    let mix_labels = (0..n).map(|i| format!("[a{i}]")).collect::<String>();
    let filter = format!(
        "{};{}amix=inputs={n}:duration=longest:dropout_transition=0,apad=whole_dur={duration_secs:.6}[out]",
        filter_parts.join(";"),
        mix_labels,
    );

    let mut cmd = Command::new(ffmpeg_path()?);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
    for seg in &segment_paths {
        cmd.args(["-i"]).arg(seg);
    }
    cmd.args(["-filter_complex", &filter, "-map", "[out]", "-ar"])
        .arg(sample_rate.to_string())
        .args(["-ac", "2"])
        .arg(output_wav);

    let status = cmd.status().map_err(|e| FfmpegCliError::SpawnFailed {
        tool: "ffmpeg",
        message: e.to_string(),
    })?;
    std::fs::remove_dir_all(&temp_dir).ok();
    if !status.success() {
        return Err(FfmpegCliError::NonZeroExit(status.code().unwrap_or(-1)));
    }
    Ok(())
}

fn build_audio_filter(clip: &AudioMixClip, seg_duration: f64) -> String {
    let volume = db_to_linear(clip.gain_db);
    let mut parts = vec![format!("volume={volume:.6}")];
    if clip.fade_in_secs > 0.0 {
        parts.push(format!("afade=t=in:st=0:d={:.6}", clip.fade_in_secs));
    }
    if clip.fade_out_secs > 0.0 {
        let st = (seg_duration - clip.fade_out_secs).max(0.0);
        parts.push(format!(
            "afade=t=out:st={st:.6}:d={:.6}",
            clip.fade_out_secs
        ));
    }
    parts.join(",")
}

fn partition_clips(
    clips: &[AudioMixClip],
) -> (Vec<&AudioMixClip>, Vec<&AudioMixClip>, Vec<&AudioMixClip>) {
    let mut voice = Vec::new();
    let mut music = Vec::new();
    let mut other = Vec::new();
    for c in clips {
        match c.role {
            Some(TrackAudioRole::Voiceover) | Some(TrackAudioRole::Dialog) => voice.push(c),
            Some(TrackAudioRole::Music) => music.push(c),
            _ => other.push(c),
        }
    }
    (voice, music, other)
}

fn db_to_linear(gain_db: f64) -> f64 {
    10_f64.powf(gain_db / 20.0)
}

#[derive(Debug, Clone)]
pub struct AudioMixClip {
    pub path: PathBuf,
    pub position_secs: f64,
    pub source_in_secs: f64,
    pub source_out_secs: f64,
    pub gain_db: f64,
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
    pub role: Option<TrackAudioRole>,
}

#[cfg(test)]
mod scaling_tests {
    use super::scaled_dimensions;

    #[test]
    fn scaled_dimensions_keeps_native_size_when_target_is_none_or_larger() {
        assert_eq!(scaled_dimensions(1920, 1080, None), (1920, 1080));
        assert_eq!(scaled_dimensions(1920, 1080, Some(1080)), (1920, 1080));
        assert_eq!(scaled_dimensions(1920, 1080, Some(2000)), (1920, 1080));
    }

    #[test]
    fn scaled_dimensions_downscales_matching_source_aspect_ratio() {
        // 16:9 source scaled to 720p height should land on the standard 1280x720.
        let (w, h) = scaled_dimensions(1920, 1080, Some(720));
        assert_eq!(h, 720);
        assert_eq!(w, 1280);
    }

    #[test]
    fn scaled_dimensions_always_even() {
        let (w, h) = scaled_dimensions(1081, 721, Some(361));
        assert_eq!(w % 2, 0);
        assert_eq!(h % 2, 0);
    }

    #[test]
    fn scaled_dimensions_rounds_up_to_even_like_ffmpeg_ffalign() {
        // ffmpeg's `scale=-2:h` resolves a `-2` dimension via FFALIGN(lrint(x), 2), which
        // rounds UP to the next even number, not down. An odd target height of 361 must
        // become 362, matching real ffmpeg output exactly (a prior version floored this to
        // 360 via integer-division truncation, desyncing our declared frame size from
        // ffmpeg's actual raw byte stream).
        let (_, h) = scaled_dimensions(1920, 1080, Some(361));
        assert_eq!(h, 362);
    }

    #[test]
    fn scaled_dimensions_width_rounds_up_like_ffmpeg_ffalign() {
        let (w, _) = scaled_dimensions(1921, 1081, Some(721));
        assert_eq!(w, 1284);
    }
}
