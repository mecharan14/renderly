# Command API

Status: **current** (matches project schema v3). This is the source of truth for the `Command` enum and
`apply_command` in `uppercut-core`. GUI, CLI, and MCP must all dispatch through this exact
set (see AGENTS.md §0.1) — none of them may mutate `Project` state any other way.

## Shape

```rust
pub enum Command {
    ImportMedia { path: String },
    AddTrack { kind: TrackKind, name: String, id: Option<Id> },
    AddClip { track_id: Id, media_id: Id, position_secs: f64, source_in_secs: f64, source_out_secs: f64 },
    SplitClip { track_id: Id, clip_id: Id, at_secs: f64 },
    TrimClip { track_id: Id, clip_id: Id, new_source_in_secs: Option<f64>, new_source_out_secs: Option<f64> },
    MoveClip { track_id: Id, clip_id: Id, new_position_secs: f64, new_track_id: Option<Id> },
    DeleteClip { track_id: Id, clip_id: Id, ripple: bool },
    AddCaption { track_id: Id, text: String, position_secs: f64, duration_secs: f64, style_id: String },
    SetCaption { track_id: Id, clip_id: Id, text: Option<String>, position_secs: Option<f64>, duration_secs: Option<f64>, style_id: Option<String> },
    SetAudioGain { track_id: Id, clip_id: Id, gain_db: f64 },
    GenerateCaptions { media_id: Id, track_id: Id, style_id: String, timeline_offset_secs: f64 },
    GenerateVoiceover { text: String, track_id: Id, position_secs: f64, output_path: String, provider: VoiceoverProvider },
    SetAudioFade { track_id: Id, clip_id: Id, fade_in_secs: f64, fade_out_secs: f64 },
    SetTrackAudioRole { track_id: Id, role: Option<TrackAudioRole> },
    SetProjectSettings { width: Option<u32>, height: Option<u32>, fps: Option<f64> },
    SetTrackFlags { track_id: Id, muted: Option<bool>, locked: Option<bool>, hidden: Option<bool> },
    RenameTrack { track_id: Id, name: String },
    DeleteTrack { track_id: Id },
    SetClipEnabled { track_id: Id, clip_id: Id, enabled: bool },
    SetClipTransform { track_id: Id, clip_id: Id, transform: ClipTransform },
    SetClipKeyframes { track_id: Id, clip_id: Id, keyframes: Vec<KeyframeTrack> },
    SetClipEffects { track_id: Id, clip_id: Id, effects: Vec<EffectInstance> },
    SetClipTransition { track_id: Id, clip_id: Id, transition: Option<ClipTransition> },
    Export { output_path: String, preset: ExportPreset },
}

pub fn apply_command(project: &mut Project, cmd: Command) -> Result<CommandOutcome, CommandError>;
```

The GUI's Tauri layer also exposes `apply_commands(Vec<Command>)`, applying a batch
atomically (all-or-nothing against one project clone, one undo snapshot) — see "Batch
application" below. This is a GUI/Tauri-layer convenience over the same `apply_command`
per-command logic, not a second core API; CLI/MCP callers just call `apply_command`
per command as before.

`CommandOutcome` carries whatever the caller needs back (e.g. `ImportMedia` returns the new
`media_id`; edit commands return `Ok(CommandOutcome::Applied)`; `GenerateVoiceover` returns
`media_id` and `clip_id`; `Export` drives the render pipeline).

Every command except `Export` is a pure mutation of `Project` and must be representable in
the project's command log for undo/redo (Phase 2+ feature; v0 just needs the enum to be
`Serialize`/`Deserialize`-clean so logging is possible later — don't build the log itself
yet).

## Commands

### `ImportMedia`
Probes the file at `path` (duration, dimensions, fps as applicable) and adds a `MediaItem`
to `project.media`. Returns the new `media_id`. Does not place anything on a track.

- Errors: file not found, unsupported/unprobeable format.

### `AddTrack`
Appends a new empty `Track` of the given `kind` (`video` | `audio` | `caption`) to
`project.tracks`. Returns the new `track_id`. `id` is normally omitted (server generates
one); the GUI supplies it for CapCut-style auto-track-on-drop, where an `AddTrack` and a
following `AddClip{track_id}` in the same `apply_commands` batch need to agree on the new
track's id before the server has actually created it (see "Batch application" below).

- Errors: caller-supplied `id` collides with an existing track id.

### `AddClip`
Adds a `VideoClip`/`AudioClip` (matching the target track's kind) referencing `media_id` to
`track_id`, placed at `position_secs`, sourced from `[source_in_secs, source_out_secs)` of
the media.

- Errors: `track_id`/`media_id` not found, track kind doesn't match media kind, resulting
  clip overlaps an existing clip on that track, `source_out_secs <= source_in_secs`, or the
  source range exceeds the media's probed duration (skipped if duration is unknown — see
  project-schema.md).

