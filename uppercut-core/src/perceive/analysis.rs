//! FFmpeg-backed media analysis: silence, scene cuts, waveform peaks.

use crate::media::{ffmpeg_available, ffmpeg_path, FfmpegCliError};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error("{0}")]
    Ffmpeg(#[from] FfmpegCliError),
    #[error("analysis parse failed: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilenceSpan {
    pub start_secs: f64,
    pub end_secs: f64,
    pub duration_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneCut {
    pub time_secs: f64,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioPeaks {
    pub bucket_secs: f64,
    pub peaks: Vec<f32>,
}

/// Detect silent spans using FFmpeg `silencedetect`.
pub fn detect_silence(
    path: &Path,
    noise_db: f64,
    min_duration_secs: f64,
) -> Result<Vec<SilenceSpan>, AnalysisError> {
    if !ffmpeg_available() {
        return Err(FfmpegCliError::NotFound.into());
    }

    let filter = format!("silencedetect=noise={noise_db}dB:d={min_duration_secs}");
    let output = Command::new(ffmpeg_path()?)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .args(["-af", &filter, "-f", "null", "-"])
        .output()
        .map_err(|e| FfmpegCliError::SpawnFailed {
            tool: "ffmpeg",
            message: e.to_string(),
        })?;

    parse_silence(&String::from_utf8_lossy(&output.stderr))
}

fn parse_silence(stderr: &str) -> Result<Vec<SilenceSpan>, AnalysisError> {
    let mut spans = Vec::new();
    let mut start: Option<f64> = None;

    for line in stderr.lines() {
        if let Some(rest) = line.split("silence_start:").nth(1) {
            if let Ok(t) = rest.trim().parse::<f64>() {
                start = Some(t);
            }
        }
        if let Some(rest) = line.split("silence_end:").nth(1) {
            let end_str = rest.split('|').next().unwrap_or(rest).trim();
            if let (Some(s), Ok(end)) = (start.take(), end_str.parse::<f64>()) {
                spans.push(SilenceSpan {
                    start_secs: s,
                    end_secs: end,
                    duration_secs: end - s,
                });
            }
        }
    }
    Ok(spans)
}

/// Detect scene changes using FFmpeg `select=gt(scene,threshold)`.
pub fn detect_scenes(path: &Path, threshold: f64) -> Result<Vec<SceneCut>, AnalysisError> {
    if !ffmpeg_available() {
        return Err(FfmpegCliError::NotFound.into());
    }

    let filter = format!("select='gt(scene,{threshold})',showinfo");
    let output = Command::new(ffmpeg_path()?)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .args(["-vf", &filter, "-f", "null", "-"])
        .output()
        .map_err(|e| FfmpegCliError::SpawnFailed {
            tool: "ffmpeg",
            message: e.to_string(),
        })?;

    parse_scenes(&String::from_utf8_lossy(&output.stderr))
}

fn parse_scenes(stderr: &str) -> Result<Vec<SceneCut>, AnalysisError> {
    let mut cuts = Vec::new();
    for line in stderr.lines() {
        if !line.contains("showinfo") {
            continue;
        }
        let mut time = None;
        let mut score = 0.3_f64;
        for part in line.split_whitespace() {
            if let Some(v) = part.strip_prefix("pts_time:") {
                time = v.parse().ok();
            }
            if let Some(v) = part.strip_prefix("scene:") {
                score = v.parse().unwrap_or(score);
            }
        }
        if let Some(t) = time {
            cuts.push(SceneCut {
                time_secs: t,
                score,
            });
        }
    }
    Ok(cuts)
}

/// Downsampled peak envelope for waveform display / agent perception.
pub fn audio_peaks(path: &Path, buckets: u32) -> Result<AudioPeaks, AnalysisError> {
    if !ffmpeg_available() {
        return Err(FfmpegCliError::NotFound.into());
    }
    if buckets == 0 {
        return Err(AnalysisError::Parse("buckets must be > 0".into()));
    }

    let duration = media_duration(path);
    let bucket_secs = duration / buckets as f64;

    let output = Command::new(ffmpeg_path()?)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "8000", "-f", "f32le", "pipe:1"])
        .output()
        .map_err(|e| FfmpegCliError::SpawnFailed {
            tool: "ffmpeg",
            message: e.to_string(),
        })?;

    if !output.status.success() {
        return Err(FfmpegCliError::NonZeroExit(output.status.code().unwrap_or(-1)).into());
    }

    let samples: &[f32] = bytemuck::cast_slice(&output.stdout);
    let peaks = bucket_peaks(samples, buckets);

    Ok(AudioPeaks { bucket_secs, peaks })
}

/// Downsample `samples` into `buckets` peak values. Guards against `buckets` exceeding the
/// sample count (very short clips, or a large requested bucket count): `samples_per_bucket`
/// floors to >= 1, so without clamping the start index, `i * samples_per_bucket` can exceed
/// `samples.len()` and panic by slicing with start > end.
fn bucket_peaks(samples: &[f32], buckets: u32) -> Vec<f32> {
    let samples_per_bucket = (samples.len() as f64 / buckets as f64).max(1.0) as usize;
    let mut peaks = Vec::with_capacity(buckets as usize);
    for i in 0..buckets as usize {
        let start = (i * samples_per_bucket).min(samples.len());
        let end = ((i + 1) * samples_per_bucket).min(samples.len());
        let peak = samples[start..end]
            .iter()
            .map(|s| s.abs())
            .fold(0.0_f32, f32::max);
        peaks.push(peak);
    }
    peaks
}

fn media_duration(path: &Path) -> f64 {
    if let Ok(p) = crate::media::probe(path) {
        if let Some(d) = p.duration_secs {
            return d;
        }
    }
    if let Ok(v) = crate::media::probe_video(path) {
        return v.duration_secs;
    }
    60.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_silence_lines() {
        let stderr = "[silencedetect @ 0x1] silence_start: 1.5\n[silencedetect @ 0x1] silence_end: 3.0 | silence_duration: 1.5\n";
        let spans = parse_silence(stderr).unwrap();
        assert_eq!(spans.len(), 1);
        assert!((spans[0].duration_secs - 1.5).abs() < 1e-9);
    }

    #[test]
    fn bucket_peaks_does_not_panic_when_buckets_exceed_samples() {
        // A very short clip (few samples) with many requested buckets used to panic:
        // `i * samples_per_bucket` could exceed `samples.len()` while `end` stayed clamped,
        // producing a `start > end` slice panic.
        let samples = [0.1_f32, 0.5, 0.2];
        let peaks = bucket_peaks(&samples, 256);
        assert_eq!(peaks.len(), 256);
        assert!(peaks.iter().any(|&p| p > 0.0));
    }

    #[test]
    fn bucket_peaks_finds_max_abs_per_bucket() {
        let samples = [0.1_f32, -0.9, 0.2, 0.3];
        let peaks = bucket_peaks(&samples, 2);
        assert_eq!(peaks.len(), 2);
        assert!((peaks[0] - 0.9).abs() < 1e-6);
        assert!((peaks[1] - 0.3).abs() < 1e-6);
    }
}
