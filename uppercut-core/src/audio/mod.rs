//! Audio synthesis (TTS) for Phase 1 voiceover tracks.

mod tts;

pub use tts::{
    is_openai_available, is_piper_available, synthesize_to_wav, TtsError, VoiceoverProvider,
};
