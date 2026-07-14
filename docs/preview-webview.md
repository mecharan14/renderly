# Webview preview migration (WASM compositor)

Status: approved 2026-07-12 (supersedes the "native child-window preview" rule; AGENTS.md §0.5
updated in the same change). This doc is the contract for the migration.

## Why

Measured on generated 1080p60 H.264 @ 51 Mbps footage in the app-equivalent Chromium engine:

| Path | Presented fps | Dropped | Per-frame paint cost |
|---|---|---|---|
| Browser pipeline → Canvas2D | 60.1 | 0 / 1544 | 0.11 ms |
| Browser pipeline → WebGL texture | 60.0 | 0 / 1573 | 0.12 ms |

The browser gives hardware decode, zero-copy compositing, and frame scheduling for free.
The native path (FFmpeg CLI → RGBA pipe → wgpu → present) pays CPU decode + pipe copies +
GPU↔CPU round-trips, and carries four per-OS child-window backends
(`renderly-app/src-tauri/src/preview/{win32,macos,linux/x11,linux/wayland}.rs`) plus the
bounds-sync fragility of OS-composited child windows. Direct manipulation (handles, masks,
guides) also becomes first-class DOM instead of overlays floating over a foreign window.

## Non-negotiables

1. **One compositor.** The preview uses `renderly-core`'s compositor compiled to `wasm32`,
   fed by browser-decoded frames. Never re-implement effects in JS — that forks
   preview↔export parity. Export and headless (CLI/MCP `render_frame`) keep the native engine.
2. **Command API and engine boundaries are unchanged.** This migration touches only how
   pixels reach the screen in `renderly-app`.
3. **Native preview stays as a runtime fallback** (env/flag) until the webview path reaches
   parity on the QA checklist, then the child-window backends are deleted in one PR.

## Architecture

```
media file ──► <video> / WebCodecs VideoDecoder (HW decode, browser-scheduled)
                    │  VideoFrame (GPU-backed)
                    ▼
    importExternalTexture / texImage2D  ──►  renderly-core compositor (wasm32 + WebGPU)
                    │                          effects/transitions/masks = same WGSL
                    ▼
              <canvas> in the editor layout (plain DOM sibling of handles/overlays)
```

- **Decode**: start with `<video>` elements per active clip + `requestVideoFrameCallback`
  (simplest, browser-scheduled); move seeking-critical paths to `WebCodecs`
  (`VideoDecoder` + mp4 demux) where frame-accurate stepping needs it. Audio stays on the
  existing rodio path initially (backend clock), then migrates to WebAudio when the webview
  owns playback end-to-end.
- **Compositor**: new crate feature `renderly-core/wasm-compositor` gating a `wasm32`
  build of `compose/` (wgpu → WebGPU). The `media/ffmpeg_cli`, `plugins` (wasmtime),
  `captions` STT/TTS, and `perceive` modules are **excluded** on wasm32 (feature-gated) —
  the wasm build takes decoded RGBA/external textures as input, nothing else.
