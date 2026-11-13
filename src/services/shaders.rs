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
// RETRO CEL PIPELINE (season_year <= 2006, e.g., Serial Experiments Lain 1998)
// ─────────────────────────────────────────────────────────────────────────────

/// Stage 2 (Retro Cel - Balanced & Ultra): Grain-Safe Pure-Luminance Cel Line Restoration
/// with Sub-Pixel Tangent Continuity (prevents stair-stepping on thin black cel lines).
const SHADER_RETRO_RESTORE_CEL: &str = r#"//!DESC Anime4K-v5.2-Retro-Restore-Cel-SmoothLine
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

    // 1. Sobel gradient vector (gx, gy) & orthogonal line tangent (-gy, gx)
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad_len = length(vec2(gx, gy));

    // Noise-floor gate: gradients below 0.045 are film grain / halftone dithering -> 0% modification
    float edge_gate = smoothstep(0.045, 0.16, grad_len);

    // 2. Sub-pixel tangent anti-aliasing along the stroke direction (connects thin black lines smoothly)
    vec2 tangent = (grad_len > 0.0001) ? (vec2(-gy, gx) / grad_len) : vec2(0.0);
    float t_pos = get_luma(HOOKED_tex(pos + tangent * pt * 0.65).rgb);
    float t_neg = get_luma(HOOKED_tex(pos - tangent * pt * 0.65).rgb);
    float tangent_avg = 0.5 * (t_pos + t_neg);

    // Smooth stair-steps along the line contour before sharpening across the line
    float base_luma = mix(lc, tangent_avg, edge_gate * 0.30);

    // 3. Pure luminance high-frequency deconvolution across the line normal
    float lmin = min(lc, min(min(ln, ls), min(le, lw)));
    float lmax = max(lc, max(max(ln, ls), max(le, lw)));

    float cross_luma = (ln + ls + le + lw) * 0.25;
    float diag_luma  = (lnw + lne + lsw + lse) * 0.25;
    float ring_luma  = cross_luma * 0.60 + diag_luma * 0.40;

    float detail = (base_luma - ring_luma) * (0.58 * edge_gate);
    float target_luma = clamp(base_luma + detail, lmin, lmax);
    float luma_delta = target_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 3 (Retro Cel - Balanced 2x): Grain-Safe 2x Tangent-Guided Smooth-Line Upscaler.
const SHADER_RETRO_UPSCALE_X2_M: &str = r#"//!DESC Anime4K-v5.2-Retro-Upscale-x2-M-SmoothLine
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

    // Sub-pixel tangent anti-aliasing along line direction (eliminates diagonal jaggies)
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad_len = length(vec2(gx, gy));
    vec2 tangent = (grad_len > 0.0001) ? (vec2(-gy, gx) / grad_len) : vec2(0.0);

    float t1 = get_luma(HOOKED_tex(pos + tangent * pt * 0.75).rgb);
    float t2 = get_luma(HOOKED_tex(pos - tangent * pt * 0.75).rgb);
    float tangent_smooth = 0.5 * (t1 + t2);

    float base_luma = mix(lc, tangent_smooth, gate * 0.34);

    // Isotropic 8-neighbor ring reference so sharpening is angle-invariant
    float cross_luma = (ln + ls + le + lw) * 0.25;
    float diag_luma  = (lnw + lne + lsw + lse) * 0.25;
    float ref_luma   = cross_luma * 0.58 + diag_luma * 0.42;

    // Soft-knee bounds prevent hard-clipping stair-steps on thin dark lines
    float sharp_luma = base_luma + (base_luma - ref_luma) * (0.50 * gate);
    float clamped_luma = clamp(sharp_luma, lmin, lmax);
    float final_luma = mix(clamped_luma, tangent_smooth, gate * 0.16);
    float luma_delta = final_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 3 (Retro Cel - Ultra 4K): Reference 35mm Cel 4K Mastering Upscaler.
/// Uses Structure-Tensor Tangent Smoothing along black ink lines + Isotropic 8-Tap
/// Pure-Luma CAS so thin and curved cel lines are 100% smooth, non-jagged, and razor-crisp.
const SHADER_RETRO_UPSCALE_X2_VL: &str = r#"//!DESC Anime4K-v5.2-Retro-Upscale-x2-VL-4K-SmoothLine
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

    // 3. Structure-Tensor Tangent Anti-Aliasing (connects diagonal & curved black ink lines seamlessly)
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad_len = length(vec2(gx, gy));
    vec2 tangent = (grad_len > 0.0001) ? (vec2(-gy, gx) / grad_len) : vec2(0.0);

    float t_pos = get_luma(HOOKED_tex(pos + tangent * pt * 0.75).rgb);
    float t_neg = get_luma(HOOKED_tex(pos - tangent * pt * 0.75).rgb);
    float tangent_luma = 0.5 * (t_pos + t_neg);

    // Blend along the line tangent to erase stair-step aliasing before sharpening across the edge
    float base_luma = mix(lc, tangent_luma, edge_gate * 0.36);

    // 4. Isotropic 8-Tap Pure-Luminance FidelityFX CAS (rotational symmetry = zero directional jaggies)
    float amp = clamp(min(lmin_all, 1.0 - lmax_all) / max(lmax_all, 0.0001), 0.0, 1.0);
    amp = sqrt(amp) * edge_gate;

    float w_cas = -0.19 * amp;
    float ring_neighbor = (ln + ls + le + lw) * 0.65 + (lnw + lne + lsw + lse) * 0.35;
    float cas_luma = (base_luma + ring_neighbor * w_cas) / max(1.0 + 4.0 * w_cas, 0.001);

    // 5. Smooth Tangent-Guided Contour Refinement (no binary step jaggies)
    float clamped_luma = clamp(cas_luma, lmin_cross, lmax_cross);
    float final_luma = mix(clamped_luma, tangent_luma, edge_gate * 0.18);
    float luma_delta = final_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

