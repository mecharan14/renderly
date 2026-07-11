import { useEffect, useRef, useState } from "react";
import { useEditorStore } from "../store/editorStore";
import { createNewProjectFlow, openExistingProjectFlow, pickAndImportMedia } from "../lib/projectFlows";
import { splitSelectedAtPlayhead } from "../timeline/interactions";
import type { TrackKind } from "../lib/types";

const TRACK_KIND_LABELS: [TrackKind, string][] = [
  ["video", "Video track"],
  ["audio", "Audio track"],
  ["caption", "Caption track"],
];

export function TopBar({ onExport }: { onExport: () => void }) {
  const project = useEditorStore((s) => s.project);
  const canUndo = useEditorStore((s) => s.canUndo);
  const canRedo = useEditorStore((s) => s.canRedo);
  const dispatch = useEditorStore((s) => s.dispatch);
  const undo = useEditorStore((s) => s.undo);
  const redo = useEditorStore((s) => s.redo);
  const saveProject = useEditorStore((s) => s.saveProject);
  const toast = useEditorStore((s) => s.toast);

  const [trackMenuOpen, setTrackMenuOpen] = useState(false);
  const trackMenuRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (trackMenuRef.current && !trackMenuRef.current.contains(e.target as Node)) {
        setTrackMenuOpen(false);
      }
    };
    document.addEventListener("click", onDocClick);
    return () => document.removeEventListener("click", onDocClick);
  }, []);

  const hasProject = !!project;

  return (
    <header className="topbar">
      <div className="brand">
        <span className="brand-mark">✂</span> Uppercut
      </div>

      <div className="topbar-group">
        <button type="button" className="btn" title="New project (Ctrl+N)" onClick={() => void createNewProjectFlow()}>
          <span className="btn-icon">＋</span>
          <span>New</span>
        </button>
        <button type="button" className="btn" title="Open project" onClick={() => void openExistingProjectFlow()}>
          <span className="btn-icon">📂</span>
          <span>Open</span>
        </button>
        <button type="button" className="btn" title="Save (Ctrl+S)" onClick={() => void saveProject()}>
          <span className="btn-icon">💾</span>
          <span>Save</span>
        </button>
      </div>

      <div className="topbar-group">
        <button
          type="button"
          className="btn btn-import"
          title="Import video or audio"
          disabled={!hasProject}
          onClick={() => void pickAndImportMedia()}
        >
          <span className="btn-icon">⬆</span>
          <span>Import</span>
        </button>
        <button
          type="button"
          className="btn"
          title="Split clip at playhead (S)"
          disabled={!hasProject}
          onClick={() => void splitSelectedAtPlayhead()}
        >
          <span className="btn-icon">✂</span>
          <span>Split</span>
        </button>
      </div>

      <div className="topbar-group">
        <button
          type="button"
          className="btn-icon-only"
          title="Undo (Ctrl+Z)"
          disabled={!canUndo}
          onClick={() => void undo()}
        >
          ↶
        </button>
        <button
          type="button"
          className="btn-icon-only"
          title="Redo (Ctrl+Y)"
          disabled={!canRedo}
          onClick={() => void redo()}
        >
          ↷
        </button>
      </div>

      <div className="track-menu" ref={trackMenuRef}>
        <button
          type="button"
          className="btn"
          disabled={!hasProject}
          onClick={() => setTrackMenuOpen((v) => !v)}
        >
          Track ▾
        </button>
        <div className={`track-menu-pop${trackMenuOpen ? " open" : ""}`}>
          {TRACK_KIND_LABELS.map(([kind, label]) => (
            <button
              key={kind}
              type="button"
              onClick={() => {
                setTrackMenuOpen(false);
                void dispatch({ command: "AddTrack", kind, name: label.replace(" track", "") }).then(
                  (ok) => ok && toast(`Added ${label.toLowerCase()}`, "success"),
                );
              }}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <span className="spacer" />

      <div className={`project-chip${hasProject ? " live" : ""}`}>
        <span className="dot" />
        <span className="label">{project ? project.name : "No project open"}</span>
      </div>

      <button
        type="button"
        className="btn-primary"
        title="Export video"
        disabled={!hasProject}
        onClick={onExport}
      >
        <span className="btn-icon">⬇</span>
        <span>Export</span>
      </button>
    </header>
  );
}