- **Bindings**: `wasm-bindgen` interface `WasmCompositor { new(canvas), set_project(json),
  render(time, frames: [(clip_id, VideoFrame|texture)]) }` in a new `renderly-wasm` crate
  (keeps wasm-bindgen out of core's default build).
- **Clock**: webview owns the playback clock for preview (rVFC-driven); the backend stays
  authoritative for export timing. `playback:tick` events reverse direction (frontend →
  store) or disappear entirely once preview is local.

## Phases

- **P1a — Paused frame in a DOM canvas (attempted 2026-07, REVERTED).** Tried to fix the
  handles-occlusion bug by hiding the native surface and rendering the composited frame to
  a webview `<img>` via a new `render_preview_frame(time) -> PNG data URL` command (reused
  `render_live_frame_png`), so the DOM transform/mask handles would compose above it
  instead of being occluded. It worked structurally (verified z-order in the harness) but
  was **not** the PoC's fast path — the PoC's 60fps came from the *browser* decoding video;
  this instead round-tripped through the engine (FFmpeg decode → composite → PNG-encode →
  base64 → IPC → webview PNG-decode) and reopened the decoder per call, so it was slow even
  after narrowing the trigger to "only while transform-editing a clip." Reverted at the
  user's request rather than layer more fixes onto a stopgap; the handles-occlusion bug is
  **still open**. Do not reintroduce a PNG-per-frame bridge as the fix — go straight to real
  P1 (`<video>`/WebCodecs decode in the webview) so speed and the occlusion fix land
  together. If a stopgap is wanted again before P1, consider a persistent/cached decoder
  server-side (avoid reopening FFmpeg per call) rather than PNG round-trips.
- **P1 — Decode + blit (no effects). IMPLEMENTED 2026-07-15.** `<video>`/`<img>`-fed
  `<canvas>` preview, default ON (opt back to native via
  `localStorage["renderly.previewEngine"] = "native"`, checked by `isWebviewPreview()` in
  `renderly-app/src/preview/webviewPreviewEngine.ts`). Cuts-only timelines play: multi-clip
  switching, seek/scrub, transforms, opacity, speed, and multi-track layering (images
  included) all render locally in the webview, mirroring `renderly-core`'s timeline
  evaluation and `compose/mod.rs`'s cover-fit + user-transform math exactly (never a
  from-scratch reimplementation — see the Non-negotiables above).

  **What was built:**
  - `renderly-app/src/preview/webviewPreviewEngine.ts` — `WebviewPreviewEngine`: a pooled
    `<video>`/`<img>` element per media item, a timeline evaluator mirroring
    `active_layers` (renderly-core/src/export/mod.rs) — tracks iterated in array order,
    first = bottom, per `compose/mod.rs`'s `composite` doc comment — a cover-fit + user
    transform draw (`coverSourceRect` + `drawLayer`, a pixel-space mirror of `cover_uv`/
    `layer_params` in `compose/mod.rs`), and a dual-mode clock: `playback:tick` drift-slews
    a local `requestAnimationFrame` clock when ticks arrive (real app), or the pure rAF
    clock drives everything standalone (browser harness, no backend). Canvas backing-store
    resize follows the same "only reassign width/height when it actually changed" rule as
    `timeline/renderer.ts`'s `syncCanvasSize`.
  - `renderly-app/src/preview/WebviewPreview.tsx` — the `<canvas>` component, positioned at
    the letterboxed content rect (same math as `PreviewHandlesOverlay`'s
    `contentBoundsInHost`), `z-index: 0` (below the handles/mask overlays at 5/6) — this is
    the actual occlusion-bug fix: handles are now plain DOM siblings painted after the
    canvas in z-order, not an OS-composited window on top of everything.
  - `PreviewPanel.tsx` renders `<WebviewPreview/>` instead of calling
    `ipc.setPreviewBounds` when `isWebviewPreview()` (so the native child window is never
    created); `PreviewHandlesOverlay`/`PreviewMaskOverlay` skip their backend
    `previewTransformOverride`/`previewMaskOverride` round trips in webview mode — the
    canvas subscribes to the store directly and redraws on every optimistic patch instead.
  - Backend: new `set_preview_mode(mode: "webview"|"native")` command
    (`renderly-app/src-tauri/src/lib.rs`), backed by `PlaybackEngine::webview_mode`
    (`AtomicBool`, `src-tauri/src/playback.rs`). In webview mode, `run_playback_loop` never
    constructs a `FrameRenderer` and does audio-only work (premix + `playback:tick`
    pacing); `play`/`seek`/`refresh_frame` skip `ensure_preview_parent` and the native
    scrub-worker render paths entirely (playhead bookkeeping still happens). The frontend
    calls `set_preview_mode` once at startup (`App.tsx`) from `isWebviewPreview()`.
  - Asset protocol: `open_project` and `apply_command`'s `MediaImported` outcome now call
    `app.asset_protocol_scope().allow_file(&media.path)` for every media item (helper
    `allow_media_assets` in `lib.rs`) so the webview's `<video src>`/`<img src>`
    (`convertFileSrc`) can actually load arbitrary project media, not just
    `$APPCACHE/media-cache/*`.
  - Harness upgrades (`renderly-app/src/dev/tauriMock.ts`,
    `renderly-app/vite.harness.config.ts`): a tiny `listen`/`emit` event bus; mock
    `play`/`pause` drive a ~33 Hz `playback:tick`/`playback:state` simulation; two real
    1920×1080@60 H.264 clips generated with ffmpeg
    (`testsrc2`/`smptebars`, 12s each) into a gitignored `renderly-app/dev-assets/`
    (harness-only `publicDir`, never touches `vite.config.ts` or ships in `dist`); the
    mock's `convertFileSrc` maps the sample project's fake media paths onto those two
    files so real browser decode is exercised end-to-end.

  **Measured in the browser harness** (`npx vite --config vite.harness.config.ts --port
  5188`, verified via the `javascript_tool`/DOM — the harness's browser-automation tab
  reports `document.hidden === true` even when fronted, which throttles
  `requestAnimationFrame`; `WebviewPreviewEngine` falls back to a `setTimeout`-paced loop
  only while `document.hidden`, purely so this harness can measure sustained playback at
  all — production/foreground windows always use rAF):
  - Canvas position: correctly sized/positioned at the letterboxed content rect
    (`getBoundingClientRect` matched `contentBoundsInHost`'s math), `z-index: 0`, below
    `.preview-handles-overlay` at `z-index: 5` — occlusion fix verified structurally.
  - Sustained playback over 6s: **fps ≈ 33.3, per-frame draw cost (canvas draw work,
    excluding scheduling) ≈ 0.8 ms, drops ≈ 189/200 frames** by the engine's own
    `window.__previewStats`. The fps/drop numbers reflect the harness tab's timer
    throttling (measured interval ≈2× the requested 16 ms, i.e. an environment artifact of
    this specific automated browser pane, not the engine) — they are **not** a clean 60fps
    measurement and should not be read as such. The one number that *is* environment-
    independent, per-frame draw cost (~0.8 ms for clearRect + fillRect + N-layer
    seek-correction + `drawImage`), is well inside a 16.7 ms/60fps budget and consistent
    with the PoC's ~0.11 ms Canvas2D `drawImage` figure (ours is higher because it
    includes per-frame layer bookkeeping the PoC's isolated benchmark didn't).
    **Foreground confirmation: 2026-07-15, the project owner ran the real desktop app and
    confirmed playback speed meets expectations** ("fast as I was expecting"), closing the
    gap the automated harness couldn't cover (its pane reports `document.hidden`, which
    throttles rAF). The formal QA-checklist numbers (4K60, 3-track, scrub-storm) below
    remain to be captured, but the P1 perf gate is considered met for 1080p60.
  - Scrub: sampled a pixel before/after seeking across the `c-round1`→`c-killcam` cut
    boundary (t=2 → t=10, testsrc2 vs smptebars source clips) — pixel value changed
    (`[243,233,103]` → `[0,161,0]`), confirming the canvas actually repaints on scrub.
  - Multi-clip cut during playback: played from t=8.5 across the t=9.4 cut to t≈11.5;
    sampled pixel matched the *next* clip's content, confirming gapless in-playback
    switching.
  - Transform redraw while paused: isolated the overlay clip (hid the covering track),
    patched its `ClipTransform` directly on the store (mirroring
    `PreviewHandlesOverlay`'s optimistic drag patch) with no `playing` and no backend
    call — canvas content changed immediately (`notifyProjectPatched`'s store-subscription
    redraw), confirming local full-rate transform preview.
  - Native fallback: `localStorage["renderly.previewEngine"] = "native"` reload correctly
    suppressed the webview canvas while leaving `PreviewHandlesOverlay` (native path)
    unaffected.

  **P1 scope limits (accepted, per the Non-negotiables above):** effects, LUTs, masks,
  chroma key, captions, and keyframed animation do **not** render in the webview preview
  yet — they're P2's WASM-compositor work. A clip with any of those active still shows its
  raw cover-fit + transform content in the webview preview (no visible error, just no
  effect); the native fallback remains the way to preview those until P2 lands. Audio
  stays entirely on the existing backend/rodio path (`playback:tick` is audio-clock-driven
  regardless of preview mode) — unaffected by this migration.
- **P2 — WASM compositor.** Feature-gate core (`wasm-compositor`), compile `compose/` to
  wasm32/WebGPU, wire transforms/opacity/transitions/effects/masks/chroma/LUTs. Success:
  pixel-diff preview vs `render_frame` export readback ≤ 1 ULP-ish tolerance on the
  effects test project.
- **P3 — Audio + full switchover.** WebAudio mixing (or keep backend audio if simpler),
  captions burn-in via the same WGSL text path, remove `set_preview_bounds`/child-window
  sync from the hot path, default the flag on.
- **P4 — Deletion.** Remove `preview/{win32,macos,linux,stub}.rs`, `present_rgba`, and the
  bounds-sync effect; simplify `playback.rs` to export/scrub-server duties.

## Risks

- **WebKitGTK (Linux)**: WebGPU/WebCodecs lag Chromium. Mitigation: WebGL2 fallback for the
  compositor's final blit is not acceptable for effects parity — instead keep the native
  fallback alive on Linux until WebKitGTK ships WebGPU, or bundle a Chromium-based webview.
- **wasm32 build of wgpu/compose**: naga/WebGPU limits (no `Rgba8UnormSrgb` storage, texture
  limits). Mitigation: the compositor already targets downlevel wgpu; CI adds a
  `cargo check --target wasm32-unknown-unknown -p renderly-core --features wasm-compositor`.
- **Memory**: VideoFrames must be `close()`d promptly; leak = decoder stall. Enforce in the
  player abstraction, not call sites.

## QA gate (flip default / delete native)

The existing `docs/qa-checklist.md` preview section, plus: 4K60 source on the dogfood
machine, 3 simultaneous video tracks + 2 effects at 60fps, scrub-storm test (drag the
playhead for 10s), export pixel-parity suite green.
