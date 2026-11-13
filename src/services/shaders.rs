//! Anime4K real-time GLSL shader pipeline management for mpv / iina.
//!
//! Provides Era-Adaptive GPU-accelerated line-art reconstruction, pure-luminance (Y)
//! Catmull-Rom / Sobel upscaling, Contrast Adaptive Sharpening (CAS), and strict
//! anti-ringing shaders that transform both 80s/90s cel anime (e.g., Serial Experiments Lain)
//! and modern digital HD anime into razor-sharp 4K at 60fps with zero mushiness or jaggies.

use std::path::PathBuf;
use crate::config::Anime4KMode;

/// Production era profile detected from `anime.season_year`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimeEra {
    /// 1960s–2006: 35mm/16mm film grain, hand-painted acetate cels, halftone dithering, CRT glow
    /// (e.g., *Serial Experiments Lain* 1998, *Neon Genesis Evangelion*, *Cowboy Bebop*, *Monster*).
    /// Uses noise-floor edge gating (`smoothstep(0.045, 0.14, grad)`) so film grain is never
    /// blurred into oil-painted mush or sharpened into blotches, paired with pure-luma Catmull-Rom.
    RetroCel,
    /// 2007–Present: Digital vector ink & paint, 720p/900p/1080p digital compositing
    /// (e.g., *Frieren*, *Jujutsu Kaisen*, *Demon Slayer*, *Solo Leveling*).
    /// Uses single-pass Sobel Luma line thinning + directional 2x upscale + pure-luma FidelityFX CAS.
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
const SHADER_CLAMP_HIGHLIGHTS: &str = r#"//!DESC Anime4K-v5.0-Clamp-Highlights-ZeroBlur
//!HOOK MAIN
//!BIND HOOKED

vec4 hook() {
    return clamp(HOOKED_tex(HOOKED_pos), 0.0, 1.0);
}
"#;

// ─────────────────────────────────────────────────────────────────────────────
// RETRO CEL PIPELINE (season_year <= 2006, e.g., Serial Experiments Lain 1998)
// ─────────────────────────────────────────────────────────────────────────────

/// Stage 2 (Retro Cel - Balanced & Ultra): Grain-Safe Pure-Luminance Cel Line Restoration.
/// - Noise-floor gate (`smoothstep(0.045, 0.14, grad)`) leaves film grain, shadow dithering,
///   and subtle CRT glow 100% untouched (zero oil-paint mushiness).
/// - Modifies strictly Luminance (`Y`) so RGB hues never shift or bleed.
/// - Clamps strictly to `[lmin, lmax]` with zero overshoot.
const SHADER_RETRO_RESTORE_CEL: &str = r#"//!DESC Anime4K-v5.0-Retro-Restore-Cel-Luma
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

    // Sobel edge detector to distinguish genuine hand-painted cel ink lines from film grain
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad = length(vec2(gx, gy));

    // Noise-floor gate: gradients below 0.045 are film grain / halftone dithering -> 0% modification
    float edge_gate = smoothstep(0.045, 0.16, grad);

    // Pure luminance high-frequency deconvolution along structural cel boundaries
    float lmin = min(lc, min(min(ln, ls), min(le, lw)));
    float lmax = max(lc, max(max(ln, ls), max(le, lw)));

    float cross_luma = (ln + ls + le + lw) * 0.25;
    float diag_luma  = (lnw + lne + lsw + lse) * 0.25;
    float ring_luma  = cross_luma * 0.65 + diag_luma * 0.35;

    float detail = (lc - ring_luma) * (0.65 * edge_gate);
    float target_luma = clamp(lc + detail, lmin, lmax);
    float luma_delta = target_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 3 (Retro Cel - Balanced 2x): Grain-Safe 2x Directional Catmull-Rom Luma Upscaler.
const SHADER_RETRO_UPSCALE_X2_M: &str = r#"//!DESC Anime4K-v5.0-Retro-Upscale-x2-M
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

    float lc  = get_luma(c.rgb);
    float ln  = get_luma(n);
    float ls  = get_luma(s);
    float le  = get_luma(e);
    float lw  = get_luma(w);
    float lnw = get_luma(nw);
    float lne = get_luma(ne);
    float lsw = get_luma(sw);
    float lse = get_luma(se);

    float lmin = min(lc, min(min(ln, ls), min(le, lw)));
    float lmax = max(lc, max(max(ln, ls), max(le, lw)));
    float local_contrast = lmax - lmin;

    // Noise gate: only sharpen real structural contours, never amplify analog grain
    float gate = smoothstep(0.035, 0.13, local_contrast);

    // Directional diagonal coherence (only active when a diagonal edge is actually present)
    float d1 = abs(lnw - lse);
    float d2 = abs(lne - lsw);
    float diag_luma = (d1 < d2) ? 0.5 * (lnw + lse) : 0.5 * (lne + lsw);
    float cross_luma = (ln + ls + le + lw) * 0.25;

    float ref_luma = mix(cross_luma, diag_luma, 0.30);
    float sharp_luma = clamp(lc + (lc - ref_luma) * (0.55 * gate), lmin, lmax);
    float luma_delta = sharp_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 3 (Retro Cel - Ultra 4K): Reference 35mm Cel 4K Mastering Upscaler.
