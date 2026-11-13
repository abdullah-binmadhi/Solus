//! ani-cli subprocess wrapper.
//! Spawns ani-cli for async playback with log streaming.

use crate::error::{AppError, Result};
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::{Child, Command};

/// Options passed to ani-cli when launching playback.
#[derive(Debug, Clone)]
pub struct PlayOptions {
    /// Title to pass to ani-cli as the search query
    pub title:   String,
    /// Episode number to play
    pub episode: u32,
    /// Quality string e.g. "1080p", "720p", "best"
    pub quality: String,
    /// true = dub, false = sub
    pub dub:     bool,
    /// Preferred player passed through to ani-cli
    pub player:  String,
    /// Subtitle language label (e.g. "Arabic", "Russian", "Hindi", "Indonesian", "English")
    pub sub_lang: Option<String>,
    /// Direct subtitle .vtt URL to pass to player
    pub sub_url:  Option<String>,
    /// Direct verified stream URL (.m3u8) for 100% sync and instant launch
    pub stream_url: Option<String>,
    /// IPC socket path to connect Solus to mpv for live progress tracking and skip cues
    pub ipc_socket: Option<String>,
    /// Playback start position in seconds (for exact resume)
    pub start_pos:  Option<f64>,
    /// Optional Anime4K GLSL shaders argument string (colon-separated)
    pub glsl_shaders: Option<String>,
    /// Optional cinema-grade subtitle styling and collision configuration
    pub sub_style: Option<crate::services::substyler::SubtitleStyleConfig>,
}

/// Build the ani-cli command arguments for a given play request.
/// Pure function — separated from spawn so it can be tested without forking.
pub fn build_args(opts: &PlayOptions) -> Vec<String> {
    let mut args = vec![
        "-S".to_string(),
        "1".to_string(),
        "-e".to_string(),
        opts.episode.to_string(),
        "-q".to_string(),
        opts.quality.clone(),
        "--exit-after-play".to_string(),
    ];

    // Dub flag
    if opts.dub {
        args.push("--dub".to_string());
    }

    // Title as final positional argument.
    // Note: Do NOT pass -L / sub_lang as command-line arguments to ani-cli because
    // ani-cli's argument loop treats unhandled flags as positional search terms,
    // which pollutes the search query (e.g. "-L Arabic <title>") and causes it to
    // select a completely different anime. Subtitle language is configured via the
    // ANI_CLI_SUB_LANG and ANI_CLI_PLAYER_FLAGS environment variables in spawn_async.
    args.push(opts.title.clone());

    args
}

/// Resolve the actual player command to give ani-cli.
///
/// Preference order:
/// - `mpv` stays the default when installed
/// - on macOS, `mpv` falls back to `iina` / `iina-cli` when `mpv` is unavailable
/// - `vlc` remains an explicit cross-platform option
pub fn resolve_player(preferred: &str) -> String {
    match preferred {
        "mpv" => {
            if command_exists("mpv") {
                #[cfg(windows)]
                if let Some(path) = windows_player_path("mpv") {
                    return path;
                }
                "mpv".to_string()
            } else if let Some(iina) = macos_iina_fallback() {
                iina
            } else if let Some(vlc) = macos_vlc_fallback() {
                vlc
            } else {
                "mpv".to_string()
            }
        }
        "iina" => macos_iina_fallback().unwrap_or_else(|| "iina".to_string()),
        "vlc" => {
            if let Some(vlc) = macos_vlc_fallback() {
                return vlc;
            }
            #[cfg(windows)]
            if let Some(path) = windows_player_path("vlc") {
                return path;
            }
            "vlc".to_string()
        }
        other => other.to_string(),
    }
}

/// Detect whether the relevant runtime dependencies are available on this machine.
pub fn detect_dependencies() -> (bool, bool, bool, bool) {
    let has_vlc = {
        #[cfg(target_os = "macos")]
        {
            command_exists("vlc") || macos_vlc_fallback().is_some()
        }
        #[cfg(windows)]
        {
            command_exists("vlc") || windows_player_path("vlc").is_some()
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            command_exists("vlc")
        }
    };
    (
        command_exists("ani-cli"),
        command_exists("mpv"),
        macos_iina_fallback().is_some(),
        has_vlc,
    )
}

fn command_exists(name: &str) -> bool {
    if let Some(path) = env::var_os("PATH") {
        let found = env::split_paths(&path).any(|dir| {
            #[cfg(unix)]
            {
                executable_exists(&dir.join(name))
            }
            #[cfg(windows)]
            {
                executable_exists(&dir.join(format!("{}.exe", name)))
                    || executable_exists(&dir.join(format!("{}.cmd", name)))
                    || executable_exists(&dir.join(format!("{}.bat", name)))
            }
            #[cfg(not(any(unix, windows)))]
            {
                executable_exists(&dir.join(name))
            }
        });
        if found {
            return true;
        }
    }

    #[cfg(windows)]
    {
        if windows_fallback_exists(name) {
            return true;
        }
    }

    false
}