// ─────────────────────────────────────────────────────────────────────────────
// MODERN DIGITAL PIPELINE (season_year >= 2007, e.g., Frieren, JJK, Solo Leveling)
// ─────────────────────────────────────────────────────────────────────────────

/// Stage 2 (Modern Digital - Balanced 2x): Tangent-Guided Smooth Line Reconstruction.
/// Eliminates binary step-function jaggies on thin black lines while tightening contours.
const SHADER_RESTORE_CNN_M: &str = r#"//!DESC Anime4K-v5.2-Modern-Restore-CNN-M-SmoothLine
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

    // 1. Sobel directional gradient & orthogonal line tangent
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad_len = length(vec2(gx, gy));
    float grad = clamp(grad_len * 1.6, 0.0, 1.0);
    float gate = smoothstep(0.02, 0.10, grad);

    // 2. Sub-pixel tangent smoothing along the ink line (eliminates stair-stepping on thin black lines)
    vec2 tangent = (grad_len > 0.0001) ? (vec2(-gy, gx) / grad_len) : vec2(0.0);
    float t_pos = get_luma(HOOKED_tex(pos + tangent * pt * 0.65).rgb);
    float t_neg = get_luma(HOOKED_tex(pos - tangent * pt * 0.65).rgb);
    float tangent_luma = 0.5 * (t_pos + t_neg);

    float lmin = min(lc, min(min(ln, ls), min(le, lw)));
    float lmax = max(lc, max(max(ln, ls), max(le, lw)));

    // 3. Continuous S-curve Hermite contour tightening (replaces jagged binary lc < mid snap!)
    float range = max(lmax - lmin, 0.0001);
    float norm_t = clamp((lc - lmin) / range, 0.0, 1.0);
    float s_curve = smoothstep(0.18, 0.82, norm_t);
    float target = mix(lmin, lmax, s_curve);

    float base_luma = mix(lc, tangent_luma, gate * 0.30);
    float pushed = mix(base_luma, target, grad * gate * 0.24);

    // 4. Isotropic 8-neighbor luminance refinement clamped to [lmin, lmax]
    float avg = (ln + ls + le + lw) * 0.16 + (lnw + lne + lsw + lse) * 0.09;
    float sharpened = clamp(pushed + (pushed - avg) * (0.42 * gate), lmin, lmax);
    float final_luma = mix(sharpened, tangent_luma, gate * 0.15);
    float luma_delta = final_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 2 (Modern Digital - Ultra 4K): Single-Pass Sub-Pixel Vector Line Reconstruction.
/// Uses Tangent Anti-Aliasing + Continuous Hermite S-Curve Contour Tightening so thin black
/// lines become silky-smooth vector strokes with zero stair-stepping or jaggedness.
const SHADER_RESTORE_CNN_VL: &str = r#"//!DESC Anime4K-v5.2-Modern-Restore-CNN-VL-SmoothLine
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

    // 1. Sobel gradient vector & orthogonal line tangent (-gy, gx)
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad_len = length(vec2(gx, gy));
    float grad = clamp(grad_len * 1.8, 0.0, 1.0);
    float gate = smoothstep(0.025, 0.11, grad);

    // 2. Sub-pixel tangent continuity (connects thin black anime lines along their exact angle)
    vec2 tangent = (grad_len > 0.0001) ? (vec2(-gy, gx) / grad_len) : vec2(0.0);
    float t_pos = get_luma(HOOKED_tex(pos + tangent * pt * 0.70).rgb);
    float t_neg = get_luma(HOOKED_tex(pos - tangent * pt * 0.70).rgb);
    float tangent_luma = 0.5 * (t_pos + t_neg);

    float lmin = min(lc, min(min(ln, ls), min(le, lw)));
    float lmax = max(lc, max(max(ln, ls), max(le, lw)));

    // 3. Continuous Hermite S-Curve contour tightening (zero binary step jaggies!)
    float range = max(lmax - lmin, 0.0001);
    float norm_t = clamp((lc - lmin) / range, 0.0, 1.0);
    float s_curve = smoothstep(0.16, 0.84, norm_t);
    float target = mix(lmin, lmax, s_curve);

    float base_luma = mix(lc, tangent_luma, gate * 0.34);
    float pushed = mix(base_luma, target, grad * gate * 0.28);

    // 4. Isotropic 8-tap luminance deconvolution across the edge normal
    float ring_blur = (ln + ls + le + lw) * 0.15 + (lnw + lne + lsw + lse) * 0.10;
    float sharp = clamp(pushed + (pushed - ring_blur) * (0.50 * gate), lmin, lmax);
    float final_luma = mix(sharp, tangent_luma, gate * 0.18);
    float luma_delta = final_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 3 (Modern Digital - Balanced 2x): 2x Tangent-Guided Smooth-Line Upscaler.
