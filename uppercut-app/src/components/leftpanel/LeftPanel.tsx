import { useEditorStore, type LeftTab } from "../../store/editorStore";
import { MediaPanel } from "./MediaPanel";
import { AudioPanel } from "./AudioPanel";
import { TextPanel } from "./TextPanel";
import { ComingSoonPanel } from "./ComingSoonPanel";

const TABS: { id: LeftTab; icon: string; label: string }[] = [
  { id: "media", icon: "🎬", label: "Media" },
  { id: "audio", icon: "♪", label: "Audio" },
  { id: "text", icon: "T", label: "Text" },
  { id: "stickers", icon: "★", label: "Stickers" },
  { id: "effects", icon: "✨", label: "Effects" },
  { id: "transitions", icon: "⇄", label: "Transitions" },
  { id: "filters", icon: "◐", label: "Filters" },
  { id: "adjustment", icon: "⚙", label: "Adjustment" },
];

const STUB_PITCH: Record<string, string> = {
  stickers: "Drop in animated stickers and shape overlays.",
  effects: "Screen-space video effects and generators.",
  transitions: "Cross-track cut, dissolve, and wipe transitions.",
  filters: "One-click color grading presets.",
  adjustment: "Manual exposure, contrast, and color wheels.",
};

export function LeftPanel() {
  const leftTab = useEditorStore((s) => s.leftTab);
  const setLeftTab = useEditorStore((s) => s.setLeftTab);

  return (
    <aside className="panel left-panel">
      <div className="tab-rail">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            type="button"
            className={`tab-rail-btn${leftTab === tab.id ? " active" : ""}`}
            title={tab.label}
            onClick={() => setLeftTab(tab.id)}
          >
            <span className="tab-rail-icon">{tab.icon}</span>
            <span className="tab-rail-label">{tab.label}</span>
          </button>
        ))}
      </div>
      <div className="left-panel-content">
        {leftTab === "media" && <MediaPanel />}
        {leftTab === "audio" && <AudioPanel />}
        {leftTab === "text" && <TextPanel />}
        {leftTab !== "media" && leftTab !== "audio" && leftTab !== "text" && (
          <ComingSoonPanel
            icon={TABS.find((t) => t.id === leftTab)?.icon ?? "✦"}
            title={TABS.find((t) => t.id === leftTab)?.label ?? ""}
            pitch={STUB_PITCH[leftTab] ?? ""}
          />
        )}
      </div>
    </aside>
  );
}
