// Pure canvas draw code for the timeline. No hex color literals here — every color comes
// from timeline/theme.ts (which reads styles/tokens.css), enforced by a grep gate (see
// docs/architecture.md). No DOM/event handling here either — see interactions.ts.

import { fileName, formatTimecode } from "../lib/format";
import { clipDurationSecs, type Project, type Selection, type Track } from "../lib/types";
import { clipLeft, RULER_H, timelineDuration, trackLayout, TRACK_LABEL_W } from "./layout";
import { getTimelineTheme } from "./theme";
import type { DragGhost } from "../store/editorStore";

export interface TimelineRenderState {
  project: Project | null;
  playheadSecs: number;
  selection: Selection | null;
  pxPerSec: number;
  dragGhost?: DragGhost | null;
  snapGuideSecs?: number | null;
}

function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  const rr = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
}

function clipFillColor(kind: Track["kind"]): string {
  const theme = getTimelineTheme();
  if (kind === "video") return theme.clipVideoBg;
  if (kind === "audio") return theme.clipAudioBg;
  return theme.clipCaptionBg;
}

/// Diagonal hatch overlay marking a locked track's lane — mouse edits are refused there
/// (see interactions.ts), but CLI/MCP agents can still edit it by design.
function drawLockedHatch(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number) {
  const theme = getTimelineTheme();
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.strokeStyle = theme.lockedHatch;
  ctx.lineWidth = 1;
  const step = 10;
  for (let sx = -h; sx < w; sx += step) {
    ctx.beginPath();
    ctx.moveTo(x + sx, y + h);
    ctx.lineTo(x + sx + h, y);
    ctx.stroke();
  }
  ctx.restore();
}

function drawDragGhost(
  ctx: CanvasRenderingContext2D,
  project: Project,
  ghost: DragGhost,
  pxPerSec: number,
  canvasW: number,
  canvasH: number,
) {
  const theme = getTimelineTheme();
  const x = clipLeft(ghost.positionSecs, pxPerSec);
  const cw = Math.max(ghost.durationSecs * pxPerSec, 8);
  const color = ghost.valid ? theme.accent : theme.danger;

  const { trackH, laneTop } = trackLayout(canvasH, Math.max(project.tracks.length, 1));
  const isNewTrack = ghost.trackIndex >= project.tracks.length;
  const y = isNewTrack ? laneTop(project.tracks.length) : laneTop(ghost.trackIndex) + 18;
  const h = isNewTrack ? trackH : trackH - 22;

  if (isNewTrack) {
    // New-track band: a full-width dashed line cueing "drop here to create a track".
    ctx.save();
    ctx.strokeStyle = color;
    ctx.setLineDash([6, 4]);
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(TRACK_LABEL_W, y);
    ctx.lineTo(canvasW, y);
    ctx.stroke();
    ctx.restore();
  }

  ctx.save();
  ctx.globalAlpha = 0.5;
  ctx.fillStyle = color;
  roundRect(ctx, x, y, cw, h, 5);
  ctx.fill();
  ctx.globalAlpha = 1;
  ctx.strokeStyle = color;
  ctx.lineWidth = 2;
  roundRect(ctx, x, y, cw, h, 5);
  ctx.stroke();
  ctx.restore();
}

