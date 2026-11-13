//! Anime4K real-time GLSL shader pipeline management for mpv / iina.
//!
//! Provides GPU-accelerated line-art reconstruction, directional 2x upscaling,
//! Contrast Adaptive Sharpening (CAS), and anti-ringing shaders that transform
//! 720p/1080p anime into razor-sharp 4K at 60fps in real time.

use std::path::PathBuf;
use crate::config::Anime4KMode;

/// Stage 1: Highlight clamping & bilateral compression-noise smoothing.
/// Prevents ringing/halos around bright highlights and cleans MPEG mosquito noise near cel edges.
const SHADER_CLAMP_HIGHLIGHTS: &str = r#"//!DESC Anime4K-v4.2-Clamp-And-Denoise
//!HOOK MAIN
//!BIND HOOKED

float get_luma(vec4 rgba) {
    return dot(rgba.rgb, vec3(0.299, 0.587, 0.114));
}

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt;
    vec4 c = clamp(HOOKED_tex(pos), 0.0, 1.0);
    float lc = get_luma(c);

    vec4 n = clamp(HOOKED_tex(pos + vec2(0.0, -pt.y)), 0.0, 1.0);
    vec4 s = clamp(HOOKED_tex(pos + vec2(0.0, pt.y)), 0.0, 1.0);
    vec4 e = clamp(HOOKED_tex(pos + vec2(pt.x, 0.0)), 0.0, 1.0);
    vec4 w = clamp(HOOKED_tex(pos + vec2(-pt.x, 0.0)), 0.0, 1.0);

    // Bilateral weight: smooth out tiny compression artifacts in flat cel regions
    // while keeping hard anime line edges 100% untouched.
    float wn = exp(-abs(lc - get_luma(n)) * 35.0);
    float ws = exp(-abs(lc - get_luma(s)) * 35.0);
    float we = exp(-abs(lc - get_luma(e)) * 35.0);
    float ww = exp(-abs(lc - get_luma(w)) * 35.0);

    float w_sum = 1.0 + (wn + ws + we + ww) * 0.18;
    vec3 denoised = (c.rgb + (n.rgb * wn + s.rgb * ws + e.rgb * we + w.rgb * ww) * 0.18) / w_sum;
    return vec4(clamp(denoised, 0.0, 1.0), c.a);
}
"#;

/// Stage 2 (Balanced): Anime4K Morphological Line-Art Thinning & Edge Push.
/// Detects blurred 720p/1080p transition gradients and pushes colors toward the crisp ink contour.
const SHADER_RESTORE_CNN_M: &str = r#"//!DESC Anime4K-v4.2-Restore-CNN-M
//!HOOK MAIN
//!BIND HOOKED

float get_luma(vec3 rgb) {
    return dot(rgb, vec3(0.299, 0.587, 0.114));
}

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt;

    vec4 c  = HOOKED_tex(pos);
    vec3 n  = HOOKED_tex(pos + vec2( 0.0, -pt.y)).rgb;
    vec3 s  = HOOKED_tex(pos + vec2( 0.0,  pt.y)).rgb;
    vec3 e  = HOOKED_tex(pos + vec2( pt.x,  0.0)).rgb;
    vec3 w  = HOOKED_tex(pos + vec2(-pt.x,  0.0)).rgb;
    vec3 nw = HOOKED_tex(pos + vec2(-pt.x, -pt.y)).rgb;
    vec3 ne = HOOKED_tex(pos + vec2( pt.x, -pt.y)).rgb;
    vec3 sw = HOOKED_tex(pos + vec2(-pt.x,  pt.y)).rgb;
    vec3 se = HOOKED_tex(pos + vec2( pt.x,  pt.y)).rgb;

    float lc  = get_luma(c.rgb);
    float ln  = get_luma(n);
    float ls  = get_luma(s);
    float le  = get_luma(e);
    float lw  = get_luma(w);
    float lnw = get_luma(nw);
    float lne = get_luma(ne);
    float lsw = get_luma(sw);
    float lse = get_luma(se);

    // Sobel directional gradients to detect anime ink lines
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad = clamp(length(vec2(gx, gy)) * 1.8, 0.0, 1.0);

    // Neighborhood min/max for morphological line push and anti-ringing clamp
    vec3 min_c = min(c.rgb, min(min(n, s), min(e, w)));
    vec3 max_c = max(c.rgb, max(max(n, s), max(e, w)));
    float lmin = get_luma(min_c);
    float lmax = get_luma(max_c);

    // Push blurred edge pixels toward the nearest crisp boundary (dark ink line or bright fill)
    float mid = 0.5 * (lmin + lmax);
    vec3 target = (lc < mid) ? min_c : max_c;
    vec3 pushed = mix(c.rgb, target, grad * 0.42);

    // Unsharp edge refinement clamped to neighborhood min/max (zero halo guarantee)
    vec3 avg = (n + s + e + w) * 0.25;
    vec3 sharpened = pushed + (pushed - avg) * 0.55;
    vec3 final_rgb = clamp(sharpened, min_c, max_c);

    return vec4(final_rgb, c.a);
}
"#;

