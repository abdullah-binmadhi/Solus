//! Dependency / onboarding overlay.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::{config, state::AppState};

pub fn render_overlay(frame: &mut Frame, state: &AppState, cfg: &config::Config) {
    let area = centered_rect(76, 70, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(
            " Playback Setup ",
            Style::default()
                .fg(Color::Rgb(180, 0, 255))
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(180, 0, 255)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .split(inner);

    let ready = state.has_ani_cli && state.has_any_player();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                if ready {
                    "Playback dependencies look ready."
                } else {
                    "Playback needs a few external tools before it feels seamless."
                },
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Press r to refresh checks, s for settings, Esc to close.",
                Style::default().fg(Color::Rgb(150, 150, 170)),
            )),
        ]),
        rows[0],
    );

    let preferred = cfg.player.as_str();
    let mut lines = vec![
        status_line("ani-cli", state.has_ani_cli, "Required for search + stream handoff"),
        status_line("mpv", state.has_mpv, "Default playback engine"),
    ];
    if cfg!(target_os = "macos") {
        lines.push(status_line("iina", state.has_iina, "macOS-native player option"));
    }
    lines.push(status_line("vlc", state.has_vlc, "Cross-platform fallback player"));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            " Preferred ",
            Style::default().fg(Color::Black).bg(Color::Rgb(235, 235, 235)),
        ),
        Span::raw(" "),
        Span::styled(
            preferred,
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(60, 0, 100))
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(lines),
        rows[1],
    );

    #[cfg(target_os = "macos")]
    let suggestions = Paragraph::new(vec![
        Line::from(Span::styled(
            "Recommended macOS setup (Homebrew)",
            Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "brew install mpv ani-cli && brew install --cask iina vlc",
            Style::default().fg(Color::Rgb(205, 205, 215)),
        )),
        Line::from(Span::styled(
            "Press 'r' to refresh dependency checks after installing.",
            Style::default().fg(Color::Rgb(150, 150, 170)),
        )),
    ]);

    #[cfg(target_os = "windows")]
    let suggestions = Paragraph::new(vec![
        Line::from(Span::styled(
            "Recommended Windows setup (PowerShell & Scoop)",
            Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            r#"scoop install mpv vlc ani-cli  (or winget: winget install --id shinchiro.mpv -e)"#,
            Style::default().fg(Color::Rgb(205, 205, 215)),
        )),
        Line::from(Span::styled(
            "Restart terminal, then press 'r' to refresh checks.",
            Style::default().fg(Color::Rgb(150, 150, 170)),
        )),
    ]);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let suggestions = Paragraph::new(vec![
        Line::from(Span::styled(
            "Recommended Linux setup",
            Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "sudo apt install -y mpv vlc  (or pacman -S mpv vlc)",
            Style::default().fg(Color::Rgb(205, 205, 215)),
        )),
        Line::from(Span::styled(
            "Press 'r' to refresh dependency checks after installing.",
            Style::default().fg(Color::Rgb(150, 150, 170)),
        )),
    ]);
    frame.render_widget(suggestions, rows[2]);

    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                " Ready ",
                Style::default()
                    .fg(Color::Black)
                    .bg(if ready {
                        Color::Rgb(235, 235, 235)
                    } else {
                        Color::Rgb(255, 190, 0)
                    }),
            ),
            Span::raw(" "),
            Span::styled(
                if ready {
                    "You can close this and start watching."
                } else {
                    "Browsing works now. Playback will fail until the missing tools are installed."
                },
                Style::default().fg(Color::Rgb(190, 190, 205)),
            ),
        ]),
    ]);
    frame.render_widget(footer, rows[3]);
}

fn status_line(name: &str, ok: bool, note: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            if ok { " OK " } else { " Missing " },
            Style::default()
                .fg(Color::Black)
                .bg(if ok { Color::Rgb(235, 235, 235) } else { Color::Rgb(255, 190, 0) })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{name:<8}"),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(note.to_string(), Style::default().fg(Color::Rgb(165, 165, 185))),
    ])
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
