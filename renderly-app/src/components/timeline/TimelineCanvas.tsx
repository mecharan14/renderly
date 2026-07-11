import { useEffect, useRef } from "react";
import { useEditorStore } from "../../store/editorStore";
import { renderTimeline, renderTimelineOverlay } from "../../timeline/renderer";
import { useTimelineInteractions } from "../../timeline/interactions";
import { readMediaDrag } from "../../lib/dragMedia";
import { readTransitionDrag, transitionFromPayload } from "../../lib/dragTransition";
import { setClipTransition } from "../../lib/commands";
import { hitTestClip, hitTestTransitionJunction, secsFromCanvasX, trackIndexAtY } from "../../timeline/layout";
import { snapTime } from "../../timeline/snap";

function currentRenderState() {
  const s = useEditorStore.getState();
  return {
    project: s.project,
    playheadSecs: s.playhead,
    selection: s.selection,
    selections: s.selections,
    pxPerSec: s.pxPerSec,
    scrollX: s.scrollX,
    scrollY: s.scrollY,
    dragGhost: s.dragGhost,
    transitionDropTarget: s.transitionDropTarget,
    snapGuideSecs: s.snapGuideSecs,
    mediaAssets: s.mediaAssets,
  };
}

function currentOverlayState() {
  const s = useEditorStore.getState();
  return {
    playheadSecs: s.playhead,
    pxPerSec: s.pxPerSec,
    scrollX: s.scrollX,
    snapGuideSecs: s.snapGuideSecs,
    hasProject: !!s.project && s.project.tracks.length > 0,
  };
}

