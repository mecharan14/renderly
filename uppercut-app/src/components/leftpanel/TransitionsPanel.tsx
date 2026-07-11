import { Blend } from "lucide-react";
import { setClipTransition } from "../../lib/commands";
import type { ClipTransition, MediaClip } from "../../lib/types";
import { clipDurationSecs } from "../../lib/types";
import { useEditorStore } from "../../store/editorStore";

export function TransitionsPanel() {
  const project = useEditorStore((s) => s.project);
  const selection = useEditorStore((s) => s.selection);
  const dispatch = useEditorStore((s) => s.dispatch);

  const track = project?.tracks.find((t) => t.id === selection?.trackId);
  const clip = track?.clips.find((c) => c.id === selection?.clipId);
  const mediaClip =
    clip && clip.type === "video" ? (clip as MediaClip) : null;
  const locked = track?.locked ?? true;

  if (!track || track.kind !== "video" || !mediaClip) {
    return (
      <div className="panel-body">
        <h3>Transitions</h3>
        <p className="empty-hint">Select a video clip to set its outgoing crossfade.</p>
      </div>
    );
  }

  const next = [...track.clips]
    .filter((c) => c.type === "video")
    .sort((a, b) => a.position_secs - b.position_secs)
    .find((c) => c.position_secs >= mediaClip.position_secs + clipDurationSecs(mediaClip) - 1e-6);

  const current = mediaClip.outgoing_transition ?? null;
  const maxDur = next
    ? Math.min(clipDurationSecs(mediaClip), clipDurationSecs(next as MediaClip)) / 2
    : clipDurationSecs(mediaClip) / 2;

  async function apply(transition: ClipTransition | null) {
    await dispatch(setClipTransition(track!.id, mediaClip!.id, transition));
  }

  return (
    <div className="panel-body transitions-panel">
      <h3>Transitions</h3>
      <p className="empty-hint">
        Outgoing crossfade on the selected clip (renderer blends into the next clip).
      </p>
      {!next ? (
        <p className="empty-hint">No following clip on this track — add one after this clip.</p>
      ) : (
        <>
          <button
            type="button"
            disabled={locked}
            onClick={() =>
              void apply({
                kind: "crossfade",
                duration_secs: Math.min(0.5, Math.max(0.05, maxDur)),
              })
            }
          >
            <Blend size={14} strokeWidth={1.75} className="btn-lucide" />
            Add crossfade
          </button>
          {current && (
            <div className="field" style={{ marginTop: "0.75rem" }}>
              <label>Duration (seconds)</label>
              <input
                type="number"
                step="0.05"
                min={0.05}
                max={maxDur}
                value={current.duration_secs}
                disabled={locked}
                onChange={(e) => {
                  const duration_secs = Math.min(
                    maxDur,
                    Math.max(0.05, parseFloat(e.target.value) || 0.05),
                  );
                  void apply({ kind: "crossfade", duration_secs });
                }}
              />
              <button
                type="button"
                className="btn-ghost"
                disabled={locked}
                onClick={() => void apply(null)}
              >
                Clear
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
