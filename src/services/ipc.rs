//! mpv IPC socket monitor.
//! Connects over Unix domain socket to track live playback progress (time-pos, duration),
//! perform automated OP/ED skip seeking, and trigger 85% completion scrobbling.

use crate::api::aniskip::SkipResult;
use crate::db;
use crate::AppMessage;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashSet;
#[cfg(unix)]
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc, RwLock,
};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::{self, Sender, UnboundedSender};

static ACTIVE_SOCKET_PATH: RwLock<Option<String>> = RwLock::new(None);
static ACTIVE_IPC_TX: RwLock<Option<UnboundedSender<String>>> = RwLock::new(None);
static PLAYBACK_PAUSED_FOR_CACHE: AtomicBool = AtomicBool::new(false);

pub fn is_playback_paused_for_cache() -> bool {
    PLAYBACK_PAUSED_FOR_CACHE.load(Ordering::Relaxed)
}

pub fn set_playback_paused_for_cache(paused: bool) {
    PLAYBACK_PAUSED_FOR_CACHE.store(paused, Ordering::Relaxed);
}

#[cfg(test)]
pub fn test_ipc_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    &LOCK
}

/// Record the active mpv IPC socket or Windows named pipe path for the running playback session.
pub fn set_active_socket_path(path: Option<String>) {
    if let Ok(mut guard) = ACTIVE_SOCKET_PATH.write() {
        *guard = path.filter(|s| !s.trim().is_empty());
    }
}

/// Return the currently registered mpv IPC socket or Windows named pipe path.
pub fn get_active_socket_path() -> Option<String> {
    ACTIVE_SOCKET_PATH
        .read()
        .ok()
        .and_then(|guard| guard.clone())
}

/// Register (or clear) the active IPC monitor's outbound command sender.
pub fn set_active_ipc_sender(tx: Option<UnboundedSender<String>>) {
    if let Ok(mut guard) = ACTIVE_IPC_TX.write() {
        *guard = tx;
    }
}

/// Dispatch an mpv JSON-RPC payload directly through the already-connected IPC monitor writer.
pub fn try_send_via_active_monitor(payload: &str) -> bool {
    let sender = ACTIVE_IPC_TX
        .read()
        .ok()
        .and_then(|guard| guard.as_ref().cloned());
    if let Some(tx) = sender {
        return tx.send(payload.to_string()).is_ok();
    }
    false
}

fn clear_active_monitor_if_same(sock_path: &str, cmd_tx: &UnboundedSender<String>) {
    if let Ok(mut guard) = ACTIVE_IPC_TX.write() {
        if guard.as_ref().is_some_and(|cur| cur.same_channel(cmd_tx)) {
            *guard = None;
        }
    }
    if let Ok(mut guard) = ACTIVE_SOCKET_PATH.write() {
        if guard.as_deref() == Some(sock_path) {
            *guard = None;
        }
    }
}

#[cfg(unix)]
type IpcStream = tokio::net::UnixStream;

#[cfg(windows)]
type IpcStream = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(not(any(unix, windows)))]
type IpcStream = tokio::io::DuplexStream;

async fn connect_ipc(sock_path: &str) -> Option<IpcStream> {
    #[cfg(unix)]
    {
        if Path::new(sock_path).exists() {
            tokio::net::UnixStream::connect(sock_path).await.ok()
        } else {
            None
        }
    }
    #[cfg(windows)]
    {
        tokio::net::windows::named_pipe::ClientOptions::new()
            .open(sock_path)
            .ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = sock_path;
        None
    }
}

/// Live buffer & stall telemetry sample emitted by the mpv IPC monitor.
#[derive(Debug, Clone, Default)]
pub struct BufferTelemetryUpdate {
    pub anime_id:            i64,
    pub episode:             u32,
    pub paused_for_cache:    bool,
    pub buffering_pct:       u8,
    pub cache_duration_secs: f64,
    pub cache_fw_bytes:      u64,
    pub cache_speed_bps:     f64,
    pub stall_edge:          bool,
}

/// Request payload for seamless mid-playback stream hot-switching via mpv JSON IPC.
#[derive(Debug, Clone)]
pub struct HotSwitchRequest {
    pub new_url:            String,
    pub resume_pos:         f64,
    pub audio_file:         Option<String>,
    pub sub_url:            Option<String>,
    pub subtitles_disabled: bool,
    pub sub_delay_ms:       i32,
    pub media_title:        String,
    pub osd_label:          String,
}

pub fn build_hot_switch_commands(req: &HotSwitchRequest) -> Vec<Value> {
    let mut cmds = vec![
        serde_json::json!({ "command": ["set_property", "start", format!("{:.2}", req.resume_pos.max(0.0))] }),
        serde_json::json!({ "command": ["set_property", "referrer", "https://zokoanime.video/"] }),
        serde_json::json!({ "command": ["set_property", "force-media-title", req.media_title] }),
    ];
    if let Some(ref audio) = req.audio_file {
        if !audio.trim().is_empty() {
            cmds.push(serde_json::json!({ "command": ["set_property", "audio-files", [audio]] }));
        }
    }
    if req.subtitles_disabled {
        cmds.push(serde_json::json!({ "command": ["set_property", "sid", "no"] }));
        cmds.push(serde_json::json!({ "command": ["set_property", "sub-visibility", false] }));
    } else if let Some(ref sub) = req.sub_url {
        if !sub.trim().is_empty() {
            cmds.push(serde_json::json!({ "command": ["set_property", "sub-files", [sub]] }));
            cmds.push(serde_json::json!({ "command": ["set_property", "sub-visibility", true] }));
        }
    }
    cmds.push(serde_json::json!({ "command": ["set_property", "sub-delay", req.sub_delay_ms as f64 / 1000.0] }));
    cmds.push(serde_json::json!({ "command": ["loadfile", req.new_url, "replace"] }));
    cmds.push(serde_json::json!({ "command": ["show-text", req.osd_label, 3500] }));
    cmds
}

