//! Project schema v1 — matches docs/project-schema.md exactly.
//! If you change a type here, update that doc in the same change.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type Id = uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub id: Id,
    pub name: String,
    pub settings: Settings,
    pub media: Vec<MediaItem>,
    pub tracks: Vec<Track>,
}

impl Project {
    pub fn new(name: impl Into<String>, settings: Settings) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            id: Id::new_v4(),
            name: name.into(),
            settings,
            media: Vec::new(),
            tracks: Vec::new(),
        }
    }

    pub fn find_media(&self, id: Id) -> Option<&MediaItem> {
        self.media.iter().find(|m| m.id == id)
    }

    pub fn find_track(&self, id: Id) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn find_track_mut(&mut self, id: Id) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Settings {
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
    /// Music ducking under voice/dialog tracks during export (dB). Default -12; set to 0 to disable.
    #[serde(default = "default_duck_db")]
    pub duck_db: f64,
}

fn default_duck_db() -> f64 {
    -12.0
}

impl Default for Settings {
    /// TikTok/shorts-friendly vertical default — matches the primary Ultra Bruno workflow.
    fn default() -> Self {
        Self {
            fps: 60.0,
            width: 1080,
            height: 1920,
            sample_rate: 48000,
            duck_db: default_duck_db(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Video,
    Audio,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: Id,
    pub path: PathBuf,
    pub kind: MediaKind,
    /// Known only for kinds/formats the prober supports today; see docs/project-schema.md
    /// and uppercut-core::media for current coverage.
    pub duration_secs: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Video,
    Audio,
    Caption,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub kind: TrackKind,
    pub name: String,
    pub clips: Vec<Clip>,
    /// Mix role for audio ducking (Phase 1). Only meaningful on audio tracks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_role: Option<TrackAudioRole>,
    /// Excluded from the audio mix on export/playback. GUI-facing; `apply_command`
    /// itself doesn't gate on it (see project-schema.md v1 note).
    #[serde(default)]
    pub muted: bool,
    /// GUI-honored only: `apply_command` deliberately does not reject edits to a locked
    /// track (CLI/MCP agents may still edit it) — the GUI's timeline interactions are
    /// responsible for refusing mouse edits when this is set.
    #[serde(default)]
    pub locked: bool,
    /// Excluded from composited video layers / burned-in captions on export/playback.
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackAudioRole {
    Voiceover,
    Dialog,
    Music,
    Ambience,
}

impl Track {
    pub fn new(kind: TrackKind, name: impl Into<String>) -> Self {
        Self {
            id: Id::new_v4(),
            kind,
            name: name.into(),
            clips: Vec::new(),
            audio_role: None,
            muted: false,
            locked: false,
            hidden: false,
        }
    }

    pub fn find_clip(&self, id: Id) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id() == id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Clip {
    Video(MediaClip),
    Audio(MediaClip),
    Caption(CaptionClip),
}

impl Clip {
    pub fn id(&self) -> Id {
        match self {
            Clip::Video(c) | Clip::Audio(c) => c.id,
            Clip::Caption(c) => c.id,
        }
    }

    pub fn position_secs(&self) -> f64 {
        match self {
            Clip::Video(c) | Clip::Audio(c) => c.position_secs,
            Clip::Caption(c) => c.position_secs,
        }
    }

    pub fn duration_secs(&self) -> f64 {
        match self {
            Clip::Video(c) | Clip::Audio(c) => c.source_out_secs - c.source_in_secs,
            Clip::Caption(c) => c.duration_secs,
        }
    }

    pub fn end_secs(&self) -> f64 {
        self.position_secs() + self.duration_secs()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaClip {
    pub id: Id,
    pub media_id: Id,
    pub position_secs: f64,
    pub source_in_secs: f64,
    pub source_out_secs: f64,
    pub gain_db: f64,
    pub enabled: bool,
    /// Fade-in duration at the clip start (audio export, Phase 1).
    #[serde(default)]
    pub fade_in_secs: f64,
    /// Fade-out duration at the clip end (audio export, Phase 1).
    #[serde(default)]
    pub fade_out_secs: f64,
}

impl Default for MediaClip {
    fn default() -> Self {
        Self {
            id: Id::new_v4(),
            media_id: Id::new_v4(),
            position_secs: 0.0,
            source_in_secs: 0.0,
            source_out_secs: 0.0,
            gain_db: 0.0,
            enabled: true,
            fade_in_secs: 0.0,
            fade_out_secs: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionClip {
    pub id: Id,
    pub text: String,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub style_id: String,
}
