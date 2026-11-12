//! Detail screen — full anime info + Netflix-style dual-mode episode browser.

use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

use crate::{
    db::cache::Anime,
    state::{AppState, EpisodeViewMode},
    ui::components::cover::HalfblockCover,
};

/// Helper to format miniature progress bar: [━━━━━━────]
pub fn format_mini_bar(pos: f64, dur: f64, width: usize) -> String {
    if dur <= 0.0 || width == 0 {
        return String::new();
    }
    let ratio = (pos / dur).clamp(0.0, 1.0);
    let filled = (ratio * width as f64).round() as usize;
    let unfilled = width.saturating_sub(filled);
    format!("[{}{}]", "━".repeat(filled), "─".repeat(unfilled))
}

/// Render the detail screen.
pub fn render(frame: &mut Frame, state: &mut AppState) {
    let Some(anime) = state.selected_anime.clone() else {
        return;
    };

    let area = frame.area();
    let action_label = match state.selected_episode.unwrap_or(1) {
        1 => " Start E1 ".to_string(),
        ep => format!(" Continue E{} ", ep),
    };

    let view_mode_label = match state.episode_view_mode {
        EpisodeViewMode::List => " View: List (v) ",
        EpisodeViewMode::Split => " View: Split (v) ",
    };

    // Back hint
    let hint = Paragraph::new(Line::from(vec![
        Span::styled(" ← Esc", Style::default().fg(Color::Rgb(120, 120, 120))),
        Span::raw("  "),
        Span::styled("Enter", Style::default().fg(Color::Rgb(180, 0, 255))),
        Span::raw(" "),
        Span::styled(action_label, Style::default().fg(Color::Rgb(220, 220, 220))),
        Span::raw("  "),
        Span::styled("j/k", Style::default().fg(Color::Rgb(180, 0, 255))),
        Span::raw(" Navigate  "),
        Span::styled("v", Style::default().fg(Color::Rgb(180, 0, 255))),
        Span::raw(format!("{} ", view_mode_label)),
        Span::styled("w", Style::default().fg(Color::Rgb(180, 0, 255))),
        Span::raw(" Watched  "),
        Span::styled("p", Style::default().fg(Color::Rgb(180, 0, 255))),
        Span::raw(" Options  "),
        Span::styled("D", Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD)),
        Span::raw(" Download  "),
        Span::styled("+", Style::default().fg(Color::Rgb(180, 0, 255))),
        Span::raw(" Watchlist  "),
        Span::styled("󰅖 Backspace", Style::default().fg(Color::Rgb(255, 85, 85))),
        Span::raw(" Drop/Remove  "),
        Span::styled("/", Style::default().fg(Color::Rgb(180, 0, 255))),
        Span::raw(" Search  "),
        Span::styled("?", Style::default().fg(Color::Rgb(180, 0, 255))),
        Span::raw(" Help"),
        if state.available_update.is_some() {
            Span::styled("  󰚰 U Update", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD))
        } else {
            Span::raw("")
        },
    ]));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // hint bar
            Constraint::Length(14), // top info panel
            Constraint::Min(0),     // Netflix-style episode browser
        ])
        .split(area);

    frame.render_widget(hint, chunks[0]);
    render_info(frame, chunks[1], state, &anime);
    render_episodes(frame, chunks[2], state, &anime);

    if state.is_download_modal_open {
        crate::ui::download::render_download_modal(frame, state);
    }
}

/// Top section: cover + metadata side by side.
fn render_info(frame: &mut Frame, area: Rect, state: &mut AppState, anime: &Anime) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(18), // cover art: 18 cols x 14 rows (inner: 16x12, exactly 0.6667 aspect ratio)
            Constraint::Length(1),  // spacer
            Constraint::Min(0),     // metadata
        ])
        .split(area);

    let cover_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(70, 70, 95)));
    let cover_inner = cover_block.inner(cols[0]);
    frame.render_widget(cover_block, cols[0]);

    // Cover: real image when terminal supports it, halfblock otherwise
    if state.has_image_support() {
        if let Some(cover) = state.detail_cache.get_mut(&anime.id) {
            let image_widget = ratatui_image::StatefulImage::new(None)
                .resize(ratatui_image::Resize::Fit(None));
            frame.render_stateful_widget(image_widget, cover_inner, cover);
        } else if state.cover_anime_id == Some(anime.id) {
            let label = if state.cover_failed_anime_id == Some(anime.id) {
                "Cover unavailable"
            } else {
                "Loading cover..."
            };
            let loading = Paragraph::new(label)
                .style(Style::default().fg(Color::Rgb(160, 160, 180)))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(loading, cover_inner);
        } else {
            frame.render_widget(
                HalfblockCover { anime_id: anime.id, title: anime.display_title() },
                cover_inner,
            );
        }
    } else {
        frame.render_widget(
            HalfblockCover { anime_id: anime.id, title: anime.display_title() },
            cover_inner,
        );
    }

    // Metadata
    render_metadata(frame, cols[2], state, anime);
}

fn render_metadata(frame: &mut Frame, area: Rect, state: &AppState, anime: &Anime) {
    let title   = anime.display_title();
    let score   = anime.score.map(|s| format!(" {:.1}", s as f32 / 10.0)).unwrap_or_else(|| " N/A".to_string());
    let max_aired = anime.aired_episodes();
    let eps = if let Some(total) = anime.episodes {
        if anime.status.as_deref() == Some("RELEASING") && max_aired < total as u32 {
            format!("{}/{} eps (Airing)", max_aired, total)
        } else {
            format!("{} eps", total)
        }
    } else if max_aired > 0 {
        format!("{} eps (Airing)", max_aired)
    } else {
        "? eps".to_string()
    };
    let fmt     = anime.format.as_deref().unwrap_or("TV");
    let status  = anime.status.as_deref().unwrap_or("Unknown");
    let year    = anime.season_year.map(|y| y.to_string()).unwrap_or_else(|| "?".to_string());
    let season  = anime.season.as_deref().unwrap_or("");
    let genres  = anime.genre_list().join(" · ");
    let dub_tag = if anime.has_dub() { "  Sub + Dub" } else { "  Sub only" };
    let play_label = match state.selected_episode.unwrap_or(1) {
        ep if ep > max_aired => " 󰌾 Locked ".to_string(),
        1 => " Start E1 ".to_string(),
        ep => format!(" Continue E{} ", ep),
    };
    let watchlist_label = if state.in_watchlist {
        " - Remove "
    } else {
        " + Watchlist "
    };

    let playback_status = if state.now_playing.is_some()
        && state.last_played_anime_id == Some(anime.id)
    {
        state.now_playing.as_deref()
    } else if state.last_played_anime_id == Some(anime.id) {
        state.last_played.as_deref()
    } else {
        None
    };

    // Split metadata area into: Top (Title, Specs, Genres, Full Genre-Attuned Synopsis) and Bottom (Action Buttons)
    let meta_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // Title, specs, genres & expanded synopsis
            Constraint::Length(1), // Action buttons bar pinned cleanly at bottom
        ])
        .split(area);

    let top_area = meta_chunks[0];
    let btn_area = meta_chunks[1];

    let genre_rows = ((genres.chars().count() as u16) / top_area.width.max(1)) + 1;
    let header_used_rows = 2 + genre_rows + 1 + if state.detail_origin_title.is_some() { 1 } else { 0 };
    let avail_desc_rows = top_area.height.saturating_sub(header_used_rows).max(2) as usize;
    let target_desc_chars = (top_area.width.saturating_sub(2) as usize) * avail_desc_rows;
    let desc = compose_anime_synopsis(anime, target_desc_chars.saturating_sub(8));

    let mut lines = Vec::new();
    if let Some(origin) = state.detail_origin_title.as_deref() {
        lines.push(Line::from(vec![
            Span::styled(
                " From ",
                Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)),
            ),
            Span::raw(" "),
            Span::styled(origin, Style::default().fg(Color::Rgb(180, 180, 200))),
        ]));
    }

    lines.extend([
        Line::from(Span::styled(
            title,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("{}  |  {}  |  {}  |  {} {}  |  {}{}", score, eps, fmt, season, year, status, dub_tag),
            Style::default().fg(Color::Rgb(160, 160, 160)),
        )),
        Line::from(Span::styled(genres, Style::default().fg(Color::Rgb(180, 0, 255)))),
        Line::from(""),
        Line::from(Span::styled(desc, Style::default().fg(Color::Rgb(215, 215, 225)))),
    ]);

    let para = Paragraph::new(lines)
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(para, top_area);

    let mut btn_spans = vec![
        Span::styled(
            play_label,
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            watchlist_label,
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(60, 60, 60)),
        ),
    ];

    if let Some(status_line) = playback_status {
        btn_spans.extend([
            Span::raw("    "),
            Span::styled(
                " Playback ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(180, 0, 255))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(status_line, Style::default().fg(Color::Rgb(210, 210, 210))),
        ]);
    }

    frame.render_widget(Paragraph::new(Line::from(btn_spans)), btn_area);
}

