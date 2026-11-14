//! Personal "Anime Wrapped" & Taste Analytics Dashboard (`a`).
//!
//! Asymmetric Bento telemetry command center combining high-resolution Ratatui
//! charts (`Canvas(Marker::Braille)` polar genre radar & cumulative growth curve,
//! half-block 24h circadian sky-wave, adaptive multi-week thermal contribution matrix,
//! era/format/season mosaic DNA, and live SQLite cover spotlight).

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Line as CanvasLine, Points},
        Block, BorderType, Borders, Clear, Paragraph,
    },
    Frame,
};
use ratatui_image::{Resize, StatefulImage};

use crate::db::user::UserAnalytics;
use crate::state::AppState;
use crate::ui::theme::ThemePalette;

type RgbColor = (u8, u8, u8);
type GradientStop = (RgbColor, RgbColor);

const GENRE_GRADIENTS: &[GradientStop] = &[
    ((245, 158, 11), (239, 68, 68)),   // 1. Solar Gold -> Flame Crimson
    ((236, 72, 153), (139, 92, 246)),  // 2. Neon Magenta -> Royal Purple
    ((6, 182, 212), (59, 130, 246)),   // 3. Cyber Cyan -> Azure Blue
    ((16, 185, 129), (163, 230, 53)),  // 4. Toxic Emerald -> Electric Lime
    ((99, 102, 241), (192, 132, 252)), // 5. Cyber Indigo -> Lilac
    ((251, 113, 133), (253, 186, 116)),// 6. Sunset Coral -> Peach
    ((20, 184, 166), (45, 212, 191)),  // 7. Electric Teal -> Mint
    ((56, 189, 248), (147, 197, 253)), // 8. Arctic Sky -> Ice Blue
];

const ERA_GRADIENTS: &[GradientStop] = &[
    ((217, 119, 6), (251, 191, 36)),   // Pre-2000: Vintage Amber
    ((37, 99, 235), (96, 165, 250)),   // 2000s: Classic Sapphire
    ((124, 58, 237), (192, 132, 252)), // 2010s: Ultra Violet
    ((225, 29, 72), (251, 113, 133)),  // 2020s: Cyber Rose
];

const ERA_COLORS: [Color; 4] = [
    Color::Rgb(251, 191, 36),
    Color::Rgb(96, 165, 250),
    Color::Rgb(192, 132, 252),
    Color::Rgb(251, 113, 133),
];

const ERA_SHORT: [&str; 4] = ["<00", "00s", "10s", "20s"];

const FORMAT_PALETTE: &[(&str, Color)] = &[
    ("TV", Color::Rgb(56, 189, 248)),
    ("ONA", Color::Rgb(192, 132, 252)),
    ("MOVIE", Color::Rgb(251, 191, 36)),
    ("OVA", Color::Rgb(52, 211, 153)),
    ("SPECIAL", Color::Rgb(251, 113, 133)),
];

const SEASON_STYLE: &[(&str, &str, Color)] = &[
    ("Winter", "❄ WIN", Color::Rgb(125, 211, 252)),
    ("Spring", "✿ SPR", Color::Rgb(244, 114, 182)),
    ("Summer", "☀ SUM", Color::Rgb(251, 191, 36)),
    ("Fall",   "❖ FAL", Color::Rgb(251, 146, 60)),
];

// ── Shared Text & Bar Primitives ─────────────────────────────────────────────

fn trunc_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else if max_chars == 1 {
        "…".to_string()
    } else {
        format!("{}…", s.chars().take(max_chars - 1).collect::<String>())
    }
}

fn lerp_rgb(a: RgbColor, b: RgbColor, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let r = (a.0 as f64 + t * (b.0 as f64 - a.0 as f64)).round() as u8;
    let g = (a.1 as f64 + t * (b.1 as f64 - a.1 as f64)).round() as u8;
    let bl = (a.2 as f64 + t * (b.2 as f64 - a.2 as f64)).round() as u8;
    Color::Rgb(r, g, bl)
}

/// Multi-stop truecolor horizontal bar with luminous leading cap and dark dotted track.
fn render_gradient_bar<'a>(
    bar_len: usize,
    total_width: usize,
    start_rgb: RgbColor,
    end_rgb: RgbColor,
) -> Vec<Span<'a>> {
    let mut spans = Vec::with_capacity(bar_len + 1);
    let capped = bar_len.min(total_width);
    if capped > 0 {
        for i in 0..capped {
            let t = if capped > 1 { i as f64 / (capped - 1) as f64 } else { 1.0 };
            let ch = if i + 1 == capped { "▌" } else { "█" };
            spans.push(Span::styled(ch, Style::default().fg(lerp_rgb(start_rgb, end_rgb, t))));
        }
    }
    let empty_len = total_width.saturating_sub(capped);
    if empty_len > 0 {
        spans.push(Span::styled(
            "░".repeat(empty_len),
            Style::default().fg(Color::Rgb(42, 46, 62)),
        ));
    }
    spans
}

/// Proportional character allocation guaranteeing total sum matches `slots`.
fn allocate_slots(weights: &[f64], slots: usize) -> Vec<usize> {
    if weights.is_empty() || slots == 0 {
        return vec![0; weights.len()];
    }
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        let mut out = vec![0; weights.len()];
        out[0] = slots;
        return out;
    }
    let exact: Vec<f64> = weights.iter().map(|w| (w / total) * slots as f64).collect();
    let mut floors: Vec<usize> = exact.iter().map(|v| v.floor() as usize).collect();
    let mut rem = slots.saturating_sub(floors.iter().sum());
    let mut remainders: Vec<(usize, f64)> = exact
        .iter()
        .enumerate()
        .map(|(i, v)| (i, v - v.floor()))
        .collect();
    remainders.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (idx, _) in remainders {
        if rem == 0 {
            break;
        }
        floors[idx] += 1;
        rem -= 1;
    }
    floors
}

fn mini_spark_str(values: &[u64]) -> String {
    const TICKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max_v = values.iter().copied().max().unwrap_or(0);
    values
        .iter()
        .map(|&v| {
            if max_v == 0 || v == 0 {
                '▁'
            } else {
                let idx = (((v as f64 / max_v as f64) * 7.0).round() as usize).min(7);
                TICKS[idx]
            }
        })
        .collect()
}

fn mini_braille_curve(values: &[f64]) -> String {
    // Converts up to 8 points into paired Braille glyph columns (2 dots per char)
    let left_dots = [0x40u32, 0x04, 0x02, 0x01];
    let right_dots = [0x80u32, 0x20, 0x10, 0x08];
    let min_v = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_v = values.iter().copied().fold(0.0f64, f64::max);
    let span = (max_v - min_v).max(0.01);

    let level = |v: f64| -> usize {
        (((v - min_v) / span) * 3.0).round().clamp(0.0, 3.0) as usize
    };

    let mut out = String::new();
    let padded: Vec<f64> = if values.len() >= 2 {
        values.to_vec()
    } else {
        vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0]
    };
    let step = (padded.len() as f64 / 8.0).max(1.0);
    let mut sampled = Vec::with_capacity(8);
    for i in 0..8 {
        let idx = ((i as f64 * step).floor() as usize).min(padded.len() - 1);
        sampled.push(padded[idx]);
    }
    for pair in sampled.chunks(2) {
        let l = level(pair[0]);
        let r = level(*pair.get(1).unwrap_or(&pair[0]));
        let code = 0x2800 | left_dots[l] | right_dots[r];
        if let Some(ch) = char::from_u32(code) {
            out.push(ch);
        }
    }
    out
}

fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        "0MB Offline".to_string()
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.0}MB Cache", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}GB Cache", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn styled_box(title: &str, title_color: Color, border_color: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(title_color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
}

fn active_pipeline_labels(state: &AppState) -> (&'static str, String) {
    let player = match state
        .playback_players
        .get(state.playback_player_cursor)
        .copied()
        .unwrap_or_default()
    {
        crate::config::Player::Mpv => "mpv",
        crate::config::Player::Iina => "iina",
        crate::config::Player::Vlc => "vlc",
    };
    let shader = if state.anime4k_mode == crate::config::Anime4KMode::Off {
        "Native".to_string()
    } else {
        format!("Anime4K:{}", state.anime4k_mode.as_str())
    };
    (player, shader)
}

// ── Top Render Entrypoint ────────────────────────────────────────────────────

/// Render the Bento Taste Analytics / Anime Wrapped dashboard.
pub fn render(frame: &mut Frame, state: &mut AppState) {
    let area = frame.area();
    frame.render_widget(Clear, area);

    let theme = state.theme();
    let outer_block = Block::default()
        .title(Span::styled(
            " ❖ SOLUS // ANIME WRAPPED & TASTE TELEMETRY ",
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary));

    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let Some(data) = state.analytics_data.clone() else {
        let loading = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  ◈ Synthesizing viewing telemetry & neural taste coordinates...",
                Style::default().fg(theme.text_dim),
            )),
        ]);
        frame.render_widget(loading, inner);
        return;
    };

    let compact = inner.height < 28 || inner.width < 92;
    let kpi_h = if compact { 3 } else { 4 };
    let header_h = 2;
    let footer_h = 1;

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),
            Constraint::Length(kpi_h),
            Constraint::Min(12),
            Constraint::Length(footer_h),
        ])
        .split(inner);

    render_identity_crest(frame, main_chunks[0], &data, state, &theme);
    render_kpi_strip(frame, main_chunks[1], &data, state, &theme);
    render_overview_bento(frame, main_chunks[2], &data, state, &theme);
    render_footer(frame, main_chunks[3], state, &theme);
}

// ── 1. Identity Crest ────────────────────────────────────────────────────────

