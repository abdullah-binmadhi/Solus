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
    let area = frame.area();
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
            Constraint::Length(4), // 6 KPI Stat Cards
            Constraint::Min(20),   // Split Body (Left = Genres + Eras/Formats; Right = Top Titles + 14-Day Velocity)
            Constraint::Length(2), // Footer Keybinding hints
        ])
        .split(inner);

    // ── 1. Archetype & Viewing Cadence Banner ──────────────────────────────────
    let banner_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(main_chunks[0]);

    let left_banner = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" ❖ TASTE ARCHETYPE: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                data.taste_archetype,
                Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            format!(
                "   Synthesized from {} episodes across {} series in local Solus catalog",
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
            Span::styled(" ◈ VIEWING CADENCE: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                data.viewing_cadence,
                Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("   CURRENT STREAK:  ", Style::default().fg(theme.text_dim)),
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
            Constraint::Ratio(1, 2),
            Constraint::Ratio(1, 2),
        ])
        .split(body_cols[0]);

    // Box 1: Genre Affinity Polarity with Relative Normalization (Zero Dead Space & Concatenation)
    let genre_card_width = left_rows[0].width as usize;
    let bar_width = genre_card_width.saturating_sub(38).clamp(8, 44);

    let mut genre_lines = vec![];
    if data.top_genres.is_empty() {
        genre_lines.push(Line::from(Span::styled(" No watched genres recorded yet.", Style::default().fg(theme.text_dim))));
    } else {
        let max_cnt = data.top_genres.first().map(|(_, c, _)| *c).unwrap_or(1) as f64;
        for (g, count, pct) in data.top_genres.iter().take(8) {
            let label = if g.chars().count() > 14 {
                format!("{}…", g.chars().take(13).collect::<String>())
            } else {
                g.clone()
            };
            let rel = (*count as f64) / max_cnt;
            let bar_len = (rel * bar_width as f64).round() as usize;
            let bar = "█".repeat(bar_len);
            let empty = "░".repeat(bar_width.saturating_sub(bar_len));
            genre_lines.push(Line::from(vec![
                Span::styled(format!(" {:<14} ", label), Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
                Span::styled(bar, Style::default().fg(theme.primary)),
                Span::styled(empty, Style::default().fg(Color::Rgb(45, 48, 65))),
                Span::styled(format!(" {:>4.1}% ", pct), Style::default().fg(theme.text_dim)),
                Span::styled(format!("({:>2} ep)", count), Style::default().fg(Color::Rgb(100, 105, 130))),
            ]));
        }
        if data.top_genres.len() < 7 {
            genre_lines.push(Line::from(""));
            genre_lines.push(Line::from(Span::styled(
                format!(" ✦ Affinity Depth: {} distinct genre vectors indexed in catalog", data.top_genres.len()),
                Style::default().fg(Color::Rgb(110, 115, 135)),
            )));
            genre_lines.push(Line::from(Span::styled(
                " ◈ Polarity: Preference dynamically computed from weighted watch duration",
                Style::default().fg(Color::Rgb(90, 95, 115)),
            )));
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

    // Box 2: Golden Era & Formats Distribution Matrix
    let era_card_width = left_rows[1].width as usize;
    let era_bar_width = era_card_width.saturating_sub(44).clamp(8, 32);

    let mut era_lines = vec![];
    let max_era_cnt = data.era_distribution.iter().map(|(_, c, _)| *c).max().unwrap_or(1) as f64;
    for (label, count, pct) in &data.era_distribution {
        let rel = if max_era_cnt > 0.0 { *count as f64 / max_era_cnt } else { 0.0 };
        let bar_len = (rel * era_bar_width as f64).round() as usize;
        let bar = "█".repeat(bar_len);
        let empty = "░".repeat(era_bar_width.saturating_sub(bar_len));
        era_lines.push(Line::from(vec![
            Span::styled(format!(" {:<22} ", label), Style::default().fg(theme.text)),
            Span::styled(bar, Style::default().fg(theme.secondary)),
            Span::styled(empty, Style::default().fg(Color::Rgb(45, 48, 65))),
            Span::styled(format!(" {:>3.0}% ", pct), Style::default().fg(theme.text_dim)),
            Span::styled(format!("({:>2} title{})", count, if *count == 1 { "" } else { "s" }), Style::default().fg(Color::Rgb(100, 105, 130))),
        ]));
    }

    // Add format segmented bar and breakdown
    let total_formats_count: usize = data.format_distribution.iter().map(|(_, c)| *c).sum();
    let fmt_div = total_formats_count.max(1) as f64;
    let tv_cnt = data.format_distribution.iter().find(|(f, _)| f.eq_ignore_ascii_case("TV")).map(|(_, c)| *c).unwrap_or(0);
    let ona_cnt = data.format_distribution.iter().find(|(f, _)| f.eq_ignore_ascii_case("ONA")).map(|(_, c)| *c).unwrap_or(0);
    let mov_cnt = data.format_distribution.iter().find(|(f, _)| f.eq_ignore_ascii_case("MOVIE")).map(|(_, c)| *c).unwrap_or(0);

    let tv_pct = (tv_cnt as f64 / fmt_div) * 100.0;
    let ona_pct = (ona_cnt as f64 / fmt_div) * 100.0;

    let sep_len = era_card_width.saturating_sub(4).clamp(10, 70);
    era_lines.push(Line::from(Span::styled("─".repeat(sep_len), Style::default().fg(Color::Rgb(40, 44, 60)))));

    let fmt_bar_width = era_card_width.saturating_sub(34).clamp(10, 38);
    let tv_bar_len = ((tv_pct / 100.0) * fmt_bar_width as f64).round() as usize;
    let ona_bar_len = fmt_bar_width.saturating_sub(tv_bar_len);

    era_lines.push(Line::from(vec![
        Span::styled(" Format Ratio: ", Style::default().fg(theme.text_dim)),
        Span::styled("█".repeat(tv_bar_len), Style::default().fg(theme.accent)),
        Span::styled("░".repeat(ona_bar_len), Style::default().fg(theme.secondary)),
        Span::styled(format!(" TV {:.0}% / ONA {:.0}%", tv_pct, ona_pct), Style::default().fg(theme.text)),
    ]));
    era_lines.push(Line::from(Span::styled(
        format!(" Breakdown: {} TV Series • {} Web ONA • {} Feature Films", tv_cnt, ona_cnt, mov_cnt),
        Style::default().fg(Color::Rgb(110, 115, 135)),
    )));
    era_lines.push(Line::from(Span::styled(
        " ✦ Production Horizon: Modern & Current Era episodic productions lead viewing history",
        Style::default().fg(Color::Rgb(90, 95, 115)),
    )));

    let era_widget = Paragraph::new(era_lines).block(
        Block::default()
            .title(Span::styled(" ❖ GOLDEN ERA & FORMAT MATRIX ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_idle)),
    );
    frame.render_widget(era_widget, left_rows[1]);

    // ── Right Column: Top Watched Anime (Top) and 14-Day Velocity (Bottom) ─────
    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 2),
            Constraint::Ratio(1, 2),
        ])
        .split(body_cols[1]);

    // Box 3: Heavy Rotation / Top Watched Titles (Multi-line rich cards, zero dead space)
    let titles_card_width = right_rows[0].width as usize;
    let mut title_lines = vec![];
    let title_icons = ["❖", "◈", "✦", "⬡"];
    let prog_bar_width = titles_card_width.saturating_sub(36).clamp(8, 30);

    for (idx, show) in data.top_shows.iter().take(4).enumerate() {
        let icon = title_icons.get(idx).copied().unwrap_or("•");
        let max_title_chars = titles_card_width.saturating_sub(8);
        let display_title = if show.title.chars().count() > max_title_chars {
            format!("{}…", show.title.chars().take(max_title_chars.saturating_sub(1)).collect::<String>())
        } else {
            show.title.clone()
        };

        // Line 1: Rank & Title
        title_lines.push(Line::from(vec![
            Span::styled(format!(" {}. {} ", idx + 1, icon), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(display_title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));

        // Line 2: Progress Bar & Ep counts
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
            Span::styled(empty, Style::default().fg(Color::Rgb(45, 48, 65))),
            Span::styled(format!(" {} ", ep_str), Style::default().fg(theme.text_dim)),
            Span::styled(format!("{:.1}h", show.total_hours), Style::default().fg(theme.accent)),
        ]));

        // Line 3: Telemetry & Pace
        let avg_ep_mins = if show.episodes_watched > 0 {
            format!("{}m/ep", ((show.total_hours * 60.0) / show.episodes_watched as f64).round() as u64)
        } else {
            "24m/ep".to_string()
        };
        let status_desc = if show.completion_pct >= 99.0 { "Completed" } else { "In Rotation" };
        title_lines.push(Line::from(vec![
            Span::styled(format!("    ▸ Pace: {}  •  Status: {}  •  AniList Index", avg_ep_mins, status_desc), Style::default().fg(Color::Rgb(105, 110, 130))),
        ]));
    }

    // Fill remaining slots up to 4 if user has fewer than 4 watched shows
    for empty_idx in data.top_shows.len()..4 {
        let icon = title_icons.get(empty_idx).copied().unwrap_or("•");
        title_lines.push(Line::from(vec![
            Span::styled(format!(" {}. {} ", empty_idx + 1, icon), Style::default().fg(Color::Rgb(80, 85, 105))),
            Span::styled("Uncharted Rotation Slot", Style::default().fg(Color::Rgb(100, 105, 125))),
        ]));
        title_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("░".repeat(prog_bar_width), Style::default().fg(Color::Rgb(40, 44, 58))),
            Span::styled(" [0 ep • 0%] ", Style::default().fg(Color::Rgb(80, 85, 105))),
            Span::styled("0.0h", Style::default().fg(Color::Rgb(70, 75, 95))),
        ]));
        title_lines.push(Line::from(Span::styled(
            "    ▸ Explore trending or tailored recommendations to rank",
            Style::default().fg(Color::Rgb(75, 80, 100)),
        )));
    }

    title_lines.push(Line::from(""));
    title_lines.push(Line::from(Span::styled(
        format!(
            " ◈ Rotation Depth: {} In Flight • {} Completed Series Logged",
            data.in_progress_shows, data.completed_shows
        ),
        Style::default().fg(Color::Rgb(120, 125, 145)),
    )));

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
            Constraint::Length(4), // Sparkline Bars (Enlarged to 4 rows)
            Constraint::Length(1), // Timeline tick marks
            Constraint::Length(1), // 7-day mini activity row
            Constraint::Min(0),    // Insights paragraph
        ])
        .split(sparkline_inner);

    let total_14d_eps: u64 = data.recent_activity.iter().sum();
    let avg_14d = total_14d_eps as f64 / 14.0;
    let spark_header = Paragraph::new(Line::from(vec![
        Span::styled(format!(" Peak: {} ep/day ", max_vel), Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
        Span::styled(format!("• 14-Day Volume: {} ep ", total_14d_eps), Style::default().fg(theme.text)),
        Span::styled(format!("• Daily Velocity: {:.1} ep/day", avg_14d), Style::default().fg(theme.text_dim)),
    ]));
    frame.render_widget(spark_header, spark_layout[0]);

    let sparkline = Sparkline::default()
        .data(&data.recent_activity)
        .style(Style::default().fg(Color::Rgb(80, 250, 123)));
    frame.render_widget(sparkline, spark_layout[1]);

    // Day tick markers across the sparkline base
    let tick_sep = velocity_card_width.saturating_sub(4).clamp(10, 70);
    let tick_label = if tick_sep >= 48 {
        " 14d      12d      10d       8d       6d       4d       2d     Today"
    } else if tick_sep >= 36 {
        " 14d    10d     7d     4d     Today"
    } else {
        " 14d ─── 7d ─── Today"
    };
    let timeline_widget = Paragraph::new(Line::from(Span::styled(tick_label, Style::default().fg(Color::Rgb(80, 85, 105)))));
    frame.render_widget(timeline_widget, spark_layout[2]);

    // 7-day mini activity chip row
    let recent_7: Vec<u64> = data.recent_activity.iter().rev().take(7).copied().collect();
    let mut chip_spans = vec![
        Span::styled(" Recent 7d: ", Style::default().fg(Color::Rgb(100, 105, 125))),
    ];
    for (i, &ep) in recent_7.iter().rev().enumerate() {
        let tag = if i == 6 { "Today" } else { &format!("-{}d", 6 - i) };
        let color = if ep > 0 { Color::Rgb(80, 250, 123) } else { Color::Rgb(70, 75, 95) };
        chip_spans.push(Span::styled(format!("[{}:{}] ", tag, ep), Style::default().fg(color)));
    }
    let chips_widget = Paragraph::new(Line::from(chip_spans));
    frame.render_widget(chips_widget, spark_layout[3]);

    let insight_text = Paragraph::new(vec![
        Line::from(Span::styled(
            format!(" ❖ Cadence: {} session specialist • Active Binge Wave", data.viewing_cadence.split('(').next().unwrap_or("Active").trim()),
            Style::default().fg(theme.text_dim),
        )),
        Line::from(Span::styled(
            " ◈ Pipeline: Realtime Anime4K Upscaling Active • Native MPV Core Decoders",
            Style::default().fg(Color::Rgb(110, 115, 135)),
        )),
    ]);
    frame.render_widget(insight_text, spark_layout[4]);

    // ── 4. Footer hints ────────────────────────────────────────────────────────
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" [Esc / q / a] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Return to Home    ", Style::default().fg(theme.text)),
        Span::styled(" [r] ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
        Span::styled("Recalculate Telemetry    ", Style::default().fg(theme.text)),
        Span::styled(" [s] ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("Settings Hub    ", Style::default().fg(theme.text)),
        Span::styled(" [t] ", Style::default().fg(Color::Rgb(189, 147, 249)).add_modifier(Modifier::BOLD)),
        Span::styled("Cycle Theme    ", Style::default().fg(theme.text)),
        Span::styled(" [?] ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
        Span::styled("Help", Style::default().fg(theme.text)),
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
    let inner_width = area.width.saturating_sub(2) as usize;
    let display_label = if label.chars().count() > inner_width {
        label.chars().take(inner_width.saturating_sub(1)).collect::<String>()
    } else {
        label.to_string()
    };
    let display_val = if value.chars().count() > inner_width {
        value.chars().take(inner_width.saturating_sub(1)).collect::<String>()
    } else {
        value.to_string()
    };
    let p = Paragraph::new(vec![
        Line::from(Span::styled(display_label, Style::default().fg(Color::Rgb(150, 150, 165)))),
        Line::from(Span::styled(display_val, Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
    ])
    .block(block);
    frame.render_widget(p, area);
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
        assert!(rendered_text.contains("Uncharted Rotation Slot"));
    }

    #[test]
    fn test_render_compact_screen_no_panic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.analytics_data = Some(UserAnalytics {
            total_episodes_watched: 2,
            total_hours_watched: 0.8,
            completed_shows: 0,
            in_progress_shows: 1,
            completion_rate: 0.0,
            avg_score: 0.0,
            top_genres: vec![("Action".to_string(), 1, 50.0)],
            era_distribution: vec![("2020s (Current Era)", 1, 100.0)],
            format_distribution: vec![("TV".to_string(), 1)],
            recent_activity: vec![0; 14],
            taste_archetype: "◈ Shonen Specialist",
            top_shows: vec![],
            viewing_cadence: "Prime Time",
            active_streak_days: 1,
            audio_preference: "Japanese Sub",
        });

        let res = terminal.draw(|frame| {
            render(frame, &state);
        });
        assert!(res.is_ok());
    }
}
