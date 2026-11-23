//! Playback options overlay.
//! Allows selecting video stream quality and dynamic subtitle language.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::state::AppState;

/// Exact geometry for every interactive region in the Playback Options modal.
/// Shared by both `render_overlay` and mouse hit-testing so click coordinates
/// never drift across terminal sizes or loading/loaded state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackOptionsLayout {
    pub modal_rect:        Rect,
    pub header_rect:       Rect,
    pub header_bar_row:    u16,
    pub player_chip_rect:  Rect,
    pub audio_chip_rect:   Rect,
    pub anime4k_chip_rect: Rect,
    pub network_chip_rect: Rect,
    pub pip_chip_rect:     Rect,
    pub corner_chip_rect:  Rect,
    pub autofs_chip_rect:  Rect,
    pub hud_chip_rect:     Rect,
    pub player_rect:       Rect,
    pub provider_rect:     Rect,
    pub quality_rect:      Rect,
    pub subtitle_rect:     Rect,
    pub footer_rect:       Rect,
    pub launch_btn_rect:   Rect,
    pub back_btn_rect:     Rect,
}

/// Compute invariant modal geometry from the full terminal frame.
pub fn compute_layout(area: Rect) -> PlaybackOptionsLayout {
    let modal_rect = centered_rect(84, 70, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Header info + network telemetry bar
            Constraint::Min(0),    // 4-column body (Player + Provider + Quality + Subtitles)
            Constraint::Length(3), // Footer hints
        ])
        .split(modal_rect);

    let header_rect = chunks[0];
    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(24),
            Constraint::Percentage(22),
            Constraint::Percentage(34),
        ])
        .split(chunks[1]);
    let footer_rect = chunks[2];

    let header_bar_row = header_rect.y.saturating_add(2);
    let inner_x = header_rect.x.saturating_add(1);
    let inner_w = header_rect.width.saturating_sub(2);
    let third = (inner_w / 3).max(1);
    let player_chip_rect = Rect {
        x: inner_x,
        y: header_bar_row,
        width: third.min(inner_w),
        height: 1,
    };
    let audio_chip_rect = Rect {
        x: inner_x.saturating_add(third),
        y: header_bar_row,
        width: third.min(inner_w.saturating_sub(third)),
        height: 1,
    };
    let a4k_x = inner_x.saturating_add(third.saturating_mul(2));
    let anime4k_chip_rect = Rect {
        x: a4k_x,
        y: header_bar_row,
        width: inner_x.saturating_add(inner_w).saturating_sub(a4k_x),
        height: 1,
    };
    let net_row = header_rect.y.saturating_add(3);
    let network_chip_rect = Rect {
        x: inner_x,
        y: net_row,
        width: 24.min(inner_w),
        height: 1,
    };
    let pip_x = inner_x.saturating_add(25);
    let pip_chip_rect = Rect {
        x: pip_x,
        y: net_row,
        width: 10.min(inner_x.saturating_add(inner_w).saturating_sub(pip_x)),
        height: 1,
    };
    let corner_x = pip_x.saturating_add(11);
    let corner_chip_rect = Rect {
        x: corner_x,
        y: net_row,
        width: 9.min(inner_x.saturating_add(inner_w).saturating_sub(corner_x)),
        height: 1,
    };
    let autofs_x = corner_x.saturating_add(10);
    let autofs_chip_rect = Rect {
        x: autofs_x,
        y: net_row,
        width: 13.min(inner_x.saturating_add(inner_w).saturating_sub(autofs_x)),
        height: 1,
    };
    let hud_x = autofs_x.saturating_add(14);
    let hud_chip_rect = Rect {
        x: hud_x,
        y: net_row,
        width: 8.min(inner_x.saturating_add(inner_w).saturating_sub(hud_x)),
        height: 1,
    };

    let f_inner_h = footer_rect.height.min(3);
    let launch_w = 18.min(footer_rect.width);
    let back_w = 16.min(footer_rect.width);
    let launch_btn_rect = Rect {
        x: footer_rect.x,
        y: footer_rect.y,
        width: launch_w,
        height: f_inner_h,
    };
    let back_btn_rect = Rect {
        x: footer_rect.right().saturating_sub(back_w),
        y: footer_rect.y,
        width: back_w,
        height: f_inner_h,
    };

    PlaybackOptionsLayout {
        modal_rect,
        header_rect,
        header_bar_row,
        player_chip_rect,
        audio_chip_rect,
        anime4k_chip_rect,
        network_chip_rect,
        pip_chip_rect,
        corner_chip_rect,
        autofs_chip_rect,
        hud_chip_rect,
        player_rect: body_cols[0],
        provider_rect: body_cols[1],
        quality_rect: body_cols[2],
        subtitle_rect: body_cols[3],
        footer_rect,
        launch_btn_rect,
        back_btn_rect,
    }
}

