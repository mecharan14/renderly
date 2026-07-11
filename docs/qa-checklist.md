# Manual QA checklist (GUI rebuild M7)

Run after a fresh clone (`npm i && npm run tauri dev` from `renderly-app/`). Mark each
item pass/fail. Failures that crash the app or freeze the window are blockers.

## Smoke

- [ ] Fresh clone: `cargo build --workspace`, `cargo test --workspace`, `npm i && npm run tauri
      dev` start without errors
- [ ] Window opens ~1600×950; cannot shrink below ~1200px wide
- [ ] Welcome screen → **Import video to start** creates a project and places the clip

## Edit path

- [ ] Import a second clip (drop or Media panel); both appear in the bin
- [ ] Video bin cards show a shimmer skeleton, then a hover-scrub filmstrip
- [ ] Drag a clip onto the timeline; move / trim / snap; undo (Ctrl+Z) / redo (Ctrl+Y)
- [ ] Split (S or razor tool C); copy / paste / duplicate (Ctrl+C/V/D)
- [ ] Mute / lock / hide a track; rename via double-click on the header
- [ ] Aspect ratio menu: switch 9:16 ↔ 16:9; **Original** restores first video dims
- [ ] Editable timecode + ±1 frame; Space play/pause; Esc exits fullscreen preview

## Captions & audio

- [ ] Text panel → add caption; style gallery updates burn-in look
- [ ] Auto-captions (Whisper available) or graceful error toast if missing
- [ ] Audio panel → TTS voiceover (Piper or OpenAI key); clip lands on an audio track

## Export (M6)

- [ ] Export dialog: TikTok / YouTube / Project size presets
- [ ] Progress bar + ETA while rendering; Cancel mid-export cleans up (no orphan temp MP4)
- [ ] Completed export plays in an external player

## Stability

- [ ] Play a ≥60s 1080p clip and **drag the window** — UI stays responsive; ffmpeg process
      count stays flat (PlaybackEngine, not per-frame spawn)
- [ ] Edit while playing pauses playback cleanly (no stuck audio)
- [ ] Kill `ffmpeg` mid-play → toast via `playback:error`, app does not crash
- [ ] Stickers / Effects / Transitions / Filters / Adjust / Extensions panels work (no Coming Soon stubs)
- [ ] Empty timeline / empty media / empty audio show clear empty states
- [ ] Stickers place on a Stickers overlay track without overlap errors on Video 1
- [ ] Extensions can Load the starter pack and gain/invert plugins from the local registry

## Phase 4 (as features land)

- [ ] Clip mask / chroma key / background removal affect preview and export alpha
- [ ] **Mask tool (M):** paused preview — drag to create rect (Shift=ellipse, Alt=from
      center); move/resize handles; inspector Enable / Invert / Feather / shape switch;
      undo after mouseup commit (live drag does not spam undo); play hides overlay;
      Select (V) restores transform handles; Windows preview click-through still works
- [ ] Audio denoise (afftdn) on an audio-track clip survives export
- [ ] Templates apply via command; multicam angle switch changes active angle
- [ ] **Linux Wayland:** native preview aligns to the letterboxed `#preview-host` rect;
      transport/mask overlays stay clickable (click-through); resize realigns (may lag one
      GTK frame); X11 session still works as before

## Shortcuts quick check

| Key | Action |
|---|---|
| Space | Play / pause |
| V / C / M | Select / razor / mask tool |
| S | Split at playhead |
| ← / → | Step ±1 frame |
| Ctrl+Z / Y | Undo / redo |
| Ctrl+S | Save |
| Ctrl+C / V / D | Copy / paste / duplicate |
| Delete | Delete selection |
| Esc | Exit fullscreen preview / close dialog |
