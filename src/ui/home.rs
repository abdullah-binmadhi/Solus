//! Home screen — Cinematic Bento & Dynamic Shelf layout.
//! Renders: Top Glass Navigation Bar + 3-Zone Hero Stage + Smart Bento Shelf Grid + Footer.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use crate::{
    db::cache::Anime,
    services::tailored::clean_synopsis_single_line,
    state::{AppState, CategoryRow, Screen},
    ui::components::cover::HalfblockCover,
};

/// Width of each anime card in the row (chars)
pub const CARD_WIDTH: u16 = 22;
/// Height of each anime card (rows)
pub const CARD_HEIGHT: u16 = 10;
/// Gap between cards
pub const CARD_GAP: u16 = 2;
/// Height of the top glass navigation bar
pub const TOP_BAR_HEIGHT: u16 = 1;
/// Height of the widescreen hero stage
pub const HERO_HEIGHT: u16 = 10;
/// Default height of each shelf tier
#[allow(dead_code)]
pub const ROW_HEIGHT: u16 = 12;
/// Height of the 2-row bottom command dock (1 top divider + 2 balanced rows)
pub const FOOTER_HEIGHT: u16 = 3;

/// Interactive actions exposed by clicking pills in the Home Bottom Command Dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeFooterAction {
    Search,
    Calendar,
    Wizard,
    Analytics,
    Remote,
    Doctor,
    Theme,
    Login,
    StopPlayback,
    UpdateNow,
    OpenDetail,
    Resume,
    Watchlist,
    Remove,
    Refresh,
    Settings,
    Help,
    Quit,
}

/// A single vertical tier on the Home screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShelfTier {
    /// Side-by-side Bento dock combining Continue Watching (1-2 items) & My Watchlist.
    BentoSplit,
    /// Standard full-width horizontal shelf.
    Single(CategoryRow),
}

/// Returns true when Continue Watching (1-2 items) and Watchlist can dock side-by-side.
pub fn is_bento_split(data: &HomeData, width: u16) -> bool {
    !data.continue_watching.is_empty()
        && data.continue_watching.len() <= 2
        && !data.watchlist.is_empty()
        && width >= 96
}

/// Width of the left Bento container for 1-2 Continue Watching cards.
pub fn bento_left_width(cw_count: usize) -> u16 {
    let count = cw_count.clamp(1, 2) as u16;
    count * CARD_WIDTH + count.saturating_sub(1) * CARD_GAP + 4
}

/// Compute centered modal bounds matching overlays so underlying Home artwork doesn't bleed through.
fn active_modal_rect(frame_area: Rect, screen: &Screen) -> Option<Rect> {
    let (pct_x, pct_y) = match screen {
        Screen::UpdateModal => (64, 45),
        Screen::RemoteModal => (64, 76),
        Screen::CastModal => (68, 55),
        Screen::Calendar => {
            if frame_area.width >= 115 && frame_area.height >= 34 {
                (94, 90)
            } else {
                (98, 94)
            }
        }
        _ => return None,
    };
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(frame_area);
    Some(
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - pct_x) / 2),
                Constraint::Percentage(pct_x),
                Constraint::Percentage((100 - pct_x) / 2),
            ])
            .split(vert[1])[1],
    )
}

/// Build the ordered list of visible vertical tiers for the current terminal width.
pub fn build_shelf_tiers(data: &HomeData, width: u16) -> Vec<ShelfTier> {
    let mut tiers = Vec::new();
    let split = is_bento_split(data, width);

    if split {
        tiers.push(ShelfTier::BentoSplit);
    } else {
        if !data.continue_watching.is_empty() {
            tiers.push(ShelfTier::Single(CategoryRow::ContinueWatching));
        }
        if !data.watchlist.is_empty() {
            tiers.push(ShelfTier::Single(CategoryRow::Watchlist));
        }
    }

    if !data.trending.is_empty() {
        tiers.push(ShelfTier::Single(CategoryRow::Trending));
    }
    if !data.popular.is_empty() {
        tiers.push(ShelfTier::Single(CategoryRow::Popular));
    }
    if !data.top_rated.is_empty() {
        tiers.push(ShelfTier::Single(CategoryRow::TopRated));
    }
    if !data.seasonal.is_empty() {
        tiers.push(ShelfTier::Single(CategoryRow::Seasonal));
    }

    tiers
}

/// Compute `(visible_cards, card_width)` for a horizontal strip of `usable_w` columns
/// so cards expand/pack cleanly edge-to-edge without leaving ~15-20 col voids on the right.
pub fn compute_row_card_metrics(usable_w: u16) -> (usize, u16) {
    let min_card_w: u16 = 20;
    let max_card_w: u16 = 25;
    let gap = CARD_GAP;
    let vis_n = ((usable_w + gap) / (min_card_w + gap)).max(1) as usize;
    let gaps_total = (vis_n.saturating_sub(1)) as u16 * gap;
    let card_w = (usable_w.saturating_sub(gaps_total) / vis_n as u16).clamp(min_card_w, max_card_w);
    (vis_n, card_w)
}

/// Compute `(start_idx, row_heights)` so visible tiers pack cleanly into `avail_h`
/// without dropping an extra shelf to small 1–3 row boundary differences.
pub fn compute_shelf_layout(
    tiers: &[ShelfTier],
    avail_h: u16,
    active_tier_idx: usize,
) -> (usize, Vec<u16>) {
    if tiers.is_empty() || avail_h == 0 {
        return (0, Vec::new());
    }

    let min_tier_h: u16 = 10;
    let max_fit = (avail_h / min_tier_h).max(1) as usize;
    let visible_rows = tiers.len().min(max_fit);
    let start_idx = active_tier_idx.saturating_sub(visible_rows.saturating_sub(1));

    let visible_slice: Vec<&ShelfTier> = tiers.iter().skip(start_idx).take(visible_rows).collect();
    if visible_slice.len() == 1 {
        let cap = match visible_slice[0] {
            ShelfTier::BentoSplit => 12,
            ShelfTier::Single(_) => 12,
        };
        return (start_idx, vec![avail_h.min(cap)]);
    }

    let mut heights = vec![min_tier_h; visible_slice.len()];
    let mut rem = avail_h.saturating_sub(min_tier_h * visible_slice.len() as u16);

    // 1. Ensure BentoSplit gets +2 rows (12 total) first for full 10-row bordered cards
    for (idx, tier) in visible_slice.iter().enumerate() {
        if matches!(tier, ShelfTier::BentoSplit) {
            let add = rem.min(12u16.saturating_sub(heights[idx]));
            heights[idx] += add;
            rem -= add;
        }
    }

    // 2. Bring every Single shelf up to 11 rows (1 header + 10-row card, zero dead padding)
    for (idx, tier) in visible_slice.iter().enumerate() {
        if rem == 0 {
            break;
        }
        if matches!(tier, ShelfTier::Single(_)) && heights[idx] < 11 {
            heights[idx] += 1;
            rem -= 1;
        }
    }

    // 3. Distribute any remaining breathing rows (up to 12 per tier) evenly
    for h in &mut heights {
        if rem == 0 {
            break;
        }
        if *h < 12 {
            *h += 1;
            rem -= 1;
        }
    }

    (start_idx, heights)
}

