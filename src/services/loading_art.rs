//! Y2K-era anime loading screen generator for `mpv`.
//! Generates a rotating 1280x720 CRT/Y2K-styled loading card embedded in the player
//! playlist so users see instant artwork and progress instead of a blank black window.

use image::{imageops, DynamicImage, GenericImageView, Rgba, RgbaImage};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const Y2K_ASSETS: [(&[u8], [u8; 3], &str); 11] = [
    (
        include_bytes!("../../assets/loading/y2k_0.jpg"),
        [110, 231, 135], // CRT Phosphor Green
        "CRT PHOSPHOR",
    ),
    (
        include_bytes!("../../assets/loading/y2k_1.jpg"),
        [96, 165, 250], // Cyber Cobalt
        "CYBER AUDIO",
    ),
    (
        include_bytes!("../../assets/loading/y2k_2.jpg"),
        [229, 231, 235], // Sketchbook Silver
        "INK SKETCH",
    ),
    (
        include_bytes!("../../assets/loading/y2k_3.jpg"),
        [165, 180, 252], // Midnight Deck Indigo
        "MIDNIGHT DECK",
    ),
    (
        include_bytes!("../../assets/loading/y2k_4.jpg"),
        [134, 239, 172], // Mint CRT Star
        "MINT STAR",
    ),
    (
        include_bytes!("../../assets/loading/y2k_5.jpg"),
        [129, 140, 248], // Neon Violet Headphones
        "NEON CAT",
    ),
    (
        include_bytes!("../../assets/loading/y2k_6.jpg"),
        [203, 213, 225], // Halftone Mono Star
        "HALFTONE STAR",
    ),
    (
        include_bytes!("../../assets/loading/y2k_7.jpg"),
        [192, 132, 252], // Cyber Punk Violet
        "CYBER VIOLET",
    ),
    (
        include_bytes!("../../assets/loading/y2k_8.jpg"),
        [94, 234, 212], // Cyan Halo Star
        "CYAN HALO",
    ),
    (
        include_bytes!("../../assets/loading/y2k_9.jpg"),
        [147, 197, 253], // Ice Matrix Grid
        "ICE MATRIX",
    ),
    (
        include_bytes!("../../assets/loading/y2k_10.jpg"),
        [45, 212, 191], // Teal Stairwell
        "TEAL SIGNAL",
    ),
];

static LOADING_VARIANT_SEQ: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Total number of bundled Y2K/anime showcase portraits.
pub const Y2K_ART_COUNT: usize = Y2K_ASSETS.len();

/// Return the raw embedded JPEG bytes and showcase tag for a curated character portrait slot.
pub fn raw_y2k_art(idx: usize) -> (&'static [u8], &'static str) {
    let entry = Y2K_ASSETS[idx % Y2K_ASSETS.len()];
    (entry.0, entry.2)
}

/// Returns the next Y2K loading screen variant index (`0..4`), rotating on every episode launch.
pub fn next_loading_variant_index() -> usize {
    let cur = LOADING_VARIANT_SEQ.load(Ordering::Relaxed);
    if cur == usize::MAX {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0)
            % Y2K_ASSETS.len();
        let _ = LOADING_VARIANT_SEQ.compare_exchange(
            usize::MAX,
            seed,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }
    LOADING_VARIANT_SEQ.fetch_add(1, Ordering::Relaxed) % Y2K_ASSETS.len()
}

