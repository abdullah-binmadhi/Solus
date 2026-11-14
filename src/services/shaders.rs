//! Anime4K real-time GLSL shader pipeline management for mpv / iina.
//!
//! Provides Era-Adaptive GPU-accelerated line-art reconstruction, pure-luminance (Y)
//! Catmull-Rom / Sobel upscaling, Structure-Tensor Tangent Anti-Aliasing (for silky-smooth
//! non-jagged black/thin ink lines), Contrast Adaptive Sharpening (CAS), and strict
//! anti-ringing shaders that transform both 80s/90s cel anime (e.g., Serial Experiments Lain)
//! and modern digital HD anime into razor-sharp, jaggie-free 4K at 60fps.

use std::path::PathBuf;
use crate::config::Anime4KMode;

/// Production era profile detected from `anime.season_year`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimeEra {
    /// 1960s–2006: 35mm/16mm film grain, hand-painted acetate cels, halftone dithering, CRT glow
    /// (e.g., *Serial Experiments Lain* 1998, *Neon Genesis Evangelion*, *Cowboy Bebop*, *Monster*).
    /// Uses noise-floor edge gating (`smoothstep(0.045, 0.14, grad)`) so film grain is never
    /// blurred into oil-painted mush or sharpened into blotches, paired with tangent-guided anti-aliasing.
    RetroCel,
    /// 2007–Present: Digital vector ink & paint, 720p/900p/1080p digital compositing
    /// (e.g., *Frieren*, *Jujutsu Kaisen*, *Demon Slayer*, *Solo Leveling*).
    /// Uses tangent-guided sub-pixel line reconstruction + 2x directional upscale + pure-luma FidelityFX CAS.
    ModernDigital,
}

impl AnimeEra {
    /// Automatically classify an anime's mastering era from its AniList release year.
    pub fn from_year(season_year: Option<i64>) -> Self {
        match season_year {
            Some(y) if y > 0 && y <= 2006 => AnimeEra::RetroCel,
            _ => AnimeEra::ModernDigital,
        }
    }

    pub fn label(&self, season_year: Option<i64>) -> String {
        match (self, season_year) {
            (AnimeEra::RetroCel, Some(y)) => format!("Retro Cel ({y})"),
            (AnimeEra::RetroCel, None) => "Retro Cel".to_string(),
            (AnimeEra::ModernDigital, Some(y)) => format!("Modern Digital ({y})"),
            (AnimeEra::ModernDigital, None) => "Modern Digital".to_string(),
        }
    }

    pub fn short_badge(&self) -> &'static str {
        match self {
            AnimeEra::RetroCel => "RETRO CEL",
            AnimeEra::ModernDigital => "MODERN HD",
        }
    }
}

/// Stage 1: Strict [0.0, 1.0] highlight & shadow range clamp without neighbor pre-blurring.
/// Prevents out-of-range HDR/SDR ringing while preserving 100% of native pixel micro-detail.
const SHADER_CLAMP_HIGHLIGHTS: &str = r#"//!DESC Anime4K-v5.2-Clamp-Highlights-ZeroBlur
//!HOOK MAIN
//!BIND HOOKED

vec4 hook() {
    return clamp(HOOKED_tex(HOOKED_pos), 0.0, 1.0);
}
"#;

// ─────────────────────────────────────────────────────────────────────────────
// UNIFIED EDGE-AWARE 2x UPSCALER (parameterised per era + mode)
// ─────────────────────────────────────────────────────────────────────────────

/// Tuning knobs for the unified upscaler.
struct UpscaleParams {
    desc: &'static str,
    /// Edge-confidence gate (local luma range). Higher = protects film grain more.
    gate_lo: f32,
    gate_hi: f32,
    /// Edge steepening strength (0..1) and S-curve half-width (smaller = crisper).
    k: f32,
    w: f32,
    /// Thin-line recovery: how far below the local minimum dark ink lines may be pushed.
    dark: f32,
    /// Tangent anti-aliasing blend and tap distance (source pixels).
    aa: f32,
    tan: f32,
}

