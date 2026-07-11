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
  Blocks,
} from "lucide-react";
import { useEditorStore, type LeftTab } from "../../store/editorStore";
import { Tooltip } from "../ui/Tooltip";
import { MediaPanel } from "./MediaPanel";
import { AudioPanel } from "./AudioPanel";
import { TextPanel } from "./TextPanel";
import { EffectsPanel } from "./EffectsPanel";
import { TransitionsPanel } from "./TransitionsPanel";
import { StickersPanel } from "./StickersPanel";
import { FiltersPanel } from "./FiltersPanel";
import { AdjustPanel } from "./AdjustPanel";
import { ExtensionsPanel } from "./ExtensionsPanel";

const TABS: { id: LeftTab; icon: LucideIcon; label: string }[] = [
  { id: "media", icon: Film, label: "Media" },
  { id: "audio", icon: Music, label: "Audio" },
  { id: "text", icon: Type, label: "Text" },
  { id: "stickers", icon: Sticker, label: "Stickers" },
  { id: "effects", icon: Sparkles, label: "Effects" },
  { id: "transitions", icon: Blend, label: "Transitions" },
  { id: "filters", icon: Aperture, label: "Filters" },
  { id: "adjustment", icon: SlidersHorizontal, label: "Adjust" },
  { id: "extensions", icon: Blocks, label: "Extensions" },
];

export function LeftPanel() {
  const leftTab = useEditorStore((s) => s.leftTab);
  const setLeftTab = useEditorStore((s) => s.setLeftTab);

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
        {leftTab === "stickers" && <StickersPanel />}
        {leftTab === "effects" && <EffectsPanel />}
        {leftTab === "transitions" && <TransitionsPanel />}
        {leftTab === "filters" && <FiltersPanel />}
        {leftTab === "adjustment" && <AdjustPanel />}
        {leftTab === "extensions" && <ExtensionsPanel />}
      </div>
    </aside>
  );
}
