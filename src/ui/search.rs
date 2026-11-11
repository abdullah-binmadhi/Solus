//! Search overlay — floats centered over the current screen.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::state::AppState;

/// Render the search overlay on top of whatever screen is below.
pub fn render_overlay(frame: &mut Frame, state: &mut AppState) {
    let area = centered_rect(76, 75, frame.area());

    // Clear the background area so it's not transparent
    frame.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // input box
            Constraint::Length(10), // focused result preview (with cover thumbnail)
            Constraint::Min(0),    // results list
        ])
        .split(area);

    render_input(frame, chunks[0], state);
    render_preview(frame, chunks[1], state);
    render_results(frame, chunks[2], state);
}

/// The search input box.
fn render_input(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();
    let query_display = format!("{}_", state.search_query); // cursor indicator

    let input = Paragraph::new(Span::styled(
        &query_display,
        Style::default().fg(Color::White),
    ))
    .block(
        Block::default()
            .title(Span::styled(
                "  Search Anime ",
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.primary)),
    );
    frame.render_widget(input, area);
}

/// The results list below the input.
fn render_results(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();
    let block = Block::default()
        .title(Span::styled(
            " 󰘳 Results ",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle));

    if state.search_results.is_empty() {
        let msg = if state.search_query.is_empty() {
            "Type to search anime across AniList..."
        } else {
            "No matching anime found."
        };
        let para = Paragraph::new(Span::styled(
            msg,
            Style::default().fg(theme.text_dim),
        ))
        .block(block);
        frame.render_widget(para, area);
        return;
    }

    let items: Vec<ListItem> = state
        .search_results
        .iter()
        .enumerate()
        .map(|(i, anime)| {
            let score = anime
                .score
                .map(|s| format!("{:.1}", s as f32 / 10.0))
                .unwrap_or_else(|| "N/A".to_string());
            let eps = anime
                .episodes
                .map(|e| format!("{} eps", e))
                .unwrap_or_else(|| "? eps".to_string());

            let style = if i == state.search_cursor {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };

            let prefix = if i == state.search_cursor { "▸ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}{}", prefix, anime.display_title()), style),
                Span::styled(
                    format!("   {}  {}", score, eps),
                    Style::default().fg(theme.text_dim),
                ),
            ]))
        })
        .collect();

    let mut list_state = ListState::default().with_selected(Some(state.search_cursor));
    frame.render_stateful_widget(
        List::new(items).block(block),
        area,
        &mut list_state,
    );
}

fn render_preview(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let theme = state.theme();
    let block = Block::default()
        .title(Span::styled(
            "  Focused Result ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(anime) = state.search_results.get(state.search_cursor).cloned() else {
        let empty = Paragraph::new(Span::styled(
            "Use ↑/↓ to browse results, Enter to view details.",
            Style::default().fg(theme.text_dim),
        ));
        frame.render_widget(empty, inner);
        return;
    };

    let score = anime
        .score
        .map(|s| format!(" {:.1}", s as f32 / 10.0))
        .unwrap_or_else(|| " N/A".to_string());
    let eps = anime
        .episodes
        .map(|e| format!("{} eps", e))
        .unwrap_or_else(|| "? eps".to_string());
    let format = anime.format.as_deref().unwrap_or("TV");
    let status = anime.status.as_deref().unwrap_or("Unknown");
    let desc = anime
        .description
        .as_deref()
        .unwrap_or("No description available.")
        .chars()
        .take(180)
        .collect::<String>();

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(14), // Cover thumbnail
            Constraint::Length(1),  // Spacer
            Constraint::Min(0),     // Info
        ])
        .split(inner);

    if state.has_image_support() {
        if let Some(cover) = state.detail_cache.get_mut(&anime.id).or_else(|| state.banner_cache.get_mut(&anime.id)).or_else(|| state.cover_cache.get_mut(&anime.id)) {
            let image_widget = ratatui_image::StatefulImage::new(None)
                .resize(ratatui_image::Resize::Fit(None));
            frame.render_stateful_widget(image_widget, cols[0], cover);
        } else {
            frame.render_widget(
                crate::ui::components::cover::HalfblockCover { anime_id: anime.id, title: anime.display_title() },
                cols[0],
            );
        }
    } else {
        frame.render_widget(
            crate::ui::components::cover::HalfblockCover { anime_id: anime.id, title: anime.display_title() },
            cols[0],
        );
    }

    let preview = Paragraph::new(vec![
        Line::from(Span::styled(
            anime.display_title(),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("{}  ·  {}  ·  {}", score, eps, format),
            Style::default().fg(theme.primary),
        )),
        Line::from(Span::styled(
            status,
            Style::default().fg(theme.text_dim),
        )),
        Line::from(""),
        Line::from(Span::styled(
            desc,
            Style::default().fg(theme.text),
        )),
    ])
    .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(preview, cols[2]);
}

/// Returns a Rect centered on `r` with given width% and height%.
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
