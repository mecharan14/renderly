import { useRef } from "react";
import { TimelineToolbar } from "./TimelineToolbar";
import { TimelineCanvas } from "./TimelineCanvas";
import { TrackHeaders } from "./TrackHeaders";

export function TimelineSection() {
  const canvasWrapRef = useRef<HTMLDivElement | null>(null);

  return (
    <div className="timeline-wrap">
      <TimelineToolbar />
      <div className="timeline-canvas-wrap" ref={canvasWrapRef}>
        <TimelineCanvas />
        <TrackHeaders containerRef={canvasWrapRef} />
      </div>
    </div>
  );
}