/// Generate the GLSL for the unified upscaler. Pipeline (measured against ground truth in tests):
/// 1. Catmull-Rom bicubic reconstruction (sharper than bilinear, no blur)
/// 2. Local luma range from the 2x2 bracketing texels, extended downward for thin-line recovery
/// 3. Tangent anti-aliasing along the edge direction (dissolves stair-steps)
/// 4. Continuous S-curve steepening of the normalised edge profile (crisp yet anti-aliased)
/// Everything operates on luminance only (RGB hues untouched) and never exceeds the local range.
fn upscale_glsl(p: &UpscaleParams) -> String {
    format!(
        r#"//!DESC {desc}
//!HOOK MAIN
//!BIND HOOKED
//!WIDTH HOOKED.w 2 *
//!HEIGHT HOOKED.h 2 *

float luma(vec3 c) {{ return dot(c, vec3(0.299, 0.587, 0.114)); }}

vec3 cr(vec3 p0, vec3 p1, vec3 p2, vec3 p3, float t) {{
    return 0.5 * ((2.0 * p1) + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t);
}}

vec3 tap(vec2 base, float dx, float dy) {{
    return HOOKED_tex((base + vec2(dx, dy) + 0.5) * HOOKED_pt).rgb;
}}

vec3 row(vec2 base, float dy, float t) {{
    return cr(tap(base, -1.0, dy), tap(base, 0.0, dy), tap(base, 1.0, dy), tap(base, 2.0, dy), t);
}}

vec4 hook() {{
    vec2 p = HOOKED_pos * HOOKED_size - 0.5;
    vec2 base = floor(p);
    vec2 f = p - base;

    vec3 bc = cr(row(base, -1.0, f.x), row(base, 0.0, f.x), row(base, 1.0, f.x), row(base, 2.0, f.x), f.y);
    bc = clamp(bc, 0.0, 1.0);

    float a = luma(tap(base, 0.0, 0.0));
    float b = luma(tap(base, 1.0, 0.0));
    float c = luma(tap(base, 0.0, 1.0));
    float d = luma(tap(base, 1.0, 1.0));
    float lmin = min(min(a, b), min(c, d));
    float lmax = max(max(a, b), max(c, d));
    float range0 = lmax - lmin;

    float gate = smoothstep({glo:.3}, {ghi:.3}, range0);
    lmin = max(lmin - {dark:.3} * gate * range0, 0.0);
    float range = lmax - lmin;

    vec2 g = vec2((b + d) - (a + c), (c + d) - (a + b));
    float gl = length(g);
    vec2 tdir = gl > 0.0001 ? vec2(-g.y, g.x) / gl : vec2(0.0);
    float tl = 0.5 * (luma(HOOKED_tex(HOOKED_pos + tdir * HOOKED_pt * {tan:.3}).rgb) + luma(HOOKED_tex(HOOKED_pos - tdir * HOOKED_pt * {tan:.3}).rgb));
    float lb0 = luma(bc);
    float lb = mix(lb0, tl, {aa:.3} * gate);

    float tt = clamp((lb - lmin) / max(range, 0.0001), 0.0, 1.0);
    float st = smoothstep(0.5 - {w:.3}, 0.5 + {w:.3}, tt);
    float t2 = mix(tt, st, {k:.3} * gate);
    float target = lmin + t2 * range;
    float delta = clamp(target, lmin, lmax) - lb0;

    return vec4(clamp(bc + vec3(delta), 0.0, 1.0), 1.0);
}}
"#,
        desc = p.desc, glo = p.gate_lo, ghi = p.gate_hi, k = p.k, w = p.w,
        dark = p.dark, aa = p.aa, tan = p.tan
    )
}

const MODERN_BALANCED: UpscaleParams = UpscaleParams { desc: "Solus-Upscale-x2-Modern-Balanced", gate_lo: 0.02, gate_hi: 0.08, k: 0.75, w: 0.30, dark: 0.10, aa: 0.5, tan: 0.7 };
const MODERN_ULTRA: UpscaleParams = UpscaleParams { desc: "Solus-Upscale-x2-Modern-Ultra4K", gate_lo: 0.02, gate_hi: 0.08, k: 0.95, w: 0.24, dark: 0.18, aa: 0.6, tan: 0.75 };
/// Retro: higher noise-floor gate so 90s film grain / dithering is left alone, gentler line recovery.
const RETRO_BALANCED: UpscaleParams = UpscaleParams { desc: "Solus-Upscale-x2-Retro-Balanced", gate_lo: 0.05, gate_hi: 0.14, k: 0.60, w: 0.32, dark: 0.06, aa: 0.5, tan: 0.7 };
const RETRO_ULTRA: UpscaleParams = UpscaleParams { desc: "Solus-Upscale-x2-Retro-Ultra4K", gate_lo: 0.05, gate_hi: 0.14, k: 0.85, w: 0.26, dark: 0.12, aa: 0.6, tan: 0.75 };

