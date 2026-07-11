import type { LucideIcon } from "lucide-react";
import {
  Film,
  Music,
  Type,
  Sticker,
  Sparkles,
  Blend,
  Aperture,
  SlidersHorizontal,
} from "lucide-react";
import { useEditorStore, type LeftTab } from "../../store/editorStore";
import { Tooltip } from "../ui/Tooltip";
import { MediaPanel } from "./MediaPanel";
import { AudioPanel } from "./AudioPanel";
import { TextPanel } from "./TextPanel";
import { ComingSoonPanel } from "./ComingSoonPanel";
import { EffectsPanel } from "./EffectsPanel";
import { TransitionsPanel } from "./TransitionsPanel";

const TABS: { id: LeftTab; icon: LucideIcon; label: string }[] = [
  { id: "media", icon: Film, label: "Media" },
  { id: "audio", icon: Music, label: "Audio" },
  { id: "text", icon: Type, label: "Text" },
  { id: "stickers", icon: Sticker, label: "Stickers" },
  { id: "effects", icon: Sparkles, label: "Effects" },
  { id: "transitions", icon: Blend, label: "Transitions" },
  { id: "filters", icon: Aperture, label: "Filters" },
  { id: "adjustment", icon: SlidersHorizontal, label: "Adjust" },
];

const STUB_PITCH: Record<string, string> = {
  stickers: "Drop in animated stickers and shape overlays.",
  filters: "One-click color grading presets.",
  adjustment: "Manual exposure, contrast, and color wheels.",
};

export function LeftPanel() {
  const leftTab = useEditorStore((s) => s.leftTab);
  const setLeftTab = useEditorStore((s) => s.setLeftTab);
  const active = TABS.find((t) => t.id === leftTab);

  return (
    <aside className="panel left-panel">
      <div className="tab-rail">
        {TABS.map((tab) => {
          const Icon = tab.icon;
          return (
            <Tooltip key={tab.id} content={tab.label} side="right">
              <button
                type="button"
                className={`tab-rail-btn${leftTab === tab.id ? " active" : ""}`}
                onClick={() => setLeftTab(tab.id)}
              >
                <Icon className="tab-rail-icon" size={18} strokeWidth={1.75} />
              </button>
            </Tooltip>
          );
        })}
      </div>
      <div className="left-panel-content">
        {leftTab === "media" && <MediaPanel />}
        {leftTab === "audio" && <AudioPanel />}
        {leftTab === "text" && <TextPanel />}
        {leftTab === "effects" && <EffectsPanel />}
        {leftTab === "transitions" && <TransitionsPanel />}
        {leftTab !== "media" &&
          leftTab !== "audio" &&
          leftTab !== "text" &&
          leftTab !== "effects" &&
          leftTab !== "transitions" &&
          active && (
            <ComingSoonPanel icon={active.icon} title={active.label} pitch={STUB_PITCH[leftTab] ?? ""} />
          )}
      </div>
    </aside>
  );
}
