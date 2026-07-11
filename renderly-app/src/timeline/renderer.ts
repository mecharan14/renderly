// Pure canvas draw code for the timeline. No hex color literals here — every color comes
// from timeline/theme.ts (which reads styles/tokens.css), enforced by a grep gate (see
// docs/architecture.md). No DOM/event handling here either — see interactions.ts.

import { fileName, formatTimecode } from "../lib/format";
import { clipDurationSecs, type Project, type Selection, type Track } from "../lib/types";
import { clipLeft, RULER_H, secsFromCanvasX, timelineDuration, trackLayout, TRACK_LABEL_W } from "./layout";
import { getTimelineTheme } from "./theme";
import type { DragGhost, MediaAssetEntry, ThumbnailAsset, WaveformAsset } from "../store/editorStore";

export interface TimelineRenderState {
  project: Project | null;
  playheadSecs: number;
  selection: Selection | null;
  pxPerSec: number;
  scrollX?: number;
  scrollY?: number;
  dragGhost?: DragGhost | null;
  snapGuideSecs?: number | null;
  mediaAssets?: Record<string, MediaAssetEntry>;
}

/// Tiles the strip image across the clip's rectangle, sampling the tile nearest each
/// on-screen column's source time — the same "filmstrip" look CapCut uses. Correctly
/// reflects trimming: `clip.source_in_secs`/`source_out_secs` (not the whole media) map
/// onto the drawn range, so a trimmed clip shows only its trimmed sub-range of tiles.
function drawVideoThumbnails(
  ctx: CanvasRenderingContext2D,
  sourceInSecs: number,
  sourceOutSecs: number,
  thumb: ThumbnailAsset,
  x: number,
  y: number,
  cw: number,
  ch: number,
) {
  if (!thumb.image || thumb.intervalSecs <= 0) return;
  const tileCount = thumb.cols * thumb.rows;
  const aspect = thumb.tileWidth / thumb.tileHeight;
  const drawH = ch;
  const drawW = Math.max(4, drawH * aspect);
  const duration = Math.max(sourceOutSecs - sourceInSecs, 1e-6);

  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, cw, ch);
  ctx.clip();
  for (let dx = 0; dx < cw; dx += drawW) {
    const sourceTime = sourceInSecs + (dx / cw) * duration;
    const tileIndex = Math.max(0, Math.min(tileCount - 1, Math.round(sourceTime / thumb.intervalSecs)));
    const tileX = (tileIndex % thumb.cols) * thumb.tileWidth;
    const tileY = Math.floor(tileIndex / thumb.cols) * thumb.tileHeight;
    ctx.drawImage(
      thumb.image,
      tileX,
      tileY,
      thumb.tileWidth,
      thumb.tileHeight,
      x + dx,
      y,
      Math.min(drawW, x + cw - (x + dx)),
      drawH,
    );
  }
  ctx.restore();
}

