// P1 webview preview engine: renders the timeline into a plain <canvas> using browser-
// native decode (<video>/<img> elements), no engine round trip. See docs/preview-webview.md
// for the architecture/why. This is a mirror of renderly-core's timeline evaluation
// (clip lookup, source-time math) and compose/mod.rs's cover-fit + user-transform math
// (see cover_uv / layer_params) — NOT a reimplementation of effects (P1 explicitly does
// not render effects/masks/LUTs/captions; that's P2's WASM compositor, see
// docs/preview-webview.md "Non-negotiables" #1).

import type { Clip, ClipTransform, MediaClip, MediaItem, Project } from "../lib/types";
import { IDENTITY_TRANSFORM, clipDurationSecs } from "../lib/types";

/// localStorage flag: set to "native" to opt back into the native wgpu child-window
/// preview (fallback during the migration — see docs/preview-webview.md phase P3/P4).
/// Any other value (including unset) uses the webview engine, which is the default.
const PREVIEW_ENGINE_KEY = "renderly.previewEngine";

export function isWebviewPreview(): boolean {
  try {
    return localStorage.getItem(PREVIEW_ENGINE_KEY) !== "native";
  } catch {
    // localStorage unavailable (e.g. some sandboxed contexts) — default to the new path.
    return true;
  }
}

/// Debug/QA stats exposed on `window.__previewStats` in dev builds (see harness
/// verification requirements in docs/preview-webview.md). Not present in production.
export interface PreviewStats {
  fps: number;
  drawMs: number;
  drops: number;
}

type ActiveLayer = {
  trackId: string;
  clip: MediaClip;
  media: MediaItem;
  sourceTimeSecs: number;
};

/// Mirrors `active_layers` in renderly-core/src/export/mod.rs: iterate tracks in array
/// order (first = bottom, per compose/mod.rs's `composite` doc comment), skip hidden
/// tracks / disabled clips / non-video tracks, and pick the one clip covering `t`.
function activeLayersAt(project: Project, t: number): ActiveLayer[] {
  const layers: ActiveLayer[] = [];
  for (const track of project.tracks) {
    if (track.kind !== "video" || track.hidden) continue;
    let best: MediaClip | null = null;
    for (const clip of track.clips as Clip[]) {
      if (clip.type !== "video" || !clip.enabled) continue;
      const dur = clipDurationSecs(clip);
      const end = clip.position_secs + dur;
      if (t >= clip.position_secs && t < end) {
        if (!best || clip.position_secs > best.position_secs) best = clip;
      }
    }
    if (!best) continue;
    const media = project.media.find((m) => m.id === best!.media_id);
    if (!media) continue;
    const speed = best.speed && best.speed > 0 ? best.speed : 1;
    const sourceTimeSecs = best.source_in_secs + (t - best.position_secs) * speed;
    layers.push({ trackId: track.id, clip: best, media, sourceTimeSecs });
  }
  return layers;
}

/// Look ahead a little past `t` on the same tracks to find the *next* clip about to
/// become active, so its <video> can be pre-seeked before the cut lands (gapless cuts).
function upcomingLayers(project: Project, t: number, horizonSecs: number): ActiveLayer[] {
  const layers: ActiveLayer[] = [];
  for (const track of project.tracks) {
    if (track.kind !== "video" || track.hidden) continue;
    let best: MediaClip | null = null;
    for (const clip of track.clips as Clip[]) {
      if (clip.type !== "video" || !clip.enabled) continue;
      const startsIn = clip.position_secs - t;
      if (startsIn > 0 && startsIn <= horizonSecs) {
        if (!best || clip.position_secs < best.position_secs) best = clip;
      }
    }
    if (!best) continue;
    const media = project.media.find((m) => m.id === best!.media_id);
    if (!media) continue;
    layers.push({ trackId: track.id, clip: best, media, sourceTimeSecs: best.source_in_secs });
  }
  return layers;
}

