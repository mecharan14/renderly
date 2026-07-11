import { useEditorStore } from "../../store/editorStore";

export function TimelineToolbar() {
  const toolMode = useEditorStore((s) => s.toolMode);
  const setTool = useEditorStore((s) => s.setTool);
  const snapEnabled = useEditorStore((s) => s.snapEnabled);
  const setSnap = useEditorStore((s) => s.setSnap);
  const pxPerSec = useEditorStore((s) => s.pxPerSec);
  const setZoom = useEditorStore((s) => s.setZoom);
  const fitZoom = useEditorStore((s) => s.fitZoom);
  const toast = useEditorStore((s) => s.toast);

  return (
    <div className="timeline-tools">
      <div className="tool-group">
        <button
          type="button"
          className={`tool-btn${toolMode === "select" ? " active" : ""}`}
          title="Move and trim clips (V)"
          onClick={() => setTool("select")}
        >
          Select
        </button>
        <button
          type="button"
          className={`tool-btn${toolMode === "razor" ? " active" : ""}`}
          title="Click a clip to split (C)"
          onClick={() => {
            setTool("razor");
            toast("Click a clip to split it", "info");
          }}
        >
          Split
        </button>
      </div>
      <button
        type="button"
        className={`tool-btn${snapEnabled ? " active" : ""}`}
        title="Snap to grid and clip edges"
        onClick={() => setSnap(!snapEnabled)}
      >
        Snap
      </button>
      <span className="tool-divider" />
      <div className="zoom-controls">
        <button type="button" className="btn-icon-only" onClick={() => setZoom(pxPerSec - 20)}>
          −
        </button>
        <span className="zoom-label">{Math.round((pxPerSec / 80) * 100)}%</span>
        <button type="button" className="btn-icon-only" onClick={() => setZoom(pxPerSec + 20)}>
          +
        </button>
        <button type="button" className="tool-btn" title="Fit timeline to window" onClick={() => fitZoom()}>
          Fit
        </button>
      </div>
    </div>
  );
}
