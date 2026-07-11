# Feature parity board (Phase 4)

Public checklist so contributors can pick independently testable slices. Each card
links schema/commands/fixtures. Check items when they ship with docs + core tests +
GUI/CLI/MCP reachability.

| ID | Feature | Status | Spec / entry points |
|----|---------|--------|---------------------|
| P4-0 | Phase 3 baseline landed; architecture status current | done | [architecture.md](architecture.md) |
| P4-1a | Shared `ClipMask` + compositor alpha path | done | schema v6; `SetClipMask`; `renderly-core/src/mask.rs` |
| P4-1b | Local background removal (heuristic + optional `RENDERLY_SEG_CLI`) | done | `SetClipBackgroundRemoval`, `GenerateBackgroundMatte`; `segmentation/` |
| P4-2 | Audio denoise (`afftdn`) on audio-track clips | done | `SetClipAudioDenoise`; audio-track-only v1 |
| P4-3 | Chroma key builtin | done | `builtin:chroma_key`; Effects panel |
| P4-3b | Shape mask authoring (rect/ellipse UI polish) | done | Mask tool (M); `PreviewMaskOverlay`; inspector enable/invert/feather/shape; `preview_mask_override` |
| P4-4 | Motion tracking → keyframes | done | `TrackMotion` |
| P4-4b | Auto-reframe | done | `AutoReframeClip` |
| P4-5 | Stabilization | done | `StabilizeClip` (centroid-smoothed keyframes) |
| P4-6 | Templates | done | pack `templates[]` + `ApplyTemplate` |
| P4-6b | Text-to-sticker | done | `GenerateSticker` (local generated PNG; not an external image model) |
| P4-7 | Multi-cam groups + angle switch | done | `CreateMulticamGroup`, `SetMulticamAngle`; export skips inactive angles |
| P4-ML | Core-hosted ONNX RVM/BiRefNet | open | Design gate: CLI/heuristic first; ONNX needs license review |
| P4-Wayland | Linux Wayland native preview | done | `wl_subsurface` + empty input region; runtime X11/Wayland dispatch; no Linux CI runner yet |

## Inference runtime decision (recorded)

Current WASM frame plugins instantiate per call and copy full RGBA — unsuitable for
RVM/BiRefNet. Phase 4.1 uses **heuristic mattes** plus optional **`RENDERLY_SEG_CLI`**
(Whisper-style). Linking `ort` / ONNX remains a follow-up after AGPL-compatible license
review — never as a user-loadable native DLL plugin.
