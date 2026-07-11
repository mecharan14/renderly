import { useEffect, useState } from "react";
import { Trash2 } from "lucide-react";
import { useEditorStore } from "../../store/editorStore";
import {
  deleteClip,
  setAudioFade,
  setAudioGain,
  setClipEnabled,
  setClipTransform,
  setTrackAudioRole,
  trimClip,
} from "../../lib/commands";
import type { ClipTransform, MediaClip, Track, TrackAudioRole } from "../../lib/types";
import { IDENTITY_TRANSFORM } from "../../lib/types";
import { MenuSelect } from "../ui/MenuSelect";
import { Tooltip } from "../ui/Tooltip";

const AUDIO_ROLES: { value: TrackAudioRole | ""; label: string }[] = [
  { value: "", label: "None" },
  { value: "voiceover", label: "Voiceover" },
  { value: "dialog", label: "Dialog" },
  { value: "music", label: "Music" },
  { value: "ambience", label: "Ambience" },
];

function clipTransform(clip: MediaClip): ClipTransform {
  return clip.transform ?? IDENTITY_TRANSFORM;
}

export function MediaClipInspector({ track, clip }: { track: Track; clip: MediaClip }) {
  const dispatch = useEditorStore((s) => s.dispatch);
  const select = useEditorStore((s) => s.select);
  const media = useEditorStore((s) => s.project?.media.find((m) => m.id === clip.media_id));

  const [gain, setGain] = useState(clip.gain_db);
  const [sourceIn, setSourceIn] = useState(clip.source_in_secs);
  const [sourceOut, setSourceOut] = useState(clip.source_out_secs);
  const [fadeIn, setFadeIn] = useState(clip.fade_in_secs);
  const [fadeOut, setFadeOut] = useState(clip.fade_out_secs);
  const [transform, setTransform] = useState<ClipTransform>(() => clipTransform(clip));

  useEffect(() => {
    setGain(clip.gain_db);
    setSourceIn(clip.source_in_secs);
    setSourceOut(clip.source_out_secs);
    setFadeIn(clip.fade_in_secs);
    setFadeOut(clip.fade_out_secs);
    setTransform(clipTransform(clip));
  }, [
    clip.id,
    clip.gain_db,
    clip.source_in_secs,
    clip.source_out_secs,
    clip.fade_in_secs,
    clip.fade_out_secs,
    clip.transform,
  ]);

  async function commitTrim() {
    await dispatch(trimClip(track.id, clip.id, sourceIn, sourceOut));
  }

  async function commitTransform(next: ClipTransform) {
    setTransform(next);
    await dispatch(setClipTransform(track.id, clip.id, next));
  }

  const showAudio = clip.type === "audio" || track.kind === "audio" || clip.type === "video";
  const showTransform = clip.type === "video" || track.kind === "video";

  return (
    <div className="inspector">
      <div className="inspector-section">
        <h3>{clip.type === "audio" ? "Audio clip" : "Video clip"}</h3>
        <p>
          {track.name} · {track.kind}
          {media ? ` · ${media.path.split(/[/\\]/).pop()}` : ""}
        </p>
        {media?.width && media?.height ? (
          <p className="empty-hint">
            Source {media.width}×{media.height}
            {media.fps ? ` · ${media.fps.toFixed(2)} fps` : ""}
            {media.duration_secs != null ? ` · ${media.duration_secs.toFixed(1)}s` : ""}
          </p>
        ) : null}
        <div className="field toggle-row">
          <input
            id="clip-enabled"
            type="checkbox"
            checked={clip.enabled}
            disabled={track.locked}
            onChange={(e) => void dispatch(setClipEnabled(track.id, clip.id, e.target.checked))}
          />
          <label htmlFor="clip-enabled">Enabled</label>
        </div>
      </div>

      <div className="inspector-section">
        <h3>Trim (source in/out)</h3>
        <div className="field">
          <label>In (seconds)</label>
          <input
            type="number"
            step="0.1"
            min={0}
            value={sourceIn}
            disabled={track.locked}
            onChange={(e) => setSourceIn(parseFloat(e.target.value))}
            onBlur={() => void commitTrim()}
          />
        </div>
        <div className="field">
          <label>Out (seconds)</label>
          <input
            type="number"
            step="0.1"
            min={0}
            value={sourceOut}
            disabled={track.locked}
            onChange={(e) => setSourceOut(parseFloat(e.target.value))}
            onBlur={() => void commitTrim()}
          />
        </div>
      </div>

      {showTransform && (
        <div className="inspector-section">
          <h3>Transform</h3>
          <div className="field">
            <label>X (NDC)</label>
            <input
              type="number"
              step="0.05"
              value={transform.x}
              disabled={track.locked}
              onChange={(e) => setTransform({ ...transform, x: parseFloat(e.target.value) || 0 })}
              onBlur={() => void commitTransform(transform)}
            />
          </div>
          <div className="field">
            <label>Y (NDC)</label>
            <input
              type="number"
              step="0.05"
              value={transform.y}
              disabled={track.locked}
              onChange={(e) => setTransform({ ...transform, y: parseFloat(e.target.value) || 0 })}
              onBlur={() => void commitTransform(transform)}
            />
          </div>
          <div className="field">
            <label>Scale X</label>
            <input
              type="number"
              step="0.05"
              min={0.01}
              value={transform.scale_x}
              disabled={track.locked}
              onChange={(e) =>
                setTransform({ ...transform, scale_x: parseFloat(e.target.value) || 1 })
              }
              onBlur={() => void commitTransform(transform)}
            />
          </div>
          <div className="field">
            <label>Scale Y</label>
            <input
              type="number"
              step="0.05"
              min={0.01}
              value={transform.scale_y}
              disabled={track.locked}
              onChange={(e) =>
                setTransform({ ...transform, scale_y: parseFloat(e.target.value) || 1 })
              }
              onBlur={() => void commitTransform(transform)}
            />
          </div>
          <div className="field">
            <label>Rotation (°)</label>
            <input
              type="number"
              step="1"
              value={transform.rotation_deg}
              disabled={track.locked}
              onChange={(e) =>
                setTransform({ ...transform, rotation_deg: parseFloat(e.target.value) || 0 })
              }
              onBlur={() => void commitTransform(transform)}
            />
          </div>
          <div className="field">
            <label>Opacity</label>
            <input
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={transform.opacity}
              disabled={track.locked}
              onChange={(e) => setTransform({ ...transform, opacity: parseFloat(e.target.value) })}
              onMouseUp={() => void commitTransform(transform)}
              onTouchEnd={() => void commitTransform(transform)}
            />
            <span>{Math.round(transform.opacity * 100)}%</span>
          </div>
        </div>
      )}

      {showTransform && (
        <div className="inspector-section">
          <h3>Keyframes</h3>
          <p className="empty-hint">Keyframe editor lands in 3.2</p>
        </div>
      )}

      {showAudio && (
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
              disabled={track.locked}
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
              disabled={track.locked}
              onChange={(e) => setFadeIn(parseFloat(e.target.value) || 0)}
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
              disabled={track.locked}
              onChange={(e) => setFadeOut(parseFloat(e.target.value) || 0)}
              onBlur={() => void dispatch(setAudioFade(track.id, clip.id, fadeIn, fadeOut))}
            />
          </div>
          {track.kind === "audio" && (
            <div className="field">
              <label>Track role</label>
              <MenuSelect
                value={track.audio_role ?? ""}
                disabled={track.locked}
                options={AUDIO_ROLES.map((r) => ({ value: r.value, label: r.label }))}
                onChange={(v) => {
                  void dispatch(setTrackAudioRole(track.id, v === "" ? null : (v as TrackAudioRole)));
                }}
              />
            </div>
          )}
        </div>
      )}

      <div className="inspector-actions">
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
