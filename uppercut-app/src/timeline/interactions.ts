// Timeline mouse interaction state machine: hit-test → drag → commit-on-mouseup. Drag
// mutates the store's `project` immutably for live visual feedback (optimistic local
// mutation), then issues exactly one command on mouseup — see docs/architecture.md.

import { useEffect, useRef, type RefObject } from "react";
import * as ipc from "../lib/ipc";
import { deleteClip, moveClip, setCaption, splitClip, trimClip } from "../lib/commands";
import type { Clip, Project } from "../lib/types";
import { useEditorStore } from "../store/editorStore";
import { hitTestClip, RULER_H, secsFromCanvasX, snapTime, trackIndexAtY } from "./layout";

type DragState =
  | {
      mode: "move";
      /// Track the clip lived on when the gesture started — this is what the eventual
      /// `MoveClip.track_id` command param must be (the backend project hasn't moved
      /// yet), even though the clip may have hopped across several tracks locally
      /// during the drag (see `currentTrackId`).
      origTrackId: string;
      currentTrackId: string;
      clipId: string;
      startX: number;
      origPos: number;
    }
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
  | { mode: "trim-right"; trackId: string; clipId: string; startX: number; origOut: number };

function withClip(project: Project, trackId: string, clipId: string, update: (c: Clip) => Clip): Project {
  return {
    ...project,
    tracks: project.tracks.map((t) =>
      t.id !== trackId
        ? t
        : { ...t, clips: t.clips.map((c) => (c.id === clipId ? update(c) : c)) },
    ),
  };
}

function findClip(project: Project, trackId: string, clipId: string): Clip | undefined {
  return project.tracks.find((t) => t.id === trackId)?.clips.find((c) => c.id === clipId);
}

/// Relocates a clip from one track's clip list to another (local optimistic state only —
/// no command dispatched here). No-op if the clip isn't found on `srcTrackId`.
function moveClipAcrossTracks(
  project: Project,
  srcTrackId: string,
  destTrackId: string,
  clipId: string,
  newPositionSecs: number,
): Project {
  let moved: Clip | undefined;
  const withoutSrc = project.tracks.map((t) => {
    if (t.id !== srcTrackId) return t;
    const clip = t.clips.find((c) => c.id === clipId);
    if (clip) moved = { ...clip, position_secs: newPositionSecs };
    return { ...t, clips: t.clips.filter((c) => c.id !== clipId) };
  });
  if (!moved) return project;
  const movedClip = moved;
  return {
    ...project,
    tracks: withoutSrc.map((t) => (t.id === destTrackId ? { ...t, clips: [...t.clips, movedClip] } : t)),
  };
}

