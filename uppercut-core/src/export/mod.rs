//! Timeline → decode → wgpu composite → encode export pipeline.

use crate::captions::{render_caption, CaptionError};
use crate::commands::ExportPreset;
use crate::compose::{ComposeError, Compositor};
use crate::media::{
    mix_timeline_audio, mux_video_audio, AudioMixClip, DuckSettings, FfmpegCliError, ReaderOptions,
    RgbaFrame, VideoEncoder, VideoReader,
};
use crate::project::{Clip, MediaKind, Project, TrackAudioRole, TrackKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("{0}")]
    Ffmpeg(#[from] FfmpegCliError),
    #[error("{0}")]
    Compose(#[from] ComposeError),
    #[error("{0}")]
    Caption(#[from] CaptionError),
    #[error("no enabled video clips on the timeline")]
    EmptyTimeline,
    #[error("media not found: {0}")]
    MediaNotFound(uuid::Uuid),
    #[error("media {0} is not video")]
    NotVideo(uuid::Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportSettings {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

impl ExportSettings {
    pub fn from_preset(preset: &ExportPreset, project: &Project) -> Self {
        match preset {
            ExportPreset::TikTok9x16 => Self {
                width: 1080,
                height: 1920,
                fps: project.settings.fps,
            },
            ExportPreset::Youtube16x9 => Self {
                width: 1920,
                height: 1080,
                fps: project.settings.fps,
            },
            ExportPreset::Custom { width, height, fps } => Self {
                width: *width,
                height: *height,
                fps: *fps,
            },
        }
    }
}

struct ActiveLayer {
    media_id: uuid::Uuid,
    path: PathBuf,
    source_time: f64,
}

struct ActiveCaption {
    text: String,
    style_id: String,
}

struct DecoderState {
    path: PathBuf,
    reader_opts: ReaderOptions,
    reader: Option<VideoReader>,
    last_source_time: f64,
}

impl DecoderState {
    fn new(path: PathBuf, reader_opts: ReaderOptions) -> Self {
        Self {
            path,
            reader_opts,
            reader: None,
            last_source_time: f64::NAN,
        }
    }

    fn frame_at(&mut self, source_time: f64) -> Result<Option<RgbaFrame>, FfmpegCliError> {
        let needs_reopen = self.reader.is_none()
            || source_time + 1e-6 < self.last_source_time
            || (source_time - self.last_source_time).abs() > 0.5;

        if needs_reopen {
            self.reader = Some(VideoReader::open_with(
                &self.path,
                source_time,
                &self.reader_opts,
            )?);
        }

        let reader = self.reader.as_mut().expect("reader just opened");
        let frame = reader.read_frame()?;
        self.last_source_time = source_time;
        Ok(frame)
    }
}

/// Decode-time knobs for a `FrameRenderer`. `target_height` decodes video layers
/// downscaled (e.g. to preview-panel resolution) instead of full source resolution;
/// `output_fps` paces decode to a fixed frame rate instead of the source's native fps.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DecodeOptions {
    pub target_height: Option<u32>,
    pub output_fps: Option<f64>,
}

/// Persistent composite-frame renderer: holds one wgpu `Compositor` and one decoder per
/// source media across repeated `render` calls, instead of recreating the GPU device and
/// respawning ffmpeg per video layer on every call (what `render_frame_at` — now a
/// one-shot wrapper around this — used to do, and what made the old per-frame preview
/// path freeze the UI thread). Callers that render a sequence of frames from the same
/// project (playback, export) should keep one `FrameRenderer` alive for the whole run.
pub struct FrameRenderer {
    settings: ExportSettings,
    decode_opts: DecodeOptions,
    compositor: Compositor,
    decoders: HashMap<uuid::Uuid, DecoderState>,
}

impl FrameRenderer {
    pub fn new(settings: ExportSettings, decode_opts: DecodeOptions) -> Result<Self, ExportError> {
        let compositor = Compositor::new(settings.width, settings.height)?;
        Ok(Self {
            settings,
            decode_opts,
            compositor,
            decoders: HashMap::new(),
        })
    }

    pub fn render(&mut self, project: &Project, time_secs: f64) -> Result<Vec<u8>, ExportError> {
        let layers = active_layers(project, time_secs)?;
        let mut rgba_layers = Vec::with_capacity(layers.len() + 2);

        let reader_opts = ReaderOptions {
            target_height: self.decode_opts.target_height,
            output_fps: self.decode_opts.output_fps,
        };
        for layer in &layers {
            let decoder = self
                .decoders
                .entry(layer.media_id)
                .or_insert_with(|| DecoderState::new(layer.path.clone(), reader_opts));
            if let Some(frame) = decoder.frame_at(layer.source_time)? {
                rgba_layers.push(frame);
            }
        }

        for cap in active_captions(project, time_secs) {
            rgba_layers.push(render_caption(
                &cap.text,
                &cap.style_id,
                self.settings.width,
                self.settings.height,
            )?);
        }

        self.compositor
            .composite(&rgba_layers)
            .map_err(ExportError::from)
    }
}

/// Render one composited RGBA frame at `time_secs` (video + burned-in captions).
///
/// One-shot convenience wrapper: builds a `FrameRenderer` (fresh GPU device + decoders)
/// for a single frame. Fine for perception/MCP call sites that render one isolated frame;
/// callers rendering a sequence (playback, export) should use `FrameRenderer` directly to
/// avoid rebuilding the compositor and decoders per frame.
pub fn render_frame_at(
    project: &Project,
    time_secs: f64,
    settings: ExportSettings,
) -> Result<Vec<u8>, ExportError> {
    let mut renderer = FrameRenderer::new(settings, DecodeOptions::default())?;
    renderer.render(project, time_secs)
}

/// Render the project's timeline to an MP4 file (video + captions + mixed audio).
pub fn export_project(
    project: &Project,
    output_path: &Path,
    preset: ExportPreset,
) -> Result<(), ExportError> {
    if !crate::media::ffmpeg_available() {
        return Err(FfmpegCliError::NotFound.into());
    }

    let settings = ExportSettings::from_preset(&preset, project);
    let duration_secs = timeline_duration(project);
    if duration_secs <= 0.0 {
        return Err(ExportError::EmptyTimeline);
    }

    if !has_video_content(project) {
        return Err(ExportError::EmptyTimeline);
    }

    let total_frames = (duration_secs * settings.fps).ceil() as u64;
    if total_frames == 0 {
        return Err(ExportError::EmptyTimeline);
    }

    let temp_dir = std::env::temp_dir().join(format!("uppercut-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).map_err(FfmpegCliError::Io)?;
    let temp_video = temp_dir.join("video.mp4");

    let mut encoder =
        VideoEncoder::open(&temp_video, settings.width, settings.height, settings.fps)?;
    // `output_fps: Some(settings.fps)` paces each source decoder to the export frame rate
    // (ffmpeg duplicates/drops frames to match) rather than the source's native fps —
    // this also fixes frame-pacing drift for sources whose fps doesn't match the export.
    let mut renderer = FrameRenderer::new(
        settings,
        DecodeOptions {
            target_height: None,
            output_fps: Some(settings.fps),
        },
    )?;

    for frame_idx in 0..total_frames {
        let t = frame_idx as f64 / settings.fps;
        let pixels = renderer.render(project, t)?;
        encoder.write_frame(&pixels)?;
    }
    encoder.finish()?;

    let audio_clips = collect_audio_clips(project);
    if audio_clips.is_empty() {
        std::fs::rename(&temp_video, output_path).or_else(|_| {
            std::fs::copy(&temp_video, output_path).map_err(FfmpegCliError::Io)?;
            Ok::<(), FfmpegCliError>(())
        })?;
    } else {
        let temp_audio = temp_dir.join("audio.wav");
        let duck = duck_settings(project);
        mix_timeline_audio(
            &audio_clips,
            project.settings.sample_rate,
            duration_secs,
            &temp_audio,
            duck,
        )?;
        mux_video_audio(&temp_video, &temp_audio, output_path)?;
    }

    std::fs::remove_dir_all(&temp_dir).ok();
    Ok(())
}

fn collect_audio_clips(project: &Project) -> Vec<AudioMixClip> {
    let mut clips = Vec::new();
    for track in &project.tracks {
        if track.kind != TrackKind::Audio || track.muted {
            continue;
        }
        for clip in &track.clips {
            let Clip::Audio(a) = clip else { continue };
            if !a.enabled {
                continue;
            }
            if let Some(media) = project.find_media(a.media_id) {
                clips.push(AudioMixClip {
                    path: media.path.clone(),
                    position_secs: a.position_secs,
                    source_in_secs: a.source_in_secs,
                    source_out_secs: a.source_out_secs,
                    gain_db: a.gain_db,
                    fade_in_secs: a.fade_in_secs,
                    fade_out_secs: a.fade_out_secs,
                    role: track.audio_role,
                });
            }
        }
    }
    clips
}

fn duck_settings(project: &Project) -> Option<DuckSettings> {
    if project.settings.duck_db >= 0.0 {
        return None;
    }
    let has_voice = project.tracks.iter().any(|t| {
        t.kind == TrackKind::Audio
            && matches!(
                t.audio_role,
                Some(TrackAudioRole::Voiceover) | Some(TrackAudioRole::Dialog)
            )
            && t.clips
                .iter()
                .any(|c| matches!(c, Clip::Audio(a) if a.enabled))
    });
    let has_music = project.tracks.iter().any(|t| {
        t.kind == TrackKind::Audio
            && t.audio_role == Some(TrackAudioRole::Music)
            && t.clips
                .iter()
                .any(|c| matches!(c, Clip::Audio(a) if a.enabled))
    });
    if has_voice && has_music {
        Some(DuckSettings {
            duck_db: project.settings.duck_db,
        })
    } else {
        None
    }
}

fn has_video_content(project: &Project) -> bool {
    project.tracks.iter().any(|track| {
        track.kind == TrackKind::Video
            && !track.hidden
            && track.clips.iter().any(|clip| match clip {
                Clip::Video(c) => c.enabled,
                _ => false,
            })
    })
}

pub fn timeline_duration(project: &Project) -> f64 {
    project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.end_secs())
        .fold(0.0_f64, f64::max)
}

/// Mix timeline audio in `[start_secs, start_secs + duration_secs)` to a WAV file,
/// shifted so the file's own t=0 is `start_secs`. Returns `false` (and writes nothing) if
/// no enabled audio clip overlaps the range. File-backed rather than in-memory so the
/// playback engine can pre-mix ranges spanning minutes, not just short scrub segments.
pub fn mix_timeline_audio_range_to_file(
    project: &Project,
    start_secs: f64,
    duration_secs: f64,
    out_path: &Path,
) -> Result<bool, ExportError> {
    if duration_secs <= 0.0 {
        return Ok(false);
    }
    if !crate::media::ffmpeg_available() {
        return Err(FfmpegCliError::NotFound.into());
    }

    let clips = collect_audio_clips(project);
    if clips.is_empty() {
        return Ok(false);
    }

    let end_secs = start_secs + duration_secs;
    let mut shifted = Vec::new();
    for clip in clips {
        let clip_len = clip.source_out_secs - clip.source_in_secs;
        let clip_end = clip.position_secs + clip_len;
        let overlap_start = start_secs.max(clip.position_secs);
        let overlap_end = end_secs.min(clip_end);
        if overlap_end <= overlap_start {
            continue;
        }
        let offset = overlap_start - clip.position_secs;
        shifted.push(AudioMixClip {
            path: clip.path.clone(),
            position_secs: overlap_start - start_secs,
            source_in_secs: clip.source_in_secs + offset,
            source_out_secs: clip.source_in_secs + offset + (overlap_end - overlap_start),
            gain_db: clip.gain_db,
            fade_in_secs: clip.fade_in_secs,
            fade_out_secs: clip.fade_out_secs,
            role: clip.role,
        });
    }
    if shifted.is_empty() {
        return Ok(false);
    }

    let duck = duck_settings(project);
    mix_timeline_audio(
        &shifted,
        project.settings.sample_rate,
        duration_secs,
        out_path,
        duck,
    )?;
    Ok(true)
}

/// Mix a slice of timeline audio into an in-memory WAV (for preview scrub).
pub fn mix_timeline_audio_segment(
    project: &Project,
    start_secs: f64,
    duration_secs: f64,
) -> Result<Vec<u8>, ExportError> {
    let temp_dir = std::env::temp_dir().join(format!("uppercut-scrub-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).map_err(FfmpegCliError::Io)?;
    let temp_wav = temp_dir.join("segment.wav");

    let wrote = mix_timeline_audio_range_to_file(project, start_secs, duration_secs, &temp_wav)?;
    let bytes = if wrote {
        std::fs::read(&temp_wav).map_err(FfmpegCliError::Io)?
    } else {
        Vec::new()
    };
    std::fs::remove_dir_all(&temp_dir).ok();
    Ok(bytes)
}

fn active_layers(project: &Project, t: f64) -> Result<Vec<ActiveLayer>, ExportError> {
    let mut layers = Vec::new();

    for track in &project.tracks {
        if track.kind != TrackKind::Video || track.hidden {
            continue;
        }
        for clip in &track.clips {
            let Clip::Video(v) = clip else { continue };
            if !v.enabled {
                continue;
            }
            let start = v.position_secs;
            let end = start + (v.source_out_secs - v.source_in_secs);
            if t >= start && t < end {
                let media = project
                    .find_media(v.media_id)
                    .ok_or(ExportError::MediaNotFound(v.media_id))?;
                if media.kind != MediaKind::Video && media.kind != MediaKind::Image {
                    return Err(ExportError::NotVideo(v.media_id));
                }
                let source_time = v.source_in_secs + (t - start);
                layers.push(ActiveLayer {
                    media_id: v.media_id,
                    path: media.path.clone(),
                    source_time,
                });
                break;
            }
        }
    }

    Ok(layers)
}

fn active_captions(project: &Project, t: f64) -> Vec<ActiveCaption> {
    let mut caps = Vec::new();
    for track in &project.tracks {
        if track.kind != TrackKind::Caption || track.hidden {
            continue;
        }
        for clip in &track.clips {
            let Clip::Caption(c) = clip else { continue };
            let end = c.position_secs + c.duration_secs;
            if t >= c.position_secs && t < end {
                caps.push(ActiveCaption {
                    text: c.text.clone(),
                    style_id: c.style_id.clone(),
                });
            }
        }
    }
    caps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, Settings};
    use std::process::Command;

    fn ffmpeg_available() -> bool {
        crate::media::ffmpeg_available()
    }

    fn generate_test_video(path: &Path) {
        let status = Command::new(crate::media::ffmpeg_path().unwrap())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=320x240:rate=30",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .status()
            .expect("ffmpeg");
        assert!(status.success());
    }

    #[test]
    fn export_cuts_only_timeline_to_mp4() {
        if !ffmpeg_available() {
            eprintln!("skipping export test: ffmpeg not on PATH");
            return;
        }

        let dir = std::env::temp_dir().join(format!("uppercut-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let video_path = dir.join("src.mp4");
        let output_path = dir.join("out.mp4");
        generate_test_video(&video_path);

        let mut project = Project::new("export-test", Settings::default());
        let media_id = uuid::Uuid::new_v4();
        project.media.push(crate::project::MediaItem {
            id: media_id,
            path: video_path.clone(),
            kind: MediaKind::Video,
            duration_secs: Some(1.0),
            width: Some(320),
            height: Some(240),
            fps: Some(30.0),
        });
        let track = crate::project::Track::new(TrackKind::Video, "V1");
        project.tracks.push(track);
        project.tracks[0]
            .clips
            .push(Clip::Video(crate::project::MediaClip {
                id: uuid::Uuid::new_v4(),
                media_id,
                position_secs: 0.0,
                source_in_secs: 0.0,
                source_out_secs: 0.5,
                gain_db: 0.0,
                enabled: true,
                fade_in_secs: 0.0,
                fade_out_secs: 0.0,
            }));

        export_project(
            &project,
            &output_path,
            ExportPreset::Custom {
                width: 320,
                height: 240,
                fps: 30.0,
            },
        )
        .expect("export");

        assert!(output_path.is_file());
        let meta = std::fs::metadata(&output_path).unwrap();
        assert!(meta.len() > 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn timeline_duration_uses_latest_clip_end() {
        let mut project = Project::new("t", Settings::default());
        let track = crate::project::Track::new(TrackKind::Video, "V1");
        project.tracks.push(track);
        project.tracks[0]
            .clips
            .push(Clip::Video(crate::project::MediaClip {
                id: uuid::Uuid::new_v4(),
                media_id: uuid::Uuid::new_v4(),
                position_secs: 1.0,
                source_in_secs: 0.0,
                source_out_secs: 2.0,
                gain_db: 0.0,
                enabled: true,
                fade_in_secs: 0.0,
                fade_out_secs: 0.0,
            }));
        assert!((timeline_duration(&project) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn mix_timeline_audio_segment_empty_without_audio() {
        let project = Project::new("t", Settings::default());
        let bytes = mix_timeline_audio_segment(&project, 0.0, 0.5).unwrap();
        assert!(bytes.is_empty());
    }

    #[test]
    fn collect_audio_clips_skips_muted_tracks() {
        let mut project = Project::new("t", Settings::default());
        let media_id = uuid::Uuid::new_v4();
        project.media.push(crate::project::MediaItem {
            id: media_id,
            path: "voice.wav".into(),
            kind: MediaKind::Audio,
            duration_secs: Some(5.0),
            width: None,
            height: None,
            fps: None,
        });
        let mut track = crate::project::Track::new(TrackKind::Audio, "A1");
        track.muted = true;
        track.clips.push(Clip::Audio(crate::project::MediaClip {
            id: uuid::Uuid::new_v4(),
            media_id,
            position_secs: 0.0,
            source_in_secs: 0.0,
            source_out_secs: 5.0,
            gain_db: 0.0,
            enabled: true,
            fade_in_secs: 0.0,
            fade_out_secs: 0.0,
        }));
        project.tracks.push(track);

        assert!(collect_audio_clips(&project).is_empty());
    }

    #[test]
    fn active_layers_and_has_video_content_skip_hidden_tracks() {
        let mut project = Project::new("t", Settings::default());
        let media_id = uuid::Uuid::new_v4();
        project.media.push(crate::project::MediaItem {
            id: media_id,
            path: "clip.mp4".into(),
            kind: MediaKind::Video,
            duration_secs: Some(5.0),
            width: Some(320),
            height: Some(240),
            fps: Some(30.0),
        });
        let mut track = crate::project::Track::new(TrackKind::Video, "V1");
        track.hidden = true;
        track.clips.push(Clip::Video(crate::project::MediaClip {
            id: uuid::Uuid::new_v4(),
            media_id,
            position_secs: 0.0,
            source_in_secs: 0.0,
            source_out_secs: 5.0,
            gain_db: 0.0,
            enabled: true,
            fade_in_secs: 0.0,
            fade_out_secs: 0.0,
        }));
        project.tracks.push(track);

        assert!(!has_video_content(&project));
        assert!(active_layers(&project, 0.0).unwrap().is_empty());
    }

    #[test]
    fn mix_timeline_audio_range_to_file_returns_false_without_audio() {
        let project = Project::new("t", Settings::default());
        let dir = std::env::temp_dir().join(format!("uppercut-mixtest-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.wav");
        let wrote = mix_timeline_audio_range_to_file(&project, 0.0, 0.5, &out).unwrap();
        assert!(!wrote);
        assert!(!out.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn frame_renderer_reuses_state_across_render_calls() {
        if !ffmpeg_available() {
            eprintln!("skipping frame renderer test: ffmpeg not on PATH");
            return;
        }

        let dir =
            std::env::temp_dir().join(format!("uppercut-framerender-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let video_path = dir.join("src.mp4");
        generate_test_video(&video_path);

        let mut project = Project::new("frame-renderer-test", Settings::default());
        let media_id = uuid::Uuid::new_v4();
        project.media.push(crate::project::MediaItem {
            id: media_id,
            path: video_path.clone(),
            kind: MediaKind::Video,
            duration_secs: Some(1.0),
            width: Some(320),
            height: Some(240),
            fps: Some(30.0),
        });
        let track = crate::project::Track::new(TrackKind::Video, "V1");
        project.tracks.push(track);
        project.tracks[0]
            .clips
            .push(Clip::Video(crate::project::MediaClip {
                id: uuid::Uuid::new_v4(),
                media_id,
                position_secs: 0.0,
                source_in_secs: 0.0,
                source_out_secs: 1.0,
                gain_db: 0.0,
                enabled: true,
                fade_in_secs: 0.0,
                fade_out_secs: 0.0,
            }));

        let settings = ExportSettings {
            width: 320,
            height: 240,
            fps: 30.0,
        };
        let mut renderer = FrameRenderer::new(settings, DecodeOptions::default()).expect("new");
        // Sequential monotonic renders reuse the same decoder without reopening ffmpeg.
        let frame_a = renderer.render(&project, 0.0).expect("render frame 0");
        let frame_b = renderer
            .render(&project, 1.0 / 30.0)
            .expect("render frame 1");
        assert_eq!(frame_a.len(), (320 * 240 * 4) as usize);
        assert_eq!(frame_b.len(), (320 * 240 * 4) as usize);

        std::fs::remove_dir_all(&dir).ok();
    }
}
