import { useState } from "react";
import { useEditorStore } from "../../store/editorStore";
import { deleteClip, setAudioFade, setAudioGain, setClipEnabled, trimClip } from "../../lib/commands";
import type { MediaClip, Track } from "../../lib/types";

export function MediaClipInspector({ track, clip }: { track: Track; clip: MediaClip }) {
  const dispatch = useEditorStore((s) => s.dispatch);
  const select = useEditorStore((s) => s.select);
  const media = useEditorStore((s) => s.project?.media.find((m) => m.id === clip.media_id));

  const [gain, setGain] = useState(clip.gain_db);
  const [sourceIn, setSourceIn] = useState(clip.source_in_secs);
  const [sourceOut, setSourceOut] = useState(clip.source_out_secs);
  const [fadeIn, setFadeIn] = useState(clip.fade_in_secs);
  const [fadeOut, setFadeOut] = useState(clip.fade_out_secs);

  async function commitTrim() {
    await dispatch(trimClip(track.id, clip.id, sourceIn, sourceOut));
  }

  return (
    <div className="inspector">
      <div className="inspector-section">
        <h3>Clip</h3>
        <p>
          {track.name} · {track.kind}
          {media ? ` · ${media.path.split(/[/\\]/).pop()}` : ""}
        </p>
        <div className="field">
          <label>
            <input
              type="checkbox"
              checked={clip.enabled}
              onChange={(e) => void dispatch(setClipEnabled(track.id, clip.id, e.target.checked))}
            />{" "}
            Enabled
          </label>
        </div>
      </div>

      <div className="inspector-section">
        <h3>Trim (source in/out)</h3>
        <div className="field">
          <label>In (seconds)</label>
          <input
            type="number"
            step="0.1"
            value={sourceIn}
            onChange={(e) => setSourceIn(parseFloat(e.target.value))}
            onBlur={() => void commitTrim()}
          />
        </div>
        <div className="field">
          <label>Out (seconds)</label>
          <input
            type="number"
            step="0.1"
            value={sourceOut}
            onChange={(e) => setSourceOut(parseFloat(e.target.value))}
            onBlur={() => void commitTrim()}
          />
        </div>
      </div>

      {clip.type === "audio" && (
        <div className="inspector-section">
          <h3>Audio</h3>
          <div className="field">
            <label>Volume (dB)</label>
            <input
              type="range"
              min={-24}
              max={12}
              step={0.5}
              value={gain}
              onChange={(e) => setGain(parseFloat(e.target.value))}
              onMouseUp={() => void dispatch(setAudioGain(track.id, clip.id, gain))}
              onTouchEnd={() => void dispatch(setAudioGain(track.id, clip.id, gain))}
            />
            <span>{gain} dB</span>
          </div>
          <div className="field">
            <label>Fade in (seconds)</label>
            <input
              type="number"
              step="0.1"
              min={0}
              value={fadeIn}
              onChange={(e) => setFadeIn(parseFloat(e.target.value))}
              onBlur={() => void dispatch(setAudioFade(track.id, clip.id, fadeIn, fadeOut))}
            />
          </div>
          <div className="field">
            <label>Fade out (seconds)</label>
            <input
              type="number"
              step="0.1"
              min={0}
              value={fadeOut}
              onChange={(e) => setFadeOut(parseFloat(e.target.value))}
              onBlur={() => void dispatch(setAudioFade(track.id, clip.id, fadeIn, fadeOut))}
            />
          </div>
        </div>
      )}

      <div className="inspector-actions">
        <button
          type="button"
          className="btn-danger"
          onClick={async () => {
            await dispatch(deleteClip(track.id, clip.id, false));
            select(null);
          }}
        >
          <span className="btn-icon">🗑</span>
          <span>Delete</span>
        </button>
      </div>
    </div>
  );
}