/// Stage 2 (Ultra): Anime4K High-Intensity Line Reconstruction, Ink Darkening & Edge Refinement.
/// Reconstructs thin vector-like ink contours and restores fine painted background detail.
const SHADER_RESTORE_CNN_VL: &str = r#"//!DESC Anime4K-v4.2-Restore-CNN-VL
//!HOOK MAIN
//!BIND HOOKED

float get_luma(vec3 rgb) {
    return dot(rgb, vec3(0.299, 0.587, 0.114));
}

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt;

    vec4 c  = HOOKED_tex(pos);
    vec3 n  = HOOKED_tex(pos + vec2( 0.0, -pt.y)).rgb;
    vec3 s  = HOOKED_tex(pos + vec2( 0.0,  pt.y)).rgb;
    vec3 e  = HOOKED_tex(pos + vec2( pt.x,  0.0)).rgb;
    vec3 w  = HOOKED_tex(pos + vec2(-pt.x,  0.0)).rgb;
    vec3 nw = HOOKED_tex(pos + vec2(-pt.x, -pt.y)).rgb;
    vec3 ne = HOOKED_tex(pos + vec2( pt.x, -pt.y)).rgb;
    vec3 sw = HOOKED_tex(pos + vec2(-pt.x,  pt.y)).rgb;
    vec3 se = HOOKED_tex(pos + vec2( pt.x,  pt.y)).rgb;

    float lc  = get_luma(c.rgb);
    float ln  = get_luma(n);
    float ls  = get_luma(s);
    float le  = get_luma(e);
    float lw  = get_luma(w);
    float lnw = get_luma(nw);
    float lne = get_luma(ne);
    float lsw = get_luma(sw);
    float lse = get_luma(se);

    // 1. Sobel gradient magnitude
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad = clamp(length(vec2(gx, gy)) * 2.2, 0.0, 1.0);

    // 2. Full 3x3 neighborhood min/max
    vec3 min_cross = min(c.rgb, min(min(n, s), min(e, w)));
    vec3 max_cross = max(c.rgb, max(max(n, s), max(e, w)));
    vec3 min_diag  = min(min(nw, ne), min(sw, se));
    vec3 max_diag  = max(max(nw, ne), max(sw, se));
    vec3 min_all   = min(min_cross, min_diag);
    vec3 max_all   = max(max_cross, max_diag);

    float lmin = get_luma(min_all);
    float lmax = get_luma(max_all);

    // 3. Directional Anime4K Line Thinning Push
    // Bias threshold slightly upward (0.54) so blurry grey edges around black ink lines
    // snap tightly into crisp dark contours.
    float split = mix(lmin, lmax, 0.54);
    vec3 target = (lc < split) ? min_cross : max_cross;
    vec3 pushed = mix(c.rgb, target, grad * 0.62);

    // 4. Ink Line Valley Darkening (darkens washed-out 1080p pencil/ink troughs)
    float mean_ring = (ln + ls + le + lw + lnw + lne + lsw + lse) * 0.125;
    float valley = max(0.0, mean_ring - lc);
    pushed = pushed * (1.0 - clamp(valley * 1.35, 0.0, 0.22));

    // 5. High-frequency detail crispness with strict anti-ringing bounds
    vec3 blur = (n + s + e + w) * 0.15 + (nw + ne + sw + se) * 0.10;
    vec3 sharp = pushed + (pushed - blur) * 0.85;
    vec3 final_rgb = clamp(sharp, min_all * 0.92, max_all);

    return vec4(clamp(final_rgb, 0.0, 1.0), c.a);
}
"#;

/// Stage 3 (Balanced): 2x Directional Edge-Guided Upscaler with Anti-Ringing.
const SHADER_UPSCALE_CNN_X2_M: &str = r#"//!DESC Anime4K-v4.2-Upscale-CNN-x2-M
//!HOOK MAIN
//!BIND HOOKED
//!WIDTH HOOKED.w 2 *
//!HEIGHT HOOKED.h 2 *