pub fn send_hot_switch_stream(req: &HotSwitchRequest) -> bool {
    let cmds = build_hot_switch_commands(req);
    let mut all_ok = true;
    for cmd in cmds {
        if !try_send_via_active_monitor(&cmd.to_string()) {
            all_ok = false;
        }
    }
    all_ok
}

pub fn build_set_network_mode_commands(mode: crate::config::NetworkMode) -> Vec<Value> {
    vec![
        serde_json::json!({ "command": ["set_property", "demuxer-readahead-secs", mode.vod_readahead_secs()] }),
        serde_json::json!({ "command": ["set_property", "cache-pause-wait", mode.vod_cache_pause_wait_secs()] }),
        serde_json::json!({ "command": ["set_property", "network-timeout", mode.network_timeout_secs()] }),
        serde_json::json!({ "command": ["set_property", "hls-bitrate", mode.hls_bitrate_cap()] }),
        serde_json::json!({ "command": ["show-text", format!("◈ Solus: {} ({}s readahead)", mode.short_tag(), mode.vod_readahead_secs()), 3000] }),
    ]
}

pub fn send_set_network_mode(mode: crate::config::NetworkMode) -> bool {
    let cmds = build_set_network_mode_commands(mode);
    let mut all_ok = true;
    for cmd in cmds {
        if !try_send_via_active_monitor(&cmd.to_string()) {
            all_ok = false;
        }
    }
    all_ok
}

/// Build mpv IPC JSON commands to toggle fullscreen mode live with an OSD confirmation toast.
pub fn build_set_fullscreen_commands(fullscreen: bool) -> Vec<Value> {
    let label = if fullscreen {
        "◈ Solus: Auto-Fullscreen ON"
    } else {
        "◈ Solus: Auto-Fullscreen OFF"
    };
    vec![
        serde_json::json!({ "command": ["set_property", "fullscreen", fullscreen] }),
        serde_json::json!({ "command": ["show-text", label, 2500] }),
    ]
}

/// Toggle fullscreen on any currently active mpv/IINA IPC session.
pub fn send_set_fullscreen(fullscreen: bool) -> bool {
    let cmds = build_set_fullscreen_commands(fullscreen);
    let mut all_ok = true;
    for cmd in cmds {
        if !try_send_via_active_monitor(&cmd.to_string()) {
            all_ok = false;
        }
    }
    all_ok
}

/// Labels for the 6 Picture-in-Picture corner & window size presets.
pub const PIP_CORNER_LABELS: [&str; 6] = [
    "Bottom-Right 30%",
    "Bottom-Left 30%",
    "Top-Right 30%",
    "Top-Left 30%",
    "Compact 24%",
    "Theater 38%",
];

/// Build mpv IPC JSON commands to toggle borderless always-on-top Picture-in-Picture mode.
pub fn build_set_pip_mode_commands(enabled: bool) -> Vec<Value> {
    if enabled {
        vec![
            serde_json::json!({ "command": ["set_property", "fullscreen", false] }),
            serde_json::json!({ "command": ["set_property", "ontop", true] }),
            serde_json::json!({ "command": ["set_property", "border", false] }),
            serde_json::json!({ "command": ["set_property", "keepaspect-window", true] }),
            serde_json::json!({ "command": ["set_property", "window-scale", 0.30] }),
            serde_json::json!({ "command": ["set_property", "geometry", "30%x30%-24-24"] }),
            serde_json::json!({ "command": ["show-text", "◈ Picture-in-Picture: ON ([p] toggle, [P] corner)", 2400] }),
        ]
    } else {
        vec![
            serde_json::json!({ "command": ["set_property", "ontop", false] }),
            serde_json::json!({ "command": ["set_property", "border", true] }),
            serde_json::json!({ "command": ["set_property", "window-scale", 1.0] }),
            serde_json::json!({ "command": ["set_property", "geometry", "50%:50%"] }),
            serde_json::json!({ "command": ["show-text", "◈ Picture-in-Picture: OFF (Standard Window)", 2000] }),
        ]
    }
}

pub fn send_set_pip_mode(enabled: bool) -> bool {
    let cmds = build_set_pip_mode_commands(enabled);
    let mut all_ok = true;
    for cmd in cmds {
        if !try_send_via_active_monitor(&cmd.to_string()) {
            all_ok = false;
        }
    }
    all_ok
}

pub fn build_cycle_pip_corner_commands() -> Vec<Value> {
    vec![serde_json::json!({ "command": ["script-binding", "solus-cycle-pip-corner"] })]
}

pub fn send_cycle_pip_corner() -> bool {
    let cmds = build_cycle_pip_corner_commands();
    let mut all_ok = true;
    for cmd in cmds {
        if !try_send_via_active_monitor(&cmd.to_string()) {
            all_ok = false;
        }
    }
    all_ok
}

pub fn build_toggle_info_hud_commands() -> Vec<Value> {
    vec![serde_json::json!({ "command": ["script-binding", "solus-toggle-hud"] })]
}

pub fn send_toggle_info_hud() -> bool {
    let cmds = build_toggle_info_hud_commands();
    let mut all_ok = true;
    for cmd in cmds {
        if !try_send_via_active_monitor(&cmd.to_string()) {
            all_ok = false;
        }
    }
    all_ok
}

pub fn build_pin_info_hud_commands() -> Vec<Value> {
    vec![serde_json::json!({ "command": ["script-binding", "solus-pin-hud"] })]
}

pub fn send_pin_info_hud() -> bool {
    let cmds = build_pin_info_hud_commands();
    let mut all_ok = true;
    for cmd in cmds {
        if !try_send_via_active_monitor(&cmd.to_string()) {
            all_ok = false;
        }
    }
    all_ok
}

/// Parse `demuxer-cache-state` JSON object returning `(cache_duration_secs, fw_bytes)`.
pub fn parse_demuxer_cache_state(line: &str) -> Option<(f64, u64)> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    if parsed.get("event")?.as_str()? != "property-change"
        || parsed.get("name")?.as_str()? != "demuxer-cache-state"
    {
        return None;
    }
    let data = parsed.get("data")?.as_object()?;
    let duration_secs = data
        .get("cache-duration")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .max(0.0);
    let fw_bytes = data
        .get("fw-bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Some((duration_secs, fw_bytes))
}

