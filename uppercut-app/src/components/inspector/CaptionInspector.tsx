import { useState } from "react";
import { useEditorStore } from "../../store/editorStore";
import { deleteClip, setCaption } from "../../lib/commands";
import { CAPTION_STYLES, type CaptionClip, type Track } from "../../lib/types";

export function CaptionInspector({ track, clip }: { track: Track; clip: CaptionClip }) {
  const dispatch = useEditorStore((s) => s.dispatch);
  const select = useEditorStore((s) => s.select);
  const toast = useEditorStore((s) => s.toast);

  const [text, setText] = useState(clip.text);
  const [style, setStyle] = useState(clip.style_id);
  const [position, setPosition] = useState(clip.position_secs);
  const [duration, setDuration] = useState(clip.duration_secs);

  return (
    <div className="inspector">
      <div className="inspector-section">
        <h3>Caption</h3>
        <div className="field">
          <label>Text</label>
          <textarea value={text} onChange={(e) => setText(e.target.value)} placeholder="Enter caption text…" />
        </div>
        <div className="field">
          <label>Style</label>
          <select value={style} onChange={(e) => setStyle(e.target.value)}>
            {CAPTION_STYLES.map((s) => (
              <option key={s} value={s}>
                {s.replace(/-/g, " ")}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="inspector-section">
        <h3>Timing</h3>
        <div className="field">
          <label>Start (seconds)</label>
          <input
            type="number"
            step="0.1"
            value={position}
            onChange={(e) => setPosition(parseFloat(e.target.value))}
            onBlur={() => void dispatch(setCaption(track.id, clip.id, { positionSecs: position }))}
          />
        </div>
        <div className="field">
          <label>Duration (seconds)</label>
          <input
            type="number"
            step="0.1"
            value={duration}
            onChange={(e) => setDuration(parseFloat(e.target.value))}
            onBlur={() => void dispatch(setCaption(track.id, clip.id, { durationSecs: duration }))}
          />
        </div>
      </div>

      <div className="inspector-actions">
        <button
          type="button"
          className="btn-primary"
          onClick={async () => {
            const ok = await dispatch(setCaption(track.id, clip.id, { text, styleId: style }));
            if (ok) toast("Caption updated", "success");
          }}
        >
          Update caption
        </button>
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
