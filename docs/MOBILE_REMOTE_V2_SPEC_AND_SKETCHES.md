# ❖ SOLUS // ZERO-LATENCY MOBILE COMPANION v2.0
## Architectural Specification, WebSocket Pipeline & Neovim Layout Sketches

> **Neovim Tip**: Open anytime in your terminal with:
> `nvim docs/MOBILE_REMOTE_V2_SPEC_AND_SKETCHES.md`
> Fold sections with `zc` / `zo`, or jump between sketches using `/◈ SKETCH`.

---

## ❖ 1. Diagnostic Analysis: Why Remote v1 Was Sluggish

### A. HTTP Short-Polling Network Overhead
* **Observed Flaw**: The client performed an HTTP `fetch('/api/status')` every 750ms over local Wi-Fi.
* **Network Cost**: Each request created a separate TCP handshake (`SYN` $\rightarrow$ `SYN-ACK` $\rightarrow$ `ACK`), transmitted HTTP headers, parsed response text, and executed a TCP tear-down (`Connection: close`).
* **UI Bottleneck**: When tapping a button (such as Play or Seek), an action `POST` collided with an in-flight polling request, causing high queuing latency (300ms–900ms) on mobile browsers.

### B. Socket Thrashing & Directory Scanning
* **Observed Flaw**: Every action executed `resolve_active_socket`, which scanned `/tmp/`, read file metadata across the directory, and opened a new `UnixStream::connect`.
* **Kernel Cost**: Opening and closing Unix domain sockets on every single tick or seek command introduced severe context switching and serialization delays.

### C. Missing Subtitle and Episode Selection
* **Observed Flaw**: Only a generic `cycle sub` command existed. Users could not see which subtitle languages were available (English, Arabic, Spanish, French, etc.), select a specific track, view episode lists, or adjust sync delays.

---

## ✦ 2. The v2.0 Architecture: Zero-Latency WebSocket Pipeline

```text
┌────────────────────────────────────────────────────────────────────────┐
│                        MOBILE REMOTE v2.0 PIPELINE                     │
├────────────────────────────────┬───────────────────────────────────────┤
│    MOBILE TOUCH CLIENT         │            SOLUS RUNTIME              │
├────────────────────────────────┼───────────────────────────────────────┤
│ HTML5 + Canvas Touch Engine    │ Tokio Async Runtime                   │
│ Persistent WebSocket (ws://)   │ Persistent mpv IPC Client Actor       │
│ Sub-Millisecond Input Dispatch │ Bidirectional State Synchronization  │
│ Optimistic Touch Feedback      │ Multi-Track Subtitle Manager          │
│ Slide-Up Drawer Modals         │ Episode Queue Dispatcher              │
└────────────────────────────────┴───────────────────────────────────────┘
```

1. **Persistent WebSocket (`ws://<IP>:8484/ws`)**:
   - Single long-lived TCP connection established on page load.
   - Control commands travel as compact JSON frames in **< 1 millisecond**.
   - Solus pushes playback telemetry (time, duration, volume, pause state) directly over the WebSocket without HTTP polling.
2. **Persistent mpv IPC Worker**:
   - The remote daemon maintains an open UnixStream connection to `mpv`. Commands are written directly without reconnecting or scanning `/tmp`.
3. **Comprehensive State Payload**:
   - Transmits available subtitle languages with active selection states.
   - Transmits full episode catalog for the current anime.
   - Exposes subtitle delay and typography presets.

---

## ◈ SKETCH 1: Main Playback Theatre Deck (OLED Dark Mode)

