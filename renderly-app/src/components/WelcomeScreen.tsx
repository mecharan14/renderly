import { Upload, FolderOpen, FilePlus } from "lucide-react";
import {
  createNewProjectFlow,
  openExistingProjectFlow,
  quickStartWithImport,
} from "../lib/projectFlows";
import { useEditorStore } from "../store/editorStore";
import { IconButton } from "./ui/IconButton";

export function WelcomeScreen() {
  const hasProject = useEditorStore((s) => !!s.project);

  return (
    <div className={`welcome-screen${hasProject ? " hidden" : ""}`}>
      <div className="welcome-card">
        <div className="welcome-brand" aria-hidden>
          <svg width="28" height="28" viewBox="0 0 16 16">
            <path
              d="M3 3.5 L8 8 L3 12.5 M8 8 L13 3.5 M8 8 L13 12.5"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </div>
        <h1>Renderly</h1>
        <p className="welcome-lead">Import a clip and start editing.</p>
        <div className="welcome-actions">
          <IconButton
            icon={Upload}
            label="Import video"
            variant="primary"
            onClick={() => void quickStartWithImport()}
          />
          <IconButton
            icon={FolderOpen}
            label="Open project"
            onClick={() => void openExistingProjectFlow()}
          />
          <IconButton
            icon={FilePlus}
            label="New project"
            variant="ghost"
            onClick={() => void createNewProjectFlow()}
          />
        </div>
      </div>
    </div>
  );
}
