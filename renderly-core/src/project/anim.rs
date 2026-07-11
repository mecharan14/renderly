//! Keyframe evaluation for Phase 3 — interpolates clip transform / volume / speed.

use super::{clamp_clip_speed, AnimProperty, ClipTransform, Easing, KeyframeTrack, MediaClip};

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
            AnimProperty::Volume | AnimProperty::Speed => {}
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

/// Instantaneous playback speed at absolute timeline time (Speed keys or base `speed`).
pub fn evaluate_speed(clip: &MediaClip, timeline_secs: f64) -> f64 {
    let local = (timeline_secs - clip.position_secs).max(0.0);
    for track in &clip.keyframes {
        if track.property == AnimProperty::Speed {
            if let Some(v) = sample_track(track, local) {
                return clamp_clip_speed(v);
            }
        }
    }
    clip.speed_factor()
}

fn has_speed_keys(clip: &MediaClip) -> bool {
    clip.keyframes
        .iter()
        .any(|t| t.property == AnimProperty::Speed && !t.keys.is_empty())
}

const INTEGRATE_DT: f64 = 1.0 / 120.0;

/// ∫ speed(τ) dτ over clip-local `[local_from, local_to]` (Riemann sum).
pub fn integrate_speed(clip: &MediaClip, local_from: f64, local_to: f64) -> f64 {
    let from = local_from.max(0.0);
    let to = local_to.max(from);
    if to - from < 1e-12 {
        return 0.0;
    }
    if !has_speed_keys(clip) {
        return (to - from) * clip.speed_factor();
    }
    let mut acc = 0.0;
    let mut t = from;
    while t < to {
        let step = (to - t).min(INTEGRATE_DT);
        let mid = t + step * 0.5;
        let s = evaluate_speed(clip, clip.position_secs + mid);
        acc += s * step;
        t += step;
    }
    acc
}

/// Timeline duration such that ∫_0^T speed = source span.
pub fn timeline_duration_secs(clip: &MediaClip) -> f64 {
    let src = (clip.source_out_secs - clip.source_in_secs).max(0.0);
    if src <= 0.0 {
        return 0.0;
    }
    if !has_speed_keys(clip) {
        return src / clip.speed_factor();
    }
    // Grow until integral covers source; after last key, speed is held.
    let mut t = 0.0;
    let mut covered = 0.0;
    let max_t = (src / 0.25) + 1.0; // worst-case at min speed
    while covered + 1e-9 < src && t < max_t {
        let step = INTEGRATE_DT;
        let s = evaluate_speed(clip, clip.position_secs + t + step * 0.5);
        let remain = src - covered;
        let need = remain / s.max(1e-6);
        if need <= step {
            return t + need;
        }
        covered += s * step;
        t += step;
    }
    t
}

/// Media source time at absolute timeline `t`.
pub fn source_time_at(clip: &MediaClip, t: f64) -> f64 {
    let local = (t - clip.position_secs).max(0.0);
    clip.source_in_secs + integrate_speed(clip, 0.0, local)
}

/// Break a clip into constant-speed audio segments for atempo chaining.
/// Returns `(timeline_offset_from_clip_start, duration_timeline, speed)` triples.
pub fn speed_segments(clip: &MediaClip) -> Vec<(f64, f64, f64)> {
    let dur = timeline_duration_secs(clip);
    if dur <= 0.0 {
        return Vec::new();
    }
    if !has_speed_keys(clip) {
        return vec![(0.0, dur, clip.speed_factor())];
    }
    let mut out = Vec::new();
    let mut t = 0.0;
    let seg = 0.05_f64; // 50ms windows
    while t < dur - 1e-9 {
        let len = (dur - t).min(seg);
        let s = evaluate_speed(clip, clip.position_secs + t + len * 0.5);
        out.push((t, len, s));
        t += len;
    }
    out
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

    #[test]
    fn constant_speed_matches_legacy() {
        let clip = MediaClip {
            source_in_secs: 0.0,
            source_out_secs: 4.0,
            speed: 2.0,
            ..MediaClip::default()
        };
        assert!((timeline_duration_secs(&clip) - 2.0).abs() < 1e-6);
        assert!((source_time_at(&clip, 1.0) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn speed_ramp_slows_timeline() {
        let clip = MediaClip {
            source_in_secs: 0.0,
            source_out_secs: 2.0,
            speed: 1.0,
            keyframes: vec![KeyframeTrack {
                property: AnimProperty::Speed,
                keys: vec![
                    Keyframe {
                        time_secs: 0.0,
                        value: 0.5,
                        easing: Easing::Linear,
                    },
                    Keyframe {
                        time_secs: 4.0,
                        value: 0.5,
                        easing: Easing::Linear,
                    },
                ],
            }],
            ..MediaClip::default()
        };
        let dur = timeline_duration_secs(&clip);
        assert!(
            (dur - 4.0).abs() < 0.05,
            "expected ~4s timeline at 0.5x for 2s source, got {dur}"
        );
    }
}
