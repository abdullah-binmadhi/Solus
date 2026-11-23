//! Control Hub v2 (`[s]`) — network-aware command palette, contextual inspector, and live telemetry.

use crate::config::{self, NetworkMode};
use crate::services::loading_art::{raw_y2k_art, Y2K_ART_COUNT};
use crate::services::remote::{find_remote_theme, REMOTE_THEMES};
use crate::services::substyler::SubtitlePreset;
use crate::state::AppState;
use crate::ui::theme::ThemePalette;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Top-level category filter tabs in the Control Hub v2 ribbon (`[0..=5]`).
pub const SETTINGS_CATEGORIES: [&str; 6] = [
    "All",
    "Network",
    "Playback",
    "Remote",
    "Subtitles",
    "System",
];

/// Modal percentage dimensions so `home.rs::active_modal_rect` accurately masks Kitty artwork.
pub fn modal_pct(area: Rect) -> (u16, u16) {
    if area.width < 96 || area.height < 28 {
        (96, 94)
    } else {
        (92, 90)
    }
}

/// Unique action identifiers for Control Hub entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubActionId {
    NetworkMode,
    AutoAbrDownshift,
    ForceIpv4,
    ManualAbrStepDown,
    SubPreset,
    SubCollisionGuard,
    CastScan,
    RemoteQrModal,
    RemoteOpenBrowser,
    RemoteToggleServer,
    RemoteTheme,
    RemoteArtMode,
    PlaybackEngine,
    PerformanceMode,
    Anime4kMode,
    DebandMode,
    MotionMode,
    AutoFullscreen,
    PipMode,
    PipCorner,
    ToggleInfoHud,
    AmbilightGlow,
    AutoSkipIntro,
    CheckUpdates,
    WhatsNewNotes,
    AutoUpdateMode,
    HealthDoctor,
    AniListSync,
    OpenCalendar,
}

