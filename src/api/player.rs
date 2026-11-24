//! Unified `libmpv` playback orchestrator (`Auto-Best`, `mpv`, `mpv.net`, `Celluloid`) & `ani-cli` launcher.
//!
//! All supported engines consume the same `shared_mpv_kv_flags` specification so GLSL mastering
//! shaders, true 4:4:4 chroma reconstruction, Lua Bento Info HUD (`i`/`I`), Floating PiP (`p`/`P`),
//! hardware debanding, 24p cadence smoothing, subtitle styling, and JSON IPC telemetry behave
//! identically across macOS, Windows, and Linux.

use crate::config::{Anime4KMode, DebandMode, MotionMode, NetworkMode, PerformanceMode, Player};
use crate::error::{AppError, Result};
use crate::services::platform::{detect_device_profile, detect_player_capabilities};
use crate::services::shaders::mpv_mastering_kv_flags;
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    sync::OnceLock,
};
use tokio::process::{Child, Command};

/// Cross-platform availability of core extractor and `libmpv`-based playback engines.
#[derive(Debug, Clone, Default)]
pub struct DependencyStatus {
    pub has_ani_cli: bool,
    pub has_mpv: bool,
    pub has_mpvnet: bool,
    pub has_celluloid: bool,
    #[allow(dead_code)]
    pub has_ffmpeg: bool,
}

impl DependencyStatus {
    pub fn has_any_player(&self) -> bool {
        self.has_mpv || self.has_mpvnet || self.has_celluloid
    }

    #[allow(dead_code)]
    pub fn is_player_installed(&self, player: Player) -> bool {
        match player {
            Player::AutoBest => self.has_any_player(),
            Player::Mpv => self.has_mpv,
            Player::MpvNet => self.has_mpvnet,
            Player::Celluloid => self.has_celluloid,
        }
    }
}

/// Options passed to the playback engine when launching an episode.
/// Hardsub visual overlap protection mode when pairing non-English subtitles
/// (e.g. Arabic) with providers that have burned-in English frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HardsubShieldMode {
    #[default]
    #[serde(alias = "opaque-mask")]
    FrostedBlur,
    SmartInpaint,
    TotalMatte,
    FrameLift,
    TopEdge,
    Off,
}

impl HardsubShieldMode {
    pub fn next(self) -> Self {
        match self {
            Self::FrostedBlur => Self::SmartInpaint,
            Self::SmartInpaint => Self::TotalMatte,
            Self::TotalMatte => Self::FrameLift,
            Self::FrameLift => Self::TopEdge,
            Self::TopEdge => Self::Off,
            Self::Off => Self::FrostedBlur,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::FrostedBlur => "Frosted Blur",
            Self::SmartInpaint => "Smart Inpaint",
            Self::TotalMatte => "Total Blackout",
            Self::FrameLift => "Frame Lift",
            Self::TopEdge => "Top Edge",
            Self::Off => "Off",
        }
    }

    pub fn shader_slug(self) -> Option<&'static str> {
        match self {
            Self::FrostedBlur => Some("blur"),
            Self::SmartInpaint => Some("inpaint"),
            Self::TotalMatte => Some("matte"),
            Self::FrameLift | Self::TopEdge | Self::Off => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HardsubBandHeight {
    Compact,
    #[default]
    Standard,
    Tall,
}

impl HardsubBandHeight {
    pub fn next(self) -> Self {
        match self {
            Self::Compact => Self::Standard,
            Self::Standard => Self::Tall,
            Self::Tall => Self::Compact,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact (12%)",
            Self::Standard => "Standard (19%)",
            Self::Tall => "Tall (25%)",
        }
    }

    pub fn bounds(self) -> (f32, f32) {
        match self {
            Self::Compact => (0.810, 0.998),
            Self::Standard => (0.730, 0.998),
            Self::Tall => (0.660, 0.998),
        }
    }

    pub fn sub_alignment(self) -> (u8, u8) {
        match self {
            Self::Compact => (98, 6),
            Self::Standard => (97, 6),
            Self::Tall => (96, 6),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayOptions {
    /// Title to pass to ani-cli as the search query
    pub title: String,
    /// Episode number to play
    pub episode: u32,
    /// Quality string e.g. "1080p", "720p", "best"
    pub quality: String,
    /// true = dub, false = sub
    pub dub: bool,
    /// Preferred player identifier (`"auto-best"`, `"mpv"`, `"mpv.net"`, `"celluloid"`)
    pub player: String,
    /// Subtitle language label (e.g. "Arabic", "English")
    pub sub_lang: Option<String>,
    /// Direct subtitle .vtt/.ass URL to pass to player
    pub sub_url: Option<String>,
    /// Direct verified stream URL (.m3u8) or offline path for instant launch
    pub stream_url: Option<String>,
    /// IPC socket / Windows pipe path connecting Solus to `libmpv`
    pub ipc_socket: Option<String>,
    /// Playback start position in seconds (for exact resume)
    pub start_pos: Option<f64>,
    /// Prebuilt fused GLSL shader list (platform separator separated)
    pub glsl_shaders: Option<String>,
    /// Cinema-grade subtitle styling and collision configuration
    pub sub_style: Option<crate::services::substyler::SubtitleStyleConfig>,
    /// Optional Y2K loading card shown before stream starts
    pub loading_image: Option<String>,
    /// Persistent subtitle synchronization delay in milliseconds (0 = no delay)
    #[serde(default)]
    pub sub_delay_ms: i32,
    /// Explicit "No subtitles" toggle
    #[serde(default)]
    pub subtitles_disabled: bool,
    /// Active network resilience mode shaping buffer cushions & HLS caps
    #[serde(default)]
    pub network_mode: NetworkMode,
    /// Force IPv4 socket binding for HTTP / transport requests
    #[serde(default)]
    pub force_ipv4: bool,
    /// Launch `libmpv` in fullscreen mode automatically
    #[serde(default)]
    pub auto_fullscreen: bool,
    /// Launch `libmpv` in floating borderless Picture-in-Picture (PiP) corner mode
    #[serde(default)]
    pub auto_pip: bool,
    /// Shader mastering preset (Off, Balanced, Ultra)
    #[serde(default)]
    pub shader_mode: Anime4KMode,
    /// GPU efficiency / thermal mastering profile
    #[serde(default)]
    pub performance_mode: PerformanceMode,
    /// Hardware debanding tier (Auto 1-pass, Off, Strong 2-pass)
    #[serde(default)]
    pub deband_mode: DebandMode,
    /// 24fps anime panning cadence & temporal interpolation
    #[serde(default)]
    pub motion_mode: MotionMode,
    /// HTTP Referer header required by the active stream provider CDN
    #[serde(default)]
    pub referer: Option<String>,
    /// Explicit demuxed HLS audio rendition URLs (ordered Sub/Dub first)
    #[serde(default)]
    pub audio_urls: Vec<String>,
    /// True if the video URL is video-only demuxed HLS requiring external `--audio-file`
    #[serde(default)]
    pub has_demuxed_audio: bool,
    /// Embedded/hardcoded subtitle status reported by the selected provider
    #[serde(default)]
    pub embedded_sub: crate::api::subtitles::EmbeddedSubtitleKind,
    /// Visual protection mode for non-English subtitles over baked English streams
    #[serde(default)]
    pub hardsub_shield: HardsubShieldMode,
    /// Band height preset for GPU hardsub eradication shaders
    #[serde(default)]
    pub hardsub_band: HardsubBandHeight,
}

/// Build the `ani-cli` command arguments for legacy fallback scraping.
pub fn build_args(opts: &PlayOptions) -> Vec<String> {
    let effective_quality = if let Some(cap) = opts.network_mode.max_height_cap() {
        let q_lower = opts.quality.trim().to_ascii_lowercase();
        if q_lower == "best" {
            format!("{cap}p")
        } else if let Some(h) = crate::api::subtitles::parse_height_from_label(&q_lower) {
            if h > cap {
                format!("{cap}p")
            } else {
                opts.quality.clone()
            }
        } else {
            opts.quality.clone()
        }
    } else {
        opts.quality.clone()
    };

    let mut args = vec![
        "-S".to_string(),
        "1".to_string(),
        "-e".to_string(),
        opts.episode.to_string(),
        "-q".to_string(),
        effective_quality,
        "--exit-after-play".to_string(),
    ];

    if opts.dub {
        args.push("--dub".to_string());
    }

    args.push(opts.title.clone());
    args
}

fn executable_exists(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Cross-platform executable discovery (`PATH` + standard Homebrew / App bundle / WinGet / Scoop paths).
pub fn find_binary(name: &str) -> Option<PathBuf> {
    if let Some(path) = env::var_os("PATH") {
        for dir in env::split_paths(&path) {
            #[cfg(windows)]
            for ext in ["exe", "cmd", "bat"] {
                let cand = dir.join(format!("{name}.{ext}"));
                if executable_exists(&cand) {
                    return Some(cand);
                }
            }
            #[cfg(not(windows))]
            {
                let cand = dir.join(name);
                if executable_exists(&cand) {
                    return Some(cand);
                }
            }
        }
    }

    let mut extras = vec![
        PathBuf::from("/opt/homebrew/bin").join(name),
        PathBuf::from("/usr/local/bin").join(name),
    ];
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        extras.push(home.join(".local/bin").join(name));
        extras.push(home.join(".cargo/bin").join(name));
    }

    #[cfg(target_os = "macos")]
    if name == "mpv" {
        extras.push(PathBuf::from("/Applications/mpv.app/Contents/MacOS/mpv"));
    }

    #[cfg(windows)]
    {
        if name == "mpvnet" || name == "mpv.net" {
            if let Ok(local) = env::var("LOCALAPPDATA") {
                extras.push(PathBuf::from(local).join(r"Programs\mpv.net\mpvnet.exe"));
            }
            extras.push(PathBuf::from(r"C:\Program Files\mpv.net\mpvnet.exe"));
        } else if name == "mpv" {
            if let Ok(local) = env::var("LOCALAPPDATA") {
                let l = PathBuf::from(local);
                extras.push(l.join(r"Microsoft\WinGet\Links\mpv.exe"));
                extras.push(l.join(r"Programs\mpv\mpv.exe"));
            }
            if let Ok(user) = env::var("USERPROFILE") {
                let u = PathBuf::from(user);
                extras.push(u.join(r"scoop\shims\mpv.exe"));
                extras.push(u.join(r"scoop\apps\mpv\current\mpv.exe"));
            }
            extras.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin\mpv.exe"));
            extras.push(PathBuf::from(r"C:\Program Files\mpv\mpv.exe"));
        }
    }

    extras.into_iter().find(|p| executable_exists(p))
}

#[cfg(windows)]
fn find_windows_ani_cli() -> Option<PathBuf> {
    if let Some(p) = find_binary("ani-cli") {
        return Some(p);
    }
    if let Ok(local) = env::var("LOCALAPPDATA") {
        let l = PathBuf::from(local);
        for sub in [
            r"Microsoft\WinGet\Links\ani-cli.cmd",
            r"Microsoft\WinGet\Links\ani-cli.bat",
            r"Microsoft\WinGet\Links\ani-cli.exe",
        ] {
            let p = l.join(sub);
            if executable_exists(&p) {
                return Some(p);
            }
        }
    }
    if let Ok(user) = env::var("USERPROFILE") {
        let u = PathBuf::from(user);
        for sub in [
            r"scoop\shims\ani-cli.cmd",
            r"scoop\shims\ani-cli.bat",
            r"scoop\shims\ani-cli.exe",
        ] {
            let p = u.join(sub);
            if executable_exists(&p) {
                return Some(p);
            }
        }
    }
    let choco = PathBuf::from(r"C:\ProgramData\chocolatey\bin\ani-cli.bat");
    if executable_exists(&choco) {
        return Some(choco);
    }
    None
}

/// Probe runtime dependencies across `PATH` and standard install locations.
pub fn detect_dependencies() -> DependencyStatus {
    let has_ani_cli = {
        #[cfg(windows)]
        {
            find_windows_ani_cli().is_some()
        }
        #[cfg(not(windows))]
        {
            find_binary("ani-cli").is_some()
        }
    };
    DependencyStatus {
        has_ani_cli,
        has_mpv: find_binary("mpv").is_some(),
        has_mpvnet: find_binary("mpvnet").is_some() || find_binary("mpv.net").is_some(),
        has_celluloid: find_binary("celluloid").is_some(),
        has_ffmpeg: find_binary("ffmpeg").is_some(),
    }
}

static DEPS_CACHE: OnceLock<DependencyStatus> = OnceLock::new();

#[allow(dead_code)]
pub fn cached_dependencies() -> &'static DependencyStatus {
    DEPS_CACHE.get_or_init(detect_dependencies)
}

/// Resolve `Player::AutoBest` (or a specific engine) into the concrete host `Player` and executable path.
/// Order for `AutoBest`: `mpv` (universal reference) -> Windows `mpv.net` -> Linux `Celluloid`.
pub fn resolve_auto_player_with_deps(player: Player, deps: &DependencyStatus) -> Player {
    match player {
        Player::AutoBest => {
            if deps.has_mpv {
                Player::Mpv
            } else if cfg!(windows) && deps.has_mpvnet {
                Player::MpvNet
            } else if cfg!(target_os = "linux") && deps.has_celluloid {
                Player::Celluloid
            } else if deps.has_mpvnet {
                Player::MpvNet
            } else if deps.has_celluloid {
                Player::Celluloid
            } else {
                Player::Mpv
            }
        }
        specific => specific,
    }
}

pub fn resolve_engine_executable(preferred: &str) -> (Player, String) {
    let parsed = Player::from_str_lossy(preferred);
    let deps = detect_dependencies();
    let effective = resolve_auto_player_with_deps(parsed, &deps);
    let bin = match effective {
        Player::Mpv | Player::AutoBest => find_binary("mpv")
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mpv".to_string()),
        Player::MpvNet => find_binary("mpvnet")
            .or_else(|| find_binary("mpv.net"))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mpvnet".to_string()),
        Player::Celluloid => find_binary("celluloid")
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "celluloid".to_string()),
    };
    (effective, bin)
}

/// Ensure wrapper scripts exist for legacy `ani-cli` external launches across `mpv` / `mpv.net` / `celluloid`.
pub fn ensure_player_wrapper(effective_player: Player, real_bin: &str) -> Result<PathBuf> {
    let wrapper_name = match effective_player {
        Player::MpvNet => "solus-mpvnet",
        Player::Celluloid => "solus-celluloid",
        Player::Mpv | Player::AutoBest => "solus-mpv",
    };

    let bin_dir = env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".local/share/solus/bin"))
        .filter(|p| !p.to_string_lossy().contains(' '))
        .unwrap_or_else(|| env::temp_dir().join("solus-bin"));

    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| AppError::Player(format!("Failed to create wrapper dir: {e}")))?;