/// Dispatch episode browser to selected layout (List or Split-Deck).
fn render_episodes(frame: &mut Frame, area: Rect, state: &mut AppState, anime: &Anime) {
    match state.episode_view_mode {
        EpisodeViewMode::List => render_netflix_list(frame, area, state, anime),
        EpisodeViewMode::Split => render_netflix_split(frame, area, state, anime),
    }
}

/// Option 1: Netflix Vertical Episode List.
/// Dense, scrollable stack of full-width horizontal cards with 16:9 thumbnails and rich metadata.
fn render_netflix_list(frame: &mut Frame, area: Rect, state: &mut AppState, anime: &Anime) {
    let max_aired = anime.aired_episodes();
    let total_eps = anime.episodes.map(|e| e as u32).unwrap_or(max_aired);
    let selected_ep = state.selected_episode.unwrap_or(1);

    let title_span = Line::from(vec![
        Span::styled(
            " 󰗃 Episodes ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("[ {} Available · {} Total ]  ", max_aired, total_eps),
            Style::default().fg(Color::Rgb(180, 0, 255)),
        ),
        Span::styled(
            " [v] Split-Deck View ",
            Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD),
        ),
    ]);

    let outer_block = Block::default()
        .title(title_span)
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));

    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    if state.episode_list.is_empty() {
        let msg = Paragraph::new("No episode data available.")
            .style(Style::default().fg(Color::Rgb(120, 120, 120)));
        frame.render_widget(msg, inner);
        return;
    }

    // Dynamic card height based on terminal height
    let card_height: u16 = if inner.height >= 21 { 7 } else if inner.height >= 14 { 6 } else { 5 };
    let visible_cards = (inner.height / card_height).max(1) as usize;
    let total_items = state.episode_list.len();

    // Ensure selected_ep is visible within viewport offset
    let ep_idx = (selected_ep.saturating_sub(1)) as usize;
    if ep_idx < state.episode_offset {
        state.episode_offset = ep_idx;
    } else if ep_idx >= state.episode_offset + visible_cards {
        state.episode_offset = ep_idx.saturating_sub(visible_cards - 1);
    }

    let end_idx = (state.episode_offset + visible_cards).min(total_items);

    let mut current_y = inner.y;
    for &ep in &state.episode_list[state.episode_offset..end_idx] {
        if current_y + card_height > inner.y + inner.height {
            break;
        }

        let is_selected = ep == selected_ep;
        let is_aired = ep <= max_aired;
        let is_watched = state.watched_episodes.contains(&ep);

        let card_rect = Rect {
            x: inner.x,
            y: current_y,
            width: inner.width.saturating_sub(if total_items > visible_cards { 1 } else { 0 }),
            height: card_height,
        };

        // Render card frame
        let border_color = if is_selected {
            Color::Rgb(180, 0, 255)
        } else if !is_aired {
            Color::Rgb(35, 35, 45)
        } else {
            Color::Rgb(50, 50, 68)
        };

        let title_line = if is_selected {
            Line::from(vec![
                Span::styled(
                    format!(" [  SELECTED • EPISODE {} ] ", ep),
                    Style::default().fg(Color::Rgb(220, 150, 255)).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    " [  Enter Play ] ",
                    Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    " [ 󰗃 p Stream Options ] ",
                    Style::default().fg(Color::White).bg(Color::Rgb(60, 60, 80)),
                ),
                Span::raw(" "),
                Span::styled(
                    if is_watched { " [ w Mark Unwatched ] " } else { " [ w Mark Watched ] " },
                    Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 55)),
                ),
                Span::raw(" "),
            ])
        } else if is_watched {
            Line::from(Span::styled(
                format!(" [ 󰄬 E{} WATCHED ] ", ep),
                Style::default().fg(Color::Rgb(100, 200, 120)),
            ))
        } else if !is_aired {
            Line::from(Span::styled(
                format!(" [ 󰌾 E{} UNAIRED ] ", ep),
                Style::default().fg(Color::Rgb(100, 100, 110)),
            ))
        } else {
            Line::from(Span::styled(
                format!(" [ E{} ] ", ep),
                Style::default().fg(Color::Rgb(180, 180, 195)),
            ))
        };

        let card_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(title_line);

        let card_inner = card_block.inner(card_rect);
        frame.render_widget(card_block, card_rect);

        // Split card horizontally: [ 16:9 Thumbnail (aspect ratio ~3.33 cells wide/high) | Episode info ]
        let thumb_width = ((card_inner.height as f32 * 3.333).round() as u16)
            .max(10)
            .min(card_inner.width.saturating_sub(20));
        let card_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(thumb_width),
                Constraint::Min(0),
            ])
            .split(card_inner);

        // ── Render 16:9 Thumbnail (Dedicated per-episode StatefulProtocol — zero Kitty collisions) ──
        let thumb_rect = card_cols[0];
        let mut rendered_img = false;
        if state.has_image_support() {
            if let Some(thumb) = state.episode_thumbnails.get_mut(&ep) {
                let img_widget = ratatui_image::StatefulImage::new(None)
                    .resize(ratatui_image::Resize::Fit(None));
                frame.render_stateful_widget(img_widget, thumb_rect, thumb);
                rendered_img = true;
            } else {
                if !state.episode_fallback_thumbnails.contains_key(&ep) {
                    if let (Some(ref mut picker), Some(ref card_img)) = (&mut state.picker, &state.current_card_img) {
                        let proto = picker.new_resize_protocol(card_img.clone());
                        state.episode_fallback_thumbnails.insert(ep, proto);
                    }
                }
                if let Some(fallback_thumb) = state.episode_fallback_thumbnails.get_mut(&ep) {
                    let img_widget = ratatui_image::StatefulImage::new(None)
                        .resize(ratatui_image::Resize::Fit(None));
                    frame.render_stateful_widget(img_widget, thumb_rect, fallback_thumb);
                    rendered_img = true;
                }
            }
        }

        if !rendered_img {
            // Stylized placeholder card
            let bg_color = if is_selected {
                Color::Rgb(30, 20, 45)
            } else if !is_aired {
                Color::Rgb(20, 20, 26)
            } else {
                Color::Rgb(25, 26, 36)
            };
            let text_color = if is_selected {
                Color::Rgb(200, 120, 255)
            } else if !is_aired {
                Color::Rgb(80, 80, 95)
            } else {
                Color::Rgb(130, 130, 150)
            };

            let badge_line = if !is_aired {
                "󰌾 LOCKED"
            } else if is_watched {
                "󰄬 WATCHED"
            } else {
                " 1080p HD"
            };

            let placeholder = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(format!("  E{:02}", ep), Style::default().fg(text_color).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled(format!("  {}", badge_line), Style::default().fg(text_color))),
            ])
            .style(Style::default().bg(bg_color));
            frame.render_widget(placeholder, thumb_rect);
        }

        // ── Render Episode Details ──
        let info_rect = card_cols[1].inner(Margin { horizontal: 1, vertical: 0 });
        let ep_title = state.episode_titles.get(&ep).cloned().unwrap_or_else(|| format!("Episode {}", ep));

        let title_style = if is_selected {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else if !is_aired {
            Style::default().fg(Color::Rgb(110, 110, 125))
        } else {
            Style::default().fg(Color::Rgb(220, 220, 230))
        };

        let status_badge = if !is_aired {
            Span::styled(" 󰌾 UNAIRED ", Style::default().fg(Color::Rgb(120, 120, 135)).bg(Color::Rgb(30, 30, 38)))
        } else if is_watched {
            Span::styled(" 󰄬 WATCHED ", Style::default().fg(Color::Rgb(40, 220, 120)).bg(Color::Rgb(20, 45, 30)))
        } else {
            Span::styled(" 󰄮 AVAILABLE ", Style::default().fg(Color::Rgb(180, 180, 255)).bg(Color::Rgb(35, 30, 55)))
        };

        let audio_meta = if !is_aired {
            "Upcoming Broadcast"
        } else if anime.has_dub() {
            "24m · 1080p · Sub + Dub"
        } else {
            "24m · 1080p · Sub"
        };

        let mut lines = Vec::new();
        let mut title_spans = vec![
            Span::styled(format!("{}. ", ep), Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
            Span::styled(ep_title, title_style),
            Span::raw("   "),
            status_badge,
        ];

        if state.offline_episodes.contains_key(&ep) {
            title_spans.push(Span::raw(" "));
            title_spans.push(Span::styled(" [ ❖ Offline ] ", Style::default().fg(Color::Rgb(80, 250, 123)).bg(Color::Rgb(20, 50, 30)).add_modifier(Modifier::BOLD)));
        }

        if let Some(ftype) = state.anime_fillers.get(&ep) {
            title_spans.push(Span::raw(" "));
            match ftype.as_str() {
                "filler" => title_spans.push(Span::styled(" [ ◈ Filler ] ", Style::default().fg(Color::Rgb(255, 140, 40)).bg(Color::Rgb(55, 30, 15)).add_modifier(Modifier::BOLD))),
                "mixed" => title_spans.push(Span::styled(" [ ◈ Mixed ] ", Style::default().fg(Color::Rgb(255, 215, 0)).bg(Color::Rgb(50, 45, 15)).add_modifier(Modifier::BOLD))),
                _ => title_spans.push(Span::styled(" [ ❖ Canon ] ", Style::default().fg(Color::Rgb(139, 233, 253)).bg(Color::Rgb(20, 40, 55)))),
            }
        }

        if let Some(&(pos, dur)) = state.anime_progress.get(&ep) {
            if dur > 0.0 && pos > 5.0 && pos / dur < 0.85 {
                let bar = format_mini_bar(pos, dur, 8);
                title_spans.push(Span::raw(" "));
                title_spans.push(Span::styled(
                    format!("{} {:02}:{:02}", bar, (pos as u64) / 60, (pos as u64) % 60),
                    Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD),
                ));
            }
        }

        title_spans.push(Span::raw("  "));
        title_spans.push(Span::styled(audio_meta, Style::default().fg(Color::Rgb(140, 140, 160))));
        lines.push(Line::from(title_spans));

        // Lines 2..N: Full Episode Synopsis (never covered or shrunk when selected, and truncated cleanly at word boundaries)
        let syn_text = episode_synopsis_text(state, anime, ep, is_aired);
        let syn_rows = info_rect.height.saturating_sub(1).max(1) as usize;
        let max_syn_chars = (info_rect.width.saturating_sub(4) as usize) * syn_rows;
        let preview = crate::services::tailored::truncate_at_word_boundary(&syn_text, max_syn_chars);

        lines.push(Line::from(vec![
            Span::styled(
                "❖ ",
                Style::default().fg(if is_selected {
                    Color::Rgb(139, 233, 253)
                } else {
                    Color::Rgb(140, 110, 190)
                }),
            ),
            Span::styled(
                preview,
                Style::default().fg(if is_selected {
                    Color::Rgb(195, 195, 215)
                } else {
                    Color::Rgb(140, 140, 162)
                }),
            ),
        ]));

        let para = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true });
        frame.render_widget(para, info_rect);

        current_y += card_height;
    }

    // Scrollbar
    if total_items > visible_cards {
        let mut scrollbar_state = ScrollbarState::new(total_items)
            .position(state.episode_offset);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            inner,
            &mut scrollbar_state,
        );
    }
}

