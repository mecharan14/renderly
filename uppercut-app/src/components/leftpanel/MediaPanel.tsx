import { useState } from "react";
import { useEditorStore, type ThumbnailAsset } from "../../store/editorStore";
import { fileName } from "../../lib/format";
import { pickAndImportMedia, importFromPath } from "../../lib/projectFlows";
import { startMediaDrag } from "../../lib/dragMedia";

/// Filmstrip thumbnail with hover-scrub: moving the mouse across the card selects which
/// strip tile to show, CapCut-style, instead of a single static frame.
function FilmstripThumb({ thumb, durationSecs }: { thumb: ThumbnailAsset; durationSecs: number }) {
  const [hoverFrac, setHoverFrac] = useState<number | null>(null);
  const tileCount = thumb.cols * thumb.rows;
  const frac = hoverFrac ?? 0;
  const sourceTime = frac * Math.max(durationSecs, 0);
  const tileIndex = Math.max(
    0,
    Math.min(tileCount - 1, thumb.intervalSecs > 0 ? Math.round(sourceTime / thumb.intervalSecs) : 0),
  );
  const col = tileIndex % thumb.cols;
  const row = Math.floor(tileIndex / thumb.cols);

  return (
    <div
      className="media-thumb video filmstrip"
      onMouseMove={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        setHoverFrac(Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width)));
      }}
      onMouseLeave={() => setHoverFrac(null)}
      style={{
        backgroundImage: `url(${thumb.stripUrl})`,
        backgroundPosition: `-${col * thumb.tileWidth}px -${row * thumb.tileHeight}px`,
        backgroundSize: `${thumb.cols * thumb.tileWidth}px ${thumb.rows * thumb.tileHeight}px`,
      }}
    />
  );
}

export function MediaPanel() {
  const project = useEditorStore((s) => s.project);
  const mediaAssets = useEditorStore((s) => s.mediaAssets);
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
          const file = e.dataTransfer.files?.[0];
          // In Tauri, OS drops usually arrive as `tauri://drag-drop` with real paths.
          // Browser-style drops may only expose a File name — still try when a path-like
          // string is present (some webviews populate `path` on the File object).
          const path =
            (file as File & { path?: string })?.path ||
            e.dataTransfer.getData("text/plain") ||
            "";
          if (path && /[/\\]/.test(path)) void importFromPath(path);
        }}
      >
        <strong>Drop video here</strong>
        <span>or click to browse files</span>
      </div>

      {items.length === 0 ? (
        <div className="empty-state">
          <div className="empty-state-icon">🎬</div>
          <p>
            <strong>No media yet</strong>
          </p>
          <p className="empty-hint">
            Drop a video above or click the drop zone. Each import is added to the timeline
            automatically.
          </p>
        </div>
      ) : (
        items.map((item) => {
          const thumb = mediaAssets[item.id]?.thumbnails;
          const pendingThumb = item.kind === "video" && !thumb;
          return (
            <div
              key={item.id}
              className="media-item"
              draggable
              onDragStart={(e) => startMediaDrag(e, item.id, item.kind, item.duration_secs ?? 5)}
              onClick={() => void placeMediaOnTimeline(item.id, item.kind)}
            >
              {thumb && thumb.image ? (
                <FilmstripThumb thumb={thumb} durationSecs={item.duration_secs ?? 0} />
              ) : pendingThumb ? (
                <div className="media-thumb skeleton" aria-label="Generating thumbnails" />
              ) : (
                <div className={`media-thumb ${item.kind}`}>{item.kind === "image" ? "🖼" : "▶"}</div>
              )}
              <div className="media-meta">
                <div className="name">{fileName(item.path)}</div>
                <div className="sub">
                  {item.kind} · {item.duration_secs?.toFixed(1) ?? "?"}s
                  {pendingThumb ? " · generating…" : ""}
                </div>
              </div>
              <span className="media-add-hint">+ timeline</span>
            </div>
          );
        })
      )}
    </div>
  );
}
