//! Zero-Latency Local LAN Mobile Remote Companion v2.0 (`solus remote`).
//! Serves an embedded, dependency-free mobile touch client over local Wi-Fi,
//! provides persistent WebSocket bidirectional event delivery (< 1ms latency),
//! renders terminal ASCII QR codes, and manages multi-track subtitle and episode switching.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UnixStream};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// Individual subtitle track representation for the mobile companion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteSubtitleTrack {
    pub id: u32,
    pub code: String,
    pub label: String,
    pub is_active: bool,
}

/// Individual episode entry for the mobile episode switcher.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteEpisodeItem {
    pub ep: u32,
    pub title: String,
    pub is_watched: bool,
    pub is_current: bool,
}

/// Shared live playback state visible to the mobile companion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePlaybackState {
    pub is_playing: bool,
    pub title: String,
    pub episode: u32,
    pub episode_title: String,
    pub poster_url: Option<String>,
    pub time_pos: f64,
    pub duration: f64,
    pub volume: i32,
    pub speed: f64,
    pub audio_mode: String,
    pub sub_active: bool,
    pub sub_lang: String,
    pub sub_delay: f64,
    pub sub_preset: String,
    pub subtitles: Vec<RemoteSubtitleTrack>,
    pub episodes: Vec<RemoteEpisodeItem>,
    pub anime4k: String,
}

impl Default for RemotePlaybackState {
    fn default() -> Self {
        Self {
            is_playing: false,
            title: "No Active Stream".to_string(),
            episode: 1,
            episode_title: "Idle".to_string(),
            poster_url: None,
            time_pos: 0.0,
            duration: 0.0,
            volume: 80,
            speed: 1.0,
            audio_mode: "sub".to_string(),
            sub_active: true,
            sub_lang: "eng".to_string(),
            sub_delay: 0.0,
            sub_preset: "Studio Minimal".to_string(),
            subtitles: vec![
                RemoteSubtitleTrack {
                    id: 1,
                    code: "eng".to_string(),
                    label: "English (Official)".to_string(),
                    is_active: true,
                },
                RemoteSubtitleTrack {
                    id: 2,
                    code: "ara".to_string(),
                    label: "Arabic / العربية".to_string(),
                    is_active: false,
                },
                RemoteSubtitleTrack {
                    id: 3,
                    code: "spa".to_string(),
                    label: "Spanish / Español".to_string(),
                    is_active: false,
                },
            ],
            episodes: Vec::new(),
            anime4k: "off".to_string(),
        }
    }
}

/// Action payload sent from the mobile touch companion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteActionRequest {
    pub action: String,
    pub delta: Option<f64>,
    pub pos: Option<f64>,
    pub value: Option<i32>,
    pub sub_id: Option<u32>,
    pub episode: Option<u32>,
    pub speed: Option<f64>,
    pub preset: Option<String>,
    pub mode: Option<String>,
}

/// Resolves the host machine's primary local LAN IP address.
pub fn get_local_ip() -> String {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0");
    if let Ok(s) = socket {
        if s.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = s.local_addr() {
                let ip = addr.ip().to_string();
                if ip != "0.0.0.0" && ip != "127.0.0.1" {
                    return ip;
                }
            }
        }
    }
    "127.0.0.1".to_string()
}

/// Generates a compact terminal-renderable ASCII QR code using Unicode Dense1x2 blocks.
pub fn generate_terminal_qr(url: &str) -> Option<String> {
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    let qr_str = code
        .render::<qrcode::render::unicode::Dense1x2>()
        .dark_color(qrcode::render::unicode::Dense1x2::Dark)
        .light_color(qrcode::render::unicode::Dense1x2::Light)
        .build();
    Some(qr_str)
}

/// Locates the active mpv IPC Unix domain socket, checking preferred paths and active /tmp sockets.
pub async fn resolve_active_socket(preferred: &str) -> Option<String> {
    // 1. Direct path check
    if std::path::Path::new(preferred).exists() {
        return Some(preferred.to_string());
    }

    // 2. Canonical symlink check
    if std::path::Path::new("/tmp/solus-mpv.sock").exists() {
        return Some("/tmp/solus-mpv.sock".to_string());
    }

    // 3. Scan /tmp for any active solus-mpv-*.sock files (newest by modified time)
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        let mut socks: Vec<(std::time::SystemTime, String)> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("solus-mpv") && name.ends_with(".sock")
            })
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                Some((modified, e.path().to_string_lossy().to_string()))
            })
            .collect();

        socks.sort_by_key(|a| std::cmp::Reverse(a.0));
        if let Some((_, path)) = socks.first() {
            return Some(path.clone());
        }
    }

    None
}

/// Persistent connection cache for mpv Unix socket IPC.
static CACHED_IPC: Mutex<Option<(String, UnixStream)>> = Mutex::const_new(None);

/// Global broadcast channel for streaming state changes to all connected WebSockets.
static BROADCAST_CHANNEL: std::sync::OnceLock<broadcast::Sender<String>> = std::sync::OnceLock::new();

/// Access the broadcast sender for streaming playback state updates.
pub fn get_broadcast_sender() -> &'static broadcast::Sender<String> {
    BROADCAST_CHANNEL.get_or_init(|| {
        let (tx, _) = broadcast::channel(64);
        tx
    })
}

/// Broadcasts current state to all connected WebSocket clients.
pub async fn broadcast_state(state: &Arc<RwLock<RemotePlaybackState>>) {
    let st = state.read().await;
    if let Ok(json) = serde_json::to_string(&*st) {
        let _ = get_broadcast_sender().send(json);
    }
}

