//! In-app self-updater modal dialog overlay.
//!
//! Guides users through available updates, live download progress, and graceful exit/restart.

use crate::state::{AppState, UpdateModalPhase};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, Paragraph},
    Frame,
};

/// Render the Update Modal overlay centered on top of the current screen.
pub fn render_overlay(frame: &mut Frame, state: &AppState) {
    let Some(ref phase) = state.update_modal_phase else {
        return;
    };

    let area = centered_rect(64, 45, frame.area());
    frame.render_widget(Clear, area);

    let _theme = state.theme();
    let border_color = match phase {
        UpdateModalPhase::Available { .. } => Color::Rgb(180, 0, 255),
        UpdateModalPhase::Downloading { .. } => Color::Rgb(0, 210, 255),
        UpdateModalPhase::RestartRequired { .. } => Color::Rgb(0, 255, 136),
        UpdateModalPhase::Failed { .. } => Color::Rgb(255, 80, 80),
    };

    let title = match phase {
        UpdateModalPhase::Available { .. } => " ❖ Solus Update Available ❖ ",
        UpdateModalPhase::Downloading { .. } => " ❖ Downloading Solus Update ❖ ",
        UpdateModalPhase::RestartRequired { .. } => " ❖ Update Successfully Applied ❖ ",
        UpdateModalPhase::Failed { .. } => " ⚠ Solus Update Failed ⚠ ",
    };

    let modal_bg = Color::Rgb(15, 16, 25);
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(modal_bg));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    match phase {
        UpdateModalPhase::Available { info } => {
            render_available(frame, inner, info, border_color);
        }
        UpdateModalPhase::Downloading { progress_pct, speed } => {
            render_downloading(frame, inner, *progress_pct, speed, border_color);
        }
        UpdateModalPhase::RestartRequired { new_version } => {
            render_restart_required(frame, inner, new_version, border_color);
        }
        UpdateModalPhase::Failed { error } => {
            render_failed(frame, inner, error, border_color);
        }
    }
}

