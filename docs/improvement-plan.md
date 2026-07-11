# Renderly — Performance, UX, Design System & Live-Agent Plan

> Status: approved 2026-07-11. This is the working plan for the current improvement push.
> Any agent implementing items from this file must first read [AGENTS.md](../AGENTS.md)
> (architecture contract, definition of done) — its rules override anything here in case
> of conflict. Work through the sprint ordering at the bottom; each item names its files.

## Context

Renderly (Rust `renderly-core` + Tauri 2/React app + CLI + MCP server) has implemented PLAN.md through Phase 4 (ML items deliberately on hold), but the app feels slower than a web editor despite the native stack, and the UX/visual layer lags behind the engine. Exploration confirmed the architecture is fundamentally right (native wgpu preview child window, command-API chokepoint, backend-owned playback clock) — the slowness comes from a handful of identifiable hot-path defects, chiefly: **software full-resolution decode, a GPU→CPU→GPU readback round-trip on every preview frame, per-frame texture allocation, whole-project JSON crossing IPC twice per edit, and full timeline-canvas reallocation ~30×/sec during playback.**

User decisions: (1) prioritize engine preview/playback perf, then frontend perf; (2) hardware decode via FFmpeg CLI `-hwaccel` now, `ffmpeg-the-third` migration deferred; (3) design system = **fresh neutral palette**, light + dark from day one (not carrying the ember-orange brand); (4) CapCut-style project lifecycle (home screen with all projects, auto-create, auto-save, import-first flow) and two reported bugs fixed: broken auto-snapping and spurious video seeking on every timeline/property edit.

Reference studied: OpenCut classic (github.com/opencut-app/opencut-classic) — snapping subsystem, group ops, actions registry, preview transform handles, HSL CSS-variable theming, TS-orchestrated WGSL effect registry.

---

## Workstream A — Preview/playback performance (engine) — TOP PRIORITY

Root cause chain per frame today: software decode at **full source res** → CPU per-pixel effects → GPU composite on the **compositor's own wgpu device** → blocking `copy_texture_to_buffer` readback to `Vec<u8>` (`renderly-core/src/compose/mod.rs:365-419`) → re-upload via `write_texture` into a **freshly created texture** on the preview's separate device (`renderly-app/src-tauri/src/preview/gfx.rs:228`).

Target architecture: one shared wgpu device owned by the app, injected into the compositor; compositor gains `RenderTarget::Surface` (preview, zero readback) vs `RenderTarget::Readback` (export, unchanged). Core still only takes `wgpu::Device/Queue` handles — no UI deps (AGENTS.md rule 2 preserved).