    let wrapper_path = bin_dir.join(wrapper_name);
    let is_celluloid = effective_player == Player::Celluloid || real_bin.contains("celluloid");

    let script_content = format!(
        r#"#!/bin/bash
REAL_PLAYER="${{SOLUS_REAL_PLAYER:-{real_bin}}}"
if [ ! -x "$REAL_PLAYER" ] && ! command -v "$REAL_PLAYER" >/dev/null 2>&1; then
    REAL_PLAYER="$(command -v mpv || command -v mpvnet || command -v celluloid || echo "{real_bin}")"
fi
args=()
video_link=""
for arg in "$@"; do
    case "$arg" in
        --sub-file=*)
            if [ -z "$SOLUS_SUB_URL" ]; then args+=("$arg"); fi
            ;;
        http*://* | /*.m3u8*)
            video_link="$arg"
            ;;
        *)
            args+=("$arg")
            ;;
    esac
done
if [ -n "$SOLUS_IPC_SOCKET" ]; then
    args+=("--input-ipc-server=$SOLUS_IPC_SOCKET")
fi
if [ -n "$SOLUS_AUDIO_SCRIPT" ]; then
    args+=("--script=$SOLUS_AUDIO_SCRIPT")
fi
if [ -n "$SOLUS_START_POS" ]; then
    args+=("--start=$SOLUS_START_POS")
fi
if [ -n "$SOLUS_SHADERS" ]; then
    args+=("--glsl-shaders=$SOLUS_SHADERS")
fi
if [ "{is_celluloid}" = "true" ]; then
    cel_args=("--new-window" "--mpv-force-window=immediate")
    for a in "${{args[@]}}"; do
        case "$a" in
            --*) cel_args+=("--mpv-${{a#--}}") ;;
            *) cel_args+=("$a") ;;
        esac
    done
    [ -n "$video_link" ] && exec "$REAL_PLAYER" "${{cel_args[@]}}" "$video_link"
    exec "$REAL_PLAYER" "${{cel_args[@]}}"
else
    [ -n "$video_link" ] && exec "$REAL_PLAYER" "${{args[@]}}" "$video_link"
    exec "$REAL_PLAYER" "${{args[@]}}"
