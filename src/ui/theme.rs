use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub name: &'static str,
    pub primary: Color,      // Active borders, primary titles, focus badges
    pub secondary: Color,    // Complementary tags, badges, search highlights
    pub accent: Color,       // Ratings, episode numbers, live statuses
    pub border_idle: Color,  // Unfocused card frames (subtle, non-distracting)
    pub border_focus: Color, // Focused card frames (glowing, crisp)
    pub text: Color,         // Primary high-contrast text
    pub text_dim: Color,     // Secondary metadata, genres, format
    pub divider: Color,      // Horizontal rail rules and internal separators
}

pub const THEMES: [ThemePalette; 6] = [
    // 0: Anime Violet / Synth (Default for Anime TUI)
    ThemePalette {
        name: "Anime Violet",
        primary: Color::Rgb(189, 147, 249),    // Lavender Purple
        secondary: Color::Rgb(255, 121, 198),  // Neon Pink
        accent: Color::Rgb(251, 191, 36),      // Star Amber
        border_idle: Color::Rgb(55, 45, 75),   // Deep Plum
        border_focus: Color::Rgb(189, 147, 249),
        text: Color::White,
        text_dim: Color::Rgb(165, 150, 190),
        divider: Color::Rgb(75, 58, 100),
    },
    // 1: Tokyo Night (Refined cyan, lilac and amber)
    ThemePalette {
        name: "Tokyo Night",
        primary: Color::Rgb(125, 207, 255),    // Sky Cyan
        secondary: Color::Rgb(187, 154, 247),  // Lilac
        accent: Color::Rgb(224, 175, 104),     // Amber Gold
        border_idle: Color::Rgb(45, 52, 75),   // Muted Navy
        border_focus: Color::Rgb(125, 207, 255),
        text: Color::Rgb(240, 244, 255),
        text_dim: Color::Rgb(130, 140, 175),
        divider: Color::Rgb(55, 62, 90),
    },
    // 2: Cyberpunk Neon (Hot pink and electric cyan)
    ThemePalette {
        name: "Cyberpunk Neon",
        primary: Color::Rgb(255, 0, 127),      // Hot Pink
        secondary: Color::Rgb(0, 240, 255),    // Neon Cyan
        accent: Color::Rgb(255, 230, 0),       // Laser Yellow
        border_idle: Color::Rgb(50, 30, 70),   // Deep Violet
        border_focus: Color::Rgb(255, 0, 127),
        text: Color::White,
        text_dim: Color::Rgb(160, 140, 190),
        divider: Color::Rgb(65, 38, 90),
    },
    // 3: Dracula Night (High contrast purple, cyan and acid green)
    ThemePalette {
        name: "Dracula Night",
        primary: Color::Rgb(189, 147, 249),    // Dracula Purple
        secondary: Color::Rgb(139, 233, 253),  // Dracula Cyan
        accent: Color::Rgb(80, 250, 123),      // Acid Green
        border_idle: Color::Rgb(68, 71, 90),   // Selection Slate
        border_focus: Color::Rgb(189, 147, 249),
        text: Color::Rgb(248, 248, 242),
        text_dim: Color::Rgb(140, 150, 185),
        divider: Color::Rgb(75, 80, 105),
    },
    // 4: Nord Frost (Glacial cyan, polar blue and aurora green)
    ThemePalette {
        name: "Nord Frost",
        primary: Color::Rgb(136, 192, 208),    // Frost Cyan
        secondary: Color::Rgb(129, 161, 193),  // Glacial Blue
        accent: Color::Rgb(163, 190, 140),     // Aurora Green
        border_idle: Color::Rgb(50, 60, 75),   // Polar Dark
        border_focus: Color::Rgb(136, 192, 208),
        text: Color::Rgb(236, 239, 244),
        text_dim: Color::Rgb(145, 160, 180),
        divider: Color::Rgb(65, 78, 100),
    },
    // 5: OLED Monochrome (Crisp white and platinum minimalist)
    ThemePalette {
        name: "OLED Platinum",
        primary: Color::Rgb(236, 239, 244),    // Pure Platinum
        secondary: Color::Rgb(190, 200, 215),  // Silver
        accent: Color::Rgb(255, 215, 0),       // Warm Gold
        border_idle: Color::Rgb(50, 55, 65),   // Dark Slate
        border_focus: Color::Rgb(236, 239, 244),
        text: Color::White,
        text_dim: Color::Rgb(140, 145, 160),
        divider: Color::Rgb(70, 75, 85),
    },
];

pub fn get_theme(index: usize) -> &'static ThemePalette {
    &THEMES[index % THEMES.len()]
}
