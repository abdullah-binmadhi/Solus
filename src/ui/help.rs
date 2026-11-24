//! Contextual Bento command matrix & keybinding overlay (`?`).

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::{
    state::{AppState, Screen},
    ui::theme::ThemePalette,
};

const BG_MODAL: Color = Color::Rgb(15, 16, 25);
const BG_CARD: Color = Color::Rgb(19, 20, 32);
const BG_CARD_ACTIVE: Color = Color::Rgb(24, 24, 40);
const BG_KEYCAP: Color = Color::Rgb(30, 32, 50);
const BG_KEYCAP_ACTIVE: Color = Color::Rgb(42, 36, 68);
const FG_PRIMARY: Color = Color::Rgb(235, 238, 250);
const FG_MUTED: Color = Color::Rgb(150, 155, 180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpSection {
    Browsing,
    Playback,
    Features,
    System,
}

struct BindingRow {
    key: &'static str,
    desc: &'static str,
}

struct SectionSpec {
    section: HelpSection,
    title: &'static str,
    subtitle: &'static str,
    bindings: &'static [BindingRow],
}

const BROWSING_BINDINGS: &[BindingRow] = &[
    BindingRow { key: "Enter/Click", desc: "Open anime detail stage" },
    BindingRow { key: "h / j / k / l", desc: "Navigate shelves & posters" },
    BindingRow { key: "1 - 6", desc: "Jump directly to shelf row" },
    BindingRow { key: "r", desc: "Resume next unwatched ep" },
    BindingRow { key: "/", desc: "Instant anime search" },
    BindingRow { key: "+ / x", desc: "Status picker / Remove entry" },
    BindingRow { key: "R", desc: "Refresh AniList & MAL shelves" },
    BindingRow { key: "q / Esc", desc: "Back or quit confirmation" },
];

const PLAYBACK_BINDINGS: &[BindingRow] = &[
    BindingRow { key: "Enter/Space", desc: "Launch highlighted episode" },
    BindingRow { key: "e / E", desc: "Enqueue / Batch range queue" },
    BindingRow { key: "p / D", desc: "Stream opts / Offline download" },
    BindingRow { key: "v / Tab", desc: "Grid view / Switch detail pane" },
    BindingRow { key: "w", desc: "Mark highlighted ep watched" },
    BindingRow { key: "n / +", desc: "Next aired ep / Status picker" },
    BindingRow { key: "c / m", desc: "LAN Cast Deck / QR remote" },
    BindingRow { key: "Esc / q", desc: "Return to previous screen" },
];

const FEATURES_BINDINGS: &[BindingRow] = &[
    BindingRow { key: "Q / Ctrl+Q", desc: "Queue & Playlist editor" },
    BindingRow { key: "a", desc: "Taste Analytics Bento Hub" },
    BindingRow { key: "w (Home)", desc: "Tailored recommendation wizard" },
    BindingRow { key: "c / C", desc: "Airing calendar / Cast Deck" },
    BindingRow { key: "m", desc: "Mobile phone QR remote control" },
    BindingRow { key: "L", desc: "AniList & MAL Auth & Merge Hub" },
    BindingRow { key: "t / T", desc: "Cycle UI visual theme" },
    BindingRow { key: "Tab / Arrows", desc: "Move focus inside overlays" },
];

const SYSTEM_BINDINGS: &[BindingRow] = &[
    BindingRow { key: "P", desc: "Switch or manage user profiles" },
    BindingRow { key: "s", desc: "Open Settings Control Hub" },
    BindingRow { key: "N", desc: "Cycle network mode (Full/WiFi/Eco)" },
    BindingRow { key: "!", desc: "System Doctor & shader pipeline" },
    BindingRow { key: "U", desc: "Check or install self-update" },
    BindingRow { key: "X", desc: "Stop active player process" },
    BindingRow { key: "? / Esc", desc: "Toggle or dismiss this matrix" },
    BindingRow { key: "Ctrl+C", desc: "Immediate emergency exit" },
];

/// Returns the responsive modal `(percent_x, percent_y)` matching `Home` backdrop clearing.
pub fn modal_pct(area: Rect) -> (u16, u16) {
    let rect = help_modal_rect(area);
    let px = ((rect.width as u32 * 100) / area.width.max(1) as u32).clamp(50, 98) as u16;
    let py = ((rect.height as u32 * 100) / area.height.max(1) as u32).clamp(50, 98) as u16;
    (px, py)
}

/// Content-fitted centered modal rectangle eliminating clipping, concatenation, and dead space.
fn help_modal_rect(area: Rect) -> Rect {
    let target_w = if area.width >= 96 {
        area.width.saturating_sub(4).min(118)
    } else {
        area.width.saturating_sub(2).max(20)
    };
    let top_h = BROWSING_BINDINGS.len().max(PLAYBACK_BINDINGS.len()) as u16 + 2;
    let bot_h = FEATURES_BINDINGS.len().max(SYSTEM_BINDINGS.len()) as u16 + 2;
    let player_bar_h: u16 = if area.height >= 28 { 4 } else { 0 };
    let desired_h = 2 + 2 + top_h + bot_h + player_bar_h + 2; // borders + header + cards + player + dock
    let target_h = desired_h.min(area.height.saturating_sub(2)).max(12);

    let x = area.x + area.width.saturating_sub(target_w) / 2;
    let y = area.y + area.height.saturating_sub(target_h) / 2;
    Rect {
        x,
        y,
        width: target_w.min(area.width),
        height: target_h.min(area.height),
    }
}

fn active_section(screen: &Screen) -> HelpSection {
    match screen {
        Screen::Home | Screen::Search => HelpSection::Browsing,
        Screen::Detail
        | Screen::PlaybackQuery
        | Screen::PlaybackOptions
        | Screen::Playback => HelpSection::Playback,
        Screen::TasteAnalytics
        | Screen::TailoredWizard
        | Screen::TailoredSummary
        | Screen::TailoredResults
        | Screen::Calendar
        | Screen::CastModal
        | Screen::CastDeck
        | Screen::RemoteModal => HelpSection::Features,
        Screen::Settings
        | Screen::Setup
        | Screen::UpdateModal
        | Screen::AniListLogin
        | Screen::ProfileModal
        | Screen::QueueModal
        | Screen::Help => HelpSection::System,
    }
}

fn context_label(screen: &Screen) -> &'static str {
    match screen {
        Screen::Home => "HOME EXPLORE",
        Screen::Search => "SEARCH",
        Screen::Detail => "ANIME DETAIL",
        Screen::PlaybackQuery | Screen::PlaybackOptions | Screen::Playback => "PLAYBACK ENGINE",
        Screen::TasteAnalytics => "TASTE ANALYTICS",
        Screen::TailoredWizard | Screen::TailoredSummary | Screen::TailoredResults => "TAILORED PICKS",
        Screen::Calendar => "RELEASE CALENDAR",
        Screen::CastModal | Screen::CastDeck => "LAN CASTING",
        Screen::RemoteModal => "MOBILE REMOTE",
        Screen::Settings => "SETTINGS HUB",
        Screen::Setup => "SYSTEM DOCTOR",
        Screen::UpdateModal => "SELF-UPDATE",
        Screen::AniListLogin => "ANILIST AUTH",
        Screen::ProfileModal => "PROFILE SWITCHER",
        Screen::QueueModal => "QUEUE & PLAYLISTS",
        Screen::Help => "COMMAND MATRIX",
    }
}