fi
"#
    );

    std::fs::write(&wrapper_path, script_content)
        .map_err(|e| AppError::Player(format!("Failed to write player wrapper: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&wrapper_path)
            .map_err(|e| AppError::Player(format!("Failed to read wrapper metadata: {e}")))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&wrapper_path, perms)
            .map_err(|e| AppError::Player(format!("Failed to set wrapper permissions: {e}")))?;
    }

    Ok(wrapper_path)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn derive_master_playlist_url(video_link: &str) -> Option<String> {
    let trimmed = video_link.split('?').next().unwrap_or(video_link);
    if trimmed.ends_with("/index.m3u8") {
        let without_file = trimmed.strip_suffix("/index.m3u8")?;
        let (parent_of_parent, _) = without_file.rsplit_once('/')?;
        if !parent_of_parent.contains("://") || parent_of_parent.ends_with(":/") {
            return None;
        }
        return Some(format!("{parent_of_parent}/master.m3u8"));
    }
    if let Some((parent, file_name)) = trimmed.rsplit_once('/') {
        if file_name.starts_with("index-f") && file_name.ends_with(".m3u8") {
            if !parent.contains("://") || parent.ends_with(":/") {
                return None;
            }
            return Some(format!("{parent}/master.m3u8"));
        }
    }
    None
}

const YOUTUBE_INPUT_CONF: &str = r#"# Solus Unified libmpv Keybindings (mpv / mpv.net / Celluloid)
k cycle pause
K cycle pause
SPACE cycle pause
j seek -10
J seek -10
l seek 10
L seek 10
LEFT seek -5
RIGHT seek 5
UP add volume 5
DOWN add volume -5
m script-binding solus-cycle-motion
M cycle mute
f cycle fullscreen
F cycle fullscreen
ESC set fullscreen no
c cycle sub-visibility
C cycle sub-visibility
p script-binding solus-toggle-pip
P script-binding solus-cycle-pip-corner
i script-binding solus-toggle-hud
I script-binding solus-pin-hud
TAB script-binding solus-skip-intro
n script-binding solus-next-ep-key-lower
N script-binding solus-next-ep-key
S script-binding solus-toggle-sub-bar
y script-binding solus-snap-sub-voice
z add sub-delay -0.1
x add sub-delay 0.1
Z add sub-delay -0.5
X add sub-delay 0.5
a script-binding solus-toggle-shaders
d script-binding solus-toggle-deband
g script-binding solus-cycle-perf
F1 script-binding stats/display-stats-toggle
0 seek 0 absolute-percent
1 seek 10 absolute-percent
2 seek 20 absolute-percent
3 seek 30 absolute-percent
4 seek 40 absolute-percent
5 seek 50 absolute-percent
6 seek 60 absolute-percent
7 seek 70 absolute-percent
8 seek 80 absolute-percent
9 seek 90 absolute-percent
< multiply speed 0.8
> multiply speed 1.25
. frame-step
, frame-back-step
"#;

pub fn ensure_youtube_input_conf() -> Option<PathBuf> {
    let dir = crate::config::Config::cache_dir()
        .unwrap_or_else(|_| env::temp_dir().join("solus-cache"));
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = dir.join("youtube-input.conf");
    let needs_write = std::fs::read_to_string(&path)
        .map(|cur| cur != YOUTUBE_INPUT_CONF)
        .unwrap_or(true);
    if needs_write && std::fs::write(&path, YOUTUBE_INPUT_CONF).is_err() {
        return None;
    }
    Some(path)
}

const SOLUS_AUDIO_LUA: &str = r##"-- Solus Unified libmpv Companion Script (mpv / mpv.net / Celluloid):
--   1. Smart Audio Cycle Bridge (# / Ctrl+a)
--   2. Floating Picture-in-Picture Controller (p toggle, P cycle corner/size)
--   3. Live Mastering Hotkeys (a Shaders, d Deband, m Motion Cadence, g Perf Tier)
--   4. Multi-Color Curated Bento Video Info HUD (i 6s toggle, I 1Hz pin, F1 raw stats)
local function solus_cycle_audio()
    local tracks = mp.get_property_native("track-list") or {}
    local audio_tracks = {}
    local cur_aid = mp.get_property_number("aid")
    for _, t in ipairs(tracks) do
        if t.type == "audio" and t.id then
            table.insert(audio_tracks, t)
        end
    end

    if #audio_tracks > 1 then
        local cur_idx = 1
        for i, t in ipairs(audio_tracks) do
            if t.id == cur_aid then
                cur_idx = i
                break
            end
        end
        local next_idx = (cur_idx % #audio_tracks) + 1
        local nxt = audio_tracks[next_idx]
        mp.set_property_number("aid", nxt.id)
        mp.commandv("seek", "0", "relative+exact")
        local label = nxt.title or nxt.lang or ("Track " .. tostring(nxt.id))
        mp.osd_message("✦ Audio: " .. tostring(label), 2.5)
    else
        mp.osd_message("✦ Switching Sub/Dub stream...", 2.5)
        io.write("__SOLUS_AUDIO_CYCLE__\n")
        io.flush()
    end
end

mp.add_forced_key_binding("#", "solus-cycle-audio-hash", solus_cycle_audio)
mp.add_forced_key_binding("SHARP", "solus-cycle-audio-sharp", solus_cycle_audio)
mp.add_forced_key_binding("Ctrl+a", "solus-cycle-audio-ctrl", solus_cycle_audio)

-- =============================================================================
-- 2. Picture-in-Picture (PiP) Floating Window Controller ([p] / [P])
-- =============================================================================
local pip_active = (mp.get_opt("solus_auto_pip") or "0") == "1"
local pip_idx = 1
local pip_presets = {
    { label = "Bottom-Right 30%", scale = 0.30, geom = "30%x30%-24-24" },
    { label = "Bottom-Left 30%",  scale = 0.30, geom = "30%x30%+24-24" },
    { label = "Top-Right 30%",    scale = 0.30, geom = "30%x30%-24+24" },
    { label = "Top-Left 30%",     scale = 0.30, geom = "30%x30%+24+24" },
    { label = "Compact 24%",      scale = 0.24, geom = "24%x24%-20-20" },
    { label = "Theater 38%",      scale = 0.38, geom = "38%x38%-28-28" },
}

local function apply_pip_preset()
    local pr = pip_presets[pip_idx]
    mp.set_property_bool("fullscreen", false)
    mp.set_property_bool("ontop", true)
    mp.set_property_bool("border", false)
    mp.set_property_bool("keepaspect-window", true)
    mp.set_property_number("window-scale", pr.scale)
    mp.set_property("geometry", pr.geom)
    mp.osd_message("◈ PiP Floating: " .. pr.label .. " ([P] corner, [p] exit)", 2.2)
end

local function toggle_pip()
    pip_active = not pip_active
    if pip_active then
        apply_pip_preset()
    else
        mp.set_property_bool("ontop", false)
        mp.set_property_bool("border", true)
        mp.set_property_number("window-scale", 1.0)
        mp.set_property("geometry", "50%:50%")
        mp.osd_message("◈ PiP Floating: OFF (Standard Window)", 2.0)
    end
end

local function cycle_pip_corner()
    if not pip_active then
        pip_active = true
    else
        pip_idx = (pip_idx % #pip_presets) + 1
    end
    apply_pip_preset()
end

mp.add_forced_key_binding("p", "solus-toggle-pip", toggle_pip)
mp.add_forced_key_binding("P", "solus-cycle-pip-corner", cycle_pip_corner)

-- =============================================================================
-- 3. Live Video Mastering Hotkeys ([a] Shaders / [d] Deband / [m] Motion / [g] Perf)
-- =============================================================================
local saved_shaders = mp.get_property("glsl-shaders", "")
local function toggle_shaders()
    local cur = mp.get_property("glsl-shaders", "")
    if cur ~= nil and cur ~= "" then
        saved_shaders = cur
        mp.set_property("glsl-shaders", "")
        mp.osd_message("◈ GLSL Shaders: OFF (Native Reference)", 2.0)
    elseif saved_shaders ~= nil and saved_shaders ~= "" then
        mp.set_property("glsl-shaders", saved_shaders)
        mp.osd_message("◈ GLSL Shaders: RESTORED (Fused Mastering + 4:4:4)", 2.0)
    else
        mp.osd_message("◈ No GLSL shaders queued for this stream", 1.8)
    end
end

local function toggle_deband()
    local cur = mp.get_property_bool("deband", false)
    mp.set_property_bool("deband", not cur)
    mp.osd_message("◈ GPU Debanding: " .. ((not cur) and "ON (1-Pass Anti-Banding)" or "OFF"), 2.0)
end

local motion_idx = 0
local function cycle_motion_cadence()
    motion_idx = (motion_idx + 1) % 3
    if motion_idx == 0 then
        mp.set_property_bool("interpolation", false)
        mp.set_property("video-sync", "audio")
        mp.osd_message("◈ Motion Cadence: OFF (Bit-Exact 24p)", 2.0)
    elseif motion_idx == 1 then
        mp.set_property("video-sync", "display-resample")
        mp.set_property("tscale", "oversample")
        mp.set_property_bool("interpolation", true)
        mp.osd_message("◈ Motion Cadence: CADENCE (24p -> 60Hz Smooth Pan)", 2.0)
    else
        mp.set_property("video-sync", "display-resample")
        mp.set_property("tscale", "mitchell")
        mp.set_property_bool("interpolation", true)
        mp.osd_message("◈ Motion Cadence: SMOOTH (120Hz Interpolation)", 2.0)
    end
end

local perf_idx = 1
local perf_profiles = {
    { label = "Eco",          opts = "shadow_lift=0.025,deblock_amt=0.18,detail_lift=0.06,cas_amt=0.18,grain_amt=0.004" },
    { label = "Balanced",     opts = "shadow_lift=0.035,deblock_amt=0.30,detail_lift=0.10,cas_amt=0.26,grain_amt=0.006" },
    { label = "Reference 4K", opts = "shadow_lift=0.045,deblock_amt=0.32,detail_lift=0.14,cas_amt=0.32,grain_amt=0.008" },
}
local function cycle_perf_preset()
    perf_idx = (perf_idx % #perf_profiles) + 1
    local p = perf_profiles[perf_idx]
    mp.set_property("glsl-shader-opts", p.opts)
    mp.osd_message("◈ Mastering Tier: " .. p.label, 2.0)
end

mp.add_forced_key_binding("a", "solus-toggle-shaders", toggle_shaders)
mp.add_forced_key_binding("d", "solus-toggle-deband", toggle_deband)
mp.add_forced_key_binding("m", "solus-cycle-motion", cycle_motion_cadence)
mp.add_forced_key_binding("g", "solus-cycle-perf", cycle_perf_preset)

-- =============================================================================
-- 4. Multi-Color Curated Bento Video Info HUD ([i] 6s / [I] Pinned 1Hz / [F1] Raw)
-- =============================================================================
local hud_ov = mp.create_osd_overlay("ass-events")
hud_ov.res_x = 1280
hud_ov.res_y = 720
local hud_visible = false
local sub_bar_open = false
local hud_pinned = false
local hud_tick_timer = nil
local hud_hide_timer = nil

local function esc_ass(s)
    if not s then return "" end
    return tostring(s):gsub("[{}]", ""):gsub("[\r\n]+", " ")
end

local function fmt_clock(s)
    if not s or s < 0 then return "--:--" end
    local tot = math.floor(s)
    local h = math.floor(tot / 3600)
    local m = math.floor((tot % 3600) / 60)
    local sec = tot % 60
    if h > 0 then
        return string.format("%d:%02d:%02d", h, m, sec)
    end
    return string.format("%02d:%02d", m, sec)
end

local function fmt_speed(bps)
    if not bps or bps <= 0 then return "" end
    if bps >= 1048576 then
        return string.format("%.1f MB/s", bps / 1048576.0)
    elseif bps >= 1024 then
        return string.format("%d KB/s", math.floor(bps / 1024.0))
    end
    return string.format("%d B/s", math.floor(bps))
end

local function hide_info_hud()
    hud_visible = false
    hud_pinned = false
    sub_bar_open = false
    if hud_tick_timer then hud_tick_timer:kill(); hud_tick_timer = nil end
    if hud_hide_timer then hud_hide_timer:kill(); hud_hide_timer = nil end
    hud_ov:remove()
end

local function render_info_hud()
    if not hud_visible then
        hud_ov:remove()
        return
    end
    local mtitle = esc_ass(mp.get_property("media-title", "Solus Stream"))
    if #mtitle > 68 then mtitle = string.sub(mtitle, 1, 65) .. "..." end

    local pos = mp.get_property_number("time-pos", 0) or 0
    local dur = mp.get_property_number("duration", 0) or 0
    local rem = math.max(0, dur - pos)
    local pct = mp.get_property_number("percent-pos", 0) or 0
    local spd = mp.get_property_number("speed", 1.0) or 1.0
    local vw = mp.get_property_number("width", 0) or 0
    local vh = mp.get_property_number("height", 0) or 0
    local fps = mp.get_property_number("estimated-vf-fps", 0) or 0
    local disp_fps = mp.get_property_number("display-fps", 0) or 0
    local raw_codec = mp.get_property("video-codec-name", mp.get_property("video-codec", "")) or ""
    local vcodec_u = esc_ass(raw_codec):upper()
    if vcodec_u:find("H264") or vcodec_u:find("AVC") then
        vcodec_u = "H.264"
    elseif vcodec_u:find("HEVC") or vcodec_u:find("H265") or vcodec_u:find("HVC") then
        vcodec_u = "H.265/HEVC"
    elseif vcodec_u:find("AV1") or vcodec_u:find("AV01") then
        vcodec_u = "AV1"
    elseif vcodec_u:find("VP9") or vcodec_u:find("VP09") then
        vcodec_u = "VP9"
    elseif vcodec_u == "" then
        vcodec_u = "—"
    elseif #vcodec_u > 16 then
        vcodec_u = string.sub(vcodec_u, 1, 14) .. ".."
    end
    local vcodec = vcodec_u
    local vbit = mp.get_property_number("video-bitrate", 0) or 0
    local hwdec = esc_ass(mp.get_property("hwdec-current", ""))
    local drops = (mp.get_property_number("decoder-frame-drop-count", 0) or 0) + (mp.get_property_number("frame-drop-count", 0) or 0)
    local vo = esc_ass(mp.get_property("current-vo", "gpu-next"))
    local avs = mp.get_property_number("avsync", 0) or 0
    local shaders_s = esc_ass(mp.get_property("glsl-shaders", ""))
    local deband_on = mp.get_property_bool("deband", false)
    local interp_on = mp.get_property_bool("interpolation", false)

    local acodec = esc_ass(mp.get_property("audio-codec-name", mp.get_property("audio-codec", "—"))):upper()
    local abit = mp.get_property_number("audio-bitrate", 0) or 0
    local sr = mp.get_property_number("audio-params/samplerate", 0) or 0
    local ch_n = mp.get_property_number("audio-params/channel-count", 2) or 2
    local vol = mp.get_property_number("volume", 100) or 100
    local muted = mp.get_property_bool("mute", false)

    local sub_vis = mp.get_property_bool("sub-visibility", false)
    local sub_delay = mp.get_property_number("sub-delay", 0) or 0
    local sub_lbl = sub_vis and string.format("ON (%+.2fs)", sub_delay) or "OFF"

    local c_dur = mp.get_property_number("demuxer-cache-duration", 0) or 0
    local c_spd = mp.get_property_number("cache-speed", 0) or 0
    local c_bytes = 0
    local cst = mp.get_property_native("demuxer-cache-state")
    if cst and cst["total-bytes"] then c_bytes = cst["total-bytes"] end

    local q_tag = (vh >= 2160 and "4K UHD") or (vh >= 1440 and "1440p QHD") or (vh >= 1080 and "1080p FHD") or (vh >= 720 and "720p HD") or "Stream"
    local win_badge = pip_active and "PIP FLOAT" or (mp.get_property_bool("fullscreen", false) and "FULLSCREEN" or "WINDOWED")
    local pin_badge = hud_pinned and "PINNED 1Hz" or "AUTO 6s"
    local hw_lbl = (hwdec ~= "" and hwdec ~= "no") and ("GPU · " .. hwdec) or "CPU Software"
    local hw_col = (hwdec ~= "" and hwdec ~= "no") and "&H99D334&" or "&H24BFFB&"
    local drop_col = (drops > 0) and "&H5E3FF4&" or "&H99D334&"
    local vol_str = muted and "MUTED" or string.format("%d%%", math.floor(vol))
    local vol_col = muted and "&H5E3FF4&" or "&H99D334&"
    local vbit_s = (vbit > 0) and string.format("%.2f Mbps", vbit / 1000000.0) or "HLS ABR"
    local abit_s = (abit > 0) and string.format("%d kbps", math.floor(abit / 1000.0)) or "Auto"
    local sr_s = (sr > 0) and string.format("%.1f kHz", sr / 1000.0) or "Auto"
    local shader_state = (shaders_s ~= "") and "Fused + 4:4:4 + A4K" or ("Native" .. (deband_on and " + Deband" or ""))
    local vo_full = vo .. (interp_on and " · Cadence" or "")
    local c_mib = c_bytes / 1048576.0
    local spd_txt = fmt_speed(c_spd)
    local buf_health = (c_dur >= 12.0) and "Healthy" or ((c_dur >= 3.0) and "Fair" or "Low")
    local buf_col = (c_dur >= 12.0) and "&H99D334&" or ((c_dur >= 3.0) and "&H24BFFB&" or "&H5E3FF4&")

    local prog_w = math.max(8, math.floor(1140 * math.min(1.0, math.max(0.0, pct / 100.0))))
    local buf_frac = math.min(1.0, c_dur / 60.0)
    local buf_w = math.max(12, math.floor(520 * buf_frac))

    local ass = string.format(
        "{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H100C09&\\1a&H18&\\p1}m 42 26 l 1238 26 l 1238 378 l 42 378{\\p0}\n" ..
        "{\\an7\\pos(0,0)\\bord2\\1a&HFF&\\3c&HF8BD38&\\p1}m 42 26 l 1238 26 l 1238 378 l 42 378{\\p0}\n" ..
        "{\\an7\\pos(66,42)\\bord0\\shad0\\fs19\\b1\\1c&HF8BD38&}◈ SOLUS INFO HUD   " ..
        "{\\1c&H24BFFB&}[%s]   {\\1c&H99D334&}[%s]   {\\1c&HFA8BA7&}[%s]\n" ..
        "{\\an7\\pos(66,70)\\bord0\\shad0\\fs24\\b1\\1c&HFA8BA7&}%s\n" ..
        "{\\an7\\pos(66,102)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}%s / %s (%d%%)  ·  -%s left  ·  Speed %.2fx\n" ..
        "{\\an7\\pos(66,128)\\bord0\\shad0\\1c&H2B2420&\\p1}m 0 0 l 1140 0 l 1140 6 l 0 6{\\p0}\n" ..
        "{\\an7\\pos(66,128)\\bord0\\shad0\\1c&HFA8BA7&\\p1}m 0 0 l %d 0 l %d 6 l 0 6{\\p0}\n" ..
        "{\\an7\\pos(66,150)\\bord0\\shad0\\fs16\\b1\\1c&HF8BD38&}▶ VIDEO & QUALITY\n" ..
        "{\\an7\\pos(66,176)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Stream  : {\\1c&HFAF4F0&\\b1}%dx%d  %s\n" ..
        "{\\an7\\pos(66,200)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Codec   : {\\1c&HF8BD38&}%s  ·  %s\n" ..
        "{\\an7\\pos(66,224)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Rate    : {\\1c&H99D334&}%.2f fps{\\1c&HC8B8A4&} (Disp %.0fHz)\n" ..
        "{\\an7\\pos(66,248)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Decode  : {\\1c%s}%s\n" ..
        "{\\an7\\pos(460,150)\\bord0\\shad0\\fs16\\b1\\1c&HFA8BA7&}♪ AUDIO & CHANNELS\n" ..
        "{\\an7\\pos(460,176)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Codec   : {\\1c&HFA8BA7&}%s  ·  %s\n" ..
        "{\\an7\\pos(460,200)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Output  : {\\1c&HFAF4F0&}%s  ·  %d ch\n" ..
        "{\\an7\\pos(460,224)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Volume  : {\\1c%s}%s {\\1c&HC8B8A4&}· Speed %.2fx\n" ..
        "{\\an7\\pos(460,248)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Subs    : {\\1c&H24BFFB&}%s\n" ..
        "{\\an7\\pos(860,150)\\bord0\\shad0\\fs16\\b1\\1c&H24BFFB&}◈ MASTERING & ENGINE\n" ..
        "{\\an7\\pos(860,176)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Shaders : {\\1c&HFA8BA7&}%s\n" ..
        "{\\an7\\pos(860,200)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}VideoOut: {\\1c&H24BFFB&}%s\n" ..
        "{\\an7\\pos(860,224)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}Dropped : {\\1c%s}%d frames\n" ..
        "{\\an7\\pos(860,248)\\bord0\\shad0\\fs16\\1c&HC8B8A4&}A/V Sync: {\\1c&H99D334&}%+.1f ms\n" ..
        "{\\an7\\pos(66,288)\\bord0\\shad0\\fs16\\b1\\1c&H24BFFB&}◈ DEMUXER CACHE   {\\1c&HFAF4F0&\\b0}%.1fs ahead  ·  %.1f MiB  %s  ·  {\\1c%s}%s\n" ..
        "{\\an7\\pos(66,314)\\bord0\\shad0\\1c&H2B2420&\\p1}m 0 0 l 520 0 l 520 10 l 0 10{\\p0}\n" ..
        "{\\an7\\pos(66,314)\\bord0\\shad0\\1c%s\\p1}m 0 0 l %d 0 l %d 10 l 0 10{\\p0}\n" ..
        "{\\an7\\pos(66,340)\\bord0\\shad0\\fs15\\1c&HC8B8A4&}Keys: {\\1c&HF8BD38&}[Tab] Skip  [N] Next  [y] Snap Sub  {\\1c&H99D334&}[p/P] PiP  {\\1c&H24BFFB&}[a] Shaders [d] Deband [m] Motion  {\\1c&HFA8BA7&}[i/I] HUD [F1] Raw",
        q_tag, win_badge, pin_badge,
        mtitle,
        fmt_clock(pos), fmt_clock(dur), math.floor(pct), fmt_clock(rem), spd,
        prog_w, prog_w,
        vw, vh, q_tag,
        vcodec, vbit_s,
        fps, disp_fps,
        hw_col, hw_lbl,
        acodec, abit_s,
        sr_s, ch_n,
        vol_col, vol_str, spd,
        sub_lbl,
        shader_state,
        vo_full,
        drop_col, drops,
        avs * 1000.0,
        c_dur, c_mib, (spd_txt ~= "") and ("· " .. spd_txt) or "", buf_col, buf_health,
        buf_col, buf_w, buf_w
    )
    hud_ov.data = ass
    hud_ov:update()
end

local function ensure_hud_timer()
    if not hud_tick_timer then
        hud_tick_timer = mp.add_periodic_timer(1.0, render_info_hud)
    end
end

local function toggle_info_hud()
    if hud_visible and not hud_pinned then
        hide_info_hud()
        return
    end
    hud_pinned = false
    hud_visible = true
    sub_bar_open = true
    if hud_hide_timer then hud_hide_timer:kill() end
    hud_hide_timer = mp.add_timeout(6.0, function()
        if not hud_pinned then hide_info_hud() end
    end)
    ensure_hud_timer()
    render_info_hud()
end

local function pin_info_hud()
    if hud_visible and hud_pinned then
        hide_info_hud()
        return
    end
    if hud_hide_timer then hud_hide_timer:kill(); hud_hide_timer = nil end
    hud_visible = true
    hud_pinned = true
    sub_bar_open = true
    ensure_hud_timer()
    render_info_hud()
end

mp.add_forced_key_binding("i", "solus-toggle-hud", pin_info_hud)
mp.add_forced_key_binding("I", "solus-pin-hud", pin_info_hud)

-- Hardsub Eradicator runtime toggler ([h] mode, [H] shader band height)
local hs_shader_dir = "__SOLUS_HS_SHADER_DIR__"
local shield_modes = {
    { name = "Frosted Blur",   slug = "blur" },
    { name = "Smart Inpaint",  slug = "inpaint" },
    { name = "Total Blackout", slug = "matte" },
    { name = "Frame Lift",     slug = nil },
    { name = "Top Edge",       slug = nil },
    { name = "Off",            slug = nil },
}
local band_sizes = {
    { label = "Compact (12%)",  tag = "81_100", sub_pos = 98, margin_y = 6 },
    { label = "Standard (19%)", tag = "73_100", sub_pos = 97, margin_y = 6 },
    { label = "Tall (25%)",     tag = "66_100", sub_pos = 96, margin_y = 6 },
}
local shield_idx = 1
local band_idx = 2

local function solus_detect_initial_shield()
    local shaders = mp.get_property_native("glsl-shaders") or {}
    local found_hs = false
    for _, p in ipairs(shaders) do
        local s = tostring(p)
        if s:find("solus_hs_", 1, true) then
            found_hs = true
            if s:find("solus_hs_inpaint_", 1, true) then shield_idx = 2
            elseif s:find("solus_hs_matte_", 1, true) then shield_idx = 3
            else shield_idx = 1 end
            if s:find("_81_100.glsl", 1, true) then band_idx = 1
            elseif s:find("_66_100.glsl", 1, true) then band_idx = 3
            else band_idx = 2 end
            break
        end
    end
    if not found_hs then
        local vay = mp.get_property_number("video-align-y", 0)
        local vzoom = mp.get_property_number("video-zoom", 0)
        local spos = mp.get_property_number("sub-pos", 100)
        if vay < -0.2 or vzoom < -0.05 then shield_idx = 4
        elseif spos <= 15 then shield_idx = 5
        elseif mp.get_property("sub-border-style", "") == "outline-and-shadow" and spos >= 95 then shield_idx = 6 end
    end
end
mp.register_event("file-loaded", solus_detect_initial_shield)

local function solus_apply_hardsub_shield(announce)
    local mode = shield_modes[shield_idx]
    local band = band_sizes[band_idx]
    local cur_shaders = mp.get_property_native("glsl-shaders") or {}
    local next_shaders = {}
    for _, p in ipairs(cur_shaders) do
        if not tostring(p):find("solus_hs_", 1, true) then
            table.insert(next_shaders, p)
        end
    end
    if mode.slug ~= nil then
        table.insert(next_shaders, hs_shader_dir .. "/solus_hs_" .. mode.slug .. "_" .. band.tag .. ".glsl")
    end
    mp.set_property_native("glsl-shaders", next_shaders)

    if mode.name == "Frame Lift" then
        mp.set_property_number("video-zoom", -0.16)
        mp.set_property_number("video-align-y", -1.0)
        mp.set_property("sub-use-margins", "yes")
        mp.set_property("sub-ass-force-margins", "yes")
        mp.set_property("sub-border-style", "outline-and-shadow")
        mp.set_property_number("sub-border-size", 2.4)
        mp.set_property_number("sub-shadow-offset", 1.0)
        mp.set_property_number("sub-margin-y", 6)
        mp.set_property_number("sub-pos", 99)
        mp.set_property("sub-ass-override", "force")
    elseif mode.name == "Top Edge" then
        mp.set_property_number("video-zoom", 0)
        mp.set_property_number("video-align-y", 0)
        mp.set_property("sub-use-margins", "no")
        mp.set_property("sub-ass-force-margins", "no")
        mp.set_property("sub-border-style", "outline-and-shadow")
        mp.set_property_number("sub-border-size", 2.6)
        mp.set_property_number("sub-shadow-offset", 1.0)
        mp.set_property_number("sub-margin-y", 18)
        mp.set_property_number("sub-pos", 7)
        mp.set_property("sub-ass-override", "force")
    elseif mode.slug ~= nil then
        mp.set_property_number("video-zoom", 0)
        mp.set_property_number("video-align-y", 0)
        mp.set_property("sub-use-margins", "no")
        mp.set_property("sub-ass-force-margins", "no")
        mp.set_property("sub-border-style", "outline-and-shadow")
        mp.set_property_number("sub-border-size", 2.6)
        mp.set_property_number("sub-shadow-offset", 1.0)
        mp.set_property_number("sub-margin-y", band.margin_y)
        mp.set_property_number("sub-pos", band.sub_pos)
        mp.set_property("sub-ass-override", "force")
    else
        mp.set_property_number("video-zoom", 0)
        mp.set_property_number("video-align-y", 0)
        mp.set_property("sub-use-margins", "yes")
        mp.set_property("sub-ass-force-margins", "no")
        mp.set_property("sub-border-style", "outline-and-shadow")
        mp.set_property_number("sub-margin-y", 38)
        mp.set_property_number("sub-pos", 100)
    end
    if announce then
        local msg = "◈ Hardsub Eradicator: " .. mode.name
        if mode.slug ~= nil then
            msg = msg .. " [" .. band.label .. "]"
        end
        mp.osd_message(msg, 2.4)
    end
end

local function solus_cycle_hardsub_shield()
    shield_idx = (shield_idx % #shield_modes) + 1
    solus_apply_hardsub_shield(true)
end

local function solus_cycle_hardsub_band()
    band_idx = (band_idx % #band_sizes) + 1
    if shield_modes[shield_idx].slug == nil then
        shield_idx = 1
    end
    solus_apply_hardsub_shield(true)
end
mp.add_forced_key_binding("h", "solus-hardsub-shield", solus_cycle_hardsub_shield)
mp.add_forced_key_binding("H", "solus-hardsub-band", solus_cycle_hardsub_band)

-- =============================================================================
-- 5. Crunchyroll/Netflix Clickable Buttons ([Skip Intro/Outro], [Next Ep])
--    & Elevated Sub Sync Bar ([S] / [i] / [-5s][-1s][-0.2s][◈ AUTO-SYNC][+0.2s][+1s][+5s])
-- =============================================================================
local utils = require 'mp.utils'
local btn_ov = mp.create_osd_overlay("ass-events")
btn_ov.res_x = 1280
btn_ov.res_y = 720

local player_ctx = {
    ep = 1,
    next_ep = nil,
    op_start = nil,
    op_end = nil,
    ed_start = nil,
    ed_end = nil,
    sub_path = nil,
}
local next_ep_dispatched = false
local hover_active_until = 0
local hovered_btn_id = nil
local active_hitboxes = {}
local sync_in_progress = false
local sync_status_msg = "Press [y] or click AUTO-SYNC VOICE for automated Speech-VAD alignment"
local sync_status_col = "&HC8B8A4&"

local function active_sub_file_path()
    if player_ctx.sub_path and player_ctx.sub_path ~= "" then
        return player_ctx.sub_path
    end
    local tracks = mp.get_property_native("track-list") or {}
    for _, t in ipairs(tracks) do
        if t.type == "sub" and t.selected and t["external-filename"] then
            return t["external-filename"]
        end
    end
    return nil
end

local function parse_sub_ts(s)
    if not s then return nil end
    s = s:gsub(",", "."):match("^%s*(.-)%s*$")
    local h, m, sec = s:match("^(%d+):(%d+):([%d%.]+)$")
    if h then
        return tonumber(h) * 3600 + tonumber(m) * 60 + tonumber(sec)
    end
    local m2, sec2 = s:match("^(%d+):([%d%.]+)$")
    if m2 then
        return tonumber(m2) * 60 + tonumber(sec2)
    end
    return nil
end

local function utf8_truncate(s, max_chars)
    if not s then return "" end
    local chars = 0
    local i = 1
    local len = #s
    while i <= len do
        local b = s:byte(i)
        local step = 1
        if b >= 0xF0 then step = 4
        elseif b >= 0xE0 then step = 3
        elseif b >= 0xC0 then step = 2 end
        if chars + 1 > max_chars then
            return string.sub(s, 1, i - 1) .. "..."
        end
        chars = chars + 1
        i = i + step
    end
    return s
end

local function find_active_cue_preview(delay_secs)
    local live_txt = esc_ass(mp.get_property("sub-text", "") or "")
    local cur_t = (mp.get_property_number("time-pos", 0) or 0) - (delay_secs or 0)
    if live_txt ~= "" then
        return string.format("Live: \"%s\"", utf8_truncate(live_txt, 38))
    end
    local spath = active_sub_file_path()
    if not spath or spath:match("^https?://") then
        return "No local cue preview — subtitles preview when cached"
    end
    local f = io.open(spath, "r")
    if not f then return "Cue preview unavailable" end
    local nearest_ts, nearest_txt, nearest_dist = nil, nil, 999999
    local prev_start = nil
    for line in f:lines() do
        local s_ts = line:match("^%s*([%d:%.]+)%s*%-%->")
        if s_ts then
            prev_start = parse_sub_ts(s_ts)
        elseif line:match("^Dialogue:") then
            local a_ts, a_txt = line:match("^Dialogue:%s*%d+,([^,]+),[^,]*,[^,]*,[^,]*,[^,]*,[^,]*,[^,]*,[^,]*,(.*)$")
            local ts = parse_sub_ts(a_ts)
            if ts and a_txt then
                local clean = esc_ass(a_txt:gsub("{[^}]*}", ""):gsub("\\N", " "))
                local d = math.abs(ts - cur_t)
                if clean ~= "" and d < nearest_dist then
                    nearest_dist, nearest_ts, nearest_txt = d, ts, clean
                end
            end
        elseif prev_start ~= nil and not line:match("^%s*$") and not line:match("^%d+$") then
            local clean = esc_ass(line:gsub("<[^>]*>", ""))
            local d = math.abs(prev_start - cur_t)
            if clean ~= "" and d < nearest_dist then
                nearest_dist, nearest_ts, nearest_txt = d, prev_start, clean
            end
            prev_start = nil
        end
    end
    f:close()
    if nearest_ts and nearest_txt then
        return string.format("Nearest @ %s: \"%s\"", fmt_clock(nearest_ts + (delay_secs or 0)), utf8_truncate(nearest_txt, 34))
    end
    return "No dialogue cues found in file"
end

local function do_skip_intro_op()
    local cur_t = mp.get_property_number("time-pos", 0) or 0
    if player_ctx.op_start and player_ctx.op_end and cur_t >= (player_ctx.op_start - 5.0) and cur_t < (player_ctx.op_end - 1.0) then
        mp.commandv("seek", player_ctx.op_end, "absolute")
        mp.osd_message(">>| Skipped Intro (AniSkip)", 2.2)
    else
        mp.commandv("seek", cur_t + 85.0, "absolute")
        mp.osd_message(">>| Skipped Intro (+85s)", 2.0)
    end
end

local function do_skip_outro_ed()
    local cur_t = mp.get_property_number("time-pos", 0) or 0
    local dur = mp.get_property_number("duration", 0) or 0
    if player_ctx.ed_start and player_ctx.ed_end and cur_t >= (player_ctx.ed_start - 10.0) and cur_t < (player_ctx.ed_end - 1.0) then
        mp.commandv("seek", player_ctx.ed_end, "absolute")
        mp.osd_message(">>| Skipped Ending (AniSkip)", 2.2)
    elseif dur > 120.0 and (dur - cur_t) <= 180.0 then
        mp.commandv("seek", math.max(cur_t, dur - 8.0), "absolute")
        mp.osd_message(">>| Skipped to End", 2.0)
    else
        mp.commandv("seek", 85.0, "relative")
        mp.osd_message(">>| Jumped Ending (+85s)", 2.0)
    end
end

local function do_skip_intro_or_outro()
    local cur_t = mp.get_property_number("time-pos", 0) or 0
    if player_ctx.ed_start and player_ctx.ed_end and cur_t >= (player_ctx.ed_start - 2.0) and cur_t < (player_ctx.ed_end - 1.0) then
        do_skip_outro_ed()
    else
        do_skip_intro_op()
    end
end

local function do_next_episode()
    if next_ep_dispatched then return end
    next_ep_dispatched = true
    mp.osd_message(">>| Loading Next Episode...", 2.5)
    -- Distinct script-message name (differs from key-binding names) prevents mpv recursion!
    mp.commandv("script-message", "solus-ipc-next-ep")
end

local function adjust_sub_delay(delta)
    local cur = mp.get_property_number("sub-delay", 0.0) or 0.0
    local nxt = math.max(-120.0, math.min(120.0, cur + delta))
    mp.set_property_number("sub-delay", nxt)
    sync_status_msg = string.format("Manual offset adjusted (%+.2fs)", nxt)
    sync_status_col = "&H24BFFB&"
    mp.osd_message(string.format("◈ Subtitle Delay: %+.2fs", nxt), 2.0)
    if hud_visible then render_info_hud() end
end

local function render_player_buttons()
    active_hitboxes = {}
    local lines = {}

    local function draw_pill(id, x1, y1, x2, y2, label, accent_bgr)
        local is_hov = (hovered_btn_id == id)
        local fill_col = is_hov and accent_bgr or "&H1A1410&"
        local text_col = is_hov and "&H08080A&" or "&HFAF4F0&"
        local border_col = accent_bgr
        table.insert(lines, string.format("{\\an7\\pos(0,0)\\bord0\\shad0\\1c%s\\1a&H28&\\p1}m %d %d l %d %d l %d %d l %d %d{\\p0}", fill_col, x1, y1, x2, y1, x2, y2, x1, y2))
        table.insert(lines, string.format("{\\an7\\pos(0,0)\\bord2\\1a&HFF&\\3c%s\\p1}m %d %d l %d %d l %d %d l %d %d{\\p0}", border_col, x1, y1, x2, y1, x2, y2, x1, y2))
        local cx = math.floor((x1 + x2) / 2)
        local cy = math.floor((y1 + y2) / 2)
        table.insert(lines, string.format("{\\an5\\pos(%d,%d)\\bord0\\shad0\\fs16\\b1\\1c%s}%s", cx, cy, text_col, label))
        table.insert(active_hitboxes, { id = id, x1 = x1, y1 = y1, x2 = x2, y2 = y2 })
    end

    -- Embedded inside Solus Info HUD ([i]/[I]) or Sub Sync ([S]) — zero random auto popups!
    if sub_bar_open or hud_visible then
        local delay = mp.get_property_number("sub-delay", 0.0) or 0.0
        local sy1 = hud_visible and 388 or 486
        local sy2 = sy1 + 92
        local py1 = sy1 + 10
        local py2 = sy1 + 46
        table.insert(lines, string.format("{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H100C09&\\1a&H14&\\p1}m 42 %d l 1238 %d l 1238 %d l 42 %d{\\p0}", sy1, sy1, sy2, sy2))
        table.insert(lines, string.format("{\\an7\\pos(0,0)\\bord2\\1a&HFF&\\3c&H24BFFB&\\p1}m 42 %d l 1238 %d l 1238 %d l 42 %d{\\p0}", sy1, sy1, sy2, sy2))
        table.insert(lines, string.format("{\\an7\\pos(62,%d)\\bord0\\shad0\\fs16\\b1\\1c&H24BFFB&}SUB SYNC %+.2fs", sy1 + 19, delay))
        draw_pill("sub_m500", 214, py1, 274, py2, "-5s", "&H24BFFB&")
        draw_pill("sub_m100", 280, py1, 340, py2, "-1s", "&H24BFFB&")
        draw_pill("sub_m20",  346, py1, 414, py2, "-0.2s", "&H24BFFB&")
        local snap_lbl = sync_in_progress and "◈ ANALYZING AUDIO..." or "◈ AUTO-SYNC VOICE [y]"
        local snap_col = sync_in_progress and "&HF8BD38&" or "&H99D334&"
        draw_pill("sub_snap", 422, py1, 676, py2, snap_lbl, snap_col)
        draw_pill("sub_p20",  684, py1, 752, py2, "+0.2s", "&H24BFFB&")
        draw_pill("sub_p100", 758, py1, 818, py2, "+1s", "&H24BFFB&")
        draw_pill("sub_p500", 824, py1, 884, py2, "+5s", "&H24BFFB&")
        draw_pill("sub_rst",  894, py1, 974, py2, "Reset", "&HFA8BA7&")
        draw_pill("sub_cls",  982, py1, 1028, py2, "X", "&H5E3FF4&")

        local cue_preview = find_active_cue_preview(delay)
        local status_txt = utf8_truncate(esc_ass(sync_status_msg), 56)
        table.insert(lines, string.format("{\\an7\\pos(62,%d)\\bord0\\shad0\\fs14\\1c&HFAF4F0&}%s", sy1 + 58, cue_preview))
        table.insert(lines, string.format("{\\an7\\pos(620,%d)\\bord0\\shad0\\fs14\\1c%s}[%s]", sy1 + 58, sync_status_col, status_txt))

        -- Quick Episode Controls bar directly below SUB SYNC section inside HUD
        local cy1 = sy2 + 8
        local cy2 = cy1 + 52
        local cpy1 = cy1 + 8
        local cpy2 = cy2 - 8
        table.insert(lines, string.format("{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H100C09&\\1a&H14&\\p1}m 42 %d l 1238 %d l 1238 %d l 42 %d{\\p0}", cy1, cy1, cy2, cy2))
        table.insert(lines, string.format("{\\an7\\pos(0,0)\\bord2\\1a&HFF&\\3c&HFA8BA7&\\p1}m 42 %d l 1238 %d l 1238 %d l 42 %d{\\p0}", cy1, cy1, cy2, cy2))
        table.insert(lines, string.format("{\\an7\\pos(62,%d)\\bord0\\shad0\\fs15\\b1\\1c&HFA8BA7&}CONTROLS", cy1 + 17))
        draw_pill("ctrl_skip_op", 214, cpy1, 470, cpy2, ">>|  SKIP INTRO  [Tab]", "&HF8BD38&")
        draw_pill("ctrl_skip_ed", 482, cpy1, 728, cpy2, ">>|  SKIP ENDING", "&HF8BD38&")
        if player_ctx.next_ep then
            draw_pill("ctrl_next_ep", 740, cpy1, 1028, cpy2, string.format(">>|  NEXT EPISODE: Ep %d  [n/N]", player_ctx.next_ep), "&HFA8BA7&")
        else
            table.insert(lines, string.format("{\\an7\\pos(750,%d)\\bord0\\shad0\\fs15\\1c&H686058&}Final Episode in Series", cy1 + 17))
        end
    end

    if #lines == 0 then
        btn_ov:remove()
    else
        btn_ov.data = table.concat(lines, "\n")
        btn_ov:update()
    end
end

local function snap_sub_to_voice()
    local cur_t = mp.get_property_number("time-pos", nil)
    if not cur_t then return end
    if sync_in_progress then
        mp.osd_message("◈ Speech-VAD sync already analyzing audio...", 1.8)
        return
    end
    sync_in_progress = true
    sync_status_msg = "Analyzing 4 speech scenes & cadence (-150s..+150s)..."
    sync_status_col = "&HF8BD38&"
    mp.osd_message("◈ Analyzing voice cadence & sync (-150s..+150s)... please wait", 3.5)
    mp.commandv("script-message", "solus-ipc-auto-sync-sub", string.format("%.2f", cur_t))
    render_player_buttons()
end

local function handle_btn_click()
    local id = hovered_btn_id
    if id == "ctrl_skip_op" then
        do_skip_intro_op()
    elseif id == "ctrl_skip_ed" then
        do_skip_outro_ed()
    elseif id == "ctrl_next_ep" then
        do_next_episode()
    elseif id == "sub_m500" then
        adjust_sub_delay(-5.0)
    elseif id == "sub_m100" then
        adjust_sub_delay(-1.0)
    elseif id == "sub_m20" then
        adjust_sub_delay(-0.2)
    elseif id == "sub_snap" then
        snap_sub_to_voice()
    elseif id == "sub_p20" then
        adjust_sub_delay(0.2)
    elseif id == "sub_p100" then
        adjust_sub_delay(1.0)
    elseif id == "sub_p500" then
        adjust_sub_delay(5.0)
    elseif id == "sub_rst" then
        mp.set_property_number("sub-delay", 0.0)
        sync_status_msg = "Delay reset to 0.00s"
        sync_status_col = "&HFA8BA7&"
        mp.osd_message("◈ Subtitle Delay Reset: 0.00s", 1.8)
        if hud_visible then render_info_hud() end
    elseif id == "sub_cls" then
        sub_bar_open = false
        if hud_visible then hide_info_hud() end
    end
    render_player_buttons()
end

mp.observe_property("mouse-pos", "native", function(_, mpos)
    if not mpos or not mpos.hover then
        if hovered_btn_id ~= nil then
            hovered_btn_id = nil
            mp.remove_key_binding("solus-btn-click")
            render_player_buttons()
        end
        return
    end
    hover_active_until = mp.get_time() + 3.5
    local dim = mp.get_property_native("osd-dimensions")
    if not dim or (dim.w or 0) <= 0 or (dim.h or 0) <= 0 then return end
    local vx = (mpos.x / dim.w) * 1280.0
    local vy = (mpos.y / dim.h) * 720.0
    local new_hov = nil
    for _, box in ipairs(active_hitboxes) do
        if vx >= box.x1 and vx <= box.x2 and vy >= box.y1 and vy <= box.y2 then
            new_hov = box.id
            break
        end
    end
    if new_hov ~= hovered_btn_id then
        hovered_btn_id = new_hov
        if hovered_btn_id ~= nil then
            mp.add_forced_key_binding("MBTN_LEFT", "solus-btn-click", handle_btn_click)
        else
            mp.remove_key_binding("solus-btn-click")
        end
    end
    render_player_buttons()
end)

mp.register_script_message("solus-sync-status", function(json_str)
    sync_in_progress = false
    local parsed = utils.parse_json(json_str or "")
    if type(parsed) == "table" then
        if parsed.ok and parsed.delay_secs ~= nil then
            mp.set_property_number("sub-delay", parsed.delay_secs)
            sync_status_col = "&H99D334&"
        else
            sync_status_col = "&H5E3FF4&"
        end
        if parsed.message and parsed.message ~= "" then
            sync_status_msg = parsed.message
            mp.osd_message("◈ " .. tostring(parsed.message), 3.2)
        end
        if hud_visible then render_info_hud() end
        render_player_buttons()
    end
end)

mp.register_script_message("solus-player-ctx", function(json_str)
    local parsed = utils.parse_json(json_str or "")
    if type(parsed) == "table" then
        player_ctx.ep = parsed.ep or player_ctx.ep
        player_ctx.next_ep = parsed.next_ep
        player_ctx.op_start = parsed.op_start
        player_ctx.op_end = parsed.op_end
        player_ctx.ed_start = parsed.ed_start
        player_ctx.ed_end = parsed.ed_end
        if parsed.sub_path and parsed.sub_path ~= "" then
            player_ctx.sub_path = parsed.sub_path
        end
        render_player_buttons()
    end
end)

mp.add_periodic_timer(0.5, render_player_buttons)

local function toggle_sub_bar()
    sub_bar_open = not sub_bar_open
    render_player_buttons()
end

mp.add_forced_key_binding("TAB", "solus-skip-intro", do_skip_intro_or_outro)
mp.add_forced_key_binding("n", "solus-next-ep-key-lower", do_next_episode)
mp.add_forced_key_binding("N", "solus-next-ep-key", do_next_episode)
mp.add_forced_key_binding("S", "solus-toggle-sub-bar", toggle_sub_bar)
mp.add_forced_key_binding("y", "solus-snap-sub-voice", snap_sub_to_voice)
"##;

pub fn ensure_solus_audio_lua() -> Option<PathBuf> {
    // Pre-warm all 9 Hardsub Eradicator GLSL presets so runtime [h]/[H] cycling is instant.
    for slug in ["blur", "inpaint", "matte"] {
        for band in [
            HardsubBandHeight::Compact,
            HardsubBandHeight::Standard,
            HardsubBandHeight::Tall,
        ] {
            let (top, bot) = band.bounds();
            let _ = crate::services::shaders::ensure_hardsub_shader_file(slug, top, bot);
        }
    }
    let dir = crate::config::Config::cache_dir()
        .unwrap_or_else(|_| env::temp_dir().join("solus-cache"));
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let shader_dir_str = crate::services::shaders::get_shaders_dir()
        .to_string_lossy()
        .replace('\\', "/");
    let rendered_lua = SOLUS_AUDIO_LUA.replace("__SOLUS_HS_SHADER_DIR__", &shader_dir_str);
    let path = dir.join("solus-audio.lua");
    let needs_write = std::fs::read_to_string(&path)
        .map(|cur| cur != rendered_lua)
        .unwrap_or(true);
    if needs_write && std::fs::write(&path, &rendered_lua).is_err() {
        return None;
    }
    Some(path)
}

/// Network transport + bounded demuxer RAM guard flags (no leading `--`).
pub fn mpv_network_kv_args(
    mode: NetworkMode,
    force_ipv4: bool,
    is_local_file: bool,
) -> Vec<String> {
    let mut kv = vec![
        "demuxer-max-bytes=75497472".to_string(),      // 72 MiB hard RAM cap
        "demuxer-max-back-bytes=25165824".to_string(), // 24 MiB rewind cushion
    ];
    if is_local_file {
        return kv;
    }
    kv.extend([
        "stream-lavf-o=reconnect=1,reconnect_streamed=1,reconnect_on_network_error=1,reconnect_delay_max=3,multiple_requests=1".to_string(),
        "demuxer-lavf-o=http_persistent=1,http_multiple=1,allowed_extensions=ALL,allowed_segment_extensions=ALL".to_string(),
        format!("hls-bitrate={}", mode.hls_bitrate_cap()),
        format!("demuxer-readahead-secs={}", mode.vod_readahead_secs()),
        "cache-pause-initial=yes".to_string(),
        format!("cache-pause-wait={:.1}", mode.vod_cache_pause_wait_secs()),
        format!("network-timeout={}", mode.network_timeout_secs()),
    ]);
    if mode.use_disk_cache() {
        kv.push("cache-on-disk=yes".to_string());
        if let Ok(cache_root) = crate::config::Config::cache_dir() {
            let mpv_cache = cache_root.join("mpv-cache");
            let _ = std::fs::create_dir_all(&mpv_cache);
            kv.push(format!("demuxer-cache-dir={}", mpv_cache.display()));
        }
    }
    if force_ipv4 {
        kv.push("ytdl-raw-options-append=force-ipv4=".to_string());
    }
    kv
}

/// Legacy `--`-prefixed wrapper preserved for network diagnostics & tests.
#[allow(dead_code)]
pub fn mpv_network_args(mode: NetworkMode, force_ipv4: bool, is_local_file: bool) -> Vec<String> {
    mpv_network_kv_args(mode, force_ipv4, is_local_file)
        .into_iter()
        .map(|s| format!("--{s}"))
        .collect()
}

/// Hardening flags for direct progressive HTTP MP4 origins (e.g., AnimeHeaven / AnimeGG)
/// that emit corrupt H.264 NAL warnings or stall under pipelined range requests (`http_multiple=1`).
pub fn mpv_direct_mp4_resilience_kv_args(direct_url: &str) -> Vec<String> {
    let lower = direct_url.to_ascii_lowercase();
    let is_http = lower.starts_with("http://") || lower.starts_with("https://");
    let is_hls = lower.contains(".m3u8") || lower.contains("/m3u8");
    if !is_http || is_hls {
        return Vec::new();
    }
    vec![
        "demuxer-lavf-o=http_persistent=1,allowed_extensions=ALL,allowed_segment_extensions=ALL".to_string(),
        "demuxer-lavf-hacks=yes".to_string(),
        "vd-lavc-dr=no".to_string(),
    ]
}

/// Single source of truth for all `libmpv` players (`mpv`, `mpv.net`, `Celluloid`, `Auto-Best`).
/// Returns `(global_kv_flags, item_kv_flags)`.
pub fn shared_mpv_kv_flags(
    opts: &PlayOptions,
    direct_url: &str,
    youtube_conf: Option<&Path>,
    audio_script: Option<&Path>,
) -> (Vec<String>, Vec<String>) {
    let is_local_file = !(direct_url.starts_with("http://") || direct_url.starts_with("https://"));
    let dev = detect_device_profile();
    let caps = detect_player_capabilities();

    let ref_url = opts
        .referer
        .as_deref()
        .filter(|r| !r.trim().is_empty())
        .unwrap_or("https://zokoanime.video/");

    let mut global_kv = vec![
        "ytdl=no".to_string(),
        "force-window=immediate".to_string(),
        "keep-open=no".to_string(),
        format!("referrer={ref_url}"),
        "input-terminal=no".to_string(),
    ];

    if opts.auto_pip {
        global_kv.push("fullscreen=no".to_string());
        global_kv.push("ontop=yes".to_string());
        global_kv.push("border=no".to_string());
        global_kv.push("keepaspect-window=yes".to_string());
        global_kv.push("window-scale=0.30".to_string());
        global_kv.push("geometry=30%x30%-24-24".to_string());
        global_kv.push("script-opts=solus_auto_pip=1".to_string());
    } else if opts.auto_fullscreen {
        global_kv.push("fullscreen=yes".to_string());
    }

    global_kv.extend(mpv_network_kv_args(opts.network_mode, opts.force_ipv4, is_local_file));
    global_kv.extend(mpv_direct_mp4_resilience_kv_args(direct_url));

    let has_shaders = opts.glsl_shaders.as_deref().is_some_and(|s| !s.is_empty());
    global_kv.extend(mpv_mastering_kv_flags(
        opts.shader_mode,
        opts.performance_mode,
        opts.deband_mode,
        opts.motion_mode,
        has_shaders,
        dev,
        caps,
    ));

    if let Some(conf_path) = youtube_conf {
        global_kv.push(format!("input-conf={}", conf_path.display()));
    }
    if let Some(script_path) = audio_script {
        global_kv.push(format!("script={}", script_path.display()));
    }
    if let Some(ref sock) = opts.ipc_socket {
        global_kv.push(format!("input-ipc-server={sock}"));
    }

    // Per-item options
    let mut item_kv = vec![format!("force-media-title={} Episode {}", opts.title, opts.episode)];
    let subs_disabled = opts.subtitles_disabled
        || opts
            .sub_lang
            .as_deref()
            .is_some_and(|l| l.eq_ignore_ascii_case("none"));
    // Never render a second container/HLS soft-embedded subtitle simultaneously.
    item_kv.push("secondary-sid=no".to_string());
    item_kv.push("sub-auto=no".to_string());
    if subs_disabled {
        item_kv.push("sid=no".to_string());
        item_kv.push("sub-visibility=no".to_string());
    } else if let Some(ref url) = opts.sub_url {
        if !url.is_empty() {
            item_kv.push(format!("sub-file={url}"));
            item_kv.push("sub-visibility=yes".to_string());
        }
    }
    if opts.sub_delay_ms != 0 {
        let delay_secs = opts.sub_delay_ms as f64 / 1000.0;
        item_kv.push(format!("sub-delay={delay_secs:.3}"));
    }
    if !opts.audio_urls.is_empty() {
        for aurl in &opts.audio_urls {
            if !aurl.trim().is_empty() {
                item_kv.push(format!("audio-file={aurl}"));
            }
        }
    } else if opts.has_demuxed_audio {
        if let Some(master_url) = derive_master_playlist_url(direct_url) {
            item_kv.push(format!("audio-file={master_url}"));
        }
    }
    item_kv.push("audio-file-auto=all".to_string());
    if opts.dub {
        item_kv.push("alang=en,eng,English".to_string());
    } else {
        item_kv.push("alang=ja,jpn,Japanese".to_string());
    }
    if let Some(pos) = opts.start_pos {
        item_kv.push(format!("start={pos}"));
    }
    if let Some(ref shaders) = opts.glsl_shaders {
        item_kv.push(format!("glsl-shaders={shaders}"));
    }
    if let Some(ref style) = opts.sub_style {
        for flag in style.to_mpv_flags() {
            item_kv.push(flag.trim_start_matches("--").to_string());
        }
    }
    let wants_non_english = opts.sub_lang.as_deref().is_some_and(|l| {
        let low = l.trim().to_ascii_lowercase();
        !low.is_empty() && low != "none" && low != "off" && !low.starts_with("eng") && low != "en"
    });
    if !subs_disabled && opts.embedded_sub.has_baked_english() && wants_non_english {
        if let Some(slug) = opts.hardsub_shield.shader_slug() {
            let (top, bot) = opts.hardsub_band.bounds();
            let (sub_pos, margin_y) = opts.hardsub_band.sub_alignment();
            if let Some(hs_path) = crate::services::shaders::ensure_hardsub_shader_file(slug, top, bot) {
                let hs_str = hs_path.to_string_lossy().into_owned();
                if let Some(existing) = item_kv.iter_mut().find(|k| k.starts_with("glsl-shaders=")) {
                    existing.push_str(crate::services::shaders::shader_path_separator());
                    existing.push_str(&hs_str);
                } else {
                    item_kv.push(format!("glsl-shaders={hs_str}"));
                }
            }
            item_kv.push("sub-use-margins=no".to_string());
            item_kv.push("sub-ass-force-margins=no".to_string());
            item_kv.push("sub-border-style=outline-and-shadow".to_string());
            item_kv.push("sub-border-size=2.6".to_string());
            item_kv.push("sub-shadow-offset=1.0".to_string());
            item_kv.push(format!("sub-margin-y={margin_y}"));
            item_kv.push(format!("sub-pos={sub_pos}"));
            item_kv.push("sub-ass-override=force".to_string());
        } else {
            match opts.hardsub_shield {
                HardsubShieldMode::FrameLift => {
                    item_kv.push("video-zoom=-0.16".to_string());
                    item_kv.push("video-align-y=-1.0".to_string());
                    item_kv.push("sub-use-margins=yes".to_string());
                    item_kv.push("sub-ass-force-margins=yes".to_string());
                    item_kv.push("sub-border-style=outline-and-shadow".to_string());
                    item_kv.push("sub-border-size=2.4".to_string());
                    item_kv.push("sub-shadow-offset=1.0".to_string());
                    item_kv.push("sub-margin-y=6".to_string());
                    item_kv.push("sub-pos=99".to_string());
                    item_kv.push("sub-ass-override=force".to_string());
                }
                HardsubShieldMode::TopEdge => {
                    item_kv.push("sub-use-margins=no".to_string());
                    item_kv.push("sub-ass-force-margins=no".to_string());
                    item_kv.push("sub-border-style=outline-and-shadow".to_string());
                    item_kv.push("sub-border-size=2.6".to_string());
                    item_kv.push("sub-shadow-offset=1.0".to_string());
                    item_kv.push("sub-margin-y=18".to_string());
                    item_kv.push("sub-pos=7".to_string());
                    item_kv.push("sub-ass-override=force".to_string());
                }
                HardsubShieldMode::FrostedBlur
                | HardsubShieldMode::SmartInpaint
                | HardsubShieldMode::TotalMatte
                | HardsubShieldMode::Off => {}
            }
        }
    }

    (global_kv, item_kv)
}

/// Build direct `mpv` arguments (`--<kv>`), wrapping stream items with `--{ ... --}` when Y2K art precedes it.
#[cfg_attr(not(test), allow(dead_code))]
pub fn build_mpv_direct_args(
    opts: &PlayOptions,
    direct_url: &str,
    youtube_conf: Option<&Path>,
) -> Vec<String> {
    build_mpv_direct_args_with_script(opts, direct_url, youtube_conf, None)
}

pub fn build_mpv_direct_args_with_script(
    opts: &PlayOptions,
    direct_url: &str,
    youtube_conf: Option<&Path>,
    audio_script: Option<&Path>,
) -> Vec<String> {
    let (global_kv, item_kv) = shared_mpv_kv_flags(opts, direct_url, youtube_conf, audio_script);
    let mut args: Vec<String> = global_kv.into_iter().map(|kv| format!("--{kv}")).collect();

    let has_loading = opts
        .loading_image
        .as_deref()
        .is_some_and(|p| !p.trim().is_empty());

    if has_loading {
        args.push("--image-display-duration=4".to_string());
        args.push("--prefetch-playlist=yes".to_string());
        args.push("--loop-playlist=no".to_string());
        if let Some(ref loading_img) = opts.loading_image {
            args.push("--{".to_string());
            args.push("--osc=no".to_string());
            args.push("--osd-bar=no".to_string());
            args.push(loading_img.clone());
            args.push("--}".to_string());
        }
        args.push("--{".to_string());
    }

    args.extend(item_kv.into_iter().map(|kv| format!("--{kv}")));
    args.push(direct_url.to_string());
    if has_loading {
        args.push("--}".to_string());
    }
    args
}

/// Build direct `mpv.net` (Windows native GUI `libmpv` shell) arguments with identical flag & IPC parity.
pub fn build_mpvnet_direct_args(
    opts: &PlayOptions,
    direct_url: &str,
    youtube_conf: Option<&Path>,
    audio_script: Option<&Path>,
) -> Vec<String> {
    build_mpv_direct_args_with_script(opts, direct_url, youtube_conf, audio_script)
}

/// Build direct `Celluloid` (Linux GTK4/Wayland `libmpv` shell) arguments translating all shared flags via `--mpv-<kv>`.
pub fn build_celluloid_direct_args(
    opts: &PlayOptions,
    direct_url: &str,
    youtube_conf: Option<&Path>,
    audio_script: Option<&Path>,
) -> Vec<String> {
    let (global_kv, item_kv) = shared_mpv_kv_flags(opts, direct_url, youtube_conf, audio_script);
    let mut args = vec!["--new-window".to_string()];
    args.extend(global_kv.into_iter().map(|kv| format!("--mpv-{kv}")));
    args.extend(item_kv.into_iter().map(|kv| format!("--mpv-{kv}")));
    args.push(direct_url.to_string());
    args
}

/// Spawn unified `libmpv` playback (`Auto-Best`, `mpv`, `mpv.net`, `Celluloid`) or `ani-cli` fallback.
pub fn spawn_async(opts: &PlayOptions) -> Result<Child> {
    let (effective_player, player_bin) = resolve_engine_executable(&opts.player);
    let yt_conf = ensure_youtube_input_conf();
    let audio_script = ensure_solus_audio_lua();

    if let Some(ref direct_url) = opts.stream_url {
        if !direct_url.is_empty() {
            let mut cmd = Command::new(&player_bin);
            let args = match effective_player {
                Player::Celluloid => build_celluloid_direct_args(
                    opts,
                    direct_url,
                    yt_conf.as_deref(),
                    audio_script.as_deref(),
                ),
                Player::MpvNet => build_mpvnet_direct_args(
                    opts,
                    direct_url,
                    yt_conf.as_deref(),
                    audio_script.as_deref(),
                ),
                Player::Mpv | Player::AutoBest => build_mpv_direct_args_with_script(
                    opts,
                    direct_url,
                    yt_conf.as_deref(),
                    audio_script.as_deref(),
                ),
            };
            cmd.args(args);

            let mut current_paths = match env::var_os("PATH") {
                Some(val) => env::split_paths(&val).collect::<Vec<_>>(),
                None => Vec::new(),
            };
            #[cfg(target_os = "macos")]
            {
                current_paths.push(PathBuf::from("/opt/homebrew/bin"));
                current_paths.push(PathBuf::from("/usr/local/bin"));
            }
            #[cfg(windows)]
            {
                if let Ok(local_app) = env::var("LOCALAPPDATA") {
                    let local = PathBuf::from(local_app);
                    current_paths.push(local.join(r"Microsoft\WinGet\Links"));
                    current_paths.push(local.join(r"Programs\mpv"));
                    current_paths.push(local.join(r"Programs\mpv.net"));
                }
                if let Ok(user_profile) = env::var("USERPROFILE") {
                    current_paths.push(PathBuf::from(user_profile).join(r"scoop\shims"));
                }
            }
            if let Ok(new_path) = env::join_paths(current_paths) {
                cmd.env("PATH", new_path);
            }

            return cmd
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| AppError::Player(format!("Failed to spawn {}: {e}", effective_player.label())));
        }
    }

    // Legacy `ani-cli` fallback for external providers
    let args = build_args(opts);
    let wrapper_path = ensure_player_wrapper(effective_player, &player_bin)?;
    let bin_dir = wrapper_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| env::temp_dir().join("solus-bin"));
    let wrapper_name = wrapper_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "solus-mpv".to_string());

    #[cfg(windows)]
    let mut cmd = {
        if let Some(target) = find_windows_ani_cli() {
            let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat") {
                let mut c = Command::new("cmd");
                c.arg("/C").arg(target);
                c
            } else {
                Command::new(target)
            }
        } else {
            return Err(AppError::Player(
                "ani-cli was not found. Install it or select AllAnime/Hianime for direct streaming.".to_string(),
            ));
        }
    };
    #[cfg(not(windows))]
    let mut cmd = Command::new("ani-cli");

    let mut current_paths = match env::var_os("PATH") {
        Some(val) => env::split_paths(&val).collect::<Vec<_>>(),
        None => Vec::new(),
    };
    #[cfg(not(windows))]
    current_paths.insert(0, bin_dir);
    if let Ok(new_path) = env::join_paths(current_paths) {
        cmd.env("PATH", new_path);
    }

    cmd.args(&args)
        .env("ANI_CLI_EXIT_AFTER_PLAY", "1")
        .env("ANI_CLI_NO_DETACH", "1");

    #[cfg(windows)]
    cmd.env("ANI_CLI_PLAYER", &player_bin);
    #[cfg(not(windows))]
    {
        cmd.env("ANI_CLI_PLAYER", &wrapper_name);
        cmd.env("SOLUS_REAL_PLAYER", &player_bin);
    }

    if let Some(ref sock) = opts.ipc_socket {
        cmd.env("SOLUS_IPC_SOCKET", sock);
    }
    if let Some(ref script_path) = audio_script {
        cmd.env("SOLUS_AUDIO_SCRIPT", script_path);
    }
    if let Some(pos) = opts.start_pos {
        cmd.env("SOLUS_START_POS", pos.to_string());
    }
    if let Some(ref shaders) = opts.glsl_shaders {
        cmd.env("SOLUS_SHADERS", shaders);
    }
    if let Some(ref lang) = opts.sub_lang {
        if !lang.is_empty() {
            cmd.env("ANI_CLI_SUB_LANG", lang);
        }
    }
    if let Some(ref url) = opts.sub_url {
        if !url.is_empty() {
            cmd.env("SOLUS_SUB_URL", url);
        }
    }

    let (global_kv, item_kv) = shared_mpv_kv_flags(opts, "", yt_conf.as_deref(), audio_script.as_deref());
    let player_flags: Vec<String> = global_kv
        .into_iter()
        .chain(item_kv)
        .filter(|kv| !kv.contains(' '))
        .map(|kv| format!("--{kv}"))
        .collect();
    cmd.env("ANI_CLI_PLAYER_FLAGS", player_flags.join(" "));

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Player(format!("Failed to spawn ani-cli: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(title: &str, episode: u32, quality: &str, dub: bool) -> PlayOptions {
        PlayOptions {
            title: title.to_string(),
            episode,
            quality: quality.to_string(),
            dub,
            player: "auto-best".to_string(),
            sub_lang: None,
            sub_url: None,
            stream_url: None,
            ipc_socket: None,
            start_pos: None,
            glsl_shaders: None,
            sub_style: None,
            loading_image: None,
            sub_delay_ms: 0,
            subtitles_disabled: false,
            network_mode: NetworkMode::Normal,
            force_ipv4: false,
            auto_fullscreen: false,
            auto_pip: false,
            shader_mode: Anime4KMode::Balanced,
            performance_mode: PerformanceMode::Balanced,
            deband_mode: DebandMode::Auto,
            motion_mode: MotionMode::Off,
            referer: None,
            audio_urls: Vec::new(),
            has_demuxed_audio: false,
            embedded_sub: crate::api::subtitles::EmbeddedSubtitleKind::None,
            hardsub_shield: HardsubShieldMode::FrostedBlur,
            hardsub_band: HardsubBandHeight::Standard,
        }
    }

    #[test]
    fn test_resolve_auto_player_matrix() {
        let win_only_net = DependencyStatus {
            has_ani_cli: false,
            has_mpv: false,
            has_mpvnet: true,
            has_celluloid: false,
            has_ffmpeg: false,
        };
        assert_eq!(resolve_auto_player_with_deps(Player::AutoBest, &win_only_net), Player::MpvNet);

        let linux_cell = DependencyStatus {
            has_ani_cli: false,
            has_mpv: false,
            has_mpvnet: false,
            has_celluloid: true,
            has_ffmpeg: false,
        };
        assert_eq!(resolve_auto_player_with_deps(Player::AutoBest, &linux_cell), Player::Celluloid);

        let all_installed = DependencyStatus {
            has_ani_cli: true,
            has_mpv: true,
            has_mpvnet: true,
            has_celluloid: true,
            has_ffmpeg: true,
        };
        assert_eq!(resolve_auto_player_with_deps(Player::AutoBest, &all_installed), Player::Mpv);
    }

    #[test]
    fn test_mpv_mpvnet_celluloid_full_customization_parity() {
        let mut o = opts("Frieren", 1, "1080p", false);
        o.glsl_shaders = Some("/shaders/01.glsl:/shaders/02.glsl".to_string());
        o.auto_pip = true;
        o.motion_mode = MotionMode::Cadence;
        o.ipc_socket = Some("/tmp/solus-mpv.sock".to_string());

        let mpv_args = build_mpv_direct_args(&o, "https://cdn.example.com/index.m3u8", None);
        let mpvnet_args = build_mpvnet_direct_args(&o, "https://cdn.example.com/index.m3u8", None, None);
        let celluloid_args = build_celluloid_direct_args(&o, "https://cdn.example.com/index.m3u8", None, None);

        assert_eq!(mpv_args, mpvnet_args);
        assert!(mpv_args.contains(&"--glsl-shaders=/shaders/01.glsl:/shaders/02.glsl".to_string()));
        assert!(mpv_args.contains(&"--cscale=bilinear".to_string()));
        assert!(mpv_args.contains(&"--deband=yes".to_string()));
        assert!(mpv_args.contains(&"--tscale=oversample".to_string()));
        assert!(mpv_args.contains(&"--demuxer-max-bytes=75497472".to_string()));

        assert!(celluloid_args.contains(&"--new-window".to_string()));
        assert!(celluloid_args.contains(&"--mpv-glsl-shaders=/shaders/01.glsl:/shaders/02.glsl".to_string()));
        assert!(celluloid_args.contains(&"--mpv-input-ipc-server=/tmp/solus-mpv.sock".to_string()));
        assert!(celluloid_args.contains(&"--mpv-tscale=oversample".to_string()));
    }

    #[test]
    fn test_build_args_and_master_playlist() {
        let args = build_args(&opts("One Piece", 5, "720p", true));
        assert!(args.contains(&"--dub".to_string()));
        assert_eq!(
            derive_master_playlist_url("https://cdn.example.com/hls/v1/1080/index.m3u8"),
            Some("https://cdn.example.com/hls/v1/master.m3u8".to_string())
        );
    }

    #[test]
    fn test_ensure_lua_and_input_conf_contain_mastering_hotkeys() {
        let yt_path = ensure_youtube_input_conf().unwrap();
        let yt_text = std::fs::read_to_string(yt_path).unwrap();
        assert!(yt_text.contains("a script-binding solus-toggle-shaders"));
        assert!(yt_text.contains("d script-binding solus-toggle-deband"));
        assert!(yt_text.contains("m script-binding solus-cycle-motion"));
        assert!(yt_text.contains("g script-binding solus-cycle-perf"));

        assert!(yt_text.contains("n script-binding solus-next-ep-key-lower"));
        assert!(yt_text.contains("N script-binding solus-next-ep-key"));

        let lua_path = ensure_solus_audio_lua().unwrap();
        let lua_text = std::fs::read_to_string(lua_path).unwrap();
        assert!(lua_text.contains("solus-toggle-pip"));
        assert!(lua_text.contains("solus-toggle-hud"));
        assert!(lua_text.contains("solus-cycle-motion"));
        assert!(lua_text.contains("solus-skip-intro"));
        assert!(lua_text.contains("solus-next-ep-key"));
        assert!(lua_text.contains("solus-ipc-next-ep"));
        assert!(lua_text.contains("next_ep_dispatched"));
        assert!(!lua_text.contains(r#""script-message", "solus-next-ep-key""#));
        assert!(lua_text.contains("solus-snap-sub-voice"));
        assert!(lua_text.contains("solus-toggle-sub-bar"));
        assert!(lua_text.contains("solus-ipc-auto-sync-sub"));
        assert!(lua_text.contains("solus-sync-status"));
        assert!(!lua_text.contains("⚡"), "Lua ASS canvas must not use missing-glyph lightning bolt");

        // Verify demuxed audio_urls emits explicit --audio-file flags whereas muxed Zoko does not
        let mut o_mux = opts("Chainsmoker Cat", 1, "1080p", false);
        o_mux.has_demuxed_audio = false;
        let mux_args = build_mpv_direct_args(&o_mux, "https://hls.example.com/p/1080/index.m3u8", None);
        assert!(!mux_args.iter().any(|a| a.starts_with("--audio-file=")));
        assert!(mux_args.iter().any(|a| a.contains("allowed_segment_extensions=ALL")));

        let mut o_demux = opts("Chainsmoker Cat", 1, "1080p", false);
        o_demux.has_demuxed_audio = true;
        o_demux.audio_urls = vec![
            "https://fetch.nexabloom.top/a/audio/ja/index.m3u8".to_string(),
            "https://fetch.nexabloom.top/a/audio/en/index.m3u8".to_string(),
        ];
        let demux_args = build_mpv_direct_args(&o_demux, "https://fetch.nexabloom.top/a/index-f1.m3u8", None);
        assert!(demux_args.contains(&"--audio-file=https://fetch.nexabloom.top/a/audio/ja/index.m3u8".to_string()));
        assert!(demux_args.contains(&"--audio-file=https://fetch.nexabloom.top/a/audio/en/index.m3u8".to_string()));

        // Verify strict soft-track isolation and GPU Hardsub Eradicator for Arabic on baked English
        let mut o_ar = opts("Chainsmoker Cat", 1, "900p", false);
        o_ar.sub_lang = Some("Arabic".to_string());
        o_ar.sub_url = Some("/tmp/ar.vtt".to_string());
        o_ar.embedded_sub = crate::api::subtitles::EmbeddedSubtitleKind::HardsubEn;
        o_ar.hardsub_shield = HardsubShieldMode::FrostedBlur;
        o_ar.hardsub_band = HardsubBandHeight::Tall;
        o_ar.glsl_shaders = Some("/tmp/anime4k.glsl".to_string());
        let shield_args = build_mpv_direct_args(&o_ar, "https://vmpx.online/master.m3u8", None);
        assert!(shield_args.contains(&"--secondary-sid=no".to_string()));
        assert!(shield_args.contains(&"--sub-auto=no".to_string()));
        assert!(shield_args.iter().any(|a| a.starts_with("--glsl-shaders=") && a.contains("/tmp/anime4k.glsl") && a.contains("solus_hs_blur_66_100.glsl")));
        assert!(shield_args.contains(&"--sub-use-margins=no".to_string()));
        assert!(shield_args.contains(&"--sub-pos=96".to_string()));
        assert!(shield_args.contains(&"--sub-border-style=outline-and-shadow".to_string()));

        o_ar.hardsub_shield = HardsubShieldMode::FrameLift;
        let lift_args = build_mpv_direct_args(&o_ar, "https://vmpx.online/master.m3u8", None);
        assert!(lift_args.contains(&"--video-zoom=-0.16".to_string()));
        assert!(lift_args.contains(&"--video-align-y=-1.0".to_string()));
        assert!(lift_args.contains(&"--sub-use-margins=yes".to_string()));
        assert!(lua_text.contains("ctrl_skip_op") && lua_text.contains("ctrl_skip_ed") && lua_text.contains("ctrl_next_ep"));
        assert!(lua_text.contains("utf8_truncate"));
    }
}
