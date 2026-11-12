//! Mobile LAN Remote Companion TUI modal.
//! Renders a compact terminal QR code and local URL for instant phone pairing.

use crate::state::AppState;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

/// Center a rect within another rect by percentage.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
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
        .split(popup_layout[1])[1]
}

pub fn render_remote_modal(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(64, 76, frame.area());
    frame.render_widget(Clear, area);

    let theme = state.theme();

    let block = Block::default()
        .title(Span::styled(
            " ❖ SOLUS COMPANION // MOBILE REMOTE CONTROL ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.secondary));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // Header instruction
            Constraint::Min(10),    // QR Code container
            Constraint::Length(5),  // Direct URL + Status
            Constraint::Length(2),  // Keybindings footer
        ])
        .split(inner);

    // 1. Instruction
    let header_p = Paragraph::new(Line::from(vec![
        Span::styled("  Point your phone camera at the QR code below ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("(Same Wi-Fi network)", Style::default().fg(Color::Rgb(140, 140, 160))),
    ])).alignment(Alignment::Center);
    frame.render_widget(header_p, chunks[0]);

    // 2. ASCII QR Code
    let qr_text = state
        .remote_qr
        .as_deref()
        .unwrap_or("Generating QR code...");

    let qr_lines: Vec<Line> = qr_text
        .lines()
        .map(|l| Line::from(Span::styled(l, Style::default().fg(Color::White).bg(Color::Rgb(10, 12, 18)))))
        .collect();

    let qr_p = Paragraph::new(qr_lines).alignment(Alignment::Center);
    frame.render_widget(qr_p, chunks[1]);

    // 3. Status & Direct URL
    let url_spans = vec![
        Line::from(vec![
            Span::styled("  ◈ Direct URL: ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {} ", state.remote_url), Style::default().fg(Color::Black).bg(Color::Rgb(240, 240, 245)).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  ❖ Network Status: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled("● Active Wi-Fi Listener (Port 8484) ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            Span::styled(" 󰄬 Ready", Style::default().fg(Color::Rgb(139, 233, 253))),
        ]),
        Line::from(vec![
            Span::styled("  ◈ Controls: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled("Touch Scrub Bar  •  Play / Pause  •  ±10s Seek  •  Sub/Dub Hot-Swap  •  Volume Slider", Style::default().fg(Color::Rgb(200, 200, 215))),
        ]),
    ];
    let status_p = Paragraph::new(url_spans);
    frame.render_widget(status_p, chunks[2]);

    // 4. Footer hints
    let footer_p = Paragraph::new(Line::from(vec![
        Span::styled("[Esc] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Dismiss  ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("[o] ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("Open in Local Browser  ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("[Space] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Toggle Playback", Style::default().fg(Color::Rgb(140, 140, 160))),
    ])).alignment(Alignment::Center);
    frame.render_widget(footer_p, chunks[3]);
}