/// Compute the starting index for a row viewport so scrolling to the end never leaves blank slots.
pub fn row_view_start(offset: usize, total: usize, visible_n: usize) -> usize {
    if visible_n == 0 {
        return 0;
    }
    offset.min(total.saturating_sub(visible_n))
}

/// Canonical metadata for a shelf row.
fn row_meta(row: &CategoryRow) -> (&'static str, &'static str, &'static str, &'static str) {
    match row {
        CategoryRow::ContinueWatching => ("continue_watching", " Resume", " Continue Watching", "RESUME"),
        CategoryRow::Watchlist => ("watchlist", "󰋜 Watchlist", "󰋜 My Watchlist", "WATCHLIST"),
        CategoryRow::Trending => ("trending", " Trending", " Trending Now", "TRENDING"),
        CategoryRow::Popular => ("popular", " Popular", " Popular Picks", "POPULAR"),
        CategoryRow::TopRated => ("top_rated", " Top Rated", " Top Rated", "TOP RATED"),
        CategoryRow::Seasonal => ("seasonal", "󰃭 Seasonal", "󰃭 Seasonal", "SEASONAL"),
    }
}

fn row_items<'a>(row: &CategoryRow, data: &'a HomeData) -> &'a [Anime] {
    match row {
        CategoryRow::ContinueWatching => &data.continue_watching,
        CategoryRow::Watchlist => &data.watchlist,
        CategoryRow::Trending => &data.trending,
        CategoryRow::Popular => &data.popular,
        CategoryRow::TopRated => &data.top_rated,
        CategoryRow::Seasonal => &data.seasonal,
    }
}

/// Non-empty category rows in canonical display order with their 1-based shortcut number.
pub fn top_bar_shelves(data: &HomeData) -> Vec<(usize, CategoryRow, &'static str)> {
    let all = [
        (1, CategoryRow::ContinueWatching, " Resume", !data.continue_watching.is_empty()),
        (2, CategoryRow::Watchlist, "󰋜 Watchlist", !data.watchlist.is_empty()),
        (3, CategoryRow::Trending, " Trending", !data.trending.is_empty()),
        (4, CategoryRow::Popular, " Popular", !data.popular.is_empty()),
        (5, CategoryRow::TopRated, " Top Rated", !data.top_rated.is_empty()),
        (6, CategoryRow::Seasonal, "󰃭 Seasonal", !data.seasonal.is_empty()),
    ];
    all.into_iter()
        .filter(|(_, _, _, non_empty)| *non_empty)
        .map(|(num, row, label, _)| (num, row, label))
        .collect()
}

/// Hit-test a click on the top navigation bar (row 0) to jump directly to a shelf.
pub fn top_bar_shelf_at(data: &HomeData, width: u16, mx: u16) -> Option<CategoryRow> {
    let mut x: u16 = 11; // "  SOLUS " (9) + "  " (2)
    for (num, row, label) in top_bar_shelves(data) {
        let pill_text = format!(" {}:{} ", num, label);
        let pill_w = pill_text.chars().count() as u16;
        if x + pill_w + 18 >= width {
            break;
        }
        if mx >= x && mx < x + pill_w {
            return Some(row);
        }
        x += pill_w + 1;
    }
    None
}

/// Render the full home screen.
pub fn render(frame: &mut Frame, state: &mut AppState, categories: &HomeData) {
    let area = frame.area();
    let banner_anime = active_banner_anime(state, categories).or(categories.featured.as_ref());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(TOP_BAR_HEIGHT), // Glass navigation & shelf bar
            Constraint::Length(HERO_HEIGHT),    // 3-Zone Widescreen Hero Stage
            Constraint::Min(0),                 // Smart Bento Shelf Grid
            Constraint::Length(FOOTER_HEIGHT),  // 2-Row Balanced Command Dock
        ])
        .split(area);

    render_top_bar(frame, chunks[0], state, categories);
    render_featured(frame, chunks[1], state, banner_anime, categories);
    render_rows(frame, chunks[2], state, categories);
    render_footer(frame, chunks[3], state);
}

