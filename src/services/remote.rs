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

/// Dispatches an mpv JSON-RPC command directly over the local Unix domain socket.
pub async fn send_mpv_json_command(sock_path: &str, cmd_value: serde_json::Value) -> bool {
    let target = match resolve_active_socket(sock_path).await {
        Some(t) => t,
        None => return false,
    };

    let mut stream = match UnixStream::connect(&target).await {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload = format!("{}\n", cmd_value);
    if stream.write_all(payload.as_bytes()).await.is_err() {
        return false;
    }
    stream.flush().await.is_ok()
}

/// Dispatches an mpv JSON-RPC string command array.
pub async fn send_mpv_command(sock_path: &str, command: &[&str]) -> bool {
    let cmd_json = serde_json::json!({
        "command": command
    });
    send_mpv_json_command(sock_path, cmd_json).await
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
                        handle_remote_action(&req, &sock_path_clone, &state_clone).await;
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

/// Executes an incoming touch remote command against mpv IPC with optimistic local state sync.
async fn handle_remote_action(
    req: &RemoteActionRequest,
    sock_path: &str,
    state: &Arc<RwLock<RemotePlaybackState>>,
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
        "skip_intro" => {
            let cmd = serde_json::json!({ "command": ["seek", 85.0, "relative"] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.time_pos = (st.time_pos + 85.0).max(0.0);
        }
        "cycle_audio" => {
            let cmd = serde_json::json!({ "command": ["cycle", "audio"] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
            let mut st = state.write().await;
            st.audio_mode = if st.audio_mode == "sub" { "dub".to_string() } else { "sub".to_string() };
        }
        "cycle_sub" => {
            let cmd = serde_json::json!({ "command": ["cycle", "sub"] });
            let _ = send_mpv_json_command(sock_path, cmd).await;
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
      max-width: 440px;
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
      transition: all 0.2s ease;
    }
    .status-dot { width: 7px; height: 7px; border-radius: 50%; background: #50fa7b; }
    .card {
      width: 100%;
      max-width: 440px;
      background: #10121a;
      border: 1px solid #1f2330;
      border-radius: 20px;
      padding: 22px 20px;
      margin-top: 20px;
      box-shadow: 0 12px 32px rgba(0,0,0,0.5);
    }
    .title { font-size: 1.3rem; font-weight: 800; color: #fff; margin-bottom: 4px; line-height: 1.25; }
    .subtitle { font-size: 0.92rem; color: #8a90a4; margin-bottom: 20px; }
    .timeline { width: 100%; margin: 12px 0; }
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
      background: #e63946;
      cursor: pointer;
      box-shadow: 0 0 10px rgba(230, 57, 70, 0.6);
    }
    .time-labels {
      display: flex;
      justify-content: space-between;
      font-size: 0.85rem;
      color: #8b92a8;
      margin-top: 8px;
      font-variant-numeric: tabular-nums;
      font-weight: 600;
    }
    .controls {
      display: flex;
      justify-content: center;
      align-items: center;
      gap: 18px;
      margin: 24px 0 14px;
    }
    .btn {
      background: #191c28;
      border: 1px solid #2b3044;
      color: #fff;
      border-radius: 14px;
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
      width: 86px;
      height: 64px;
      background: #e63946;
      border: none;
      font-size: 1.5rem;
      border-radius: 18px;
      box-shadow: 0 6px 18px rgba(230, 57, 70, 0.45);
    }
    .btn-play:active { background: #c52b37; }
    .quick-actions {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 12px;
      margin-top: 18px;
    }
    .btn-action { height: 46px; font-size: 0.88rem; color: #cfd4e2; }
    .volume-box {
      margin-top: 22px;
      padding-top: 16px;
      border-top: 1px solid #1a1c24;
      display: flex;
      align-items: center;
      gap: 14px;
    }
    .vol-icon { font-size: 1.25rem; color: #8a90a4; cursor: pointer; padding: 4px; }
    .vol-val { font-size: 0.8rem; font-weight: 700; color: #71778c; width: 38px; text-align: right; }
  </style>
</head>
<body>
  <div class="header">
    <div class="brand">❖ SOLUS REMOTE</div>
    <div class="status-badge" id="conn-badge"><div class="status-dot"></div><span id="conn-text">LIVE</span></div>
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
      <button class="btn btn-play" id="play-btn" onclick="togglePlay()">▶</button>
      <button class="btn btn-seek" onclick="sendAction('seek', { delta: 10 })">+10s</button>
    </div>

    <div class="quick-actions">
      <button class="btn btn-action" onclick="sendAction('skip_intro')">◈ Skip Intro</button>
      <button class="btn btn-action" onclick="sendAction('seek', { delta: 85 })">◈ Jump Recap</button>
      <button class="btn btn-action" onclick="sendAction('cycle_sub')">Subtitles ▾</button>
      <button class="btn btn-action" onclick="sendAction('cycle_audio')">Audio (Sub/Dub)</button>
    </div>

    <div class="volume-box">
      <span class="vol-icon" onclick="sendAction('volume', { delta: -5 })">󰕿</span>
      <input type="range" min="0" max="100" value="80" class="slider" id="volume-slider">
      <span class="vol-icon" onclick="sendAction('volume', { delta: 5 })">󰕾</span>
      <span class="vol-val" id="vol-val">80%</span>
    </div>
  </div>

  <script>
    let isDragging = false;
    let localIsPlaying = false;
    let localTimePos = 0;
    let localDuration = 0;
    const scrubber = document.getElementById('scrubber');
    const playBtn = document.getElementById('play-btn');
    const volSlider = document.getElementById('volume-slider');
    const volVal = document.getElementById('vol-val');

    scrubber.addEventListener('input', () => {
      isDragging = true;
      document.getElementById('time-pos').innerText = formatTime(scrubber.value);
    });

    scrubber.addEventListener('change', () => {
      const pos = parseFloat(scrubber.value);
      localTimePos = pos;
      sendAction('set_pos', { pos: pos });
      setTimeout(() => { isDragging = false; }, 300);
    });

    scrubber.addEventListener('touchend', () => {
      setTimeout(() => { isDragging = false; }, 350);
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

    function formatTime(s) {
      if (!s || isNaN(s) || s < 0) return '00:00';
      const m = Math.floor(s / 60);
      const sec = Math.floor(s % 60);
      return `${m.toString().padStart(2, '0')}:${sec.toString().padStart(2, '0')}`;
    }

    async function updateStatus() {
      try {
        const res = await fetch('/api/status');
        if (!res.ok) {
          document.getElementById('conn-badge').style.background = '#2a1a14';
          document.getElementById('conn-badge').style.color = '#ffb86c';
          document.getElementById('conn-text').innerText = 'STANDBY';
          return;
        }
        document.getElementById('conn-badge').style.background = '#142a1e';
        document.getElementById('conn-badge').style.color = '#50fa7b';
        document.getElementById('conn-text').innerText = 'LIVE';

        const data = await res.json();
        localIsPlaying = data.is_playing;
        localTimePos = data.time_pos;
        localDuration = data.duration;

        document.getElementById('title').innerText = data.title || 'Solus Anime Theatre';
        document.getElementById('subtitle').innerText = data.is_playing 
          ? `Episode ${data.episode} — ${data.episode_title}`
          : 'Ready for playback';

        if (!isDragging) {
          document.getElementById('time-pos').innerText = formatTime(data.time_pos);
          document.getElementById('time-dur').innerText = formatTime(data.duration);
          playBtn.innerText = data.is_playing ? '❚❚' : '▶';

          if (data.duration > 0) {
            scrubber.max = data.duration;
            scrubber.value = data.time_pos;
          }
        }

        if (!volSlider.matches(':active')) {
          volSlider.value = data.volume;
          volVal.innerText = data.volume + '%';
        }
      } catch (e) {
        document.getElementById('conn-badge').style.background = '#2a1a14';
        document.getElementById('conn-badge').style.color = '#ffb86c';
        document.getElementById('conn-text').innerText = 'STANDBY';
      }
    }

    async function sendAction(action, payload = {}) {
      if (navigator.vibrate) {
        try { navigator.vibrate(25); } catch (e) {}
      }

      try {
        await fetch('/api/action', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ action, ...payload })
        });
        setTimeout(updateStatus, 120);
      } catch (e) {}
    }

    setInterval(updateStatus, 750);
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
