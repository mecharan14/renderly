import { useEffect, useState } from "react";
import { Upload, FolderOpen, FilePlus, Trash2 } from "lucide-react";
import {
  createNewProjectFlow,
  openExistingProjectFlow,
  quickStartWithImport,
} from "../lib/projectFlows";
import { useEditorStore } from "../store/editorStore";
import { IconButton } from "./ui/IconButton";
import { WindowControls } from "./ui/WindowControls";
import * as ipc from "../lib/ipc";

function formatModified(ms: number): string {
  if (!ms) return "";
  const d = new Date(ms);
  return d.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatDuration(secs: number): string {
  if (!secs || secs < 0.05) return "Empty";
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return m > 0 ? `${m}m ${s}s` : `${s}s`;
}

export function WelcomeScreen() {
  const hasProject = useEditorStore((s) => !!s.project);
  const loadProjectFromPath = useEditorStore((s) => s.loadProjectFromPath);
  const toast = useEditorStore((s) => s.toast);
  const [projects, setProjects] = useState<ipc.ProjectSummary[]>([]);
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);

  async function refresh() {
    setLoading(true);
    try {
      const list = await ipc.listProjects();
      setProjects(list);
      // Warm missing thumbs in the background.
      for (const p of list) {
        if (p.thumbPath) {
          setThumbs((t) => ({ ...t, [p.path]: ipc.assetUrl(p.thumbPath!) }));
        } else {
          void ipc.projectThumbnail(p.path).then((path) => {
            if (path) setThumbs((t) => ({ ...t, [p.path]: ipc.assetUrl(path) }));
          });
        }
      }
    } catch (e) {
      toast(`Could not list projects: ${e instanceof Error ? e.message : String(e)}`, "error");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (!hasProject) void refresh();
  }, [hasProject]);

  async function openCard(path: string) {
    try {
      await loadProjectFromPath(path);
      toast("Project opened", "success");
    } catch (e) {
      toast(`Open failed: ${e instanceof Error ? e.message : String(e)}`, "error");
    }
  }

  async function removeCard(path: string, name: string) {
    if (!window.confirm(`Delete “${name}”? This cannot be undone.`)) return;
    try {
      await ipc.deleteProject(path);
      await refresh();
    } catch (e) {
      toast(`Delete failed: ${e instanceof Error ? e.message : String(e)}`, "error");
    }
  }

  return (
    <div className={`welcome-screen${hasProject ? " hidden" : ""}`}>
      <div className="welcome-titlebar" data-tauri-drag-region>
        <WindowControls />
      </div>
      <div className="welcome-home">
        <div className="welcome-home-header">
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
          <div>
            <h1>Renderly</h1>
            <p className="welcome-lead">Your projects</p>
          </div>
          <div className="welcome-home-actions">
            <IconButton
              icon={Upload}
              label="Import"
              variant="primary"
              onClick={() => void quickStartWithImport()}
            />
            <IconButton
              icon={FilePlus}
              label="New"
              onClick={() => void createNewProjectFlow()}
            />
            <IconButton
              icon={FolderOpen}
              label="Open…"
              variant="ghost"
              onClick={() => void openExistingProjectFlow()}
            />
          </div>
        </div>

        {loading ? (
          <p className="welcome-empty">Loading projects…</p>
        ) : projects.length === 0 ? (
          <div className="welcome-empty-card">
            <p>
              <strong>No projects yet</strong>
            </p>
            <p className="empty-hint">Import a clip or create a new project to get started.</p>
            <IconButton
              icon={Upload}
              label="Import video"
              variant="primary"
              onClick={() => void quickStartWithImport()}
            />
          </div>
        ) : (
          <div className="project-gallery">
            <button
              type="button"
              className="project-card project-card-new"
              onClick={() => void createNewProjectFlow()}
            >
              <FilePlus size={28} strokeWidth={1.5} />
              <span>New project</span>
            </button>
            {projects.map((p) => (
              <div key={p.path} className="project-card">
                <button
                  type="button"
                  className="project-card-main"
                  onClick={() => void openCard(p.path)}
                >
                  <div
                    className="project-card-thumb"
                    style={
                      thumbs[p.path]
                        ? {
                            backgroundImage: `url(${thumbs[p.path]})`,
                            backgroundSize: "cover",
                            backgroundPosition: "center",
                          }
                        : undefined
                    }
                  />
                  <div className="project-card-meta">
                    <div className="name">{p.name}</div>
                    <div className="sub">
                      {formatDuration(p.durationSecs)} · {p.width}×{p.height}
                    </div>
                    <div className="sub">{formatModified(p.modifiedMs)}</div>
                  </div>
                </button>
                <button
                  type="button"
                  className="project-card-delete"
                  title="Delete project"
                  onClick={(e) => {
                    e.stopPropagation();
                    void removeCard(p.path, p.name);
                  }}
                >
                  <Trash2 size={14} strokeWidth={2} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