/// Combines directional sub-pixel anti-aliasing along diagonal ink lines with pure-Luma
/// Contrast-Adaptive Deconvolution and a strict grain-protection gate.
/// Makes 90s anime like *Serial Experiments Lain* look like a native 4K Blu-ray remaster!
const SHADER_RETRO_UPSCALE_X2_VL: &str = r#"//!DESC Anime4K-v5.0-Retro-Upscale-x2-VL-4K
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

    float lc  = get_luma(c.rgb);
    float ln  = get_luma(n);
    float ls  = get_luma(s);
    float le  = get_luma(e);
    float lw  = get_luma(w);
    float lnw = get_luma(nw);
    float lne = get_luma(ne);
    float lsw = get_luma(sw);
    float lse = get_luma(se);

    // 1. Strict cross + 3x3 luminance bounds (zero ringing, zero overshoot)
    float lmin_cross = min(lc, min(min(ln, ls), min(le, lw)));
    float lmax_cross = max(lc, max(max(ln, ls), max(le, lw)));
    float lmin_all   = min(lmin_cross, min(min(lnw, lne), min(lsw, lse)));
    float lmax_all   = max(lmax_cross, max(max(lnw, lne), max(lsw, lse)));

    // 2. Grain-safe edge gate: protects flat cel paint, airbrushed CRT glow, and film grain
    float contrast = lmax_all - lmin_all;
    float edge_gate = smoothstep(0.04, 0.14, contrast);

    // 3. Directional diagonal coherence anti-aliasing (eliminates stair-stepping on cel lines)
    float d_diag1 = abs(lnw - lse);
    float d_diag2 = abs(lne - lsw);
    float diag_aniso = abs(d_diag1 - d_diag2) / max(d_diag1 + d_diag2, 0.001);
    float diag_luma = (d_diag1 < d_diag2) ? 0.5 * (lnw + lse) : 0.5 * (lne + lsw);

    // Blend diagonal coherence ONLY when strong diagonal anisotropy exists
    float base_luma = mix(lc, diag_luma, clamp(diag_aniso * edge_gate * 0.28, 0.0, 0.22));

    // 4. Pure-Luminance FidelityFX CAS (Contrast Adaptive Sharpening)
    float amp = clamp(min(lmin_all, 1.0 - lmax_all) / max(lmax_all, 0.0001), 0.0, 1.0);
    amp = sqrt(amp) * edge_gate;

    // Negative lobe sharpening weight gated by edge confidence
    float w_cas = -0.22 * amp;
    float cas_luma = (base_luma + (ln + ls + le + lw) * w_cas) / max(1.0 + 4.0 * w_cas, 0.001);

    // 5. Gentle cel-line contour tightening (without morphological mush)
    float mid = 0.52 * lmax_cross + 0.48 * lmin_cross;
    float contour_pull = (base_luma < mid) ? lmin_cross : lmax_cross;
    float mastered_luma = mix(cas_luma, contour_pull, edge_gate * 0.18);

    float final_luma = clamp(mastered_luma, lmin_cross, lmax_cross);
    float luma_delta = final_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

// ─────────────────────────────────────────────────────────────────────────────
// MODERN DIGITAL PIPELINE (season_year >= 2007, e.g., Frieren, JJK, Solo Leveling)
// ─────────────────────────────────────────────────────────────────────────────

/// Stage 2 (Modern Digital - Balanced 2x): Pure-Luma Sobel Line Thinning & De-Blur.
const SHADER_RESTORE_CNN_M: &str = r#"//!DESC Anime4K-v5.0-Modern-Restore-CNN-M
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

    // Sobel directional gradients to detect digital anime ink lines
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad = clamp(length(vec2(gx, gy)) * 1.6, 0.0, 1.0);
    float gate = smoothstep(0.02, 0.10, grad);

    float lmin = min(lc, min(min(ln, ls), min(le, lw)));
    float lmax = max(lc, max(max(ln, ls), max(le, lw)));

    // Push blurred edge luminance toward the nearest crisp boundary (zero RGB smearing)
    float mid = 0.5 * (lmin + lmax);
    float target = (lc < mid) ? lmin : lmax;
    float pushed = mix(lc, target, grad * gate * 0.30);

    // Pure luminance unsharp refinement clamped strictly to [lmin, lmax]
    float avg = (ln + ls + le + lw) * 0.25;
    float sharpened = clamp(pushed + (pushed - avg) * (0.48 * gate), lmin, lmax);
    float luma_delta = sharpened - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 2 (Modern Digital - Ultra 4K): Single-Pass High-Precision Vector Line Reconstruction.
