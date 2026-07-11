// The ONLY file that imports @tauri-apps/api or @tauri-apps/plugin-dialog. Every backend
// call and event subscription is typed and funneled through here — components never call
// `invoke`/`listen` directly (enforced by grep gate, see docs/architecture.md).

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { Project } from "./types";

export interface HistoryStatus {
  can_undo: boolean;
  can_redo: boolean;
}

export interface ProjectChangedPayload {
  revision: number;
  can_undo: boolean;
  can_redo: boolean;
}

export interface PlaybackTickPayload {
  time_secs: number;
  playing: boolean;
}

export interface PlaybackStatePayload {
  playing: boolean;
  time_secs: number;
}

export interface PlaybackErrorPayload {
  message: string;
}

export interface DragDropPayload {
  paths?: string[];
}

export interface ThumbnailsReadyPayload {
  media_id: string;
  strip_path: string;
  cols: number;
  rows: number;
  tile_width: number;
  tile_height: number;
  interval_secs: number;
}

export interface WaveformReadyPayload {
  media_id: string;
  peaks: number[];
  bucket_secs: number;
}

export type ExportPhase = "video" | "audio" | "mux";

export interface ExportProgressPayload {
  phase: ExportPhase;
  frame: number;
  total_frames: number;
  fraction: number;
}

/** Shorthand presets or a full `ExportPreset` JSON value (e.g. custom w/h/fps). */
export type ExportPresetArg =
  | "tiktok"
  | "youtube"
  | { custom: { width: number; height: number; fps: number } };

export interface MediaAssetsPayload {
  thumbnails?: ThumbnailsReadyPayload;
  waveform?: WaveformReadyPayload;
}

// ---- Project lifecycle ----

export function quickStartProject(): Promise<string> {
  return invoke<string>("quick_start_project");
}

export function newProject(path: string, name: string): Promise<void> {
  return invoke("new_project", { path, name });
}

export function openProject(path: string): Promise<void> {
  return invoke("open_project", { path });
}

export function saveProject(): Promise<void> {
  return invoke("save_project");
}

export function getProject(): Promise<Project> {
  return invoke<Project>("get_project");
}

// ---- Commands / history ----

export function applyCommand(command: Record<string, unknown>): Promise<string> {
  return invoke<string>("apply_command", { command });
}

/// Atomic batch — all-or-nothing, one undo step. For gestures that are logically a
/// single edit but need more than one Command (e.g. auto-track-on-drop).
export function applyCommands(commands: Record<string, unknown>[]): Promise<string[]> {
  return invoke<string[]>("apply_commands", { commands });
}

export function undo(): Promise<HistoryStatus> {
  return invoke<HistoryStatus>("undo");
}

export function redo(): Promise<HistoryStatus> {
  return invoke<HistoryStatus>("redo");
}

// ---- Export ----

export function exportProject(
  outputPath: string,
  preset: ExportPresetArg | Record<string, unknown>,
): Promise<void> {
  return invoke("export_project", { outputPath, preset });
}

export function cancelExport(): Promise<void> {
  return invoke("cancel_export");
}

// ---- Media assets (thumbnails / waveforms) ----

export function requestMediaAssets(mediaId: string): Promise<void> {
  return invoke("request_media_assets", { mediaId });
}

export function getMediaAssets(mediaId: string): Promise<MediaAssetsPayload> {
  return invoke<MediaAssetsPayload>("get_media_assets", { mediaId });
}

/// Converts a real filesystem path (as returned by `get_media_assets`/`media:*-ready`)
/// into a URL the webview can load — backed by the Tauri asset protocol, scoped to the
/// media cache dir (see `tauri.conf.json`'s `security.assetProtocol.scope`).
export function assetUrl(path: string): string {
  return convertFileSrc(path);
}

// ---- Preview / playback ----