```text
┌────────────────────────────────────────────────────────┐
│ ❖ SOLUS REMOTE                   ● 3ms  [󰄬 OLED] [ ✕ ] │
├────────────────────────────────────────────────────────┤
│                                                        │
│   ╭──────────╮  FRIEREN: BEYOND JOURNEY'S END          │
│   │  POSTER  │  Episode 04                             │
│   │ THUMBNAIL│  "The Land Where Souls Rest"            │
│   │  [1080p] │  ★ 9.1 · TV Series · Studio Madhouse    │
│   ╰──────────╯                                         │
│                                                        │
│ ── Playback Scrubber ───────────────────────────────── │
│                                                        │
│   ●━━━━━━━━━━━━━━━━━━━━━━━━━━━━━───────○────────────── │
│   08:42                                         -15:18 │
│                                                        │
│ ── Primary Transport Deck ──────────────────────────── │
│                                                        │
│    ╭──────────╮      ╭──────────────╮      ╭──────────╮│
│    │          │      │              │      │          ││
│    │   -10s   │      │    ❚❚ PAUSE   │      │   +10s   ││
│    │          │      │              │      │          ││
│    ╰──────────╯      ╰──────────────╯      ╰──────────╯│
│                                                        │
│ ── Quick Action Controls ───────────────────────────── │
│                                                        │
│   ╭────────────────╮ ╭────────────────╮ ╭────────────╮│
│   │ ◈ Skip Intro   │ │ 󰒭 Next Episode │ │ 󰄬 Fullscr  ││
│   ╰────────────────╯ ╰────────────────╯ ╰────────────╯│
│                                                        │
│ ── Volume Attenuator ───────────────────────────────── │
│                                                        │
│   󰕿   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━●─────   󰕾   82%    │
│                                                        │
│ ── Bottom Drawer Navigation Bar ────────────────────── │
│                                                        │
│   [ 📑 Subtitles ]     [ 🎬 Episodes ]    [ 🎛 Audio ] │
└────────────────────────────────────────────────────────┘
```

---

## ◈ SKETCH 2: Subtitle & Audio Drawer (Slide-Up Modal)

```text
┌────────────────────────────────────────────────────────┐
│ ── 📑 Subtitle Manager & Sync Deck ─────────── [ ✕ ] ── │
├────────────────────────────────────────────────────────┤
│                                                        │
│  [󰄬] Subtitles Enabled           Audio: [ SUB ] [ DUB ]│
│                                                        │
│  Select Subtitle Language:                             │
│  ┌──────────────────────────────────────────────────┐  │
│  │ 󰄬 01. English [Official Crunchyroll]    (eng)    │  │
│  │    02. Arabic / العربية [Verified]      (ara)    │  │
│  │    03. Spanish / Español [Castellano]   (spa)    │  │
│  │    04. French / Français                (fra)    │  │
│  │    05. German / Deutsch                 (deu)    │  │
│  │    06. Japanese Signs & Songs           (jpn)    │  │
│  └──────────────────────────────────────────────────┘  │
│                                                        │
│  Subtitle Timing Fine-Tuning:                          │
│  ╭─────────╮         ╭───────────────╮        ╭─────────╮
│  │ - 250ms │         │  Sync: 0.00s  │        │ + 250ms │
│  ╰─────────╯         ╰───────────────╯        ╰─────────╯
│                                                        │
│  Typography Style Presets:                             │
│  [ Studio Minimal ]  [ Netflix Clean ]  [ Anime Fansub ]│
│                                                        │
│  Signage Collision Guard:   [󰄬 Active (Auto-Shift)]    │
└────────────────────────────────────────────────────────┘
```

---

## ◈ SKETCH 3: Episode Switcher Drawer (Slide-Up Modal)

```text
┌────────────────────────────────────────────────────────┐
│ ── 🎬 Episode Selector ─────────────────────── [ ✕ ] ── │
├────────────────────────────────────────────────────────┤
│                                                        │
│  Current: Ep 04 — "The Land Where Souls Rest"          │
│                                                        │
│  ┌──────────────────────────────────────────────────┐  │
│  │ 󰄬 01. The Journey's End                 [Watched]│  │
│  │ 󰄬 02. It Didn't Have to Be Magic...     [Watched]│  │
│  │ 󰄬 03. Killing Magic                     [Watched]│  │
│  │ ▶ 04. The Land Where Souls Rest         [Playing]│  │
│  │   05. Phantoms of the Dead                       │  │
│  │   06. The Hero of the Village                    │  │
│  │   07. Like a Fairy Tale                          │  │
│  │   08. Frieren the Slayer                         │  │
│  └──────────────────────────────────────────────────┘  │
│                                                        │
│  [ Tap any episode to immediately switch playback ]    │
└────────────────────────────────────────────────────────┘
```