fn render_identity_crest(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    state: &AppState,
    theme: &ThemePalette,
) {
    let status_min = if area.width >= 118 { 42 } else if area.width >= 90 { 36 } else { 22 };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(24),
            Constraint::Length(status_min.min(area.width / 2)),
        ])
        .split(area);

    let left_w = cols[0].width as usize;
    let shows_total = data.completed_shows + data.in_progress_shows;
    let streak_str = format!(
        "Streak: ★ {}d Active (Best: {}d)",
        data.active_streak_days, data.longest_streak_days
    );
    let catalog_meta = format!(
        " {} · {} Show{} · {} Ep",
        data.year_span_label,
        shows_total,
        if shows_total == 1 { "" } else { "s" },
        data.total_episodes_watched
    );

    let cadence_short = data.viewing_cadence.trim();
    let max_vel = data.recent_activity.iter().copied().max().unwrap_or(0);
    let vol_14d: u64 = data.recent_activity.iter().sum();
    let delta_tag = match data.velocity_delta_pct {
        Some(pct) if pct >= 0.0 => format!("+{:.0}% vs prior 14d", pct),
        Some(pct) => format!("{:.0}% vs prior 14d", pct),
        None => if data.total_episodes_watched <= 3 { "Calibration Phase".to_string() } else { "Steady Tempo".to_string() },
    };

    let arch_budget = left_w.saturating_sub(16 + catalog_meta.chars().count());
    let cad_budget = left_w.saturating_sub(17 + streak_str.chars().count());

    let left_lines = vec![
        Line::from(vec![
            Span::styled(" ❖ ARCHETYPE: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                trunc_chars(data.taste_archetype, arch_budget.max(18)),
                Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" │", Style::default().fg(Color::Rgb(60, 65, 88))),
            Span::styled(trunc_chars(&catalog_meta, left_w.saturating_sub(24)), Style::default().fg(theme.text_dim)),
        ]),
        Line::from(vec![
            Span::styled(" ◈ CADENCE:   ", Style::default().fg(theme.text_dim)),
            Span::styled(
                trunc_chars(cadence_short, cad_budget.max(16)),
                Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" │ ", Style::default().fg(Color::Rgb(60, 65, 88))),
            Span::styled(streak_str, Style::default().fg(theme.accent)),
        ]),
    ];
    frame.render_widget(Paragraph::new(left_lines), cols[0]);

    let spotlight_tag = if data.top_shows.is_empty() {
        "TELEMETRY IDLE".to_string()
    } else {
        let idx = (state.analytics_podium_idx % data.top_shows.len()) + 1;
        format!("SPOTLIGHT #{}/{}", idx, data.top_shows.len())
    };
    let top_status = vec![
        Span::styled("❖ ", Style::default().fg(theme.primary)),
        Span::styled(
            format!("{} GENRES", data.top_genres.len()),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(Color::Rgb(75, 80, 105))),
        Span::styled(
            format!(" {} ", spotlight_tag),
            Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];

    let vel_line = format!(
        "Peak: {} ep/d · 14d Vol: {} · {} ",
        max_vel, vol_14d, delta_tag
    );
    let right_lines = vec![
        Line::from(top_status).alignment(Alignment::Right),
        Line::from(Span::styled(
            trunc_chars(&vel_line, cols[1].width.saturating_sub(1) as usize),
            Style::default().fg(Color::Rgb(135, 140, 165)),
        ))
        .alignment(Alignment::Right),
    ];
    frame.render_widget(Paragraph::new(right_lines), cols[1]);
}

// ── 2. 5-Cell Micro-Chart KPI Ribbon ─────────────────────────────────────────

fn render_kpi_strip(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    state: &AppState,
    theme: &ThemePalette,
) {
    let kpi_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 5),
            Constraint::Ratio(1, 5),
            Constraint::Ratio(1, 5),
            Constraint::Ratio(1, 5),
            Constraint::Ratio(1, 5),
        ])
        .split(area);

    // KPI 1: Watch Time + Braille curve
    let mins = (data.total_hours_watched * 60.0).round() as u64;
    let watch_val = format!("{:.1}h ({}m)", data.total_hours_watched, mins);
    let pace_mins = if data.total_episodes_watched > 0 {
        (mins as f64 / data.total_episodes_watched as f64).round() as u64
    } else {
        24
    };
    let watch_spark = mini_braille_curve(
        data.cumulative_hours_30d
            .get(data.cumulative_hours_30d.len().saturating_sub(8)..)
            .unwrap_or(&[0.0, data.total_hours_watched]),
    );
    let watch_sub = format!("Pace: {}m/ep · +{:.1}h 7d", pace_mins, data.recent_7d_hours);
    render_micro_kpi(
        frame,
        kpi_cols[0],
        "◈ WATCH TIME",
        &watch_spark,
        &watch_val,
        &watch_sub,
        theme.primary,
        Color::Rgb(56, 189, 248),
    );

    // KPI 2: Episodes + 14d Sparkline
    let ep_spark = mini_spark_str(data.recent_activity.get(data.recent_activity.len().saturating_sub(9)..).unwrap_or(&[]));
    let peak_vel = data.recent_activity.iter().copied().max().unwrap_or(0);
    let vol_14d: u64 = data.recent_activity.iter().sum();
    let ep_val = format!("{} ep logged", data.total_episodes_watched);
    let ep_sub = format!("Peak: {}/d · Vol 14d: {}", peak_vel, vol_14d);
    render_micro_kpi(
        frame,
        kpi_cols[1],
        "❖ EPISODES",
        &ep_spark,
        &ep_val,
        &ep_sub,
        theme.secondary,
        Color::Rgb(80, 250, 123),
    );

    // KPI 3: Completion + Arc Gauge
    let arc = match data.completion_rate as u32 {
        90..=100 => "●",
        65..=89 => "◕",
        35..=64 => "◑",
        1..=34 => "◔",
        _ => "○",
    };
    let gauge_fill = ((data.completion_rate / 100.0) * 6.0).round() as usize;
    let arc_badge = format!(
        "{} {}{}",
        arc,
        "█".repeat(gauge_fill.min(6)),
        "░".repeat(6usize.saturating_sub(gauge_fill))
    );
    let shows_total = data.completed_shows + data.in_progress_shows;
    let comp_val = format!("{:.0}% ({}/{})", data.completion_rate, data.completed_shows, shows_total);
    let comp_sub = format!("{} Active · {} Queue", data.in_progress_shows, data.total_watchlist_shows);
    render_micro_kpi(
        frame,
        kpi_cols[2],
        "✦ COMPLETION",
        &arc_badge,
        &comp_val,
        &comp_sub,
        Color::Rgb(80, 250, 123),
        Color::Rgb(16, 185, 129),
    );

    // KPI 4: Avg Score + Bucket Spread
    let score_u64: Vec<u64> = data.score_buckets.iter().rev().map(|&c| c as u64).collect();
    let score_spark = mini_spark_str(&score_u64);
    let score_val = if data.avg_score > 0.0 {
        format!("{:.1} Mean", data.avg_score)
    } else {
        "N/A".to_string()
    };
    let score_tier = if data.avg_score >= 85.0 {
        "Tier: Critically Acclaimed"
    } else if data.avg_score >= 78.0 {
        "Tier: Cult & Curated"
    } else if data.avg_score > 0.0 {
        "Tier: Eclectic Taste"
    } else {
        "Tier: Unscored Catalog"
    };
    render_micro_kpi(
        frame,
        kpi_cols[3],
        "★ TASTE SCORE",
        &score_spark,
        &score_val,
        score_tier,
        Color::Rgb(255, 184, 108),
        Color::Rgb(251, 191, 36),
    );

    // KPI 5: Pipeline + Audio Ratio
    let sub_blocks = ((data.sub_pct / 100.0) * 6.0).round() as usize;
    let pipe_badge = format!(
        "{}{}",
        "█".repeat(sub_blocks.min(6)),
        "░".repeat(6usize.saturating_sub(sub_blocks))
    );
    let pipe_val = format!("{:.0}% Sub · {:.0}% Dub", data.sub_pct, data.dub_pct);
    let (player, shader) = active_pipeline_labels(state);
    let pipe_sub = format!("{} · {} · {}", player, shader, format_bytes(data.offline_cached_bytes));
    render_micro_kpi(
        frame,
        kpi_cols[4],
        "▸ PIPELINE",
        &pipe_badge,
        &pipe_val,
        &pipe_sub,
        Color::Rgb(189, 147, 249),
        Color::Rgb(56, 189, 248),
    );
}

#[allow(clippy::too_many_arguments)]
fn render_micro_kpi(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    right_spark: &str,
    primary_val: &str,
    subtitle: &str,
    border_color: Color,
    spark_color: Color,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let w = inner.width as usize;
    if w < 6 || inner.height == 0 {
        return;
    }

    let spark_w = right_spark.chars().count();
    let label_max = w.saturating_sub(spark_w + 2);
    let title_str = trunc_chars(title, label_max);
    let pad = w.saturating_sub(title_str.chars().count() + spark_w + 1);

    let mut lines = vec![Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(title_str, Style::default().fg(Color::Rgb(150, 156, 178))),
        Span::raw(" ".repeat(pad)),
        Span::styled(right_spark.to_string(), Style::default().fg(spark_color)),
    ])];

    if inner.height >= 2 {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                trunc_chars(primary_val, w.saturating_sub(1)),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if inner.height >= 3 {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                trunc_chars(subtitle, w.saturating_sub(1)),
                Style::default().fg(Color::Rgb(115, 120, 145)),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

// ── 3. Asymmetric 3-Column Overview Bento ──────────────────────────────────

fn render_overview_bento(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    state: &mut AppState,
    theme: &ThemePalette,
) {
    if area.width >= 132 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(32), // Left: Genre Radar + Era/Format/Season DNA
                Constraint::Percentage(38), // Center: 24h Sky-Wave + Thermal Grid + Spline/Scores
                Constraint::Percentage(30), // Right: #1 Cover Spotlight + Horizon + Milestones
            ])
            .split(area);

        let left_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(15),
                Constraint::Length(15),
            ])
            .split(cols[0]);
        render_genre_radar_card(frame, left_rows[0], data, theme);
        render_era_format_season_card(frame, left_rows[1], data, theme);

        let center_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),
                Constraint::Length(11),
                Constraint::Length(9),
            ])
            .split(cols[1]);
        render_circadian_card(frame, center_rows[0], data, theme);
        render_thermal_heatmap_card(frame, center_rows[1], data, theme);
        render_growth_and_scores_card(frame, center_rows[2], data, theme);

        let right_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10),
                Constraint::Min(9),
                Constraint::Length(9),
            ])
            .split(cols[2]);
        render_podium_spotlight_card(frame, right_rows[0], data, state, theme);
        render_rotation_horizon_card(frame, right_rows[1], data, state.analytics_list_scroll, theme);
        render_milestones_diagnostics_card(frame, right_rows[2], data, state, theme);
    } else if area.width >= 92 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(48),
                Constraint::Percentage(52),
            ])
            .split(area);

        let left_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(42),
                Constraint::Percentage(32),
                Constraint::Percentage(26),
            ])
            .split(cols[0]);
        render_genre_radar_card(frame, left_rows[0], data, theme);
        render_era_format_season_card(frame, left_rows[1], data, theme);
        render_growth_and_scores_card(frame, left_rows[2], data, theme);

        let right_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(34),
                Constraint::Percentage(36),
                Constraint::Percentage(30),
            ])
            .split(cols[1]);
        render_podium_spotlight_card(frame, right_rows[0], data, state, theme);
        render_thermal_heatmap_card(frame, right_rows[1], data, theme);
        render_rotation_horizon_card(frame, right_rows[2], data, state.analytics_list_scroll, theme);
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(38),
                Constraint::Percentage(32),
                Constraint::Percentage(30),
            ])
            .split(area);
        render_podium_spotlight_card(frame, rows[0], data, state, theme);
        render_thermal_heatmap_card(frame, rows[1], data, theme);
        render_era_format_season_card(frame, rows[2], data, theme);
    }
}

// ── Chart 1A: Braille Polar Genre Radar + Compact 2-Column Ranked Strip ──────

