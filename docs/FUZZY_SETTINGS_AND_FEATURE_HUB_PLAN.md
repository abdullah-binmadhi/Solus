# ❖ SOLUS // FUZZY SETTINGS & FEATURE COMMAND HUB BLUEPRINT

> **Purpose**: Complete overhaul of the Solus Settings overlay. Replaces the buggy, overlapping 10-row grid with a high-performance **Split-Deck Fuzzy Feature Command Hub** inspired by Neovim's Telescope and Raycast.
>
> **Neovim Navigation**: Open anytime with `nvim docs/FUZZY_SETTINGS_AND_FEATURE_HUB_PLAN.md` (use `zc` / `zo` to fold sections or `/❖` / `/✦` to jump).

---

## ❖ 1. Diagnostic Review of Current Issues

### A. The Screenshot Bug (Layout Overflow & Text Collision)
- **Observed Behavior**: In `src/ui/settings.rs`, setting rows collided with the bottom status card:
  - `"Status Ready for playbackd 2x ultra 4k"` and `"Changes save immediately to config.toml Ambilight: enabled disabled"` rendered directly on top of each other.
- **Root Cause**: Fixed row constraints (`Constraint::Length(2)`) inside a single vertical layout exceeded the inner terminal viewport height, causing subsequent widgets to clip and draw over identical line coordinates.

### B. Feature Discoverability & Keybind Friction
- **The Problem**: Powerful new capabilities (Cinema Subtitles, Mobile LAN Remote, TV Casting) were bound to disparate single-character keys (<kbd>m</kbd>, <kbd>c</kbd>, <kbd>s</kbd>). Users should not have to memorize a dozen hotkeys just to discover and launch features.
- **Redundant Clutter**: Global settings for **Player**, **Quality**, **Audio (Sub/Dub)**, and **Default Subtitle Language** are redundant. These are already dynamically negotiated per-show and per-provider inside the **Split-Deck Episode & Playback Options Matrix**.

---

## ✦ 2. The Solution: Split-Deck Fuzzy Feature Command Hub

Instead of a passive preferences form, Settings becomes a **Fuzzy Feature Command Hub**:

```
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│                                  ❖ SOLUS // CONTROL HUB                                      │
├──────────────────────────────────────────────────────────────────────────────────────────────┤
│  > Type to filter features, actions, and settings (e.g. 'cast', 'remote', 'sub', 'update')_  │
├──────────────────────────────────────────────┬───────────────────────────────────────────────┤
│  COMMAND PALETTE (Fuzzy Filtered)            │  LIVE FEATURE INSPECTOR & PREVIEW             │
│                                              │                                               │
│  ❖ CINEMA SUBTITLES                          │  Selected: Cinema Subtitle Styler             │
│    [󰄬] Preset: Studio Minimal ▾              │  Preset: Studio Minimal (Trebuchet MS)        │
│    [󰄬] Signage Collision Guard: Enabled      │                                               │
│                                              │  ┌─ Live Typography Preview ────────────────┐ │
│  ❖ LIVING ROOM & MOBILE                      │  │ "I am the one who knocks...              │ │
│    [󰄬] Scan & Beam to TV (AirPlay/DLNA)      │  │  and the truth cannot be erased."        │ │
│    [󰄬] Launch Mobile Remote QR Modal         │  └──────────────────────────────────────────┘ │
│    [󰄬] Open Touch Remote in Browser          │                                               │
│                                              │  Signage Guard:                               │
│  ❖ RENDERING & PLAYBACK                      │  Detects bottom Japanese on-screen signs and  │
│    [󰄬] Real-Time Anime4K: Balanced 2x ▾      │  dynamically shifts dialogue upward ({\an8}). │
│    [󰄬] Dynamic Cover Ambilight: Enabled      │                                               │
│    [󰄬] Auto-Skip OP/ED (AniSkip): Enabled    │  [Enter] Cycle Preset   [Space] Toggle Guard  │
│                                              │                                               │
│  ❖ SYSTEM & UPDATES                          │                                               │
│    [󰄬] Check for Updates (v1.0.6)            │                                               │
│    [󰄬] Auto-Update Channel: Check Only ▾     │                                               │
│    [󰄬] Re-run Dependency Health Doctor       │                                               │
└──────────────────────────────────────────────┴───────────────────────────────────────────────┘
```

---

## ◈ 3. Key Architectural Decisions

1. **Purge Redundant Settings**:
   - **Remove**: `Player` (mpv/iina/vlc), `Quality` (1080p/720p/etc), `Audio` (sub/dub), and `Subtitle Language` (English/Arabic/etc).
   - **Why**: These are already chosen inside the 3-column episode launcher (`PlaybackOptions`), where providers actually report their available resolutions and languages.