export function renderTimeline(canvas: HTMLCanvasElement, state: TimelineRenderState): void {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const theme = getTimelineTheme();

  const dpr = window.devicePixelRatio || 1;
  const rect = canvas.getBoundingClientRect();
  canvas.width = rect.width * dpr;
  canvas.height = rect.height * dpr;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  const w = rect.width;
  const h = rect.height;

  ctx.fillStyle = theme.timelineBg;
  ctx.fillRect(0, 0, w, h);

  const { project, playheadSecs, selection, pxPerSec, dragGhost, snapGuideSecs } = state;

  if (!project || project.tracks.length === 0) {
    ctx.fillStyle = theme.text3;
    ctx.font = "500 13px Inter, Segoe UI, sans-serif";
    ctx.textAlign = "center";
    ctx.fillText("Import media to build your timeline", w / 2, h / 2);
    ctx.textAlign = "left";
    return;
  }

  const duration = timelineDuration(project);
  const { trackH, laneTop } = trackLayout(h, project.tracks.length);

  // Ruler
  ctx.fillStyle = theme.rulerBg;
  ctx.fillRect(0, 0, w, RULER_H);
  ctx.strokeStyle = theme.border;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, RULER_H);
  ctx.lineTo(w, RULER_H);
  ctx.stroke();

  ctx.fillStyle = theme.rulerText;
  ctx.font = "500 10px Inter, Segoe UI, sans-serif";
  const step = pxPerSec >= 60 ? 1 : pxPerSec >= 30 ? 2 : 5;
  for (let t = 0; t <= duration; t += step) {
    const x = clipLeft(t, pxPerSec);
    if (x < TRACK_LABEL_W) continue;
    ctx.strokeStyle = t % 5 === 0 ? theme.rulerLineStrong : theme.rulerLineWeak;
    ctx.beginPath();
    ctx.moveTo(x, RULER_H);
    ctx.lineTo(x, h);
    ctx.stroke();
    if (t % 5 === 0) {
      ctx.fillText(formatTimecode(t).slice(0, 5), x + 3, 14);
    }
  }

  // Track labels/mute/lock/hide/delete are a real DOM column (components/timeline/
  // TrackHeaders.tsx), overlaid on top of the TRACK_LABEL_W gutter reserved here — not
  // drawn on canvas, so they can be actual interactive controls (dbl-click rename,
  // toggle buttons) instead of hit-tested canvas regions.
  project.tracks.forEach((track, i) => {
    const y = laneTop(i);

    ctx.fillStyle = theme.trackLaneBg;
    ctx.fillRect(0, y - 2, w, trackH + 4);

    for (const clip of track.clips) {
      const x = clipLeft(clip.position_secs, pxPerSec);
      const cw = Math.max(clipDurationSecs(clip) * pxPerSec, 8);
      const cy = y + 18;
      const ch = trackH - 22;
      const selected = selection?.trackId === track.id && selection?.clipId === clip.id;
      const disabled = clip.type !== "caption" && !clip.enabled;

      ctx.fillStyle = clipFillColor(track.kind);
      ctx.globalAlpha = disabled ? 0.4 : selected ? 1 : 0.92;
      roundRect(ctx, x, cy, cw, ch, 5);
      ctx.fill();
      ctx.globalAlpha = 1;

      if (selected) {
        ctx.strokeStyle = theme.clipBorderSelected;
        ctx.lineWidth = 2;
        roundRect(ctx, x, cy, cw, ch, 5);
        ctx.stroke();
      }

      ctx.fillStyle = theme.text1;
      ctx.font = "500 10px Inter, Segoe UI, sans-serif";
      const label =
        clip.type === "caption"
          ? clip.text.slice(0, 20)
          : fileName(project.media.find((m) => m.id === clip.media_id)?.path ?? track.kind).slice(
              0,
              16,
            );
      if (cw > 28) ctx.fillText(label, x + 6, y + trackH - 10);
    }

    if (track.locked) {
      drawLockedHatch(ctx, TRACK_LABEL_W, y, w - TRACK_LABEL_W, trackH);
    }
  });

  if (dragGhost) {
    drawDragGhost(ctx, project, dragGhost, pxPerSec, w, h);
  }

  if (snapGuideSecs != null) {
    const gx = clipLeft(snapGuideSecs, pxPerSec);
    ctx.save();
    ctx.strokeStyle = theme.snapGuide;
    ctx.setLineDash([4, 3]);
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(gx, RULER_H);
    ctx.lineTo(gx, h);
    ctx.stroke();
    ctx.restore();
  }

  const phx = clipLeft(playheadSecs, pxPerSec);
  ctx.strokeStyle = theme.playhead;
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.moveTo(phx, 0);
  ctx.lineTo(phx, h);
  ctx.stroke();

  ctx.fillStyle = theme.playhead;
  ctx.beginPath();
  ctx.moveTo(phx - 5, 0);
  ctx.lineTo(phx + 5, 0);
  ctx.lineTo(phx, 8);
  ctx.closePath();
  ctx.fill();
}