/// Dispatches an mpv JSON-RPC command over the persistent Unix domain socket connection.
pub async fn send_mpv_json_command(preferred_sock: &str, cmd_value: serde_json::Value) -> bool {
    let target_sock = match resolve_active_socket(preferred_sock).await {
        Some(s) => s,
        None => return false,
    };

    let payload = format!("{}\n", cmd_value);
    let mut guard = CACHED_IPC.lock().await;

    // Try reusing existing open stream
    if let Some((ref cached_path, ref mut stream)) = *guard {
        if cached_path == &target_sock
            && stream.write_all(payload.as_bytes()).await.is_ok()
            && stream.flush().await.is_ok()
        {
            return true;
        }
    }

    // Connect fresh stream
    if let Ok(mut stream) = UnixStream::connect(&target_sock).await {
        let ok = stream.write_all(payload.as_bytes()).await.is_ok() && stream.flush().await.is_ok();
        *guard = Some((target_sock, stream));
        return ok;
    }

    *guard = None;
    false
}

/// Dispatches an mpv JSON-RPC string array command.
pub async fn send_mpv_command(sock_path: &str, command: &[&str]) -> bool {
    let cmd_json = serde_json::json!({
        "command": command
    });
    send_mpv_json_command(sock_path, cmd_json).await
}

/// Spawns the background HTTP & WebSocket server for the mobile remote companion.
pub fn spawn_remote_server(
    port: u16,
    state: Arc<RwLock<RemotePlaybackState>>,
    sock_path: String,
    shutdown: Arc<AtomicBool>,
    tx_app: Option<tokio::sync::mpsc::Sender<crate::AppMessage>>,
) {
    tokio::spawn(async move {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind Solus remote server on {}: {}", addr, e);
                return;
            }
        };

        while !shutdown.load(Ordering::Relaxed) {
            let (socket, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    continue;
                }
            };

            let state_clone = Arc::clone(&state);
            let sock_path_clone = sock_path.clone();
            let tx_app_clone = tx_app.clone();

            tokio::spawn(async move {
                // Peek bytes to determine whether this is an HTTP request or a WebSocket upgrade
                let mut peek_buf = [0u8; 1024];
                let n = socket.peek(&mut peek_buf).await.unwrap_or(0);
                let peek_str = String::from_utf8_lossy(&peek_buf[..n]);

                if peek_str.contains("Upgrade: websocket") || peek_str.contains("upgrade: websocket") {
                    if let Ok(ws) = tokio_tungstenite::accept_async(socket).await {
                        handle_ws_client(ws, state_clone, sock_path_clone, tx_app_clone).await;
                    }
                } else {
                    handle_http_client(socket, state_clone, sock_path_clone, tx_app_clone).await;
                }
            });
        }
    });
}