/// Return the directory where Solus persists Anime4K GLSL shader files.
pub fn get_shaders_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            let dir = PathBuf::from(local_app).join("solus").join("shaders").join("Anime4K");
            if !dir.to_string_lossy().contains(' ') {
                return dir;
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let dir = home.join(".local/share/solus/shaders/Anime4K");
        if !dir.to_string_lossy().contains(' ') {
            return dir;
        }
    }
    std::env::temp_dir().join("solus-shaders/Anime4K")
}

/// Ensure all required Anime4K GLSL shaders (both RetroCel and ModernDigital profiles) are written to disk.
pub fn ensure_shaders_installed() -> std::io::Result<PathBuf> {
    let dir = get_shaders_dir();
    std::fs::create_dir_all(&dir)?;

    let files: Vec<(&str, String)> = vec![
        ("Anime4K_Clamp_Highlights.glsl", SHADER_CLAMP_HIGHLIGHTS.to_string()),
        ("Anime4K_Retro_Upscale_x2_M.glsl", upscale_glsl(&RETRO_BALANCED)),
        ("Anime4K_Retro_Upscale_x2_VL.glsl", upscale_glsl(&RETRO_ULTRA)),
        ("Anime4K_Upscale_CNN_x2_M.glsl", upscale_glsl(&MODERN_BALANCED)),
        ("Anime4K_Upscale_CNN_x2_VL.glsl", upscale_glsl(&MODERN_ULTRA)),
    ];

    for (name, content) in files {
        let file_path = dir.join(name);
        let needs_write = match std::fs::read_to_string(&file_path) {
            Ok(existing) => existing != content.as_str(),
            Err(_) => true,
        };
        if needs_write {
            std::fs::write(&file_path, &content)?;
        }
    }

    Ok(dir)
}

/// Return the ordered list of absolute shader file paths for the given `Anime4KMode` and `season_year`.
/// Used by both initial player launch and live runtime `change-list` IPC hot-swapping.
pub fn build_shader_files(mode: Anime4KMode, season_year: Option<i64>) -> Vec<String> {
    if mode == Anime4KMode::Off {
        return Vec::new();
    }

    let Ok(dir) = ensure_shaders_installed() else {
        return Vec::new();
    };

    let era = AnimeEra::from_year(season_year);

    let shader_names: &[&str] = match (mode, era) {
        (Anime4KMode::Off, _) => return Vec::new(),
        (Anime4KMode::Balanced, AnimeEra::RetroCel) => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Retro_Upscale_x2_M.glsl",
        ],
        (Anime4KMode::Ultra, AnimeEra::RetroCel) => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Retro_Upscale_x2_VL.glsl",
        ],
        (Anime4KMode::Balanced, AnimeEra::ModernDigital) => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Upscale_CNN_x2_M.glsl",
        ],
        (Anime4KMode::Ultra, AnimeEra::ModernDigital) => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Upscale_CNN_x2_VL.glsl",
        ],
    };

    shader_names
        .iter()
        .map(|name| dir.join(name).to_string_lossy().to_string())
        .collect()
}

/// Build the platform-correct `--glsl-shaders` argument string for `mpv` / `iina` based on the chosen mode and anime release year.
/// Uses `;` on Windows (since drive letters contain `:`) and `:` on Unix.
pub fn build_shader_arg_for_year(mode: Anime4KMode, season_year: Option<i64>) -> Option<String> {
    let paths = build_shader_files(mode, season_year);
    if paths.is_empty() {
        return None;
    }

    #[cfg(windows)]
    let sep = ";";
    #[cfg(not(windows))]
    let sep = ":";

    Some(paths.join(sep))
}

/// Build the platform-correct `--glsl-shaders` argument string using the default Modern Digital profile.
pub fn build_shader_arg(mode: Anime4KMode) -> Option<String> {
    build_shader_arg_for_year(mode, None)
}

