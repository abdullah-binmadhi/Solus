# ❖ ARCHITECTURAL BLUEPRINT // CINEMA SUBTITLES, MOBILE REMOTE & TV CAST

> **Target Systems**:
> 1. **Cinema-Grade Subtitle Styler & Signage Collision Avoidance** (Part 1, #5)
> 2. **Zero-Setup Local LAN Mobile Remote Companion** (Part 2, #6)
> 3. **Native AirPlay & Chromecast / DLNA Direct TV Sender** (Part 2, #7)
>
> **Neovim Navigation**: Open anytime with `nvim docs/SUBTITLES_REMOTE_AND_TV_CAST_PLAN.md` (use `zc` / `zo` to fold sections or `/❖` / `/✦` to jump).

---

## ❖ 1. System Architecture & Component Interaction

```
┌─────────────────────────────────────────────────────────────────────────────────────────────┐
│                                     SOLUS TUI RUNTIME                                       │
│                                                                                             │
│   ┌────────────────────────┐   ┌───────────────────────────┐   ┌────────────────────────┐   │
│   │  Subtitle Styler       │   │  Local Mesh Remote        │   │  TV Direct Cast Engine │   │
│   │  & Collision Guard     │   │  (Embedded Tokio HTTP/WS) │   │  (mDNS & SSDP Sender)  │   │
│   └───────────┬────────────┘   └─────────────┬─────────────┘   └───────────┬────────────┘   │
│               │                              │                             │                │
│       Dynamic libass args            WebSockets (Port 8484)          AirPlay / DLNA / UPnP  │
│               │                              │                             │                │
└───────────────┼──────────────────────────────┼─────────────────────────────┼────────────────┘
                ▼                              ▼                             ▼
   ┌────────────────────────┐    ┌───────────────────────────┐ ┌──────────────────────────────┐
   │    Local mpv Player    │    │  Mobile Touch Companion   │ │ Living Room TV / Apple TV 4K │
   │  - Font Overrides      │    │  - Real-Time Scrub Bar    │ │ - 1080p/4K Hardware Direct   │
   │  - Signage Margin Lift │    │  - Play/Pause/Volume      │ │ - Synchronized Clocks        │
   │  - High-Contrast Plate │    │  - Sub/Dub & Episode List │ │ - Zero Terminal Lag          │
   └────────────────────────┘    └───────────────────────────┘ └──────────────────────────────┘
```

---

## ✦ 2. Feature 1: Cinema-Grade Subtitle Styler & Signage Collision Avoidance

### A. The Visual Problem & Solution
Standard streaming subtitles and raw `.ass` files suffer from two major flaws:
1. **Low Legibility & Eye Strain**: Thin fonts without sufficient stroke outlines get lost against bright anime backgrounds (e.g., explosions, sky scenes, white snow).
2. **Signage Collision Overlap**: When on-screen Japanese text (store signs, phone text messages, episode titles) is translated at the bottom of the screen, dialogue subtitles overlap directly on top of the sign translation, rendering both completely unreadable.

### B. Signage Collision Avoidance (Visual Diagram)

```
┌────────────────────────────────────────────────────────────────────────────────┐
│ ◈ WITHOUT COLLISION GUARD (Standard Clashing Subtitles)                       │
├────────────────────────────────────────────────────────────────────────────────┤
│                                                                                │
│                        [ Bright Anime Sky / Explosion ]                        │
│                                                                                │
│                                                                                │
│             ┌────────────────────────────────────────────────────┐             │
│             │ [Japanese Sign]: "POLICE HEADQUARTERS - 3RD WARD"  │             │
│             │ "Wait, didn't you hear what he just said to us?!"  │ <── CLASH!  │
│             └────────────────────────────────────────────────────┘             │
└────────────────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────────────────┐
│ ❖ WITH SOLUS COLLISION AVOIDANCE (Intelligent Signage Offset Lift)             │
├────────────────────────────────────────────────────────────────────────────────┤
│                                                                                │
│                        [ Bright Anime Sky / Explosion ]                        │
│                                                                                │
│             ┌────────────────────────────────────────────────────┐             │
│             │ "Wait, didn't you hear what he just said to us?!"  │ <── LIFTED  │
│             └────────────────────────────────────────────────────┘     BY 70px │
│             ┌────────────────────────────────────────────────────┐             │
│             │ [Sign]: POLICE HEADQUARTERS - 3RD WARD             │ <── CLEAN   │
│             └────────────────────────────────────────────────────┘             │
└────────────────────────────────────────────────────────────────────────────────┘
```

### C. TUI Subtitle Studio & Live Preview Sketch (`Settings -> Subtitles`)

```
┌─ Settings // Subtitle Studio ────────────────────────────────────────────── [Esc: Back] ─┐
│                                                                                          │
│  Presets:  [ ❖ Studio Minimal ]   [ Netflix Clean ]   [ Retro Fansub ]   [ OLED Plate ]  │
│                                                                                          │
│  Typography Configuration:                                                               │
│    Font Family:     < Gandhi Sans >          Border Outline:   < 2.8 px >                │
│    Font Size:       < 48 pt >                Drop Shadow:      < 1.5 px (Soft) >         │
│    Primary Color:   < #FFFFFF >              Backdrop Plate:   < 25% Opacity >           │
│    Collision Guard: < [󰄬] Enabled >          Margin Bottom:    < 42 px >                 │
│                                                                                          │
│ ┌─ Live In-Terminal Rendering Preview ─────────────────────────────────────────────────┐ │
│ │                                                                                      │ │
│ │  [Dark Background Preview]                                                           │ │
│ │  "I am the one who knocks... and the truth cannot be erased."                        │ │
│ │                                                                                      │ │
│ │  [High-Luminance White Preview]                                                      │ │
│ │  ┌────────────────────────────────────────────────────────────────────────────────┐  │ │
│ │  │ "Even against blazing white light, text remains crisp and razor-sharp."        │  │ │
│ │  └────────────────────────────────────────────────────────────────────────────────┘  │ │
│ └──────────────────────────────────────────────────────────────────────────────────────┘ │
│  [↑/↓] Navigate  [←/→] Adjust Value  [Space] Cycle Preset  [Enter] Save & Apply          │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

### D. Technical Implementation
1. **Config Model (`src/config.rs`)**:
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct SubtitleStyleConfig {
       pub preset: SubtitlePreset,
       pub font_family: String,
       pub font_size: u32,
       pub border_size: f32,
       pub shadow_offset: f32,
       pub margin_y: u32,
       pub backdrop_opacity: f32,
       pub collision_avoidance: bool,
   }
   ```
2. **Collision Detector (`src/services/subsync.rs`)**:
   - Parses `.ass` event blocks. When a `Dialogue` event contains `\pos` or style names containing `sign` / `title` in the bottom 20% of screen height (`y > 850` on 1080p), any overlapping simultaneous dialogue line is automatically shifted upwards by `margin_y + 65px` via injected `\pos(x, y - 75)`.

---

## ✦ 3. Feature 2: Zero-Setup Local LAN Mobile Remote Companion (`solus remote`)

### A. How It Works
- Solus hosts an embedded, ultra-lightweight Tokio HTTP + WebSocket server on port `8484` (LAN only).
- Pressing `R` in Solus opens a modal rendering a native **ASCII QR code** and LAN URL.
- Opening the link on a phone browser serves a single-file, zero-dependency embedded HTML5 touch client (`include_str!("remote_client.html")`).
- The phone communicates with Solus over WebSockets (`/ws`), which relays commands directly to the `mpv` IPC socket with `< 10ms` latency.

### B. Terminal QR Code & Remote Modal Sketch (Press `R`)

```
┌─ Solus Companion // Mobile Remote Control ────────────────────────────────── [Esc: Close] ─┐
│                                                                                            │
│   Point your phone's camera at the QR code below (Connected to same Wi-Fi):               │
│                                                                                            │
│         ▄▄▄▄▄▄▄  ▄ ▄▄ ▄▄  ▄▄▄▄▄▄▄                                                          │
│         █ ▄▄▄ █  █▄█▀█▄█  █ ▄▄▄ █       Direct URL:                                        │
│         █ ███ █ ▀▄ ▄▀▄ █  █ ███ █       http://192.168.1.42:8484                           │
│         █▄▄▄▄▄█ █ █ █ █▀  █▄▄▄▄▄█                                                          │
│         ▄▄▄▄  ▄ ▄▄█▄▄█▄▄  ▄▄▄ ▄▄▄       Status:                                            │
│         █▀█▄▀▄█▀▄█ ▀ ▄█▀ ▀█▄█ █▄█       ● Active Listener on 0.0.0.0:8484                  │
│         ▄▄ █ ▄▄▄ ▀█ ▄ █ ▄ ▄  █▀▀█       ● Connected: 1 Phone (iPhone 15 Pro, Safari)       │
│         ▄▄▄▄▄▄▄ █▄▀█▀ ▄█  █ ▄ █▄█                                                          │
│         █ ▄▄▄ █ █▄ █ ▄█▀  █▄█ █▄█       Security:                                          │
│         █ ███ █ █▀▀ █▄▄▄  ▄ ▄▀▄▄█       LAN-Only Isolated Token Auth                       │
│         █▄▄▄▄▄█ █ ▄▄ ▄▀█  █▀█▀▄██                                                          │
│                                                                                            │
│   [Space] Pause/Play   [o] Open Browser Locally   [r] Regenerate Token   [Esc] Dismiss     │
└────────────────────────────────────────────────────────────────────────────────────────────┘
```

### C. Mobile Phone Touchscreen Interface Layout (Web UI Mockup)

```
┌──────────────────────────────────────────────┐
│  ❖ SOLUS REMOTE                  [● LIVE] ≡  │
├──────────────────────────────────────────────┤
│                                              │
│    ┌──────────────┐   Frieren: Beyond        │
│    │  [Cover Art] │   Journey's End          │
│    │   1080p BD   │   Episode 14             │
│    └──────────────┘   "Privilege of the... " │
│                                              │
│  ──────────────────────────────────────────  │
│  14:32 ━━━━━━━━━━━━●─────────────── 24:10    │
│                                              │
│         ┌───────┐ ┌─────────┐ ┌───────┐      │
│         │ -10s  │ │  ❚❚ / ▶ │ │ +10s  │      │
│         └───────┘ └─────────┘ └───────┘      │
│                                              │
│  [ ◈ Skip Opening ]    [ ◈ Jump Recap ]      │
│                                              │
│  Volume Control:                             │
│  󰕿 ━━━━━━━━━━━━━━●━━━━━ 󰕾 75%              │
│                                              │
│  Quick Switcher:                             │
│  [ Audio: Japanese (Sub) ▾ ]  [ Sub: Eng ▾ ] │
│                                              │
│ ┌─ Quick Episode Selector ─────────────────┐ │
│ │ [Ep 12]  [Ep 13]  [● Ep 14]  [Ep 15] ... │ │
│ └──────────────────────────────────────────┘ │
└──────────────────────────────────────────────┘
```

---

## ✦ 4. Feature 3: Native AirPlay & Chromecast / DLNA Direct TV Sender

### A. How It Works
- Discovers TV streaming devices across the local subnet using zero-configuration networking:
  - **Apple TV & AirPlay 2 Smart TVs**: Discovered via `mDNS` (`_airplay._tcp.local`).
  - **LG webOS, Samsung Tizen & Sony Bravia**: Discovered via `SSDP` (UPnP AVTransport).
  - **Chromecast & Google TV**: Discovered via `_googlecast._tcp.local`.
- Pressing `c` opens the TV Casting Drawer.
- Selecting a target hands the direct HLS stream URL to the TV hardware decoder.
- Solus switches into **"TV Command Deck"** mode: your terminal becomes a synchronized command center displaying cover art, timeline, episode notes, and keyboard controls.

### B. TUI Cast Device Selector Sketch (Press `c` on Episode)

```
┌─ Beam to Living Room // Target Device Picker ─────────────────────────────── [Esc: Cancel] ─┐
│                                                                                             │
│  Scanning local Wi-Fi network for AirPlay, Google Cast & DLNA devices... (300ms)            │
│                                                                                             │
│    [1] Living Room Apple TV 4K        AirPlay 2     192.168.1.100    Ready (1080p/4K)       │
│    [2] LG OLED C3 (webOS)             DLNA / UPnP   192.168.1.108    Ready (Direct Play)    │
│    [3] Bedroom Chromecast with GTV    Google Cast   192.168.1.115    Standby                │
│    [4] Local Terminal Display         mpv Native    localhost        Active                 │
│                                                                                             │
│  Stream Mode:  [ ❖ Direct HLS Stream ]     Subtitles: [ Embedded ASS / VTT ]                │
│                                                                                             │
│  [↑/↓] Select Target   [Enter] Cast Stream Immediately   [s] Rescan Network                 │
└─────────────────────────────────────────────────────────────────────────────────────────────┘
```

### C. TUI "TV Command Deck" Active Cast View

```
┌─ ❖ SOLUS // CASTING TO: Living Room Apple TV 4K ───────────────────────── [c: Stop Cast] ─┐
│                                                                                           │
│  ┌────────────────────────┐  Frieren: Beyond Journey's End                                │
│  │                        │  Episode 14 // Privilege of the Experienced                   │
│  │     [ Kitty / ANSI     │  Quality: 1080p Master HLS (Direct Passthrough)               │
│  │       Cover Art ]      │  Audio: Japanese FLAC 2.0 (Stereo)                            │
│  │                        │  Subtitles: English (Official Crunchyroll via AirPlay)        │
│  └────────────────────────┘                                                               │
│                                                                                           │
│  Playback Status: PLAYING                                                                 │
│  16:45 ━━━━━━━━━━━━━━━━━━━━━━●━━━━━━━━━━━━━━━━━━━━ 24:10  [-07:25]                        │
│                                                                                           │
│  Live Synopsis:                                                                           │
│  Frieren and her party arrive in a bustling fortress city. Stark faces an unexpected      │
│  dilemma when an aristocratic noble requests his presence at a private ball...            │
│                                                                                           │
│ ┌─ Quick Remote Keybindings ────────────────────────────────────────────────────────────┐ │
│ │ [Space] Pause/Resume TV    [←/→] Seek ±10s    [↑/↓] TV Volume    [n] Next Episode     │ │
│ │ [s] Skip Intro             [a] Toggle Sub/Dub [c] Disconnect TV  [q] Back to Library  │ │
│ └───────────────────────────────────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## ◈ 5. Technical Specifications & Data Models

### A. Subtitle Styler Structs (`src/services/subtitles/styler.rs`)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubtitlePreset {
    StudioMinimal,
    NetflixClean,
    RetroFansub,
    OledPlate,
    Custom,
}

impl SubtitlePreset {
    pub fn to_mpv_args(&self, cfg: &SubtitleStyleConfig) -> Vec<String> {
        let mut args = vec![
            format!("--sub-font={}", cfg.font_family),
            format!("--sub-font-size={}", cfg.font_size),
            format!("--sub-border-size={}", cfg.border_size),
            format!("--sub-shadow-offset={}", cfg.shadow_offset),
            format!("--sub-margin-y={}", cfg.margin_y),
            "--sub-ass-override=force".to_string(),
        ];
        if cfg.backdrop_opacity > 0.0 {
            args.push(format!("--sub-back-color=0.0/0.0/0.0/{:.2}", cfg.backdrop_opacity));
        }
        args
    }
}
```

### B. Mobile Remote WebSocket Server (`src/services/remote/mod.rs`)
```rust
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::Html,
    routing::get,
    Router,
};
use std::net::SocketAddr;

pub async fn start_remote_server(port: u16, tx: tokio::sync::mpsc::Sender<AppMessage>) {
    let app = Router::new()
        .route("/", get(|| async { Html(include_str!("remote_client.html")) }))
        .route("/ws", get(|ws: WebSocketUpgrade| async move {
            ws.on_upgrade(|socket| handle_socket(socket, tx))
        }));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

### C. Cast Device & Protocol Engine (`src/services/cast/mod.rs`)
```rust
#[derive(Debug, Clone)]
pub enum CastProtocol {
    AirPlay { ip: String, port: u16 },
    Dlna { control_url: String },
    GoogleCast { ip: String, port: u16 },
}

#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub id: String,
    pub name: String,
    pub protocol: CastProtocol,
    pub resolution_cap: String,
}

impl DiscoveredDevice {
    pub async fn cast_hls(&self, stream_url: &str, title: &str) -> Result<(), AppError> {
        match &self.protocol {
            CastProtocol::AirPlay { ip, port } => {
                let client = reqwest::Client::new();
                let body = format!("Content-Location: {}\nStart-Position: 0.0\n", stream_url);
                client.post(format!("http://{}:{}/play", ip, port))
                    .header("Content-Type", "text/parameters")
                    .body(body)
                    .send()
                    .await?;
            }
            CastProtocol::Dlna { control_url } => {
                // Send standard UPnP SetAVTransportURI SOAP envelope
            }
            CastProtocol::GoogleCast { .. } => {
                // Connect TLS socket and emit CastV2 Media Load packet
            }
        }
        Ok(())
    }
}
```

---

## ❖ 6. Implementation Roadmap & Milestones

```
   Phase 1: Subtitle Studio           Phase 2: Mobile Remote            Phase 3: Native TV Cast
 ┌───────────────────────────┐     ┌───────────────────────────┐     ┌───────────────────────────┐
 │ • libass style generator  │     │ • Tokio HTTP/WS server    │     │ • mDNS & SSDP scanner     │
 │ • Collision lift offsets  │ ──► │ • Terminal QR code render │ ──► │ • AirPlay / DLNA sender   │
 │ • TUI Live preview card   │     │ • Embedded touch HTML/JS  │     │ • TV Command Deck TUI     │
 └───────────────────────────┘     └───────────────────────────┘     └───────────────────────────┘
```

1. **Phase 1: Subtitle Studio & Signage Collision Guard**:
   - Create `src/services/subtitles/styler.rs`.
   - Implement `SubtitlePreset::to_mpv_args` with signage collision offset parsing.
   - Add Subtitle Studio tab to Settings (`s`).
2. **Phase 2: Zero-Setup LAN Mobile Remote**:
   - Create `src/services/remote/mod.rs` and single-file `remote_client.html`.
   - Implement ASCII QR code terminal modal triggered by keybind `R`.
   - Wire WebSocket protocol to `src/services/ipc.rs` for live two-way telemetry.
3. **Phase 3: Native AirPlay & DLNA Direct TV Sender**:
   - Create `src/services/cast/mod.rs` for mDNS and SSDP discovery.
   - Implement AirPlay HTTP POST and DLNA SOAP transport.
   - Build TUI TV Command Deck dashboard for live synchronized living room control.