/// Top 1-line Glass Navigation Bar with brand badge, interactive shelf pills, and telemetry.
fn render_top_bar(frame: &mut Frame, area: Rect, state: &AppState, data: &HomeData) {
    let theme = state.theme();
    let mut spans = vec![
        Span::styled(
            "  SOLUS ",
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
    ];

    let mut used_width: u16 = 11;

    // Reserve space for right-side status badges
    let user_badge = if state.auth.is_authenticated() {
        format!("󰄬 {}", state.auth.username.as_deref().unwrap_or("AniList"))
    } else {
        "󰍂 Guest".to_string()
    };
    let right_text = format!("{}  ·  ◈ {} ", user_badge, theme.name);
    let right_width = right_text.chars().count() as u16 + 2;

    for (num, row, label) in top_bar_shelves(data) {
        let is_active = state.active_row == row;
        let pill_text = format!(" {}:{} ", num, label);
        let pill_w = pill_text.chars().count() as u16;
        if used_width + pill_w + right_width >= area.width {
            break;
        }
        let style = if is_active {
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.text_dim)
                .bg(theme.border_idle)
        };
        spans.push(Span::styled(pill_text, style));
        spans.push(Span::raw(" "));
        used_width += pill_w + 1;
    }

    let remaining = area.width.saturating_sub(used_width + right_width) as usize;
    if remaining > 0 {
        spans.push(Span::raw(" ".repeat(remaining)));
    }

    if area.width >= used_width + right_width {
        let auth_color = if state.auth.is_authenticated() {
            Color::Rgb(80, 250, 123)
        } else {
            theme.text_dim
        };
        spans.push(Span::styled(
            user_badge,
            Style::default().fg(auth_color).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("  ·  ", Style::default().fg(theme.divider)));
        spans.push(Span::styled(
            format!("◈ {} ", theme.name),
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[derive(Clone)]
struct DockChip {
    icon: &'static str,
    key: &'static str,
    label: String,
    key_color: Color,
    label_color: Color,
    priority: u8,
    bg: Option<Color>,
    action: Option<HomeFooterAction>,
}

impl DockChip {
    fn width(&self) -> usize {
        let icon_w = if self.icon.is_empty() {
            0
        } else {
            self.icon.chars().count() + 1
        };
        let key_w = self.key.chars().count();
        let lbl_w = if self.label.is_empty() {
            0
        } else {
            1 + self.label.chars().count()
        };
        let pad = if self.bg.is_some() { 2 } else { 0 };
        icon_w + key_w + lbl_w + pad
    }

    fn push_spans(&self, spans: &mut Vec<Span<'static>>) {
        if let Some(bg_col) = self.bg {
            let mut text = String::from(" ");
            if !self.icon.is_empty() {
                text.push_str(self.icon);
                text.push(' ');
            }
            text.push_str(self.key);
            if !self.label.is_empty() {
                text.push(' ');
                text.push_str(&self.label);
            }
            text.push(' ');
            spans.push(Span::styled(
                text,
                Style::default()
                    .fg(self.key_color)
                    .bg(bg_col)
                    .add_modifier(Modifier::BOLD),
            ));
            return;
        }

        if !self.icon.is_empty() {
            spans.push(Span::styled(
                format!("{} ", self.icon),
                Style::default().fg(self.key_color),
            ));
        }
        spans.push(Span::styled(
            self.key.to_string(),
            Style::default().fg(self.key_color).add_modifier(Modifier::BOLD),
        ));
        if !self.label.is_empty() {
            spans.push(Span::styled(
                format!(" {}", self.label),
                Style::default().fg(self.label_color),
            ));
        }
    }
}

struct DockRowPlan {
    line: Line<'static>,
    hitboxes: Vec<(u16, u16, HomeFooterAction)>,
}

fn build_dock_row_plan(
    badge_text: &str,
    badge_fg: Color,
    badge_bg: Color,
    left_candidates: Vec<DockChip>,
    right_candidates: Vec<DockChip>,
    width: usize,
) -> DockRowPlan {
    let badge_w = badge_text.chars().count();
    let gutter_w = if width >= 116 { 2usize } else { 1usize };
    let gutter_str = if gutter_w == 2 { "  " } else { " " };
    let trailing_w = 1usize;

    // 1. Fit right-pinned cluster first (ordered highest priority last or explicit keep order)
    let mut right_chosen = select_chips_by_priority(&right_candidates, width.saturating_sub(badge_w + 4), gutter_w);
    if right_chosen.is_empty() && !right_candidates.is_empty() {
        // Guarantee at least the highest-priority rightmost control if very narrow
        if let Some(top) = right_candidates.iter().min_by_key(|c| c.priority) {
            if badge_w + 3 + top.width() + trailing_w <= width {
                right_chosen.push(top.clone());
            }
        }
    }

    let right_w: usize = if right_chosen.is_empty() {
        0
    } else {
        right_chosen.iter().map(DockChip::width).sum::<usize>()
            + gutter_w * right_chosen.len().saturating_sub(1)
            + trailing_w
    };

    // 2. Fit left cluster strictly inside remaining budget so zero right-edge clipping can occur
    let badge_gap = if width >= badge_w + 2 { 2usize } else { 0usize };
    let min_center_gap = if right_w > 0 { gutter_w.max(2) } else { 0usize };
    let left_budget = width.saturating_sub(badge_w + badge_gap + min_center_gap + right_w);
    let left_chosen = select_chips_by_priority(&left_candidates, left_budget, gutter_w);

    let left_w: usize = if left_chosen.is_empty() {
        0
    } else {
        left_chosen.iter().map(DockChip::width).sum::<usize>()
            + gutter_w * left_chosen.len().saturating_sub(1)
    };

    let center_gap = width.saturating_sub(badge_w + badge_gap + left_w + right_w);

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut hitboxes: Vec<(u16, u16, HomeFooterAction)> = Vec::new();
    let mut cursor_x = 0usize;

    spans.push(Span::styled(
        badge_text.to_string(),
        Style::default()
            .fg(badge_fg)
            .bg(badge_bg)
            .add_modifier(Modifier::BOLD),
    ));
    cursor_x += badge_w;

    if badge_gap > 0 {
        spans.push(Span::raw("  "));
        cursor_x += badge_gap;
    }

    for (idx, chip) in left_chosen.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw(gutter_str));
            cursor_x += gutter_w;
        }
        let w = chip.width();
        if let Some(act) = chip.action {
            hitboxes.push((cursor_x as u16, (cursor_x + w) as u16, act));
        }
        chip.push_spans(&mut spans);
        cursor_x += w;
    }

    if center_gap > 0 {
        spans.push(Span::raw(" ".repeat(center_gap)));
        cursor_x += center_gap;
    }

    for (idx, chip) in right_chosen.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw(gutter_str));
            cursor_x += gutter_w;
        }
        let w = chip.width();
        if let Some(act) = chip.action {
            hitboxes.push((cursor_x as u16, (cursor_x + w) as u16, act));
        }
        chip.push_spans(&mut spans);
        cursor_x += w;
    }

    if !right_chosen.is_empty() {
        spans.push(Span::raw(" "));
    }

    DockRowPlan {
        line: Line::from(spans),
        hitboxes,
    }
}

/// Select highest-priority chips (`priority` 0 first) that fit cleanly inside `budget`, preserving display order.
fn select_chips_by_priority(candidates: &[DockChip], budget: usize, sep_w: usize) -> Vec<DockChip> {
    if budget == 0 || candidates.is_empty() {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by_key(|&i| (candidates[i].priority, i));

    let mut chosen = vec![false; candidates.len()];
    let mut used = 0usize;
    let mut count = 0usize;

    for idx in order {
        let extra = candidates[idx].width() + if count > 0 { sep_w } else { 0 };
        if used + extra <= budget {
            chosen[idx] = true;
            used += extra;
            count += 1;
        }
    }

    candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| if chosen[i] { Some(c.clone()) } else { None })
        .collect()
}

