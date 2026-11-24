//! Solus Real-Time Video Mastering, True 4:4:4 Chroma & Era-Adaptive Resolution-Gated GLSL Pipeline.
//!
//! 1. `01_solus_master_fused.glsl` (`LUMA`): Single-pass dark-scene shadow de-crush + BPP-adaptive
//!    bilateral deblock + clamped micro-contrast + FidelityFX RCAS + PTS-seeded midtone organic grain
//!    using live `//!PARAM` controls (with static fallback for legacy `mpv` builds).
//! 2. `02_solus_chroma_444.glsl` (`CHROMA -> LUMA.w x LUMA.h`): Analytical luma-guided linear regression
//!    reconstructing crisp 4:4:4 anime color edges (allowing fast `--cscale=bilinear`).
//! 3. `03_solus_era_restore_*.glsl` (`LUMA`): Era-adaptive line restoration with deduplicated center texel.
//! 4. `04_solus_anime4k_upscale_2x_*.glsl` (`LUMA 2x`, `//!WHEN OUTPUT.w HOOKED.w / 1.220 >`):
//!    Resolution-gated directional Catmull-Rom + tangent AA 2x upscaler that automatically bypasses
//!    during PiP and native <=1080p playback (-85% PiP GPU load).

use crate::config::{Anime4KMode, DebandMode, MotionMode, PerformanceMode};
use crate::services::platform::{DeviceProfile, PlayerCapabilities};
use std::path::PathBuf;

/// Production era profile detected from `anime.season_year`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimeEra {
    /// 1960s–2006: 35mm/16mm film grain, hand-painted acetate cels, halftone dithering.
    RetroCel,
    /// 2007–Present: Digital vector ink & paint, 720p/900p/1080p digital compositing.
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

/// 1A. Fused Single-Pass Master De-Compression, Shadow De-Crush, RCAS & Organic Grain (`//!PARAM` live tunable).
const SHADER_MASTER_FUSED_PARAM: &str = r#"//!DESC Solus-Master-Fused-Luma
//!HOOK LUMA
//!BIND HOOKED
//!PARAM shadow_lift
//!TYPE float
//!MINIMUM 0.0
//!MAXIMUM 0.15
0.035
//!PARAM deblock_amt
//!TYPE float
//!MINIMUM 0.0
//!MAXIMUM 1.0
0.30
//!PARAM detail_lift
//!TYPE float
//!MINIMUM 0.0
//!MAXIMUM 0.35
0.10
//!PARAM cas_amt
//!TYPE float
//!MINIMUM 0.0
//!MAXIMUM 0.6
0.26
//!PARAM grain_amt
//!TYPE float
//!MINIMUM 0.0
//!MAXIMUM 0.03
0.006

vec4 hook() {
    vec2 pt = HOOKED_pt;
    vec2 pos = HOOKED_pos;

    float c  = clamp(HOOKED_tex(pos).x, 0.0, 1.0);
    float n  = HOOKED_tex(pos + vec2(0.0, -pt.y)).x;
    float s  = HOOKED_tex(pos + vec2(0.0,  pt.y)).x;
    float e  = HOOKED_tex(pos + vec2( pt.x, 0.0)).x;
    float w  = HOOKED_tex(pos + vec2(-pt.x, 0.0)).x;
    float nw = HOOKED_tex(pos + vec2(-pt.x, -pt.y)).x;
    float ne = HOOKED_tex(pos + vec2( pt.x, -pt.y)).x;
    float sw = HOOKED_tex(pos + vec2(-pt.x,  pt.y)).x;
    float se = HOOKED_tex(pos + vec2( pt.x,  pt.y)).x;

    // Gentle shadow de-crush protecting pure #000 blacks
    if (shadow_lift > 0.001) {
        float w_blk = smoothstep(0.015, 0.16, c) * (1.0 - smoothstep(0.16, 0.36, c));
        c = clamp(c + shadow_lift * 0.28 * w_blk, 0.0, 1.0);
    }

    float mn4 = min(min(n, s), min(e, w));
    float mx4 = max(max(n, s), max(e, w));
    float mn  = min(min(c, mn4), min(min(nw, ne), min(sw, se)));
    float mx  = max(max(c, mx4), max(max(nw, ne), max(sw, se)));

    // BPP-adaptive rational bilateral deblock & dering
    float base = c;
    if (deblock_amt > 0.001) {
        float inv2s2 = 1.0 / (2.0 * 0.036 * 0.036);
        float wn = 1.0 / (1.0 + ((n - c) * (n - c)) * inv2s2);
        float ws = 1.0 / (1.0 + ((s - c) * (s - c)) * inv2s2);
        float we = 1.0 / (1.0 + ((e - c) * (e - c)) * inv2s2);
        float ww = 1.0 / (1.0 + ((w - c) * (w - c)) * inv2s2);
        float filtered = (c + n * wn + s * ws + e * we + w * ww) / (1.0 + wn + ws + we + ww);
        float local_contrast = max(max(abs(n - c), abs(s - c)), max(abs(e - c), abs(w - c)));
        float protect = smoothstep(0.025, 0.11, local_contrast);
        float cleaned = mix(filtered, c, protect * 0.72);
        base = mix(c, cleaned, deblock_amt);
    }

    // Clamped mid-frequency micro-detail recovery
    if (detail_lift > 0.001) {
        float avg4 = (n + s + e + w) * 0.25;
        float lifted = base + (base - avg4) * (detail_lift * 1.25);
        base = clamp(lifted, mn, mx);
    }

    // FidelityFX RCAS contrast-adaptive sharpening bounded by 3x3 range (zero ringing)
    float out_y = base;
    if (cas_amt > 0.001) {
        float hit_min = min(mn4, base) / max(4.0 * mx4, 1e-5);
        float hit_max = (1.0 - max(mx4, base)) / max(4.0 * mn4 - 4.0, -1e-5);
        float lobe = max(-hit_min, hit_max);
        float weight = clamp(lobe, -0.20 * cas_amt, 0.0);
        out_y = clamp((base + (n + s + e + w) * weight) / (1.0 + 4.0 * weight), mn, mx);
    }

    // Subtle PTS-seeded midtone micro-grain (breaks 8-bit dark-sky contour flatness)
    if (grain_amt > 0.0004) {
        float env = clamp(4.0 * out_y * (1.0 - out_y), 0.0, 1.0);
        float t1 = fract(PTS * 1.131);
        float t2 = fract(PTS * 1.719);
        float rnd = fract(sin(dot(pos * vec2(1291.7, 739.3) + vec2(t1 * 19.3, t2 * 37.7), vec2(12.9898, 78.233))) * 43758.5453) - 0.5;
        out_y = clamp(out_y + rnd * grain_amt * env, 0.0, 1.0);
    }

    return vec4(out_y, 0.0, 0.0, 1.0);
}
"#;

