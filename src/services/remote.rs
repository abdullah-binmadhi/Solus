//! Zero-Setup Local LAN Mobile Remote Companion (`solus remote`).
//! Serves an embedded, dependency-free mobile touch client over local Wi-Fi,
//! renders terminal ASCII QR codes, and relays touch commands to mpv IPC.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UnixStream};
use tokio::sync::RwLock;

/// Shared live playback state visible to the mobile companion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePlaybackState {
    pub is_playing: bool,
    pub title: String,
    pub episode: u32,
    pub episode_title: String,
    pub time_pos: f64,
    pub duration: f64,
    pub volume: i32,
    pub sub_lang: String,
    pub audio_mode: String,
}

impl Default for RemotePlaybackState {
    fn default() -> Self {
        Self {
            is_playing: false,
            title: "No Active Stream".to_string(),
            episode: 1,
            episode_title: "Idle".to_string(),
            time_pos: 0.0,
            duration: 0.0,
            volume: 80,
            sub_lang: "eng".to_string(),
            audio_mode: "sub".to_string(),
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

/// Dispatches an mpv JSON-RPC command directly over the local Unix domain socket.
pub async fn send_mpv_command(sock_path: &str, command: &[&str]) -> bool {
    let mut stream = match UnixStream::connect(sock_path).await {
        Ok(s) => s,
        Err(_) => return false,
    };

    let cmd_json = serde_json::json!({
        "command": command
    });
    let payload = format!("{}\n", cmd_json);
    stream.write_all(payload.as_bytes()).await.is_ok()
}

/// Spawns the background HTTP server for the mobile remote companion.
pub fn spawn_remote_server(
    port: u16,
    state: Arc<RwLock<RemotePlaybackState>>,
    sock_path: String,
    shutdown: Arc<AtomicBool>,
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
            let (mut socket, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    continue;
                }
            };

            let state_clone = Arc::clone(&state);
            let sock_path_clone = sock_path.clone();

            tokio::spawn(async move {
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
                    let st = state_clone.read().await;
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
                    // Extract JSON body
                    let body = if let Some(idx) = request.find("\r\n\r\n") {
                        &request[idx + 4..]
                    } else {
                        ""
                    };

                    if let Ok(req) = serde_json::from_str::<RemoteActionRequest>(body) {
                        handle_remote_action(&req, &sock_path_clone).await;
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
            });
        }
    });
}

/// Executes an incoming touch remote command against mpv IPC.
async fn handle_remote_action(req: &RemoteActionRequest, sock_path: &str) {
    match req.action.as_str() {
        "play_pause" => {
            send_mpv_command(sock_path, &["cycle", "pause"]).await;
        }
        "seek" => {
            let delta = req.delta.unwrap_or(10.0);
            let delta_str = delta.to_string();
            send_mpv_command(sock_path, &["seek", &delta_str, "relative"]).await;
        }
        "set_pos" => {
            if let Some(pos) = req.pos {
                let pos_str = pos.to_string();
                send_mpv_command(sock_path, &["set_property", "time-pos", &pos_str]).await;
            }
        }
        "volume" => {
            let delta = req.delta.unwrap_or(5.0);
            let delta_str = delta.to_string();
            send_mpv_command(sock_path, &["add", "volume", &delta_str]).await;
        }
        "skip_intro" => {
            send_mpv_command(sock_path, &["seek", "85", "relative"]).await;
        }
        "cycle_audio" => {
            send_mpv_command(sock_path, &["cycle", "audio"]).await;
        }
        "cycle_sub" => {
            send_mpv_command(sock_path, &["cycle", "sub"]).await;
        }
        _ => {}
    }
}

