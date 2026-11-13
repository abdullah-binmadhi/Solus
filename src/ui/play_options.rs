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

/// Render the playback options overlay on top of the detail screen.
pub fn render_overlay(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(84, 70, frame.area());
    frame.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Header info
            Constraint::Min(0),    // 4-column body (Player + Provider + Quality + Subtitles)
            Constraint::Length(3), // Footer hints
        ])
        .split(area);

    let audio_label = if state.pending_dub {
        "Dub"
    } else {
        "Sub"
    };

    let title_line = state
        .selected_anime
        .as_ref()
        .map(|a| {
            let ep = state.selected_episode.unwrap_or(1);
            format!("{} • Episode {}", a.short_title(), ep)
        })
        .unwrap_or_else(|| "Choose options before launch".to_string());

    let (dub_badge_text, dub_badge_style) = match state.dub_available {
        Some(true) => (" [Sub + Dub] ", Style::default().fg(Color::Rgb(100, 255, 120)).add_modifier(Modifier::BOLD)),
        Some(false) => (" [Sub only] ", Style::default().fg(Color::Rgb(255, 160, 80)).add_modifier(Modifier::BOLD)),
        None => ("", Style::default()),
    };

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "  ",
                Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                title_line,
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(dub_badge_text, dub_badge_style),
        ]),
        Line::from(vec![
            Span::styled(" Player: ", Style::default().fg(Color::Rgb(160, 160, 175))),
            Span::styled(
                format!(" {} ", state.selected_player().as_str()),
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(140, 30, 200))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("([p] toggle)", Style::default().fg(Color::Rgb(120, 120, 135))),
            Span::raw("   "),
            Span::styled("Audio: ", Style::default().fg(Color::Rgb(160, 160, 175))),
            Span::styled(
                format!(" {} ", audio_label),
                Style::default()
                    .fg(Color::White)
                    .bg(if state.pending_dub { Color::Rgb(120, 40, 180) } else { Color::Rgb(40, 40, 60) })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                if state.dub_available == Some(false) {
                    "([a] Sub only)"
                } else {
                    "([a] toggle)"
                },
                Style::default().fg(if state.dub_available == Some(false) { Color::Rgb(220, 120, 80) } else { Color::Rgb(120, 120, 135) }),
            ),
            Span::raw("   "),
            Span::styled("Anime4K: ", Style::default().fg(Color::Rgb(160, 160, 175))),
            Span::styled(
                format!(" {} ", state.anime4k_mode.as_str()),
                Style::default()
                    .fg(Color::White)
                    .bg(match state.anime4k_mode {
                        crate::config::Anime4KMode::Off => Color::Rgb(50, 50, 65),
                        crate::config::Anime4KMode::Balanced => Color::Rgb(40, 120, 180),
                        crate::config::Anime4KMode::Ultra => Color::Rgb(180, 0, 255),
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("([u] toggle)", Style::default().fg(Color::Rgb(120, 120, 135))),
        ]),
    ])
    .block(
        Block::default()
            .title(Span::styled(
                "  Playback Configuration ",
                Style::default()
                    .fg(Color::Rgb(180, 0, 255))
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(180, 0, 255))),
    );
    frame.render_widget(header, chunks[0]);

    // Split body into 4 columns: Player (col 0), Provider (col 1), Quality (col 2), Subtitles (col 3)
    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(24),
            Constraint::Percentage(22),
            Constraint::Percentage(34),
        ])
        .split(chunks[1]);

    // ── Col 0: Media Player Selection ────────────────────────────────────
    let pl_focused = state.playback_focus_column == 0;
    let pl_border_style = if pl_focused {
        Style::default().fg(Color::Rgb(180, 0, 255))
    } else {
        Style::default().fg(Color::Rgb(80, 80, 100))
    };

    let pl_block = Block::default()
        .title(Span::styled(
            if pl_focused { " 󰐊 Player (Active) " } else { " 󰐊 Player " },
            pl_border_style.add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(pl_border_style);

    let pl_inner = pl_block.inner(body_cols[0]);
    frame.render_widget(pl_block, body_cols[0]);

    let pl_items: Vec<ListItem> = state
        .playback_players
        .iter()
        .enumerate()
        .map(|(i, player)| {
            let is_selected = i == state.playback_player_cursor;
            let radio = if is_selected { " [●] " } else { " [○] " };
            let (name, is_avail) = match player {
                crate::config::Player::Mpv => ("mpv", state.has_mpv),
                crate::config::Player::Iina => {
                    if cfg!(target_os = "macos") {
                        ("iina", state.has_iina)
                    } else {
                        ("iina (macOS)", false)
                    }
                }
                crate::config::Player::Vlc => ("vlc", state.has_vlc),
            };

            let style = if !is_avail {
                Style::default().fg(Color::Rgb(110, 110, 125))
            } else if is_selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(170, 170, 185))
            };

            let radio_style = if is_selected {
                Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(100, 100, 115))
            };

            ListItem::new(Line::from(vec![
                Span::styled(radio, radio_style),
                Span::styled(name, style),
            ]))
        })
        .collect();

    let mut pl_state = ListState::default().with_selected(Some(state.playback_player_cursor));
    frame.render_stateful_widget(List::new(pl_items), pl_inner, &mut pl_state);

    // ── Col 1: Streaming Provider Selection ──────────────────────────────
    let p_focused = state.playback_focus_column == 1;
    let p_border_style = if p_focused {
        Style::default().fg(Color::Rgb(180, 0, 255))
    } else {
        Style::default().fg(Color::Rgb(80, 80, 100))
    };

    let p_block = Block::default()
        .title(Span::styled(
            if p_focused { " 󰢹 Provider (Active) " } else { " 󰢹 Provider " },
            p_border_style.add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(p_border_style);

    let p_inner = p_block.inner(body_cols[1]);
    frame.render_widget(p_block, body_cols[1]);

    if state.playback_providers.is_empty() {
        let empty_p = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Default Provider",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
        ]);
        frame.render_widget(empty_p, p_inner);
    } else {
        let p_items: Vec<ListItem> = state
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
                    Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(100, 100, 115))
                };
                let label = if is_unsupported_dub {
                    format!("{} (No Dub)", provider.name)
                } else {
                    provider.name.clone()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(radio, radio_style),
                    Span::styled(label, style),
                ]))
            })
            .collect();

        let mut p_state = ListState::default().with_selected(Some(state.playback_provider_cursor));
        frame.render_stateful_widget(List::new(p_items), p_inner, &mut p_state);
    }

    // ── Col 2: Quality Selection ──────────────────────────────────────────
    let q_focused = state.playback_focus_column == 2;
    let q_border_style = if q_focused {
        Style::default().fg(Color::Rgb(180, 0, 255))
    } else {
        Style::default().fg(Color::Rgb(80, 80, 100))
    };

    let q_block = Block::default()
        .title(Span::styled(
            if q_focused { " 󰗃 Stream Quality (Active) " } else { " 󰗃 Stream Quality " },
            q_border_style.add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(q_border_style);

    let q_inner = q_block.inner(body_cols[2]);
    frame.render_widget(q_block, body_cols[2]);

    if state.is_loading_subs {
        let loading_q = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  󰑮 Detecting resolutions...",
                Style::default().fg(Color::Rgb(180, 0, 255)),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Verifying stream codecs...",
                Style::default().fg(Color::Rgb(130, 130, 145)),
            )),
        ]);
        frame.render_widget(loading_q, q_inner);
    } else if state.playback_qualities.is_empty() {
        let empty_q = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Default Stream",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
        ]);
        frame.render_widget(empty_q, q_inner);
    } else {
        let q_items: Vec<ListItem> = state
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
                    Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(100, 100, 115))
                };
                ListItem::new(Line::from(vec![
                    Span::styled(radio, radio_style),
                    Span::styled(&quality.label, style),
                ]))
            })
            .collect();

        let mut q_state = ListState::default().with_selected(Some(state.playback_quality_cursor));
        frame.render_stateful_widget(List::new(q_items), q_inner, &mut q_state);
    }

    // ── Col 3: Subtitle Language Selection ───────────────────────────────
    let sub_focused = state.playback_focus_column == 3;
    let sub_border_style = if sub_focused {
        Style::default().fg(Color::Rgb(180, 0, 255))
    } else {
        Style::default().fg(Color::Rgb(80, 80, 100))
    };

    let sub_block = Block::default()
        .title(Span::styled(
            if sub_focused { " 󰨖 Available Subtitles (Active) " } else { " 󰨖 Available Subtitles " },
            sub_border_style.add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(sub_border_style);

    let sub_inner = sub_block.inner(body_cols[3]);
    frame.render_widget(sub_block, body_cols[3]);

    if state.is_loading_subs {
        let loading_p = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  󰑮 Detecting stream languages...",
                Style::default().fg(Color::Rgb(180, 0, 255)),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Checking available subtitle tracks...",
                Style::default().fg(Color::Rgb(130, 130, 145)),
            )),
        ]);
        frame.render_widget(loading_p, sub_inner);
    } else {
        let sub_items: Vec<ListItem> = state
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
                    Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(100, 100, 115))
                };

                ListItem::new(Line::from(vec![
                    Span::styled(radio, radio_style),
                    Span::styled(&sub.name, style),
                ]))
            })
            .collect();

        let mut sub_state = ListState::default().with_selected(Some(state.playback_sub_cursor));
        frame.render_stateful_widget(List::new(sub_items), sub_inner, &mut sub_state);
    }

    // ── Footer: Keymap Bar ───────────────────────────────────────────────
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(" Launch  "),
        Span::styled(" Tab/h/l ", Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
        Span::raw(" Column  "),
        Span::styled(" 1-4 ", Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
        Span::raw(" Jump  "),
        Span::styled(" j/k ", Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
        Span::raw(" Select  "),
        Span::styled(" p ", Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
        Span::raw(" Player  "),
        Span::styled(" a ", Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
        Span::raw(" Audio  "),
        Span::styled(" u ", Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
        Span::raw(" Anime4K  "),
        Span::styled(" Esc/q ", Style::default().fg(Color::White).bg(Color::Rgb(50, 50, 65))),
        Span::raw(" Back"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(180, 0, 255))),
    );
    frame.render_widget(footer, chunks[2]);
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