fn build_footer_dock_rows(state: &AppState, width: usize) -> (DockRowPlan, DockRowPlan) {
    let theme = state.theme();
    let emerald = Color::Rgb(80, 250, 123);

    // ── Row 1: Discovery & System Features + Account / Theme / Live Status ─────
    let mut explore_left = vec![
        DockChip {
            icon: "",
            key: "[/]",
            label: "Search".to_string(),
            key_color: theme.primary,
            label_color: theme.text,
            priority: 0,
            bg: None,
            action: Some(HomeFooterAction::Search),
        },
        DockChip {
            icon: "󰃭",
            key: "[c]",
            label: "Calendar".to_string(),
            key_color: emerald,
            label_color: theme.text,
            priority: 1,
            bg: None,
            action: Some(HomeFooterAction::Calendar),
        },
        DockChip {
            icon: "󰋽",
            key: "[w]",
            label: "Wizard".to_string(),
            key_color: theme.accent,
            label_color: theme.text,
            priority: 2,
            bg: None,
            action: Some(HomeFooterAction::Wizard),
        },
        DockChip {
            icon: "",
            key: "[a]",
            label: if width >= 156 { "Analytics" } else { "Stats" }.to_string(),
            key_color: theme.secondary,
            label_color: theme.text,
            priority: 4,
            bg: None,
            action: Some(HomeFooterAction::Analytics),
        },
        DockChip {
            icon: "◎",
            key: "[m]",
            label: "Remote".to_string(),
            key_color: theme.secondary,
            label_color: theme.text,
            priority: 5,
            bg: None,
            action: Some(HomeFooterAction::Remote),
        },
        DockChip {
            icon: "",
            key: "[!]",
            label: "Health".to_string(),
            key_color: theme.text_dim,
            label_color: theme.text_dim,
            priority: 6,
            bg: None,
            action: Some(HomeFooterAction::Doctor),
        },
    ];

    // If room permits cleanly, include Update chip inside Row 1 status or Remote right cluster
    let mut explore_right: Vec<DockChip> = Vec::new();
    if let Some(ref track) = state.now_playing {
        let short_track: String = track.chars().take(18).collect();
        explore_right.push(DockChip {
            icon: "󰐊",
            key: "[X]",
            label: format!("Stop ({})", short_track),
            key_color: Color::Black,
            label_color: Color::Black,
            priority: 1,
            bg: Some(theme.accent),
            action: Some(HomeFooterAction::StopPlayback),
        });
    } else if let Some((_, ep, pct, ref _spd)) = state.active_download {
        explore_right.push(DockChip {
            icon: "󰇚",
            key: "",
            label: format!("Ep {}: {:.0}%", ep, pct),
            key_color: Color::Rgb(139, 233, 253),
            label_color: Color::Rgb(139, 233, 253),
            priority: 1,
            bg: None,
            action: None,
        });
    } else if let Some((pct, ref _spd)) = state.update_progress {
        explore_right.push(DockChip {
            icon: "󰚰",
            key: "",
            label: format!("Updating {:.0}%", pct),
            key_color: emerald,
            label_color: emerald,
            priority: 1,
            bg: None,
            action: None,
        });
    } else if let Some(ref update) = state.available_update {
        explore_right.push(DockChip {
            icon: "󰚰",
            key: "[U]",
            label: format!("Update ({})", update.latest_version),
            key_color: Color::Black,
            label_color: Color::Black,
            priority: 1,
            bg: Some(emerald),
            action: Some(HomeFooterAction::UpdateNow),
        });
    }

    explore_right.push(DockChip {
        icon: "◈",
        key: "[T]",
        label: theme.name.to_string(),
        key_color: theme.secondary,
        label_color: theme.text,
        priority: 3,
        bg: None,
        action: Some(HomeFooterAction::Theme),
    });
    explore_right.push(DockChip {
        icon: "󰄬",
        key: "[L]",
        label: if state.auth.is_authenticated() {
            state.auth.username.as_deref().unwrap_or("AniList").to_string()
        } else {
            "Login".to_string()
        },
        key_color: if state.auth.is_authenticated() { emerald } else { theme.primary },
        label_color: if state.auth.is_authenticated() { emerald } else { theme.text },
        priority: 2,
        bg: None,
        action: Some(HomeFooterAction::Login),
    });

    // If nothing dynamic is playing/downloading/updating and width is narrow, shrink Priority 6 quietly
    let _ = &mut explore_left;

    let row1 = build_dock_row_plan(
        " EXPLORE ",
        theme.primary,
        theme.border_idle,
        explore_left,
        explore_right,
        width,
    );

    // ── Row 2: Card & Navigation Controls + Settings / Help / Quit ─────────────
    let mut actions_left = vec![
        DockChip {
            icon: "▸",
            key: "[hjkl]",
            label: "Move".to_string(),
            key_color: theme.secondary,
            label_color: theme.text_dim,
            priority: 3,
            bg: None,
            action: None,
        },
        DockChip {
            icon: "",
            key: "[1-6]",
            label: "Shelf".to_string(),
            key_color: theme.secondary,
            label_color: theme.text_dim,
            priority: 6,
            bg: None,
            action: None,
        },
        DockChip {
            icon: "󰘳",
            key: "[Enter]",
            label: "Watch".to_string(),
            key_color: Color::White,
            label_color: theme.text,
            priority: 0,
            bg: None,
            action: Some(HomeFooterAction::OpenDetail),
        },
        DockChip {
            icon: "",
            key: "[r]",
            label: "Resume".to_string(),
            key_color: theme.primary,
            label_color: theme.text,
            priority: 1,
            bg: None,
            action: Some(HomeFooterAction::Resume),
        },
        DockChip {
            icon: "󰋜",
            key: "[+]",
            label: "Watchlist".to_string(),
            key_color: theme.accent,
            label_color: theme.text,
            priority: 2,
            bg: None,
            action: Some(HomeFooterAction::Watchlist),
        },
    ];

    if state.active_row == CategoryRow::ContinueWatching || state.active_row == CategoryRow::Watchlist {
        actions_left.push(DockChip {
            icon: "󰅖",
            key: "[x]",
            label: "Remove".to_string(),
            key_color: Color::Rgb(255, 85, 85),
            label_color: Color::Rgb(255, 130, 130),
            priority: 4,
            bg: None,
            action: Some(HomeFooterAction::Remove),
        });
    }

    actions_left.push(DockChip {
        icon: "↺",
        key: "[R]",
        label: "Refresh".to_string(),
        key_color: theme.text_dim,
        label_color: theme.text_dim,
        priority: 5,
        bg: None,
        action: Some(HomeFooterAction::Refresh),
    });

    let actions_right = vec![
        DockChip {
            icon: "󰒓",
            key: "[s]",
            label: "Settings".to_string(),
            key_color: theme.primary,
            label_color: theme.text,
            priority: 2,
            bg: None,
            action: Some(HomeFooterAction::Settings),
        },
        DockChip {
            icon: "",
            key: "[?]",
            label: "Help".to_string(),
            key_color: theme.accent,
            label_color: theme.text,
            priority: 0,
            bg: None,
            action: Some(HomeFooterAction::Help),
        },
        DockChip {
            icon: "󰈆",
            key: "[q]",
            label: "Quit".to_string(),
            key_color: theme.text_dim,
            label_color: theme.text_dim,
            priority: 1,
            bg: None,
            action: Some(HomeFooterAction::Quit),
        },
    ];

    let row2 = build_dock_row_plan(
        " ACTIONS ",
        theme.secondary,
        theme.border_idle,
        actions_left,
        actions_right,
        width,
    );

    (row1, row2)
}

/// Hit-test a left-click inside the 2-row Home Command Dock (`FOOTER_HEIGHT = 3`).
pub fn footer_action_at(state: &AppState, width: u16, height: u16, mx: u16, my: u16) -> Option<HomeFooterAction> {
    let dock_top = height.saturating_sub(FOOTER_HEIGHT);
    if my <= dock_top || my >= height {
        return None;
    }
    let (row1, row2) = build_footer_dock_rows(state, width as usize);
    let target = if my == dock_top + 1 { &row1 } else { &row2 };
    target
        .hitboxes
        .iter()
        .find(|(x0, x1, _)| mx >= *x0 && mx < *x1)
        .map(|(_, _, act)| *act)
}

fn render_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.divider));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (row1, row2) = build_footer_dock_rows(state, inner.width as usize);
    frame.render_widget(Paragraph::new(vec![row1.line, row2.line]), inner);
}

