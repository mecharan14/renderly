import { useEffect, useRef } from "react";
import { useEditorStore } from "../../store/editorStore";
import * as ipc from "../../lib/ipc";
import { TransportBar } from "./TransportBar";

/// Fits the project's aspect ratio inside the host, letterboxed, and sends that sub-rect
/// (not the full host rect) to the backend — the native wgpu child window is sized to
/// exactly the letterboxed content area, so the host's dark background shows through as
/// pillar/letterbox bars. See docs/architecture.md "Playback engine".
async function syncPreviewBounds(host: HTMLElement, aspect: number) {
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  const rect = host.getBoundingClientRect();
  if (rect.width < 1 || rect.height < 1) return;

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
  const x = rect.left + (rect.width - width) / 2;
  const y = rect.top + (rect.height - height) / 2;

  try {
    await ipc.setPreviewBounds(Math.round(x), Math.round(y), Math.round(width), Math.round(height));
  } catch (e) {
    console.warn("preview bounds:", e);
  }
}

export function PreviewPanel() {
  const project = useEditorStore((s) => s.project);
  const importBusy = useEditorStore((s) => s.importBusy);
  const hostRef = useRef<HTMLDivElement | null>(null);

  const hasClips = !!project?.tracks.some((t) => t.clips.length > 0);
  const aspect = project ? project.settings.width / project.settings.height : 9 / 16;

  // Bounds-sync and frame-render must be sequenced, not raced: the native preview
  // surface is only created the first time `set_preview_bounds` runs, and rendering a
  // frame before that (e.g. two independent effects firing in parallel) silently drops
  // the frame — see docs/architecture.md "Playback engine". `ResizeObserver` also fires
  // once immediately on `observe()`, so this one callback covers both mount and resize,
  // exactly mirroring the pre-React app's `syncPreviewBounds().then(refreshPreview)`.
  useEffect(() => {
    const host = hostRef.current;
    if (!host || !project) return;

    const syncAndRender = async () => {
      await syncPreviewBounds(host, aspect);
      if (!useEditorStore.getState().playing) {
        await ipc.seek(useEditorStore.getState().playhead).catch(() => {});
      }
    };

    void syncAndRender();
    const observer = new ResizeObserver(() => void syncAndRender());
    observer.observe(host);
    return () => observer.disconnect();
  }, [project, aspect]);

  let hintContent: React.ReactNode;
  if (importBusy) {
    hintContent = (
      <>
        <span className="icon spinner" />
        <span>Importing your media…</span>
      </>
    );
  } else if (!project) {
    hintContent = (
      <>
        <span className="icon">🎬</span>
        <span>Create or import to start editing</span>
      </>
    );
  } else if (!hasClips) {
    hintContent = (
      <>
        <span className="icon">⬆</span>
        <span>
          <strong>Import a video</strong>
          <br />
          Click the media panel or Import button
        </span>
      </>
    );
  } else {
    hintContent = (
      <>
        <span className="icon">▶</span>
        <span>Press play to preview</span>
      </>
    );
  }

  return (
    <div className="preview-column">
      <section
        id="preview-host"
        ref={hostRef}
        className={`preview-host${hasClips ? " has-clips" : ""}${importBusy ? " loading" : ""}`}
      >
        <div className="hint">{hintContent}</div>
      </section>
      <TransportBar />
    </div>
  );
}
