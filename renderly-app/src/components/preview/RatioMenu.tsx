import { ASPECT_PRESETS, type Project } from "../../lib/types";
import { setProjectSettings } from "../../lib/commands";
import { useEditorStore } from "../../store/editorStore";
import { MenuSelect, type MenuSelectOption } from "../ui/MenuSelect";

function originalDims(project: Project): { width: number; height: number } | null {
  const firstVideo = project.media.find(
    (m) => m.kind === "video" && m.width && m.height && m.width > 0 && m.height > 0,
  );
  if (!firstVideo?.width || !firstVideo?.height) return null;
  return { width: firstVideo.width, height: firstVideo.height };
}

function currentPresetId(project: Project): string {
  const { width, height } = project.settings;
  const match = ASPECT_PRESETS.find((p) => p.width === width && p.height === height);
  if (match) return match.id;
  const orig = originalDims(project);
  if (orig && orig.width === width && orig.height === height) return "original";
  return "custom";
}

function shortLabel(project: Project | null, value: string): string {
  if (!project) return "9:16";
  if (value === "original") {
    const orig = originalDims(project);
    return orig ? `${orig.width}×${orig.height}` : "Original";
  }
  if (value === "custom") return `${project.settings.width}×${project.settings.height}`;
  return ASPECT_PRESETS.find((p) => p.id === value)?.label ?? value;
}

export function RatioMenu({ compact = true }: { compact?: boolean }) {
  const project = useEditorStore((s) => s.project);
  const dispatch = useEditorStore((s) => s.dispatch);
  const toast = useEditorStore((s) => s.toast);

  const value = project ? currentPresetId(project) : "9:16";
  const orig = project ? originalDims(project) : null;

  const options: MenuSelectOption[] = [
    ...ASPECT_PRESETS.map((p) => ({
      value: p.id,
      label: `${p.label} · ${p.width}×${p.height}`,
    })),
    {
      value: "original",
      label: orig ? `Original · ${orig.width}×${orig.height}` : "Original",
      disabled: !orig,
    },
  ];
  if (project && value === "custom") {
    options.push({
      value: "custom",
      label: `Custom · ${project.settings.width}×${project.settings.height}`,
      disabled: true,
    });
  }

  return (
    <MenuSelect
      className="ratio-menu"
      compact={compact}
      tooltip="Canvas aspect ratio"
      disabled={!project}
      value={value}
      displayLabel={shortLabel(project, value)}
      options={options}
      onChange={(id) => {
        if (!project || id === "custom") return;
        void (async () => {
          if (id === "original") {
            if (!orig) {
              toast("Import a video first to use Original", "info");
              return;
            }
            const ok = await dispatch(setProjectSettings({ width: orig.width, height: orig.height }));
            if (ok) toast(`Canvas ${orig.width}×${orig.height}`, "success");
            return;
          }
          const preset = ASPECT_PRESETS.find((p) => p.id === id);
          if (!preset) return;
          const ok = await dispatch(setProjectSettings({ width: preset.width, height: preset.height }));
          if (ok) toast(`Canvas ${preset.label}`, "success");
        })();
      }}
    />
  );
}
