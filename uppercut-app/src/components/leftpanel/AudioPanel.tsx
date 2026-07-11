import { useState } from "react";
import { useEditorStore } from "../../store/editorStore";
import { fileName } from "../../lib/format";
import { generateVoiceover } from "../../lib/commands";
import { pickAndImportMedia } from "../../lib/projectFlows";
import { startMediaDrag } from "../../lib/dragMedia";

export function AudioPanel() {
  const project = useEditorStore((s) => s.project);
  const placeMediaOnTimeline = useEditorStore((s) => s.placeMediaOnTimeline);
  const ensureTrack = useEditorStore((s) => s.ensureTrack);
  const dispatch = useEditorStore((s) => s.dispatch);
  const playhead = useEditorStore((s) => s.playhead);
  const toast = useEditorStore((s) => s.toast);

  const [ttsText, setTtsText] = useState("");
  const [provider, setProvider] = useState<"piper_local" | "open_ai">("piper_local");
  const [busy, setBusy] = useState(false);

  const items = (project?.media ?? []).filter((m) => m.kind === "audio");

  async function generate() {
    if (!ttsText.trim() || !project) return;
    setBusy(true);
    try {
      const track = await ensureTrack("audio", "Voiceover");
      const outputPath = `${project.name.replace(/[^\w-]+/g, "_")}-voiceover-${Date.now()}.wav`;
      const ok = await dispatch(
        generateVoiceover(
          ttsText.trim(),
          track.id,
          playhead,
          outputPath,
          provider === "open_ai" ? { provider: "open_ai", voice: "alloy" } : { provider: "piper_local" },
        ),
      );
      if (ok) {
        toast("Voiceover generated", "success");
        setTtsText("");
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel-body">
      <div className="inspector-section">
        <h3>Text to speech</h3>
        <div className="field">
          <label>Script</label>
          <textarea
            value={ttsText}
            onChange={(e) => setTtsText(e.target.value)}
            placeholder="Type narration text…"
          />
        </div>
        <div className="field">
          <label>Voice provider</label>
          <select value={provider} onChange={(e) => setProvider(e.target.value as typeof provider)}>
            <option value="piper_local">Piper (local)</option>
            <option value="open_ai">OpenAI (BYO key)</option>
          </select>
        </div>
        <div className="inspector-actions">
          <button
            type="button"
            className="btn-primary"
            disabled={!project || busy || !ttsText.trim()}
            onClick={() => void generate()}
          >
            {busy ? "Generating…" : "Generate voiceover"}
          </button>
        </div>
      </div>

      <div className="inspector-section">
        <h3>Audio files</h3>
        <button type="button" className="btn" onClick={() => void pickAndImportMedia()}>
          Import audio
        </button>
      {items.length === 0 ? (
        <div className="empty-state">
          <div className="empty-state-icon">♪</div>
          <p>
            <strong>No audio yet</strong>
          </p>
          <p className="empty-hint">
            Import music or SFX, or generate a voiceover above. Items appear here and can be
            dragged onto an audio track.
          </p>
        </div>
      ) : (
          items.map((item) => (
            <div
              key={item.id}
              className="media-item"
              draggable
              onDragStart={(e) => startMediaDrag(e, item.id, item.kind, item.duration_secs ?? 5)}
              onClick={() => void placeMediaOnTimeline(item.id, item.kind)}
            >
              <div className="media-thumb audio">♪</div>
              <div className="media-meta">
                <div className="name">{fileName(item.path)}</div>
                <div className="sub">{item.duration_secs?.toFixed(1) ?? "?"}s</div>
              </div>
              <span className="media-add-hint">+ timeline</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
