//! Apple-inspired Split-Stage Release Calendar (`[c]`) UI.
//! Features a 7-day Segmented Day Ribbon (`Mon`–`Sun`), multi-week time travel (`[H/L]` / `[/]`),
//! chronological Tracked-first release timeline, and a rich Poster & Episode Inspector card.

use crate::services::calendar::{
    build_week_buckets, compute_week_bounds, format_countdown, format_week_offset_tag,
    today_weekday_idx, CalendarDayEntry, CalendarFilter,
};
use crate::services::tailored::{clean_synopsis, get_thematic_hook, truncate_at_word_boundary};
use crate::state::AppState;
use crate::ui::components::cover::HalfblockCover;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Render the centered Release Calendar sheet modal (`[c]`).
pub fn render_overlay(frame: &mut Frame, state: &mut AppState) {
    let full = frame.area();
    let (pct_x, pct_y) = if full.width >= 115 && full.height >= 34 {
        (94, 90)
    } else {
        (98, 94)
    };
    let area = centered_rect(pct_x, pct_y, full);
    frame.render_widget(Clear, area);

    let theme = state.theme();
    let now = if state.calendar_today_unix > 0 {
        state.calendar_today_unix
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    };
    let tz = state.calendar_tz_offset_secs;
    let bounds = compute_week_bounds(now, tz, state.calendar_week_offset);
    let week_tag = format_week_offset_tag(state.calendar_week_offset);

    let title_badge = format!(
        " 󰃭 RELEASE CALENDAR · {} ({}) ",
        bounds.range_label, week_tag
    );

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(
            title_badge,
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Week navigation & Scope filter pills + hairline divider
            Constraint::Length(3), // 7-Day Segmented Ribbon (Mon..=Sun)
            Constraint::Min(0),    // Main Split Stage (Timeline + Apple TV+ Inspector)
            Constraint::Length(2), // Responsive Footer Hints
        ])
        .split(inner);

    let cached_releases = state
        .calendar_cache
        .get(&state.calendar_week_offset)
        .cloned()
        .unwrap_or_default();

    // Precompute counts across all 3 scope modes so filter pills display exact live numbers
    let all_count = cached_releases
        .iter()
        .filter(|r| r.airing_at >= bounds.start_unix && r.airing_at <= bounds.end_unix || r.is_movie_premiere)
        .count();
    let tracked_week_count = cached_releases
        .iter()
        .filter(|r| state.calendar_tracked_ids.contains(&r.anime.id))
        .count();
    let movies_week_count = cached_releases
        .iter()
        .filter(|r| {
            r.is_movie_premiere
                || matches!(r.anime.format.as_deref(), Some("MOVIE" | "OVA" | "SPECIAL"))
        })
        .count();

    render_top_controls(
        frame,
        chunks[0],
        &bounds.range_label,
        &week_tag,
        state.calendar_week_offset,
        state.calendar_filter,
        all_count,
        tracked_week_count,
        movies_week_count,
        &theme,
    );

    let (buckets, day_has_tracked) = build_week_buckets(
        &cached_releases,
        state.calendar_filter,
        &state.calendar_tracked_ids,
        &state.calendar_watched_map,
        &bounds,
        tz,
    );

    let today_idx = today_weekday_idx(now, tz);
    render_day_ribbon(
        frame,
        chunks[1],
        &bounds,
        state.calendar_day_idx,
        state.calendar_week_offset == 0,
        today_idx,
        &buckets,
        &day_has_tracked,
        &theme,
    );

    // ── MAIN SPLIT STAGE ───────────────────────────────────────────────────────
    if state.is_loading_calendar && !state.calendar_cache.contains_key(&state.calendar_week_offset) {
        let loading = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("Fetching {} releases & movie premieres from AniList...", bounds.range_label),
                Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Tip: Press [H] or [L] to jump between previous and upcoming weeks anytime.",
                Style::default().fg(theme.text_dim),
            )),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(loading, chunks[2]);
    } else {
        let day_entries = &buckets[state.calendar_day_idx.min(6)];
        if day_entries.is_empty() {
            render_empty_day_state(
                frame,
                chunks[2],
                &bounds,
                state.calendar_day_idx.min(6),
                state.calendar_filter,
                &buckets,
                &theme,
            );
        } else {
            let safe_cursor = state.calendar_cursor.min(day_entries.len().saturating_sub(1));
            state.calendar_cursor = safe_cursor;

            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(46), // Left: Chronological Release Timeline
                    Constraint::Percentage(54), // Right: Apple TV+ Poster & Episode Inspector
                ])
                .split(chunks[2]);

            render_timeline_pane(
                frame,
                cols[0],
                day_entries,
                safe_cursor,
                now,
                &theme,
            );

            if let Some(selected) = day_entries.get(safe_cursor) {
                render_inspector_pane(
                    frame,
                    cols[1],
                    state,
                    selected,
                    now,
                    state.calendar_week_offset == 0 && state.calendar_day_idx == today_idx,
                    &theme,
                );
            }
        }
    }

    render_footer(frame, chunks[3], state.calendar_week_offset, &theme);
}

