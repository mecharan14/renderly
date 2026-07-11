import { create } from "zustand";
import * as ipc from "../lib/ipc";
import { addCaption, addClip, addTrack } from "../lib/commands";
import { timelineDuration, TRACK_LABEL_W } from "../timeline/layout";
import {
  clipDurationSecs,
  type Clip,
  type MediaKind,
  type Project,
  type Selection,
  type Track,
  type TrackKind,
} from "../lib/types";

export type ToolMode = "select" | "razor";
export type LeftTab =
  | "media"
  | "audio"
  | "text"
  | "stickers"
  | "effects"
  | "transitions"
  | "filters"
  | "adjustment";
export type ToastKind = "info" | "success" | "error";

export interface ToastItem {
  id: number;
  message: string;
  kind: ToastKind;
}

let nextToastId = 1;

export interface DragGhost {
  mediaId: string;
  kind: MediaKind;
  durationSecs: number;
  positionSecs: number;
  /// Index into `project.tracks`, or `project.tracks.length` for "drop below the last
  /// track" — the cue to auto-create a new track (see `layout.trackIndexAtY`).
  trackIndex: number;
  /// Whether `trackIndex` is a real, kind-compatible drop target — drives ghost color
  /// (valid vs. invalid) the same way CapCut shows a red ghost over an incompatible track.
  valid: boolean;
}

interface EditorStore {
  project: Project | null;
  projectPath: string | null;
  playhead: number;
  playing: boolean;
  selection: Selection | null;
  toolMode: ToolMode;
  snapEnabled: boolean;
  pxPerSec: number;
  leftTab: LeftTab;
  canUndo: boolean;
  canRedo: boolean;
  toasts: ToastItem[];
  importBusy: boolean;
  clipboard: { clip: Clip; trackKind: TrackKind } | null;
  dragGhost: DragGhost | null;
  contextMenu: { x: number; y: number; trackId: string; clipId: string; atSecs: number } | null;
  snapGuideSecs: number | null;

  toast(message: string, kind?: ToastKind): void;
  dismissToast(id: number): void;
  select(sel: Selection | null): void;
  setTool(tool: ToolMode): void;
  setSnap(enabled: boolean): void;
  setZoom(px: number): void;
  fitZoom(): void;
  setDragGhost(ghost: DragGhost | null): void;
  dropMediaOnTimeline(): Promise<void>;
  openContextMenu(x: number, y: number, trackId: string, clipId: string, atSecs: number): void;
  closeContextMenu(): void;
  setSnapGuide(secs: number | null): void;
  setLeftTab(tab: LeftTab): void;
  setPlayheadLocal(secs: number): void;

  refetchProject(): Promise<void>;
  loadProjectFromPath(path: string): Promise<void>;
  quickStart(): Promise<boolean>;
  createNewProject(path: string, name: string): Promise<void>;
  saveProject(): Promise<void>;
  dispatch(command: Record<string, unknown>, quiet?: boolean): Promise<boolean>;
  dispatchBatch(commands: Record<string, unknown>[], quiet?: boolean): Promise<boolean>;
  undo(): Promise<void>;
  redo(): Promise<void>;

  copySelection(): void;
  pasteAtPlayhead(): Promise<void>;
  duplicateSelection(): Promise<void>;

  ensureStarterTracks(quiet?: boolean): Promise<void>;
  ensureTrack(kind: TrackKind, name: string): Promise<Track>;
  importMediaSmart(path: string): Promise<void>;
  placeMediaOnTimeline(mediaId: string, kind: MediaKind, quiet?: boolean): Promise<void>;

  startPlayback(): Promise<void>;
  stopPlayback(): void;
  seekTo(secs: number): Promise<void>;
  scrubAt(secs: number): void;

  onPlaybackTick(payload: { time_secs: number; playing: boolean }): void;
  onPlaybackState(payload: { playing: boolean; time_secs: number }): void;
}