/// 3-Zone Widescreen Hero Stage at the top: Cover Frame + Editorial Info + Telemetry HUD.
fn render_featured(frame: &mut Frame, area: Rect, state: &mut AppState, anime: Option<&Anime>, data: &HomeData) {
    let theme = state.theme();

    let banner = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(
            "  FEATURED SPOTLIGHT ",
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
        frame.render_widget(Paragraph::new(msg).wrap(ratatui::widgets::Wrap { trim: true }), inner);
        return;
    };

    let show_hud = inner.width >= 104;
    let cols = if show_hud {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(26), // Cinema 16:9 Cover Frame
                Constraint::Length(2),  // Breathing spacer
                Constraint::Min(0),     // Editorial Metadata & CTA Deck
                Constraint::Length(26), // Right Telemetry HUD
            ])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(24), // Cover Frame
                Constraint::Length(2),  // Breathing spacer
                Constraint::Min(0),     // Editorial Metadata & CTA Deck
            ])
            .split(inner)
    };

    // ── Zone 1: Cinema Cover Frame ───────────────────────────────────────────
    let cover_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle));
    let cover_inner = cover_block.inner(cols[0]);
    frame.render_widget(cover_block, cols[0]);

    let cover_occluded = active_modal_rect(frame.area(), &state.screen)
        .is_some_and(|m| m.intersects(cover_inner));
    if state.has_image_support() && !cover_occluded {
        if let Some(cover) = state
            .anime_banners
            .get_mut(&anime.id)
            .or_else(|| state.banner_cache.get_mut(&anime.id))
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

    // ── Shared Metadata Preparation ──────────────────────────────────────────
    let (_, _, _, badge_word) = row_meta(&state.active_row);
    let active_idx = {
        let (key, _, _, _) = row_meta(&state.active_row);
        state.row_offset(key) + 1
    };
    let focus_chip = match state.active_row {
        CategoryRow::Trending | CategoryRow::TopRated => format!(" {} #{} ", badge_word, active_idx),
        _ => format!(" {} ", badge_word),
    };

    let fmt = anime.format.as_deref().unwrap_or("TV");
    let year = anime.season_year.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string());
    let eps = anime.episodes.map(|e| format!("{} EPS", e)).unwrap_or_else(|| "? EPS".to_string());
    let status = anime.status.as_deref().unwrap_or("UNKNOWN");
    let title = anime.display_title();
    let native = anime.title_native.as_deref().unwrap_or("");

    let watched = state
        .banner_progress
        .filter(|(anime_id, _)| *anime_id == anime.id)
        .map(|(_, watched)| watched)
        .unwrap_or(0);
    let in_watchlist = data.watchlist.iter().any(|item| item.id == anime.id);

    let play_label = if let Some(next_ep) = data.resume_next.get(&anime.id) {
        format!("  Continue E{} ", next_ep)
    } else {
        "  Start E1 ".to_string()
    };
    let watchlist_label = if in_watchlist {
        " 󰄬 In Watchlist "
    } else {
        " + Watchlist "
    };

    // ── Zone 2: Editorial Metadata (Strictly 8 lines — zero clipping) ─────────
    let center_w = cols[2].width as usize;

    // Row 0: Focus Pill + Specs
    let mut row0_spans = vec![
        Span::styled(
            focus_chip,
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::default()),
        Span::styled(format!("{} {}", fmt, year), Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        Span::styled("  ·  ", Style::default().fg(theme.divider)),
        Span::styled(eps, Style::default().fg(theme.text)),
        Span::styled("  ·  ", Style::default().fg(theme.divider)),
        Span::styled(status, Style::default().fg(theme.text_dim)),
    ];
    if !show_hud {
        if let Some(s) = anime.score {
            row0_spans.push(Span::styled("  ·  ", Style::default().fg(theme.divider)));
            row0_spans.push(Span::styled(
                format!(" {:.1}", s as f32 / 10.0),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ));
        }
    }

    // Row 1: Title + Native Subtitle (single line)
    let title_budget = center_w.saturating_sub(2);
    let mut title_spans = Vec::new();
    if !native.is_empty() && title.chars().count() + native.chars().count() + 5 <= title_budget {
        title_spans.push(Span::styled(
            title,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        title_spans.push(Span::styled("  ·  ", Style::default().fg(theme.divider)));
        title_spans.push(Span::styled(
            native,
            Style::default().fg(theme.text_dim).add_modifier(Modifier::ITALIC),
        ));
    } else {
        title_spans.push(Span::styled(
            truncate_chars(title, title_budget),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
    }

    // Row 2: Curated Genre Pills (max 4, guaranteed single line)
    let mut genre_spans = Vec::new();
    let mut g_used = 0usize;
    for genre in anime.genre_list().into_iter().take(4) {
        let chip = format!(" {} ", genre);
        let chip_len = chip.chars().count() + 1;
        if g_used + chip_len > center_w {
            break;
        }
        genre_spans.push(Span::styled(
            chip,
            Style::default().fg(theme.accent).bg(theme.border_idle),
        ));
        genre_spans.push(Span::raw(" "));
        g_used += chip_len;
    }
    if genre_spans.is_empty() {
        genre_spans.push(Span::styled(" Anime ", Style::default().fg(theme.text_dim).bg(theme.border_idle)));
    }

    // Rows 4-5: Strictly 2-line word-wrapped synopsis
    let raw_desc = anime
        .description
        .as_deref()
        .unwrap_or("No description available.");
    let cleaned_desc = clean_synopsis_single_line(raw_desc);
    let (syn_line1, syn_line2) = wrap_two_lines(&cleaned_desc, center_w.saturating_sub(1));

    // Row 6: Optional external player launch status OR breathing spacer
    let launch_status = state
        .last_played
        .as_deref()
        .filter(|_| state.last_played_anime_id == Some(anime.id));
    let row6 = if let Some(status_msg) = launch_status {
        Line::from(vec![
            Span::styled(
                " 󰐊 Player Active ",
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                truncate_chars(status_msg, center_w.saturating_sub(20)),
                Style::default().fg(theme.text_dim),
            ),
        ])
    } else {
        Line::from("")
    };

    // Row 7: Tactile Action Deck (Always Visible)
    let row7 = Line::from(vec![
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
                .fg(Color::Black)
                .bg(if in_watchlist { theme.secondary } else { theme.accent })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            " 󰘳 [d] Details ",
            Style::default().fg(theme.text).bg(theme.divider),
        ),
        Span::raw("  "),
        Span::styled(
            "  [r] Resume ",
            Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            " 󰋽 [w] Wizard ",
            Style::default().fg(theme.accent).bg(theme.border_idle),
        ),
    ]);

    let center_lines = vec![
        Line::from(row0_spans),
        Line::from(title_spans),
        Line::from(genre_spans),
        Line::from(""),
        Line::from(Span::styled(syn_line1, Style::default().fg(theme.text_dim))),
        Line::from(Span::styled(syn_line2, Style::default().fg(theme.text_dim))),
        row6,
        row7,
    ];
    frame.render_widget(Paragraph::new(center_lines), cols[2]);

    // ── Zone 3: Right Telemetry HUD ──────────────────────────────────────────
    if show_hud {
        let hud_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme.divider));
        let hud_inner = hud_block.inner(cols[3]);
        frame.render_widget(hud_block, cols[3]);

        let score_val = anime.score.map(|s| s as f32 / 10.0);
        let score_text = score_val
            .map(|v| format!(" {:.1} / 10", v))
            .unwrap_or_else(|| " N/A".to_string());
        let score_fill = score_val.map(|v| ((v / 10.0) * 10.0).round() as usize).unwrap_or(0).min(10);
        let score_tier = match anime.score.unwrap_or(0) {
            85..=100 => "Acclaimed",
            75..=84 => "Top Pick",
            60..=74 => "Solid",
            1..=59 => "Mixed",
            _ => "Unrated",
        };

        let total_eps = anime.episodes.unwrap_or(0).max(0) as usize;
        let prog_text = if total_eps > 0 {
            format!("{} / {} EPS", watched, total_eps)
        } else {
            format!("{} WATCHED", watched)
        };
        let (prog_fill, prog_pct_label) = if total_eps > 0 {
            let pct = ((watched as f32 / total_eps as f32).clamp(0.0, 1.0) * 100.0).round() as usize;
            let fill = ((watched as f32 / total_eps as f32).clamp(0.0, 1.0) * 10.0).round() as usize;
            (fill.min(10), format!("{}%", pct))
        } else if watched > 0 {
            (10, "Active".to_string())
        } else {
            (0, "Unwatched".to_string())
        };

        let audio_tag = if anime.has_dub() { "SUB / DUB" } else { "SUB ONLY" };
        let shader_tag = match state.anime4k_mode {
            crate::config::Anime4KMode::Off => None,
            crate::config::Anime4KMode::Balanced => Some("◈ A4K 2x"),
            crate::config::Anime4KMode::Ultra => Some("◈ A4K 4K"),
        };

        let mut audio_line = vec![
            Span::styled("  AUDIO   ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!(" {} ", audio_tag),
                Style::default().fg(theme.text).bg(theme.border_idle).add_modifier(Modifier::BOLD),
            ),
        ];
        if let Some(sh) = shader_tag {
            audio_line.push(Span::raw(" "));
            audio_line.push(Span::styled(sh, Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)));
        }

        let hud_lines = vec![
            Line::from(Span::styled(
                "  TELEMETRY",
                Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled("  SCORE   ", Style::default().fg(theme.text_dim)),
                Span::styled(score_text, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("█".repeat(score_fill), Style::default().fg(theme.accent)),
                Span::styled("░".repeat(10usize.saturating_sub(score_fill)), Style::default().fg(theme.divider)),
                Span::styled(format!("  {}", score_tier), Style::default().fg(theme.text_dim)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  WATCH   ", Style::default().fg(theme.text_dim)),
                Span::styled(prog_text, Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("█".repeat(prog_fill), Style::default().fg(theme.primary)),
                Span::styled("░".repeat(10usize.saturating_sub(prog_fill)), Style::default().fg(theme.divider)),
                Span::styled(format!("  {}", prog_pct_label), Style::default().fg(theme.text_dim)),
            ]),
            Line::from(""),
            Line::from(audio_line),
        ];
        frame.render_widget(Paragraph::new(hud_lines), hud_inner);
    }
}

fn active_banner_anime<'a>(state: &AppState, data: &'a HomeData) -> Option<&'a Anime> {
    let (row_key, _, _, _) = row_meta(&state.active_row);
    let items = row_items(&state.active_row, data);
    items.get(state.row_offset(row_key))
}

/// Render all visible category tiers (with Smart Bento Split for Continue Watching + Watchlist).
fn render_rows(frame: &mut Frame, area: Rect, state: &mut AppState, data: &HomeData) {
    let tiers = build_shelf_tiers(data, area.width);
    if tiers.is_empty() {
        return;
    }

    let active_tier_idx = tiers
        .iter()
        .position(|tier| match tier {
            ShelfTier::BentoSplit => {
                matches!(state.active_row, CategoryRow::ContinueWatching | CategoryRow::Watchlist)
            }
            ShelfTier::Single(row) => *row == state.active_row,
        })
        .unwrap_or(0);

    let (start_idx, heights) = compute_shelf_layout(&tiers, area.height, active_tier_idx);
    let constraints: Vec<Constraint> = heights.iter().map(|&h| Constraint::Length(h)).collect();

    let tier_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, tier) in tiers.iter().skip(start_idx).take(heights.len()).enumerate() {
        if i >= tier_areas.len() {
            break;
        }
        match tier {
            ShelfTier::BentoSplit => render_bento_split(frame, tier_areas[i], state, data),
            ShelfTier::Single(row) => render_single_row(frame, tier_areas[i], state, row, data),
        }
    }
}

