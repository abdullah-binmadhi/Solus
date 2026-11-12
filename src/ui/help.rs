//! Help overlay — keybinding reference, shown with ? key.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

/// Render the help overlay centered on screen.
pub fn render_overlay(frame: &mut Frame) {
    let area = centered_rect(55, 70, frame.area());
    frame.render_widget(Clear, area);

    let keybindings = vec![
        ("Navigation", vec![
            ("j / ↓",      "Move down (rows / menus)"),
            ("k / ↑",      "Move up (rows / menus)"),
            ("h / ←",      "Scroll left (cards / episodes / related)"),
            ("l / →",      "Scroll right (cards / episodes / related)"),
            ("Tab",        "Toggle Detail focus: Episodes / More Like This"),
        ]),
        ("Actions", vec![
            ("Enter",      "Open detail (Home) / Play episode (Detail)"),
            ("d",          "Open detail from Home / jump 4 eps down (Detail)"),
            ("u",          "Jump 4 eps up (Detail) / Toggle Anime4K (Options)"),
            ("r",          "Quick resume from Home / Recalculate metrics"),
            ("a",          "Anime Wrapped & Taste Analytics Dashboard (Home)"),
            ("L",          "AniList Account Login / Cloud Library Sync (Home)"),
            ("w",          "Tailored Recommendation Wizard (Home) / Watched (Detail)"),
            ("D",          "Batch Offline Downloader Modal (Detail)"),
            ("v / Tab",    "Toggle Episode Queue Layout (Split Deck / List)"),
            ("Esc",        "Back / close overlay"),
            ("/",          "Open search"),
            ("s",          "Open settings (Player, Quality, Anime4K, Ambilight)"),
            ("t",          "Hot-swap color theme palette"),
            ("!",          "Open setup & dependency health check"),
            ("+",          "Add to / remove from watchlist"),
            ("x / Backspace", "Remove card / Stop playback"),
            ("n",          "Play next episode from Detail"),
            ("q",          "Quit / Stop playback"),
        ]),
        ("Other", vec![
            ("?",          "Toggle this help"),
            ("Shift+R",    "Refresh home screen"),
            ("Search",     "Type normally, use ↑/↓ to pick results and preview"),
            ("Settings",   "Use j/k to focus rows, h/l or Enter to change"),
            ("Setup",      "Use r to refresh dependency checks"),
            ("Ctrl+C",     "Force quit"),
        ]),
    ];

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " Solus — Keybindings ",
            Style::default()
                .fg(Color::Rgb(180, 0, 255))
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for (section, bindings) in &keybindings {
        lines.push(Line::from(Span::styled(
            format!("  {} ", section),
            Style::default()
                .fg(Color::Rgb(220, 220, 220))
                .add_modifier(Modifier::UNDERLINED),
        )));
        for (key, desc) in bindings {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:>12}  ", key),
                    Style::default()
                        .fg(Color::Rgb(180, 0, 255))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(*desc, Style::default().fg(Color::Rgb(190, 190, 190))),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "  Press ? or Esc to close",
        Style::default().fg(Color::Rgb(100, 100, 120)),
    )));

    let help = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(180, 0, 255))),
    );
    frame.render_widget(help, area);
}

/// Render the toast notification bar at the bottom of the screen.
pub fn render_toast(frame: &mut Frame, message: &str) {
    let area   = frame.area();
    let height = 1u16;
    let toast_area = Rect {
        x:      area.x,
        y:      area.y + area.height.saturating_sub(height),
        width:  area.width,
        height,
    };

    let toast = Paragraph::new(Span::styled(
        format!("  ✓ {}", message),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(180, 0, 255))
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(toast, toast_area);
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