/// Option 3: Netflix TV Split-Deck View.
/// Left column: rapid-scrolling Episode Queue.
/// Right column: expansive 16:9 Thumbnail Theater + full episode inspector.
fn render_netflix_split(frame: &mut Frame, area: Rect, state: &mut AppState, anime: &Anime) {
    let max_aired = anime.aired_episodes();
    let total_eps = anime.episodes.map(|e| e as u32).unwrap_or(max_aired);
    let selected_ep = state.selected_episode.unwrap_or(1);

    let title_span = Line::from(vec![
        Span::styled(
            " 󰗃 Episode Guide (Split-Deck View) ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("[ {} / {} eps ]  ", max_aired, total_eps),
            Style::default().fg(Color::Rgb(180, 0, 255)),
        ),
        Span::styled(
            " [v] List View ",
            Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD),
        ),
    ]);

    let outer_block = Block::default()
        .title(title_span)
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));

    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(44), // Left: Episode Queue
            Constraint::Min(0),     // Right: Episode Inspector Theater
        ])
        .split(inner);

    // ── Left Column: Episode Queue ──
    let q_block = Block::default()
        .title(Span::styled(" 󰗃 Episode Queue (j/k) ", Style::default().fg(Color::Rgb(200, 200, 215)).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));
    let q_inner = q_block.inner(cols[0]);
    frame.render_widget(q_block, cols[0]);

    let visible_rows = q_inner.height as usize;
    let total_eps_count = state.episode_list.len();

    // Adjust scroll offset to keep selected_ep in view
    let ep_idx = (selected_ep.saturating_sub(1)) as usize;
    if ep_idx < state.episode_offset {
        state.episode_offset = ep_idx;
    } else if ep_idx >= state.episode_offset + visible_rows {
        state.episode_offset = ep_idx.saturating_sub(visible_rows - 1);
    }

    let end_idx = (state.episode_offset + visible_rows).min(total_eps_count);

    let mut q_lines = Vec::new();
    for &ep in &state.episode_list[state.episode_offset..end_idx] {
        let is_selected = ep == selected_ep;
        let is_aired = ep <= max_aired;
        let is_watched = state.watched_episodes.contains(&ep);

        let default_title = format!("Episode {}", ep);
        let ep_title = state.episode_titles.get(&ep).unwrap_or(&default_title);
        let truncated_title: String = ep_title.chars().take(13).collect();

        let prefix = if is_selected { "▶ " } else { "  " };

        let mut spans = vec![
            Span::styled(format!("{prefix}E{ep:<2} {truncated_title:<13}"), if is_selected {
                Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)
            } else if !is_aired {
                Style::default().fg(Color::Rgb(85, 85, 100))
            } else if is_watched {
                Style::default().fg(Color::Rgb(110, 180, 130))
            } else {
                Style::default().fg(Color::Rgb(200, 200, 210))
            }),
        ];

        // Offline badge
        if state.offline_episodes.contains_key(&ep) {
            spans.push(Span::styled(" 󰇚", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)));
        }

        // Filler badge
        if let Some(ftype) = state.anime_fillers.get(&ep) {
            if ftype == "filler" {
                spans.push(Span::styled(" ◈", Style::default().fg(Color::Rgb(255, 140, 40)).add_modifier(Modifier::BOLD)));
            } else if ftype == "mixed" {
                spans.push(Span::styled(" ◈", Style::default().fg(Color::Rgb(255, 215, 0)).add_modifier(Modifier::BOLD)));
            }
        }

        // Progress or watched tag
        if let Some(&(pos, dur)) = state.anime_progress.get(&ep) {
            if dur > 0.0 && pos > 5.0 && pos / dur < 0.85 {
                spans.push(Span::styled(
                    format!(" {:02}:{:02}", (pos as u64) / 60, (pos as u64) % 60),
                    Style::default().fg(Color::Rgb(139, 233, 253)),
                ));
            } else if is_watched {
                spans.push(Span::styled(" 󰄬", Style::default().fg(Color::Rgb(110, 180, 130))));
            }
        } else if is_watched {
            spans.push(Span::styled(" 󰄬", Style::default().fg(Color::Rgb(110, 180, 130))));
        } else if !is_aired {
            spans.push(Span::styled(" 󰌾", Style::default().fg(Color::Rgb(85, 85, 100))));
        }

        q_lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(q_lines), q_inner);

    // ── Right Column: Episode Inspector ──
    let insp_block = Block::default()
        .title(Span::styled(
            format!(" 󰄬 Episode {} Inspector ", selected_ep),
            Style::default().fg(Color::Rgb(220, 220, 240)).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(180, 0, 255)));
    let insp_inner = insp_block.inner(cols[1]);
    frame.render_widget(insp_block, cols[1]);

    // Split inspector into: Top 16:9 thumbnail frame + Quick specs, Bottom: text details
    let max_thumb_h = (insp_inner.height / 2).clamp(6, 11);
    let insp_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(max_thumb_h),
            Constraint::Min(0),
        ])
        .split(insp_inner);

    // Calculate exact 16:9 width (~3.33 columns per row) so thumbnail fits frame with zero deadspace
    let thumb_w = ((max_thumb_h as f32 * 3.333).round() as u16).min(insp_rows[0].width);
    let top_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(thumb_w),
            Constraint::Min(0),
        ])
        .split(insp_rows[0]);

    let thumb_area = top_split[0];
    let specs_area = top_split[1];
    let is_aired = selected_ep <= max_aired;
    let is_watched = state.watched_episodes.contains(&selected_ep);

    let mut rendered_img = false;
    if state.has_image_support() {
        if let Some(thumb) = state.episode_thumbnails.get_mut(&selected_ep) {
            let img_widget = ratatui_image::StatefulImage::new(None)
                .resize(ratatui_image::Resize::Fit(None));
            frame.render_stateful_widget(img_widget, thumb_area, thumb);
            rendered_img = true;
        } else {
            if !state.episode_fallback_thumbnails.contains_key(&selected_ep) {
                if let (Some(ref mut picker), Some(ref card_img)) = (&mut state.picker, &state.current_card_img) {
                    let proto = picker.new_resize_protocol(card_img.clone());
                    state.episode_fallback_thumbnails.insert(selected_ep, proto);
                }
            }
            if let Some(fallback_thumb) = state.episode_fallback_thumbnails.get_mut(&selected_ep) {
                let img_widget = ratatui_image::StatefulImage::new(None)
                    .resize(ratatui_image::Resize::Fit(None));
                frame.render_stateful_widget(img_widget, thumb_area, fallback_thumb);
                rendered_img = true;
            }
        }
    }

    if !rendered_img {
        let bg = if !is_aired { Color::Rgb(20, 20, 25) } else { Color::Rgb(30, 25, 45) };
        let placeholder = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  󰗃 EPISODE {} HIGH-DEF PREVIEW", selected_ep),
                Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                if !is_aired { "  󰌾 Broadcast locked until air date" } else { "  1080p Master HLS Stream" },
                Style::default().fg(Color::Rgb(140, 140, 160)),
            )),
        ])
        .style(Style::default().bg(bg));
        frame.render_widget(placeholder, thumb_area);
    }

    // Render Quick Specs & Action Controls on the right side of top_split (keeping bottom area 100% dedicated to synopsis)
    if specs_area.width > 12 {
        let specs_inner = specs_area.inner(Margin { horizontal: 1, vertical: 0 });
        let audio_label = if anime.has_dub() { "Japanese / English Dub" } else { "Japanese Audio (Sub only)" };
        let status_label = if !is_aired { "󰌾 Locked (Unaired)" } else if is_watched { "󰄬 Completed" } else { "󰄮 Unwatched" };
        let status_style = if is_watched {
            Style::default().fg(Color::Rgb(40, 220, 120))
        } else if !is_aired {
            Style::default().fg(Color::Rgb(140, 140, 155))
        } else {
            Style::default().fg(Color::Rgb(180, 180, 255))
        };
        let mut spec_lines = vec![
            Line::from(Span::styled("❖ STREAM SPECIFICATION", Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled("  Resolution: 1080p Master HLS", Style::default().fg(Color::Rgb(220, 220, 230)))),
            Line::from(Span::styled(format!("  Audio: {}", audio_label), Style::default().fg(Color::Rgb(190, 190, 210)))),
            Line::from(Span::styled("  Subtitles: English & Multi-Lang", Style::default().fg(Color::Rgb(160, 160, 180)))),
            Line::from(vec![
                Span::styled("  Status: ", Style::default().fg(Color::Rgb(160, 160, 180))),
                Span::styled(status_label, status_style),
            ]),
        ];

        // Filler classification badge
        if let Some(ftype) = state.anime_fillers.get(&selected_ep) {
            let (label, color) = match ftype.as_str() {
                "filler" => ("[ ◈ Filler ]", Color::Rgb(255, 140, 40)),
                "mixed" => ("[ ◈ Mixed Canon/Filler ]", Color::Rgb(255, 215, 0)),
                _ => ("[ ❖ Manga Canon ]", Color::Rgb(139, 233, 253)),
            };
            spec_lines.push(Line::from(vec![
                Span::styled("  Canon: ", Style::default().fg(Color::Rgb(160, 160, 180))),
                Span::styled(label, Style::default().fg(color).add_modifier(Modifier::BOLD)),
            ]));
        }

        // Offline status badge
        if state.offline_episodes.contains_key(&selected_ep) {
            spec_lines.push(Line::from(vec![
                Span::styled("  Vault: ", Style::default().fg(Color::Rgb(160, 160, 180))),
                Span::styled("[ ❖ Offline Local — 0ms Launch ]", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            ]));
        }

        // Playback progress bar
        if let Some(&(pos, dur)) = state.anime_progress.get(&selected_ep) {
            if dur > 0.0 {
                let pct = (pos / dur * 100.0).clamp(0.0, 100.0) as u32;
                let bar = format_mini_bar(pos, dur, 10);
                spec_lines.push(Line::from(vec![
                    Span::styled("  Progress: ", Style::default().fg(Color::Rgb(160, 160, 180))),
                    Span::styled(
                        format!("{:02}:{:02} / {:02}:{:02} ({}%) {}", (pos as u64) / 60, (pos as u64) % 60, (dur as u64) / 60, (dur as u64) % 60, pct, bar),
                        Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
        }

        spec_lines.push(Line::from(""));
        spec_lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(" [  Enter Play ] ", Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(" [ 󰇚 D Download ] ", Style::default().fg(Color::Rgb(180, 220, 255)).bg(Color::Rgb(40, 50, 70)).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(" [ 󰗃 p Options ] ", Style::default().fg(Color::White).bg(Color::Rgb(60, 60, 80))),
        ]));
        spec_lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                if is_watched { " [ w Mark Unwatched ] " } else { " [ w Mark Watched ] " },
                Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 55)),
            ),
            Span::raw(" "),
            Span::styled(" [ v List View ] ", Style::default().fg(Color::Black).bg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
        ]));

        let specs_p = Paragraph::new(spec_lines);
        frame.render_widget(specs_p, specs_inner);
    }

    // Bottom Details: 100% dedicated to Episode Title & Full Episode Synopsis (zero overlapping buttons)
    let details_area = insp_rows[1].inner(Margin { horizontal: 1, vertical: 0 });
    let default_title = format!("Episode {}", selected_ep);
    let full_title = state.episode_titles.get(&selected_ep).unwrap_or(&default_title);

    let mut details_lines = Vec::new();
    details_lines.push(Line::from(vec![
        Span::styled(
            format!("{}. {}", selected_ep, full_title),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            if !is_aired {
                "󰌾 Unaired Episode · Release Scheduled Soon"
            } else if is_watched {
                "󰄬 Watched · 24m · 1080p Stream"
            } else {
                "󰄮 Unwatched · 24m · 1080p Stream"
            },
            Style::default().fg(if is_watched { Color::Rgb(60, 220, 130) } else if !is_aired { Color::Rgb(130, 130, 150) } else { Color::Rgb(180, 0, 255) }),
        ),
    ]));
    details_lines.push(Line::from(""));

    let syn_text = episode_synopsis_text(state, anime, selected_ep, is_aired);
    let avail_syn_rows = details_area.height.saturating_sub(2).max(1) as usize;
    let max_syn_chars = (details_area.width.saturating_sub(4) as usize) * avail_syn_rows;
    let formatted_syn = crate::services::tailored::truncate_at_word_boundary(&syn_text, max_syn_chars.saturating_sub(20));

    details_lines.push(Line::from(vec![
        Span::styled("❖ EPISODE SYNOPSIS: ", Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD)),
        Span::styled(formatted_syn, Style::default().fg(Color::Rgb(220, 220, 235))),
    ]));

    let para = Paragraph::new(details_lines).wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(para, details_area);
}