fn render_genre_radar_card(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    theme: &ThemePalette,
) {
    let title = if data.top_genres.len() > 1
        && data.top_genres.iter().take(6).all(|g| g.1 == data.top_genres[0].1)
    {
        "◈ GENRE RADAR POLARITY (Equilibrium)"
    } else {
        "◈ GENRE RADAR POLARITY"
    };
    let block = styled_box(title, theme.primary, theme.border_idle);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if data.top_genres.is_empty() || inner.height < 4 || inner.width < 18 {
        frame.render_widget(
            Paragraph::new(" No watched genres recorded yet.")
                .style(Style::default().fg(theme.text_dim)),
            inner,
        );
        return;
    }

    let ideal_radar_h = ((inner.width as f64 * 0.34).round() as u16).clamp(11, 15);
    let max_radar_h = inner.height.saturating_sub(4).max(6);
    let radar_h = ideal_radar_h.min(max_radar_h);
    let rem_rows = inner.height.saturating_sub(radar_h);
    let has_div = rem_rows >= 3;
    let radar_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if has_div {
            vec![
                Constraint::Length(radar_h),
                Constraint::Length(1),
                Constraint::Min(2),
            ]
        } else {
            vec![Constraint::Min(1), Constraint::Length(0), Constraint::Length(0)]
        })
        .split(inner);

    render_polar_radar_canvas(frame, radar_chunks[0], &data.top_genres, theme);

    if has_div {
        let sep = "─".repeat(radar_chunks[1].width as usize);
        frame.render_widget(
            Paragraph::new(Span::styled(sep, Style::default().fg(Color::Rgb(42, 46, 62)))),
            radar_chunks[1],
        );
        let chip_rows = radar_chunks[2].height as usize;
        if chip_rows >= data.top_genres.len().min(7) + 1 {
            // Spacious layout: full-width multi-stop bars + affinity summary line
            render_full_width_genre_bars(frame, radar_chunks[2], &data.top_genres, theme);
        } else {
            render_two_col_genre_chips(frame, radar_chunks[2], &data.top_genres, theme);
        }
    }
}

fn render_polar_radar_canvas(
    frame: &mut Frame,
    area: Rect,
    top_genres: &[(String, usize, f64)],
    theme: &ThemePalette,
) {
    if area.width < 22 || area.height < 6 {
        return;
    }
    let axes_count = top_genres.len().clamp(3, 6);
    let max_cnt = top_genres.first().map(|g| g.1).unwrap_or(1).max(1) as f64;
    let angles: Vec<f64> = (0..axes_count)
        .map(|i| std::f64::consts::FRAC_PI_2 - (2.0 * std::f64::consts::PI * i as f64) / axes_count as f64)
        .collect();

    let top_pad: u16 = 1;
    let bot_pad: u16 = if axes_count >= 4 { 1 } else { 0 };
    if area.height <= top_pad + bot_pad + 3 {
        return;
    }
    let canvas_area = Rect {
        x: area.x,
        y: area.y + top_pad,
        width: area.width,
        height: area.height - top_pad - bot_pad,
    };

    let items: Vec<(String, f64, f64)> = top_genres
        .iter()
        .take(axes_count)
        .map(|(name, cnt, pct)| (name.clone(), (*cnt as f64 / max_cnt).clamp(0.18, 1.0), *pct))
        .collect();

    // Compute exact full-label column requirements so standard genres never truncate
    let w = canvas_area.width as usize;
    let is_bottom_diag = |ang: f64| axes_count == 5 && ang.sin() < -0.6;
    let side_len = |i: usize| -> usize {
        let (ref name, _, pct) = items[i];
        name.chars().count() + format!(" {:.0}%", pct).len()
    };
    let max_left_full = angles
        .iter()
        .enumerate()
        .filter(|&(i, a)| a.cos() < -0.25 && !is_bottom_diag(*angles.get(i).unwrap()))
        .map(|(i, _)| side_len(i))
        .max()
        .unwrap_or(0);
    let max_right_full = angles
        .iter()
        .enumerate()
        .filter(|&(i, a)| a.cos() > 0.25 && !is_bottom_diag(*angles.get(i).unwrap()))
        .map(|(i, _)| side_len(i))
        .max()
        .unwrap_or(0);
    let max_abs_cos = angles
        .iter()
        .enumerate()
        .filter(|&(i, a)| a.cos().abs() >= 0.25 && !is_bottom_diag(*angles.get(i).unwrap()))
        .map(|(_, a)| a.cos().abs())
        .fold(0.866f64, f64::max);

    let x_half = canvas_area.width as f64;
    let y_half = (canvas_area.height * 2) as f64;
    let r_vert = (y_half - 0.8).max(4.0);
    let min_poly_cols = (w as f64 * 0.22).clamp(8.0, 12.0);
    let max_side_budget = (w as f64 - min_poly_cols - 6.0).max(10.0) as usize;
    let (left_budget, right_budget) = if max_left_full + max_right_full <= max_side_budget {
        (max_left_full, max_right_full)
    } else {
        let half = max_side_budget / 2;
        if max_left_full <= half {
            (max_left_full, max_side_budget - max_left_full)
        } else if max_right_full <= half {
            (max_side_budget - max_right_full, max_right_full)
        } else {
            (half, max_side_budget - half)
        }
    };
    let avail_poly_cols = (w as f64 - left_budget as f64 - right_budget as f64 - 6.0).max(min_poly_cols);
    let r_horiz = avail_poly_cols / max_abs_cos.max(0.5);
    let r = r_vert.min(r_horiz).max(4.0);

    // Nudge center column slightly only if asymmetric side labels need a few extra columns
    let poly_half_cols = (r * max_abs_cos) / 2.0;
    let min_center_col = 1.0 + left_budget as f64 + 2.0 + poly_half_cols;
    let max_center_col = w as f64 - 1.0 - right_budget as f64 - 2.0 - poly_half_cols;
    let center_col = if min_center_col <= max_center_col {
        (w as f64 / 2.0).clamp(min_center_col, max_center_col)
    } else {
        (min_center_col + max_center_col) * 0.5
    };
    let cx = (center_col - w as f64 / 2.0) * 2.0;
    let cy = 0.0f64;

    let p_col = theme.primary;
    let a_col = theme.accent;
    let s_col = theme.secondary;

    let canvas_items = items.clone();
    let canvas_angles = angles.clone();
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-x_half, x_half])
        .y_bounds([-y_half, y_half])
        .paint(move |ctx| {
            let n = canvas_items.len();

            // Concentric guide rings (33%, 66%, 100%)
            for (scale, col) in [
                (0.33, Color::Rgb(38, 42, 60)),
                (0.66, Color::Rgb(50, 56, 78)),
                (1.00, Color::Rgb(78, 84, 114)),
            ] {
                for i in 0..n {
                    let a1 = canvas_angles[i];
                    let a2 = canvas_angles[(i + 1) % n];
                    ctx.draw(&CanvasLine {
                        x1: cx + r * scale * a1.cos(),
                        y1: cy + r * scale * a1.sin(),
                        x2: cx + r * scale * a2.cos(),
                        y2: cy + r * scale * a2.sin(),
                        color: col,
                    });
                }
            }

            // Spokes
            for &ang in &canvas_angles {
                ctx.draw(&CanvasLine {
                    x1: cx,
                    y1: cy,
                    x2: cx + r * ang.cos(),
                    y2: cy + r * ang.sin(),
                    color: Color::Rgb(54, 58, 82),
                });
            }

            // Filled inner glow chords + polygon perimeter
            let data_pts: Vec<(f64, f64)> = canvas_angles
                .iter()
                .zip(canvas_items.iter())
                .map(|(&ang, (_, norm, _))| {
                    let rad = r * (0.18 + 0.82 * norm);
                    (cx + rad * ang.cos(), cy + rad * ang.sin())
                })
                .collect();

            for scale in [0.45, 0.72] {
                for i in 0..n {
                    let (p1x, p1y) = data_pts[i];
                    let (p2x, p2y) = data_pts[(i + 1) % n];
                    ctx.draw(&CanvasLine {
                        x1: cx + (p1x - cx) * scale,
                        y1: cy + (p1y - cy) * scale,
                        x2: cx + (p2x - cx) * scale,
                        y2: cy + (p2y - cy) * scale,
                        color: s_col,
                    });
                }
            }

            for i in 0..n {
                let (x1, y1) = data_pts[i];
                let (x2, y2) = data_pts[(i + 1) % n];
                ctx.draw(&CanvasLine { x1, y1, x2, y2, color: p_col });
            }

            ctx.draw(&Points {
                coords: &data_pts,
                color: a_col,
            });
        });

    frame.render_widget(canvas, canvas_area);

    // Overlay vertex axis labels strictly outside the Braille polygon using exact grid cell inverse
    let ch_f = canvas_area.height as f64;
    let w_f = canvas_area.width as f64;
    let dot_col = |vx: f64| -> u16 {
        let pix = ((vx + x_half) * (2.0 * w_f - 1.0) / (2.0 * x_half))
            .clamp(0.0, (2.0 * w_f - 1.0).max(0.0)) as u16;
        pix / 2
    };
    let dot_row = |vy: f64| -> u16 {
        let pix = ((y_half - vy) * (4.0 * ch_f - 1.0) / (2.0 * y_half))
            .clamp(0.0, (4.0 * ch_f - 1.0).max(0.0)) as u16;
        pix / 4
    };
    let center_u16 = (center_col.round() as u16).min(area.width.saturating_sub(1));

    for (i, &ang) in angles.iter().enumerate() {
        let (ref g_name, _, pct) = items[i];
        let pct_str = format!(" {:.0}%", pct);
        let pct_len = pct_str.len();
        let cos_a = ang.cos();
        let sin_a = ang.sin();

        let make_line = |max_total: usize| -> Line<'_> {
            let max_name = max_total.saturating_sub(pct_len).max(3);
            let shown = if g_name.chars().count() <= max_name {
                g_name.clone()
            } else {
                trunc_chars(g_name, max_name)
            };
            Line::from(vec![
                Span::styled(shown, Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
                Span::styled(pct_str.clone(), Style::default().fg(a_col)),
            ])
        };

        if cos_a.abs() < 0.25 {
            // Top / bottom apex rendered strictly above/below vertex dot row
            let vr = dot_row(cy + r * sin_a);
            let label_y = if sin_a > 0.0 {
                (canvas_area.y + vr).saturating_sub(1).max(area.y)
            } else {
                (canvas_area.y + vr + 1).min(area.y + area.height - 1)
            };
            let max_w = w.saturating_sub(2);
            let line = make_line(max_w);
            let lw = line.width().min(max_w) as u16;
            let start_x = area.x + center_u16.saturating_sub(lw / 2).min(area.width.saturating_sub(lw + 1));
            frame.render_widget(
                Paragraph::new(line),
                Rect { x: start_x, y: label_y, width: lw, height: 1 },
            );
        } else if is_bottom_diag(ang) {
            let label_y = area.y + area.height - 1;
            if cos_a < 0.0 {
                let avail = (center_u16.saturating_sub(2)) as usize;
                let line = make_line(avail);
                frame.render_widget(
                    Paragraph::new(line).alignment(Alignment::Right),
                    Rect { x: area.x + 1, y: label_y, width: avail as u16, height: 1 },
                );
            } else {
                let start_col = (center_u16 + 2).min(area.width.saturating_sub(1));
                let avail = area.width.saturating_sub(start_col + 1) as usize;
                let line = make_line(avail);
                frame.render_widget(
                    Paragraph::new(line),
                    Rect { x: area.x + start_col, y: label_y, width: avail as u16, height: 1 },
                );
            }
        } else {
            let vx = cx + r * cos_a;
            let vy = cy + r * sin_a;
            let vc = dot_col(vx);
            let vr = dot_row(vy);
            let label_y = canvas_area.y + vr;
            if cos_a < 0.0 {
                let end_col = vc.saturating_sub(1);
                let avail = end_col.saturating_sub(1) as usize;
                let line = make_line(avail.min(left_budget.max(6)));
                let lw = (line.width() as u16).min(avail as u16);
                let sx = area.x + end_col.saturating_sub(lw);
                frame.render_widget(
                    Paragraph::new(line),
                    Rect { x: sx, y: label_y, width: lw, height: 1 },
                );
            } else {
                let start_col = (vc + 2).min(area.width.saturating_sub(1));
                let avail = area.width.saturating_sub(start_col + 1) as usize;
                let line = make_line(avail.min(right_budget.max(6)));
                let lw = (line.width() as u16).min(avail as u16);
                frame.render_widget(
                    Paragraph::new(line),
                    Rect { x: area.x + start_col, y: label_y, width: lw, height: 1 },
                );
            }
        }
    }
}