/// Render the contextual 2x2 Bento command matrix modal overlay.
pub fn render_overlay(frame: &mut Frame, state: &AppState) {
    let area = help_modal_rect(frame.area());
    frame.render_widget(Clear, area);

    let theme = state.theme();
    let context_screen = if state.screen == Screen::Help {
        &state.overlay_base
    } else {
        &state.screen
    };
    let focused_section = active_section(context_screen);
    let ctx_name = context_label(context_screen);

    let outer = Block::default()
        .title(Line::from(vec![
            Span::styled(
                " ◈ SOLUS // COMMAND MATRIX ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("• CONTEXT: ", Style::default().fg(FG_MUTED)),
            Span::styled(
                format!("{} ", ctx_name),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus))
        .style(Style::default().bg(BG_MODAL));

    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if inner.width < 12 || inner.height < 6 {
        return;
    }

    let padded = Rect {
        x: inner.x + 1,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };

    let show_header_rule = padded.height >= 24;
    let show_dock_rule = padded.height >= 22;
    let show_player_strip = padded.height >= 26;
    let header_h = if show_header_rule { 2 } else { 1 };
    let player_h = if show_player_strip { 4 } else { 0 };
    let dock_h = if show_dock_rule { 2 } else { 1 };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),
            Constraint::Min(0),
            Constraint::Length(player_h),
            Constraint::Length(dock_h),
        ])
        .split(padded);

    render_header(frame, rows[0], ctx_name, &theme, show_header_rule);

    let sections = [
        SectionSpec {
            section: HelpSection::Browsing,
            title: "Browsing & Discovery",
            subtitle: "Home shelves & search",
            bindings: BROWSING_BINDINGS,
        },
        SectionSpec {
            section: HelpSection::Playback,
            title: "Detail & Playback",
            subtitle: "Episodes, queue & stream setup",
            bindings: PLAYBACK_BINDINGS,
        },
        SectionSpec {
            section: HelpSection::Features,
            title: "Intelligence & Companion",
            subtitle: "Queue, analytics, calendar & LAN",
            bindings: FEATURES_BINDINGS,
        },
        SectionSpec {
            section: HelpSection::System,
            title: "System & Quick Bridges",
            subtitle: "Hub, profiles, doctor & runtime",
            bindings: SYSTEM_BINDINGS,
        },
    ];

    render_bento_cards(frame, rows[1], &sections, focused_section, &theme);
    if show_player_strip {
        render_player_strip(frame, rows[2], &theme);
    }
    render_action_dock(frame, rows[3], &theme, show_dock_rule);
}

