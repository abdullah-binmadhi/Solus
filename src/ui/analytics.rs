//! Personal "Anime Wrapped" & Taste Analytics Dashboard (`a`).
//!
//! Visual telemetry command center aggregating local SQLite watch history
//! and AniList metadata into real-time metrics: watch time, completion rates,
//! genre polarities, era distributions, and velocity sparklines.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Sparkline},
    Frame,
};

use crate::state::AppState;

/// Render the Taste Analytics / Anime Wrapped dashboard.
pub fn render(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(84, 88, frame.area());
    frame.render_widget(Clear, area);

    let theme = state.theme();

    let block = Block::default()
        .title(Span::styled(
            " ❖ SOLUS // ANIME WRAPPED & TASTE TELEMETRY ❖ ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(ref data) = state.analytics_data else {
        let loading = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  󰑮 Calculating viewing telemetry from local watch history...",
                Style::default().fg(theme.text_dim),
            )),
        ]);
        frame.render_widget(loading, inner);
        return;
    };

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Persona Archetype Banner
            Constraint::Length(5), // Top Stat Blocks
            Constraint::Min(0),    // Split Body (Genres/Eras on left, Formats/Sparkline on right)
            Constraint::Length(2), // Footer Keybinding hints
        ])
        .split(inner);

    // 1. Archetype Banner
    let banner = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" TASTE ARCHETYPE: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                data.taste_archetype,
                Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            "Synthesized from your complete local playback telemetry and AniList catalog",
            Style::default().fg(Color::Rgb(120, 120, 140)),
        )),
    ]);
    frame.render_widget(banner, main_chunks[0]);

    // 2. Stat Blocks
    let stat_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(main_chunks[1]);

    render_stat_card(frame, stat_cols[0], "WATCH TIME", &format!("{:.1}h", data.total_hours_watched), theme.primary);
    render_stat_card(frame, stat_cols[1], "EPISODES", &format!("{}", data.total_episodes_watched), theme.secondary);
    render_stat_card(frame, stat_cols[2], "COMPLETED", &format!("{} shows", data.completed_shows), theme.accent);
    render_stat_card(frame, stat_cols[3], "COMPLETION", &format!("{:.0}%", data.completion_rate), Color::Rgb(80, 250, 123));
    let score_str = if data.avg_score > 0.0 { format!("{:.1}%", data.avg_score) } else { "N/A".to_string() };
    render_stat_card(frame, stat_cols[4], "AVG SCORE", &score_str, Color::Rgb(255, 184, 108));

    // 3. Body: Left = Genres & Eras; Right = Formats & Velocity Sparkline
    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(52),
            Constraint::Percentage(48),
        ])
        .split(main_chunks[2]);

    // Left Column: Genres and Eras
    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(58),
            Constraint::Percentage(42),
        ])
        .split(body_cols[0]);

    // Genres Box
    let mut genre_lines = vec![];
    if data.top_genres.is_empty() {
        genre_lines.push(Line::from(Span::styled("No watched genres recorded yet.", Style::default().fg(theme.text_dim))));
    } else {
        for (g, count, pct) in &data.top_genres {
            let bar_len = ((*pct / 100.0) * 16.0).round() as usize;
            let bar: String = "█".repeat(bar_len);
            let empty: String = "░".repeat(16usize.saturating_sub(bar_len));
            genre_lines.push(Line::from(vec![
                Span::styled(format!(" {:<14} ", g), Style::default().fg(theme.text)),
                Span::styled(bar, Style::default().fg(theme.primary)),
                Span::styled(empty, Style::default().fg(theme.border_idle)),
                Span::styled(format!(" {:>4.1}% ", pct), Style::default().fg(theme.text_dim)),
                Span::styled(format!("({} ep)", count), Style::default().fg(Color::Rgb(100, 100, 120))),
            ]));
        }
    }
    let genre_widget = Paragraph::new(genre_lines).block(
        Block::default()
            .title(Span::styled(" ◈ GENRE AFFINITY POLARITY ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_idle)),
    );
    frame.render_widget(genre_widget, left_rows[0]);

    // Eras Box
    let mut era_lines = vec![];
    for (label, count, pct) in &data.era_distribution {
        let bar_len = ((*pct / 100.0) * 14.0).round() as usize;
        let bar: String = "█".repeat(bar_len);
        let empty: String = "░".repeat(14usize.saturating_sub(bar_len));
        era_lines.push(Line::from(vec![
            Span::styled(format!(" {:<22} ", label), Style::default().fg(theme.text)),
            Span::styled(bar, Style::default().fg(theme.secondary)),
            Span::styled(empty, Style::default().fg(theme.border_idle)),
            Span::styled(format!(" {:>3.0}% ", pct), Style::default().fg(theme.text_dim)),
            Span::styled(format!("({} titles)", count), Style::default().fg(Color::Rgb(100, 100, 120))),
        ]));
    }
    let era_widget = Paragraph::new(era_lines).block(
        Block::default()
            .title(Span::styled(" ❖ GOLDEN ERA DISTRIBUTION ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_idle)),
    );
    frame.render_widget(era_widget, left_rows[1]);

    // Right Column: Formats & Sparkline
    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Percentage(55),
        ])
        .split(body_cols[1]);

    // Formats Box
    let mut format_lines = vec![];
    for (fmt, count) in &data.format_distribution {
        format_lines.push(Line::from(vec![
            Span::styled(format!(" 󰈹 {:<8} ", fmt), Style::default().fg(theme.text)),
            Span::styled(format!("{} titles watched", count), Style::default().fg(theme.accent)),
        ]));
    }
    if format_lines.is_empty() {
        format_lines.push(Line::from(Span::styled("No formats recorded yet.", Style::default().fg(theme.text_dim))));
    }
    let format_widget = Paragraph::new(format_lines).block(
        Block::default()
            .title(Span::styled(" 󰈹 FORMATS ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_idle)),
    );
    frame.render_widget(format_widget, right_rows[0]);

    // Sparkline Box
    let sparkline_block = Block::default()
        .title(Span::styled(" 󰓅 14-DAY VELOCITY (EPISODES/DAY) ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_idle));
    let sparkline_inner = sparkline_block.inner(right_rows[1]);
    frame.render_widget(sparkline_block, right_rows[1]);

    let spark_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(sparkline_inner);

    let max_vel = data.recent_activity.iter().max().copied().unwrap_or(0);
    let spark_header = Paragraph::new(Line::from(vec![
        Span::styled(format!(" Peak Velocity: {} ep/day ", max_vel), Style::default().fg(Color::Rgb(80, 250, 123))),
        Span::styled(" 14 days ago 󰁔 today ", Style::default().fg(theme.text_dim)),
    ]));
    frame.render_widget(spark_header, spark_layout[0]);

    let sparkline = Sparkline::default()
        .data(&data.recent_activity)
        .style(Style::default().fg(Color::Rgb(80, 250, 123)));
    frame.render_widget(sparkline, spark_layout[1]);

    let insight_text = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(" Active in-progress queue: {} titles", data.in_progress_shows),
            Style::default().fg(theme.text_dim),
        )),
        Line::from(Span::styled(
            " Pro tip: Anime4K GPU upscaling delivers native 4K at 60fps.",
            Style::default().fg(Color::Rgb(140, 140, 160)),
        )),
    ]);
    frame.render_widget(insight_text, spark_layout[2]);

    // 4. Footer hints
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" [Esc / q / a] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Return to Home   ", Style::default().fg(theme.text)),
        Span::styled(" [r] ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
        Span::styled("Refresh Telemetry", Style::default().fg(theme.text)),
    ]));
    frame.render_widget(footer, main_chunks[3]);
}

fn render_stat_card(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    color: Color,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));
    let p = Paragraph::new(vec![
        Line::from(Span::styled(label, Style::default().fg(Color::Rgb(150, 150, 165)))),
        Line::from(Span::styled(value, Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
    ])
    .block(block);
    frame.render_widget(p, area);
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