/// Parse a numeric `property-change` (float or integer) by property name.
pub fn parse_numeric_property_change(line: &str, target_name: &str) -> Option<f64> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    if parsed.get("event")?.as_str()? == "property-change"
        && parsed.get("name")?.as_str()? == target_name
    {
        return parsed.get("data")?.as_f64();
    }
    None
}

/// Atomic exit classification written by IPC monitor & checked when `mpv` exits.
pub const PLAYBACK_EXIT_UNKNOWN: u8 = 0;
pub const PLAYBACK_EXIT_NATURAL_EOF: u8 = 1;
pub const PLAYBACK_EXIT_USER_QUIT: u8 = 2;

/// Configuration for launching an mpv IPC monitoring session.
pub struct IpcMonitorConfig {
    pub anime_id:           i64,
    pub mal_id:             Option<i64>,
    pub episode:            u32,
    pub next_episode:       Option<u32>,
    pub sub_path:           Option<String>,
    pub sock_path:          String,
    pub pool:               SqlitePool,
    pub tx:                 Sender<AppMessage>,
    pub auth:               crate::config::AuthConfig,
    pub trackers:           crate::config::TrackersConfig,
    pub skip_intervals:     Vec<SkipResult>,
    pub auto_skip_intro:    bool,
    pub has_loading_screen: bool,
    pub playback_exit_kind: Option<Arc<AtomicU8>>,
}

/// Parse `end-file` event reason from mpv JSON-RPC (`"eof"`, `"quit"`, `"stop"`, `"error"`).
pub fn parse_end_file_reason(line: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    if parsed.get("event")?.as_str()? == "end-file" {
        return parsed.get("reason")?.as_str().map(|s| s.to_string());
    }
    None
}

/// Parse `playlist-pos` property changes from mpv JSON-RPC.
pub fn parse_playlist_pos_change(line: &str) -> Option<i64> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    if parsed.get("event")?.as_str()? == "property-change"
        && parsed.get("name")?.as_str()? == "playlist-pos"
    {
        return parsed.get("data")?.as_i64();
    }
    None
}

/// Parse boolean property changes (e.g., `eof-reached`) from mpv JSON-RPC.
pub fn parse_bool_property_change(line: &str, target_name: &str) -> Option<bool> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    if parsed.get("event")?.as_str()? == "property-change"
        && parsed.get("name")?.as_str()? == target_name
    {
        return parsed.get("data")?.as_bool();
    }
    None
}

/// Format a live buffering progress bar for mpv OSD while the stream connects.
pub fn format_y2k_loader_osd(elapsed_ms: u64) -> String {
    let cycle = (elapsed_ms / 120) as usize % 12;
    let filled = (cycle + 1).min(10);
    let empty = 10usize.saturating_sub(filled);
    let secs = elapsed_ms as f64 / 1000.0;
    format!(
        "◈ SOLUS // STREAM LOADER  [{}{}]  {:.1}s",
        "■".repeat(filled),
        "□".repeat(empty),
        secs
    )
}

/// Format an ASS `osd-overlay` vector bar positioned directly inside the 1280x720 loading card's bar track.
pub fn format_loader_ass_overlay(elapsed_ms: u64) -> String {
    // Smoothly advance 0% -> 95% over 2.5s, then pulse gently while waiting for first video frame
    let pct = if elapsed_ms < 2500 {
        (elapsed_ms as f64 / 2500.0) * 0.95
    } else {
        0.95
    };
    let bar_w = ((568.0 * pct).round() as u32).clamp(16, 568);
    format!(
        "{{\\an7\\pos(600,398)\\p1\\bord0\\shad0\\1c&HFFFFFF&}}m 0 0 l {bar_w} 0 l {bar_w} 14 l 0 14{{\\p0}}"
    )
}

/// Helper to parse property change data from mpv JSON-RPC.
pub fn parse_property_change(line: &str) -> Option<(String, f64)> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    if parsed.get("event")?.as_str()? == "property-change" {
        let name = parsed.get("name")?.as_str()?.to_string();
        let data = parsed.get("data")?.as_f64()?;
        return Some((name, data));
    }
    None
}

/// Parse audio tracks from an mpv JSON-RPC `track-list` property-change event.
/// Preserves exact `mpv_aid` IDs while sorting Japanese original first, English dub second, and remaining dubs cleanly.
pub fn parse_audio_tracks_from_property_change(
    line: &str,
) -> Option<Vec<crate::services::remote::RemoteAudioTrack>> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    if parsed.get("event")?.as_str()? != "property-change" {
        return None;
    }
    if parsed.get("name")?.as_str()? != "track-list" {
        return None;
    }
    let items = parsed.get("data")?.as_array()?;

    let mut raw_tracks: Vec<(usize, crate::services::remote::RemoteAudioTrack)> = Vec::new();
    let mut used_labels: HashSet<String> = HashSet::new();

    for entry in items {
        if entry.get("type").and_then(|v| v.as_str()) != Some("audio") {
            continue;
        }
        let Some(aid_u64) = entry.get("id").and_then(|v| v.as_u64()) else {
            continue;
        };
        let aid = aid_u64 as u32;
        let lang = entry
            .get("lang")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let title = entry
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let selected = entry
            .get("selected")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let lang_missing = lang.is_empty() || lang.eq_ignore_ascii_case("und");
        let title_missing = title.is_empty() || title.eq_ignore_ascii_case("und");

        let (code, base_name) = if lang_missing && title_missing {
            if aid == 1 {
                ("jpn".to_string(), "Japanese".to_string())
            } else {
                crate::api::subtitles::resolve_lang_meta(&format!("Audio Track {aid}"), "")
            }
        } else {
            crate::api::subtitles::resolve_lang_meta(title, lang)
        };

        let (mut label, is_dub) =
            crate::services::remote::format_audio_track_label(&code, &base_name);
        if !used_labels.insert(label.clone()) {
            label = format!("{label} (#{aid})");
            used_labels.insert(label.clone());
        }

        let priority = if !is_dub {
            0
        } else if code.starts_with("eng") || code == "en" {
            1
        } else if code.starts_with("ara") {
            2
        } else if code.starts_with("spa") {
            3
        } else if code.starts_with("por") {
            4
        } else if code.starts_with("fra") || code.starts_with("fre") {
            5
        } else if code.starts_with("ger") || code.starts_with("deu") {
            6
        } else if code.starts_with("rus") {
            7
        } else if code.starts_with("ind") {
            8
        } else {
            9
        };

        raw_tracks.push((
            priority,
            crate::services::remote::RemoteAudioTrack {
                id: aid,
                code,
                label,
                is_active: selected,
                is_dub,
                mpv_aid: Some(aid),
            },
        ));
    }

    if raw_tracks.is_empty() {
        return None;
    }

    raw_tracks.sort_by_key(|(prio, _)| *prio);
    let tracks: Vec<crate::services::remote::RemoteAudioTrack> = raw_tracks
        .into_iter()
        .enumerate()
        .map(|(idx, (_, mut track))| {
            track.id = (idx + 1) as u32;
            track
        })
        .collect();

    Some(tracks)
}

