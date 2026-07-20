//! Client for the live Tauri WebSocket bridge (improvement-plan E2).
//!
//! Discovery: `%LOCALAPPDATA%/renderly/bridge.json` (Windows) or
//! `$XDG_DATA_HOME/renderly/bridge.json` (Unix). Connects only when the discovery file
//! exists and the loopback WebSocket handshake succeeds; otherwise callers stay headless.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[derive(Debug, Clone, Deserialize)]
pub struct BridgeDiscovery {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    #[serde(default)]
    pub project_path: Option<String>,
}

pub fn discovery_path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(|h| PathBuf::from(h).join("AppData").join("Local"))
                    .unwrap_or_else(|| PathBuf::from("."))
            })
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(|h| PathBuf::from(h).join(".local").join("share"))
                    .unwrap_or_else(|| PathBuf::from("."))
            })
    };
    base.join("renderly").join("bridge.json")
}

pub fn read_discovery() -> Option<BridgeDiscovery> {
    let path = discovery_path();
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Best-effort pid liveness. Failure → treat as alive and rely on TCP connect.
fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                text.contains(&pid.to_string())
            }
            Err(_) => true,
        }
    }
    #[cfg(unix)]
    {
        // `kill(pid, 0)` is the portable Unix liveness check. `/proc/{pid}` only exists
        // on Linux — macOS/BSD have no such path, so a procfs probe falsely reports
        // every pid as dead there.
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        let rc = unsafe { kill(pid as i32, 0) };
        if rc == 0 {
            true
        } else {
            // ESRCH → gone. EPERM → exists but not signalable → still alive.
            // Any other errno: treat as alive and let TCP decide. Compare the raw errno:
            // Rust maps ESRCH to ErrorKind::Uncategorized (NotFound is ENOENT), so a
            // kind()-based check would misreport dead processes as alive.
            const ESRCH: i32 = 3; // same value on Linux, macOS, and the BSDs
            std::io::Error::last_os_error().raw_os_error() != Some(ESRCH)
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        true
    }
}

fn paths_match(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// True when discovery names a project path and it matches `session_path`.
pub fn discovery_matches_project(disc: &BridgeDiscovery, session_path: &Path) -> bool {
    match &disc.project_path {
        Some(p) if !p.is_empty() => paths_match(Path::new(p), session_path),
        _ => false,
    }
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct BridgeClient {
    ws: Ws,
    token: String,
    next_id: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    message: String,
}

impl BridgeClient {
    pub async fn connect(disc: &BridgeDiscovery) -> Result<Self, String> {
        let url = format!("ws://127.0.0.1:{}/", disc.port);
        let (ws, _) = tokio::time::timeout(Duration::from_secs(2), connect_async(&url))
            .await
            .map_err(|_| "bridge connect timed out".to_string())?
            .map_err(|e| format!("bridge connect: {e}"))?;
        Ok(Self {
            ws,
            token: disc.token.clone(),
            next_id: 1,
        })
    }

    pub async fn call(&mut self, method: &str, mut params: Value) -> Result<Value, String> {
        if !params.is_object() {
            params = json!({});
        }
        if let Some(obj) = params.as_object_mut() {
            obj.insert("token".into(), json!(self.token));
        }
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.ws
            .send(Message::Text(req.to_string().into()))
            .await
            .map_err(|e| format!("bridge send: {e}"))?;

        let deadline = Duration::from_secs(120);
        let started = std::time::Instant::now();
        while started.elapsed() < deadline {
            let msg = tokio::time::timeout(Duration::from_secs(30), self.ws.next())
                .await
                .map_err(|_| "bridge recv timed out".to_string())?
                .ok_or_else(|| "bridge closed".to_string())?
                .map_err(|e| format!("bridge recv: {e}"))?;
            let Message::Text(text) = msg else {
                continue;
            };
            let resp: JsonRpcResponse =
                serde_json::from_str(&text).map_err(|e| format!("bridge parse: {e}"))?;
            if let Some(err) = resp.error {
                return Err(err.message);
            }
            return resp
                .result
                .ok_or_else(|| "bridge response missing result".to_string());
        }
        Err("bridge call timed out".into())
    }
}

/// Try live bridge; `None` means stay headless.
pub async fn try_live_bridge(
    session_path: Option<&Path>,
) -> Option<(BridgeDiscovery, BridgeClient)> {
    let disc = read_discovery()?;
    if !pid_alive(disc.pid) {
        return None;
    }
    if let Some(path) = session_path {
        if !discovery_matches_project(&disc, path) {
            return None;
        }
    }
    match BridgeClient::connect(&disc).await {
        Ok(client) => Some((disc, client)),
        Err(_) => None,
    }
}

#[derive(Debug, Serialize)]
pub struct EditorStatusHeadless {
    pub live: bool,
    pub project_path: Option<String>,
    pub playhead: Option<f64>,
    pub selection: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_parses_with_and_without_project_path() {
        let full: BridgeDiscovery = serde_json::from_str(
            r#"{"pid":123,"port":4567,"token":"abc","project_path":"C:/p.renderly.json"}"#,
        )
        .unwrap();
        assert_eq!(full.pid, 123);
        assert_eq!(full.port, 4567);
        assert_eq!(full.project_path.as_deref(), Some("C:/p.renderly.json"));

        let bare: BridgeDiscovery =
            serde_json::from_str(r#"{"pid":1,"port":2,"token":"t"}"#).unwrap();
        assert!(bare.project_path.is_none());
    }

    #[test]
    fn discovery_matches_project_requires_same_canonical_path() {
        let dir = std::env::temp_dir();
        let file = dir.join(format!(
            "renderly-mcp-disc-{}.renderly.json",
            std::process::id()
        ));
        std::fs::write(&file, "{}").unwrap();

        let matching = BridgeDiscovery {
            pid: 1,
            port: 1,
            token: String::new(),
            project_path: Some(file.to_string_lossy().into_owned()),
        };
        assert!(discovery_matches_project(&matching, &file));

        let other = BridgeDiscovery {
            pid: 1,
            port: 1,
            token: String::new(),
            project_path: Some(
                dir.join("other.renderly.json")
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        assert!(!discovery_matches_project(&other, &file));

        let none = BridgeDiscovery {
            pid: 1,
            port: 1,
            token: String::new(),
            project_path: None,
        };
        assert!(!discovery_matches_project(&none, &file));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn pid_liveness_sanity() {
        assert!(!pid_alive(0));
        // Our own pid is definitionally alive.
        assert!(pid_alive(std::process::id()));
    }
}