### `SplitClip`
Splits the clip at timeline position `at_secs` into two clips with adjusted
`source_in_secs`/`source_out_secs`, both keeping the same `media_id`. New clip on the right
gets a fresh id; the left retains the original id. Media clips copy `transform`; keyframes
are split at the cut (left keeps `t < split`, right remaps `t' = t - split`); effects are
copied with new instance ids on the right clip.

- Errors: `at_secs` not strictly inside the clip's timeline span.

### `TrimClip`
Adjusts `source_in_secs` and/or `source_out_secs` in place (ripple is not implied — this
only changes which part of the source plays, not other clips' positions). At least one of
the two optional fields must be `Some`.

- Errors: `clip_id` is a caption clip (trim only applies to media clips), resulting
  `source_out_secs <= source_in_secs`, new range exceeds media bounds, or the resulting
  on-timeline span (position unchanged, duration recomputed from the new range) overlaps a
  neighboring clip on the same track.

### `MoveClip`
Changes a clip's `position_secs`, optionally moving it to a different track
(`new_track_id`). Used for reordering clips to match the script.

- Errors: destination track kind mismatch, resulting overlap with another clip on the
  destination track.

### `DeleteClip`
Removes a clip. If `ripple: true`, all subsequent clips on the same track shift left to
close the gap; if `false`, a gap is left in place.

- Errors: clip not found on track.

### `AddCaption`
Adds a `CaptionClip` with `text` at `position_secs` for `duration_secs`, tagged with
`style_id` (a built-in style name in v0; asset-pack-provided styles come in Phase 3). Target
track must be `kind: "caption"`.

- Errors: track kind mismatch, `duration_secs <= 0`, overlap with existing caption clip on
  that track.

### `SetCaption` (Phase 2)
Updates an existing `CaptionClip` on a caption track. At least one of `text`,
`position_secs`, `duration_secs`, or `style_id` must be `Some`. Omitted fields are left
unchanged. Used by the GUI caption inspector.

- Errors: track/clip not found, track kind mismatch, no fields to change, resulting
  `duration_secs <= 0`, or resulting overlap with another caption on the same track.

### `SetAudioGain`
Sets `gain_db` on an audio-bearing clip (`AudioClip`, or a `VideoClip` with embedded audio
once that's modeled — v0 audio gain applies to `AudioClip` only).

- Errors: clip not found, or clip type has no audio.

### `GenerateCaptions` (Phase 1)
Runs local Whisper STT on `media_id` (video or audio) and adds one `CaptionClip` per
transcript segment to `track_id` (must be a caption track). Segments are placed at
`timeline_offset_secs + segment.start_secs` with duration `segment.end - segment.start`.
Uses `style_id` for export burn-in (built-in: `tiktok-bold-yellow`, `tiktok-minimal`,
`tiktok-box`, `youtube-lower-thirds`).

Requires `whisper-cli` (or `whisper`) on PATH and `UPPERCUT_WHISPER_MODEL` pointing at a
ggml model file.

- Errors: track/media not found, track kind mismatch, caption overlap, Whisper unavailable.

### `GenerateVoiceover` (Phase 1)
Synthesizes narration from `text` using `provider` and writes WAV to `output_path`, then
imports it and places a clip on `track_id` at `position_secs`. Returns `VoiceoverGenerated`
with `media_id` and `clip_id`.

`VoiceoverProvider` variants (JSON tag `provider`):

- `piper_local` — local Piper ONNX via `piper` CLI; requires `UPPERCUT_PIPER_MODEL`.
- `open_ai` — OpenAI TTS (`tts-1`); requires `OPENAI_API_KEY` (BYO, opt-in).

- Errors: track not found or not audio, TTS unavailable, overlap, probe/import failure.

### `SetAudioFade` (Phase 1)
Sets `fade_in_secs` and `fade_out_secs` on an audio clip. Applied during export via FFmpeg
`afade`.

- Errors: clip not found, not audio, negative fade durations.

### `SetTrackAudioRole` (Phase 1)
Sets `audio_role` on an audio track: `voiceover`, `dialog`, `music`, or `ambience`. Pass
`null` for `role` to clear. When a voice/dialog track and a music track are both present,
export applies sidechain ducking using `settings.duck_db` (default −12 dB).

- Errors: track not found, track is not audio.

### `SetProjectSettings` (GUI rebuild M1)
Changes project-level output settings (`width`, `height`, `fps` — e.g. an aspect-ratio
switch in the GUI). At least one field must be `Some`.

- Errors: no fields given, resulting `width`/`height` is `0`, or resulting `fps <= 0`.

### `SetTrackFlags` (GUI rebuild M1)
Sets `muted`, `locked`, and/or `hidden` on a track. At least one field must be `Some`;
omitted fields are left unchanged. `muted`/`hidden` are enforced by the engine (export and
playback both skip muted/hidden tracks); `locked` is GUI-honored only — this command does
not itself reject edits to a locked track (see project-schema.md's `Track` note).

- Errors: track not found, no fields given.

### `RenameTrack` (GUI rebuild M1)
Sets a track's `name`.

- Errors: track not found.

### `DeleteTrack` (GUI rebuild M1)
Removes a track and all its clips.

- Errors: track not found.

### `SetClipEnabled` (GUI rebuild M1)
Soft-enables/disables a media clip (`VideoClip`/`AudioClip`) without deleting it — the
existing `enabled` field on those clip types, now settable directly instead of only via
`AddClip`'s default.

- Errors: track/clip not found, or the clip is a `CaptionClip` (no `enabled` field).

### `SetClipTransform` (Phase 3.1)
Replaces the static `transform` on a media clip (video or audio). Finite numbers only;
`opacity` is clamped to `0..=1`.

- Errors: track/clip not found, not a media clip, or non-finite transform fields.

### `SetClipKeyframes` (Phase 3.1)
Replaces all keyframe tracks on a media clip. Validates unique properties, non-negative
finite times, finite values, opacity keys in `0..=1`; sorts keys by time on write.

- Errors: track/clip not found, not a media clip, or invalid keyframe data.

### `SetClipEffects` (Phase 3.1 / 3.4)
Replaces the effect-instance list on a media clip. Validates unique instance ids, known
builtin `effect_id`s (`builtin:color_adjust`, `builtin:blur`, `builtin:lut_contrast`,
`builtin:lut_warm`), finite params, and clamps known params to reasonable ranges.
Enabled instances are executed by the compositor (Phase 3.4).

- Errors: track/clip not found, not a media clip, or invalid effect list.

### `SetClipTransition` (Phase 3.5)
Sets or clears (`None`) the outgoing `ClipTransition` on a video media clip. Only
`crossfade` is supported. Requires a following clip on the same track; duration must be
`> 0` and ≤ half of each adjacent clip.

- Errors: not a video track, no following clip, invalid duration.

### `Export`
Renders the current `Project` timeline to `output_path` using `preset` (e.g.
`TikTok9x16`, `Youtube16x9`, or a `Custom { width, height, fps }` variant). Muxes mixed
audio (with fades and optional music ducking) and burns caption clips. Video layers honor
evaluated transform+opacity and enabled builtin effects; audio mix uses evaluated volume
(including Volume keyframes).

`Export` does not mutate `Project`. Progress / cancel for interactive clients go through
`uppercut_core::export::export_project_with_progress` (GUI Tauri command + CLI status
line); `apply_command(Command::Export)` and MCP still call the fire-and-forget
`export_project` wrapper. Returning `false` from the progress callback yields
`ExportError::Cancelled` and removes the export temp directory.

## Batch application (GUI rebuild M3)

The Tauri `apply_commands` command takes `Vec<Command>` and applies them in order against
a single `Project` clone: all-or-nothing (any command failing discards the whole batch,
leaving the session's project untouched) and exactly one undo snapshot/save/`project:changed`
emit for the whole batch, not one per command. This exists for gestures that are logically
a single edit but need more than one `Command` — the motivating case is CapCut-style
auto-track-on-drop: dropping a media card below the last timeline track dispatches
`[AddTrack{id: Some(new_id)}, AddClip{track_id: new_id, ...}]` as one batch, so undo
removes both the track and the clip together instead of leaving an empty track behind.
`apply_commands` is Tauri/GUI-layer only (`uppercut-app/src-tauri/src/lib.rs`), not part
of `uppercut-core`'s public API — CLI and MCP keep calling `apply_command` once per
command, which is sufficient for scripted/agent use.

## Non-goals for later Phase 3

WASM plugin invocation, asset-pack commands, and additional transition kinds are later
milestones — see [project-schema.md](project-schema.md) "What's intentionally not in v3 yet".
Do not add a command for a feature that has no schema representation yet.

## Version history

- **v0** (Phase 0): the initial 10 commands, matching project schema v0.
- **v0 + Phase 1** (non-breaking): added `GenerateCaptions`, `GenerateVoiceover`,
  `SetAudioFade`, `SetTrackAudioRole`; export muxes audio with fades/ducking and burns captions.
- **GUI rebuild M1** (non-breaking, project schema v1): added `SetProjectSettings`,
  `SetTrackFlags`, `RenameTrack`, `DeleteTrack`, `SetClipEnabled`.
- **GUI rebuild M3** (non-breaking): `AddTrack` gained an optional `id` field (server
  still generates one when omitted); added the Tauri-layer `apply_commands` batch API.
- **GUI rebuild M6** (non-breaking): `export_project_with_progress` + `ExportError::Cancelled`
  for cooperative cancel; GUI emits `export:progress` (~10 Hz) and exposes `cancel_export`.
- **Phase 3.1** (project schema v2): added `SetClipTransform`, `SetClipKeyframes`,
  `SetClipEffects`; `SplitClip` inherits transform / splits keyframes / remints effect ids.
- **Phase 3.4**: `SetClipEffects` executes builtin WGSL effects.
- **Phase 3.5** (project schema v3): added `SetClipTransition` (crossfade).

- **Phase 3.4** (non-breaking): `SetClipEffects` validates known builtin ids + param
  clamps; compositor executes `builtin:color_adjust` / `blur` / `lut_contrast` / `lut_warm`.