/// Render Continue Watching (1-2 cards) docked side-by-side with My Watchlist in one Bento tier.
fn render_bento_split(frame: &mut Frame, area: Rect, state: &mut AppState, data: &HomeData) {
    let theme = state.theme();
    let left_w = bento_left_width(data.continue_watching.len());

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(left_w),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    // ── Left Bento Module: Continue Watching ─────────────────────────────────
    let cw_active = state.active_row == CategoryRow::ContinueWatching;
    let cw_border = if cw_active { theme.primary } else { theme.border_idle };
    let cw_title_style = if cw_active {
        Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD)
    };
    let cw_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(cw_border))
        .title(Span::styled(
            format!("  Continue Watching ({}) ", data.continue_watching.len()),
            cw_title_style,
        ));
    let cw_inner = cw_block.inner(cols[0]);
    frame.render_widget(cw_block, cols[0]);

    let cw_offset = state.row_offset("continue_watching");
    let cw_vis_n = data.continue_watching.len().min(2);
    let cw_start = row_view_start(cw_offset, data.continue_watching.len(), cw_vis_n);
    for (i, anime) in data.continue_watching.iter().skip(cw_start).take(cw_vis_n).enumerate() {
        let x = cw_inner.x + 1 + i as u16 * (CARD_WIDTH + CARD_GAP);
        let rect = Rect {
            x,
            y: cw_inner.y,
            width: CARD_WIDTH,
            height: CARD_HEIGHT.min(cw_inner.height),
        };
        if rect.x + rect.width <= cw_inner.x + cw_inner.width {
            let progress = data.progress_labels.get(&anime.id).map(String::as_str);
            let selected = cw_active && (cw_start + i == cw_offset);
            render_card(frame, rect, anime, selected, None, progress, state);
        }
    }

    // ── Right Bento Module: My Watchlist ─────────────────────────────────────
    let wl_active = state.active_row == CategoryRow::Watchlist;
    let wl_border = if wl_active { theme.primary } else { theme.border_idle };
    let wl_title_style = if wl_active {
        Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD)
    };
    let wl_offset = state.row_offset("watchlist");
    let wl_dots = format_pagination_dots(wl_offset, data.watchlist.len());
    let wl_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(wl_border))
        .title(Span::styled(
            format!(" 󰋜 My Watchlist ({}) ", data.watchlist.len()),
            wl_title_style,
        ))
        .title(
            Line::from(vec![
                Span::styled(wl_dots, Style::default().fg(if wl_active { theme.primary } else { theme.text_dim })),
                Span::styled(
                    format!(" [ {}/{} ] ", (wl_offset + 1).min(data.watchlist.len()), data.watchlist.len()),
                    Style::default().fg(theme.text_dim),
                ),
            ])
            .alignment(Alignment::Right),
        );
    let wl_inner = wl_block.inner(cols[2]);
    frame.render_widget(wl_block, cols[2]);

    let wl_usable_w = wl_inner.width.saturating_sub(2);
    let (wl_vis_n, wl_card_w) = compute_row_card_metrics(wl_usable_w);
    let wl_start = row_view_start(wl_offset, data.watchlist.len(), wl_vis_n);
    for (i, anime) in data.watchlist.iter().skip(wl_start).take(wl_vis_n).enumerate() {
        let x = wl_inner.x + 1 + i as u16 * (wl_card_w + CARD_GAP);
        let rect = Rect {
            x,
            y: wl_inner.y,
            width: wl_card_w,
            height: CARD_HEIGHT.min(wl_inner.height),
        };
        if rect.x + rect.width <= wl_inner.x + wl_inner.width {
            let progress = data.progress_labels.get(&anime.id).map(String::as_str);
            let selected = wl_active && (wl_start + i == wl_offset);
            render_card(frame, rect, anime, selected, None, progress, state);
        }
    }
}