fn render_available(
    frame: &mut Frame,
    area: Rect,
    info: &crate::services::updater::UpdateInfo,
    accent: Color,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Version diff
            Constraint::Length(3), // Asset info
            Constraint::Min(4),    // Release highlights / notes
            Constraint::Length(3), // Action buttons
        ])
        .split(area);

    // Version diff
    let version_lines = vec![
        Line::from(vec![
            Span::styled("Current Version: ", Style::default().fg(Color::Rgb(160, 160, 180))),
            Span::styled(&info.current_version, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("   ──►   ", Style::default().fg(accent).add_modifier(Modifier::BOLD)),
            Span::styled("New Version: ", Style::default().fg(Color::Rgb(160, 160, 180))),
            Span::styled(&info.latest_version, Style::default().fg(Color::Rgb(0, 255, 136)).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
    ];
    frame.render_widget(Paragraph::new(version_lines).alignment(Alignment::Center), chunks[0]);

    // Target package/asset info
    let pkg_type = if info.is_homebrew {
        "Homebrew Formula Package"
    } else {
        "Standalone Precompiled Binary (Direct Atomic Install)"
    };
    let asset_lines = vec![
        Line::from(vec![
            Span::styled("Target Asset: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(&info.asset_name, Style::default().fg(Color::Rgb(0, 210, 255))),
        ]),
        Line::from(vec![
            Span::styled("Install Method: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(pkg_type, Style::default().fg(Color::White)),
        ]),
    ];
    frame.render_widget(Paragraph::new(asset_lines), chunks[1]);

    // Release highlights / notes
    let mut notes_lines = vec![
        Line::from(Span::styled("Release Highlights:", Style::default().fg(accent).add_modifier(Modifier::UNDERLINED))),
    ];
    if let Some(ref notes) = info.release_notes {
        for line in notes.lines().take(5) {
            let cleaned = clean_highlight_line(line);
            if !cleaned.is_empty() {
                notes_lines.push(Line::from(vec![
                    Span::styled("  • ", Style::default().fg(accent)),
                    Span::styled(cleaned, Style::default().fg(Color::Rgb(210, 210, 220))),
                ]));
            }
        }
    } else {
        notes_lines.push(Line::from(Span::styled("  • Stability improvements and latest feature updates.", Style::default().fg(Color::Rgb(200, 200, 200)))));
    }
    frame.render_widget(Paragraph::new(notes_lines), chunks[2]);

    // Key actions
    let action_line = if info.is_homebrew {
        Line::from(vec![
            Span::styled("[Esc] ", Style::default().fg(Color::Rgb(180, 180, 180)).add_modifier(Modifier::BOLD)),
            Span::styled("Close  (Run 'brew upgrade solus' in your terminal)", Style::default().fg(Color::White)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " [Enter / u] Update Now ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(0, 255, 136))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  (2-4 seconds)    ", Style::default().fg(Color::Rgb(150, 155, 175))),
            Span::styled("[Esc] ", Style::default().fg(Color::Rgb(180, 180, 180)).add_modifier(Modifier::BOLD)),
            Span::styled("Remind Me Later", Style::default().fg(Color::Rgb(180, 180, 180))),
        ])
    };
    frame.render_widget(Paragraph::new(vec![action_line]).alignment(Alignment::Center), chunks[3]);
}

/// Strip markdown bold markers, bullets, and inline code ticks for crisp TUI rendering.
fn clean_highlight_line(line: &str) -> String {
    line.trim()
        .trim_start_matches(['*', '-', ' '])
        .replace("**", "")
        .replace('`', "")
        .trim()
        .to_string()
}

fn render_downloading(
    frame: &mut Frame,
    area: Rect,
    pct: f32,
    speed: &str,
    accent: Color,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Status text
            Constraint::Length(3), // Progress gauge
            Constraint::Length(2), // Speed & details
            Constraint::Min(1),    // Note
        ])
        .split(area);

    let status = Paragraph::new(vec![
        Line::from(Span::styled(
            "Downloading release binary from GitHub CDN...",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(status, chunks[0]);

    // Ratatui Gauge widget
    let clamped_pct = pct.clamp(0.0, 100.0);
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(accent)))
        .gauge_style(Style::default().fg(accent).bg(Color::Rgb(30, 30, 45)))
        .percent(clamped_pct as u16)
        .label(Span::styled(
            format!("{:.1}%", clamped_pct),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(gauge, chunks[1]);

    let details = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Transfer Speed: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(speed, Style::default().fg(Color::Rgb(0, 255, 136)).add_modifier(Modifier::BOLD)),
            Span::styled("   •   ", Style::default().fg(Color::Rgb(80, 80, 100))),
            Span::styled("Applying atomic in-use file swap...", Style::default().fg(Color::Rgb(180, 180, 200))),
        ]),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(details, chunks[2]);

    let note = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "Solus will prompt you to exit as soon as installation completes.",
            Style::default().fg(Color::Rgb(150, 150, 170)),
        )),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(note, chunks[3]);
}

fn render_restart_required(
    frame: &mut Frame,
    area: Rect,
    new_version: &str,
    accent: Color,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Success banner
            Constraint::Length(4), // Exit instructions
            Constraint::Length(3), // Action prompt
        ])
        .split(area);

    let success_banner = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("✓ Solus has been updated successfully to {}!", new_version),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(success_banner, chunks[0]);

    let instructions = Paragraph::new(vec![
        Line::from(Span::styled(
            "Please exit Solus and open it again to enter with all the new features.",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "Your anime progress, settings, and watchlist are safely preserved.",
            Style::default().fg(Color::Rgb(160, 160, 180)),
        )),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(instructions, chunks[1]);

    let actions = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("[ Enter ] ", Style::default().fg(Color::Rgb(0, 255, 136)).add_modifier(Modifier::BOLD)),
            Span::styled("Exit Solus Now", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("          ", Style::default().fg(Color::Reset)),
            Span::styled("[ Esc ] ", Style::default().fg(Color::Rgb(160, 160, 180)).add_modifier(Modifier::BOLD)),
            Span::styled("Continue Using Current Session", Style::default().fg(Color::Rgb(160, 160, 180))),
        ]),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(actions, chunks[2]);
}

fn render_failed(frame: &mut Frame, area: Rect, error: &str, accent: Color) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Header
            Constraint::Min(4),    // Error message
            Constraint::Length(2), // Close prompt
        ])
        .split(area);

    let header = Paragraph::new(vec![
        Line::from(Span::styled(
            "Automatic update could not be completed.",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(header, chunks[0]);

    let err_msg = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("Error Details: {}", error),
            Style::default().fg(Color::Rgb(255, 160, 160)),
        )),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(err_msg, chunks[1]);

    let actions = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("[Enter / Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Dismiss", Style::default().fg(Color::Rgb(180, 180, 180))),
        ]),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(actions, chunks[2]);
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
    use crate::services::updater::UpdateInfo;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_update_modal_shows_update_button_and_clean_markdown() {
        let mut state = AppState::new();
        state.open_update_modal(UpdateInfo {
            current_version: "v1.0.7 (4d9ef06)".to_string(),
            latest_version: "main@87f9ca4".to_string(),
            download_url: "https://example.com/solus".to_string(),
            asset_name: "solus-macos-aarch64".to_string(),
            release_notes: Some("Automated build for commit `87f9ca4`: demo\n**Full Changelog**: https://example.com".to_string()),
            is_homebrew: false,
        });

        let backend = TestBackend::new(140, 42);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_overlay(f, &state)).unwrap();

        let text: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Update Now"));
        assert!(text.contains("Full Changelog: https://example.com"));
        assert!(!text.contains("Full Changelog**"));
    }
}
