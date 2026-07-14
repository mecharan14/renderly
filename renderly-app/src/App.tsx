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
import { connectStoreToBackendEvents } from "./store/editorStore";
import * as ipc from "./lib/ipc";
import { importFromPath } from "./lib/projectFlows";
import { handleGlobalKeyDown } from "./lib/actions";
import { isWebviewPreview } from "./preview/webviewPreviewEngine";

export function App() {
  const [exportOpen, setExportOpen] = useState(false);

  useEffect(() => connectStoreToBackendEvents(), []);

  useEffect(() => {
    void ipc.setPreviewMode(isWebviewPreview() ? "webview" : "native").catch(() => {});
  }, []);

  useEffect(() => {
    const unsubDrop = ipc.onDragDrop((paths) => {
      const path = paths.find((p) =>
        /\.(mp4|mov|mkv|webm|avi|png|jpe?g|webp|gif|bmp|mp3|wav|m4a|aac|flac|ogg)$/i.test(p),
      );
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
      handleGlobalKeyDown(e, { openExport: () => setExportOpen(true) });
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
