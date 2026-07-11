import { useEffect, useState } from "react";
import { Trash2 } from "lucide-react";
import { useEditorStore } from "../../store/editorStore";
import { deleteClip, setCaption } from "../../lib/commands";
import type { CaptionClip, Track } from "../../lib/types";
import { CaptionStyleGallery } from "../CaptionStyleGallery";
import { Tooltip } from "../ui/Tooltip";

export function CaptionInspector({ track, clip }: { track: Track; clip: CaptionClip }) {
  const dispatch = useEditorStore((s) => s.dispatch);
  const select = useEditorStore((s) => s.select);
  const toast = useEditorStore((s) => s.toast);

  const [text, setText] = useState(clip.text);
  const [style, setStyle] = useState(clip.style_id);
  const [position, setPosition] = useState(clip.position_secs);
  const [duration, setDuration] = useState(clip.duration_secs);

  useEffect(() => {
    setText(clip.text);
    setStyle(clip.style_id);
    setPosition(clip.position_secs);
    setDuration(clip.duration_secs);
  }, [clip.id, clip.text, clip.style_id, clip.position_secs, clip.duration_secs]);

  return (
    <div className="inspector">
      <div className="inspector-section">
        <h3>Caption</h3>
        <div className="field">
          <label>Text</label>
          <textarea
            value={text}
            disabled={track.locked}
            onChange={(e) => setText(e.target.value)}
            placeholder="Enter caption text…"
          />
        </div>
        <div className="field">
          <label>Style</label>
          <CaptionStyleGallery
            value={style}
            onChange={(id) => {
              setStyle(id);
              void dispatch(setCaption(track.id, clip.id, { styleId: id }));
            }}
          />
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
            disabled={track.locked}
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
            disabled={track.locked}
            onChange={(e) => setDuration(parseFloat(e.target.value))}
            onBlur={() => void dispatch(setCaption(track.id, clip.id, { durationSecs: duration }))}
          />
        </div>
      </div>

      <div className="inspector-actions">
        <button
          type="button"
          className="btn-primary"
          disabled={track.locked}
          onClick={async () => {
            const ok = await dispatch(setCaption(track.id, clip.id, { text, styleId: style }));
            if (ok) toast("Caption updated", "success");
          }}
        >
          Update caption
        </button>
        <Tooltip content="Delete clip">
          <button
            type="button"
            className="btn-danger"
            disabled={track.locked}
            onClick={async () => {
              await dispatch(deleteClip(track.id, clip.id, false));
              select(null);
            }}
          >
            <Trash2 size={14} strokeWidth={1.75} className="btn-lucide" />
          </button>
        </Tooltip>
      </div>
    </div>
  );
}