- **A1. Texture pooling + reuse (quick win).** Cache the preview frame texture keyed on (w,h) in `gfx.rs:present_rgba` (`gfx.rs:217-265`); recreate only on size change. Same for per-layer textures in `compose/mod.rs:458` (small `TexturePool` keyed by size).
- **A2. Preview-resolution decode during playback.** `playback.rs:171-181` passes `target_height: None` (full-res) to dodge a scale-rounding drift bug. Fix root cause: even-dimension rounding (`scale=-2:H`) with stride derived from FFmpeg's actual output size; unit-test odd-dimension sources. Decoding 1080p/4K at panel height is a 4–9× decode/memcpy reduction — likely the single biggest felt win.
- **A3. Hardware decode via CLI flags.** Add probed `-hwaccel` (`cuda`/`qsv`/`d3d11va`, fallback `auto` → software on failure) before `-i` in `VideoReader::open_with` (`renderly-core/src/media/ffmpeg_cli.rs:431`). ~50 lines; moves H.264/HEVC decode off CPU. Record `ffmpeg-the-third` zero-copy migration as a later phase behind the `VideoReader` boundary.
- **A4. Kill the preview readback — shared device (the architectural fix).** `Compositor::with_device(Arc<Device>, Arc<Queue>)` (existing self-owned constructor kept for CLI/MCP/export); new `compose_to_view(&TextureView)` path; playback loop composites straight into the surface texture and presents. Handle surface format (Bgra8Unorm vs Rgba8Unorm) via a format param on the final pass; move surface acquire/present into the playback worker thread. `present_rgba` leaves the hot path (stays for scrub worker until it's migrated too).
- **A5. Decoded-frame cache + scrub prefetch.** `DecoderState::frame_at` (`renderly-core/src/export/mod.rs:103-138`) respawns FFmpeg on any backward seek or >0.5s jump. Add an LRU ring of preview-res frames (~60 frames ≈ 90 MB at 720p, configurable) + direction-aware ±0.5s prefetch in the existing scrub worker (already latest-wins + epoch-guarded, `playback.rs:59-87, 473`). Export's monotonic path untouched.
- **A6. Segmented audio premix.** `playback.rs:341` premixes the entire play range to a temp WAV before playback (multi-second start latency). Mix in ~5s chunks with `-ss/-t` bounds: chunk 0 first (play starts in ~100–300 ms), append following chunks to the rodio `Sink` (stays master clock); invalidate queued chunks on seek/edit. Reuses the existing filtergraph code.
- **A7. GPU-ify CPU per-pixel effects (can trail).** Order: pack LUTs (per-pixel trilinear in `packs/mod.rs:293` → one WGSL 3D-texture sample), then chroma/matte/mask (`export/mod.rs:191-201`) as WGSL passes beside the existing builtin GPU effects. WASM plugins stay CPU (sandbox) but cache `Store`+`Instance` per plugin instead of per-call (`plugins/mod.rs:201`).
- **A8. Hardware encode for export.** Parameterize the hardcoded `-c:v libx264` (`ffmpeg_cli.rs:558`) with an `ExportSettings` struct; probe `h264_nvenc`/`hevc_nvenc`/`qsv` with software fallback. (Pairs with C5 export dialog.)
- **A0. Perf HUD first.** Tiny frame-time/decode-time overlay in `gfx.rs` (or tracing counters) so every A-step is measured — "measure from day one" (PLAN.md §7).

## Workstream B — Frontend performance

- **B1. Canvas resize guard + layered playhead.** `timeline/renderer.ts:199-203` sets `canvas.width/height` unconditionally every render (clears + reallocates the bitmap ~30×/sec during playback, plus a `getBoundingClientRect` layout read). Guard behind actual size change; track size via the existing `ResizeObserver` (`TimelineCanvas.tsx:68-74`). Move the playhead (and snap guides) to a separate overlay — a GPU-composited DOM div is simplest — so playhead motion stops repainting all clips/thumbnails/waveforms (`renderer.ts:368-392`). Split the render effect's deps (`TimelineCanvas.tsx:42-66`) into structural vs transient.
- **B2. Single project fetch per edit.** `dispatch()` refetches the full project (`editorStore.ts:401-422`) and the `project:changed` handler refetches again (`editorStore.ts:667-673`) — two full-project IPC round-trips per edit. Fix: client-generated `mutationId` echoed in the `project:changed` payload; handler skips ids already handled. Same for `dispatchBatch`/undo/redo.
- **B3. Delta protocol (after B2).** `apply_command` response returns `{ revision: u64, patch: RFC-6902 JSON Patch }` computed generically with the `json-patch` crate's `diff()` on pre/post project; `project:changed` carries `{ revision }` only; frontend applies patch (`fast-json-patch`), falls back to full fetch on revision mismatch (covers external/MCP edits). Zero per-command frontend work — preserves the one-command-API property. Also swap the per-call full `Project` clone in play/seek/scrub Tauri commands (`src-tauri/src/lib.rs:932-1033`) for an `Arc<Project>` snapshot bumped on edit.
- **B4. Memoization sweep.** Stable selector for `mediaAssets` so thumbnail/waveform arrivals don't force full timeline repaints; keep the existing good patterns (optimistic drag + commit-on-release, throttled overlays) untouched.

## Workstream BX — Reported bugs (fix in Sprint 1)

- **BX1. Spurious seek on every edit (root cause found).** `dispatch()`/`dispatchBatch()` unconditionally call `ipc.seek(get().playhead)` after every command and stop playback before mutating (`editorStore.ts:407, 411, 433`) — so any timeline drag commit or inspector property tweak forces a pause + re-seek + scrub re-render. Fix: drop the unconditional `seek`; after an edit, the backend should re-render the current frame itself when the composition at the playhead changed (add a lightweight `refresh_frame` path in `playback.rs` scrub worker, or have `apply_command` trigger it). Only pause playback for structural edits that invalidate open decoders (split/move/delete under the playhead), not for property changes like opacity/gain.
- **BX2. Auto-snapping broken (root cause identified in `timeline/layout.ts:73-97`).** Two compounding flaws: (a) the threshold is converted px→seconds (`10 / pxPerSec`) and at high zoom (`pxPerSec` ≥ 300) becomes smaller than one frame period, so `snapToFrame` quantization always wins and the magnet feels dead; (b) `collectSnapTimes` injects a 1-second grid (`layout.ts:67`) that floods candidates at low zoom, so clip edges rarely win. Fix: compare distances **in pixels**, use a zoom-independent px threshold, prioritize snap sources (clip edges > playhead > grid), and only frame-quantize when no magnet target hit. This lands as the first half of C2's snapping subsystem extraction.
- **BX3. Verify drag-to-timeline auto track creation end-to-end.** The store already batches `AddTrack`+`AddClip` when dropping below the last lane (`editorStore.ts:312-339`); test the CapCut behaviors explicitly — drop on empty timeline area, drop of audio → audio track, image/video → video track, invalid-target coloring — and fix gaps found.

## Workstream G — Project lifecycle & assets (CapCut-style)

- **G1. Project home screen.** Replace the current welcome screen with a CapCut-style gallery: list all projects from `default_projects_dir()` (already exists — `src-tauri/src/lib.rs:300`; quick-start already writes `Untitled {ts}.renderly.json` there) with name, thumbnail (render frame 0 via the existing FrameRenderer, cached beside the project file), duration, last-modified; card actions: open, rename, duplicate, delete (to recycle bin); prominent "New project" tile. New Tauri commands: `list_projects`, `rename_project`, `delete_project`, `project_thumbnail`.
- **G2. Auto-create + auto-save.** "New project" auto-creates in the projects dir with no dialog (reuse `quick_start_project`, `lib.rs:287`). Auto-save: debounced `save_project` (~2s after the last `project:changed`, plus on window close/blur) so Ctrl+S becomes optional; a subtle "Saved" indicator replaces unsaved-changes anxiety. Backend already funnels all mutations through the command chokepoint, so hooking the emit path is sufficient.
- **G3. Import-first flow.** On entering a fresh empty project, land the user in an import-forward state (CapCut-style): large drop-target with click-to-browse in the media panel area; both click and OS drag-drop already exist (`App.tsx:22-40`, `MediaPanel.tsx`) — this is layout/emphasis work, not new plumbing.
- **G4. Full asset-kind support in the bin.** Core already models Video/Audio/Image (`renderly-core/src/project/mod.rs:115`). Fix the bin to show audio (currently filtered, `MediaPanel.tsx:46`) with waveform strips, and images with static thumbs; per-kind metadata (resolution/fps/codec/size/duration). Merges with C5.

## Workstream C — UX parity (OpenCut-inspired)

- **C1. Actions registry (foundation, do first).** New `src/lib/actions.ts` modeled on OpenCut's `invokeAction`: `{ id, label, shortcut, isEnabled(state), run() }`. Keyboard handler (`App.tsx:42-147`), toolbar, context menu, and menus all resolve through it. Delivery vehicle for C6 and a future command palette.
- **C2. Snapping subsystem.** Starts with the BX2 bug fix, then extract from `timeline/interactions.ts` into `timeline/snap.ts` with OpenCut's source model (clip-edge / playhead / marker sources → candidates → nearest-within-**pixel**-threshold resolver); draw snap guides on the B1 overlay.
- **C3. Group operations.** Group move/trim for multi-selection in `interactions.ts`, committed as one `apply_commands` batch (backend already atomic with one undo snapshot — `lib.rs:1043-1066`). No core changes.
- **C4. Transition affordances on the timeline.** Draw chevron/overlap badges at clip junctions in `timeline/renderer.ts`; click/drag-from-`TransitionsPanel` onto a junction dispatches the existing `SetClipTransition`.
- **C5. Richer media bin + export dialog.** Media-bin half merges into G4 (audio + image assets, metadata). Export dialog: codec/bitrate-or-CRF/custom resolution/audio bitrate, backed by A8's `ExportSettings`.
- **C6. Expose orphaned backend commands.** TrackMotion/Stabilize/AutoReframe as clip context-menu actions with progress toasts; multicam group + angle switcher in inspector; ApplyTemplate/GenerateSticker/GenerateVoiceover in their panels. UI-only — commands exist and are tested in core.
- **C7. Curve keyframe editor (last).** Rebuild `inspector/KeyframeEditor.tsx` as a canvas time×value curve editor with draggable handles, reusing easing from `project/anim.rs` and the timeline theme bridge.

## Workstream D — Design system (fresh neutral palette, light + dark)

- **D1. Two-layer token architecture in `styles/tokens.css`.**
  - *Primitives (theme-agnostic HSL scales):* `--neutral-{50..950}` (slightly cool gray, zinc-like), `--accent-{50..950}` (calm professional blue ≈ hsl(215 85% 55%)), plus `--danger/--success/--warning` scales. Ember orange retires; optionally kept only as a highlight for AI-agent-driven actions.
  - *Semantic tokens* under `:root[data-theme="dark"]` and `:root[data-theme="light"]`, **keeping every existing token name** (`--bg-0..3, --text-1..3, --accent, --clip-*, --playhead, --timeline-bg`, radii, spacing) so nothing breaks, plus OpenCut-style additions (`--muted`, `--destructive`, `--border`).
  - Light theme: near-white chrome (`hsl(220 15% 97%)` family) but keep the timeline **darker than chrome even in light mode** (pro-NLE convention; validate waveform/text contrast). `color-scheme` set per theme; toggle writes `data-theme` on `<html>`, persisted, defaulting to `prefers-color-scheme`.
- **D2. Canvas theme bridge — keep and fix.** `timeline/theme.ts` reading CSS vars is the right pattern. Add cache reset + full re-render on theme change; move the hardcoded `rgba(255,255,255,…)` ruler lines and locked-hatch into tokens (wrong in light mode).
- **D3. Incremental migration of the 2047-line `globals.css`.** (1) mechanical sweep replacing raw hex/rgba with semantic tokens (grep-driven; extend the existing "no color literals in canvas code" gate to all CSS); (2) split by component area into per-area CSS files as C-workstream touches them — no Tailwind retrofit; (3) grow `src/components/ui/` inventory (Button, Slider, Tabs, Dialog, Toggle, Field; Tooltip/MenuSelect/Toast exist) and document in `docs/design-system.md`. Ship light theme behind a dev toggle until the contrast sweep completes.

## Workstream E — Live Claude-agent bridge (MCP ↔ running app)

**Current distance:** closer than expected. renderly-mcp already exposes the full command surface + perception (render_frame, transcript, scenes, silence, peaks) but is fully headless — it and the app are two processes that would silently clobber each other's writes to the same `.renderly.json` (no file watcher, no inbound channel; app state at `lib.rs:411-414`). The hard part — a single `apply_command`/`apply_commands` chokepoint with atomic undo + `project:changed` emission (`lib.rs:690+`) — already exists, so the bridge is a thin adapter.

**Recommended: local WebSocket bridge inside the Tauri app; MCP proxies to it when the app is running, falls back to today's headless file mode otherwise.** (File-watching rejected: two-writer lost updates, unmergeable undo, duplicate engine for perception. Deep-link rejected: no request/response.)

- **E1. `src-tauri/src/bridge.rs`:** tokio WebSocket JSON-RPC on `127.0.0.1:0`, random token auth, loopback only. Methods are thin wrappers over the exact functions the Tauri commands use: `apply_command(s)`, `get_project`, `undo/redo`, `render_frame`, `seek/play/pause`, `export`. Agent edits enter the **shared undo history** (human can Ctrl-Z the agent) and emit `project:changed` so the GUI updates live.
- **E2. Discovery:** app writes `{app_data_dir}/renderly/bridge.json` `{ pid, port, token, project_path }` on start, deletes on exit; MCP checks pid-liveness + project-path match per call, connects with token, else stays headless. New MCP tool `get_editor_status` → `{ live, project_path, playhead, selection }`.
- **E3. Live perception:** `render_frame` routes through the bridge to the app's FrameRenderer (the readback path A4 retains exists for exactly this) — the agent sees the same pixels the user sees. Optional `set_playhead` to direct the user's attention. Concurrency via the existing `Mutex<AppState>`; B3's revision counter lets the MCP detect staleness (nice-to-have, not required).
- **E4. Docs + tests:** bridge protocol spec in `docs/` (contract surface like command-api.md); AGENTS.md update. renderly-mcp and bridge land with tests (both crates currently have zero).

## Workstream F — Gaps vs PLAN.md (summary)

- Phases 0–3: functionally complete. Phase 4: heuristic implementations done per parity board; ML upgrades (RVM/BiRefNet) deliberately on hold — no action.
- **Hardware accel (NVDEC/NVENC/QSV) promised in PLAN.md §3 — absent everywhere** → A3/A8.
- **~Half of Phase-4 features have no GUI** (multicam, tracking, stabilize, reframe, templates, sticker, voiceover) → C6.
- Audio stack diverges from plan (FFmpeg-premix + rodio instead of cpal/symphonia/rubato) — acceptable; A6 pays down its latency debt; note divergence in PLAN.md.
- MCP HTTP transport never built — superseded by Workstream E.
- OTIO interop absent (PLAN.md §2) — log on parity board, low priority.
- Tests: core ~82, app 3, **cli 0, mcp 0** — agent-facing surfaces are untested → E4.
- Verify macOS/Linux CI build matrix (P3 exit criterion) is actually green.

---

## Ordering (respecting user priority: engine perf → frontend perf → the rest)

```
Sprint 1 (bugs + independent quick wins + baseline):
  BX1 spurious-seek fix, BX2 snapping fix, BX3 drop/auto-track verification,
  A0 perf HUD, A1 texture pool, A2 preview-res decode, A3 -hwaccel,
  B1 canvas guard + layered playhead, B2 single fetch
Sprint 2 (architecture + project lifecycle):
  A4 shared-device compose-to-surface, A5 frame cache + prefetch, A6 segmented audio,
  G1 project home screen, G2 auto-create/auto-save, G3 import-first flow, G4 asset kinds
Sprint 3:
  E1+E2 live bridge, C1 actions registry, D1 tokens (dark first, light behind flag)
Sprint 4:
  B3 JSON-Patch deltas, E3 live perception, C2(full)/C3/C5, A8 hw encode, D2/D3
Later:
  A7 GPU effects (Completed), C4 (Completed), C6 (Completed), C7 (Completed), light theme GA (Completed), ffmpeg-the-third migration
```

Hard dependencies: A4→A7(preview benefit), C1→C6, D1→D3, E1→E3, B2→B3, BX2→C2.

## Critical files

- `renderly-app/src-tauri/src/preview/playback.rs` — A2, A5, A6, A4 render loop
- `renderly-app/src-tauri/src/preview/gfx.rs` — A0, A1, A4 surface path
- `renderly-core/src/compose/mod.rs` — A4 injected device + render-target split, A1 pooling
- `renderly-core/src/media/ffmpeg_cli.rs` — A3 hwaccel, A8 ExportSettings
- `renderly-app/src-tauri/src/lib.rs` — B2/B3 chokepoint, E1 bridge wiring, Arc<Project> snapshot
- `renderly-app/src/timeline/renderer.ts` + `components/timeline/TimelineCanvas.tsx` — B1, C4
- `renderly-app/src/store/editorStore.ts` — BX1 seek removal, B2, B3 patch application
- `renderly-app/src/timeline/layout.ts` + `interactions.ts` — BX2 snapping fix, C2/C3
- `renderly-app/src/components/leftpanel/MediaPanel.tsx` + new home-screen component — G1–G4
- `renderly-app/src/styles/tokens.css` + `src/timeline/theme.ts` — D1, D2
- `renderly-mcp/src/server.rs` — E2 proxy mode
- `docs/` — command-api/bridge spec updates per AGENTS.md definition of done

## Verification

- Every sprint: `cargo build/test --workspace`, `cargo fmt --check`, `cargo clippy -- -D warnings` (AGENTS.md DoD).
- Perf: A0 HUD numbers before/after each A-step (decode ms, compose ms, present ms, frame time at 1080p60 source); target = sustained realtime playback at panel res with <50% of one core.
- Frontend: with DevTools performance trace, confirm playhead motion no longer triggers full-canvas repaint (B1) and one `get_project` (later: one patch) per edit (B2/B3).
- A2 regression guard: unit test odd-dimension sources produce stride-consistent frames.
- E: integration test — start app with a project, run renderly-mcp, `get_editor_status` reports live, `apply_command` over bridge updates GUI + is undoable via Ctrl+Z, `render_frame` returns the live frame; kill app → MCP falls back headless.
- D: theme toggle flips light/dark including canvas timeline; automated contrast check on token pairs.
- BX/G: manual repro checklist — inspector slider change must not pause/seek playback (BX1); clip dragged near another's edge at max and min zoom snaps flush (BX2); drop video/audio/image onto empty timeline creates the right track kind (BX3/G4); close app mid-edit → reopen → home screen shows the project with the edit persisted (G2).