/// Generates a 1280x720 Y2K anime loading card on disk and returns its path.
pub fn generate_y2k_loading_screen(
    anime_title: &str,
    episode: u32,
    variant_idx: usize,
) -> Option<PathBuf> {
    let (bytes, accent_rgb, tag) = Y2K_ASSETS[variant_idx % Y2K_ASSETS.len()];
    let src = image::load_from_memory(bytes).ok()?;

    let width = 1280u32;
    let height = 720u32;

    // 1. Darkened blurred ambient backdrop
    let mut small_bg = src
        .resize_to_fill(160, 90, imageops::FilterType::Triangle)
        .to_rgba8();
    small_bg = imageops::blur(&small_bg, 9.0);
    for px in small_bg.pixels_mut() {
        px[0] = ((px[0] as u16 * 18) / 100) as u8;
        px[1] = ((px[1] as u16 * 18) / 100) as u8;
        px[2] = ((px[2] as u16 * 18) / 100) as u8;
        px[3] = 255;
    }
    let mut canvas = imageops::resize(&small_bg, width, height, imageops::FilterType::Triangle);

    // 2. Subtle Y2K CRT scanlines
    for y in (0..height).step_by(3) {
        for x in 0..width {
            let px = canvas.get_pixel_mut(x, y);
            px[0] = ((px[0] as u16 * 78) / 100) as u8;
            px[1] = ((px[1] as u16 * 78) / 100) as u8;
            px[2] = ((px[2] as u16 * 78) / 100) as u8;
        }
    }

    let accent = Rgba([accent_rgb[0], accent_rgb[1], accent_rgb[2], 255]);
    let dim_border = Rgba([
        (accent_rgb[0] as u16 * 45 / 100) as u8,
        (accent_rgb[1] as u16 * 45 / 100) as u8,
        (accent_rgb[2] as u16 * 45 / 100) as u8,
        255,
    ]);
    let white = Rgba([242, 244, 250, 255]);
    let muted = Rgba([162, 168, 190, 255]);
    let black = Rgba([10, 12, 18, 255]);

    // 3. Outer Y2K CRT frame + corner brackets
    draw_rect_outline(&mut canvas, 26, 26, width - 52, height - 52, 2, dim_border);
    draw_corner_brackets(&mut canvas, 22, 22, width - 44, height - 44, 24, 3, accent);

    // 4. Left-side framed anime artwork (460x460)
    let art_size = 460u32;
    let art_x = 84u32;
    let art_y = (height - art_size) / 2;
    let art_fg = src
        .resize_to_fill(art_size, art_size, imageops::FilterType::CatmullRom)
        .to_rgba8();
    imageops::overlay(&mut canvas, &art_fg, art_x as i64, art_y as i64);
    draw_rect_outline(&mut canvas, art_x - 3, art_y - 3, art_size + 6, art_size + 6, 3, accent);
    draw_y2k_star(&mut canvas, art_x + art_size - 22, art_y + 22, 12, accent);
    draw_y2k_star(&mut canvas, art_x + 22, art_y + art_size - 22, 9, white);

    // 5. Right-side HUD typography
    let hud_x = 594u32;
    fill_rect(&mut canvas, hud_x, 148, 300, 36, accent);
    draw_text(&mut canvas, "SOLUS // PLAYER", hud_x + 14, 156, 3, black);
    draw_text(&mut canvas, tag, hud_x + 318, 158, 2, accent);

    draw_text(&mut canvas, "NOW LOADING...", hud_x, 220, 5, white);

    let ep_line = format!("EPISODE {:02}", episode);
    draw_text(&mut canvas, &ep_line, hud_x, 286, 3, accent);

    let clean_title: String = anime_title
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | ':' | '-' | '.' | '/' | '!'))
        .take(26)
        .collect();
    if !clean_title.is_empty() {
        draw_text(&mut canvas, &clean_title, hud_x, 326, 3, white);
    }

    // Track outline & active progress bar are rendered together in ASS overlay (`format_loader_ass_overlay`)
    // so both scale identically across 16:9 and 16:10 displays without drift.
    draw_text(&mut canvas, "BUFFERING STREAM...", hud_x, 424, 2, muted);

    // YouTube Keybindings Legend (positioned safely above bottom OSC zone)
    fill_rect(&mut canvas, hud_x, 458, 580, 2, dim_border);
    draw_text(&mut canvas, "PLAYER KEYS:", hud_x, 474, 2, accent);
    draw_text(&mut canvas, "K/SPACE PLAY   J/L +-10S   P/SHIFT+P PIP", hud_x, 500, 2, white);
    draw_text(&mut canvas, "I/SHIFT+I HUD  F FULL    C SUBS   0-9 SEEK", hud_x, 524, 2, muted);

    let dir = crate::config::Config::cache_dir()
        .map(|d| d.join("loading"))
        .unwrap_or_else(|_| std::env::temp_dir().join("solus-loading"));
    std::fs::create_dir_all(&dir).ok()?;
    let out_path = dir.join(format!("solus_y2k_{}.jpg", variant_idx % Y2K_ASSETS.len()));
    DynamicImage::ImageRgba8(canvas)
        .to_rgb8()
        .save_with_format(&out_path, image::ImageFormat::Jpeg)
        .ok()?;
    Some(out_path)
}