/// Interaction hint classification for list badges and inspector controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubActionKind {
    Cycle { options: Vec<&'static str>, current: usize },
    Toggle(bool),
    Launch(&'static str),
    Execute(&'static str),
}

/// A searchable feature action entry in the command hub.
#[derive(Debug, Clone)]
pub struct HubItem {
    pub id:          HubActionId,
    pub cat_idx:     usize,
    pub category:    &'static str,
    pub label:       &'static str,
    pub keywords:    &'static str,
    pub badge:       String,
    pub description: &'static str,
    pub kind:        HubActionKind,
}

/// Truncate text strictly by Unicode scalar boundaries so narrow viewports never panic.
fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let mut out: String = text.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

/// Assembles the complete catalog of feature actions from current runtime state.
pub fn get_hub_items(cfg: &config::Config, state: &AppState) -> Vec<HubItem> {
    let net_idx = match state.network_mode {
        NetworkMode::Normal        => 0,
        NetworkMode::CongestedWifi => 1,
        NetworkMode::UltraSaver    => 2,
    };
    let sub_preset_idx = match cfg.subtitle_style.preset {
        SubtitlePreset::StudioMinimal => 0,
        SubtitlePreset::NetflixClean  => 1,
        SubtitlePreset::AnimeFansub   => 2,
        SubtitlePreset::OledPlate     => 3,
        SubtitlePreset::Custom        => 0,
    };
    let anime4k_idx = match cfg.anime4k_mode {
        config::Anime4KMode::Off      => 0,
        config::Anime4KMode::Balanced => 1,
        config::Anime4KMode::Ultra    => 2,
    };
    let engine_idx = config::Player::ALL
        .iter()
        .position(|&p| p == cfg.player)
        .unwrap_or(0);
    let perf_idx = config::PerformanceMode::ALL
        .iter()
        .position(|&m| m == cfg.performance_mode)
        .unwrap_or(2);
    let deband_idx = config::DebandMode::ALL
        .iter()
        .position(|&m| m == cfg.deband_mode)
        .unwrap_or(0);
    let motion_idx = config::MotionMode::ALL
        .iter()
        .position(|&m| m == cfg.motion_mode)
        .unwrap_or(0);
    let update_idx = if !cfg.update.check_on_startup {
        2
    } else if cfg.update.auto_install {
        0
    } else {
        1
    };

    let active_theme = find_remote_theme(&cfg.remote.theme_id);
    let art_idx = cfg.remote.art_index.unwrap_or(active_theme.default_art_idx);
    let art_slot = (art_idx % Y2K_ART_COUNT) + 1;
    let abr_badge = if state.live_playback.started_at.is_some() {
        format!("{}p LIVE", state.live_playback.current_height.max(360))
    } else {
        "IDLE".to_string()
    };

    vec![
        // ── 1. NETWORK & STREAMING ────────────────────────────────────────────
        HubItem {
            id: HubActionId::NetworkMode,
            cat_idx: 1,
            category: "◈ NETWORK & STREAMING",
            label: "Adaptive Network Mode",
            keywords: "network wifi eco congested saver bandwidth readahead cache cap",
            badge: format!("[ {} ▾ ]", state.network_mode.short_tag()),
            description: "Switch between Full Speed (uncapped), Congested Wi-Fi (720p30 + 120s disk cache), and Ultra Saver (480p30 + 180s disk cache + thumbnail throttling).",
            kind: HubActionKind::Cycle {
                options: vec!["NET:FULL", "NET:WIFI", "NET:ECO"],
                current: net_idx,
            },
        },
        HubItem {
            id: HubActionId::AutoAbrDownshift,
            cat_idx: 1,
            category: "◈ NETWORK & STREAMING",
            label: "Auto-ABR Downshift",
            keywords: "abr downshift buffer stall bitrate ladder mpv hot switch",
            badge: if cfg.auto_abr_downshift { "[✓] Enabled".into() } else { "[ ] Disabled".into() },
            description: "Automatically step down live HLS stream resolution/FPS via seamless mpv IPC replace when throughput drops below 60% bitrate or stalls repeat.",
            kind: HubActionKind::Toggle(cfg.auto_abr_downshift),
        },
        HubItem {
            id: HubActionId::ForceIpv4,
            cat_idx: 1,
            category: "◈ NETWORK & STREAMING",
            label: "Force IPv4 Transport",
            keywords: "ipv4 ipv6 network timeout stalled route yt-dlp mpv reqwest",
            badge: if cfg.force_ipv4 { "[✓] Forced".into() } else { "[ ] Auto".into() },
            description: "Resolve endpoints strictly over IPv4 in reqwest, mpv, VLC, and yt-dlp to bypass broken IPv6 peering routes on congested Wi-Fi.",
            kind: HubActionKind::Toggle(cfg.force_ipv4),
        },
        HubItem {
            id: HubActionId::ManualAbrStepDown,
            cat_idx: 1,
            category: "◈ NETWORK & STREAMING",
            label: "Step Down Live Stream Tier",
            keywords: "step down quality abr hot switch ladder lower tier manual",
            badge: format!("[ {} ]", abr_badge),
            description: "Immediately downshift the active mpv stream to the next lower FPS or resolution tier from the pre-resolved HLS ladder without closing the window.",
            kind: HubActionKind::Execute("Downshift Active Stream"),
        },

        // ── 2. VISUAL & PLAYBACK ENGINE ───────────────────────────────────────
        HubItem {
            id: HubActionId::PlaybackEngine,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Playback Engine (libmpv)",
            keywords: "player engine mpv mpvnet celluloid auto-best libmpv wrapper",
            badge: format!("[ {} ▾ ]", cfg.player.label()),
            description: "Unified libmpv engine family (Auto-Best -> mpv -> Win mpv.net / Linux Celluloid). Every GLSL shader, Lua HUD, PiP window, and IPC feature runs identically across all engines.",
            kind: HubActionKind::Cycle {
                options: vec!["Auto-Best", "mpv", "mpv.net", "Celluloid"],
                current: engine_idx,
            },
        },
        HubItem {
            id: HubActionId::PerformanceMode,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Performance & Mastering Tier",
            keywords: "performance eco balanced reference 4k thermal gpu wattage battery",
            badge: format!("[ {} ▾ ]", cfg.performance_mode.as_str()),
            description: "Controls GPU shader workload & thermal ceiling: Auto reacts to battery/heat, Eco keeps single-pass fused Luma only, Balanced enables 4:4:4 + 2x gating, Reference 4K pushes maximum detail.",
            kind: HubActionKind::Cycle {
                options: vec!["Auto", "Eco", "Balanced", "Reference 4K"],
                current: perf_idx,
            },
        },
        HubItem {
            id: HubActionId::DebandMode,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "GPU Hardware Debanding",
            keywords: "deband banding dark sky 8bit contours filter",
            badge: format!("[ {} ▾ ]", cfg.deband_mode.as_str()),
            description: "Hardware 1-pass or 2-pass gradient debanding inside libplacebo that removes dark-scene compression contours cleanly at <0.4ms GPU cost.",
            kind: HubActionKind::Cycle {
                options: vec!["Auto (1-Pass)", "Off", "Strong (2-Pass)"],
                current: deband_idx,
            },
        },
        HubItem {
            id: HubActionId::MotionMode,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "24p Motion Cadence",
            keywords: "motion cadence 24fps panning interpolation oversample 60hz 120hz",
            badge: format!("[ {} ▾ ]", cfg.motion_mode.as_str()),
            description: "Smooths slow 24fps anime camera pans on 60Hz/120Hz panels via display-resample oversampling without introducing soap-opera artifacts.",
            kind: HubActionKind::Cycle {
                options: vec!["Off", "Cadence 24->60", "Smooth 120Hz"],
                current: motion_idx,
            },
        },
        HubItem {
            id: HubActionId::AutoFullscreen,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Auto-Fullscreen on Launch",
            keywords: "fullscreen fs cinema launch window maximize player mpv mpvnet celluloid",
            badge: if cfg.auto_fullscreen { "[✓] Enabled".into() } else { "[ ] Disabled".into() },
            description: "Automatically enter fullscreen mode when launching any libmpv engine (mpv, mpv.net, Celluloid), and sync live via IPC.",
            kind: HubActionKind::Toggle(cfg.auto_fullscreen),
        },
        HubItem {
            id: HubActionId::PipMode,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Floating Picture-in-Picture (PiP)",
            keywords: "pip picture in picture float floating mini corner ontop work",
            badge: if state.live_playback.pip_mode || cfg.auto_pip { "[✓] Float ON".into() } else { "[ ] Disabled".into() },
            description: "Pin a borderless 16:9 floating mpv window in the corner above all apps so you can watch & work simultaneously ([p] toggle, [P] corner in mpv).",
            kind: HubActionKind::Toggle(state.live_playback.pip_mode || cfg.auto_pip),
        },
        HubItem {
            id: HubActionId::PipCorner,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "PiP Window Corner & Size",
            keywords: "pip corner size bottom top left right compact theater",
            badge: format!(
                "[ {} ▾ ]",
                crate::services::ipc::PIP_CORNER_LABELS[state.pip_corner_idx % crate::services::ipc::PIP_CORNER_LABELS.len()]
            ),
            description: "Cycle floating Picture-in-Picture corners and scales (Bottom-Right 30% -> Bottom-Left -> Top-Right -> Top-Left -> Compact 24% -> Theater 38%).",
            kind: HubActionKind::Cycle {
                options: crate::services::ipc::PIP_CORNER_LABELS.to_vec(),
                current: state.pip_corner_idx % crate::services::ipc::PIP_CORNER_LABELS.len(),
            },
        },
        HubItem {
            id: HubActionId::ToggleInfoHud,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Multi-Color Bento Video Info HUD",
            keywords: "hud info stats overlay codec fps bitrate buffer bento diagnostics",
            badge: "[ Toggle i / I ]".into(),
            description: "Toggle the multi-color ASS Bento Info HUD inside mpv (i = 6s overlay, I = 1Hz live pin, F1 = raw mpv stats).",
            kind: HubActionKind::Execute("Toggle Info HUD"),
        },
        HubItem {
            id: HubActionId::Anime4kMode,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Anime4K Shader Engine",
            keywords: "anime4k glsl gpu upscaling sharp denoise shaders",
            badge: format!("[ {} ▾ ]", cfg.anime4k_mode.as_str()),
            description: "Real-time GLSL neural restoration & upscaling for mpv. Balanced restores lines cleanly; Ultra adds perceptual sharpening.",
            kind: HubActionKind::Cycle {
                options: vec!["Off", "Balanced", "Ultra"],
                current: anime4k_idx,
            },
        },
        HubItem {
            id: HubActionId::AmbilightGlow,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Adaptive Ambilight Glow",
            keywords: "ambilight colors dynamic glow bias lighting",
            badge: if cfg.ambilight_enabled { "[✓] Enabled".into() } else { "[ ] Disabled".into() },
            description: "Dynamically adapt TUI ambient card framing to extract cover art tonal hues during browsing and detail views.",
            kind: HubActionKind::Toggle(cfg.ambilight_enabled),
        },
        HubItem {
            id: HubActionId::AutoSkipIntro,
            cat_idx: 2,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Auto-Skip OP/ED (AniSkip)",
            keywords: "skip intro outro aniskip opening ending automatic",
            badge: if cfg.auto_skip_intro { "[✓] Enabled".into() } else { "[ ] Disabled".into() },
            description: "Automatically jump past Opening/Ending timestamps reported by AniSkip and broadcast chapter OSD notifications.",
            kind: HubActionKind::Toggle(cfg.auto_skip_intro),
        },

        // ── 3. TELEVISION & MOBILE REMOTE ─────────────────────────────────────
        HubItem {
            id: HubActionId::RemoteToggleServer,
            cat_idx: 3,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Mobile Remote Daemon",
            keywords: "mobile phone remote server port 8484 companion websocket",
            badge: if cfg.remote.enabled {
                format!("[✓] :{} ({})", cfg.remote.port, state.remote_ws_clients)
            } else {
                "[ ] Disabled".into()
            },
            description: "Local Axum WebSocket companion server hosting Solus's themed touch deck on LAN so phones control scrubbing, tracks, network mode, and shaders.",
            kind: HubActionKind::Toggle(cfg.remote.enabled),
        },
        HubItem {
            id: HubActionId::RemoteTheme,
            cat_idx: 3,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Mobile Remote Theme",
            keywords: "remote theme skin y2k anime gaming zen hifi svg icons",
            badge: format!("[ {} ▾ ]", active_theme.name),
            description: "Select from 12 multi-genre mobile companion skins (Y2K CRT, Shibuya Punk, Manga Ink, Tactical HUD, Arcade 16-Bit, Kyoto Sumi, Hi-Fi Deck) changing SVG icon packs and button geometry live.",
            kind: HubActionKind::Cycle {
                options: REMOTE_THEMES.iter().map(|t| t.name).collect(),
                current: REMOTE_THEMES
                    .iter()
                    .position(|t| t.id == active_theme.id)
                    .unwrap_or(0),
            },
        },
        HubItem {
            id: HubActionId::RemoteArtMode,
            cat_idx: 3,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Remote Character Art",
            keywords: "remote character portrait art companion blend poster minimal",
            badge: format!("[ {} #{:02} ▾ ]", cfg.remote.art_mode.to_ascii_uppercase(), art_slot),
            description: "Configure how bundled character artwork renders on the mobile remote (Companion Card, Atmospheric Blend, Hero Poster, or Minimal).",
            kind: HubActionKind::Cycle {
                options: vec!["companion", "blend", "poster", "off"],
                current: match cfg.remote.art_mode.as_str() {
                    "blend" => 1,
                    "poster" => 2,
                    "off" => 3,
                    _ => 0,
                },
            },
        },
        HubItem {
            id: HubActionId::RemoteQrModal,
            cat_idx: 3,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Mobile Remote QR Code",
            keywords: "mobile phone qr scan link remote browser pair",
            badge: "[ QR Modal ]".into(),
            description: "Display a scannable terminal QR code for your LAN IP address and jump straight into the mobile remote companion.",
            kind: HubActionKind::Launch("Show QR Code"),
        },
        HubItem {
            id: HubActionId::RemoteOpenBrowser,
            cat_idx: 3,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Open Remote in Browser",
            keywords: "mobile phone remote browser open local preview",
            badge: "[ Launch URL ]".into(),
            description: "Open the LAN companion web deck immediately in your desktop default web browser to preview themes and controls.",
            kind: HubActionKind::Execute("Open Browser"),
        },
        HubItem {
            id: HubActionId::CastScan,
            cat_idx: 3,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Scan & Beam to TV",
            keywords: "cast chromecast dlna airplay tv beam wireless",
            badge: "[ Launch Scanner ]".into(),
            description: "Scan local Wi-Fi for DLNA/UPnP Smart TVs and Chromecast receivers to beam active episodes directly to your television.",
            kind: HubActionKind::Launch("Scan Devices"),
        },

        // ── 4. CINEMATIC SUBTITLES ────────────────────────────────────────────
        HubItem {
            id: HubActionId::SubPreset,
            cat_idx: 4,
            category: "❖ CINEMATIC SUBTITLES",
            label: "Typography Preset",
            keywords: "subtitles font preset style netflix anime fansub oled",
            badge: format!("[ {} ▾ ]", cfg.subtitle_style.preset.label()),
            description: "Configure live libass typography presets rendered dynamically via mpv IPC or launch flags.",
            kind: HubActionKind::Cycle {
                options: vec!["Studio Minimal", "Netflix Clean", "Anime Fansub", "OLED Plate"],
                current: sub_preset_idx,
            },
        },
        HubItem {
            id: HubActionId::SubCollisionGuard,
            cat_idx: 4,
            category: "❖ CINEMATIC SUBTITLES",
            label: "Signage Collision Guard",
            keywords: "subtitles overlap signs typeset collision guard",
            badge: if cfg.subtitle_style.collision_avoidance { "[✓] Enabled".into() } else { "[ ] Disabled".into() },
            description: "Keep dialogue clear of OP/ED karaoke and on-screen signage typesets in mpv.",
            kind: HubActionKind::Toggle(cfg.subtitle_style.collision_avoidance),
        },

        // ── 5. SYSTEM & MAINTENANCE ───────────────────────────────────────────
        HubItem {
            id: HubActionId::CheckUpdates,
            cat_idx: 5,
            category: "❖ SYSTEM & MAINTENANCE",
            label: "Check for Solus Updates",
            keywords: "update self update version upgrade github release",
            badge: if state.is_checking_update {
                "[ Checking... ]".into()
            } else if let Some(ref u) = state.available_update {
                format!("[ {} New! ]", u.latest_version)
            } else {
                format!("[ v{} ]", env!("CARGO_PKG_VERSION"))
            },
            description: "Query GitHub Releases for newer Solus binary versions or Homebrew upgrade notifications.",
            kind: HubActionKind::Execute("Check Releases"),
        },
        HubItem {
            id: HubActionId::WhatsNewNotes,
            cat_idx: 5,
            category: "❖ SYSTEM & MAINTENANCE",
            label: "What's New & Release Notes",
            keywords: "whats new release notes changelog update fixes features plain english",
            badge: "[ View Notes ]".into(),
            description: "Open a plain-English, jargon-free popup summarizing what is new, improved, and fixed in this version of Solus.",
            kind: HubActionKind::Launch("Open What's New"),
        },
        HubItem {
            id: HubActionId::AutoUpdateMode,
            cat_idx: 5,
            category: "❖ SYSTEM & MAINTENANCE",
            label: "Auto-Update Policy",
            keywords: "auto update silent background install prompt",
            badge: match update_idx {
                0 => "[ Silent Install ▾ ]".into(),
                1 => "[ Notify Modal ▾ ]".into(),
                _ => "[ Disabled ▾ ]".into(),
            },
            description: "Choose whether startup automatically downloads new releases in the background, opens an interactive prompt, or stays manual.",
            kind: HubActionKind::Cycle {
                options: vec!["Silent Auto-Install", "Notify Modal", "Manual Only"],
                current: update_idx,
            },
        },
        HubItem {
            id: HubActionId::HealthDoctor,
            cat_idx: 5,
            category: "❖ SYSTEM & MAINTENANCE",
            label: "Dependency Health Doctor",
            keywords: "doctor diagnostics ani-cli mpv iina vlc ffmpeg binaries",
            badge: if state.has_ani_cli && state.has_any_player() { "[✓] Healthy".into() } else { "[!] Issues".into() },
            description: "Open runtime dependency doctor to inspect ani-cli, player binaries, yt-dlp, and SQLite health.",
            kind: HubActionKind::Launch("Open Doctor"),
        },
        HubItem {
            id: HubActionId::AniListSync,
            cat_idx: 5,
            category: "❖ SYSTEM & MAINTENANCE",
            label: "Connected Trackers & Sync",
            keywords: "anilist myanimelist mal sync account token oauth xml library",
            badge: match (state.auth.has_anilist(), state.auth.has_mal()) {
                (true, true)  => "[ AL + MAL ]".into(),
                (true, false) => "[ AniList ✓ ]".into(),
                (false, true) => "[ MAL ✓ ]".into(),
                (false, false) => "[ Setup ]".into(),
            },
            description: "Open Account Setup to link AniList or MyAnimeList via OAuth, configure write-back tokens, or import XML backups.",
            kind: HubActionKind::Launch("Open Setup"),
        },
        HubItem {
            id: HubActionId::OpenCalendar,
            cat_idx: 5,
            category: "❖ DISCOVERY & SCHEDULE",
            label: "Seasonal & Movie Calendar",
            keywords: "calendar schedule airing upcoming releases movies ova",
            badge: "[ Open View ]".into(),
            description: "Inspect upcoming simulcast airings, airing countdowns, and theatrical anime premieres.",
            kind: HubActionKind::Launch("Open Calendar"),
        },
    ]
}

/// Filter hub items by active category ribbon (`0` = All) and optional fuzzy search query.
///
/// When `state.settings_query` is non-empty, search spans all categories with higher weight
/// on `item.label` so `"cast"` ranks `CastScan` first and `"remote"` ranks `Remote*` first.
pub fn get_filtered_items(cfg: &config::Config, state: &AppState) -> Vec<HubItem> {
    let items = get_hub_items(cfg, state);
    let query = state.settings_query.trim();

    if query.is_empty() {
        if state.settings_category == 0 {
            return items;
        }
        return items
            .into_iter()
            .filter(|i| i.cat_idx == state.settings_category)
            .collect();
    }

    let q_lower = query.to_ascii_lowercase();
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, HubItem)> = items
        .into_iter()
        .filter_map(|item| {
            let label_lower = item.label.to_ascii_lowercase();
            let kw_lower = item.keywords.to_ascii_lowercase();
            let corpus = format!("{} {} {}", item.category, item.label, item.keywords);
            let fuzzy = matcher.fuzzy_match(&corpus, query);
            let exact_label = label_lower.contains(&q_lower);
            let exact_kw = kw_lower.contains(&q_lower);
            if let Some(base) = fuzzy {
                let bonus = if exact_label { 10_000 } else { 0 } + if exact_kw { 5_000 } else { 0 };
                Some((base + bonus, item))
            } else if exact_label || exact_kw {
                let bonus = if exact_label { 10_000 } else { 5_000 };
                Some((bonus, item))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    scored.into_iter().map(|(_, item)| item).collect()
}

/// Render Control Hub v2 (`[s]`) with responsive desktop and compact layouts.
pub fn render_overlay(frame: &mut Frame, state: &mut AppState, cfg: &config::Config) {
    let full = frame.area();
    let pct = modal_pct(full);
    let area = centered_rect(pct.0, pct.1, full);
    frame.render_widget(Clear, area);

    let theme = state.theme();
    let outer_block = Block::default()
        .title(Line::from(vec![
            Span::styled(
                " ◈ SOLUS // CONTROL HUB v2 ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", state.network_mode.short_tag()),
                Style::default()
                    .fg(net_mode_color(state.network_mode, &theme))
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus))
        .style(Style::default().bg(Color::Rgb(10, 11, 17)));

    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    if inner.width < 32 || inner.height < 12 {
        return;
    }

    let filtered = get_filtered_items(cfg, state);
    let selected_idx = state.settings_cursor.min(filtered.len().saturating_sub(1));
    let selected_item = filtered.get(selected_idx).cloned();

    let is_compact = full.width < 96 || full.height < 28;
    if is_compact {
        render_compact_hub(frame, inner, state, cfg, &filtered, selected_idx, selected_item.as_ref(), &theme);
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top Live Stream & Network Pulse
            Constraint::Length(3), // Category Ribbon + Search Strip
            Constraint::Min(10),   // Main 2-column stage
            Constraint::Length(1), // Footer Keybar
        ])
        .split(inner);

    render_pulse_banner(frame, rows[0], state, cfg, &theme);
    render_ribbon_and_search(frame, rows[1], state, &theme);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(54),
            Constraint::Percentage(46),
        ])
        .split(rows[2]);

    render_action_list_column(frame, cols[0], state, cfg, &filtered, selected_idx, &theme, true);

    let right_tiers = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Contextual Inspector
            Constraint::Length(9), // Persistent Live Telemetry & Session Monitor
        ])
        .split(cols[1]);

    render_inspector_card(frame, right_tiers[0], selected_item.as_ref(), state, cfg, &theme);
    render_live_telemetry_panel(frame, right_tiers[1], state, cfg, &theme);

    render_footer_keybar(frame, rows[3], &theme);
}

