//! UI rendering for the Tailored Algorithm:
//! 1. Questionnaire Wizard modal
//! 2. Solus Taste Constellation & Interactive Persona Dashboard
//! 3. Tailored Recommendations Shelf

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

use crate::{
    services::tailored::{
        calculate_persona, calculate_polarity, clean_synopsis, get_dimension_descriptor,
        get_primal_elements, get_question_active_pill, get_questions,
        get_spiritual_kinship_details, get_tasting_notes, get_thematic_hook, Persona,
    },
    state::AppState,
};

/// Helper to center a rectangular area within another.
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

/// Helper to render a Unicode block gauge bar (e.g. `[████████░░] 80%`).
fn render_gauge_bar(value: u8, width: usize, fill_color: Color) -> Line<'static> {
    let fill_count = ((value as usize) * width) / 100;
    let empty_count = width.saturating_sub(fill_count);

    let filled = "█".repeat(fill_count);
    let empty = "░".repeat(empty_count);

    Line::from(vec![
        Span::styled("[", Style::default().fg(Color::Rgb(100, 100, 120))),
        Span::styled(filled, Style::default().fg(fill_color).add_modifier(Modifier::BOLD)),
        Span::styled(empty, Style::default().fg(Color::Rgb(50, 50, 65))),
        Span::styled("] ", Style::default().fg(Color::Rgb(100, 100, 120))),
        Span::styled(
            format!("{:>3}%", value),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Render a mathematically symmetrical 5-axis Pentagon Radar Chart using Braille subpixels.
fn render_pentagon_radar(frame: &mut Frame, area: Rect, persona: &Persona) {
    if area.width < 28 || area.height < 7 {
        return;
    }

    let int_val = persona.gauges[0].1;
    let lore_val = persona.gauges[1].1;
    let emo_val = persona.gauges[2].1;
    let combat_val = persona.gauges[3].1;
    let dark_val = persona.gauges[4].1;

    let p_color = persona.primary_color;
    let a_color = persona.accent_color;
    let outer_grid_color = Color::Rgb(85, 90, 120);
    let inner_grid_color = Color::Rgb(46, 50, 68);
    let spoke_color = Color::Rgb(58, 62, 85);

    // 1 horizontal Braille dot = 1 X-unit, 1 vertical Braille dot = 1 Y-unit (square pixels)
    let x_half = area.width as f64;
    let y_half = (area.height * 2) as f64;
    let cx = 0.0f64;
    let cy = -1.5f64;
    let r = (y_half - 6.0).min(x_half - 24.0).max(6.0);

    // 5 regular pentagon vertices starting at 90° (Top) clockwise:
    // 0: Top (Intellect), 1: Top-Right (Combat), 2: Bottom-Right (Darkness),
    // 3: Bottom-Left (Emotion), 4: Top-Left (Lore)
    let angles: [f64; 5] = [
        90.0f64.to_radians(),
        18.0f64.to_radians(),
        (-54.0f64).to_radians(),
        (-126.0f64).to_radians(),
        162.0f64.to_radians(),
    ];
    let values: [u8; 5] = [int_val, combat_val, dark_val, emo_val, lore_val];

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-x_half, x_half])
        .y_bounds([-y_half, y_half])
        .paint(move |ctx| {
            let outer_pts: Vec<(f64, f64)> = angles
                .iter()
                .map(|a| (cx + r * a.cos(), cy + r * a.sin()))
                .collect();
            let mid_pts: Vec<(f64, f64)> = angles
                .iter()
                .map(|a| (cx + (r * 0.5) * a.cos(), cy + (r * 0.5) * a.sin()))
                .collect();
            let data_pts: Vec<(f64, f64)> = angles
                .iter()
                .zip(values.iter())
                .map(|(a, &v)| {
                    let s = 0.15 + 0.85 * (v as f64 / 100.0);
                    (cx + r * s * a.cos(), cy + r * s * a.sin())
                })
                .collect();

            // 1. Inner 50% pentagon ring
            for i in 0..5 {
                let (x1, y1) = mid_pts[i];
                let (x2, y2) = mid_pts[(i + 1) % 5];
                ctx.draw(&CanvasLine { x1, y1, x2, y2, color: inner_grid_color });
            }

            // 2. Radial spoke axes
            for &(vx, vy) in &outer_pts {
                ctx.draw(&CanvasLine { x1: cx, y1: cy, x2: vx, y2: vy, color: spoke_color });
            }

            // 3. Outer 100% pentagon frame
            for i in 0..5 {
                let (x1, y1) = outer_pts[i];
                let (x2, y2) = outer_pts[(i + 1) % 5];
                ctx.draw(&CanvasLine { x1, y1, x2, y2, color: outer_grid_color });
            }

            // 4. User Taste Polygon (glowing primary)
            for i in 0..5 {
                let (x1, y1) = data_pts[i];
                let (x2, y2) = data_pts[(i + 1) % 5];
                ctx.draw(&CanvasLine { x1, y1, x2, y2, color: p_color });
            }

            // 5. Crisp vertex anchor points
            ctx.draw(&Points {
                coords: &data_pts,
                color: a_color,
            });

            // 6. Symmetrical axis labels around the 5 corners
            let top_text = format!("INTELLECT {}%", int_val);
            let top_half_w = top_text.chars().count() as f64;
            ctx.print(
                (cx - top_half_w).max(-x_half + 1.0),
                (cy + r + 3.2).min(y_half - 1.5),
                Line::from(vec![
                    Span::styled("INTELLECT ", Style::default().fg(Color::Rgb(205, 208, 225)).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{}%", int_val), Style::default().fg(a_color).add_modifier(Modifier::BOLD)),
                ]),
            );

            // Top-Right: COMBAT
            ctx.print(
                (outer_pts[1].0 + 3.0).min(x_half - 24.0),
                outer_pts[1].1 + 0.5,
                Line::from(vec![
                    Span::styled(format!("{}% ", combat_val), Style::default().fg(a_color).add_modifier(Modifier::BOLD)),
                    Span::styled("COMBAT", Style::default().fg(Color::Rgb(190, 194, 215))),
                ]),
            );

            // Bottom-Right: DARKNESS
            ctx.print(
                (outer_pts[2].0 + 4.0).min(x_half - 28.0),
                (outer_pts[2].1 - 1.2).max(-y_half + 1.5),
                Line::from(vec![
                    Span::styled(format!("{}% ", dark_val), Style::default().fg(a_color).add_modifier(Modifier::BOLD)),
                    Span::styled("DARKNESS", Style::default().fg(Color::Rgb(190, 194, 215))),
                ]),
            );

            // Bottom-Left: EMOTION (right-aligned to vertex)
            let emo_str = format!("EMOTION {}%", emo_val);
            let emo_units = emo_str.chars().count() as f64 * 2.0;
            ctx.print(
                (outer_pts[3].0 - 4.0 - emo_units).max(-x_half + 1.0),
                (outer_pts[3].1 - 1.2).max(-y_half + 1.5),
                Line::from(vec![
                    Span::styled("EMOTION ", Style::default().fg(Color::Rgb(190, 194, 215))),
                    Span::styled(format!("{}%", emo_val), Style::default().fg(a_color).add_modifier(Modifier::BOLD)),
                ]),
            );

            // Top-Left: LORE (right-aligned to vertex)
            let lore_str = format!("LORE {}%", lore_val);
            let lore_units = lore_str.chars().count() as f64 * 2.0;
            ctx.print(
                (outer_pts[4].0 - 3.0 - lore_units).max(-x_half + 1.0),
                outer_pts[4].1 + 0.5,
                Line::from(vec![
                    Span::styled("LORE ", Style::default().fg(Color::Rgb(190, 194, 215))),
                    Span::styled(format!("{}%", lore_val), Style::default().fg(a_color).add_modifier(Modifier::BOLD)),
                ]),
            );
        });

    frame.render_widget(canvas, area);
}

/// Render the Bottom-Left Radar + Tone & Genre Affinities quadrant cleanly.
fn render_radar_and_summary(frame: &mut Frame, area: Rect, persona: &Persona) {
    let p_color = persona.primary_color;
    let a_color = persona.accent_color;
    let (analytical, energetic, tone_label, audience) = calculate_polarity(persona);

    let anal_chars = ((analytical as usize) * 12) / 100;
    let ener_chars = 12 - anal_chars;
    let anal_bar = "█".repeat(anal_chars);
    let ener_bar = "░".repeat(ener_chars);

    let ((action_val, action_desc), (mystery_val, mystery_desc), (world_val, world_desc), dominant) =
        get_primal_elements(persona);

    let mut action_line = vec![
        Span::styled(format!("   • {:<26}", action_desc), Style::default().fg(Color::Rgb(190, 190, 210))),
    ];
    action_line.extend(render_gauge_bar(action_val, 10, Color::Rgb(255, 110, 50)).spans);

    let mut mystery_line = vec![
        Span::styled(format!("   • {:<26}", mystery_desc), Style::default().fg(Color::Rgb(190, 190, 210))),
    ];
    mystery_line.extend(render_gauge_bar(mystery_val, 10, Color::Rgb(125, 207, 255)).spans);

    let mut world_line = vec![
        Span::styled(format!("   • {:<26}", world_desc), Style::default().fg(Color::Rgb(190, 190, 210))),
    ];
    world_line.extend(render_gauge_bar(world_val, 10, Color::Rgb(80, 250, 123)).spans);

    let summary_lines = vec![
        Line::from(Span::styled("   ── TONE & PACING BALANCE ───────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))),
        Line::from(vec![
            Span::styled("   [", Style::default().fg(Color::Rgb(90, 90, 120))),
            Span::styled(anal_bar, Style::default().fg(p_color)),
            Span::styled(ener_bar, Style::default().fg(a_color)),
            Span::styled("] ", Style::default().fg(Color::Rgb(90, 90, 120))),
            Span::styled(
                format!("{}% Analytical / {}% Action", analytical, energetic),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("   Tone: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(tone_label, Style::default().fg(p_color).add_modifier(Modifier::BOLD)),
            Span::styled("  ·  Audience: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(audience, Style::default().fg(Color::Rgb(200, 200, 220))),
        ]),
        Line::from(""),
        Line::from(Span::styled("   ── CORE GENRE AFFINITIES ────────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))),
        Line::from(action_line),
        Line::from(mystery_line),
        Line::from(world_line),
        Line::from(vec![
            Span::styled("   Primary Focus: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(dominant, Style::default().fg(p_color).add_modifier(Modifier::BOLD)),
        ]),
    ];

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Min(9),    // Symmetrical Pentagon Radar
            Constraint::Length(9), // Direct stats summary
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "   ── TASTE RADAR (5-AXIS PROFILE) ────────────────────────",
            Style::default().fg(Color::Rgb(140, 140, 160)),
        ))),
        chunks[0],
    );
    render_pentagon_radar(frame, chunks[1], persona);
    frame.render_widget(Paragraph::new(summary_lines), chunks[2]);
}

/// Render the 10-question wizard modal.
pub fn render_wizard_modal(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(76, 72, frame.area());
    frame.render_widget(Clear, area);

    let theme = state.theme();
    let questions = get_questions();
    let q = &questions[state.tailored_step.min(questions.len() - 1)];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(
            format!(" 󰋽 TASTE WIZARD • STEP {} OF 10 ", state.tailored_step + 1),
            Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Question title + subtitle + progress track
            Constraint::Min(0),    // Options list
            Constraint::Length(3), // Footer hints
        ])
        .split(inner);

    // Header & Progress track
    let mut progress_spans = Vec::new();
    for i in 0..10 {
        if i == state.tailored_step {
            progress_spans.push(Span::styled("◆ ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)));
        } else if i < state.tailored_step {
            progress_spans.push(Span::styled("◈ ", Style::default().fg(theme.accent)));
        } else {
            progress_spans.push(Span::styled("◇ ", Style::default().fg(Color::Rgb(70, 70, 90))));
        }
    }

    let header_lines = vec![
        Line::from(vec![
            Span::styled(format!(" {} ", q.symbol), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(q.title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("   ", Style::default()),
            Span::styled(q.subtitle, Style::default().fg(Color::Rgb(160, 160, 180))),
        ]),
        Line::from(progress_spans),
    ];
    frame.render_widget(Paragraph::new(header_lines), chunks[0]);

    // Options list
    let is_deal_breakers = q.is_multi_select;
    let mut option_lines = Vec::new();

    for (idx, opt) in q.options.iter().enumerate() {
        let is_focused = idx == state.tailored_cursor;
        let is_selected = if is_deal_breakers {
            state.tailored_answers.deal_breakers.contains(&idx)
        } else {
            state.tailored_answers.choices[state.tailored_step] == idx
        };

        let bullet = if is_deal_breakers {
            if is_selected { "[x] " } else { "[ ] " }
        } else {
            if is_selected { "[●] " } else { "[○] " }
        };

        let number_prefix = format!("{}. ", idx + 1);

        let row_style = if is_focused {
            Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(220, 220, 235))
        };

        option_lines.push(Line::from(vec![
            Span::styled(format!("  {} ", bullet), if is_selected { Style::default().fg(theme.accent).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Rgb(120, 120, 140)) }),
            Span::styled(number_prefix, Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(format!("{} ", opt.symbol), Style::default().fg(theme.secondary)),
            Span::styled(format!(" {} ", opt.label), row_style),
        ]));

        option_lines.push(Line::from(vec![
            Span::styled("        ", Style::default()),
            Span::styled(opt.detail, Style::default().fg(Color::Rgb(140, 140, 160))),
        ]));
        option_lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(option_lines), chunks[1]);

    // Footer hints
    let footer_text = if is_deal_breakers {
        Line::from(vec![
            Span::styled(" [Space] ", Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" Toggle Deal-Breaker  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(" Finish & View Constellation  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [b] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Back  ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(" [Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Exit", Style::default().fg(Color::Rgb(140, 140, 160))),
        ])
    } else {
        Line::from(vec![
            Span::styled(" [1-5] ", Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" Quick Pick  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(" Confirm & Next  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [b] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Prev  ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(" [Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Exit", Style::default().fg(Color::Rgb(140, 140, 160))),
        ])
    };

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));
    frame.render_widget(Paragraph::new(footer_text).block(footer_block), chunks[2]);
}

/// Render the interactive Solus Taste Constellation Dashboard (Option 1: 4-Quadrant Adventurer Plate).
pub fn render_taste_dashboard(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(94, 88, frame.area());
    frame.render_widget(Clear, area);

    let persona = calculate_persona(&state.tailored_answers);
    let questions = get_questions();

    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(persona.primary_color))
        .title(Span::styled(
            " 󰋽 SOLUS TASTE PROFILE • VIEWER SUMMARY ",
            Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD),
        ));

    let inner = main_block.inner(area);
    frame.render_widget(main_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // 4-Quadrant dashboard
            Constraint::Length(3), // Footer actions
        ])
        .split(inner);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(44), // Left Column: Persona Gauges + 2D Radar
            Constraint::Percentage(56), // Right Column: Attuned Matrix + Flavor & Telemetry
        ])
        .split(chunks[0]);

    // ── LEFT COLUMN QUADRANTS ───────────────────────────────────────────────────
    let left_quadrants = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(13), // Top-Left: Viewer Profile & Taste Gauges
            Constraint::Min(0),     // Bottom-Left: 5-Axis Pentagon Radar & Affinities
        ])
        .split(panes[0]);

    // Top-Left: Profile & Gauges
    let tl_block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::Rgb(65, 65, 85)));
    let tl_inner = tl_block.inner(left_quadrants[0]);
    frame.render_widget(tl_block, left_quadrants[0]);

    let mut tl_lines = vec![
        Line::from(Span::styled(" ── YOUR VIEWER PROFILE ────────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))),
        Line::from(vec![
            Span::styled(format!(" {} ", persona.title), Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(format!("\"{}\"", persona.epithet), Style::default().fg(Color::Rgb(180, 180, 205))),
        ]),
    ];

    let mut res_spans = vec![Span::styled(" Top Genres:   ", Style::default().fg(Color::Rgb(140, 140, 160)))];
    for &tag in &persona.resonance_tags {
        res_spans.push(Span::styled(
            format!(" {} ", tag),
            Style::default().fg(persona.primary_color).bg(Color::Rgb(30, 25, 45)).add_modifier(Modifier::BOLD),
        ));
        res_spans.push(Span::styled(" ", Style::default()));
    }
    tl_lines.push(Line::from(res_spans));
    tl_lines.push(Line::from(vec![
        Span::styled(" Match Engine: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("10-Factor Preference Model · Ready", Style::default().fg(Color::Rgb(190, 190, 210))),
    ]));
    tl_lines.push(Line::from(""));
    tl_lines.push(Line::from(Span::styled(" ── TASTE BREAKDOWN ────────────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))));

    for &(gauge_name, value) in &persona.gauges {
        let desc = get_dimension_descriptor(gauge_name, value);
        let bar_line = render_gauge_bar(value, 9, persona.primary_color);
        let mut line_spans = vec![
            Span::styled(format!(" {:<20} ", gauge_name), Style::default().fg(Color::Rgb(210, 210, 225))),
        ];
        line_spans.extend(bar_line.spans);
        line_spans.push(Span::styled(format!(" · {}", desc), Style::default().fg(Color::Rgb(150, 150, 175))));
        tl_lines.push(Line::from(line_spans));
    }
    frame.render_widget(Paragraph::new(tl_lines), tl_inner);

    // Bottom-Left: 5-Axis Symmetrical Pentagon Radar & Affinities
    let bl_block = Block::default()
        .borders(Borders::TOP | Borders::RIGHT)
        .border_style(Style::default().fg(Color::Rgb(65, 65, 85)));
    let bl_inner = bl_block.inner(left_quadrants[1]);
    frame.render_widget(bl_block, left_quadrants[1]);

    render_radar_and_summary(frame, bl_inner, &persona);

    // ── RIGHT COLUMN QUADRANTS ──────────────────────────────────────────────────
    let right_quadrants = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(14), // Top-Right: Interactive 10-Attribute Attunement Matrix
            Constraint::Min(0),     // Bottom-Right: Flavor Notes + Kinship + Suggest Button
        ])
        .split(panes[1]);

    // Top-Right: Interactive Answer Matrix
    let tr_inner = right_quadrants[0];
    let mut tr_lines = vec![
        Line::from(vec![
            Span::styled(" ── YOUR SELECTIONS ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled("[Navigate: j/k, Quick-Edit: Enter] ──────────────────", Style::default().fg(persona.accent_color)),
        ]),
    ];

    for (idx, q) in questions.iter().enumerate().take(10) {
        let is_cursor = state.tailored_summary_cursor == idx;

        let answer_text = if idx == 9 {
            if state.tailored_answers.deal_breakers.is_empty() {
                "None (Open to all)".to_string()
            } else {
                let labels: Vec<&str> = state
                    .tailored_answers
                    .deal_breakers
                    .iter()
                    .filter_map(|&d| q.options.get(d).map(|o| o.label))
                    .collect();
                labels.join(" · ")
            }
        } else {
            let choice_idx = state.tailored_answers.choices[idx];
            q.options.get(choice_idx).map(|o| o.label).unwrap_or("Unset").to_string()
        };

        let short_question_name = match idx {
            0 => "1. Mood",
            1 => "2. Length",
            2 => "3. Realm",
            3 => "4. Hero",
            4 => "5. Conflict",
            5 => "6. Darkness",
            6 => "7. Visual Era",
            7 => "8. Status",
            8 => "9. Lore Depth",
            9 => "10. Exclude",
            _ => "Question",
        };

        let active_pill = get_question_active_pill(&state.tailored_answers, idx);

        let tile_style = if is_cursor {
            Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let badge = if is_cursor {
            Span::styled("[Edit 󰏫]  ", Style::default().fg(Color::Black).bg(Color::Rgb(250, 250, 250)).add_modifier(Modifier::BOLD))
        } else {
            Span::styled("[󰄬]     ", Style::default().fg(persona.primary_color))
        };

        tr_lines.push(Line::from(vec![
            badge,
            Span::styled(format!(" {:<13}", short_question_name), Style::default().fg(persona.primary_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {:<26} ", answer_text.chars().take(26).collect::<String>()), tile_style),
            Span::styled(format!(" {:<18}", active_pill), Style::default().fg(Color::Rgb(140, 140, 165))),
        ]));
    }
    tr_lines.push(Line::from(vec![
        Span::styled(" ── Profile Status: 10/10 Preferences Set · Confidence: 99% ───────────", Style::default().fg(Color::Rgb(90, 90, 115))),
    ]));
    frame.render_widget(Paragraph::new(tr_lines), tr_inner);

    // Bottom-Right: Taste Tags + Similar Anime + Summary + Suggest Button
    let br_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(65, 65, 85)));
    let br_inner = br_block.inner(right_quadrants[1]);
    frame.render_widget(br_block, right_quadrants[1]);

    let mut br_lines = vec![
        Line::from(Span::styled(" ── SELECTED PREFERENCE TAGS ────────────────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))),
    ];

    let tasting_notes = get_tasting_notes(&state.tailored_answers);
    for chunk in tasting_notes.chunks(2) {
        let mut row_spans = vec![Span::styled("  ", Style::default())];
        for &note in chunk {
            row_spans.push(Span::styled(
                format!(" [ {} ] ", note),
                Style::default().fg(persona.primary_color).bg(Color::Rgb(30, 26, 44)).add_modifier(Modifier::BOLD),
            ));
            row_spans.push(Span::styled("  ", Style::default()));
        }
        br_lines.push(Line::from(row_spans));
    }

    br_lines.push(Line::from(""));
    br_lines.push(Line::from(Span::styled(" ── SIMILAR LANDMARK ANIME ────────────────────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))));
    br_lines.push(Line::from(Span::styled("  Reference series that match your current preferences:", Style::default().fg(Color::Rgb(150, 150, 170)))));
    let kinship = get_spiritual_kinship_details(&state.tailored_answers);
    for (k_idx, &(title, vibe)) in kinship.iter().enumerate() {
        if k_idx % 2 == 0 {
            let next_title = kinship.get(k_idx + 1);
            let mut line_spans = vec![
                Span::styled("  • ", Style::default().fg(persona.primary_color)),
                Span::styled(format!("{:<18}", title), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" ({})  ", vibe), Style::default().fg(Color::Rgb(140, 140, 160))),
            ];
            if let Some(&(t2, v2)) = next_title {
                line_spans.push(Span::styled("• ", Style::default().fg(persona.primary_color)));
                line_spans.push(Span::styled(format!("{:<18}", t2), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
                line_spans.push(Span::styled(format!(" ({})", v2), Style::default().fg(Color::Rgb(140, 140, 160))));
            }
            br_lines.push(Line::from(line_spans));
        }
    }

    br_lines.push(Line::from(""));
    br_lines.push(Line::from(Span::styled(" ── CATALOG SUMMARY ───────────────────────────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))));
    let veto_count = state.tailored_answers.deal_breakers.len() * 16;
    br_lines.push(Line::from(vec![
        Span::styled("  Candidate Pool: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("300 Titles Evaluated in Local Cache", Style::default().fg(Color::White)),
        Span::styled("  · Excluded: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled(format!("{} Filtered", veto_count), Style::default().fg(Color::Rgb(255, 120, 120))),
    ]));
    br_lines.push(Line::from(vec![
        Span::styled("  Best Match:     ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("99% Profile Fit Projected", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
        Span::styled("  · Format: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("Finished 1-Cour TV Series", Style::default().fg(Color::Rgb(190, 190, 210))),
    ]));

    // Suggest Button (Cursor Index 10)
    let is_suggest_cursor = state.tailored_summary_cursor == 10;
    let suggest_style = if is_suggest_cursor {
        Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(persona.primary_color).bg(Color::Rgb(35, 25, 55)).add_modifier(Modifier::BOLD)
    };

    br_lines.push(Line::from(""));
    br_lines.push(Line::from(Span::styled(
        "  ╔═══════════════════════════════════════════════════════════════════╗",
        Style::default().fg(if is_suggest_cursor { persona.primary_color } else { Color::Rgb(70, 70, 95) }),
    )));
    br_lines.push(Line::from(vec![
        Span::styled("  ║       ", Style::default().fg(if is_suggest_cursor { persona.primary_color } else { Color::Rgb(70, 70, 95) })),
        Span::styled(
            " ▶ SUGGEST ANIME RECOMMENDATIONS [Enter / s] ▶ ",
            suggest_style,
        ),
        Span::styled("       ║", Style::default().fg(if is_suggest_cursor { persona.primary_color } else { Color::Rgb(70, 70, 95) })),
    ]));
    br_lines.push(Line::from(Span::styled(
        "  ╚═══════════════════════════════════════════════════════════════════╝",
        Style::default().fg(if is_suggest_cursor { persona.primary_color } else { Color::Rgb(70, 70, 95) }),
    )));
    br_lines.push(Line::from(Span::styled(
        "       Generates up to 30 tailored anime picks with posters & synopses",
        Style::default().fg(Color::Rgb(130, 130, 150)),
    )));

    frame.render_widget(Paragraph::new(br_lines), br_inner);

    // Footer actions
    let footer_text = Line::from(vec![
        Span::styled(" [j/k] ", Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD)),
        Span::styled(" Select Attribute  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)),
        Span::styled(" Quick-Edit Answer / Suggest  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(" [s] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Instant Suggest  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(" [Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Back to Home", Style::default().fg(Color::Rgb(140, 140, 160))),
    ]);

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));
    frame.render_widget(Paragraph::new(footer_text).block(footer_block), chunks[1]);

    // If currently editing a question from the dashboard, render quick-edit overlay on top!
    if let Some(edit_idx) = state.tailored_editing_step {
        render_quick_edit_overlay(frame, state, edit_idx, &persona);
    }
}

/// Render the in-place quick-edit modal when modifying a single answer from the dashboard.
fn render_quick_edit_overlay(frame: &mut Frame, state: &AppState, step_idx: usize, persona: &crate::services::tailored::Persona) {
    let area = centered_rect(65, 55, frame.area());
    frame.render_widget(Clear, area);

    let questions = get_questions();
    let q = &questions[step_idx.min(questions.len() - 1)];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(persona.accent_color))
        .title(Span::styled(
            format!(" 󰏫 EDIT PREFERENCE: {} ", q.title.to_uppercase()),
            Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Subtitle
            Constraint::Min(0),    // Choices
            Constraint::Length(2), // Confirm hint
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(q.subtitle, Style::default().fg(Color::Rgb(180, 180, 200))),
        ])),
        chunks[0],
    );

    let is_deal_breakers = q.is_multi_select;
    let mut lines = Vec::new();

    for (idx, opt) in q.options.iter().enumerate() {
        let is_cursor = idx == state.tailored_cursor;
        let is_selected = if is_deal_breakers {
            state.tailored_answers.deal_breakers.contains(&idx)
        } else {
            state.tailored_answers.choices[step_idx] == idx
        };

        let bullet = if is_deal_breakers {
            if is_selected { "[x] " } else { "[ ] " }
        } else {
            if is_selected { "[●] " } else { "[○] " }
        };

        let row_style = if is_cursor {
            Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(persona.accent_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", bullet), if is_selected { Style::default().fg(persona.accent_color) } else { Style::default().fg(Color::Rgb(120, 120, 140)) }),
            Span::styled(format!("{} ", opt.symbol), Style::default().fg(persona.primary_color)),
            Span::styled(format!(" {} ", opt.label), row_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("        ", Style::default()),
            Span::styled(opt.detail, Style::default().fg(Color::Rgb(140, 140, 160))),
        ]));
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(lines), chunks[1]);

    let footer = Line::from(vec![
        Span::styled(" [Enter/Space] ", Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)),
        Span::styled(" Save & Recalculate Diagram   ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(" [Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Cancel", Style::default().fg(Color::Rgb(140, 140, 160))),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[2]);
}

/// Render the Tailored Anime Results shelf with cover artwork, rich synopsis, and Live Vibe Tuner.
pub fn render_tailored_results(frame: &mut Frame, state: &mut AppState) {
    let area = frame.area();
    frame.render_widget(Clear, area);

    let persona = calculate_persona(&state.tailored_answers);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(persona.primary_color))
        .title(Span::styled(
            format!(" 󰋽 TAILORED RECOMMENDATIONS • {} MATCHES ", state.tailored_results.len()),
            Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let has_active_tuner = state.tuner_config.has_active_modifiers();
    let tuner_height = if state.tuner_open {
        5
    } else if has_active_tuner {
        1
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),            // Persona banner
            Constraint::Length(tuner_height), // Live Vibe Tuner dock (when open or active)
            Constraint::Min(0),               // Two columns: Left (Ranked list), Right (Selected preview)
            Constraint::Length(2),            // Footer hints
        ])
        .split(inner);

    // Profile header banner
    let mut banner_spans = vec![
        Span::styled(" Taste Profile: ", Style::default().fg(Color::Rgb(160, 160, 180))),
        Span::styled(format!(" {} ", persona.title), Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(Color::Rgb(80, 80, 100))),
        Span::styled(persona.epithet, Style::default().fg(Color::Rgb(200, 200, 220))),
    ];
    if let Some((_, ref anchor_title, _)) = state.tuner_config.anchor_anime {
        banner_spans.push(Span::styled("   ", Style::default()));
        banner_spans.push(Span::styled(
            format!(" 󰄬 Similar to: {} ", anchor_title),
            Style::default().fg(Color::Black).bg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(banner_spans)), chunks[0]);

    // ── LIVE VIBE TUNER DOCK ───────────────────────────────────────────────────
    if state.tuner_open {
        render_live_tuner_dock(frame, state, chunks[1], &persona);
    } else if has_active_tuner {
        render_active_tuner_summary(frame, state, chunks[1], &persona);
    }

    if state.tailored_results.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("No candidates matched all active Live Tuner pills & filters.", Style::default().fg(Color::Rgb(255, 110, 110)).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(Span::styled("Press [c] to Reset Live Tuner filters, [f] to open the Live Tuner, or [r] to adjust your Taste Profile.", Style::default().fg(Color::Rgb(180, 180, 200)))),
        ]).alignment(Alignment::Center);
        frame.render_widget(msg, chunks[2]);
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(44), // Left: Recommendation list
                Constraint::Percentage(56), // Right: Show synopsis, artwork & details
            ])
            .split(chunks[2]);

        // ── LEFT COLUMN: Ranked Recommendation List ────────────────────────────
        let left_block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));
        let left_inner = left_block.inner(cols[0]);
        frame.render_widget(left_block, cols[0]);

        let mut list_lines = Vec::new();
        let visible_count = (left_inner.height as usize / 3).max(1);
        let offset = if state.tailored_results_cursor >= visible_count {
            state.tailored_results_cursor - visible_count + 1
        } else {
            0
        };

        for (idx, m) in state.tailored_results.iter().enumerate().skip(offset).take(visible_count) {
            let is_focused = idx == state.tailored_results_cursor;
            let match_badge = format!(" [{}% MATCH] ", m.match_percentage);

            let row_style = if is_focused {
                Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            };

            let badge_style = if m.match_percentage >= 90 {
                Style::default().fg(Color::Black).bg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)
            };

            list_lines.push(Line::from(vec![
                Span::styled(format!(" {:>2}. ", idx + 1), Style::default().fg(Color::Rgb(140, 140, 160))),
                Span::styled(match_badge, badge_style),
                Span::styled(format!(" {} ", m.anime.short_title()), row_style),
            ]));

            let mut reason_spans = vec![Span::styled("       ", Style::default())];
            for r in &m.reason_tags {
                reason_spans.push(Span::styled(format!(" {} ", r), Style::default().fg(persona.primary_color).bg(Color::Rgb(30, 28, 42))));
                reason_spans.push(Span::styled(" ", Style::default()));
            }
            list_lines.push(Line::from(reason_spans));
            list_lines.push(Line::from(""));
        }

        frame.render_widget(Paragraph::new(list_lines), left_inner);

        // ── RIGHT COLUMN: Selected Anime Details, Artwork & Storytelling ───────
        if let Some(focused) = state.tailored_results.get(state.tailored_results_cursor) {
            let anime = focused.anime.clone();
            let right_inner = cols[1];

            let right_sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(16), // Top: Cover Thumbnail + Specs & Resonance
                    Constraint::Min(0),     // Bottom: Narrative Chronicle & Enhanced Synopsis
                ])
                .split(right_inner);

            // Top section split: Poster art + Metadata
            let top_cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(20), // Moderately large poster (20 cols x 16 rows, inner 18x14)
                    Constraint::Length(1),  // Spacer
                    Constraint::Min(0),     // Metadata & Alignment details
                ])
                .split(right_sections[0]);

            // Cover box with stylish rounded border
            let cover_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(persona.primary_color));
            let cover_inner = cover_block.inner(top_cols[0]);
            frame.render_widget(cover_block, top_cols[0]);

            if state.has_image_support() {
                if let Some(cover) = state.detail_cache.get_mut(&anime.id)
                    .or_else(|| state.banner_cache.get_mut(&anime.id))
                    .or_else(|| state.cover_cache.get_mut(&anime.id))
                {
                    let image_widget = ratatui_image::StatefulImage::new(None)
                        .resize(ratatui_image::Resize::Fit(None));
                    frame.render_stateful_widget(image_widget, cover_inner, cover);
                } else {
                    let loading_label = if state.cover_failed_anime_id == Some(anime.id) {
                        "Poster\nUnavailable"
                    } else {
                        "Loading\nPoster..."
                    };
                    let loading = Paragraph::new(loading_label)
                        .style(Style::default().fg(persona.primary_color))
                        .alignment(Alignment::Center);
                    frame.render_widget(loading, cover_inner);
                }
            } else {
                frame.render_widget(
                    crate::ui::components::cover::HalfblockCover { anime_id: anime.id, title: anime.display_title() },
                    cover_inner,
                );
            }

            // Right side of top section: Metadata
            let title = anime.display_title();
            let subtitle = if anime.title_romaji != title && !anime.title_romaji.is_empty() {
                Some(anime.title_romaji.clone())
            } else {
                anime.title_native.clone()
            };
            let score_str = anime.score.map(|s| format!(" {:.1}", s as f32 / 10.0)).unwrap_or_else(|| " ?".to_string());
            let eps_str = anime.episodes.map(|e| format!("{} eps", e)).unwrap_or_else(|| "? eps".to_string());
            let status_str = anime.status.as_deref().unwrap_or("UNKNOWN");
            let format_str = anime.format.as_deref().unwrap_or("TV");

            let mut meta_lines = vec![
                Line::from(vec![
                    Span::styled(format!("{} ", title), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
            ];
            if let Some(sub) = subtitle {
                meta_lines.push(Line::from(Span::styled(format!("  {}", sub), Style::default().fg(Color::Rgb(140, 140, 160)))));
            }

            let match_badge = format!(" [{}% TASTE MATCH] ", focused.match_percentage);
            meta_lines.push(Line::from(""));
            meta_lines.push(Line::from(vec![
                Span::styled(match_badge, Style::default().fg(Color::Black).bg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
                Span::styled(" ", Style::default()),
                Span::styled(format!(" {} ", score_str), Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {} · {} · {}", eps_str, format_str, status_str), Style::default().fg(Color::Rgb(200, 200, 220))),
            ]));

            meta_lines.push(Line::from(""));
            let mut genre_spans = vec![Span::styled("Genres: ", Style::default().fg(Color::Rgb(140, 140, 160)))];
            for (g_idx, g) in anime.genre_list().iter().take(4).enumerate() {
                if g_idx > 0 {
                    genre_spans.push(Span::styled(" · ", Style::default().fg(Color::Rgb(80, 80, 100))));
                }
                genre_spans.push(Span::styled(format!("[{}]", g), Style::default().fg(persona.primary_color)));
            }
            meta_lines.push(Line::from(genre_spans));

            meta_lines.push(Line::from(""));
            meta_lines.push(Line::from(vec![
                Span::styled("󰄬 Profile Fit:  ", Style::default().fg(persona.accent_color).add_modifier(Modifier::BOLD)),
                Span::styled("Matched cleanly to your Taste Profile & Mood.", Style::default().fg(Color::Rgb(180, 180, 200))),
            ]));
            let guard_note = if state.tuner_config.hide_sequels {
                "Standalone / Season 1 Entry (Sequels hidden)"
            } else {
                "All Seasons & Sequels Included"
            };
            meta_lines.push(Line::from(vec![
                Span::styled("󰄬 Sequel Guard: ", Style::default().fg(Color::Rgb(80, 250, 123))),
                Span::styled(guard_note, Style::default().fg(Color::Rgb(160, 160, 180))),
            ]));

            frame.render_widget(Paragraph::new(meta_lines), top_cols[2]);

            // ── BOTTOM SECTION: Story Synopsis & Overview ─────────────────────
            let bottom_block = Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::Rgb(65, 65, 85)))
                .title(Span::styled(
                    " ── STORY SYNOPSIS & OVERVIEW ── ",
                    Style::default().fg(persona.primary_color).add_modifier(Modifier::BOLD),
                ));
            let bottom_inner = bottom_block.inner(right_sections[1]);
            frame.render_widget(bottom_block, right_sections[1]);

            let hook = get_thematic_hook(&anime);
            let raw_desc = anime.description.as_deref().unwrap_or("No description available.");
            let cleaned_synopsis = clean_synopsis(raw_desc);

            let mut story_lines = vec![
                Line::from(Span::styled(hook, Style::default().fg(persona.accent_color).add_modifier(Modifier::ITALIC))),
                Line::from(""),
            ];

            for para in cleaned_synopsis.split("\n\n") {
                let p_trim = para.trim();
                if !p_trim.is_empty() {
                    story_lines.push(Line::from(Span::styled(
                        p_trim.replace('\n', " "),
                        Style::default().fg(Color::Rgb(215, 215, 230)),
                    )));
                    story_lines.push(Line::from(""));
                }
            }

            let synopsis_p = Paragraph::new(story_lines)
                .wrap(ratatui::widgets::Wrap { trim: true });
            frame.render_widget(synopsis_p, bottom_inner);
        }
    }

    // Footer actions (context-aware: tuner hints when dock is open, browse hints otherwise)
    let footer_text = if state.tuner_open {
        Line::from(vec![
            Span::styled(" [h/l] ", Style::default().fg(Color::Black).bg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            Span::styled(" Select Pill  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [j/k] ", Style::default().fg(Color::Black).bg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD)),
            Span::styled(" Switch Row  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [Space/Enter] ", Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD)),
            Span::styled(" Toggle (+Boost / -Hide)  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [c] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Reset  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [f/Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Close Tuner", Style::default().fg(Color::Rgb(140, 140, 160))),
        ])
    } else {
        Line::from(vec![
            Span::styled(" [f] ", Style::default().fg(Color::Black).bg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            Span::styled(" Live Tuner  ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
            Span::styled(" [m] ", Style::default().fg(Color::Black).bg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD)),
            Span::styled(" More Like This  ", Style::default().fg(Color::Rgb(139, 233, 253)).add_modifier(Modifier::BOLD)),
            Span::styled(" [x] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Dismiss  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD)),
            Span::styled(" Play  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [d] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Details  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [+] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Watchlist  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [r] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Profile  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Home", Style::default().fg(Color::Rgb(140, 140, 160))),
        ])
    };

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));
    frame.render_widget(Paragraph::new(footer_text).block(footer_block), chunks[3]);
}

/// Render the expanded zero-question interactive Live Vibe Tuner bar above the results list.
fn render_live_tuner_dock(
    frame: &mut Frame,
    state: &AppState,
    area: Rect,
    persona: &crate::services::tailored::Persona,
) {
    let emerald = Color::Rgb(80, 250, 123);
    let cyan = Color::Rgb(139, 233, 253);
    let red = Color::Rgb(255, 110, 110);

    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(emerald))
        .title(Span::styled(
            " 󰒓 LIVE VIBE TUNER (h/l select, j/k row, Space toggle, f/Esc close) ",
            Style::default().fg(emerald).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Row 0: Discovery Tiers & Quick Toggles (7 interactive pills: 0..=6)
    let mut row0_spans = vec![Span::styled(" Discovery: ", Style::default().fg(Color::Rgb(150, 150, 175)))];
    let row0_items = [
        (0usize, "All Picks", state.tuner_config.discovery_tier == 0),
        (1usize, "Hidden Gems", state.tuner_config.discovery_tier == 1),
        (2usize, "Acclaimed 82%+", state.tuner_config.discovery_tier == 2),
        (3usize, "Modern 2022+", state.tuner_config.discovery_tier == 3),
        (4usize, "Hide Sequels", state.tuner_config.hide_sequels),
        (5usize, "Finished Only", state.tuner_config.finished_only),
        (6usize, "Reset", false),
    ];

    for (col_idx, label, is_on) in row0_items {
        let focused = state.tuner_row == 0 && state.tuner_col == col_idx;
        let prefix = if col_idx == 6 {
            "󰑐 "
        } else if is_on {
            "● "
        } else {
            "○ "
        };
        let pill_text = format!(" {}{} ", prefix, label);
        let style = if focused {
            Style::default().fg(Color::Black).bg(cyan).add_modifier(Modifier::BOLD)
        } else if is_on {
            Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(185, 185, 205)).bg(Color::Rgb(34, 32, 48))
        };
        row0_spans.push(Span::styled(pill_text, style));
        row0_spans.push(Span::raw(" "));
    }

    // Row 1: Dynamic Tag Boost (`+`) / Hide (`-`) Chips
    let mut row1_spans = vec![Span::styled(" Tune Tags: ", Style::default().fg(Color::Rgb(150, 150, 175)))];
    if state.tuner_available_tags.is_empty() {
        row1_spans.push(Span::styled("No tag chips extracted yet.", Style::default().fg(Color::Rgb(120, 120, 140))));
    } else {
        for (col_idx, tag) in state.tuner_available_tags.iter().enumerate() {
            let focused = state.tuner_row == 1 && state.tuner_col == col_idx;
            let is_boosted = state.tuner_config.boosted_tags.contains(tag);
            let is_muted = state.tuner_config.muted_tags.contains(tag);

            let (prefix, base_style) = if is_boosted {
                ("+ ", Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD))
            } else if is_muted {
                ("- ", Style::default().fg(Color::Black).bg(red).add_modifier(Modifier::BOLD))
            } else {
                ("○ ", Style::default().fg(Color::Rgb(185, 185, 205)).bg(Color::Rgb(34, 32, 48)))
            };

            let style = if focused {
                Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            row1_spans.push(Span::styled(format!(" {}{} ", prefix, tag), style));
            row1_spans.push(Span::raw(" "));
        }
    }

    let hint_line = Line::from(Span::styled(
        " Tip: [Space] cycles tags (Neutral -> +Boost -> -Hide). Press [m] on any anime for Similar Picks.",
        Style::default().fg(Color::Rgb(130, 130, 155)),
    ));

    frame.render_widget(
        Paragraph::new(vec![Line::from(row0_spans), Line::from(row1_spans), hint_line]),
        inner,
    );
}

/// Compact 1-line strip shown when the Live Vibe Tuner drawer is closed, summarizing active modifiers.
fn render_active_tuner_summary(
    frame: &mut Frame,
    state: &AppState,
    area: Rect,
    persona: &crate::services::tailored::Persona,
) {
    let emerald = Color::Rgb(80, 250, 123);
    let red = Color::Rgb(255, 110, 110);
    let mut spans = vec![Span::styled(" Active Tuners: ", Style::default().fg(Color::Rgb(140, 140, 165)))];

    match state.tuner_config.discovery_tier {
        1 => spans.push(Span::styled(" Hidden Gems ", Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD))),
        2 => spans.push(Span::styled(" Acclaimed 82%+ ", Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD))),
        3 => spans.push(Span::styled(" Modern 2022+ ", Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD))),
        _ => {}
    }
    if state.tuner_config.finished_only {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(" Finished Only ", Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)));
    }
    if !state.tuner_config.hide_sequels {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(" Sequels Allowed ", Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD)));
    }
    for t in &state.tuner_config.boosted_tags {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!(" +{} ", t), Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD)));
    }
    for t in &state.tuner_config.muted_tags {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!(" -{} ", t), Style::default().fg(Color::Black).bg(red).add_modifier(Modifier::BOLD)));
    }
    if !state.tuner_config.dismissed_ids.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!(" {} Hidden ", state.tuner_config.dismissed_ids.len()),
            Style::default().fg(Color::Rgb(180, 180, 200)).bg(Color::Rgb(40, 38, 55)),
        ));
    }
    spans.push(Span::styled("  ([c] Reset · [f] Tune)", Style::default().fg(Color::Rgb(120, 120, 145))));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