/// Mirror of `cover_uv` in renderly-core/src/compose/mod.rs, expressed as a pixel-space
/// crop rect on the source instead of a UV scale/offset — equivalent math, just phrased
/// for `ctx.drawImage`'s 9-arg form.
function coverSourceRect(
  srcW: number,
  srcH: number,
  outW: number,
  outH: number,
): { sx: number; sy: number; sw: number; sh: number } {
  const srcAspect = srcW / srcH;
  const outAspect = outW / outH;
  let sw: number;
  let sh: number;
  if (srcAspect > outAspect) {
    sh = srcH;
    sw = srcH * outAspect;
  } else {
    sw = srcW;
    sh = srcW / outAspect;
  }
  return { sx: (srcW - sw) / 2, sy: (srcH - sh) / 2, sw, sh };
}

const SEEK_CORRECT_THRESHOLD_SECS = 0.08;
const PRESEEK_HORIZON_SECS = 0.5;
const DRIFT_SNAP_THRESHOLD_SECS = 0.25;
const DRIFT_SLEW_FACTOR = 0.2;

export class WebviewPreviewEngine {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private project: Project | null = null;
  private assetUrl: (path: string) => string;

  private videoPool = new Map<string, HTMLVideoElement>();
  private imgPool = new Map<string, HTMLImageElement>();

  private playing = false;
  private clockBaseSecs = 0;
  private clockBasePerf = 0;
  private pausedTimeSecs = 0;

  private rafId: number | null = null;
  /// True when `rafId` was scheduled via `setTimeout` rather than `requestAnimationFrame`
  /// — see `scheduleFrame`'s doc comment.
  private usingTimerFallback = false;
  private disposed = false;

  private cssW = 0;
  private cssH = 0;

  // Perf stats (dev only).
  private frameCount = 0;
  private statsWindowStart = 0;
  private lastDrawT = 0;
  private lastFrameTime = 0;
  private drops = 0;

  constructor(canvas: HTMLCanvasElement, assetUrl: (path: string) => string) {
    this.canvas = canvas;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("2d context unavailable");
    this.ctx = ctx;
    this.assetUrl = assetUrl;
    this.statsWindowStart = performance.now();
  }

  /// Resize the backing bitmap. Only reassigns `canvas.width`/`.height` when the target
  /// actually changed — see the identical rationale in timeline/renderer.ts's
  /// `syncCanvasSize`: writing those properties (even to their current value) discards
  /// and reallocates the bitmap, which would be a needless GPU/CPU cost done every frame
  /// if called from the draw loop instead of a resize observer.
  setSize(cssW: number, cssH: number): void {
    const dpr = window.devicePixelRatio || 1;
    const targetW = Math.max(1, Math.round(cssW * dpr));
    const targetH = Math.max(1, Math.round(cssH * dpr));
    if (this.canvas.width !== targetW || this.canvas.height !== targetH) {
      this.canvas.width = targetW;
      this.canvas.height = targetH;
    }
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.cssW = cssW;
    this.cssH = cssH;
    // Repaint immediately at the current time so a resize doesn't show a stale frame.
    this.drawFrame(this.currentTimeSecs());
  }

  setProject(project: Project | null): void {
    this.project = project;
    this.reconcileMediaElements();
    this.drawFrame(this.currentTimeSecs());
  }

  private reconcileMediaElements(): void {
    const wanted = new Set<string>();
    for (const media of this.project?.media ?? []) {
      if (media.kind === "video") {
        wanted.add(media.id);
        if (!this.videoPool.has(media.id)) {
          const el = document.createElement("video");
          el.muted = true;
          el.preload = "auto";
          el.playsInline = true;
          el.crossOrigin = "anonymous";
          el.src = this.assetUrl(media.path);
          el.style.display = "none";
          document.body.appendChild(el);
          this.videoPool.set(media.id, el);
        }
      } else if (media.kind === "image") {
        wanted.add(media.id);
        if (!this.imgPool.has(media.id)) {
          const el = new Image();
          el.src = this.assetUrl(media.path);
          this.imgPool.set(media.id, el);
        }
      }
    }
    for (const [id, el] of this.videoPool) {
      if (!wanted.has(id)) {
        el.pause();
        el.removeAttribute("src");
        el.load();
        el.remove();
        this.videoPool.delete(id);
      }
    }
    for (const id of this.imgPool.keys()) {
      if (!wanted.has(id)) this.imgPool.delete(id);
    }
  }

  private currentTimeSecs(): number {
    if (!this.playing) return this.pausedTimeSecs;
    return this.clockBaseSecs + (performance.now() - this.clockBasePerf) / 1000;
  }