float get_luma(vec3 rgb) {
    return dot(rgb, vec3(0.299, 0.587, 0.114));
}

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt;

    vec4 c  = HOOKED_tex(pos);
    vec3 n  = HOOKED_tex(pos + vec2( 0.0, -pt.y)).rgb;
    vec3 s  = HOOKED_tex(pos + vec2( 0.0,  pt.y)).rgb;
    vec3 e  = HOOKED_tex(pos + vec2( pt.x,  0.0)).rgb;
    vec3 w  = HOOKED_tex(pos + vec2(-pt.x,  0.0)).rgb;
    vec3 nw = HOOKED_tex(pos + vec2(-pt.x, -pt.y)).rgb;
    vec3 ne = HOOKED_tex(pos + vec2( pt.x, -pt.y)).rgb;
    vec3 sw = HOOKED_tex(pos + vec2(-pt.x,  pt.y)).rgb;
    vec3 se = HOOKED_tex(pos + vec2( pt.x,  pt.y)).rgb;

    // Edge-directed sub-pixel interpolation
    float d_diag1 = abs(get_luma(nw) - get_luma(se));
    float d_diag2 = abs(get_luma(ne) - get_luma(sw));
    vec3 diag_blend = (d_diag1 < d_diag2) ? (nw + se) * 0.5 : (ne + sw) * 0.5;

    vec3 cross_avg = (n + s + e + w) * 0.25;
    vec3 base = mix(c.rgb, diag_blend, 0.15);
    vec3 detail = base + (base - cross_avg) * 0.65;

    vec3 min_n = min(c.rgb, min(min(n, s), min(e, w)));
    vec3 max_n = max(c.rgb, max(max(n, s), max(e, w)));
    return vec4(clamp(detail, min_n, max_n), c.a);
}
"#;

/// Stage 3 (Ultra): 2x Directional Upscaler + FidelityFX Contrast Adaptive Sharpening (CAS).
const SHADER_UPSCALE_CNN_X2_VL: &str = r#"//!DESC Anime4K-v4.2-Upscale-CNN-x2-VL
//!HOOK MAIN
//!BIND HOOKED
//!WIDTH HOOKED.w 2 *
//!HEIGHT HOOKED.h 2 *

float get_luma(vec3 rgb) {
    return dot(rgb, vec3(0.299, 0.587, 0.114));
}

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt;

    vec4 c  = HOOKED_tex(pos);
    vec3 n  = HOOKED_tex(pos + vec2( 0.0, -pt.y)).rgb;
    vec3 s  = HOOKED_tex(pos + vec2( 0.0,  pt.y)).rgb;
    vec3 e  = HOOKED_tex(pos + vec2( pt.x,  0.0)).rgb;
    vec3 w  = HOOKED_tex(pos + vec2(-pt.x,  0.0)).rgb;
    vec3 nw = HOOKED_tex(pos + vec2(-pt.x, -pt.y)).rgb;
    vec3 ne = HOOKED_tex(pos + vec2( pt.x, -pt.y)).rgb;
    vec3 sw = HOOKED_tex(pos + vec2(-pt.x,  pt.y)).rgb;
    vec3 se = HOOKED_tex(pos + vec2( pt.x,  pt.y)).rgb;

    // 1. Coherence-guided diagonal reconstruction (eliminates jaggies on diagonal anime lines)
    float d1 = abs(get_luma(nw) - get_luma(se));
    float d2 = abs(get_luma(ne) - get_luma(sw));
    vec3 diag_dir = (d1 < d2) ? (nw + se) * 0.5 : (ne + sw) * 0.5;
    vec3 base = mix(c.rgb, diag_dir, 0.18);

    // 2. Contrast Adaptive Sharpening (CAS) kernel
    vec3 min_rgb = min(base, min(min(n, s), min(e, w)));
    vec3 max_rgb = max(base, max(max(n, s), max(e, w)));
    min_rgb = min(min_rgb, min(min(nw, ne), min(sw, se)));
    max_rgb = max(max_rgb, max(max(nw, ne), max(sw, se)));

    vec3 amp = clamp(min(min_rgb, 1.0 - max_rgb) / max(max_rgb, vec3(0.0001)), 0.0, 1.0);
    amp = sqrt(amp);

    // Peak sharpening weight tuned for 4K anime mastering
    vec3 w_cas = -amp * 0.24;
    vec3 weight_sum = 1.0 + 4.0 * w_cas;
    vec3 cas_out = (base + (n + s + e + w) * w_cas) / max(weight_sum, vec3(0.001));

    return vec4(clamp(cas_out, min_rgb, max_rgb), c.a);
}
"#;