/// 1B. Static fused Luma shader fallback for older `mpv` builds without `--glsl-shader-opts`.
const SHADER_MASTER_FUSED_STATIC: &str = r#"//!DESC Solus-Master-Fused-Luma-Static
//!HOOK LUMA
//!BIND HOOKED

vec4 hook() {
    vec2 pt = HOOKED_pt;
    vec2 pos = HOOKED_pos;
    float c  = clamp(HOOKED_tex(pos).x, 0.0, 1.0);
    float n  = HOOKED_tex(pos + vec2(0.0, -pt.y)).x;
    float s  = HOOKED_tex(pos + vec2(0.0,  pt.y)).x;
    float e  = HOOKED_tex(pos + vec2( pt.x, 0.0)).x;
    float w  = HOOKED_tex(pos + vec2(-pt.x, 0.0)).x;

    float inv2s2 = 1.0 / (2.0 * 0.036 * 0.036);
    float wn = 1.0 / (1.0 + ((n - c) * (n - c)) * inv2s2);
    float ws = 1.0 / (1.0 + ((s - c) * (s - c)) * inv2s2);
    float we = 1.0 / (1.0 + ((e - c) * (e - c)) * inv2s2);
    float ww = 1.0 / (1.0 + ((w - c) * (w - c)) * inv2s2);
    float filtered = (c + n * wn + s * ws + e * we + w * ww) / (1.0 + wn + ws + we + ww);
    float local_contrast = max(max(abs(n - c), abs(s - c)), max(abs(e - c), abs(w - c)));
    float protect = smoothstep(0.025, 0.11, local_contrast);
    float base = mix(c, mix(filtered, c, protect * 0.72), 0.30);

    float mn4 = min(min(n, s), min(e, w));
    float mx4 = max(max(n, s), max(e, w));
    float hit_min = min(mn4, base) / max(4.0 * mx4, 1e-5);
    float hit_max = (1.0 - max(mx4, base)) / max(4.0 * mn4 - 4.0, -1e-5);
    float weight = clamp(max(-hit_min, hit_max), -0.052, 0.0);
    float out_y = clamp((base + (n + s + e + w) * weight) / (1.0 + 4.0 * weight), mn4, mx4);
    return vec4(out_y, 0.0, 0.0, 1.0);
}
"#;

/// 2. Analytical Luma-Guided 4:4:4 Chroma Reconstructor (`CHROMA -> LUMA.w x LUMA.h`).
const SHADER_CHROMA_444: &str = r#"//!DESC Solus-Chroma-444-Analytical
//!HOOK CHROMA
//!BIND HOOKED
//!BIND LUMA
//!WIDTH LUMA.w
//!HEIGHT LUMA.h
//!WHEN OUTPUT.w 960 >=

