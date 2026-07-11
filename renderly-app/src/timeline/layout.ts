// Pure geometry helpers shared by renderer.ts (drawing) and interactions.ts (hit-testing) —
// keeping both in agreement about where things are on the canvas.

import { clipDurationSecs, type Clip, type Project } from "../lib/types";

export const TRACK_LABEL_W = 88;
export const RULER_H = 24;
export const TRACK_H = 48;
export const TRACK_GAP = 6;
export const CONTENT_PAD_X = 8;

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

export function collectSnapTimes(
  project: Project,
  playheadSecs: number,
  excludeClipId?: string,
): number[] {
  const times = new Set<number>([playheadSecs, 0, timelineDuration(project)]);
  for (const track of project.tracks) {
    for (const clip of track.clips) {
      if (clip.id === excludeClipId) continue;
      times.add(clip.position_secs);
      times.add(clip.position_secs + clipDurationSecs(clip));
    }
  }
  // Coarse second grid (not every frame — that floods the set at long durations).
  const duration = timelineDuration(project);
  for (let t = 1; t < duration; t += 1) times.add(t);
  return [...times];
}

const SNAP_THRESHOLD_PX = 10;

export function snapTime(
  secs: number,
  project: Project,
  playheadSecs: number,
  pxPerSec: number,
  snapEnabled: boolean,
  excludeClipId?: string,
): number {
  const fps = project.settings.fps || 30;
  const framed = snapToFrame(Math.max(0, secs), fps);
  if (!snapEnabled) return framed;

  const threshold = SNAP_THRESHOLD_PX / pxPerSec;
  let best = framed;
  let bestDist = threshold;
  for (const target of collectSnapTimes(project, playheadSecs, excludeClipId)) {
    const dist = Math.abs(target - secs);
    if (dist < bestDist) {
      bestDist = dist;
      best = target;
    }
  }
  // Prefer exact clip-edge hits; otherwise keep frame-quantized value.
  return bestDist < threshold ? best : framed;
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
      if (y < y0 + 18 || x < cx || x > cx + cw) continue;
      if (x <= cx + TRIM_HANDLE_PX) return { trackId: track.id, clip, trackIndex: ti, edge: "left" };
      if (x >= cx + cw - TRIM_HANDLE_PX) {
        return { trackId: track.id, clip, trackIndex: ti, edge: "right" };
      }
      return { trackId: track.id, clip, trackIndex: ti, edge: "body" };
    }
  }
  return null;
}
