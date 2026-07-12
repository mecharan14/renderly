// Browser dev harness ONLY. Aliased in place of the four @tauri-apps modules by
// vite.harness.config.ts so the real app (which needs a running Tauri backend) can be
// rendered and iterated on in a plain browser. Never imported by the production build.

import type { Project } from "../lib/types";

const sampleProject: Project = {
  schema_version: 6,
  id: "mock-project",
  name: "Ultra Bruno ep.12",
  settings: { fps: 60, width: 1080, height: 1920, sample_rate: 48000, duck_db: -12 },
  media: [
    { id: "m-round1", path: "C:/clips/round1.mp4", kind: "video", duration_secs: 42, width: 1920, height: 1080, fps: 60 },
    { id: "m-killcam", path: "C:/clips/kill-cam.mp4", kind: "video", duration_secs: 18, width: 1920, height: 1080, fps: 60 },
    { id: "m-round2", path: "C:/clips/round2.mp4", kind: "video", duration_secs: 55, width: 1920, height: 1080, fps: 60 },
    { id: "m-facecam", path: "C:/clips/facecam.mp4", kind: "video", duration_secs: 60, width: 1280, height: 720, fps: 30 },
    { id: "m-vo", path: "C:/audio/vo-take2.wav", kind: "audio", duration_secs: 65, width: null, height: null, fps: null },
    { id: "m-bgm", path: "C:/audio/bgm-loop.mp3", kind: "audio", duration_secs: 90, width: null, height: null, fps: null },
    { id: "m-logo", path: "C:/img/logo.png", kind: "image", duration_secs: null, width: 512, height: 512, fps: null },
  ],
  tracks: [
    {
      id: "t-v2", kind: "video", name: "Overlay", muted: false, locked: false, hidden: false,
      clips: [
        { type: "video", id: "c-facecam", media_id: "m-facecam", position_secs: 10, source_in_secs: 0, source_out_secs: 6.2, gain_db: 0, enabled: true, fade_in_secs: 0, fade_out_secs: 0, transform: { x: 0, y: -0.55, scale_x: 0.32, scale_y: 0.32, rotation_deg: 0, opacity: 1 } },
      ],
    },
    {
      id: "t-v1", kind: "video", name: "Gameplay", muted: false, locked: false, hidden: false,
      clips: [
        { type: "video", id: "c-round1", media_id: "m-round1", position_secs: 0, source_in_secs: 0, source_out_secs: 9.4, gain_db: 0, enabled: true, fade_in_secs: 0, fade_out_secs: 0, outgoing_transition: { kind: "crossfade", duration_secs: 0.5 } },
        { type: "video", id: "c-killcam", media_id: "m-killcam", position_secs: 9.4, source_in_secs: 2, source_out_secs: 10.3, gain_db: -3, enabled: true, fade_in_secs: 0, fade_out_secs: 0, transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation_deg: 0, opacity: 1 }, effects: [{ id: "e1", effect_id: "builtin:color_adjust", enabled: true, params: { contrast: 1.1 } }] },
        { type: "video", id: "c-round2", media_id: "m-round2", position_secs: 18, source_in_secs: 0, source_out_secs: 11.1, gain_db: 0, enabled: true, fade_in_secs: 0, fade_out_secs: 0 },
      ],
    },
    {
      id: "t-a1", kind: "audio", name: "Voiceover", audio_role: "voiceover", muted: false, locked: false, hidden: false,
      clips: [
        { type: "audio", id: "c-vo", media_id: "m-vo", position_secs: 2.5, source_in_secs: 0, source_out_secs: 18, gain_db: 0, enabled: true, fade_in_secs: 0.2, fade_out_secs: 0.4 },
        { type: "audio", id: "c-bgm", media_id: "m-bgm", position_secs: 22, source_in_secs: 0, source_out_secs: 12, gain_db: -12, enabled: true, fade_in_secs: 0, fade_out_secs: 1 },
      ],
    },
    {
      id: "t-c1", kind: "caption", name: "Captions", muted: false, locked: false, hidden: false,
      clips: [
        { type: "caption", id: "c-cap1", text: "okay so THIS run", position_secs: 3, duration_secs: 2.4, style_id: "tiktok-bold-yellow" },
        { type: "caption", id: "c-cap2", text: "was absolutely cursed", position_secs: 6, duration_secs: 3, style_id: "tiktok-bold-yellow" },
        { type: "caption", id: "c-cap3", text: "watch the corner", position_secs: 11, duration_secs: 2.2, style_id: "tiktok-bold-yellow" },
      ],
    },
  ],
  asset_pack_paths: [],
  wasm_plugin_paths: [],
};