/// Render a full-width horizontal category shelf with active section rail, pagination dots, and cards.
fn render_single_row(frame: &mut Frame, area: Rect, state: &mut AppState, row: &CategoryRow, data: &HomeData) {
    let theme = state.theme();
    let (key, _, label, _) = row_meta(row);
    let items = row_items(row, data);
    let is_active = state.active_row == *row;
    let ranked = matches!(row, CategoryRow::Trending | CategoryRow::TopRated);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let offset = state.row_offset(key);
    let dots = format_pagination_dots(offset, items.len());

    let rail_color = if is_active { theme.primary } else { theme.divider };
    let label_style = if is_active {
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD)
    };

    let title_prefix = format!("── {} ({}) ", label, items.len());
    let title_suffix = format!(" {} [ {}/{} ] ──", dots, (offset + 1).min(items.len()), items.len());
    let sep_len = (area.width as usize)
        .saturating_sub(title_prefix.chars().count() + title_suffix.chars().count() + 1);

    let title_line = Line::from(vec![
        Span::styled(title_prefix, label_style),
        Span::styled("─".repeat(sep_len), Style::default().fg(rail_color)),
        Span::styled(
            format!(" {} ", dots),
            Style::default().fg(if is_active { theme.primary } else { theme.divider }),
        ),
        Span::styled(
            format!("[ {}/{} ] ──", (offset + 1).min(items.len()), items.len()),
            Style::default().fg(theme.text_dim),
        ),
    ]);
    frame.render_widget(Paragraph::new(title_line), chunks[0]);

    let card_area = chunks[1];
    let (visible_n, card_w) = compute_row_card_metrics(card_area.width);
    let view_start = row_view_start(offset, items.len(), visible_n);

    for (i, anime) in items.iter().skip(view_start).take(visible_n).enumerate() {
        let x = card_area.x + i as u16 * (card_w + CARD_GAP);
        let rect = Rect {
            x,
            y: card_area.y,
            width: card_w,
            height: CARD_HEIGHT.min(card_area.height),
        };
        if rect.x + rect.width <= card_area.x + card_area.width {
            let progress = data.progress_labels.get(&anime.id).map(String::as_str);
            let rank = if ranked { Some(view_start + i + 1) } else { None };
            let selected = is_active && (view_start + i == offset);
            render_card(frame, rect, anime, selected, rank, progress, state);
        }
    }
}

