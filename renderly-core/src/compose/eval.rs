//! Decode-free timeline evaluation: "which layers are active at time `t`, with what source
//! times, keyframed params, and transition progress" — the parity-critical logic shared by
//! the native export/preview path (`export::FrameRenderer`) and the wasm preview compositor
//! (`renderly-wasm`). See docs/preview-webview.md P2.
//!
//! This module deliberately has **no** decode/IO dependency: it only reads `Project` state
//! (paths, keyframes, transitions) and produces [`ActiveLayer`]/[`ActiveCaption`] descriptors.
//! Callers are responsible for turning `ActiveLayer::path`/`source_time` into actual pixels
//! (FFmpeg decode natively, `<video>`/`<img>` elements in the browser) — never re-derive which
//! layers are active or their transform/transition params independently, or preview and
//! export will drift apart.

use crate::project::{
    evaluate_transform, Clip, ClipMask, ClipTransform, EffectInstance, MediaKind, Project,
    TrackKind,
};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EvalError {
    #[error("media not found: {0}")]
    MediaNotFound(uuid::Uuid),
    #[error("media {0} is not video")]
    NotVideo(uuid::Uuid),
}

/// One layer to be composited at a given time, bottom-to-top by track order (see
/// `compose::Compositor::composite`'s doc comment). Decode-free: `path`/`source_time`
/// describe *what* to decode, not decoded pixels.
#[derive(Debug, Clone)]
pub struct ActiveLayer {
    /// Decoder/element cache key (usually track id; crossfade incoming uses a derived key so
    /// two decoders/elements can be open at once during a transition).
    pub decoder_key: uuid::Uuid,
    pub path: PathBuf,
    pub source_time: f64,
    pub transform: ClipTransform,
    pub effects: Vec<EffectInstance>,
    pub transition: Option<crate::compose::LayerTransition>,
    pub mask: Option<ClipMask>,
    pub background_removal: Option<crate::project::BackgroundRemoval>,
}

#[derive(Debug, Clone)]
pub struct ActiveCaption {
    pub text: String,
    pub style_id: String,
}

