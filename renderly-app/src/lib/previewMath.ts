// Mirror of renderly-core cover-fit + user transform (compose/mod.rs + composite.wgsl).
// Overlay coordinates: CSS pixels within the letterboxed preview content rect.
// NDC: x/y in [-1,1], +Y up (matches wgpu clip space).

import type { ClipTransform } from "./types";
import { IDENTITY_TRANSFORM } from "./types";

export type NdcPoint = { x: number; y: number };
export type PxPoint = { x: number; y: number };

export type HandleKind =
  | "move"
  | "rotate"
  | "scale-nw"
  | "scale-ne"
  | "scale-sw"
  | "scale-se"
  | "scale-n"
  | "scale-s"
  | "scale-e"
  | "scale-w";

const UNIT_CORNERS: NdcPoint[] = [
  { x: -1, y: -1 },
  { x: 1, y: -1 },
  { x: 1, y: 1 },
  { x: -1, y: 1 },
];

export function resolveTransform(t?: ClipTransform | null): ClipTransform {
  return t ?? IDENTITY_TRANSFORM;
}

/** Apply user scale → rotate → translate in NDC (same order as composite.wgsl). */
export function transformNdcPoint(p: NdcPoint, t: ClipTransform): NdcPoint {
  let x = p.x * t.scale_x;
  let y = p.y * t.scale_y;
  const rad = (t.rotation_deg * Math.PI) / 180;
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  const rx = x * c - y * s;
  const ry = x * s + y * c;
  return { x: rx + t.x, y: ry + t.y };
}

export function transformedCorners(t: ClipTransform): NdcPoint[] {
  return UNIT_CORNERS.map((p) => transformNdcPoint(p, t));
}

export function ndcToPx(p: NdcPoint, w: number, h: number): PxPoint {
  return {
    x: ((p.x + 1) / 2) * w,
    y: ((1 - p.y) / 2) * h,
  };
}

export function pxToNdc(p: PxPoint, w: number, h: number): NdcPoint {
  return {
    x: (p.x / w) * 2 - 1,
    y: 1 - (p.y / h) * 2,
  };
}

export function rotationHandleNdc(t: ClipTransform): NdcPoint {
  const topMid = transformNdcPoint({ x: 0, y: 1 }, t);
  const cx = t.x;
  const cy = t.y;
  const dx = topMid.x - cx;
  const dy = topMid.y - cy;
  const len = Math.hypot(dx, dy) || 1;
  const offset = 0.12;
  return { x: topMid.x + (dx / len) * offset, y: topMid.y + (dy / len) * offset };
}

function dist(a: PxPoint, b: PxPoint): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function pointInPolygon(p: PxPoint, poly: PxPoint[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const xi = poly[i].x;
    const yi = poly[i].y;
    const xj = poly[j].x;
    const yj = poly[j].y;
    const intersect =
      yi > p.y !== yj > p.y && p.x < ((xj - xi) * (p.y - yi)) / (yj - yi + 1e-12) + xi;
    if (intersect) inside = !inside;
  }
  return inside;
}

export function hitTestHandle(
  localPx: PxPoint,
  t: ClipTransform,
  w: number,
  h: number,
  handlePx = 10,
): HandleKind | null {
  const corners = transformedCorners(t).map((c) => ndcToPx(c, w, h));
  const labels: HandleKind[] = ["scale-sw", "scale-se", "scale-ne", "scale-nw"];
  for (let i = 0; i < 4; i++) {
    if (dist(localPx, corners[i]) <= handlePx) return labels[i];
  }

  const mid = (a: PxPoint, b: PxPoint): PxPoint => ({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });
  const edges: { kind: HandleKind; p: PxPoint }[] = [
    { kind: "scale-s", p: mid(corners[0], corners[1]) },
    { kind: "scale-e", p: mid(corners[1], corners[2]) },
    { kind: "scale-n", p: mid(corners[2], corners[3]) },
    { kind: "scale-w", p: mid(corners[3], corners[0]) },
  ];
  for (const e of edges) {
    if (dist(localPx, e.p) <= handlePx) return e.kind;
  }

  const rot = ndcToPx(rotationHandleNdc(t), w, h);
  if (dist(localPx, rot) <= handlePx + 2) return "rotate";

  if (pointInPolygon(localPx, corners)) return "move";
  return null;
}

export type DragMods = { shift: boolean; alt: boolean };

export type DragOutcome = {
  transform: ClipTransform;
  /** Vertical center guide active (clip centered on canvas x). */
  guideV: boolean;
  /** Horizontal center guide active (clip centered on canvas y). */
  guideH: boolean;
};

/** Unit-square point each scale handle drags (NDC, +Y up). */
const HANDLE_UNIT: Partial<Record<HandleKind, NdcPoint>> = {
  "scale-nw": { x: -1, y: 1 },
  "scale-ne": { x: 1, y: 1 },
  "scale-sw": { x: -1, y: -1 },
  "scale-se": { x: 1, y: -1 },
  "scale-n": { x: 0, y: 1 },
  "scale-s": { x: 0, y: -1 },
  "scale-e": { x: 1, y: 0 },
  "scale-w": { x: -1, y: 0 },
};

const MIN_SCALE = 0.05;
/** Upper bound so a scale drag can't run off to infinity (issue: boundless box). */
const MAX_SCALE = 8;
/** Clip center can't be dragged more than this far off-canvas (NDC). */
const MAX_OFFSET = 2;
const clampScale = (v: number) => Math.max(MIN_SCALE, Math.min(MAX_SCALE, v));
const clampOffset = (v: number) => Math.max(-MAX_OFFSET, Math.min(MAX_OFFSET, v));
/** Move snap threshold in NDC (~1.5% of the frame). */
const MOVE_SNAP_NDC = 0.03;
/** Free rotation soft-snaps to multiples of 90° within this many degrees. */
const ROTATE_SOFT_SNAP_DEG = 3;

