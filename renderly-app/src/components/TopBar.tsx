import { FilePlus, FolderOpen, Save, Undo2, Redo2, Download } from "lucide-react";
import { useEditorStore } from "../store/editorStore";
import { createNewProjectFlow, openExistingProjectFlow } from "../lib/projectFlows";
import { IconButton } from "./ui/IconButton";
import { WindowControls } from "./ui/WindowControls";
import * as ipc from "../lib/ipc";

function BrandMark() {
  return (
    <svg className="brand-mark-svg" width="16" height="16" viewBox="0 0 16 16" aria-hidden>
      <path
        d="M3 3.5 L8 8 L3 12.5 M8 8 L13 3.5 M8 8 L13 12.5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function TopBar({ onExport }: { onExport: () => void }) {
  const project = useEditorStore((s) => s.project);
  const canUndo = useEditorStore((s) => s.canUndo);
  const canRedo = useEditorStore((s) => s.canRedo);
  const undo = useEditorStore((s) => s.undo);
  const redo = useEditorStore((s) => s.redo);
  const saveProject = useEditorStore((s) => s.saveProject);

  const hasProject = !!project;

  return (
    <header
      className="topbar"
      data-tauri-drag-region
      onDoubleClick={() => void ipc.toggleMaximizeWindow()}
    >
      <div className="brand" data-tauri-drag-region>
        <BrandMark />
        <span className="brand-name">Renderly</span>
      </div>

      <div className="topbar-group">
        <IconButton
          icon={FilePlus}
          iconOnly
          tooltip="New project (Ctrl+N)"
          onClick={() => void createNewProjectFlow()}
        />
        <IconButton
          icon={FolderOpen}
          iconOnly
          tooltip="Open project"
          onClick={() => void openExistingProjectFlow()}
        />
        <IconButton
          icon={Save}
          iconOnly
          tooltip="Save (Ctrl+S)"
          onClick={() => void saveProject()}
        />
      </div>

      <div className="topbar-group">
        <IconButton
          icon={Undo2}
          iconOnly
          tooltip="Undo (Ctrl+Z)"
          disabled={!canUndo}
          onClick={() => void undo()}
        />
        <IconButton
          icon={Redo2}
          iconOnly
          tooltip="Redo (Ctrl+Y)"
          disabled={!canRedo}
          onClick={() => void redo()}
        />
      </div>

      <span className="spacer" data-tauri-drag-region />

      <div className={`project-chip${hasProject ? " live" : ""}`} data-tauri-drag-region>
        <span className="dot" />
        <span className="label">{project ? project.name : "No project"}</span>
      </div>

      <IconButton
        icon={Download}
        label="Export"
        variant="primary"
        tooltip="Export video"
        disabled={!hasProject}
        onClick={onExport}
      />

      <WindowControls />
    </header>
  );
}
