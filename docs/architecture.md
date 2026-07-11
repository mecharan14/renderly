# Architecture

Status: **GUI rebuild M0–M7 complete.** Export drives FFmpeg subprocess decode/encode with an
offscreen wgpu compositor; the Tauri app adds a native wgpu preview surface on Windows, now
backed by a persistent playback engine (see "Playback engine" below) instead of the Phase 2
MVP's per-frame render path. Linked libav via `ffmpeg-the-third` replaces the subprocess
bridge once vcpkg/FFMPEG_DIR is standard in dev/CI. Phase 3 adds effects graph, plugin host,
and asset packs — see [PLAN.md](../PLAN.md). Manual QA: [qa-checklist.md](qa-checklist.md).

## Crate graph

```
                     ┌───────────────────┐
                     │   uppercut-core     │   headless engine, no UI deps
                     │  (lib crate)        │
                     └─────────┬─────────┘
              ┌────────────────┼────────────────┐
              │                │                 │
   ┌──────────▼──────┐ ┌───────▼───────┐ ┌───────▼────────┐
   │  uppercut-cli    │ │ uppercut-mcp   │ │ uppercut-app    │
   │  (bin)           │ │ (bin, MCP       │ │ (bin, Tauri 2   │
   │                  │ │  server)        │ │  desktop app)   │
   └──────────────────┘ └────────────────┘ └────────────────┘
```

Dependency direction is strictly downward: `uppercut-core` never imports from the other
three. All three frontends depend on `uppercut-core` and nothing else depends on them.

## `uppercut-core`

Owns:

- **Project model** — serde types matching [project-schema.md](project-schema.md).
- **Command API** — the `Command` enum and `apply_command(&mut Project, Command) -> Result<...>`
  function matching [command-api.md](command-api.md). This is the *only* sanctioned way to
  mutate a project. Undo/redo is (eventually) a command log, not ad hoc state snapshots.
- **Media I/O** — FFmpeg-backed decode/encode. Phase 0 invokes `ffmpeg`/`ffprobe` as
  subprocesses (no link-time libav dependency); migrate to `ffmpeg-the-third` when dev/CI
  ships FFmpeg development libraries consistently.
- **Compositing** — wgpu offscreen render graph (Phase 0: scale/blit layers to output
  resolution; effects graph in Phase 3).
- **Perception** — frame rendering to image, transcript (whisper-rs), scene/silence
  detection. These back the MCP perception tools and are engine functions, not MCP-specific
  code, so the CLI can expose them too.

Internal module boundaries (indicative, refine as code lands):

```
uppercut-core/src/
  project/       schema types, versioning, (de)serialization
  commands/      Command enum, apply_command, per-command logic + unit tests
  media/         decode, encode, probe (FFmpeg)
  compose/       wgpu render graph, text/caption rendering
  audio/         mixing, TTS, STT
  perceive/      frame render-to-image, transcript, scene/silence detection
```

## `uppercut-cli`

Thin binary. Subcommands operate on a project JSON file on disk:

- `new-project` — create an empty project file at schema v0.
- `apply` — apply one or more commands (from args or a JSON script file) and save.
- `show` — print project state (or a summary) for inspection.
- `export` — render the project to a video file.

This is the simplest agent-drivable surface and the first thing Phase 0's milestone
("CLI renders a cuts-only timeline JSON to MP4") runs through.

## `uppercut-mcp`

MCP server (stdio, HTTP later) wrapping the same `uppercut-core` API as the CLI, plus
perception tools exposed as MCP tools. Built in Phase 1 — do not build ahead of the CLI
proving the command API is sufficient.

## `uppercut-app`

Tauri 2 desktop app. Rust backend commands call into `uppercut-core` exactly like the CLI
does — the Tauri command layer is another thin dispatcher, not a reimplementation.

Two rendering surfaces coexist in one window:

