//! Split-Deck Fuzzy Feature Command Hub & Settings Palette.
//! Replaces the static preferences form with an omni-searchable control center
//! allowing users to discover, configure, and launch features without cryptic keybinds.

use crate::config;
use crate::services::substyler::SubtitlePreset;
use crate::state::AppState;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Identifiers for every dispatchable feature and preference in Solus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubActionId {
    SubPreset,
    SubCollisionGuard,
    CastScan,
    RemoteQrModal,
    RemoteOpenBrowser,
    RemoteToggleServer,
    Anime4kMode,
    AmbilightGlow,
    AutoSkipIntro,
    CheckUpdates,
    AutoUpdateMode,
    HealthDoctor,
    AniListSync,
}

/// The interaction type of each hub action.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum HubActionKind {
    Cycle { options: Vec<&'static str>, current: usize },
    Toggle(bool),
    Launch(&'static str),
    Execute(&'static str),
}

/// A searchable feature action entry in the command hub.
#[derive(Debug, Clone)]
pub struct HubItem {
    pub id: HubActionId,
    pub category: &'static str,
    pub label: &'static str,
    pub keywords: &'static str,
    pub badge: String,
    pub description: &'static str,
    pub kind: HubActionKind,
}

/// Assembles the complete catalog of feature actions from current runtime state.
pub fn get_hub_items(cfg: &config::Config, state: &AppState) -> Vec<HubItem> {
    let sub_preset_idx = match cfg.subtitle_style.preset {
        SubtitlePreset::StudioMinimal => 0,
        SubtitlePreset::NetflixClean  => 1,
        SubtitlePreset::AnimeFansub   => 2,
        SubtitlePreset::OledPlate     => 3,
        SubtitlePreset::Custom        => 0,
    };

    let anime4k_idx = match cfg.anime4k_mode {
        config::Anime4KMode::Off => 0,
        config::Anime4KMode::Balanced => 1,
        config::Anime4KMode::Ultra => 2,
    };

    let update_idx = if !cfg.update.check_on_startup {
        2
    } else if cfg.update.auto_install {
        1
    } else {
        0
    };

    vec![
        // ── ❖ CINEMATIC SUBTITLES ──────────────────────────────────────────
        HubItem {
            id: HubActionId::SubPreset,
            category: "❖ CINEMATIC SUBTITLES",
            label: "Typography Preset",
            keywords: "sub subtitle font styling preset appearance libass text minimal netflix fansub oled",
            badge: format!("[ {} ▾ ]", cfg.subtitle_style.preset.label()),
            description: "High-legibility typography presets with custom stroke outlines, drop shadows, and opacity plates.",
            kind: HubActionKind::Cycle {
                options: vec!["Studio Minimal", "Netflix Clean", "Anime Fansub", "OLED Plate"],
                current: sub_preset_idx,
            },
        },
        HubItem {
            id: HubActionId::SubCollisionGuard,
            category: "❖ CINEMATIC SUBTITLES",
            label: "Signage Collision Guard",
            keywords: "collision signage signs overlap lift avoid text bottom japanese",
            badge: if cfg.subtitle_style.collision_avoidance { "[󰄬] Enabled".to_string() } else { "[ ] Disabled".to_string() },
            description: "Auto-detects bottom-screen Japanese signage translations and shifts dialogue upward to eliminate overlapping text.",
            kind: HubActionKind::Toggle(cfg.subtitle_style.collision_avoidance),
        },

        // ── ❖ TELEVISION & MOBILE REMOTE ───────────────────────────────────
        HubItem {
            id: HubActionId::CastScan,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Scan & Beam to TV",
            keywords: "cast tv television airplay dlna upnp apple stream living room screen monitor",
            badge: "[ AirPlay / DLNA ]".to_string(),
            description: "Scans local Wi-Fi for Apple TV, LG webOS, Samsung Tizen, and Sony TVs to cast pristine 1080p/4K streams directly.",
            kind: HubActionKind::Launch("CastModal"),
        },
        HubItem {
            id: HubActionId::RemoteQrModal,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Mobile Remote QR Code",
            keywords: "remote phone qr mobile companion touch web wifi lan couch bed",
            badge: "[ QR Code ◈ ]".to_string(),
            description: "Displays a scannable ASCII QR code to control playback, volume, and episode switching from your smartphone.",
            kind: HubActionKind::Launch("RemoteModal"),
        },
        HubItem {
            id: HubActionId::RemoteOpenBrowser,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Open Remote in Browser",
            keywords: "browser web remote open http link localhost chrome safari",
            badge: "[ Port 8484 ]".to_string(),
            description: "Opens the zero-dependency OLED touch client in your local web browser.",
            kind: HubActionKind::Execute("OpenBrowser"),
        },
        HubItem {
            id: HubActionId::RemoteToggleServer,
            category: "❖ TELEVISION & MOBILE REMOTE",
            label: "Mobile Remote Daemon",
            keywords: "server daemon listener port 8484 background mobile lan",
            badge: if cfg.remote.enabled { "[󰄬] Active (8484)".to_string() } else { "[ ] Disabled".to_string() },
            description: "Toggles the background lightweight Tokio HTTP server on local port 8484.",
            kind: HubActionKind::Toggle(cfg.remote.enabled),
        },

        // ── ❖ VISUAL & PLAYBACK ENGINE ─────────────────────────────────────
        HubItem {
            id: HubActionId::Anime4kMode,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Anime4K Shader Engine",
            keywords: "anime4k gpu shader upscale 4k 2x balanced ultra compute glsl mpv",
            badge: format!("[ {} ▾ ]", cfg.anime4k_mode.as_str()),
            description: "Direct OS GPU GLSL shaders reconstructing line-art and upscaling compressed streams to 4K in real time.",
            kind: HubActionKind::Cycle {
                options: vec!["off", "balanced 2x", "ultra 4k"],
                current: anime4k_idx,
            },
        },
        HubItem {
            id: HubActionId::AmbilightGlow,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Adaptive Ambilight Glow",
            keywords: "ambilight color theme palette border glow dynamic dynamicimage hsl",
            badge: if cfg.ambilight_enabled { "[󰄬] Enabled".to_string() } else { "[ ] Disabled".to_string() },
            description: "Samples RGB color histograms from anime cover posters to dynamically tint Solus terminal borders per show.",
            kind: HubActionKind::Toggle(cfg.ambilight_enabled),
        },
        HubItem {
            id: HubActionId::AutoSkipIntro,
            category: "❖ VISUAL & PLAYBACK ENGINE",
            label: "Auto-Skip OP/ED (AniSkip)",
            keywords: "aniskip skip intro outro opening ending theme automated",
            badge: if cfg.auto_skip_intro { "[󰄬] Enabled".to_string() } else { "[ ] Disabled".to_string() },
            description: "Queries public AniSkip API timestamps to seamlessly jump over opening and ending musical sequences.",
            kind: HubActionKind::Toggle(cfg.auto_skip_intro),
        },

        // ── ❖ SYSTEM & MAINTENANCE ─────────────────────────────────────────
        HubItem {
            id: HubActionId::CheckUpdates,
            category: "❖ SYSTEM & MAINTENANCE",
            label: "Check for Solus Updates",
            keywords: "update github release check version download upgrade",
            badge: if state.is_checking_update {
                "Checking... 󰑋".to_string()
            } else if let Some(ref u) = state.available_update {
                format!("New: {} 󰄬", u.latest_version)
            } else {
                format!("v{} (Latest)", env!("CARGO_PKG_VERSION"))
            },
            description: "Polls GitHub Releases for the latest binary and atomically replaces the executable in place.",
            kind: HubActionKind::Execute("CheckUpdate"),
        },
        HubItem {
            id: HubActionId::AutoUpdateMode,
            category: "❖ SYSTEM & MAINTENANCE",
            label: "Auto-Update Policy",
            keywords: "auto update startup channel install check background",
            badge: match update_idx {
                0 => "[ Check Only ▾ ]".to_string(),
                1 => "[ Auto-Install ▾ ]".to_string(),
                _ => "[ Off ▾ ]".to_string(),
            },
            description: "Configures whether Solus checks for updates on startup or installs new releases automatically.",
            kind: HubActionKind::Cycle {
                options: vec!["check only", "auto-install", "off"],
                current: update_idx,
            },
        },
        HubItem {
            id: HubActionId::HealthDoctor,
            category: "❖ SYSTEM & MAINTENANCE",
            label: "Dependency Health Doctor",
            keywords: "doctor dependencies setup ani-cli mpv iina vlc check health",
            badge: if state.has_ani_cli && state.has_any_player() { "[󰄬] Healthy".to_string() } else { "[!] Issues".to_string() },
            description: "Validates terminal prerequisites including ani-cli, mpv, ffmpeg, and graphical protocol drivers.",
            kind: HubActionKind::Launch("Setup"),
        },
        HubItem {
            id: HubActionId::AniListSync,
            category: "❖ SYSTEM & MAINTENANCE",
            label: "AniList Library Sync",
            keywords: "anilist sync token account auth login backup import export",
            badge: if state.auth.token.is_some() { "[󰄬] Connected".to_string() } else { "[ ] Offline".to_string() },
            description: "Synchronizes Watching, Completed, and Planning lists with AniList and manages offline JSON backups.",
            kind: HubActionKind::Launch("AniListLogin"),
        },
    ]
}

/// Filters and ranks hub items using the Skim fuzzy matcher over titles, categories, and keywords.
pub fn get_filtered_items(cfg: &config::Config, state: &AppState) -> Vec<HubItem> {
    let items = get_hub_items(cfg, state);
    let query = state.settings_query.trim();

    if query.is_empty() {
        return items;
    }

    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, HubItem)> = items
        .into_iter()
        .filter_map(|item| {
            let corpus = format!("{} {} {}", item.category, item.label, item.keywords);
            matcher.fuzzy_match(&corpus, query).map(|score| (score, item))
        })
        .collect();

    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    scored.into_iter().map(|(_, item)| item).collect()
}

/// Renders the Split-Deck Fuzzy Feature Command Hub.
pub fn render_overlay(frame: &mut Frame, state: &AppState, cfg: &config::Config) {
    let area = centered_rect(88, 88, frame.area());
    frame.render_widget(Clear, area);

    let theme = state.theme();

    let block = Block::default()
        .title(Span::styled(
            " ❖ SOLUS // CONTROL HUB & FEATURE PALETTE ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.secondary));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let root_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Search Input Bar
            Constraint::Min(10),   // Split Deck (Actions List + Contextual Inspector)
            Constraint::Length(2), // Navigation Footer
        ])
        .split(inner);

    // ── 1. Search Input Bar ───────────────────────────────────────────────────
    let query_display = if state.settings_query.is_empty() {
        Line::from(vec![
            Span::styled(" ❖ > ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled("Type to filter features & commands (e.g. 'cast', 'remote', 'sub', 'update')...", Style::default().fg(Color::Rgb(120, 125, 145))),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ❖ > ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(&state.settings_query, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("█", Style::default().fg(theme.primary)),
        ])
    };

    let search_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if state.settings_query.is_empty() { Color::Rgb(45, 48, 65) } else { theme.primary }));

    let search_inner = search_block.inner(root_chunks[0]);
    frame.render_widget(search_block, root_chunks[0]);
    frame.render_widget(Paragraph::new(query_display), search_inner);

    // ── 2. Split Deck Body ────────────────────────────────────────────────────
    let deck_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(52), // Left: Action List
            Constraint::Percentage(48), // Right: Contextual Inspector
        ])
        .split(root_chunks[1]);

    let filtered = get_filtered_items(cfg, state);
    let selected_idx = state.settings_cursor.min(filtered.len().saturating_sub(1));

    // Left Pane: Action List
    let list_block = Block::default()
        .title(Span::styled(
            if state.settings_query.is_empty() {
                " ◈ Categorized Actions "
            } else {
                " ◈ Filtered Actions "
            },
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(40, 44, 60)));

    let list_inner = list_block.inner(deck_chunks[0]);
    frame.render_widget(list_block, deck_chunks[0]);

    let mut action_lines = Vec::new();
    let mut last_category = "";
    let width = list_inner.width as usize;

    if filtered.is_empty() {
        action_lines.push(Line::from(Span::styled(
            "    No matching features or commands found.",
            Style::default().fg(Color::Rgb(140, 140, 160)),
        )));
    } else {
        for (idx, item) in filtered.iter().enumerate() {
            // Category header (only when not searching for cleaner hierarchy)
            if state.settings_query.is_empty() && item.category != last_category {
                if !last_category.is_empty() {
                    action_lines.push(Line::from(""));
                }
                action_lines.push(Line::from(Span::styled(
                    format!("  ── {} ", item.category),
                    Style::default().fg(Color::Rgb(110, 115, 135)).add_modifier(Modifier::BOLD),
                )));
                last_category = item.category;
            }

            let is_focused = idx == selected_idx;
            let cursor = if is_focused { " ▶ " } else { "   " };
            let cursor_len = 3;

            let badge_str = format!(" {} ", item.badge);
            let badge_len = badge_str.chars().count();

            // Calculate exact space available for the label without touching the badge
            let max_label_len = width.saturating_sub(cursor_len + badge_len + 1);

            let display_label = if item.label.chars().count() > max_label_len && max_label_len > 4 {
                let truncated: String = item.label.chars().take(max_label_len.saturating_sub(1)).collect();
                format!("{}…", truncated)
            } else {
                item.label.to_string()
            };

            let label_len = display_label.chars().count();
            let gap_len = width.saturating_sub(cursor_len + label_len + badge_len);
            let gap = " ".repeat(gap_len);

            let row_bg = if is_focused {
                Color::Rgb(36, 42, 60)
            } else {
                Color::Reset
            };

            let cursor_style = if is_focused {
                Style::default().fg(Color::Rgb(255, 120, 140)).bg(row_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(90, 95, 115)).bg(row_bg)
            };

            let label_style = if is_focused {
                Style::default().fg(Color::White).bg(row_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(215, 220, 235)).bg(row_bg)
            };

            let gap_style = Style::default().bg(row_bg);

            let badge_style = if is_focused {
                Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(150, 155, 175)).bg(Color::Rgb(22, 25, 36))
            };

            let line = Line::from(vec![
                Span::styled(cursor, cursor_style),
                Span::styled(display_label, label_style),
                Span::styled(gap, gap_style),
                Span::styled(badge_str, badge_style),
            ]);
            action_lines.push(line);
        }
    }

    frame.render_widget(Paragraph::new(action_lines), list_inner);

    // Right Pane: Contextual Inspector
    let inspector_block = Block::default()
        .title(Span::styled(
            " ❖ Contextual Feature Inspector ",
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary));

    let inspector_inner = inspector_block.inner(deck_chunks[1]);
    frame.render_widget(inspector_block, deck_chunks[1]);

    if let Some(focused_item) = filtered.get(selected_idx) {
        render_inspector_card(inspector_inner, frame, focused_item, cfg, state, &theme);
    }

    // ── 3. Footer Keybind Hints ───────────────────────────────────────────────
    let footer_p = Paragraph::new(Line::from(vec![
        Span::styled("[↑/↓] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Select   ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("[Enter] ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
        Span::styled("Execute / Cycle   ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("[Backspace] ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("Delete Filter   ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("[Esc] ", Style::default().fg(Color::Rgb(255, 85, 85)).add_modifier(Modifier::BOLD)),
        Span::styled("Dismiss", Style::default().fg(Color::Rgb(140, 140, 160))),
    ])).alignment(Alignment::Center);

    frame.render_widget(footer_p, root_chunks[2]);
}

/// Renders the right-hand inspection card for whichever item is currently focused.
fn render_inspector_card(
    area: Rect,
    frame: &mut Frame,
    item: &HubItem,
    cfg: &config::Config,
    state: &AppState,
    theme: &crate::ui::theme::ThemePalette,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header title + category badge
            Constraint::Length(3), // Description paragraph
            Constraint::Min(6),    // Feature-specific preview widget / specs
            Constraint::Length(2), // Action trigger instruction
        ])
        .split(area);

    // 1. Header
    let header_lines = vec![
        Line::from(vec![
            Span::styled(format!(" {} ", item.category), Style::default().fg(Color::Black).bg(theme.secondary).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(format!("❖ {}", item.label), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
    ];
    frame.render_widget(Paragraph::new(header_lines), chunks[0]);

    // 2. Description with wrapping
    frame.render_widget(
        Paragraph::new(item.description)
            .style(Style::default().fg(Color::Rgb(180, 185, 205)))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );

    // 3. Feature-Specific Rich Content
    match item.id {
        HubActionId::SubPreset | HubActionId::SubCollisionGuard => {
            let preview_lines = vec![
                Line::from(vec![
                    Span::styled("  Font Family: ", Style::default().fg(theme.accent)),
                    Span::styled(&cfg.subtitle_style.font_family, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("  •  Size: {}pt  •  Border: {:.1}px", cfg.subtitle_style.font_size, cfg.subtitle_style.border_size), Style::default().fg(Color::Rgb(140, 140, 160))),
                ]),
                Line::from(""),
                Line::from(Span::styled("  ┌─ Live In-Terminal Typography Preview ──────────────┐", Style::default().fg(Color::Rgb(70, 75, 100)))),
                Line::from(Span::styled("  │                                                    │", Style::default().fg(Color::Rgb(70, 75, 100)))),
                Line::from(Span::styled("  │  \"I am the one who knocks...                       │", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled("  │   and the truth cannot be erased.\"                 │", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled("  │                                                    │", Style::default().fg(Color::Rgb(70, 75, 100)))),
                Line::from(Span::styled("  └────────────────────────────────────────────────────┘", Style::default().fg(Color::Rgb(70, 75, 100)))),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  ◈ Signage Guard: ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        if cfg.subtitle_style.collision_avoidance {
                            "Active (Auto-shifts text {\\an8} over signage)"
                        } else {
                            "Inactive (Sign collisions unhandled)"
                        },
                        Style::default().fg(Color::Rgb(170, 175, 195)),
                    ),
                ]),
            ];
            frame.render_widget(Paragraph::new(preview_lines).wrap(Wrap { trim: true }), chunks[2]);
        }
        HubActionId::CastScan => {
            let cast_lines = vec![
                Line::from(vec![
                    Span::styled("  Protocols: ", Style::default().fg(theme.accent)),
                    Span::styled("AirPlay 2  •  DLNA / UPnP AVTransport", Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("  Transmission: ", Style::default().fg(theme.accent)),
                    Span::styled("Direct HLS Passthrough (Hardware Decoded)", Style::default().fg(Color::Rgb(80, 250, 123))),
                ]),
                Line::from(vec![
                    Span::styled("  Device Targets: ", Style::default().fg(theme.accent)),
                    Span::styled("Apple TV 4K, LG webOS, Samsung, Sony Bravia", Style::default().fg(Color::Rgb(180, 185, 205))),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  ◈ Action: ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                    Span::styled("Press [Enter] to scan local Wi-Fi and beam playback directly.", Style::default().fg(Color::Rgb(200, 205, 220))),
                ]),
            ];
            frame.render_widget(Paragraph::new(cast_lines).wrap(Wrap { trim: true }), chunks[2]);
        }
        HubActionId::RemoteQrModal | HubActionId::RemoteOpenBrowser | HubActionId::RemoteToggleServer => {
            let remote_lines = vec![
                Line::from(vec![
                    Span::styled("  Network URL: ", Style::default().fg(theme.accent)),
                    Span::styled(format!("http://{}:8484", crate::services::remote::get_local_ip()), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("  Daemon Status: ", Style::default().fg(theme.accent)),
                    Span::styled("● Active Wi-Fi Companion Listener", Style::default().fg(Color::Rgb(80, 250, 123))),
                ]),
                Line::from(vec![
                    Span::styled("  Touch Controls: ", Style::default().fg(theme.accent)),
                    Span::styled("Live scrub bar, ±10s seek, Sub/Dub, Volume", Style::default().fg(Color::Rgb(180, 185, 205))),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  ◈ Instant Pairing: ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                    Span::styled("Press [Enter] to display the high-contrast ASCII QR code.", Style::default().fg(Color::Rgb(200, 205, 220))),
                ]),
            ];
            frame.render_widget(Paragraph::new(remote_lines).wrap(Wrap { trim: true }), chunks[2]);
        }
        HubActionId::Anime4kMode => {
            let a4k_lines = vec![
                Line::from(vec![
                    Span::styled("  Current Shader: ", Style::default().fg(theme.accent)),
                    Span::styled(cfg.anime4k_mode.as_str(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("  GPU Pipeline: ", Style::default().fg(theme.accent)),
                    Span::styled("Direct Metal / Vulkan GLSL compute shader injection", Style::default().fg(Color::Rgb(180, 185, 205))),
                ]),
                Line::from(vec![
                    Span::styled("  Effect: ", Style::default().fg(theme.accent)),
                    Span::styled("Restores fine lines, eliminates compression artifacts, 4K upscale.", Style::default().fg(Color::Rgb(80, 250, 123))),
                ]),
            ];
            frame.render_widget(Paragraph::new(a4k_lines).wrap(Wrap { trim: true }), chunks[2]);
        }
        HubActionId::CheckUpdates | HubActionId::AutoUpdateMode => {
            let update_lines = vec![
                Line::from(vec![
                    Span::styled("  Installed Version: ", Style::default().fg(theme.accent)),
                    Span::styled(format!("Solus v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("  Channel: ", Style::default().fg(theme.accent)),
                    Span::styled("GitHub Official Releases (Universal Multi-Arch)", Style::default().fg(Color::Rgb(180, 185, 205))),
                ]),
                Line::from(vec![
                    Span::styled("  Live Status: ", Style::default().fg(theme.accent)),
                    Span::styled(
                        if let Some(ref u) = state.available_update {
                            format!("Update {} ready to install", u.latest_version)
                        } else {
                            "Up to date".to_string()
                        },
                        Style::default().fg(Color::Rgb(80, 250, 123)),
                    ),
                ]),
            ];
            frame.render_widget(Paragraph::new(update_lines).wrap(Wrap { trim: true }), chunks[2]);
        }
        _ => {
            let generic_lines = vec![
                Line::from(vec![
                    Span::styled("  Persistence: ", Style::default().fg(theme.accent)),
                    Span::styled("Automatically saved to ~/.config/solus/config.toml", Style::default().fg(Color::Rgb(140, 140, 160))),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  ◈ Control: ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                    Span::styled("Press [Enter] to toggle or execute this action immediately.", Style::default().fg(Color::Rgb(200, 205, 220))),
                ]),
            ];
            frame.render_widget(Paragraph::new(generic_lines).wrap(Wrap { trim: true }), chunks[2]);
        }
    }

    // 4. Action Trigger Hint
    let trigger_text = match item.kind {
        HubActionKind::Cycle { .. } => "[Enter] Cycle Forward   [h/l] Cycle Option",
        HubActionKind::Toggle(_) => "[Enter / Space] Toggle Value",
        HubActionKind::Launch(_) => "[Enter] Launch Screen Immediately",
        HubActionKind::Execute(_) => "[Enter] Execute Action Now",
    };

    let trigger_p = Paragraph::new(Line::from(vec![
        Span::styled(trigger_text, Style::default().fg(Color::Black).bg(Color::Rgb(240, 240, 245)).add_modifier(Modifier::BOLD)),
    ])).alignment(Alignment::Right);

    frame.render_widget(trigger_p, chunks[3]);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
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
        .split(vert[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::state::AppState;

    #[test]
    fn test_hub_items_do_not_contain_redundant_selectors() {
        let cfg = Config::default();
        let state = AppState::default();
        let items = get_hub_items(&cfg, &state);

        // Player, quality, audio (sub/dub), and subtitle language must NOT be present
        for item in &items {
            assert!(!item.label.to_lowercase().contains("player binary"), "Redundant Player found");
            assert!(!item.label.to_lowercase().contains("preferred quality"), "Redundant Quality found");
            assert!(!item.label.to_lowercase().contains("preferred audio"), "Redundant Audio found");
            assert!(!item.label.to_lowercase().contains("subtitle language"), "Redundant Subtitle Lang found");
        }
    }

    #[test]
    fn test_hub_items_contain_new_major_features() {
        let cfg = Config::default();
        let state = AppState::default();
        let items = get_hub_items(&cfg, &state);

        let labels: Vec<&str> = items.iter().map(|i| i.label).collect();
        assert!(labels.iter().any(|l| l.contains("Typography Preset")));
        assert!(labels.iter().any(|l| l.contains("Signage Collision Guard")));
        assert!(labels.iter().any(|l| l.contains("Mobile Remote Daemon")));
        assert!(labels.iter().any(|l| l.contains("Mobile Remote QR Code")));
        assert!(labels.iter().any(|l| l.contains("Scan & Beam to TV")));
        assert!(labels.iter().any(|l| l.contains("Anime4K")));
    }

    #[test]
    fn test_hub_filter_matches_and_ranks() {
        let cfg = Config::default();
        let mut state = AppState::default();

        // Empty query returns all
        state.settings_query = String::new();
        let res_all = get_filtered_items(&cfg, &state);
        let all_count = get_hub_items(&cfg, &state).len();
        assert_eq!(res_all.len(), all_count);

        // "cast" should match casting items at top (via keywords or label)
        state.settings_query = "cast".to_string();
        let res_cast = get_filtered_items(&cfg, &state);
        assert!(!res_cast.is_empty());
        assert_eq!(res_cast[0].id, HubActionId::CastScan);

        // "remote" query
        state.settings_query = "remote".to_string();
        let res_remote = get_filtered_items(&cfg, &state);
        assert!(!res_remote.is_empty());
        assert!(res_remote[0].label.to_lowercase().contains("remote"));

        // "sub" query
        state.settings_query = "sub".to_string();
        let res_sub = get_filtered_items(&cfg, &state);
        assert!(!res_sub.is_empty());
        assert!(res_sub.iter().any(|i| i.label.contains("Preset") || i.label.contains("Typography")));
    }
}
