import { useState } from "react";
import { useEditorStore } from "../../store/editorStore";
import { fileName } from "../../lib/format";
import { pickAndImportMedia } from "../../lib/projectFlows";
import { startMediaDrag } from "../../lib/dragMedia";

export function MediaPanel() {
  const project = useEditorStore((s) => s.project);
  const placeMediaOnTimeline = useEditorStore((s) => s.placeMediaOnTimeline);
  const [dragOver, setDragOver] = useState(false);

  const items = (project?.media ?? []).filter((m) => m.kind !== "audio");

  return (
    <div className="panel-body">
      <div
        className={`drop-zone${dragOver ? " drag-over" : ""}`}
        onClick={() => void pickAndImportMedia()}
        onDragOver={(e) => {
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDragOver(false);
        }}
      >
        <strong>Drop video here</strong>
        <span>or click to browse files</span>
      </div>

      {items.length === 0 ? (
        <p className="empty-hint">
          Imported files appear here. Each one is added to the timeline automatically.
        </p>
      ) : (
        items.map((item) => (
          <div
            key={item.id}
            className="media-item"
            draggable
            onDragStart={(e) => startMediaDrag(e, item.id, item.kind, item.duration_secs ?? 5)}
            onClick={() => void placeMediaOnTimeline(item.id, item.kind)}
          >
            <div className={`media-thumb ${item.kind}`}>{item.kind === "image" ? "🖼" : "▶"}</div>
            <div className="media-meta">
              <div className="name">{fileName(item.path)}</div>
              <div className="sub">
                {item.kind} · {item.duration_secs?.toFixed(1) ?? "?"}s
              </div>
            </div>
            <span className="media-add-hint">+ timeline</span>
          </div>
        ))
      )}
    </div>
  );
}
