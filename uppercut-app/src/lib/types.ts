// TS mirror of uppercut-core's project schema v1 (docs/project-schema.md). Keep in sync.

export interface Project {
  schema_version: number;
  id: string;
  name: string;
  settings: ProjectSettings;
  media: MediaItem[];
  tracks: Track[];
}

export interface ProjectSettings {
  fps: number;
  width: number;
  height: number;
  sample_rate: number;
  duck_db: number;
}

export type MediaKind = "video" | "audio" | "image";

export interface MediaItem {
  id: string;
  path: string;
  kind: MediaKind;
  duration_secs: number | null;
  width: number | null;
  height: number | null;
  fps: number | null;
}

export type TrackKind = "video" | "audio" | "caption";
export type TrackAudioRole = "voiceover" | "dialog" | "music" | "ambience";

export interface Track {
  id: string;
  kind: TrackKind;
  name: string;
  clips: Clip[];
  audio_role?: TrackAudioRole | null;
  muted: boolean;
  locked: boolean;
  hidden: boolean;
}

export interface MediaClip {
  type: "video" | "audio";
  id: string;
  media_id: string;
  position_secs: number;
  source_in_secs: number;
  source_out_secs: number;
  gain_db: number;
  enabled: boolean;
  fade_in_secs: number;
  fade_out_secs: number;
}

export interface CaptionClip {
  type: "caption";
  id: string;
  text: string;
  position_secs: number;
  duration_secs: number;
  style_id: string;
}

export type Clip = MediaClip | CaptionClip;

export function clipId(clip: Clip): string {
  return clip.id;
}

export function clipPositionSecs(clip: Clip): number {
  return clip.position_secs;
}

export function clipDurationSecs(clip: Clip): number {
  return clip.type === "caption" ? clip.duration_secs : clip.source_out_secs - clip.source_in_secs;
}

export function clipEndSecs(clip: Clip): number {
  return clipPositionSecs(clip) + clipDurationSecs(clip);
}

export const CAPTION_STYLES = [
  "tiktok-bold-yellow",
  "tiktok-minimal",
  "tiktok-box",
  "youtube-lower-thirds",
] as const;

export const TRACK_KINDS = ["video", "audio", "caption"] as const;

export interface Selection {
  trackId: string;
  clipId: string;
}
