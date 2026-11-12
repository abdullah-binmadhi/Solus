//! UI rendering for the Tailored Algorithm:
//! 1. Questionnaire Wizard modal
//! 2. Solus Taste Constellation & Interactive Persona Dashboard
//! 3. Tailored Recommendations Shelf

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
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

/// Helper to render the 2D Astral Star Constellation Radar & Aetheric Polarity meter.
fn render_celestial_radar(persona: &Persona) -> Vec<Line<'static>> {
    let int_val = persona.gauges[0].1;
    let lore_val = persona.gauges[1].1;
    let emo_val = persona.gauges[2].1;
    let combat_val = persona.gauges[3].1;
    let dark_val = persona.gauges[4].1;

    let node_glyph = |v: u8| -> &'static str {
        if v >= 85 { "❖" } else if v >= 60 { "✦" } else { "✧" }
    };

    let p_color = persona.primary_color;
    let a_color = persona.accent_color;
    let line_star = Style::default().fg(Color::Rgb(70, 70, 100));

    let (umbral, astral, aspect, demographic) = calculate_polarity(persona);

    let umbral_chars = ((umbral as usize) * 12) / 100;
    let astral_chars = 12 - umbral_chars;
    let umbral_bar = "█".repeat(umbral_chars);
    let astral_bar = "░".repeat(astral_chars);

    let ((fire_val, fire_desc), (wind_val, wind_desc), (earth_val, earth_desc), dominant) = get_primal_elements(persona);
    let mut fire_line = vec![
        Span::styled(format!("   ✦ {:<32}", fire_desc), Style::default().fg(Color::Rgb(190, 190, 210))),
    ];
    fire_line.extend(render_gauge_bar(fire_val, 10, Color::Rgb(255, 110, 50)).spans);

    let mut wind_line = vec![
        Span::styled(format!("   ✦ {:<32}", wind_desc), Style::default().fg(Color::Rgb(190, 190, 210))),
    ];
    wind_line.extend(render_gauge_bar(wind_val, 10, Color::Rgb(125, 207, 255)).spans);

    let mut earth_line = vec![
        Span::styled(format!("   ✦ {:<32}", earth_desc), Style::default().fg(Color::Rgb(190, 190, 210))),
    ];
    earth_line.extend(render_gauge_bar(earth_val, 10, Color::Rgb(80, 250, 123)).spans);

    let dom_line = vec![
        Span::styled("   Dominant: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled(dominant, Style::default().fg(p_color).add_modifier(Modifier::BOLD)),
    ];

    let vel_line = vec![
        Span::styled("   Velocity: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("Measured 1-Cour Arc  ·  Spectrum: 80% Uplifting / 20% Bittersweet", Style::default().fg(Color::Rgb(200, 200, 220))),
    ];

    vec![
        Line::from(Span::styled("   ── ✦ CELESTIAL ASTROLOGIAN RADAR ──", Style::default().fg(Color::Rgb(140, 140, 160)))),
        Line::from(vec![
            Span::styled("              [INTELLECT] ", Style::default().fg(Color::Rgb(200, 200, 225))),
            Span::styled(format!("{}%", int_val), Style::default().fg(a_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("                   ", Style::default()),
            Span::styled(node_glyph(int_val), Style::default().fg(p_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("                  / \\", line_star),
        ]),
        Line::from(vec![
            Span::styled("                 /   \\", line_star),
        ]),
        Line::from(vec![
            Span::styled("  [LORE] ", Style::default().fg(Color::Rgb(180, 180, 200))),
            Span::styled(format!("{}% ", lore_val), Style::default().fg(a_color)),
            Span::styled(node_glyph(lore_val), Style::default().fg(p_color).add_modifier(Modifier::BOLD)),
            Span::styled(" ─────── ", line_star),
            Span::styled(node_glyph(combat_val), Style::default().fg(p_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}% ", combat_val), Style::default().fg(a_color)),
            Span::styled("[COMBAT]", Style::default().fg(Color::Rgb(180, 180, 200))),
        ]),
        Line::from(vec![
            Span::styled("                 \\   /", line_star),
        ]),
        Line::from(vec![
            Span::styled("                  \\ /", line_star),
        ]),
        Line::from(vec![
            Span::styled("   [EMO] ", Style::default().fg(Color::Rgb(180, 180, 200))),
            Span::styled(format!("{}% ", emo_val), Style::default().fg(a_color)),
            Span::styled(node_glyph(emo_val), Style::default().fg(p_color)),
            Span::styled("          ", Style::default()),
            Span::styled(node_glyph(dark_val), Style::default().fg(p_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}% ", dark_val), Style::default().fg(a_color)),
            Span::styled("[DARK]", Style::default().fg(Color::Rgb(180, 180, 200))),
        ]),
        Line::from(""),
        Line::from(Span::styled("   ── AETHERIC BALANCE & SOUL ASPECT ──", Style::default().fg(Color::Rgb(140, 140, 160)))),
        Line::from(vec![
            Span::styled("   [", Style::default().fg(Color::Rgb(90, 90, 120))),
            Span::styled(umbral_bar, Style::default().fg(p_color)),
            Span::styled(astral_bar, Style::default().fg(a_color)),
            Span::styled("] ", Style::default().fg(Color::Rgb(90, 90, 120))),
            Span::styled(format!("{}% Umbral / {}% Astral", umbral, astral), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("   Aspect: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(aspect, Style::default().fg(p_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("   Affinity: ", Style::default().fg(Color::Rgb(140, 140, 160))),
            Span::styled(demographic, Style::default().fg(Color::Rgb(200, 200, 220))),
        ]),
        Line::from(""),
        Line::from(Span::styled("   ── PRIMAL ELEMENTAL ATTUNEMENT ──", Style::default().fg(Color::Rgb(140, 140, 160)))),
        Line::from(fire_line),
        Line::from(wind_line),
        Line::from(earth_line),
        Line::from(dom_line),
        Line::from(""),
        Line::from(Span::styled("   ── NARRATIVE VELOCITY & DISPERSION ──", Style::default().fg(Color::Rgb(140, 140, 160)))),
        Line::from(vel_line),
    ]
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
            format!(" ◈ THE SOLUS TASTE WIZARD • STEP {} OF 10 ◈ ", state.tailored_step + 1),
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
            " ❖ THE SOLUS TASTE CONSTELLATION • SOUL PERSONA ❖ ",
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
            Constraint::Length(14), // Top-Left: Persona & Dimensional Gauges
            Constraint::Min(0),     // Bottom-Left: 2D Celestial Constellation Radar
        ])
        .split(panes[0]);

    // Top-Left: Persona & Gauges
    let tl_block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::Rgb(65, 65, 85)));
    let tl_inner = tl_block.inner(left_quadrants[0]);
    frame.render_widget(tl_block, left_quadrants[0]);

    let mut tl_lines = vec![
        Line::from(Span::styled(" ── YOUR ANIME SOUL PERSONA ──────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))),
        Line::from(vec![
            Span::styled(format!(" {} ", persona.title), Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(format!("\"{}\"", persona.epithet), Style::default().fg(Color::Rgb(180, 180, 205))),
        ]),
    ];

    let mut res_spans = vec![Span::styled(" Resonances: ", Style::default().fg(Color::Rgb(140, 140, 160)))];
    for &tag in &persona.resonance_tags {
        res_spans.push(Span::styled(
            format!("✦ {} ", tag),
            Style::default().fg(persona.primary_color).bg(Color::Rgb(30, 25, 45)).add_modifier(Modifier::BOLD),
        ));
        res_spans.push(Span::styled(" ", Style::default()));
    }
    tl_lines.push(Line::from(res_spans));
    tl_lines.push(Line::from(vec![
        Span::styled(" Soul Crystal: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("Azem Fourteenth Seat Resonant · Primal Attunement", Style::default().fg(Color::Rgb(190, 190, 210))),
    ]));
    tl_lines.push(Line::from(""));
    tl_lines.push(Line::from(Span::styled(" ── DIMENSIONAL TASTE GAUGES ────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))));

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

    // Bottom-Left: 2D Celestial Constellation Radar & Polarity
    let bl_block = Block::default()
        .borders(Borders::TOP | Borders::RIGHT)
        .border_style(Style::default().fg(Color::Rgb(65, 65, 85)));
    let bl_inner = bl_block.inner(left_quadrants[1]);
    frame.render_widget(bl_block, left_quadrants[1]);

    let radar_lines = render_celestial_radar(&persona);
    frame.render_widget(Paragraph::new(radar_lines), bl_inner);

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
            Span::styled(" ── ◈ ATTUNED MATRIX ", Style::default().fg(Color::Rgb(140, 140, 160))),
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
            Span::styled("[Attune 󰏫]", Style::default().fg(Color::Black).bg(Color::Rgb(250, 250, 250)).add_modifier(Modifier::BOLD))
        } else {
            Span::styled("[󰄬]", Style::default().fg(persona.primary_color))
        };

        tr_lines.push(Line::from(vec![
            badge,
            Span::styled(format!(" {:<13}", short_question_name), Style::default().fg(persona.primary_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {:<26} ", answer_text.chars().take(26).collect::<String>()), tile_style),
            Span::styled(format!(" {:<18}", active_pill), Style::default().fg(Color::Rgb(140, 140, 165))),
        ]));
    }
    tr_lines.push(Line::from(vec![
        Span::styled(" ── Vector Resonance: 10/10 Harmonized · Confidence: 99.4% ────────────", Style::default().fg(Color::Rgb(90, 90, 115))),
    ]));
    frame.render_widget(Paragraph::new(tr_lines), tr_inner);

    // Bottom-Right: Flavor Profile Pills + Spiritual Kinship + Telemetry + Suggest Button
    let br_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(65, 65, 85)));
    let br_inner = br_block.inner(right_quadrants[1]);
    frame.render_widget(br_block, right_quadrants[1]);

    let mut br_lines = vec![
        Line::from(Span::styled(" ── ⬡ ANIME FLAVOR PROFILE ──────────────────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))),
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
    br_lines.push(Line::from(Span::styled(" ── ✧ SPIRITUAL KINSHIP ARCHIVES ─────────────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))));
    br_lines.push(Line::from(Span::styled("  Landmark titles sharing the same narrative soul DNA:", Style::default().fg(Color::Rgb(150, 150, 170)))));
    let kinship = get_spiritual_kinship_details(&state.tailored_answers);
    for (k_idx, &(title, vibe)) in kinship.iter().enumerate() {
        if k_idx % 2 == 0 {
            let next_title = kinship.get(k_idx + 1);
            let mut line_spans = vec![
                Span::styled("  ✦ ", Style::default().fg(persona.primary_color)),
                Span::styled(format!("{:<18}", title), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" ({})  ", vibe), Style::default().fg(Color::Rgb(140, 140, 160))),
            ];
            if let Some(&(t2, v2)) = next_title {
                line_spans.push(Span::styled("✦ ", Style::default().fg(persona.primary_color)));
                line_spans.push(Span::styled(format!("{:<18}", t2), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
                line_spans.push(Span::styled(format!(" ({})", v2), Style::default().fg(Color::Rgb(140, 140, 160))));
            }
            br_lines.push(Line::from(line_spans));
        }
    }

    br_lines.push(Line::from(""));
    br_lines.push(Line::from(Span::styled(" ── ⟐ ALLAGAN ARCHIVE TELEMETRY ──────────────────────────────────────", Style::default().fg(Color::Rgb(140, 140, 160)))));
    let veto_count = state.tailored_answers.deal_breakers.len() * 16;
    br_lines.push(Line::from(vec![
        Span::styled("  Candidate Pool: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("300 Titles Evaluated in Local Cache", Style::default().fg(Color::White)),
        Span::styled("  · Vetoes: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled(format!("{} Filtered", veto_count), Style::default().fg(Color::Rgb(255, 120, 120))),
    ]));
    br_lines.push(Line::from(vec![
        Span::styled("  Top Affinity:   ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("99% Synergy Projected", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
        Span::styled("  · Cour Format: ", Style::default().fg(Color::Rgb(140, 140, 160))),
        Span::styled("Concluded 1-Cour TV Series", Style::default().fg(Color::Rgb(190, 190, 210))),
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
        "       ✦ Attunes 15 tailored recommendations with poster art & storytelling",
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
            format!(" 󰏫 ATTUNE ATTRIBUTE: {} 󰏫 ", q.title.to_uppercase()),
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

/// Render the Tailored Anime Results shelf with cover artwork and rich storytelling synopsis.
pub fn render_tailored_results(frame: &mut Frame, state: &mut AppState) {
    let area = frame.area();
    frame.render_widget(Clear, area);

    let persona = calculate_persona(&state.tailored_answers);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(persona.primary_color))
        .title(Span::styled(
            format!(" ❖ SOLUS TAILORED SHELF • {} MATCHES ❖ ", state.tailored_results.len()),
            Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Persona banner
            Constraint::Min(0),    // Two columns: Left (Ranked list), Right (Selected preview)
            Constraint::Length(3), // Footer hints
        ])
        .split(inner);

    // Persona header banner
    let banner_text = Line::from(vec![
        Span::styled(" Attuned Persona: ", Style::default().fg(Color::Rgb(160, 160, 180))),
        Span::styled(format!(" {} ", persona.title), Style::default().fg(Color::Black).bg(persona.primary_color).add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(Color::Rgb(80, 80, 100))),
        Span::styled(persona.epithet, Style::default().fg(Color::Rgb(200, 200, 220))),
    ]);
    frame.render_widget(Paragraph::new(banner_text), chunks[0]);

    if state.tailored_results.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("No candidates strictly matched all selected filters and deal-breakers.", Style::default().fg(Color::Rgb(255, 100, 100)))),
            Line::from(Span::styled("Press [r] to adjust your deal-breakers or broaden your episode count.", Style::default().fg(Color::Rgb(180, 180, 200)))),
        ]).alignment(Alignment::Center);
        frame.render_widget(msg, chunks[1]);
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(44), // Left: Recommendation list
            Constraint::Percentage(56), // Right: Show synopsis, artwork & details
        ])
        .split(chunks[1]);

    // ── LEFT COLUMN: Ranked Recommendation List ────────────────────────────────
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

    // ── RIGHT COLUMN: Selected Anime Details, Artwork & Storytelling ───────────
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
                    "⬡ Loading\nAstral Poster..."
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
                Span::styled(format!("❖ {} ", title), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]),
        ];
        if let Some(sub) = subtitle {
            meta_lines.push(Line::from(Span::styled(format!("  ◈ {}", sub), Style::default().fg(Color::Rgb(140, 140, 160)))));
        }

        let match_badge = format!(" [{}% AETHERIC RESONANCE] ", focused.match_percentage);
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
            Span::styled("✦ Soul Synergy: ", Style::default().fg(persona.accent_color).add_modifier(Modifier::BOLD)),
            Span::styled("Directly tuned to your preference matrix.", Style::default().fg(Color::Rgb(180, 180, 200))),
        ]));
        meta_lines.push(Line::from(vec![
            Span::styled("✦ Deal-Breakers: ", Style::default().fg(Color::Rgb(80, 250, 123))),
            Span::styled("100% Compliant (0 violations)", Style::default().fg(Color::Rgb(160, 160, 180))),
        ]));

        frame.render_widget(Paragraph::new(meta_lines), top_cols[2]);

        // ── BOTTOM SECTION: Narrative Chronicle & Storytelling Synopsis ────────
        let bottom_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::Rgb(65, 65, 85)))
            .title(Span::styled(
                " ── ❖ NARRATIVE CHRONICLE & AETHERIC LORE ── ",
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

    // Footer actions
    let footer_text = Line::from(vec![
        Span::styled(" [f] ", Style::default().fg(Color::Black).bg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
        Span::styled(" ✦ Attune / Refine Results  ", Style::default().fg(Color::Rgb(80, 250, 123)).add_modifier(Modifier::BOLD)),
        Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(persona.accent_color).add_modifier(Modifier::BOLD)),
        Span::styled(" Play Now  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(" [d] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Open Detail  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(" [+] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Add to Watchlist  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(" [r] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Re-tune Diagram  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(" [Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Back to Home", Style::default().fg(Color::Rgb(140, 140, 160))),
    ]);

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));
    frame.render_widget(Paragraph::new(footer_text).block(footer_block), chunks[2]);

    if state.is_refining_tailored {
        render_refine_modal(frame, state);
    }
}

/// Render the 6-question Crucible adaptive refinement modal.
pub fn render_refine_modal(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(76, 76, frame.area());
    frame.render_widget(Clear, area);

    let refine_questions = crate::services::tailored::get_refinement_questions(state.tailored_answers.choices[0]);
    let step = state.tailored_refine_step.min(5);

    let emerald = Color::Rgb(80, 250, 123);
    let cyan = Color::Rgb(139, 233, 253);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(emerald))
        .title(Span::styled(
            format!(" ❖ AETHERIC CRUCIBLE • STEP {} OF 6 ❖ ", step + 1),
            Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Header + progress
            Constraint::Min(0),    // Body (options or anchor search)
            Constraint::Length(3), // Footer hints
        ])
        .split(inner);

    // Progress track (6 steps)
    let mut progress_spans = Vec::new();
    for i in 0..6 {
        if i == step {
            progress_spans.push(Span::styled("◆ ", Style::default().fg(emerald).add_modifier(Modifier::BOLD)));
        } else if i < step {
            progress_spans.push(Span::styled("◈ ", Style::default().fg(cyan)));
        } else {
            progress_spans.push(Span::styled("◇ ", Style::default().fg(Color::Rgb(70, 70, 90))));
        }
    }

    if step < 5 {
        // Steps 1 to 5: Multi-select questions
        let q = &refine_questions[step];
        let selected_count = state.refine_selections[step].len();

        let header_lines = vec![
            Line::from(vec![
                Span::styled(format!(" {} ", q.symbol), Style::default().fg(cyan).add_modifier(Modifier::BOLD)),
                Span::styled(q.title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(
                    format!("[ {} selected ]", selected_count),
                    Style::default().fg(if selected_count > 0 { emerald } else { Color::Rgb(140, 140, 160) }),
                ),
            ]),
            Line::from(vec![
                Span::styled("   ", Style::default()),
                Span::styled(q.subtitle, Style::default().fg(Color::Rgb(170, 170, 190))),
            ]),
            Line::from(progress_spans),
        ];
        frame.render_widget(Paragraph::new(header_lines), chunks[0]);

        // Options list with checkboxes
        let mut option_lines = Vec::new();
        for (idx, opt) in q.options.iter().enumerate() {
            let is_focused = idx == state.tailored_refine_cursor;
            let is_checked = state.refine_selections[step].contains(&idx);

            let check_badge = if is_checked { "[x]" } else { "[ ]" };
            let check_style = if is_checked {
                Style::default().fg(emerald).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(110, 110, 130))
            };

            let pointer = if is_focused { " ▶ " } else { "   " };
            let num_badge = format!(" [{}] ", idx + 1);
            let badge_style = if is_focused {
                Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(140, 140, 160))
            };

            let label_style = if is_focused {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else if is_checked {
                Style::default().fg(Color::Rgb(220, 240, 225))
            } else {
                Style::default().fg(Color::Rgb(180, 180, 200))
            };

            let detail_style = if is_focused {
                Style::default().fg(cyan)
            } else {
                Style::default().fg(Color::Rgb(120, 120, 140))
            };

            option_lines.push(Line::from(vec![
                Span::styled(pointer, Style::default().fg(emerald).add_modifier(Modifier::BOLD)),
                Span::styled(check_badge, check_style),
                Span::styled(num_badge, badge_style),
                Span::styled(format!("{} ", opt.symbol), Style::default().fg(cyan)),
                Span::styled(opt.label, label_style),
            ]));
            option_lines.push(Line::from(vec![
                Span::styled("           ", Style::default()),
                Span::styled(opt.detail, detail_style),
            ]));
            option_lines.push(Line::from(""));
        }

        let options_p = Paragraph::new(option_lines)
            .wrap(ratatui::widgets::Wrap { trim: true });
        frame.render_widget(options_p, chunks[1]);

        // Footer
        let footer_text = Line::from(vec![
            Span::styled(" [Space] / [1-5] ", Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD)),
            Span::styled(" Toggle [x]  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" Confirm & Next  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [b] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Back  ", Style::default().fg(Color::Rgb(180, 180, 200))),
            Span::styled(" [Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel", Style::default().fg(Color::Rgb(140, 140, 160))),
        ]);
        let footer_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));
        frame.render_widget(Paragraph::new(footer_text).block(footer_block), chunks[2]);
    } else {
        // Step 6 (index 5): Keystone Anchor Anime Search
        let header_lines = vec![
            Line::from(vec![
                Span::styled(" ❖ ", Style::default().fg(cyan).add_modifier(Modifier::BOLD)),
                Span::styled("Keystone Benchmark Anchor", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled("[ Discovery Keystone ]", Style::default().fg(emerald)),
            ]),
            Line::from(vec![
                Span::styled("   ", Style::default()),
                Span::styled("Enter a benchmark anime you love (e.g. Death Note, Monster, Steins;Gate) to attune direct peer DNA.", Style::default().fg(Color::Rgb(170, 170, 190))),
            ]),
            Line::from(progress_spans),
        ];
        frame.render_widget(Paragraph::new(header_lines), chunks[0]);

        // Anchor input and search candidate area
        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Search input bar
                Constraint::Min(0),    // Candidates list
                Constraint::Length(3), // Selected preview badge
            ])
            .split(chunks[1]);

        let input_text = if state.refine_anchor_query.is_empty() {
            "Type anime title (e.g. Death Note, Vinland Saga, Monster)..."
        } else {
            &state.refine_anchor_query
        };
        let input_style = if state.refine_anchor_query.is_empty() {
            Style::default().fg(Color::Rgb(120, 120, 140))
        } else {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        };
        let input_p = Paragraph::new(Line::from(vec![
            Span::styled("    ", Style::default().fg(cyan)),
            Span::styled(input_text, input_style),
            Span::styled("▎", Style::default().fg(emerald)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(cyan))
                .title(" Anchor Anime Search "),
        );
        frame.render_widget(input_p, body_chunks[0]);

        // Candidates
        let mut candidate_lines = Vec::new();
        if state.is_searching_anchor {
            candidate_lines.push(Line::from(Span::styled(
                "  Searching AniList archives for keystone anchor...",
                Style::default().fg(Color::Rgb(150, 150, 180)),
            )));
        } else if state.refine_anchor_results.is_empty() {
            if state.refine_anchor_query.is_empty() {
                candidate_lines.push(Line::from(Span::styled(
                    "  Start typing to search AniList for your favorite anchor anime.",
                    Style::default().fg(Color::Rgb(130, 130, 150)),
                )));
                candidate_lines.push(Line::from(""));
                candidate_lines.push(Line::from(Span::styled(
                    "  ✦ Selecting an anchor pulls verified community peer recommendations (+35 pts).",
                    Style::default().fg(Color::Rgb(120, 120, 145)),
                )));
                candidate_lines.push(Line::from(Span::styled(
                    "  ✦ Press [s] to skip anchor attunement and run purely on questionnaire criteria.",
                    Style::default().fg(Color::Rgb(120, 120, 145)),
                )));
            } else {
                candidate_lines.push(Line::from(Span::styled(
                    "  No anime found matching your query on AniList.",
                    Style::default().fg(Color::Rgb(220, 100, 100)),
                )));
            }
        } else {
            for (idx, a) in state.refine_anchor_results.iter().take(6).enumerate() {
                let is_focused = idx == state.refine_anchor_cursor;
                let pointer = if is_focused { " ▶ " } else { "   " };
                let num_badge = format!(" [{}] ", idx + 1);
                let badge_style = if is_focused {
                    Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(140, 140, 160))
                };
                let title_style = if is_focused {
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(190, 190, 210))
                };
                let meta = format!(
                    "{} · Score: {}% · {}",
                    a.season_year.map(|y| y.to_string()).unwrap_or_else(|| "Unknown".to_string()),
                    a.score.unwrap_or(0),
                    a.format.as_deref().unwrap_or("TV"),
                );
                candidate_lines.push(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(emerald).add_modifier(Modifier::BOLD)),
                    Span::styled(num_badge, badge_style),
                    Span::styled(a.display_title(), title_style),
                    Span::raw("  "),
                    Span::styled(format!("({})", meta), Style::default().fg(Color::Rgb(130, 130, 150))),
                ]));
            }
        }
        frame.render_widget(Paragraph::new(candidate_lines), body_chunks[1]);

        // Selected preview
        let preview_line = if let Some(ref sel) = state.refine_anchor_selected {
            Line::from(vec![
                Span::styled(" ✦ Anchor Locked: ", Style::default().fg(emerald).add_modifier(Modifier::BOLD)),
                Span::styled(sel.display_title(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(" · Community Peer Vector Attuned", Style::default().fg(cyan)),
            ])
        } else if let Some(focused) = state.refine_anchor_results.get(state.refine_anchor_cursor) {
            Line::from(vec![
                Span::styled(" ◈ Highlighted Anchor: ", Style::default().fg(cyan).add_modifier(Modifier::BOLD)),
                Span::styled(focused.display_title(), Style::default().fg(Color::White)),
                Span::styled(" · Press Enter to Lock & Attune", Style::default().fg(Color::Rgb(140, 140, 160))),
            ])
        } else {
            Line::from(Span::styled(
                " ✧ Optional: Type anime title above or press [s] to skip anchor attunement",
                Style::default().fg(Color::Rgb(130, 130, 150)),
            ))
        };
        frame.render_widget(Paragraph::new(preview_line), body_chunks[2]);

        // Footer for Step 6
        let footer_text = Line::from(vec![
            Span::styled(" [Type] ", Style::default().fg(Color::Black).bg(cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" Query  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [↑/↓] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Select  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(emerald).add_modifier(Modifier::BOLD)),
            Span::styled(" Attune & Finalize  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(" [s] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Skip Anchor  ", Style::default().fg(Color::Rgb(180, 180, 200))),
            Span::styled(" [b] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Back  ", Style::default().fg(Color::Rgb(180, 180, 200))),
            Span::styled(" [Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel", Style::default().fg(Color::Rgb(140, 140, 160))),
        ]);
        let footer_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::Rgb(60, 60, 80)));
        frame.render_widget(Paragraph::new(footer_text).block(footer_block), chunks[2]);
    }
}
