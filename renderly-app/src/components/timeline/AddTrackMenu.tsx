import { useEffect, useRef, useState } from "react";
import { Layers } from "lucide-react";
import { useEditorStore } from "../../store/editorStore";
import type { TrackKind } from "../../lib/types";
import { IconButton } from "../ui/IconButton";

const TRACK_KIND_LABELS: [TrackKind, string][] = [
  ["video", "Video track"],
  ["audio", "Audio track"],
  ["caption", "Caption track"],
];

/** Add-track menu for the timeline toolbar. */
export function AddTrackMenu() {
  const project = useEditorStore((s) => s.project);
  const dispatch = useEditorStore((s) => s.dispatch);
  const toast = useEditorStore((s) => s.toast);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  return (
    <div className="track-menu" ref={ref}>
      <IconButton
        icon={Layers}
        iconOnly
        size="sm"
        tooltip={open ? undefined : "Add track"}
        disabled={!project}
        className={open ? "active" : ""}
        onClick={() => setOpen((v) => !v)}
      />
      <div className={`track-menu-pop${open ? " open" : ""}`}>
        {TRACK_KIND_LABELS.map(([kind, label]) => (
          <button
            key={kind}
            type="button"
            onClick={() => {
              setOpen(false);
              void dispatch({ command: "AddTrack", kind, name: label.replace(" track", "") }).then(
                (ok) => ok && toast(`Added ${label.toLowerCase()}`, "success"),
              );
            }}
          >
            {label}
          </button>
        ))}
      </div>
    </div>
  );
}