#[cfg(windows)]
fn windows_player_path(name: &str) -> Option<String> {
    let candidates: Vec<PathBuf> = match name {
        "vlc" => vec![
            PathBuf::from(r"C:\Program Files\VideoLAN\VLC\vlc.exe"),
            PathBuf::from(r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe"),
        ],
        "mpv" => {
            let mut list = Vec::new();
            if let Ok(local_app) = env::var("LOCALAPPDATA") {
                let local = PathBuf::from(local_app);
                list.push(local.join(r"Microsoft\WinGet\Links\mpv.exe"));
                list.push(local.join(r"Programs\mpv\mpv.exe"));
            }
            if let Ok(user_profile) = env::var("USERPROFILE") {
                let user = PathBuf::from(user_profile);
                list.push(user.join(r"scoop\shims\mpv.exe"));
                list.push(user.join(r"scoop\apps\mpv\current\mpv.exe"));
            }
            list.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin\mpv.exe"));
            list
        }
        _ => Vec::new(),
    };

    for path in candidates {
        if executable_exists(&path) {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}

#[cfg(windows)]
fn find_windows_ani_cli() -> Option<PathBuf> {
    // 1. Search PATH entries for .cmd, .bat, or .exe
    if let Some(path) = env::var_os("PATH") {
        for dir in env::split_paths(&path) {
            for ext in &["cmd", "bat", "exe"] {
                let candidate = dir.join(format!("ani-cli.{}", ext));
                if executable_exists(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    // 2. WinGet links directory (%LOCALAPPDATA%\Microsoft\WinGet\Links)
    if let Ok(local_app) = env::var("LOCALAPPDATA") {
        let local = PathBuf::from(local_app);
        for sub in &[
            r"Microsoft\WinGet\Links\ani-cli.cmd",
            r"Microsoft\WinGet\Links\ani-cli.bat",
            r"Microsoft\WinGet\Links\ani-cli.exe",
        ] {
            let p = local.join(sub);
            if executable_exists(&p) {
                return Some(p);
            }
        }
    }

    // 3. Scoop shims directory (%USERPROFILE%\scoop\shims)
    if let Ok(user_profile) = env::var("USERPROFILE") {
        let user = PathBuf::from(user_profile);
        for sub in &[
            r"scoop\shims\ani-cli.cmd",
            r"scoop\shims\ani-cli.bat",
            r"scoop\shims\ani-cli.exe",
        ] {
            let p = user.join(sub);
            if executable_exists(&p) {
                return Some(p);
            }
        }
    }

    // 4. Chocolatey directory
    let choco_p = PathBuf::from(r"C:\ProgramData\chocolatey\bin\ani-cli.bat");
    if executable_exists(&choco_p) {
        return Some(choco_p);
    }

    None
}

#[cfg(windows)]
fn windows_fallback_exists(name: &str) -> bool {
    if windows_player_path(name).is_some() {
        return true;
    }
    if name == "ani-cli" {
        return find_windows_ani_cli().is_some();
    }
    false
}

fn executable_exists(path: &Path) -> bool {
    path.is_file()
}

fn macos_iina_fallback() -> Option<String> {
    if env::consts::OS != "macos" {
        return None;
    }

    let app_path = PathBuf::from("/Applications/IINA.app/Contents/MacOS/iina-cli");
    if executable_exists(&app_path) {
        return Some(app_path.to_string_lossy().into_owned());
    }

    if command_exists("iina") {
        return Some("iina".to_string());
    }

    None
}

fn macos_vlc_fallback() -> Option<String> {
    if env::consts::OS != "macos" {
        return None;
    }

    let app_path = PathBuf::from("/Applications/VLC.app/Contents/MacOS/VLC");
    if executable_exists(&app_path) {
        return Some(app_path.to_string_lossy().into_owned());
    }

    if command_exists("vlc") {
        return Some("vlc".to_string());
    }

    None
}

/// Ensure the Solus player wrapper script exists, is executable, and return its path.
/// The wrapper intercepts ani-cli's invocation to:
/// 1. Inject `--audio-file` with the master playlist when HLS multi-audio split streams are detected (e.g. Cyberpunk Edgerunners).
/// 2. Clear and set `--sub-file` so ani-cli's English fallback never overrides the user's chosen subtitle track.
/// 3. Match audio track selection with the user's Dub / Sub preference.
pub fn ensure_player_wrapper(real_player: &str) -> Result<PathBuf> {
    let wrapper_name = if real_player.contains("iina") {
        "solus-iina"
    } else if real_player.contains("vlc") {
        "solus-vlc"
    } else {
        "solus-mpv"
    };

    // We choose a directory path strictly guaranteed to have NO spaces
    // (e.g. ~/.local/share/solus/bin or /tmp/solus-bin) because ani-cli executes
    // $player_function unquoted in bash ($player_function --referrer=...).
    let bin_dir = env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".local/share/solus/bin"))
        .filter(|p| !p.to_string_lossy().contains(' '))
        .unwrap_or_else(|| env::temp_dir().join("solus-bin"));

    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| AppError::Player(format!("Failed to create wrapper dir: {e}")))?;

    let wrapper_path = bin_dir.join(wrapper_name);

    let script_content = r#"#!/bin/bash
set -e

REAL_PLAYER="${SOLUS_REAL_PLAYER:-}"
if [ -z "$REAL_PLAYER" ]; then
    REAL_PLAYER="$(command -v mpv || echo "/opt/homebrew/bin/mpv")"
fi

args=()
video_link=""
refr=""

for arg in "$@"; do
    case "$arg" in
        --referrer=*)
            refr="${arg#--referrer=}"
            args+=("$arg")
            ;;
        --sub-file=*)
            # When SOLUS_SUB_URL is set, omit ani-cli's fallback sub-file
            if [ -z "$SOLUS_SUB_URL" ]; then
                args+=("$arg")
            fi
            ;;
        http*://* | /*.m3u8*)
            video_link="$arg"
            ;;
        *)
            args+=("$arg")
            ;;
    esac
done

is_iina=0
is_vlc=0
if [[ "$REAL_PLAYER" =~ iina ]] || [[ "$REAL_PLAYER" =~ IINA ]]; then
    is_iina=1
elif [[ "$REAL_PLAYER" =~ vlc ]] || [[ "$REAL_PLAYER" =~ VLC ]]; then
    is_vlc=1
fi

if [ -n "$SOLUS_SUB_URL" ]; then
    if [ "$is_iina" -eq 1 ]; then
        escaped_sub="${SOLUS_SUB_URL//:/\\:}"
        args+=("--mpv-sub-files=$escaped_sub" "--mpv-sub-visibility=yes")
    elif [ "$is_vlc" -eq 1 ]; then
        args+=(":sub-file=$SOLUS_SUB_URL")
    else
        args+=("--sub-file=$SOLUS_SUB_URL" "--sub-visibility=yes" "--sub-auto=fuzzy")
    fi
fi

if [[ "$video_link" =~ /index\.m3u8$ ]]; then
    master_url="${video_link%/*/*}/master.m3u8"
    if [ -n "$master_url" ]; then
        has_audio=$(curl -s -m 2 -e "$refr" "$master_url" 2>/dev/null | grep -m 1 "TYPE=AUDIO" || true)
        if [ -n "$has_audio" ]; then
            if [ "$is_iina" -eq 1 ]; then
                escaped_master="${master_url//:/\\:}"
                args+=("--mpv-audio-file=$escaped_master")
            elif [ "$is_vlc" -eq 1 ]; then
                video_link="$master_url"
            else
                args+=("--audio-file=$master_url" "--audio-file-auto=all")
            fi
        fi
    fi
fi

if [ "$SOLUS_AUDIO_DUB" = "1" ]; then
    if [ "$is_iina" -eq 1 ]; then
        args+=("--mpv-alang=en,eng,English")
    elif [ "$is_vlc" -eq 1 ]; then
        args+=("--audio-language=en,eng")
    else
        args+=("--alang=en,eng,English")
    fi
else
    if [ "$is_iina" -eq 1 ]; then
        args+=("--mpv-alang=ja,jpn,Japanese")
    elif [ "$is_vlc" -eq 1 ]; then
        args+=("--audio-language=ja,jpn")
    else
        args+=("--alang=ja,jpn,Japanese")
    fi
fi

if [ -n "$SOLUS_IPC_SOCKET" ]; then
    if [ "$is_iina" -eq 1 ]; then
        args+=("--mpv-input-ipc-server=$SOLUS_IPC_SOCKET")
    elif [ "$is_vlc" -eq 0 ]; then
        args+=("--input-ipc-server=$SOLUS_IPC_SOCKET")
    fi
fi

if [ -n "$SOLUS_START_POS" ]; then
    if [ "$is_iina" -eq 1 ]; then
        args+=("--mpv-start=$SOLUS_START_POS")
    elif [ "$is_vlc" -eq 1 ]; then
        args+=(":start-time=$SOLUS_START_POS")
    else
        args+=("--start=$SOLUS_START_POS")
    fi
fi

if [ -n "$SOLUS_SHADERS" ]; then
    if [ "$is_iina" -eq 1 ]; then
        args+=("--mpv-glsl-shaders=$SOLUS_SHADERS" "--mpv-scale=ewa_lanczossharp" "--mpv-cscale=ewa_lanczossharp" "--mpv-dscale=mitchell" "--mpv-correct-downscaling=yes" "--mpv-linear-downscaling=yes" "--mpv-sigmoid-upscaling=yes" "--mpv-deband=yes" "--mpv-deband-iterations=2" "--mpv-deband-threshold=35" "--mpv-deband-range=16")
    elif [ "$is_vlc" -eq 0 ]; then
        args+=("--glsl-shaders=$SOLUS_SHADERS" "--scale=ewa_lanczossharp" "--cscale=ewa_lanczossharp" "--dscale=mitchell" "--correct-downscaling=yes" "--linear-downscaling=yes" "--sigmoid-upscaling=yes" "--deband=yes" "--deband-iterations=2" "--deband-threshold=35" "--deband-range=16")
    fi
fi

if [ "$is_vlc" -eq 1 ]; then
    vlc_args=("--play-and-exit" "--no-video-title-show")
    item_opts=()
    for a in "${args[@]}"; do
        case "$a" in
            --input-terminal=*|--force-media-title=*|--sub-files-clr|--sub-auto=*|--keep-running|--no-stdin)
                ;;
            --referrer=*)
                vlc_args+=("--http-referrer=${a#--referrer=}")
                ;;
            :http-referrer=*)
                vlc_args+=("--http-referrer=${a#:http-referrer=}")
                ;;
            --sub-file=*)
                item_opts+=(":sub-file=${a#--sub-file=}")
                ;;
            :sub-file=*)
                item_opts+=("$a")
                ;;
            --start=*)
                item_opts+=(":start-time=${a#--start=}")
                ;;
            :start-time=*)
                item_opts+=("$a")
                ;;
            --audio-language=*|--meta-title=*)
                vlc_args+=("$a")
                ;;
            *)
                vlc_args+=("$a")
                ;;
        esac
    done
    if [ -n "$video_link" ]; then
        exec "$REAL_PLAYER" "${vlc_args[@]}" "$video_link" "${item_opts[@]}"
    else
        exec "$REAL_PLAYER" "${vlc_args[@]}" "${item_opts[@]}"
    fi