#[allow(clippy::too_many_arguments)]
fn render_compact_hub(
    frame: &mut Frame,
    inner: Rect,
    state: &mut AppState,
    cfg: &config::Config,
    filtered: &[HubItem],
    selected_idx: usize,
    selected_item: Option<&HubItem>,
    theme: &ThemePalette,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Single-line compact pulse
            Constraint::Length(3), // Search + active category badge
            Constraint::Min(7),    // Scrollable action list
            Constraint::Length(5), // Compact Contextual + Telemetry summary
            Constraint::Length(1), // Footer Keybar
        ])
        .split(inner);

    let pulse = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" [ {} ] ", state.network_mode.short_tag()),
            Style::default()
                .fg(net_mode_color(state.network_mode, theme))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "ABR:{}  IPv4:{}  |  {}",
                if cfg.auto_abr_downshift { "ON" } else { "OFF" },
                if cfg.force_ipv4 { "ON" } else { "OFF" },
                if state.live_playback.started_at.is_some() {
                    state.live_playback.compact_status_text()
                } else {
                    format!("Remote :{}", cfg.remote.port)
                }
            ),
            Style::default().fg(theme.text_dim),
        ),
    ]));
    frame.render_widget(pulse, rows[0]);

    render_ribbon_and_search(frame, rows[1], state, theme);
    render_action_list_column(frame, rows[2], state, cfg, filtered, selected_idx, theme, false);

    let summary_block = Block::default()
        .title(" INSPECTOR & TELEMETRY ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle));
    let s_inner = summary_block.inner(rows[3]);
    frame.render_widget(summary_block, rows[3]);

    let desc_line = selected_item
        .map(|it| format!("{} — {}", it.label, it.description))
        .unwrap_or_else(|| "Type to search settings or press [N] for Network Mode.".to_string());
    let summary_text = vec![
        Line::from(Span::styled(
            truncate_chars(&desc_line, s_inner.width as usize),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            truncate_chars(&build_telemetry_line(state, cfg), s_inner.width as usize),
            Style::default().fg(theme.accent),
        )),
    ];
    frame.render_widget(Paragraph::new(summary_text), s_inner);
    render_footer_keybar(frame, rows[4], theme);
}

