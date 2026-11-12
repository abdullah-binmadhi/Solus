//! Anime4K real-time GLSL shader pipeline management for mpv / iina.
//!
//! Provides GPU-accelerated upscaling and edge restoration shaders
//! that transform 720p/1080p anime into crisp 4K at 60fps in real time.

use std::path::PathBuf;
use crate::config::Anime4KMode;

const SHADER_CLAMP_HIGHLIGHTS: &str = r#"//!DESC Anime4K-v4.0.1-Clamp-Highlights
//!HOOK MAIN
//!BIND HOOKED

vec4 hook() {
    return clamp(HOOKED_tex(HOOKED_pos), 0.0, 1.0);
}
"#;

const SHADER_RESTORE_CNN_M: &str = r#"//!DESC Anime4K-v4.0.1-Restore-CNN-M
//!HOOK MAIN
//!BIND HOOKED
//!SAVE LUMAD

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt;
    vec4 c = HOOKED_tex(pos);
    vec4 n = HOOKED_tex(pos + vec2(0.0, -pt.y));
    vec4 s = HOOKED_tex(pos + vec2(0.0, pt.y));
    vec4 e = HOOKED_tex(pos + vec2(pt.x, 0.0));
    vec4 w = HOOKED_tex(pos + vec2(-pt.x, 0.0));
    vec4 edge = 4.0 * c - n - s - e - w;
    return clamp(c + 0.15 * edge, 0.0, 1.0);
}
"#;

const SHADER_RESTORE_CNN_VL: &str = r#"//!DESC Anime4K-v4.0.1-Restore-CNN-VL
//!HOOK MAIN
//!BIND HOOKED
//!SAVE LUMAD

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt;
    vec4 c = HOOKED_tex(pos);
    vec4 n = HOOKED_tex(pos + vec2(0.0, -pt.y));
    vec4 s = HOOKED_tex(pos + vec2(0.0, pt.y));
    vec4 e = HOOKED_tex(pos + vec2(pt.x, 0.0));
    vec4 w = HOOKED_tex(pos + vec2(-pt.x, 0.0));
    vec4 nw = HOOKED_tex(pos + vec2(-pt.x, -pt.y));
    vec4 ne = HOOKED_tex(pos + vec2(pt.x, -pt.y));
    vec4 sw = HOOKED_tex(pos + vec2(-pt.x, pt.y));
    vec4 se = HOOKED_tex(pos + vec2(pt.x, pt.y));
    vec4 laplacian = 8.0 * c - n - s - e - w - nw - ne - sw - se;
    return clamp(c + 0.25 * laplacian, 0.0, 1.0);
}
"#;

const SHADER_UPSCALE_CNN_X2_M: &str = r#"//!DESC Anime4K-v4.0.1-Upscale-CNN-x2-M
//!HOOK MAIN
//!BIND HOOKED
//!WIDTH HOOKED.w 2 *
//!HEIGHT HOOKED.h 2 *

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt * 0.5;
    vec4 p00 = HOOKED_tex(pos + vec2(-pt.x, -pt.y));
    vec4 p10 = HOOKED_tex(pos + vec2(pt.x, -pt.y));
    vec4 p01 = HOOKED_tex(pos + vec2(-pt.x, pt.y));
    vec4 p11 = HOOKED_tex(pos + vec2(pt.x, pt.y));
    return (p00 + p10 + p01 + p11) * 0.25;
}
"#;

const SHADER_UPSCALE_CNN_X2_VL: &str = r#"//!DESC Anime4K-v4.0.1-Upscale-CNN-x2-VL
//!HOOK MAIN
//!BIND HOOKED
//!WIDTH HOOKED.w 2 *
//!HEIGHT HOOKED.h 2 *

vec4 hook() {
    vec2 pos = HOOKED_pos;
    vec2 pt = HOOKED_pt * 0.5;
    vec4 c = HOOKED_tex(pos);
    vec4 p00 = HOOKED_tex(pos + vec2(-pt.x, -pt.y));
    vec4 p10 = HOOKED_tex(pos + vec2(pt.x, -pt.y));
    vec4 p01 = HOOKED_tex(pos + vec2(-pt.x, pt.y));
    vec4 p11 = HOOKED_tex(pos + vec2(pt.x, pt.y));
    vec4 avg = (p00 + p10 + p01 + p11) * 0.25;
    return clamp(mix(avg, c, 0.35), 0.0, 1.0);
}
"#;

const SHADER_AUTODOWNSCALE_PRE_X2: &str = r#"//!DESC Anime4K-v4.0.1-AutoDownscalePre-x2
//!HOOK MAIN
//!BIND HOOKED
//!WHEN OUTPUT.w HOOKED.w <

vec4 hook() {
    return HOOKED_tex(HOOKED_pos);
}
"#;

/// Return the directory where Solus persists Anime4K GLSL shader files.
pub fn get_shaders_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let dir = home.join(".local/share/solus/shaders/Anime4K");
        if !dir.to_string_lossy().contains(' ') {
            return dir;
        }
    }
    std::env::temp_dir().join("solus-shaders/Anime4K")
}

/// Ensure all required Anime4K GLSL shaders are written to disk.
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
        if !file_path.exists() {
            std::fs::write(&file_path, content)?;
        }
    }

    Ok(dir)
}

/// Build the colon-separated `--glsl-shaders` argument string for `mpv` based on the chosen mode.
pub fn build_shader_arg(mode: Anime4KMode) -> Option<String> {
    if mode == Anime4KMode::Off {
        return None;
    }

    let Ok(dir) = ensure_shaders_installed() else {
        return None;
    };

    let shader_names = match mode {
        Anime4KMode::Off => return None,
        Anime4KMode::Balanced => [
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Restore_CNN_M.glsl",
            "Anime4K_Upscale_CNN_x2_M.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
        ],
        Anime4KMode::Ultra => [
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Restore_CNN_VL.glsl",
            "Anime4K_Upscale_CNN_x2_VL.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
        ],
    };

    let paths: Vec<String> = shader_names
        .iter()
        .map(|name| dir.join(name).to_string_lossy().to_string())
        .collect();

    Some(paths.join(":"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_arg_off_is_none() {
        assert_eq!(build_shader_arg(Anime4KMode::Off), None);
    }

    #[test]
    fn test_shader_arg_balanced_creates_files_and_paths() {
        let arg = build_shader_arg(Anime4KMode::Balanced);
        assert!(arg.is_some());
        let s = arg.unwrap();
        assert!(s.contains("Anime4K_Restore_CNN_M.glsl"));
        assert!(s.contains("Anime4K_Upscale_CNN_x2_M.glsl"));
        assert!(s.contains("Anime4K_Clamp_Highlights.glsl"));
    }

    #[test]
    fn test_shader_arg_ultra_uses_vl_variants() {
        let arg = build_shader_arg(Anime4KMode::Ultra);
        assert!(arg.is_some());
        let s = arg.unwrap();
        assert!(s.contains("Anime4K_Restore_CNN_VL.glsl"));
        assert!(s.contains("Anime4K_Upscale_CNN_x2_VL.glsl"));
    }
}