  play(atSecs: number): void {
    this.playing = true;
    this.clockBaseSecs = atSecs;
    this.clockBasePerf = performance.now();
    this.frameCount = 0;
    this.statsWindowStart = performance.now();
    this.lastFrameTime = 0;
    this.drops = 0;
    if (this.rafId == null) this.scheduleFrame();
  }

  /// `requestAnimationFrame` is throttled/suspended by the browser when the document is
  /// hidden (backgrounded tab or, in this repo's automated browser-pane test harness, a
  /// tab that's never actually given compositor focus — see docs/preview-webview.md
  /// "Harness verification"). That throttling is *correct* product behavior for a real
  /// backgrounded window (no point burning GPU on an invisible canvas) but would make the
  /// harness unable to measure sustained playback fps at all, so fall back to a ~60Hz
  /// `setTimeout` only while `document.hidden` is true.
  private scheduleFrame(): void {
    this.usingTimerFallback = typeof document !== "undefined" && document.hidden;
    this.rafId = this.usingTimerFallback
      ? (window.setTimeout(this.loop, 16) as unknown as number)
      : requestAnimationFrame(this.loop);
  }

  private cancelScheduledFrame(): void {
    if (this.rafId == null) return;
    if (this.usingTimerFallback) window.clearTimeout(this.rafId);
    else cancelAnimationFrame(this.rafId);
    this.rafId = null;
  }

  pause(atSecs: number): void {
    this.playing = false;
    this.pausedTimeSecs = atSecs;
    for (const v of this.videoPool.values()) v.pause();
    this.drawFrame(atSecs);
  }

  /// Backend `playback:tick` arrives ~30Hz while the audio path is the master clock.
  /// Slews the local rAF clock toward the reported time (snaps if drift is large, e.g.
  /// after a seek) rather than following it directly, so playback stays visually smooth
  /// between ticks instead of stepping. In the browser harness (no backend), no ticks
  /// ever arrive and the pure rAF clock (see `currentTimeSecs`) drives everything.
  onTick(tickSecs: number): void {
    if (!this.playing) return;
    const now = performance.now();
    const predicted = this.clockBaseSecs + (now - this.clockBasePerf) / 1000;
    const drift = tickSecs - predicted;
    this.clockBaseSecs =
      Math.abs(drift) > DRIFT_SNAP_THRESHOLD_SECS ? tickSecs : predicted + drift * DRIFT_SLEW_FACTOR;
    this.clockBasePerf = now;
  }

  /// Paused-state scrub: set the target time, seek any active video (redraw fires on its
  /// 'seeked' event so the paint reflects the actually-decoded frame), and draw
  /// immediately too so a cached/instant seek doesn't wait.
  seek(atSecs: number): void {
    this.pausedTimeSecs = atSecs;
    if (this.playing) return; // live playback frame will pick this up on its own
    this.drawFrame(atSecs);
  }

  /// Called when the store's project changes while paused (e.g. a transform-handle drag
  /// patches the project optimistically) — redraw at the current time so the change is
  /// visible immediately, at full local frame rate, without any backend round trip.
  notifyProjectPatched(): void {
    if (this.playing) return;
    this.drawFrame(this.pausedTimeSecs);
  }

  private loop = (): void => {
    if (this.disposed) return;
    if (!this.playing) {
      this.rafId = null;
      return;
    }
    const t = this.currentTimeSecs();
    this.drawFrame(t);
    this.scheduleFrame();
  };

  private drawFrame(t: number): void {
    const drawStart = performance.now();
    const { ctx, cssW, cssH } = this;
    ctx.save();
    ctx.clearRect(0, 0, cssW, cssH);
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, cssW, cssH);