fn net_mode_color(mode: NetworkMode, theme: &ThemePalette) -> Color {
    match mode {
        NetworkMode::Normal        => theme.primary,
        NetworkMode::CongestedWifi => Color::Rgb(245, 185, 66),
        NetworkMode::UltraSaver    => Color::Rgb(80, 220, 145),
    }
}

fn net_mode_profile_summary(mode: NetworkMode) -> &'static str {
    match mode {
        NetworkMode::Normal        => "Uncapped · 45s buf",
        NetworkMode::CongestedWifi => "<=720p30 · 120s Disk",
        NetworkMode::UltraSaver    => "<=480p30 · 180s Disk",
    }
}

fn render_pulse_banner(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    cfg: &config::Config,
    theme: &ThemePalette,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle))
        .style(Style::default().bg(Color::Rgb(14, 16, 25)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let net_color = net_mode_color(state.network_mode, theme);
    let line = Line::from(vec![
        Span::styled(
            format!(" [ {} ] ", state.network_mode.short_tag()),
            Style::default().fg(Color::Black).bg(net_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", net_mode_profile_summary(state.network_mode)),
            Style::default().fg(theme.text),
        ),
        Span::styled(" | ", Style::default().fg(theme.divider)),
        Span::styled(
            format!(
                "Auto-ABR: {}",
                if cfg.auto_abr_downshift { "ON" } else { "OFF" }
            ),
            Style::default().fg(if cfg.auto_abr_downshift { theme.accent } else { theme.text_dim }),
        ),
        Span::styled(" | ", Style::default().fg(theme.divider)),
        Span::styled(
            format!("IPv4: {}", if cfg.force_ipv4 { "ON" } else { "OFF" }),
            Style::default().fg(if cfg.force_ipv4 { Color::Rgb(245, 185, 66) } else { theme.text_dim }),
        ),
        Span::styled(" | ", Style::default().fg(theme.divider)),
        Span::styled(
            format!(
                "Remote: {} ({} cli) ",
                if cfg.remote.enabled {
                    format!(":{}", cfg.remote.port)
                } else {
                    "OFF".to_string()
                },
                state.remote_ws_clients
            ),
            Style::default().fg(theme.secondary),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), inner);
}

fn render_ribbon_and_search(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    theme: &ThemePalette,
) {
    let active_search = !state.settings_query.is_empty();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if active_search { theme.primary } else { theme.border_idle }))
        .style(Style::default().bg(Color::Rgb(13, 15, 23)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(62),
            Constraint::Percentage(38),
        ])
        .split(inner);

    // Left: Category Tabs
    let mut tab_spans: Vec<Span<'static>> = vec![Span::raw(" ")];
    let mut used = 1usize;
    let budget = cols[0].width as usize;
    for (idx, label) in SETTINGS_CATEGORIES.iter().enumerate() {
        let active = active_search && idx == 0 || (!active_search && state.settings_category == idx);
        let pill = format!("[{idx}]{label}");
        let pill_w = pill.chars().count() + 1;
        if used + pill_w > budget {
            break;
        }
        if active {
            tab_spans.push(Span::styled(
                format!(" {idx}:{label} "),
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ));
            tab_spans.push(Span::raw(" "));
        } else {
            tab_spans.push(Span::styled(
                format!(" {idx}:{label}"),
                Style::default().fg(theme.text_dim),
            ));
            tab_spans.push(Span::raw(" "));
        }
        used += pill_w + 1;
    }
    frame.render_widget(Paragraph::new(Line::from(tab_spans)), cols[0]);

    // Right: Search Input
    let q_spans = if state.settings_query.is_empty() {
        vec![
            Span::styled("│ ", Style::default().fg(theme.divider)),
            Span::styled("Filter: ", Style::default().fg(theme.secondary)),
            Span::styled("type to search…", Style::default().fg(theme.text_dim)),
        ]
    } else {
        vec![
            Span::styled("│ ", Style::default().fg(theme.divider)),
            Span::styled("Filter: ", Style::default().fg(theme.primary)),
            Span::styled(
                state.settings_query.clone(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled("_", Style::default().fg(theme.primary)),
        ]
    };
    frame.render_widget(Paragraph::new(Line::from(q_spans)), cols[1]);
}

#[allow(clippy::too_many_arguments)]
fn render_action_list_column(
    frame: &mut Frame,
    area: Rect,
    state: &mut AppState,
    cfg: &config::Config,
    filtered: &[HubItem],
    selected_idx: usize,
    theme: &ThemePalette,
    allow_summary_fill: bool,
) {
    // Count how many visual rows items take (including category headers when category == All and query empty)
    let show_headers = state.settings_query.is_empty();
    let total_entries = filtered.len();
    let estimated_rows = if show_headers {
        let mut cats = 0usize;
        let mut last = "";
        for it in filtered {
            if it.category != last {
                cats += 1;
                last = it.category;
            }
        }
        total_entries + cats
    } else {
        total_entries
    };

    let spare_rows = area.height.saturating_sub(2).saturating_sub(estimated_rows as u16);
    let (list_area, summary_opt) = if allow_summary_fill && spare_rows >= 5 && area.height >= 14 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(7), Constraint::Length(5)])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let list_block = Block::default()
        .title(Span::styled(
            format!(" ACTIONS ({}) ", filtered.len()),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle));
    let inner = list_block.inner(list_area);
    frame.render_widget(list_block, list_area);

    if filtered.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  No matching commands. Press Backspace or Esc.",
                Style::default().fg(theme.text_dim),
            )),
            inner,
        );
    } else if inner.height > 0 && inner.width > 4 {
        // Flatten display rows (headers + items) tracking row index of selected item
        #[derive(Clone)]
        enum RowKind<'a> {
            Header(&'static str),
            Item(usize, &'a HubItem),
        }

        let mut rows: Vec<RowKind<'_>> = Vec::with_capacity(filtered.len() + 6);
        let mut last_cat = "";
        let mut selected_visual_row = 0usize;
        for (idx, item) in filtered.iter().enumerate() {
            if show_headers && item.category != last_cat {
                rows.push(RowKind::Header(item.category));
                last_cat = item.category;
            }
            if idx == selected_idx {
                selected_visual_row = rows.len();
            }
            rows.push(RowKind::Item(idx, item));
        }

        let cap = inner.height as usize;
        if selected_visual_row < state.settings_scroll_offset {
            state.settings_scroll_offset = selected_visual_row;
        } else if selected_visual_row >= state.settings_scroll_offset + cap {
            state.settings_scroll_offset = selected_visual_row + 1 - cap;
        }
        // Keep header directly above first visible item visible when scrolling to top
        if selected_visual_row == 1
            && matches!(rows.first(), Some(RowKind::Header(_)))
            && state.settings_scroll_offset == 1
        {
            state.settings_scroll_offset = 0;
        }
        let max_scroll = rows.len().saturating_sub(cap);
        if state.settings_scroll_offset > max_scroll {
            state.settings_scroll_offset = max_scroll;
        }

        let need_scrollbar = rows.len() > cap;
        let text_width = inner.width.saturating_sub(if need_scrollbar { 2 } else { 0 }) as usize;
        let list_rect = Rect::new(inner.x, inner.y, text_width as u16, inner.height);

        let lines: Vec<Line<'static>> = rows
            .iter()
            .skip(state.settings_scroll_offset)
            .take(cap)
            .map(|rk| match rk {
                RowKind::Header(cat) => Line::from(Span::styled(
                    format!(" {}", truncate_chars(cat, text_width.saturating_sub(2))),
                    Style::default()
                        .fg(theme.secondary)
                        .add_modifier(Modifier::BOLD),
                )),
                RowKind::Item(idx, item) => {
                    let is_sel = *idx == selected_idx;
                    let prefix = if is_sel { " ▸ " } else { "   " };
                    let badge_str = format!(" {} ", item.badge);
                    let badge_w = badge_str.chars().count();
                    let label_budget = text_width.saturating_sub(3 + 2 + badge_w);
                    let label = truncate_chars(item.label, label_budget);
                    let label_w = label.chars().count();
                    let pad = text_width
                        .saturating_sub(3 + label_w + badge_w)
                        .max(1);

                    let row_bg = if is_sel { Color::Rgb(28, 32, 48) } else { Color::Reset };
                    let label_fg = if is_sel { Color::White } else { theme.text };
                    let badge_fg = match &item.kind {
                        HubActionKind::Toggle(true)  => Color::Rgb(110, 231, 135),
                        HubActionKind::Toggle(false) => theme.text_dim,
                        HubActionKind::Cycle { .. }  => Color::Rgb(245, 195, 85),
                        HubActionKind::Launch(_)     => theme.primary,
                        HubActionKind::Execute(_)    => Color::Rgb(135, 200, 255),
                    };
                    Line::from(vec![
                        Span::styled(
                            prefix,
                            Style::default()
                                .fg(theme.primary)
                                .bg(row_bg)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            label,
                            Style::default()
                                .fg(label_fg)
                                .bg(row_bg)
                                .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() }),
                        ),
                        Span::styled(" ".repeat(pad), Style::default().bg(row_bg)),
                        Span::styled(
                            badge_str,
                            Style::default()
                                .fg(badge_fg)
                                .bg(row_bg)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ])
                }
            })
            .collect();

        frame.render_widget(Paragraph::new(lines), list_rect);

        if need_scrollbar {
            let sb_x = inner.x + inner.width.saturating_sub(1);
            let thumb_h = ((cap * cap) / rows.len().max(1)).clamp(1, cap);
            let thumb_top = (state.settings_scroll_offset * (cap - thumb_h))
                .checked_div(max_scroll)
                .unwrap_or(0);
            for y in 0..cap {
                let ch = if y >= thumb_top && y < thumb_top + thumb_h {
                    "┃"
                } else {
                    "│"
                };
                let fg = if ch == "┃" { theme.primary } else { theme.divider };
                frame.render_widget(
                    Paragraph::new(Span::styled(ch, Style::default().fg(fg))),
                    Rect::new(sb_x, inner.y + y as u16, 1, 1),
                );
            }
        }
    }

    if let Some(summary_area) = summary_opt {
        render_active_profile_summary(frame, summary_area, state, cfg, theme);
    }
}

