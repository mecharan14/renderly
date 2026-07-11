# Agent instructions for Uppercut

This file is the contract for any coding agent (Claude Code, or otherwise) working in this
repository. Read this before writing code. The full product vision, roadmap, and rationale
live in [PLAN.md](PLAN.md) — read it too if you haven't. This file is the *enforceable*
subset: the rules that keep independent contributors (human or AI) from drifting apart.

`CLAUDE.md` in this repo just points here — there is one set of rules, not two.

## 0. Non-negotiable architecture decisions

These were deliberately chosen in PLAN.md and are **not up for re-litigation** in a normal
PR. If one of these seems wrong, say so explicitly and ask — do not quietly work around it.

1. **One command API for everyone.** All edits to a project are expressed as commands
   defined in `uppercut-core` (spec: [docs/command-api.md](docs/command-api.md)). The GUI,
   the MCP server, and the CLI are all thin dispatchers over the same command enum. Never
   implement an edit operation directly against project state in the app, CLI, or MCP crate
   — add or extend a command in `uppercut-core` and call it from there.
2. **The engine never depends on the UI.** `uppercut-core` must build and be fully testable
   with zero UI dependencies (no Tauri, no windowing, no webview types). Data flows
   core → {cli, mcp, app}, never the other direction.
3. **Text-first, versioned project format.** Project state is serde-JSON
   (spec: [docs/project-schema.md](docs/project-schema.md)), documented, and diffable.
   Any schema change bumps the schema version and updates the doc in the same PR.
4. **AI needs perception, not just control.** The MCP surface is edit commands *plus*
   read-only perception tools (render frame at time T, transcript, scene/silence detection,
   waveform peaks). Don't add an edit-only tool without asking whether a perception
   counterpart is needed.
5. **Native video preview, not webview.** In `uppercut-app`, the playback/preview surface is
   a native wgpu surface embedded in the window — frames never round-trip through the
   webview/JS bridge. Webview is chrome only (panels, dialogs, inspectors).
6. **Plugins are sandboxed WASM (code) or declarative asset packs (no code).** Never add a
   plugin mechanism that loads native dynamic libraries or unsandboxed scripts.
7. **No paywalled or cloud-only-by-default features.** Local models (Whisper, local TTS)
   are the default path; cloud AI services are opt-in via user-supplied API keys. Nothing
   in this project ever requires a subscription to use.
8. **License is AGPL-3.0.** Keep it. Don't add dependencies whose license is incompatible
   with AGPL-3.0 distribution without flagging it first.
9. **Never market or document this project as CapCut-affiliated.** "Open-source alternative
   to paywalled editors" — not a clone, not affiliated, not endorsed.

## 1. Repo layout

```
uppercut-core/     headless Rust engine: project model, command API, media I/O,
                   compositing, captions/TTS/STT integration. No UI deps. Owns docs/project-schema.md
                   and docs/command-api.md as its contract.
uppercut-cli/      thin binary: load/apply-commands/save/export via uppercut-core. Used for
                   scripting, testing, and as the simplest possible agent-drivable surface.
uppercut-mcp/      MCP server exposing uppercut-core commands + perception tools over stdio/HTTP.
uppercut-app/      Tauri 2 desktop app. Rust backend calls uppercut-core; frontend (src/) is
                   the webview UI chrome; native preview surface is separate from the webview.
docs/              specs that are the source of truth: schema, command API, architecture.
PLAN.md            product vision, roadmap, tech stack rationale. Not a spec — read docs/ for that.
AGENTS.md          this file.
```

Until a crate exists yet in an early phase, treat its section of `docs/` as the spec to
implement against — write the doc before or alongside the code, not after.

## 2. Definition of done

Before considering a change complete:

- `cargo build --workspace` and `cargo test --workspace` pass.
- `cargo fmt --check` and `cargo clippy --workspace -- -D warnings` are clean.
- If you touched the project schema or command set, `docs/project-schema.md` /
  `docs/command-api.md` are updated in the same change, and the schema version bumped if
  the change is breaking.
- New commands in `uppercut-core` have at least one unit test that applies them to a
  minimal project and asserts the resulting state.
- No feature is reachable only through the GUI — if the GUI can do it, a command exists
  that the CLI/MCP can also invoke.

## 3. Conventions

- Rust edition/toolchain: see root `Cargo.toml` / `rust-toolchain.toml` once scaffolded —
  don't introduce a second one.
- Errors: use `thiserror` for library error types in `uppercut-core`; avoid `unwrap()`/
  `expect()` outside tests and `main()`.
- Keep `uppercut-core` free of `println!`/logging side effects in library code; use the
  `tracing` crate if instrumentation is needed.
- Prefer small, focused PRs that map to one roadmap item in PLAN.md §4 or one task in the
  active task list — avoid bundling unrelated refactors with feature work.

## 4. Current phase

Check PLAN.md §4 for the phase roadmap. **Phase 4 is in progress** (schema v6: masks,
background removal, chroma, audio denoise, tracking/stabilize/reframe, templates,
multicam). See [docs/parity-board.md](docs/parity-board.md).

## 5. When in doubt

If a request conflicts with §0, or a spec in `docs/` doesn't cover the case you're
implementing, stop and ask rather than guessing — these documents exist so independent
agents converge on the same design instead of diverging.