/// Manages a live, persistent WebSocket client session.
async fn handle_ws_client(
    ws: WebSocketStream<TcpStream>,
    state: Arc<RwLock<RemotePlaybackState>>,
    sock_path: String,
    tx_app: Option<tokio::sync::mpsc::Sender<crate::AppMessage>>,
) {
    let (mut ws_sender, mut ws_receiver) = ws.split();
    let mut broadcast_rx = get_broadcast_sender().subscribe();

    // 1. Push immediate full state to newly connected phone
    {
        let st = state.read().await;
        if let Ok(json) = serde_json::to_string(&*st) {
            let _ = ws_sender.send(Message::Text(json.into())).await;
        }
    }

    // 2. Event loop: bidirectional sub-millisecond dispatch
    loop {
        tokio::select! {
            // Incoming action from mobile touch UI
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let text_str = text.as_str();
                        if let Ok(req) = serde_json::from_str::<RemoteActionRequest>(text_str) {
                            handle_remote_action(&req, &sock_path, &state, tx_app.as_ref()).await;
                            broadcast_state(&state).await;
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = ws_sender.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            // Outgoing playback progress / metadata update from Solus
            Ok(json_payload) = broadcast_rx.recv() => {
                if ws_sender.send(Message::Text(json_payload.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Fallback HTTP handler for initial page load, REST health checks, and non-WebSocket clients.
async fn handle_http_client(
    mut socket: TcpStream,
    state: Arc<RwLock<RemotePlaybackState>>,
    sock_path: String,
    tx_app: Option<tokio::sync::mpsc::Sender<crate::AppMessage>>,
) {
    let mut buffer = [0u8; 4096];
    let n = match socket.read(&mut buffer).await {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let request = String::from_utf8_lossy(&buffer[..n]);
    let mut lines = request.lines();
    let request_line = match lines.next() {
        Some(l) => l,
        None => return,
    };

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }

    let method = parts[0];
    let path = parts[1];

    if method == "GET" && (path == "/" || path == "/index.html") {
        let body = REMOTE_HTML;
        let resp = format!(
            "HTTP/1.1 200 OK\r\n\
            Content-Type: text/html; charset=utf-8\r\n\
            Content-Length: {}\r\n\
            Connection: close\r\n\
            \r\n{}",
            body.len(),
            body
        );
        let _ = socket.write_all(resp.as_bytes()).await;
    } else if method == "GET" && path == "/api/status" {
        let st = state.read().await;
        let json_bytes = serde_json::to_vec(&*st).unwrap_or_default();
        let resp = format!(
            "HTTP/1.1 200 OK\r\n\
            Content-Type: application/json\r\n\
            Access-Control-Allow-Origin: *\r\n\
            Content-Length: {}\r\n\
            Connection: close\r\n\
            \r\n{}",
            json_bytes.len(),
            String::from_utf8_lossy(&json_bytes)
        );
        let _ = socket.write_all(resp.as_bytes()).await;
    } else if method == "POST" && path == "/api/action" {
        let body = if let Some(idx) = request.find("\r\n\r\n") {
            &request[idx + 4..]
        } else {
            ""
        };

        if let Ok(req) = serde_json::from_str::<RemoteActionRequest>(body) {
            handle_remote_action(&req, &sock_path, &state, tx_app.as_ref()).await;
            broadcast_state(&state).await;
        }

        let resp = "HTTP/1.1 200 OK\r\n\
            Content-Type: application/json\r\n\
            Access-Control-Allow-Origin: *\r\n\
            Content-Length: 15\r\n\
            Connection: close\r\n\
            \r\n{\"status\":\"ok\"}";
        let _ = socket.write_all(resp.as_bytes()).await;
    } else if method == "OPTIONS" {
        let resp = "HTTP/1.1 204 No Content\r\n\
            Access-Control-Allow-Origin: *\r\n\
            Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
            Access-Control-Allow-Headers: Content-Type\r\n\
            Connection: close\r\n\r\n";
        let _ = socket.write_all(resp.as_bytes()).await;
    } else {
        let resp = "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n";
        let _ = socket.write_all(resp.as_bytes()).await;
    }
}

/// Executes touch remote commands against mpv IPC with optimistic local state sync and app message relay.
async fn handle_remote_action(
    req: &RemoteActionRequest,
    sock_path: &str,
    state: &Arc<RwLock<RemotePlaybackState>>,
    tx_app: Option<&tokio::sync::mpsc::Sender<crate::AppMessage>>,
) {
    match req.action.as_str() {
        "play_pause" => {
            let cmd = serde_json::json!({ "command": ["cycle", "pause"] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.is_playing = !st.is_playing;
        }
        "seek" => {
            let delta = req.delta.unwrap_or(10.0);
            let cmd = serde_json::json!({ "command": ["seek", delta, "relative"] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.time_pos = (st.time_pos + delta).max(0.0);
        }
        "set_pos" => {
            if let Some(pos) = req.pos {
                let cmd = serde_json::json!({ "command": ["set_property", "time-pos", pos] });
                let _ = send_mpv_json_command(sock_path, cmd).await;
                let mut st = state.write().await;
                st.time_pos = pos;
            }
        }
        "volume" => {
            let delta = req.delta.unwrap_or(5.0);
            let cmd = serde_json::json!({ "command": ["add", "volume", delta] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.volume = (st.volume + delta as i32).clamp(0, 100);
        }
        "set_volume" => {
            if let Some(val) = req.value {
                let clamped = val.clamp(0, 100);
                let cmd = serde_json::json!({ "command": ["set_property", "volume", clamped] });
                let _ = send_mpv_json_command(sock_path, cmd).await;
                let mut st = state.write().await;
                st.volume = clamped;
            }
        }
        "set_sub" => {
            if let Some(sub_id) = req.sub_id {
                let cmd = serde_json::json!({ "command": ["set_property", "sid", sub_id] });
                let _ = send_mpv_json_command(sock_path, cmd).await;
                let mut st = state.write().await;
                let mut active_label = None;
                for s in &mut st.subtitles {
                    s.is_active = s.id == sub_id;
                    if s.is_active {
                        active_label = Some(s.label.clone());
                    }
                }
                if let Some(lbl) = active_label {
                    st.sub_lang = lbl;
                }
                st.sub_active = true;
            }
        }
        "toggle_sub_visibility" => {
            let cmd = serde_json::json!({ "command": ["cycle", "sub-visibility"] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.sub_active = !st.sub_active;
        }
        "set_sub_delay" => {
            let delta = req.delta.unwrap_or(0.25);
            let cmd = serde_json::json!({ "command": ["add", "sub-delay", delta] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.sub_delay += delta;
        }
        "reset_sub_delay" => {
            let cmd = serde_json::json!({ "command": ["set_property", "sub-delay", 0.0] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.sub_delay = 0.0;
        }
        "set_sub_preset" => {
            if let Some(ref preset) = req.preset {
                let mut st = state.write().await;
                st.sub_preset = preset.clone();
            }
        }
        "select_episode" => {
            if let Some(ep) = req.episode {
                if let Some(tx) = tx_app {
                    let _ = tx.send(crate::AppMessage::RemotePlayEpisode(ep)).await;
                }
                let mut st = state.write().await;
                st.episode = ep;
                st.is_playing = true;
                for item in &mut st.episodes {
                    item.is_current = item.ep == ep;
                }
            }
        }
        "toggle_audio" => {
            let cmd = serde_json::json!({ "command": ["cycle", "audio"] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.audio_mode = if st.audio_mode == "sub" { "dub".to_string() } else { "sub".to_string() };
        }
        "set_speed" => {
            if let Some(speed) = req.speed {
                let cmd = serde_json::json!({ "command": ["set_property", "speed", speed] });
                let _ = send_mpv_json_command(sock_path, cmd).await;
                let mut st = state.write().await;
                st.speed = speed;
            }
        }
        "skip_intro" => {
            let cmd = serde_json::json!({ "command": ["seek", 85.0, "relative"] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.time_pos = (st.time_pos + 85.0).max(0.0);
        }
        "toggle_fullscreen" => {
            let cmd = serde_json::json!({ "command": ["cycle", "fullscreen"] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
        }
        _ => {}
    }
}

/// Single-file zero-dependency responsive OLED mobile touch client v2.0.
const REMOTE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no, viewport-fit=cover">
  <title>Solus Companion</title>
  <style>
    :root {
      --bg: #07080c;
      --surface: #0f1118;
      --surface-elevated: #161a25;
      --border: #1a1e2c;
      --border-focus: #2d344d;
      --primary: #e63946;
      --primary-glow: rgba(230, 57, 70, 0.4);
      --accent: #50fa7b;
      --accent-muted: #142a1e;
      --text: #f0f3fc;
      --text-muted: #7d849b;
      --radius-sm: 10px;
      --radius-md: 16px;
      --radius-lg: 22px;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; -webkit-tap-highlight-color: transparent; }
    body {
      background: var(--bg);
      color: var(--text);
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      min-height: 100vh;
      min-height: 100dvh;
      display: flex;
      flex-direction: column;
      align-items: center;
      padding: 14px 16px 88px;
      user-select: none;
      overflow-x: hidden;
    }
    .header {
      width: 100%;
      max-width: 440px;
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding-bottom: 12px;
      border-bottom: 1px solid var(--border);
    }
    .brand { font-size: 1.15rem; font-weight: 800; letter-spacing: 1px; color: var(--primary); }
    .header-actions { display: flex; align-items: center; gap: 8px; }
    .status-badge {
      font-size: 0.72rem;
      background: var(--accent-muted);
      color: var(--accent);
      padding: 4px 10px;
      border-radius: 12px;
      display: flex;
      align-items: center;
      gap: 6px;
      font-weight: 700;
      transition: all 0.2s ease;
    }
    .status-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); }
    .status-badge.standby { background: #2a1a14; color: #ffb86c; }
    .status-badge.standby .status-dot { background: #ffb86c; }
    .btn-fs {
      background: var(--surface);
      border: 1px solid var(--border);
      color: var(--text);
      font-size: 0.75rem;
      font-weight: 700;
      padding: 4px 10px;
      border-radius: 12px;
      cursor: pointer;
    }

    /* Main Deck Card */
    .card {
      width: 100%;
      max-width: 440px;
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: var(--radius-lg);
      padding: 20px 18px;
      margin-top: 16px;
      box-shadow: 0 12px 36px rgba(0,0,0,0.55);
    }
    .hero-meta {
      display: flex;
      gap: 14px;
      align-items: center;
      margin-bottom: 16px;
    }
    .poster-box {
      width: 58px;
      height: 82px;
      background: #141724;
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      overflow: hidden;
      flex-shrink: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 1.5rem;
      color: var(--primary);
    }
    .poster-box img { width: 100%; height: 100%; object-fit: cover; }
    .hero-details { flex: 1; overflow: hidden; }
    .title { font-size: 1.22rem; font-weight: 800; color: #fff; margin-bottom: 3px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .subtitle { font-size: 0.88rem; color: var(--text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; margin-bottom: 4px; }
    .badges-row { display: flex; gap: 6px; font-size: 0.72rem; font-weight: 700; }
    .tag-badge { background: #1a1f2e; color: #9aa2ba; padding: 2px 8px; border-radius: 6px; }

    /* Scrubber */
    .timeline { width: 100%; margin: 12px 0 6px; }
    .slider {
      width: 100%;
      height: 8px;
      border-radius: 4px;
      background: #252a3a;
      outline: none;
      -webkit-appearance: none;
      cursor: pointer;
    }
    .slider::-webkit-slider-thumb {
      -webkit-appearance: none;
      width: 22px;
      height: 22px;
      border-radius: 50%;
      background: var(--primary);
      cursor: pointer;
      box-shadow: 0 0 12px var(--primary-glow);
    }
    .time-labels {
      display: flex;
      justify-content: space-between;
      font-size: 0.82rem;
      color: #8b92a8;
      margin-top: 8px;
      font-variant-numeric: tabular-nums;
      font-weight: 700;
    }

    /* Primary Transport Buttons */
    .controls {
      display: flex;
      justify-content: center;
      align-items: center;
      gap: 16px;
      margin: 20px 0 12px;
    }
    .btn {
      background: var(--surface-elevated);
      border: 1px solid var(--border);
      color: var(--text);
      border-radius: var(--radius-md);
      cursor: pointer;
      display: flex;
      justify-content: center;
      align-items: center;
      font-weight: 700;
      transition: background 0.12s, transform 0.08s;
      outline: none;
    }
    .btn:active { transform: scale(0.92); background: #262c3f; }
    .btn-seek { width: 68px; height: 54px; font-size: 1rem; }
    .btn-play {
      width: 88px;
      height: 64px;
      background: var(--primary);
      border: none;
      font-size: 1.5rem;
      border-radius: var(--radius-md);
      box-shadow: 0 6px 20px var(--primary-glow);
    }
    .btn-play:active { background: #c52b37; }

    /* Quick Action Row */
    .quick-actions {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 10px;
      margin-top: 14px;
    }
    .btn-action { height: 44px; font-size: 0.85rem; color: #cfd4e2; }

    /* Volume Row */
    .volume-box {
      margin-top: 20px;
      padding-top: 14px;
      border-top: 1px solid var(--border);
      display: flex;
      align-items: center;
      gap: 12px;
    }
    .vol-icon { font-size: 1.25rem; color: var(--text-muted); cursor: pointer; padding: 4px; }
    .vol-val { font-size: 0.82rem; font-weight: 700; color: #71778c; width: 42px; text-align: right; }

    /* Fixed Bottom Navigation Deck */
    .bottom-nav {
      position: fixed;
      bottom: 0;
      left: 0;
      right: 0;
      background: rgba(11, 13, 20, 0.94);
      backdrop-filter: blur(16px);
      -webkit-backdrop-filter: blur(16px);
      border-top: 1px solid var(--border);
      padding: 10px 16px 14px;
      display: flex;
      justify-content: center;
      gap: 10px;
      z-index: 100;
    }
    .nav-btn {
      flex: 1;
      max-width: 140px;
      height: 48px;
      background: var(--surface);
      border: 1px solid var(--border);
      color: var(--text);
      font-size: 0.82rem;
      font-weight: 700;
      border-radius: var(--radius-sm);
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      gap: 3px;
      cursor: pointer;
    }
    .nav-btn:active { background: var(--surface-elevated); transform: scale(0.95); }
    .nav-icon { font-size: 0.95rem; color: var(--primary); }

    /* Slide-Up Drawers (Modals) */
    .drawer-backdrop {
      position: fixed;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
      background: rgba(0, 0, 0, 0.7);
      backdrop-filter: blur(8px);
      -webkit-backdrop-filter: blur(8px);
      z-index: 200;
      opacity: 0;
      pointer-events: none;
      transition: opacity 0.22s ease;
    }
    .drawer-backdrop.active { opacity: 1; pointer-events: auto; }
    .drawer {
      position: fixed;
      left: 0;
      right: 0;
      bottom: 0;
      max-height: 84vh;
      background: #0f121a;
      border-top: 1px solid var(--border-focus);
      border-top-left-radius: 24px;
      border-top-right-radius: 24px;
      padding: 16px 18px 30px;
      z-index: 210;
      transform: translateY(100%);
      transition: transform 0.28s cubic-bezier(0.16, 1, 0.3, 1);
      display: flex;
      flex-direction: column;
      box-shadow: 0 -12px 40px rgba(0,0,0,0.8);
    }
    .drawer.active { transform: translateY(0); }
    .drawer-handle {
      width: 44px;
      height: 5px;
      background: #2b3044;
      border-radius: 3px;
      margin: 0 auto 14px;
    }
    .drawer-header {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-bottom: 16px;
      padding-bottom: 10px;
      border-bottom: 1px solid var(--border);
    }
    .drawer-title { font-size: 1.05rem; font-weight: 800; color: #fff; }
    .drawer-close {
      background: #1a1e2b;
      border: none;
      color: var(--text-muted);
      width: 32px;
      height: 32px;
      border-radius: 50%;
      font-size: 0.9rem;
      cursor: pointer;
      display: flex;
      align-items: center;
      justify-content: center;
      font-weight: 800;
    }
    .drawer-content { overflow-y: auto; flex: 1; -webkit-overflow-scrolling: touch; }

    /* Drawer Sections & List Items */
    .section-label { font-size: 0.78rem; font-weight: 800; color: #8a92aa; letter-spacing: 0.5px; margin: 12px 0 8px; }
    .item-list { display: flex; flex-direction: column; gap: 8px; }
    .list-item {
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      padding: 12px 14px;
      display: flex;
      justify-content: space-between;
      align-items: center;
      cursor: pointer;
      transition: background 0.1s, border-color 0.1s;
    }
    .list-item:active { background: #1c2130; }
    .list-item.selected {
      border-color: var(--primary);
      background: #1f1b24;
    }
    .item-left { display: flex; align-items: center; gap: 10px; font-weight: 600; font-size: 0.9rem; }
    .check-mark { color: var(--primary); font-weight: 800; width: 16px; }
    .lang-code { font-size: 0.72rem; background: #1f2434; color: #9aa2bc; padding: 2px 7px; border-radius: 6px; font-weight: 700; }

    /* Button Groups & Sync Bars */
    .btn-group { display: flex; gap: 8px; margin: 8px 0; }
    .btn-pill {
      flex: 1;
      height: 40px;
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      color: var(--text);
      font-size: 0.82rem;
      font-weight: 700;
      cursor: pointer;
    }
    .btn-pill.active { background: var(--primary); border-color: var(--primary); color: #fff; }
    .btn-pill:active { transform: scale(0.95); }
    .sync-bar {
      display: flex;
      align-items: center;
      justify-content: space-between;
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      padding: 8px 12px;
      margin-top: 6px;
    }
    .sync-val { font-weight: 800; color: var(--accent); font-size: 0.95rem; font-variant-numeric: tabular-nums; }
    .sync-btn {
      background: #1d2232;
      border: 1px solid var(--border);
      color: #fff;
      padding: 6px 14px;
      border-radius: 8px;
      font-size: 0.82rem;
      font-weight: 700;
      cursor: pointer;
    }
  </style>
</head>
<body>
  <!-- Header -->
  <div class="header">
    <div class="brand">❖ SOLUS REMOTE</div>
    <div class="header-actions">
      <button class="btn-fs" onclick="sendAction('toggle_fullscreen')">󰄬 Fullscreen</button>
      <div class="status-badge" id="conn-badge">
        <div class="status-dot"></div><span id="conn-text">LIVE</span>
      </div>
    </div>
  </div>

  <!-- Primary Playback Card -->
  <div class="card">
    <div class="hero-meta">
      <div class="poster-box" id="poster-box">❖</div>
      <div class="hero-details">
        <div class="title" id="title">Solus Theatre</div>
        <div class="subtitle" id="subtitle">Connecting to local stream...</div>
        <div class="badges-row">
          <span class="tag-badge" id="audio-badge">SUB</span>
          <span class="tag-badge" id="sub-badge">English</span>
          <span class="tag-badge" id="res-badge">1080p</span>
        </div>
      </div>
    </div>

    <!-- Scrubber -->
    <div class="timeline">
      <input type="range" min="0" max="100" value="0" class="slider" id="scrubber">
      <div class="time-labels">
        <span id="time-pos">00:00</span>
        <span id="time-dur">00:00</span>
      </div>
    </div>

    <!-- Transport Controls -->
    <div class="controls">
      <button class="btn btn-seek" onclick="sendAction('seek', { delta: -10 })">-10s</button>
      <button class="btn btn-play" id="play-btn" onclick="togglePlay()">▶</button>
      <button class="btn btn-seek" onclick="sendAction('seek', { delta: 10 })">+10s</button>
    </div>

    <!-- Quick Actions -->
    <div class="quick-actions">
      <button class="btn btn-action" onclick="sendAction('skip_intro')">◈ Skip Intro</button>
      <button class="btn btn-action" onclick="sendAction('seek', { delta: 85 })">◈ Jump Recap</button>
      <button class="btn btn-action" onclick="sendAction('toggle_audio')">Audio (Sub/Dub)</button>
      <button class="btn btn-action" onclick="sendAction('toggle_sub_visibility')">Subtitles On/Off</button>
    </div>

    <!-- Volume Attenuator -->
    <div class="volume-box">
      <span class="vol-icon" onclick="sendAction('volume', { delta: -5 })">󰕿</span>
      <input type="range" min="0" max="100" value="80" class="slider" id="volume-slider">
      <span class="vol-icon" onclick="sendAction('volume', { delta: 5 })">󰕾</span>
      <span class="vol-val" id="vol-val">80%</span>
    </div>
  </div>

  <!-- Bottom Drawer Navigation Bar -->
  <div class="bottom-nav">
    <button class="nav-btn" onclick="openDrawer('subtitles')">
      <span class="nav-icon">❖</span>Subtitles
    </button>
    <button class="nav-btn" onclick="openDrawer('episodes')">
      <span class="nav-icon">◈</span>Episodes
    </button>
    <button class="nav-btn" onclick="openDrawer('audio')">
      <span class="nav-icon">✦</span>Audio & Speed
    </button>
  </div>

  <!-- Backdrop for Drawers -->
  <div class="drawer-backdrop" id="backdrop" onclick="closeAllDrawers()"></div>

  <!-- 1. Subtitle & Sync Drawer -->
  <div class="drawer" id="drawer-subtitles">
    <div class="drawer-handle"></div>
    <div class="drawer-header">
      <div class="drawer-title">❖ Subtitle Manager & Sync</div>
      <button class="drawer-close" onclick="closeAllDrawers()">✕</button>
    </div>
    <div class="drawer-content">
      <div class="section-label">SELECT SUBTITLE LANGUAGE</div>
      <div class="item-list" id="subtitles-list">
        <!-- Rendered dynamically -->
      </div>

      <div class="section-label">SUBTITLE TIMING SYNC</div>
      <div class="sync-bar">
        <button class="sync-btn" onclick="sendAction('set_sub_delay', { delta: -0.25 })">-250ms</button>
        <span class="sync-val" id="sub-delay-val">0.00s</span>
        <button class="sync-btn" onclick="sendAction('set_sub_delay', { delta: 0.25 })">+250ms</button>
      </div>

      <div class="section-label">TYPOGRAPHY STYLE PRESET</div>
      <div class="btn-group">
        <button class="btn-pill" id="preset-studio" onclick="setPreset('Studio Minimal')">Studio</button>
        <button class="btn-pill" id="preset-netflix" onclick="setPreset('Netflix Clean')">Netflix</button>
        <button class="btn-pill" id="preset-fansub" onclick="setPreset('Anime Fansub')">Fansub</button>
        <button class="btn-pill" id="preset-oled" onclick="setPreset('OLED Plate')">OLED</button>
      </div>
    </div>
  </div>

  <!-- 2. Episode Switcher Drawer -->
  <div class="drawer" id="drawer-episodes">
    <div class="drawer-handle"></div>
    <div class="drawer-header">
      <div class="drawer-title">◈ Series Episodes</div>
      <button class="drawer-close" onclick="closeAllDrawers()">✕</button>
    </div>
    <div class="drawer-content">
      <div class="section-label">TAP TO SWITCH PLAYBACK</div>
      <div class="item-list" id="episodes-list">
        <!-- Rendered dynamically -->
      </div>
    </div>
  </div>

  <!-- 3. Audio & Speed Drawer -->
  <div class="drawer" id="drawer-audio">
    <div class="drawer-handle"></div>
    <div class="drawer-header">
      <div class="drawer-title">✦ Audio, Speed & Video</div>
      <button class="drawer-close" onclick="closeAllDrawers()">✕</button>
    </div>
    <div class="drawer-content">
      <div class="section-label">PLAYBACK SPEED RATE</div>
      <div class="btn-group">
        <button class="btn-pill" id="speed-075" onclick="setSpeed(0.75)">0.75x</button>
        <button class="btn-pill" id="speed-100" onclick="setSpeed(1.0)">1.0x</button>
        <button class="btn-pill" id="speed-125" onclick="setSpeed(1.25)">1.25x</button>
        <button class="btn-pill" id="speed-150" onclick="setSpeed(1.5)">1.5x</button>
      </div>

      <div class="section-label">AUDIO MODE</div>
      <div class="btn-group">
        <button class="btn-pill" id="audio-sub-btn" onclick="sendAction('toggle_audio')">Japanese (Sub)</button>
        <button class="btn-pill" id="audio-dub-btn" onclick="sendAction('toggle_audio')">English (Dub)</button>
      </div>

      <div class="section-label">REAL-TIME ANIME4K SHADER</div>
      <div class="btn-group">
        <button class="btn-pill" id="a4k-off">Off</button>
        <button class="btn-pill" id="a4k-balanced">Balanced 2x</button>
        <button class="btn-pill" id="a4k-ultra">Ultra 4K</button>
      </div>
    </div>
  </div>

  <script>
    let ws = null;
    let isDragging = false;
    let localIsPlaying = false;
    let localTimePos = 0;
    let localDuration = 0;
    let currentState = null;

    const scrubber = document.getElementById('scrubber');
    const playBtn = document.getElementById('play-btn');
    const volSlider = document.getElementById('volume-slider');
    const volVal = document.getElementById('vol-val');

    scrubber.addEventListener('input', () => {
      isDragging = true;
      const pos = parseFloat(scrubber.value);
      document.getElementById('time-pos').innerText = formatTime(pos);
      document.getElementById('time-dur').innerText = `-${formatTime(Math.max(0, localDuration - pos))}`;
    });

    scrubber.addEventListener('change', () => {
      const pos = parseFloat(scrubber.value);
      localTimePos = pos;
      sendAction('set_pos', { pos: pos });
      setTimeout(() => { isDragging = false; }, 250);
    });

    scrubber.addEventListener('touchend', () => {
      setTimeout(() => { isDragging = false; }, 300);
    });

    volSlider.addEventListener('input', () => {
      volVal.innerText = volSlider.value + '%';
    });

    volSlider.addEventListener('change', () => {
      const val = parseInt(volSlider.value);
      sendAction('set_volume', { value: val });
    });

    function togglePlay() {
      localIsPlaying = !localIsPlaying;
      playBtn.innerText = localIsPlaying ? '❚❚' : '▶';
      sendAction('play_pause');
    }

    function setSpeed(speed) {
      sendAction('set_speed', { speed: speed });
      updateSpeedPills(speed);
    }

    function setPreset(preset) {
      sendAction('set_sub_preset', { preset: preset });
      updatePresetPills(preset);
    }

    function formatTime(s) {
      if (!s || isNaN(s) || s < 0) return '00:00';
      const m = Math.floor(s / 60);
      const sec = Math.floor(s % 60);
      return `${m.toString().padStart(2, '0')}:${sec.toString().padStart(2, '0')}`;
    }

    /* Drawer Controls */
    function openDrawer(id) {
      closeAllDrawers();
      document.getElementById('backdrop').classList.add('active');
      const drawer = document.getElementById(`drawer-${id}`);
      if (drawer) drawer.classList.add('active');
      if (navigator.vibrate) navigator.vibrate(20);
    }

    function closeAllDrawers() {
      document.getElementById('backdrop').classList.remove('active');
      document.querySelectorAll('.drawer').forEach(d => d.classList.remove('active'));
    }

    /* State Renderer */
    function renderState(data) {
      if (!data) return;
      currentState = data;
      localIsPlaying = data.is_playing;
      localDuration = data.duration;
      if (!isDragging) localTimePos = data.time_pos;

      document.getElementById('title').innerText = data.title || 'Solus Anime Theatre';
      document.getElementById('subtitle').innerText = data.is_playing 
        ? `Episode ${data.episode} — ${data.episode_title}`
        : 'Idle / Ready for playback';

      document.getElementById('audio-badge').innerText = (data.audio_mode || 'sub').toUpperCase();
      document.getElementById('sub-badge').innerText = data.sub_lang || 'Subtitles';

      // Poster thumbnail
      const pBox = document.getElementById('poster-box');
      if (data.poster_url && data.poster_url.startsWith('http')) {
        pBox.innerHTML = `<img src="${data.poster_url}" alt="Poster" />`;
      } else {
        pBox.innerHTML = '❖';
      }

      playBtn.innerText = data.is_playing ? '❚❚' : '▶';

      if (!isDragging && data.duration > 0) {
        scrubber.max = data.duration;
        scrubber.value = data.time_pos;
        document.getElementById('time-pos').innerText = formatTime(data.time_pos);
        document.getElementById('time-dur').innerText = `-${formatTime(Math.max(0, data.duration - data.time_pos))}`;
      }

      if (!volSlider.matches(':active')) {
        volSlider.value = data.volume;
        volVal.innerText = `${data.volume}%`;
      }

      // Subtitle Delay
      const delay = data.sub_delay || 0.0;
      document.getElementById('sub-delay-val').innerText = `${delay >= 0 ? '+' : ''}${delay.toFixed(2)}s`;

      // Subtitles List
      renderSubtitlesList(data.subtitles);

      // Episodes List
      renderEpisodesList(data.episodes, data.episode);

      // Speed & Audio Pills
      updateSpeedPills(data.speed || 1.0);
      updateAudioPills(data.audio_mode || 'sub');
      updatePresetPills(data.sub_preset || 'Studio Minimal');
    }

    function renderSubtitlesList(subs) {
      const container = document.getElementById('subtitles-list');
      if (!subs || subs.length === 0) {
        container.innerHTML = '<div style="color:#7d849b;padding:8px;font-size:0.85rem;">No alternate subtitle tracks available.</div>';
        return;
      }
      container.innerHTML = subs.map(s => `
        <div class="list-item ${s.is_active ? 'selected' : ''}" onclick="selectSubtitle(${s.id})">
          <div class="item-left">
            <span class="check-mark">${s.is_active ? '󰄬' : ''}</span>
            <span>${s.label}</span>
          </div>
          <span class="lang-code">${s.code.toUpperCase()}</span>
        </div>
      `).join('');
    }

    function renderEpisodesList(episodes, currentEp) {
      const container = document.getElementById('episodes-list');
      if (!episodes || episodes.length === 0) {
        container.innerHTML = '<div style="color:#7d849b;padding:8px;font-size:0.85rem;">Episode catalog ready on launch.</div>';
        return;
      }
      container.innerHTML = episodes.map(e => `
        <div class="list-item ${e.ep === currentEp ? 'selected' : ''}" onclick="selectEpisode(${e.ep})">
          <div class="item-left">
            <span class="check-mark">${e.ep === currentEp ? '▶' : (e.is_watched ? '󰄬' : '')}</span>
            <span>Ep ${e.ep}. ${e.title}</span>
          </div>
          <span class="lang-code">${e.ep === currentEp ? 'PLAYING' : (e.is_watched ? 'WATCHED' : '')}</span>
        </div>
      `).join('');
    }

    function selectSubtitle(id) {
      sendAction('set_sub', { sub_id: id });
      closeAllDrawers();
    }

    function selectEpisode(ep) {
      sendAction('select_episode', { episode: ep });
      closeAllDrawers();
    }

    function updateSpeedPills(speed) {
      ['075', '100', '125', '150'].forEach(s => {
        const btn = document.getElementById(`speed-${s}`);
        if (btn) btn.classList.remove('active');
      });
      const match = speed <= 0.8 ? '075' : speed <= 1.1 ? '100' : speed <= 1.35 ? '125' : '150';
      const el = document.getElementById(`speed-${match}`);
      if (el) el.classList.add('active');
    }

    function updateAudioPills(mode) {
      const sub = document.getElementById('audio-sub-btn');
      const dub = document.getElementById('audio-dub-btn');
      if (sub && dub) {
        if (mode === 'dub') {
          sub.classList.remove('active');
          dub.classList.add('active');
        } else {
          sub.classList.add('active');
          dub.classList.remove('active');
        }
      }
    }

    function updatePresetPills(preset) {
      ['studio', 'netflix', 'fansub', 'oled'].forEach(p => {
        const el = document.getElementById(`preset-${p}`);
        if (el) el.classList.remove('active');
      });
      const p = preset.toLowerCase();
      const target = p.includes('netflix') ? 'netflix' : p.includes('fansub') ? 'fansub' : p.includes('oled') ? 'oled' : 'studio';
      const el = document.getElementById(`preset-${target}`);
      if (el) el.classList.add('active');
    }

    /* Zero-Latency WebSocket Connection Engine */
    function connectWs() {
      const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
      const url = `${proto}//${location.host}/ws`;

      try {
        ws = new WebSocket(url);
        ws.onopen = () => {
          document.getElementById('conn-badge').className = 'status-badge';
          document.getElementById('conn-text').innerText = 'LIVE';
        };

        ws.onmessage = (event) => {
          try {
            const data = JSON.parse(event.data);
            renderState(data);
          } catch (e) {}
        };

        ws.onclose = () => {
          document.getElementById('conn-badge').className = 'status-badge standby';
          document.getElementById('conn-text').innerText = 'RECONNECTING';
          setTimeout(connectWs, 1200);
        };

        ws.onerror = () => {
          if (ws) ws.close();
        };
      } catch (e) {
        setTimeout(connectWs, 2000);
      }
    }

    async function sendAction(action, payload = {}) {
      if (navigator.vibrate) {
        try { navigator.vibrate(25); } catch (e) {}
      }

      const body = { action, ...payload };

      // Sub-millisecond WebSocket frame if connected
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(body));
        return;
      }

      // HTTP fallback
      try {
        await fetch('/api/action', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body)
        });
      } catch (e) {}
    }

    // Initialize WebSocket on page load
    connectWs();

    // Fallback polling only if WebSocket fails completely
    setInterval(() => {
      if (!ws || ws.readyState !== WebSocket.OPEN) {
        fetch('/api/status').then(r => r.json()).then(renderState).catch(() => {});
      }
    }, 2500);
  </script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_local_ip_returns_valid_string() {
        let ip = get_local_ip();
        assert!(!ip.is_empty());
    }

    #[test]
    fn test_generate_terminal_qr_creates_dense_blocks() {
        let qr = generate_terminal_qr("http://192.168.1.50:8484");
        assert!(qr.is_some());
        let content = qr.unwrap();
        assert!(content.contains('█') || content.contains('▀') || content.contains('▄'));
    }

    #[test]
    fn test_remote_state_serialization() {
        let st = RemotePlaybackState {
            title: "Frieren".to_string(),
            episode: 14,
            time_pos: 120.0,
            duration: 1440.0,
            subtitles: vec![RemoteSubtitleTrack {
                id: 1,
                code: "eng".to_string(),
                label: "English".to_string(),
                is_active: true,
            }],
            episodes: vec![RemoteEpisodeItem {
                ep: 1,
                title: "Journey's End".to_string(),
                is_watched: true,
                is_current: false,
            }],
            ..Default::default()
        };

        let json = serde_json::to_string(&st).unwrap();
        assert!(json.contains("Frieren"));
        assert!(json.contains("1440.0"));
        assert!(json.contains("English"));
        assert!(json.contains("Journey's End"));
    }
}