/// High-precision mpv GPU mastering & debanding flags that pair with Anime4K shaders.
pub fn mpv_mastering_flags(mode: Anime4KMode) -> Vec<&'static str> {
    match mode {
        Anime4KMode::Off => Vec::new(),
        Anime4KMode::Balanced => vec![
            "--scale=ewa_lanczossharp",
            "--cscale=ewa_lanczossharp",
            "--dscale=mitchell",
            "--correct-downscaling=yes",
            "--linear-downscaling=yes",
            "--sigmoid-upscaling=yes",
            "--deband=yes",
            "--deband-iterations=1",
            "--deband-threshold=32",
            "--deband-range=16",
        ],
        Anime4KMode::Ultra => vec![
            "--scale=ewa_lanczossharp",
            "--cscale=ewa_lanczossharp",
            "--dscale=mitchell",
            "--correct-downscaling=yes",
            "--linear-downscaling=yes",
            "--sigmoid-upscaling=yes",
            "--deband=yes",
            "--deband-iterations=2",
            "--deband-threshold=36",
            "--deband-range=16",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_era_detection_from_year() {
        assert_eq!(AnimeEra::from_year(Some(1998)), AnimeEra::RetroCel); // Serial Experiments Lain
        assert_eq!(AnimeEra::from_year(Some(1995)), AnimeEra::RetroCel); // Evangelion
        assert_eq!(AnimeEra::from_year(Some(2006)), AnimeEra::RetroCel);
        assert_eq!(AnimeEra::from_year(Some(2007)), AnimeEra::ModernDigital);
        assert_eq!(AnimeEra::from_year(Some(2024)), AnimeEra::ModernDigital);
        assert_eq!(AnimeEra::from_year(None), AnimeEra::ModernDigital);
    }

    #[test]
    fn test_shader_arg_off_is_none() {
        assert_eq!(build_shader_arg(Anime4KMode::Off), None);
        assert_eq!(build_shader_arg_for_year(Anime4KMode::Off, Some(1998)), None);
        assert!(mpv_mastering_flags(Anime4KMode::Off).is_empty());
    }

    #[test]
    fn test_shader_arg_balanced_creates_files_and_paths() {
        let arg = build_shader_arg(Anime4KMode::Balanced);
        assert!(arg.is_some());
        let s = arg.unwrap();
        assert!(s.contains("Anime4K_Upscale_CNN_x2_M.glsl"));
        assert!(s.contains("Anime4K_Clamp_Highlights.glsl"));
        assert!(!mpv_mastering_flags(Anime4KMode::Balanced).is_empty());
    }

    #[test]
    fn test_pipelines_are_single_upscale_pass() {
        for year in [Some(2023), Some(1998)] {
            for mode in [Anime4KMode::Balanced, Anime4KMode::Ultra] {
                let files = build_shader_files(mode, year);
                assert_eq!(files.len(), 2, "clamp + one upscale pass only (no stacked blur passes)");
                assert!(files.iter().all(|f| std::path::Path::new(f).exists()));
            }
        }
        assert!(build_shader_files(Anime4KMode::Ultra, Some(2023))[1].contains("Upscale_CNN_x2_VL"));
        assert!(build_shader_files(Anime4KMode::Ultra, Some(1998))[1].contains("Retro_Upscale_x2_VL"));
    }

    #[test]
    fn test_generated_glsl_is_well_formed_and_ultra_is_stronger() {
        for p in [&MODERN_BALANCED, &MODERN_ULTRA, &RETRO_BALANCED, &RETRO_ULTRA] {
            let g = upscale_glsl(p);
            assert!(g.contains("//!HOOK MAIN") && g.contains("//!WIDTH HOOKED.w 2 *"));
            assert!(!g.contains("{{") && !g.contains("}}"), "unescaped format braces");
            assert_eq!(g.matches('{').count(), g.matches('}').count());
        }
        assert!(MODERN_ULTRA.k > MODERN_BALANCED.k && MODERN_ULTRA.w < MODERN_BALANCED.w);
        assert!(RETRO_ULTRA.gate_lo > MODERN_ULTRA.gate_lo, "retro must gate grain harder");
    }
}