fn fill_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    let (mw, mh) = img.dimensions();
    for py in y..y.saturating_add(h).min(mh) {
        for px in x..x.saturating_add(w).min(mw) {
            img.put_pixel(px, py, color);
        }
    }
}

fn draw_rect_outline(
    img: &mut RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    thickness: u32,
    color: Rgba<u8>,
) {
    fill_rect(img, x, y, w, thickness, color);
    fill_rect(img, x, y + h.saturating_sub(thickness), w, thickness, color);
    fill_rect(img, x, y, thickness, h, color);
    fill_rect(img, x + w.saturating_sub(thickness), y, thickness, h, color);
}

fn draw_corner_brackets(
    img: &mut RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    len: u32,
    t: u32,
    color: Rgba<u8>,
) {
    // Top-left
    fill_rect(img, x, y, len, t, color);
    fill_rect(img, x, y, t, len, color);
    // Top-right
    fill_rect(img, x + w.saturating_sub(len), y, len, t, color);
    fill_rect(img, x + w.saturating_sub(t), y, t, len, color);
    // Bottom-left
    fill_rect(img, x, y + h.saturating_sub(t), len, t, color);
    fill_rect(img, x, y + h.saturating_sub(len), t, len, color);
    // Bottom-right
    fill_rect(img, x + w.saturating_sub(len), y + h.saturating_sub(t), len, t, color);
    fill_rect(img, x + w.saturating_sub(t), y + h.saturating_sub(len), t, len, color);
}

fn draw_y2k_star(img: &mut RgbaImage, cx: u32, cy: u32, radius: i32, color: Rgba<u8>) {
    let (mw, mh) = img.dimensions();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let ax = dx.abs();
            let ay = dy.abs();
            // 4-point Y2K sparkle curve
            if (ax * ay <= (radius / 2).max(1)) && (ax + ay <= radius) {
                let px = cx as i32 + dx;
                let py = cy as i32 + dy;
                if px >= 0 && py >= 0 && (px as u32) < mw && (py as u32) < mh {
                    img.put_pixel(px as u32, py as u32, color);
                }
            }
        }
    }
}

fn draw_text(img: &mut RgbaImage, text: &str, x: u32, y: u32, scale: u32, color: Rgba<u8>) {
    let mut cursor_x = x;
    for ch in text.chars() {
        let glyph = glyph_5x7(ch);
        for (row_idx, &row_bits) in glyph.iter().enumerate() {
            for col_idx in 0..5 {
                if (row_bits >> (4 - col_idx)) & 1 == 1 {
                    fill_rect(
                        img,
                        cursor_x + col_idx * scale,
                        y + row_idx as u32 * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
        cursor_x += 6 * scale;
    }
}

/// Compact 5x7 pixel bitmap font for Y2K HUD captions.
fn glyph_5x7(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0E, 0x11, 0x10, 0x0E, 0x01, 0x11, 0x0E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        '3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        '<' => [0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
        '>' => [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
        _ => [0x00; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_loading_variant_cycles_all_eleven_themes() {
        let mut seen = Vec::new();
        for _ in 0..Y2K_ASSETS.len() {
            seen.push(next_loading_variant_index());
        }
        seen.sort();
        assert_eq!(seen, (0..Y2K_ASSETS.len()).collect::<Vec<_>>());
    }

    #[test]
    fn test_generate_y2k_loading_screen_creates_1280x720_cards() {
        for idx in 0..Y2K_ASSETS.len() {
            let path = generate_y2k_loading_screen("Serial Experiments Lain", 1, idx)
                .expect("should generate loading card");
            assert!(path.is_file());
            let img = image::open(&path).expect("should decode generated JPEG");
            assert_eq!(img.dimensions(), (1280, 720));
        }
    }
}
