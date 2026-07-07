import "./styles.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { Clip, EditorState, Project, Track } from "./types";
import { CAPTION_STYLES } from "./types";
import { btn, fileName, formatTimecode, toast } from "./ui";

const TRACK_LABEL_W = 88;
const RULER_H = 24;

const state: EditorState = {
  project: null,
  projectPath: null,
  playheadSecs: 0,
  selected: null,
  isPlaying: false,
};

let playTimer: number | null = null;
let timelineCanvas: HTMLCanvasElement | null = null;
let timelineCtx: CanvasRenderingContext2D | null = null;
let pixelsPerSec = 80;
let welcomeEl: HTMLElement | null = null;
let playBtn: HTMLButtonElement | null = null;
let timecodeEl: HTMLElement | null = null;
let exportPreset = "tiktok";

type TimelineTool = "select" | "razor";
let timelineTool: TimelineTool = "select";
let snapEnabled = true;
const TRIM_HANDLE_PX = 8;
const SNAP_THRESHOLD_PX = 10;
const MIN_ZOOM = 25;
const MAX_ZOOM = 320;

const MEDIA_OPEN_FILTERS = [
  { name: "Video", extensions: ["mp4", "mov", "mkv", "webm", "avi"] },
  { name: "Audio", extensions: ["mp3", "wav", "m4a", "aac"] },
  {
    name: "All media",
    extensions: ["mp4", "mov", "mkv", "webm", "avi", "mp3", "wav", "m4a", "aac"],
  },
];

let importBusy = false;
let coachMarkEl: HTMLElement | null = null;
let durationHintEl: HTMLElement | null = null;

type DragState =
  | { mode: "move"; trackId: string; clipId: string; startX: number; origPos: number }
  | {
      mode: "trim-left";
      trackId: string;
      clipId: string;
      startX: number;
      origIn: number;
      origOut: number;
      origPos: number;
    }
  | {
      mode: "trim-left-caption";
      trackId: string;
      clipId: string;
      startX: number;
      origPos: number;
      origDur: number;
    }
  | {
      mode: "trim-right";
      trackId: string;
      clipId: string;
      startX: number;
      origOut: number;
    };

let dragState: DragState | null = null;

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

function isMediaPath(path: string): boolean {
  return /\.(mp4|mov|mkv|webm|avi|mp3|wav|m4a|aac)$/i.test(path);
}

function projectHasClips(project: Project | null): boolean {
  return !!project?.tracks.some((t) => t.clips.length > 0);
}

function setImportBusy(busy: boolean) {
  importBusy = busy;
  document.getElementById("preview-host")?.classList.toggle("loading", busy);
}

function dismissCoachMark() {
  coachMarkEl?.remove();
  coachMarkEl = null;
}

function showCoachMark(message: string) {
  dismissCoachMark();
  const host = document.getElementById("preview-host");
  if (!host) return;
  coachMarkEl = el("div", "coach-mark");
  coachMarkEl.textContent = message;
  host.appendChild(coachMarkEl);
  requestAnimationFrame(() => coachMarkEl?.classList.add("show"));
  const dismiss = () => dismissCoachMark();
  playBtn?.addEventListener("click", dismiss, { once: true });
  window.setTimeout(dismiss, 8000);
}

async function ensureProjectReady(): Promise<boolean> {
  if (state.project) return true;
  try {
    setImportBusy(true);
    const path = await invoke<string>("quick_start_project");
    await loadProjectFromPath(path);
    await ensureStarterTracks(true);
    return true;
  } catch (e) {
    showError("Could not start project", e);
    return false;
  } finally {
    setImportBusy(false);
  }
}

async function finishFirstClipExperience(mediaPath: string) {
  state.playheadSecs = 0;
  if (state.project) {
    for (const track of state.project.tracks) {
      const clip = track.clips[0];
      if (clip) {
        state.selected = { trackId: track.id, clipId: clip.id };
        break;
      }
    }
  }
  render();
  await syncPreviewBounds();
  await refreshPreview();
  showCoachMark("Press ▶ or Space to preview your clip");
  toast(`${fileName(mediaPath)} is ready — hit play!`, "success");
  playBtn?.classList.add("pulse-once");
  window.setTimeout(() => playBtn?.classList.remove("pulse-once"), 2200);
}

async function quickStartWithImport() {
  try {
    const ready = await ensureProjectReady();
    if (!ready) return;
    const mediaPath = dialogPath(
      await open({
        multiple: false,
        title: "Choose a video or audio file",
        filters: MEDIA_OPEN_FILTERS,
      }),
    );
    if (!mediaPath) {
      toast("Project ready — import media when you're set", "info");
      return;
    }
    await importMediaSmart(mediaPath);
  } catch (e) {
    showError("Quick start failed", e);
  }
}

async function importFromPath(path: string) {
  if (!isMediaPath(path)) {
    toast("That file type is not supported yet", "error");
    return;
  }
  if (!state.project) {
    const ready = await ensureProjectReady();
    if (!ready) return;
  }
  await importMediaSmart(path);
}

function showError(title: string, err: unknown) {
  const msg = err instanceof Error ? err.message : String(err);
  console.error(title, err);
  toast(`${title}: ${msg}`, "error");
}

function dialogPath(result: string | string[] | null): string | null {
  if (!result) return null;
  return Array.isArray(result) ? (result[0] ?? null) : result;
}

function clipLeft(secs: number): number {
  return TRACK_LABEL_W + 8 + secs * pixelsPerSec;
}

function secsFromCanvasX(x: number): number {
  return Math.max(0, (x - TRACK_LABEL_W - 8) / pixelsPerSec);
}

async function refreshPreview() {
  if (!state.project) return;
  try {
    await invoke("update_preview", { timeSecs: state.playheadSecs });
  } catch (e) {
    console.warn("preview update:", e);
  }
}