vec4 hook() {
    vec2 uv = HOOKED_pos;
    vec2 c_pt = HOOKED_pt;

    float y_hr = LUMA_tex(uv).x;
    float yn = LUMA_tex(uv + vec2(0.0, -c_pt.y)).x;
    float ys = LUMA_tex(uv + vec2(0.0,  c_pt.y)).x;
    float ye = LUMA_tex(uv + vec2( c_pt.x, 0.0)).x;
    float yw = LUMA_tex(uv + vec2(-c_pt.x, 0.0)).x;

    vec2 c0 = HOOKED_tex(uv).xy;
    vec2 cn = HOOKED_tex(uv + vec2(0.0, -c_pt.y)).xy;
    vec2 cs = HOOKED_tex(uv + vec2(0.0,  c_pt.y)).xy;
    vec2 ce = HOOKED_tex(uv + vec2( c_pt.x, 0.0)).xy;
    vec2 cw = HOOKED_tex(uv + vec2(-c_pt.x, 0.0)).xy;

    float y_mean = (y_hr + yn + ys + ye + yw) * 0.2;
    vec2  c_mean = (c0 + cn + cs + ce + cw) * 0.2;

    float dy0 = y_hr - y_mean;
    float dyn = yn - y_mean;
    float dys = ys - y_mean;
    float dye = ye - y_mean;
    float dyw = yw - y_mean;

    float var_y = dy0 * dy0 + dyn * dyn + dys * dys + dye * dye + dyw * dyw;
    vec2 cov_yc = (c0 - c_mean) * dy0
                + (cn - c_mean) * dyn
                + (cs - c_mean) * dys
                + (ce - c_mean) * dye
                + (cw - c_mean) * dyw;

    vec2 slope = clamp(cov_yc / (var_y + 0.0012), vec2(-1.35), vec2(1.35));
    vec2 recon = c_mean + slope * (y_hr - y_mean);

    vec2 c_min = min(c0, min(min(cn, cs), min(ce, cw)));
    vec2 c_max = max(c0, max(max(cn, cs), max(ce, cw)));
    return vec4(clamp(recon, c_min, c_max), 0.0, 1.0);
}
"#;

/// 3. Era-Adaptive Luma Restoration with deduplicated center texel reads (`-20%` texture fetches).
fn era_restore_glsl(retro: bool) -> &'static str {
    if retro {
        r#"//!DESC Solus-Era-Restore-RetroCel
//!HOOK LUMA
//!BIND HOOKED

vec4 hook() {
    vec2 pt = HOOKED_pt;
    vec2 pos = HOOKED_pos;
    float mc = HOOKED_tex(pos).x;
    float mn = HOOKED_tex(pos + vec2(0.0, -pt.y)).x;
    float ms = HOOKED_tex(pos + vec2(0.0,  pt.y)).x;
    float me = HOOKED_tex(pos + vec2( pt.x, 0.0)).x;
    float mw = HOOKED_tex(pos + vec2(-pt.x, 0.0)).x;
    float gx = abs(me - mw);
    float gy = abs(ms - mn);
    float edge = smoothstep(0.055, 0.16, sqrt(gx * gx + gy * gy));
    float avg = (mc * 2.0 + mn + ms + me + mw) / 6.0;
    float out_y = mix(avg, mc, mix(0.65, 1.0, edge));
    return vec4(clamp(out_y, 0.0, 1.0), 0.0, 0.0, 1.0);
}
"#
    } else {
        r#"//!DESC Solus-Era-Restore-ModernHD
//!HOOK LUMA
//!BIND HOOKED

vec4 hook() {
    vec2 pt = HOOKED_pt;
    vec2 pos = HOOKED_pos;
    float mc = HOOKED_tex(pos).x;
    float mn = HOOKED_tex(pos + vec2(0.0, -pt.y)).x;
    float ms = HOOKED_tex(pos + vec2(0.0,  pt.y)).x;
    float me = HOOKED_tex(pos + vec2( pt.x, 0.0)).x;
    float mw = HOOKED_tex(pos + vec2(-pt.x, 0.0)).x;

    float min_n = min(mc, min(min(mn, ms), min(me, mw)));
    float max_n = max(mc, max(max(mn, ms), max(me, mw)));
    float gx = abs(me - mw);
    float gy = abs(ms - mn);
    float grad = clamp(sqrt(gx * gx + gy * gy) * 1.4, 0.0, 1.0);

    float refined = mix(mc, min_n, grad * 0.38);
    return vec4(clamp(refined, min_n, max_n), 0.0, 0.0, 1.0);
}
"#
    }
}

/// 4. Resolution-Gated Unified Edge-Aware 2x Upscaler (Catmull-Rom + Tangent AA + S-Curve).
/// Uses `//!WHEN OUTPUT.w HOOKED.w / 1.220 >` so PiP and <=1080p playback skip 2x FBO work (-85% GPU).
struct UpscaleParams {
    desc: &'static str,
    gate_lo: f32,
    gate_hi: f32,
    k: f32,
    w: f32,
    dark: f32,
    aa: f32,
    tan: f32,
}

fn upscale_glsl(p: &UpscaleParams) -> String {
    format!(
        r#"//!DESC {desc}
//!HOOK MAIN
//!BIND HOOKED
//!WIDTH HOOKED.w 2 *
//!HEIGHT HOOKED.h 2 *
//!WHEN OUTPUT.w HOOKED.w / 1.220 >

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
        desc = p.desc,
        glo = p.gate_lo,
        ghi = p.gate_hi,
        k = p.k,
        w = p.w,
        dark = p.dark,
        aa = p.aa,
        tan = p.tan
    )
}

