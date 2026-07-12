// Pure geometry helpers shared by renderer.ts (drawing) and interactions.ts (hit-testing) —
// keeping both in agreement about where things are on the canvas.

import {
  clipDurationSecs,
  type Clip,
  type ClipTransition,
  type MediaClip,
  type Project,
  type Track,
} from "../lib/types";

export const TRACK_LABEL_W = 88;
export const RULER_H = 24;
export const TRACK_H = 48;
export const TRACK_GAP = 6;
export const CONTENT_PAD_X = 8;
/// Clip card insets within the lane (approved redesign: labels live inside the card, so
/// the card fills most of the lane). Shared by renderer.ts drawing and hit-testing here.
export const CLIP_TOP_PAD = 6;
export const CLIP_BOTTOM_PAD = 4;

export function clipLeft(secs: number, pxPerSec: number, scrollX = 0): number {
  return TRACK_LABEL_W + CONTENT_PAD_X + secs * pxPerSec - scrollX;
}

export function secsFromCanvasX(x: number, pxPerSec: number, scrollX = 0): number {
  return Math.max(0, (x - TRACK_LABEL_W - CONTENT_PAD_X + scrollX) / pxPerSec);
}

export function timelineDuration(project: Project): number {
  let end = 0;
  for (const track of project.tracks) {
    for (const clip of track.clips) {
      end = Math.max(end, clip.position_secs + clipDurationSecs(clip));
    }
  }
  return Math.max(end, 1);
}

export function contentWidthPx(project: Project, pxPerSec: number): number {
  // Extra pad so the playhead / last clip aren't flush against the right edge.
  return TRACK_LABEL_W + CONTENT_PAD_X + timelineDuration(project) * pxPerSec + 200;
}

export function contentHeightPx(trackCount: number): number {
  return RULER_H + 8 + Math.max(trackCount, 1) * (TRACK_H + TRACK_GAP) + 40;
}

export function maxScrollX(project: Project, pxPerSec: number, viewportW: number): number {
  return Math.max(0, contentWidthPx(project, pxPerSec) - viewportW);
}

export function maxScrollY(trackCount: number, viewportH: number): number {
  return Math.max(0, contentHeightPx(trackCount) - viewportH);
}

export function snapToFrame(secs: number, fps: number): number {
  const f = Math.max(1, fps);
  return Math.max(0, Math.round(secs * f) / f);
}

export function trackLayout(_viewportH: number, _trackCount: number, scrollY = 0) {
  const trackH = TRACK_H;
  return {
    trackH,
    laneTop: (i: number) => RULER_H + 4 + i * (trackH + TRACK_GAP) - scrollY,
  };
}

/// Which track lane `y` falls in, for drag-and-drop targeting. Returns an index in
/// `[0, trackCount)` for an existing lane, or `trackCount` itself if `y` is below every
/// existing track — the caller's cue to auto-create a new track (CapCut-style drop below
/// the timeline).
export function trackIndexAtY(
  canvasHeight: number,
  trackCount: number,
  y: number,
  scrollY = 0,
): number {
  const { trackH, laneTop } = trackLayout(canvasHeight, Math.max(trackCount, 1), scrollY);
  for (let i = 0; i < trackCount; i++) {
    if (y < laneTop(i) + trackH + 3) return i;
  }
  return trackCount;
}

export interface ClipHit {
  trackId: string;
  clip: Clip;
  trackIndex: number;
  edge: "left" | "right" | "body";
}

const TRIM_HANDLE_PX = 8;

export function hitTestClip(
  project: Project,
  x: number,
  y: number,
  canvasHeight: number,
  pxPerSec: number,
  scrollX = 0,
  scrollY = 0,
): ClipHit | null {
  const { trackH, laneTop } = trackLayout(canvasHeight, project.tracks.length, scrollY);

  for (let ti = 0; ti < project.tracks.length; ti++) {
    const track = project.tracks[ti];
    const y0 = laneTop(ti);
    if (y < y0 || y > y0 + trackH) continue;
    for (const clip of track.clips) {
      const cx = clipLeft(clip.position_secs, pxPerSec, scrollX);
      const cw = clipDurationSecs(clip) * pxPerSec;
      if (y < y0 + CLIP_TOP_PAD || x < cx || x > cx + cw) continue;
      if (x <= cx + TRIM_HANDLE_PX) return { trackId: track.id, clip, trackIndex: ti, edge: "left" };
      if (x >= cx + cw - TRIM_HANDLE_PX) {
        return { trackId: track.id, clip, trackIndex: ti, edge: "right" };
      }
      return { trackId: track.id, clip, trackIndex: ti, edge: "body" };
    }
  }
  return null;
}