fn clamp_label(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let prefix: String = text.chars().take(max_chars - 1).collect();
    format!("{}…", prefix.trim_end())
}

/// Render the playback options overlay on top of the detail screen.
pub fn render_overlay(frame: &mut Frame, state: &AppState) {
    let layout = compute_layout(frame.area());
    frame.render_widget(Clear, layout.modal_rect);
    let theme = state.theme();

    let audio_label = if state.pending_dub {
        "Dub"
    } else {
        "Sub"
    };

    let (header_icon, title_suffix) = match state.playback_options_purpose {
        crate::state::PlaybackOptionsPurpose::LaunchNow => ("  ", String::new()),
        crate::state::PlaybackOptionsPurpose::EnqueueSingle => (" ◈ ADD TO QUEUE: ", String::new()),
        crate::state::PlaybackOptionsPurpose::EditQueueItem { .. } => (" ◈ EDIT QUEUE ENTRY: ", String::new()),
    };

    let title_budget = (layout.header_rect.width as usize).saturating_sub(36).max(10);
    let title_line = state
        .selected_anime
        .as_ref()
        .map(|a| {
            let ep = state.selected_episode.unwrap_or(1);
            let raw = format!("{} • Ep {}{}", a.short_title(), ep, title_suffix);
            clamp_label(&raw, title_budget)
        })
        .unwrap_or_else(|| "Choose options before launch".to_string());

    let (dub_badge_text, dub_badge_style) = match state.dub_available {
        Some(true) => (" [Sub + Dub] ", Style::default().fg(Color::Rgb(100, 255, 120)).add_modifier(Modifier::BOLD)),
        Some(false) => (" [Sub only] ", Style::default().fg(Color::Rgb(255, 160, 80)).add_modifier(Modifier::BOLD)),
        None => ("", Style::default()),
    };

    let season_year = state.selected_anime.as_ref().and_then(|a| a.season_year);
    let era = crate::services::shaders::AnimeEra::from_year(season_year);
    let era_text = match season_year {
        Some(y) => format!(" [{} • {}] ", era.short_badge(), y),
        None => format!(" [{}] ", era.short_badge()),
    };
    let era_style = match era {
        crate::services::shaders::AnimeEra::RetroCel => Style::default().fg(Color::Rgb(255, 190, 90)).add_modifier(Modifier::BOLD),
        crate::services::shaders::AnimeEra::ModernDigital => Style::default().fg(Color::Rgb(90, 210, 255)).add_modifier(Modifier::BOLD),
    };

    let player_badge = if state.selected_player() == crate::config::Player::AutoBest {
        let eff = if state.has_mpv {
            "mpv"
        } else if cfg!(windows) && state.has_mpvnet {
            "mpv.net"
        } else if cfg!(target_os = "linux") && state.has_celluloid {
            "Celluloid"
        } else {
            "mpv"
        };
        format!(" Auto-Best ({eff}) ")
    } else {
        format!(" {} ", state.selected_player().label())
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                header_icon,
                Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                title_line,
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(dub_badge_text, dub_badge_style),
            Span::styled(era_text, era_style),
        ]),
        Line::from(vec![
            Span::styled(" Player: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                player_badge,
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("([p] toggle)", Style::default().fg(theme.text_dim)),
            Span::raw("   "),
            Span::styled("Audio: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!(" {} ", audio_label),
                Style::default()
                    .fg(if state.pending_dub { Color::Black } else { Color::White })
                    .bg(if state.pending_dub { theme.secondary } else { theme.border_idle })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                if state.dub_available == Some(false) {
                    "([a] Sub only)"
                } else {
                    "([a] toggle)"
                },
                Style::default().fg(if state.dub_available == Some(false) { Color::Rgb(220, 120, 80) } else { theme.text_dim }),
            ),
            Span::raw("   "),
            Span::styled("Anime4K: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!(" {} ", state.anime4k_mode.as_str()),
                Style::default()
                    .fg(if state.anime4k_mode == crate::config::Anime4KMode::Ultra { Color::Black } else { Color::White })
                    .bg(match state.anime4k_mode {
                        crate::config::Anime4KMode::Off => theme.border_idle,
                        crate::config::Anime4KMode::Balanced => Color::Rgb(40, 120, 180),
                        crate::config::Anime4KMode::Ultra => theme.primary,
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(" ([u])", Style::default().fg(theme.text_dim)),
            Span::raw("   "),
            Span::styled("Delay: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                if state.subtitle_delay_ms == 0 {
                    " 0ms ".to_string()
                } else {
                    format!(" {:+}ms ", state.subtitle_delay_ms)
                },
                Style::default()
                    .fg(if state.subtitle_delay_ms == 0 { Color::White } else { Color::Black })
                    .bg(if state.subtitle_delay_ms == 0 { theme.border_idle } else { theme.accent })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        {
            let net_bg = match state.network_mode {
                crate::config::NetworkMode::Normal => theme.border_idle,
                crate::config::NetworkMode::CongestedWifi => Color::Rgb(245, 185, 66),
                crate::config::NetworkMode::UltraSaver => Color::Rgb(80, 220, 145),
            };
            let net_fg = if state.network_mode.is_congested() {
                Color::Black
            } else {
                Color::White
            };
            let cap_desc = match state.network_mode {
                crate::config::NetworkMode::Normal => "Uncapped · 45s buf · Auto-ABR",
                crate::config::NetworkMode::CongestedWifi => "<=720p30 · 120s Disk · Compact Thumbs",
                crate::config::NetworkMode::UltraSaver => "<=480p30 · 180s Disk · Low Bandwidth",
            };
            let pip_on = state.live_playback.pip_mode || state.auto_pip;
            let pip_pill = if pip_on { "[PiP:ON] " } else { "[PiP:OFF]" };
            let autofs_pill = if state.auto_fullscreen { "[AutoFS:ON] " } else { "[AutoFS:OFF]" };
            let mut row3 = vec![
                Span::styled(" Network: ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!(" {} ", state.network_mode.short_tag()),
                    Style::default().fg(net_fg).bg(net_bg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" [n] ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    format!(" {pip_pill} "),
                    Style::default()
                        .fg(if pip_on { Color::Black } else { theme.text })
                        .bg(if pip_on { theme.primary } else { Color::Reset })
                        .add_modifier(if pip_on { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(
                    " [Corner] ",
                    Style::default().fg(if pip_on { theme.accent } else { theme.text_dim }),
                ),
                Span::styled(
                    format!(" {autofs_pill} "),
                    Style::default()
                        .fg(if state.auto_fullscreen { Color::Black } else { theme.text })
                        .bg(if state.auto_fullscreen { theme.secondary } else { Color::Reset })
                        .add_modifier(if state.auto_fullscreen { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(" [i HUD] ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            ];
            if state.live_playback.started_at.is_some() {
                row3.push(Span::styled(" · ", Style::default().fg(theme.divider)));
                row3.push(Span::styled(
                    state.live_playback.compact_status_text(),
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                ));
            } else {
                row3.push(Span::styled(" · ", Style::default().fg(theme.divider)));
                row3.push(Span::styled(cap_desc, Style::default().fg(theme.text_dim)));
            }
            Line::from(row3)
        },
    ])
    .block(
        Block::default()
            .title(Span::styled(
                "  Playback Configuration ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_focus)),
    );
    frame.render_widget(header, layout.header_rect);

    // ── Col 0: Media Player Selection ────────────────────────────────────
    let pl_focused = state.playback_focus_column == 0;
    let pl_border_style = if pl_focused {
        Style::default().fg(theme.border_focus)
    } else {
        Style::default().fg(theme.border_idle)
    };

    let pl_block = Block::default()
        .title(Span::styled(
            if pl_focused { " 󰐊 Player (Active) " } else { " 󰐊 Player " },
            pl_border_style.add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(pl_border_style);

    let pl_inner = pl_block.inner(layout.player_rect);
    frame.render_widget(pl_block, layout.player_rect);

    let pl_budget = (pl_inner.width as usize).saturating_sub(7).max(4);
    let pl_items: Vec<ListItem> = state
        .playback_players
        .iter()
        .enumerate()
        .map(|(i, player)| {
            let is_selected = i == state.playback_player_cursor;
            let radio = if is_selected { " [●] " } else { " [○] " };
            let (name, is_avail) = match player {
                crate::config::Player::AutoBest => ("Auto-Best (libmpv)", state.has_any_player()),
                crate::config::Player::Mpv => ("mpv (100%)", state.has_mpv),
                crate::config::Player::MpvNet => {
                    if cfg!(windows) {
                        ("mpv.net (Win 99%)", state.has_mpvnet)
                    } else {
                        ("mpv.net (Win)", false)
                    }
                }
                crate::config::Player::Celluloid => {
                    if cfg!(target_os = "linux") {
                        ("Celluloid (GTK 95%)", state.has_celluloid)
                    } else {
                        ("Celluloid (Linux)", false)
                    }
                }
            };

            let style = if !is_avail {
                Style::default().fg(Color::Rgb(110, 110, 125))
            } else if is_selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(170, 170, 185))
            };

            let radio_style = if is_selected {
                Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text_dim)
            };

            ListItem::new(Line::from(vec![
                Span::styled(radio, radio_style),
                Span::styled(clamp_label(name, pl_budget), style),
            ]))
        })
        .collect();

    let mut pl_state = ListState::default().with_selected(Some(state.playback_player_cursor));
    frame.render_stateful_widget(List::new(pl_items), pl_inner, &mut pl_state);

    // ── Col 1: Streaming Provider Selection ──────────────────────────────
    let p_focused = state.playback_focus_column == 1;
    let p_border_style = if p_focused {
        Style::default().fg(theme.border_focus)
    } else {
        Style::default().fg(theme.border_idle)
    };

    let p_block = Block::default()
        .title(Span::styled(
            if p_focused { " 󰢹 Provider (Active) " } else { " 󰢹 Provider " },
            p_border_style.add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(p_border_style);

    let p_inner = p_block.inner(layout.provider_rect);
    frame.render_widget(p_block, layout.provider_rect);

    let p_budget = (p_inner.width as usize).saturating_sub(7).max(4);
    let p_items: Vec<ListItem> = if state.is_loading_subs {
        vec![ListItem::new(Line::from(vec![
            Span::styled(" ⟳ ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(clamp_label("Probing sources…", p_budget), Style::default().fg(theme.text_dim)),
        ]))]
    } else if state.playback_providers.is_empty() {
        vec![ListItem::new(Line::from(vec![
            Span::styled(" ✗ ", Style::default().fg(Color::Rgb(255, 110, 110)).add_modifier(Modifier::BOLD)),
            Span::styled(clamp_label("No source for Ep", p_budget), Style::default().fg(Color::Rgb(255, 130, 130))),
        ]))]
    } else {
        state
            .playback_providers
            .iter()
            .enumerate()
            .map(|(i, provider)| {
                let is_selected = i == state.playback_provider_cursor;
                let radio = if is_selected { " [●] " } else { " [○] " };
                let is_unsupported_dub = state.pending_dub && !provider.supports_dub;
                let style = if is_unsupported_dub {
                    Style::default().fg(Color::Rgb(90, 90, 105))
                } else if is_selected {
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(160, 160, 175))
                };
                let radio_style = if is_selected {
                    Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text_dim)
                };
                let label = if is_unsupported_dub {
                    format!("{} (No Dub)", provider.name)
                } else if let Some(ref res) = provider.verified_res {
                    format!("{} [✓ {}]", provider.name, res)
                } else {
                    provider.name.clone()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(radio, radio_style),
                    Span::styled(clamp_label(&label, p_budget), style),
                ]))
            })
            .collect()
    };

    let mut p_state = ListState::default().with_selected(Some(
        state.playback_provider_cursor.min(p_items.len().saturating_sub(1)),
    ));
    frame.render_stateful_widget(List::new(p_items), p_inner, &mut p_state);

    // ── Col 2: Quality Selection ──────────────────────────────────────────
    let q_focused = state.playback_focus_column == 2;
    let q_border_style = if q_focused {
        Style::default().fg(theme.border_focus)
    } else {
        Style::default().fg(theme.border_idle)
    };

    let q_cap_tag = state
        .network_mode
        .max_height_cap()
        .map(|h| format!(" (<={h}p)"))
        .unwrap_or_default();
    let q_title = if q_focused {
        format!(" 󰗃 Quality{q_cap_tag} (Active) ")
    } else {
        format!(" 󰗃 Quality{q_cap_tag} ")
    };

    let q_block = Block::default()
        .title(Span::styled(
            q_title,
            q_border_style.add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(q_border_style);

    let q_inner = q_block.inner(layout.quality_rect);
    frame.render_widget(q_block, layout.quality_rect);

    let q_budget = (q_inner.width as usize).saturating_sub(7).max(4);
    let (q_items, q_sel): (Vec<ListItem>, usize) = if state.is_loading_subs && state.playback_qualities.is_empty() {
        (
            vec![
                ListItem::new(Line::from(vec![
                    Span::styled(" [·] ", Style::default().fg(theme.primary)),
                    Span::styled(clamp_label("Detecting resolutions...", q_budget), Style::default().fg(theme.text_dim)),
                ])),
                ListItem::new(Line::from(vec![
                    Span::styled("     ", Style::default()),
                    Span::styled(clamp_label("Verifying codecs...", q_budget), Style::default().fg(theme.text_dim)),
                ])),
            ],
            0,
        )
    } else if state.playback_qualities.is_empty() {
        (
            vec![ListItem::new(Line::from(vec![
                Span::styled(" [●] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                Span::styled(clamp_label("Default Stream", q_budget), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]))],
            0,
        )
    } else {
        let items: Vec<ListItem> = state
            .playback_qualities
            .iter()
            .enumerate()
            .map(|(i, quality)| {
                let is_selected = i == state.playback_quality_cursor;
                let radio = if is_selected { " [●] " } else { " [○] " };
                let style = if is_selected {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(160, 160, 175))
                };
                let radio_style = if is_selected {
                    Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text_dim)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(radio, radio_style),
                    Span::styled(clamp_label(&quality.display_with_bitrate(), q_budget), style),
                ]))
            })
            .collect();
        let sel = state.playback_quality_cursor.min(items.len().saturating_sub(1));
        (items, sel)
    };

    let mut q_state = ListState::default().with_selected(Some(q_sel));
    frame.render_stateful_widget(List::new(q_items), q_inner, &mut q_state);

    // ── Col 3: Subtitle Language Selection ───────────────────────────────
    let sub_focused = state.playback_focus_column == 3;
    let sub_border_style = if sub_focused {
        Style::default().fg(theme.border_focus)
    } else {
        Style::default().fg(theme.border_idle)
    };

    let sub_block = Block::default()
        .title(Span::styled(
            if sub_focused { " 󰨖 Subtitles (Active) " } else { " 󰨖 Available Subtitles " },
            sub_border_style.add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(sub_border_style);

    let sub_inner = sub_block.inner(layout.subtitle_rect);
    frame.render_widget(sub_block, layout.subtitle_rect);

    let sub_budget = (sub_inner.width as usize).saturating_sub(7).max(4);
    let (sub_items, sub_sel): (Vec<ListItem>, usize) = if state.is_loading_subs && state.playback_sub_languages.len() <= 2 {
        (
            vec![
                ListItem::new(Line::from(vec![
                    Span::styled(" [·] ", Style::default().fg(theme.primary)),
                    Span::styled(clamp_label("No subtitles", sub_budget), Style::default().fg(theme.text_dim)),
                ])),
                ListItem::new(Line::from(vec![
                    Span::styled("     ", Style::default()),
                    Span::styled(clamp_label("Detecting subtitle tracks...", sub_budget), Style::default().fg(theme.text_dim)),
                ])),
            ],
            0,
        )
    } else {
        let items: Vec<ListItem> = state
            .playback_sub_languages
            .iter()
            .enumerate()
            .map(|(i, sub)| {
                let is_selected = i == state.playback_sub_cursor;
                let radio = if is_selected { " [●] " } else { " [○] " };
                let style = if is_selected {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(170, 170, 185))
                };
                let radio_style = if is_selected {
                    Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text_dim)
                };

                ListItem::new(Line::from(vec![
                    Span::styled(radio, radio_style),
                    Span::styled(clamp_label(&sub.name, sub_budget), style),
                ]))
            })
            .collect();
        let sel = state.playback_sub_cursor.min(items.len().saturating_sub(1));
        (items, sel)
    };

    let mut sub_state = ListState::default().with_selected(Some(sub_sel));
    frame.render_stateful_widget(List::new(sub_items), sub_inner, &mut sub_state);

    // ── Footer: Keymap Bar ───────────────────────────────────────────────
    let enter_label = match state.playback_options_purpose {
        crate::state::PlaybackOptionsPurpose::LaunchNow => " Launch  ",
        crate::state::PlaybackOptionsPurpose::EnqueueSingle => " Save to Queue  ",
        crate::state::PlaybackOptionsPurpose::EditQueueItem { .. } => " Apply Changes  ",
    };
    let mut footer_spans = vec![
        Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(enter_label),
    ];
    if state.playback_options_purpose == crate::state::PlaybackOptionsPurpose::LaunchNow {
        footer_spans.extend([
            Span::styled(" e ", Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::raw(" Enqueue  "),
        ]);
    }
    footer_spans.extend([
        Span::styled(" Tab/h/l ", Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::raw(" Col  "),
        Span::styled(" j/k ", Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::raw(" Pick  "),
        Span::styled(" p ", Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::raw(" Player  "),
        Span::styled(" a ", Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::raw(" Audio  "),
        Span::styled(" n/N ", Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::raw(" Net  "),
        Span::styled(" w/W ", Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::raw(" PiP  "),
        Span::styled(" f ", Style::default().fg(Color::Black).bg(theme.secondary).add_modifier(Modifier::BOLD)),
        Span::raw(" FS  "),
        Span::styled(" i/I ", Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::raw(" HUD  "),
        Span::styled(" [/] ", Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::raw(" Delay  "),
        Span::styled(" Esc/q ", Style::default().fg(Color::White).bg(theme.border_idle)),
        Span::raw(" Back"),
    ]);
    let footer = Paragraph::new(Line::from(footer_spans))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_focus)),
    );
    frame.render_widget(footer, layout.footer_rect);
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
    fn test_play_options_modal_geometry_identical_between_loading_and_loaded() {
        for &(w, h) in &[(84u16, 28u16), (90, 28), (120, 36)] {
            let area = Rect::new(0, 0, w, h);
            let layout_loading = compute_layout(area);
            let layout_loaded = compute_layout(area);
            assert_eq!(layout_loading, layout_loaded);

            // Ensure loading state renders without overlapping or shifting box geometry.
            let mut loading_state = AppState::new();
            loading_state.selected_episode = Some(13);
            loading_state.open_playback_options(
                "Hunter x Hunter (2011)".to_string(),
                0,
                true,
                crate::config::Player::Mpv,
            );

            let mut loaded_state = AppState::new();
            loaded_state.selected_episode = Some(13);
            loaded_state.open_playback_options(
                "Hunter x Hunter (2011)".to_string(),
                0,
                true,
                crate::config::Player::Mpv,
            );
            loaded_state.is_loading_subs = false;
            loaded_state.playback_providers = vec![crate::api::subtitles::ProviderOption {
                id: "hd1".into(),
                name: "HD-1 Very Long Stream Provider Label Name".into(),
                server_label: "HD-1".into(),
                supports_sub: true,
                supports_dub: true,
                is_verified: true,
                verified_res: Some("1080p".into()),
                master_url: None,
                qualities: vec![],
                subtitles: vec![],
            }];
            loaded_state.playback_qualities = vec![
                crate::api::subtitles::QualityOption::simple("1080p", ""),
                crate::api::subtitles::QualityOption::simple("720p", ""),
            ];
            loaded_state.playback_sub_languages = vec![
                crate::api::subtitles::SubtitleOption::none_option(),
                crate::api::subtitles::SubtitleOption {
                    name: "English (US) [Official]".into(),
                    code: "eng".into(),
                    url: "https://example.com/en.vtt".into(),
                    stream_label: "Primary".into(),
                },
                crate::api::subtitles::SubtitleOption {
                    name: "Arabic [Auto-Synced]".into(),
                    code: "ara".into(),
                    url: "https://example.com/ar.vtt".into(),
                    stream_label: "Auto-Synced".into(),
                },
            ];

            let mut term_loading = Terminal::new(TestBackend::new(w, h)).unwrap();
            term_loading
                .draw(|f| render_overlay(f, &loading_state))
                .unwrap();

            let mut term_loaded = Terminal::new(TestBackend::new(w, h)).unwrap();
            term_loaded
                .draw(|f| render_overlay(f, &loaded_state))
                .unwrap();

            // Verify header top border and column border x-positions match identically.
            let buf_loading = term_loading.backend().buffer();
            let buf_loaded = term_loaded.backend().buffer();
            let body_top_y = layout_loading.provider_rect.y;
            for rect in [
                layout_loading.player_rect,
                layout_loading.provider_rect,
                layout_loading.quality_rect,
                layout_loading.subtitle_rect,
            ] {
                assert_eq!(rect.y, body_top_y);
                assert_eq!(
                    buf_loading.cell((rect.x, rect.y)).unwrap().symbol(),
                    buf_loaded.cell((rect.x, rect.y)).unwrap().symbol()
                );
                assert_eq!(
                    buf_loading.cell((rect.right() - 1, rect.bottom() - 1)).unwrap().symbol(),
                    buf_loaded.cell((rect.right() - 1, rect.bottom() - 1)).unwrap().symbol()
                );
            }
        }
    }

    #[test]
    fn test_play_options_network_chip_and_capped_quality_render() {
        let mut state = AppState::new();
        state.selected_episode = Some(4);
        state.network_mode = crate::config::NetworkMode::CongestedWifi;
        state.open_playback_options("Frieren".to_string(), 0, false, crate::config::Player::Mpv);
        state.is_loading_subs = false;
        state.playback_qualities = vec![crate::api::subtitles::QualityOption {
            label: "720p".into(),
            url: String::new(),
            height: Some(720),
            tbr_kbps: Some(1600),
            fps_x10: Some(240),
        }];
        let area = Rect::new(0, 0, 120, 36);
        let layout = compute_layout(area);
        assert_eq!(layout.network_chip_rect.height, 1);
        let mut term = Terminal::new(TestBackend::new(120, 36)).unwrap();
        term.draw(|f| render_overlay(f, &state)).unwrap();
        let mut text = String::new();
        let buf = term.backend().buffer();
        for y in 0..36 {
            for x in 0..120 {
                text.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(text.contains("NET:WIFI"));
        assert!(text.contains("(<=720p)"));
        assert!(text.contains("720p · 1.6M"));
    }
}
