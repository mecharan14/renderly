import { useEffect, useRef } from "react";
import { useEditorStore } from "../../store/editorStore";
import { renderTimeline } from "../../timeline/renderer";
import { useTimelineInteractions } from "../../timeline/interactions";
import { readMediaDrag } from "../../lib/dragMedia";
import { hitTestClip, secsFromCanvasX, snapTime, trackIndexAtY } from "../../timeline/layout";

function currentRenderState() {
  const s = useEditorStore.getState();
  return {
    project: s.project,
    playheadSecs: s.playhead,
    selection: s.selection,
    pxPerSec: s.pxPerSec,
    scrollX: s.scrollX,
    scrollY: s.scrollY,
    dragGhost: s.dragGhost,
    snapGuideSecs: s.snapGuideSecs,
    mediaAssets: s.mediaAssets,
  };
}

export function TimelineCanvas() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const project = useEditorStore((s) => s.project);
  const playhead = useEditorStore((s) => s.playhead);
  const selection = useEditorStore((s) => s.selection);
  const pxPerSec = useEditorStore((s) => s.pxPerSec);
  const scrollX = useEditorStore((s) => s.scrollX);
  const scrollY = useEditorStore((s) => s.scrollY);
  const toolMode = useEditorStore((s) => s.toolMode);
  const dragGhost = useEditorStore((s) => s.dragGhost);
  const snapGuideSecs = useEditorStore((s) => s.snapGuideSecs);
  const mediaAssets = useEditorStore((s) => s.mediaAssets);
  const snapEnabled = useEditorStore((s) => s.snapEnabled);
  const setDragGhost = useEditorStore((s) => s.setDragGhost);
  const dropMediaOnTimeline = useEditorStore((s) => s.dropMediaOnTimeline);
  const openContextMenu = useEditorStore((s) => s.openContextMenu);

  useTimelineInteractions(canvasRef);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    renderTimeline(canvas, {
      project,
      playheadSecs: playhead,
      selection,
      pxPerSec,
      scrollX,
      scrollY,
      dragGhost,
      snapGuideSecs,
      mediaAssets,
    });
  }, [
    project,
    playhead,
    selection,
    pxPerSec,
    scrollX,
    scrollY,
    dragGhost,
    snapGuideSecs,
    mediaAssets,
  ]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const observer = new ResizeObserver(() => renderTimeline(canvas, currentRenderState()));
    observer.observe(canvas);
    return () => observer.disconnect();
  }, []);

  return (
    <canvas
      ref={canvasRef}
      id="timeline"
      className={toolMode === "razor" ? "razor-mode" : ""}
      onDragOver={(e) => {
        const payload = readMediaDrag(e);
        if (!payload || !project) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";

        const rect = e.currentTarget.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;
        const rawSecs = secsFromCanvasX(x, pxPerSec, scrollX);
        const positionSecs = snapTime(rawSecs, project, playhead, pxPerSec, snapEnabled);
        const trackIndex = trackIndexAtY(rect.height, project.tracks.length, y, scrollY);
        const targetTrack = project.tracks[trackIndex];
        const wantsKind = payload.kind === "audio" ? "audio" : "video";
        const valid = !targetTrack || targetTrack.kind === wantsKind;

        setDragGhost({
          mediaId: payload.mediaId,
          kind: payload.kind,
          durationSecs: payload.durationSecs,
          positionSecs,
          trackIndex,
          valid,
        });
      }}
      onDragLeave={() => setDragGhost(null)}
      onDrop={(e) => {
        const payload = readMediaDrag(e);
        if (!payload) return;
        e.preventDefault();
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
  );
}