fn render_full_width_genre_bars(
    frame: &mut Frame,
    area: Rect,
    top_genres: &[(String, usize, f64)],
    theme: &ThemePalette,
) {
    let w = area.width as usize;
    let h = area.height as usize;
    let name_w = if w >= 42 { 14 } else { 12 };
    let bar_w = w.saturating_sub(name_w + 17).clamp(6, 28);
    let max_cnt = top_genres.first().map(|g| g.1).unwrap_or(1).max(1) as f64;
    let mut lines = Vec::with_capacity(h);

    for (idx, (g, cnt, pct)) in top_genres.iter().take(h.saturating_sub(1)).enumerate() {
        let fill = ((*cnt as f64 / max_cnt) * bar_w as f64).round() as usize;
        let (s_rgb, e_rgb) = GENRE_GRADIENTS[idx % GENRE_GRADIENTS.len()];
        let mut row = vec![
            Span::styled(format!(" #{} ", idx + 1), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{:<nw$} ", trunc_chars(g, name_w), nw = name_w),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ];
        row.extend(render_gradient_bar(fill, bar_w, s_rgb, e_rgb));
        row.push(Span::styled(format!(" {:>4.1}% ({:>2})", pct, cnt), Style::default().fg(theme.text_dim)));
        lines.push(Line::from(row));
    }
    if lines.len() < h {
        let summary = format!(" ✦ {} Distinct Genre Vectors Indexed · Weighted by Watch Time", top_genres.len());
        lines.push(Line::from(Span::styled(
            trunc_chars(&summary, w),
            Style::default().fg(Color::Rgb(115, 120, 145)),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_two_col_genre_chips(
    frame: &mut Frame,
    area: Rect,
    top_genres: &[(String, usize, f64)],
    theme: &ThemePalette,
) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let max_cnt = top_genres.first().map(|g| g.1).unwrap_or(1).max(1) as f64;
    let rows_per_col = area.height as usize;

    for col_idx in 0..2 {
        let mut lines = Vec::with_capacity(rows_per_col);
        let cw = cols[col_idx].width as usize;
        let name_w = cw.saturating_sub(14).clamp(7, 13);
        let bar_w = cw.saturating_sub(name_w + 10).clamp(3, 12);
        for row in 0..rows_per_col {
            let idx = col_idx * rows_per_col + row;
            let Some((g, cnt, pct)) = top_genres.get(idx) else {
                break;
            };
            let fill = ((*cnt as f64 / max_cnt) * bar_w as f64).round() as usize;
            let (s_rgb, e_rgb) = GENRE_GRADIENTS[idx % GENRE_GRADIENTS.len()];
            let mut spans = vec![
                Span::styled(format!(" #{} ", idx + 1), Style::default().fg(Color::Rgb(120, 125, 150))),
                Span::styled(
                    format!("{:<nw$} ", trunc_chars(g, name_w), nw = name_w),
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{:>3.0}% ", pct), Style::default().fg(theme.text_dim)),
            ];
            spans.extend(render_gradient_bar(fill, bar_w, s_rgb, e_rgb));
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), cols[col_idx]);
    }
}

// ── Chart 1B: Era Waffle Mosaic + All-Format DNA + Seasonal Compass ──────────

fn render_era_format_season_card(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    theme: &ThemePalette,
) {
    let block = styled_box("❖ ERA, FORMAT & SEASON DNA", theme.secondary, theme.border_idle);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let w = inner.width as usize;
    if w < 16 || inner.height < 4 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();
    let vintage_text = match data.weighted_median_year {
        Some(y) => format!("Weighted Vintage: {}", y),
        None => "Catalog Spread".to_string(),
    };
    lines.push(Line::from(vec![
        Span::styled(" ERA WAFFLE MOSAIC ", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD)),
        Span::styled(format!("({})", vintage_text), Style::default().fg(theme.accent)),
    ]));

    // Proportional 40-cell waffle ribbon across the full width
    let mosaic_w = w.saturating_sub(2).clamp(16, 48);
    let era_weights: Vec<f64> = data.era_distribution.iter().map(|(_, c, _)| *c as f64).collect();
    let era_cells = allocate_slots(&era_weights, mosaic_w);
    let mosaic_glyphs = ["▓", "▒", "█", "░"];
    let mut mosaic_spans = vec![Span::raw(" ")];
    if era_weights.iter().sum::<f64>() <= 0.0 {
        mosaic_spans.push(Span::styled("░".repeat(mosaic_w), Style::default().fg(Color::Rgb(45, 50, 68))));
    } else {
        for (idx, &cells) in era_cells.iter().enumerate() {
            if cells > 0 {
                mosaic_spans.push(Span::styled(
                    mosaic_glyphs[idx % 4].repeat(cells),
                    Style::default().fg(ERA_COLORS[idx % 4]),
                ));
            }
        }
    }
    lines.push(Line::from(mosaic_spans));

    let mut era_legend = vec![Span::raw(" ")];
    for (idx, (_, _, pct)) in data.era_distribution.iter().enumerate() {
        if idx > 0 {
            era_legend.push(Span::styled(" · ", Style::default().fg(Color::Rgb(60, 65, 88))));
        }
        era_legend.push(Span::styled(ERA_SHORT[idx], Style::default().fg(ERA_COLORS[idx]).add_modifier(Modifier::BOLD)));
        era_legend.push(Span::styled(format!(":{:.0}%", pct), Style::default().fg(theme.text_dim)));
    }
    lines.push(Line::from(era_legend));

    lines.push(Line::from(Span::styled(
        "─".repeat(w),
        Style::default().fg(Color::Rgb(42, 46, 62)),
    )));

    // 4 Decade Depth Bars
    let era_bar_w = w.saturating_sub(27).clamp(6, 22);
    let max_era = data.era_distribution.iter().map(|(_, c, _)| *c).max().unwrap_or(1).max(1) as f64;
    for (idx, (label, count, pct)) in data.era_distribution.iter().enumerate() {
        let short_lbl = match idx {
            0 => "<2000",
            1 => "2000s",
            2 => "2010s",
            _ => "2020s",
        };
        let _ = label;
        let fill = ((*count as f64 / max_era) * era_bar_w as f64).round() as usize;
        let (s_rgb, e_rgb) = ERA_GRADIENTS[idx % ERA_GRADIENTS.len()];
        let mut row = vec![
            Span::styled(format!(" {:<6}", short_lbl), Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        ];
        row.extend(render_gradient_bar(fill, era_bar_w, s_rgb, e_rgb));
        row.push(Span::styled(format!(" {:>3.0}% ({:>2}t)", pct, count), Style::default().fg(theme.text_dim)));
        lines.push(Line::from(row));
    }

    if inner.height >= 10 {
        lines.push(Line::from(Span::styled(
            "─".repeat(w),
            Style::default().fg(Color::Rgb(42, 46, 62)),
        )));

        // Continuous All-Format Segmented Bar
        let fmt_total: usize = data.format_distribution.iter().map(|(_, c)| *c).sum();
        let fmt_bar_w = w.saturating_sub(22).clamp(8, 28);
        let fmt_weights: Vec<f64> = data.format_distribution.iter().map(|(_, c)| *c as f64).collect();
        let fmt_slots = allocate_slots(&fmt_weights, fmt_bar_w);
        let mut fmt_bar = vec![Span::styled(" Format: ", Style::default().fg(theme.text_dim))];
        if fmt_total == 0 {
            fmt_bar.push(Span::styled("░".repeat(fmt_bar_w), Style::default().fg(Color::Rgb(45, 50, 68))));
        } else {
            for ((fmt, _), &slots) in data.format_distribution.iter().zip(fmt_slots.iter()) {
                if slots == 0 {
                    continue;
                }
                let col = FORMAT_PALETTE
                    .iter()
                    .find(|(k, _)| fmt.eq_ignore_ascii_case(k))
                    .map(|(_, c)| *c)
                    .unwrap_or(Color::Rgb(148, 163, 184));
                fmt_bar.push(Span::styled("█".repeat(slots), Style::default().fg(col)));
            }
        }
        if let Some((top_fmt, cnt)) = data.format_distribution.first() {
            let pct = if fmt_total > 0 { (*cnt as f64 / fmt_total as f64) * 100.0 } else { 0.0 };
            fmt_bar.push(Span::styled(
                format!(" {} {:.0}%", top_fmt, pct),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(fmt_bar));

        let fmt_detail: Vec<String> = data
            .format_distribution
            .iter()
            .take(4)
            .map(|(f, c)| format!("{} {}", c, f))
            .collect();
        let fmt_str = if fmt_detail.is_empty() {
            "No formats logged".to_string()
        } else {
            fmt_detail.join(" · ")
        };
        lines.push(Line::from(Span::styled(
            format!("         {}", trunc_chars(&fmt_str, w.saturating_sub(9))),
            Style::default().fg(Color::Rgb(120, 125, 145)),
        )));
    }

    if inner.height >= 12 {
        // Seasonal Airing Compass
        let mut sea_spans = vec![Span::styled(" Season: ", Style::default().fg(theme.text_dim))];
        for (idx, (_, cnt, pct)) in data.season_distribution.iter().enumerate() {
            let (_, badge, col) = SEASON_STYLE[idx % SEASON_STYLE.len()];
            if idx > 0 {
                sea_spans.push(Span::raw(" "));
            }
            let txt = if w >= 42 {
                format!("{} {:.0}%", badge, pct)
            } else {
                format!("{} {}", badge.chars().next().unwrap_or('•'), cnt)
            };
            sea_spans.push(Span::styled(txt, Style::default().fg(col)));
        }
        lines.push(Line::from(sea_spans));
    }

    if inner.height >= 13 {
        // True Data-Driven Synthesis Insight (never contradicts real era or format)
        let dom_era = data
            .era_distribution
            .iter()
            .max_by_key(|(_, c, _)| *c)
            .filter(|(_, c, _)| *c > 0)
            .map(|(l, _, _)| *l)
            .unwrap_or("Mixed Era");
        let dom_fmt = data
            .format_distribution
            .first()
            .map(|(f, _)| f.as_str())
            .unwrap_or("TV");
        let dom_genre = data
            .top_genres
            .first()
            .map(|(g, _, _)| g.as_str())
            .unwrap_or("Anime");
        let syn = format!("◈ Synthesis: {} {} · {} lead", dom_era.split('(').next().unwrap_or(dom_era).trim(), dom_fmt, dom_genre);
        lines.push(Line::from(Span::styled(
            format!(" {}", trunc_chars(&syn, w.saturating_sub(1))),
            Style::default().fg(Color::Rgb(145, 150, 175)),
        )));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

// ── Chart 2A: 24H Half-Block Circadian Sky-Wave & 7D Weekday Pulse ───────────

fn circadian_phase_color(hour: usize) -> Color {
    match hour {
        0..=5 => Color::Rgb(129, 140, 248),   // Midnight Indigo
        6..=11 => Color::Rgb(251, 146, 60),   // Dawn Coral Gold
        12..=17 => Color::Rgb(52, 211, 153),  // Afternoon Emerald
        _ => Color::Rgb(236, 72, 153),        // Prime Neon Magenta
    }
}

fn render_circadian_card(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    theme: &ThemePalette,
) {
    let block = styled_box("◈ 24H CIRCADIAN SKY-WAVE & WEEKDAY PULSE", Color::Rgb(56, 189, 248), theme.border_idle);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let w = inner.width as usize;
    if w < 24 || inner.height < 5 {
        return;
    }

    let max_hr = data.hourly_distribution.iter().copied().max().unwrap_or(0);
    let total_hr: u64 = data.hourly_distribution.iter().sum();
    let peak_val = data.hourly_distribution[data.peak_hour.min(23)];
    let peak_pct = if total_hr > 0 { (peak_val as f64 / total_hr as f64) * 100.0 } else { 0.0 };

    let phase_name = match data.peak_hour {
        0..=5 => "Night Owl (00–05h)",
        6..=11 => "Morning Dawn (06–11h)",
        12..=17 => "Afternoon (12–17h)",
        _ => "Prime Time (18–23h)",
    };

    let mut lines = Vec::new();
    let hdr_left = format!(" Window: {}", phase_name);
    let hdr_right = if total_hr > 0 {
        format!("▼ Peak {:02}h ({:.0}%) ", data.peak_hour, peak_pct)
    } else {
        "Awaiting watch logs ".to_string()
    };
    let hdr_gap = w.saturating_sub(hdr_left.chars().count() + hdr_right.chars().count());
    lines.push(Line::from(vec![
        Span::styled(hdr_left, Style::default().fg(theme.text)),
        Span::raw(" ".repeat(hdr_gap)),
        Span::styled(hdr_right, Style::default().fg(Color::Rgb(251, 191, 36)).add_modifier(Modifier::BOLD)),
    ]));

    // Half-block vertical skyline across 24 hours; column width 2 (or 1 on narrow)
    let col_w = if w >= 50 { 2 } else { 1 };
    let chart_w = 24 * col_w;
    let left_pad = (w.saturating_sub(chart_w)) / 2;
    let wave_rows = inner.height.saturating_sub(4).max(2) as usize;
    let total_sub_levels = (wave_rows * 2) as f64;

    for r in 0..wave_rows {
        let low_sub = ((wave_rows - 1 - r) * 2) as f64;
        let high_sub = low_sub + 1.0;
        let mut row_spans = vec![Span::raw(" ".repeat(left_pad))];

        for (hr, &val) in data.hourly_distribution.iter().enumerate() {
            let phase_col = circadian_phase_color(hr);
            if max_hr == 0 || val == 0 {
                // Subtle baseline dots on bottom row, nocturnal guide at 06h/18h
                if r + 1 == wave_rows && hr % 3 == 0 {
                    row_spans.push(Span::styled(
                        if col_w == 2 { "· " } else { "·" },
                        Style::default().fg(Color::Rgb(58, 62, 85)),
                    ));
                } else {
                    row_spans.push(Span::raw(" ".repeat(col_w)));
                }
                continue;
            }

            let filled = ((val as f64 / max_hr as f64) * total_sub_levels).clamp(0.8, total_sub_levels);
            let sym = if filled >= high_sub + 0.5 {
                "█"
            } else if filled >= low_sub + 0.35 {
                "▄"
            } else {
                " "
            };
            let is_peak = hr == data.peak_hour;
            let style = if is_peak {
                Style::default().fg(phase_col).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(phase_col)
            };
            row_spans.push(Span::styled(sym.repeat(col_w), style));
        }
        lines.push(Line::from(row_spans));
    }

    // Hour axis ticks aligned directly under 00h..23h columns
    let mut axis = String::from(" ".repeat(left_pad));
    if col_w == 2 {
        axis.push_str("00h   03h   06h   09h   12h   15h   18h   21h  23h");
    } else {
        axis.push_str("0  3  6  9  12 15 18 21 ");
    }
    lines.push(Line::from(Span::styled(
        trunc_chars(&axis, w),
        Style::default().fg(Color::Rgb(115, 120, 145)),
    )));

    // Divider & 7-Day Weekday Pulse
    lines.push(Line::from(Span::styled(
        "─".repeat(w),
        Style::default().fg(Color::Rgb(42, 46, 62)),
    )));

    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max_d = data.weekday_distribution.iter().copied().max().unwrap_or(0);
    let mut dow_spans = vec![Span::raw(" ")];
    for (idx, (&name, &val)) in DAYS.iter().zip(data.weekday_distribution.iter()).enumerate() {
        if idx > 0 {
            dow_spans.push(Span::raw(if w >= 50 { "  " } else { " " }));
        }
        let glyph = if val == 0 || max_d == 0 {
            '░'
        } else {
            BARS[((val as f64 / max_d as f64) * 7.0).round().clamp(0.0, 7.0) as usize]
        };
        let col = if val == 0 {
            Color::Rgb(62, 66, 88)
        } else if val == max_d {
            Color::Rgb(80, 250, 123)
        } else {
            theme.primary
        };
        let short_day = if w >= 46 { name } else { &name[..2] };
        dow_spans.push(Span::styled(format!("{} ", short_day), Style::default().fg(theme.text_dim)));
        dow_spans.push(Span::styled(glyph.to_string(), Style::default().fg(col).add_modifier(Modifier::BOLD)));
    }
    lines.push(Line::from(dow_spans));

    frame.render_widget(Paragraph::new(lines), inner);
}

// ── Chart 2B: Adaptive N-Week × 7-Day Thermal Matrix + Sidebar ───────────────

fn thermal_cell_style(count: u64) -> (&'static str, Color) {
    match count {
        0 => ("░", Color::Rgb(34, 38, 56)),
        1 => ("▫", Color::Rgb(38, 122, 98)),
        2..=3 => ("▒", Color::Rgb(16, 185, 129)),
        4..=6 => ("▓", Color::Rgb(56, 189, 248)),
        _ => ("█", Color::Rgb(251, 191, 36)),
    }
}

fn render_thermal_heatmap_card(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    theme: &ThemePalette,
) {
    let sidebar_w: usize = if area.width >= 48 { 17 } else { 0 };
    let grid_avail_w = area.width.saturating_sub(sidebar_w as u16 + 5) as usize;
    let weeks = (grid_avail_w / 2).clamp(8, 26);

    let title = format!("◈ {}-DAY THERMAL MATRIX ({}W × 7D)", weeks * 7, weeks);
    let block = styled_box(&title, Color::Rgb(80, 250, 123), theme.border_idle);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let w = inner.width as usize;
    if w < 20 || inner.height < 5 {
        return;
    }

    // Build calendar-aligned Mon..Sun columns ending cleanly on today_weekday_idx
    let len = data.thermal_activity.len();
    let total_cells = (weeks - 1) * 7 + (data.today_weekday_idx.min(6) + 1);
    let slice_start = len.saturating_sub(total_cells);
    let window_days = data.thermal_activity.get(slice_start..).unwrap_or(&[]);
    let vol_window: u64 = window_days.iter().sum();
    let peak_window = window_days.iter().copied().max().unwrap_or(0);

    let focus_day = data
        .weekday_distribution
        .iter()
        .enumerate()
        .max_by_key(|&(_, v)| v)
        .filter(|&(_, &v)| v > 0)
        .map(|(i, _)| ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][i])
        .unwrap_or("Quiet");

    let mut lines = Vec::new();
    let hdr = format!(
        " {}w Vol: {} ep │ Peak: {}/d │ Focus: {}",
        weeks, vol_window, peak_window, focus_day
    );
    lines.push(Line::from(Span::styled(
        trunc_chars(&hdr, w),
        Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD),
    )));

    // Sidebar stats paired cleanly with the 7 day rows
    let spark_14 = mini_spark_str(&data.recent_activity);
    let today_ep = data.recent_activity.last().copied().unwrap_or(0);
    let delta_lbl = match data.velocity_delta_pct {
        Some(p) if p >= 0.0 => format!("+{:.0}% vs-14d", p),
        Some(p) => format!("{:.0}% vs-14d", p),
        None => "New Horizon".to_string(),
    };
    let sidebar_rows = [
        ("14d Sparkline:".to_string(), Color::Rgb(130, 135, 160)),
        (spark_14, Color::Rgb(56, 189, 248)),
        (format!("Streak:   {:>3}d", data.active_streak_days), theme.accent),
        (format!("Best:     {:>3}d", data.longest_streak_days), Color::Rgb(251, 191, 36)),
        (format!("Wknd:     {:>3.0}%", data.weekend_share_pct), theme.secondary),
        (format!("Today:   +{:<2}ep", today_ep), Color::Rgb(80, 250, 123)),
        (delta_lbl, theme.text_dim),
    ];

    const ROW_LABELS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];
    for (dow, &lbl) in ROW_LABELS.iter().enumerate() {
        let mut row_spans = vec![Span::styled(
            format!(" {} ", lbl),
            Style::default().fg(if dow >= 5 { Color::Rgb(160, 165, 190) } else { Color::Rgb(105, 110, 135) }),
        )];
        for wk in 0..weeks {
            let cell_offset = wk * 7 + dow;
            if cell_offset < window_days.len() {
                let cnt = window_days[cell_offset];
                let (sym, col) = thermal_cell_style(cnt);
                row_spans.push(Span::styled(format!("{} ", sym), Style::default().fg(col)));
            } else {
                row_spans.push(Span::raw("  "));
            }
        }
        if sidebar_w > 0 {
            let (s_txt, s_col) = &sidebar_rows[dow];
            row_spans.push(Span::styled("│ ", Style::default().fg(Color::Rgb(50, 54, 74))));
            row_spans.push(Span::styled(
                trunc_chars(s_txt, sidebar_w.saturating_sub(2)),
                Style::default().fg(*s_col),
            ));
        }
        lines.push(Line::from(row_spans));
    }

    // Bottom month axis + heat legend
    if inner.height >= 9 {
        let mut mon_chars = vec![' '; weeks * 2];
        for (abs_idx, abbr) in &data.thermal_month_labels {
            if *abs_idx >= slice_start {
                let rel_day = abs_idx - slice_start;
                let wk_col = (rel_day / 7) * 2;
                if wk_col + 3 <= mon_chars.len() {
                    for (ci, ch) in abbr.chars().take(3).enumerate() {
                        mon_chars[wk_col + ci] = ch;
                    }
                }
            }
        }
        let mon_axis: String = mon_chars.into_iter().collect();
        let mut axis_spans = vec![
            Span::raw("   "),
            Span::styled(mon_axis, Style::default().fg(Color::Rgb(115, 120, 145))),
        ];
        if sidebar_w > 0 {
            axis_spans.push(Span::styled("│ ", Style::default().fg(Color::Rgb(50, 54, 74))));
            axis_spans.push(Span::styled("░", Style::default().fg(Color::Rgb(45, 50, 68))));
            axis_spans.push(Span::styled("0 ", Style::default().fg(theme.text_dim)));
            axis_spans.push(Span::styled("▫", Style::default().fg(Color::Rgb(38, 122, 98))));
            axis_spans.push(Span::styled("1 ", Style::default().fg(theme.text_dim)));
            axis_spans.push(Span::styled("▒", Style::default().fg(Color::Rgb(16, 185, 129))));
            axis_spans.push(Span::styled("3 ", Style::default().fg(theme.text_dim)));
            axis_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(251, 191, 36))));
            axis_spans.push(Span::styled("7+", Style::default().fg(theme.text_dim)));
        }
        lines.push(Line::from(axis_spans));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

// ── Chart 2C: 30d Cumulative Braille Spline & Score Calibration ───────────────

fn render_growth_and_scores_card(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    theme: &ThemePalette,
) {
    let block = styled_box("✦ GROWTH SPLINE & SCORE CALIBRATION", theme.accent, theme.border_idle);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 26 || inner.height < 4 {
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(56), Constraint::Percentage(44)])
        .split(inner);

    // Left: Cumulative Hours Braille Curve
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(2), Constraint::Length(1)])
        .split(cols[0]);

    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" 30d Cumulative: {:.1}h", data.total_hours_watched),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        left_chunks[0],
    );

    let pts = &data.cumulative_hours_30d;
    let min_h = pts.iter().copied().fold(f64::INFINITY, f64::min).min(0.0);
    let max_h = pts.iter().copied().fold(0.5f64, f64::max).max(1.0);
    let points_owned = pts.clone();
    let line_col = theme.primary;
    let dot_col = Color::Rgb(80, 250, 123);

    let spline = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, (points_owned.len().saturating_sub(1).max(1)) as f64])
        .y_bounds([min_h, max_h * 1.1])
        .paint(move |ctx| {
            // Subtle horizon reference
            ctx.draw(&CanvasLine {
                x1: 0.0,
                y1: min_h,
                x2: (points_owned.len().saturating_sub(1).max(1)) as f64,
                y2: min_h,
                color: Color::Rgb(50, 54, 72),
            });
            for win in points_owned.windows(2).enumerate() {
                let (i, w_pts) = win;
                ctx.draw(&CanvasLine {
                    x1: i as f64,
                    y1: w_pts[0],
                    x2: (i + 1) as f64,
                    y2: w_pts[1],
                    color: line_col,
                });
            }
            if let Some(&last_y) = points_owned.last() {
                let last_x = points_owned.len().saturating_sub(1) as f64;
                ctx.draw(&Points {
                    coords: &[(last_x, last_y)],
                    color: dot_col,
                });
            }
        });
    frame.render_widget(spline, left_chunks[1]);

    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" 0h ──── -15d ──── Today ({:.1}h)", data.total_hours_watched),
            Style::default().fg(Color::Rgb(110, 115, 138)),
        )),
        left_chunks[2],
    );

    // Right: Score Calibration Histogram
    let rw = cols[1].width as usize;
    let max_b = data.score_buckets.iter().copied().max().unwrap_or(1).max(1) as f64;
    let bar_w = rw.saturating_sub(14).clamp(4, 14);
    let labels = ["90+", "80+", "70+", "<70"];
    let colors: [GradientStop; 4] = [
        ((251, 191, 36), (80, 250, 123)),
        ((56, 189, 248), (129, 140, 248)),
        ((168, 85, 247), (236, 72, 153)),
        ((100, 116, 139), (148, 163, 184)),
    ];

    let mut score_lines = vec![Line::from(Span::styled(
        "│ Score Calibration",
        Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD),
    ))];
    for (idx, (&lbl, &cnt)) in labels.iter().zip(data.score_buckets.iter()).enumerate() {
        if score_lines.len() >= cols[1].height as usize {
            break;
        }
        let fill = ((cnt as f64 / max_b) * bar_w as f64).round() as usize;
        let mut row = vec![
            Span::styled("│ ", Style::default().fg(Color::Rgb(50, 54, 74))),
            Span::styled(format!("{:<4}", lbl), Style::default().fg(theme.text)),
        ];
        row.extend(render_gradient_bar(fill, bar_w, colors[idx].0, colors[idx].1));
        row.push(Span::styled(format!(" {:>2}s", cnt), Style::default().fg(theme.text_dim)));
        score_lines.push(Line::from(row));
    }
    if score_lines.len() < cols[1].height as usize {
        let mean_str = if data.avg_score > 0.0 {
            format!("│ Mean: {:.1} ★", data.avg_score)
        } else {
            "│ Mean: Unscored".to_string()
        };
        score_lines.push(Line::from(Span::styled(
            mean_str,
            Style::default().fg(Color::Rgb(251, 191, 36)),
        )));
    }
    frame.render_widget(Paragraph::new(score_lines), cols[1]);
}