async function ensureStarterTracks(quiet = false) {
  if (!state.project || state.project.tracks.length > 0) return;
  await applyCommand({ command: "AddTrack", kind: "video", name: "Video 1" });
  await applyCommand({ command: "AddTrack", kind: "audio", name: "Audio 1" });
  await applyCommand({ command: "AddTrack", kind: "caption", name: "Captions" });
  if (!quiet) toast("Timeline ready — import media to start editing", "info");
}

async function loadProjectFromPath(path: string) {
  try {
    await invoke("open_project", { path });
    state.project = await invoke<Project>("get_project");
    state.projectPath = path;
    state.playheadSecs = 0;
    state.selected = null;
    setWelcomeVisible(false);
    render();
    await syncPreviewBounds();
    await refreshPreview();
  } catch (e) {
    showError("Load project failed", e);
  }
}

async function saveProject() {
  if (!state.projectPath) return;
  try {
    await invoke("save_project");
    toast("Project saved", "success");
  } catch (e) {
    showError("Save failed", e);
  }
}

async function applyCommand(command: Record<string, unknown>, quiet = false) {
  if (!state.project) {
    showError("No project", "Create or open a project first.");
    return false;
  }
  try {
    await invoke("apply_command", { command });
    state.project = await invoke<Project>("get_project");
    render();
    await refreshPreview();
    return true;
  } catch (e) {
    if (!quiet) showError("Edit failed", e);
    return false;
  }
}

function setWelcomeVisible(visible: boolean) {
  welcomeEl?.classList.toggle("hidden", !visible);
}

function timelineDuration(project: Project): number {
  let end = 0;
  for (const track of project.tracks) {
    for (const clip of track.clips) {
      const clipEnd =
        clip.type === "caption"
          ? clip.position_secs + clip.duration_secs
          : clip.position_secs + (clip.source_out_secs - clip.source_in_secs);
      end = Math.max(end, clipEnd);
    }
  }
  return Math.max(end, 1);
}

function clipDuration(clip: Clip): number {
  if (clip.type === "caption") return clip.duration_secs;
  return clip.source_out_secs - clip.source_in_secs;
}

function collectSnapTimes(excludeClipId?: string): number[] {
  if (!state.project) return [];
  const times = new Set<number>([state.playheadSecs, 0]);
  const duration = timelineDuration(state.project);
  for (let t = 0; t <= duration; t += 1) times.add(t);
  for (const track of state.project.tracks) {
    for (const clip of track.clips) {
      if (clip.id === excludeClipId) continue;
      times.add(clip.position_secs);
      times.add(clip.position_secs + clipDuration(clip));
    }
  }
  return [...times];
}

function snapTime(secs: number, excludeClipId?: string): number {
  if (!snapEnabled) return Math.max(0, secs);
  const threshold = SNAP_THRESHOLD_PX / pixelsPerSec;
  let best = Math.max(0, secs);
  let bestDist = threshold;
  for (const target of collectSnapTimes(excludeClipId)) {
    const dist = Math.abs(target - secs);
    if (dist < bestDist) {
      bestDist = dist;
      best = target;
    }
  }
  return best;
}

function trackLayout(h: number, trackCount: number) {
  const usable = h - RULER_H - 8;
  const trackH = Math.min(52, usable / Math.max(trackCount, 1));
  return { trackH, laneTop: (i: number) => RULER_H + 4 + i * (trackH + 6) };
}

function hitTestClip(x: number, y: number): {
  track: Track;
  clip: Clip;
  trackIndex: number;
  edge: "left" | "right" | "body";
} | null {
  if (!state.project || !timelineCanvas) return null;
  const rect = timelineCanvas.getBoundingClientRect();
  const { trackH, laneTop } = trackLayout(rect.height, state.project.tracks.length);

  for (let ti = 0; ti < state.project.tracks.length; ti++) {
    const track = state.project.tracks[ti];
    const y0 = laneTop(ti);
    if (y < y0 || y > y0 + trackH) continue;
    for (const clip of track.clips) {
      const cx = clipLeft(clip.position_secs);
      const cw = clipDuration(clip) * pixelsPerSec;
      if (y < y0 + 18 || x < cx || x > cx + cw) continue;
      if (x <= cx + TRIM_HANDLE_PX) return { track, clip, trackIndex: ti, edge: "left" };
      if (x >= cx + cw - TRIM_HANDLE_PX) return { track, clip, trackIndex: ti, edge: "right" };
      return { track, clip, trackIndex: ti, edge: "body" };
    }
  }
  return null;
}

function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  const rr = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
}

function clipFill(ctx: CanvasRenderingContext2D, kind: Track["kind"]) {
  if (kind === "video") {
    const g = ctx.createLinearGradient(0, 0, 200, 0);
    g.addColorStop(0, "#3b82f6");
    g.addColorStop(1, "#2563eb");
    ctx.fillStyle = g;
  } else if (kind === "audio") {
    const g = ctx.createLinearGradient(0, 0, 200, 0);
    g.addColorStop(0, "#22c55e");
    g.addColorStop(1, "#16a34a");
    ctx.fillStyle = g;
  } else {
    const g = ctx.createLinearGradient(0, 0, 200, 0);
    g.addColorStop(0, "#f59e0b");
    g.addColorStop(1, "#d97706");
    ctx.fillStyle = g;
  }
}

