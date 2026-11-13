//! Home screen — Netflix-style category rows.
//! Renders: Featured banner + Continue Watching + Trending + Popular + Top Rated + Seasonal.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use crate::{
    db::cache::Anime,
    state::{AppState, CategoryRow},
    ui::components::cover::HalfblockCover,
};

/// Width of each anime card in the row (chars)
const CARD_WIDTH: u16  = 22;
/// Height of each anime card (rows)
const CARD_HEIGHT: u16 = 10;
/// Gap between cards
const CARD_GAP: u16    = 2;

/// Render the full home screen.
pub fn render(frame: &mut Frame, state: &mut AppState, categories: &HomeData) {
    let area = frame.area();
    let banner_anime = active_banner_anime(state, categories).or(categories.featured.as_ref());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Featured banner
            Constraint::Length(1),  // Spacer
            Constraint::Min(0),     // Rows
            Constraint::Length(2),  // Bottom Navigation Bar with top rail
        ])
        .split(area);

    render_featured(frame, chunks[0], state, banner_anime, categories);
    render_rows(frame, chunks[2], state, categories);
    render_footer(frame, chunks[3], state);
}

fn render_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();

    let anilist_span = if state.auth.is_authenticated() {
        let name = state.auth.username.as_deref().unwrap_or("AniList");
        Span::styled(format!("󰄬 [a] {} ", name), Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("󰄬 [a] AniList Login ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))
    };

    let mut spans = vec![
        Span::styled("  [/] Search ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(theme.divider)),
        Span::styled("󰋽 [w] Wizard ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(theme.divider)),
        Span::styled(format!("󰃭 [T] {} ", theme.name), Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(theme.divider)),
        anilist_span,
        Span::styled(" · ", Style::default().fg(theme.divider)),
    ];

    if let Some(ref track) = state.now_playing {
        spans.push(Span::styled(
            format!("󰐊 Playing: {} ", track.chars().take(25).collect::<String>()),
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" [X] Stop ", Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(" · ", Style::default().fg(theme.divider)));
    }

    if let Some((_, ep, pct, ref spd)) = state.active_download {
        spans.push(Span::styled(
            format!("󰇚 Downloading Ep {}: {:.0}% ({}) ", ep, pct, spd),
            Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" · ", Style::default().fg(theme.divider)));
    }

    if let Some((pct, ref spd)) = state.update_progress {
        spans.push(Span::styled(
            format!("󰚰 Updating: {:.0}% ({}) ", pct, spd),
            Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" · ", Style::default().fg(theme.divider)));
    } else if let Some(ref update) = state.available_update {
        spans.push(Span::styled(
            format!("󰚰 [U] Update ({}) ", update.latest_version),
            Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" · ", Style::default().fg(theme.divider)));
    }

    if state.active_row == CategoryRow::ContinueWatching || state.active_row == CategoryRow::Watchlist {
        spans.push(Span::styled("󰅖 [x] Remove", Style::default().fg(Color::Rgb(255, 85, 85)).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(" · ", Style::default().fg(theme.divider)));
    }

    spans.extend(vec![
        Span::styled("󰘳 [Enter] Watch", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(theme.divider)),
        Span::styled(" [r] Resume", Style::default().fg(theme.text)),
        Span::styled(" · ", Style::default().fg(theme.divider)),
        Span::styled("󰋜 [+] Watchlist", Style::default().fg(theme.accent)),
        Span::styled(" · ", Style::default().fg(theme.divider)),
        Span::styled("󰒓 [s] Settings", Style::default().fg(theme.text_dim)),
        Span::styled(" · ", Style::default().fg(theme.divider)),
        Span::styled(" [?] Help", Style::default().fg(theme.text_dim)),
        Span::styled(" · ", Style::default().fg(theme.divider)),
        Span::styled("󰈆 [q] Quit ", Style::default().fg(theme.text_dim)),
    ]);

    let line = Line::from(spans);

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.divider));

    let footer = Paragraph::new(line)
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(footer, area);
}

/// Featured banner at the top — highlights the currently selected anime or first trending anime.
fn render_featured(frame: &mut Frame, area: Rect, state: &mut AppState, anime: Option<&Anime>, data: &HomeData) {
    let theme = state.theme();

    let banner = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(
            "  SOLUS • FEATURED SELECTION ",
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = banner.inner(area);
    frame.render_widget(banner, area);

    let Some(anime) = anime else {
        let msg = vec![
            Line::from(Span::styled(
                " 󰑮 Syncing Anime Library...",
                Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Fetching trending anime and episode updates from AniList...",
                Style::default().fg(theme.text_dim),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(" [/] Live Search", Style::default().fg(theme.secondary)),
                Span::styled("   ·   ", Style::default().fg(theme.divider)),
                Span::styled(format!("󰃭 [T] Theme: {}", theme.name), Style::default().fg(theme.text_dim)),
            ]),
        ];
        let p = Paragraph::new(msg).wrap(ratatui::widgets::Wrap { trim: true });
        frame.render_widget(p, inner);
        return;
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(CARD_WIDTH), // Cover art (matches card width = 22, inner = 20x6)
            Constraint::Length(1),          // Vertical divider
            Constraint::Min(0),             // Anime info & actions
        ])
        .split(inner);

    let title   = anime.display_title();
    let genres  = anime.genre_list().join(" · ");
    let score   = anime
        .score
        .map(|s| format!(" {:.1}", s as f32 / 10.0))
        .unwrap_or_else(|| " N/A".to_string());
    let eps     = anime
        .episodes
        .map(|e| format!("{} eps", e))
        .unwrap_or_else(|| "? eps".to_string());
    let fmt     = anime.format.as_deref().unwrap_or("TV");
    let status  = anime.status.as_deref().unwrap_or("Unknown");
    let year    = anime
        .season_year
        .map(|value| value.to_string())
        .unwrap_or_else(|| "?".to_string());
    let in_watchlist = data.watchlist.iter().any(|item| item.id == anime.id);
    let watchlist_label = if in_watchlist {
        " - Remove "
    } else {
        " + Watchlist "
    };
    let play_label = if let Some(next_ep) = data.resume_next.get(&anime.id) {
        format!(" Continue E{} ", next_ep)
    } else {
        " Start ".to_string()
    };
    let watched = state
        .banner_progress
        .filter(|(anime_id, _)| *anime_id == anime.id)
        .map(|(_, watched)| watched)
        .unwrap_or(0);
    let progress = format!(
        "{} / {} watched",
        watched,
        anime.episodes.map(|value| value.to_string()).unwrap_or_else(|| "?".to_string())
    );
    let desc    = anime
        .description
        .as_deref()
        .unwrap_or("No description available.")
        .chars()
        .take(120)
        .collect::<String>();
    let launch_status = state
        .last_played
        .as_deref()
        .filter(|_| state.last_played_anime_id == Some(anime.id));
    let focus_label = match state.active_row {
        CategoryRow::ContinueWatching => " Continue Watching ",
        CategoryRow::Watchlist => " My Watchlist ",
        CategoryRow::Recommended => " For You ",
        CategoryRow::Trending => " Trending Now ",
        CategoryRow::Popular => " Popular Picks ",
        CategoryRow::TopRated => " Top Rated ",
        CategoryRow::Seasonal => " Seasonal ",
    };

    let cover_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle));
    let cover_inner = cover_block.inner(cols[0]);
    frame.render_widget(cover_block, cols[0]);

    if state.has_image_support() {
        if let Some(cover) = state.anime_banners.get_mut(&anime.id)
            .or_else(|| state.banner_cache.get_mut(&anime.id))
            .or_else(|| state.cover_cache.get_mut(&anime.id))
            .or_else(|| state.detail_cache.get_mut(&anime.id))
        {
            let image_widget = ratatui_image::StatefulImage::new(None)
                .resize(ratatui_image::Resize::Fit(None));
            frame.render_stateful_widget(image_widget, cover_inner, cover);
        } else {
            let label = if state.cover_failed_anime_id == Some(anime.id) {
                "Cover unavailable"
            } else {
                "󰑮 Loading cover..."
            };
            let loading = Paragraph::new(label)
                .style(Style::default().fg(theme.text_dim))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(loading, cover_inner);
        }
    } else {
        frame.render_widget(
            HalfblockCover { anime_id: anime.id, title: anime.display_title() },
            cover_inner,
        );
    }

    // Vertical divider line between thumbnail and info
    let divider = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(theme.divider));
    frame.render_widget(divider, cols[1]);

    let meta_line = Line::from(vec![
        Span::styled(
            format!(" {} ", focus_label),
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::default()),
        Span::styled(
            score,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(theme.divider)),
        Span::styled(
            eps,
            Style::default().fg(theme.text),
        ),
        Span::styled("  ·  ", Style::default().fg(theme.divider)),
        Span::styled(
            format!("{} {}", fmt, year),
            Style::default().fg(theme.text),
        ),
        Span::styled("  ·  ", Style::default().fg(theme.divider)),
        Span::styled(
            status,
            Style::default().fg(theme.text_dim),
        ),
    ]);

    let mut content = vec![
        Line::from(Span::styled(
            title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        meta_line,
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!(" {} ", progress),
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("   ", Style::default()),
            Span::styled(genres, Style::default().fg(theme.accent)),
        ]),
        Line::from(Span::styled(
            desc,
            Style::default().fg(theme.text_dim),
        )),
    ];

    if let Some(status) = launch_status {
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::styled(
                " External player launched ",
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(status, Style::default().fg(theme.text_dim)),
        ]));
    }

    content.push(Line::from(""));
    content.push(Line::from(vec![
        Span::styled(
            play_label,
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            watchlist_label,
            Style::default()
                .fg(Color::Black)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            " 󰘳 [d] Details ",
            Style::default()
                .fg(theme.text)
                .bg(theme.divider),
        ),
        Span::raw("   "),
        Span::styled(
            "  [r] Resume ",
            Style::default()
                .fg(Color::Black)
                .bg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            format!(" 󰃭 [T] {} ", theme.name),
            Style::default()
                .fg(theme.primary)
                .bg(theme.border_idle)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    let block = Paragraph::new(content)
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(block, cols[2]);
}

fn active_banner_anime<'a>(state: &AppState, data: &'a HomeData) -> Option<&'a Anime> {
    let (row_key, items) = match state.active_row {
        CategoryRow::ContinueWatching => ("continue_watching", &data.continue_watching),
        CategoryRow::Watchlist        => ("watchlist", &data.watchlist),
        CategoryRow::Recommended      => ("recommended", &data.recommended),
        CategoryRow::Trending         => ("trending", &data.trending),
        CategoryRow::Popular          => ("popular", &data.popular),
        CategoryRow::TopRated         => ("top_rated", &data.top_rated),
        CategoryRow::Seasonal         => ("seasonal", &data.seasonal),
    };

    items.get(state.row_offset(row_key))
}

/// Render all category rows.
fn render_rows(frame: &mut Frame, area: Rect, state: &mut AppState, data: &HomeData) {
    let rows: Vec<(String, &str, &[Anime])> = vec![
        (" Continue Watching".to_string(), "continue_watching", &data.continue_watching),
        ("󰋜 My Watchlist".to_string(),      "watchlist",         &data.watchlist),
        (recommended_label(data), "recommended",    &data.recommended),
        (" Trending".to_string(),         "trending",          &data.trending),
        (" Popular".to_string(),          "popular",           &data.popular),
        (" Top Rated".to_string(),        "top_rated",         &data.top_rated),
        ("󰃭 Seasonal".to_string(),         "seasonal",          &data.seasonal),
    ];

    // Filter out empty rows
    let visible: Vec<_> = rows.into_iter().filter(|(_, _, items)| !items.is_empty()).collect();
    let row_count        = visible.len() as u16;

    if row_count == 0 {
        return;
    }

    // Each row = 1 (label) + CARD_HEIGHT (cards) + 1 (gap)
    let row_height  = 1 + CARD_HEIGHT + 1;
    let max_rows_that_fit = (area.height / row_height).max(1);
    let visible_rows      = row_count.min(max_rows_that_fit);
    let constraints: Vec<Constraint> = (0..visible_rows)
        .map(|_| Constraint::Length(row_height))
        .collect();

    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    // Determine which row key is currently active for highlight
    let active_key = match state.active_row {
        CategoryRow::ContinueWatching => "continue_watching",
        CategoryRow::Watchlist        => "watchlist",
        CategoryRow::Recommended      => "recommended",
        CategoryRow::Trending         => "trending",
        CategoryRow::Popular          => "popular",
        CategoryRow::TopRated         => "top_rated",
        CategoryRow::Seasonal         => "seasonal",
    };

    let visible_rows = visible_rows as usize;
    let active_idx   = visible
        .iter()
        .position(|(_, key, _)| *key == active_key)
        .unwrap_or(0);
    let start_idx    = active_idx.saturating_sub(visible_rows.saturating_sub(1));

    for (i, (label, key, items)) in visible.iter().skip(start_idx).take(visible_rows).enumerate() {
        if i < row_areas.len() {
            render_row(
                frame,
                row_areas[i],
                state,
                label,
                key,
                items,
                *key == active_key,
                &data.recommended_reasons,
                &data.progress_labels,
            );
        }
    }
}

/// Render a single horizontal category row with a label and cards.
#[allow(clippy::too_many_arguments)]
fn render_row(
    frame:     &mut Frame,
    area:      Rect,
    state:     &mut AppState,
    label:     &str,
    key:       &str,
    items:     &[Anime],
    is_active: bool,
    recommended_reasons: &HashMap<i64, String>,
    progress_labels: &HashMap<i64, String>,
) {
    let theme = state.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    // Section Rail with divider line
    let rail_color = if is_active { theme.primary } else { theme.divider };
    let label_style = if is_active {
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text_dim)
            .add_modifier(Modifier::BOLD)
    };

    let title_prefix = format!("── {} ", label);
    let title_suffix = format!(" [ {} anime ] ──", items.len());
    let sep_len = (area.width as usize).saturating_sub(title_prefix.chars().count() + title_suffix.chars().count() + 1);

    let title_line = Line::from(vec![
        Span::styled(title_prefix, label_style),
        Span::styled("─".repeat(sep_len), Style::default().fg(rail_color)),
        Span::styled(title_suffix, Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(Paragraph::new(title_line), chunks[0]);

    // Cards — the first visible card is the selected one when this row is active
    let card_area   = chunks[1];
    let offset      = state.row_offset(key);
    let visible_n   = (card_area.width / (CARD_WIDTH + CARD_GAP)).max(1) as usize;
    let visible_items: Vec<&Anime> = items.iter().skip(offset).take(visible_n).collect();

    for (i, anime) in visible_items.iter().enumerate() {
        let x    = card_area.x + i as u16 * (CARD_WIDTH + CARD_GAP);
        let rect = Rect {
            x,
            y:      card_area.y,
            width:  CARD_WIDTH,
            height: CARD_HEIGHT,
        };
        if rect.x + rect.width <= card_area.x + card_area.width {
            let reason = if key == "recommended" {
                recommended_reasons.get(&anime.id).map(String::as_str)
            } else {
                None
            };
            let progress = progress_labels.get(&anime.id).map(String::as_str);
            render_card(frame, rect, anime, is_active && i == 0, reason, progress, state);
        }
    }
}

/// Render a single anime card (cover + title + score).
/// Dual-tier rounded framing: subtle border on idle cards, illuminated border + badge on focused card.
fn render_card(
    frame: &mut Frame,
    area: Rect,
    anime: &Anime,
    selected: bool,
    reason: Option<&str>,
    progress: Option<&str>,
    state: &mut AppState,
) {
    if area.height < 4 {
        return;
    }

    let theme = state.theme();

    // Dual-tier rounded framing
    let (border_color, title_badge) = if selected {
        (theme.border_focus, Some("  SELECTED "))
    } else {
        (theme.border_idle, None)
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    if let Some(badge) = title_badge {
        block = block.title(Span::styled(
            badge,
            Style::default()
                .fg(Color::Black)
                .bg(theme.border_focus)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // cover art
            Constraint::Length(1), // title
            Constraint::Length(1), // score + format
        ])
        .split(inner);

    if state.has_image_support() {
        if let Some(cover) = state.cover_cache.get_mut(&anime.id) {
            let image_widget = ratatui_image::StatefulImage::new(None)
                .resize(ratatui_image::Resize::Fit(None));
            frame.render_stateful_widget(image_widget, chunks[0], cover);
        } else {
            frame.render_widget(
                HalfblockCover { anime_id: anime.id, title: anime.display_title() },
                chunks[0],
            );
        }
    } else {
        frame.render_widget(
            HalfblockCover { anime_id: anime.id, title: anime.display_title() },
            chunks[0],
        );
    }

    // Title (cleanly truncated)
    let max_title_chars = (chunks[1].width as usize).saturating_sub(1);
    let title_str = anime.short_title();
    let short_title = if title_str.chars().count() > max_title_chars {
        let t: String = title_str.chars().take(max_title_chars.saturating_sub(1)).collect();
        format!("{}…", t)
    } else {
        title_str
    };
    let title = Paragraph::new(Span::styled(
        short_title,
        Style::default()
            .fg(if selected { Color::White } else { theme.text })
            .add_modifier(if selected { Modifier::BOLD | Modifier::UNDERLINED } else { Modifier::BOLD }),
    ));
    frame.render_widget(title, chunks[1]);

    // Score + format (fully transparent background)
    let meta_text = match (progress, reason) {
        (Some(progress), Some(label)) => format!(
            "{} · {}",
            truncate_reason(progress, 10),
            truncate_reason(label, 7)
        ),
        (Some(progress), None) => truncate_reason(progress, 18),
        (None, Some(label)) => format!("• {}", truncate_reason(label, 18)),
        (None, None) => {
            let score_str = anime
                .score
                .map(|s| format!(" {:.1}", s as f32 / 10.0))
                .unwrap_or_else(|| " N/A".to_string());
            let fmt_str   = anime.format.as_deref().unwrap_or("TV");
            format!("{} · {}", score_str, fmt_str)
        }
    };
    let meta_color = if selected {
        theme.accent
    } else if reason.is_some() || progress.is_some() {
        theme.secondary
    } else {
        theme.text_dim
    };
    let meta = Paragraph::new(Span::styled(
        meta_text,
        Style::default().fg(meta_color),
    ));
    frame.render_widget(meta, chunks[2]);
}

fn truncate_reason(reason: &str, max_chars: usize) -> String {
    let chars: Vec<char> = reason.chars().collect();
    if chars.len() <= max_chars {
        reason.to_string()
    } else {
        let truncated: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        format!("{}…", truncated)
    }
}

fn recommended_label(data: &HomeData) -> String {
    if let Some(seed) = data.continue_watching.first() {
        format!("󰛩 Because You Watched {}", truncate_reason(seed.short_title().as_str(), 18))
    } else {
        "󰛩 Because You Watched".to_string()
    }
}

/// All data needed to render the home screen.
pub struct HomeData {
    pub featured:          Option<Anime>,
    pub continue_watching: Vec<Anime>,
    pub watchlist:         Vec<Anime>,
    pub recommended:      Vec<Anime>,
    pub recommended_reasons: HashMap<i64, String>,
    pub progress_labels:  HashMap<i64, String>,
    pub resume_next:      HashMap<i64, u32>,
    pub trending:          Vec<Anime>,
    pub popular:           Vec<Anime>,
    pub top_rated:         Vec<Anime>,
    pub seasonal:          Vec<Anime>,
}

impl HomeData {
    pub fn empty() -> Self {
        Self {
            featured:          None,
            continue_watching: Vec::new(),
            watchlist:         Vec::new(),
            recommended:      Vec::new(),
            recommended_reasons: HashMap::new(),
            progress_labels:  HashMap::new(),
            resume_next:      HashMap::new(),
            trending:          Vec::new(),
            popular:           Vec::new(),
            top_rated:         Vec::new(),
            seasonal:          Vec::new(),
        }
    }
}
