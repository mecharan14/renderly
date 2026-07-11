import { useRef } from "react";
import { useEditorStore } from "../../store/editorStore";
import { TimelineToolbar } from "./TimelineToolbar";
import { TimelineCanvas } from "./TimelineCanvas";
import { TrackHeaders } from "./TrackHeaders";

export function TimelineSection() {
  const canvasWrapRef = useRef<HTMLDivElement | null>(null);
  const project = useEditorStore((s) => s.project);
  const hasClips = !!project?.tracks.some((t) => t.clips.length > 0);

  return (
    <div className="timeline-wrap">
      <TimelineToolbar />
      <div className="timeline-canvas-wrap" ref={canvasWrapRef}>
        <TimelineCanvas />
        <TrackHeaders containerRef={canvasWrapRef} />
        {project && !hasClips && (
          <div className="timeline-empty" aria-live="polite">
            <strong>Timeline is empty</strong>
            <span>Import media or drag a clip from the Media panel onto a track.</span>
          </div>
        )}
      </div>
    </div>
  );
}
