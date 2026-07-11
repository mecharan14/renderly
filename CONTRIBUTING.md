# Contributing to Uppercut

Thanks for considering contributing. Uppercut's Phase 2 GUI rebuild (M0–M7) is complete —
see [PLAN.md](PLAN.md) §4 and [AGENTS.md](AGENTS.md) §4. Phase 3 (effects / WASM plugins)
is next; check the current phase before starting large work.

Before a release-minded PR, walk [docs/qa-checklist.md](docs/qa-checklist.md).

## Two ways to contribute, no Rust required for either

- **Asset packs** (stickers, LUTs/filters, caption style presets, SFX, transitions,
  templates): a folder of JSON + media files, no code. Format lands in Phase 3 — see
  PLAN.md §5. Watch for the `uppercut-registry` repo once it exists.
- **Code** (engine, plugins, UI): Rust for `uppercut-core`/`uppercut-cli`/`uppercut-mcp`,
  TypeScript/Svelte for `uppercut-app`'s UI chrome, or WASM (any language) for plugins once
  the plugin SDK lands.

## Ground rules

Read [AGENTS.md](AGENTS.md) first — it's written for AI agents but the architecture rules,
repo layout, and "definition of done" checklist apply equally to human PRs. In short:

1. Every edit operation goes through the command API in `uppercut-core` — never implement
   editing logic directly in the CLI, MCP server, or app.
2. `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test --workspace` must pass.
3. If you change the project schema or command set, update `docs/project-schema.md` or
   `docs/command-api.md` in the same PR.
4. Keep PRs scoped to one roadmap item or issue — small and reviewable beats broad.

## Setup

```
git clone <repo-url>
cd video-editor
cargo build --workspace
cargo test --workspace
```

FFmpeg must be discoverable on your system for `uppercut-core` to build once media I/O
lands (Phase 0 spike) — installation instructions will be added here once that dependency
is wired up.

## Reporting issues / proposing features

Open an issue describing the problem or the roadmap item you want to pick up. For anything
that isn't already in PLAN.md's roadmap, briefly explain how it fits the project's
non-negotiables (AGENTS.md §0) before sending a large PR — saves everyone rework.

## License

By contributing, you agree your contribution is licensed under AGPL-3.0 (see LICENSE).