/// Check if a given playback position falls inside any un-skipped interval.
pub fn check_skip_interval<'a>(
    pos: f64,
    intervals: &'a [SkipResult],
    skipped_types: &HashSet<String>,
) -> Option<&'a SkipResult> {
    for item in intervals {
        let key = format!("{}:{:.1}", item.skip_type, item.interval.start_time);
        if skipped_types.contains(&key) {
            continue;
        }
        // If within the skip window (allow 0.5s pre-trigger, and until 1.0s before end)
        if pos >= item.interval.start_time - 0.5 && pos < item.interval.end_time - 1.0 {
            return Some(item);
        }
    }
    None
}

/// Calculate whether the completion percentage has crossed the 85% threshold.
pub fn is_completion_threshold_reached(pos: f64, duration: f64) -> bool {
    if duration < 60.0 {
        return false;
    }
    (pos / duration) >= 0.85
}

/// Spawns the IPC background monitor task.
pub fn spawn_mpv_ipc_monitor(config: IpcMonitorConfig) {
    tokio::spawn(async move {
        run_mpv_ipc_monitor(config).await;
    });
}

async fn run_mpv_ipc_monitor(config: IpcMonitorConfig) {
    let sock_path_for_cleanup = config.sock_path.clone();
    set_active_socket_path(Some(config.sock_path.clone()));

    // Spawn AniSkip resolution in the background (needed both for auto-skip AND interactive Skip Intro/Outro pills).
    let (skip_tx, skip_rx) = tokio::sync::oneshot::channel::<Vec<SkipResult>>();
    if config.skip_intervals.is_empty() {
        let mal_hint = config.mal_id;
        let aid = config.anime_id;
        let ep = config.episode;
        let tx_app = config.tx.clone();
        tokio::spawn(async move {
            let mid = if let Some(m) = mal_hint {
                Some(m)
            } else {
                let client = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(3))
                    .timeout(Duration::from_secs(4))
                    .build()
                    .unwrap_or_default();
                let resolved = crate::api::anilist::resolve_mal_id(&client, aid).await;
                if let Some(m) = resolved {
                    let _ = tx_app.send(AppMessage::AnimeMalIdReady(aid, m)).await;
                }
                resolved
            };
            let skips = if let Some(m) = mid {
                crate::api::aniskip::fetch_skip_times(m, ep).await
            } else {
                Vec::new()
            };
            let _ = skip_tx.send(skips);
        });
    } else {
        let _ = skip_tx.send(Vec::new());
    }

    let mut stream = None;
    for _ in 0..300 {
        tokio::time::sleep(Duration::from_millis(40)).await;
        if let Some(s) = connect_ipc(&config.sock_path).await {
            stream = Some(s);
            break;
        }
    }

    let stream = match stream {
        Some(s) => s,
        None => {
            if get_active_socket_path().as_deref() == Some(sock_path_for_cleanup.as_str()) {
                set_active_socket_path(None);
            }
            // Clean up socket file if left behind
            #[cfg(unix)]
            let _ = std::fs::remove_file(&sock_path_for_cleanup);
            return;
        }
    };

    drive_ipc_stream(stream, config, skip_rx).await;

    // Clean up socket file
    #[cfg(unix)]
    let _ = std::fs::remove_file(&sock_path_for_cleanup);
}

fn build_player_ctx_payload(
    episode: u32,
    next_episode: Option<u32>,
    skip_intervals: &[SkipResult],
    sub_path: Option<&str>,
) -> String {
    let op = skip_intervals
        .iter()
        .find(|s| s.skip_type == "op" || s.skip_type == "mixed-op");
    let ed = skip_intervals
        .iter()
        .find(|s| s.skip_type == "ed" || s.skip_type == "mixed-ed");
    let ctx = serde_json::json!({
        "ep": episode,
        "next_ep": next_episode,
        "op_start": op.map(|s| s.interval.start_time),
        "op_end": op.map(|s| s.interval.end_time),
        "ed_start": ed.map(|s| s.interval.start_time),
        "ed_end": ed.map(|s| s.interval.end_time),
        "sub_path": sub_path,
    });
    serde_json::json!({
        "command": ["script-message", "solus-player-ctx", ctx.to_string()]
    })
    .to_string()
}

