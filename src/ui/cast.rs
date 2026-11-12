//! TV Casting TUI overlays.
//! Includes the device scanner/picker modal and the full TV Command Deck dashboard.

use crate::state::AppState;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, Paragraph},
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

/// Formats seconds into MM:SS or HH:MM:SS.
fn format_seconds(seconds: f64) -> String {
    let total_secs = seconds.max(0.0) as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

/// Render the Target Device Picker modal overlay.
pub fn render_cast_modal(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(68, 55, frame.area());
    frame.render_widget(Clear, area);

    let theme = state.theme();

    let block = Block::default()
        .title(Span::styled(
            " ❖ BEAM TO LIVING ROOM // TARGET DEVICE PICKER ",
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
            Constraint::Length(2),  // Scan status banner
            Constraint::Min(6),     // Device list
            Constraint::Length(3),  // Protocol badges info
            Constraint::Length(2),  // Footer keybind hints
        ])
        .split(inner);

    // 1. Scan status
    let scan_msg = if state.is_scanning_cast {
        Line::from(vec![
            Span::styled("  ◈ Scanning local Wi-Fi for AirPlay & DLNA devices... ", Style::default().fg(theme.accent)),
            Span::styled("󰑋", Style::default().fg(Color::Rgb(80, 250, 123))),
        ])
    } else {
        Line::from(vec![
            Span::styled("  ◈ Discovered Devices on Local Subnet: ", Style::default().fg(Color::Rgb(160, 165, 180))),
            Span::styled(format!("{} found ", state.cast_devices.len()), Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            Span::styled("󰄬 Ready to Stream", Style::default().fg(Color::Rgb(139, 233, 253))),
        ])
    };
    frame.render_widget(Paragraph::new(scan_msg), chunks[0]);

    // 2. Device List
    let mut item_lines = Vec::new();

    if state.cast_devices.is_empty() {
        item_lines.push(Line::from(Span::styled(
            "    No wireless TVs detected. Press [s] to rescan or verify devices are on same Wi-Fi.",
            Style::default().fg(Color::Rgb(140, 140, 160)),
        )));
    } else {
        for (idx, dev) in state.cast_devices.iter().enumerate() {
            let is_selected = idx == state.cast_selected_idx;

            let (cursor, line_style, badge_style) = if is_selected {
                (
                    " ❖ ",
                    Style::default().fg(Color::White).bg(Color::Rgb(35, 40, 58)).add_modifier(Modifier::BOLD),
                    Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD),
                )
            } else {
                (
                    "   ",
                    Style::default().fg(Color::Rgb(210, 215, 230)),
                    Style::default().fg(Color::Rgb(140, 140, 160)).bg(Color::Rgb(20, 22, 32)),
                )
            };

            let line = Line::from(vec![
                Span::styled(cursor, line_style),
                Span::styled(format!("[{}] ", idx + 1), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<30} ", dev.name), line_style),
                Span::styled(format!(" {} ", dev.protocol.badge()), badge_style),
                Span::styled(format!("  {}  ", dev.endpoint()), Style::default().fg(Color::Rgb(140, 140, 160))),
                Span::styled("Ready (1080p Direct)", Style::default().fg(Color::Rgb(80, 250, 123))),
            ]);
            item_lines.push(line);
        }
    }
    frame.render_widget(Paragraph::new(item_lines), chunks[1]);

    // 3. Information note
    let info_spans = Line::from(vec![
        Span::styled("  ◈ Stream Mode: ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("Direct HLS Passthrough (Hardware Decoded on TV)  •  Zero Laptop CPU Usage", Style::default().fg(Color::Rgb(180, 185, 200))),
    ]);
    frame.render_widget(Paragraph::new(info_spans), chunks[2]);

    // 4. Footer hints
    let footer_p = Paragraph::new(Line::from(vec![
        Span::styled("[↑/↓] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("Select Target   ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("[Enter] ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
        Span::styled("Cast Stream Immediately   ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("[s] ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("Rescan Network   ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("[Esc] ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("Cancel", Style::default().fg(Color::Rgb(140, 140, 160))),
    ])).alignment(Alignment::Center);
    frame.render_widget(footer_p, chunks[3]);
}

/// Render the full-screen "TV Command Deck" dashboard while casting.
pub fn render_cast_deck(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let theme = state.theme();

    let target_name = state
        .active_cast_device
        .as_ref()
        .map(|d| d.name.as_str())
        .unwrap_or("Living Room TV");

    let block = Block::default()
        .title(Span::styled(
            format!(" ❖ SOLUS // CASTING TO: {} ", target_name),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(theme.secondary));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),   // Anime Title & Episode Meta
            Constraint::Length(4),   // Large Synchronized Clocks & Gauge
            Constraint::Min(6),      // Live Synopsis & Details
            Constraint::Length(4),   // Keybindings Command Bar
        ])
        .split(inner);

    // 1. Meta
    let title = state
        .selected_anime
        .as_ref()
        .map(|a| a.display_title())
        .unwrap_or("Anime Stream");
    let ep_num = state.selected_episode.unwrap_or(1);
    let ep_title = state
        .episode_titles
        .get(&ep_num)
        .cloned()
        .unwrap_or_else(|| format!("Episode {}", ep_num));

    let meta_lines = vec![
        Line::from(vec![
            Span::styled(format!("  {} ", title), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("— Episode {} ", ep_num), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(format!("\"{}\"", ep_title), Style::default().fg(Color::Rgb(160, 165, 180))),
        ]),
        Line::from(vec![
            Span::styled("  ◈ Stream Pipeline: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled("1080p Master HLS Passthrough  ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            Span::styled("• Audio: Stereo FLAC  • Subtitles: Integrated", Style::default().fg(Color::Rgb(140, 140, 160))),
        ]),
    ];
    frame.render_widget(Paragraph::new(meta_lines), chunks[0]);

    // 2. Synchronized Clocks & Timeline
    let pos = state.cast_playback_pos;
    let dur = state.cast_playback_dur.max(1.0);
    let ratio = (pos / dur).clamp(0.0, 1.0);

    let clock_line = Line::from(vec![
        Span::styled(format!("  {} ", format_seconds(pos)), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("━━━━━━━━━━━━━━━━━ ", Style::default().fg(theme.secondary)),
        Span::styled(format!("{} ", format_seconds(dur)), Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled(format!(" [-{}] ", format_seconds((dur - pos).max(0.0))), Style::default().fg(theme.accent)),
        Span::styled("● STREAMING LIVE", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
    ]);
    frame.render_widget(Paragraph::new(clock_line), chunks[1]);

    let gauge_area = Rect {
        x: chunks[1].x + 2,
        y: chunks[1].y + 1,
        width: chunks[1].width.saturating_sub(4),
        height: 1,
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(theme.primary).bg(Color::Rgb(25, 28, 38)))
        .ratio(ratio)
        .label("");
    frame.render_widget(gauge, gauge_area);

    // 3. Live Synopsis
    let synopsis = state
        .episode_synopses
        .get(&ep_num)
        .or_else(|| state.selected_anime.as_ref().and_then(|a| a.description.as_ref()))
        .map(|s| s.as_str())
        .unwrap_or("Playback running smoothly on television hardware decoder.");

    let synopsis_block = Block::default()
        .title(Span::styled(" ◈ Episode Narrative & Live Status ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(40, 44, 60)));

    let syn_inner = synopsis_block.inner(chunks[2]);
    frame.render_widget(synopsis_block, chunks[2]);
    frame.render_widget(Paragraph::new(synopsis).style(Style::default().fg(Color::Rgb(200, 205, 220))), syn_inner);

    // 4. Remote Keybindings
    let remote_box = Block::default()
        .title(Span::styled(" ❖ Television Remote Controls ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary));

    let remote_inner = remote_box.inner(chunks[3]);
    frame.render_widget(remote_box, chunks[3]);

    let key_lines = vec![
        Line::from(vec![
            Span::styled("  [Space] ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            Span::styled("Pause/Resume TV   ", Style::default().fg(Color::White)),
            Span::styled("[←/→] ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled("Seek ±10s   ", Style::default().fg(Color::White)),
            Span::styled("[↑/↓] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled("TV Volume   ", Style::default().fg(Color::White)),
            Span::styled("[n] ", Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD)),
            Span::styled("Next Episode   ", Style::default().fg(Color::White)),
            Span::styled("[c] ", Style::default().fg(Color::Rgb(255, 85, 85)).add_modifier(Modifier::BOLD)),
            Span::styled("Stop & Disconnect", Style::default().fg(Color::White)),
        ]),
    ];
    frame.render_widget(Paragraph::new(key_lines), remote_inner);
}