// ── Chart 3A: #1 Crowned Obsession Cover Spotlight + Episode Dot Matrix ──────

fn format_episode_dots(watched: usize, total: Option<usize>) -> String {
    match total {
        Some(t) if t > 0 && t <= 26 => {
            let done = watched.min(t);
            let mut s = String::with_capacity(t + 4);
            for i in 0..t {
                if i == 13 {
                    s.push(' ');
                }
                s.push(if i < done { '●' } else { '○' });
            }
            s
        }
        Some(t) if t > 26 => {
            let cells = 18;
            let done = (((watched as f64 / t as f64) * cells as f64).round() as usize).min(cells);
            format!("{}{}", "●".repeat(done), "○".repeat(cells - done))
        }
        _ => "●".repeat(watched.clamp(1, 16)),
    }
}

fn render_podium_spotlight_card(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    state: &mut AppState,
    theme: &ThemePalette,
) {
    let idx = if data.top_shows.is_empty() {
        0
    } else {
        state.analytics_podium_idx % data.top_shows.len()
    };
    let rank_badge = match idx {
        0 => "#1 CROWNED OBSESSION",
        1 => "#2 PODIUM RUNNER-UP",
        2 => "#3 HEAVY ROTATION",
        _ => "❖ SPOTLIGHT SERIES",
    };
    let block = styled_box(rank_badge, Color::Rgb(251, 191, 36), theme.border_idle);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(show) = data.top_shows.get(idx) else {
        let empty = vec![
            Line::from(""),
            Line::from(Span::styled(
                " No watched episodes yet.",
                Style::default().fg(theme.text_dim),
            )),
            Line::from(Span::styled(
                " Watch an anime from Home to unlock spotlight telemetry!",
                Style::default().fg(Color::Rgb(110, 115, 138)),
            )),
        ];
        frame.render_widget(Paragraph::new(empty), inner);
        return;
    };

    let main_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(2)])
        .split(inner);

    let art_w = if main_rows[0].width >= 34 { 12 } else { 0 };
    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(art_w), Constraint::Min(14)])
        .split(main_rows[0]);

    if art_w > 0 {
        let art_frame = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.primary));
        let art_inner = art_frame.inner(top_cols[0]);
        frame.render_widget(art_frame, top_cols[0]);

        let rendered_image = if state.has_image_support() {
            if let Some(proto) = state
                .detail_cache
                .get_mut(&show.id)
                .or_else(|| state.cover_cache.get_mut(&show.id))
            {
                let img = StatefulImage::new(None).resize(Resize::Fit(None));
                frame.render_stateful_widget(img, art_inner, proto);
                true
            } else {
                false
            }
        } else {
            false
        };

        if !rendered_image {
            let initials: String = show
                .title
                .split_whitespace()
                .filter_map(|w| w.chars().next())
                .take(3)
                .collect();
            let placeholder = Paragraph::new(vec![
                Line::from(Span::styled("◈", Style::default().fg(theme.primary))),
                Line::from(Span::styled(
                    initials,
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled("COVER", Style::default().fg(theme.text_dim))),
            ])
            .alignment(Alignment::Center);
            frame.render_widget(placeholder, art_inner);
        }
    }

    // Metadata beside Cover Art
    let info_w = top_cols[1].width as usize;
    let bar_w = info_w.saturating_sub(7).clamp(6, 22);
    let fill = ((show.completion_pct / 100.0) * bar_w as f64).round() as usize;
    let (p_s, p_e) = if show.completion_pct >= 99.0 {
        ((245, 158, 11), (16, 185, 129))
    } else {
        ((6, 182, 212), (168, 85, 247))
    };
    let mut bar_spans = vec![Span::raw(" ")];
    bar_spans.extend(render_gradient_bar(fill, bar_w, p_s, p_e));
    bar_spans.push(Span::styled(format!(" {:>3.0}%", show.completion_pct), Style::default().fg(theme.text)));

    let year_s = show.season_year.map(|y| y.to_string()).unwrap_or_else(|| "----".to_string());
    let score_s = show.score.map(|s| format!("★ {}", s)).unwrap_or_else(|| "★ --".to_string());
    let avg_m = if show.episodes_watched > 0 {
        ((show.total_hours * 60.0) / show.episodes_watched as f64).round() as u64
    } else {
        24
    };
    let ep_str = match show.total_episodes {
        Some(t) => format!(" {}/{} ep · {:.1}h · {}m/ep", show.episodes_watched, t, show.total_hours, avg_m),
        None => format!(" {} ep · {:.1}h · {}m/ep", show.episodes_watched, show.total_hours, avg_m),
    };
    let dots = format_episode_dots(show.episodes_watched, show.total_episodes);
    let rem_str = match show.total_episodes {
        Some(t) if t > show.episodes_watched => format!(" {} left", t - show.episodes_watched),
        Some(_) => " Complete".to_string(),
        None => String::new(),
    };
    let genres_joined = if show.genres.is_empty() {
        "Anime Series".to_string()
    } else {
        show.genres.iter().take(3).cloned().collect::<Vec<_>>().join(" · ")
    };

    let info_lines = vec![
        Line::from(Span::styled(
            format!(" {}", trunc_chars(&show.title, info_w.saturating_sub(1))),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" {} · {} · {}", year_s, show.format, score_s),
            Style::default().fg(theme.accent),
        )),
        Line::from(bar_spans),
        Line::from(Span::styled(
            trunc_chars(&ep_str, info_w),
            Style::default().fg(theme.text_dim),
        )),
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(trunc_chars(&dots, info_w.saturating_sub(rem_str.len() + 2)), Style::default().fg(Color::Rgb(80, 250, 123))),
            Span::styled(rem_str, Style::default().fg(theme.text_dim)),
        ]),
        Line::from(Span::styled(
            format!(" {}", trunc_chars(&genres_joined, info_w.saturating_sub(1))),
            Style::default().fg(Color::Rgb(145, 150, 175)),
        )),
    ];
    frame.render_widget(Paragraph::new(info_lines), top_cols[1]);

    // Bottom status & cycling hint
    let canon_note = match show.canon_ratio_pct {
        Some(c) => format!("{:.0}% Canon Mix", c),
        None => "Local SQLite Indexed".to_string(),
    };
    let status_lbl = if show.completion_pct >= 99.0 { "Completed" } else { "In Rotation" };
    let nav_hint = if data.top_shows.len() > 1 {
        format!("[←/→] Cycle #1–#{}  [Enter] Detail", data.top_shows.len().min(6))
    } else {
        format!("Next Up: Ep {:02}  [Enter] Detail", show.episodes_watched + 1)
    };
    let footer_lines = vec![
        Line::from(Span::styled(
            format!(" Status: {} · {}", status_lbl, canon_note),
            Style::default().fg(Color::Rgb(120, 125, 150)),
        )),
        Line::from(Span::styled(
            format!(" {}", nav_hint),
            Style::default().fg(theme.primary),
        )),
    ];
    frame.render_widget(Paragraph::new(footer_lines), main_rows[1]);
}

