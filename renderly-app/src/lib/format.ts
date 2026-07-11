export function formatTimecode(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  const f = Math.floor((secs % 1) * 100);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(f).padStart(2, "0")}`;
}

/** Parse `MM:SS.ff`, `M:SS`, or raw seconds into timeline seconds. */
export function parseTimecode(text: string): number | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  if (/^\d+(\.\d+)?$/.test(trimmed)) {
    const n = Number(trimmed);
    return Number.isFinite(n) && n >= 0 ? n : null;
  }
  const m = trimmed.match(/^(\d+):(\d{1,2})(?:\.(\d{1,2}))?$/);
  if (!m) return null;
  const mins = Number(m[1]);
  const secs = Number(m[2]);
  const frac = m[3] ? Number(m[3].padEnd(2, "0")) / 100 : 0;
  if (!Number.isFinite(mins) || !Number.isFinite(secs) || secs >= 60) return null;
  return mins * 60 + secs + frac;
}

export function fileName(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}
