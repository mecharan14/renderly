import { useEffect, useRef, useState } from "react";
import { runExportFlow } from "../../lib/projectFlows";

export function ExportDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const dialogRef = useRef<HTMLDialogElement | null>(null);
  const [preset, setPreset] = useState<"tiktok" | "youtube">("tiktok");

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  return (
    <dialog ref={dialogRef} onClose={onClose}>
      <h3>Export video</h3>
      <p className="dialog-sub">Choose a platform preset, then pick where to save your MP4.</p>
      <div className="preset-grid">
        <button
          type="button"
          className={`preset-card${preset === "tiktok" ? " selected" : ""}`}
          onClick={() => setPreset("tiktok")}
        >
          <strong>TikTok / Reels</strong>
          <span>1080×1920 · 9:16</span>
        </button>
        <button
          type="button"
          className={`preset-card${preset === "youtube" ? " selected" : ""}`}
          onClick={() => setPreset("youtube")}
        >
          <strong>YouTube</strong>
          <span>1920×1080 · 16:9</span>
        </button>
      </div>
      <menu>
        <button type="button" className="btn" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="btn-primary"
          onClick={() => {
            onClose();
            void runExportFlow(preset);
          }}
        >
          Export MP4
        </button>
      </menu>
    </dialog>
  );
}
