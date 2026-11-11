//! AniList Zero-Token Library Synchronization & JSON Archive Ingestion overlay.

use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::state::AppState;

/// Render the AniList Library Sync modal overlay.
pub fn render_overlay(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(76, 56, frame.area());
    frame.render_widget(Clear, area);

    let theme = state.theme();

    let block = Block::default()
        .title(Span::styled(
            " ❖ ANILIST & LOCAL LIBRARY SYNCHRONIZATION ",
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // If an import just completed, render the completion report
    if let Some(ref stats) = state.import_stats {
        render_import_success(frame, inner, state, stats);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab navigation bar
            Constraint::Length(3), // Connected user badge (if connected)
            Constraint::Min(0),    // Active tab body
            Constraint::Length(2), // Status feedback / error message
            Constraint::Length(2), // Footer keybind hints
        ])
        .split(inner);

    // ── 1. Tab Bar ──
    render_tab_bar(frame, chunks[0], state);

    // ── 2. Connected Badge (Optional) ──
    if state.auth.is_authenticated() {
        render_connected_badge(frame, chunks[1], state);
    }

    // ── 3. Active Tab Content ──
    let body_area = if state.auth.is_authenticated() {
        chunks[2]
    } else {
        // Expand body if no connected badge
        let expanded = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tab bar
                Constraint::Min(0),    // Active tab body
                Constraint::Length(2), // Status
                Constraint::Length(2), // Footer
            ])
            .split(inner);
        expanded[1]
    };

    match state.account_sync_tab {
        0 => render_tab_file_import(frame, body_area, state),
        1 => render_tab_username_sync(frame, body_area, state),
        _ => render_tab_backup_export(frame, body_area, state),
    }

    // ── 4. Status Line ──
    if let Some(ref status) = state.login_status {
        let is_err = status.to_lowercase().contains("error") || status.to_lowercase().contains("failed");
        let status_color = if is_err { Color::Rgb(255, 95, 95) } else { theme.accent };
        let status_p = Paragraph::new(Line::from(vec![
            Span::styled(" 󰋽 ", Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
            Span::styled(status, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]));
        frame.render_widget(status_p, chunks[3]);
    }

    // ── 5. Footer Actions ──
    render_footer_hints(frame, chunks[4], state);
}

/// Render the mode selection tabs with Solus heraldry glyphs.
fn render_tab_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();

    let tab_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(area);

    let tabs = [
        (0, " ❖ LOCAL ARCHIVE [1] "),
        (1, " ✦ AETHERIAL SYNC [2] "),
        (2, " ◈ BACKUP VAULT [3] "),
    ];

    for (idx, label) in tabs {
        let is_active = state.account_sync_tab == idx;
        let (style, border_style) = if is_active {
            (
                Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD),
                Style::default().fg(theme.primary),
            )
        } else {
            (
                Style::default().fg(theme.text_dim),
                Style::default().fg(Color::Rgb(45, 45, 60)),
            )
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style);
        let inner = block.inner(tab_cols[idx]);
        frame.render_widget(block, tab_cols[idx]);

        let p = Paragraph::new(Span::styled(label, style))
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(p, inner);
    }
}

/// Render connected badge when an account or archive is linked.
fn render_connected_badge(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();
    let name = state.auth.username.as_deref().unwrap_or("Local Archive");
    let mode = state.auth.sync_mode.as_deref().unwrap_or("Synchronized");

    let badge = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" 󰄬 CONNECTED: ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            Span::styled(name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  ·  Mode: {}", mode), Style::default().fg(theme.text_dim)),
            Span::styled("   [r Re-sync]   [x Disconnect]", Style::default().fg(theme.secondary)),
        ]),
    ]);
    frame.render_widget(badge, area);
}

