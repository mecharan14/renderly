# Asset packs (Phase 3)

Declarative, no-code extensions. A pack is a directory containing `pack.json` plus assets.

## Layout

```
my-pack/
  pack.json
  luts/
    film.cube
  stickers/
    star.png
  sfx/
    blip.wav
```

## `pack.json` (v1)

```jsonc
{
  "id": "my-pack",          // unique slug; used in effect ids
  "name": "My Pack",
  "version": "1",
  "caption_styles": [ /* optional */ ],
  "luts": [
    { "id": "film", "label": "Film", "path": "luts/film.cube" }
  ],
  "transitions": [
    // Aliases of builtin TransitionKind only — no custom shaders in v1
    { "id": "quick", "label": "Quick wipe", "kind": "wipe_left", "default_duration_secs": 0.35 }
  ],
  "stickers": [
    { "id": "star", "label": "Star", "path": "stickers/star.png", "default_duration_secs": 3.0 }
  ],
  "sfx": [
    { "id": "blip", "label": "Blip", "path": "sfx/blip.wav" }
  ],
  "templates": [
    {
      "id": "cold-open",
      "label": "Cold open",
      "commands": [
        // Serialized Command values; AddClip.position_secs is offset by ApplyTemplate
      ]
    }
  ]
}
```

### Caption styles

Fields: `id`, `label`, `font_scale` (relative), `fill_rgba`, optional `stroke_rgba` /
`shadow_rgba` / `box_rgba`, `anchor` (`top`|`center`|`bottom`). Resolved after builtins
when burning captions.

### LUTs

IRIDAS `.cube` (`LUT_3D_SIZE`). Applied on the CPU before GPU upload. Effect id:

`pack:<pack_id>:lut:<lut_id>`

Params: `intensity` (0..1, default 1).

### Stickers

PNG (or other image) placed as an image `MediaClip` on a video track (no new clip type).
Command: `AddStickerFromPack { pack_id, sticker_id, track_id, position_secs }` — imports
the asset into the media pool if needed, then `AddClip`.

### SFX

Audio file placed as an audio `MediaClip`. Command: `AddSfxFromPack { pack_id, sfx_id,
track_id, position_secs }`.

### Templates (Phase 4)

Named command sequences. Each entry has `id`, `label`, and `commands` (JSON `Command`
objects). `ApplyTemplate { pack_id, template_id, position_secs }` deserializes each
command, adds `position_secs` to every `AddClip.position_secs`, and applies them
sequentially — rolling back the project clone on any failure.

## Commands

- `LoadAssetPack { path }` — validate + record path on the project
- `UnloadAssetPack { pack_id }`
- `AddStickerFromPack` / `AddSfxFromPack`
- `ApplyTemplate`

See example: [`examples/packs/starter`](../examples/packs/starter).
Registry seed: [`examples/registry/index.json`](../examples/registry/index.json).