2. **Instant Action Execution (`Enter`)**:
   - Pressing <kbd>Enter</kbd> on **"Scan & Beam to TV"** immediately launches the TV device scanner without needing <kbd>c</kbd>.
   - Pressing <kbd>Enter</kbd> on **"Launch Mobile Remote QR Modal"** immediately opens the QR code without needing <kbd>m</kbd>.
   - Pressing <kbd>Enter</kbd> on **"Open Touch Remote in Browser"** opens `http://192.168.x.x:8484` in the default browser.
   - Pressing <kbd>Enter</kbd> on **"Check for Updates"** checks GitHub releases and starts the fluid update pipeline.
3. **Fuzzy Search Across All Keywords**:
   - Typing `cast` or `tv` or `airplay` filters down to the TV casting actions.
   - Typing `phone` or `qr` or `remote` filters to the mobile companion actions.
   - Typing `sub` or `font` filters to subtitle presets and collision avoidance.
   - Typing `gpu` or `4k` filters to Anime4K upscaling.
4. **Split-Deck Layout with Zero Text Collisions**:
   - Left side (55% width): Search bar + scrollable, categorized command list.
   - Right side (45% width): Dynamic contextual inspector card rendering live metadata, network IPs, and visual subtitle previews.

---

## ✦ 4. Interactive ASCII Sketches

### Sketch A: Default State (All Actions Grouped by Domain)

```
┌─ ❖ SOLUS // CONTROL HUB & FEATURE PALETTE ────────────────────────────────── [Esc: Close] ─┐
│                                                                                            │
│  🔍 > Type to filter (e.g. 'cast', 'remote', 'sub', 'update', 'skip')...                   │
│                                                                                            │
│ ┌─ Feature Actions ──────────────────────────┐ ┌─ Contextual Feature Inspector ───────────┐ │
│ │                                            │ │                                           │ │
│ │ ── ❖ CINEMATIC SUBTITLES ───────────────── │ │ ❖ CINEMA SUBTITLE STYLER                  │ │
│ │  ▶ Preset: Studio Minimal ▾               │ │ Preset: Studio Minimal                    │ │
│ │    Signage Collision Guard: [󰄬] Enabled    │ │ Font: Trebuchet MS (48pt, Outline 2.5)    │ │
│ │                                            │ │                                           │ │
│ │ ── ❖ TELEVISION & MOBILE REMOTE ────────── │ │ ┌─ Live In-Terminal Preview ────────────┐ │ │
│ │    Scan & Beam to TV (AirPlay/DLNA)        │ │ │ "I am the one who knocks...           │ │ │
│ │    Launch Mobile Remote QR Modal           │ │ │  and the truth cannot be erased."     │ │ │
│ │    Open Touch Remote in Web Browser        │ │ └───────────────────────────────────────┘ │ │
│ │                                            │ │                                           │ │
│ │ ── ❖ VISUAL & PLAYBACK ENGINE ──────────── │ │ ◈ Signage Collision Guard:                │ │
│ │    Real-Time Anime4K: [ Balanced 2x ▾ ]    │ │ Detects on-screen Japanese signs and      │ │
│ │    Cover-Adaptive Ambilight: [󰄬] Enabled   │ │ lifts dialogue upward to eliminate        │ │
│ │    Auto-Skip OP/ED: [󰄬] Enabled            │ │ visual overlap collisions.                │ │
│ │                                            │ │                                           │ │
│ │ ── ❖ SYSTEM & MAINTENANCE ──────────────── │ │ Status: Saved to ~/.config/solus/         │ │
│ │    Check for Solus Updates                 │ │                                           │ │
│ │    Auto-Update: [ Check Only ▾ ]           │ │ [Enter/l] Cycle Preset                    │ │
│ │    Dependency Health Doctor (!)            │ │ [Space]   Toggle Collision Guard          │ │
│ └────────────────────────────────────────────┘ └───────────────────────────────────────────┘ │
│  [↑/↓] Select Item   [Enter] Execute / Cycle   [Backspace] Clear Filter   [Esc] Dismiss     │
└────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

### Sketch B: Filtered State (User Types `cast`)

```
┌─ ❖ SOLUS // CONTROL HUB & FEATURE PALETTE ────────────────────────────────── [Esc: Close] ─┐
│                                                                                            │
│  🔍 > cast█                                                                                │
│                                                                                            │
│ ┌─ Filtered Actions (2 Matches) ─────────────┐ ┌─ Contextual Feature Inspector ───────────┐ │
│ │                                            │ │                                           │ │
│ │  ▶ Scan & Beam to TV (AirPlay / DLNA)     │ │ ❖ NATIVE TELEVISION STREAM SENDER         │ │
│ │    TV Auto-Discovery: [󰄬] Enabled          │ │                                           │ │
│ │                                            │ │ Protocol: AirPlay 2 + DLNA / UPnP         │ │
│ │                                            │ │ Transmission: Direct HLS Passthrough      │ │
│ │                                            │ │ Network Scope: Local Wi-Fi Subnet         │ │
│ │                                            │ │ Target Hardware: Apple TV, LG webOS,      │ │
│ │                                            │ │                  Samsung Tizen, Sony TV   │ │
│ │                                            │ │                                           │ │
│ │                                            │ │ ◈ Action:                                 │ │
│ │                                            │ │ Pressing [Enter] immediately broadcasts   │ │
│ │                                            │ │ SSDP and AirPlay probes to discover       │ │
│ │                                            │ │ wireless screens in your room.            │ │
│ │                                            │ │                                           │ │
│ │                                            │ │ [Enter] Launch TV Target Picker           │ │
│ └────────────────────────────────────────────┘ └───────────────────────────────────────────┘ │
│  [↑/↓] Select Item   [Enter] Execute Action   [Backspace] Edit Search   [Esc] Dismiss       │
└────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