/// Render a single anime card (cover + title + rich metadata/progress bar).
fn render_card(
    frame: &mut Frame,
    area: Rect,
    anime: &Anime,
    selected: bool,
    rank: Option<usize>,
    progress: Option<&str>,
    state: &mut AppState,
) {
    if area.height < 4 {
        return;
    }

    let theme = state.theme();

    let border_color = if selected {
        theme.border_focus
    } else {
        theme.border_idle
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    if selected {
        let badge = match rank {
            Some(r) => format!("  #{} · FOCUS ", r),
            None => "  FOCUS ".to_string(),
        };
        block = block.title(Span::styled(
            badge,
            Style::default()
                .fg(Color::Black)
                .bg(theme.border_focus)
                .add_modifier(Modifier::BOLD),
        ));
    } else if let Some(r) = rank {
        block = block.title(Span::styled(
            format!(" #{} ", r),
            Style::default()
                .fg(if r <= 3 { theme.accent } else { theme.text_dim })
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
            Constraint::Length(1), // score / progress bar
        ])
        .split(inner);

    let cover_occluded = active_modal_rect(frame.area(), &state.screen)
        .is_some_and(|m| m.intersects(chunks[0]));
    if state.has_image_support() && !cover_occluded {
        if let Some(cover) = state.cover_cache.get_mut(&anime.id) {
            let image_widget = ratatui_image::StatefulImage::new(None)
                .resize(ratatui_image::Resize::Fit(None));
            frame.render_stateful_widget(image_widget, chunks[0], cover);
        } else {
            frame.render_widget(
                HalfblockCover {
                    anime_id: anime.id,
                    title: anime.display_title(),
                },
                chunks[0],
            );
        }
    } else {
        frame.render_widget(
            HalfblockCover {
                anime_id: anime.id,
                title: anime.display_title(),
            },
            chunks[0],
        );
    }

    // Title (crisp white bold when focused, clean truncation)
    let max_title_chars = chunks[1].width as usize;
    let title_str = anime.short_title();
    let short_title = truncate_chars(&title_str, max_title_chars);
    let title = Paragraph::new(Span::styled(
        short_title,
        Style::default()
            .fg(if selected { Color::White } else { theme.text })
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(title, chunks[1]);

    // Metadata / Colorized Progress Line
    let meta_line = build_card_meta_line(anime, progress, selected, &theme);
    frame.render_widget(Paragraph::new(meta_line), chunks[2]);
}

/// Build the bottom line of a card with colorized progress bars or score/format chips.
fn build_card_meta_line(
    anime: &Anime,
    progress: Option<&str>,
    selected: bool,
    theme: &crate::ui::theme::ThemePalette,
) -> Line<'static> {
    if let Some(prog) = progress {
        // Parse mini bar format like "E2 19:07 [━━────]" into dual-tone unicode bar
        if let Some(bracket_idx) = prog.find('[') {
            let prefix = prog[..bracket_idx].trim().to_string();
            let bar_raw = prog[bracket_idx..].trim_matches(|c| c == '[' || c == ']');
            let filled: String = bar_raw.chars().filter(|&c| c == '━').collect();
            let unfilled: String = bar_raw.chars().filter(|&c| c == '─').collect();
            let prefix_color = if selected { theme.accent } else { theme.secondary };
            let fill_color = if selected { theme.accent } else { theme.primary };
            return Line::from(vec![
                Span::styled(format!("{} ", prefix), Style::default().fg(prefix_color).add_modifier(Modifier::BOLD)),
                Span::styled(filled, Style::default().fg(fill_color)),
                Span::styled(unfilled, Style::default().fg(theme.divider)),
            ]);
        }

        return Line::from(Span::styled(
            truncate_chars(prog, 20),
            Style::default()
                .fg(if selected { theme.accent } else { theme.secondary })
                .add_modifier(Modifier::BOLD),
        ));
    }

    let score_str = anime
        .score
        .map(|s| format!(" {:.1}", s as f32 / 10.0))
        .unwrap_or_else(|| " N/A".to_string());
    let fmt_str = anime.format.as_deref().unwrap_or("TV").to_string();
    let is_airing = anime.status.as_deref() == Some("RELEASING");

    let mut spans = vec![
        Span::styled(
            score_str,
            Style::default()
                .fg(if selected { theme.accent } else { theme.text })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(theme.divider)),
        Span::styled(fmt_str, Style::default().fg(theme.text_dim)),
    ];
    if is_airing {
        spans.push(Span::styled(" · ", Style::default().fg(theme.divider)));
        spans.push(Span::styled("●", Style::default().fg(Color::Rgb(80, 250, 123))));
    }
    Line::from(spans)
}

fn format_pagination_dots(offset: usize, total: usize) -> String {
    if total <= 1 {
        return "●".to_string();
    }
    let slots = 5usize.min(total);
    let active_slot = if total <= slots {
        offset.min(slots - 1)
    } else {
        (offset * (slots - 1)) / (total - 1)
    };
    (0..slots)
        .map(|i| if i == active_slot { '●' } else { '○' })
        .collect()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max_chars {
        text.to_string()
    } else {
        let t: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", t)
    }
}

/// Word-wrap `text` into at most 2 lines of `width` chars, clamping any overflow with `…`.
fn wrap_two_lines(text: &str, width: usize) -> (String, String) {
    if width < 4 || text.is_empty() {
        return (String::new(), String::new());
    }
    let mut lines: [String; 2] = [String::new(), String::new()];
    let mut line_idx = 0usize;
    let mut overflowed = false;

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        let cur_len = lines[line_idx].chars().count();
        let needed = if cur_len == 0 { word_len } else { cur_len + 1 + word_len };

        if needed <= width {
            if cur_len > 0 {
                lines[line_idx].push(' ');
            }
            lines[line_idx].push_str(word);
        } else if line_idx == 0 {
            line_idx = 1;
            if word_len <= width {
                lines[1].push_str(word);
            } else {
                lines[1] = truncate_chars(word, width);
                overflowed = true;
                break;
            }
        } else {
            overflowed = true;
            break;
        }
    }

    if overflowed {
        let cur_len = lines[1].chars().count();
        if cur_len >= width {
            let trimmed: String = lines[1].chars().take(width.saturating_sub(1)).collect();
            lines[1] = format!("{}…", trimmed);
        } else if !lines[1].ends_with('…') {
            lines[1].push('…');
        }
    }

    (lines[0].clone(), lines[1].clone())
}

/// All data needed to render the home screen.
pub struct HomeData {
    pub featured: Option<Anime>,
    pub continue_watching: Vec<Anime>,
    pub watchlist: Vec<Anime>,
    pub progress_labels: HashMap<i64, String>,
    pub resume_next: HashMap<i64, u32>,
    pub trending: Vec<Anime>,
    pub popular: Vec<Anime>,
    pub top_rated: Vec<Anime>,
    pub seasonal: Vec<Anime>,
}

impl HomeData {
    pub fn empty() -> Self {
        Self {
            featured: None,
            continue_watching: Vec::new(),
            watchlist: Vec::new(),
            progress_labels: HashMap::new(),
            resume_next: HashMap::new(),
            trending: Vec::new(),
            popular: Vec::new(),
            top_rated: Vec::new(),
            seasonal: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.continue_watching.is_empty()
            && self.watchlist.is_empty()
            && self.trending.is_empty()
            && self.popular.is_empty()
            && self.top_rated.is_empty()
            && self.seasonal.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_two_lines_clamps_long_text() {
        let text = "Several hundred years ago, humans were nearly exterminated by giants called Titans who devour people without reason.";
        let (l1, l2) = wrap_two_lines(text, 40);
        assert!(l1.chars().count() <= 40);
        assert!(l2.chars().count() <= 40);
        assert!(l2.ends_with('…'));
    }

    #[test]
    fn test_bento_split_activates_for_small_continue_watching() {
        let mut data = HomeData::empty();
        data.continue_watching.push(Anime::default());
        data.watchlist.push(Anime::default());
        assert!(is_bento_split(&data, 120));
        assert!(!is_bento_split(&data, 80));

        data.continue_watching.push(Anime::default());
        assert!(is_bento_split(&data, 120));

        data.continue_watching.push(Anime::default());
        assert!(!is_bento_split(&data, 120));
    }

    #[test]
    fn test_row_view_start_prevents_trailing_blanks() {
        assert_eq!(row_view_start(0, 8, 4), 0);
        assert_eq!(row_view_start(5, 8, 4), 4);
        assert_eq!(row_view_start(7, 8, 4), 4);
        assert_eq!(row_view_start(1, 2, 2), 0);
    }

    #[test]
    fn test_home_footer_dock_never_truncates_or_clips_help() {
        use ratatui::{backend::TestBackend, Terminal};
        for width in [148u16, 126u16, 96u16] {
            let mut state = AppState::new();
            state.auth.username = Some("abdullah3".to_string());
            let backend = TestBackend::new(width, 38);
            let mut terminal = Terminal::new(backend).unwrap();
            let area = Rect::new(0, 35, width, FOOTER_HEIGHT);
            terminal
                .draw(|f| render_footer(f, area, &state))
                .expect("footer dock should render");

            let buf = terminal.backend().buffer();
            let mut rendered = String::new();
            for y in area.y..area.y + area.height {
                for x in 0..width {
                    rendered.push_str(buf[(x, y)].symbol());
                }
                rendered.push('\n');
            }
            assert!(rendered.contains("EXPLORE"), "missing EXPLORE badge at w={}", width);
            assert!(rendered.contains("ACTIONS"), "missing ACTIONS badge at w={}", width);
            assert!(rendered.contains("Search"), "missing Search at w={}", width);
            assert!(rendered.contains("Calendar"), "missing Calendar at w={}", width);
            assert!(rendered.contains("Help"), "missing full Help label at w={}", width);
            assert!(rendered.contains("Quit"), "missing full Quit label at w={}", width);
            assert!(rendered.contains("Settings"), "missing Settings at w={}", width);
        }
    }

    #[test]
    fn test_three_shelves_fit_on_48_row_terminal_without_empty_gap() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut data = HomeData::empty();
        data.continue_watching.push(Anime::default());
        data.watchlist.push(Anime::default());
        data.trending.push(Anime::default());
        data.popular.push(Anime::default());
        data.top_rated.push(Anime::default());
        data.featured = Some(Anime::default());

        let mut state = AppState::new();
        let backend = TestBackend::new(148, 48);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render(f, &mut state, &data))
            .expect("home screen should render 3 tiers at 148x48");

        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..48 {
            for x in 0..148 {
                rendered.push_str(buf[(x, y)].symbol());
            }
            rendered.push('\n');
        }
        assert!(rendered.contains("Continue Watching"), "missing row 1");
        assert!(rendered.contains("Trending Now"), "missing row 2");
        assert!(rendered.contains("Popular Picks"), "third shelf disappeared at 48 rows!");
        assert!(rendered.contains("ACTIONS"), "missing bottom command dock");
    }

    #[test]
    fn test_bento_watchlist_fills_width_without_dead_right_void() {
        // Matches Screenshot (168 cols wide -> left_w 26, spacer 1, wl outer 141, inner 139, usable 137)
        let (vis_n, card_w) = compute_row_card_metrics(137);
        assert_eq!(vis_n, 6);
        assert_eq!(card_w, 21);
        let used = vis_n as u16 * card_w + (vis_n as u16 - 1) * CARD_GAP;
        assert!(137 - used <= 2, "leftover margin {} too wide", 137 - used);

        // Also check 160-col terminal (wl usable = 129)
        let (vis_n2, card_w2) = compute_row_card_metrics(129);
        let used2 = vis_n2 as u16 * card_w2 + (vis_n2 as u16 - 1) * CARD_GAP;
        assert!(129 - used2 <= 4, "leftover margin {} too wide at 129 usable", 129 - used2);
    }
}