/// Literary tone archetype inferred from an anime's genre and tag profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenreVoice {
    /// Depressing, gloomy, bleak, psychologically suffocating (Psychological, Thriller, Horror, Mystery, Crime, Tragedy)
    GloomyNoir,
    /// Comedic, witty, irreverent, chaotic, and fun to read (Comedy, Parody, Slapstick, Gag)
    ComedicRiot,
    /// High-voltage, tactical, competitive intensity (Sports)
    FierceArena,
    /// Warm, tender, observant, and emotionally charming (Romance, Slice of Life)
    WarmHeartfelt,
    /// Cerebral, neon-lit, speculative dystopia (Sci-Fi, Mecha, Cyberpunk)
    CyberDystopia,
    /// Kinetic, high-stakes, dramatic momentum (Action, Adventure, Fantasy, Supernatural)
    VisceralEpic,
    /// Somber, poignant, and deeply introspective (Drama)
    MelancholicElegy,
}

fn detect_genre_voice(anime: &Anime) -> GenreVoice {
    let genres = anime.genre_list();
    let lower: Vec<String> = genres.iter().map(|g| g.to_lowercase()).collect();
    let has = |kw: &str| lower.iter().any(|g| g.contains(kw));

    let is_dark = has("psychological")
        || has("thriller")
        || has("horror")
        || has("suicide")
        || has("tragedy")
        || has("terrorism")
        || has("gore")
        || has("crime")
        || has("noir")
        || has("denpa")
        || (has("mystery") && !has("comedy"));

    if is_dark {
        GenreVoice::GloomyNoir
    } else if has("comedy") || has("parody") || has("slapstick") || has("gag") {
        GenreVoice::ComedicRiot
    } else if has("sports") {
        GenreVoice::FierceArena
    } else if has("slice of life") || has("romance") || has("iyashikei") {
        GenreVoice::WarmHeartfelt
    } else if has("sci-fi") || has("mecha") || has("cyberpunk") {
        GenreVoice::CyberDystopia
    } else if has("action") || has("adventure") || has("fantasy") || has("supernatural") {
        GenreVoice::VisceralEpic
    } else {
        GenreVoice::MelancholicElegy
    }
}

