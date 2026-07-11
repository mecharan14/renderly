// Drag-and-drop payload for applying a transition from TransitionsPanel onto a timeline
// junction (improvement-plan C4).

import type { ClipTransition, TransitionKind } from "./types";

export const TRANSITION_DRAG_MIME = "application/x-renderly-transition";

export interface TransitionDragPayload {
  kind: TransitionKind;
  duration_secs: number;
}

export function startTransitionDrag(
  e: React.DragEvent,
  kind: TransitionKind,
  durationSecs: number,
) {
  const payload: TransitionDragPayload = { kind, duration_secs: durationSecs };
  e.dataTransfer.setData(TRANSITION_DRAG_MIME, JSON.stringify(payload));
  e.dataTransfer.effectAllowed = "copy";
}

export function readTransitionDrag(e: React.DragEvent | DragEvent): TransitionDragPayload | null {
  const raw = e.dataTransfer?.getData(TRANSITION_DRAG_MIME);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as TransitionDragPayload;
  } catch {
    return null;
  }
}

export function transitionFromPayload(payload: TransitionDragPayload): ClipTransition {
  return { kind: payload.kind, duration_secs: payload.duration_secs };
}
