// Higher-level flows that combine native dialogs (ipc.ts) with store actions. Shared by
// WelcomeScreen and TopBar so "New/Open/Import" behave identically everywhere.

import * as ipc from "./ipc";
import { useEditorStore } from "../store/editorStore";
import { fileName } from "./format";

function isMediaPath(path: string): boolean {
  return /\.(mp4|mov|mkv|webm|avi|mp3|wav|m4a|aac)$/i.test(path);
}

export async function importFromPath(path: string): Promise<void> {
  const store = useEditorStore.getState();
  if (!isMediaPath(path)) {
    store.toast("That file type is not supported yet", "error");
    return;
  }
  if (!store.project) {
    const ready = await store.quickStart();
    if (!ready) return;
  }
  await useEditorStore.getState().importMediaSmart(path);
}

export async function pickAndImportMedia(): Promise<void> {
  const store = useEditorStore.getState();
  if (!store.project) {
    await quickStartWithImport();
    return;
  }
  const path = await ipc.pickMediaFile();
  if (!path) return;
  await store.importMediaSmart(path);
}

export async function quickStartWithImport(): Promise<void> {
  const store = useEditorStore.getState();
  try {
    const ready = await store.quickStart();
    if (!ready) return;
    const mediaPath = await ipc.pickMediaFile();
    if (!mediaPath) {
      store.toast("Project ready — import media when you're set", "info");
      return;
    }
    await store.importMediaSmart(mediaPath);
  } catch (e) {
    store.toast(`Quick start failed: ${e instanceof Error ? e.message : String(e)}`, "error");
  }
}

export async function createNewProjectFlow(): Promise<void> {
  const store = useEditorStore.getState();
  const path = await ipc.pickProjectSavePath();
  if (!path) return;
  await store.createNewProject(path, "Untitled edit");
  store.toast("New project created", "success");
  const mediaPath = await ipc.pickMediaFile();
  if (mediaPath) await store.importMediaSmart(mediaPath);
  else store.toast("Import media when you're ready", "info");
}

export async function openExistingProjectFlow(): Promise<void> {
  const store = useEditorStore.getState();
  const path = await ipc.pickProjectFileToOpen();
  if (!path) return;
  await store.loadProjectFromPath(path);
  store.toast("Project opened", "success");
}

export async function runExportFlow(preset: string): Promise<void> {
  const store = useEditorStore.getState();
  if (!store.project) return;
  const outputPath = await ipc.pickExportSavePath(`${store.project.name}.mp4`);
  if (!outputPath) return;
  try {
    store.toast("Exporting… this may take a minute", "info");
    await ipc.exportProject(outputPath, preset);
    store.toast(`Exported to ${fileName(outputPath)}`, "success");
  } catch (e) {
    store.toast(`Export failed: ${e instanceof Error ? e.message : String(e)}`, "error");
  }
}
