// Pure geometry helpers shared by renderer.ts (drawing) and interactions.ts (hit-testing) —
// keeping both in agreement about where things are on the canvas.

import { clipDurationSecs, type Clip, type Project } from "../lib/types";

export const TRACK_LABEL_W = 88;
export const RULER_H = 24;

export function clipLeft(secs: number, pxPerSec: number): number {
  return TRACK_LABEL_W + 8 + secs * pxPerSec;
}

export function secsFromCanvasX(x: number, pxPerSec: number): number {
  return Math.max(0, (x - TRACK_LABEL_W - 8) / pxPerSec);
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

export function collectSnapTimes(project: Project, playheadSecs: number, excludeClipId?: string): number[] {
  const times = new Set<number>([playheadSecs, 0]);
  const duration = timelineDuration(project);
  for (let t = 0; t <= duration; t += 1) times.add(t);
  for (const track of project.tracks) {
    for (const clip of track.clips) {
      if (clip.id === excludeClipId) continue;
      times.add(clip.position_secs);
      times.add(clip.position_secs + clipDurationSecs(clip));
    }
  }
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
  if (!snapEnabled) return Math.max(0, secs);
  const threshold = SNAP_THRESHOLD_PX / pxPerSec;
  let best = Math.max(0, secs);
  let bestDist = threshold;
  for (const target of collectSnapTimes(project, playheadSecs, excludeClipId)) {
    const dist = Math.abs(target - secs);
    if (dist < bestDist) {
      bestDist = dist;
      best = target;
    }
  }
  return best;
}

export function trackLayout(h: number, trackCount: number) {
  const usable = h - RULER_H - 8;
  const trackH = Math.min(52, usable / Math.max(trackCount, 1));
  return { trackH, laneTop: (i: number) => RULER_H + 4 + i * (trackH + 6) };
}

/// Which track lane `y` falls in, for drag-and-drop targeting. Returns an index in
/// `[0, trackCount)` for an existing lane, or `trackCount` itself if `y` is below every
/// existing track — the caller's cue to auto-create a new track (CapCut-style drop below
/// the timeline).
export function trackIndexAtY(canvasHeight: number, trackCount: number, y: number): number {
  const { trackH, laneTop } = trackLayout(canvasHeight, Math.max(trackCount, 1));
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
): ClipHit | null {
  const { trackH, laneTop } = trackLayout(canvasHeight, project.tracks.length);

  for (let ti = 0; ti < project.tracks.length; ti++) {
    const track = project.tracks[ti];
    const y0 = laneTop(ti);
    if (y < y0 || y > y0 + trackH) continue;
    for (const clip of track.clips) {
      const cx = clipLeft(clip.position_secs, pxPerSec);
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
