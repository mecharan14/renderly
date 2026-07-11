# Claude Code instructions for Renderly

Read [AGENTS.md](AGENTS.md) — it is the full contract for working in this repo (architecture
rules, repo layout, definition of done, current phase) and applies to Claude Code exactly as
it does to any other agent. This file only adds Claude-Code-specific notes.

- Product vision and roadmap: [PLAN.md](PLAN.md).
- Specs to implement against: [docs/project-schema.md](docs/project-schema.md),
  [docs/command-api.md](docs/command-api.md), [docs/architecture.md](docs/architecture.md),
  [docs/mcp-agent-guide.md](docs/mcp-agent-guide.md).
- This repo is the eventual *target* of the MCP server described in PLAN.md — until
  `renderly-mcp` exists and is wired up, drive the project through `renderly-cli` instead.
- Use TaskCreate/TaskUpdate to track multi-step work against the roadmap phases in PLAN.md
  §4 rather than inventing an unrelated task structure.
