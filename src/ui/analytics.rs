//! Personal "Anime Wrapped" & Taste Analytics Dashboard (`a`).
//!
//! Visual telemetry command center aggregating local SQLite watch history
//! and AniList metadata into real-time metrics: watch time, completion rates,
//! genre polarities, era distributions, top watched anime, and velocity sparklines.

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
    let area = centered_rect(94, 92, frame.area());
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
            Constraint::Length(4), // Persona Archetype & Viewing Cadence Banner
            Constraint::Length(4), // 6 KPI Stat Blocks
            Constraint::Min(10),   // Split Body (Left = Genres + Eras/Formats; Right = Top Titles + 14-Day Velocity)
            Constraint::Length(2), // Footer Keybinding hints
        ])
        .split(inner);

    // ── 1. Archetype & Viewing Cadence Banner ──────────────────────────────────
    let banner_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(52),
            Constraint::Percentage(48),
        ])
        .split(main_chunks[0]);

    let left_banner = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" TASTE ARCHETYPE: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                data.taste_archetype,
                Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            format!(
                " Synthesized from {} episodes across {} series in local catalog",
                data.total_episodes_watched,
                data.completed_shows + data.in_progress_shows
            ),
            Style::default().fg(Color::Rgb(120, 125, 145)),
        )),
    ]);
    frame.render_widget(left_banner, banner_cols[0]);

    let max_vel = data.recent_activity.iter().max().copied().unwrap_or(0);
    let right_banner = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" VIEWING CADENCE: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                data.viewing_cadence,
                Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" CURRENT STREAK:  ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!("◈ {} Day{} Active", data.active_streak_days, if data.active_streak_days == 1 { "" } else { "s" }),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  •  Peak Velocity: {} ep/day", max_vel),
                Style::default().fg(Color::Rgb(120, 125, 145)),
            ),
        ]),
    ]);
    frame.render_widget(right_banner, banner_cols[1]);

    // ── 2. Stat Blocks (Row of 6 KPI Cards) ───────────────────────────────────
    let stat_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
        ])
        .split(main_chunks[1]);

    let mins = (data.total_hours_watched * 60.0).round() as u64;
    let watch_str = format!("{:.1}h ({}m)", data.total_hours_watched, mins);
    render_stat_card(frame, stat_cols[0], "❖ WATCH TIME", &watch_str, theme.primary);
    render_stat_card(frame, stat_cols[1], "◈ EPISODES", &format!("{} ep", data.total_episodes_watched), theme.secondary);
    render_stat_card(
        frame,
        stat_cols[2],
        "✦ COMPLETED",
        &format!("{} / {} shows", data.completed_shows, data.completed_shows + data.in_progress_shows),
        theme.accent,
    );
    render_stat_card(
        frame,
        stat_cols[3],
        "⬡ COMPLETION",
        &format!("{:.0}%", data.completion_rate),
        Color::Rgb(80, 250, 123),
    );
    let score_str = if data.avg_score > 0.0 {
        format!("{:.1}%", data.avg_score)
    } else {
        "N/A".to_string()
    };
    render_stat_card(frame, stat_cols[4], "★ AVG SCORE", &score_str, Color::Rgb(255, 184, 108));
    render_stat_card(frame, stat_cols[5], "󰕾 AUDIO PREF", data.audio_preference, Color::Rgb(189, 147, 249));

    // ── 3. Split Body: Left = Genres & Eras/Formats; Right = Top Titles & Velocity ─
    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(main_chunks[2]);

    // ── Left Column: Genres (Top) and Eras & Formats (Bottom) ──────────────────
    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(55),
            Constraint::Percentage(45),
        ])
        .split(body_cols[0]);

    // Box 1: Genre Affinity Polarity with Elastic Bars (Zero Concatenation)
    let genre_card_width = left_rows[0].width as usize;
    let bar_width = genre_card_width.saturating_sub(34).clamp(8, 36);

    let mut genre_lines = vec![];
    if data.top_genres.is_empty() {
        genre_lines.push(Line::from(Span::styled(" No watched genres recorded yet.", Style::default().fg(theme.text_dim))));
    } else {
        for (g, count, pct) in &data.top_genres {
            let label = if g.chars().count() > 13 {
                format!("{}…", g.chars().take(12).collect::<String>())
            } else {
                g.clone()
            };
            let bar_len = ((*pct / 100.0) * bar_width as f64).round() as usize;
            let bar = "█".repeat(bar_len);
            let empty = "░".repeat(bar_width.saturating_sub(bar_len));
            genre_lines.push(Line::from(vec![
                Span::styled(format!(" {:<13} ", label), Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
                Span::styled(bar, Style::default().fg(theme.primary)),
                Span::styled(empty, Style::default().fg(theme.border_idle)),
                Span::styled(format!(" {:>4.1}% ", pct), Style::default().fg(theme.text_dim)),
                Span::styled(format!("({} ep)", count), Style::default().fg(Color::Rgb(100, 105, 130))),
            ]));
        }
    }
    let genre_widget = Paragraph::new(genre_lines).block(
        Block::default()
            .title(Span::styled(" ◈ GENRE AFFINITY POLARITY ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_idle)),
    );
    frame.render_widget(genre_widget, left_rows[0]);

    // Box 2: Golden Era & Formats Distribution
    let era_card_width = left_rows[1].width as usize;
    let era_bar_width = era_card_width.saturating_sub(44).clamp(8, 28);

    let mut era_lines = vec![];
    for (label, count, pct) in &data.era_distribution {
        let bar_len = ((*pct / 100.0) * era_bar_width as f64).round() as usize;
        let bar = "█".repeat(bar_len);
        let empty = "░".repeat(era_bar_width.saturating_sub(bar_len));
        era_lines.push(Line::from(vec![
            Span::styled(format!(" {:<22} ", label), Style::default().fg(theme.text)),
            Span::styled(bar, Style::default().fg(theme.secondary)),
            Span::styled(empty, Style::default().fg(theme.border_idle)),
            Span::styled(format!(" {:>3.0}% ", pct), Style::default().fg(theme.text_dim)),
            Span::styled(format!("({} title{})", count, if *count == 1 { "" } else { "s" }), Style::default().fg(Color::Rgb(100, 105, 130))),
        ]));
    }

    // Add format segmented bar
    let total_formats_count: usize = data.format_distribution.iter().map(|(_, c)| *c).sum();
    let fmt_div = total_formats_count.max(1) as f64;
    let tv_cnt = data.format_distribution.iter().find(|(f, _)| f.eq_ignore_ascii_case("TV")).map(|(_, c)| *c).unwrap_or(0);
    let ona_cnt = data.format_distribution.iter().find(|(f, _)| f.eq_ignore_ascii_case("ONA")).map(|(_, c)| *c).unwrap_or(0);
    let mov_cnt = data.format_distribution.iter().find(|(f, _)| f.eq_ignore_ascii_case("MOVIE")).map(|(_, c)| *c).unwrap_or(0);

    let tv_pct = (tv_cnt as f64 / fmt_div) * 100.0;
    let ona_pct = (ona_cnt as f64 / fmt_div) * 100.0;

    let sep_len = era_card_width.saturating_sub(4).clamp(10, 60);
    era_lines.push(Line::from(Span::styled("─".repeat(sep_len), Style::default().fg(Color::Rgb(40, 44, 60)))));

    let fmt_bar_width = era_card_width.saturating_sub(32).clamp(10, 36);
    let tv_bar_len = ((tv_pct / 100.0) * fmt_bar_width as f64).round() as usize;
    let ona_bar_len = fmt_bar_width.saturating_sub(tv_bar_len);

    era_lines.push(Line::from(vec![
        Span::styled(" Format Ratio: ", Style::default().fg(theme.text_dim)),
        Span::styled("█".repeat(tv_bar_len), Style::default().fg(theme.accent)),
        Span::styled("░".repeat(ona_bar_len), Style::default().fg(theme.secondary)),
        Span::styled(format!(" TV {:.0}% / ONA {:.0}%", tv_pct, ona_pct), Style::default().fg(theme.text)),
    ]));
    era_lines.push(Line::from(Span::styled(
        format!(" Breakdown: {} TV Series • {} Web ONA • {} Films", tv_cnt, ona_cnt, mov_cnt),
        Style::default().fg(Color::Rgb(110, 115, 135)),
    )));

    let era_widget = Paragraph::new(era_lines).block(
        Block::default()
            .title(Span::styled(" ❖ GOLDEN ERA & FORMAT DISTRIBUTION ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_idle)),
    );
    frame.render_widget(era_widget, left_rows[1]);

    // ── Right Column: Top Watched Anime (Top) and 14-Day Velocity (Bottom) ─────
    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(52),
            Constraint::Percentage(48),
        ])
        .split(body_cols[1]);

    // Box 3: Heavy Rotation / Top Watched Titles
    let titles_card_width = right_rows[0].width as usize;
    let mut title_lines = vec![];
    if data.top_shows.is_empty() {
        title_lines.push(Line::from(Span::styled(" No anime playback recorded yet.", Style::default().fg(theme.text_dim))));
    } else {
        let title_icons = ["❖", "◈", "✦", "⬡"];
        let prog_bar_width = titles_card_width.saturating_sub(34).clamp(8, 24);

        for (idx, show) in data.top_shows.iter().enumerate() {
            let icon = title_icons.get(idx).copied().unwrap_or("•");
            let max_title_chars = titles_card_width.saturating_sub(8);
            let display_title = if show.title.chars().count() > max_title_chars {
                format!("{}…", show.title.chars().take(max_title_chars.saturating_sub(1)).collect::<String>())
            } else {
                show.title.clone()
            };

            title_lines.push(Line::from(vec![
                Span::styled(format!(" {}. {} ", idx + 1, icon), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                Span::styled(display_title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]));

            let bar_len = ((show.completion_pct / 100.0) * prog_bar_width as f64).round() as usize;
            let bar = "█".repeat(bar_len);
            let empty = "░".repeat(prog_bar_width.saturating_sub(bar_len));
            let ep_str = match show.total_episodes {
                Some(tot) => format!("[{:>2}/{} ep • {:>3.0}%]", show.episodes_watched, tot, show.completion_pct),
                None => format!("[{:>2} ep watched]", show.episodes_watched),
            };

            title_lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(bar, Style::default().fg(theme.secondary)),
                Span::styled(empty, Style::default().fg(theme.border_idle)),
                Span::styled(format!(" {} ", ep_str), Style::default().fg(theme.text_dim)),
                Span::styled(format!("{:.1}h", show.total_hours), Style::default().fg(theme.accent)),
            ]));
        }

        title_lines.push(Line::from(""));
        title_lines.push(Line::from(Span::styled(
            format!(
                " Active Catalog: {} Series in Flight • {} Completed Shows",
                data.in_progress_shows, data.completed_shows
            ),
            Style::default().fg(Color::Rgb(110, 115, 135)),
        )));
    }

    let titles_widget = Paragraph::new(title_lines).block(
        Block::default()
            .title(Span::styled(" ❖ HEAVY ROTATION / TOP WATCHED TITLES ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_idle)),
    );
    frame.render_widget(titles_widget, right_rows[0]);

    // Box 4: 14-Day Velocity & Binge Matrix
    let velocity_card_width = right_rows[1].width as usize;
    let sparkline_block = Block::default()
        .title(Span::styled(
            " 󰓅 14-DAY VELOCITY & BINGE MATRIX ",
            Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle));

    let sparkline_inner = sparkline_block.inner(right_rows[1]);
    frame.render_widget(sparkline_block, right_rows[1]);

    let spark_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Stats Header
            Constraint::Length(3), // Sparkline Bars
            Constraint::Length(1), // Timeline tick marks
            Constraint::Min(0),    // Insights paragraph
        ])
        .split(sparkline_inner);

    let total_14d_eps: u64 = data.recent_activity.iter().sum();
    let spark_header = Paragraph::new(Line::from(vec![
        Span::styled(format!(" Peak: {} ep/day ", max_vel), Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
        Span::styled(format!("• 14-Day Volume: {} ep ", total_14d_eps), Style::default().fg(theme.text)),
        Span::styled("• 14 days ago 󰁔 today", Style::default().fg(theme.text_dim)),
    ]));
    frame.render_widget(spark_header, spark_layout[0]);

    let sparkline = Sparkline::default()
        .data(&data.recent_activity)
        .style(Style::default().fg(Color::Rgb(80, 250, 123)));
    frame.render_widget(sparkline, spark_layout[1]);

    // Day tick markers across the sparkline base
    let tick_sep = velocity_card_width.saturating_sub(4).clamp(10, 56);
    let tick_label = if tick_sep >= 40 {
        " 14d   12d   10d   8d    6d    4d    2d   Today"
    } else {
        " 14d ─── 7d ─── Today"
    };
    let timeline_widget = Paragraph::new(Line::from(Span::styled(tick_label, Style::default().fg(Color::Rgb(80, 85, 105)))));
    frame.render_widget(timeline_widget, spark_layout[2]);

    let insight_text = Paragraph::new(vec![
        Line::from(Span::styled(
            format!(" Binge Cadence: {} session specialist with {} peak episodes", data.viewing_cadence.split('(').next().unwrap_or("Active"), max_vel),
            Style::default().fg(theme.text_dim),
        )),
        Line::from(Span::styled(
            " Hardware Pipeline: Anime4K Realtime Shaders Active • OLED Minimal Subtitles",
            Style::default().fg(Color::Rgb(110, 115, 135)),
        )),
    ]);
    frame.render_widget(insight_text, spark_layout[3]);

    // ── 4. Footer hints ────────────────────────────────────────────────────────
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" [Esc / q / a] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Return to Home    ", Style::default().fg(theme.text)),
        Span::styled(" [r] ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
        Span::styled("Recalculate Live Telemetry    ", Style::default().fg(theme.text)),
        Span::styled(" [s] ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("Settings Hub", Style::default().fg(theme.text)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::user::{TopWatchedAnime, UserAnalytics};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_render_analytics_layout_option_a() {
        let backend = TestBackend::new(140, 45);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.analytics_data = Some(UserAnalytics {
            total_episodes_watched: 16,
            total_hours_watched: 5.8,
            completed_shows: 0,
            in_progress_shows: 3,
            completion_rate: 0.0,
            avg_score: 81.3,
            top_genres: vec![
                ("Cyberpunk".to_string(), 2, 4.7),
                ("Denpa".to_string(), 2, 4.7),
                ("Drama".to_string(), 2, 4.7),
                ("Sci-Fi".to_string(), 2, 4.7),
                ("Urban".to_string(), 2, 4.7),
                ("Action".to_string(), 1, 2.3),
            ],
            era_distribution: vec![
                ("Pre-2000 (Golden Age)", 1, 33.0),
                ("2000s (Classic Era)", 0, 0.0),
                ("2010s (Modern Era)", 0, 0.0),
                ("2020s (Current Era)", 2, 67.0),
            ],
            format_distribution: vec![("TV".to_string(), 2), ("ONA".to_string(), 1)],
            recent_activity: vec![0, 0, 0, 0, 0, 0, 2, 10, 0, 0, 1, 3, 0, 0],
            taste_archetype: "✦ Eclectic Polymath",
            top_shows: vec![
                TopWatchedAnime {
                    id: 1,
                    title: "Cyberpunk: Edgerunners".to_string(),
                    episodes_watched: 10,
                    total_episodes: Some(10),
                    total_hours: 3.6,
                    completion_pct: 100.0,
                },
                TopWatchedAnime {
                    id: 2,
                    title: "Frieren: Beyond Journey's End".to_string(),
                    episodes_watched: 4,
                    total_episodes: Some(28),
                    total_hours: 1.5,
                    completion_pct: 14.3,
                },
            ],
            viewing_cadence: "❖ Night Owl (23:00 - 04:00)",
            active_streak_days: 2,
            audio_preference: "Japanese Sub",
        });

        terminal
            .draw(|frame| {
                render(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered_text: String = buffer
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();

        assert!(rendered_text.contains("SOLUS // ANIME WRAPPED"));
        assert!(rendered_text.contains("Eclectic Polymath"));
        assert!(rendered_text.contains("Night Owl"));
        assert!(rendered_text.contains("Cyberpunk: Edgerunners"));
        assert!(rendered_text.contains("Frieren"));
        assert!(rendered_text.contains("Format Ratio"));
        assert!(rendered_text.contains("14-DAY VELOCITY"));
    }
}