#[allow(clippy::too_many_arguments)]
fn render_top_controls(
    frame: &mut Frame,
    area: Rect,
    range_label: &str,
    week_tag: &str,
    week_offset: i32,
    filter: CalendarFilter,
    all_count: usize,
    tracked_count: usize,
    movies_count: usize,
    theme: &crate::ui::theme::ThemePalette,
) {
    let w = area.width as usize;
    let (nav_left, nav_right, filter_prefix, pill_all, pill_trk, pill_mov) = if w >= 122 {
        (
            " ‹ Prev ([/H) ".to_string(),
            " Next (L/]) › ".to_string(),
            "Filter [f]: ",
            format!(" {} ({}) ", CalendarFilter::All.label(), all_count),
            format!(" {} ({}) ", CalendarFilter::TrackedOnly.label(), tracked_count),
            format!(" {} ({}) ", CalendarFilter::MoviesAndSpecials.label(), movies_count),
        )
    } else if w >= 92 {
        (
            " ‹ ".to_string(),
            " › ".to_string(),
            "[f] ",
            format!(" {} ({}) ", CalendarFilter::All.label(), all_count),
            format!(" {} ({}) ", CalendarFilter::TrackedOnly.label(), tracked_count),
            format!(" {} ({}) ", CalendarFilter::MoviesAndSpecials.label(), movies_count),
        )
    } else {
        (
            " ‹ ".to_string(),
            " › ".to_string(),
            "[f] ",
            format!(" {}:{} ", CalendarFilter::All.label(), all_count),
            format!(" {}:{} ", CalendarFilter::TrackedOnly.label(), tracked_count),
            format!(" {}:{} ", CalendarFilter::MoviesAndSpecials.label(), movies_count),
        )
    };

    let week_badge_bg = if week_offset == 0 {
        Color::Rgb(80, 250, 123)
    } else {
        theme.accent
    };

    let range_span_text = format!("{} ", range_label);
    let week_span_text = format!(" {} ", week_tag);

    let left_w = nav_left.chars().count()
        + 1
        + range_span_text.chars().count()
        + week_span_text.chars().count()
        + 1
        + nav_right.chars().count();
    let right_w = filter_prefix.chars().count()
        + pill_all.chars().count()
        + 1
        + pill_trk.chars().count()
        + 1
        + pill_mov.chars().count();

    let mut top_spans = vec![
        Span::styled(
            nav_left,
            Style::default().fg(Color::Black).bg(theme.secondary).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            range_span_text,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            week_span_text,
            Style::default().fg(Color::Black).bg(week_badge_bg).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            nav_right,
            Style::default().fg(Color::Black).bg(theme.secondary).add_modifier(Modifier::BOLD),
        ),
    ];

    let gap = w.saturating_sub(left_w + right_w);
    if gap > 0 {
        top_spans.push(Span::raw(" ".repeat(gap)));
    } else {
        top_spans.push(Span::raw(" "));
    }

    top_spans.push(Span::styled(filter_prefix, Style::default().fg(theme.text_dim)));
    let pills = [
        (CalendarFilter::All, pill_all),
        (CalendarFilter::TrackedOnly, pill_trk),
        (CalendarFilter::MoviesAndSpecials, pill_mov),
    ];
    for (idx, (kind, text)) in pills.into_iter().enumerate() {
        if idx > 0 {
            top_spans.push(Span::raw(" "));
        }
        let active = filter == kind;
        let style = if active {
            Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_dim).bg(Color::Rgb(34, 34, 50))
        };
        top_spans.push(Span::styled(text, style));
    }

    let divider = Span::styled("─".repeat(area.width as usize), Style::default().fg(theme.divider));
    frame.render_widget(
        Paragraph::new(vec![Line::from(top_spans), Line::from(divider)]),
        area,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_day_ribbon(
    frame: &mut Frame,
    area: Rect,
    bounds: &crate::services::calendar::WeekBounds,
    selected_idx: usize,
    is_current_week: bool,
    today_idx: usize,
    buckets: &[Vec<CalendarDayEntry>; 7],
    day_has_tracked: &[bool; 7],
    theme: &crate::ui::theme::ThemePalette,
) {
    let day_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 7); 7])
        .split(area);

    for i in 0..7 {
        let meta = &bounds.days[i];
        let is_selected = i == selected_idx;
        let is_today = is_current_week && i == today_idx;
        let count = buckets[i].len();
        let has_tracked = day_has_tracked[i];

        let border_color = if is_selected {
            theme.primary
        } else if is_today {
            theme.accent
        } else {
            theme.border_idle
        };

        let card_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));
        let card_inner = card_block.inner(day_cols[i]);
        frame.render_widget(card_block, day_cols[i]);

        let dot_w = if has_tracked { 2 } else { 0 };
        let eff_text = (card_inner.width as usize).saturating_sub(dot_w);
        let today_full = format!("TODAY · {} {:02} ({})", meta.short_name, meta.day_num, count);
        let full = format!("{} {:02} ({})", meta.short_name, meta.day_num, count);
        let mid = format!("{} {:02}", meta.short_name, meta.day_num);

        let label = if is_today && today_full.chars().count() <= eff_text {
            today_full
        } else if full.chars().count() <= eff_text {
            full
        } else if mid.chars().count() <= eff_text {
            mid
        } else {
            meta.short_name.to_string()
        };

        let dot_fg = if is_selected {
            Color::Black
        } else {
            Color::Rgb(80, 250, 123)
        };

        let mut spans = Vec::new();
        if has_tracked {
            spans.push(Span::styled(
                "● ",
                if is_selected {
                    Style::default().fg(dot_fg).bg(theme.primary).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(dot_fg)
                },
            ));
        }

        let text_style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD)
        } else if is_today {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_dim)
        };
        spans.push(Span::styled(label, text_style));

        let line = Line::from(spans);
        let p = Paragraph::new(line)
            .alignment(Alignment::Center)
            .style(if is_selected {
                Style::default().bg(theme.primary)
            } else {
                Style::default()
            });
        frame.render_widget(p, card_inner);
    }
}

