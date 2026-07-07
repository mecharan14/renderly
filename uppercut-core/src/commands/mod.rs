//! Command API v0 — matches docs/command-api.md exactly. This is the *only* sanctioned way
//! to mutate a `Project` (see AGENTS.md §0.1). GUI, CLI, and MCP all dispatch here.

use crate::audio::{synthesize_to_wav, VoiceoverProvider};
use crate::media::{self, MediaError};
use crate::project::{CaptionClip, Clip, Id, MediaClip, Project, Track, TrackAudioRole, TrackKind};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "PascalCase")]
pub enum Command {
    ImportMedia {
        path: String,
    },
    AddTrack {
        kind: TrackKind,
        name: String,
    },
    AddClip {
        track_id: Id,
        media_id: Id,
        position_secs: f64,
        source_in_secs: f64,
        source_out_secs: f64,
    },
    SplitClip {
        track_id: Id,
        clip_id: Id,
        at_secs: f64,
    },
    TrimClip {
        track_id: Id,
        clip_id: Id,
        new_source_in_secs: Option<f64>,
        new_source_out_secs: Option<f64>,
    },
    MoveClip {
        track_id: Id,
        clip_id: Id,
        new_position_secs: f64,
        new_track_id: Option<Id>,
    },
    DeleteClip {
        track_id: Id,
        clip_id: Id,
        ripple: bool,
    },
    AddCaption {
        track_id: Id,
        text: String,
        position_secs: f64,
        duration_secs: f64,
        style_id: String,
    },
    /// Update an existing caption clip (Phase 2 caption editor).
    SetCaption {
        track_id: Id,
        clip_id: Id,
        text: Option<String>,
        position_secs: Option<f64>,
        duration_secs: Option<f64>,
        style_id: Option<String>,
    },
    SetAudioGain {
        track_id: Id,
        clip_id: Id,
        gain_db: f64,
    },
    Export {
        output_path: String,
        preset: ExportPreset,
    },
    /// Run local Whisper STT on a media item and add caption clips to a caption track.
    GenerateCaptions {
        media_id: Id,
        track_id: Id,
        style_id: String,
        /// Seconds added to each segment timestamp when placing on the timeline.
        #[serde(default)]
        timeline_offset_secs: f64,
    },
    /// Synthesize narration audio (Piper local or OpenAI BYO) and place on an audio track.
    GenerateVoiceover {
        text: String,
        track_id: Id,
        position_secs: f64,
        output_path: String,
        provider: VoiceoverProvider,
    },
    /// Set fade-in/out on an audio clip (applied during export).
    SetAudioFade {
        track_id: Id,
        clip_id: Id,
        fade_in_secs: f64,
        fade_out_secs: f64,
    },
    /// Assign mix role on an audio track (voiceover/dialog/music/ambience) for ducking.
    SetTrackAudioRole {
        track_id: Id,
        role: Option<TrackAudioRole>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportPreset {
    TikTok9x16,
    Youtube16x9,
    Custom { width: u32, height: u32, fps: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandOutcome {
    MediaImported { media_id: Id },
    TrackAdded { track_id: Id },
    ClipAdded { clip_id: Id },
    ClipSplit { left_id: Id, right_id: Id },
    CaptionsGenerated { count: usize },
    VoiceoverGenerated { media_id: Id, clip_id: Id },
    Applied,
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("track not found: {0}")]
    TrackNotFound(Id),
    #[error("media not found: {0}")]
    MediaNotFound(Id),
    #[error("clip not found: {0} on track {1}")]
    ClipNotFound(Id, Id),
    #[error("track {0} is kind {1:?}, expected {2:?}")]
    TrackKindMismatch(Id, TrackKind, TrackKind),
    #[error("clip range [{0}, {1}) overlaps an existing clip on track {2}")]
    Overlap(f64, f64, Id),
    #[error("invalid range: source_out_secs ({0}) <= source_in_secs ({1})")]
    InvalidRange(f64, f64),
    #[error("source range exceeds media duration ({0}s)")]
    ExceedsMediaDuration(f64),
    #[error("split point {0} is not strictly inside the clip's span")]
    SplitOutOfBounds(f64),
    #[error("TrimClip requires at least one of new_source_in_secs/new_source_out_secs")]
    TrimRequiresChange,
    #[error("SetCaption requires at least one field to change")]
    SetCaptionRequiresChange,
    #[error("clip has no audio: {0}")]
    NoAudio(Id),
    #[error("{0}")]
    Media(#[from] MediaError),
    #[error("{0}")]
    Export(#[from] crate::export::ExportError),
    #[error("{0}")]
    Perceive(#[from] crate::perceive::PerceiveError),
    #[error("{0}")]
    Tts(#[from] crate::audio::TtsError),
    #[error("invalid fade: fade durations must be >= 0")]
    InvalidFade,
    #[error("not yet implemented: {0}")]
    NotImplemented(&'static str),
}

pub fn apply_command(project: &mut Project, cmd: Command) -> Result<CommandOutcome, CommandError> {
    match cmd {
        Command::ImportMedia { path } => import_media(project, &path),
        Command::AddTrack { kind, name } => add_track(project, kind, name),
        Command::AddClip {
            track_id,
            media_id,
            position_secs,
            source_in_secs,
            source_out_secs,
        } => add_clip(
            project,
            track_id,
            media_id,
            position_secs,
            source_in_secs,
            source_out_secs,
        ),
        Command::SplitClip {
            track_id,
            clip_id,
            at_secs,
        } => split_clip(project, track_id, clip_id, at_secs),
        Command::TrimClip {
            track_id,
            clip_id,
            new_source_in_secs,
            new_source_out_secs,
        } => trim_clip(
            project,
            track_id,
            clip_id,
            new_source_in_secs,
            new_source_out_secs,
        ),
        Command::MoveClip {
            track_id,
            clip_id,
            new_position_secs,
            new_track_id,
        } => move_clip(project, track_id, clip_id, new_position_secs, new_track_id),
        Command::DeleteClip {
            track_id,
            clip_id,
            ripple,
        } => delete_clip(project, track_id, clip_id, ripple),
        Command::AddCaption {
            track_id,
            text,
            position_secs,
            duration_secs,
            style_id,
        } => add_caption(
            project,
            track_id,
            text,
            position_secs,
            duration_secs,
            style_id,
        ),
        Command::SetCaption {
            track_id,
            clip_id,
            text,
            position_secs,
            duration_secs,
            style_id,
        } => set_caption(
            project,
            track_id,
            clip_id,
            text,
            position_secs,
            duration_secs,
            style_id,
        ),
        Command::SetAudioGain {
            track_id,
            clip_id,
            gain_db,
        } => set_audio_gain(project, track_id, clip_id, gain_db),
        Command::Export {
            output_path,
            preset,
        } => export_project_cmd(project, &output_path, preset),
        Command::GenerateCaptions {
            media_id,
            track_id,
            style_id,
            timeline_offset_secs,
        } => generate_captions(project, media_id, track_id, style_id, timeline_offset_secs),
        Command::GenerateVoiceover {
            text,
            track_id,
            position_secs,
            output_path,
            provider,
        } => generate_voiceover(
            project,
            &text,
            track_id,
            position_secs,
            &output_path,
            provider,
        ),
        Command::SetAudioFade {
            track_id,
            clip_id,
            fade_in_secs,
            fade_out_secs,
        } => set_audio_fade(project, track_id, clip_id, fade_in_secs, fade_out_secs),
        Command::SetTrackAudioRole { track_id, role } => {
            set_track_audio_role(project, track_id, role)
        }
    }
}

fn import_media(project: &mut Project, path: &str) -> Result<CommandOutcome, CommandError> {
    use crate::project::MediaItem;
    use std::path::PathBuf;

    let path_buf = PathBuf::from(path);
    let probed = media::probe(&path_buf)?;
    let media_id = Id::new_v4();
    project.media.push(MediaItem {
        id: media_id,
        path: path_buf,
        kind: probed.kind.expect("probe() always sets kind on success"),
        duration_secs: probed.duration_secs,
        width: probed.width,
        height: probed.height,
        fps: probed.fps,
    });
    Ok(CommandOutcome::MediaImported { media_id })
}

fn add_track(
    project: &mut Project,
    kind: TrackKind,
    name: String,
) -> Result<CommandOutcome, CommandError> {
    let track = Track::new(kind, name);
    let track_id = track.id;
    project.tracks.push(track);
    Ok(CommandOutcome::TrackAdded { track_id })
}

fn clip_kind_matches(track_kind: TrackKind, clip: &Clip) -> bool {
    matches!(
        (track_kind, clip),
        (TrackKind::Video, Clip::Video(_))
            | (TrackKind::Audio, Clip::Audio(_))
            | (TrackKind::Caption, Clip::Caption(_))
    )
}

fn clip_track_kind(clip: &Clip) -> TrackKind {
    match clip {
        Clip::Video(_) => TrackKind::Video,
        Clip::Audio(_) => TrackKind::Audio,
        Clip::Caption(_) => TrackKind::Caption,
    }
}

fn check_no_overlap(
    track: &Track,
    position_secs: f64,
    duration_secs: f64,
    ignore_clip: Option<Id>,
) -> Result<(), CommandError> {
    let new_end = position_secs + duration_secs;
    for clip in &track.clips {
        if Some(clip.id()) == ignore_clip {
            continue;
        }
        let existing_start = clip.position_secs();
        let existing_end = clip.end_secs();
        if position_secs < existing_end && existing_start < new_end {
            return Err(CommandError::Overlap(position_secs, new_end, track.id));
        }
    }
    Ok(())
}

fn add_clip(
    project: &mut Project,
    track_id: Id,
    media_id: Id,
    position_secs: f64,
    source_in_secs: f64,
    source_out_secs: f64,
) -> Result<CommandOutcome, CommandError> {
    if source_out_secs <= source_in_secs {
        return Err(CommandError::InvalidRange(source_out_secs, source_in_secs));
    }

    let media_kind = project
        .find_media(media_id)
        .ok_or(CommandError::MediaNotFound(media_id))?
        .kind;
    let media_duration = project.find_media(media_id).unwrap().duration_secs;

    if let Some(duration) = media_duration {
        if source_out_secs > duration {
            return Err(CommandError::ExceedsMediaDuration(duration));
        }
    }

    let track = project
        .find_track_mut(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;

    let expected_track_kind = match media_kind {
        crate::project::MediaKind::Video => TrackKind::Video,
        crate::project::MediaKind::Audio => TrackKind::Audio,
        crate::project::MediaKind::Image => TrackKind::Video,
    };
    if track.kind != expected_track_kind {
        return Err(CommandError::TrackKindMismatch(
            track_id,
            track.kind,
            expected_track_kind,
        ));
    }

    let duration_secs = source_out_secs - source_in_secs;
    check_no_overlap(track, position_secs, duration_secs, None)?;

    let clip_id = Id::new_v4();
    let media_clip = MediaClip {
        id: clip_id,
        media_id,
        position_secs,
        source_in_secs,
        source_out_secs,
        gain_db: 0.0,
        enabled: true,
        fade_in_secs: 0.0,
        fade_out_secs: 0.0,
    };
    let clip = match track.kind {
        TrackKind::Video => Clip::Video(media_clip),
        TrackKind::Audio => Clip::Audio(media_clip),
        TrackKind::Caption => unreachable!("caption tracks rejected above"),
    };
    track.clips.push(clip);

    Ok(CommandOutcome::ClipAdded { clip_id })
}

fn split_clip(
    project: &mut Project,
    track_id: Id,
    clip_id: Id,
    at_secs: f64,
) -> Result<CommandOutcome, CommandError> {
    let track = project
        .find_track_mut(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;

    let idx = track
        .clips
        .iter()
        .position(|c| c.id() == clip_id)
        .ok_or(CommandError::ClipNotFound(clip_id, track_id))?;

    let original = track.clips[idx].clone();
    let start = original.position_secs();
    let end = original.end_secs();
    if at_secs <= start || at_secs >= end {
        return Err(CommandError::SplitOutOfBounds(at_secs));
    }

    let split_offset = at_secs - start;
    let right_id = Id::new_v4();

    let (left, right) = match original {
        Clip::Video(mut m) => {
            let mut right_m = m.clone();
            right_m.id = right_id;
            m.source_out_secs = m.source_in_secs + split_offset;
            right_m.position_secs = at_secs;
            right_m.source_in_secs = m.source_out_secs;
            (Clip::Video(m), Clip::Video(right_m))
        }
        Clip::Audio(mut m) => {
            let mut right_m = m.clone();
            right_m.id = right_id;
            m.source_out_secs = m.source_in_secs + split_offset;
            right_m.position_secs = at_secs;
            right_m.source_in_secs = m.source_out_secs;
            (Clip::Audio(m), Clip::Audio(right_m))
        }
        Clip::Caption(mut c) => {
            let mut right_c = c.clone();
            right_c.id = right_id;
            c.duration_secs = split_offset;
            right_c.position_secs = at_secs;
            right_c.duration_secs = end - at_secs;
            (Clip::Caption(c), Clip::Caption(right_c))
        }
    };

    let left_id = left.id();
    track.clips[idx] = left;
    track.clips.push(right);

    Ok(CommandOutcome::ClipSplit { left_id, right_id })
}

fn trim_clip(
    project: &mut Project,
    track_id: Id,
    clip_id: Id,
    new_source_in_secs: Option<f64>,
    new_source_out_secs: Option<f64>,
) -> Result<CommandOutcome, CommandError> {
    if new_source_in_secs.is_none() && new_source_out_secs.is_none() {
        return Err(CommandError::TrimRequiresChange);
    }

    let track = project
        .find_track_mut(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;
    let clip = track
        .clips
        .iter_mut()
        .find(|c| c.id() == clip_id)
        .ok_or(CommandError::ClipNotFound(clip_id, track_id))?;

    let media_clip = match clip {
        Clip::Video(m) | Clip::Audio(m) => m,
        Clip::Caption(_) => return Err(CommandError::TrimRequiresChange),
    };

    let new_in = new_source_in_secs.unwrap_or(media_clip.source_in_secs);
    let new_out = new_source_out_secs.unwrap_or(media_clip.source_out_secs);
    if new_out <= new_in {
        return Err(CommandError::InvalidRange(new_out, new_in));
    }

    media_clip.source_in_secs = new_in;
    media_clip.source_out_secs = new_out;

    Ok(CommandOutcome::Applied)
}

fn move_clip(
    project: &mut Project,
    track_id: Id,
    clip_id: Id,
    new_position_secs: f64,
    new_track_id: Option<Id>,
) -> Result<CommandOutcome, CommandError> {
    let dest_track_id = new_track_id.unwrap_or(track_id);

    let src_track = project
        .find_track(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;
    let clip = src_track
        .find_clip(clip_id)
        .ok_or(CommandError::ClipNotFound(clip_id, track_id))?
        .clone();

    if dest_track_id != track_id {
        let dest_track = project
            .find_track(dest_track_id)
            .ok_or(CommandError::TrackNotFound(dest_track_id))?;
        if !clip_kind_matches(dest_track.kind, &clip) {
            return Err(CommandError::TrackKindMismatch(
                dest_track_id,
                dest_track.kind,
                clip_track_kind(&clip),
            ));
        }
    }

    let duration = clip.duration_secs();
    {
        let dest_track = project.find_track(dest_track_id).unwrap();
        let ignore = if dest_track_id == track_id {
            Some(clip_id)
        } else {
            None
        };
        check_no_overlap(dest_track, new_position_secs, duration, ignore)?;
    }

    // Remove from source, update position, insert into destination.
    let src_track = project.find_track_mut(track_id).unwrap();
    let idx = src_track
        .clips
        .iter()
        .position(|c| c.id() == clip_id)
        .unwrap();
    let mut moved = src_track.clips.remove(idx);
    match &mut moved {
        Clip::Video(m) | Clip::Audio(m) => m.position_secs = new_position_secs,
        Clip::Caption(c) => c.position_secs = new_position_secs,
    }

    let dest_track = project.find_track_mut(dest_track_id).unwrap();
    dest_track.clips.push(moved);

    Ok(CommandOutcome::Applied)
}

fn delete_clip(
    project: &mut Project,
    track_id: Id,
    clip_id: Id,
    ripple: bool,
) -> Result<CommandOutcome, CommandError> {
    let track = project
        .find_track_mut(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;

    let idx = track
        .clips
        .iter()
        .position(|c| c.id() == clip_id)
        .ok_or(CommandError::ClipNotFound(clip_id, track_id))?;

    let removed = track.clips.remove(idx);

    if ripple {
        let gap_start = removed.position_secs();
        let gap = removed.duration_secs();
        for clip in track.clips.iter_mut() {
            if clip.position_secs() >= gap_start {
                match clip {
                    Clip::Video(m) | Clip::Audio(m) => m.position_secs -= gap,
                    Clip::Caption(c) => c.position_secs -= gap,
                }
            }
        }
    }

    Ok(CommandOutcome::Applied)
}

fn add_caption(
    project: &mut Project,
    track_id: Id,
    text: String,
    position_secs: f64,
    duration_secs: f64,
    style_id: String,
) -> Result<CommandOutcome, CommandError> {
    let track = project
        .find_track_mut(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;

    if track.kind != TrackKind::Caption {
        return Err(CommandError::TrackKindMismatch(
            track_id,
            track.kind,
            TrackKind::Caption,
        ));
    }

    check_no_overlap(track, position_secs, duration_secs, None)?;

    let clip_id = Id::new_v4();
    track.clips.push(Clip::Caption(CaptionClip {
        id: clip_id,
        text,
        position_secs,
        duration_secs,
        style_id,
    }));

    Ok(CommandOutcome::ClipAdded { clip_id })
}

fn set_caption(
    project: &mut Project,
    track_id: Id,
    clip_id: Id,
    text: Option<String>,
    position_secs: Option<f64>,
    duration_secs: Option<f64>,
    style_id: Option<String>,
) -> Result<CommandOutcome, CommandError> {
    if text.is_none() && position_secs.is_none() && duration_secs.is_none() && style_id.is_none() {
        return Err(CommandError::SetCaptionRequiresChange);
    }

    let track = project
        .find_track(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;
    if track.kind != TrackKind::Caption {
        return Err(CommandError::TrackKindMismatch(
            track_id,
            track.kind,
            TrackKind::Caption,
        ));
    }

    // Resolve the existing clip first: if `clip_id` doesn't exist, report ClipNotFound
    // rather than silently defaulting position/duration to 0.0/0.1 and risking a
    // misleading Overlap error instead.
    let existing = track
        .find_clip(clip_id)
        .ok_or(CommandError::ClipNotFound(clip_id, track_id))?;
    let new_position = position_secs.unwrap_or_else(|| existing.position_secs());
    let new_duration = duration_secs.unwrap_or_else(|| existing.duration_secs());

    check_no_overlap(track, new_position, new_duration, Some(clip_id))?;

    let track = project
        .find_track_mut(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;
    let clip = track
        .clips
        .iter_mut()
        .find(|c| c.id() == clip_id)
        .ok_or(CommandError::ClipNotFound(clip_id, track_id))?;

    match clip {
        Clip::Caption(c) => {
            if let Some(t) = text {
                c.text = t;
            }
            if let Some(p) = position_secs {
                c.position_secs = p;
            }
            if let Some(d) = duration_secs {
                c.duration_secs = d;
            }
            if let Some(s) = style_id {
                c.style_id = s;
            }
            Ok(CommandOutcome::Applied)
        }
        _ => Err(CommandError::ClipNotFound(clip_id, track_id)),
    }
}

fn export_project_cmd(
    _project: &mut Project,
    output_path: &str,
    preset: ExportPreset,
) -> Result<CommandOutcome, CommandError> {
    use crate::export::export_project;
    use std::path::Path;

    export_project(_project, Path::new(output_path), preset)?;
    Ok(CommandOutcome::Applied)
}

fn generate_captions(
    project: &mut Project,
    media_id: Id,
    track_id: Id,
    style_id: String,
    timeline_offset_secs: f64,
) -> Result<CommandOutcome, CommandError> {
    use crate::captions::BUILTIN_STYLES;
    use crate::perceive::transcribe_media;

    if !BUILTIN_STYLES.contains(&style_id.as_str()) {
        // Allow any style id but warn via default fallback in renderer — still accept custom ids.
    }

    let track = project
        .find_track(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;
    if track.kind != TrackKind::Caption {
        return Err(CommandError::TrackKindMismatch(
            track_id,
            track.kind,
            TrackKind::Caption,
        ));
    }

    let transcript = transcribe_media(project, media_id)?;
    let mut count = 0usize;

    for seg in transcript.segments {
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }
        let duration = (seg.end_secs - seg.start_secs).max(0.1);
        let position = timeline_offset_secs + seg.start_secs;
        // Skip segments that fail to place (e.g. two whisper segments landing on
        // overlapping timestamps) instead of aborting the whole batch via `?` — an error
        // partway through would otherwise still leave the already-added captions mutated
        // into `project` while reporting failure, silently losing the rest of the
        // transcript. Best-effort captioning of everything that fits is more useful to an
        // agent than an all-or-nothing batch.
        if add_caption(
            project,
            track_id,
            text.to_string(),
            position,
            duration,
            style_id.clone(),
        )
        .is_ok()
        {
            count += 1;
        }
    }

    Ok(CommandOutcome::CaptionsGenerated { count })
}

fn set_audio_gain(
    project: &mut Project,
    track_id: Id,
    clip_id: Id,
    gain_db: f64,
) -> Result<CommandOutcome, CommandError> {
    let track = project
        .find_track_mut(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;
    let clip = track
        .clips
        .iter_mut()
        .find(|c| c.id() == clip_id)
        .ok_or(CommandError::ClipNotFound(clip_id, track_id))?;

    match clip {
        Clip::Audio(m) => {
            m.gain_db = gain_db;
            Ok(CommandOutcome::Applied)
        }
        _ => Err(CommandError::NoAudio(clip_id)),
    }
}

fn set_audio_fade(
    project: &mut Project,
    track_id: Id,
    clip_id: Id,
    fade_in_secs: f64,
    fade_out_secs: f64,
) -> Result<CommandOutcome, CommandError> {
    if fade_in_secs < 0.0 || fade_out_secs < 0.0 {
        return Err(CommandError::InvalidFade);
    }

    let track = project
        .find_track_mut(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;
    let clip = track
        .clips
        .iter_mut()
        .find(|c| c.id() == clip_id)
        .ok_or(CommandError::ClipNotFound(clip_id, track_id))?;

    match clip {
        Clip::Audio(m) => {
            m.fade_in_secs = fade_in_secs;
            m.fade_out_secs = fade_out_secs;
            Ok(CommandOutcome::Applied)
        }
        _ => Err(CommandError::NoAudio(clip_id)),
    }
}

fn set_track_audio_role(
    project: &mut Project,
    track_id: Id,
    role: Option<TrackAudioRole>,
) -> Result<CommandOutcome, CommandError> {
    let track = project
        .find_track_mut(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;
    if track.kind != TrackKind::Audio {
        return Err(CommandError::TrackKindMismatch(
            track_id,
            track.kind,
            TrackKind::Audio,
        ));
    }
    track.audio_role = role;
    Ok(CommandOutcome::Applied)
}

fn generate_voiceover(
    project: &mut Project,
    text: &str,
    track_id: Id,
    position_secs: f64,
    output_path: &str,
    provider: VoiceoverProvider,
) -> Result<CommandOutcome, CommandError> {
    use crate::project::MediaItem;
    use std::path::PathBuf;

    let track = project
        .find_track(track_id)
        .ok_or(CommandError::TrackNotFound(track_id))?;
    if track.kind != TrackKind::Audio {
        return Err(CommandError::TrackKindMismatch(
            track_id,
            track.kind,
            TrackKind::Audio,
        ));
    }

    let path_buf = PathBuf::from(output_path);
    if let Some(parent) = path_buf.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(MediaError::Io)?;
        }
    }

    synthesize_to_wav(text, &provider, &path_buf)?;

    let probed = media::probe(&path_buf)?;
    let duration = probed.duration_secs.unwrap_or(0.1).max(0.1);
    let media_id = Id::new_v4();
    project.media.push(MediaItem {
        id: media_id,
        path: path_buf,
        kind: probed.kind.unwrap_or(crate::project::MediaKind::Audio),
        duration_secs: Some(duration),
        width: probed.width,
        height: probed.height,
        fps: probed.fps,
    });

    let outcome = add_clip(project, track_id, media_id, position_secs, 0.0, duration)?;
    let clip_id = match outcome {
        CommandOutcome::ClipAdded { clip_id } => clip_id,
        _ => unreachable!(),
    };

    Ok(CommandOutcome::VoiceoverGenerated { media_id, clip_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, Settings};
    use std::io::Write;

    fn test_project() -> Project {
        Project::new("test", Settings::default())
    }

    fn write_temp_wav(dir: &std::path::Path, name: &str, duration_secs: f64) -> std::path::PathBuf {
        let sample_rate = 48000u32;
        let byte_rate = sample_rate * 2; // 16-bit mono
        let data_size = (byte_rate as f64 * duration_secs) as u32;
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_size).to_le_bytes()).unwrap();
        f.write_all(b"WAVE").unwrap();
        f.write_all(b"fmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        f.write_all(&1u16.to_le_bytes()).unwrap(); // mono
        f.write_all(&sample_rate.to_le_bytes()).unwrap();
        f.write_all(&byte_rate.to_le_bytes()).unwrap();
        f.write_all(&2u16.to_le_bytes()).unwrap(); // block align
        f.write_all(&16u16.to_le_bytes()).unwrap(); // bits per sample
        f.write_all(b"data").unwrap();
        f.write_all(&data_size.to_le_bytes()).unwrap();
        f.write_all(&vec![0u8; data_size as usize]).unwrap();
        path
    }

    #[test]
    fn import_media_wav_probes_duration() {
        let dir = std::env::temp_dir().join(format!("uppercut-test-{}", Id::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav_path = write_temp_wav(&dir, "clip.wav", 2.0);

        let mut project = test_project();
        let outcome = apply_command(
            &mut project,
            Command::ImportMedia {
                path: wav_path.to_string_lossy().to_string(),
            },
        )
        .unwrap();

        let CommandOutcome::MediaImported { media_id } = outcome else {
            panic!("expected MediaImported");
        };
        let media = project.find_media(media_id).unwrap();
        assert_eq!(media.kind, crate::project::MediaKind::Audio);
        assert!((media.duration_secs.unwrap() - 2.0).abs() < 0.01);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_clip_rejects_overlap() {
        let dir = std::env::temp_dir().join(format!("uppercut-test-{}", Id::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav_path = write_temp_wav(&dir, "clip.wav", 10.0);

        let mut project = test_project();
        let media_id = match apply_command(
            &mut project,
            Command::ImportMedia {
                path: wav_path.to_string_lossy().to_string(),
            },
        )
        .unwrap()
        {
            CommandOutcome::MediaImported { media_id } => media_id,
            _ => unreachable!(),
        };
        let track_id = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Audio,
                name: "A1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };

        apply_command(
            &mut project,
            Command::AddClip {
                track_id,
                media_id,
                position_secs: 0.0,
                source_in_secs: 0.0,
                source_out_secs: 5.0,
            },
        )
        .unwrap();

        let err = apply_command(
            &mut project,
            Command::AddClip {
                track_id,
                media_id,
                position_secs: 2.0,
                source_in_secs: 0.0,
                source_out_secs: 3.0,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::Overlap(_, _, _)));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn split_clip_produces_two_contiguous_clips() {
        let dir = std::env::temp_dir().join(format!("uppercut-test-{}", Id::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav_path = write_temp_wav(&dir, "clip.wav", 10.0);

        let mut project = test_project();
        let media_id = match apply_command(
            &mut project,
            Command::ImportMedia {
                path: wav_path.to_string_lossy().to_string(),
            },
        )
        .unwrap()
        {
            CommandOutcome::MediaImported { media_id } => media_id,
            _ => unreachable!(),
        };
        let track_id = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Audio,
                name: "A1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };
        let clip_id = match apply_command(
            &mut project,
            Command::AddClip {
                track_id,
                media_id,
                position_secs: 0.0,
                source_in_secs: 0.0,
                source_out_secs: 6.0,
            },
        )
        .unwrap()
        {
            CommandOutcome::ClipAdded { clip_id } => clip_id,
            _ => unreachable!(),
        };

        let outcome = apply_command(
            &mut project,
            Command::SplitClip {
                track_id,
                clip_id,
                at_secs: 2.0,
            },
        )
        .unwrap();
        let CommandOutcome::ClipSplit { left_id, right_id } = outcome else {
            panic!()
        };

        let track = project.find_track(track_id).unwrap();
        let left = track.find_clip(left_id).unwrap();
        let right = track.find_clip(right_id).unwrap();
        assert!((left.duration_secs() - 2.0).abs() < 1e-9);
        assert!((right.duration_secs() - 4.0).abs() < 1e-9);
        assert!((left.end_secs() - right.position_secs()).abs() < 1e-9);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_clip_with_ripple_shifts_later_clips() {
        let dir = std::env::temp_dir().join(format!("uppercut-test-{}", Id::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav_path = write_temp_wav(&dir, "clip.wav", 10.0);

        let mut project = test_project();
        let media_id = match apply_command(
            &mut project,
            Command::ImportMedia {
                path: wav_path.to_string_lossy().to_string(),
            },
        )
        .unwrap()
        {
            CommandOutcome::MediaImported { media_id } => media_id,
            _ => unreachable!(),
        };
        let track_id = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Audio,
                name: "A1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };
        let first = match apply_command(
            &mut project,
            Command::AddClip {
                track_id,
                media_id,
                position_secs: 0.0,
                source_in_secs: 0.0,
                source_out_secs: 2.0,
            },
        )
        .unwrap()
        {
            CommandOutcome::ClipAdded { clip_id } => clip_id,
            _ => unreachable!(),
        };
        let second = match apply_command(
            &mut project,
            Command::AddClip {
                track_id,
                media_id,
                position_secs: 2.0,
                source_in_secs: 0.0,
                source_out_secs: 3.0,
            },
        )
        .unwrap()
        {
            CommandOutcome::ClipAdded { clip_id } => clip_id,
            _ => unreachable!(),
        };

        apply_command(
            &mut project,
            Command::DeleteClip {
                track_id,
                clip_id: first,
                ripple: true,
            },
        )
        .unwrap();

        let track = project.find_track(track_id).unwrap();
        let remaining = track.find_clip(second).unwrap();
        assert!((remaining.position_secs() - 0.0).abs() < 1e-9);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_caption_requires_caption_track() {
        let mut project = test_project();
        let track_id = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Video,
                name: "V1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };

        let err = apply_command(
            &mut project,
            Command::AddCaption {
                track_id,
                text: "hi".into(),
                position_secs: 0.0,
                duration_secs: 1.0,
                style_id: "default".into(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, CommandError::TrackKindMismatch(_, _, _)));
    }

    #[test]
    fn set_caption_updates_text_and_timing() {
        let mut project = test_project();
        let track_id = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Caption,
                name: "C1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };

        let clip_id = match apply_command(
            &mut project,
            Command::AddCaption {
                track_id,
                text: "hello".into(),
                position_secs: 1.0,
                duration_secs: 2.0,
                style_id: "tiktok-bold-yellow".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::ClipAdded { clip_id } => clip_id,
            _ => unreachable!(),
        };

        apply_command(
            &mut project,
            Command::SetCaption {
                track_id,
                clip_id,
                text: Some("updated".into()),
                position_secs: Some(1.5),
                duration_secs: Some(2.5),
                style_id: None,
            },
        )
        .unwrap();

        let clip = project
            .find_track(track_id)
            .unwrap()
            .find_clip(clip_id)
            .unwrap();
        match clip {
            Clip::Caption(c) => {
                assert_eq!(c.text, "updated");
                assert!((c.position_secs - 1.5).abs() < 1e-9);
                assert!((c.duration_secs - 2.5).abs() < 1e-9);
                assert_eq!(c.style_id, "tiktok-bold-yellow");
            }
            _ => panic!("expected caption clip"),
        }

        let err = apply_command(
            &mut project,
            Command::SetCaption {
                track_id,
                clip_id,
                text: None,
                position_secs: None,
                duration_secs: None,
                style_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::SetCaptionRequiresChange));
    }

    #[test]
    fn set_caption_reports_clip_not_found_not_overlap() {
        // A bogus clip_id used to fall through to default position/duration (0.0/0.1)
        // before failing, which could spuriously report Overlap instead of ClipNotFound
        // if those defaults happened to collide with an existing caption.
        let mut project = test_project();
        let track_id = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Caption,
                name: "C1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };
        apply_command(
            &mut project,
            Command::AddCaption {
                track_id,
                text: "hello".into(),
                position_secs: 0.0,
                duration_secs: 0.1,
                style_id: "tiktok-bold-yellow".into(),
            },
        )
        .unwrap();

        let err = apply_command(
            &mut project,
            Command::SetCaption {
                track_id,
                clip_id: Id::new_v4(),
                text: Some("nope".into()),
                position_secs: None,
                duration_secs: None,
                style_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::ClipNotFound(_, _)));
    }

    #[test]
    fn move_clip_kind_mismatch_reports_clip_kind_not_dest_kind_twice() {
        let mut project = test_project();
        let video_track = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Video,
                name: "V1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };
        let caption_track = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Caption,
                name: "C1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };
        let clip_id = match apply_command(
            &mut project,
            Command::AddCaption {
                track_id: caption_track,
                text: "hi".into(),
                position_secs: 0.0,
                duration_secs: 1.0,
                style_id: "tiktok-bold-yellow".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::ClipAdded { clip_id } => clip_id,
            _ => unreachable!(),
        };

        let err = apply_command(
            &mut project,
            Command::MoveClip {
                track_id: caption_track,
                clip_id,
                new_position_secs: 0.0,
                new_track_id: Some(video_track),
            },
        )
        .unwrap_err();
        match err {
            CommandError::TrackKindMismatch(_, dest_kind, expected_kind) => {
                assert_eq!(dest_kind, TrackKind::Video);
                assert_eq!(expected_kind, TrackKind::Caption);
            }
            other => panic!("expected TrackKindMismatch, got {other:?}"),
        }
    }

    #[test]
    fn generate_captions_requires_caption_track() {
        let mut project = test_project();
        let media_id = uuid::Uuid::new_v4();
        project.media.push(crate::project::MediaItem {
            id: media_id,
            path: "x.wav".into(),
            kind: crate::project::MediaKind::Audio,
            duration_secs: Some(1.0),
            width: None,
            height: None,
            fps: None,
        });
        let track_id = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Video,
                name: "V1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };

        let err = apply_command(
            &mut project,
            Command::GenerateCaptions {
                media_id,
                track_id,
                style_id: "tiktok-bold-yellow".into(),
                timeline_offset_secs: 0.0,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::TrackKindMismatch(_, _, _)));
    }

    #[test]
    fn set_audio_fade_on_audio_clip() {
        let mut project = test_project();
        let media_id = uuid::Uuid::new_v4();
        project.media.push(crate::project::MediaItem {
            id: media_id,
            path: "voice.wav".into(),
            kind: crate::project::MediaKind::Audio,
            duration_secs: Some(5.0),
            width: None,
            height: None,
            fps: None,
        });
        let track_id = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Audio,
                name: "A1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };
        let clip_id = match apply_command(
            &mut project,
            Command::AddClip {
                track_id,
                media_id,
                position_secs: 0.0,
                source_in_secs: 0.0,
                source_out_secs: 5.0,
            },
        )
        .unwrap()
        {
            CommandOutcome::ClipAdded { clip_id } => clip_id,
            _ => unreachable!(),
        };

        apply_command(
            &mut project,
            Command::SetAudioFade {
                track_id,
                clip_id,
                fade_in_secs: 0.5,
                fade_out_secs: 1.0,
            },
        )
        .unwrap();

        let clip = project
            .find_track(track_id)
            .unwrap()
            .find_clip(clip_id)
            .unwrap();
        match clip {
            Clip::Audio(c) => {
                assert!((c.fade_in_secs - 0.5).abs() < 1e-9);
                assert!((c.fade_out_secs - 1.0).abs() < 1e-9);
            }
            _ => panic!("expected audio clip"),
        }
    }

    #[test]
    fn set_track_audio_role_requires_audio_track() {
        let mut project = test_project();
        let track_id = match apply_command(
            &mut project,
            Command::AddTrack {
                kind: TrackKind::Video,
                name: "V1".into(),
            },
        )
        .unwrap()
        {
            CommandOutcome::TrackAdded { track_id } => track_id,
            _ => unreachable!(),
        };

        let err = apply_command(
            &mut project,
            Command::SetTrackAudioRole {
                track_id,
                role: Some(crate::project::TrackAudioRole::Music),
            },
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::TrackKindMismatch(_, _, _)));
    }

    #[test]
    fn export_requires_ffmpeg_or_empty_timeline() {
        let mut project = test_project();
        let err = apply_command(
            &mut project,
            Command::Export {
                output_path: "out.mp4".into(),
                preset: ExportPreset::TikTok9x16,
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, CommandError::Export(_))
                || matches!(err, CommandError::NotImplemented(_)),
            "unexpected: {err:?}"
        );
    }
}