- **Webview** — UI chrome: media bin, timeline widget (canvas-rendered inside the webview),
  inspectors, dialogs. Built with **React 19 + TypeScript + Zustand** (GUI rebuild M2;
  PLAN.md §3 left the framework choice open — React was picked for its ecosystem and
  because the canvas-heavy timeline benefits from keeping React out of the hot render path
  entirely, see below). State lives in one Zustand store
  (`uppercut-app/src/store/editorStore.ts`); components read it via selectors and never
  call `invoke`/`listen` directly — `uppercut-app/src/lib/ipc.ts` is the *only* file that
  imports `@tauri-apps/api`, so the backend surface is typed and grep-auditable in one
  place (`docs/command-api.md`'s command builders live in `lib/commands.ts` next to it).
- **Native preview surface** — a wgpu child HWND on Windows (Phase 2 v1), receiving
  composited frames from the playback engine in `uppercut-core`. Never proxies frames
  through the webview/JS bridge.

**Timeline architecture:** the canvas timeline is deliberately *not* a React render
target. `uppercut-app/src/timeline/renderer.ts` is a pure `(canvas, state) => void` draw
function (colors sourced from `timeline/theme.ts`, which reads `styles/tokens.css` custom
properties — no hex literals in `renderer.ts`, enforced by grep gate) and
`timeline/interactions.ts` is a mouse-event state machine (hit-test → drag → commit-on-
mouseup) that mutates the Zustand store directly. `components/timeline/TimelineCanvas.tsx`
is a thin host: one `<canvas>`, a `useEffect` that calls `renderTimeline` on relevant store
changes, and the `useTimelineInteractions` hook. This keeps 60fps drag feedback off React's
reconciler — the same drag-commit pattern the old vanilla-TS timeline used (optimistic
local mutation during drag, one command dispatched on mouseup), just running inside a
Zustand-store-backed React app instead of a hand-rolled `render()` loop.

Phase 2 MVP → GUI rebuild M2/M3 (complete): media bin (draggable cards),
canvas timeline with drag/trim/split/razor/snap/zoom/fit, DOM `TrackHeaders` column
(dbl-click rename, mute/lock/hide toggles, delete), cross-track drag, locked-track mouse
guard (hatched overlay; CLI/MCP can still edit a locked track by design), bin→timeline
drag with a ghost preview and CapCut-style auto-track-on-drop, right-click `ContextMenu`
(split/duplicate/copy/paste/delete/ripple-delete/enable-disable), full keymap (see
`App.tsx`'s `onKeyDown`), transport with editable timecode + ±1 frame buttons + fullscreen
preview (Esc exits; bounds recompute), aspect-ratio quick-switch (`RatioMenu` →
`SetProjectSettings`, including Original from first video media dims), tab rail
(Media/Audio/Text live; Stickers/Effects/Transitions/Filters/Adjustment stubbed for
Phase 3), inspectors (video/audio clip with gain/fades/enable/trim, caption style gallery,
project canvas settings), Text panel auto-captions + Audio panel TTS voiceover, export
dialog with progress bar / ETA / cancel (M6), M7 polish (Coming Soon panels, empty states,
media skeletons, focus rings / scrollbars, `playback:error` toasts, pause-on-edit).
Thumbnails/waveforms are M4. macOS/Linux preview surfaces land in Phase 3.

Tauri commands: `quick_start_project`, `new_project`, `open_project`, `save_project`,
`get_project`, `apply_command`, `apply_commands`, `undo`, `redo`, `export_project`,
`cancel_export`, `play`, `pause`, `seek`, `scrub_audio` are `async fn`, dispatched off the
main thread (see "Playback engine" below for why). One exception: **`set_preview_bounds`
stays a sync `fn`**, deliberately — see the callout below. `apply_commands` (GUI rebuild
M3) batches several `Command`s atomically — one project clone, one undo snapshot, one
save, one `project:changed` emit — for gestures that are logically a single edit but need
more than one core command (see command-api.md "Batch application"); it's a thin
Tauri-layer convenience over `uppercut_core::apply_command`, not a second core API.
`export_project` clones the project and never holds `edit_lock` during encode; it accepts
preset shorthand (`"tiktok"` / `"youtube"`) or full `ExportPreset` JSON (including
`Custom`), throttles `export:progress` to ~10 Hz, and cooperates with `cancel_export`.

### Playback engine

Phase 2's original playback path froze the window on play: every Tauri command was a sync
`fn`, which Tauri 2 runs on the main thread, and the JS driver was a `setInterval` at
`1000/fps` calling `update_preview` — which called `render_frame_at`, creating a **new
wgpu Instance+Adapter+Device** and a fresh decoder map on *every tick*, and spawning
**ffprobe + ffmpeg per video layer per frame**. At 60 fps that's ~60 GPU device creations
and ~120 subprocess spawns a second, on the thread that also has to pump window messages —
hence "Not Responding".

The fix has two parts:

- **`uppercut_core::export::FrameRenderer`** — a persistent renderer that holds one wgpu
  `Compositor` and one open decoder per source media across repeated `render()` calls,
  instead of rebuilding both per call. `render_frame_at` is now a one-shot convenience
  wrapper around a throwaway `FrameRenderer`, fine for perception/MCP call sites that
  render a single isolated frame; anything rendering a sequence (playback, export) keeps
  one `FrameRenderer` alive for the whole run. `export_project`'s loop and the playback
  engine below both use it directly. Decode-time scaling/pacing (`DecodeOptions` /
  `media::ReaderOptions`, applied via `VideoReader::open_with`) let playback decode at
  preview-panel resolution and at the project's fps instead of full source
  resolution/native fps.
- **`uppercut-app/src-tauri/src/playback.rs::PlaybackEngine`** — owns two long-lived
  workers, managed from `AppState`, so the four playback commands (`play`, `pause`,
  `seek`, `scrub_audio`) never block the async runtime's worker pool for more than a
  clone + a channel send:
  - A **play-session worker thread**, spawned by `play(time_secs)` and torn down by
    `pause()`/a new `play()`/session switch. It pre-mixes timeline audio **once**, for
    `[time_secs, duration)`, into a temp WAV via `mix_timeline_audio_range_to_file` (a
    single ffmpeg filtergraph, not one spawn per playback chunk), starts a rodio `Sink`
    on it, then loops: `t = start + clock`, where `clock` is `Sink::get_pos()` when audio
    is present (rodio quantizes this to buffer boundaries, so it's read directly rather
    than blended with a wall-clock delta) or `Instant::elapsed()` when the timeline has
    no audio; render via `FrameRenderer::render` at `t`; present to the native preview
    surface; emit `playback:tick`. A `seek(time_secs)` call while playing writes into a
    one-slot `Mutex<Option<f64>>` that the loop checks every iteration and coalesces (a
    newer seek arriving before the loop observes an older one simply overwrites it) — on
    observing one it stops the sink, discards the temp WAV, and restarts the pre-mix/sink
    from the new position, keeping the same `FrameRenderer` (so already-open decoders
    just reopen at the new time instead of the whole renderer rebuilding).
  - An always-on **scrub worker thread** serves `seek` calls made while paused and all
    `scrub_audio` calls. Requests are coalesced the same way (one-slot, latest wins) and
    the worker caches its own `FrameRenderer`, rebuilding it only when the requested
    output settings actually change — so repeated scrub calls during a timeline drag
    reuse the same wgpu device instead of recreating one per call. Each request carries
    the `PlaybackEngine`'s `play_epoch` (bumped once per `play()` call) captured at
    submission time; the worker re-checks it against the current epoch both before
    starting the render and again right before presenting, skipping the frame if a `play()`
    landed in between — otherwise a scrub queued just before the user hits Play can finish
    its (non-instantaneous, real-decode) render *after* playback has already started
    presenting live frames, overwriting one with stale content for a frame interval.
  - `pause()`/`stop()` join the play-session worker thread, which can block for as long as
    an in-flight audio pre-mix takes (multi-second, on a long timeline) if called right
    after `play()` starts — that join runs inside `spawn_blocking`, not inline in the
    async command handler, so it can't stall the tokio worker pool behind it (it does not
    make the *join itself* faster — that would need the pre-mix to be cancellable, a
    bigger change not yet done).
- The frontend never advances the playhead itself. The old `setInterval` loop is gone;
  `main.ts` only listens for `playback:tick`/`playback:state` and reflects `time_secs`
  into the timeline UI.

**Win32 thread affinity:** `set_preview_bounds` is the only call site that creates the
native preview child HWND and its wgpu swapchain (`PreviewPanel::set_bounds` ->
`ensure_child_window` / `GfxState::new`). It must stay a *sync* Tauri command — Tauri
dispatches sync commands on the main thread, and Win32 windows must be created on a
thread that pumps messages for them. Making it `async fn` (as an earlier draft of this
work did) moves that `CreateWindowExW` call onto a background tokio worker thread with no
message loop, which hangs the very first time the preview panel is sized. Presenting
frames onto the already-created surface from the playback/scrub worker threads is fine —
only creation needs the main thread.

### Undo/redo

`uppercut-app/src-tauri/src/lib.rs::History` is a bounded (100-entry) stack of full
`Project` snapshots — `undo: Vec<Project>`, `redo: Vec<Project>` — held in `AppState`
alongside the session, not in `uppercut-core`. `apply_command` pushes the *pre-mutation*
project onto `undo` (and clears `redo`, as any new edit invalidates the old redo branch)
only after the command has actually succeeded; `undo`/`redo` pop a snapshot, push the
current project onto the other stack, and install the popped one as the session's project.

This does **not** violate AGENTS.md's "every edit through `apply_command`" rule: every
entry in the stack is a `Project` value that only ever came from a successful
`apply_command` call (or a previous undo/redo) — restoring one doesn't construct or mutate
project state through any path other than that. It's app-session-layer state management
(what to show right now), not a second way to edit a project. The command-log-based
undo/redo `uppercut-core` eventually wants (see command-api.md's `Export` note on
serializable commands) is a different, more powerful mechanism (replay/audit/collab) that
can replace this later without changing the contract GUI/CLI/MCP see.

`quick_start_project`/`new_project`/`open_project` all clear history — undoing past a
project switch would try to restore a snapshot from a different project entirely.

**Serialization.** `AppState::edit_lock` (a `tauri::async_runtime::Mutex<()>`, i.e. a
`tokio` async mutex — needed because it's held across `.await` points, which a
`parking_lot` guard can't safely do without the `send_guard` feature) is acquired for the
full duration of `apply_command`, `apply_commands`, `undo`, `redo`, and project
open/create/quick-start, end to end: snapshot, compute, history push, session write-back,
and save. Earlier, these steps happened as several independent lock acquisitions
(`session`, then `history`, then `session` again), so two overlapping calls — e.g. a
double-tapped Ctrl+Z firing two `undo` invokes before the first resolved — could each read
the same pre-mutation project and race on which write-back landed last, silently
corrupting the undo/redo stacks. `edit_lock` makes that structurally impossible: every
whole-project mutation is now fully serialized, not just "usually fine because it's rare."

**Session-lock discipline.** `AppState::commit_project` is the only way any of the above
write a project back into the session: it holds `session`'s lock just long enough to swap
in the new `Project` and read the save path, then does the actual serialize+`fs::write` in
`spawn_blocking`, outside any lock. This matters because `session`'s lock is also what
`play`/`seek`/`scrub_audio`/`get_project` need — a blocking disk write held under that lock
(the previous behavior) stalls all of those behind every single edit's save.

### Event contract

| Event | Payload | Emitted by |
|---|---|---|
| `playback:tick` | `{ time_secs: f64, playing: bool }` | ~30 Hz while playing; once on seek/EOF |
| `playback:state` | `{ playing: bool, time_secs: f64 }` | play start, pause, end-of-timeline |
| `playback:error` | `{ message: string }` | renderer init failure; throttled present/render errors mid-play (toast, no crash) |
| `project:changed` | `{ revision: u64, can_undo: bool, can_redo: bool }` | every successful `apply_command`, `undo`, `redo`, and on project open/new/quick-start |
| `media:thumbnails-ready` | `{ media_id, strip_path, cols, rows, tile_width, tile_height, interval_secs }` | media asset worker, on import and on project open (cache hit or fresh generation) |
| `media:waveform-ready` | `{ media_id, peaks: f32[], bucket_secs }` | media asset worker, same triggers as above |
| `export:progress` | `{ phase: "video"\|"audio"\|"mux", frame, total_frames, fraction }` | ~10 Hz during `export_project`; `cancel_export` stops at the next checkpoint |

### Tauri command surface (M7)

| Command | Notes |
|---|---|
| `quick_start_project` / `new_project` / `open_project` / `save_project` / `get_project` | Session I/O; clears history on open/new/quick-start |
| `apply_command` / `apply_commands` | Core edits; `apply_commands` is one undo snapshot |
| `undo` / `redo` | Session snapshot stacks (see Undo/redo) |
| `export_project` / `cancel_export` | Progress via `export:progress`; clones project, no `edit_lock` |
| `play` / `pause` / `seek` / `scrub_audio` | PlaybackEngine worker |
| `set_preview_bounds` | **Sync** — creates/resizes native preview HWND on the UI thread |
| `request_media_assets` / `get_media_assets` | Thumbnail/waveform cache |

Mutating GUI edits pause playback first (CapCut-style) so decoders and audio don't race the
new project state.

`project:changed` doesn't carry the project itself — the frontend refetches via
`get_project` on receipt, so it's the single source of truth for "something changed,
re-read state," regardless of whether the mutation came from the user's own `apply_command`
call or from undo/redo.

### Media assets (thumbnails + waveforms, M4)

`uppercut-app/src-tauri/src/media_assets.rs` generates and caches a tiled thumbnail-strip
PNG (`uppercut_core::generate_thumbnail_strip` — one ffmpeg call, not one spawn per
thumbnail) and waveform peaks (`uppercut_core::audio_peaks`, reused as-is from the
perception module) for each imported media item, entirely off the edit path: it's a plain
`tauri::async_runtime::spawn` task, not gated by `AppState::edit_lock`, so importing media
or opening a project with many items never slows down `apply_command`/`undo`/`redo`.

- **Cache key:** `(path, mtime)` hashed with `std::hash::Hash`/`DefaultHasher` — not
  cryptographic, deliberately (see the doc comment on `cache_key`), since this only needs
  to be a stable local-disk cache key, not a security boundary. Cached in
  `{app_cache_dir}/media-cache/{key}.png` + `{key}.json` (the JSON sidecar holds
  cols/rows/tile dimensions/interval and the peaks array, so `get_media_assets` can answer
  synchronously from a cache hit with zero ffmpeg work).
- **Triggers:** automatically after a successful `ImportMedia` command, and for every
  media item already in the project on `open_project` (a cache hit there is a cheap file
  read, so this is safe to call unconditionally rather than tracking "did this project
  already get its assets"). `request_media_assets(media_id)` exists for the frontend to
  explicitly retry after a failure.
- **Delivery:** `media:thumbnails-ready`/`media:waveform-ready` events (see the table
  above) for the automatic paths, or synchronously via `get_media_assets(media_id)` for a
  cache-hit check with no generation triggered (used on mount/selection).
- **Serving the strip PNG to the webview:** the Tauri asset protocol, scoped to
  `$APPCACHE/media-cache/*` (`tauri.conf.json`'s `security.assetProtocol`) — the frontend
  converts the real filesystem path to a loadable URL via `convertFileSrc`
  (`lib/ipc.ts::assetUrl`), not base64-encoded over IPC.
- **Frontend rendering:** `timeline/renderer.ts` tiles the strip image across a video
  clip's rectangle, sampling the tile nearest each on-screen column's source time (mapped
  through `source_in_secs`/`source_out_secs`, so a trimmed clip shows only its trimmed
  sub-range); audio clips draw a `Path2D`-free (plain `fillRect` bars) waveform sliced the
  same way from the peaks array. `MediaPanel.tsx`'s bin cards show a hover-scrub
  filmstrip (mouse X position selects which strip tile to display via CSS
  `background-position`), CapCut-style.

Note: `perceive`'s MCP-facing thumbnail tool (letting an agent request a strip
independent of the GUI cache) is a follow-up, not yet wired up.

## Plugins (Phase 3+)

Two tiers, not yet implemented:

- **Asset packs** — data-only (JSON manifest + media), interpreted by `uppercut-core`
  against existing engine capabilities (transitions-as-shader-params, caption style
  presets, LUTs, stickers). No sandboxing concerns because there's no code.
- **WASM plugins** — sandboxed via `wasmtime`, capability-scoped API for frame effects,
  audio effects, generators/analyzers, and integrations. Design doc to be added under
  `docs/plugin-api.md` when Phase 3 starts.

## Why this shape

See PLAN.md §2 for the four architectural principles this graph exists to serve — in
short: one command API keeps AI and human editing paths identical by construction, and the
core/frontend split keeps the engine testable and the UI swappable.