function renderTimeline() {
  if (!timelineCanvas || !timelineCtx) return;

  const dpr = window.devicePixelRatio || 1;
  const rect = timelineCanvas.getBoundingClientRect();
  timelineCanvas.width = rect.width * dpr;
  timelineCanvas.height = rect.height * dpr;
  timelineCtx.setTransform(dpr, 0, 0, dpr, 0, 0);

  const w = rect.width;
  const h = rect.height;

  timelineCtx.fillStyle = "#0c0d12";
  timelineCtx.fillRect(0, 0, w, h);

  if (!state.project || state.project.tracks.length === 0) {
    timelineCtx.fillStyle = "#6b7285";
    timelineCtx.font = "500 13px Inter, Segoe UI, sans-serif";
    timelineCtx.textAlign = "center";
    timelineCtx.fillText("Import media to build your timeline", w / 2, h / 2);
    timelineCtx.textAlign = "left";
    return;
  }

  const project = state.project;
  const duration = timelineDuration(project);
  const { trackH, laneTop } = trackLayout(h, project.tracks.length);

  // Ruler
  timelineCtx.fillStyle = "#12141a";
  timelineCtx.fillRect(0, 0, w, RULER_H);
  timelineCtx.strokeStyle = "rgba(255,255,255,0.06)";
  timelineCtx.lineWidth = 1;
  timelineCtx.beginPath();
  timelineCtx.moveTo(0, RULER_H);
  timelineCtx.lineTo(w, RULER_H);
  timelineCtx.stroke();

  timelineCtx.fillStyle = "#9aa3b5";
  timelineCtx.font = "500 10px Inter, Segoe UI, sans-serif";
  const step = pixelsPerSec >= 60 ? 1 : pixelsPerSec >= 30 ? 2 : 5;
  for (let t = 0; t <= duration; t += step) {
    const x = clipLeft(t);
    if (x < TRACK_LABEL_W) continue;
    timelineCtx.strokeStyle = t % 5 === 0 ? "rgba(255,255,255,0.1)" : "rgba(255,255,255,0.04)";
    timelineCtx.beginPath();
    timelineCtx.moveTo(x, RULER_H);
    timelineCtx.lineTo(x, h);
    timelineCtx.stroke();
    if (t % 5 === 0) {
      timelineCtx.fillText(formatTimecode(t).slice(0, 5), x + 3, 14);
    }
  }

  project.tracks.forEach((track, i) => {
    const y = laneTop(i);

    timelineCtx.fillStyle = "#12141a";
    timelineCtx.fillRect(0, y - 2, w, trackH + 4);

    timelineCtx.fillStyle = "#16181f";
    timelineCtx.fillRect(0, y, TRACK_LABEL_W, trackH);

    timelineCtx.fillStyle = "#9aa3b5";
    timelineCtx.font = "600 10px Inter, Segoe UI, sans-serif";
    const icon = track.kind === "video" ? "▶" : track.kind === "audio" ? "♪" : "T";
    timelineCtx.fillText(icon, 8, y + 16);
    timelineCtx.font = "500 11px Inter, Segoe UI, sans-serif";
    timelineCtx.fillStyle = "#f2f4f8";
    timelineCtx.fillText(track.name, 22, y + 16);
    timelineCtx.fillStyle = "#6b7285";
    timelineCtx.font = "10px Inter, Segoe UI, sans-serif";
    timelineCtx.fillText(track.kind, 22, y + 30);

    for (const clip of track.clips) {
      const x = clipLeft(clip.position_secs);
      const cw = Math.max(clipDuration(clip) * pixelsPerSec, 8);
      const cy = y + 18;
      const ch = trackH - 22;
      const selected =
        state.selected?.trackId === track.id && state.selected?.clipId === clip.id;

      clipFill(timelineCtx, track.kind);
      timelineCtx.globalAlpha = selected ? 1 : 0.92;
      roundRect(timelineCtx, x, cy, cw, ch, 5);
      timelineCtx.fill();
      timelineCtx.globalAlpha = 1;

      if (selected) {
        timelineCtx.strokeStyle = "#fff";
        timelineCtx.lineWidth = 2;
        roundRect(timelineCtx, x, cy, cw, ch, 5);
        timelineCtx.stroke();
      }

      timelineCtx.fillStyle = "rgba(255,255,255,0.95)";
      timelineCtx.font = "500 10px Inter, Segoe UI, sans-serif";
      const label =
        clip.type === "caption" ? clip.text.slice(0, 20) : fileName(
            state.project!.media.find((m) => m.id === clip.media_id)?.path ?? track.kind,
          ).slice(0, 16);
      if (cw > 28) timelineCtx.fillText(label, x + 6, y + trackH - 10);
    }
  });

  const phx = clipLeft(state.playheadSecs);
  timelineCtx.strokeStyle = "#f43f5e";
  timelineCtx.lineWidth = 2;
  timelineCtx.beginPath();
  timelineCtx.moveTo(phx, 0);
  timelineCtx.lineTo(phx, h);
  timelineCtx.stroke();

  timelineCtx.fillStyle = "#f43f5e";
  timelineCtx.beginPath();
  timelineCtx.moveTo(phx - 5, 0);
  timelineCtx.lineTo(phx + 5, 0);
  timelineCtx.lineTo(phx, 8);
  timelineCtx.closePath();
  timelineCtx.fill();
}

