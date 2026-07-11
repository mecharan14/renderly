# Renderly extension registry (seed)

Curated, machine-readable index of example asset packs and WASM plugins that ship with
this repository. See [`index.json`](index.json).

Community contributions: open a PR adding an entry that points at a pack/plugin directory
with a valid `pack.json` / `plugin.json`. A future dedicated `renderly-registry` GitHub
repo may host remote URLs; until then, this file is the local seed.

## `index.json` fields

| Field | Notes |
|-------|--------|
| `id` | Unique slug |
| `kind` | `pack` or `plugin` |
| `path` | Repo-relative path (preferred for in-tree examples) |
| `git_url` | Optional remote clone URL (future) |
| `summary` | One-line description |
| `schema_version` | Manifest schema for this entry (`1` today) |

CI validates that `index.json` parses and that listed local `path` entries exist.

## Load in the app

Use the **Extensions** left-rail tab: browse loaded packs/plugins, add folders, or Load from
this registry when running from a repo checkout.

CLI / commands: `LoadAssetPack` / `LoadWasmPlugin` with the directory path.

## Specs

- [Asset packs](../../docs/asset-pack.md)
- [Plugin API](../../docs/plugin-api.md)