/// Tab 0: Local JSON Archive Ingestion.
fn render_tab_file_import(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Explainer
            Constraint::Length(3), // Path input box
            Constraint::Length(1), // Spacer
            Constraint::Min(0),    // Detected files
        ])
        .split(area);

    let explainer = Paragraph::new(vec![
        Line::from(Span::styled(
            "❖ Hydrate your library from an AniList export, GDPR backup, or JSON file.",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Populates Continue Watching, Watchlist, and full episode progress completely offline.",
            Style::default().fg(theme.text_dim),
        )),
    ]);
    frame.render_widget(explainer, rows[0]);

    // Path input box
    let input_display = if state.login_token_input.is_empty() {
        "Type path or drag & drop JSON file here (e.g. ~/Downloads/animelist.json)...".to_string()
    } else {
        format!("{}_", state.login_token_input)
    };
    let input_style = if state.login_token_input.is_empty() {
        Style::default().fg(theme.text_dim)
    } else {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    };

    let input_block = Block::default()
        .title(Span::styled(" ❖ JSON ARCHIVE FILE PATH ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.secondary));
    let input_p = Paragraph::new(Span::styled(input_display, input_style))
        .block(input_block);
    frame.render_widget(input_p, rows[1]);

    // Auto-detected files from ~/Downloads
    if !state.detected_export_files.is_empty() {
        let mut detected_lines = vec![
            Line::from(Span::styled("✦ AUTO-DETECTED EXPORTS IN DOWNLOADS:", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))),
        ];

        for (idx, path) in state.detected_export_files.iter().enumerate().take(3) {
            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("export.json");
            let is_sel = idx == state.detected_file_cursor;
            let (bullet, style) = if is_sel {
                ("  ◈ [Tab Select] ", Style::default().fg(Color::White).bg(theme.secondary).add_modifier(Modifier::BOLD))
            } else {
                ("  • ", Style::default().fg(theme.text_dim))
            };

            detected_lines.push(Line::from(vec![
                Span::styled(bullet, style),
                Span::raw(" "),
                Span::styled(file_name, if is_sel { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(theme.text) }),
                Span::styled(format!("  ({})", path.display()), Style::default().fg(theme.text_dim)),
            ]));
        }

        let detected_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(50, 50, 70)));
        let detected_p = Paragraph::new(detected_lines).block(detected_block);
        frame.render_widget(detected_p, rows[3]);
    }
}

/// Tab 1: Aetherial Public Username Live Sync.
fn render_tab_username_sync(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Explainer
            Constraint::Length(3), // Username input box
            Constraint::Length(1), // Spacer
            Constraint::Min(0),    // Details box
        ])
        .split(area);

    let explainer = Paragraph::new(vec![
        Line::from(Span::styled(
            "✦ Synchronize library over-the-air directly from AniList's public GraphQL API.",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Zero tokens or passwords required — as long as your profile is public, Solus syncs your progress.",
            Style::default().fg(theme.text_dim),
        )),
    ]);
    frame.render_widget(explainer, rows[0]);

    // Username input box
    let input_display = if state.login_token_input.is_empty() {
        "Enter your AniList username (e.g. Zenith)...".to_string()
    } else {
        format!("{}_", state.login_token_input)
    };
    let input_style = if state.login_token_input.is_empty() {
        Style::default().fg(theme.text_dim)
    } else {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    };

    let input_block = Block::default()
        .title(Span::styled(" ✦ ANILIST USERNAME ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent));
    let input_p = Paragraph::new(Span::styled(input_display, input_style))
        .block(input_block);
    frame.render_widget(input_p, rows[1]);

    let details_lines = vec![
        Line::from(Span::styled("❖ LIVE SYNC CAPABILITIES:", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("  ◈ Pulls Watching, Planning, and Completed lists in 1 second.", Style::default().fg(theme.text))),
        Line::from(Span::styled("  ◈ Automatically updates Continue Watching and Watchlist on Home.", Style::default().fg(theme.text))),
        Line::from(Span::styled("  ◈ Enables instant 1-key library re-sync with [r] anytime.", Style::default().fg(theme.accent))),
        Line::from(""),
        Line::from(Span::styled("  Endpoint: https://graphql.anilist.co (MediaListCollection)", Style::default().fg(theme.text_dim))),
    ];

    let details_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(50, 50, 70)));
    let details_p = Paragraph::new(details_lines).block(details_block);
    frame.render_widget(details_p, rows[3]);
}