function renderInspector(container: HTMLElement) {
  container.innerHTML = "";
  if (!state.project || !state.selected) {
    const empty = el("div", "inspector-empty");
    empty.innerHTML = `<div class="icon">✦</div><p>Select a clip on the timeline to edit its properties.</p>`;
    container.appendChild(empty);
    return;
  }
  const track = state.project.tracks.find((t) => t.id === state.selected!.trackId);
  const clip = track?.clips.find((c) => c.id === state.selected!.clipId);
  if (!track || !clip) return;

  const wrap = el("div", "inspector");

  if (clip.type === "caption") {
    const sec = el("div", "inspector-section");
    sec.appendChild(el("h3", undefined, "Caption"));
    const textField = el("div", "field");
    textField.appendChild(el("label", undefined, "Text"));
    const text = document.createElement("textarea");
    text.value = clip.text;
    text.placeholder = "Enter caption text…";
    textField.appendChild(text);
    sec.appendChild(textField);

    const styleField = el("div", "field");
    styleField.appendChild(el("label", undefined, "Style"));
    const style = document.createElement("select");
    CAPTION_STYLES.forEach((s) => {
      const opt = document.createElement("option");
      opt.value = s;
      opt.textContent = s.replace(/-/g, " ");
      if (s === clip.style_id) opt.selected = true;
      style.appendChild(opt);
    });
    styleField.appendChild(style);
    sec.appendChild(styleField);
    wrap.appendChild(sec);

    const timing = el("div", "inspector-section");
    timing.appendChild(el("h3", undefined, "Timing"));
    for (const [label, val, key] of [
      ["Start (seconds)", clip.position_secs, "position_secs"],
      ["Duration (seconds)", clip.duration_secs, "duration_secs"],
    ] as const) {
      const f = el("div", "field");
      f.appendChild(el("label", undefined, label));
      const input = document.createElement("input");
      input.type = "number";
      input.step = "0.1";
      input.value = String(val);
      f.appendChild(input);
      input.addEventListener("change", () => {
        void applyCommand({
          command: "SetCaption",
          track_id: track.id,
          clip_id: clip.id,
          text: null,
          position_secs: key === "position_secs" ? parseFloat(input.value) : null,
          duration_secs: key === "duration_secs" ? parseFloat(input.value) : null,
          style_id: null,
        });
      });
      timing.appendChild(f);
    }
    wrap.appendChild(timing);

    const apply = btn("Update caption", { primary: true });
    apply.addEventListener("click", () => {
      void applyCommand({
        command: "SetCaption",
        track_id: track.id,
        clip_id: clip.id,
        text: text.value,
        position_secs: clip.position_secs,
        duration_secs: clip.duration_secs,
        style_id: style.value,
      }).then((ok) => ok && toast("Caption updated", "success"));
    });
    const actions = el("div", "inspector-actions");
    actions.appendChild(apply);
    wrap.appendChild(actions);
  } else {
    const sec = el("div", "inspector-section");
    sec.appendChild(el("h3", undefined, "Clip"));
    sec.appendChild(el("p", undefined, `${track.name} · ${track.kind}`));
    const gainField = el("div", "field");
    gainField.appendChild(el("label", undefined, "Volume (dB)"));
    const gain = document.createElement("input");
    gain.type = "range";
    gain.min = "-24";
    gain.max = "12";
    gain.step = "0.5";
    gain.value = String(clip.gain_db);
    const gainVal = el("span", undefined, `${clip.gain_db} dB`);
    gain.addEventListener("input", () => {
      gainVal.textContent = `${gain.value} dB`;
    });
    gain.addEventListener("change", () => {
      void applyCommand({
        command: "SetAudioGain",
        track_id: track.id,
        clip_id: clip.id,
        gain_db: parseFloat(gain.value),
      });
    });
    gainField.appendChild(gain);
    gainField.appendChild(gainVal);
    sec.appendChild(gainField);
    wrap.appendChild(sec);
  }

  const del = btn("Delete", { className: "btn-danger", icon: "🗑" });
  del.addEventListener("click", async () => {
    await applyCommand({
      command: "DeleteClip",
      track_id: track.id,
      clip_id: clip.id,
      ripple: false,
    });
    state.selected = null;
    render();
  });
  const actions = wrap.querySelector(".inspector-actions") ?? el("div", "inspector-actions");
  actions.appendChild(del);
  if (!wrap.contains(actions)) wrap.appendChild(actions);

  container.appendChild(wrap);
}

function renderMediaList(container: HTMLElement) {
  container.innerHTML = "";

  const drop = el("div", "drop-zone");
  drop.innerHTML = `<strong>Drop video here</strong><span>or click to browse files</span>`;
  drop.addEventListener("click", () => void pickAndImportMedia());
  drop.addEventListener("dragover", (e) => {
    e.preventDefault();
    drop.classList.add("drag-over");
  });
  drop.addEventListener("dragleave", () => drop.classList.remove("drag-over"));
  drop.addEventListener("drop", (e) => {
    e.preventDefault();
    drop.classList.remove("drag-over");
  });
  container.appendChild(drop);

  if (!state.project?.media.length) {
    container.appendChild(
      el("p", "empty-hint", "Imported files appear here. Each one is added to the timeline automatically."),
    );
    return;
  }

  for (const item of state.project.media) {
    const row = el("div", "media-item");
    const thumb = el("div", `media-thumb ${item.kind}`);
    thumb.textContent = item.kind === "audio" ? "♪" : "▶";
    const meta = el("div", "media-meta");
    meta.appendChild(el("div", "name", fileName(item.path)));
    meta.appendChild(
      el("div", "sub", `${item.kind} · ${item.duration_secs?.toFixed(1) ?? "?"}s`),
    );
    row.appendChild(thumb);
    row.appendChild(meta);
    row.appendChild(el("span", "media-add-hint", "+ timeline"));
    row.addEventListener("click", () => void placeMediaOnTimeline(item.id, item.kind));
    container.appendChild(row);
  }
}

async function ensureTrack(kind: "video" | "audio" | "caption", name: string) {
  let track = state.project!.tracks.find((t) => t.kind === kind);
  if (track) return track;
  const ok = await applyCommand({ command: "AddTrack", kind, name }, true);
  if (!ok) throw new Error(`Could not create ${kind} track`);
  state.project = await invoke<Project>("get_project");
  track = state.project!.tracks.find((t) => t.kind === kind)!;
  return track;
}

