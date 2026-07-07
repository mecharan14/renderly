//! MCP tool handlers for Uppercut — every edit dispatches through `uppercut_core::apply_command`.

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uppercut_core::{
    apply_command, audio_peaks as analyze_audio_peaks, commands::ExportPreset,
    detect_scenes as analyze_scenes, detect_silence as analyze_silence, export_project, media,
    perceive, project::Project, transcribe_media, Command,
};

#[derive(Debug)]
pub struct UppercutMcp {
    #[expect(dead_code, reason = "tool_handler macro accesses tool_router")]
    tool_router: ToolRouter<Self>,
    session: Arc<Mutex<Option<Session>>>,
}

#[derive(Debug)]
struct Session {
    path: PathBuf,
    project: Project,
}

impl UppercutMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            session: Arc::new(Mutex::new(None)),
        }
    }

    fn with_session<F>(&self, f: F) -> Result<String, String>
    where
        F: FnOnce(&mut Session) -> Result<String, String>,
    {
        let mut guard = self
            .session
            .lock()
            .map_err(|e| format!("session lock poisoned: {e}"))?;
        let session = guard
            .as_mut()
            .ok_or_else(|| "no project open; call open_project or new_project first".to_string())?;
        f(session)
    }

    fn save(session: &Session) -> Result<(), String> {
        let data = serde_json::to_string_pretty(&session.project)
            .map_err(|e| format!("serialize project: {e}"))?;
        std::fs::write(&session.path, data).map_err(|e| format!("write project: {e}"))
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NewProjectRequest {
    pub path: String,
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_fps")]
    pub fps: f64,
}

fn default_name() -> String {
    "untitled".into()
}
fn default_width() -> u32 {
    1080
}
fn default_height() -> u32 {
    1920
}
fn default_fps() -> f64 {
    60.0
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenProjectRequest {
    pub path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ApplyCommandRequest {
    /// A single Command object — same JSON as uppercut-cli `apply`.
    pub command: serde_json::Value,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExportRequest {
    pub output_path: String,
    #[serde(default = "default_preset")]
    pub preset: String,
}

fn default_preset() -> String {
    "tiktok".into()
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ProbeMediaRequest {
    pub path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TranscriptRequest {
    pub media_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RenderFrameRequest {
    pub time_secs: f64,
    #[serde(default = "default_preset")]
    pub preset: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SilenceDetectRequest {
    pub media_id: String,
    #[serde(default = "default_noise_db")]
    pub noise_db: f64,
    #[serde(default = "default_min_silence")]
    pub min_duration_secs: f64,
}

fn default_noise_db() -> f64 {
    -30.0
}
fn default_min_silence() -> f64 {
    0.5
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SceneDetectRequest {
    pub media_id: String,
    #[serde(default = "default_scene_threshold")]
    pub threshold: f64,
}

fn default_scene_threshold() -> f64 {
    0.4
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AudioPeaksRequest {
    pub media_id: String,
    #[serde(default = "default_buckets")]
    pub buckets: u32,
}

fn default_buckets() -> u32 {
    256
}

#[tool_router]
impl UppercutMcp {
    #[tool(description = "Create a new empty .uppercut.json project file and open it")]
    fn new_project(&self, Parameters(req): Parameters<NewProjectRequest>) -> String {
        self.new_project_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn new_project_impl(&self, req: NewProjectRequest) -> Result<String, String> {
        use uppercut_core::project::Settings;
        let path = PathBuf::from(&req.path);
        let project = Project::new(
            req.name,
            Settings {
                fps: req.fps,
                width: req.width,
                height: req.height,
                sample_rate: 48000,
                duck_db: -12.0,
            },
        );
        let data = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        *self.session.lock().map_err(|e| e.to_string())? = Some(Session { path, project });
        Ok(format!("created and opened {}", req.path))
    }

    #[tool(description = "Open an existing .uppercut.json project file")]
    fn open_project(&self, Parameters(req): Parameters<OpenProjectRequest>) -> String {
        self.open_project_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn open_project_impl(&self, req: OpenProjectRequest) -> Result<String, String> {
        let path = PathBuf::from(&req.path);
        let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let project: Project = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        *self.session.lock().map_err(|e| e.to_string())? = Some(Session { path, project });
        Ok(format!("opened {}", req.path))
    }

    #[tool(description = "Return the current project JSON")]
    fn get_project(&self) -> String {
        self.get_project_impl()
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn get_project_impl(&self) -> Result<String, String> {
        self.with_session(|session| {
            serde_json::to_string_pretty(&session.project).map_err(|e| e.to_string())
        })
    }

    #[tool(description = "Apply one uppercut-core Command and save the project")]
    fn apply_command_tool(&self, Parameters(req): Parameters<ApplyCommandRequest>) -> String {
        self.apply_command_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn apply_command_impl(&self, req: ApplyCommandRequest) -> Result<String, String> {
        let cmd: Command = serde_json::from_value(req.command)
            .map_err(|e| format!("invalid command JSON: {e}"))?;
        self.with_session(|session| {
            let outcome = apply_command(&mut session.project, cmd).map_err(|e| e.to_string())?;
            Self::save(session)?;
            Ok(format!("{outcome:?}"))
        })
    }

    #[tool(description = "Render the open project to an MP4 file")]
    fn export(&self, Parameters(req): Parameters<ExportRequest>) -> String {
        self.export_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn export_impl(&self, req: ExportRequest) -> Result<String, String> {
        let preset = parse_preset(&req.preset)?;
        self.with_session(|session| {
            export_project(
                &session.project,
                PathBuf::from(&req.output_path).as_path(),
                preset,
            )
            .map_err(|e| e.to_string())?;
            Ok(format!("exported to {}", req.output_path))
        })
    }

    #[tool(description = "Detect silent spans in a project media item (FFmpeg silencedetect)")]
    fn detect_silence_tool(&self, Parameters(req): Parameters<SilenceDetectRequest>) -> String {
        self.detect_silence_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn detect_silence_impl(&self, req: SilenceDetectRequest) -> Result<String, String> {
        let media_id: uuid::Uuid = req
            .media_id
            .parse()
            .map_err(|e| format!("invalid media_id UUID: {e}"))?;
        self.with_session(|session| {
            let path = media_path(&session.project, media_id)?;
            let spans = analyze_silence(&path, req.noise_db, req.min_duration_secs)
                .map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&spans).map_err(|e| e.to_string())
        })
    }

    #[tool(description = "Detect scene cuts in a project media item (FFmpeg scene filter)")]
    fn detect_scenes_tool(&self, Parameters(req): Parameters<SceneDetectRequest>) -> String {
        self.detect_scenes_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn detect_scenes_impl(&self, req: SceneDetectRequest) -> Result<String, String> {
        let media_id: uuid::Uuid = req
            .media_id
            .parse()
            .map_err(|e| format!("invalid media_id UUID: {e}"))?;
        self.with_session(|session| {
            let path = media_path(&session.project, media_id)?;
            let cuts = analyze_scenes(&path, req.threshold).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&cuts).map_err(|e| e.to_string())
        })
    }

    #[tool(description = "Downsampled audio peak envelope for waveform / agent perception")]
    fn get_audio_peaks(&self, Parameters(req): Parameters<AudioPeaksRequest>) -> String {
        self.get_audio_peaks_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn get_audio_peaks_impl(&self, req: AudioPeaksRequest) -> Result<String, String> {
        let media_id: uuid::Uuid = req
            .media_id
            .parse()
            .map_err(|e| format!("invalid media_id UUID: {e}"))?;
        self.with_session(|session| {
            let path = media_path(&session.project, media_id)?;
            let peaks = analyze_audio_peaks(&path, req.buckets).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&peaks).map_err(|e| e.to_string())
        })
    }

    #[tool(description = "Probe a media file on disk (duration, dimensions, kind)")]
    fn probe_media(&self, Parameters(req): Parameters<ProbeMediaRequest>) -> String {
        self.probe_media_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn probe_media_impl(&self, req: ProbeMediaRequest) -> Result<String, String> {
        let probed = media::probe(PathBuf::from(&req.path).as_path()).map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&probed).map_err(|e| e.to_string())
    }

    #[tool(description = "Whisper transcript for a media_id in the open project (local STT)")]
    fn get_transcript(&self, Parameters(req): Parameters<TranscriptRequest>) -> String {
        self.get_transcript_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn get_transcript_impl(&self, req: TranscriptRequest) -> Result<String, String> {
        let media_id: uuid::Uuid = req
            .media_id
            .parse()
            .map_err(|e| format!("invalid media_id UUID: {e}"))?;
        self.with_session(|session| {
            let transcript =
                transcribe_media(&session.project, media_id).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&transcript).map_err(|e| e.to_string())
        })
    }

    #[tool(
        description = "Render a PNG preview of the composited frame at time_secs (base64-encoded PNG in JSON)"
    )]
    fn render_frame(&self, Parameters(req): Parameters<RenderFrameRequest>) -> String {
        self.render_frame_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn render_frame_impl(&self, req: RenderFrameRequest) -> Result<String, String> {
        let preset = parse_preset(&req.preset)?;
        self.with_session(|session| {
            let png = perceive::render_frame_png(&session.project, req.time_secs, preset)
                .map_err(|e| e.to_string())?;
            let encoded = base64_encode(&png);
            serde_json::to_string(&serde_json::json!({
                "time_secs": req.time_secs,
                "png_base64": encoded,
                "byte_len": png.len(),
            }))
            .map_err(|e| e.to_string())
        })
    }
}

fn media_path(project: &Project, media_id: uuid::Uuid) -> Result<std::path::PathBuf, String> {
    project
        .find_media(media_id)
        .map(|m| m.path.clone())
        .ok_or_else(|| format!("media not found: {media_id}"))
}

fn parse_preset(name: &str) -> Result<ExportPreset, String> {
    match name {
        "tiktok" => Ok(ExportPreset::TikTok9x16),
        "youtube" => Ok(ExportPreset::Youtube16x9),
        other => Err(format!("unknown preset '{other}', use tiktok or youtube")),
    }
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 63) as usize] as char);
        out.push(TABLE[((triple >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[tool_handler]
impl ServerHandler for UppercutMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Uppercut MCP — drive video edits through the command API. \
                 Open or create a project first, then apply_command with JSON commands \
                 (ImportMedia, AddTrack, AddClip, GenerateCaptions, GenerateVoiceover, \
                 SetAudioFade, SetTrackAudioRole, Export, …). \
                 Perception: probe_media, get_transcript, render_frame, detect_silence, \
                 detect_scenes, get_audio_peaks.",
        )
    }
}