elif [ "$is_iina" -eq 1 ]; then
    iina_args=("--keep-running" "--no-stdin")
    if [ -n "$refr" ]; then
        iina_args+=("--mpv-referrer=$refr")
    else
        iina_args+=("--mpv-referrer=https://zokoanime.video/")
    fi
    iina_args+=("--mpv-user-agent=Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
    for a in "${args[@]}"; do
        case "$a" in
            --input-terminal=*|--keep-running|--no-stdin|--mpv-input-ipc-server=*|--input-ipc-server=*)
                ;;
            --referrer=*)
                ;;
            --force-media-title=*)
                iina_args+=("--mpv-force-media-title=${a#--force-media-title=}")
                ;;
            --sub-file=*)
                iina_args+=("--mpv-sub-file=${a#--sub-file=}" "--mpv-sub-visibility=yes")
                ;;
            --sub-files=*)
                iina_args+=("--mpv-sub-file=${a#--sub-files=}" "--mpv-sub-visibility=yes")
                ;;
            --mpv-sub-files=*)
                iina_args+=("--mpv-sub-file=${a#--mpv-sub-files=}" "--mpv-sub-visibility=yes")
                ;;
            --start=*)
                iina_args+=("--mpv-start=${a#--start=}")
                ;;
            --mpv-*)
                iina_args+=("$a")
                ;;
            --*)
                iina_args+=("--mpv-${a#--}")
                ;;
            *)
                iina_args+=("$a")
                ;;
        esac
    done
    if [ -n "$video_link" ]; then
        exec "$REAL_PLAYER" "${iina_args[@]}" "$video_link"
    else
        exec "$REAL_PLAYER" "${iina_args[@]}"
    fi