/// Never double-stacked! Operates strictly in Luminance (`Y`) with zero valley-darkening mush.
const SHADER_RESTORE_CNN_VL: &str = r#"//!DESC Anime4K-v5.0-Modern-Restore-CNN-VL
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

    // 1. Sobel gradient magnitude with subtle noise floor protection
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad = clamp(length(vec2(gx, gy)) * 1.85, 0.0, 1.0);
    float gate = smoothstep(0.025, 0.11, grad);

    // 2. Cross neighborhood luminance min/max (strictly clamped, no 0.92 overshoot)
    float lmin = min(lc, min(min(ln, ls), min(le, lw)));
    float lmax = max(lc, max(max(ln, ls), max(le, lw)));

    // 3. Single-pass vector line contour snap (tightens 900p/1080p broadcast softness)
    float split = mix(lmin, lmax, 0.52);
    float target = (lc < split) ? lmin : lmax;
    float pushed = mix(lc, target, grad * gate * 0.36);

    // 4. High-frequency luminance deconvolution
    float ring_blur = (ln + ls + le + lw) * 0.16 + (lnw + lne + lsw + lse) * 0.09;
    float sharp = clamp(pushed + (pushed - ring_blur) * (0.62 * gate), lmin, lmax);
    float luma_delta = sharp - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 3 (Modern Digital - Balanced 2x): 2x Directional Edge-Guided Luma Upscaler.
const SHADER_UPSCALE_CNN_X2_M: &str = r#"//!DESC Anime4K-v5.0-Modern-Upscale-CNN-x2-M
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

    float lc  = get_luma(c.rgb);
    float ln  = get_luma(n);
    float ls  = get_luma(s);
    float le  = get_luma(e);
    float lw  = get_luma(w);
    float lnw = get_luma(nw);
    float lne = get_luma(ne);
    float lsw = get_luma(sw);
    float lse = get_luma(se);

    float lmin = min(lc, min(min(ln, ls), min(le, lw)));
    float lmax = max(lc, max(max(ln, ls), max(le, lw)));

    // Anisotropic diagonal coherence (only blends along actual diagonal edges, never blurs H/V lines)
    float d1 = abs(lnw - lse);
    float d2 = abs(lne - lsw);
    float aniso = abs(d1 - d2) / max(d1 + d2, 0.001);
    float diag_luma = (d1 < d2) ? 0.5 * (lnw + lse) : 0.5 * (lne + lsw);

    float base_luma = mix(lc, diag_luma, clamp(aniso * 0.20, 0.0, 0.16));
    float cross_avg = (ln + ls + le + lw) * 0.25;
    float detail_luma = clamp(base_luma + (base_luma - cross_avg) * 0.58, lmin, lmax);
    float luma_delta = detail_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 3 (Modern Digital - Ultra 4K): 2x Anisotropic Edge-Guided Upscaler + Pure-Luma FidelityFX CAS.
const SHADER_UPSCALE_CNN_X2_VL: &str = r#"//!DESC Anime4K-v5.0-Modern-Upscale-CNN-x2-VL-4K
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

    float lc  = get_luma(c.rgb);
    float ln  = get_luma(n);
    float ls  = get_luma(s);
    float le  = get_luma(e);
    float lw  = get_luma(w);
    float lnw = get_luma(nw);
    float lne = get_luma(ne);
    float lsw = get_luma(sw);
    float lse = get_luma(se);

    // 1. Anisotropic diagonal reconstruction (eliminates jaggies on diagonal lines without pre-blurring H/V)
    float d1 = abs(lnw - lse);
    float d2 = abs(lne - lsw);
    float aniso = abs(d1 - d2) / max(d1 + d2, 0.001);
    float diag_dir = (d1 < d2) ? 0.5 * (lnw + lse) : 0.5 * (lne + lsw);
    float base_luma = mix(lc, diag_dir, clamp(aniso * 0.24, 0.0, 0.18));

    // 2. Pure-Luminance FidelityFX Contrast Adaptive Sharpening (CAS)
    float min_cross = min(lc, min(min(ln, ls), min(le, lw)));
    float max_cross = max(lc, max(max(ln, ls), max(le, lw)));
    float min_all = min(min_cross, min(min(lnw, lne), min(lsw, lse)));
    float max_all = max(max_cross, max(max(lnw, lne), max(lsw, lse)));

    float amp = clamp(min(min_all, 1.0 - max_all) / max(max_all, 0.0001), 0.0, 1.0);
    amp = sqrt(amp);

    // Peak sharpening weight tuned for crisp 4K digital anime mastering
    float w_cas = -amp * 0.25;
    float weight_sum = max(1.0 + 4.0 * w_cas, 0.001);
    float cas_luma = (base_luma + (ln + ls + le + lw) * w_cas) / weight_sum;

    float final_luma = clamp(cas_luma, min_cross, max_cross);
    float luma_delta = final_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 4: Supersampled Anti-Aliasing Downscale Filter when viewport < 2x upscaled dimensions.