// ── Chart 3B: Rotation & Promoted Watchlist Horizon (Zero Fake Ghosts) ───────

fn render_rotation_horizon_card(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    list_scroll: usize,
    theme: &ThemePalette,
) {
    let title = if data.top_shows.len() < 3 && !data.watchlist_queue.is_empty() {
        "❖ WATCHLIST HORIZON (Promoted Queue)"
    } else {
        "❖ ROTATION & WATCHLIST HORIZON"
    };
    let block = styled_box(title, theme.accent, theme.border_idle);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let w = inner.width as usize;
    let h = inner.height as usize;
    if w < 18 || h < 3 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Show active watched series #2.. (or #1 summary if only 1 show and queue below)
    let start_idx = if data.top_shows.len() > 1 { 1 } else { 0 };
    let max_active_rows = if data.watchlist_queue.is_empty() { 4 } else { 3 };
    for (offset, show) in data.top_shows.iter().skip(start_idx).take(max_active_rows).enumerate() {
        let rank = start_idx + offset + 1;
        let bar_w = 8usize;
        let fill = ((show.completion_pct / 100.0) * bar_w as f64).round() as usize;
        let title_budget = w.saturating_sub(bar_w + 13);
        let mut r1 = vec![
            Span::styled(format!(" {}. ", rank), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{:<width$}", trunc_chars(&show.title, title_budget), width = title_budget),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::raw("["),
        ];
        r1.extend(render_gradient_bar(fill, bar_w, (6, 182, 212), (168, 85, 247)));
        r1.push(Span::styled(format!("] {:>3.0}%", show.completion_pct), Style::default().fg(theme.text_dim)));
        lines.push(Line::from(r1));

        let g = show.genres.first().map(|s| s.as_str()).unwrap_or("Anime");
        let ep_tot = show.total_episodes.map(|t| format!("/{}", t)).unwrap_or_default();
        let meta = format!("    {}{} ep · {:.1}h · {}", show.episodes_watched, ep_tot, show.total_hours, g);
        lines.push(Line::from(Span::styled(
            trunc_chars(&meta, w),
            Style::default().fg(Color::Rgb(115, 120, 145)),
        )));
    }

    // Watchlist Horizon entries (zero fake placeholders!)
    if !data.watchlist_queue.is_empty() {
        let hdr_txt = format!(
            "── WATCHLIST ({} shows · {} ep) ",
            data.total_watchlist_shows, data.total_watchlist_eps
        );
        let pad_dashes = "─".repeat(w.saturating_sub(hdr_txt.chars().count()));
        lines.push(Line::from(Span::styled(
            trunc_chars(&format!("{}{}", hdr_txt, pad_dashes), w),
            Style::default().fg(theme.secondary),
        )));

        for item in &data.watchlist_queue {
            let ep_tag = match item.remaining_episodes {
                Some(e) => format!("{:>2}ep (~{:.0}h)", e, item.est_hours),
                None => format!("~{:.0}h", item.est_hours),
            };
            let t_budget = w.saturating_sub(ep_tag.len() + 5);
            lines.push(Line::from(vec![
                Span::styled(" ▸ ", Style::default().fg(theme.accent)),
                Span::styled(
                    format!("{:<width$} ", trunc_chars(&item.title, t_budget), width = t_budget),
                    Style::default().fg(theme.text),
                ),
                Span::styled(ep_tag, Style::default().fg(Color::Rgb(120, 125, 150))),
            ]));
        }
        let total_h: f64 = data.watchlist_queue.iter().map(|i| i.est_hours).sum();
        lines.push(Line::from(Span::styled(
            trunc_chars(
                &format!(" Backlog Horizon: ~{:.1}h across {} queued titles", total_h, data.total_watchlist_shows),
                w,
            ),
            Style::default().fg(Color::Rgb(100, 105, 130)),
        )));
    } else if data.top_shows.is_empty() {
        lines.push(Line::from(Span::styled(
            " Watchlist is empty. Press [w] for Tailored Picks!",
            Style::default().fg(theme.text_dim),
        )));
    }

    // Chronological watch feed below
    if !data.recent_watch_log.is_empty() {
        let hdr_txt = "── RECENTLY LOGGED ";
        let pad_dashes = "─".repeat(w.saturating_sub(hdr_txt.chars().count()));
        lines.push(Line::from(Span::styled(
            trunc_chars(&format!("{}{}", hdr_txt, pad_dashes), w),
            Style::default().fg(theme.primary),
        )));
        for entry in &data.recent_watch_log {
            let title_b = w.saturating_sub(22);
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", entry.formatted_time), Style::default().fg(Color::Rgb(105, 110, 135))),
                Span::styled(format!("E{:02} ", entry.episode), Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
                Span::styled(trunc_chars(&entry.title, title_b), Style::default().fg(theme.text)),
            ]));
        }
    }

    let max_skip = lines.len().saturating_sub(h);
    let visible: Vec<Line<'_>> = lines.into_iter().skip(list_scroll.min(max_skip)).take(h).collect();
    frame.render_widget(Paragraph::new(visible), inner);
}