fn render_empty_day_state(
    frame: &mut Frame,
    area: Rect,
    bounds: &crate::services::calendar::WeekBounds,
    day_idx: usize,
    filter: CalendarFilter,
    buckets: &[Vec<CalendarDayEntry>; 7],
    theme: &crate::ui::theme::ThemePalette,
) {
    let day = &bounds.days[day_idx];
    let reason = match filter {
        CalendarFilter::All => format!("No seasonal episodes or movies scheduled for {}, {}.", day.full_name, day.date_short),
        CalendarFilter::TrackedOnly => format!("None of your Continue Watching or Watchlist shows release on {}, {}.", day.full_name, day.date_short),
        CalendarFilter::MoviesAndSpecials => format!("No movies, OVAs, or specials premiere on {}, {}.", day.full_name, day.date_short),
    };

    let active_days: Vec<String> = bounds
        .days
        .iter()
        .enumerate()
        .filter(|(idx, _)| !buckets[*idx].is_empty())
        .map(|(_, d)| format!("{} ({})", d.short_name, buckets[d.day_idx].len()))
        .collect();

    let mut lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            reason,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    if !active_days.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("Releases found this week on: {}", active_days.join("  ·  ")),
            Style::default().fg(theme.secondary),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "Use [h/l] or [1-7] to switch days, [f] to change filter, or [[/]] for adjacent weeks.",
        Style::default().fg(theme.text_dim),
    )));

    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