export const useEditorStore = create<EditorStore>((set, get) => ({
  project: null,
  projectPath: null,
  playhead: 0,
  playing: false,
  selection: null,
  toolMode: "select",
  snapEnabled: true,
  pxPerSec: 80,
  leftTab: "media",
  canUndo: false,
  canRedo: false,
  toasts: [],
  importBusy: false,
  clipboard: null,
  dragGhost: null,
  contextMenu: null,
  snapGuideSecs: null,

  toast(message, kind = "info") {
    const id = nextToastId++;
    set((s) => ({ toasts: [...s.toasts, { id, message, kind }] }));
    window.setTimeout(() => get().dismissToast(id), 3200);
  },
  dismissToast(id) {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },

  select(sel) {
    set({ selection: sel });
  },
  setTool(tool) {
    set({ toolMode: tool });
  },
  setSnap(enabled) {
    set({ snapEnabled: enabled });
  },
  setZoom(px) {
    set({ pxPerSec: Math.min(320, Math.max(25, px)) });
  },
  fitZoom() {
    const project = get().project;
    if (!project) return;
    const canvas = document.getElementById("timeline");
    if (!canvas) return;
    const availableWidth = canvas.clientWidth - TRACK_LABEL_W - 16;
    const duration = timelineDuration(project);
    if (duration <= 0 || availableWidth <= 0) return;
    get().setZoom(availableWidth / duration);
  },
  setLeftTab(tab) {
    set({ leftTab: tab });
  },
  setPlayheadLocal(secs) {
    set({ playhead: Math.max(0, secs) });
  },
  setDragGhost(ghost) {
    set({ dragGhost: ghost });
  },
  openContextMenu(x, y, trackId, clipId, atSecs) {
    set({ contextMenu: { x, y, trackId, clipId, atSecs }, selection: { trackId, clipId } });
  },
  closeContextMenu() {
    set({ contextMenu: null });
  },
  setSnapGuide(secs) {
    set({ snapGuideSecs: secs });
  },

  async dropMediaOnTimeline() {
    const { project, dragGhost } = get();
    set({ dragGhost: null });
    if (!project || !dragGhost || !dragGhost.valid) return;

    const trackKind: TrackKind = dragGhost.kind === "audio" ? "audio" : "video";
    const existingTrack = project.tracks[dragGhost.trackIndex] as Track | undefined;

    if (existingTrack) {
      await get().dispatch(
        addClip(existingTrack.id, dragGhost.mediaId, dragGhost.positionSecs, 0, dragGhost.durationSecs),
      );
      return;
    }

    // Dropped below the last track — CapCut-style auto-track: AddTrack + AddClip as one
    // atomic batch, so undo removes both together instead of leaving an empty track. The
    // new track's id is generated here (not server-side) so the AddClip in the SAME
    // batch can reference it — apply_commands has no way to thread one command's
    // server-generated output into a later command in the same call.
    const trackName =
      trackKind === "video" ? `Video ${project.tracks.length + 1}` : `Audio ${project.tracks.length + 1}`;
    const newTrackId = crypto.randomUUID();
    await get().dispatchBatch([
      addTrack(trackKind, trackName, newTrackId),
      addClip(newTrackId, dragGhost.mediaId, dragGhost.positionSecs, 0, dragGhost.durationSecs),
    ]);
  },

  async refetchProject() {
    if (!get().projectPath) return;
    try {
      const project = await ipc.getProject();
      set({ project });
    } catch (e) {
      console.warn("refetch project:", e);
    }
  },

  async loadProjectFromPath(path) {
    try {
      await ipc.openProject(path);
      const project = await ipc.getProject();
      set({ project, projectPath: path, playhead: 0, selection: null, canUndo: false, canRedo: false });
    } catch (e) {
      get().toast(`Load project failed: ${errMsg(e)}`, "error");
    }
  },

  async quickStart() {
    if (get().project) return true;
    set({ importBusy: true });
    try {
      const path = await ipc.quickStartProject();
      await get().loadProjectFromPath(path);
      await get().ensureStarterTracks(true);
      return true;
    } catch (e) {
      get().toast(`Could not start project: ${errMsg(e)}`, "error");
      return false;
    } finally {
      set({ importBusy: false });
    }
  },

  async createNewProject(path, name) {
    await ipc.newProject(path, name);
    await get().loadProjectFromPath(path);
    await get().ensureStarterTracks(true);
  },

  async saveProject() {
    if (!get().projectPath) return;
    try {
      await ipc.saveProject();
      get().toast("Project saved", "success");
    } catch (e) {
      get().toast(`Save failed: ${errMsg(e)}`, "error");
    }
  },

  async dispatch(command, quiet = false) {
    if (!get().project) {
      get().toast("Create or open a project first.", "error");
      return false;
    }
    try {
      await ipc.applyCommand(command);
      await get().refetchProject();
      await ipc.seek(get().playhead).catch(() => {});
      return true;
    } catch (e) {
      if (!quiet) get().toast(`Edit failed: ${errMsg(e)}`, "error");
      // The backend rejected the command, so `store.project` may still be holding an
      // optimistic local mutation (e.g. a timeline drag preview) that was never actually
      // applied — resync from the real backend state instead of leaving that phantom
      // edit visible until some unrelated later command happens to refetch.
      await get().refetchProject();
      return false;
    }
  },

  async dispatchBatch(commands, quiet = false) {
    if (!get().project) {
      get().toast("Create or open a project first.", "error");
      return false;
    }
    try {
      await ipc.applyCommands(commands);
      await get().refetchProject();
      await ipc.seek(get().playhead).catch(() => {});
      return true;
    } catch (e) {
      if (!quiet) get().toast(`Edit failed: ${errMsg(e)}`, "error");
      await get().refetchProject();
      return false;
    }
  },

  async undo() {
    try {
      const status = await ipc.undo();
      set({ canUndo: status.can_undo, canRedo: status.can_redo });
      await get().refetchProject();
      await ipc.seek(get().playhead).catch(() => {});
    } catch (e) {
      console.warn("undo:", e);
    }
  },

  async redo() {
    try {
      const status = await ipc.redo();
      set({ canUndo: status.can_undo, canRedo: status.can_redo });
      await get().refetchProject();
      await ipc.seek(get().playhead).catch(() => {});
    } catch (e) {
      console.warn("redo:", e);
    }
  },

  copySelection() {
    const { project, selection } = get();
    if (!project || !selection) return;
    const track = project.tracks.find((t) => t.id === selection.trackId);
    const clip = track?.clips.find((c) => c.id === selection.clipId);
    if (!track || !clip) return;
    set({ clipboard: { clip, trackKind: track.kind } });
    get().toast("Copied", "info");
  },

  async pasteAtPlayhead() {
    const { project, clipboard, playhead } = get();
    if (!project || !clipboard) return;
    const track =
      (get().selection && project.tracks.find((t) => t.id === get().selection!.trackId)?.kind === clipboard.trackKind
        ? project.tracks.find((t) => t.id === get().selection!.trackId)
        : undefined) ?? project.tracks.find((t) => t.kind === clipboard.trackKind);
    if (!track) {
      get().toast(`No ${clipboard.trackKind} track to paste onto`, "error");
      return;
    }
    await get().dispatch(pasteCommand(clipboard.clip, track.id, playhead));
  },

  async duplicateSelection() {
    const { project, selection } = get();
    if (!project || !selection) return;
    const track = project.tracks.find((t) => t.id === selection.trackId);
    const clip = track?.clips.find((c) => c.id === selection.clipId);
    if (!track || !clip) return;
    await get().dispatch(pasteCommand(clip, track.id, clip.position_secs + clipDurationSecs(clip)));
  },

  async ensureStarterTracks(quiet = false) {
    const project = get().project;
    if (!project || project.tracks.length > 0) return;
    await get().dispatch({ command: "AddTrack", kind: "video", name: "Video 1" }, true);
    await get().dispatch({ command: "AddTrack", kind: "audio", name: "Audio 1" }, true);
    await get().dispatch({ command: "AddTrack", kind: "caption", name: "Captions" }, true);
    if (!quiet) get().toast("Timeline ready — import media to start editing", "info");
  },

  async ensureTrack(kind, name) {
    const existing = get().project?.tracks.find((t) => t.kind === kind);
    if (existing) return existing;
    const ok = await get().dispatch({ command: "AddTrack", kind, name }, true);
    if (!ok) throw new Error(`Could not create ${kind} track`);
    const track = get().project?.tracks.find((t) => t.kind === kind);
    if (!track) throw new Error(`Track ${kind} missing after creation`);
    return track;
  },

  async importMediaSmart(path) {
    if (!get().project) {
      get().toast("Create or open a project first.", "error");
      return;
    }
    set({ importBusy: true });
    try {
      const ok = await get().dispatch({ command: "ImportMedia", path });
      if (!ok) return;
      const mediaList = get().project?.media ?? [];
      const media = mediaList[mediaList.length - 1];
      if (!media) return;
      await get().placeMediaOnTimeline(media.id, media.kind, true);
      get().toast(`Added ${basename(path)} to timeline`, "success");
    } finally {
      set({ importBusy: false });
    }
  },

  async placeMediaOnTimeline(mediaId, kind, quiet = false) {
    const project = get().project;
    if (!project) return;
    const media = project.media.find((m) => m.id === mediaId);
    if (!media) return;
    const trackKind: TrackKind = kind === "audio" ? "audio" : "video";
    const track = await get().ensureTrack(trackKind, trackKind === "video" ? "Video 1" : "Audio 1");
    const dur = media.duration_secs ?? 5;

    // Placing at the playhead is usually right, but if that would land on top of an
    // existing clip on this track (e.g. clicking an already-placed media item again),
    // fall back to appending after the last clip instead of failing with an overlap
    // error — CapCut-style "just make it fit" rather than a dead-end command failure.
    const desired = get().playhead;
    const overlaps = track.clips.some(
      (c) => desired < c.position_secs + clipDurationSecs(c) && c.position_secs < desired + dur,
    );
    const position = overlaps
      ? track.clips.reduce((end, c) => Math.max(end, c.position_secs + clipDurationSecs(c)), 0)
      : desired;

    const ok = await get().dispatch({
      command: "AddClip",
      track_id: track.id,
      media_id: mediaId,
      position_secs: position,
      source_in_secs: 0,
      source_out_secs: dur,
    });
    if (ok && !quiet) get().toast(`Added ${basename(media.path)} to timeline`, "success");
  },

  async startPlayback() {
    if (!get().project) return;
    set({ playing: true });
    try {
      await ipc.play(get().playhead);
    } catch (e) {
      console.warn("playback:", e);
      set({ playing: false });
    }
  },

  stopPlayback() {
    set({ playing: false });
    ipc
      .pause()
      .then((timeSecs) => set({ playhead: timeSecs }))
      .catch(() => {});
  },

  async seekTo(secs) {
    const clamped = Math.max(0, secs);
    set({ playhead: clamped });
    try {
      await ipc.seek(clamped);
    } catch (e) {
      console.warn("seek:", e);
    }
  },

  scrubAt(secs) {
    const clamped = Math.max(0, secs);
    set({ playhead: clamped });
    void ipc.scrubAudio(clamped).catch(() => {});
  },

  onPlaybackTick(payload) {
    set({ playhead: payload.time_secs });
  },
  onPlaybackState(payload) {
    set({ playing: payload.playing, playhead: payload.time_secs });
  },
}));

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function basename(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

function pasteCommand(clip: Clip, trackId: string, positionSecs: number): Record<string, unknown> {
  if (clip.type === "caption") {
    return addCaption(trackId, clip.text, positionSecs, clip.duration_secs, clip.style_id);
  }
  return addClip(trackId, clip.media_id, positionSecs, clip.source_in_secs, clip.source_out_secs);
}

/// Wires backend events (playback + project mutation) into the store. Call once at app
/// startup. Returns an unsubscribe function.
export function connectStoreToBackendEvents(): () => void {
  const unsubTick = ipc.onPlaybackTick((p) => useEditorStore.getState().onPlaybackTick(p));
  const unsubState = ipc.onPlaybackState((p) => useEditorStore.getState().onPlaybackState(p));
  const unsubChanged = ipc.onProjectChanged((p) => {
    useEditorStore.setState({ canUndo: p.can_undo, canRedo: p.can_redo });
    void useEditorStore.getState().refetchProject();
  });
  return () => {
    unsubTick();
    unsubState();
    unsubChanged();
  };
}