function rotatePoint(p: NdcPoint, deg: number): NdcPoint {
  const rad = (deg * Math.PI) / 180;
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  return { x: p.x * c - p.y * s, y: p.x * s + p.y * c };
}

/**
 * Direct-manipulation drag semantics (CapCut/OpenCut conventions):
 * - move: snaps to canvas center on either axis (Alt disables snapping)
 * - rotate: Shift steps 15°; otherwise soft-snaps near 0/±90/180
 * - corner scale: uniform, opposite corner anchored; Shift = free distort;
 *   Alt = scale from center
 * - edge scale: single-axis stretch, opposite edge anchored; Alt = from center
 */
export function applyHandleDrag(
  kind: HandleKind,
  start: ClipTransform,
  startNdc: NdcPoint,
  curNdc: NdcPoint,
  mods: DragMods = { shift: false, alt: false },
): DragOutcome {
  const dx = curNdc.x - startNdc.x;
  const dy = curNdc.y - startNdc.y;

  if (kind === "move") {
    let x = clampOffset(start.x + dx);
    let y = clampOffset(start.y + dy);
    let guideV = false;
    let guideH = false;
    if (!mods.alt) {
      if (Math.abs(x) < MOVE_SNAP_NDC) {
        x = 0;
        guideV = true;
      }
      if (Math.abs(y) < MOVE_SNAP_NDC) {
        y = 0;
        guideH = true;
      }
    }
    return { transform: { ...start, x, y }, guideV, guideH };
  }

  if (kind === "rotate") {
    const a0 = Math.atan2(startNdc.y - start.y, startNdc.x - start.x);
    const a1 = Math.atan2(curNdc.y - start.y, curNdc.x - start.x);
    let deg = start.rotation_deg + ((a1 - a0) * 180) / Math.PI;
    while (deg > 180) deg -= 360;
    while (deg < -180) deg += 360;
    if (mods.shift) {
      deg = Math.round(deg / 15) * 15;
    } else {
      const nearest = Math.round(deg / 90) * 90;
      if (Math.abs(deg - nearest) < ROTATE_SOFT_SNAP_DEG) deg = nearest;
    }
    if (deg === -180) deg = 180;
    return { transform: { ...start, rotation_deg: deg }, guideV: false, guideH: false };
  }

  // —— Scale ——
  const hu = HANDLE_UNIT[kind];
  if (!hu) return { transform: start, guideV: false, guideH: false };
  const au: NdcPoint = { x: -hu.x, y: -hu.y };

  // Local space: rotation removed, origin at the clip center.
  const toLocal = (p: NdcPoint): NdcPoint =>
    rotatePoint({ x: p.x - start.x, y: p.y - start.y }, -start.rotation_deg);
  const localStart = toLocal(startNdc);
  const localCur = toLocal(curNdc);

  // Measure the drag relative to the anchor so the anchored point stays put.
  const anchorLocal: NdcPoint = { x: au.x * start.scale_x, y: au.y * start.scale_y };
  const from = { x: localStart.x - anchorLocal.x, y: localStart.y - anchorLocal.y };
  const to = { x: localCur.x - anchorLocal.x, y: localCur.y - anchorLocal.y };

  let scale_x = start.scale_x;
  let scale_y = start.scale_y;
  const isCorner = hu.x !== 0 && hu.y !== 0;

  if (isCorner) {
    if (mods.shift) {
      // Free distort.
      if (Math.abs(from.x) > 1e-4) scale_x = clampScale(start.scale_x * Math.abs(to.x / from.x));
      if (Math.abs(from.y) > 1e-4) scale_y = clampScale(start.scale_y * Math.abs(to.y / from.y));
    } else {
      // Uniform: preserve aspect using the diagonal distance ratio.
      const u = Math.hypot(to.x, to.y) / Math.max(1e-4, Math.hypot(from.x, from.y));
      scale_x = clampScale(start.scale_x * u);
      scale_y = clampScale(start.scale_y * u);
    }
  } else if (hu.x !== 0) {
    if (Math.abs(from.x) > 1e-4) scale_x = clampScale(start.scale_x * Math.abs(to.x / from.x));
  } else {
    if (Math.abs(from.y) > 1e-4) scale_y = clampScale(start.scale_y * Math.abs(to.y / from.y));
  }

  let center: NdcPoint = { x: start.x, y: start.y };
  if (!mods.alt) {
    // Re-derive the center so the anchor point does not move.
    const anchorWorld = transformNdcPoint(au, start);
    const anchorLocalNew = rotatePoint(
      { x: au.x * scale_x, y: au.y * scale_y },
      start.rotation_deg,
    );
    center = { x: anchorWorld.x - anchorLocalNew.x, y: anchorWorld.y - anchorLocalNew.y };
  }

  return {
    transform: { ...start, scale_x, scale_y, x: center.x, y: center.y },
    guideV: false,
    guideH: false,
  };
}

/** Rotation-aware resize cursor for a handle, given its screen-space direction from the box center. */
export function cursorForHandle(
  kind: HandleKind,
  handlePx: PxPoint,
  centerPx: PxPoint,
): string {
  if (kind === "move") return "move";
  if (kind === "rotate") return "grab";
  const angle = (Math.atan2(handlePx.y - centerPx.y, handlePx.x - centerPx.x) * 180) / Math.PI;
  // Bucket into 8 directions; opposite directions share a cursor.
  const bucket = Math.round(((angle + 360) % 360) / 45) % 4;
  return ["ew-resize", "nwse-resize", "ns-resize", "nesw-resize"][bucket];
}
