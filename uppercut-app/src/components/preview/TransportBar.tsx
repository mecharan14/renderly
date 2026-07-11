import { useEditorStore } from "../../store/editorStore";
import { formatTimecode } from "../../lib/format";
import { timelineDuration } from "../../timeline/layout";

export function TransportBar() {
  const project = useEditorStore((s) => s.project);
  const playhead = useEditorStore((s) => s.playhead);
  const playing = useEditorStore((s) => s.playing);
  const startPlayback = useEditorStore((s) => s.startPlayback);
  const stopPlayback = useEditorStore((s) => s.stopPlayback);

  const hasClips = !!project?.tracks.some((t) => t.clips.length > 0);
  const duration = project && hasClips ? formatTimecode(timelineDuration(project)) : "—";

  return (
    <div className="transport-bar">
      <button
        type="button"
        className={`play-btn${playing ? " playing" : ""}`}
        title="Play / Pause (Space)"
        disabled={!project}
        onClick={() => (playing ? stopPlayback() : void startPlayback())}
      >
        {playing ? "⏸" : "▶"}
      </button>
      <span className="timecode">{formatTimecode(playhead)}</span>
      <span className="timecode-sub">/ {duration}</span>
    </div>
  );
}