fn render_header(
    frame: &mut Frame,
    area: Rect,
    ctx_name: &str,
    theme: &ThemePalette,
    show_rule: bool,
) {
    let header_rows = if show_rule && area.height >= 2 {
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

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(header_rows[0]);

    let left = Line::from(vec![
        Span::styled(" ◆ ", Style::default().fg(theme.accent)),
        Span::styled("Active Pane Highlighted: ", Style::default().fg(FG_MUTED)),
        Span::styled(
            ctx_name,
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let right = Line::from(vec![
        Span::styled("Theme: ", Style::default().fg(FG_MUTED)),
        Span::styled(
            format!("{} ", theme.name),
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .alignment(Alignment::Right);

    frame.render_widget(Paragraph::new(left), cols[0]);
    frame.render_widget(Paragraph::new(right), cols[1]);

    if show_rule && header_rows.len() > 1 {
        let rule = "─".repeat(area.width as usize);
        frame.render_widget(
            Paragraph::new(Span::styled(rule, Style::default().fg(theme.divider))),
            header_rows[1],
        );
    }
}

fn render_bento_cards(
    frame: &mut Frame,
    area: Rect,
    sections: &[SectionSpec; 4],
    focused: HelpSection,
    theme: &ThemePalette,
) {
    // 2x2 Bento matrix when horizontal budget avoids label truncation; 1-column fallback on narrow panes.
    if area.width >= 76 && area.height >= 16 {
        let top_rows = (sections[0].bindings.len().max(sections[1].bindings.len()) + 2) as u16;
        let bot_rows = (sections[2].bindings.len().max(sections[3].bindings.len()) + 2) as u16;
        let grid_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(top_rows.min(area.height / 2)),
                Constraint::Min(bot_rows.min(area.height.saturating_sub(top_rows.min(area.height / 2)))),
            ])
            .split(area);

        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(grid_rows[0]);
        let bot_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(grid_rows[1]);

        let rects = [top_cols[0], top_cols[1], bot_cols[0], bot_cols[1]];
        for (idx, spec) in sections.iter().enumerate() {
            render_section_card(frame, rects[idx], spec, spec.section == focused, theme);
        }
    } else {
        let constraints = [
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ];
        let stack = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);
        for (idx, spec) in sections.iter().enumerate() {
            render_section_card(frame, stack[idx], spec, spec.section == focused, theme);
        }
    }
}

fn render_section_card(
    frame: &mut Frame,
    area: Rect,
    spec: &SectionSpec,
    is_active: bool,
    theme: &ThemePalette,
) {
    let card_bg = if is_active { BG_CARD_ACTIVE } else { BG_CARD };
    let keycap_bg = if is_active {
        BG_KEYCAP_ACTIVE
    } else {
        BG_KEYCAP
    };
    let border_fg = if is_active {
        theme.primary
    } else {
        theme.border_idle
    };
    let title_fg = if is_active {
        theme.primary
    } else {
        theme.secondary
    };

    let mut title_spans = vec![Span::styled(
        format!(" {} ", spec.title),
        Style::default().fg(title_fg).add_modifier(Modifier::BOLD),
    )];
    if is_active {
        title_spans.push(Span::styled(
            "◆ ACTIVE ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let block = Block::default()
        .title(Line::from(title_spans))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_fg))
        .style(Style::default().bg(card_bg));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 8 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let max_rows = inner.height as usize;

    // Show contextual subtitle only when all 4 cards have extra spare height so rows align evenly.
    if max_rows > spec.bindings.len() + 1 {
        lines.push(Line::from(Span::styled(
            format!("  {}", spec.subtitle),
            Style::default().fg(theme.text_dim),
        )));
    }

    let key_col_width: usize = if inner.width >= 48 { 14 } else { 13 };
    let key_fg = if is_active {
        theme.accent
    } else {
        theme.primary
    };

    for row in spec.bindings {
        if lines.len() >= max_rows {
            break;
        }
        let pill = format!(" {} ", row.key);
        let pill_chars = pill.chars().count();
        let pad = key_col_width.saturating_sub(pill_chars) + 2;
        let desc_budget = (inner.width as usize).saturating_sub(1 + pill_chars + pad);
        let desc_text = if row.desc.chars().count() > desc_budget {
            let mut s: String = row.desc.chars().take(desc_budget.saturating_sub(1)).collect();
            if desc_budget >= 2 {
                s.push('…');
            }
            s
        } else {
            row.desc.to_string()
        };

        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                pill,
                Style::default()
                    .fg(key_fg)
                    .bg(keycap_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" ".repeat(pad)),
            Span::styled(desc_text, Style::default().fg(FG_PRIMARY)),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_player_chip_row(chips: &[(&str, &str, Color, u8)], max_w: usize) -> Line<'static> {
    let chip_width = |key: &str, label: &str| key.chars().count() + label.chars().count() + 3;
    let sep_w = 2usize;
    let mut include = vec![true; chips.len()];

    loop {
        let mut count = 0usize;
        let mut total = 0usize;
        for (idx, (k, l, _, _)) in chips.iter().enumerate() {
            if include[idx] {
                if count > 0 {
                    total += sep_w;
                }
                total += chip_width(k, l);
                count += 1;
            }
        }
        if total <= max_w {
            break;
        }
        if let Some((drop_idx, _)) = chips
            .iter()
            .enumerate()
            .filter(|(idx, _)| include[*idx])
            .max_by_key(|(idx, (_, _, _, prio))| (*prio, *idx))
        {
            include[drop_idx] = false;
        } else {
            break;
        }
    }

    let mut spans = Vec::new();
    for (idx, (key, label, color, _)) in chips.iter().enumerate() {
        if !include[idx] {
            continue;
        }
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(dock_pill(key, label, *color));
    }
    Line::from(spans).alignment(Alignment::Center)
}

fn render_player_strip(frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    let block = Block::default()
        .title(Span::styled(
            "  In-Player Controls (Unified libmpv) ",
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_idle))
        .style(Style::default().bg(BG_CARD));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let max_w = inner.width as usize;
    let row1: [(&str, &str, Color, u8); 9] = [
        ("Space/k", "Pause", theme.primary, 1),
        ("j/l", "±10s", theme.accent, 2),
        ("#/Ctrl+a", "Audio", theme.secondary, 1),
        ("c", "Sub", theme.primary, 3),
        ("M", "Mute", FG_PRIMARY, 5),
        ("f", "Full", FG_PRIMARY, 4),
        ("p/P", "PiP", theme.accent, 4),
        ("0-9", "Jump%", theme.accent, 7),
        ("q", "Exit", FG_MUTED, 1),
    ];
    let row2: [(&str, &str, Color, u8); 7] = [
        ("i/I", "HUD", theme.primary, 2),
        ("Tab", "Skip OP/ED", theme.accent, 1),
        ("n/N", "Next Ep", theme.secondary, 1),
        ("y", "Voice Sync", theme.primary, 2),
        ("S", "Sync Bar", theme.secondary, 3),
        ("z/x", "Delay", FG_MUTED, 6),
        ("a/d/m/g", "4K/Deband/24p/Perf", theme.accent, 5),
    ];

    let mut lines = vec![render_player_chip_row(&row1, max_w)];
    if inner.height >= 2 {
        lines.push(render_player_chip_row(&row2, max_w));
    }
    frame.render_widget(Paragraph::new(lines), inner);
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

    let theme_label = format!("Theme: {}", theme.name);
    let dock = Line::from(vec![
        dock_pill("s", "Settings Hub", theme.primary),
        Span::raw("   "),
        dock_pill("t", &theme_label, theme.secondary),
        Span::raw("   "),
        dock_pill("!", "System Doctor", theme.accent),
        Span::raw("   "),
        dock_pill("?/Esc", "Dismiss", FG_MUTED),
    ])
    .alignment(Alignment::Center);

    frame.render_widget(Paragraph::new(dock), dock_area);
}

fn dock_pill(key: &str, label: &str, accent: Color) -> Span<'static> {
    Span::styled(
        format!("[{}] {}", key, label),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )
}

/// Render the toast notification bar at the bottom of the screen.
pub fn render_toast(frame: &mut Frame, message: &str, theme: &ThemePalette) {
    let area = frame.area();
    let height = 1u16;
    let toast_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(height),
        width: area.width,
        height,
    };

    let toast = Paragraph::new(Span::styled(
        format!("  ✓ {}", message),
        Style::default()
            .fg(Color::Black)
            .bg(theme.primary)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(toast, toast_area);
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
    fn test_command_matrix_renders_all_bento_sections_desktop_and_compact() {
        for (w, h) in [(140u16, 40u16), (90u16, 28u16)] {
            let mut state = AppState::default();
            state.screen = Screen::Help;
            state.overlay_base = Screen::Home;

            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|f| render_overlay(f, &state))
                .expect("command matrix should render cleanly");

            let text = buffer_text(terminal.backend().buffer());
            assert!(text.contains("SOLUS // COMMAND MATRIX"), "missing header at {}x{}", w, h);
            assert!(text.contains("Browsing & Discovery"), "missing browsing card at {}x{}", w, h);
            assert!(text.contains("Detail & Playback"), "missing playback card at {}x{}", w, h);
            assert!(
                text.contains("Intelligence & Companion"),
                "missing features card at {}x{}",
                w,
                h
            );
            assert!(
                text.contains("System & Quick Bridges"),
                "missing system card at {}x{}",
                w,
                h
            );
            assert!(text.contains("Settings Hub"), "missing Settings action dock at {}x{}", w, h);
            assert!(text.contains("System Doctor"), "missing Doctor action dock at {}x{}", w, h);
            assert!(text.contains("Queue & Playlist editor"), "missing Queue keybind in help matrix");
            if h >= 32 {
                assert!(text.contains("In-Player Controls (Unified libmpv)"), "missing player bar");
                assert!(text.contains("[q] Exit"), "player strip must not clip [q] Exit mid-word at {}x{}", w, h);
                assert!(text.contains("[#/Ctrl+a] Audio"), "missing audio switch keybind at {}x{}", w, h);
                assert!(text.contains("[y] Voice Sync"), "missing Voice Sync keybind at {}x{}", w, h);
                assert!(text.contains("[Tab] Skip OP/ED"), "missing Skip OP/ED keybind at {}x{}", w, h);
            }
        }
    }

    #[test]
    fn test_command_matrix_highlights_active_overlay_base_context() {
        let mut state = AppState::default();
        state.screen = Screen::Help;
        state.overlay_base = Screen::Detail;

        let backend = TestBackend::new(130, 38);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_overlay(f, &state)).unwrap();

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("CONTEXT: ANIME DETAIL"), "expected ANIME DETAIL badge");
        assert!(
            text.contains("Detail & Playback◆ ACTIVE") || text.contains("Detail & Playback ◆ ACTIVE"),
            "expected Detail & Playback card to carry ACTIVE badge"
        );
        assert!(text.contains("[q] Exit"), "player strip must include intact [q] Exit");
    }

    #[test]
    fn test_command_matrix_handles_narrow_terminal_without_panicking() {
        let mut state = AppState::default();
        state.screen = Screen::Help;
        state.overlay_base = Screen::TasteAnalytics;

        let backend = TestBackend::new(64, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_overlay(f, &state))
            .expect("narrow terminal fallback must not panic");

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("COMMAND MATRIX"));
        assert!(text.contains("TASTE ANALYTICS"));
    }
}