async function placeMediaOnTimeline(mediaId: string, kind: string, quiet = false) {
  if (!state.project) return;
  const media = state.project.media.find((m) => m.id === mediaId);
  if (!media) return;
  const trackKind = kind === "audio" ? "audio" : "video";
  const track = await ensureTrack(
    trackKind,
    trackKind === "video" ? "Video 1" : "Audio 1",
  );
  const dur = media.duration_secs ?? 5;
  const ok = await applyCommand({
    command: "AddClip",
    track_id: track.id,
    media_id: mediaId,
    position_secs: snapTime(state.playheadSecs),
    source_in_secs: 0,
    source_out_secs: dur,
  });
  if (ok && !quiet) toast(`Added ${fileName(media.path)} to timeline`, "success");
}

async function importMediaSmart(path: string) {
  if (!state.project) {
    showError("Import", "Create or open a project first.");
    return;
  }
  const wasEmpty = !projectHasClips(state.project);
  setImportBusy(true);
  try {
    const ok = await applyCommand({ command: "ImportMedia", path });
    if (!ok) return;
    const media = state.project!.media[state.project!.media.length - 1];
    if (!media) return;
    await placeMediaOnTimeline(media.id, media.kind, true);
    if (wasEmpty) await finishFirstClipExperience(path);
    else toast(`Added ${fileName(path)} to timeline`, "success");
  } finally {
    setImportBusy(false);
  }
}

async function pickAndImportMedia() {
  if (!state.project) {
    await quickStartWithImport();
    return;
  }
  const path = dialogPath(
    await open({
      multiple: false,
      title: "Choose a video or audio file",
      filters: MEDIA_OPEN_FILTERS,
    }),
  );
  if (!path) return;
  await importMediaSmart(path);
}

function render() {
  const chip = document.querySelector(".project-chip");
  if (chip) {
    chip.classList.toggle("live", !!state.project);
    const label = chip.querySelector(".label");
    if (label) {
      label.textContent = state.project
        ? state.project.name
        : "No project open";
    }
  }

  document.querySelectorAll("[data-needs-project]").forEach((el) => {
    (el as HTMLButtonElement).disabled = !state.project;
  });

  if (timecodeEl) {
    timecodeEl.textContent = formatTimecode(state.playheadSecs);
  }

  if (playBtn) {
    playBtn.textContent = state.isPlaying ? "⏸" : "▶";
    playBtn.classList.toggle("playing", state.isPlaying);
    if (state.isPlaying) dismissCoachMark();
  }

  const previewHost = document.getElementById("preview-host");
  if (previewHost) {
    previewHost.classList.toggle("has-clips", projectHasClips(state.project));
  }

  if (durationHintEl && state.project) {
    durationHintEl.textContent = projectHasClips(state.project)
      ? `/ ${formatTimecode(timelineDuration(state.project))}`
      : "/ —";
  }

  renderMediaList(document.getElementById("media-list")!);
  renderInspector(document.getElementById("inspector")!);
  renderTimeline();

  const hint = document.querySelector(".preview-host .hint");
  if (hint) {
    if (importBusy) {
      hint.innerHTML = `<span class="icon spinner"></span><span>Importing your media…</span>`;
    } else if (!state.project) {
      hint.innerHTML = `<span class="icon">🎬</span><span>Create or import to start editing</span>`;
    } else if (!projectHasClips(state.project)) {
      hint.innerHTML = `<span class="icon">⬆</span><span><strong>Import a video</strong><br/>Click the media panel or Import button</span>`;
    } else {
      hint.innerHTML = `<span class="icon">▶</span><span>Press play to preview</span>`;
    }
  }
}

async function syncPreviewBounds() {
  if (!state.project) return;
  const host = document.getElementById("preview-host");
  if (!host) return;
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  const rect = host.getBoundingClientRect();
  if (rect.width < 1 || rect.height < 1) return;
  try {
    await invoke("set_preview_bounds", {
      x: Math.round(rect.left),
      y: Math.round(rect.top),
      width: Math.round(rect.width),
      height: Math.round(rect.height),
    });
  } catch (e) {
    console.warn("preview bounds:", e);
  }
}

function stopPlayback() {
  state.isPlaying = false;
  if (playTimer !== null) {
    window.clearInterval(playTimer);
    playTimer = null;
  }
  void invoke("stop_playback").catch(() => {});
  render();
}

async function startPlayback() {
  if (!state.project) return;
  stopPlayback();
  state.isPlaying = true;
  render();
  const fps = state.project.settings.fps;
  try {
    await invoke("start_playback", { timeSecs: state.playheadSecs });
  } catch (e) {
    console.warn("audio playback:", e);
  }
  playTimer = window.setInterval(async () => {
    if (!state.project) return;
    state.playheadSecs += 1 / fps;
    const dur = timelineDuration(state.project);
    if (state.playheadSecs >= dur) {
      state.playheadSecs = 0;
      stopPlayback();
      return;
    }
    render();
    await refreshPreview();
  }, 1000 / fps);
}