---

## ◈ SKETCH 4: Audio, Video & Precision Control Drawer

```text
┌────────────────────────────────────────────────────────┐
│ ── 🎛 Audio, Speed & Video Processing ──────── [ ✕ ] ── │
├────────────────────────────────────────────────────────┤
│                                                        │
│  Playback Speed Rate:                                  │
│  [ 0.75x ]    [ 1.0x (Normal) ]    [ 1.25x ]   [ 1.5x ]│
│                                                        │
│  Real-Time Anime4K Upscaler Engine:                    │
│  [ Off ]            [ Balanced 2x ]      [ Ultra 4K ]  │
│                                                        │
│  Audio Equalizer / Night Mode:                         │
│  [ Normal ]         [ Vocal Boost ]      [ Night Mode ]│
│                                                        │
│  Sleep Timer:                                          │
│  [ Off ]     [ 15 min ]     [ 30 min ]     [ End Ep ]  │
│                                                        │
│  Terminal Viewport:                                    │
│  [ Toggle Ambilight Glow: 󰄬 ]   [ Hide Terminal Deck ] │
└────────────────────────────────────────────────────────┘
```

---

## ⬡ 3. WebSocket Data Protocol Specification

### A. Client $\rightarrow$ Server (Instant JSON Action Frames)
```json
{ "action": "play_pause" }
{ "action": "seek", "delta": 10.0 }
{ "action": "set_pos", "pos": 524.0 }
{ "action": "set_volume", "value": 85 }
{ "action": "set_sub", "sub_id": 2 }
{ "action": "set_sub_delay", "delay_ms": 100 }
{ "action": "set_sub_preset", "preset": "netflix_clean" }
{ "action": "select_episode", "episode": 5 }
{ "action": "set_speed", "speed": 1.25 }
{ "action": "skip_intro" }
{ "action": "cycle_audio" }
```

### B. Server $\rightarrow$ Client (Real-Time Broadcast Frame)
```json
{
  "is_playing": true,
  "title": "Frieren: Beyond Journey's End",
  "episode": 4,
  "episode_title": "The Land Where Souls Rest",
  "poster_url": "https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx154587-n2bBgALjlLKA.jpg",
  "time_pos": 524.0,
  "duration": 1440.0,
  "volume": 82,
  "speed": 1.0,
  "audio_mode": "sub",
  "subtitles": [
    { "id": 1, "code": "eng", "name": "English [Crunchyroll]", "active": true },
    { "id": 2, "code": "ara", "name": "Arabic (العربية)", "active": false },
    { "id": 3, "code": "spa", "name": "Spanish (Español)", "active": false }
  ],
  "episodes": [
    { "ep": 1, "title": "The Journey's End", "watched": true },
    { "ep": 2, "title": "It Didn't Have to Be Magic...", "watched": true },
    { "ep": 3, "title": "Killing Magic", "watched": true },
    { "ep": 4, "title": "The Land Where Souls Rest", "watched": false },
    { "ep": 5, "title": "Phantoms of the Dead", "watched": false }
  ],
  "sub_delay": 0.0,
  "sub_preset": "Studio Minimal",
  "anime4k": "ultra"
}
```

---

## ❖ 4. Implementation Strategy

1. **WebSocket Protocol Engine**:
   - Integrate lightweight WebSocket upgrade handler in `src/services/remote.rs`.
   - Maintain active client connections in a broadcast set (`tokio::sync::broadcast`).
2. **Persistent IPC Actor**:
   - Keep a single long-lived Unix socket connection to mpv.
   - Forward commands from WebSocket directly to mpv without reconnecting.
3. **Rich Subtitle & Episode Extraction**:
   - Populate `subtitles` from `state.playback_sub_languages` and mpv `track-list`.
   - Populate `episodes` from `state.episode_titles` and SQLite `watched_episodes`.
4. **Touch Client Overhaul**:
   - Embed high-performance HTML5 + CSS3 OLED interface with sliding drawers for Subtitles, Episodes, and Audio.
   - Zero external libraries or CDNs: 100% self-contained single payload.