export function TimelineCanvas() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const overlayRef = useRef<HTMLCanvasElement | null>(null);
  const project = useEditorStore((s) => s.project);
  const playhead = useEditorStore((s) => s.playhead);
  const selection = useEditorStore((s) => s.selection);
  const selections = useEditorStore((s) => s.selections);
  const pxPerSec = useEditorStore((s) => s.pxPerSec);
  const scrollX = useEditorStore((s) => s.scrollX);
  const scrollY = useEditorStore((s) => s.scrollY);
  const toolMode = useEditorStore((s) => s.toolMode);
  const dragGhost = useEditorStore((s) => s.dragGhost);
  const transitionDropTarget = useEditorStore((s) => s.transitionDropTarget);
  const snapGuideSecs = useEditorStore((s) => s.snapGuideSecs);
  const mediaAssets = useEditorStore((s) => s.mediaAssets);
  const themeEpoch = useEditorStore((s) => s.themeEpoch);
  const snapEnabled = useEditorStore((s) => s.snapEnabled);
  const setDragGhost = useEditorStore((s) => s.setDragGhost);
  const setTransitionDropTarget = useEditorStore((s) => s.setTransitionDropTarget);
  const dispatch = useEditorStore((s) => s.dispatch);
  const dropMediaOnTimeline = useEditorStore((s) => s.dropMediaOnTimeline);
  const openContextMenu = useEditorStore((s) => s.openContextMenu);

  useTimelineInteractions(canvasRef);

  // Structural repaint: clips, thumbnails, waveforms, drag ghost, track lanes. Deliberately
  // excludes `playhead`/`snapGuideSecs` — those move ~30x/sec during playback/scrub and
  // live on the separate overlay canvas below instead, so they don't force this (much
  // more expensive) redraw on every tick (B1: this used to be one canvas/one effect).
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    renderTimeline(canvas, {
      project,
      playheadSecs: playhead,
      selection,
      selections,
      pxPerSec,
      scrollX,
      scrollY,
      dragGhost,
      transitionDropTarget,
      snapGuideSecs,
      mediaAssets,
    });
  }, [project, selection, selections, pxPerSec, scrollX, scrollY, dragGhost, transitionDropTarget, mediaAssets, themeEpoch]);

  // Transient overlay repaint: playhead + snap guide only.
  useEffect(() => {
    const canvas = overlayRef.current;
    if (!canvas) return;
    renderTimelineOverlay(canvas, {
      playheadSecs: playhead,
      pxPerSec,
      scrollX,
      snapGuideSecs,
      hasProject: !!project && project.tracks.length > 0,
    });
  }, [playhead, snapGuideSecs, pxPerSec, scrollX, project, themeEpoch]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const overlay = overlayRef.current;
    if (!canvas || !overlay) return;
    const observer = new ResizeObserver(() => {
      renderTimeline(canvas, currentRenderState());
      renderTimelineOverlay(overlay, currentOverlayState());
    });
    observer.observe(canvas);
    return () => observer.disconnect();
  }, []);

  return (
    <>
      <canvas
        ref={canvasRef}
        id="timeline"
        className={toolMode === "razor" ? "razor-mode" : ""}
        onDragOver={(e) => {
          const transitionPayload = readTransitionDrag(e);
          if (transitionPayload && project) {
            e.preventDefault();
            e.dataTransfer.dropEffect = "copy";
            setDragGhost(null);

            const rect = e.currentTarget.getBoundingClientRect();
            const x = e.clientX - rect.left;
            const y = e.clientY - rect.top;
            const hit = hitTestTransitionJunction(
              project,
              x,
              y,
              rect.height,
              pxPerSec,
              scrollX,
              scrollY,
            );
            if (!hit) {
              setTransitionDropTarget(null);
              return;
            }
            const track = project.tracks.find((t) => t.id === hit.trackId);
            const valid = !!track && track.kind === "video" && !track.locked;
            setTransitionDropTarget({ trackId: hit.trackId, clipId: hit.clipId, valid });
            return;
          }

          const payload = readMediaDrag(e);
          if (!payload || !project) return;
          e.preventDefault();
          e.dataTransfer.dropEffect = "copy";
          setTransitionDropTarget(null);

          const rect = e.currentTarget.getBoundingClientRect();
          const x = e.clientX - rect.left;
          const y = e.clientY - rect.top;
          const rawSecs = secsFromCanvasX(x, pxPerSec, scrollX);
          const positionSecs = snapTime(rawSecs, project, playhead, pxPerSec, snapEnabled);
          const trackIndex = trackIndexAtY(rect.height, project.tracks.length, y, scrollY);
          const targetTrack = project.tracks[trackIndex];
          const wantsKind = payload.kind === "audio" ? "audio" : "video";
          // Locked tracks are GUI-honored only (core deliberately still accepts the
          // resulting AddClip — see `Track::locked`'s doc comment) — the ghost must flag
          // this as invalid itself, matching the same check clip-drag cross-track moves
          // already apply in `interactions.ts`, or a drop would look accepted (green
          // ghost) and then just silently land on the locked track anyway.
          const valid = !targetTrack || (targetTrack.kind === wantsKind && !targetTrack.locked);

          setDragGhost({
            mediaId: payload.mediaId,
            kind: payload.kind,
            durationSecs: payload.durationSecs,
            positionSecs,
            trackIndex,
            valid,
          });
        }}
        onDragLeave={() => {
          setDragGhost(null);
          setTransitionDropTarget(null);
        }}
        onDrop={(e) => {
          const transitionPayload = readTransitionDrag(e);
          if (transitionPayload && project) {
            e.preventDefault();
            const rect = e.currentTarget.getBoundingClientRect();
            const x = e.clientX - rect.left;
            const y = e.clientY - rect.top;
            const hit = hitTestTransitionJunction(
              project,
              x,
              y,
              rect.height,
              pxPerSec,
              scrollX,
              scrollY,
            );
            setTransitionDropTarget(null);
            if (!hit) return;
            const track = project.tracks.find((t) => t.id === hit.trackId);
            if (!track || track.kind !== "video" || track.locked) return;
            void dispatch(
              setClipTransition(hit.trackId, hit.clipId, transitionFromPayload(transitionPayload)),
            );
            return;
          }

          const payload = readMediaDrag(e);
          if (!payload) return;
          e.preventDefault();
          setTransitionDropTarget(null);
          void dropMediaOnTimeline();
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          if (!project) return;
          const rect = e.currentTarget.getBoundingClientRect();
          const x = e.clientX - rect.left;
          const y = e.clientY - rect.top;
          const hit = hitTestClip(project, x, y, rect.height, pxPerSec, scrollX, scrollY);
          if (!hit) return;
          const atSecs = snapTime(
            secsFromCanvasX(x, pxPerSec, scrollX),
            project,
            playhead,
            pxPerSec,
            snapEnabled,
          );
          openContextMenu(e.clientX, e.clientY, hit.trackId, hit.clip.id, atSecs);
        }}
      />
      <canvas ref={overlayRef} id="timeline-overlay" />
    </>
  );
}