else
    if [ -n "$video_link" ]; then
        exec "$REAL_PLAYER" "${args[@]}" "$video_link"
    else
        exec "$REAL_PLAYER" "${args[@]}"
    fi
fi
"#;

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

/// Spawn ani-cli asynchronously. stdout + stderr are piped for log streaming.
/// The player is detached by default (ani-cli default behaviour).
/// Returns a tokio Child — caller is responsible for reading I/O and waiting.
pub fn spawn_async(opts: &PlayOptions) -> Result<Child> {
    let args = build_args(opts);
    let player = resolve_player(&opts.player);
    let wrapper_path = ensure_player_wrapper(&player)?;
    let bin_dir = wrapper_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| env::temp_dir().join("solus-bin"));
    #[cfg(not(windows))]
    let wrapper_name = wrapper_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "solus-mpv".to_string());

    // 1. Direct stream playback:
    // If we resolved the exact verified stream URL (.m3u8), launch the player directly
    // through solus-mpv. This eliminates ani-cli fuzzy text searches, guaranteeing 100%
    // frame-perfect subtitle synchronization, accurate resolution, and sub-100ms launch.
    if let Some(ref direct_url) = opts.stream_url {
        if !direct_url.is_empty() {
            let is_vlc = player.to_lowercase().contains("vlc");
            let is_iina = player.to_lowercase().contains("iina");

            let mut cmd = if is_vlc || is_iina {
                Command::new(&player)
            } else {
                #[cfg(windows)]
                {
                    Command::new(&player)
                }
                #[cfg(not(windows))]
                {
                    Command::new(&wrapper_path)
                }
            };

            if is_vlc {
                // VLC-native arguments:
                // --play-and-exit ensures VLC immediately quits when playback finishes or the window is closed
                cmd.arg("--play-and-exit");
                cmd.arg("--no-video-title-show");
                cmd.arg(format!("--meta-title={} Episode {}", opts.title, opts.episode));
                cmd.arg("--http-referrer=https://zokoanime.video/");
                cmd.arg("--http-user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");
                if opts.dub {
                    cmd.arg("--audio-language=en,eng");
                } else {
                    cmd.arg("--audio-language=ja,jpn");
                }
                cmd.arg(direct_url);
                if let Some(ref url) = opts.sub_url {
                    if !url.is_empty() {
                        cmd.arg(format!(":sub-file={}", url));
                    }
                }
                if let Some(pos) = opts.start_pos {
                    cmd.arg(format!(":start-time={}", pos));
                }
            } else if is_iina {
                // IINA-native arguments (--keep-running prevents iina-cli from exiting prematurely)
                // All options must precede the media URL to satisfy iina-cli syntax and header requirements.
                cmd.arg("--keep-running");
                cmd.arg("--no-stdin");
                cmd.arg("--mpv-referrer=https://zokoanime.video/");
                cmd.arg("--mpv-user-agent=Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");
                cmd.arg(format!("--mpv-force-media-title={} Episode {}", opts.title, opts.episode));
                if let Some(ref sock) = opts.ipc_socket {
                    cmd.env("SOLUS_IPC_SOCKET", sock);
                    cmd.arg(format!("--mpv-input-ipc-server={}", sock));
                }
                if let Some(ref url) = opts.sub_url {
                    if !url.is_empty() {
                        cmd.arg(format!("--mpv-sub-file={}", url));
                        cmd.arg("--mpv-sub-visibility=yes");
                    }
                }
                if opts.dub {
                    cmd.arg("--mpv-alang=en,eng,English");
                } else {
                    cmd.arg("--mpv-alang=ja,jpn,Japanese");
                }
                if let Some(pos) = opts.start_pos {
                    cmd.arg(format!("--mpv-start={}", pos));
                }
                if let Some(ref shaders) = opts.glsl_shaders {
                    cmd.arg(format!("--mpv-glsl-shaders={}", shaders));
                    cmd.arg("--mpv-scale=ewa_lanczossharp");
                    cmd.arg("--mpv-cscale=ewa_lanczossharp");
                    cmd.arg("--mpv-dscale=mitchell");
                    cmd.arg("--mpv-correct-downscaling=yes");
                    cmd.arg("--mpv-linear-downscaling=yes");
                    cmd.arg("--mpv-sigmoid-upscaling=yes");
                    cmd.arg("--mpv-deband=yes");
                    cmd.arg("--mpv-deband-iterations=2");
                    cmd.arg("--mpv-deband-threshold=35");
                    cmd.arg("--mpv-deband-range=16");
                }
                if let Some(ref style) = opts.sub_style {
                    for flag in style.to_mpv_flags() {
                        cmd.arg(format!("--mpv-{}", flag.trim_start_matches("--")));
                    }
                }
                // Media URL is placed strictly trailing
                cmd.arg(direct_url);
            } else {
                // Standard MPV engine arguments
                cmd.arg("--referrer=https://zokoanime.video/");
                cmd.arg("--input-terminal=no");
                cmd.arg(format!("--force-media-title={} Episode {}", opts.title, opts.episode));

                #[cfg(windows)]
                {
                    if let Some(ref url) = opts.sub_url {
                        if !url.is_empty() {
                            cmd.arg(format!("--sub-file={}", url));
                            cmd.arg("--sub-visibility=yes");
                            cmd.arg("--sub-auto=fuzzy");
                        }
                    }
                    if opts.dub {
                        cmd.arg("--alang=en,eng,English");
                    } else {
                        cmd.arg("--alang=ja,jpn,Japanese");
                    }
                }

                if let Some(ref sock) = opts.ipc_socket {
                    cmd.env("SOLUS_IPC_SOCKET", sock);
                    cmd.arg(format!("--input-ipc-server={}", sock));
                }
                if let Some(pos) = opts.start_pos {
                    cmd.env("SOLUS_START_POS", pos.to_string());
                    cmd.arg(format!("--start={}", pos));
                }

                if let Some(ref shaders) = opts.glsl_shaders {
                    cmd.env("SOLUS_SHADERS", shaders);
                    cmd.arg(format!("--glsl-shaders={}", shaders));
                    cmd.arg("--scale=ewa_lanczossharp");
                    cmd.arg("--cscale=ewa_lanczossharp");
                    cmd.arg("--dscale=mitchell");
                    cmd.arg("--correct-downscaling=yes");
                    cmd.arg("--linear-downscaling=yes");
                    cmd.arg("--sigmoid-upscaling=yes");
                    cmd.arg("--deband=yes");
                    cmd.arg("--deband-iterations=2");
                    cmd.arg("--deband-threshold=35");
                    cmd.arg("--deband-range=16");
                }

                if let Some(ref style) = opts.sub_style {
                    for flag in style.to_mpv_flags() {
                        cmd.arg(&flag);
                    }
                }

                cmd.arg(direct_url);
            }

            let mut current_paths = match env::var_os("PATH") {
                Some(val) => env::split_paths(&val).collect::<Vec<_>>(),
                None => Vec::new(),
            };
            current_paths.insert(0, bin_dir.clone());
            #[cfg(target_os = "macos")]
            {
                current_paths.push(PathBuf::from("/Applications/IINA.app/Contents/MacOS"));
                current_paths.push(PathBuf::from("/Applications/VLC.app/Contents/MacOS"));
                current_paths.push(PathBuf::from("/opt/homebrew/bin"));
                current_paths.push(PathBuf::from("/usr/local/bin"));
            }
            #[cfg(windows)]
            {
                if let Ok(local_app) = env::var("LOCALAPPDATA") {
                    let local = PathBuf::from(local_app);
                    current_paths.push(local.join(r"Microsoft\WinGet\Links"));
                    current_paths.push(local.join(r"Programs\mpv"));
                }
                if let Ok(user_profile) = env::var("USERPROFILE") {
                    let user = PathBuf::from(user_profile);
                    current_paths.push(user.join(r"scoop\shims"));
                }
                current_paths.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin"));
                current_paths.push(PathBuf::from(r"C:\Program Files\VideoLAN\VLC"));
                current_paths.push(PathBuf::from(r"C:\Program Files (x86)\VideoLAN\VLC"));
            }
            if let Ok(new_path) = env::join_paths(current_paths) {
                cmd.env("PATH", new_path);
            }

            cmd.env("SOLUS_REAL_PLAYER", &player);
            if opts.dub {
                cmd.env("SOLUS_AUDIO_DUB", "1");
            } else {
                cmd.env("SOLUS_AUDIO_DUB", "0");
            }

            if let Some(ref url) = opts.sub_url {
                if !url.is_empty() {
                    cmd.env("SOLUS_SUB_URL", url);
                }
            }

            return cmd
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| AppError::Player(format!("Failed to spawn direct player: {e}")));
        }
    }

    // 2. Fallback playback via ani-cli
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
                "ani-cli was not found. Install it via 'scoop install ani-cli' or select AllAnime provider for direct streaming.".to_string(),
            ));
        }
    };
    #[cfg(not(windows))]
    let mut cmd = Command::new("ani-cli");

    // Prepend wrapper directory and well-known tool directories to PATH
    let mut current_paths = match env::var_os("PATH") {
        Some(val) => env::split_paths(&val).collect::<Vec<_>>(),
        None => Vec::new(),
    };
    current_paths.insert(0, bin_dir);
    #[cfg(windows)]
    {
        if let Ok(local_app) = env::var("LOCALAPPDATA") {
            let local = PathBuf::from(local_app);
            current_paths.push(local.join(r"Microsoft\WinGet\Links"));
            current_paths.push(local.join(r"Programs\mpv"));
        }
        if let Ok(user_profile) = env::var("USERPROFILE") {
            let user = PathBuf::from(user_profile);
            current_paths.push(user.join(r"scoop\shims"));
        }
        current_paths.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin"));
        current_paths.push(PathBuf::from(r"C:\Program Files\VideoLAN\VLC"));
        current_paths.push(PathBuf::from(r"C:\Program Files\Git\bin"));
        current_paths.push(PathBuf::from(r"C:\Program Files\Git\usr\bin"));
    }
    if let Ok(new_path) = env::join_paths(current_paths) {
        cmd.env("PATH", new_path);
    }

    cmd.args(&args)
        .env("ANI_CLI_EXIT_AFTER_PLAY", "1")
        .env("ANI_CLI_NO_DETACH", "1");

    #[cfg(windows)]
    {
        cmd.env("ANI_CLI_PLAYER", &player);
    }
    #[cfg(not(windows))]
    {
        cmd.env("ANI_CLI_PLAYER", &wrapper_name);
        cmd.env("SOLUS_REAL_PLAYER", &player);
    }

    if opts.dub {
        cmd.env("SOLUS_AUDIO_DUB", "1");
    } else {
        cmd.env("SOLUS_AUDIO_DUB", "0");
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

    let mut player_flags = Vec::new();
    if player.contains("iina") {
        player_flags.push("--keep-running".to_string());
        player_flags.push("--no-stdin".to_string());
    } else if player.contains("vlc") {
        player_flags.push("--play-and-exit".to_string());
        player_flags.push("--no-video-title-show".to_string());
    } else {
        player_flags.push("--input-terminal=no".to_string());
    }

    if let Some(ref sock) = opts.ipc_socket {
        cmd.env("SOLUS_IPC_SOCKET", sock);
        if player.contains("iina") {
            player_flags.push(format!("--mpv-input-ipc-server={}", sock));
        } else if !player.contains("vlc") {
            player_flags.push(format!("--input-ipc-server={}", sock));
        }
    }

    if let Some(pos) = opts.start_pos {
        cmd.env("SOLUS_START_POS", pos.to_string());
        if player.contains("iina") {
            player_flags.push(format!("--mpv-start={}", pos));
        } else if player.contains("vlc") {
            player_flags.push(format!(":start-time={}", pos));
        } else {
            player_flags.push(format!("--start={}", pos));
        }
    }

    if let Some(ref shaders) = opts.glsl_shaders {
        cmd.env("SOLUS_SHADERS", shaders);
        if player.contains("iina") {
            player_flags.push(format!("--mpv-glsl-shaders={}", shaders));
        } else if !player.contains("vlc") {
            player_flags.push(format!("--glsl-shaders={}", shaders));
        }
    }

    if let Some(ref url) = opts.sub_url {
        if !url.is_empty() {
            if player.contains("iina") {
                player_flags.push(format!("--mpv-sub-file={}", url));
                player_flags.push("--mpv-sub-visibility=yes".to_string());
            } else if player.contains("vlc") {
                player_flags.push(format!(":sub-file={}", url));
            } else {
                // Default: mpv. Pass without literal quotes so sh parameter expansion
                // passes a clean argument. --sub-files-clr guarantees any preceding
                // fallback sub-file is eliminated, and the user's chosen subtitle is rendered.
                player_flags.push("--sub-files-clr".to_string());
                player_flags.push(format!("--sub-file={}", url));
                player_flags.push("--sub-visibility=yes".to_string());
                player_flags.push("--sub-auto=fuzzy".to_string());
            }
        }
    }

    if let Some(ref style) = opts.sub_style {
        for flag in style.to_mpv_flags() {
            if player.contains("iina") {
                player_flags.push(format!("--mpv-{}", flag.trim_start_matches("--")));
            } else if !player.contains("vlc") {
                player_flags.push(flag);
            }
        }
    }

    cmd.env("ANI_CLI_PLAYER_FLAGS", player_flags.join(" "));

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Player(format!("Failed to spawn ani-cli: {e}")))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(title: &str, episode: u32, quality: &str, dub: bool) -> PlayOptions {
        PlayOptions {
            title:   title.to_string(),
            episode,
            quality: quality.to_string(),
            dub,
            player:  "mpv".to_string(),
            sub_lang: None,
            sub_url:  None,
            stream_url: None,
            ipc_socket: None,
            start_pos:  None,
            glsl_shaders: None,
            sub_style: None,
        }
    }

    #[test]
    fn test_build_args_sub_mode() {
        let args = build_args(&opts("Naruto", 1, "1080p", false));
        assert!(args.contains(&"-S".to_string()));
        assert!(args.contains(&"1".to_string()));
        assert!(args.contains(&"-e".to_string()));
        assert!(args.contains(&"1".to_string()));
        assert!(args.contains(&"-q".to_string()));
        assert!(args.contains(&"1080p".to_string()));
        assert!(!args.contains(&"--dub".to_string()));
        assert!(!args.contains(&"--no-detach".to_string()));
        assert!(args.contains(&"Naruto".to_string()));
    }

    #[test]
    fn test_build_args_dub_mode() {
        let args = build_args(&opts("One Piece", 5, "720p", true));
        assert!(args.contains(&"--dub".to_string()));
        assert!(args.contains(&"5".to_string()));
        assert!(args.contains(&"720p".to_string()));
        assert!(args.contains(&"One Piece".to_string()));
    }

    #[test]
    fn test_build_args_title_is_last() {
        let args = build_args(&opts("Frieren", 3, "best", false));
        assert_eq!(args.last().unwrap(), "Frieren");
    }

    #[test]
    fn test_build_args_episode_follows_flag() {
        let args = build_args(&opts("Test", 7, "720p", false));
        let e_pos = args.iter().position(|a| a == "-e").unwrap();
        assert_eq!(args[e_pos + 1], "7");
    }

    #[test]
    fn test_build_args_quality_follows_flag() {
        let args = build_args(&opts("Test", 1, "480p", false));
        let q_pos = args.iter().position(|a| a == "-q").unwrap();
        assert_eq!(args[q_pos + 1], "480p");
    }

    #[test]
    fn test_build_args_v4_14_contract() {
        let args = build_args(&PlayOptions {
            title:   "One Piece".to_string(),
            episode: 5,
            quality: "720p".to_string(),
            dub:     true,
            player:  "mpv".to_string(),
            sub_lang: None,
            sub_url:  None,
            stream_url: None,
            ipc_socket: None,
            start_pos:  None,
            glsl_shaders: None,
            sub_style: None,
        });
        assert_eq!(
            args,
            vec!["-S", "1", "-e", "5", "-q", "720p", "--exit-after-play", "--dub", "One Piece"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<String>>()
        );
    }

    #[test]
    fn test_build_args_select_nth_precedes_title() {
        let args = build_args(&opts("Test", 1, "720p", false));
        let s_pos = args.iter().position(|a| a == "-S").unwrap();
        assert_eq!(args[s_pos + 1], "1");
        assert_eq!(args.last().unwrap(), "Test");
    }

    #[test]
    fn test_build_args_does_not_pollute_query_with_sub_lang() {
        let args = build_args(&PlayOptions {
            title:   "Frieren".to_string(),
            episode: 1,
            quality: "1080p".to_string(),
            dub:     false,
            player:  "mpv".to_string(),
            sub_lang: Some("Arabic".to_string()),
            sub_url:  Some("https://example.com/sub.vtt".to_string()),
            stream_url: None,
            ipc_socket: None,
            start_pos:  None,
            glsl_shaders: None,
            sub_style: None,
        });
        assert_eq!(
            args,
            vec!["-S", "1", "-e", "1", "-q", "1080p", "--exit-after-play", "Frieren"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<String>>()
        );
        assert!(!args.contains(&"-L".to_string()));
        assert!(!args.contains(&"Arabic".to_string()));
    }

    #[test]
    fn test_resolve_player_keeps_vlc() {
        let resolved = resolve_player("vlc");
        assert!(resolved.to_lowercase().contains("vlc"));
    }

    #[test]
    fn test_ensure_player_wrapper_creates_executable_wrapper() {
        let wrapper = ensure_player_wrapper("mpv").expect("Failed to create mpv wrapper");
        assert!(wrapper.exists());
        assert!(wrapper.file_name().unwrap().to_str().unwrap().contains("mpv"));

        let content = std::fs::read_to_string(&wrapper).expect("Failed to read wrapper");
        assert!(content.contains("TYPE=AUDIO"));
        assert!(content.contains("SOLUS_SUB_URL"));
        assert!(content.contains("SOLUS_AUDIO_DUB"));

        let iina_wrapper = ensure_player_wrapper("iina").expect("Failed to create iina wrapper");
        assert!(iina_wrapper.file_name().unwrap().to_str().unwrap().contains("iina"));

        let vlc_wrapper = ensure_player_wrapper("vlc").expect("Failed to create vlc wrapper");
        assert!(vlc_wrapper.file_name().unwrap().to_str().unwrap().contains("vlc"));
        let vlc_content = std::fs::read_to_string(&vlc_wrapper).expect("Failed to read vlc wrapper");
        assert!(vlc_content.contains("--play-and-exit"));
        assert!(vlc_content.contains("--no-video-title-show"));

        let iina_content = std::fs::read_to_string(&iina_wrapper).expect("Failed to read iina wrapper");
        assert!(iina_content.contains("--keep-running"));
        assert!(iina_content.contains("--no-stdin"));
        assert!(iina_content.contains("--mpv-user-agent"));
    }
}