async fn drive_ipc_stream<S>(
    stream: S,
    config: IpcMonitorConfig,
    mut skip_rx: tokio::sync::oneshot::Receiver<Vec<SkipResult>>,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let IpcMonitorConfig {
        anime_id,
        mal_id: _,
        episode,
        next_episode,
        sub_path,
        sock_path,
        pool,
        tx,
        auth,
        trackers,
        mut skip_intervals,
        auto_skip_intro,
        has_loading_screen,
        playback_exit_kind,
    } = config;

    let (reader_half, mut writer_half) = tokio::io::split(stream);
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<String>();
    set_active_socket_path(Some(sock_path.clone()));
    set_active_ipc_sender(Some(cmd_tx.clone()));
    set_playback_paused_for_cache(false);

    // Subscribe to time-pos, duration, track-list, playlist-pos, eof-reached, buffer telemetry, and sub-delay
    let observe_cmds = [
        r#"{"command": ["observe_property", 1, "time-pos"]}"#,
        r#"{"command": ["observe_property", 2, "duration"]}"#,
        r#"{"command": ["observe_property", 3, "track-list"]}"#,
        r#"{"command": ["observe_property", 4, "playlist-pos"]}"#,
        r#"{"command": ["observe_property", 5, "eof-reached"]}"#,
        r#"{"command": ["observe_property", 6, "paused-for-cache"]}"#,
        r#"{"command": ["observe_property", 7, "cache-buffering-state"]}"#,
        r#"{"command": ["observe_property", 8, "cache-speed"]}"#,
        r#"{"command": ["observe_property", 9, "demuxer-cache-state"]}"#,
        r#"{"command": ["observe_property", 10, "sub-delay"]}"#,
    ];
    for cmd in observe_cmds {
        let _ = writer_half.write_all(format!("{}\n", cmd).as_bytes()).await;
    }
    let init_ctx = build_player_ctx_payload(episode, next_episode, &skip_intervals, sub_path.as_deref());
    let _ = writer_half.write_all(format!("{init_ctx}\n").as_bytes()).await;
    let _ = writer_half.flush().await;

    let mut reader = BufReader::new(reader_half).lines();
    let mut current_pos = 0.0;
    let mut current_duration = 0.0;
    let mut last_saved_pos = 0.0;
    let mut last_save_time = tokio::time::Instant::now();
    let mut scrobbled = false;
    let mut near_eof_prefetched = false;
    let mut skipped_intervals: HashSet<String> = HashSet::new();
    let mut last_audio_tracks: Vec<crate::services::remote::RemoteAudioTrack> = Vec::new();
    let mut in_loading_screen = has_loading_screen;
    let mut switched_to_stream = false;
    let mut retried_playlist_next = false;
    let mut stream_started = !has_loading_screen;
    let mut skip_rx_pending = true;
    let loader_start = tokio::time::Instant::now();
    let mut loader_tick = tokio::time::interval(Duration::from_millis(120));
    let mut paused_for_cache = false;
    let mut buffering_pct: u8 = 100;
    let mut cache_duration_secs: f64 = 0.0;
    let mut cache_fw_bytes: u64 = 0;
    let mut cache_speed_bps: f64 = 0.0;
    let mut last_telemetry_emit = tokio::time::Instant::now();

    loop {
        tokio::select! {
            biased;
            skip_res = &mut skip_rx, if skip_rx_pending => {
                skip_rx_pending = false;
                if let Ok(skips) = skip_res {
                    if skip_intervals.is_empty() && !skips.is_empty() {
                        skip_intervals = skips;
                        let ctx_cmd = build_player_ctx_payload(
                            episode,
                            next_episode,
                            &skip_intervals,
                            sub_path.as_deref(),
                        );
                        let _ = writer_half.write_all(format!("{ctx_cmd}\n").as_bytes()).await;
                        let _ = writer_half.flush().await;
                    }
                }
            }
            cmd_opt = cmd_rx.recv() => {
                match cmd_opt {
                    Some(cmd_payload) => {
                        let data = if cmd_payload.ends_with('\n') {
                            cmd_payload
                        } else {
                            format!("{cmd_payload}\n")
                        };
                        if writer_half.write_all(data.as_bytes()).await.is_err()
                            || writer_half.flush().await.is_err()
                        {
                            break;
                        }
                    }
                    None => break,
                }
            }
            _ = loader_tick.tick(), if in_loading_screen => {
                let elapsed_ms = loader_start.elapsed().as_millis() as u64;
                let ass_data = format_loader_ass_overlay(elapsed_ms);
                let osd_cmd = serde_json::json!({
                    "command": ["osd-overlay", 1, "ass-events", ass_data, 1280, 720]
                });
                let _ = writer_half.write_all(format!("{osd_cmd}\n").as_bytes()).await;
                if elapsed_ms >= 1500 && !switched_to_stream {
                    switched_to_stream = true;
                    let _ = writer_half.write_all(b"{\"command\":[\"playlist-next\"]}\n").await;
                } else if elapsed_ms >= 3200 && !stream_started && !retried_playlist_next {
                    retried_playlist_next = true;
                    let _ = writer_half.write_all(b"{\"command\":[\"playlist-next\"]}\n").await;
                }
                let _ = writer_half.flush().await;
            }
            line_res = reader.next_line() => {
                let Ok(Some(line)) = line_res else {
                    break;
                };

                if line.contains("client-message") && line.contains("solus-next-episode") {
                    let _ = tx
                        .send(AppMessage::PlaybackLog("__SOLUS_NEXT_EPISODE__".to_string()))
                        .await;
                    continue;
                }

                if let Some(pos) = parse_playlist_pos_change(&line) {
                    if pos >= 1 {
                        stream_started = true;
                    }
                }

                if let Some(reason) = parse_end_file_reason(&line) {
                    if let Some(ref kind) = playback_exit_kind {
                        if reason == "quit" {
                            kind.store(PLAYBACK_EXIT_USER_QUIT, Ordering::Relaxed);
                        } else if reason == "eof"
                            && (stream_started || current_duration > 10.0 || current_pos > 1.0)
                        {
                            kind.store(PLAYBACK_EXIT_NATURAL_EOF, Ordering::Relaxed);
                        }
                    }
                }

                if parse_bool_property_change(&line, "eof-reached") == Some(true)
                    && (stream_started || current_duration > 10.0 || current_pos > 1.0)
                {
                    if let Some(ref kind) = playback_exit_kind {
                        kind.store(PLAYBACK_EXIT_NATURAL_EOF, Ordering::Relaxed);
                    }
                    let _ = writer_half.write_all(b"{\"command\":[\"quit\"]}\n").await;
                    let _ = writer_half.flush().await;
                    break;
                }

                if in_loading_screen {
                    if let Some((ref name, val)) = parse_property_change(&line) {
                        if name == "duration" && val > 10.0 {
                            current_duration = val;
                            stream_started = true;
                            continue;
                        }
                        if stream_started && name == "time-pos" && val > 0.05 {
                            current_pos = val;
                            in_loading_screen = false;
                            let _ = writer_half.write_all(b"{\"command\":[\"osd-overlay\",1,\"none\",\"\"]}\n").await;
                            let _ = writer_half.flush().await;
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                if let Some(tracks) = parse_audio_tracks_from_property_change(&line) {
                    if tracks != last_audio_tracks {
                        last_audio_tracks = tracks.clone();
                        let _ = tx.send(AppMessage::AudioTracksUpdated(tracks)).await;
                    }
                }

                // Live buffer & stall telemetry (strictly gated after loading screen finishes)
                let mut telemetry_changed = false;
                let mut stall_edge = false;
                if let Some(pfc) = parse_bool_property_change(&line, "paused-for-cache") {
                    if pfc != paused_for_cache {
                        stall_edge = pfc && !paused_for_cache;
                        paused_for_cache = pfc;
                        set_playback_paused_for_cache(pfc);
                        telemetry_changed = true;
                    }
                }
                if let Some(pct) = parse_numeric_property_change(&line, "cache-buffering-state") {
                    let clamped = pct.round().clamp(0.0, 100.0) as u8;
                    if clamped != buffering_pct {
                        buffering_pct = clamped;
                        telemetry_changed = true;
                    }
                }
                if let Some(spd) = parse_numeric_property_change(&line, "cache-speed") {
                    cache_speed_bps = spd.max(0.0);
                    telemetry_changed = true;
                }
                if let Some((dur_s, fw_b)) = parse_demuxer_cache_state(&line) {
                    cache_duration_secs = dur_s;
                    cache_fw_bytes = fw_b;
                    telemetry_changed = true;
                }
                if telemetry_changed
                    && (stall_edge || last_telemetry_emit.elapsed() >= Duration::from_millis(500))
                {
                    last_telemetry_emit = tokio::time::Instant::now();
                    let _ = tx
                        .send(AppMessage::PlaybackBufferTelemetry(BufferTelemetryUpdate {
                            anime_id,
                            episode,
                            paused_for_cache,
                            buffering_pct,
                            cache_duration_secs,
                            cache_fw_bytes,
                            cache_speed_bps,
                            stall_edge,
                        }))
                        .await;
                }

                if let Some((name, val)) = parse_property_change(&line) {
                    match name.as_str() {
                        "sub-delay" => {
                            let delay_ms = (val * 1000.0).round() as i32;
                            let _ = tx
                                .send(AppMessage::PlayerSubDelayChanged { anime_id, delay_ms })
                                .await;
                        }
                        "duration" => {
                            current_duration = val;
                        }
                        "time-pos" => {
                            current_pos = val;

                            // 1. Auto-skip intro / outro
                            if auto_skip_intro {
                                if let Some(to_skip) = check_skip_interval(current_pos, &skip_intervals, &skipped_intervals) {
                                    let key = format!("{}:{:.1}", to_skip.skip_type, to_skip.interval.start_time);
                                    skipped_intervals.insert(key);

                                    let seek_cmd = format!(
                                        "{{\"command\": [\"seek\", {}, \"absolute\"]}}\n",
                                        to_skip.interval.end_time
                                    );
                                    let osd_label = match to_skip.skip_type.as_str() {
                                        "ed" | "mixed-ed" => "Outro",
                                        _ => "Intro",
                                    };
                                    let osd_cmd = format!(
                                        "{{\"command\": [\"show-text\", \"󰄬 Solus: Auto-skipped {}\", 3000]}}\n",
                                        osd_label
                                    );

                                    let _ = writer_half.write_all(seek_cmd.as_bytes()).await;
                                    let _ = writer_half.write_all(osd_cmd.as_bytes()).await;
                                    let _ = writer_half.flush().await;

                                    let _ = tx
                                        .send(AppMessage::PlaybackLog(format!(
                                            "󰄬 Auto-skipped {} at {:.1}s -> {:.1}s",
                                            osd_label, current_pos, to_skip.interval.end_time
                                        )))
                                        .await;
                                }
                            }

                            // 2. Auto-scrobble once 85% completion threshold is reached
                            if !scrobbled && is_completion_threshold_reached(current_pos, current_duration) {
                                scrobbled = true;
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;

                                let pool_clone = pool.clone();
                                let mut auth_clone = auth.clone();
                                let trackers_clone = trackers.clone();
                                tokio::spawn(async move {
                                    let _ = db::user::record_watched(&pool_clone, anime_id, episode as i64, now).await;
                                    if let Ok(Some(anime)) = db::cache::get_anime(&pool_clone, anime_id).await {
                                        let _ = crate::services::scrobble::sync_episode_watched_all(
                                            &pool_clone,
                                            &anime,
                                            episode,
                                            &mut auth_clone,
                                            &trackers_clone,
                                            now,
                                        )
                                        .await;
                                    } else if let Some(token) = auth_clone.anilist_token() {
                                        let client = crate::api::anilist::AniListClient::with_token(Some(token));
                                        let _ = client.save_progress(anime_id, episode).await;
                                    }
                                });

                                let _ = tx.send(AppMessage::EpisodeAutoScrobbled(anime_id, episode)).await;
                                let _ = tx
                                    .send(AppMessage::PlaybackLog(format!(
                                        "󰄬 Episode {} completed (85% reached) — auto-scrobbled",
                                        episode
                                    )))
                                    .await;
                            }

                            // 2b. Trigger Just-In-Time N+1 queue prefetch strictly within 90s of EOF
                            // so CDN stream URLs never expire while watching long episodes.
                            if !near_eof_prefetched
                                && current_duration > 60.0
                                && current_duration > current_pos
                                && (current_duration - current_pos) <= 90.0
                            {
                                near_eof_prefetched = true;
                                let _ = tx.send(AppMessage::NearEofPrefetch(anime_id, episode)).await;
                            }

                            // 3. Throttle progress updates to UI and database
                            let now_instant = tokio::time::Instant::now();
                            if (current_pos - last_saved_pos).abs() >= 3.0 || now_instant.duration_since(last_save_time).as_secs() >= 4 {
                                last_saved_pos = current_pos;
                                last_save_time = now_instant;
                                let now_unix = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;

                                let _ = db::user::save_playback_progress(
                                    &pool,
                                    anime_id,
                                    episode,
                                    current_pos,
                                    current_duration,
                                    now_unix,
                                )
                                .await;

                                let _ = tx
                                    .send(AppMessage::PlaybackProgressUpdated(
                                        anime_id,
                                        episode,
                                        current_pos,
                                        current_duration,
                                    ))
                                    .await;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    clear_active_monitor_if_same(&sock_path, &cmd_tx);
    set_playback_paused_for_cache(false);

    // On player exit, persist the final position
    if current_pos > 0.0 {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let _ = db::user::save_playback_progress(
            &pool,
            anime_id,
            episode,
            current_pos,
            current_duration,
            now_unix,
        )
        .await;
        let _ = tx
            .send(AppMessage::PlaybackProgressUpdated(
                anime_id,
                episode,
                current_pos,
                current_duration,
            ))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::aniskip::SkipInterval;

    #[test]
    fn test_parse_property_change() {
        let line = r#"{"event":"property-change","id":1,"name":"time-pos","data":142.5}"#;
        let res = parse_property_change(line);
        assert_eq!(res, Some(("time-pos".to_string(), 142.5)));

        let dur_line = r#"{"event":"property-change","id":2,"name":"duration","data":1420.0}"#;
        let res2 = parse_property_change(dur_line);
        assert_eq!(res2, Some(("duration".to_string(), 1420.0)));

        let other = r#"{"request_id":0,"error":"success"}"#;
        assert_eq!(parse_property_change(other), None);
    }

    #[test]
    fn test_check_skip_interval() {
        let intervals = vec![
            SkipResult {
                interval: SkipInterval { start_time: 50.0, end_time: 140.0 },
                skip_type: "op".to_string(),
            },
            SkipResult {
                interval: SkipInterval { start_time: 1350.0, end_time: 1440.0 },
                skip_type: "ed".to_string(),
            },
        ];
        let mut skipped = HashSet::new();

        // Outside intervals
        assert!(check_skip_interval(20.0, &intervals, &skipped).is_none());

        // Inside OP interval
        let hit = check_skip_interval(50.5, &intervals, &skipped);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().skip_type, "op");

        // Mark OP as skipped
        skipped.insert("op:50.0".to_string());
        assert!(check_skip_interval(50.5, &intervals, &skipped).is_none());

        // Inside ED interval
        let hit_ed = check_skip_interval(1360.0, &intervals, &skipped);
        assert!(hit_ed.is_some());
        assert_eq!(hit_ed.unwrap().skip_type, "ed");
    }

    #[test]
    fn test_is_completion_threshold_reached() {
        assert!(!is_completion_threshold_reached(500.0, 1420.0)); // ~35%
        assert!(!is_completion_threshold_reached(1200.0, 1420.0)); // 84.5%
        assert!(is_completion_threshold_reached(1207.0, 1420.0));  // 85.0%
        assert!(is_completion_threshold_reached(1400.0, 1420.0));  // 98.5%
        assert!(!is_completion_threshold_reached(10.0, 20.0));    // < 60s video ignored
    }

    #[tokio::test]
    async fn test_try_send_via_active_monitor_delivers_commands() {
        let _guard = test_ipc_lock().lock().await;
        let pool = crate::db::init(":memory:").await.unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let (client_io, server_io) = tokio::io::duplex(4096);

        let config = IpcMonitorConfig {
            anime_id: 1,
            mal_id: None,
            episode: 1,
            next_episode: Some(2),
            sub_path: None,
            sock_path: r"\\.\pipe\solus-mpv-1-1".to_string(),
            pool,
            tx,
            auth: crate::config::AuthConfig::default(),
            trackers: crate::config::TrackersConfig::default(),
            skip_intervals: Vec::new(),
            auto_skip_intro: false,
            has_loading_screen: false,
            playback_exit_kind: None,
        };

        let (skip_tx, skip_rx) = tokio::sync::oneshot::channel();
        let _ = skip_tx.send(Vec::new());
        let task = tokio::spawn(async move {
            drive_ipc_stream(client_io, config, skip_rx).await;
        });

        let (server_read, _server_write) = tokio::io::split(server_io);
        let mut lines = BufReader::new(server_read).lines();

        // First ten commands must be observe_property subscriptions (+ init script-message)
        let mut observed = Vec::new();
        for _ in 0..10 {
            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.contains("observe_property"));
            observed.push(line);
        }
        let ctx_line = lines.next_line().await.unwrap().unwrap();
        assert!(ctx_line.contains("solus-player-ctx"));
        assert!(observed[4].contains("eof-reached"));
        assert!(observed[5].contains("paused-for-cache"));
        assert!(observed[6].contains("cache-buffering-state"));
        assert!(observed[7].contains("cache-speed"));
        assert!(observed[8].contains("demuxer-cache-state"));
        assert_eq!(
            get_active_socket_path().as_deref(),
            Some(r"\\.\pipe\solus-mpv-1-1")
        );

        // Send a command through the active monitor channel
        assert!(try_send_via_active_monitor(r#"{"command":["cycle","pause"]}"#));
        let l_cmd = lines.next_line().await.unwrap().unwrap();
        assert_eq!(l_cmd, r#"{"command":["cycle","pause"]}"#);

        drop(lines);
        drop(_server_write);
        let _ = task.await;
    }

    #[test]
    fn test_parse_audio_tracks_from_property_change_multi_dub() {
        let event = r#"{
            "event": "property-change",
            "id": 3,
            "name": "track-list",
            "data": [
                {"id": 1, "type": "video", "selected": true},
                {"id": 1, "type": "audio", "lang": "es-419", "title": "Spanish (Latin America)", "selected": false},
                {"id": 2, "type": "audio", "lang": "pt-BR", "title": "Portuguese (Brazil)", "selected": false},
                {"id": 3, "type": "audio", "lang": "jpn", "title": "Japanese", "selected": true},
                {"id": 4, "type": "audio", "lang": "fra", "title": "French", "selected": false},
                {"id": 5, "type": "audio", "lang": "eng", "title": "English", "selected": false},
                {"id": 1, "type": "sub", "lang": "eng", "selected": true}
            ]
        }"#;

        let tracks = parse_audio_tracks_from_property_change(event).expect("audio tracks parsed");
        assert_eq!(tracks.len(), 5);
        assert_eq!(tracks[0].code, "jpn");
        assert_eq!(tracks[0].mpv_aid, Some(3));
        assert!(tracks[0].is_active);
        assert!(!tracks[0].is_dub);

        assert_eq!(tracks[1].code, "eng");
        assert_eq!(tracks[1].mpv_aid, Some(5));
        assert!(!tracks[1].is_active);
        assert!(tracks[1].is_dub);

        assert_eq!(tracks[2].code, "spa-419");
        assert_eq!(tracks[2].mpv_aid, Some(1));
        assert_eq!(tracks[3].code, "por-br");
        assert_eq!(tracks[3].mpv_aid, Some(2));
        assert_eq!(tracks[4].code, "fra");
        assert_eq!(tracks[4].mpv_aid, Some(4));
    }

    #[test]
    fn test_parse_playlist_pos_change_and_loader_osd() {
        let ev0 = r#"{"event":"property-change","id":4,"name":"playlist-pos","data":0}"#;
        let ev1 = r#"{"event":"property-change","id":4,"name":"playlist-pos","data":1}"#;
        assert_eq!(parse_playlist_pos_change(ev0), Some(0));
        assert_eq!(parse_playlist_pos_change(ev1), Some(1));
        let osd = format_y2k_loader_osd(600);
        assert!(osd.contains("STREAM LOADER"));
        assert!(!osd.contains("Y2K"));
        assert!(osd.contains("0.6s"));
        assert_eq!(
            parse_end_file_reason(r#"{"event":"end-file","reason":"eof"}"#).as_deref(),
            Some("eof")
        );
        assert_eq!(
            parse_end_file_reason(r#"{"event":"end-file","reason":"quit"}"#).as_deref(),
            Some("quit")
        );
        assert_eq!(
            parse_end_file_reason(r#"{"event":"end-file","reason":"stop"}"#).as_deref(),
            Some("stop")
        );
    }

    #[test]
    fn test_buffer_telemetry_parsers_and_hot_switch_commands() {
        let demux = r#"{"event":"property-change","id":9,"name":"demuxer-cache-state","data":{"cache-duration":42.5,"fw-bytes":18874368}}"#;
        let parsed = parse_demuxer_cache_state(demux).unwrap();
        assert!((parsed.0 - 42.5).abs() < 1e-6);
        assert_eq!(parsed.1, 18_874_368);

        let spd = r#"{"event":"property-change","id":8,"name":"cache-speed","data":1536000}"#;
        assert_eq!(parse_numeric_property_change(spd, "cache-speed"), Some(1_536_000.0));

        let pct = r#"{"event":"property-change","id":7,"name":"cache-buffering-state","data":67}"#;
        assert_eq!(parse_numeric_property_change(pct, "cache-buffering-state"), Some(67.0));

        let cmds = build_hot_switch_commands(&HotSwitchRequest {
            new_url: "https://cdn.example.com/720/index.m3u8".into(),
            resume_pos: 312.4,
            audio_file: Some("https://cdn.example.com/audio/jpn.m3u8".into()),
            sub_url: Some("https://cdn.example.com/en.vtt".into()),
            subtitles_disabled: false,
            sub_delay_ms: -250,
            media_title: "Frieren Episode 12".into(),
            osd_label: "Auto-ABR: 720p (~1.6 Mbps)".into(),
        });
        let joined: Vec<String> = cmds.iter().map(|v| v.to_string()).collect();
        assert!(joined.iter().any(|s| s.contains("\"referrer\",\"https://zokoanime.video/\"")));
        assert!(joined.iter().any(|s| s.contains("\"audio-files\"")));
        assert!(joined.iter().any(|s| s.contains("\"sub-files\"")));
        assert!(joined.iter().any(|s| s.contains("\"sub-delay\",-0.25")));
        assert!(joined.iter().any(|s| s.contains("\"loadfile\",\"https://cdn.example.com/720/index.m3u8\",\"replace\"")));

        let net_cmds = build_set_network_mode_commands(crate::config::NetworkMode::CongestedWifi);
        assert_eq!(net_cmds.len(), 5);
        assert!(net_cmds[0].to_string().contains("120"));
        assert!(net_cmds[3].to_string().contains("1800000"));

        let pip_on = build_set_pip_mode_commands(true);
        assert_eq!(pip_on.len(), 7);
        assert!(pip_on[0].to_string().contains("\"fullscreen\",false"));
        assert!(pip_on[1].to_string().contains("\"ontop\",true"));
        assert!(pip_on[2].to_string().contains("\"border\",false"));
        assert!(pip_on[5].to_string().contains("30%x30%-24-24"));

        let pip_off = build_set_pip_mode_commands(false);
        assert_eq!(pip_off.len(), 5);
        assert!(pip_off[0].to_string().contains("\"ontop\",false"));
        assert!(pip_off[1].to_string().contains("\"border\",true"));

        assert!(build_cycle_pip_corner_commands()[0].to_string().contains("solus-cycle-pip-corner"));
        assert!(build_toggle_info_hud_commands()[0].to_string().contains("solus-toggle-hud"));
        assert!(build_pin_info_hud_commands()[0].to_string().contains("solus-pin-hud"));
    }
}
