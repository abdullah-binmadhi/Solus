//! Dependency doctor & interactive playback pipeline overlay (`!`).

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::{
    config::{self, Anime4KMode, AudioMode, Player},
    state::AppState,
    ui::theme::ThemePalette,
};

const BG_MODAL: Color = Color::Rgb(15, 16, 25);
const BG_CARD: Color = Color::Rgb(19, 20, 32);
const BG_CARD_ALT: Color = Color::Rgb(22, 24, 38);
const BG_PILL: Color = Color::Rgb(30, 32, 50);
const FG_PRIMARY: Color = Color::Rgb(235, 238, 250);
const FG_MUTED: Color = Color::Rgb(150, 155, 180);
const COLOR_OK: Color = Color::Rgb(80, 250, 123);
const COLOR_WARN: Color = Color::Rgb(255, 184, 108);
const COLOR_ERR: Color = Color::Rgb(255, 95, 95);

/// Returns the responsive modal `(percent_x, percent_y)` matching `render_overlay`.
pub fn modal_pct(area: Rect) -> (u16, u16) {
    if area.width >= 100 && area.height >= 30 {
        (84, 78)
    } else {
        (94, 88)
    }
}

/// Video players exposed for interactive selection on the current OS.
pub fn supported_players() -> &'static [Player] {
    #[cfg(windows)]
    {
        &[Player::AutoBest, Player::Mpv, Player::MpvNet]
    }
    #[cfg(target_os = "linux")]
    {
        &[Player::AutoBest, Player::Mpv, Player::Celluloid]
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        &[Player::AutoBest, Player::Mpv]
    }
}

/// Returns whether a given `libmpv`-compatible engine is detected on this machine.
pub fn player_installed(player: Player, state: &AppState) -> bool {
    match player {
        Player::AutoBest => state.has_any_player(),
        Player::Mpv => state.has_mpv,
        Player::MpvNet => state.has_mpvnet,
        Player::Celluloid => state.has_celluloid,
    }
}

/// Render the System Doctor & Interactive Playback Pipeline modal overlay.
pub fn render_overlay(frame: &mut Frame, state: &AppState, cfg: &config::Config) {
    let frame_area = frame.area();
    let (pct_x, pct_y) = modal_pct(frame_area);
    let area = centered_rect(pct_x, pct_y, frame_area);
    frame.render_widget(Clear, area);

    let theme = state.theme();
    let ready = state.has_ani_cli && state.has_any_player();
    let border_color = if ready {
        theme.border_focus
    } else {
        COLOR_WARN
    };
    let status_pill = if ready { "• HEALTHY " } else { "• ACTION REQUIRED " };
    let status_fg = if ready { COLOR_OK } else { COLOR_ERR };

    let outer = Block::default()
        .title(Line::from(vec![
            Span::styled(
                " ◈ SOLUS // SYSTEM DOCTOR & PIPELINE ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                status_pill,
                Style::default().fg(status_fg).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(BG_MODAL));

    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if inner.width < 12 || inner.height < 8 {
        return;
    }

    let padded = Rect {
        x: inner.x + 1,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };

    let dock_h = if padded.height >= 20 { 2 } else { 1 };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(dock_h),
        ])
        .split(padded);

    render_status_banner(frame, rows[0], state, cfg, ready, &theme);

    if padded.width >= 76 && rows[1].height >= 14 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
            .split(rows[1]);

        let right_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(cols[1]);

        render_binary_matrix(frame, cols[0], state, cfg, &theme);
        render_pipeline_controls(frame, right_rows[0], state, cfg, &theme);
        render_remediation_card(frame, right_rows[1], state, ready, &theme);
    } else {
        let stack = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(38),
                Constraint::Percentage(36),
                Constraint::Percentage(26),
            ])
            .split(rows[1]);

        render_binary_matrix(frame, stack[0], state, cfg, &theme);
        render_pipeline_controls(frame, stack[1], state, cfg, &theme);
        render_remediation_card(frame, stack[2], state, ready, &theme);
    }

    render_action_dock(frame, rows[2], &theme, dock_h == 2);
}