/// Render the left chronological timeline with zero mid-word truncation and exact scroll tracking.
fn render_timeline_pane(
    frame: &mut Frame,
    area: Rect,
    entries: &[CalendarDayEntry],
    cursor: usize,
    now_unix: i64,
    theme: &crate::ui::theme::ThemePalette,
) {
    let left_block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(theme.divider));
    let inner = left_block.inner(area);
    frame.render_widget(left_block, area);

    if inner.width < 12 || inner.height < 3 {
        return;
    }

    let tracked_total = entries.iter().filter(|e| e.is_tracked).count();
    let other_total = entries.len().saturating_sub(tracked_total);

    let emerald = Color::Rgb(80, 250, 123);
    let row_hl_bg = Color::Rgb(36, 38, 56);
    let title_width = (inner.width as usize).saturating_sub(4).max(10);

    // Pre-build structured blocks per entry so scroll bounds match wrapped line heights accurately
    struct RenderedBlock {
        entry_idx: Option<usize>,
        lines: Vec<Line<'static>>,
    }

    let mut blocks: Vec<RenderedBlock> = Vec::with_capacity(entries.len() + 2);
    let mut emitted_other_header = false;

    if tracked_total > 0 {
        blocks.push(RenderedBlock {
            entry_idx: None,
            lines: vec![Line::from(Span::styled(
                format!("  YOUR TRACKED RELEASES ({}) ", tracked_total),
                Style::default().fg(emerald).add_modifier(Modifier::BOLD),
            ))],
        });
    }

    for (idx, entry) in entries.iter().enumerate() {
        if !entry.is_tracked && !emitted_other_header {
            emitted_other_header = true;
            let header_text = if tracked_total > 0 {
                format!(" 󰃭 ALL OTHER SEASONAL & MOVIES ({}) ", other_total)
            } else {
                format!(" 󰃭 SEASONAL & MOVIE RELEASES ({}) ", other_total)
            };
            blocks.push(RenderedBlock {
                entry_idx: None,
                lines: vec![Line::from(Span::styled(
                    header_text,
                    Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
                ))],
            });
        }

        let focused = idx == cursor;
        let anime = entry.anime();
        let has_aired = entry.release.airing_at <= now_unix;

        let status_text = if entry.is_watched_episode {
            " AIRED ✓ ".to_string()
        } else if has_aired {
            " AIRED ".to_string()
        } else {
            format!(" {} ", format_countdown(entry.release.airing_at, now_unix))
        };

        let status_style = if entry.is_watched_episode {
            Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD)
        } else if has_aired {
            Style::default().fg(Color::Black).bg(theme.secondary).add_modifier(Modifier::BOLD)
        } else if focused {
            Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.accent).bg(Color::Rgb(34, 34, 52)).add_modifier(Modifier::BOLD)
        };

        let ep_badge = if entry.release.is_movie_premiere || anime.format.as_deref() == Some("MOVIE") {
            " [MOVIE]".to_string()
        } else {
            match anime.format.as_deref() {
                Some("OVA" | "SPECIAL") => format!(" [{} EP {:02}]", anime.format.as_deref().unwrap_or("OVA"), entry.release.episode),
                Some("TV_SHORT") => format!(" [SHORT {:02}]", entry.release.episode),
                _ => format!(" [EP {:02}]", entry.release.episode),
            }
        };

        let mut meta_spans = vec![
            Span::styled(
                if focused { "▸ " } else { "  " },
                Style::default().fg(if focused { theme.primary } else { theme.text_dim }).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} ", entry.local_hhmm),
                Style::default().fg(if focused { Color::White } else { theme.text }).add_modifier(Modifier::BOLD),
            ),
            Span::styled(status_text, status_style),
            Span::styled(
                ep_badge,
                Style::default().fg(if entry.is_movie_or_special { theme.accent } else { theme.primary }).add_modifier(Modifier::BOLD),
            ),
        ];

        if let Some(sc) = anime.score {
            meta_spans.push(Span::styled(
                format!(" ·  {:.1}", sc as f32 / 10.0),
                Style::default().fg(theme.text_dim),
            ));
        }

        let mut item_lines = vec![Line::from(meta_spans)];

        // Word-wrap title onto up to 2 full-width lines cleanly (never truncating mid-word!)
        let wrapped = wrap_title_lines(anime.display_title(), title_width);
        let title_style = if focused {
            Style::default().fg(Color::White).bg(row_hl_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
        };
        for tline in wrapped {
            let pad = (inner.width as usize).saturating_sub(2 + tline.chars().count());
            let mut spans = vec![
                Span::styled(
                    "  ",
                    if focused { Style::default().bg(row_hl_bg) } else { Style::default() },
                ),
                Span::styled(tline, title_style),
            ];
            if focused && pad > 0 {
                spans.push(Span::styled(" ".repeat(pad), Style::default().bg(row_hl_bg)));
            }
            item_lines.push(Line::from(spans));
        }

        // Subtle spacing line between entries for rhythm without deadspace
        item_lines.push(Line::from(""));

        blocks.push(RenderedBlock {
            entry_idx: Some(idx),
            lines: item_lines,
        });
    }

    // Compute exact line range around cursor so the selected item is always completely in view
    let max_rows = inner.height as usize;
    let mut selected_start_line = 0usize;
    let mut selected_end_line = 0usize;
    let mut flat_lines: Vec<Line<'static>> = Vec::new();

    for b in blocks {
        let start = flat_lines.len();
        flat_lines.extend(b.lines);
        let end = flat_lines.len();
        if b.entry_idx == Some(cursor) {
            selected_start_line = start;
            selected_end_line = end;
        }
    }

    let scroll_top = if selected_end_line <= max_rows {
        0
    } else if selected_end_line.saturating_sub(max_rows) < selected_start_line {
        selected_end_line.saturating_sub(max_rows)
    } else {
        selected_start_line
    };

    let visible: Vec<Line<'static>> = flat_lines
        .into_iter()
        .skip(scroll_top)
        .take(max_rows)
        .collect();

    frame.render_widget(Paragraph::new(visible), inner);
}