    const project = this.project;
    if (project && cssW > 0 && cssH > 0) {
      const active = activeLayersAt(project, t);
      const activeIds = new Set(active.map((l) => l.media.id));

      // Pre-seek upcoming clips' videos so the cut is gapless.
      for (const upcoming of upcomingLayers(project, t, PRESEEK_HORIZON_SECS)) {
        const video = this.videoPool.get(upcoming.media.id);
        if (!video || activeIds.has(upcoming.media.id)) continue;
        if (Math.abs(video.currentTime - upcoming.sourceTimeSecs) > SEEK_CORRECT_THRESHOLD_SECS) {
          try {
            video.currentTime = upcoming.sourceTimeSecs;
          } catch {
            /* not seekable yet (metadata not loaded) — next frame will retry */
          }
        }
      }

      // Pause any video that isn't part of this frame's active set.
      for (const [id, video] of this.videoPool) {
        if (!activeIds.has(id) && !video.paused) video.pause();
      }

      for (const layer of active) {
        const transform: ClipTransform = layer.clip.transform ?? IDENTITY_TRANSFORM;
        if (layer.media.kind === "video") {
          const video = this.videoPool.get(layer.media.id);
          if (!video || video.readyState < 2) continue;
          const speed = layer.clip.speed && layer.clip.speed > 0 ? layer.clip.speed : 1;
          if (video.playbackRate !== speed) video.playbackRate = speed;
          const drift = Math.abs(video.currentTime - layer.sourceTimeSecs);
          if (drift > SEEK_CORRECT_THRESHOLD_SECS) {
            try {
              video.currentTime = layer.sourceTimeSecs;
            } catch {
              /* ignore */
            }
          }
          if (this.playing && video.paused) void video.play().catch(() => {});
          if (!this.playing && !video.paused) video.pause();
          this.drawLayer(video, video.videoWidth || layer.media.width || 1, video.videoHeight || layer.media.height || 1, transform);
        } else if (layer.media.kind === "image") {
          const img = this.imgPool.get(layer.media.id);
          if (!img || !img.complete || img.naturalWidth === 0) continue;
          this.drawLayer(img, img.naturalWidth, img.naturalHeight, transform);
        }
      }
    }
    ctx.restore();

    // Stats (dev only) — see docs/preview-webview.md harness verification.
    this.lastDrawT = performance.now() - drawStart;
    this.frameCount += 1;
    if (this.lastFrameTime > 0) {
      const gap = drawStart - this.lastFrameTime;
      const fps = this.project?.settings.fps || 60;
      if (this.playing && gap > (1000 / fps) * 1.5) this.drops += 1;
    }
    this.lastFrameTime = drawStart;
    const elapsed = performance.now() - this.statsWindowStart;
    if (elapsed >= 1000) {
      const fps = (this.frameCount * 1000) / elapsed;
      this.publishStats({ fps, drawMs: this.lastDrawT, drops: this.drops });
      this.frameCount = 0;
      this.statsWindowStart = performance.now();
    } else {
      this.publishStats({
        fps: (globalThis as { __previewStats?: PreviewStats }).__previewStats?.fps ?? 0,
        drawMs: this.lastDrawT,
        drops: this.drops,
      });
    }
  }

  private drawLayer(
    source: CanvasImageSource,
    srcW: number,
    srcH: number,
    transform: ClipTransform,
  ): void {
    const { ctx, cssW, cssH } = this;
    const { sx, sy, sw, sh } = coverSourceRect(srcW, srcH, cssW, cssH);
    ctx.save();
    ctx.globalAlpha = Math.max(0, Math.min(1, transform.opacity));
    ctx.translate(cssW / 2, cssH / 2);
    ctx.translate((transform.x * cssW) / 2, (-transform.y * cssH) / 2);
    // Screen-space rotation is the negation of the NDC (+Y up) rotation — see the
    // identical note (`svgAngle = -transform.rotation_deg`) in PreviewHandlesOverlay.tsx.
    ctx.rotate((-transform.rotation_deg * Math.PI) / 180);
    ctx.scale(transform.scale_x, transform.scale_y);
    ctx.drawImage(source, sx, sy, sw, sh, -cssW / 2, -cssH / 2, cssW, cssH);
    ctx.restore();
  }

  private publishStats(stats: PreviewStats): void {
    if ((import.meta as { env?: { DEV?: boolean } }).env?.DEV) {
      (globalThis as unknown as { __previewStats?: PreviewStats }).__previewStats = stats;
    }
  }

  dispose(): void {
    this.disposed = true;
    this.cancelScheduledFrame();
    for (const v of this.videoPool.values()) {
      v.pause();
      v.removeAttribute("src");
      v.load();
      v.remove();
    }
    this.videoPool.clear();
    this.imgPool.clear();
  }
}
