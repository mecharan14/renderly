<!-- Keep PRs scoped to one roadmap/parity-board item or issue. -->

## What and why

<!-- What changes, and what problem it solves. Link the issue or parity-board ID. -->

## How it was verified

<!--
Not "it compiles" — what did you actually observe? For UI/preview work, the browser
harness (`npx vite --config vite.harness.config.ts`) with `window.__store` /
`window.__previewStats` is the fastest evidence. Screenshots or a clip for visual changes.
-->

## Checklist

- [ ] Edit operations go through the command API in `renderly-core` (nothing editing-shaped in the CLI, MCP, or app)
- [ ] No effect reimplemented in JavaScript — preview and export still share one compositor
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
- [ ] Frontend touched: `npx tsc --noEmit` and `npm run build` from `renderly-app/`
- [ ] Compositor touched: `npm run build:wasm` and `cargo check --target wasm32-unknown-unknown -p renderly-core -p renderly-wasm`
- [ ] Schema or command-set changes update `docs/project-schema.md` / `docs/command-api.md` in this PR