/// `peaks` covers the whole source media at a fixed `bucketSecs` stride regardless of
/// trim — slices out just the buckets in `[sourceInSecs, sourceOutSecs)` so a trimmed
/// clip's waveform matches what will actually play.
function drawWaveform(
  ctx: CanvasRenderingContext2D,
  sourceInSecs: number,
  sourceOutSecs: number,
  wave: WaveformAsset,
  x: number,
  y: number,
  cw: number,
  ch: number,
  color: string,
) {
  const { peaks, bucketSecs } = wave;
  if (peaks.length === 0 || bucketSecs <= 0) return;
  const startIdx = Math.max(0, Math.floor(sourceInSecs / bucketSecs));
  const endIdx = Math.min(peaks.length, Math.ceil(sourceOutSecs / bucketSecs));
  const count = endIdx - startIdx;
  if (count <= 0) return;

  const mid = y + ch / 2;
  const barW = Math.max(1, cw / count);
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, cw, ch);
  ctx.clip();
  ctx.fillStyle = color;
  for (let i = 0; i < count; i++) {
    const peak = Math.min(1, peaks[startIdx + i] ?? 0);
    const barH = Math.max(1, peak * ch);
    ctx.fillRect(x + i * barW, mid - barH / 2, Math.max(1, barW - 0.5), barH);
  }
  ctx.restore();
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
  scrollX = 0,
  scrollY = 0,
) {
  const theme = getTimelineTheme();
  const x = clipLeft(ghost.positionSecs, pxPerSec, scrollX);
  const cw = Math.max(ghost.durationSecs * pxPerSec, 8);
  const color = ghost.valid ? theme.accent : theme.danger;

  const { trackH, laneTop } = trackLayout(canvasH, Math.max(project.tracks.length, 1), scrollY);
  const isNewTrack = ghost.trackIndex >= project.tracks.length;
  const y = isNewTrack ? laneTop(project.tracks.length) : laneTop(ghost.trackIndex) + 18;
  const h = isNewTrack ? trackH : trackH - 22;

  if (isNewTrack) {
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

  const {
    project,
    playheadSecs,
    selection,
    pxPerSec,
    dragGhost,
    snapGuideSecs,
    scrollX = 0,
    scrollY = 0,
  } = state;

  if (!project || project.tracks.length === 0) {
    ctx.fillStyle = theme.text3;
    ctx.font = "500 13px var(--font-ui), Segoe UI, sans-serif";
    ctx.textAlign = "center";
    ctx.fillText("Import media to build your timeline", w / 2, h / 2);
    ctx.textAlign = "left";
    return;
  }

  const duration = timelineDuration(project);
  const { trackH, laneTop } = trackLayout(h, project.tracks.length, scrollY);

  // Lane background (scrollable content region)
  ctx.fillStyle = theme.timelineBg;
  ctx.fillRect(TRACK_LABEL_W, RULER_H, w - TRACK_LABEL_W, h - RULER_H);

  // Ruler (fixed height, scrolls horizontally with content)
  ctx.fillStyle = theme.rulerBg;
  ctx.fillRect(0, 0, w, RULER_H);
  ctx.strokeStyle = theme.border;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, RULER_H + 0.5);
  ctx.lineTo(w, RULER_H + 0.5);
  ctx.stroke();

  ctx.fillStyle = theme.rulerText;
  ctx.font = "500 10px var(--font-ui), Segoe UI, sans-serif";
  const step = pxPerSec >= 120 ? 0.5 : pxPerSec >= 60 ? 1 : pxPerSec >= 30 ? 2 : 5;
  const t0 = Math.max(0, Math.floor(secsFromCanvasX(TRACK_LABEL_W, pxPerSec, scrollX) / step) * step);
  const t1 = secsFromCanvasX(w + 40, pxPerSec, scrollX);
  for (let t = t0; t <= Math.max(duration, t1); t += step) {
    const x = clipLeft(t, pxPerSec, scrollX);
    if (x < TRACK_LABEL_W - 2 || x > w + 2) continue;
    const major = Math.abs(t % (step >= 1 ? 5 : 1)) < 1e-6 || Math.abs(t) < 1e-9;
    ctx.strokeStyle = major ? theme.rulerLineStrong : theme.rulerLineWeak;
    ctx.beginPath();
    ctx.moveTo(x + 0.5, RULER_H);
    ctx.lineTo(x + 0.5, h);
    ctx.stroke();
    // Tick marks on the ruler
    ctx.beginPath();
    ctx.moveTo(x + 0.5, RULER_H - (major ? 10 : 5));
    ctx.lineTo(x + 0.5, RULER_H);
    ctx.stroke();
    if (major) {
      ctx.fillStyle = theme.rulerText;
      ctx.fillText(formatTimecode(t).slice(0, 8), x + 4, 12);
    }
  }

  // Track labels/mute/lock/hide/delete are a real DOM column (components/timeline/
  // TrackHeaders.tsx), overlaid on top of the TRACK_LABEL_W gutter reserved here — not
  // drawn on canvas, so they can be actual interactive controls (dbl-click rename,
  // toggle buttons) instead of hit-tested canvas regions.
  project.tracks.forEach((track, i) => {
    const y = laneTop(i);
    if (y + trackH < RULER_H || y > h) return;

    ctx.fillStyle = theme.trackLaneBg;
    ctx.fillRect(TRACK_LABEL_W, y - 2, w - TRACK_LABEL_W, trackH + 4);

    for (const clip of track.clips) {
      const x = clipLeft(clip.position_secs, pxPerSec, scrollX);
      const cw = Math.max(clipDurationSecs(clip) * pxPerSec, 8);
      if (x + cw < TRACK_LABEL_W || x > w) continue;
      const cy = y + 18;
      const ch = trackH - 22;
      const selected = selection?.trackId === track.id && selection?.clipId === clip.id;
      const disabled = clip.type !== "caption" && !clip.enabled;

      ctx.fillStyle = clipFillColor(track.kind);
      ctx.globalAlpha = disabled ? 0.4 : selected ? 1 : 0.92;
      roundRect(ctx, x, cy, cw, ch, 5);
      ctx.fill();
      ctx.globalAlpha = 1;

      if (clip.type === "video") {
        const thumb = state.mediaAssets?.[clip.media_id]?.thumbnails;
        if (thumb) {
          drawVideoThumbnails(ctx, clip.source_in_secs, clip.source_out_secs, thumb, x, cy, cw, ch);
        }
      } else if (clip.type === "audio") {
        const waveform = state.mediaAssets?.[clip.media_id]?.waveform;
        if (waveform) {
          drawWaveform(
            ctx,
            clip.source_in_secs,
            clip.source_out_secs,
            waveform,
            x,
            cy,
            cw,
            ch,
            theme.clipAudioWave,
          );
        }
      }

      if (selected) {
        ctx.strokeStyle = theme.clipBorderSelected;
        ctx.lineWidth = 2;
        roundRect(ctx, x, cy, cw, ch, 5);
        ctx.stroke();
        // Trim handle affordances
        ctx.fillStyle = theme.clipBorderSelected;
        ctx.fillRect(x, cy, 3, ch);
        ctx.fillRect(x + cw - 3, cy, 3, ch);
      }

      ctx.fillStyle = theme.text1;
      ctx.font = "500 10px var(--font-ui), Segoe UI, sans-serif";
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
    drawDragGhost(ctx, project, dragGhost, pxPerSec, w, h, scrollX, scrollY);
  }

  if (snapGuideSecs != null) {
    const gx = clipLeft(snapGuideSecs, pxPerSec, scrollX);
    if (gx >= TRACK_LABEL_W && gx <= w) {
      ctx.save();
      ctx.strokeStyle = theme.snapGuide;
      ctx.setLineDash([4, 3]);
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.moveTo(gx + 0.5, RULER_H);
      ctx.lineTo(gx + 0.5, h);
      ctx.stroke();
      ctx.restore();
    }
  }

  // Playhead — polished marker with ruler head + shadow line
  const phx = clipLeft(playheadSecs, pxPerSec, scrollX);
  if (phx >= TRACK_LABEL_W - 8 && phx <= w + 8) {
    ctx.save();
    ctx.shadowColor = "rgba(0,0,0,0.55)";
    ctx.shadowBlur = 4;
    ctx.strokeStyle = theme.playhead;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(phx + 0.5, RULER_H - 2);
    ctx.lineTo(phx + 0.5, h);
    ctx.stroke();
    ctx.shadowBlur = 0;

    // Capsule head on the ruler
    const headW = 10;
    const headH = 14;
    ctx.fillStyle = theme.playhead;
    roundRect(ctx, phx - headW / 2, 4, headW, headH, 2);
    ctx.fill();
    // Accent notch
    ctx.fillStyle = theme.accent;
    ctx.fillRect(phx - 1, 6, 2, headH - 4);
    ctx.restore();
  }

  // Fixed gutter over label column (so grid lines don't show under headers)
  ctx.fillStyle = theme.trackHeaderBg;
  ctx.fillRect(0, RULER_H, TRACK_LABEL_W, h - RULER_H);
  ctx.strokeStyle = theme.border;
  ctx.beginPath();
  ctx.moveTo(TRACK_LABEL_W + 0.5, RULER_H);
  ctx.lineTo(TRACK_LABEL_W + 0.5, h);
  ctx.stroke();
}
