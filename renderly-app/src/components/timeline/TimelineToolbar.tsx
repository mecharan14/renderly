import {
  MousePointer2,
  Scissors,
  Magnet,
  ZoomIn,
  ZoomOut,
  Maximize,
  SplitSquareHorizontal,
  CircleDashed,
} from "lucide-react";
import { useEditorStore } from "../../store/editorStore";
import { splitSelectedAtPlayhead } from "../../timeline/interactions";
import { IconButton } from "../ui/IconButton";
import { Tooltip } from "../ui/Tooltip";
import { AddTrackMenu } from "./AddTrackMenu";

export function TimelineToolbar() {
  const toolMode = useEditorStore((s) => s.toolMode);
  const setTool = useEditorStore((s) => s.setTool);
  const snapEnabled = useEditorStore((s) => s.snapEnabled);
  const setSnap = useEditorStore((s) => s.setSnap);
  const pxPerSec = useEditorStore((s) => s.pxPerSec);
  const setZoom = useEditorStore((s) => s.setZoom);
  const fitZoom = useEditorStore((s) => s.fitZoom);
  const toast = useEditorStore((s) => s.toast);
  const project = useEditorStore((s) => s.project);

  return (
    <div className="timeline-tools">
      <div className="tool-group">
        <Tooltip content="Select · move and trim (V)">
          <button
            type="button"
            className={`tool-btn icon-only${toolMode === "select" ? " active" : ""}`}
            onClick={() => setTool("select")}
          >
            <MousePointer2 size={15} strokeWidth={1.75} />
          </button>
        </Tooltip>
        <Tooltip content="Razor · click a clip to split (C)">
          <button
            type="button"
            className={`tool-btn icon-only${toolMode === "razor" ? " active" : ""}`}
            onClick={() => {
              setTool("razor");
              toast("Click a clip to split it", "info");
            }}
          >
            <Scissors size={15} strokeWidth={1.75} />
          </button>
        </Tooltip>
        <Tooltip content="Mask · draw/edit rect or ellipse on preview (M)">
          <button
            type="button"
            className={`tool-btn icon-only${toolMode === "mask" ? " active" : ""}`}
            onClick={() => {
              setTool("mask");
              toast("Drag on the paused preview to draw a mask", "info");
            }}
          >
            <CircleDashed size={15} strokeWidth={1.75} />
          </button>
        </Tooltip>
      </div>

      <IconButton
        icon={SplitSquareHorizontal}
        iconOnly
        size="sm"
        tooltip="Split at playhead (S)"
        disabled={!project}
        onClick={() => void splitSelectedAtPlayhead()}
      />

      <Tooltip content="Snap to grid and clip edges">
        <button
          type="button"
          className={`tool-btn icon-only${snapEnabled ? " active" : ""}`}
          onClick={() => setSnap(!snapEnabled)}
        >
          <Magnet size={15} strokeWidth={1.75} />
        </button>
      </Tooltip>

      <AddTrackMenu />

      <span className="tool-divider" />

      <div className="zoom-controls">
        <IconButton
          icon={ZoomOut}
          iconOnly
          size="sm"
          tooltip="Zoom out"
          onClick={() => setZoom(pxPerSec - 20)}
        />
        <span className="zoom-label">{Math.round((pxPerSec / 80) * 100)}%</span>
        <IconButton
          icon={ZoomIn}
          iconOnly
          size="sm"
          tooltip="Zoom in"
          onClick={() => setZoom(pxPerSec + 20)}
        />
        <IconButton
          icon={Maximize}
          iconOnly
          size="sm"
          tooltip="Fit timeline to window"
          onClick={() => fitZoom()}
        />
      </div>
    </div>
  );
}