// ── Chart 3C: Auto-Tiered Milestones & Live Runtime Diagnostics ──────────────

fn next_tier(val: usize, tiers: &[usize]) -> usize {
    tiers.iter().copied().find(|&t| t > val).unwrap_or(tiers[tiers.len() - 1] * 2)
}

fn render_milestones_diagnostics_card(
    frame: &mut Frame,
    area: Rect,
    data: &UserAnalytics,
    state: &AppState,
    theme: &ThemePalette,
) {
    let block = styled_box("✦ TIERED MILESTONES & DIAGNOSTICS", theme.primary, theme.border_idle);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let w = inner.width as usize;
    if w < 18 || inner.height < 4 {
        return;
    }

    let ep_goal = next_tier(data.total_episodes_watched, &[12, 24, 50, 100, 250, 500, 1000, 2500]);
    let hr_val = data.total_hours_watched.round() as usize;
    let hr_goal = next_tier(hr_val, &[5, 10, 25, 50, 100, 200, 500, 1000]);
    let st_goal = next_tier(data.active_streak_days, &[3, 7, 14, 30, 60, 100]);

    let bar_w = w.saturating_sub(28).clamp(4, 14);
    let milestones = [
        (format!("Ep Tier ({})", ep_goal), (data.total_episodes_watched as f64 / ep_goal as f64) * 100.0, ((245, 158, 11), (80, 250, 123))),
        (format!("Hours ({}h)", hr_goal), (data.total_hours_watched / hr_goal as f64) * 100.0, ((6, 182, 212), (168, 85, 247))),
        (format!("Streak ({}d)", st_goal), (data.active_streak_days as f64 / st_goal as f64) * 100.0, ((236, 72, 153), (251, 191, 36))),
    ];

    let mut lines = Vec::new();
    for (lbl, raw_pct, (s_rgb, e_rgb)) in milestones {
        let pct = raw_pct.clamp(0.0, 100.0);
        let fill = ((pct / 100.0) * bar_w as f64).round() as usize;
        let mut row = vec![
            Span::styled(format!(" ▸ {:<17}", trunc_chars(&lbl, 17)), Style::default().fg(theme.text)),
        ];
        row.extend(render_gradient_bar(fill, bar_w, s_rgb, e_rgb));
        row.push(Span::styled(format!(" {:>3.0}%", pct), Style::default().fg(theme.text_dim)));
        lines.push(Line::from(row));
    }

    if inner.height >= 5 {
        lines.push(Line::from(Span::styled(
            "─".repeat(w),
            Style::default().fg(Color::Rgb(42, 46, 62)),
        )));
    }

    // Dynamic computed insight bullets (100% faithful to actual user data & config)
    let dom_era = data
        .era_distribution
        .iter()
        .max_by_key(|(_, c, _)| *c)
        .filter(|(_, c, _)| *c > 0)
        .map(|(l, _, p)| format!("{:.0}% {}", p, l.split('(').next().unwrap_or(l).trim()))
        .unwrap_or_else(|| "Fresh Catalog".to_string());
    let g_lead = data.top_genres.first().map(|(g, _, p)| format!("{} ({:.0}%)", g, p)).unwrap_or_else(|| "Diverse".to_string());
    let (player, shader) = active_pipeline_labels(state);
    let canon_note = match data.weighted_canon_pct {
        Some(c) => format!("{:.0}% Canon", c),
        None => format!("{} Offline Ep", data.offline_cached_episodes),
    };

    let bullets = [
        format!("▸ {} · Lead Genre: {}", dom_era, g_lead),
        format!("▸ Peak {:02}:00 Window · {}", data.peak_hour, canon_note),
        format!("▸ {} · {} · {}", player, shader, format_bytes(data.offline_cached_bytes)),
    ];
    for b in bullets {
        if lines.len() >= inner.height as usize {
            break;
        }
        lines.push(Line::from(Span::styled(
            format!(" {}", trunc_chars(&b, w.saturating_sub(1))),
            Style::default().fg(Color::Rgb(120, 125, 150)),
        )));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

// ── 4. Footer Hints ──────────────────────────────────────────────────────────

fn render_footer(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    theme: &ThemePalette,
) {
    let items: &[(&str, &str, Color)] = &[
        ("[Esc/q/a]", "Home", theme.primary),
        ("[←/→]", "Cycle Cover", theme.accent),
        ("[Enter]", "Detail", Color::Rgb(80, 250, 123)),
        ("[↑/↓]", "Scroll", Color::Rgb(56, 189, 248)),
        ("[r]", "Refresh", Color::Rgb(251, 191, 36)),
        ("[t]", "Theme", Color::Rgb(189, 147, 249)),
        ("[s]", "Settings", Color::Rgb(251, 113, 133)),
        ("[?]", "Help", Color::Rgb(148, 163, 184)),
    ];
    let _ = state;
    let mut spans = vec![Span::raw(" ")];
    for (i, &(k, lbl, col)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(format!("{} ", k), Style::default().fg(col).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(lbl, Style::default().fg(theme.text)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::user::{RecentWatchEntry, TopWatchedAnime, WatchlistQueueItem};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn sample_rich_analytics() -> UserAnalytics {
        let mut hourly = [0u64; 24];
        hourly[15] = 4;
        hourly[21] = 12;
        hourly[22] = 8;

        UserAnalytics {
            total_episodes_watched: 42,
            total_hours_watched: 17.2,
            completed_shows: 2,
            in_progress_shows: 2,
            completion_rate: 50.0,
            avg_score: 85.4,
            top_genres: vec![
                ("Sci-Fi".to_string(), 24, 32.0),
                ("Psychological".to_string(), 18, 24.0),
                ("Cyberpunk".to_string(), 14, 18.0),
                ("Drama".to_string(), 12, 16.0),
                ("Mystery".to_string(), 8, 10.0),
            ],
            era_distribution: vec![
                ("Pre-2000 (Golden Age)", 1, 25.0),
                ("2000s (Classic Era)", 1, 25.0),
                ("2010s (Modern Era)", 1, 25.0),
                ("2020s (Current Era)", 1, 25.0),
            ],
            format_distribution: vec![("TV".to_string(), 3), ("ONA".to_string(), 1)],
            recent_activity: vec![0, 0, 1, 0, 2, 0, 3, 1, 0, 4, 2, 0, 6, 3],
            taste_archetype: "✦ Cyberpunk Visionary & Futurist",
            top_shows: vec![
                TopWatchedAnime {
                    id: 1,
                    title: "Steins;Gate".to_string(),
                    episodes_watched: 24,
                    total_episodes: Some(24),
                    total_hours: 9.8,
                    completion_pct: 100.0,
                    season_year: Some(2011),
                    format: "TV".to_string(),
                    score: Some(91),
                    genres: vec!["Sci-Fi".into(), "Psychological".into()],
                    canon_ratio_pct: Some(100.0),
                    last_watched_at: 2_000,
                },
                TopWatchedAnime {
                    id: 2,
                    title: "Serial Experiments Lain".to_string(),
                    episodes_watched: 13,
                    total_episodes: Some(13),
                    total_hours: 5.2,
                    completion_pct: 100.0,
                    season_year: Some(1998),
                    format: "TV".to_string(),
                    score: Some(80),
                    genres: vec!["Cyberpunk".into(), "Psychological".into()],
                    canon_ratio_pct: None,
                    last_watched_at: 1_500,
                },
            ],
            viewing_cadence: "◈ Prime Time (18:00 - 22:00)",
            active_streak_days: 4,
            audio_preference: "Japanese Sub",
            sub_pct: 85.0,
            dub_pct: 15.0,
            hourly_distribution: hourly,
            peak_hour: 21,
            weekday_distribution: [1, 2, 0, 3, 5, 8, 4],
            weekend_share_pct: 52.0,
            thermal_activity: vec![1; 182],
            thermal_month_labels: vec![(0, "Jun".into()), (30, "Jul".into()), (120, "Oct".into())],
            today_weekday_idx: 5,
            longest_streak_days: 7,
            cumulative_hours_30d: (0..30).map(|i| i as f64 * 0.6).collect(),
            recent_7d_hours: 4.4,
            velocity_delta_pct: Some(25.0),
            season_distribution: vec![("Winter", 1, 25.0), ("Spring", 2, 50.0), ("Summer", 1, 25.0), ("Fall", 0, 0.0)],
            weighted_median_year: Some(2009),
            year_span_label: "1998–2024 Catalog".to_string(),
            score_buckets: [1, 2, 1, 0],
            watchlist_queue: vec![WatchlistQueueItem {
                id: 3,
                title: "Monster".to_string(),
                remaining_episodes: Some(74),
                est_hours: 29.6,
                completion_pct: 0.0,
                primary_genre: "Seinen".to_string(),
            }],
            total_watchlist_shows: 1,
            total_watchlist_eps: 74,
            offline_cached_episodes: 3,
            offline_cached_bytes: 512 * 1024 * 1024,
            recent_watch_log: vec![RecentWatchEntry {
                anime_id: 1,
                title: "Steins;Gate".to_string(),
                episode: 24,
                watched_at: 2_000,
                formatted_time: "11-14 21:00".to_string(),
            }],
            weighted_canon_pct: Some(100.0),
        }
    }

    #[test]
    fn test_render_bento_overview_single_screen() {
        let backend = TestBackend::new(154, 42);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.analytics_data = Some(sample_rich_analytics());

        // Verify single-screen Overview Bento renders cleanly on both 165x50 and 154x42
        let backend_w = TestBackend::new(165, 50);
        let mut term_w = Terminal::new(backend_w).unwrap();
        term_w.draw(|f| render(f, &mut state)).unwrap();
        let text_w: String = term_w.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(text_w.contains("SOLUS // ANIME WRAPPED"));
        assert!(text_w.contains("GENRE RADAR POLARITY"));
        assert!(text_w.contains("24H CIRCADIAN SKY-WAVE"));
        assert!(text_w.contains("THERMAL MATRIX"));
        assert!(text_w.contains("#1 CROWNED OBSESSION"));
        assert!(text_w.contains("WATCHLIST HORIZON"));
        assert!(!text_w.contains("Uncharted Rotation Slot"));
        assert!(!text_w.contains("[2 GENRE DNA]"));
        assert!(!text_w.contains("[3 TIMELINE]"));
        assert!(!text_w.contains("[4 QUEUE]"));

        terminal.draw(|f| render(f, &mut state)).unwrap();
        let text: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("GENRE RADAR POLARITY"));
        assert!(text.contains("24H CIRCADIAN SKY-WAVE"));
        assert!(text.contains("THERMAL MATRIX"));
        assert!(text.contains("#1 CROWNED OBSESSION"));
        assert!(text.contains("Steins;Gate"));
        assert!(text.contains("Monster"));
    }

    #[test]
    fn test_radar_full_labels_and_intact_hexagon_bottom() {
        let mut state = AppState::new();
        let mut a = sample_rich_analytics();
        a.top_genres = vec![
            ("Cyberpunk".to_string(), 10, 14.0),
            ("Drama".to_string(), 10, 14.0),
            ("Mystery".to_string(), 10, 14.0),
            ("Philosophical".to_string(), 10, 14.0),
            ("Psychological".to_string(), 10, 14.0),
            ("Sci-Fi".to_string(), 10, 14.0),
        ];
        state.analytics_data = Some(a);

        let backend = TestBackend::new(165, 50);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut state)).unwrap();
        let buf = term.backend().buffer();

        let mut rows_txt = Vec::with_capacity(50);
        for y in 0..50u16 {
            let mut row = String::new();
            for x in 0..56u16 {
                row.push_str(buf[(x, y)].symbol());
            }
            rows_txt.push(row);
        }
        let col1_text = rows_txt.join("\n");

        // All 6 long genre names must appear in full without ellipsis truncation
        assert!(col1_text.contains("Cyberpunk 14%"), "Cyberpunk cut off:\n{}", col1_text);
        assert!(col1_text.contains("Drama 14%"), "Drama cut off:\n{}", col1_text);
        assert!(col1_text.contains("Mystery 14%"), "Mystery cut off:\n{}", col1_text);
        assert!(col1_text.contains("Philosophical 14%"), "Philosophical cut off:\n{}", col1_text);
        assert!(col1_text.contains("Psychological 14%"), "Psychological cut off:\n{}", col1_text);
        assert!(col1_text.contains("Sci-Fi 14%"), "Sci-Fi cut off:\n{}", col1_text);
        assert!(!col1_text.contains("Cyberp…"));
        assert!(!col1_text.contains("Psycho…"));
        assert!(!col1_text.contains("Philos…"));

        // Verify the row strictly above "Philosophical 14%" still contains the bottom vertex Braille dot
        let top_lbl_y = rows_txt.iter().position(|r| r.contains("Cyberpunk 14%")).unwrap();
        let bot_lbl_y = rows_txt.iter().position(|r| r.contains("Philosophical 14%")).unwrap();
        assert!(bot_lbl_y > top_lbl_y + 4);

        let accent_col = state.theme().accent;
        let has_braille_on_row = |ry: usize, only_accent: bool| -> bool {
            (2..50u16).any(|x| {
                let cell = &buf[(x, ry as u16)];
                let is_braille = cell.symbol().chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c) && c != '\u{2800}');
                is_braille && (!only_accent || cell.fg == accent_col)
            })
        };
        assert!(has_braille_on_row(top_lbl_y + 1, true), "Top vertex missing on row {}", top_lbl_y + 1);
        assert!(has_braille_on_row(bot_lbl_y - 1, true), "Bottom vertex missing on row {}\n{}", bot_lbl_y - 1, col1_text);
    }

    #[test]
    fn test_render_day1_single_show_without_ghost_slots() {
        let backend = TestBackend::new(154, 42);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        let mut a = UserAnalytics::default();
        a.total_episodes_watched = 1;
        a.total_hours_watched = 1.2;
        a.in_progress_shows = 1;
        a.avg_score = 80.0;
        a.taste_archetype = "✦ High-Tech Neon Dystopian";
        a.era_distribution = vec![
            ("Pre-2000 (Golden Age)", 1, 100.0),
            ("2000s (Classic Era)", 0, 0.0),
            ("2010s (Modern Era)", 0, 0.0),
            ("2020s (Current Era)", 0, 0.0),
        ];
        a.format_distribution = vec![("TV".into(), 1)];
        a.top_shows = vec![TopWatchedAnime {
            id: 99,
            title: "Serial Experiments Lain".into(),
            episodes_watched: 1,
            total_episodes: Some(13),
            total_hours: 1.2,
            completion_pct: 8.0,
            season_year: Some(1998),
            format: "TV".into(),
            score: Some(80),
            genres: vec!["Cyberpunk".into(), "Psychological".into(), "Sci-Fi".into()],
            canon_ratio_pct: None,
            last_watched_at: 1_000,
        }];
        a.watchlist_queue = vec![WatchlistQueueItem {
            id: 100,
            title: "Texhnolyze".into(),
            remaining_episodes: Some(22),
            est_hours: 8.8,
            completion_pct: 0.0,
            primary_genre: "Cyberpunk".into(),
        }];
        a.total_watchlist_shows = 1;
        a.total_watchlist_eps = 22;
        state.analytics_data = Some(a);

        terminal.draw(|f| render(f, &mut state)).unwrap();
        let text: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(!text.contains("Uncharted Rotation Slot"));
        assert!(!text.contains("Modern & Current Era"));
        assert!(text.contains("Pre-2000"));
        assert!(text.contains("Texhnolyze"));
    }

    #[test]
    fn test_render_compact_screen_no_panic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.analytics_data = Some(UserAnalytics::default());
        assert!(terminal.draw(|f| render(f, &mut state)).is_ok());
    }
}
