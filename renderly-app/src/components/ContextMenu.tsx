import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useEditorStore } from "../store/editorStore";
import { setClipEnabled, splitClip } from "../lib/commands";

export function ContextMenu() {
  const menu = useEditorStore((s) => s.contextMenu);
  const project = useEditorStore((s) => s.project);
  const dispatch = useEditorStore((s) => s.dispatch);
  const closeContextMenu = useEditorStore((s) => s.closeContextMenu);
  const copySelection = useEditorStore((s) => s.copySelection);
  const pasteAtPlayhead = useEditorStore((s) => s.pasteAtPlayhead);
  const duplicateSelection = useEditorStore((s) => s.duplicateSelection);
  const clipboard = useEditorStore((s) => s.clipboard);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useEffect(() => {
    if (!menu) return;
    const onDocClick = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) closeContextMenu();
    };
    const onEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeContextMenu();
    };
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onEscape);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onEscape);
    };
  }, [menu, closeContextMenu]);

  // Menu opens at the raw click point; once its real size is known (after this layout
  // pass, before paint — no visible flash), clamp it back on-screen. Without this, a
  // right-click near the window's right/bottom edge rendered the menu partially or fully
  // off-screen with its bottom items ("Delete"/"Ripple delete") unreachable.
  useLayoutEffect(() => {
    if (!menu || !rootRef.current) {
      setPos(null);
      return;
    }
    const rect = rootRef.current.getBoundingClientRect();
    const margin = 4;
    const left = Math.max(margin, Math.min(menu.x, window.innerWidth - rect.width - margin));
    const top = Math.max(margin, Math.min(menu.y, window.innerHeight - rect.height - margin));
    setPos({ left, top });
  }, [menu]);

  if (!menu) return null;

  const track = project?.tracks.find((t) => t.id === menu.trackId);
  const clip = track?.clips.find((c) => c.id === menu.clipId);
  const locked = track?.locked ?? false;
  const isMedia = clip?.type === "video" || clip?.type === "audio";

  function run(fn: () => void) {
    closeContextMenu();
    fn();
  }

  return (
    <div
      className="context-menu"
      ref={rootRef}
      style={{ left: pos?.left ?? menu.x, top: pos?.top ?? menu.y }}
    >
      <button
        type="button"
        disabled={locked}
        onClick={() => run(() => void dispatch(splitClip(menu.trackId, menu.clipId, menu.atSecs)))}
      >
        Split at click
      </button>
      <button type="button" disabled={locked} onClick={() => run(() => void duplicateSelection())}>
        Duplicate
      </button>
      <button type="button" onClick={() => run(copySelection)}>
        Copy
      </button>
      <button type="button" disabled={locked || !clipboard} onClick={() => run(() => void pasteAtPlayhead())}>
        Paste
      </button>
      {isMedia && clip && (
        <button
          type="button"
          disabled={locked}
          onClick={() => run(() => void dispatch(setClipEnabled(menu.trackId, menu.clipId, !clip.enabled)))}
        >
          {clip.enabled ? "Disable clip" : "Enable clip"}
        </button>
      )}
      <div className="context-menu-divider" />
      <button
        type="button"
        className="danger"
        disabled={locked}
        onClick={() =>
          run(async () => {
            await useEditorStore.getState().dispatch({
              command: "DeleteClip",
              track_id: menu.trackId,
              clip_id: menu.clipId,
              ripple: false,
            });
            useEditorStore.getState().select(null);
          })
        }
      >
        Delete
      </button>
      <button
        type="button"
        className="danger"
        disabled={locked}
        onClick={() =>
          run(async () => {
            await useEditorStore.getState().dispatch({
              command: "DeleteClip",
              track_id: menu.trackId,
              clip_id: menu.clipId,
              ripple: true,
            });
            useEditorStore.getState().select(null);
          })
        }
      >
        Ripple delete
      </button>
    </div>
  );
}