### Sketch C: Filtered State (User Types `remote`)

```
┌─ ❖ SOLUS // CONTROL HUB & FEATURE PALETTE ────────────────────────────────── [Esc: Close] ─┐
│                                                                                            │
│  🔍 > remote█                                                                              │
│                                                                                            │
│ ┌─ Filtered Actions (3 Matches) ─────────────┐ ┌─ Contextual Feature Inspector ───────────┐ │
│ │                                            │ │                                           │ │
│ │  ▶ Launch Mobile Remote QR Modal          │ │ ❖ MOBILE COMPANION REMOTE                 │ │
│ │    Open Touch Remote in Browser            │ │                                           │ │
│ │    Mobile Remote Server: [󰄬] Enabled       │ │ URL: http://192.168.1.42:8484             │ │
│ │                                            │ │ Status: ● Active Wi-Fi Listener           │ │
│ │                                            │ │ Features: Scrub slider, ±10s seek,        │ │
│ │                                            │ │           Sub/Dub swap, Volume slider     │ │
│ │                                            │ │                                           │ │
│ │                                            │ │ ▄▄▄▄▄▄▄  ▄ ▄▄ ▄▄  ▄▄▄▄▄▄▄                 │ │
│ │                                            │ │ █ ▄▄▄ █  █▄█▀█▄█  █ ▄▄▄ █                 │ │
│ │                                            │ │ █ ███ █ ▀▄ ▄▀▄ █  █ ███ █                 │ │
│ │                                            │ │ █▄▄▄▄▄█ █ █ █ █▀  █▄▄▄▄▄█                 │ │
│ │                                            │ │                                           │ │
│ │                                            │ │ [Enter] Open Fullscreen QR Modal          │ │
│ └────────────────────────────────────────────┘ └───────────────────────────────────────────┘ │
│  [↑/↓] Select Item   [Enter] Open QR Modal   [Backspace] Edit Search   [Esc] Dismiss        │
└────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## ◈ 5. Data Model & Action Registry

```rust
pub enum HubActionKind {
    /// Cycles through multiple preset options (e.g. Subtitle Preset, Anime4K, Update Mode)
    Cycle { values: Vec<String>, current: usize },
    /// Toggles a boolean flag (e.g. Collision Guard, Ambilight, Auto-Skip)
    Toggle(bool),
    /// Triggers an immediate screen transition or modal (e.g. Cast Modal, Remote QR Modal)
    Launch(HubScreenTarget),
    /// Spawns an external task (e.g. Open browser link, check update)
    Execute,
}

pub struct HubAction {
    pub id: &'static str,
    pub category: &'static str,
    pub label: &'static str,
    pub keywords: &'static str,
    pub description: &'static str,
    pub kind: HubActionKind,
}
```

---

## ❖ 6. Implementation Plan & Action Items

1. **Purge Redundant Elements**:
   - Remove `PLAYER_CHOICES`, `QUALITY_CHOICES`, `AUDIO_CHOICES`, and `SUBTITLE_CHOICES` from `settings.rs`.
   - Free up vertical space and eliminate user confusion.
2. **Build Split-Deck UI (`src/ui/settings.rs`)**:
   - Add search input field at the top (`state.settings_query`).
   - Left pane: Filtered `List` with category headers and selection pointer `▶`.
   - Right pane: Rich contextual card (Subtitle preview, Remote QR preview, TV status).
3. **Fuzzy Search Integration**:
   - Use `fuzzy_matcher::skim::SkimMatcherV2` to match `action.label + action.keywords`.
   - Up/Down navigates only the filtered items.
4. **Action Dispatcher (`src/main.rs`)**:
   - <kbd>Enter</kbd> executes the selected action directly:
     - `launch_remote` → Opens `Screen::RemoteModal`
     - `launch_cast` → Scans network and opens `Screen::CastModal`
     - `open_browser` → Opens `http://<ip>:8484`
     - `sub_preset` → Cycles preset and auto-saves
     - `collision_guard` → Toggles guard and auto-saves
     - `anime4k` → Cycles Anime4K and auto-saves
     - `check_update` → Triggers update check
