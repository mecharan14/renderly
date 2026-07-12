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
- **P1 — Decode + blit (no effects).** `<video>`-fed canvas preview behind
  `RENDERLY_WEBVIEW_PREVIEW=1`; cuts-only timelines play (multi-clip switching, seek,
  scrub). Success: 1080p60 zero-drop playback on the QA machine, scrub latency ≤ native.
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