export function setPreviewBounds(
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<void> {
  return invoke("set_preview_bounds", { x, y, width, height });
}

export function play(timeSecs: number): Promise<void> {
  return invoke("play", { timeSecs });
}

export function pause(): Promise<number> {
  return invoke<number>("pause");
}

export function seek(timeSecs: number): Promise<void> {
  return invoke("seek", { timeSecs });
}

export function scrubAudio(timeSecs: number): Promise<void> {
  return invoke("scrub_audio", { timeSecs });
}

// ---- Events ----

export function onPlaybackTick(cb: (payload: PlaybackTickPayload) => void): () => void {
  const unlisten = listen<PlaybackTickPayload>("playback:tick", (e) => cb(e.payload));
  return () => void unlisten.then((f) => f());
}

export function onPlaybackState(cb: (payload: PlaybackStatePayload) => void): () => void {
  const unlisten = listen<PlaybackStatePayload>("playback:state", (e) => cb(e.payload));
  return () => void unlisten.then((f) => f());
}

export function onPlaybackError(cb: (payload: PlaybackErrorPayload) => void): () => void {
  const unlisten = listen<PlaybackErrorPayload>("playback:error", (e) => cb(e.payload));
  return () => void unlisten.then((f) => f());
}

export function onProjectChanged(cb: (payload: ProjectChangedPayload) => void): () => void {
  const unlisten = listen<ProjectChangedPayload>("project:changed", (e) => cb(e.payload));
  return () => void unlisten.then((f) => f());
}

export function onThumbnailsReady(cb: (payload: ThumbnailsReadyPayload) => void): () => void {
  const unlisten = listen<ThumbnailsReadyPayload>("media:thumbnails-ready", (e) => cb(e.payload));
  return () => void unlisten.then((f) => f());
}

export function onWaveformReady(cb: (payload: WaveformReadyPayload) => void): () => void {
  const unlisten = listen<WaveformReadyPayload>("media:waveform-ready", (e) => cb(e.payload));
  return () => void unlisten.then((f) => f());
}

export function onExportProgress(cb: (payload: ExportProgressPayload) => void): () => void {
  const unlisten = listen<ExportProgressPayload>("export:progress", (e) => cb(e.payload));
  return () => void unlisten.then((f) => f());
}

export function onDragDrop(cb: (paths: string[]) => void): () => void {
  const unlisten = listen<DragDropPayload>("tauri://drag-drop", (e) =>
    cb(e.payload.paths ?? []),
  );
  return () => void unlisten.then((f) => f());
}

export function onDragEnter(cb: () => void): () => void {
  const unlisten = listen("tauri://drag-enter", () => cb());
  return () => void unlisten.then((f) => f());
}

export function onDragLeave(cb: () => void): () => void {
  const unlisten = listen("tauri://drag-leave", () => cb());
  return () => void unlisten.then((f) => f());
}

// ---- Native dialogs ----

export const MEDIA_OPEN_FILTERS = [
  { name: "Video", extensions: ["mp4", "mov", "mkv", "webm", "avi"] },
  {
    name: "All media",
    extensions: ["mp4", "mov", "mkv", "webm", "avi", "mp3", "wav", "m4a", "aac"],
  },
];

function firstPath(result: string | string[] | null): string | null {
  if (!result) return null;
  return Array.isArray(result) ? (result[0] ?? null) : result;
}

export async function pickMediaFile(): Promise<string | null> {
  const result = await open({
    multiple: false,
    title: "Choose a video or audio file",
    filters: MEDIA_OPEN_FILTERS,
  });
  return firstPath(result);
}

export async function pickProjectFileToOpen(): Promise<string | null> {
  const result = await open({
    filters: [{ name: "Uppercut project", extensions: ["uppercut.json"] }],
  });
  return firstPath(result);
}

export async function pickProjectSavePath(): Promise<string | null> {
  const result = await save({
    filters: [{ name: "Uppercut project", extensions: ["uppercut.json"] }],
  });
  return firstPath(result);
}

export async function pickExportSavePath(defaultPath: string): Promise<string | null> {
  const result = await save({
    filters: [{ name: "MP4 video", extensions: ["mp4"] }],
    defaultPath,
  });
  return firstPath(result);
}