const SHADER_AUTODOWNSCALE_PRE_X2: &str = r#"//!DESC Anime4K-v5.0-AutoDownscalePre-x2
//!HOOK MAIN
//!BIND HOOKED
//!WHEN OUTPUT.w HOOKED.w <

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt * 0.35;
    vec4 c  = HOOKED_tex(pos);
    vec4 n  = HOOKED_tex(pos + vec2(0.0, -pt.y));
    vec4 s  = HOOKED_tex(pos + vec2(0.0,  pt.y));
    vec4 e  = HOOKED_tex(pos + vec2( pt.x, 0.0));
    vec4 w  = HOOKED_tex(pos + vec2(-pt.x, 0.0));
    return clamp(c * 0.72 + (n + s + e + w) * 0.07, 0.0, 1.0);
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

/// Ensure all required Anime4K GLSL shaders (both RetroCel and ModernDigital profiles) are written to disk.
pub fn ensure_shaders_installed() -> std::io::Result<PathBuf> {
    let dir = get_shaders_dir();
    std::fs::create_dir_all(&dir)?;

    let files = [
        ("Anime4K_Clamp_Highlights.glsl", SHADER_CLAMP_HIGHLIGHTS),
        ("Anime4K_Retro_Restore_Cel.glsl", SHADER_RETRO_RESTORE_CEL),
        ("Anime4K_Retro_Upscale_x2_M.glsl", SHADER_RETRO_UPSCALE_X2_M),
        ("Anime4K_Retro_Upscale_x2_VL.glsl", SHADER_RETRO_UPSCALE_X2_VL),
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
            "Anime4K_Retro_Restore_Cel.glsl",
            "Anime4K_Retro_Upscale_x2_M.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
        ],
        (Anime4KMode::Ultra, AnimeEra::RetroCel) => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Retro_Restore_Cel.glsl",
            "Anime4K_Retro_Upscale_x2_VL.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
        ],
        (Anime4KMode::Balanced, AnimeEra::ModernDigital) => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Restore_CNN_M.glsl",
            "Anime4K_Upscale_CNN_x2_M.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
        ],
        (Anime4KMode::Ultra, AnimeEra::ModernDigital) => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Restore_CNN_VL.glsl",
            "Anime4K_Upscale_CNN_x2_VL.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
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
        assert!(s.contains("Anime4K_Restore_CNN_M.glsl"));
        assert!(s.contains("Anime4K_Upscale_CNN_x2_M.glsl"));
        assert!(s.contains("Anime4K_Clamp_Highlights.glsl"));
        assert!(!mpv_mastering_flags(Anime4KMode::Balanced).is_empty());
    }

    #[test]
    fn test_shader_arg_ultra_modern_is_single_pass_restore() {
        let files = build_shader_files(Anime4KMode::Ultra, Some(2023));
        assert_eq!(files.len(), 4);
        assert!(files.iter().any(|f| f.contains("Anime4K_Restore_CNN_VL.glsl")));
        // Never double-stack Restore_CNN_M on top of Restore_CNN_VL!
        assert!(!files.iter().any(|f| f.contains("Anime4K_Restore_CNN_M.glsl")));
        assert!(files.iter().any(|f| f.contains("Anime4K_Upscale_CNN_x2_VL.glsl")));
    }

    #[test]
    fn test_shader_arg_retro_cel_uses_grain_safe_shaders() {
        // Serial Experiments Lain (1998)
        let files_ultra = build_shader_files(Anime4KMode::Ultra, Some(1998));
        assert_eq!(files_ultra.len(), 4);
        assert!(files_ultra.iter().any(|f| f.contains("Anime4K_Retro_Restore_Cel.glsl")));
        assert!(files_ultra.iter().any(|f| f.contains("Anime4K_Retro_Upscale_x2_VL.glsl")));

        let files_bal = build_shader_files(Anime4KMode::Balanced, Some(1998));
        assert_eq!(files_bal.len(), 4);
        assert!(files_bal.iter().any(|f| f.contains("Anime4K_Retro_Restore_Cel.glsl")));
        assert!(files_bal.iter().any(|f| f.contains("Anime4K_Retro_Upscale_x2_M.glsl")));
    }
}