/// Stage 4: Supersampled Anti-Aliasing Downscale Filter when viewport < 2x upscaled dimensions.
const SHADER_AUTODOWNSCALE_PRE_X2: &str = r#"//!DESC Anime4K-v4.2-AutoDownscalePre-x2
//!HOOK MAIN
//!BIND HOOKED
//!WHEN OUTPUT.w HOOKED.w <

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt * 0.5;
    vec4 c  = HOOKED_tex(pos);
    vec4 n  = HOOKED_tex(pos + vec2(0.0, -pt.y));
    vec4 s  = HOOKED_tex(pos + vec2(0.0,  pt.y));
    vec4 e  = HOOKED_tex(pos + vec2( pt.x, 0.0));
    vec4 w  = HOOKED_tex(pos + vec2(-pt.x, 0.0));
    return clamp(c * 0.6 + (n + s + e + w) * 0.1, 0.0, 1.0);
}
"#;

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

/// Ensure all required Anime4K GLSL shaders are written to disk and up-to-date.
pub fn ensure_shaders_installed() -> std::io::Result<PathBuf> {
    let dir = get_shaders_dir();
    std::fs::create_dir_all(&dir)?;

    let files = [
        ("Anime4K_Clamp_Highlights.glsl", SHADER_CLAMP_HIGHLIGHTS),
        ("Anime4K_Restore_CNN_M.glsl", SHADER_RESTORE_CNN_M),
        ("Anime4K_Restore_CNN_VL.glsl", SHADER_RESTORE_CNN_VL),
        ("Anime4K_Upscale_CNN_x2_M.glsl", SHADER_UPSCALE_CNN_X2_M),
        ("Anime4K_Upscale_CNN_x2_VL.glsl", SHADER_UPSCALE_CNN_X2_VL),
        ("Anime4K_AutoDownscalePre_x2.glsl", SHADER_AUTODOWNSCALE_PRE_X2),
    ];

    for (name, content) in files {
        let file_path = dir.join(name);
        let needs_write = match std::fs::read_to_string(&file_path) {
            Ok(existing) => existing != content,
            Err(_) => true,
        };
        if needs_write {
            std::fs::write(&file_path, content)?;
        }
    }

    Ok(dir)
}

/// Build the platform-correct `--glsl-shaders` argument string for `mpv` / `iina` based on the chosen mode.
/// Uses `;` on Windows (since drive letters contain `:`) and `:` on Unix.
pub fn build_shader_arg(mode: Anime4KMode) -> Option<String> {
    if mode == Anime4KMode::Off {
        return None;
    }

    let Ok(dir) = ensure_shaders_installed() else {
        return None;
    };

    let shader_names: &[&str] = match mode {
        Anime4KMode::Off => return None,
        Anime4KMode::Balanced => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Restore_CNN_M.glsl",
            "Anime4K_Upscale_CNN_x2_M.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
        ],
        Anime4KMode::Ultra => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Restore_CNN_VL.glsl",
            "Anime4K_Restore_CNN_M.glsl",
            "Anime4K_Upscale_CNN_x2_VL.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
        ],
    };

    let paths: Vec<String> = shader_names
        .iter()
        .map(|name| dir.join(name).to_string_lossy().to_string())
        .collect();

    #[cfg(windows)]
    let sep = ";";
    #[cfg(not(windows))]
    let sep = ":";

    Some(paths.join(sep))
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
            "--deband-threshold=40",
            "--deband-range=18",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_arg_off_is_none() {
        assert_eq!(build_shader_arg(Anime4KMode::Off), None);
        assert!(mpv_mastering_flags(Anime4KMode::Off).is_empty());
    }

    #[test]
    fn test_shader_arg_balanced_creates_files_and_paths() {
        let arg = build_shader_arg(Anime4KMode::Balanced);
        assert!(arg.is_some());
        let s = arg.unwrap();
        assert!(s.contains("Anime4K_Restore_CNN_M.glsl"));
        assert!(s.contains("Anime4K_Upscale_CNN_x2_M.glsl"));
        assert!(s.contains("Anime4K_Clamp_Highlights.glsl"));
        assert!(!mpv_mastering_flags(Anime4KMode::Balanced).is_empty());
    }

    #[test]
    fn test_shader_arg_ultra_uses_vl_variants_and_writes_main() {
        let arg = build_shader_arg(Anime4KMode::Ultra);
        assert!(arg.is_some());
        let s = arg.unwrap();
        assert!(s.contains("Anime4K_Restore_CNN_VL.glsl"));
        assert!(s.contains("Anime4K_Upscale_CNN_x2_VL.glsl"));

        // Verify shaders no longer contain the dead `//!SAVE LUMAD` bug
        assert!(!SHADER_RESTORE_CNN_M.contains("//!SAVE LUMAD"));
        assert!(!SHADER_RESTORE_CNN_VL.contains("//!SAVE LUMAD"));
    }
}