/// Compose a rich, multi-line top anime synopsis that fills the top metadata panel
/// and reflects the literary voice of the anime's genre without spoiling twists.
fn compose_anime_synopsis(anime: &Anime, max_chars: usize) -> String {
    let raw_desc = anime
        .description
        .as_deref()
        .unwrap_or("No description available.");
    let base = crate::services::tailored::clean_synopsis_single_line(raw_desc);
    let voice = detect_genre_voice(anime);
    let title = anime.display_title();
    let seed = (anime.id.unsigned_abs() as usize) % 4;

    let genre_coda = match voice {
        GenreVoice::GloomyNoir => match seed {
            0 => format!("Beneath the surface of {}, a suffocating stillness hangs over every interaction—where fractured loyalties, quiet paranoia, and the cold machinery of fate leave no room for innocence or easy redemption.", title),
            1 => format!("Steeped in bleak urban dread and psychological erosion, {} pulls its cast into a tightening spiral where every unanswered question carries the heavy, inescapable scent of tragedy.", title),
            2 => format!("In the shadow-drenched world of {}, hope is treated as a fragile illusion; each chapter peels back another layer of isolation, guilt, and the quiet terror of minds pushed past their breaking point.", title),
            _ => format!("Haunted by alienation and the gloomy weight of irreversible choices, {} unfolds as a relentless descent into moral ambiguity where nobody walks away unscathed.", title),
        },
        GenreVoice::ComedicRiot => match seed {
            0 => format!("Armed with zero self-awareness and an alarming commitment to making every simple problem ten times worse, the cast of {} turns everyday life into a masterclass of escalating comedic chaos and deadpan disaster.", title),
            1 => format!("Whether through catastrophic misunderstandings, wildly overconfident schemes, or sheer weaponized stupidity, {} delivers a nonstop barrage of punchlines where dignity is always the very first casualty.", title),
            2 => format!("Packed with razor-sharp comedic timing, absurdly expressive reactions, and schemes that implode in record time, {} is a gleefully unhinged ride that refuses to take itself seriously for even a second.", title),
            _ => format!("In {}, common sense is strictly optional: every episode snowballs from a minor inconvenience into an uproarious spectacle of chaotic banter, bruised egos, and glorious comedic misfires.", title),
        },
        GenreVoice::FierceArena => format!(
            "Driven by blisteringly high competitive stakes, tactical mind games, and the raw hunger to evolve on the field, {} captures every heartbeat of the grind where a single split-second decision separates triumph from heartbreak.",
            title
        ),
        GenreVoice::WarmHeartfelt => format!(
            "Told with gentle warmth, observant humor, and a tender eye for the quiet moments between words, {} celebrates the fragile, everyday connections that slowly transform ordinary days into something unforgettable.",
            title
        ),
        GenreVoice::CyberDystopia => format!(
            "Set against the cold hum of high-concept technology and shifting futuristic horizons, {} probes the fragile line between human consciousness, engineered destiny, and the hidden truths wired beneath society.",
            title
        ),
        GenreVoice::VisceralEpic => format!(
            "Fueled by high-octane momentum, clashing convictions, and trials that forge unbreakable resolve in the heat of danger, {} builds an expansive chronicle where every confrontation raises the stakes.",
            title
        ),
        GenreVoice::MelancholicElegy => format!(
            "Rich with emotional weight, unspoken regrets, and the quiet ache of characters searching for solace in a bruised world, {} weaves a deeply human portrait of resilience and vulnerability.",
            title
        ),
    };

    let genre_coda_2 = match voice {
        GenreVoice::GloomyNoir => "Every episode tightens the psychological vise a little further, trading comfort for cold atmosphere, moral rot, and a haunting sense of inevitability as the darkness closes in around the cast.".to_string(),
        GenreVoice::ComedicRiot => "No matter how simple the setup looks on paper, you can count on this crew to overcomplicate it into a glorious, laugh-out-loud catastrophe before the opening scene is even halfway done.".to_string(),
        GenreVoice::FierceArena => "As rivalries sharpen and the margin for error vanishes, every match becomes a crucible of ego, teamwork, and relentless physical evolution under pressure.".to_string(),
        GenreVoice::WarmHeartfelt => "With an unhurried pace and genuine affection for its cast, the story turns small everyday detours and shared smiles into deeply comforting chapters.".to_string(),
        GenreVoice::CyberDystopia => "Layered with speculative intrigue and atmospheric tension, each revelation peels back another circuit of the mystery governing their world.".to_string(),
        GenreVoice::VisceralEpic => "Blending high-stakes action with unyielding personal stakes, the journey pushes its cast through a gauntlet where every choice echoes across the realm.".to_string(),
        GenreVoice::MelancholicElegy => "Through quiet introspection and deeply felt character drama, every chapter lingers on the fragile beauty and bittersweet cost of human connection.".to_string(),
    };

    let mut combined = if base.chars().count() < max_chars.saturating_sub(60) {
        format!("{}. {}", base.trim_end_matches('.'), genre_coda)
    } else {
        base
    };

    if combined.chars().count() < max_chars.saturating_sub(90) {
        combined = format!("{} {}", combined.trim_end_matches('.').to_string() + ".", genre_coda_2);
    }

    crate::services::tailored::truncate_at_word_boundary(&combined, max_chars.max(280))
}

/// Strip final-scene spoiler sentences from overly long wiki-style recaps while preserving
/// the rich setup and rising action of the episode.
fn sanitize_episode_core_without_spoilers(cleaned: &str) -> String {
    let sentences: Vec<&str> = cleaned
        .split(". ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if sentences.is_empty() {
        return String::new();
    }

    // Filter out explicit ending-spoiler markers if the summary has multiple sentences
    let mut kept = Vec::new();
    for (idx, &s) in sentences.iter().enumerate() {
        let lower = s.to_lowercase();
        let is_late_spoiler = idx >= 2
            && (lower.starts_with("in the final scene")
                || lower.starts_with("at the very end")
                || lower.starts_with("in the end,")
                || lower.contains("dies at the end")
                || lower.contains("turns out to be the killer")
                || lower.contains("is revealed to be dead"));
        if !is_late_spoiler {
            kept.push(s.trim_end_matches('.'));
        }
    }

    if kept.is_empty() {
        return cleaned.to_string();
    }
    format!("{}.", kept.join(". "))
}