/// Render the right-hand Apple TV+ Poster & Episode Inspector card.
fn render_inspector_pane(
    frame: &mut Frame,
    area: Rect,
    state: &mut AppState,
    entry: &CalendarDayEntry,
    now_unix: i64,
    is_today_selected: bool,
    theme: &crate::ui::theme::ThemePalette,
) {
    let anime = entry.anime().clone();
    let emerald = Color::Rgb(80, 250, 123);
    let top_h = if area.height >= 22 { 14 } else { 11 };

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_h), // Top: Poster + Episode Metadata
            Constraint::Min(0),        // Bottom: Release Action Note & Story Synopsis
        ])
        .split(area);

    let hero_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(1),  // Left margin
            Constraint::Length(19), // Portrait poster frame (19x14, inner 17x12)
            Constraint::Length(2),  // Clean gutter
            Constraint::Min(0),     // Word-wrapped metadata stack
        ])
        .split(right_chunks[0]);

    let cover_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary));
    let cover_inner = cover_block.inner(hero_cols[1]);
    frame.render_widget(cover_block, hero_cols[1]);

    if state.has_image_support() {
        if let Some(cover) = state
            .detail_cache
            .get_mut(&anime.id)
            .or_else(|| state.banner_cache.get_mut(&anime.id))
            .or_else(|| state.cover_cache.get_mut(&anime.id))
        {
            let image_widget = ratatui_image::StatefulImage::new(None)
                .resize(ratatui_image::Resize::Fit(None));
            frame.render_stateful_widget(image_widget, cover_inner, cover);
        } else if state.cover_failed_anime_id == Some(anime.id) {
            frame.render_widget(
                HalfblockCover {
                    anime_id: anime.id,
                    title: anime.display_title(),
                },
                cover_inner,
            );
        } else {
            let loading = Paragraph::new("Loading\nPoster...")
                .style(Style::default().fg(theme.text_dim))
                .alignment(Alignment::Center);
            frame.render_widget(loading, cover_inner);
        }
    } else {
        frame.render_widget(
            HalfblockCover {
                anime_id: anime.id,
                title: anime.display_title(),
            },
            cover_inner,
        );
    }

    // Build word-wrapped metadata lines (zero truncation!)
    let title = anime.display_title();
    let romaji_sub = if anime.title_romaji != title && !anime.title_romaji.is_empty() {
        Some(anime.title_romaji.as_str())
    } else {
        None
    };

    let has_aired = entry.release.airing_at <= now_unix;
    let ep_badge = if entry.release.is_movie_premiere || anime.format.as_deref() == Some("MOVIE") {
        " MOVIE PREMIERE ".to_string()
    } else if let Some(total) = anime.episodes.filter(|&e| e > 0) {
        format!(" EPISODE {} OF {} ", entry.release.episode, total)
    } else {
        format!(" EPISODE {} ", entry.release.episode)
    };

    let status_badge = if entry.is_watched_episode {
        " 󰄬 WATCHED ".to_string()
    } else if has_aired {
        " AIRED · STREAM READY ".to_string()
    } else {
        format!(" {} ", format_countdown(entry.release.airing_at, now_unix))
    };

    let status_style = if entry.is_watched_episode || has_aired {
        Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD)
    };

    let mut badge_spans = vec![
        Span::styled(ep_badge, Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(status_badge, status_style),
    ];
    if let Some(sc) = anime.score {
        badge_spans.push(Span::raw(" "));
        badge_spans.push(Span::styled(
            format!("  {:.1} ", sc as f32 / 10.0),
            Style::default().fg(Color::Black).bg(theme.secondary).add_modifier(Modifier::BOLD),
        ));
    }

    let schedule_text = if is_today_selected {
        if entry.release.is_movie_premiere {
            format!("Today ({}) · Feature Premiere", entry.day_label)
        } else {
            format!("Today at {} (Local Time)", entry.local_hhmm)
        }
    } else if entry.release.is_movie_premiere {
        format!("{} · Release Day", entry.day_label)
    } else {
        format!("{} at {} (Local Time)", entry.day_label, entry.local_hhmm)
    };

    let library_line = if entry.is_watched_episode {
        Line::from(vec![
            Span::styled("󰄬 Library:  ", Style::default().fg(emerald).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("Episode {} already marked watched", entry.release.episode),
                Style::default().fg(theme.text),
            ),
        ])
    } else if entry.is_tracked {
        Line::from(vec![
            Span::styled(" Library:  ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(
                "Tracked in your library ([+] to remove)",
                Style::default().fg(theme.text),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("○ Library:  ", Style::default().fg(theme.text_dim)),
            Span::styled(
                "Press [+] to add to My Watchlist",
                Style::default().fg(theme.text_dim),
            ),
        ])
    };

    let mut genre_spans = vec![Span::styled("Format:   ", Style::default().fg(theme.text_dim))];
    let fmt = anime.format.as_deref().unwrap_or("TV");
    genre_spans.push(Span::styled(
        fmt,
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    ));
    let genres = anime.genre_list();
    if !genres.is_empty() {
        for g in genres.iter().take(4) {
            genre_spans.push(Span::styled(" · ", Style::default().fg(theme.divider)));
            genre_spans.push(Span::styled(g.clone(), Style::default().fg(theme.primary)));
        }
    } else if let Some(yr) = anime.season_year {
        genre_spans.push(Span::styled(" · ", Style::default().fg(theme.divider)));
        genre_spans.push(Span::styled(yr.to_string(), Style::default().fg(theme.secondary)));
    }

    let mut meta_lines = vec![Line::from(Span::styled(
        title,
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    ))];
    if let Some(sub) = romaji_sub {
        meta_lines.push(Line::from(Span::styled(sub, Style::default().fg(theme.text_dim))));
    }
    meta_lines.push(Line::from(""));
    meta_lines.push(Line::from(badge_spans));
    meta_lines.push(Line::from(""));
    meta_lines.push(Line::from(vec![
        Span::styled("󰃭 Release:  ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
        Span::styled(schedule_text, Style::default().fg(Color::White)),
    ]));
    meta_lines.push(library_line);
    meta_lines.push(Line::from(genre_spans));

    frame.render_widget(
        Paragraph::new(meta_lines).wrap(Wrap { trim: true }),
        hero_cols[3],
    );

    // ── BOTTOM SYNOPSIS & ACTION OVERVIEW ──────────────────────────────────────
    let syn_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.divider))
        .title(Span::styled(
            " ── EPISODE & STORY OVERVIEW ── ",
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
        ));
    let syn_outer = Rect {
        x: right_chunks[1].x + 1,
        y: right_chunks[1].y,
        width: right_chunks[1].width.saturating_sub(1),
        height: right_chunks[1].height,
    };
    let syn_inner = syn_block.inner(syn_outer);
    frame.render_widget(syn_block, syn_outer);

    let action_callout = if entry.release.is_movie_premiere {
        if has_aired {
            format!("󰄬 Released {} — Press [Enter] to watch or [d] for details.", entry.day_label)
        } else {
            format!("󰃭 Feature premiere scheduled for {} ({}).", entry.day_label, format_countdown(entry.release.airing_at, now_unix))
        }
    } else if has_aired {
        format!(
            "󰄬 Episode {} is out now — Press [Enter] to stream this exact episode.",
            entry.release.episode
        )
    } else {
        format!(
            "󰃭 Episode {} airs {} at {} ({}). Press [d] for full episode list.",
            entry.release.episode,
            entry.day_label,
            entry.local_hhmm,
            format_countdown(entry.release.airing_at, now_unix),
        )
    };

    let mut story_lines = vec![
        Line::from(Span::styled(
            action_callout,
            Style::default()
                .fg(if has_aired { emerald } else { theme.accent })
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    let raw_desc = anime.description.as_deref().unwrap_or("").trim();
    if raw_desc.is_empty() {
        story_lines.push(Line::from(Span::styled(
            get_thematic_hook(&anime),
            Style::default().fg(theme.text).add_modifier(Modifier::ITALIC),
        )));
        story_lines.push(Line::from(""));
        let eps_note = anime
            .episodes
            .map(|e| format!("Planned run: {} episodes", e))
            .unwrap_or_else(|| "Broadcast run in progress".to_string());
        let season_note = match (&anime.season, anime.season_year) {
            (Some(s), Some(y)) => format!("{} {}", s, y),
            (_, Some(y)) => format!("{}", y),
            _ => "Current Broadcast".to_string(),
        };
        story_lines.push(Line::from(Span::styled(
            format!(
                "No full AniList synopsis has been published yet. Listed under {} ({}).",
                season_note, eps_note
            ),
            Style::default().fg(theme.text_dim),
        )));
    } else {
        let cleaned = clean_synopsis(raw_desc);
        for para in cleaned.split("\n\n") {
            let trimmed = para.trim();
            if !trimmed.is_empty() {
                story_lines.push(Line::from(Span::styled(
                    trimmed.replace('\n', " "),
                    Style::default().fg(theme.text),
                )));
                story_lines.push(Line::from(""));
            }
        }
    }

    frame.render_widget(
        Paragraph::new(story_lines).wrap(Wrap { trim: true }),
        syn_inner,
    );
}

fn render_footer(
    frame: &mut Frame,
    area: Rect,
    week_offset: i32,
    theme: &crate::ui::theme::ThemePalette,
) {
    let avail = area.width as usize;
    let emerald = Color::Rgb(80, 250, 123);
    let today_lbl = if week_offset != 0 { "This Week" } else { "Today" };

    let wide_items: Vec<(&str, &str, Color)> = vec![
        ("[h/l]", "Day", theme.primary),
        ("[j/k]", "Episode", theme.secondary),
        ("[H/L]", "Prev/Next Week", theme.secondary),
        ("[t]", today_lbl, emerald),
        ("[f]", "Filter", theme.primary),
        ("[Enter]", "Watch Ep", theme.accent),
        ("[+]", "Watchlist", theme.text),
        ("[d]", "Details", theme.text),
        ("[Esc]", "Close", theme.text_dim),
    ];
    let med_items: Vec<(&str, &str, Color)> = vec![
        ("[h/l]", "Day", theme.primary),
        ("[j/k]", "Ep", theme.secondary),
        ("[H/L]", "Week", theme.secondary),
        ("[t]", "Today", emerald),
        ("[f]", "Filter", theme.primary),
        ("[Enter]", "Watch", theme.accent),
        ("[+]", "List", theme.text),
        ("[d]", "Info", theme.text),
        ("[Esc]", "Close", theme.text_dim),
    ];
    let compact_items: Vec<(&str, &str, Color)> = vec![
        ("[hjkl]", "Nav", theme.primary),
        ("[H/L]", "Week", theme.secondary),
        ("[t]", "Today", emerald),
        ("[f]", "Filter", theme.primary),
        ("[↵]", "Play", theme.accent),
        ("[+]", "List", theme.text),
        ("[d]", "Info", theme.text),
        ("[Esc]", "Close", theme.text_dim),
    ];

    let total_width = |list: &[(&str, &str, Color)], sep_w: usize| -> usize {
        list.iter()
            .enumerate()
            .map(|(idx, (k, l, _))| k.chars().count() + 1 + l.chars().count() + if idx > 0 { sep_w } else { 0 })
            .sum::<usize>()
    };

    let (items, sep) = if total_width(&wide_items, 3) + 2 <= avail {
        (wide_items, " · ")
    } else if total_width(&med_items, 3) + 2 <= avail {
        (med_items, " · ")
    } else if total_width(&compact_items, 3) + 2 <= avail {
        (compact_items, " · ")
    } else {
        (compact_items, " ")
    };

    let sep_w = sep.chars().count();
    let mut used = 1usize;
    let mut spans = vec![Span::raw(" ")];

    for (idx, (key, label, color)) in items.into_iter().enumerate() {
        let seg_len = key.chars().count() + 1 + label.chars().count() + if idx > 0 { sep_w } else { 0 };
        if used + seg_len > avail.saturating_sub(1) {
            break;
        }
        if idx > 0 {
            spans.push(Span::styled(sep, Style::default().fg(theme.divider)));
        }
        spans.push(Span::styled(
            key,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {}", label),
            Style::default().fg(theme.text_dim),
        ));
        used += seg_len;
    }

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.divider));
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(block)
            .alignment(Alignment::Center),
        area,
    );
}

