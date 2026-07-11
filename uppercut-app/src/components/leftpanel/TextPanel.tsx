import { useState } from "react";
import { useEditorStore } from "../../store/editorStore";
import { addCaption, generateCaptions } from "../../lib/commands";
import { fileName } from "../../lib/format";
import { CAPTION_STYLES } from "../../lib/types";

export function TextPanel() {
  const project = useEditorStore((s) => s.project);
  const ensureTrack = useEditorStore((s) => s.ensureTrack);
  const dispatch = useEditorStore((s) => s.dispatch);
  const playhead = useEditorStore((s) => s.playhead);
  const toast = useEditorStore((s) => s.toast);

  const [text, setText] = useState("");
  const [style, setStyle] = useState<string>(CAPTION_STYLES[0]);
  const [autoMediaId, setAutoMediaId] = useState("");
  const [autoBusy, setAutoBusy] = useState(false);

  const transcribable = (project?.media ?? []).filter((m) => m.kind === "video" || m.kind === "audio");

  async function addTextClip() {
    if (!text.trim() || !project) return;
    const track = await ensureTrack("caption", "Captions");
    const ok = await dispatch(addCaption(track.id, text.trim(), playhead, 2, style));
    if (ok) {
      toast("Caption added", "success");
      setText("");
    }
  }

  async function runAutoCaptions() {
    if (!autoMediaId || !project) return;
    setAutoBusy(true);
    try {
      const track = await ensureTrack("caption", "Captions");
      const ok = await dispatch(generateCaptions(autoMediaId, track.id, style));
      if (ok) toast("Auto captions generated", "success");
    } finally {
      setAutoBusy(false);
    }
  }

  return (
    <div className="panel-body">
      <div className="inspector-section">
        <h3>Add text</h3>
        <div className="field">
          <label>Caption text</label>
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
        <div className="inspector-actions">
          <button type="button" className="btn-primary" disabled={!project || !text.trim()} onClick={() => void addTextClip()}>
            Add at playhead
          </button>
        </div>
      </div>

      <div className="inspector-section">
        <h3>Auto captions</h3>
        <div className="field">
          <label>Source media</label>
          <select value={autoMediaId} onChange={(e) => setAutoMediaId(e.target.value)}>
            <option value="">Choose video or audio…</option>
            {transcribable.map((m) => (
              <option key={m.id} value={m.id}>
                {fileName(m.path)}
              </option>
            ))}
          </select>
        </div>
        <div className="inspector-actions">
          <button
            type="button"
            className="btn"
            disabled={!autoMediaId || autoBusy}
            onClick={() => void runAutoCaptions()}
          >
            {autoBusy ? "Transcribing…" : "Generate from speech"}
          </button>
        </div>
        <p className="empty-hint">Requires whisper-cli on PATH and UPPERCUT_WHISPER_MODEL set.</p>
      </div>
    </div>
  );
}
