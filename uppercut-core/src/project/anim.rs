//! Keyframe evaluation for Phase 3.1 — interpolates clip transform / volume at a
//! timeline time. Effects are not evaluated here (store-only until 3.2).

use super::{AnimProperty, ClipTransform, Easing, KeyframeTrack, MediaClip};

/// Evaluate the effective transform at `timeline_secs` (absolute on the project timeline).
pub fn evaluate_transform(clip: &MediaClip, timeline_secs: f64) -> ClipTransform {
    let local = (timeline_secs - clip.position_secs).max(0.0);
    let mut t = clip.transform.clamp_opacity();
    for track in &clip.keyframes {
        let Some(v) = sample_track(track, local) else {
            continue;
        };
        match track.property {
            AnimProperty::PosX => t.x = v,
            AnimProperty::PosY => t.y = v,
            AnimProperty::ScaleX => t.scale_x = v,
            AnimProperty::ScaleY => t.scale_y = v,
            AnimProperty::Rotation => t.rotation_deg = v,
            AnimProperty::Opacity => t.opacity = v.clamp(0.0, 1.0),
            AnimProperty::Volume => {}
        }
    }
    t
}

/// Effective gain in dB: base `gain_db` plus optional `Volume` keyframe track
/// (keyframe values are absolute dB when present, replacing the static gain).
pub fn evaluate_volume_db(clip: &MediaClip, timeline_secs: f64) -> f64 {
    let local = (timeline_secs - clip.position_secs).max(0.0);
    for track in &clip.keyframes {
        if track.property == AnimProperty::Volume {
            if let Some(v) = sample_track(track, local) {
                return v;
            }
        }
    }
    clip.gain_db
}

fn sample_track(track: &KeyframeTrack, local_secs: f64) -> Option<f64> {
    if track.keys.is_empty() {
        return None;
    }
    let mut keys = track.keys.clone();
    keys.sort_by(|a, b| {
        a.time_secs
            .partial_cmp(&b.time_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if local_secs <= keys[0].time_secs {
        return Some(keys[0].value);
    }
    if local_secs >= keys[keys.len() - 1].time_secs {
        return Some(keys[keys.len() - 1].value);
    }

    for i in 0..keys.len() - 1 {
        let a = &keys[i];
        let b = &keys[i + 1];
        if local_secs >= a.time_secs && local_secs <= b.time_secs {
            let span = (b.time_secs - a.time_secs).max(1e-9);
            let u = ((local_secs - a.time_secs) / span).clamp(0.0, 1.0);
            let eased = apply_easing(u, a.easing);
            return Some(a.value + (b.value - a.value) * eased);
        }
    }
    None
}

fn apply_easing(t: f64, easing: Easing) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match easing {
        Easing::Linear => t,
        Easing::EaseIn => t * t,
        Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
        Easing::EaseInOut => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Id, Keyframe};

    fn clip_with_opacity_keys() -> MediaClip {
        MediaClip {
            id: Id::new_v4(),
            media_id: Id::new_v4(),
            position_secs: 10.0,
            source_in_secs: 0.0,
            source_out_secs: 5.0,
            transform: ClipTransform {
                opacity: 1.0,
                ..ClipTransform::default()
            },
            keyframes: vec![KeyframeTrack {
                property: AnimProperty::Opacity,
                keys: vec![
                    Keyframe {
                        time_secs: 0.0,
                        value: 0.0,
                        easing: Easing::Linear,
                    },
                    Keyframe {
                        time_secs: 2.0,
                        value: 1.0,
                        easing: Easing::Linear,
                    },
                ],
            }],
            ..MediaClip::default()
        }
    }

    #[test]
    fn interpolates_opacity_midpoint() {
        let clip = clip_with_opacity_keys();
        let t = evaluate_transform(&clip, 11.0); // local = 1.0 → halfway
        assert!((t.opacity - 0.5).abs() < 1e-6);
    }

    #[test]
    fn volume_keyframe_overrides_gain() {
        let mut clip = MediaClip {
            gain_db: -6.0,
            ..MediaClip::default()
        };
        clip.keyframes.push(KeyframeTrack {
            property: AnimProperty::Volume,
            keys: vec![Keyframe {
                time_secs: 0.0,
                value: -12.0,
                easing: Easing::Linear,
            }],
        });
        assert!((evaluate_volume_db(&clip, 0.0) + 12.0).abs() < 1e-9);
    }
}
