import { useEffect, useState } from "react";
import { TopBar } from "./components/TopBar";
import { LeftPanel } from "./components/leftpanel/LeftPanel";
import { PreviewPanel } from "./components/preview/PreviewPanel";
import { InspectorPanel } from "./components/inspector/InspectorPanel";
import { TimelineSection } from "./components/timeline/TimelineSection";
import { ExportDialog } from "./components/dialogs/ExportDialog";
import { Toasts } from "./components/Toasts";
import { WelcomeScreen } from "./components/WelcomeScreen";
import { ContextMenu } from "./components/ContextMenu";
import { connectStoreToBackendEvents, useEditorStore } from "./store/editorStore";
import * as ipc from "./lib/ipc";
import { importFromPath, createNewProjectFlow } from "./lib/projectFlows";
import { deleteSelected, splitSelectedAtPlayhead } from "./timeline/interactions";
import { timelineDuration } from "./timeline/layout";

export function App() {
  const [exportOpen, setExportOpen] = useState(false);

  useEffect(() => connectStoreToBackendEvents(), []);

  useEffect(() => {
    const unsubDrop = ipc.onDragDrop((paths) => {
      const path = paths.find((p) => /\.(mp4|mov|mkv|webm|avi|mp3|wav|m4a|aac)$/i.test(p));
      if (path) void importFromPath(path);
    });
    const unsubEnter = ipc.onDragEnter(() => {
      document.querySelector(".drop-zone")?.classList.add("drag-over");
    });
    const unsubLeave = ipc.onDragLeave(() => {
      document.querySelector(".drop-zone")?.classList.remove("drag-over");
    });
    return () => {
      unsubDrop();
      unsubEnter();
      unsubLeave();
    };
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      const store = useEditorStore.getState();
      const mod = e.ctrlKey || e.metaKey;

      if (e.code === "Space") {
        e.preventDefault();
        if (!store.project) return;
        if (store.playing) store.stopPlayback();
        else void store.startPlayback();
        return;
      }

      // Ctrl/Cmd combos first — single-letter shortcuts below must not also fire for
      // these (e.g. Ctrl+V pasting must not also switch the tool to "select" via "v").
      if (mod) {
        if (e.key === "s" || e.key === "S") {
          e.preventDefault();
          void store.saveProject();
        } else if (e.key === "n" && !e.shiftKey) {
          e.preventDefault();
          void createNewProjectFlow();
        } else if (e.key === "z" || e.key === "Z") {
          e.preventDefault();
          if (e.shiftKey) void store.redo();
          else void store.undo();
        } else if (e.key === "y" || e.key === "Y") {
          e.preventDefault();
          void store.redo();
        } else if (e.key === "c" || e.key === "C") {
          e.preventDefault();
          store.copySelection();
        } else if (e.key === "v" || e.key === "V") {
          e.preventDefault();
          void store.pasteAtPlayhead();
        } else if (e.key === "d" || e.key === "D") {
          e.preventDefault();
          void store.duplicateSelection();
        }
        return;
      }

      if (e.key === "s" || e.key === "S") {
        void splitSelectedAtPlayhead();
        return;
      }
      if (e.key === "v" || e.key === "V") {
        store.setTool("select");
        return;
      }
      if (e.key === "c" || e.key === "C") {
        store.setTool("razor");
        return;
      }
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        void deleteSelected(e.shiftKey);
        return;
      }
      if (e.key === "+" || e.key === "=") {
        store.setZoom(store.pxPerSec + 20);
        return;
      }
      if (e.key === "-" || e.key === "_") {
        store.setZoom(store.pxPerSec - 20);
        return;
      }
      if (e.key === "Home") {
        e.preventDefault();
        void store.seekTo(0);
        return;
      }
      if (e.key === "End") {
        e.preventDefault();
        if (store.project) void store.seekTo(timelineDuration(store.project));
        return;
      }
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        const dir = e.key === "ArrowLeft" ? -1 : 1;
        const fps = store.project?.settings.fps ?? 30;
        const step = e.shiftKey ? 1 : 1 / fps;
        void store.seekTo(store.playhead + dir * step);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <>
      <WelcomeScreen />
      <TopBar onExport={() => setExportOpen(true)} />
      <div className="workspace">
        <LeftPanel />
        <PreviewPanel />
        <InspectorPanel />
      </div>
      <TimelineSection />
      <ExportDialog open={exportOpen} onClose={() => setExportOpen(false)} />
      <ContextMenu />
      <Toasts />
    </>
  );
}
