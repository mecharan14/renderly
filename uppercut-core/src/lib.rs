//! Headless engine for Uppercut. No UI dependencies — see docs/architecture.md.
//! `project` and `commands` are the contract described in docs/project-schema.md and
//! docs/command-api.md; keep them in sync with those documents.

pub mod audio;
pub mod captions;
pub mod commands;
pub mod compose;
pub mod export;
pub mod mask;
pub mod media;
pub mod packs;
pub mod perceive;
pub mod plugins;
pub mod project;
pub mod segmentation;

pub use audio::{TtsError, VoiceoverProvider};
pub use commands::{apply_command, Command, CommandError, CommandOutcome};
pub use compose::{builtin_effect_ids, BUILTIN_EFFECT_IDS};
pub use export::{
    export_project, export_project_with_progress, mix_timeline_audio_range_to_file,
    mix_timeline_audio_segment, render_frame_at, timeline_duration, DecodeOptions, ExportError,
    ExportPhase, ExportProgress, ExportSettings, FrameRenderer,
};
pub use media::{generate_thumbnail_strip, ReaderOptions, ThumbnailStrip};
pub use packs::{load_pack, LoadedPack};
pub use perceive::{
    audio_peaks, detect_scenes, detect_silence, transcribe_media, AnalysisError, AudioPeaks,
    PerceiveError, SceneCut, SilenceSpan, Transcript, TranscriptSegment,
};
pub use plugins::{compile_gain_wasm, compile_invert_wasm, PluginHost};
pub use project::Project;