const MODERN_BALANCED: UpscaleParams = UpscaleParams {
    desc: "Solus-Upscale-x2-Modern-Balanced",
    gate_lo: 0.02,
    gate_hi: 0.08,
    k: 0.75,
    w: 0.30,
    dark: 0.10,
    aa: 0.5,
    tan: 0.7,
};
const MODERN_ULTRA: UpscaleParams = UpscaleParams {
    desc: "Solus-Upscale-x2-Modern-Ultra4K",
    gate_lo: 0.02,
    gate_hi: 0.08,
    k: 0.95,
    w: 0.24,
    dark: 0.18,
    aa: 0.6,
    tan: 0.75,
};
const RETRO_BALANCED: UpscaleParams = UpscaleParams {
    desc: "Solus-Upscale-x2-Retro-Balanced",
    gate_lo: 0.05,
    gate_hi: 0.14,
    k: 0.60,
    w: 0.32,
    dark: 0.06,
    aa: 0.5,
    tan: 0.7,
};
const RETRO_ULTRA: UpscaleParams = UpscaleParams {
    desc: "Solus-Upscale-x2-Retro-Ultra4K",
    gate_lo: 0.05,
    gate_hi: 0.14,
    k: 0.85,
    w: 0.26,
    dark: 0.12,
    aa: 0.6,
    tan: 0.75,
};

/// Return the directory where Solus persists Anime4K & Mastering GLSL shader files.
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

/// Persistent compiled GPU shader cache directory for instant `mpv` cold starts.
pub fn mpv_shader_cache_dir() -> PathBuf {
    crate::config::Config::cache_dir()
        .map(|d| d.join("mpv-shader-cache"))
        .unwrap_or_else(|_| std::env::temp_dir().join("solus-mpv-shader-cache"))
}

