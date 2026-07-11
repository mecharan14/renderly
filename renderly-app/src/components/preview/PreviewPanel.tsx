import { useEffect, useRef, useState } from "react";
import { Film, Upload, Play } from "lucide-react";
import { useEditorStore } from "../../store/editorStore";
import * as ipc from "../../lib/ipc";
import { TransportBar } from "./TransportBar";
import { PreviewHandlesOverlay } from "./PreviewHandlesOverlay";
import { PreviewMaskOverlay } from "./PreviewMaskOverlay";

/// Fits the project's aspect ratio inside the host, letterboxed, and sends that sub-rect
/// (not the full host rect) to the backend — the native wgpu child window is sized to
/// exactly the letterboxed content area, so the host's dark background shows through as
/// pillar/letterbox bars. See docs/architecture.md "Playback engine".
async function syncPreviewBounds(
  host: HTMLElement,
  aspect: number,
  last: { x: number; y: number; w: number; h: number } | null,
): Promise<{ x: number; y: number; w: number; h: number } | null> {
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  const rect = host.getBoundingClientRect();
  if (rect.width < 2 || rect.height < 2) return last;

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
  const x = Math.round(rect.left + (rect.width - width) / 2);
  const y = Math.round(rect.top + (rect.height - height) / 2);
  const w = Math.round(width);
  const h = Math.round(height);

  if (last && last.x === x && last.y === y && last.w === w && last.h === h) {
    return last;
  }

  try {
    await ipc.setPreviewBounds(x, y, w, h);
  } catch (e) {
    console.warn("preview bounds:", e);
    return last;
  }
  return { x, y, w, h };
}

export function PreviewPanel() {
  const project = useEditorStore((s) => s.project);
  const importBusy = useEditorStore((s) => s.importBusy);
  const playing = useEditorStore((s) => s.playing);
  const hostRef = useRef<HTMLDivElement | null>(null);
  const lastBoundsRef = useRef<{ x: number; y: number; w: number; h: number } | null>(null);
  const [fullscreen, setFullscreen] = useState(false);

  const hasClips = !!project?.tracks.some((t) => t.clips.length > 0);
  const aspect = project ? project.settings.width / project.settings.height : 9 / 16;

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !project) return;

    let cancelled = false;
    let debounceTimer: number | null = null;

    const sync = async (opts?: { seek?: boolean }) => {
      if (cancelled) return;
      const next = await syncPreviewBounds(host, aspect, lastBoundsRef.current);
      if (cancelled || !next) return;
      const changed =
        !lastBoundsRef.current ||
        lastBoundsRef.current.w !== next.w ||
        lastBoundsRef.current.h !== next.h;
      lastBoundsRef.current = next;
      // Only re-seek when size actually changed and we're paused — avoids flicker storms
      // during window drag (position-only updates).
      if (opts?.seek !== false && changed && !useEditorStore.getState().playing) {
        await ipc.seek(useEditorStore.getState().playhead).catch(() => {});
      }
    };

    const schedule = (seek = true) => {
      if (debounceTimer != null) window.clearTimeout(debounceTimer);
      debounceTimer = window.setTimeout(() => void sync({ seek }), 32);
    };

    void sync({ seek: true });
    // Immediate follow-up after layout settles (fullscreen toggle, etc.)
    const raf = requestAnimationFrame(() => void sync({ seek: true }));

    const observer = new ResizeObserver(() => schedule(true));
    observer.observe(host);

    const unlistenGeom = ipc.onWindowGeometryChange(() => schedule(false));

    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
      if (debounceTimer != null) window.clearTimeout(debounceTimer);
      observer.disconnect();
      unlistenGeom();
    };
  }, [project, aspect, fullscreen]);

  useEffect(() => {
    if (!fullscreen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setFullscreen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [fullscreen]);

  let hintContent: React.ReactNode = null;
  if (importBusy) {
    hintContent = (
      <>
        <span className="icon spinner" />
        <span>Importing…</span>
      </>
    );
  } else if (!project) {
    hintContent = (
      <>
        <span className="icon">
          <Film size={22} strokeWidth={1.5} />
        </span>
        <span>Import or open a project</span>
      </>
    );
  } else if (!hasClips) {
    hintContent = (
      <>
        <span className="icon">
          <Upload size={22} strokeWidth={1.5} />
        </span>
        <span>Drop media in the bin to begin</span>
      </>
    );
  } else if (!playing) {
    hintContent = (
      <>
        <span className="icon">
          <Play size={22} strokeWidth={1.5} />
        </span>
        <span>Space to play</span>
      </>
    );
  }

  return (
    <div className={`preview-column${fullscreen ? " preview-fullscreen" : ""}`}>
      <section
        id="preview-host"
        ref={hostRef}
        className={`preview-host${hasClips ? " has-clips" : ""}${importBusy ? " loading" : ""}${playing ? " is-playing" : ""}`}
      >
        {hintContent && <div className="hint">{hintContent}</div>}
        {hasClips && project ? (
          <>
            <PreviewHandlesOverlay hostRef={hostRef} aspect={aspect} />
            <PreviewMaskOverlay hostRef={hostRef} aspect={aspect} />
          </>
        ) : null}
      </section>
      <TransportBar
        fullscreen={fullscreen}
        onToggleFullscreen={() => setFullscreen((v) => !v)}
      />
    </div>
  );
}