/// Single-file zero-dependency responsive OLED mobile touch client.
const REMOTE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
  <title>Solus Companion</title>
  <style>
    * { box-sizing: border-box; margin: 0; padding: 0; -webkit-tap-highlight-color: transparent; }
    body {
      background: #090a0f;
      color: #f0f2f5;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      min-height: 100vh;
      display: flex;
      flex-direction: column;
      align-items: center;
      padding: 18px 16px;
      user-select: none;
    }
    .header {
      width: 100%;
      max-width: 420px;
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding-bottom: 14px;
      border-bottom: 1px solid #1a1c24;
    }
    .brand { font-size: 1.15rem; font-weight: 800; letter-spacing: 1px; color: #e63946; }
    .status-badge {
      font-size: 0.75rem;
      background: #142a1e;
      color: #50fa7b;
      padding: 4px 10px;
      border-radius: 12px;
      display: flex;
      align-items: center;
      gap: 6px;
      font-weight: 600;
    }
    .status-dot { width: 7px; height: 7px; border-radius: 50%; background: #50fa7b; }
    .card {
      width: 100%;
      max-width: 420px;
      background: #10121a;
      border: 1px solid #1f2330;
      border-radius: 16px;
      padding: 20px 18px;
      margin-top: 20px;
      box-shadow: 0 8px 24px rgba(0,0,0,0.4);
    }
    .title { font-size: 1.25rem; font-weight: 700; color: #fff; margin-bottom: 4px; }
    .subtitle { font-size: 0.9rem; color: #8a90a4; margin-bottom: 18px; }
    .timeline { width: 100%; margin: 10px 0; }
    .slider {
      width: 100%;
      height: 6px;
      border-radius: 3px;
      background: #252a3a;
      outline: none;
      -webkit-appearance: none;
    }
    .slider::-webkit-slider-thumb {
      -webkit-appearance: none;
      width: 18px;
      height: 18px;
      border-radius: 50%;
      background: #e63946;
      cursor: pointer;
    }
    .time-labels {
      display: flex;
      justify-content: space-between;
      font-size: 0.8rem;
      color: #71778c;
      margin-top: 6px;
      font-variant-numeric: tabular-nums;
    }
    .controls {
      display: flex;
      justify-content: center;
      align-items: center;
      gap: 16px;
      margin: 22px 0 10px;
    }
    .btn {
      background: #191c28;
      border: 1px solid #2b3044;
      color: #fff;
      border-radius: 12px;
      cursor: pointer;
      display: flex;
      justify-content: center;
      align-items: center;
      font-weight: 600;
      transition: background 0.15s, transform 0.1s;
    }
    .btn:active { transform: scale(0.94); background: #222738; }
    .btn-seek { width: 62px; height: 50px; font-size: 0.95rem; }
    .btn-play {
      width: 80px;
      height: 60px;
      background: #e63946;
      border: none;
      font-size: 1.4rem;
      border-radius: 16px;
    }
    .btn-play:active { background: #c52b37; }
    .quick-actions {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 10px;
      margin-top: 16px;
    }
    .btn-action { height: 42px; font-size: 0.85rem; color: #cfd4e2; }
    .volume-box {
      margin-top: 18px;
      padding-top: 14px;
      border-top: 1px solid #1a1c24;
      display: flex;
      align-items: center;
      gap: 12px;
    }
    .vol-icon { font-size: 1.1rem; color: #8a90a4; }
  </style>
</head>
<body>
  <div class="header">
    <div class="brand">❖ SOLUS REMOTE</div>
    <div class="status-badge"><div class="status-dot"></div>LIVE</div>
  </div>

  <div class="card">
    <div class="title" id="title">Solus Player</div>
    <div class="subtitle" id="subtitle">Connecting to local stream...</div>

    <div class="timeline">
      <input type="range" min="0" max="100" value="0" class="slider" id="scrubber">
      <div class="time-labels">
        <span id="time-pos">00:00</span>
        <span id="time-dur">00:00</span>
      </div>
    </div>

    <div class="controls">
      <button class="btn btn-seek" onclick="sendAction('seek', { delta: -10 })">-10s</button>
      <button class="btn btn-play" id="play-btn" onclick="sendAction('play_pause')">▶</button>
      <button class="btn btn-seek" onclick="sendAction('seek', { delta: 10 })">+10s</button>
    </div>

    <div class="quick-actions">
      <button class="btn btn-action" onclick="sendAction('skip_intro')">◈ Skip Intro</button>
      <button class="btn btn-action" onclick="sendAction('seek', { delta: 85 })">◈ Jump Recap</button>
      <button class="btn btn-action" onclick="sendAction('cycle_sub')">Subtitles ▾</button>
      <button class="btn btn-action" onclick="sendAction('cycle_audio')">Audio ▾</button>
    </div>

    <div class="volume-box">
      <span class="vol-icon" onclick="sendAction('volume', { delta: -5 })">󰕿</span>
      <input type="range" min="0" max="100" value="80" class="slider" id="volume-slider" oninput="sendAction('volume', { delta: 0 })">
      <span class="vol-icon" onclick="sendAction('volume', { delta: 5 })">󰕾</span>
    </div>
  </div>

  <script>
    let isDragging = false;
    const scrubber = document.getElementById('scrubber');
    const playBtn = document.getElementById('play-btn');

    scrubber.addEventListener('mousedown', () => isDragging = true);
    scrubber.addEventListener('touchstart', () => isDragging = true);
    scrubber.addEventListener('change', () => {
      isDragging = false;
      sendAction('set_pos', { pos: parseFloat(scrubber.value) });
    });

    function formatTime(s) {
      if (!s || isNaN(s)) return '00:00';
      const m = Math.floor(s / 60);
      const sec = Math.floor(s % 60);
      return `${m.toString().padStart(2, '0')}:${sec.toString().padStart(2, '0')}`;
    }

    async function updateStatus() {
      try {
        const res = await fetch('/api/status');
        if (!res.ok) return;
        const data = await res.json();
        document.getElementById('title').innerText = data.title || 'Solus Anime';
        document.getElementById('subtitle').innerText = `Episode ${data.episode} — ${data.episode_title}`;
        document.getElementById('time-pos').innerText = formatTime(data.time_pos);
        document.getElementById('time-dur').innerText = formatTime(data.duration);
        playBtn.innerText = data.is_playing ? '❚❚' : '▶';

        if (!isDragging && data.duration > 0) {
          scrubber.max = data.duration;
          scrubber.value = data.time_pos;
        }
      } catch (e) {}
    }

    async function sendAction(action, payload = {}) {
      try {
        await fetch('/api/action', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ action, ...payload })
        });
        setTimeout(updateStatus, 150);
      } catch (e) {}
    }

    setInterval(updateStatus, 800);
    updateStatus();
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
            ..Default::default()
        };

        let json = serde_json::to_string(&st).unwrap();
        assert!(json.contains("Frieren"));
        assert!(json.contains("1440.0"));
    }
}
