import { Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import * as ipc from "../../lib/ipc";
import { Tooltip } from "./Tooltip";

/** Two overlapping squares — restore-from-maximize affordance. */
function RestoreIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden>
      <rect
        x="3.5"
        y="1.5"
        width="7"
        height="7"
        rx="0.5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.25"
      />
      <rect
        x="1.5"
        y="3.5"
        width="7"
        height="7"
        rx="0.5"
        fill="var(--bg-1)"
        stroke="currentColor"
        strokeWidth="1.25"
      />
    </svg>
  );
}

export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let alive = true;
    void ipc.isWindowMaximized().then((v) => {
      if (alive) setMaximized(v);
    });
    return () => {
      alive = false;
    };
  }, []);

  async function toggleMax() {
    await ipc.toggleMaximizeWindow();
    setMaximized(await ipc.isWindowMaximized());
  }

  return (
    <div className="window-controls">
      <Tooltip content="Minimize" side="bottom">
        <button type="button" className="window-ctrl" onClick={() => void ipc.minimizeWindow()}>
          <Minus size={14} strokeWidth={1.75} />
        </button>
      </Tooltip>
      <Tooltip content={maximized ? "Restore" : "Maximize"} side="bottom">
        <button type="button" className="window-ctrl" onClick={() => void toggleMax()}>
          {maximized ? <RestoreIcon /> : <Square size={12} strokeWidth={1.75} />}
        </button>
      </Tooltip>
      <Tooltip content="Close" side="bottom">
        <button
          type="button"
          className="window-ctrl window-ctrl-close"
          onClick={() => void ipc.closeWindow()}
        >
          <X size={14} strokeWidth={1.75} />
        </button>
      </Tooltip>
    </div>
  );
}