/// Word-wrap a title across up to 3 lines cleanly at word boundaries without cutting words in half.
fn wrap_title_lines(text: &str, max_width: usize) -> Vec<String> {
    if max_width < 8 || text.chars().count() <= max_width {
        return vec![text.to_string()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut overflow_words: Vec<&str> = Vec::new();

    for word in text.split_whitespace() {
        if lines.len() == 2 && !overflow_words.is_empty() {
            overflow_words.push(word);
            continue;
        }
        let need = if current.is_empty() {
            word.chars().count()
        } else {
            current.chars().count() + 1 + word.chars().count()
        };
        if need <= max_width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        } else if current.is_empty() {
            lines.push(truncate_at_word_boundary(word, max_width));
        } else {
            lines.push(std::mem::take(&mut current));
            if lines.len() == 2 {
                overflow_words.push(word);
            } else {
                current.push_str(word);
            }
        }
    }

    if !overflow_words.is_empty() {
        let joined = overflow_words.join(" ");
        lines.push(truncate_at_word_boundary(&joined, max_width));
    } else if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        vec![truncate_at_word_boundary(text, max_width)]
    } else {
        lines
    }
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
    use crate::api::anilist::CalendarRelease;
    use crate::db::cache::Anime;
    use ratatui::{backend::TestBackend, Terminal};

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn seed_calendar_state() -> AppState {
        let mut state = AppState::new();
        let now = 1_762_992_000; // Thursday Nov 13, 2026 UTC roughly
        state.open_calendar(0, now);
        let tracked = Anime {
            id: 42,
            title_english: Some("Frieren Beyond Journey's End Season 2".to_string()),
            title_romaji: "Sousou no Frieren 2".to_string(),
            episodes: Some(12),
            status: Some("RELEASING".to_string()),
            format: Some("TV".to_string()),
            score: Some(91),
            description: Some("The party continues their northern journey with new allies.".to_string()),
            ..Default::default()
        };
        let movie = Anime {
            id: 77,
            title_english: Some("Original Sci-Fi Feature Premiere Film".to_string()),
            title_romaji: "Gekijouban Sci-Fi".to_string(),
            episodes: Some(1),
            status: Some("FINISHED".to_string()),
            format: Some("MOVIE".to_string()),
            score: Some(84),
            description: Some("A standalone theatrical feature event.".to_string()),
            ..Default::default()
        };
        state.calendar_tracked_ids.insert(42);
        state.calendar_cache.insert(
            0,
            vec![
                CalendarRelease {
                    schedule_id: 1001,
                    airing_at: now + 3600,
                    episode: 7,
                    is_movie_premiere: false,
                    anime: tracked,
                },
                CalendarRelease {
                    schedule_id: 1002,
                    airing_at: now - 1800,
                    episode: 1,
                    is_movie_premiere: true,
                    anime: movie,
                },
            ],
        );
        state
    }

    #[test]
    fn test_render_calendar_overlay_desktop_and_compact() {
        for (w, h) in [(140u16, 40u16), (90u16, 26u16)] {
            let mut state = seed_calendar_state();
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|f| render_overlay(f, &mut state))
                .expect("calendar render should not panic");
            let text = buffer_text(terminal.backend().buffer());
            assert!(text.contains("RELEASE CALENDAR"), "missing title at {}x{}", w, h);
            for day in ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"] {
                assert!(text.contains(day), "missing {} ribbon at {}x{}", day, w, h);
            }
            assert!(text.contains("All"), "missing All pill at {}x{}", w, h);
            assert!(text.contains("Tracked"), "missing Tracked pill at {}x{}", w, h);
            assert!(text.contains("Movies"), "missing Movies pill at {}x{}", w, h);
            assert!(text.contains("YOUR TRACKED RELEASES"), "missing tracked header at {}x{}", w, h);
            assert!(text.contains("Frieren Beyond"), "missing anime title at {}x{}", w, h);
            assert!(text.contains("Watchlist") || text.contains("List"), "missing watchlist footer hint at {}x{}", w, h);
            assert!(text.contains("Close"), "missing Close footer hint at {}x{}", w, h);
        }
    }

    #[test]
    fn test_wrap_title_lines_keeps_whole_words() {
        let lines = wrap_title_lines("Reincarnated as a Vending Machine Walking the Dungeon", 22);
        assert!(lines.len() >= 2 && lines.len() <= 3);
        for line in &lines {
            assert!(line.chars().count() <= 22);
        }
        assert_eq!(lines[0], "Reincarnated as a");
    }
}
