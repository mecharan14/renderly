# Architecture

Status: **Phase 2 complete.** Export drives FFmpeg subprocess decode/encode with an
offscreen wgpu compositor; the Tauri app adds a native wgpu preview surface on Windows.
Linked libav via `ffmpeg-the-third` replaces the subprocess bridge once vcpkg/FFMPEG_DIR is
standard in dev/CI. Later phases add effects graph, plugin host, and full direct-manipulation
timeline tools — see [PLAN.md](../PLAN.md).

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

- **Webview** — UI chrome: media bin, timeline widget (canvas-rendered inside the webview
  or a native overlay — decide in Phase 2), inspectors, dialogs.
- **Native preview surface** — a wgpu child HWND on Windows (Phase 2 v1), receiving
  composited frames from `render_frame_at` in `uppercut-core`. Never proxies frames through
  the webview/JS bridge.

Phase 2 MVP (complete): media bin, canvas timeline with drag/trim/split/razor/snap/zoom,
transport with audio scrub, caption inspector (`SetCaption`), export dialog. macOS/Linux
preview surfaces land in Phase 3.

Tauri commands: `new_project`, `open_project`, `save_project`, `get_project`, `apply_command`,
`export_project`, `set_preview_bounds`, `update_preview`, `start_playback`, `stop_playback`,
`scrub_audio`.

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
