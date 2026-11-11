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
    let area = centered_rect(68, 64, frame.area());
    frame.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Header info
            Constraint::Min(0),    // Dual-column body (Quality + Subtitles)
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
        ]),
        Line::from(vec![
            Span::styled(" Audio Mode: ", Style::default().fg(Color::Rgb(160, 160, 175))),
            Span::styled(
                format!(" {} ", audio_label),
                Style::default()
                    .fg(Color::White)
                    .bg(if state.pending_dub { Color::Rgb(120, 40, 180) } else { Color::Rgb(40, 40, 60) })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "([a] toggle)",
                Style::default().fg(Color::Rgb(120, 120, 135)),
            ),
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

    // Split body into Quality (left) and Subtitle Languages (right)
    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(42),
            Constraint::Percentage(58),
        ])
        .split(chunks[1]);

    // ── Left: Quality Selection ──────────────────────────────────────────
    let q_focused = !state.playback_sub_focused;
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

    let q_inner = q_block.inner(body_cols[0]);
    frame.render_widget(q_block, body_cols[0]);

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

    // ── Right: Subtitle Language Selection ───────────────────────────────
    let sub_focused = state.playback_sub_focused;
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

    let sub_inner = sub_block.inner(body_cols[1]);
    frame.render_widget(sub_block, body_cols[1]);

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
        Span::raw(" Focus  "),
        Span::styled(" j/k ", Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
        Span::raw(" Select  "),
        Span::styled(" a ", Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
        Span::raw(" Audio  "),
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
