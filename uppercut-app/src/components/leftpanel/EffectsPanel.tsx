import { Sparkles, Trash2 } from "lucide-react";
import { setClipEffects } from "../../lib/commands";
import type { EffectInstance, MediaClip, Track } from "../../lib/types";
import { useEditorStore } from "../../store/editorStore";

export const BUILTIN_EFFECTS: {
  id: string;
  label: string;
  defaults: Record<string, number>;
}[] = [
  {
    id: "builtin:color_adjust",
    label: "Color adjust",
    defaults: { exposure: 0, contrast: 1, saturation: 1 },
  },
  { id: "builtin:blur", label: "Blur", defaults: { radius: 4 } },
  { id: "builtin:lut_contrast", label: "LUT Contrast", defaults: { intensity: 1 } },
  { id: "builtin:lut_warm", label: "LUT Warm", defaults: { intensity: 1 } },
];

function newId(): string {
  return crypto.randomUUID();
}

export function EffectsPanel() {
  const project = useEditorStore((s) => s.project);
  const selection = useEditorStore((s) => s.selection);
  const dispatch = useEditorStore((s) => s.dispatch);

  const track = project?.tracks.find((t) => t.id === selection?.trackId);
  const clip = track?.clips.find((c) => c.id === selection?.clipId);
  const mediaClip =
    clip && clip.type !== "caption" ? (clip as MediaClip) : null;
  const locked = track?.locked ?? true;

  if (!mediaClip || !track || track.kind === "caption") {
    return (
      <div className="panel-body">
        <h3>Effects</h3>
        <p className="empty-hint">Select a video or audio clip to add effects.</p>
      </div>
    );
  }

  const effects = mediaClip.effects ?? [];

  async function commit(next: EffectInstance[]) {
    await dispatch(setClipEffects(track!.id, mediaClip!.id, next));
  }

  return (
    <div className="panel-body effects-panel">
      <h3>Effects</h3>
      <p className="empty-hint">Builtin GPU effects applied before composite.</p>
      <div className="effects-catalog">
        {BUILTIN_EFFECTS.map((e) => (
          <button
            key={e.id}
            type="button"
            disabled={locked}
            onClick={() =>
              void commit([
                ...effects,
                {
                  id: newId(),
                  effect_id: e.id,
                  enabled: true,
                  params: { ...e.defaults },
                },
              ])
            }
          >
            <Sparkles size={14} strokeWidth={1.75} />
            {e.label}
          </button>
        ))}
      </div>
      <EffectList
        track={track}
        clip={mediaClip}
        effects={effects}
        locked={locked}
        onCommit={commit}
      />
    </div>
  );
}

export function EffectList({
  track,
  clip,
  effects,
  locked,
  onCommit,
}: {
  track: Track;
  clip: MediaClip;
  effects: EffectInstance[];
  locked: boolean;
  onCommit: (next: EffectInstance[]) => Promise<void>;
}) {
  void track;
  void clip;

  if (effects.length === 0) {
    return <p className="empty-hint">No effects on this clip.</p>;
  }

  return (
    <ul className="effect-list">
      {effects.map((fx, idx) => {
        const meta = BUILTIN_EFFECTS.find((b) => b.id === fx.effect_id);
        return (
          <li key={fx.id} className={!fx.enabled ? "disabled" : ""}>
            <div className="effect-list-header">
              <label className="toggle-row">
                <input
                  type="checkbox"
                  checked={fx.enabled}
                  disabled={locked}
                  onChange={(e) => {
                    const next = effects.map((x, i) =>
                      i === idx ? { ...x, enabled: e.target.checked } : x,
                    );
                    void onCommit(next);
                  }}
                />
                <span>{meta?.label ?? fx.effect_id}</span>
              </label>
              <button
                type="button"
                className="btn-ghost"
                disabled={locked}
                title="Remove"
                onClick={() => void onCommit(effects.filter((_, i) => i !== idx))}
              >
                <Trash2 size={14} strokeWidth={1.75} />
              </button>
            </div>
            {Object.keys(fx.params).map((key) => (
              <div className="field" key={key}>
                <label>{key}</label>
                <input
                  type="range"
                  min={key === "radius" ? 0 : key === "exposure" ? -2 : 0}
                  max={key === "radius" ? 32 : key === "exposure" ? 2 : 2}
                  step={0.05}
                  value={fx.params[key]}
                  disabled={locked || !fx.enabled}
                  onChange={(e) => {
                    const v = parseFloat(e.target.value);
                    const next = effects.map((x, i) =>
                      i === idx ? { ...x, params: { ...x.params, [key]: v } } : x,
                    );
                    // Optimistic local only until mouseup — but range needs live commit throttle.
                    // Commit on change is OK for few effects.
                    void onCommit(next);
                  }}
                />
                <span>{fx.params[key].toFixed(2)}</span>
              </div>
            ))}
          </li>
        );
      })}
    </ul>
  );
}
