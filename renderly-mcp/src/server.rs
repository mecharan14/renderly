//! MCP tool handlers for Renderly — every edit dispatches through `renderly_core::apply_command`
//! when headless, or through the live Tauri WebSocket bridge (E1/E2) when the app is open
//! on the same project.

use crate::bridge_client::{try_live_bridge, BridgeClient, EditorStatusHeadless};
use renderly_core::{
    apply_command, audio_peaks as analyze_audio_peaks, commands::ExportPreset,
    detect_scenes as analyze_scenes, detect_silence as analyze_silence, export_project, media,
    perceive, project::Project, transcribe_media, Command,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct RenderlyMcp {
    #[expect(dead_code, reason = "tool_handler macro accesses tool_router")]
    tool_router: ToolRouter<Self>,
    session: Arc<Mutex<Option<Session>>>,
}

#[derive(Debug)]
struct Session {
    path: PathBuf,
    project: Project,
}

impl RenderlyMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            session: Arc::new(Mutex::new(None)),
        }
    }

    fn with_session<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut Session) -> Result<R, String>,
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

    fn block_on_bridge<F, T>(f: F) -> Result<T, String>
    where
        F: std::future::Future<Output = Result<T, String>>,
    {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f))
    }

    /// When the desktop app is live on the same project path, run `live`; otherwise `headless`.
    fn with_live_or_headless<R>(
        &self,
        live: impl FnOnce(&mut BridgeClient) -> Result<R, String>,
        headless: impl FnOnce(&mut Session) -> Result<R, String>,
    ) -> Result<R, String> {
        let path = {
            let guard = self
                .session
                .lock()
                .map_err(|e| format!("session lock poisoned: {e}"))?;
            guard.as_ref().map(|s| s.path.clone())
        };
        let bridged = Self::block_on_bridge(async { Ok(try_live_bridge(path.as_deref()).await) })?;
        if let Some((_disc, mut client)) = bridged {
            return live(&mut client);
        }
        self.with_session(headless)
    }

    async fn sync_session_from_bridge(
        session: &mut Session,
        client: &mut BridgeClient,
    ) -> Result<(), String> {
        let value = client.call("get_project", json!({})).await?;
        // Bridge returns `{ project, revision }` (B3); accept a bare Project for older shapes.
        let project_val = value.get("project").cloned().unwrap_or(value);
        session.project = serde_json::from_value(project_val).map_err(|e| e.to_string())?;
        Ok(())
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
    /// A single Command object — same JSON as renderly-cli `apply`.
    pub command: serde_json::Value,
}