/// Next video clip on the same track at or after `clip`'s timeline end (C4 junction target).
export function nextVideoClipOnTrack(track: Track, clip: MediaClip): MediaClip | null {
  const end = clip.position_secs + clipDurationSecs(clip);
  return (
    [...track.clips]
      .filter((c): c is MediaClip => c.type === "video")
      .sort((a, b) => a.position_secs - b.position_secs)
      .find((c) => c.id !== clip.id && c.position_secs >= end - 1e-6) ?? null
  );
}

export interface TransitionJunction {
  trackId: string;
  trackIndex: number;
  outgoingClipId: string;
  junctionSecs: number;
  junctionX: number;
  laneY: number;
  laneH: number;
  bodyY: number;
  bodyH: number;
  transition: ClipTransition | null;
  overlapPx: number;
}

const JUNCTION_HIT_HALF_W = 10;

export function listTransitionJunctions(
  project: Project,
  canvasHeight: number,
  pxPerSec: number,
  scrollX = 0,
  scrollY = 0,
): TransitionJunction[] {
  const { trackH, laneTop } = trackLayout(canvasHeight, project.tracks.length, scrollY);
  const out: TransitionJunction[] = [];

  for (let ti = 0; ti < project.tracks.length; ti++) {
    const track = project.tracks[ti];
    if (track.kind !== "video") continue;
    const y = laneTop(ti);
    const bodyY = y + CLIP_TOP_PAD;
    const bodyH = trackH - CLIP_TOP_PAD - CLIP_BOTTOM_PAD;

    const videos = [...track.clips]
      .filter((c): c is MediaClip => c.type === "video")
      .sort((a, b) => a.position_secs - b.position_secs);

    for (const clip of videos) {
      const next = nextVideoClipOnTrack(track, clip);
      if (!next) continue;
      const junctionSecs = clip.position_secs + clipDurationSecs(clip);
      const junctionX = clipLeft(junctionSecs, pxPerSec, scrollX);
      const transition = clip.outgoing_transition ?? null;
      const overlapPx = transition ? Math.max(4, transition.duration_secs * pxPerSec) : 0;
      out.push({
        trackId: track.id,
        trackIndex: ti,
        outgoingClipId: clip.id,
        junctionSecs,
        junctionX,
        laneY: y,
        laneH: trackH,
        bodyY,
        bodyH,
        transition,
        overlapPx,
      });
    }
  }
  return out;
}

export interface TransitionJunctionHit {
  trackId: string;
  clipId: string;
  trackIndex: number;
}

export function hitTestTransitionJunction(
  project: Project,
  x: number,
  y: number,
  canvasHeight: number,
  pxPerSec: number,
  scrollX = 0,
  scrollY = 0,
): TransitionJunctionHit | null {
  for (const j of listTransitionJunctions(project, canvasHeight, pxPerSec, scrollX, scrollY)) {
    if (y < j.bodyY || y > j.bodyY + j.bodyH) continue;
    // Badge sits just inside the outgoing clip — avoid the right-edge trim handle.
    if (x >= j.junctionX - 16 && x <= j.junctionX) {
      return { trackId: j.trackId, clipId: j.outgoingClipId, trackIndex: j.trackIndex };
    }
    if (Math.abs(x - j.junctionX) <= JUNCTION_HIT_HALF_W && y >= j.bodyY + 4 && y <= j.bodyY + j.bodyH - 4) {
      return { trackId: j.trackId, clipId: j.outgoingClipId, trackIndex: j.trackIndex };
    }
  }
  return null;
}