function setupTimelineInteraction(canvas: HTMLCanvasElement) {
  canvas.addEventListener("wheel", (ev) => {
    if (!ev.ctrlKey && !ev.metaKey) return;
    ev.preventDefault();
    pixelsPerSec = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, pixelsPerSec + (ev.deltaY > 0 ? -10 : 10)));
    const zl = document.querySelector(".zoom-label");
    if (zl) zl.textContent = `${Math.round(pixelsPerSec)}%`;
    render();
  });

  canvas.addEventListener("mousedown", (ev) => {
    if (!state.project) return;
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const hit = hitTestClip(x, y);

    if (hit && timelineTool === "razor") {
      void applyCommand({
        command: "SplitClip",
        track_id: hit.track.id,
        clip_id: hit.clip.id,
        at_secs: snapTime(secsFromCanvasX(x)),
      });
      return;
    }

    if (hit) {
      state.selected = { trackId: hit.track.id, clipId: hit.clip.id };
      if (timelineTool === "select" && hit.edge === "left" && hit.clip.type === "caption") {
        dragState = {
          mode: "trim-left-caption",
          trackId: hit.track.id,
          clipId: hit.clip.id,
          startX: x,
          origPos: hit.clip.position_secs,
          origDur: hit.clip.duration_secs,
        };
      } else if (timelineTool === "select" && hit.edge === "left" && hit.clip.type !== "caption") {
        dragState = {
          mode: "trim-left",
          trackId: hit.track.id,
          clipId: hit.clip.id,
          startX: x,
          origIn: hit.clip.source_in_secs,
          origOut: hit.clip.source_out_secs,
          origPos: hit.clip.position_secs,
        };
      } else if (timelineTool === "select" && hit.edge === "right") {
        dragState = {
          mode: "trim-right",
          trackId: hit.track.id,
          clipId: hit.clip.id,
          startX: x,
          origOut:
            hit.clip.type === "caption" ? hit.clip.duration_secs : hit.clip.source_out_secs,
        };
      } else if (timelineTool === "select") {
        dragState = {
          mode: "move",
          trackId: hit.track.id,
          clipId: hit.clip.id,
          startX: x,
          origPos: hit.clip.position_secs,
        };
      }
      render();
      return;
    }

    if (y >= RULER_H) {
      state.playheadSecs = snapTime(secsFromCanvasX(x));
      state.selected = null;
      render();
      void refreshPreview();
      void invoke("scrub_audio", { timeSecs: state.playheadSecs }).catch(() => {});
    }
  });

  window.addEventListener("mousemove", (ev) => {
    if (!dragState || !state.project || !timelineCanvas) return;
    const rect = timelineCanvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const delta = (x - dragState.startX) / pixelsPerSec;
    const track = state.project.tracks.find((t) => t.id === dragState!.trackId);
    const clip = track?.clips.find((c) => c.id === dragState!.clipId);
    if (!clip) return;

    if (dragState.mode === "move") {
      clip.position_secs = snapTime(Math.max(0, dragState.origPos + delta), clip.id);
    } else if (dragState.mode === "trim-right") {
      if (clip.type === "caption") {
        clip.duration_secs = Math.max(0.1, dragState.origOut + delta);
      } else {
        clip.source_out_secs = Math.max(clip.source_in_secs + 0.05, dragState.origOut + delta);
      }
    } else if (dragState.mode === "trim-left" && clip.type !== "caption") {
      const newIn = Math.min(dragState.origOut - 0.05, dragState.origIn + delta);
      const inDelta = newIn - dragState.origIn;
      clip.source_in_secs = newIn;
      clip.position_secs = dragState.origPos + inDelta;
    } else if (dragState.mode === "trim-left-caption" && clip.type === "caption") {
      const newPos = snapTime(Math.max(0, dragState.origPos + delta), clip.id);
      const posDelta = newPos - dragState.origPos;
      clip.position_secs = newPos;
      clip.duration_secs = Math.max(0.1, dragState.origDur - posDelta);
    }
    render();
  });

  window.addEventListener("mouseup", async () => {
    if (!dragState || !state.project) return;
    const saved = dragState;
    const { trackId, clipId, mode } = saved;
    const track = state.project.tracks.find((t) => t.id === trackId);
    const clip = track?.clips.find((c) => c.id === clipId);
    dragState = null;
    if (!clip) return;

    if (mode === "move") {
      await applyCommand({
        command: "MoveClip",
        track_id: trackId,
        clip_id: clipId,
        new_position_secs: snapTime(clip.position_secs, clipId),
        new_track_id: null,
      });
    } else if (mode === "trim-right") {
      if (clip.type === "caption") {
        await applyCommand({
          command: "SetCaption",
          track_id: trackId,
          clip_id: clipId,
          text: null,
          position_secs: null,
          duration_secs: Math.max(0.1, clip.duration_secs),
          style_id: null,
        });
      } else {
        await applyCommand({
          command: "TrimClip",
          track_id: trackId,
          clip_id: clipId,
          new_source_in_secs: null,
          new_source_out_secs: clip.source_out_secs,
        });
      }
    } else if (mode === "trim-left-caption" && clip.type === "caption") {
      await applyCommand({
        command: "SetCaption",
        track_id: trackId,
        clip_id: clipId,
        text: null,
        position_secs: clip.position_secs,
        duration_secs: Math.max(0.1, clip.duration_secs),
        style_id: null,
      });
    } else if (mode === "trim-left" && clip.type !== "caption") {
      const inDelta = clip.source_in_secs - saved.origIn;
      if (Math.abs(inDelta) > 1e-6) {
        await applyCommand({
          command: "TrimClip",
          track_id: trackId,
          clip_id: clipId,
          new_source_in_secs: clip.source_in_secs,
          new_source_out_secs: null,
        });
        await applyCommand({
          command: "MoveClip",
          track_id: trackId,
          clip_id: clipId,
          new_position_secs: clip.position_secs,
          new_track_id: null,
        });
      }
    }
  });
}

async function createNewProject() {
  const path = dialogPath(
    await save({
      filters: [{ name: "Uppercut project", extensions: ["uppercut.json"] }],
    }),
  );
  if (!path) return;
  await invoke("new_project", { path, name: "Untitled edit" });
  await loadProjectFromPath(path);
  await ensureStarterTracks(true);
  toast("New project created", "success");
  const mediaPath = dialogPath(
    await open({
      multiple: false,
      title: "Choose a video or audio file",
      filters: MEDIA_OPEN_FILTERS,
    }),
  );
  if (mediaPath) await importMediaSmart(mediaPath);
  else toast("Import media when you're ready", "info");
}

async function openExistingProject() {
  const path = dialogPath(
    await open({
      filters: [{ name: "Uppercut project", extensions: ["uppercut.json"] }],
    }),
  );
  if (!path) return;
  await loadProjectFromPath(path);
  toast("Project opened", "success");
}