/// Parse the `command` tool parameter into a `Command`, accepting both a bare JSON object
/// and a JSON-encoded string. Claude Code's MCP client serialises `serde_json::Value`
/// parameters as a JSON string (the schema carries no explicit `type: object`), while
/// other clients send the object directly — both shapes must work. Callers re-serialise
/// the parsed command before forwarding (e.g. to the live bridge) so downstream always
/// sees a canonical object.
fn parse_command_param(value: &serde_json::Value) -> Result<Command, String> {
    match value {
        serde_json::Value::String(s) => {
            serde_json::from_str(s).map_err(|e| format!("invalid command JSON: {e}"))
        }
        v => serde_json::from_value(v.clone()).map_err(|e| format!("invalid command JSON: {e}")),
    }
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
impl RenderlyMcp {
    #[tool(description = "Create a new empty .renderly.json project file and open it")]
    fn new_project(&self, Parameters(req): Parameters<NewProjectRequest>) -> String {
        self.new_project_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn new_project_impl(&self, req: NewProjectRequest) -> Result<String, String> {
        use renderly_core::project::Settings;
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

    #[tool(description = "Open an existing .renderly.json project file")]
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
        self.with_live_or_headless(
            |client| {
                let value =
                    Self::block_on_bridge(async { client.call("get_project", json!({})).await })?;
                // Prefer bare project JSON for agents; fall back to full snapshot if needed.
                let project = value.get("project").unwrap_or(&value);
                serde_json::to_string_pretty(project).map_err(|e| e.to_string())
            },
            |session| serde_json::to_string_pretty(&session.project).map_err(|e| e.to_string()),
        )
    }

    #[tool(
        name = "apply_command",
        description = "Apply one renderly-core Command and save the project"
    )]
    fn apply_command_tool(&self, Parameters(req): Parameters<ApplyCommandRequest>) -> String {
        self.apply_command_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn apply_command_impl(&self, req: ApplyCommandRequest) -> Result<String, String> {
        let cmd = parse_command_param(&req.command)?;
        // Re-serialise the parsed command so the bridge always receives a JSON object,
        // not the raw (possibly string-encoded) value the MCP client sent us.
        let cmd_value = serde_json::to_value(&cmd).map_err(|e| format!("internal error: {e}"))?;
        self.with_live_or_headless(
            |client| {
                let result = Self::block_on_bridge(async {
                    client
                        .call("apply_command", json!({ "command": cmd_value }))
                        .await
                })?;
                // Keep the MCP-side session mirror fresh for perception tools that stay headless.
                if let Ok(mut guard) = self.session.lock() {
                    if let Some(session) = guard.as_mut() {
                        let _ = Self::block_on_bridge(async {
                            Self::sync_session_from_bridge(session, client).await
                        });
                    }
                }
                Ok(result
                    .get("outcome")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ok")
                    .to_string())
            },
            |session| {
                let outcome =
                    apply_command(&mut session.project, cmd).map_err(|e| e.to_string())?;
                Self::save(session)?;
                Ok(format!("{outcome:?}"))
            },
        )
    }

    #[tool(description = "Render the open project to an MP4 file")]
    fn export(&self, Parameters(req): Parameters<ExportRequest>) -> String {
        self.export_impl(req)
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn export_impl(&self, req: ExportRequest) -> Result<String, String> {
        let preset = parse_preset(&req.preset)?;
        self.with_live_or_headless(
            |client| {
                Self::block_on_bridge(async {
                    client
                        .call(
                            "export",
                            json!({
                                "output_path": req.output_path,
                                "preset": req.preset,
                            }),
                        )
                        .await
                })?;
                Ok(format!("exported to {}", req.output_path))
            },
            |session| {
                export_project(
                    &session.project,
                    PathBuf::from(&req.output_path).as_path(),
                    preset,
                )
                .map_err(|e| e.to_string())?;
                Ok(format!("exported to {}", req.output_path))
            },
        )
    }

    #[tool(
        name = "detect_silence",
        description = "Detect silent spans in a project media item (FFmpeg silencedetect)"
    )]
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

    #[tool(
        name = "detect_scenes",
        description = "Detect scene cuts in a project media item (FFmpeg scene filter)"
    )]
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
        self.with_live_or_headless(
            |client| {
                let value = Self::block_on_bridge(async {
                    client
                        .call(
                            "render_frame",
                            json!({
                                "time_secs": req.time_secs,
                                "preset": req.preset,
                            }),
                        )
                        .await
                })?;
                serde_json::to_string(&value).map_err(|e| e.to_string())
            },
            |session| {
                let png = perceive::render_frame_png(&session.project, req.time_secs, preset)
                    .map_err(|e| e.to_string())?;
                let encoded = base64_encode(&png);
                serde_json::to_string(&serde_json::json!({
                    "time_secs": req.time_secs,
                    "png_base64": encoded,
                    "byte_len": png.len(),
                }))
                .map_err(|e| e.to_string())
            },
        )
    }

    #[tool(
        description = "Whether the Renderly desktop app is live on this project, plus playhead/selection"
    )]
    fn get_editor_status(&self) -> String {
        self.get_editor_status_impl()
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    fn get_editor_status_impl(&self) -> Result<String, String> {
        let session_path = {
            let guard = self
                .session
                .lock()
                .map_err(|e| format!("session lock poisoned: {e}"))?;
            guard.as_ref().map(|s| s.path.clone())
        };
        // Status does not require a path match — report live app even if MCP has no session.
        let status = Self::block_on_bridge(async {
            if let Some((_disc, mut client)) = try_live_bridge(None).await {
                // If MCP has a project open on a different path, still report live but note mismatch.
                let mut value = client.call("get_editor_status", json!({})).await?;
                if let (Some(session_path), Some(obj)) =
                    (session_path.as_ref(), value.as_object_mut())
                {
                    let live_path = obj
                        .get("project_path")
                        .and_then(|v| v.as_str())
                        .map(PathBuf::from);
                    let matched = live_path
                        .as_ref()
                        .map(|p| {
                            p.canonicalize().unwrap_or_else(|_| p.clone())
                                == session_path
                                    .canonicalize()
                                    .unwrap_or_else(|_| session_path.clone())
                        })
                        .unwrap_or(false);
                    obj.insert("path_match".into(), json!(matched));
                }
                Ok(value)
            } else {
                Ok(serde_json::to_value(EditorStatusHeadless {
                    live: false,
                    project_path: session_path.map(|p| p.to_string_lossy().into_owned()),
                    playhead: None,
                    selection: None,
                })
                .unwrap_or(json!({ "live": false })))
            }
        })?;
        serde_json::to_string_pretty(&status).map_err(|e| e.to_string())
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
impl ServerHandler for RenderlyMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Renderly MCP — drive video edits through the command API. \
                 Open or create a project first, then apply_command with JSON commands \
                 (ImportMedia, AddTrack, AddClip, GenerateCaptions, GenerateVoiceover, \
                 SetAudioFade, SetTrackAudioRole, Export, …). \
                 When the Renderly desktop app is running on the same project, edits go \
                 through the live bridge (shared undo + live GUI). Use get_editor_status \
                 to check. Perception: probe_media, get_transcript, render_frame, \
                 detect_silence, detect_scenes, get_audio_peaks.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> String {
        std::env::temp_dir()
            .join(format!(
                "renderly-mcp-test-{tag}-{}.renderly.json",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }

    /// `with_live_or_headless` uses `block_in_place`, which must run on a multi-thread
    /// runtime worker — spawn the sync body onto one. The session points at a temp path,
    /// so even if a real desktop bridge is running on this machine the project-path check
    /// keeps these tests headless.
    fn on_worker(f: impl FnOnce() + Send + 'static) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async { tokio::spawn(async move { f() }).await.unwrap() });
    }

    #[test]
    fn parse_command_param_accepts_object_and_string() {
        let object = serde_json::json!({ "command": "AddTrack", "kind": "video", "name": "V1" });
        let as_object = parse_command_param(&object).expect("bare object parses");
        // Claude Code's MCP client sends Value params as a JSON-encoded STRING — the
        // regression this pins is that both shapes must produce the same Command.
        let as_string = parse_command_param(&serde_json::Value::String(object.to_string()))
            .expect("JSON-encoded string parses");
        assert_eq!(
            serde_json::to_value(&as_object).unwrap(),
            serde_json::to_value(&as_string).unwrap()
        );
    }

    #[test]
    fn parse_command_param_rejects_garbage() {
        assert!(parse_command_param(&serde_json::Value::String("not json".into())).is_err());
        assert!(parse_command_param(&serde_json::json!({ "command": "NoSuchCommand" })).is_err());
    }

    #[test]
    fn new_project_writes_file_and_opens_session() {
        let path = temp_path("new");
        let mcp = RenderlyMcp::new();
        let msg = mcp
            .new_project_impl(NewProjectRequest {
                path: path.clone(),
                name: "test".into(),
                width: 1920,
                height: 1080,
                fps: 30.0,
            })
            .unwrap();
        assert!(msg.contains("created"));
        let data = std::fs::read_to_string(&path).unwrap();
        let project: Project = serde_json::from_str(&data).unwrap();
        assert_eq!(project.name, "test");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn headless_apply_command_persists_to_disk() {
        on_worker(|| {
            let path = temp_path("apply");
            let mcp = RenderlyMcp::new();
            mcp.new_project_impl(NewProjectRequest {
                path: path.clone(),
                name: "test".into(),
                width: 1280,
                height: 720,
                fps: 30.0,
            })
            .unwrap();

            let outcome = mcp
                .apply_command_impl(ApplyCommandRequest {
                    command: serde_json::json!({
                        "command": "AddTrack", "kind": "video", "name": "V1"
                    }),
                })
                .unwrap();
            assert!(!outcome.starts_with("error"));

            // The edit must be saved back to the project file (headless contract).
            let data = std::fs::read_to_string(&path).unwrap();
            let project: Project = serde_json::from_str(&data).unwrap();
            assert_eq!(project.tracks.len(), 1);
            assert_eq!(project.tracks[0].name, "V1");

            let shown = mcp.get_project_impl().unwrap();
            assert!(shown.contains("\"V1\""));
            std::fs::remove_file(&path).ok();
        });
    }

    #[test]
    fn apply_command_rejects_invalid_command_json() {
        on_worker(|| {
            let path = temp_path("invalid");
            let mcp = RenderlyMcp::new();
            mcp.new_project_impl(NewProjectRequest {
                path: path.clone(),
                name: "test".into(),
                width: 1280,
                height: 720,
                fps: 30.0,
            })
            .unwrap();
            let err = mcp
                .apply_command_impl(ApplyCommandRequest {
                    command: serde_json::json!({ "command": "NoSuchCommand" }),
                })
                .unwrap_err();
            assert!(err.contains("invalid command JSON"));
            std::fs::remove_file(&path).ok();
        });
    }
}
