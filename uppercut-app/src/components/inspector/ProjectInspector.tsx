import { useEditorStore } from "../../store/editorStore";

export function ProjectInspector() {
  const project = useEditorStore((s) => s.project);
  if (!project) {
    return (
      <div className="inspector-empty">
        <div className="icon">✦</div>
        <p>Select a clip on the timeline to edit its properties.</p>
      </div>
    );
  }

  return (
    <div className="inspector">
      <div className="inspector-section">
        <h3>Project</h3>
        <p>{project.name}</p>
        <p className="empty-hint">
          {project.settings.width}×{project.settings.height} · {project.settings.fps} fps
        </p>
      </div>
    </div>
  );
}