/// Evaluate which video-track layers are active at `t`, with per-layer source time, keyframed
/// transform/opacity, and transition progress. Tracks are iterated in project order
/// (first = bottom); within a transition window both the outgoing and incoming clip are
/// emitted so the caller can blend them.
pub fn active_layers(project: &Project, t: f64) -> Result<Vec<ActiveLayer>, EvalError> {
    let mut layers = Vec::new();

    for track in &project.tracks {
        if track.kind != TrackKind::Video || track.hidden {
            continue;
        }

        let mut video_clips: Vec<&crate::project::MediaClip> = track
            .clips
            .iter()
            .filter_map(|c| match c {
                Clip::Video(v) if v.enabled && multicam_angle_active(project, v) => Some(v),
                _ => None,
            })
            .collect();
        video_clips.sort_by(|a, b| {
            a.position_secs
                .partial_cmp(&b.position_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Transition window: during [cut-d, cut) emit outgoing + incoming for WGSL blend.
        let mut handled = false;
        for (i, v) in video_clips.iter().enumerate() {
            let start = v.position_secs;
            let end = start + v.timeline_duration_secs();
            let Some(tr) = v.outgoing_transition.as_ref() else {
                continue;
            };
            let d = tr.duration_secs;
            if d <= 0.0 {
                continue;
            }
            let window_start = end - d;
            if t < window_start || t >= end {
                continue;
            }
            let Some(incoming) = video_clips.get(i + 1).copied() else {
                continue;
            };
            // Incoming must abut (or nearly abut) this cut for renderer-only overlap.
            if (incoming.position_secs - end).abs() > 0.05 && incoming.position_secs < end - 1e-6 {
                continue;
            }

            let u = ((t - window_start) / d).clamp(0.0, 1.0) as f32;
            let out_media = project
                .find_media(v.media_id)
                .ok_or(EvalError::MediaNotFound(v.media_id))?;
            let in_media = project
                .find_media(incoming.media_id)
                .ok_or(EvalError::MediaNotFound(incoming.media_id))?;
            if out_media.kind != MediaKind::Video && out_media.kind != MediaKind::Image {
                return Err(EvalError::NotVideo(v.media_id));
            }
            if in_media.kind != MediaKind::Video && in_media.kind != MediaKind::Image {
                return Err(EvalError::NotVideo(incoming.media_id));
            }

            let out_xf = evaluate_transform(v, t);
            let in_xf = evaluate_transform(incoming, incoming.position_secs);

            layers.push(ActiveLayer {
                decoder_key: track.id,
                path: out_media.path.clone(),
                source_time: v.source_time_at(t),
                transform: out_xf,
                effects: v.effects.clone(),
                transition: Some(crate::compose::LayerTransition {
                    kind: tr.kind,
                    progress: u,
                    is_incoming: false,
                }),
                mask: v.mask.clone(),
                background_removal: v.background_removal.clone(),
            });
            layers.push(ActiveLayer {
                // Distinct from track.id so decoders/elements keep a second stream open.
                decoder_key: uuid::Uuid::from_u128(track.id.as_u128() ^ 0xC0_FF_EE_51_u128),
                path: in_media.path.clone(),
                source_time: incoming.source_in_secs + (t - window_start) * incoming.speed_factor(),
                transform: in_xf,
                effects: incoming.effects.clone(),
                transition: Some(crate::compose::LayerTransition {
                    kind: tr.kind,
                    progress: u,
                    is_incoming: true,
                }),
                mask: incoming.mask.clone(),
                background_removal: incoming.background_removal.clone(),
            });
            handled = true;
            break;
        }
        if handled {
            continue;
        }

        for clip in &track.clips {
            let Clip::Video(v) = clip else { continue };
            if !v.enabled || !multicam_angle_active(project, v) {
                continue;
            }
            let start = v.position_secs;
            let end = start + v.timeline_duration_secs();
            if t >= start && t < end {
                let media = project
                    .find_media(v.media_id)
                    .ok_or(EvalError::MediaNotFound(v.media_id))?;
                if media.kind != MediaKind::Video && media.kind != MediaKind::Image {
                    return Err(EvalError::NotVideo(v.media_id));
                }
                layers.push(ActiveLayer {
                    decoder_key: track.id,
                    path: media.path.clone(),
                    source_time: v.source_time_at(t),
                    transform: evaluate_transform(v, t),
                    effects: v.effects.clone(),
                    transition: None,
                    mask: v.mask.clone(),
                    background_removal: v.background_removal.clone(),
                });
                break;
            }
        }
    }

    Ok(layers)
}

/// True when the clip is not in a multicam group, or is the group's active angle.
pub fn multicam_angle_active(project: &Project, clip: &crate::project::MediaClip) -> bool {
    let Some(group_id) = clip.multicam_group_id else {
        return true;
    };
    let Some(group) = project.multicam_groups.iter().find(|g| g.id == group_id) else {
        return true;
    };
    group
        .angle_clip_ids
        .get(group.active_angle)
        .copied()
        .map(|id| id == clip.id)
        .unwrap_or(false)
}

/// Evaluate which caption-track clips are active (visible, burned-in) at `t`.
pub fn active_captions(project: &Project, t: f64) -> Vec<ActiveCaption> {
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
    use crate::project::{
        Clip, ClipTransition, MediaClip, MediaItem, Project, Settings, Track, TransitionKind,
    };

    fn base_project() -> Project {
        let mut project = Project::new("eval-test", Settings::default());
        project.tracks.push(Track::new(TrackKind::Video, "V1"));
        project
    }

    fn add_media(project: &mut Project, kind: MediaKind, path: &str) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        project.media.push(MediaItem {
            id,
            path: PathBuf::from(path),
            kind,
            duration_secs: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30.0),
        });
        id
    }

    #[test]
    fn active_layers_empty_project_has_no_layers() {
        let project = base_project();
        let layers = active_layers(&project, 0.0).unwrap();
        assert!(layers.is_empty());
    }

    #[test]
    fn active_layers_picks_clip_covering_time_and_computes_source_time() {
        let mut project = base_project();
        let media_id = add_media(&mut project, MediaKind::Video, "a.mp4");
        let clip = MediaClip {
            id: uuid::Uuid::new_v4(),
            media_id,
            position_secs: 2.0,
            source_in_secs: 0.0,
            source_out_secs: 5.0,
            gain_db: 0.0,
            enabled: true,
            ..Default::default()
        };
        project.tracks[0].clips.push(Clip::Video(clip));

        let layers = active_layers(&project, 3.0).unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].path, PathBuf::from("a.mp4"));
        // source_time_at(3.0) for a clip starting at 2.0 with source_in 0.0 and speed 1x = 1.0.
        assert!((layers[0].source_time - 1.0).abs() < 1e-9);
        assert!(layers[0].transition.is_none());
    }

    #[test]
    fn active_layers_outside_clip_range_yields_nothing() {
        let mut project = base_project();
        let media_id = add_media(&mut project, MediaKind::Video, "a.mp4");
        let clip = MediaClip {
            id: uuid::Uuid::new_v4(),
            media_id,
            position_secs: 2.0,
            source_in_secs: 0.0,
            source_out_secs: 5.0,
            enabled: true,
            ..Default::default()
        };
        project.tracks[0].clips.push(Clip::Video(clip));

        assert!(active_layers(&project, 0.5).unwrap().is_empty());
        assert!(active_layers(&project, 8.0).unwrap().is_empty());
    }

    #[test]
    fn active_layers_missing_media_is_an_eval_error() {
        let mut project = base_project();
        let missing_id = uuid::Uuid::new_v4();
        let clip = MediaClip {
            id: uuid::Uuid::new_v4(),
            media_id: missing_id,
            position_secs: 0.0,
            source_in_secs: 0.0,
            source_out_secs: 5.0,
            enabled: true,
            ..Default::default()
        };
        project.tracks[0].clips.push(Clip::Video(clip));

        let err = active_layers(&project, 0.0).unwrap_err();
        assert_eq!(err, EvalError::MediaNotFound(missing_id));
    }

    #[test]
    fn active_layers_hidden_track_is_skipped() {
        let mut project = base_project();
        let media_id = add_media(&mut project, MediaKind::Video, "a.mp4");
        let clip = MediaClip {
            id: uuid::Uuid::new_v4(),
            media_id,
            position_secs: 0.0,
            source_in_secs: 0.0,
            source_out_secs: 5.0,
            enabled: true,
            ..Default::default()
        };
        project.tracks[0].clips.push(Clip::Video(clip));
        project.tracks[0].hidden = true;

        assert!(active_layers(&project, 1.0).unwrap().is_empty());
    }

    #[test]
    fn active_layers_transition_window_emits_outgoing_and_incoming() {
        let mut project = base_project();
        let media_a = add_media(&mut project, MediaKind::Video, "a.mp4");
        let media_b = add_media(&mut project, MediaKind::Video, "b.mp4");

        let clip_a = MediaClip {
            id: uuid::Uuid::new_v4(),
            media_id: media_a,
            position_secs: 0.0,
            source_in_secs: 0.0,
            source_out_secs: 4.0,
            enabled: true,
            outgoing_transition: Some(ClipTransition {
                kind: TransitionKind::Crossfade,
                duration_secs: 1.0,
            }),
            ..Default::default()
        };
        let clip_b = MediaClip {
            id: uuid::Uuid::new_v4(),
            media_id: media_b,
            position_secs: 4.0,
            source_in_secs: 0.0,
            source_out_secs: 4.0,
            enabled: true,
            ..Default::default()
        };

        project.tracks[0].clips.push(Clip::Video(clip_a));
        project.tracks[0].clips.push(Clip::Video(clip_b));

        // Mid-transition: t = 3.5 is inside [3.0, 4.0) window.
        let layers = active_layers(&project, 3.5).unwrap();
        assert_eq!(layers.len(), 2);
        assert!(layers[0].transition.as_ref().unwrap().progress > 0.0);
        assert!(!layers[0].transition.as_ref().unwrap().is_incoming);
        assert!(layers[1].transition.as_ref().unwrap().is_incoming);
    }

    #[test]
    fn active_captions_only_returns_captions_covering_time() {
        let mut project = base_project();
        project
            .tracks
            .push(Track::new(TrackKind::Caption, "Captions"));
        let track_idx = project.tracks.len() - 1;
        project.tracks[track_idx]
            .clips
            .push(Clip::Caption(crate::project::CaptionClip {
                id: uuid::Uuid::new_v4(),
                position_secs: 1.0,
                duration_secs: 2.0,
                text: "hello".into(),
                style_id: "default".into(),
            }));

        assert!(active_captions(&project, 0.5).is_empty());
        let caps = active_captions(&project, 2.0);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].text, "hello");
    }
}