fn render_status_banner(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    cfg: &config::Config,
    ready: bool,
    theme: &ThemePalette,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if ready { COLOR_OK } else { COLOR_WARN }))
        .style(Style::default().bg(BG_CARD));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 8 || inner.height == 0 {
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(inner);

    let active_ok = player_installed(cfg.player, state);
    let left = if !ready {
        Line::from(vec![
            Span::styled(" ▲ ", Style::default().fg(COLOR_ERR).add_modifier(Modifier::BOLD)),
            Span::styled(
                "Missing required dependency in PATH",
                Style::default().fg(FG_PRIMARY).add_modifier(Modifier::BOLD),
            ),
        ])
    } else if !active_ok {
        Line::from(vec![
            Span::styled(" ! ", Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("Selected '{}' missing — press [p] to switch", cfg.player.as_str()),
                Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ◆ ", Style::default().fg(COLOR_OK).add_modifier(Modifier::BOLD)),
            Span::styled(
                "Playback pipeline verified & ready to stream",
                Style::default().fg(FG_PRIMARY).add_modifier(Modifier::BOLD),
            ),
        ])
    };

    let os_label = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    };

    let right = Line::from(vec![
        Span::styled(format!("{} • ", os_label), Style::default().fg(FG_MUTED)),
        Span::styled(
            format!("Engine: {} ", cfg.player.as_str().to_uppercase()),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .alignment(Alignment::Right);

    frame.render_widget(Paragraph::new(left), cols[0]);
    frame.render_widget(Paragraph::new(right), cols[1]);
}

fn render_binary_matrix(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    cfg: &config::Config,
    theme: &ThemePalette,
) {
    let block = Block::default()
        .title(Span::styled(
            " Core Binary & Player Matrix ",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle))
        .style(Style::default().bg(BG_CARD));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 10 || inner.height == 0 {
        return;
    }

    let roomy = inner.height >= 12;
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Required stream extractor
    push_binary_row(
        &mut lines,
        "ani-cli",
        "Stream resolver",
        state.has_ani_cli,
        true,
        false,
        roomy,
        theme,
    );

    lines.push(Line::from(Span::styled(
        " ".to_string() + &"─".repeat(inner.width.saturating_sub(2) as usize),
        Style::default().fg(theme.divider),
    )));

    for &player in &Player::ALL {
        let installed = player_installed(player, state);
        let is_selected = cfg.player == player;
        let role = match player {
            Player::AutoBest => "Auto-Best (libmpv parity default)",
            Player::Mpv => "100% Universal + vo=gpu-next + GLSL",
            Player::MpvNet => {
                if cfg!(windows) {
                    "Win 99% native libmpv GUI shell"
                } else {
                    "Win 99% libmpv shell (Windows Only)"
                }
            }
            Player::Celluloid => {
                if cfg!(target_os = "linux") {
                    "Linux GTK 95% native libmpv shell"
                } else {
                    "Linux GTK 95% libmpv shell (Linux Only)"
                }
            }
        };
        push_binary_row(
            &mut lines,
            player.label(),
            role,
            installed,
            false,
            is_selected,
            roomy,
            theme,
        );
    }

    if inner.height as usize > lines.len() + 2 {
        let dev = crate::services::platform::detect_device_profile();
        let caps = crate::services::platform::detect_player_capabilities();
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  ❖ ", Style::default().fg(theme.accent)),
            Span::styled(
                format!("{} · vo={}", dev.decode_summary(), caps.effective_vo()),
                Style::default().fg(FG_MUTED),
            ),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

#[allow(clippy::too_many_arguments)]
fn push_binary_row(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    role: &'static str,
    installed: bool,
    required: bool,
    is_selected: bool,
    roomy: bool,
    theme: &ThemePalette,
) {
    let (badge, badge_fg, badge_bg) = if is_selected && installed {
        (" ● ACTIVE ", Color::Black, COLOR_OK)
    } else if is_selected && !installed {
        (" ! MISSING ", Color::Black, COLOR_ERR)
    } else if installed {
        (" ✓ READY ", COLOR_OK, BG_PILL)
    } else if required {
        (" ✗ REQUIRED ", COLOR_ERR, BG_PILL)
    } else {
        (" ○ ABSENT ", FG_MUTED, BG_PILL)
    };

    let name_fg = if is_selected {
        theme.accent
    } else if installed {
        FG_PRIMARY
    } else {
        FG_MUTED
    };

    lines.push(Line::from(vec![
        Span::raw(" "),
        Span::styled(
            format!("{:<9}", name),
            Style::default().fg(name_fg).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            badge,
            Style::default()
                .fg(badge_fg)
                .bg(badge_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    if roomy {
        lines.push(Line::from(Span::styled(
            format!("   {}", role),
            Style::default().fg(FG_MUTED),
        )));
    }
}

fn render_pipeline_controls(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    cfg: &config::Config,
    theme: &ThemePalette,
) {
    let block = Block::default()
        .title(Span::styled(
            " Interactive Pipeline ◆ LIVE ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .style(Style::default().bg(BG_CARD_ALT));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 12 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Row 1: Interactive player selector strip ([p])
    let mut player_spans = vec![
        Span::styled(
            " [p] Engine  ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    for &p in supported_players() {
        let active = cfg.player == p;
        let ok = player_installed(p, state);
        let dot = if ok { "●" } else { "○" };
        let style = if active {
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(if ok { FG_PRIMARY } else { FG_MUTED })
                .bg(BG_PILL)
        };
        player_spans.push(Span::styled(format!(" {} {} ", dot, p.as_str()), style));
        player_spans.push(Span::raw(" "));
    }
    lines.push(Line::from(player_spans));

    if inner.height >= 7 {
        lines.push(Line::from(""));
    }

    // Row 2: Interactive Anime4K GLSL shader strip ([u])
    let mut shader_spans = vec![
        Span::styled(
            " [u] Anime4K ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    for mode in [Anime4KMode::Off, Anime4KMode::Balanced, Anime4KMode::Ultra] {
        let active = state.anime4k_mode == mode;
        let label = match mode {
            Anime4KMode::Off => "Off",
            Anime4KMode::Balanced => "2x",
            Anime4KMode::Ultra => "4K",
        };
        let style = if active {
            Style::default()
                .fg(Color::Black)
                .bg(theme.secondary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(FG_MUTED).bg(BG_PILL)
        };
        shader_spans.push(Span::styled(format!(" {} ", label), style));
        shader_spans.push(Span::raw(" "));
    }
    lines.push(Line::from(shader_spans));

    if inner.height >= 8 {
        let shader_note = format!(
            "     GLSL: {} · Perf {} · Deband {} · Motion {}",
            state.anime4k_mode.as_str(),
            cfg.performance_mode.short_tag(),
            cfg.deband_mode.short_tag(),
            cfg.motion_mode.short_tag()
        );
        lines.push(Line::from(Span::styled(
            shader_note,
            Style::default().fg(FG_MUTED),
        )));
        lines.push(Line::from(""));
    }

    // Row 3: Stream defaults
    let audio_label = match cfg.audio_mode {
        AudioMode::Sub => "Sub",
        AudioMode::Dub => "Dub",
    };
    lines.push(Line::from(vec![
        Span::styled("     Quality ", Style::default().fg(FG_MUTED)),
        Span::styled(
            cfg.quality.as_str().to_string(),
            Style::default().fg(FG_PRIMARY).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  •  Audio ", Style::default().fg(FG_MUTED)),
        Span::styled(
            audio_label,
            Style::default().fg(FG_PRIMARY).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  •  Sub ", Style::default().fg(FG_MUTED)),
        Span::styled(cfg.default_sub_lang.clone(), Style::default().fg(FG_PRIMARY)),
    ]));

    // Row 4: Enhancers
    lines.push(Line::from(vec![
        Span::styled("     Intro Skip ", Style::default().fg(FG_MUTED)),
        Span::styled(
            if cfg.auto_skip_intro { "On" } else { "Off" },
            Style::default().fg(if cfg.auto_skip_intro { COLOR_OK } else { FG_MUTED }),
        ),
        Span::styled("  •  Ambilight ", Style::default().fg(FG_MUTED)),
        Span::styled(
            if cfg.ambilight_enabled { "On" } else { "Off" },
            Style::default().fg(if cfg.ambilight_enabled { COLOR_OK } else { FG_MUTED }),
        ),
    ]));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_remediation_card(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    ready: bool,
    theme: &ThemePalette,
) {
    let block = Block::default()
        .title(Span::styled(
            " Remediation & Shell Commands ",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle))
        .style(Style::default().bg(BG_CARD));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 12 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    if cfg!(target_os = "macos") {
        let core_missing = !state.has_ani_cli || !state.has_mpv;
        lines.push(Line::from(vec![
            Span::styled(" $ ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(
                "brew install ani-cli mpv",
                Style::default()
                    .fg(if core_missing { COLOR_WARN } else { FG_PRIMARY })
                    .add_modifier(if core_missing { Modifier::BOLD } else { Modifier::empty() }),
            ),
        ]));
    } else if cfg!(windows) {
        lines.push(Line::from(vec![
            Span::styled(" $ ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled("scoop install mpv ani-cli   # or winget install mpv.net", Style::default().fg(FG_PRIMARY)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(" $ ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled("sudo pacman -S ani-cli mpv celluloid", Style::default().fg(FG_PRIMARY)),
        ]));
    }

    if inner.height >= 4 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            if ready {
                " ✓ Binaries active — press [r] after updates to re-probe."
            } else {
                " ! Run commands above, then press [r] to re-probe PATH."
            },
            Style::default().fg(if ready { COLOR_OK } else { COLOR_WARN }),
        )));
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_action_dock(frame: &mut Frame, area: Rect, theme: &ThemePalette, show_rule: bool) {
    let rows = if show_rule && area.height >= 2 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1)])
            .split(area)
    };

    if show_rule && rows.len() > 1 {
        let rule = "─".repeat(area.width as usize);
        frame.render_widget(
            Paragraph::new(Span::styled(rule, Style::default().fg(theme.divider))),
            rows[0],
        );
    }

    let dock_area = if show_rule && rows.len() > 1 {
        rows[1]
    } else {
        rows[0]
    };

    let dock = Line::from(vec![
        dock_pill("p", "Switch Player", theme.primary),
        Span::raw("   "),
        dock_pill("u", "Anime4K", theme.secondary),
        Span::raw("   "),
        dock_pill("r", "Re-probe", theme.accent),
        Span::raw("   "),
        dock_pill("s", "Settings", theme.primary),
        Span::raw("   "),
        dock_pill("Esc/Enter", "Done", FG_MUTED),
    ])
    .alignment(Alignment::Center);

    frame.render_widget(Paragraph::new(dock), dock_area);
}

fn dock_pill<'a>(key: &'a str, label: &'a str, accent: Color) -> Span<'a> {
    Span::styled(
        format!("[{}] {}", key, label),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )
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

    #[test]
    fn test_system_doctor_renders_healthy_state_and_pipeline_desktop_and_compact() {
        for (w, h) in [(140u16, 40u16), (90u16, 28u16)] {
            let mut state = AppState::default();
            state.has_ani_cli = true;
            state.has_mpv = true;
            state.has_mpvnet = false;
            state.has_celluloid = false;
            state.anime4k_mode = Anime4KMode::Balanced;

            let mut cfg = config::Config::default();
            cfg.player = Player::AutoBest;

            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|f| render_overlay(f, &state, &cfg))
                .expect("system doctor overlay should render cleanly");

            let text = buffer_text(terminal.backend().buffer());
            assert!(
                text.contains("SYSTEM DOCTOR & PIPELINE"),
                "missing title at {}x{}",
                w,
                h
            );
            assert!(text.contains("HEALTHY"), "missing HEALTHY pill at {}x{}", w, h);
            assert!(text.contains("ani-cli"), "missing ani-cli row at {}x{}", w, h);
            assert!(text.contains("mpv"), "missing mpv row at {}x{}", w, h);
            assert!(text.contains("ACTIVE"), "missing ACTIVE engine pill at {}x{}", w, h);
            assert!(
                text.contains("Interactive Pipeline"),
                "missing interactive pipeline card at {}x{}",
                w,
                h
            );
        }
    }

    #[test]
    fn test_system_doctor_flags_missing_dependencies_and_narrow_fallback() {
        let mut state = AppState::default();
        state.has_ani_cli = false;
        state.has_mpv = false;
        state.has_mpvnet = false;
        state.has_celluloid = false;
        let cfg = config::Config::default();

        let backend = TestBackend::new(68, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_overlay(f, &state, &cfg))
            .expect("narrow missing state must not panic");

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("ACTION REQUIRED"));
        assert!(text.contains("REQUIRED"));
        assert!(text.contains("Switch Player"));
    }
}