const SHADER_UPSCALE_CNN_X2_M: &str = r#"//!DESC Anime4K-v5.2-Modern-Upscale-CNN-x2-M-SmoothLine
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
    float gate = smoothstep(0.02, 0.10, lmax - lmin);

    // Sub-pixel tangent anti-aliasing along line direction
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad_len = length(vec2(gx, gy));
    vec2 tangent = (grad_len > 0.0001) ? (vec2(-gy, gx) / grad_len) : vec2(0.0);

    float t_pos = get_luma(HOOKED_tex(pos + tangent * pt * 0.75).rgb);
    float t_neg = get_luma(HOOKED_tex(pos - tangent * pt * 0.75).rgb);
    float tangent_luma = 0.5 * (t_pos + t_neg);

    float base_luma = mix(lc, tangent_luma, gate * 0.35);
    float ring_avg = (ln + ls + le + lw) * 0.15 + (lnw + lne + lsw + lse) * 0.10;
    float detail_luma = clamp(base_luma + (base_luma - ring_avg) * (0.48 * gate), lmin, lmax);
    float final_luma = mix(detail_luma, tangent_luma, gate * 0.16);
    float luma_delta = final_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 3 (Modern Digital - Ultra 4K): 2x Structure-Tensor Tangent Upscaler + Isotropic 8-Tap Luma CAS.
/// Delivers razor-sharp clarity while keeping black and thin anime lines 100% smooth and jaggie-free.
const SHADER_UPSCALE_CNN_X2_VL: &str = r#"//!DESC Anime4K-v5.2-Modern-Upscale-CNN-x2-VL-4K-SmoothLine
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

    float min_cross = min(lc, min(min(ln, ls), min(le, lw)));
    float max_cross = max(lc, max(max(ln, ls), max(le, lw)));
    float min_all = min(min_cross, min(min(lnw, lne), min(lsw, lse)));
    float max_all = max(max_cross, max(max(lnw, lne), max(lsw, lse)));
    float edge_gate = smoothstep(0.02, 0.10, max_all - min_all);

    // 1. Structure-Tensor Tangent Smoothing along the exact stroke angle (eliminates jaggies on thin black lines)
    float gx = (lne + 2.0 * le + lse) - (lnw + 2.0 * lw + lsw);
    float gy = (lsw + 2.0 * ls + lse) - (lnw + 2.0 * ln + lne);
    float grad_len = length(vec2(gx, gy));
    vec2 tangent = (grad_len > 0.0001) ? (vec2(-gy, gx) / grad_len) : vec2(0.0);

    float t_pos = get_luma(HOOKED_tex(pos + tangent * pt * 0.75).rgb);
    float t_neg = get_luma(HOOKED_tex(pos - tangent * pt * 0.75).rgb);
    float tangent_luma = 0.5 * (t_pos + t_neg);

    float base_luma = mix(lc, tangent_luma, edge_gate * 0.36);

    // 2. Isotropic 8-Tap Pure-Luminance FidelityFX CAS (rotational symmetry avoids 90-degree grid stair-steps)
    float amp = clamp(min(min_all, 1.0 - max_all) / max(max_all, 0.0001), 0.0, 1.0);
    amp = sqrt(amp) * edge_gate;

    float w_cas = -amp * 0.21;
    float weight_sum = max(1.0 + 4.0 * w_cas, 0.001);
    float ring_neighbor = (ln + ls + le + lw) * 0.65 + (lnw + lne + lsw + lse) * 0.35;
    float cas_luma = (base_luma + ring_neighbor * w_cas) / weight_sum;

    // 3. Soft-clamped tangent blend keeps thin lines silky smooth yet razor crisp
    float clamped_luma = clamp(cas_luma, min_cross, max_cross);
    float final_luma = mix(clamped_luma, tangent_luma, edge_gate * 0.18);
    float luma_delta = final_luma - lc;

    return vec4(clamp(c.rgb + vec3(luma_delta), 0.0, 1.0), c.a);
}
"#;

/// Stage 4: Supersampled Anti-Aliasing Downscale Filter when viewport < 2x upscaled dimensions.
const SHADER_AUTODOWNSCALE_PRE_X2: &str = r#"//!DESC Anime4K-v5.2-AutoDownscalePre-x2
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
