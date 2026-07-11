import { useEffect, useRef, useState, type RefObject } from "react";
import { useEditorStore } from "../../store/editorStore";
import { deleteTrack, renameTrack, setTrackFlags } from "../../lib/commands";
import { trackLayout, TRACK_LABEL_W } from "../../timeline/layout";
import type { Track } from "../../lib/types";

/// DOM column of track headers overlaid on the TRACK_LABEL_W gutter the canvas reserves
/// but no longer draws into (see renderer.ts) — real interactive controls (dbl-click
/// rename, mute/lock/hide toggles, delete) instead of canvas hit-testing.
export function TrackHeaders({ containerRef }: { containerRef: RefObject<HTMLElement | null> }) {
  const project = useEditorStore((s) => s.project);
  const dispatch = useEditorStore((s) => s.dispatch);
  const [height, setHeight] = useState(0);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [menuOpen, setMenuOpen] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const update = () => setHeight(el.clientHeight);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, [containerRef]);

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setMenuOpen(null);
    };
    document.addEventListener("click", onDocClick);
    return () => document.removeEventListener("click", onDocClick);
  }, []);

  if (!project || project.tracks.length === 0 || height === 0) return null;

  const { trackH, laneTop } = trackLayout(height, project.tracks.length);

  function commitRename(track: Track) {
    setRenaming(null);
    const trimmed = renameValue.trim();
    if (trimmed && trimmed !== track.name) void dispatch(renameTrack(track.id, trimmed));
  }

  return (
    <div className="track-headers" style={{ width: TRACK_LABEL_W }} ref={rootRef}>
      {project.tracks.map((track, i) => (
        <div key={track.id} className="track-header-row" style={{ top: laneTop(i), height: trackH }}>
          <span className="track-header-icon">
            {track.kind === "video" ? "▶" : track.kind === "audio" ? "♪" : "T"}
          </span>
          {renaming === track.id ? (
            <input
              autoFocus
              className="track-header-rename"
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onBlur={() => commitRename(track)}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitRename(track);
                if (e.key === "Escape") setRenaming(null);
              }}
            />
          ) : (
            <span
              className={`track-header-name${track.hidden || track.muted ? " dimmed" : ""}`}
              title="Double-click to rename"
              onDoubleClick={() => {
                setRenaming(track.id);
                setRenameValue(track.name);
              }}
            >
              {track.name}
            </span>
          )}
          <div className="track-header-actions">
            <button
              type="button"
              className={`track-flag-btn${track.muted ? " active" : ""}`}
              title={track.muted ? "Unmute" : "Mute"}
              onClick={() => void dispatch(setTrackFlags(track.id, { muted: !track.muted }))}
            >
              M
            </button>
            <button
              type="button"
              className={`track-flag-btn${track.locked ? " active" : ""}`}
              title={track.locked ? "Unlock" : "Lock"}
              onClick={() => void dispatch(setTrackFlags(track.id, { locked: !track.locked }))}
            >
              L
            </button>
            <button
              type="button"
              className={`track-flag-btn${track.hidden ? " active" : ""}`}
              title={track.hidden ? "Show" : "Hide"}
              onClick={() => void dispatch(setTrackFlags(track.id, { hidden: !track.hidden }))}
            >
              H
            </button>
            <div className="track-header-menu">
              <button
                type="button"
                className="track-flag-btn"
                title="More"
                onClick={() => setMenuOpen(menuOpen === track.id ? null : track.id)}
              >
                ⋯
              </button>
              {menuOpen === track.id && (
                <div className="track-header-menu-pop">
                  <button
                    type="button"
                    onClick={async () => {
                      setMenuOpen(null);
                      await dispatch(deleteTrack(track.id));
                    }}
                  >
                    Delete track
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}