export function useTimelineInteractions(canvasRef: RefObject<HTMLCanvasElement | null>) {
  const dragStateRef = useRef<DragState | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const onWheel = (ev: WheelEvent) => {
      if (!ev.ctrlKey && !ev.metaKey) return;
      ev.preventDefault();
      const store = useEditorStore.getState();
      store.setZoom(store.pxPerSec + (ev.deltaY > 0 ? -10 : 10));
    };

    const onMouseDown = (ev: MouseEvent) => {
      const store = useEditorStore.getState();
      const project = store.project;
      if (!project) return;
      const rect = canvas.getBoundingClientRect();
      const x = ev.clientX - rect.left;
      const y = ev.clientY - rect.top;
      const hit = hitTestClip(project, x, y, rect.height, store.pxPerSec);

      if (hit) {
        const hitTrack = project.tracks.find((t) => t.id === hit.trackId);
        const locked = hitTrack?.locked ?? false;

        if (locked) {
          store.select({ trackId: hit.trackId, clipId: hit.clip.id });
          store.toast("Track is locked — unlock it to edit with the mouse", "info");
          return;
        }

        if (store.toolMode === "razor") {
          const at = snapTime(
            secsFromCanvasX(x, store.pxPerSec),
            project,
            store.playhead,
            store.pxPerSec,
            store.snapEnabled && !ev.altKey,
          );
          void store.dispatch(splitClip(hit.trackId, hit.clip.id, at));
          return;
        }

        store.select({ trackId: hit.trackId, clipId: hit.clip.id });
        if (store.toolMode === "select" && hit.edge === "left" && hit.clip.type === "caption") {
          dragStateRef.current = {
            mode: "trim-left-caption",
            trackId: hit.trackId,
            clipId: hit.clip.id,
            startX: x,
            origPos: hit.clip.position_secs,
            origDur: hit.clip.duration_secs,
          };
        } else if (store.toolMode === "select" && hit.edge === "left" && hit.clip.type !== "caption") {
          dragStateRef.current = {
            mode: "trim-left",
            trackId: hit.trackId,
            clipId: hit.clip.id,
            startX: x,
            origIn: hit.clip.source_in_secs,
            origOut: hit.clip.source_out_secs,
            origPos: hit.clip.position_secs,
          };
        } else if (store.toolMode === "select" && hit.edge === "right") {
          dragStateRef.current = {
            mode: "trim-right",
            trackId: hit.trackId,
            clipId: hit.clip.id,
            startX: x,
            origOut: hit.clip.type === "caption" ? hit.clip.duration_secs : hit.clip.source_out_secs,
          };
        } else if (store.toolMode === "select") {
          dragStateRef.current = {
            mode: "move",
            origTrackId: hit.trackId,
            currentTrackId: hit.trackId,
            clipId: hit.clip.id,
            startX: x,
            origPos: hit.clip.position_secs,
          };
        }
        return;
      }

      if (store.toolMode === "razor") return;

      if (y >= RULER_H) {
        const secs = snapTime(
          secsFromCanvasX(x, store.pxPerSec),
          project,
          store.playhead,
          store.pxPerSec,
          store.snapEnabled && !ev.altKey,
        );
        store.setPlayheadLocal(secs);
        store.select(null);
        // Renders the frame and plays a short audio blip in one coalesced backend
        // request — no need to also call `seek`.
        void ipc.scrubAudio(secs).catch(() => {});
      }
    };

    const onMouseMove = (ev: MouseEvent) => {
      const drag = dragStateRef.current;
      const store = useEditorStore.getState();
      const project = store.project;
      if (!drag || !project) return;
      const rect = canvas.getBoundingClientRect();
      const x = ev.clientX - rect.left;
      const y = ev.clientY - rect.top;
      const delta = (x - drag.startX) / store.pxPerSec;

      let nextProject: Project | null = null;

      if (drag.mode === "move") {
        const clip = findClip(project, drag.currentTrackId, drag.clipId);
        if (!clip) return;
        const rawPos = Math.max(0, drag.origPos + delta);
        const pos = snapTime(
          rawPos,
          project,
          store.playhead,
          store.pxPerSec,
          store.snapEnabled && !ev.altKey,
          clip.id,
        );
        store.setSnapGuide(Math.abs(pos - rawPos) > 1e-6 ? pos : null);

        const destIndex = trackIndexAtY(rect.height, project.tracks.length, y);
        const destTrack = project.tracks[destIndex];
        const srcTrack = project.tracks.find((t) => t.id === drag.currentTrackId);
        const canCrossMove =
          destTrack && srcTrack && destTrack.id !== srcTrack.id && destTrack.kind === srcTrack.kind && !destTrack.locked;

        if (canCrossMove) {
          nextProject = moveClipAcrossTracks(project, drag.currentTrackId, destTrack.id, clip.id, pos);
          dragStateRef.current = { ...drag, currentTrackId: destTrack.id };
        } else {
          nextProject = withClip(project, drag.currentTrackId, clip.id, (c) => ({ ...c, position_secs: pos }));
        }
      } else {
        const clip = findClip(project, drag.trackId, drag.clipId);
        if (!clip) return;

        if (drag.mode === "trim-right") {
          nextProject = withClip(project, drag.trackId, drag.clipId, (c) =>
            c.type === "caption"
              ? { ...c, duration_secs: Math.max(0.1, drag.origOut + delta) }
              : { ...c, source_out_secs: Math.max(c.source_in_secs + 0.05, drag.origOut + delta) },
          );
        } else if (drag.mode === "trim-left" && clip.type !== "caption") {
          const newIn = Math.min(drag.origOut - 0.05, drag.origIn + delta);
          const inDelta = newIn - drag.origIn;
          nextProject = withClip(project, drag.trackId, drag.clipId, (c) =>
            c.type === "caption"
              ? c
              : { ...c, source_in_secs: newIn, position_secs: drag.origPos + inDelta },
          );
        } else if (drag.mode === "trim-left-caption" && clip.type === "caption") {
          const newPos = snapTime(
            Math.max(0, drag.origPos + delta),
            project,
            store.playhead,
            store.pxPerSec,
            store.snapEnabled && !ev.altKey,
            clip.id,
          );
          const posDelta = newPos - drag.origPos;
          nextProject = withClip(project, drag.trackId, drag.clipId, (c) =>
            c.type === "caption"
              ? { ...c, position_secs: newPos, duration_secs: Math.max(0.1, drag.origDur - posDelta) }
              : c,
          );
        }
      }

      if (nextProject) useEditorStore.setState({ project: nextProject });
    };

    const onMouseUp = (ev: MouseEvent) => {
      const drag = dragStateRef.current;
      dragStateRef.current = null;
      const store = useEditorStore.getState();
      store.setSnapGuide(null);
      const project = store.project;
      if (!drag || !project) return;

      if (drag.mode === "move") {
        const clip = findClip(project, drag.currentTrackId, drag.clipId);
        if (!clip) return;
        const pos = snapTime(
          clip.position_secs,
          project,
          store.playhead,
          store.pxPerSec,
          store.snapEnabled && !ev.altKey,
          clip.id,
        );
        const newTrackId = drag.currentTrackId !== drag.origTrackId ? drag.currentTrackId : null;
        void store.dispatch(moveClip(drag.origTrackId, clip.id, pos, newTrackId));
        return;
      }

      const clip = findClip(project, drag.trackId, drag.clipId);
      if (!clip) return;

      if (drag.mode === "trim-right") {
        if (clip.type === "caption") {
          void store.dispatch(
            setCaption(drag.trackId, clip.id, { durationSecs: Math.max(0.1, clip.duration_secs) }),
          );
        } else {
          void store.dispatch(trimClip(drag.trackId, clip.id, null, clip.source_out_secs));
        }
      } else if (drag.mode === "trim-left-caption" && clip.type === "caption") {
        void store.dispatch(
          setCaption(drag.trackId, clip.id, {
            positionSecs: clip.position_secs,
            durationSecs: Math.max(0.1, clip.duration_secs),
          }),
        );
      } else if (drag.mode === "trim-left" && clip.type !== "caption") {
        const inDelta = clip.source_in_secs - drag.origIn;
        if (Math.abs(inDelta) > 1e-6) {
          // One atomic batch, not two sequential dispatches: TrimClip alone (position
          // unchanged) and MoveClip alone are each individually valid, so applying them
          // as separate commands used to flicker the clip back to its pre-drag position
          // between the two, and could leave it trimmed-but-not-repositioned forever if
          // the second dispatch's overlap check failed after the first had already
          // committed. A single undo step for what is, from the user's perspective, one
          // gesture.
          void store.dispatchBatch([
            trimClip(drag.trackId, clip.id, clip.source_in_secs, null),
            moveClip(drag.trackId, clip.id, clip.position_secs),
          ]);
        }
      }
    };

    canvas.addEventListener("wheel", onWheel, { passive: false });
    canvas.addEventListener("mousedown", onMouseDown);
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      canvas.removeEventListener("wheel", onWheel);
      canvas.removeEventListener("mousedown", onMouseDown);
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [canvasRef]);
}

/// Split-at-playhead / delete used by toolbar buttons and keyboard shortcuts — not part
/// of the mouse drag state machine, but colocated here since they share the same command
/// vocabulary.
export async function splitSelectedAtPlayhead(): Promise<void> {
  const store = useEditorStore.getState();
  if (!store.selection) {
    store.toast("Select a clip first, then split at the playhead", "info");
    return;
  }
  const track = store.project?.tracks.find((t) => t.id === store.selection!.trackId);
  if (track?.locked) {
    store.toast("Track is locked — unlock it to edit", "info");
    return;
  }
  await store.dispatch(splitClip(store.selection.trackId, store.selection.clipId, store.playhead));
}

export async function deleteSelected(ripple: boolean): Promise<void> {
  const store = useEditorStore.getState();
  if (!store.selection) return;
  const track = store.project?.tracks.find((t) => t.id === store.selection!.trackId);
  if (track?.locked) {
    store.toast("Track is locked — unlock it to edit", "info");
    return;
  }
  await store.dispatch(deleteClip(store.selection.trackId, store.selection.clipId, ripple));
  store.select(null);
}
