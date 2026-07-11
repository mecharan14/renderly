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

export function applyHandleDrag(
  kind: HandleKind,
  start: ClipTransform,
  startNdc: NdcPoint,
  curNdc: NdcPoint,
): ClipTransform {
  const dx = curNdc.x - startNdc.x;
  const dy = curNdc.y - startNdc.y;

  if (kind === "move") {
    return { ...start, x: start.x + dx, y: start.y + dy };
  }

  if (kind === "rotate") {
    const a0 = Math.atan2(startNdc.y - start.y, startNdc.x - start.x);
    const a1 = Math.atan2(curNdc.y - start.y, curNdc.x - start.x);
    let deg = start.rotation_deg + ((a1 - a0) * 180) / Math.PI;
    // Normalize loosely
    while (deg > 180) deg -= 360;
    while (deg < -180) deg += 360;
    return { ...start, rotation_deg: deg };
  }

  // Scale: distance from center in start vs current, along local axes after inverse rotation.
  const invRad = (-start.rotation_deg * Math.PI) / 180;
  const c = Math.cos(invRad);
  const s = Math.sin(invRad);
  const toLocal = (p: NdcPoint): NdcPoint => {
    const ox = p.x - start.x;
    const oy = p.y - start.y;
    return { x: ox * c - oy * s, y: ox * s + oy * c };
  };
  const localStart = toLocal(startNdc);
  const localCur = toLocal(curNdc);

  let scale_x = start.scale_x;
  let scale_y = start.scale_y;
  const minScale = 0.05;

  const sx = (axis: "x" | "y", from: number, to: number) => {
    if (Math.abs(from) < 1e-4) return axis === "x" ? scale_x : scale_y;
    const ratio = to / from;
    return Math.max(minScale, (axis === "x" ? start.scale_x : start.scale_y) * ratio);
  };

  switch (kind) {
    case "scale-e":
    case "scale-w":
      scale_x = sx("x", localStart.x, localCur.x);
      break;
    case "scale-n":
    case "scale-s":
      scale_y = sx("y", localStart.y, localCur.y);
      break;
    case "scale-ne":
    case "scale-nw":
    case "scale-se":
    case "scale-sw": {
      scale_x = sx("x", localStart.x, localCur.x);
      scale_y = sx("y", localStart.y, localCur.y);
      // Uniform-ish: average for nicer corner feel
      const u = (scale_x / start.scale_x + scale_y / start.scale_y) / 2;
      scale_x = Math.max(minScale, start.scale_x * u);
      scale_y = Math.max(minScale, start.scale_y * u);
      break;
    }
    default:
      break;
  }

  return { ...start, scale_x, scale_y };
}
