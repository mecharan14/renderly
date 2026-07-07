//! Read-only perception helpers for MCP and CLI (Phase 1): transcript, frame preview.

mod analysis;

pub use analysis::{
    audio_peaks, detect_scenes, detect_silence, AnalysisError, AudioPeaks, SceneCut, SilenceSpan,
};

use crate::commands::ExportPreset;
use crate::export::{render_frame_at, ExportError, ExportSettings};
use crate::media::{ffmpeg_available, ffmpeg_path, FfmpegCliError};
use crate::project::{MediaKind, Project};
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PerceiveError {
    #[error("{0}")]
    Export(#[from] ExportError),
    #[error("{0}")]
    Ffmpeg(#[from] FfmpegCliError),
    #[error("media not found: {0}")]
    MediaNotFound(Uuid),
    #[error("whisper not available; install whisper.cpp CLI and set UPPERCUT_WHISPER_MODEL")]
    WhisperNotAvailable,
    #[error("whisper failed: {0}")]
    WhisperFailed(String),
    #[error("png encode failed: {0}")]
    PngEncode(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub media_id: Uuid,
    pub segments: Vec<TranscriptSegment>,
}

/// Render the composited timeline frame at `time_secs` as PNG bytes.
pub fn render_frame_png(
    project: &Project,
    time_secs: f64,
    preset: ExportPreset,
) -> Result<Vec<u8>, PerceiveError> {
    let settings = ExportSettings::from_preset(&preset, project);
    let rgba = render_frame_at(project, time_secs, settings)?;
    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png);
    encoder
        .write_image(
            &rgba,
            settings.width,
            settings.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| PerceiveError::PngEncode(e.to_string()))?;
    Ok(png)
}

/// Transcribe a media item with whisper.cpp CLI (local STT).
pub fn transcribe_media(project: &Project, media_id: Uuid) -> Result<Transcript, PerceiveError> {
    if !ffmpeg_available() {
        return Err(FfmpegCliError::NotFound.into());
    }
    let media = project
        .find_media(media_id)
        .ok_or(PerceiveError::MediaNotFound(media_id))?;
    if media.kind != MediaKind::Video && media.kind != MediaKind::Audio {
        return Err(PerceiveError::WhisperFailed(
            "transcription requires video or audio media".into(),
        ));
    }

    let model =
        std::env::var("UPPERCUT_WHISPER_MODEL").map_err(|_| PerceiveError::WhisperNotAvailable)?;
    let whisper = find_whisper_cli().ok_or(PerceiveError::WhisperNotAvailable)?;

    let dir = std::env::temp_dir().join(format!("uppercut-whisper-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(FfmpegCliError::Io)?;
    let wav = dir.join("audio.wav");
    let out_prefix = dir.join("out");

    let status = Command::new(ffmpeg_path()?)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&media.path)
        .args(["-vn", "-ar", "16000", "-ac", "1", "-f", "wav"])
        .arg(&wav)
        .status()
        .map_err(|e| FfmpegCliError::SpawnFailed {
            tool: "ffmpeg",
            message: e.to_string(),
        })?;
    if !status.success() {
        return Err(PerceiveError::WhisperFailed(
            "ffmpeg audio extract failed".into(),
        ));
    }

    let status = Command::new(&whisper)
        .args(["-m", &model, "-f"])
        .arg(&wav)
        .args(["-oj", "-of"])
        .arg(&out_prefix)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| PerceiveError::WhisperFailed(e.to_string()))?;
    if !status.success() {
        return Err(PerceiveError::WhisperFailed(
            "whisper-cli exited with error".into(),
        ));
    }

    let json_path = dir.join("out.json");
    let data = std::fs::read_to_string(&json_path)
        .map_err(|e| PerceiveError::WhisperFailed(e.to_string()))?;
    let parsed: WhisperJson =
        serde_json::from_str(&data).map_err(|e| PerceiveError::WhisperFailed(e.to_string()))?;

    let segments = parsed
        .transcription
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| {
            let start = s.timestamps.as_ref()?.from.parse::<f64>().ok()?;
            let end = s.timestamps.as_ref()?.to.parse::<f64>().ok()?;
            Some(TranscriptSegment {
                start_secs: start,
                end_secs: end,
                text: s.text.trim().to_string(),
            })
        })
        .collect();

    std::fs::remove_dir_all(&dir).ok();

    Ok(Transcript { media_id, segments })
}

fn find_whisper_cli() -> Option<PathBuf> {
    for name in ["whisper-cli", "whisper", "main"] {
        if let Some(path) = which_tool(name) {
            return Some(path);
        }
    }
    None
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

#[derive(Debug, Deserialize)]
struct WhisperJson {
    transcription: Option<Vec<WhisperSegment>>,
}

#[derive(Debug, Deserialize)]
struct WhisperSegment {
    timestamps: Option<WhisperTimestamps>,
    text: String,
}

#[derive(Debug, Deserialize)]
struct WhisperTimestamps {
    from: String,
    to: String,
}
