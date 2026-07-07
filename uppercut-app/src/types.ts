export interface Project {
  schema_version: number;
  id: string;
  name: string;
  settings: {
    fps: number;
    width: number;
    height: number;
    sample_rate: number;
    duck_db: number;
  };
  media: MediaItem[];
  tracks: Track[];
}

export interface MediaItem {
  id: string;
  path: string;
  kind: "video" | "audio" | "image";
  duration_secs: number | null;
}

export interface Track {
  id: string;
  kind: "video" | "audio" | "caption";
  name: string;
  clips: Clip[];
  audio_role?: string;
}

export type Clip =
  | {
      type: "video" | "audio";
      id: string;
      media_id: string;
      position_secs: number;
      source_in_secs: number;
      source_out_secs: number;
      gain_db: number;
      enabled: boolean;
    }
  | {
      type: "caption";
      id: string;
      text: string;
      position_secs: number;
      duration_secs: number;
      style_id: string;
    };

export interface EditorState {
  project: Project | null;
  projectPath: string | null;
  playheadSecs: number;
  selected: { trackId: string; clipId: string } | null;
  isPlaying: boolean;
}

export const CAPTION_STYLES = [
  "tiktok-bold-yellow",
  "tiktok-minimal",
  "tiktok-box",
  "youtube-lower-thirds",
];

export const TRACK_KINDS = ["video", "audio", "caption"] as const;
