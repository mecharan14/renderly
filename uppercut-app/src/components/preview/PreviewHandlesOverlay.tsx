import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { setClipTransform } from "../../lib/commands";
import * as ipc from "../../lib/ipc";
import {
  applyHandleDrag,
  hitTestHandle,
  ndcToPx,
  pxToNdc,
  resolveTransform,
  rotationHandleNdc,
  transformedCorners,
  type HandleKind,
} from "../../lib/previewMath";
import type { ClipTransform, MediaClip } from "../../lib/types";
import { clipDurationSecs } from "../../lib/types";
import { useEditorStore } from "../../store/editorStore";

type ContentBounds = { left: number; top: number; width: number; height: number };

function contentBoundsInHost(
  host: HTMLElement,
  aspect: number,
): ContentBounds | null {
  const rect = host.getBoundingClientRect();
  if (rect.width < 2 || rect.height < 2) return null;
  const hostAspect = rect.width / rect.height;
  let width: number;
  let height: number;
  if (hostAspect > aspect) {
    height = rect.height;
    width = height * aspect;
  } else {
    width = rect.width;
    height = width / aspect;
  }
  return {
    left: (rect.width - width) / 2,
    top: (rect.height - height) / 2,
    width,
    height,
  };
}

function activeVideoSelection(
  playhead: number,
): { trackId: string; clip: MediaClip; locked: boolean } | null {
  const { project, selection } = useEditorStore.getState();
  if (!project || !selection) return null;
  const track = project.tracks.find((t) => t.id === selection.trackId);
  if (!track || track.kind !== "video" || track.hidden) return null;
  const clip = track.clips.find((c) => c.id === selection.clipId);
  if (!clip || clip.type !== "video" || !clip.enabled) return null;
  const end = clip.position_secs + clipDurationSecs(clip);
  if (playhead < clip.position_secs || playhead >= end) return null;
  return { trackId: track.id, clip, locked: track.locked };
}

function patchClipTransform(
  trackId: string,
  clipId: string,
  transform: ClipTransform,
) {
  const project = useEditorStore.getState().project;
  if (!project) return;
  useEditorStore.setState({
    project: {
      ...project,
      tracks: project.tracks.map((t) =>
        t.id !== trackId
          ? t
          : {
              ...t,
              clips: t.clips.map((c) =>
                c.id === clipId && c.type !== "caption" ? { ...c, transform } : c,
              ),
            },
      ),
    },
  });
}

type DragRef = {
  kind: HandleKind;
  trackId: string;
  clipId: string;
  startTransform: ClipTransform;
  startNdc: { x: number; y: number };
  lastPreviewMs: number;
};

export function PreviewHandlesOverlay({
  hostRef,
  aspect,
}: {
  hostRef: React.RefObject<HTMLDivElement | null>;
  aspect: number;
}) {
  const playhead = useEditorStore((s) => s.playhead);
  const selection = useEditorStore((s) => s.selection);
  const playing = useEditorStore((s) => s.playing);
  const project = useEditorStore((s) => s.project);
  const toolMode = useEditorStore((s) => s.toolMode);
  const dispatch = useEditorStore((s) => s.dispatch);
  const setDragging = useEditorStore((s) => s.setDragging);

  const [bounds, setBounds] = useState<ContentBounds | null>(null);
  const dragRef = useRef<DragRef | null>(null);
  const overlayRef = useRef<HTMLDivElement | null>(null);

  const active = useMemo(() => {
    if (playing || !project || toolMode === "mask") return null;
    return activeVideoSelection(playhead);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- selection/project drive refresh
  }, [playhead, playing, project, selection, toolMode]);

  const transform = resolveTransform(active?.clip.transform);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const sync = () => setBounds(contentBoundsInHost(host, aspect));
    sync();
    const ro = new ResizeObserver(sync);
    ro.observe(host);
    return () => ro.disconnect();
  }, [hostRef, aspect, project]);

  const previewOverride = useCallback(
    (trackId: string, clipId: string, t: ClipTransform) => {
      const now = performance.now();
      const drag = dragRef.current;
      if (drag && now - drag.lastPreviewMs < 40) return;
      if (drag) drag.lastPreviewMs = now;
      void ipc.previewTransformOverride(trackId, clipId, t, useEditorStore.getState().playhead);
    },
    [],
  );

  useEffect(() => {
    const onMove = (ev: MouseEvent) => {
      const drag = dragRef.current;
      const overlay = overlayRef.current;
      if (!drag || !overlay) return;
      const rect = overlay.getBoundingClientRect();
      const local = { x: ev.clientX - rect.left, y: ev.clientY - rect.top };
      const curNdc = pxToNdc(local, rect.width, rect.height);
      const next = applyHandleDrag(drag.kind, drag.startTransform, drag.startNdc, curNdc);
      patchClipTransform(drag.trackId, drag.clipId, next);
      previewOverride(drag.trackId, drag.clipId, next);
    };

    const onUp = async () => {
      const drag = dragRef.current;
      if (!drag) return;
      dragRef.current = null;
      setDragging(false);
      const projectNow = useEditorStore.getState().project;
      const clip = projectNow?.tracks
        .find((t) => t.id === drag.trackId)
        ?.clips.find((c) => c.id === drag.clipId);
      const t =
        clip && clip.type !== "caption"
          ? resolveTransform(clip.transform)
          : drag.startTransform;
      await dispatch(setClipTransform(drag.trackId, drag.clipId, t));
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [dispatch, previewOverride, setDragging]);

  if (!active || !bounds || active.locked || bounds.width < 8) return null;

  const w = bounds.width;
  const h = bounds.height;
  const corners = transformedCorners(transform).map((c) => ndcToPx(c, w, h));
  const rot = ndcToPx(rotationHandleNdc(transform), w, h);
  const poly = corners.map((c) => `${c.x},${c.y}`).join(" ");

  const onMouseDown = (ev: React.MouseEvent) => {
    if (ev.button !== 0 || active.locked) return;
    const overlay = overlayRef.current;
    if (!overlay) return;
    const rect = overlay.getBoundingClientRect();
    const local = { x: ev.clientX - rect.left, y: ev.clientY - rect.top };
    const kind = hitTestHandle(local, transform, w, h);
    if (!kind) return;
    ev.preventDefault();
    ev.stopPropagation();
    setDragging(true);
    dragRef.current = {
      kind,
      trackId: active.trackId,
      clipId: active.clip.id,
      startTransform: { ...transform },
      startNdc: pxToNdc(local, w, h),
      lastPreviewMs: 0,
    };
  };

  return (
    <div
      ref={overlayRef}
      className="preview-handles-overlay"
      style={{
        left: bounds.left,
        top: bounds.top,
        width: bounds.width,
        height: bounds.height,
      }}
      onMouseDown={onMouseDown}
    >
      <svg className="preview-handles-svg" width={w} height={h}>
        <polygon points={poly} className="preview-handle-box" />
        <line
          x1={(corners[2].x + corners[3].x) / 2}
          y1={(corners[2].y + corners[3].y) / 2}
          x2={rot.x}
          y2={rot.y}
          className="preview-handle-rot-line"
        />
        {corners.map((c, i) => (
          <circle key={i} cx={c.x} cy={c.y} r={5} className="preview-handle-knob" />
        ))}
        <circle cx={rot.x} cy={rot.y} r={5} className="preview-handle-knob rotate" />
      </svg>
    </div>
  );
}