async function runExport() {
  if (!state.project) return;
  const outputPath = dialogPath(
    await save({
      filters: [{ name: "MP4 video", extensions: ["mp4"] }],
      defaultPath: `${state.project.name}.mp4`,
    }),
  );
  if (!outputPath) return;
  try {
    toast("Exporting… this may take a minute", "info");
    await invoke("export_project", { outputPath, preset: exportPreset });
    toast(`Exported to ${fileName(outputPath)}`, "success");
  } catch (e) {
    showError("Export failed", e);
  }
}

function buildWelcome(): HTMLElement {
  const screen = el("div", "welcome-screen");
  welcomeEl = screen;
  const card = el("div", "welcome-card");
  card.innerHTML = `
    <div class="logo">✂</div>
    <h1>Start editing in seconds</h1>
    <p>Pick a video and we'll set up your timeline automatically. No manual track setup.</p>
    <ol class="welcome-steps">
      <li><span>1</span> Import your clip</li>
      <li><span>2</span> Preview and trim</li>
      <li><span>3</span> Export when done</li>
    </ol>
  `;
  const actions = el("div", "welcome-actions");
  const importBtn = btn("Import video to start", { primary: true, icon: "⬆" });
  importBtn.addEventListener("click", () => void quickStartWithImport());
  const openBtn = btn("Open existing project", { icon: "📂" });
  openBtn.addEventListener("click", () => void openExistingProject());
  const newBtn = btn("Create project file…", { className: "btn-ghost" });
  newBtn.addEventListener("click", () => void createNewProject());
  actions.append(importBtn, openBtn, newBtn);
  card.appendChild(actions);
  screen.appendChild(card);
  return screen;
}

function buildUi(root: HTMLElement) {
  root.appendChild(buildWelcome());

  const topbar = el("header", "topbar");
  const brand = el("div", "brand");
  brand.innerHTML = `<span class="brand-mark">✂</span> Uppercut`;

  const fileGroup = el("div", "topbar-group");
  const btnNew = btn("New", { icon: "＋", title: "New project (Ctrl+N)" });
  const btnOpen = btn("Open", { icon: "📂", title: "Open project" });
  const btnSave = btn("Save", { icon: "💾", title: "Save (Ctrl+S)" });
  fileGroup.append(btnNew, btnOpen, btnSave);

  const editGroup = el("div", "topbar-group");
  const btnImport = btn("Import", {
    className: "btn-import",
    icon: "⬆",
    title: "Import video or audio",
  });
  btnImport.dataset.needsProject = "1";
  const btnSplit = btn("Split", { icon: "✂", title: "Split clip at playhead (S)" });
  btnSplit.dataset.needsProject = "1";
  editGroup.append(btnImport, btnSplit);

  const trackMenu = el("div", "track-menu");
  const btnTrack = btn("Track ▾", { title: "Add timeline track" });
  btnTrack.dataset.needsProject = "1";
  const trackPop = el("div", "track-menu-pop");
  for (const [kind, label] of [
    ["video", "Video track"],
    ["audio", "Audio track"],
    ["caption", "Caption track"],
  ] as const) {
    const item = document.createElement("button");
    item.textContent = label;
    item.addEventListener("click", () => {
      trackPop.classList.remove("open");
      void applyCommand({
        command: "AddTrack",
        kind,
        name: label.replace(" track", ""),
      }).then((ok) => ok && toast(`Added ${label.toLowerCase()}`, "success"));
    });
    trackPop.appendChild(item);
  }
  trackMenu.append(btnTrack, trackPop);
  btnTrack.addEventListener("click", () => trackPop.classList.toggle("open"));
  document.addEventListener("click", (e) => {
    if (!trackMenu.contains(e.target as Node)) trackPop.classList.remove("open");
  });

  const chip = el("div", "project-chip");
  chip.innerHTML = `<span class="dot"></span><span class="label">No project open</span>`;

  const btnExport = btn("Export", { primary: true, icon: "⬇", title: "Export video" });
  btnExport.dataset.needsProject = "1";

  topbar.append(brand, fileGroup, editGroup, trackMenu, el("span", "spacer"), chip, btnExport);

  const workspace = el("div", "workspace");

  const left = el("aside", "panel");
  left.appendChild(el("div", "panel-header")).appendChild(el("h2", undefined, "Media"));
  const mediaList = el("div", "panel-body");
  mediaList.id = "media-list";
  left.appendChild(mediaList);

  const center = el("div", "preview-column");
  const previewHost = el("section", "preview-host");
  previewHost.id = "preview-host";
  previewHost.appendChild(el("div", "hint"));
  const transport = el("div", "transport-bar");
  playBtn = el("button", "play-btn", "▶") as HTMLButtonElement;
  playBtn.title = "Play / Pause (Space)";
  playBtn.disabled = true;
  playBtn.dataset.needsProject = "1";
  timecodeEl = el("span", "timecode", "00:00.00");
  durationHintEl = el("span", "timecode-sub", "/ —");
  transport.append(playBtn, timecodeEl, durationHintEl);
  center.append(previewHost, transport);

  const right = el("aside", "panel panel-right");
  right.appendChild(el("div", "panel-header")).appendChild(el("h2", undefined, "Properties"));
  const inspector = el("div", "panel-body");
  inspector.id = "inspector";
  right.appendChild(inspector);

  workspace.append(left, center, right);

  const timelineWrap = el("div", "timeline-wrap");
  const tlTools = el("div", "timeline-tools");
  const toolGroup = el("div", "tool-group");
  const btnSelect = el("button", "tool-btn active", "Select");
  btnSelect.title = "Move and trim clips (V)";
  const btnRazor = el("button", "tool-btn", "Split");
  btnRazor.title = "Click a clip to split (C)";
  toolGroup.append(btnSelect, btnRazor);
  const btnSnap = el("button", "tool-btn active", "Snap");
  btnSnap.title = "Snap to grid and clip edges";
  tlTools.append(toolGroup, btnSnap, el("span", "tool-divider"));
  const zoomControls = el("div", "zoom-controls");
  const btnZoomOut = btn("−", { className: "btn-icon-only" });
  const zoomLabel = el("span", "zoom-label", "100%");
  const btnZoomIn = btn("+", { className: "btn-icon-only" });
  zoomControls.append(btnZoomOut, zoomLabel, btnZoomIn);
  tlTools.append(zoomControls);

  timelineCanvas = el("canvas");
  timelineCanvas.id = "timeline";
  timelineWrap.append(tlTools, timelineCanvas);

  root.append(topbar, workspace, timelineWrap);

  btnNew.addEventListener("click", () => void createNewProject());
  btnOpen.addEventListener("click", () => void openExistingProject());
  btnSave.addEventListener("click", () => void saveProject());
  btnImport.addEventListener("click", () => void pickAndImportMedia());
  btnSplit.addEventListener("click", async () => {
    if (!state.selected) {
      toast("Select a clip first, then split at the playhead", "info");
      return;
    }
    await applyCommand({
      command: "SplitClip",
      track_id: state.selected.trackId,
      clip_id: state.selected.clipId,
      at_secs: state.playheadSecs,
    });
  });
  btnExport.addEventListener("click", () => void showExportDialog());

  playBtn.addEventListener("click", () => {
    if (state.isPlaying) stopPlayback();
    else void startPlayback();
  });

  btnSelect.addEventListener("click", () => {
    timelineTool = "select";
    btnSelect.classList.add("active");
    btnRazor.classList.remove("active");
    timelineCanvas!.classList.remove("razor-mode");
  });
  btnRazor.addEventListener("click", () => {
    timelineTool = "razor";
    btnRazor.classList.add("active");
    btnSelect.classList.remove("active");
    timelineCanvas!.classList.add("razor-mode");
    toast("Click a clip to split it", "info");
  });
  btnSnap.addEventListener("click", () => {
    snapEnabled = !snapEnabled;
    btnSnap.classList.toggle("active", snapEnabled);
  });
  btnZoomIn.addEventListener("click", () => {
    pixelsPerSec = Math.min(MAX_ZOOM, pixelsPerSec + 20);
    zoomLabel.textContent = `${Math.round((pixelsPerSec / 80) * 100)}%`;
    render();
  });
  btnZoomOut.addEventListener("click", () => {
    pixelsPerSec = Math.max(MIN_ZOOM, pixelsPerSec - 20);
    zoomLabel.textContent = `${Math.round((pixelsPerSec / 80) * 100)}%`;
    render();
  });

  setupTimelineInteraction(timelineCanvas);
  window.addEventListener("resize", () => {
    render();
    void syncPreviewBounds().then(refreshPreview);
  });

  window.addEventListener("keydown", (e) => {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (e.code === "Space") {
      e.preventDefault();
      if (!state.project) return;
      if (state.isPlaying) stopPlayback();
      else void startPlayback();
    }
    if (e.key === "s" || e.key === "S") {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        void saveProject();
      } else if (state.selected) {
        void applyCommand({
          command: "SplitClip",
          track_id: state.selected.trackId,
          clip_id: state.selected.clipId,
          at_secs: state.playheadSecs,
        });
      }
    }
    if (e.key === "v" || e.key === "V") btnSelect.click();
    if (e.key === "c" || e.key === "C") btnRazor.click();
    if (e.key === "n" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void createNewProject();
    }
  });
}

