//! Headless engine for Uppercut. No UI dependencies — see docs/architecture.md.
//! `project` and `commands` are the contract described in docs/project-schema.md and
//! docs/command-api.md; keep them in sync with those documents.

pub mod audio;
pub mod captions;
pub mod commands;
pub mod compose;
pub mod export;
pub mod media;
pub mod perceive;
pub mod project;

pub use audio::{TtsError, VoiceoverProvider};
pub use commands::{apply_command, Command, CommandError, CommandOutcome};
pub use export::{
    export_project, mix_timeline_audio_range_to_file, mix_timeline_audio_segment, render_frame_at,
    timeline_duration, DecodeOptions, ExportError, ExportSettings, FrameRenderer,
};
pub use media::ReaderOptions;
pub use perceive::{
    audio_peaks, detect_scenes, detect_silence, transcribe_media, AnalysisError, AudioPeaks,
    PerceiveError, SceneCut, SilenceSpan, Transcript, TranscriptSegment,
};
pub use project::Project;
