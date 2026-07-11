import { useEditorStore } from "../store/editorStore";
import {
  createNewProjectFlow,
  openExistingProjectFlow,
  quickStartWithImport,
} from "../lib/projectFlows";

export function WelcomeScreen() {
  const hasProject = useEditorStore((s) => !!s.project);

  return (
    <div className={`welcome-screen${hasProject ? " hidden" : ""}`}>
      <div className="welcome-card">
        <div className="logo">✂</div>
        <h1>Start editing in seconds</h1>
        <p>Pick a video and we'll set up your timeline automatically. No manual track setup.</p>
        <ol className="welcome-steps">
          <li>
            <span>1</span> Import your clip
          </li>
          <li>
            <span>2</span> Preview and trim
          </li>
          <li>
            <span>3</span> Export when done
          </li>
        </ol>
        <div className="welcome-actions">
          <button type="button" className="btn-primary" onClick={() => void quickStartWithImport()}>
            <span className="btn-icon">⬆</span>
            <span>Import video to start</span>
          </button>
          <button type="button" className="btn" onClick={() => void openExistingProjectFlow()}>
            <span className="btn-icon">📂</span>
            <span>Open existing project</span>
          </button>
          <button type="button" className="btn-ghost" onClick={() => void createNewProjectFlow()}>
            <span>Create project file…</span>
          </button>
        </div>
      </div>
    </div>
  );
}