fn render_active_profile_summary(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    cfg: &config::Config,
    theme: &ThemePalette,
) {
    let block = Block::default()
        .title(" ACTIVE STACK & SHORTCUTS ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let r_theme = find_remote_theme(&cfg.remote.theme_id);
    let art_idx = cfg.remote.art_index.unwrap_or(r_theme.default_art_idx);
    let (_bytes, art_tag) = raw_y2k_art(art_idx);
    let lines = vec![
        Line::from(vec![
            Span::styled(" Profile: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!("{} {}", state.active_profile_icon, state.active_profile_name),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  Player: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                cfg.player.as_str().to_uppercase(),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Remote Skin: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!("{} ({}) · Art: {}", r_theme.name, r_theme.genre, art_tag),
                Style::default().fg(theme.secondary),
            ),
        ]),
        Line::from(Span::styled(
            " Tips: Press [N] to cycle network profiles · [Tab / 0-5] filter tabs",
            Style::default().fg(theme.text_dim),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_inspector_card(
    frame: &mut Frame,
    area: Rect,
    item_opt: Option<&HubItem>,
    state: &AppState,
    cfg: &config::Config,
    theme: &ThemePalette,
) {
    let block = Block::default()
        .title(" ◈ CONTEXTUAL INSPECTOR ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus))
        .style(Style::default().bg(Color::Rgb(13, 15, 22)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(item) = item_opt else {
        frame.render_widget(
            Paragraph::new("No setting selected.").style(Style::default().fg(theme.text_dim)),
            inner,
        );
        return;
    };

    let mut lines: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::styled(
                item.category,
                Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  ", Style::default().fg(theme.divider)),
            Span::styled(
                item.badge.clone(),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            item.label,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            item.description,
            Style::default().fg(theme.text),
        )),
        Line::from(""),
    ];

    match item.id {
        HubActionId::NetworkMode
        | HubActionId::AutoAbrDownshift
        | HubActionId::ForceIpv4
        | HubActionId::ManualAbrStepDown => {
            lines.extend(render_network_matrix_lines(state.network_mode, theme));
        }
        HubActionId::RemoteTheme | HubActionId::RemoteArtMode => {
            let meta = find_remote_theme(&cfg.remote.theme_id);
            let art_idx = cfg.remote.art_index.unwrap_or(meta.default_art_idx);
            let (_bytes, art_tag) = raw_y2k_art(art_idx);
            let bg = Color::Rgb(meta.swatch_bg.0, meta.swatch_bg.1, meta.swatch_bg.2);
            let acc = Color::Rgb(meta.swatch_accent.0, meta.swatch_accent.1, meta.swatch_accent.2);

            lines.push(Line::from(vec![
                Span::styled("Theme: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!("{} ({})", meta.name, meta.genre),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("  ", Style::default().bg(bg)),
                Span::raw(" "),
                Span::styled("  ", Style::default().bg(acc)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Shape & Icons: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!("{} · {} SVG pack", meta.shape_style.to_ascii_uppercase(), meta.icon_pack),
                    Style::default().fg(theme.secondary),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Portrait Mode: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!(
                        "{} (#{:02} {})",
                        cfg.remote.art_mode.to_ascii_uppercase(),
                        (art_idx % Y2K_ART_COUNT) + 1,
                        art_tag
                    ),
                    Style::default().fg(theme.accent),
                ),
            ]));
        }
        HubActionId::SubPreset => {
            lines.push(Line::from(vec![
                Span::styled("Font & Scale: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!(
                        "{} · {}pt · Border {:.1}px",
                        cfg.subtitle_style.font_family,
                        cfg.subtitle_style.font_size,
                        cfg.subtitle_style.border_size
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
        HubActionId::AutoFullscreen => {
            lines.push(Line::from(vec![
                Span::styled("Player Flag: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    match cfg.player {
                        config::Player::Celluloid => "--mpv-fullscreen=yes (GTK libmpv)",
                        config::Player::AutoBest | config::Player::Mpv | config::Player::MpvNet => {
                            "--fullscreen=yes (live IPC synced)"
                        }
                    },
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Shortcut   : ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    "Press [f] inside the player anytime to toggle fullscreen",
                    Style::default().fg(theme.accent),
                ),
            ]));
        }
        HubActionId::PlaybackEngine
        | HubActionId::PerformanceMode
        | HubActionId::Anime4kMode
        | HubActionId::DebandMode
        | HubActionId::MotionMode => {
            let dev = crate::services::platform::detect_device_profile();
            let caps = crate::services::platform::detect_player_capabilities();
            lines.push(Line::from(vec![
                Span::styled("Host Silicon: ", Style::default().fg(theme.text_dim)),
                Span::styled(dev.inspector_header(), Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("HW & Video  : ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!("{} · {}", dev.recommended_hwdec, caps.inspector_summary()),
                    Style::default().fg(theme.accent),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Engine Parity: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!("{} ({}) · Bounded RAM: 72 MiB", cfg.player.label(), cfg.player.parity_badge()),
                    Style::default().fg(theme.secondary),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("In-Player Keys: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    "[a] Shaders  [d] Deband  [m] Motion  [g] Perf  [i/I] HUD  [p/P] PiP",
                    Style::default().fg(Color::Rgb(120, 235, 175)),
                ),
            ]));
        }
        HubActionId::PipMode | HubActionId::PipCorner => {
            let corner_lbl = crate::services::ipc::PIP_CORNER_LABELS
                [state.pip_corner_idx % crate::services::ipc::PIP_CORNER_LABELS.len()];
            lines.push(Line::from(vec![
                Span::styled("Window Mode: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!("Borderless Always-On-Top · {corner_lbl}"),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Player Keys: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    "[p] Toggle PiP Float   [P] Cycle Corner & Size",
                    Style::default().fg(theme.accent),
                ),
            ]));
        }
        HubActionId::ToggleInfoHud => {
            lines.push(Line::from(vec![
                Span::styled("Overlay    : ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    "Multi-Color ASS Bento Card (Video · Audio · Cache)",
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Player Keys: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    "[i] 6s Overlay   [I] Pin 1Hz Live   [F1] Raw Stats",
                    Style::default().fg(theme.accent),
                ),
            ]));
        }
        HubActionId::HealthDoctor => {
            let dev = crate::services::platform::detect_device_profile();
            let caps = crate::services::platform::detect_player_capabilities();
            lines.push(Line::from(vec![
                Span::styled("Engines : ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!(
                        "ani-cli={} · mpv={} · mpv.net={} · celluloid={}",
                        if state.has_ani_cli { "OK" } else { "NO" },
                        if state.has_mpv { "OK" } else { "NO" },
                        if state.has_mpvnet { "OK" } else { "NO" },
                        if state.has_celluloid { "OK" } else { "NO" },
                    ),
                    Style::default().fg(theme.text),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Pipeline: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!("{} · {}", dev.recommended_hwdec, caps.effective_vo()),
                    Style::default().fg(theme.accent),
                ),
            ]));
        }
        _ => {}
    }

    lines.push(Line::from(""));
    let control_hint = match &item.kind {
        HubActionKind::Cycle { .. } => "Controls: [← / → / Enter / Space] cycle options",
        HubActionKind::Toggle(_)    => "Controls: [Enter / Space / ← / →] toggle state",
        HubActionKind::Launch(_)    => "Controls: [Enter] open panel",
        HubActionKind::Execute(_)   => "Controls: [Enter] run immediately",
    };
    lines.push(Line::from(Span::styled(
        control_hint,
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_network_matrix_lines(active: NetworkMode, theme: &ThemePalette) -> Vec<Line<'static>> {
    let style_for = |m: NetworkMode| -> Style {
        if m == active {
            Style::default()
                .fg(net_mode_color(m, theme))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_dim)
        }
    };
    let rows = [
        ("Setting    ", "FULL     ", "WIFI     ", "ECO      "),
        ("Height/FPS ", "Uncapped ", "<=720p30 ", "<=480p30 "),
        ("Readahead  ", "45s RAM  ", "120s Disk", "180s Disk"),
        ("Thumbnails ", "HD / 12  ", "Compact/3", "Compact/1"),
    ];
    let mut out = Vec::with_capacity(rows.len());
    for (idx, (k, full, wifi, eco)) in rows.into_iter().enumerate() {
        let base = if idx == 0 {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_dim)
        };
        out.push(Line::from(vec![
            Span::styled(k, base),
            Span::styled(" | ", Style::default().fg(theme.divider)),
            Span::styled(full, style_for(NetworkMode::Normal)),
            Span::styled(" | ", Style::default().fg(theme.divider)),
            Span::styled(wifi, style_for(NetworkMode::CongestedWifi)),
            Span::styled(" | ", Style::default().fg(theme.divider)),
            Span::styled(eco, style_for(NetworkMode::UltraSaver)),
        ]));
    }
    out
}

fn build_telemetry_line(state: &AppState, cfg: &config::Config) -> String {
    if state.live_playback.started_at.is_some() {
        state.live_playback.compact_status_text()
    } else {
        format!(
            "Idle · {} · Remote :{} ({} cli)",
            state.network_mode.short_tag(),
            cfg.remote.port,
            state.remote_ws_clients
        )
    }
}

fn render_live_telemetry_panel(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    cfg: &config::Config,
    theme: &ThemePalette,
) {
    let block = Block::default()
        .title(" ● LIVE STREAM & TELEMETRY MONITOR ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if state.live_playback.paused_for_cache {
            Color::Rgb(245, 185, 66)
        } else {
            theme.border_idle
        }))
        .style(Style::default().bg(Color::Rgb(11, 13, 20)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let tp = &state.live_playback;
    let session_text = if let Some(aid) = tp.anime_id {
        let title = state
            .selected_anime
            .as_ref()
            .filter(|a| a.id == aid)
            .map(|a| a.display_title().to_string())
            .unwrap_or_else(|| format!("Anime #{}", aid));
        format!(
            "{} · Ep {} · {}p · {}",
            truncate_chars(&title, 22),
            tp.episode,
            tp.current_height,
            tp.player.to_ascii_uppercase()
        )
    } else {
        "Idle — Launch an episode for live buffer telemetry".to_string()
    };

    let ladder_text = tp
        .anime_id
        .and_then(|aid| state.stream_ladder_cache.get(&(aid, tp.episode, tp.is_dub)))
        .map(|l| {
            let rungs = l.ladder_summary_rungs((tp.current_height > 0).then_some(tp.current_height));
            if rungs.is_empty() {
                "Auto".to_string()
            } else {
                rungs
                    .into_iter()
                    .map(|(h, active)| if active { format!("[{h}p]") } else { format!("{h}p") })
                    .collect::<Vec<_>>()
                    .join(" · ")
            }
        })
        .unwrap_or_else(|| "None cached (pre-resolves on playback)".to_string());

    let fill_ratio = (tp.cache_duration_secs / (state.network_mode.vod_readahead_secs() as f64).max(1.0))
        .clamp(0.0, 1.0);
    let bar_slots = 12usize;
    let filled = (fill_ratio * bar_slots as f64).round() as usize;
    let gauge = format!("{}{}", "█".repeat(filled), "░".repeat(bar_slots - filled));

    let abr_note = tp
        .last_abr_note
        .as_deref()
        .unwrap_or("Ready (triggers on >=2 stalls/60s or <60% throughput)");

    let bg_mode = if tp.paused_for_cache {
        "PAUSED (mpv buffering)".to_string()
    } else {
        format!(
            "{} concurrent · {}",
            state.network_mode.max_bg_image_concurrency(tp.started_at.is_some()),
            if state.network_mode.prefer_compact_artwork() {
                "Compact Artwork"
            } else {
                "Full HD Artwork"
            }
        )
    };

    let r_theme = find_remote_theme(&cfg.remote.theme_id);
    let lines = vec![
        Line::from(vec![
            Span::styled("Session : ", Style::default().fg(theme.text_dim)),
            Span::styled(session_text, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Ladder  : ", Style::default().fg(theme.text_dim)),
            Span::styled(truncate_chars(&ladder_text, inner.width.saturating_sub(11) as usize), Style::default().fg(theme.secondary)),
        ]),
        Line::from(vec![
            Span::styled("Buffer  : ", Style::default().fg(theme.text_dim)),
            Span::styled(format!("[{gauge}] "), Style::default().fg(theme.accent)),
            Span::styled(tp.compact_status_text(), Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Auto-ABR: ", Style::default().fg(theme.text_dim)),
            Span::styled(truncate_chars(abr_note, inner.width.saturating_sub(11) as usize), Style::default().fg(Color::Rgb(245, 195, 85))),
        ]),
        Line::from(vec![
            Span::styled("BG Art  : ", Style::default().fg(theme.text_dim)),
            Span::styled(bg_mode, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Remote  : ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!(
                    "{} · {} cli · {}",
                    state.remote_url, state.remote_ws_clients, r_theme.name
                ),
                Style::default().fg(theme.primary),
            ),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_footer_keybar(frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    let hints = Line::from(vec![
        Span::styled(" [↑↓] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Navigate", Style::default().fg(theme.text_dim)),
        Span::styled("   [Tab / 0-5] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Category", Style::default().fg(theme.text_dim)),
        Span::styled("   [←/→/Enter] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Toggle", Style::default().fg(theme.text_dim)),
        Span::styled("   [N] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Net Mode", Style::default().fg(theme.text_dim)),
        Span::styled("   [Esc] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Close ", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(
        Paragraph::new(hints).alignment(Alignment::Center),
        area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn test_hub_items_do_not_contain_redundant_selectors() {
        let cfg = config::Config::default();
        let state = AppState::new();
        let items = get_hub_items(&cfg, &state);
        for item in items {
            let l = item.label.to_lowercase();
            assert!(!l.contains("player binary"), "redundant item leaked: {}", item.label);
            assert!(!l.contains("preferred quality"), "redundant item leaked: {}", item.label);
            assert!(!l.contains("preferred audio"), "redundant item leaked: {}", item.label);
            assert!(!l.contains("subtitle language"), "redundant item leaked: {}", item.label);
        }
    }

    #[test]
    fn test_hub_items_contain_new_major_features() {
        let cfg = config::Config::default();
        let state = AppState::new();
        let labels: Vec<&str> = get_hub_items(&cfg, &state).iter().map(|i| i.label).collect();
        assert!(labels.iter().any(|l| l.contains("Typography Preset")));
        assert!(labels.iter().any(|l| l.contains("Signage Collision Guard")));
        assert!(labels.iter().any(|l| l.contains("Mobile Remote Daemon")));
        assert!(labels.iter().any(|l| l.contains("Mobile Remote QR Code")));
        assert!(labels.iter().any(|l| l.contains("Scan & Beam to TV")));
        assert!(labels.iter().any(|l| l.contains("Anime4K")));
        assert!(labels.iter().any(|l| l.contains("Adaptive Network Mode")));
        assert!(labels.iter().any(|l| l.contains("Mobile Remote Theme")));
    }

    #[test]
    fn test_hub_filter_matches_and_ranks() {
        let cfg = config::Config::default();
        let mut state = AppState::new();

        state.settings_query = "cast".to_string();
        let res_cast = get_filtered_items(&cfg, &state);
        assert!(!res_cast.is_empty());
        assert_eq!(res_cast[0].id, HubActionId::CastScan);

        state.settings_query = "remote".to_string();
        let res_remote = get_filtered_items(&cfg, &state);
        assert!(res_remote.len() >= 3);
        assert!(res_remote[0].label.to_lowercase().contains("remote"));

        state.settings_query = "sub".to_string();
        let res_sub = get_filtered_items(&cfg, &state);
        assert!(res_sub.iter().any(|i| i.label.contains("Preset") || i.label.contains("Typography")));
    }

    #[test]
    fn test_settings_hub_v2_desktop_and_compact_render_without_concatenation() {
        let cfg = config::Config::default();
        let mut state = AppState::new();
        state.network_mode = NetworkMode::CongestedWifi;
        state.remote_url = "http://192.168.1.44:8484".to_string();
        state.live_playback.reset_for_session(1, 3, false, false, "mpv", 720, 300, 1450, "https://cdn/ep3.m3u8");
        state.live_playback.cache_duration_secs = 28.0;

        let mut desktop_term = Terminal::new(TestBackend::new(140, 42)).unwrap();
        desktop_term
            .draw(|f| render_overlay(f, &mut state, &cfg))
            .unwrap();
        let buf = desktop_term.backend().buffer();
        let mut all_text = String::new();
        for y in 0..42 {
            for x in 0..140 {
                let sym = buf[(x, y)].symbol();
                for ch in sym.chars() {
                    let cp = ch as u32;
                    assert!(!(0x1F300..=0x1FAFF).contains(&cp), "Emoji leaked in settings: {ch}");
                }
                all_text.push_str(sym);
            }
            all_text.push('\n');
        }
        assert!(all_text.contains("CONTROL HUB v2"));
        assert!(all_text.contains("NET:WIFI"));
        assert!(all_text.contains("CONTEXTUAL INSPECTOR"));
        assert!(all_text.contains("LIVE STREAM & TELEMETRY MONITOR"));
        assert!(!all_text.contains("NavigateCategory"));

        let mut compact_term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        compact_term
            .draw(|f| render_overlay(f, &mut state, &cfg))
            .unwrap();
    }
}
