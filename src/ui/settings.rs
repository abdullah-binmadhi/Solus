//! Settings overlay — persistent global preferences.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::{config, state::AppState};

const PLAYER_CHOICES: [&str; 3] = ["mpv", "iina", "vlc"];
const QUALITY_CHOICES: [&str; 5] = ["best", "1080p", "720p", "480p", "360p"];
const AUDIO_CHOICES: [&str; 2] = ["sub", "dub"];
const SKIP_CHOICES: [&str; 2] = ["enabled", "disabled"];
const ANIME4K_CHOICES: [&str; 3] = ["off", "balanced 2x", "ultra 4k"];
const AMBILIGHT_CHOICES: [&str; 2] = ["enabled", "disabled"];
pub const SUBTITLE_CHOICES: [&str; 11] = [
    "English",
    "Arabic",
    "Russian",
    "Indonesian",
    "Spanish",
    "French",
    "German",
    "Italian",
    "Portuguese",
    "Japanese",
    "Turkish",
];

pub fn render_overlay(frame: &mut Frame, state: &AppState, cfg: &config::Config) {
    let area = centered_rect(68, 86, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(
            " Settings ",
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
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Persisted defaults for every new playback session",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Use j/k to focus a row, h/l to change it, Esc to close.",
                Style::default().fg(Color::Rgb(150, 150, 170)),
            )),
        ]),
        rows[0],
    );

    render_setting_row(rows[1], "Player", &PLAYER_CHOICES, player_index(&cfg.player), state.settings_cursor == 0, frame);
    render_setting_row(rows[2], "Quality", &QUALITY_CHOICES, quality_index(&cfg.quality), state.settings_cursor == 1, frame);
    render_setting_row(rows[3], "Audio", &AUDIO_CHOICES, audio_index(&cfg.audio_mode), state.settings_cursor == 2, frame);
    render_setting_row(rows[4], "Subtitle", &SUBTITLE_CHOICES, sub_index(&cfg.default_sub_lang), state.settings_cursor == 3, frame);
    render_setting_row(rows[5], "Auto-Skip", &SKIP_CHOICES, if cfg.auto_skip_intro { 0 } else { 1 }, state.settings_cursor == 4, frame);
    render_setting_row(rows[6], "Anime4K", &ANIME4K_CHOICES, anime4k_index(&cfg.anime4k_mode), state.settings_cursor == 5, frame);
    render_setting_row(rows[7], "Ambilight", &AMBILIGHT_CHOICES, if cfg.ambilight_enabled { 0 } else { 1 }, state.settings_cursor == 6, frame);

    let preferred_ready = match cfg.player {
        config::Player::Mpv => state.has_mpv || state.has_iina,
        config::Player::Iina => state.has_iina,
        config::Player::Vlc => state.has_vlc,
    };
    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                " Status ",
                Style::default()
                    .fg(Color::Black)
                    .bg(if preferred_ready {
                        Color::Rgb(235, 235, 235)
                    } else {
                        Color::Rgb(255, 190, 0)
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                if preferred_ready {
                    "Ready for playback"
                } else {
                    "Current player missing, fallback may be used"
                },
                Style::default().fg(Color::Rgb(195, 195, 210)),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Changes save immediately to config.toml",
            Style::default().fg(Color::Rgb(120, 120, 140)),
        )),
    ]);
    frame.render_widget(footer, rows[6]);
}

fn render_setting_row(
    area: Rect,
    label: &str,
    values: &[&str],
    selected: usize,
    focused: bool,
    frame: &mut Frame,
) {
    let row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(12), Constraint::Min(0)])
        .split(area);

    let label_style = if focused {
        Style::default()
            .fg(Color::Rgb(180, 0, 255))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(170, 170, 185))
    };
    frame.render_widget(
        Paragraph::new(Span::styled(format!("{label}:"), label_style)),
        row[0],
    );

    let mut spans = Vec::new();
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
        }
        let active = idx == selected;
        spans.push(Span::styled(
            format!(" {} ", value),
            Style::default()
                .fg(if active { Color::Black } else { Color::Rgb(220, 220, 230) })
                .bg(if active {
                    if focused { Color::Rgb(180, 0, 255) } else { Color::Rgb(235, 235, 235) }
                } else {
                    Color::Reset
                })
                .add_modifier(if active { Modifier::BOLD } else { Modifier::empty() }),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        row[1],
    );
}

fn player_index(player: &config::Player) -> usize {
    match player {
        config::Player::Mpv => 0,
        config::Player::Iina => 1,
        config::Player::Vlc => 2,
    }
}

fn quality_index(quality: &config::Quality) -> usize {
    match quality {
        config::Quality::Best => 0,
        config::Quality::P1080 => 1,
        config::Quality::P720 => 2,
        config::Quality::P480 => 3,
        config::Quality::P360 => 4,
    }
}

fn audio_index(audio: &config::AudioMode) -> usize {
    match audio {
        config::AudioMode::Sub => 0,
        config::AudioMode::Dub => 1,
    }
}

fn anime4k_index(mode: &config::Anime4KMode) -> usize {
    match mode {
        config::Anime4KMode::Off => 0,
        config::Anime4KMode::Balanced => 1,
        config::Anime4KMode::Ultra => 2,
    }
}

fn sub_index(lang: &str) -> usize {
    SUBTITLE_CHOICES
        .iter()
        .position(|&l| l.eq_ignore_ascii_case(lang))
        .unwrap_or(0)
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
