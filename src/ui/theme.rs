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

/// Dynamically extract an ambilight theme palette from an anime's cover art.
///
/// Samples image pixels in HSV color space to identify dominant vibrant colors,
/// boosting luminance and saturation so the terminal borders and highlights
/// vividly harmonize with the anime currently in focus.
pub fn extract_ambilight_palette(img: &image::DynamicImage, base: &ThemePalette) -> ThemePalette {
    use image::GenericImageView;

    let thumb = img.thumbnail(48, 48);
    let mut hue_bins = [0.0f32; 12];
    let mut bin_rgb_sums = [(0.0f32, 0.0f32, 0.0f32); 12];

    for (_, _, pixel) in thumb.pixels() {
        let r = pixel[0] as f32 / 255.0;
        let g = pixel[1] as f32 / 255.0;
        let b = pixel[2] as f32 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        // Skip near-black or very dark pixels
        if max < 0.16 {
            continue;
        }

        let sat = if max == 0.0 { 0.0 } else { delta / max };
        // Skip desaturated grays/whites
        if sat < 0.18 || (max > 0.90 && sat < 0.15) {
            continue;
        }

        let hue = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / delta).rem_euclid(6.0))
        } else if max == g {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };

        let bin_idx = ((hue / 30.0) as usize) % 12;
        let weight = sat * (0.35 + 0.65 * max);

        hue_bins[bin_idx] += weight;
        bin_rgb_sums[bin_idx].0 += r * weight;
        bin_rgb_sums[bin_idx].1 += g * weight;
        bin_rgb_sums[bin_idx].2 += b * weight;
    }

    // Find the most dominant hue bin
    let mut best_bin = 0;
    let mut best_weight = 0.0f32;
    for (i, &w) in hue_bins.iter().enumerate() {
        if w > best_weight {
            best_weight = w;
            best_bin = i;
        }
    }

    if best_weight <= 0.05 {
        // Not enough color information; keep base theme
        return *base;
    }

    let p_sum = bin_rgb_sums[best_bin];
    let avg_r = (p_sum.0 / best_weight).clamp(0.0, 1.0);
    let avg_g = (p_sum.1 / best_weight).clamp(0.0, 1.0);
    let avg_b = (p_sum.2 / best_weight).clamp(0.0, 1.0);

    // Boost primary for high vibrancy on dark terminal background
    let (pr, pg, pb) = boost_vibrancy(avg_r, avg_g, avg_b);

    // Find secondary bin with at least 60 degrees hue separation
    let mut second_bin = (best_bin + 6) % 12;
    let mut second_weight = 0.0f32;
    for (i, &w) in hue_bins.iter().enumerate() {
        let diff = (i as isize - best_bin as isize).abs();
        let dist = diff.min(12 - diff);
        if dist >= 2 && w > second_weight {
            second_weight = w;
            second_bin = i;
        }
    }

    let (sr, sg, sb) = if second_weight > 0.05 {
        let s_sum = bin_rgb_sums[second_bin];
        let sr = (s_sum.0 / second_weight).clamp(0.0, 1.0);
        let sg = (s_sum.1 / second_weight).clamp(0.0, 1.0);
        let sb = (s_sum.2 / second_weight).clamp(0.0, 1.0);
        boost_vibrancy(sr, sg, sb)
    } else {
        // Fallback: rotate primary hue by ~150 degrees
        ((255 - pr).clamp(80, 240), (255 - pg).clamp(100, 240), (255 - pb).clamp(120, 255))
    };

    let accent = Color::Rgb(251, 191, 36); // Crisp golden amber for ratings & counters
    let border_focus = Color::Rgb(pr, pg, pb);
    let primary = Color::Rgb(pr, pg, pb);
    let secondary = Color::Rgb(sr, sg, sb);
    let border_idle = Color::Rgb(
        (pr / 5).saturating_add(30),
        (pg / 5).saturating_add(30),
        (pb / 5).saturating_add(45),
    );
    let divider = Color::Rgb(
        (pr / 6).saturating_add(40),
        (pg / 6).saturating_add(40),
        (pb / 6).saturating_add(55),
    );
    let text = Color::Rgb(248, 248, 252);
    let text_dim = Color::Rgb(
        (pr / 5).saturating_add(150),
        (pg / 5).saturating_add(150),
        (pb / 5).saturating_add(170),
    );

    ThemePalette {
        name: "Ambilight",
        primary,
        secondary,
        accent,
        border_idle,
        border_focus,
        text,
        text_dim,
        divider,
    }
}

fn boost_vibrancy(r: f32, g: f32, b: f32) -> (u8, u8, u8) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let target_val = 0.88f32;
    let target_sat = 0.70f32;

    if delta == 0.0 {
        return (200, 200, 220);
    }

    let sat = delta / max;
    let sat_mult = if sat < target_sat { target_sat / sat.max(0.05) } else { 1.0 };
    let val_mult = if max < target_val { target_val / max.max(0.05) } else { 1.0 };

    let mid = (r + g + b) / 3.0;
    let nr = ((mid + (r - mid) * sat_mult) * val_mult).clamp(0.0, 1.0);
    let ng = ((mid + (g - mid) * sat_mult) * val_mult).clamp(0.0, 1.0);
    let nb = ((mid + (b - mid) * sat_mult) * val_mult).clamp(0.0, 1.0);

    ((nr * 255.0) as u8, (ng * 255.0) as u8, (nb * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ambilight_from_vibrant_red_image() {
        let mut img = image::RgbImage::new(48, 48);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([220, 30, 40]);
        }
        let dyn_img = image::DynamicImage::ImageRgb8(img);
        let base = get_theme(0);
        let theme = extract_ambilight_palette(&dyn_img, base);
        assert_eq!(theme.name, "Ambilight");
        if let Color::Rgb(r, g, b) = theme.primary {
            assert!(r > g && r > b, "Red should dominate");
        } else {
            panic!("Expected RGB color");
        }
    }

    #[test]
    fn test_extract_ambilight_monochrome_keeps_base() {
        let mut img = image::RgbImage::new(48, 48);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([10, 10, 10]);
        }
        let dyn_img = image::DynamicImage::ImageRgb8(img);
        let base = get_theme(0);
        let theme = extract_ambilight_palette(&dyn_img, base);
        assert_eq!(theme.name, base.name);
    }
}
