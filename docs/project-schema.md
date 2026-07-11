# Project schema — v1

Status: **current**. This is the source of truth for `uppercut-core`'s project
model. Implementation types in `uppercut-core/src/project/` must match this document; if
they diverge, fix whichever one is wrong and note it in the same PR. Schema changes bump
`schema_version` and are documented in the "Version history" section at the bottom.

The project file is a single JSON document, human-readable, git-diffable, on disk with
extension `.uppercut.json`.

## Top-level shape

```jsonc
{
  "schema_version": 1,
  "id": "b3f1c2a0-...-uuid",
  "name": "ultra-bruno-ep12",
  "settings": {
    "fps": 60.0,
    "width": 1080,
    "height": 1920,
    "sample_rate": 48000,
    "duck_db": -12.0
  },
  "media": [ /* MediaItem[] */ ],
  "tracks": [ /* Track[] */ ]
}
```

| Field | Type | Notes |
|---|---|---|
| `schema_version` | u32 | `1` for this spec. Loaders must reject unknown newer versions rather than guess. |
| `id` | string (UUIDv4) | Stable project identity, generated on creation. |
| `name` | string | Human-facing project name; not used for file paths. |
| `settings.fps` | f64 | Timeline/output frame rate. |
| `settings.width`, `settings.height` | u32 | Output resolution in pixels (e.g. 1080×1920 for TikTok). |
| `settings.sample_rate` | u32 | Audio sample rate in Hz (e.g. 48000). |
| `settings.duck_db` | f64 | Music ducking under voice/dialog during export (dB). Default −12; `0` disables ducking. |
| `media` | MediaItem[] | Pool of imported source files, referenced by id from clips. |
| `tracks` | Track[] | Ordered list, index = stacking/mix order (video: top track drawn last/on top; audio: all tracks mixed; captions: rendered above video). |

## MediaItem

```jsonc
{
  "id": "media-uuid",
  "path": "C:/footage/round1_raw.mp4",
  "kind": "video",            // "video" | "audio" | "image"
  "duration_secs": 184.5,      // null if the prober doesn't support this format yet
  "width": 1920,                // null if unknown, present for video/image
  "height": 1080,               // null if unknown, present for video/image
  "fps": 60.0                    // null if unknown, present for video
}
```

Paths are stored as given at import time (absolute, or relative to the project file's
directory — decide and enforce one convention when `ImportMedia` is implemented; recommend
relative-to-project for portability). Probing (duration/dimensions/fps) happens once at
import via `uppercut-core::media::probe` and is cached here rather than re-probed on load.

`duration_secs`/`width`/`height`/`fps` are **nullable**: v0's prober covers a growing set of
formats without requiring a system FFmpeg install (see `uppercut-core/src/media/mod.rs` for
current coverage — WAV today, more as the Phase 0 media spine work lands). Commands that
need bounds against media duration (e.g. `AddClip`) skip that validation when the value is
`None` rather than failing — see command-api.md.

## Track

```jsonc
{
  "id": "track-uuid",
  "kind": "video",             // "video" | "audio" | "caption"
  "name": "Gameplay A",
  "audio_role": "music",       // optional on audio tracks: voiceover | dialog | music | ambience
  "muted": false,               // v1: excluded from the audio mix on export/playback
  "locked": false,              // v1: GUI-honored only, see note below
  "hidden": false,               // v1: excluded from composited video/burned-in caption layers
  "clips": [ /* Clip[], see below — shape depends on track kind */ ]
}
```

Clips within a track are not required to be pre-sorted in the JSON; consumers sort by
`position_secs` on load. Clips within a single track must not overlap (enforced by
`apply_command`, not by the schema itself).

`muted`/`locked`/`hidden` default to `false` and are omittable in v0 project files (loaded
via `#[serde(default)]`, no migration step needed). `muted` and `hidden` are enforced by
the engine itself (export and the playback engine both skip muted/hidden tracks — see
`uppercut-core/src/export/mod.rs`). `locked` is **GUI-honored only**: `apply_command`
deliberately does not reject edits to a locked track, so CLI/MCP agents can still edit it;
the GUI's timeline interactions are responsible for refusing mouse edits when it's set.

## Clip variants

`Clip` is a tagged union on an implicit `type` discriminant matching the track kind it
lives in. (Rust: an enum `Clip { Video(VideoClip), Audio(AudioClip), Caption(CaptionClip) }`,
serialized with `#[serde(tag = "type")]`.)

### VideoClip / AudioClip (media-backed)

```jsonc
{
  "type": "video",             // or "audio"
  "id": "clip-uuid",
  "media_id": "media-uuid",
  "position_secs": 12.0,        // where this clip starts on the timeline
  "source_in_secs": 3.5,        // in-point within the source media
  "source_out_secs": 9.0,       // out-point within the source media
  "gain_db": 0.0,               // audio-only; present but ignored (0.0) for video-only clips with no embedded audio use
  "fade_in_secs": 0.0,          // audio export fade-in (Phase 1)
  "fade_out_secs": 0.0,         // audio export fade-out (Phase 1)
  "enabled": true                // soft-disable without deleting
}
```

Duration on the timeline = `source_out_secs - source_in_secs`. Speed ramping is out of
scope for v0 (Phase 4 feature) — no `speed` field yet; add one only when that feature is
implemented, not preemptively.

### CaptionClip

```jsonc
{
  "type": "caption",
  "id": "clip-uuid",
  "text": "he just does NOT miss",
  "position_secs": 12.0,
  "duration_secs": 2.2,
  "style_id": "tiktok-bold-yellow"   // references a built-in or asset-pack style; free-text id, no inline styling in v0
}
```

Word-level timing (for word-by-word highlight caption styles) is deliberately deferred:
v0 captions are line-level. Add a `words: [{ text, start_offset_secs, end_offset_secs }]`
field in a later schema version once the caption renderer needs it — don't add it unused.

## What's intentionally not in v0

Keyframes/animation, transitions, effects/filters, plugin references, and multi-cam are
Phase 3+ features (PLAN.md §4) and have no schema representation yet. When they land, they
extend `Clip` and/or add new top-level collections — update `schema_version` and this doc
together with the code.

## Version history

- **v0** (Phase 0): initial shape above — media pool, video/audio/caption tracks,
  line-level captions, no effects/transitions/keyframes.
- **v0 + Phase 1** (non-breaking): `Track.audio_role`, `MediaClip.fade_in_secs` /
  `fade_out_secs`, `Settings.duck_db`.
- **v1** (GUI rebuild M1, non-breaking): `Track.muted` / `locked` / `hidden`. v0 files
  load unchanged (fields default to `false`); no migration required.
