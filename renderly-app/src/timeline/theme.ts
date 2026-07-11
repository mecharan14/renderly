// Reads CSS custom properties from styles/tokens.css once and caches them, so canvas draw
// code (renderer.ts) never hardcodes a color literal — grep-gated in CI/review (see
// docs/architecture.md). Call `getTimelineTheme()` lazily; it self-initializes on first use
// and re-reads if the cache hasn't been built yet (cheap — one getComputedStyle call).

export interface TimelineTheme {
  timelineBg: string;
  rulerBg: string;
  rulerText: string;
  rulerLineStrong: string;
  rulerLineWeak: string;
  trackHeaderBg: string;
  trackLaneBg: string;
  text1: string;
  text2: string;
  text3: string;
  clipVideoBg: string;
  clipAudioBg: string;
  clipAudioWave: string;
  clipCaptionBg: string;
  clipBorderSelected: string;
  playhead: string;
  snapGuide: string;
  accent: string;
  border: string;
  danger: string;
  lockedHatch: string;
}

let cached: TimelineTheme | null = null;

function cssVar(styles: CSSStyleDeclaration, name: string): string {
  return styles.getPropertyValue(name).trim();
}

export function getTimelineTheme(): TimelineTheme {
  if (cached) return cached;
  const styles = getComputedStyle(document.documentElement);
  cached = {
    timelineBg: cssVar(styles, "--timeline-bg"),
    rulerBg: cssVar(styles, "--ruler-bg"),
    rulerText: cssVar(styles, "--ruler-text"),
    rulerLineStrong: "rgba(255,255,255,0.1)",
    rulerLineWeak: "rgba(255,255,255,0.04)",
    trackHeaderBg: cssVar(styles, "--track-header-bg"),
    trackLaneBg: cssVar(styles, "--track-lane-bg"),
    text1: cssVar(styles, "--text-1"),
    text2: cssVar(styles, "--text-2"),
    text3: cssVar(styles, "--text-3"),
    clipVideoBg: cssVar(styles, "--clip-video-bg"),
    clipAudioBg: cssVar(styles, "--clip-audio-bg"),
    clipAudioWave: cssVar(styles, "--clip-audio-wave"),
    clipCaptionBg: cssVar(styles, "--clip-caption-bg"),
    clipBorderSelected: cssVar(styles, "--clip-border-selected"),
    playhead: cssVar(styles, "--playhead"),
    snapGuide: cssVar(styles, "--snap-guide"),
    accent: cssVar(styles, "--accent"),
    border: cssVar(styles, "--border"),
    danger: cssVar(styles, "--danger"),
    lockedHatch: "rgba(0,0,0,0.35)",
  };
  return cached;
}

/// Test-only / hot-reload escape hatch: forces the next `getTimelineTheme()` call to
/// re-read the DOM (tokens.css is not expected to change at runtime otherwise).
export function resetTimelineThemeCache(): void {
  cached = null;
}