function showExportDialog() {
  const dialog = document.getElementById("export-dialog") as HTMLDialogElement | null;
  dialog?.showModal();
}

function buildExportDialog(): HTMLDialogElement {
  const dialog = document.createElement("dialog");
  dialog.id = "export-dialog";
  dialog.innerHTML = `
    <h3>Export video</h3>
    <p class="dialog-sub">Choose a platform preset, then pick where to save your MP4.</p>
    <div class="preset-grid">
      <button type="button" class="preset-card selected" data-preset="tiktok">
        <strong>TikTok / Reels</strong><span>1080×1920 · 9:16</span>
      </button>
      <button type="button" class="preset-card" data-preset="youtube">
        <strong>YouTube</strong><span>1920×1080 · 16:9</span>
      </button>
    </div>
    <menu>
      <button type="button" class="btn" value="cancel">Cancel</button>
      <button type="button" class="btn-primary" id="export-go">Export MP4</button>
    </menu>`;
  document.body.appendChild(dialog);

  dialog.querySelectorAll(".preset-card").forEach((card) => {
    card.addEventListener("click", () => {
      dialog.querySelectorAll(".preset-card").forEach((c) => c.classList.remove("selected"));
      card.classList.add("selected");
      exportPreset = (card as HTMLElement).dataset.preset ?? "tiktok";
    });
  });

  dialog.querySelector('[value="cancel"]')!.addEventListener("click", () => dialog.close());
  dialog.querySelector("#export-go")!.addEventListener("click", () => {
    dialog.close();
    void runExport();
  });

  return dialog;
}

window.addEventListener("DOMContentLoaded", () => {
  const root = document.getElementById("app")!;
  buildExportDialog();
  buildUi(root);
  render();

  void listen<{ paths?: string[] }>("tauri://drag-drop", (event) => {
    const path = event.payload.paths?.find(isMediaPath);
    if (path) void importFromPath(path);
  });

  void listen("tauri://drag-enter", () => {
    document.querySelector(".drop-zone")?.classList.add("drag-over");
  });
  void listen("tauri://drag-leave", () => {
    document.querySelector(".drop-zone")?.classList.remove("drag-over");
  });

  const host = document.getElementById("preview-host");
  if (host) {
    new ResizeObserver(() => {
      void syncPreviewBounds().then(refreshPreview);
    }).observe(host);
  }
});
