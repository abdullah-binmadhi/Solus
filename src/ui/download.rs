//! Offline episode download modal overlay.

use crate::state::AppState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

/// Center a rect within another rect by percentage.
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

pub fn render_download_modal(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(56, 46, frame.area());
    frame.render_widget(Clear, area);

    let anime_title = state
        .selected_anime
        .as_ref()
        .map(|a| a.display_title())
        .unwrap_or("Anime");
    let selected_ep = state.selected_episode.unwrap_or(1);
    let max_aired = state
        .selected_anime
        .as_ref()
        .map(|a| a.aired_episodes())
        .unwrap_or(1);

    let unwatched_count = (1..=max_aired)
        .filter(|ep| !state.watched_episodes.contains(ep))
        .count();

    let block = Block::default()
        .title(Span::styled(
            " 󰇚 Offline Episode Downloader (Shift+D) ",
            Style::default()
                .fg(Color::Rgb(139, 233, 253))
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(139, 233, 253)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Anime title and vault path info
            Constraint::Length(1), // Spacer
            Constraint::Length(3), // Option 0: Current episode
            Constraint::Length(3), // Option 1: Episode range
            Constraint::Length(3), // Option 2: All unwatched
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Footer hints
        ])
        .split(inner);

    // Header info
    let header_lines = vec![
        Line::from(vec![
            Span::styled("Target Vault: ", Style::default().fg(Color::Rgb(160, 160, 180))),
            Span::styled(
                format!("~/Movies/Solus/{}/", crate::services::downloader::sanitize_filename(anime_title)),
                Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Offline Ready: ", Style::default().fg(Color::Rgb(160, 160, 180))),
            Span::styled(
                format!("{} / {} episodes downloaded", state.offline_episodes.len(), max_aired),
                Style::default().fg(Color::Rgb(241, 250, 140)),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(header_lines), chunks[0]);

    // Option 0: Current Episode
    let is_curr = state.download_modal_mode == 0;
    let curr_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if is_curr { Color::Rgb(139, 233, 253) } else { Color::Rgb(60, 60, 80) }));
    let curr_inner = curr_block.inner(chunks[2]);
    frame.render_widget(curr_block, chunks[2]);
    let curr_p = Paragraph::new(Line::from(vec![
        Span::styled(
            if is_curr { " ▶ " } else { "   " },
            Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("Download Current: Episode {}", selected_ep),
            if is_curr {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 180, 190))
            },
        ),
    ]));
    frame.render_widget(curr_p, curr_inner);

    // Option 1: Episode Range
    let is_range = state.download_modal_mode == 1;
    let range_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if is_range { Color::Rgb(139, 233, 253) } else { Color::Rgb(60, 60, 80) }));
    let range_inner = range_block.inner(chunks[3]);
    frame.render_widget(range_block, chunks[3]);

    let input_display = if state.download_range_input.is_empty() {
        if is_range { "Type e.g. 1-12 or 1-4█".to_string() } else { "e.g. 1-12".to_string() }
    } else if is_range {
        format!("{}█", state.download_range_input)
    } else {
        state.download_range_input.clone()
    };

    let range_p = Paragraph::new(Line::from(vec![
        Span::styled(
            if is_range { " ▶ " } else { "   " },
            Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Episode Range: ",
            if is_range {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 180, 190))
            },
        ),
        Span::styled(
            format!("[ {} ]", input_display),
            Style::default().fg(Color::Rgb(255, 121, 198)).add_modifier(Modifier::BOLD),
        ),
    ]));
    frame.render_widget(range_p, range_inner);

    // Option 2: All Unwatched Episodes
    let is_all = state.download_modal_mode == 2;
    let all_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if is_all { Color::Rgb(139, 233, 253) } else { Color::Rgb(60, 60, 80) }));
    let all_inner = all_block.inner(chunks[4]);
    frame.render_widget(all_block, chunks[4]);
    let all_p = Paragraph::new(Line::from(vec![
        Span::styled(
            if is_all { " ▶ " } else { "   " },
            Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("Download All Unwatched ({} episodes remaining)", unwatched_count),
            if is_all {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 180, 190))
            },
        ),
    ]));
    frame.render_widget(all_p, all_inner);

    // Footer hints
    let footer_lines = vec![
        Line::from(vec![
            Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD)),
            Span::raw(" Queue Download  "),
            Span::styled(" [j/k] ", Style::default().fg(Color::Rgb(200, 200, 220)).add_modifier(Modifier::BOLD)),
            Span::raw(" Select Mode  "),
            Span::styled(" [Esc] ", Style::default().fg(Color::Rgb(200, 200, 220)).add_modifier(Modifier::BOLD)),
            Span::raw(" Cancel"),
        ]),
    ];
    frame.render_widget(Paragraph::new(footer_lines), chunks[6]);
}