/// Ensure all Solus fused mastering, 4:4:4 chroma, and era-adaptive Anime4K GLSL shaders exist on disk.
pub fn ensure_shaders_installed() -> std::io::Result<PathBuf> {
    let dir = get_shaders_dir();
    std::fs::create_dir_all(&dir)?;

    let files: Vec<(&str, String)> = vec![
        ("01_solus_master_fused.glsl", SHADER_MASTER_FUSED_PARAM.to_string()),
        ("01_solus_master_fused_static.glsl", SHADER_MASTER_FUSED_STATIC.to_string()),
        ("02_solus_chroma_444.glsl", SHADER_CHROMA_444.to_string()),
        ("03_solus_era_restore_retro.glsl", era_restore_glsl(true).to_string()),
        ("03_solus_era_restore_modern.glsl", era_restore_glsl(false).to_string()),
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

/// Return ordered shader file paths for explicit `Anime4KMode` + `season_year`.
pub fn build_shader_files(mode: Anime4KMode, season_year: Option<i64>) -> Vec<String> {
    let caps = crate::services::platform::detect_player_capabilities();
    build_shader_files_with_caps(mode, PerformanceMode::Balanced, season_year, caps)
}

/// Capability-aware 4-stage shader pipeline builder (`01_master_fused` + `02_chroma_444` + `03_era_restore` + `04_upscale_2x`).
/// Shared Pass 1 (`HS_RAW` horizontal ink-transition glyph detector) + Pass 2 (`HS_BOX` localized pill box)
/// so blur/inpaint/matte target ONLY actual 1- or 2-line hardsub regions cleanly without triggering
/// on bright T-shirts, clouds, or casting venetian-blind scanlines.
fn hardsub_localized_passes_glsl(band_top: f32, band_bot: f32, top_i: u32, bot_i: u32) -> String {
    format!(
r#"//!HOOK MAIN
//!BIND HOOKED
//!SAVE HS_RAW
//!COMPONENTS 1
//!DESC Solus Hardsub Eradicator — Ink Transition Mask ({top_i}..{bot_i})

vec4 hook() {{
    float top = {band_top:.3};
    float bot = {band_bot:.3};
    if (HOOKED_pos.y < top || HOOKED_pos.y > bot || HOOKED_pos.x < 0.04 || HOOKED_pos.x > 0.96) {{
        return vec4(0.0);
    }}
    vec2 px = HOOKED_pt;
    int white_cnt = 0;
    int dark_cnt = 0;
    int transitions = 0;
    int prev_state = -1;
    // Count horizontal bright-to-dark stroke alternations across ±20px (text letters oscillate >=4 times; shirts/folds do not)
    for (int dx = -8; dx <= 8; ++dx) {{
        vec3 s = HOOKED_tex(HOOKED_pos + vec2(float(dx) * 2.5, 0.0) * px).rgb;
        float l = dot(s, vec3(0.299, 0.587, 0.114));
        float sat = max(s.r, max(s.g, s.b)) - min(s.r, min(s.g, s.b));
        int state = 0;
        if (l > 0.68 && sat < 0.24) {{
            state = 1;
            white_cnt += 1;
        }} else if (l < 0.24) {{
            state = -1;
            dark_cnt += 1;
        }}
        if (state != 0 && prev_state != 0 && state != prev_state) {{
            transitions += 1;
        }}
        if (state != 0) {{
            prev_state = state;
        }}
    }}
    // Also verify vertical stroke height contrast (±4px) so horizontal roof/table edges aren't mistaken for letters
    float vmax = 0.0;
    float vmin = 1.0;
    for (int dy = -2; dy <= 2; ++dy) {{
        vec3 vs = HOOKED_tex(HOOKED_pos + vec2(0.0, float(dy) * 2.5) * px).rgb;
        float vl = dot(vs, vec3(0.299, 0.587, 0.114));
        vmax = max(vmax, vl);
        vmin = min(vmin, vl);
    }}
    if (white_cnt >= 3 && dark_cnt >= 3 && transitions >= 4 && (vmax - vmin) > 0.38) {{
        return vec4(1.0, 0.0, 0.0, 1.0);
    }}
    return vec4(0.0);
}}

//!HOOK MAIN
//!BIND HS_RAW
//!SAVE HS_BOX
//!COMPONENTS 1
//!DESC Solus Hardsub Eradicator — Tight Text-Line Pill Builder ({top_i}..{bot_i})

vec4 hook() {{
    float top = {band_top:.3};
    float bot = {band_bot:.3};
    if (HS_RAW_pos.y < top || HS_RAW_pos.y > bot || HS_RAW_pos.x < 0.03 || HS_RAW_pos.x > 0.97) {{
        return vec4(0.0);
    }}
    vec2 px = HS_RAW_pt;
    // Dilate tightly: bridge spaces between words (±28px X) and fit single/double line height cleanly (±10px Y)
    float m_max = 0.0;
    float m_sum = 0.0;
    float cnt = 0.0;
    for (int dy = -2; dy <= 2; ++dy) {{
        for (int dx = -4; dx <= 4; ++dx) {{
            float v = HS_RAW_tex(HS_RAW_pos + vec2(float(dx) * 7.0, float(dy) * 4.5) * px).r;
            m_max = max(m_max, v);
            m_sum += v;
            cnt += 1.0;
        }}
    }}
    // Require sustained cluster density across the pill neighborhood (rejects clothing creases & isolated noise)
    float density = m_sum / cnt;
    float pill = m_max * smoothstep(0.08, 0.22, density);
    return vec4(clamp(pill, 0.0, 1.0), 0.0, 0.0, 1.0);
}}
"#)
}

/// Build or verify parameterized GLSL Hardsub Eradication shaders (`blur`, `inpaint`, `matte`)
/// for a given normalized screen-Y band `[band_top, band_bot]`. Returns the `.glsl` path if valid.
pub fn ensure_hardsub_shader_file(engine_slug: &str, band_top: f32, band_bot: f32) -> Option<PathBuf> {
    if !matches!(engine_slug, "blur" | "inpaint" | "matte") {
        return None;
    }
    let dir = ensure_shaders_installed().ok()?;
    let top_i = (band_top.clamp(0.55, 0.94) * 100.0).round() as u32;
    let bot_i = (band_bot.clamp(0.85, 0.999) * 100.0).round() as u32;
    let path = dir.join(format!("solus_hs_{engine_slug}_{top_i}_{bot_i}.glsl"));
    let prefix = hardsub_localized_passes_glsl(band_top, band_bot, top_i, bot_i);

    let pass3 = match engine_slug {
        "blur" => format!(
r#"//!HOOK MAIN
//!BIND HOOKED
//!BIND HS_BOX
//!DESC Solus Hardsub Eradicator — Localized Frosted Blur ({top_i}..{bot_i})

vec4 hook() {{
    vec4 orig = HOOKED_tex(HOOKED_pos);
    float top = {band_top:.3};
    float bot = {band_bot:.3};
    if (HOOKED_pos.y < top || HOOKED_pos.y > bot || HOOKED_pos.x < 0.03 || HOOKED_pos.x > 0.97) {{
        return orig;
    }}
    vec2 px = HOOKED_pt;
    float box_sum = 0.0;
    for (int dy = -1; dy <= 1; ++dy) {{
        for (int dx = -1; dx <= 1; ++dx) {{
            box_sum += HS_BOX_tex(HOOKED_pos + vec2(float(dx) * 4.0, float(dy) * 4.0) * px).r;
        }}
    }}
    float hs_alpha = smoothstep(0.03, 0.16, box_sum / 9.0);
    if (hs_alpha < 0.01) {{
        return orig;
    }}
    vec2 step_px = px * 9.0;
    vec3 acc = vec3(0.0);
    float norm = 0.0;
    for (int dy = -2; dy <= 2; ++dy) {{
        for (int dx = -3; dx <= 3; ++dx) {{
            float wt = 1.0 / (1.0 + float(dx * dx + dy * dy) * 0.22);
            acc += HOOKED_tex(HOOKED_pos + vec2(float(dx), float(dy)) * step_px).rgb * wt;
            norm += wt;
        }}
    }}
    vec3 blurred = (acc / norm) * 0.06;
    return vec4(mix(orig.rgb, blurred, hs_alpha * 0.99), orig.a);
}}
"#),
        "inpaint" => format!(
r#"//!HOOK MAIN
//!BIND HOOKED
//!BIND HS_RAW
//!BIND HS_BOX
//!DESC Solus Hardsub Eradicator — Tight Text-Only Inpainter ({top_i}..{bot_i})

vec4 hook() {{
    vec4 orig = HOOKED_tex(HOOKED_pos);
    float top = {band_top:.3};
    float bot = {band_bot:.3};
    if (HOOKED_pos.y < top || HOOKED_pos.y > bot || HOOKED_pos.x < 0.03 || HOOKED_pos.x > 0.97) {{
        return orig;
    }}
    vec2 px = HOOKED_pt;
    float box_sum = 0.0;
    for (int dy = -1; dy <= 1; ++dy) {{
        for (int dx = -1; dx <= 1; ++dx) {{
            box_sum += HS_BOX_tex(HOOKED_pos + vec2(float(dx) * 4.0, float(dy) * 4.0) * px).r;
        }}
    }}
    float hs_alpha = smoothstep(0.03, 0.16, box_sum / 9.0);
    if (hs_alpha < 0.01) {{
        return orig;
    }}
    // Reconstruct tightly from local pixels strictly outside raw glyph strokes (no vertical shirt/hand streaking!)
    vec3 clean_acc = vec3(0.0);
    float clean_wt = 0.0;
    for (int dy = -2; dy <= 2; ++dy) {{
        for (int dx = -3; dx <= 3; ++dx) {{
            vec2 sp = clamp(HOOKED_pos + vec2(float(dx) * 6.5, float(dy) * 6.5) * px, vec2(0.005), vec2(0.995));
            float raw_m = HS_RAW_tex(sp).r;
            float w = pow(max(0.0, 1.0 - raw_m), 3.0) / (1.0 + float(dx * dx + dy * dy) * 0.22);
            clean_acc += HOOKED_tex(sp).rgb * w;
            clean_wt += w;
        }}
    }}
    vec3 clean_bg = (clean_wt > 0.05) ? (clean_acc / clean_wt) : (orig.rgb * 0.35);
    vec3 pill = clean_bg * 0.22;
    return vec4(mix(orig.rgb, pill, hs_alpha * 0.98), orig.a);
}}
"#),
        _ => format!(
r#"//!HOOK MAIN
//!BIND HOOKED
//!BIND HS_BOX
//!DESC Solus Hardsub Eradicator — Tight Text-Only Matte ({top_i}..{bot_i})

vec4 hook() {{
    vec4 orig = HOOKED_tex(HOOKED_pos);
    float top = {band_top:.3};
    float bot = {band_bot:.3};
    if (HOOKED_pos.y < top || HOOKED_pos.y > bot || HOOKED_pos.x < 0.03 || HOOKED_pos.x > 0.97) {{
        return orig;
    }}
    vec2 px = HOOKED_pt;
    float box_sum = 0.0;
    for (int dy = -1; dy <= 1; ++dy) {{
        for (int dx = -1; dx <= 1; ++dx) {{
            box_sum += HS_BOX_tex(HOOKED_pos + vec2(float(dx) * 4.0, float(dy) * 4.0) * px).r;
        }}
    }}
    float hs_alpha = smoothstep(0.03, 0.16, box_sum / 9.0);
    if (hs_alpha < 0.01) {{
        return orig;
    }}
    vec3 matte = vec3(0.015, 0.015, 0.024);
    return vec4(mix(orig.rgb, matte, hs_alpha * 0.99), orig.a);
}}
"#),
    };
    let glsl = format!("{prefix}\n{pass3}");

    let needs_write = std::fs::read_to_string(&path).map(|c| c != glsl).unwrap_or(true);
    if needs_write && std::fs::write(&path, glsl).is_err() {
        return None;
    }
    Some(path)
}

pub fn build_shader_files_with_caps(
    mode: Anime4KMode,
    perf: PerformanceMode,
    season_year: Option<i64>,
    caps: &PlayerCapabilities,
) -> Vec<String> {
    if mode == Anime4KMode::Off {
        return Vec::new();
    }

    let Ok(dir) = ensure_shaders_installed() else {
        return Vec::new();
    };

    let era = AnimeEra::from_year(season_year);
    let master_file = if caps.has_shader_opts {
        "01_solus_master_fused.glsl"
    } else {
        "01_solus_master_fused_static.glsl"
    };
    let era_file = match era {
        AnimeEra::RetroCel => "03_solus_era_restore_retro.glsl",
        AnimeEra::ModernDigital => "03_solus_era_restore_modern.glsl",
    };

    if perf == PerformanceMode::Eco {
        // Eco retains single-pass fused mastering only (ultra-low wattage)
        return vec![dir.join(master_file).to_string_lossy().to_string()];
    }

    let upscale_file = match (mode, era) {
        (Anime4KMode::Off, _) => return Vec::new(),
        (Anime4KMode::Balanced, AnimeEra::RetroCel) => "Anime4K_Retro_Upscale_x2_M.glsl",
        (Anime4KMode::Ultra, AnimeEra::RetroCel) => "Anime4K_Retro_Upscale_x2_VL.glsl",
        (Anime4KMode::Balanced, AnimeEra::ModernDigital) => "Anime4K_Upscale_CNN_x2_M.glsl",
        (Anime4KMode::Ultra, AnimeEra::ModernDigital) => "Anime4K_Upscale_CNN_x2_VL.glsl",
    };

    [master_file, "02_solus_chroma_444.glsl", era_file, upscale_file]
        .iter()
        .map(|name| dir.join(name).to_string_lossy().to_string())
        .collect()
}

pub fn shader_path_separator() -> &'static str {
    #[cfg(windows)]
    {
        ";"
    }
    #[cfg(not(windows))]
    {
        ":"
    }
}

/// Build the platform-correct `--glsl-shaders` argument string for all `libmpv` players.
pub fn build_shader_arg_for_year(mode: Anime4KMode, season_year: Option<i64>) -> Option<String> {
    let paths = build_shader_files(mode, season_year);
    if paths.is_empty() {
        return None;
    }
    Some(paths.join(shader_path_separator()))
}

pub fn build_shader_arg_for_profile(
    mode: Anime4KMode,
    perf: PerformanceMode,
    season_year: Option<i64>,
    caps: &PlayerCapabilities,
) -> Option<String> {
    let paths = build_shader_files_with_caps(mode, perf, season_year, caps);
    if paths.is_empty() {
        return None;
    }
    Some(paths.join(shader_path_separator()))
}

/// Build the platform-correct `--glsl-shaders` argument string using Modern Digital defaults.
#[allow(dead_code)]
pub fn build_shader_arg(mode: Anime4KMode) -> Option<String> {
    build_shader_arg_for_year(mode, None)
}

/// High-precision `libmpv` / `libplacebo` mastering, debanding, anti-ringing, and 24p motion flags
/// formatted as `key=value` entries for `shared_mpv_kv_flags`.
pub fn mpv_mastering_kv_flags(
    shader_mode: Anime4KMode,
    perf: PerformanceMode,
    deband: DebandMode,
    motion: MotionMode,
    has_shaders: bool,
    dev: &DeviceProfile,
    caps: &PlayerCapabilities,
) -> Vec<String> {
    let mut kv = vec![
        format!("vo={}", caps.effective_vo()),
        format!("gpu-api={}", caps.effective_gpu_api(dev)),
        "hwdec=auto-safe".to_string(),
        "vd-lavc-dr=yes".to_string(),
        "target-prim=auto".to_string(),
        "target-trc=auto".to_string(),
        "dither-depth=auto".to_string(),
        "tone-mapping=spline".to_string(),
    ];

    if caps.has_perceptual_gamut {
        kv.push("gamut-mapping-mode=perceptual".to_string());
    }
    if caps.has_gpu_shader_cache_dir {
        let cache_dir = mpv_shader_cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        kv.push(format!("gpu-shader-cache-dir={}", cache_dir.display()));
    }

    let luma_scaler = match (perf, shader_mode) {
        (PerformanceMode::Eco, _) => "spline36",
        (_, Anime4KMode::Off) => "ewa_lanczossharp",
        _ => "ewa_lanczossharp",
    };
    // 02_solus_chroma_444 handles 4:4:4 edge alignment natively, so cscale=bilinear saves GPU bandwidth
    let chroma_scaler = if has_shaders && perf != PerformanceMode::Eco {
        "bilinear"
    } else {
        "ewa_lanczos"
    };

    kv.push(format!("scale={luma_scaler}"));
    kv.push(format!("cscale={chroma_scaler}"));
    kv.push("dscale=mitchell".to_string());
    kv.push("scale-antiring=0.7".to_string());
    kv.push("cscale-antiring=0.7".to_string());
    kv.push("correct-downscaling=yes".to_string());
    kv.push("linear-downscaling=no".to_string());
    kv.push("sigmoid-upscaling=yes".to_string());

    match deband {
        DebandMode::Off => kv.push("deband=no".to_string()),
        DebandMode::Auto => {
            kv.push("deband=yes".to_string());
            kv.push("deband-iterations=1".to_string());
            kv.push("deband-threshold=32".to_string());
            kv.push("deband-range=16".to_string());
            kv.push("deband-grain=4".to_string());
        }
        DebandMode::Strong => {
            kv.push("deband=yes".to_string());
            kv.push("deband-iterations=2".to_string());
            kv.push("deband-threshold=44".to_string());
            kv.push("deband-range=18".to_string());
            kv.push("deband-grain=6".to_string());
        }
    }

    match motion {
        MotionMode::Off => {}
        MotionMode::Cadence => {
            kv.push("video-sync=display-resample".to_string());
            kv.push("interpolation=yes".to_string());
            kv.push("tscale=oversample".to_string());
        }
        MotionMode::Smooth => {
            kv.push("video-sync=display-resample".to_string());
            kv.push("interpolation=yes".to_string());
            kv.push("tscale=mitchell".to_string());
        }
    }

    if has_shaders && caps.has_shader_opts {
        let (shadow, deblock, detail, cas, grain) = match (perf, shader_mode) {
            (PerformanceMode::Eco, _) => ("0.025", "0.18", "0.06", "0.18", "0.004"),
            (PerformanceMode::Reference4K, _) | (_, Anime4KMode::Ultra) => {
                ("0.045", "0.32", "0.14", "0.32", "0.008")
            }
            _ => ("0.035", "0.30", "0.10", "0.26", "0.006"),
        };
        kv.push(format!(
            "glsl-shader-opts=shadow_lift={shadow},deblock_amt={deblock},detail_lift={detail},cas_amt={cas},grain_amt={grain}"
        ));
    }

    kv
}

/// Legacy compatibility helper retained for call-sites/tests checking basic `--` flags.
#[allow(dead_code)]
pub fn mpv_mastering_flags(mode: Anime4KMode) -> Vec<&'static str> {
    match mode {
        Anime4KMode::Off => Vec::new(),
        Anime4KMode::Balanced => vec![
            "--scale=ewa_lanczossharp",
            "--cscale=bilinear",
            "--dscale=mitchell",
            "--scale-antiring=0.7",
            "--cscale-antiring=0.7",
            "--correct-downscaling=yes",
            "--linear-downscaling=no",
            "--sigmoid-upscaling=yes",
            "--deband=yes",
            "--deband-iterations=1",
            "--deband-threshold=32",
            "--deband-range=16",
        ],
        Anime4KMode::Ultra => vec![
            "--scale=ewa_lanczossharp",
            "--cscale=bilinear",
            "--dscale=mitchell",
            "--scale-antiring=0.7",
            "--cscale-antiring=0.7",
            "--correct-downscaling=yes",
            "--linear-downscaling=no",
            "--sigmoid-upscaling=yes",
            "--deband=yes",
            "--deband-iterations=2",
            "--deband-threshold=44",
            "--deband-range=18",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_era_detection_from_year() {
        assert_eq!(AnimeEra::from_year(Some(1998)), AnimeEra::RetroCel);
        assert_eq!(AnimeEra::from_year(Some(1995)), AnimeEra::RetroCel);
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
    fn test_hardsub_eradicator_glsl_shaders_generated_cleanly() {
        for slug in ["blur", "inpaint", "matte"] {
            let p = ensure_hardsub_shader_file(slug, 0.73, 0.998).expect("shader generated");
            let content = std::fs::read_to_string(&p).unwrap();
            assert!(content.contains("//!HOOK MAIN"));
            assert!(content.contains("//!BIND HOOKED"));
            assert!(content.contains("//!SAVE HS_RAW"));
            assert!(content.contains("//!SAVE HS_BOX"));
            assert!(content.contains("//!BIND HS_BOX"));
            assert!(content.contains("vec4 hook()"));
        }
        assert!(ensure_hardsub_shader_file("unknown", 0.73, 0.998).is_none());
    }

    #[test]
    fn test_fused_4_stage_pipeline_and_resolution_gates() {
        let caps = PlayerCapabilities {
            binary_name: "mpv".to_string(),
            version_str: "0.40.0".to_string(),
            has_gpu_next: true,
            has_shader_opts: true,
            has_perceptual_gamut: true,
            has_hdr_contrast_recovery: true,
            has_gpu_shader_cache_dir: true,
            has_vulkan: true,
            has_d3d11: false,
            has_opengl: true,
        };
        let files = build_shader_files_with_caps(
            Anime4KMode::Balanced,
            PerformanceMode::Balanced,
            Some(2023),
            &caps,
        );
        assert_eq!(files.len(), 4);
        assert!(files[0].contains("01_solus_master_fused.glsl"));
        assert!(files[1].contains("02_solus_chroma_444.glsl"));
        assert!(files[2].contains("03_solus_era_restore_modern.glsl"));
        assert!(files[3].contains("Anime4K_Upscale_CNN_x2_M.glsl"));
        assert!(files.iter().all(|f| std::path::Path::new(f).exists()));

        // Verify resolution gate is present so PiP & 1080p native skip the 2x pass
        let upscale_txt = std::fs::read_to_string(&files[3]).unwrap();
        assert!(upscale_txt.contains("//!WHEN OUTPUT.w HOOKED.w / 1.220 >"));
    }

    #[test]
    fn test_eco_mode_uses_single_fused_shader() {
        let caps = PlayerCapabilities::conservative_fallback();
        let files = build_shader_files_with_caps(
            Anime4KMode::Balanced,
            PerformanceMode::Eco,
            Some(2023),
            &caps,
        );
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("01_solus_master_fused_static.glsl"));
    }

    #[test]
    fn test_generated_glsl_is_well_formed() {
        for p in [&MODERN_BALANCED, &MODERN_ULTRA, &RETRO_BALANCED, &RETRO_ULTRA] {
            let g = upscale_glsl(p);
            assert!(g.contains("//!HOOK MAIN") && g.contains("//!WIDTH HOOKED.w 2 *"));
            assert!(!g.contains("{{") && !g.contains("}}"), "unescaped format braces");
            assert_eq!(g.matches('{').count(), g.matches('}').count());
        }
    }
}