/// Return a rich, full-paragraph (~950–1,180 char) episode-specific synopsis for `ep`
/// matching the expansive paragraph length of Terror in Resonance, written in the literary
/// voice of the anime's genre (comedic & fun for Comedy, depressing & gloomy for Dark genres, etc.)
/// without burning key episode twists.
fn episode_synopsis_text(state: &AppState, anime: &Anime, ep: u32, is_aired: bool) -> String {
    let voice = detect_genre_voice(anime);
    let show_title = anime.display_title();
    let ep_title = state
        .episode_titles
        .get(&ep)
        .cloned()
        .unwrap_or_else(|| format!("Episode {}", ep));
    let total_eps = anime.episodes.map(|e| e.max(1) as u32).unwrap_or_else(|| anime.aired_episodes().max(12));

    if !is_aired {
        return match voice {
            GenreVoice::GloomyNoir => format!(
                "Episode {} (\"{}\") of {} remains sealed in the dark awaiting network transmission. As the overarching shadow of the series deepens, the next link in this bleak chain of psychological tension, quiet dread, and irreversible consequences is poised to surface as soon as the broadcast lock lifts.",
                ep, ep_title, show_title
            ),
            GenreVoice::ComedicRiot => format!(
                "Episode {} (\"{}\") of {} hasn't aired yet, which means the cast is currently backstage cooking up their next catastrophic lapse in judgment. Expect fresh comedic chaos, zero impulse control, and another round of gloriously bad ideas the moment the broadcast goes live.",
                ep, ep_title, show_title
            ),
            _ => format!(
                "Episode {} (\"{}\") of {} is currently scheduled for upcoming broadcast. The full genre-attuned episode chronicle and 1080p high-definition preview will unlock automatically upon network release.",
                ep, ep_title, show_title
            ),
        };
    }

    // 1. Extract or synthesize the episode-specific core premise (spoiler-guarded)
    let raw_core = state
        .episode_synopses
        .get(&ep)
        .map(|s| crate::services::tailored::clean_synopsis_single_line(s))
        .filter(|s| s.len() > 25)
        .map(|s| sanitize_episode_core_without_spoilers(&s));

    // If the upstream synopsis is already a full, rich ~850+ character paragraph (like Terror in Resonance),
    // preserve it intact and only lightly frame it if needed to reach ~950+ characters.
    if let Some(ref core) = raw_core {
        if core.chars().count() >= 880 {
            return core.clone();
        }
    }

    let core_premise = if let Some(core) = raw_core {
        core
    } else if let Some(raw_desc) = anime.description.as_deref() {
        let cleaned = crate::services::tailored::clean_synopsis_single_line(raw_desc);
        let sentences: Vec<&str> = cleaned
            .split(". ")
            .map(str::trim)
            .filter(|s| s.len() > 20)
            .collect();
        if !sentences.is_empty() {
            let s1 = sentences[(ep.saturating_sub(1) as usize) % sentences.len()].trim_end_matches('.');
            let s2 = if sentences.len() > 1 {
                sentences[(ep as usize) % sentences.len()].trim_end_matches('.')
            } else {
                ""
            };
            if !s2.is_empty() && s1 != s2 {
                format!("{}. {}.", s1, s2)
            } else {
                format!("{}.", s1)
            }
        } else {
            format!("In \"{}\", the central conflict of {} enters a decisive new phase.", ep_title, show_title)
        }
    } else {
        format!("In \"{}\", the unfolding story of {} reaches Chapter {}.", ep_title, show_title, ep)
    };

    // 2. Deterministic per-episode variation indices so every episode reads uniquely
    let h_idx = ((anime.id.unsigned_abs() as u32).wrapping_add(ep.wrapping_mul(3))) as usize % 6;
    let m_idx = ((anime.id.unsigned_abs() as u32).wrapping_add(ep.wrapping_mul(7))) as usize % 6;
    let m2_idx = (m_idx + 3) % 6;
    let c_idx = ((anime.id.unsigned_abs() as u32).wrapping_add(ep.wrapping_mul(11))) as usize % 6;

    let arc_phase = if ep == 1 {
        "the opening chapter"
    } else if ep * 4 <= total_eps {
        "the early rising arc"
    } else if ep * 2 <= total_eps + 1 {
        "the mid-season turning point"
    } else if ep < total_eps {
        "the late-arc crucible"
    } else {
        "the culmination of the arc"
    };

    // 3. Genre-attuned Opening Lead-In, Two-Stage Mid-Episode Escalations, Atmosphere Bridge, and Spoiler-Free Closing Cadence
    let (opening, escalation, escalation_2, bridge, closing) = match voice {
        GenreVoice::GloomyNoir => {
            let openings = [
                format!("A cold, suffocating weight settles over {} as Episode {} (\"{}\") pulls the curtain back on {}.", show_title, ep, ep_title, arc_phase),
                format!("Shadows lengthen across the bleak landscape of {} in \"{}\", where the illusion of safety continues to rot from the inside out.", show_title, ep_title),
                format!("There is no comfort to be found in Episode {} (\"{}\") of {}—only the quiet, gnawing dread of a trap slowly clicking shut.", ep, ep_title, show_title),
                format!("Under a pall of heavy silence and creeping paranoia, \"{}\" drags the fragile cast of {} deeper into an unforgiving psychological abyss.", ep_title, show_title),
                format!("Every breath feels borrowed in Episode {} (\"{}\"), as {} strips away another layer of certainty and replaces it with cold, clinical tension.", ep, ep_title, show_title),
                format!("Haunted by past fractures and the looming specter of fallout, \"{}\" marks a dark, brooding escalation in {}'s downward spiral.", ep_title, show_title),
            ];
            let escalations = [
                "As the minutes tick by with agonizing precision, unspoken suspicions fester between the characters, turning every glance and half-truth into a silent calculation of survival and guilt.",
                "Rather than offering relief, each new revelation only tightens the noose: the boundary between perpetrator and victim blurs in the gloom, and the psychological toll begins to fracture even the most composed minds.",
                "In the dim, claustrophobic spaces between what is said and what is concealed, a sense of inescapable isolation takes hold, forcing decisions whose moral cost cannot be undone.",
                "Every attempt to regain control only exposes how deeply the rot runs; trust erodes into quiet desperation as the surrounding world remains coldly indifferent to their struggle.",
                "With the pressure mounting in near-silence, small details take on a sinister weight, and the ghosts of earlier choices return to demand a heavy, unforgiving reckoning.",
                "Caught between a hostile reality and their own unraveling composure, the characters are pushed toward a precipice where rational detachment threatens to snap entirely.",
            ];
            let bridge = "Beneath the bleak surface of the investigation, the air grows heavier with every passing scene—steeped in urban alienation, quiet fatalism, and the suffocating awareness that some fractures can never be mended.";
            let closings = [
                "Without giving away the final blow, the episode builds toward a chilling, hollow crescendo that leaves a lingering sense of dread long after the screen fades to black.",
                "By the time the closing moments draw near, the atmosphere is thick with impending ruin—setting up a haunting cliffhanger where nobody's conscience or safety is guaranteed.",
                "Holding its darkest cards close to the chest, this chapter closes on a deeply unsettling note that reshapes the stakes for everything that follows in the shadows ahead.",
                "What remains is a bleak, razor-sharp standoff where the true fallout is only beginning to surface, leaving the viewer suspended in cold anticipation.",
                "Refusing any easy catharsis, the narrative tightens its grip right up to the final frame, promising that the worst of the storm has yet to break.",
                "As the tension reaches a suffocating pitch without spoiling the ultimate turn, a single quiet shift in the dark changes the trajectory of the entire arc.",
            ];
            (openings[h_idx].clone(), escalations[m_idx], escalations[m2_idx], bridge, closings[c_idx])
        }
        GenreVoice::ComedicRiot => {
            let openings = [
                format!("Common sense takes another unscheduled vacation in {} as Episode {} (\"{}\") kicks off {} with maximum confidence and zero foresight.", show_title, ep, ep_title, arc_phase),
                format!("If there is a calm, rational way to handle a situation, the cast of {} wants absolutely nothing to do with it in Episode {} (\"{}\").", show_title, ep, ep_title),
                format!("Brace yourself for weaponized absurdity in \"{}\", where {} proves once again that a tiny misunderstanding can be escalated into a full-blown comedic crisis.", ep_title, show_title),
                format!("Dignity is immediately thrown out the window as Episode {} (\"{}\") launches {} into another gloriously unhinged chapter of bad ideas executed with total conviction.", ep, ep_title, show_title),
                format!("Things start off suspiciously normal in \"{}\"—which in {} is the universal warning sign that absolute pandemonium is about to break loose within thirty seconds.", ep_title, show_title),
                format!("Armed with terrible timing, bruised egos, and questionable life choices, {} charges headfirst into Episode {} (\"{}\") like a runaway comedic train.", show_title, ep, ep_title),
            ];
            let escalations = [
                "What begins as a seemingly manageable predicament snowballs at record speed as over-the-top reactions, deadpan betrayals, and galaxy-brain schemes collide in the worst possible way.",
                "Every attempt to fix the problem only pours gasoline on the fire, unleashing a rapid-fire chain of slapstick misfires, shameless bluffing, and banter that hits with pinpoint comedic timing.",
                "Between the wildly expressive facial meltdowns and plans that backfire before they even finish explaining them, the episode delights in pulling the rug out from under everyone involved.",
                "Nobody's pride survives intact as stubbornness meets sheer bad luck, turning an ordinary day into an escalating circus of chaotic improvisation and hilarious double-takes.",
                "With each character operating on a completely different wavelength of delusion, their competing agendas crash together into a masterclass of self-inflicted comedic disaster.",
                "Just when it looks like the dust might actually settle, a fresh layer of ridiculousness drops from the sky to make sure nobody escapes with a straight face.",
            ];
            let bridge = "Instead of pumping the brakes like normal human beings, the crew doubles down on their worst instincts—turning every minor awkward pause into an all-out war of petty comebacks, dramatic overreactions, and glorious buffoonery.";
            let closings = [
                "Without spoiling the final punchline, the chaos builds to an uproarious peak that leaves the cast scrambling and guarantees you'll be grinning straight into the credits.",
                "Saving its best comedic curveball for the home stretch, the episode wraps up in peak form—proving that in this world, nobody learns a single lesson and it's funnier that way.",
                "Without giving away how the scheme ultimately implodes, the final act delivers pure comedic gold that raises the bar for the madness waiting in the next episode.",
                "By the time the dust refuses to settle, the stage is set for an effortlessly funny payoff that keeps the momentum—and the laughs—rolling at full speed.",
                "Holding back the ultimate gag for you to experience firsthand, this installment cements itself as a riotous highlight of the season's comedic run.",
                "It all hurtles toward a gloriously chaotic finish that leaves every ego bruised, the status quo in shambles, and the audience thoroughly entertained.",
            ];
            (openings[h_idx].clone(), escalations[m_idx], escalations[m2_idx], bridge, closings[c_idx])
        }
        GenreVoice::FierceArena => {
            let openings = [
                format!("The competitive pulse of {} surges to a blistering tempo in Episode {} (\"{}\") as {} puts every ounce of training on the line.", show_title, ep, ep_title, arc_phase),
                format!("Under the unforgiving glare of the arena, \"{}\" pushes the contenders of {} into a high-stakes test of vision, stamina, and nerve.", ep_title, show_title),
                format!("Every muscle and tactical instinct is stretched to the limit in Episode {} (\"{}\"), where {} refuses to let anyone coast on past victories.", ep, ep_title, show_title),
                format!("With pride and momentum hanging in the balance, \"{}\" ignites a fierce new chapter in {}'s relentless climb toward the top.", ep_title, show_title),
                format!("The margin for error shrinks to a razor's edge in Episode {} (\"{}\") of {}, where split-second decisions dictate who controls the tempo.", ep, ep_title, show_title),
                format!("Raw ambition meets cold tactical reality in \"{}\", driving {} into a clash where grit alone may not be enough to break through.", ep_title, show_title),
            ];
            let escalations = [
                "As the pace intensifies, both physical exhaustion and mental pressure mount—forcing the players to read the field in real time, adapt their formations, and confront their own technical blind spots.",
                "Every clash of styles exposes the gap between where they are and where they need to be, turning the match into a Chess-like battle of spatial awareness, ego, and sheer refusal to yield.",
                "With the opposition closing down every familiar avenue of attack, instinct and preparation are forced to evolve on the fly under the weight of expectation.",
                "Beneath the roar of competition, personal rivalries and unspoken doubts sharpen the stakes of every play, demanding a breakthrough when stamina is already burning low.",
                "Each momentum swing tests the cohesion of the squad, challenging them to trust their vision and execute under suffocating defensive pressure.",
                "Refusing to back down, the contenders trade tactical blows in a high-velocity showcase of footwork, anticipation, and unyielding competitive fire.",
            ];
            let bridge = "Every bead of sweat and split-second adjustment carries the weight of hours spent grinding in the shadows, transforming the contest into a visceral test of willpower where hesitation is punished immediately.";
            let closings = [
                "Without spoiling the decisive play, the episode surges toward an electric climax that leaves the pulse racing and sets the stage for the next whistle.",
                "Keeping the final outcome unspoiled, the tension builds to a breathless crescendo where a single moment of brilliance or hesitation will define everything that follows.",
                "By the final minutes, the intensity is off the charts—leaving the arena charged with adrenaline and the next phase of the rivalry wide open.",
                "Holding the ultimate turning point in reserve, this chapter delivers pure competitive cinema that captures why every second on the field matters.",
                "As the clock winds down toward a pivotal showdown, the hunger to evolve burns brighter than ever heading into the next installment.",
                "It all converges on a high-stakes cliffhanger that tests the heart of a true competitor without giving away who claims the upper hand.",
            ];
            (openings[h_idx].clone(), escalations[m_idx], escalations[m2_idx], bridge, closings[c_idx])
        }
        GenreVoice::WarmHeartfelt => {
            let openings = [
                format!("A gentle, sunlit warmth threads through {} as Episode {} (\"{}\") brings a tender new moment of connection to {}.", show_title, ep, ep_title, arc_phase),
                format!("With its signature blend of charm and quiet emotional honesty, \"{}\" invites us back into the everyday world of {}.", ep_title, show_title),
                format!("Small gestures carry unspoken meaning in Episode {} (\"{}\") of {}, where ordinary routines slowly give way to something heartfelt.", ep, ep_title, show_title),
                format!("Set against the comforting rhythm of daily life, \"{}\" captures the subtle shifts in distance between the characters of {}.", ep_title, show_title),
                format!("There is a disarming sincerity to Episode {} (\"{}\"), as {} lingers on the little moments that quietly bring people closer together.", ep, ep_title, show_title),
                format!("Blending lighthearted warmth with genuine emotional resonance, \"{}\" deepens the bonds at the heart of {}.", ep_title, show_title),
            ];
            let escalations = [
                "Through shared conversations, hesitant glances, and the endearing awkwardness of trying to be honest with oneself, the episode lets its characters breathe and grow at a natural, unhurried pace.",
                "Even the simplest interactions feel rich with affection and subtle humor, revealing new sides of familiar personalities as they navigate the delicate balance between comfort and vulnerability.",
                "As unexpected detours nudges them out of their usual habits, unspoken feelings bubble gently toward the surface in ways that feel both relatable and deeply endearing.",
                "Every scene is crafted with a warm, observant eye for human connection, turning fleeting everyday moments into memories that linger between the cast.",
                "Between the gentle banter and quiet pauses, the emotional distance between them shifts by degrees, making every small step forward feel genuinely earned.",
                "With warmth and understated grace, the narrative weaves together light comedy and sincere emotion without ever forcing the sentiment.",
            ];
            let bridge = "Wrapped in soft seasonal atmosphere and effortless charm, the story reminds us how a single kind word or shared laugh can quietly brighten an entire day.";
            let closings = [
                "Without spoiling the sweetest beats of the episode, the story settles into a warm, lingering cadence that leaves the heart a little fuller.",
                "Keeping its most touching moment for you to watch unfold, this chapter closes on a tender note that makes the characters even easier to root for.",
                "By the time the episode winds down, a quiet shift in their dynamic promises even more heartfelt developments in the days ahead.",
                "Leaving the final emotional payoff unspoiled, it delivers a soothing, memorable installment that captures the magic of the series at its best.",
                "As the afternoon light fades on another chapter, the bonds forged here quietly reshape the warmth of the journey moving forward.",
                "It concludes with a gentle, resonant beat that celebrates the beauty of small steps and leaves you eager for the next chapter.",
            ];
            (openings[h_idx].clone(), escalations[m_idx], escalations[m2_idx], bridge, closings[c_idx])
        }
        GenreVoice::CyberDystopia => {
            let openings = [
                format!("The neon-lit circuitry of {} hums with rising tension as Episode {} (\"{}\") initiates a critical new sequence in {}.", show_title, ep, ep_title, arc_phase),
                format!("Across the shifting digital and mechanical frontiers of {}, \"{}\" blurs the boundary between human agency and systemic control.", show_title, ep_title),
                format!("High-concept intrigue and atmospheric tension converge in Episode {} (\"{}\") of {}, where every signal carries a hidden agenda.", ep, ep_title, show_title),
                format!("In the cold architecture of {}'s future, \"{}\" uncovers a new layer of the enigma wired beneath the surface of reality.", show_title, ep_title),
                format!("As data streams and human instincts collide in Episode {} (\"{}\"), {} pushes its speculative premise into uncharted territory.", ep, ep_title, show_title),
                format!("Beneath the hum of technology and fractured memory, \"{}\" accelerates the cerebral mystery driving {}.", ep_title, show_title),
            ];
            let escalations = [
                "As anomalies multiply across the system, the characters are forced to question the reliability of their own perceptions while navigating a web of technological and philosophical stakes.",
                "Every discovery opens two more questions about the architecture of their world, blending sleek futuristic atmosphere with the raw human struggle to preserve identity and free will.",
                "With unseen forces recalibrating the board in real time, the line between observer and target dissolves amidst high-tech tension and calculated risk.",
                "Each scene layers atmospheric world-building over tightening suspense, challenging the cast to outthink a system designed to anticipate their every move.",
                "Caught between cold logic and visceral human impulse, their choices send ripples through a network where no secret stays buried forever.",
                "As the technological and personal threads tighten together, the true scale of the conspiracy begins to cast a long shadow over the crew.",
            ];
            let bridge = "Amidst the hum of servers and the cold geometry of a meticulously engineered world, every philosophical question becomes a matter of immediate survival for those caught in the circuit.";
            let closings = [
                "Without spoiling the final revelation in the feed, the episode builds to a cerebral, pulse-quickening cliffhanger that rewires the stakes ahead.",
                "Keeping its key twist encrypted until you press play, this chapter closes on a haunting, high-voltage note that expands the horizon of the series.",
                "By the final frame, the equilibrium of their world has shifted irrevocably—setting up a thrilling trajectory for the next transmission.",
                "Holding back the decisive turn of the third act, the narrative leaves you immersed in a rich, thought-provoking atmosphere of sci-fi suspense.",
                "As the signal fades on a pivotal standoff, the questions raised here promise to echo across every episode that follows.",
                "It culminates in a taut, visionary sequence that showcases the series' signature blend of intellect and adrenaline without giving away the endgame.",
            ];
            (openings[h_idx].clone(), escalations[m_idx], escalations[m2_idx], bridge, closings[c_idx])
        }
        GenreVoice::VisceralEpic => {
            let openings = [
                format!("The stakes of {} surge forward with cinematic intensity as Episode {} (\"{}\") ignites {}.", show_title, ep, ep_title, arc_phase),
                format!("Gathering storm clouds and clashing convictions define \"{}\" as {} drives its heroes into treacherous new territory.", ep_title, show_title),
                format!("Action, resolve, and escalating danger converge in Episode {} (\"{}\") of {}, where standing still is no longer an option.", ep, ep_title, show_title),
                format!("As rival forces and ancient perils close in, \"{}\" marks a pivotal test of strength and loyalty across {}.", ep_title, show_title),
                format!("Every step forward demands a price in Episode {} (\"{}\"), pushing the central journey of {} toward a volatile flashpoint.", ep, ep_title, show_title),
                format!("Charged with kinetic momentum and rising tension, \"{}\" raises the curtain on a defining confrontation in {}.", ep_title, show_title),
            ];
            let escalations = [
                "As opposing wills collide and the margin for survival narrows, the characters are forced to rely on hard-won instincts, tested camaraderie, and the courage to face overwhelming odds.",
                "Beneath the spectacle of the conflict, personal vows and conflicting ideals add weight to every strike and strategic gambit on the battlefield.",
                "With danger pressing in from multiple fronts, unexpected complications force them to adapt on the fly while protecting what matters most.",
                "Each sequence builds momentum with deliberate pacing, balancing explosive escalation against the quiet determination of characters who refuse to break.",
                "As old certainties are tested in the crucible of combat and discovery, the true measure of their strength is called into question.",
                "Between razor-sharp tension and surging adrenaline, the narrative weaves character growth directly into the heat of the unfolding trial.",
            ];
            let bridge = "Across the widening theater of conflict, alliances are tested under fire and the true cost of ambition becomes impossible to ignore as the storm gathers strength.";
            let closings = [
                "Without spoiling how the clash resolves, the episode hurtles toward an electrifying final act that leaves the momentum sky-high.",
                "Keeping the decisive turning point unspoiled, this installment delivers a gripping, high-impact chapter that reshapes the road ahead.",
                "By the time the dust begins to swirl in the closing moments, the balance of power has shifted in a way that demands immediate follow-up.",
                "Holding its biggest surprise in reserve for the screen, the story sets up a thrilling continuation where the stakes have never been higher.",
                "As the tension peaks on the edge of a major breakthrough, the stage is set for an unforgettable next chapter in the saga.",
                "It all builds to a commanding, spoiler-free crescendo that captures the relentless spirit and scale of the series.",
            ];
            (openings[h_idx].clone(), escalations[m_idx], escalations[m2_idx], bridge, closings[c_idx])
        }
        GenreVoice::MelancholicElegy => {
            let openings = [
                format!("A quiet, poignant gravity settles over {} in Episode {} (\"{}\") as {} brings buried emotions to the surface.", show_title, ep, ep_title, arc_phase),
                format!("Woven with emotional delicacy and lingering introspection, \"{}\" traces the fragile threads binding the cast of {}.", ep_title, show_title),
                format!("In Episode {} (\"{}\") of {}, the weight of unspoken words and past scars shapes every interaction.", ep, ep_title, show_title),
                format!("With somber grace and piercing honesty, \"{}\" explores the quiet crossroads facing the characters of {}.", ep_title, show_title),
                format!("Every memory carries an echo in Episode {} (\"{}\"), as {} delves deeper into the vulnerability at the heart of its story.", ep, ep_title, show_title),
                format!("Set against an atmosphere of quiet longing and human complexity, \"{}\" unfolds as a deeply resonant chapter of {}.", ep_title, show_title),
            ];
            let escalations = [
                "As the characters navigate the delicate space between what they wish to protect and what they fear to lose, small confessions and quiet silences speak louder than grand declarations.",
                "Each interaction peels back another layer of guarded emotion, revealing how deeply their fates and unresolved regrets are intertwined.",
                "Caught between the pull of the past and the uncertainty of tomorrow, they are forced to confront truths they have spent so long trying to outrun.",
                "With understated dramatic power, the episode captures the ache of flawed people reaching for understanding in a world that rarely makes it easy.",
                "Every choice ripples through the fragile equilibrium of their relationships, testing the resilience of the bonds they have struggled to build.",
                "Through careful pacing and deeply felt character work, the narrative turns quiet personal dilemmas into moments of genuine dramatic weight.",
            ];
            let bridge = "Beneath the surface of every conversation lies a tender, melancholic current—where the desire for forgiveness and the fear of being truly seen pull the characters in opposite directions.";
            let closings = [
                "Without giving away the final emotional turn, the episode draws to a hauntingly resonant close that lingers in the mind.",
                "Keeping its most poignant beat unspoiled, this chapter leaves the characters at a crossroads that quietly transforms the story ahead.",
                "By the closing moments, an unspoken shift between them sets the stage for a deeply moving continuation in the next installment.",
                "Holding the ultimate resolution close to the chest, the narrative delivers a rich, emotionally absorbing chronicle from start to finish.",
                "As the final scene settles into a quiet hush, the emotional stakes of the journey feel more immediate and personal than ever.",
                "It concludes on a bittersweet, unforgettable note that honors the depth of the characters without spoiling what awaits them next.",
            ];
            (openings[h_idx].clone(), escalations[m_idx], escalations[m2_idx], bridge, closings[c_idx])
        }
    };

    let mut full = format!("{} {} {} {}", opening, core_premise, escalation, closing);
    if full.chars().count() < 860 {
        full = format!("{} {} {} {} {}", opening, core_premise, escalation, escalation_2, closing);
    }
    if full.chars().count() < 860 {
        full = format!("{} {} {} {} {} {}", opening, core_premise, escalation, escalation_2, bridge, closing);
    }
    let cleaned = crate::services::tailored::clean_synopsis_single_line(&full);
    crate::services::tailored::truncate_at_word_boundary(&cleaned, 1080)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genre_voice_detection_and_full_length_synopsis() {
        let mut state = AppState::new();
        state.episode_titles.insert(1, "The First Disaster".to_string());
        state.episode_synopses.insert(1, "Kazuma and Aqua arrive in a new town and immediately get into debt.".to_string());

        let comedy_anime = Anime {
            id: 30831,
            title_romaji: "KonoSuba".to_string(),
            genres: serde_json::to_string(&vec!["Adventure", "Comedy", "Fantasy"]).unwrap(),
            episodes: Some(10),
            ..Default::default()
        };

        assert_eq!(detect_genre_voice(&comedy_anime), GenreVoice::ComedicRiot);
        let comedy_syn = episode_synopsis_text(&state, &comedy_anime, 1, true);
        assert!(comedy_syn.chars().count() >= 750, "Synopsis should be expanded to Terror in Resonance length: got {} chars", comedy_syn.chars().count());
        assert!(comedy_syn.contains("Kazuma and Aqua"));

        let dark_anime = Anime {
            id: 20661,
            title_romaji: "Zankyou no Terror".to_string(),
            title_english: Some("Terror in Resonance".to_string()),
            genres: serde_json::to_string(&vec!["Drama", "Mystery", "Psychological", "Thriller"]).unwrap(),
            episodes: Some(11),
            ..Default::default()
        };

        assert_eq!(detect_genre_voice(&dark_anime), GenreVoice::GloomyNoir);
        let dark_syn = episode_synopsis_text(&state, &dark_anime, 1, true);
        assert!(dark_syn.chars().count() >= 750, "Dark synopsis should be expanded to Terror in Resonance length: got {} chars", dark_syn.chars().count());
    }

    #[test]
    fn test_format_mini_bar() {
        assert_eq!(format_mini_bar(0.0, 100.0, 10), "[──────────]");
        assert_eq!(format_mini_bar(50.0, 100.0, 10), "[━━━━━─────]");
        assert_eq!(format_mini_bar(100.0, 100.0, 10), "[━━━━━━━━━━]");
        assert_eq!(format_mini_bar(0.0, 0.0, 10), "");
    }
}