/// Tab 2: Backup Vault (Export).
fn render_tab_backup_export(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();

    let block = Block::default()
        .title(Span::styled(" ◈ SOLUS LIBRARY BACKUP VAULT ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from(Span::styled(
            "◈ Export your complete Solus watch history and watchlist to a clean JSON file.",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled("Destination: ~/.config/ani-tui/solus_backup.json", Style::default().fg(theme.accent))),
        Line::from(""),
        Line::from(Span::styled("This backup can be imported into another Solus install or archived safely.", Style::default().fg(theme.text_dim))),
        Line::from(""),
        Line::from(Span::styled("Press [Enter] to generate and write the backup archive.", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
    ];

    frame.render_widget(Paragraph::new(lines), inner.inner(Margin { horizontal: 1, vertical: 1 }));
}

/// Render the completion report when an import succeeds.
fn render_import_success(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    stats: &crate::services::importer::ImportStats,
) {
    let theme = state.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Header
            Constraint::Length(7), // Stats breakdown box
            Constraint::Length(2), // Spacer
            Constraint::Min(0),    // Return button
        ])
        .split(area.inner(Margin { horizontal: 2, vertical: 1 }));

    let header = Paragraph::new(vec![
        Line::from(Span::styled(
            "❖ LIBRARY HYDRATION COMPLETE",
            Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("Successfully ingested {} anime titles and {} episodes into Solus SQLite!", stats.total_anime, stats.episodes_logged),
            Style::default().fg(Color::White),
        )),
    ]);
    frame.render_widget(header, chunks[0]);

    let stats_lines = vec![
        Line::from(vec![
            Span::styled("  󰄬 Currently Watching:  ", Style::default().fg(Color::Rgb(180, 0, 255)).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} shows (hydrated into Continue Watching)", stats.watching_count), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  󰄬 Completed Anime:     ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} shows (logged into player history)", stats.completed_count), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  󰄬 Watchlist (Planning):", Style::default().fg(Color::Rgb(100, 180, 255)).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} shows (added to Home carousel)", stats.planning_count), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  󰌾 Paused / Dropped:    ", Style::default().fg(Color::Rgb(160, 160, 180))),
            Span::styled(format!("{} shows (indexed for instant search)", stats.dropped_count), Style::default().fg(theme.text_dim)),
        ]),
    ];

    let stats_box = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary));
    let stats_p = Paragraph::new(stats_lines).block(stats_box);
    frame.render_widget(stats_p, chunks[1]);

    let return_lines = vec![
        Line::from(Span::styled(
            "[ Enter / Esc ] Return to Home Screen",
            Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD),
        )),
    ];
    frame.render_widget(Paragraph::new(return_lines), chunks[3]);
}

/// Render footer action keybindings.
fn render_footer_hints(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = state.theme();

    let hint_spans = match state.account_sync_tab {
        0 => vec![
            Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Import File  ", Style::default().fg(Color::White)),
            Span::styled(" [Tab] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled("Use Detected Path  ", Style::default().fg(theme.text_dim)),
            Span::styled(" [1/2/3] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled("Switch Mode  ", Style::default().fg(theme.text_dim)),
            Span::styled(" [Esc] ", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD)),
            Span::styled("Cancel", Style::default().fg(theme.text_dim)),
        ],
        1 => vec![
            Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Connect & Sync  ", Style::default().fg(Color::White)),
            Span::styled(" [1/2/3] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled("Switch Mode  ", Style::default().fg(theme.text_dim)),
            Span::styled(" [Esc] ", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD)),
            Span::styled("Cancel", Style::default().fg(theme.text_dim)),
        ],
        _ => vec![
            Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Export Backup  ", Style::default().fg(Color::White)),
            Span::styled(" [1/2/3] ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled("Switch Mode  ", Style::default().fg(theme.text_dim)),
            Span::styled(" [Esc] ", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD)),
            Span::styled("Cancel", Style::default().fg(theme.text_dim)),
        ],
    };

    frame.render_widget(Paragraph::new(Line::from(hint_spans)), area);
}

/// Helper: center a Rect within another Rect by percentage.
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