const extensionCatalog = {
  packs: [
    { id: "neon-pack", name: "Neon Gamer Pack", path: "C:/packs/neon", stickers: [{ id: "s1", label: "Boom", default_duration_secs: 1 }, { id: "s2", label: "GG", default_duration_secs: 1.5 }], sfx: [{ id: "sfx1", label: "Whoosh" }], luts: [{ id: "lut1", label: "Neon Night" }], transitions: [], templates: [{ id: "tpl1", label: "Montage Intro" }] },
  ],
  plugins: [
    { id: "glow", name: "Glow FX", path: "C:/plugins/glow", has_frame: true, has_audio: false },
    { id: "bass", name: "Bass Boost", path: "C:/plugins/bass", has_frame: false, has_audio: true },
  ],
};

let revision = 1;

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function invoke<T = unknown>(cmd: string, _args?: any): Promise<T> {
  const reply = (v: unknown) => Promise.resolve(v as T);
  switch (cmd) {
    case "list_projects":
      return reply([
        { path: "C:/proj/ep12.renderly.json", name: "Ultra Bruno ep.12", durationSecs: 84, modifiedMs: Date.now() - 7.2e6, width: 1080, height: 1920, fps: 60 },
        { path: "C:/proj/montage.renderly.json", name: "ranked grind montage", durationSecs: 58, modifiedMs: Date.now() - 9e7, width: 1080, height: 1920, fps: 60 },
        { path: "C:/proj/recap.renderly.json", name: "tourney recap", durationSecs: 192, modifiedMs: Date.now() - 3.4e8, width: 1920, height: 1080, fps: 30 },
      ]);
    case "get_project":
      return reply({ project: sampleProject, revision });
    case "open_project":
    case "quick_start_project":
    case "new_project":
      return reply("C:/proj/ep12.renderly.json");
    case "list_extensions":
      return reply(extensionCatalog);
    case "list_registry":
      return reply([
        { id: "retro-pack", kind: "pack", summary: "Retro CRT filters and stickers", schema_version: 1, resolved_path: "C:/reg/retro" },
        { id: "denoise-pro", kind: "plugin", summary: "Studio-grade audio denoise", schema_version: 1, resolved_path: "C:/reg/denoise" },
      ]);
    case "apply_command":
    case "apply_commands":
      revision += 1;
      return reply({ revision, patch: [], outcome: "ok", outcomes: ["ok"] });
    case "undo":
    case "redo":
      revision += 1;
      return reply({ can_undo: true, can_redo: true, revision, patch: [] });
    case "get_media_assets":
      return reply({});
    case "pause":
      return reply(0);
    case "project_thumbnail":
      return reply(null);
    default:
      return reply(undefined);
  }
}

export function convertFileSrc(path: string): string {
  return path;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function listen<T = unknown>(_event: string, _cb: (e: { payload: T }) => void): Promise<() => void> {
  return Promise.resolve(() => {});
}

const noop = () => Promise.resolve();
const noopUnlisten = () => Promise.resolve(() => {});

export function getCurrentWindow() {
  return {
    minimize: noop,
    maximize: noop,
    unmaximize: noop,
    toggleMaximize: noop,
    close: noop,
    startDragging: noop,
    isMaximized: () => Promise.resolve(false),
    scaleFactor: () => Promise.resolve(1),
    innerPosition: () => Promise.resolve({ x: 0, y: 0 }),
    innerSize: () => Promise.resolve({ width: 1600, height: 950 }),
    onResized: noopUnlisten,
    onMoved: noopUnlisten,
    onScaleChanged: noopUnlisten,
  };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function open(_opts?: any): Promise<string | null> {
  return Promise.resolve(null);
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function save(_opts?: any): Promise<string | null> {
  return Promise.resolve(null);
}
