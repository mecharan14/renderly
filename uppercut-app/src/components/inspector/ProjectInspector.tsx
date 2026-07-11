import { useEffect, useState } from "react";
import { useEditorStore } from "../../store/editorStore";
import { setProjectSettings } from "../../lib/commands";
import { ASPECT_PRESETS } from "../../lib/types";

export function ProjectInspector() {
  const project = useEditorStore((s) => s.project);
  const dispatch = useEditorStore((s) => s.dispatch);
  const toast = useEditorStore((s) => s.toast);

  const [width, setWidth] = useState(project?.settings.width ?? 1080);
  const [height, setHeight] = useState(project?.settings.height ?? 1920);
  const [fps, setFps] = useState(project?.settings.fps ?? 60);

  useEffect(() => {
    if (!project) return;
    setWidth(project.settings.width);
    setHeight(project.settings.height);
    setFps(project.settings.fps);
  }, [project?.id, project?.settings.width, project?.settings.height, project?.settings.fps]);

  if (!project) {
    return (
      <div className="inspector-empty">
        <div className="icon">✦</div>
        <p>
          <strong>No project open</strong>
          <br />
          Import a video or open a project to edit canvas settings and clip properties.
        </p>
      </div>
    );
  }

  async function applySettings() {
    const ok = await dispatch(
      setProjectSettings({
        width: Math.max(1, Math.round(width)),
        height: Math.max(1, Math.round(height)),
        fps: Math.max(1, fps),
      }),
    );
    if (ok) toast("Project settings updated", "success");
  }

  return (
    <div className="inspector">
      <div className="inspector-section">
        <h3>Project</h3>
        <p>{project.name}</p>
        <p className="empty-hint">
          {project.media.length} media · {project.tracks.length} tracks
        </p>
        <p className="empty-hint">Select a clip on the timeline for clip-specific properties.</p>
      </div>

      <div className="inspector-section">
        <h3>Canvas</h3>
        <div className="field">
          <label>Quick preset</label>
          <select
            value={
              ASPECT_PRESETS.find(
                (p) => p.width === project.settings.width && p.height === project.settings.height,
              )?.id ?? ""
            }
            onChange={(e) => {
              const preset = ASPECT_PRESETS.find((p) => p.id === e.target.value);
              if (!preset) return;
              setWidth(preset.width);
              setHeight(preset.height);
              void dispatch(
                setProjectSettings({ width: preset.width, height: preset.height }),
              ).then((ok) => ok && toast(`Canvas ${preset.label}`, "success"));
            }}
          >
            <option value="">Custom</option>
            {ASPECT_PRESETS.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label} ({p.width}×{p.height})
              </option>
            ))}
          </select>
        </div>
        <div className="field">
          <label>Width</label>
          <input
            type="number"
            min={1}
            step={2}
            value={width}
            onChange={(e) => setWidth(parseInt(e.target.value, 10) || 1)}
          />
        </div>
        <div className="field">
          <label>Height</label>
          <input
            type="number"
            min={1}
            step={2}
            value={height}
            onChange={(e) => setHeight(parseInt(e.target.value, 10) || 1)}
          />
        </div>
        <div className="field">
          <label>FPS</label>
          <input
            type="number"
            min={1}
            step={1}
            value={fps}
            onChange={(e) => setFps(parseFloat(e.target.value) || 1)}
          />
        </div>
        <div className="inspector-actions">
          <button type="button" className="btn-primary" onClick={() => void applySettings()}>
            Apply settings
          </button>
        </div>
      </div>
    </div>
  );
}
