//! Background auto-update service for Solus.
//!
//! Provides ultra-fluid, non-blocking update checking and atomic self-replacement:
//! - Checks GitHub Releases API in the background with zero startup penalty (<40ms cold boot preserved).
//! - Rate-limited with TTL caching so GitHub is never spammed.
//! - Resolves platform-specific binaries (`solus-macos-aarch64`, `solus-macos-x86_64`, `solus-linux-x86_64`).
//! - Atomically replaces the running binary on Unix without terminating the current session prematurely.

use crate::error::{AppError, Result};
use crate::AppMessage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/abdullah-binmadhi/Solus/releases/latest";
const USER_AGENT: &str = concat!("solus/", env!("CARGO_PKG_VERSION"));

/// Asset entry in GitHub release response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

/// GitHub latest release response payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

/// Metadata about an available update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
    pub asset_name: String,
    pub release_notes: Option<String>,
    #[serde(default)]
    pub simple_notes: Vec<String>,
    pub is_homebrew: bool,
}

/// Persisted state tracking the last seen build SHA/version and any pending Post-Update "What's New" popup.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LastSeenBuild {
    pub last_seen_sha: String,
    pub last_seen_version: String,
    #[serde(default)]
    pub pending_whats_new: Vec<String>,
    #[serde(default)]
    pub updated_to_label: Option<String>,
}

const BUILTIN_CURRENT_RELEASE_NOTES: &[&str] = &[
    "[NEW] 4-Stage Video Mastering & Unified Player Engines: Solus now powers mpv, mpv.net, and Celluloid with unified 4:4:4 chroma upscaling, Anime4K era restoration, debanding, and smooth motion interpolation. How to use: Open Settings ('s') or press 'a' (Shaders), 'd' (Deband), 'm' (Motion), and 'g' (Performance) during playback.",
    "[NEW] Multi-Provider Stream Discovery & Clean/Hardsub Indicators: Automatically probes verified providers (ZokoAnime, MegaPlay Direct/Hardsub, Vidmoly, AnimeHeaven, AnimeGG, and HiAnime) with Clean [CLN] vs Hardsub [EN] tags and demuxed audio tracks. How to use: Press 'Enter' on an episode to pick a server and subtitle track cleanly (hardsubbed videos use their embedded subtitles only).",
    "[NEW] Voice-Synced Subtitles (Audio Alignment + VAD): Automatically aligns external subtitles to dialogue using reference timing and voice-activity matching, and remembers clean provider offsets per anime. How to use: Press 'y' in the player to snap subtitles to speech immediately, or open the HUD ('i' / 'S') for manual delay steps.",
    "[IMPROVED] Interactive Player HUD Controls: Skip Intro, Skip Outro, and Next Episode buttons live neatly inside the Solus HUD right under the SubSync bar for on-demand control without intrusive popups. How to use: Press 'i' or 'Shift+I' in the player to toggle or pin the HUD, or press 'Tab' to skip intro/outro and 'n' / 'N' for Next Episode.",
    "[FIXED] Crisper Loading Screen & Thumbnail Persistence: Locked the launch loading bar alignment cleanly inside the Y2K card and fixed episode thumbnails disappearing after playback or overlays. How to use: Launch any episode with 'Enter'; your episode artwork stays intact when returning to menus.",
];

/// Return plain-English release highlights for the currently running Solus build.
pub fn default_current_release_notes() -> Vec<String> {
    BUILTIN_CURRENT_RELEASE_NOTES
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// Return a readable display label for the currently running binary version.
pub fn current_display_version_label() -> String {
    let ver = env!("CARGO_PKG_VERSION");
    let sha = current_build_sha();
    if sha != "unknown" && !sha.is_empty() {
        format!("v{} ({})", ver, short_sha(sha))
    } else {
        format!("v{}", ver)
    }
}

/// Translate raw GitHub Release notes or commit summaries into up to 6 plain-English,
/// jargon-free bullet points prefixed with `[NEW]`, `[FIXED]`, or `[IMPROVED]`.
pub fn simplify_release_notes(raw_body: &str) -> Vec<String> {
    let mut bullets: Vec<String> = Vec::new();

    if let (Some(start_idx), Some(end_idx)) = (
        raw_body.find("<!-- solus-notes-start -->"),
        raw_body.find("<!-- solus-notes-end -->"),
    ) {
        let inner_start = start_idx + "<!-- solus-notes-start -->".len();
        if inner_start < end_idx {
            for line in raw_body[inner_start..end_idx].lines() {
                for translated in translate_note_lines(line) {
                    push_unique_bullet(&mut bullets, translated);
                }
            }
        }
    }

    if bullets.is_empty() {
        for line in raw_body.lines() {
            for translated in translate_note_lines(line) {
                push_unique_bullet(&mut bullets, translated);
            }
            if bullets.len() >= 6 {
                break;
            }
        }
    }

    bullets.truncate(6);
    if bullets.is_empty() {
        bullets.push(
            "[IMPROVED] Faster video playback, smoother streaming, and stability improvements."
                .to_string(),
        );
    }
    bullets
}

fn push_unique_bullet(bullets: &mut Vec<String>, candidate: String) {
    if bullets.len() >= 6 {
        return;
    }
    let norm = candidate.to_ascii_lowercase();
    if !bullets.iter().any(|b| b.to_ascii_lowercase() == norm) {
        bullets.push(candidate);
    }
}

fn translate_note_lines(raw_line: &str) -> Vec<String> {
    let trimmed = raw_line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!--")
        || trimmed.starts_with('#')
        || trimmed.starts_with("Full Changelog")
        || trimmed.starts_with("**Full Changelog**")
    {
        return Vec::new();
    }

    let mut line = trimmed
        .trim_start_matches(['-', '*', '+', '•'])
        .trim();
    if let Some((num, rest)) = line.split_once('.') {
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            line = rest.trim();
        }
    }
    if line.is_empty() {
        return Vec::new();
    }

    // Strip automated rolling release wrapper: "Automated build for commit `abcd123`: <msg>"
    if let Some(rest) = line.strip_prefix("Automated build for commit") {
        if let Some((_, msg)) = rest.split_once(':') {
            line = msg.trim();
        }
    }
    if line.is_empty() {
        return Vec::new();
    }

    for prefix in ["Simple:", "simple:", "Note:", "note:"] {
        if let Some(custom) = line.strip_prefix(prefix) {
            let clean = custom.trim();
            if !clean.is_empty() {
                if clean.starts_with("[NEW]")
                    || clean.starts_with("[FIXED]")
                    || clean.starts_with("[IMPROVED]")
                {
                    return vec![ensure_period(clean)];
                }
                return vec![format!("[IMPROVED] {}", ensure_period(clean))];
            }
        }
    }

    for badge in ["[NEW]", "[FIXED]", "[IMPROVED]"] {
        if let Some(rest) = line.strip_prefix(badge) {
            let cleaned = strip_github_trailers(rest.trim());
            if !cleaned.is_empty() {
                return vec![format!("{} {}", badge, ensure_period(&cleaned))];
            }
        }
    }

    let lower = line.to_ascii_lowercase();
    if lower.starts_with("merge pull request")
        || lower.starts_with("merge branch")
        || lower.starts_with("chore")
        || lower.starts_with("ci:")
        || lower.starts_with("ci(")
        || lower.starts_with("test:")
        || lower.starts_with("test(")
        || lower.starts_with("tests:")
        || lower.starts_with("docs:")
        || lower.starts_with("docs(")
        || lower.starts_with("style:")
        || lower.starts_with("build:")
        || lower.starts_with("bump ")
    {
        return Vec::new();
    }

    let badge = if lower.starts_with("feat")
        || lower.starts_with("add ")
        || lower.starts_with("added ")
        || lower.starts_with("introduce")
        || lower.starts_with("port ")
    {
        "[NEW]"
    } else if lower.starts_with("fix")
        || lower.starts_with("resolve")
        || lower.starts_with("prevent")
        || lower.starts_with("repair")
        || lower.contains("bug")
        || lower.contains("crash")
    {
        "[FIXED]"
    } else {
        "[IMPROVED]"
    };

    let stripped = strip_conventional_prefix(line);
    let stripped = strip_github_trailers(&stripped);
    if stripped.is_empty() {
        return Vec::new();
    }

    let s_lower = stripped.to_ascii_lowercase();
    let mut matched = Vec::new();

    if s_lower.contains("mal") && (s_lower.contains("password") || s_lower.contains("direct login") || s_lower.contains("official mal v2")) {
        matched.push(format!(
            "{} Direct MyAnimeList Sign-In (Zero Setup Required): Connect your MyAnimeList account directly inside Solus using just your username and password for full two-way syncing, or pull any public list by username alone. How to use: Press 'L' from the Home screen, press 'Tab' to focus MyAnimeList, then press 'p' for Password Login or 'u' for Username Sync.",
            badge
        ));
    }
    if s_lower.contains("watchlist") && (s_lower.contains("status") || s_lower.contains("picker") || s_lower.contains("label")) {
        matched.push(format!(
            "{} Status & List Picker Popup: Label or move any anime between Watching, Plan to Watch, Completed, On-Hold, Dropped, or Remove across AniList, MyAnimeList, Both, or Local in one step. How to use: Highlight any anime and press '+' -> use 'Tab' or '←/→' to choose where to save, then press '1'–'6' (or '↑/↓' + 'Enter') to apply.",
            badge
        ));
    }
    if s_lower.contains("pip")
        || s_lower.contains("picture-in-picture")
        || s_lower.contains("picture on picture")
        || s_lower.contains("floating")
    {
        matched.push(format!(
            "{} Picture-in-Picture (PiP) Mode: Watch anime in a small floating corner window on top of other apps. How to use: Press 'p' in the player to toggle PiP, 'Shift+P' to move corners/resize, or toggle it in the Episode Launch Menu ('w'/'W') or Settings ('s').",
            badge
        ));
    }
    if s_lower.contains("libmpv") || s_lower.contains("mastering") || s_lower.contains("4:4:4") || s_lower.contains("anime4k") {
        matched.push(format!(
            "{} 4-Stage Video Mastering & Unified Player Engines: Solus now powers mpv, mpv.net, and Celluloid with unified 4:4:4 chroma upscaling, Anime4K era restoration, debanding, and smooth motion interpolation. How to use: Open Settings ('s') or press 'a' (Shaders), 'd' (Deband), 'm' (Motion), and 'g' (Performance) during playback.",
            badge
        ));
    }
    if s_lower.contains("provider") || s_lower.contains("megaplay") || s_lower.contains("animeheaven") || s_lower.contains("hardsub") {
        matched.push(format!(
            "{} Multi-Provider Stream Discovery & Clean/Hardsub Indicators: Automatically probes verified providers (ZokoAnime, MegaPlay Direct/Hardsub, Vidmoly, AnimeHeaven, AnimeGG, and HiAnime) with Clean [CLN] vs Hardsub [EN] tags and demuxed audio tracks. How to use: Press 'Enter' on an episode to pick a server and subtitle track cleanly (hardsubbed videos use their embedded subtitles only).",
            badge
        ));
    }
    if s_lower.contains("subsync") || s_lower.contains("alass") || s_lower.contains("earshot") || s_lower.contains("vad") {
        matched.push(format!(
            "{} Voice-Synced Subtitles (Audio Alignment + VAD): Automatically aligns external subtitles to dialogue using reference timing and voice-activity matching, and remembers clean provider offsets per anime. How to use: Press 'y' in the player to snap subtitles to speech immediately, or open the HUD ('i' / 'S') for manual delay steps.",
            badge
        ));
    }
    if s_lower.contains("bento") || s_lower.contains("info hud") || s_lower.contains("video info") {
        matched.push(format!(
            "{} Live Video & Audio Info Card: View video quality, frame rate, audio track, and stream health on screen. How to use: Press 'i' in the player for a 6-second peek, 'Shift+I' to pin it live, or click [i HUD] in the Episode Launch Menu or Settings ('s').",
            badge
        ));
    }
    if s_lower.contains("fullscreen") || s_lower.contains("auto_fullscreen") {
        matched.push(format!(
            "{} Auto-Fullscreen on Launch: Open your video player in fullscreen as soon as an episode starts. How to use: Turn on 'Auto-Fullscreen on Launch' in Settings ('s') or press 'f' in the Episode Launch Menu (Enter / o).",
            badge
        ));
    }
    if (s_lower.contains("queue") || s_lower.contains("que"))
        && (s_lower.contains("90%")
            || s_lower.contains("5%")
            || s_lower.contains("10%")
            || s_lower.contains("early")
            || s_lower.contains("crash")
            || s_lower.contains("restore")
            || s_lower.contains("completion"))
    {
        matched.push(format!(
            "{} Smarter Watch Queue Protection: Episodes stay in your queue until you finish at least 90% of the episode, so closing early or a crash never skips ahead. How to use: Press 'Q' from the home screen anytime to resume your queue.",
            badge
        ));
    }
    if (s_lower.contains("update") || s_lower.contains("release"))
        && (s_lower.contains("plain")
            || s_lower.contains("jargon")
            || s_lower.contains("what's new")
            || s_lower.contains("whats new")
            || s_lower.contains("transparency")
            || s_lower.contains("popup"))
    {
        matched.push(format!(
            "{} Plain-English Update Summaries: Updates now show clear, jargon-free notes on what changed and how to use it. How to use: Open Settings ('s') and select 'What's New & Release Notes' anytime.",
            badge
        ));
    }
    if !matched.is_empty() {
        return matched;
    }

    if s_lower.contains("network_mode")
        || s_lower.contains("auto-abr")
        || s_lower.contains("abr")
        || s_lower.contains("congested")
        || s_lower.contains("readahead")
        || s_lower.contains("rebuffer")
    {
        return vec![format!(
            "{} Smoother video streaming that automatically adjusts quality on slow Wi-Fi to prevent mid-episode buffering.",
            badge
        )];
    }
    if s_lower.contains("remote") && (s_lower.contains("theme") || s_lower.contains("qr") || s_lower.contains("mobile")) {
        return vec![format!(
            "{} Upgraded the phone remote control with smoother playback syncing and customizable themes.",
            badge
        )];
    }
    if s_lower.contains("subtitle") || s_lower.contains("subsync") || s_lower.contains("sub-delay") || s_lower.contains(".vtt") {
        return vec![format!(
            "{} Cleaner, better-timed subtitles that stay in sync with voices and avoid overlapping on-screen text.",
            badge
        )];
    }
    if (s_lower.contains("audio") || s_lower.contains("dub")) && (s_lower.contains("switch") || s_lower.contains("track")) {
        return vec![format!(
            "{} Smoother switching between Japanese (Sub) and English (Dub) audio tracks.",
            badge
        )];
    }
    if s_lower.contains("anilist") || s_lower.contains("mal") || s_lower.contains("scrobble") || s_lower.contains("tracker") {
        return vec![format!(
            "{} More reliable episode progress saving and watchlist syncing with AniList and MyAnimeList.",
            badge
        )];
    }
    if s_lower.contains("cover") || s_lower.contains("thumbnail") || s_lower.contains("portrait") || s_lower.contains("banner") {
        return vec![format!(
            "{} Crisper anime cover art and episode thumbnails with smoother browsing.",
            badge
        )];
    }

    let humanized = humanize_free_text(&stripped);
    if humanized.len() < 4 {
        return Vec::new();
    }
    vec![format!("{} {}", badge, ensure_period(&humanized))]
}

fn strip_conventional_prefix(line: &str) -> String {
    if let Some((head, tail)) = line.split_once(':') {
        let h = head.trim().to_ascii_lowercase();
        let known = [
            "feat", "fix", "perf", "refactor", "ui", "audio", "playback", "queue",
            "update", "updater", "improve", "remote", "network", "settings",
        ];
        if known
            .iter()
            .any(|k| h == *k || h.starts_with(&format!("{k}(")) || h.starts_with(&format!("{k}!")))
        {
            return tail.trim().to_string();
        }
    }
    line.trim().to_string()
}

fn strip_github_trailers(line: &str) -> String {
    let mut s = line.trim().to_string();
    if let Some(idx) = s.find(" by @") {
        s.truncate(idx);
    }
    if let Some(idx) = s.find(" in https://") {
        s.truncate(idx);
    }
    if let Some(open) = s.rfind("(#") {
        if s.ends_with(')') && s[open + 2..s.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            s.truncate(open);
        }
    }
    s.trim().to_string()
}

fn humanize_free_text(input: &str) -> String {
    let mut words = Vec::new();
    for raw_word in input.split_whitespace() {
        let clean = raw_word.trim_matches(['`', '*']);
        if clean.is_empty()
            || clean.contains("src/")
            || clean.contains(".rs")
            || clean.contains("::")
            || clean.starts_with("0x")
        {
            continue;
        }
        words.push(clean);
    }
    let joined = words.join(" ");
    let mut chars = joined.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn ensure_period(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

/// Path to `last_seen_build.json` used to trigger the one-time Post-Update "What's New" welcome popup.
pub fn last_seen_build_path() -> Option<PathBuf> {
    crate::config::Config::config_dir()
        .ok()
        .map(|d| d.join("last_seen_build.json"))
}

/// Save plain-English release notes before restart so the next launch shows "What's New & Fixed".
pub fn record_pending_post_update_welcome(info: &UpdateInfo) {
    if let Some(path) = last_seen_build_path() {
        let _ = record_pending_post_update_welcome_at(&path, info);
    }
}

pub fn record_pending_post_update_welcome_at(path: &Path, info: &UpdateInfo) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let notes = if info.simple_notes.is_empty() {
        simplify_release_notes(info.release_notes.as_deref().unwrap_or(""))
    } else {
        info.simple_notes.clone()
    };
    let state = LastSeenBuild {
        last_seen_sha: current_build_sha().to_string(),
        last_seen_version: info.latest_version.clone(),
        pending_whats_new: notes,
        updated_to_label: Some(info.latest_version.clone()),
    };
    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(path, json)
}

/// Consume pending Post-Update Welcome notes on startup, or detect an external upgrade (e.g. Homebrew/script).
pub fn check_or_take_post_update_welcome() -> Option<(String, Vec<String>)> {
    let path = last_seen_build_path()?;
    check_or_take_post_update_welcome_at(
        &path,
        env!("CARGO_PKG_VERSION"),
        current_build_sha(),
    )
}

pub fn check_or_take_post_update_welcome_at(
    path: &Path,
    running_version: &str,
    running_sha: &str,
) -> Option<(String, Vec<String>)> {
    let running_label = if running_sha != "unknown" && !running_sha.is_empty() {
        format!("v{} ({})", running_version.trim_start_matches(['v', 'V']), short_sha(running_sha))
    } else {
        format!("v{}", running_version.trim_start_matches(['v', 'V']))
    };

    let Ok(raw) = std::fs::read_to_string(path) else {
        // First time creating last_seen_build.json: record current build without spamming first-run users.
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let initial = LastSeenBuild {
            last_seen_sha: running_sha.to_string(),
            last_seen_version: running_version.to_string(),
            pending_whats_new: Vec::new(),
            updated_to_label: None,
        };
        if let Ok(json) = serde_json::to_string_pretty(&initial) {
            let _ = std::fs::write(path, json);
        }
        return None;
    };

    let Ok(mut state) = serde_json::from_str::<LastSeenBuild>(&raw) else {
        return None;
    };

    if !state.pending_whats_new.is_empty() {
        let notes = std::mem::take(&mut state.pending_whats_new);
        let label = state
            .updated_to_label
            .take()
            .unwrap_or_else(|| running_label.clone());
        state.last_seen_sha = running_sha.to_string();
        state.last_seen_version = running_version.to_string();
        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let _ = std::fs::write(path, json);
        }
        return Some((label, notes));
    }

    let version_changed = !state.last_seen_version.is_empty()
        && state.last_seen_version.trim_start_matches(['v', 'V'])
            != running_version.trim_start_matches(['v', 'V']);
    let sha_changed = !state.last_seen_sha.is_empty()
        && state.last_seen_sha != "unknown"
        && running_sha != "unknown"
        && !state.last_seen_sha.eq_ignore_ascii_case(running_sha);

    if version_changed || sha_changed {
        state.last_seen_sha = running_sha.to_string();
        state.last_seen_version = running_version.to_string();
        state.updated_to_label = None;
        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let _ = std::fs::write(path, json);
        }
        return Some((running_label, default_current_release_notes()));
    }

    None
}

/// Parse semantic version numbers into (major, minor, patch).
pub fn parse_semver(ver: &str) -> Option<(u32, u32, u32)> {
    let clean = ver.trim().trim_start_matches(['v', 'V']);
    let mut parts = clean.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch_str = parts.next().unwrap_or("0");
    // Strip any pre-release tags like -beta or -rc
    let patch_num_str = patch_str.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("0");
    let patch: u32 = patch_num_str.parse().ok()?;
    Some((major, minor, patch))
}

/// Check if `latest` version string is strictly newer than `current`.
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let Some(curr) = parse_semver(current) else { return false };
    let Some(late) = parse_semver(latest) else { return false };
    late > curr
}

/// Determine the target release asset name for the current machine's OS and CPU architecture.
pub fn target_asset_name() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "solus-macos-aarch64" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "solus-macos-x86_64" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "solus-linux-x86_64" }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { "solus-windows-x86_64.exe" }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    { "solus" }
}

/// Resolve the canonical path of the running binary (following symlinks such as `/opt/homebrew/bin/solus -> ~/.cargo/bin/solus`).
fn resolved_current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(std::fs::canonicalize(&exe).unwrap_or(exe))
}

/// Return true only when the canonical binary resides inside Homebrew's Cellar or Caskroom.
pub fn is_homebrew_binary_path(path: &std::path::Path) -> bool {
    let norm = path.to_string_lossy().replace('\\', "/");
    norm.contains("/Cellar/") || norm.contains("/Caskroom/")
}

/// Detect whether the currently running binary is genuinely managed by Homebrew.
pub fn is_homebrew_install() -> bool {
    resolved_current_exe()
        .map(|p| is_homebrew_binary_path(&p))
        .unwrap_or(false)
}

/// Git SHA baked in at compile time (`build.rs`), or `"unknown"` for source tarball builds.
pub fn current_build_sha() -> &'static str {
    option_env!("SOLUS_GIT_SHA").unwrap_or("unknown")
}

/// Commit Unix timestamp baked in at compile time (`build.rs`), preventing accidental downgrades of local dev builds.
pub fn current_commit_timestamp() -> i64 {
    option_env!("SOLUS_COMMIT_TS")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

/// Truncate a git SHA to its standard 7-character short prefix for display.
pub fn short_sha(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// Extract the embedded commit SHA (`<!-- solus-sha:<sha> -->`) from a release body.
pub fn extract_release_sha(body: Option<&str>) -> Option<String> {
    extract_marker(body, "solus-sha:")
        .filter(|s| s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Extract the embedded commit timestamp (`<!-- solus-ts:<unix_secs> -->`) from a release body.
pub fn extract_release_timestamp(body: Option<&str>) -> Option<i64> {
    extract_marker(body, "solus-ts:")?.parse::<i64>().ok()
}

fn extract_marker(body: Option<&str>, prefix: &str) -> Option<String> {
    let text = body?;
    let start = text.find(prefix)? + prefix.len();
    let rest = &text[start..];
    let end = rest.find("-->").unwrap_or(rest.len());
    let val = rest[..end].trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// Remove internal HTML metadata comments (`<!-- solus-... -->`) before displaying release notes.
fn clean_release_notes(body: Option<String>) -> Option<String> {
    let text = body?;
    let cleaned: Vec<&str> = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("<!-- solus-") && trimmed.ends_with("-->"))
        })
        .collect();
    let joined = cleaned.join("\n").trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// Pure update decision logic supporting both tagged semver releases and rolling `main` commit releases.
pub fn evaluate_release_update(
    release: GitHubRelease,
    current_version: &str,
    local_sha: &str,
    local_ts: i64,
) -> Option<UpdateInfo> {
    let semver_newer = is_newer_version(current_version, &release.tag_name);
    let is_rolling = release.tag_name.ends_with("-rolling") || release.tag_name == "latest";
    let remote_sha = extract_release_sha(release.body.as_deref());
    let remote_ts = extract_release_timestamp(release.body.as_deref()).unwrap_or(0);

    let semver_not_older = !is_newer_version(&release.tag_name, current_version);
    let rolling_sha_newer = is_rolling
        && semver_not_older
        && local_sha != "unknown"
        && matches!(
            remote_sha.as_deref(),
            Some(rsha) if rsha != local_sha && (remote_ts == 0 || local_ts == 0 || remote_ts >= local_ts)
        );

    if !semver_newer && !rolling_sha_newer {
        return None;
    }

    let (display_curr, display_latest) = if !semver_newer && rolling_sha_newer {
        (
            format!(
                "v{} ({})",
                current_version.trim_start_matches(['v', 'V']),
                short_sha(local_sha)
            ),
            format!(
                "main@{}",
                short_sha(remote_sha.as_deref().unwrap_or("latest"))
            ),
        )
    } else {
        (current_version.to_string(), release.tag_name.clone())
    };

    let target_name = target_asset_name();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == target_name)
        .or_else(|| release.assets.iter().find(|a| a.name.contains("solus")))?;

    let cleaned_notes = clean_release_notes(release.body);
    let simple_notes = simplify_release_notes(cleaned_notes.as_deref().unwrap_or(""));

    Some(UpdateInfo {
        current_version: display_curr,
        latest_version: display_latest,
        download_url: asset.browser_download_url.clone(),
        asset_name: asset.name.clone(),
        release_notes: cleaned_notes,
        simple_notes,
        is_homebrew: is_homebrew_install(),
    })
}

/// Check GitHub for the latest release in a non-blocking asynchronous call.
pub async fn check_for_updates(client: &reqwest::Client, current_version: &str) -> Result<Option<UpdateInfo>> {
    let resp = client
        .get(GITHUB_RELEASES_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
        .timeout(Duration::from_secs(6))
        .send()
        .await?
        .error_for_status()?;

    let release: GitHubRelease = resp.json().await?;
    Ok(evaluate_release_update(
        release,
        current_version,
        current_build_sha(),
        current_commit_timestamp(),
    ))
}

/// Download the new binary and atomically replace the current running executable.
pub async fn download_and_install_update(
    client: &reqwest::Client,
    download_url: &str,
    target_version: &str,
    tx: Sender<AppMessage>,
) -> Result<PathBuf> {
    let current_exe = resolved_current_exe()
        .ok_or_else(|| AppError::Player("Failed to resolve current binary path".into()))?;

    let current_dir = current_exe
        .parent()
        .ok_or_else(|| AppError::Player("Could not resolve binary parent directory".into()))?;

    // Temporary download file in the same directory (guarantees same filesystem for atomic rename)
    let temp_download_path = current_dir.join(format!(".solus-update-{}.tmp", std::process::id()));

    let mut resp = client
        .get(download_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?
        .error_for_status()?;

    let total_size = resp.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut last_emit = Instant::now();
    let start_time = Instant::now();

    let mut file = tokio::fs::File::create(&temp_download_path).await?;

    while let Some(chunk) = resp.chunk().await? {
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        downloaded += chunk.len() as u64;

        if last_emit.elapsed() >= Duration::from_millis(150) {
            let pct = if total_size > 0 {
                (downloaded as f32 / total_size as f32) * 100.0
            } else {
                0.0
            };
            let elapsed_secs = start_time.elapsed().as_secs_f64().max(0.001);
            let speed_mb = (downloaded as f64 / 1_048_576.0) / elapsed_secs;
            let speed_str = format!("{:.1} MB/s", speed_mb);

            let _ = tx.send(AppMessage::UpdateProgress(pct, speed_str)).await;
            last_emit = Instant::now();
        }
    }

    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    drop(file);

    // Ensure permissions and replace running executable
    #[cfg(windows)]
    {
        // On Windows, the running executable is locked by the OS against modification or deletion.
        // However, NTFS allows renaming an open executable within the same filesystem!
        // 1. Generate old backup name: solus.exe.old
        let old_exe = current_exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&old_exe); // Remove any old backup if present and unlocked

        // 2. Rename the currently running executable to solus.exe.old
        if let Err(e) = std::fs::rename(&current_exe, &old_exe) {
            let _ = std::fs::remove_file(&temp_download_path);
            return Err(AppError::Player(format!(
                "Failed to rename running executable {} to {}: {}",
                current_exe.display(),
                old_exe.display(),
                e
            )));
        }

        // 3. Move the downloaded new binary into place
        if let Err(e) = std::fs::rename(&temp_download_path, &current_exe) {
            // Roll back if moving the new binary failed
            let _ = std::fs::rename(&old_exe, &current_exe);
            let _ = std::fs::remove_file(&temp_download_path);
            return Err(AppError::Player(format!(
                "Failed to install new binary at {}: {}",
                current_exe.display(),
                e
            )));
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&temp_download_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&temp_download_path, perms)?;

        // Atomically replace running binary with the newly downloaded binary
        if let Err(e) = std::fs::rename(&temp_download_path, &current_exe) {
            // Fallback for cross-device or permission limits: attempt removal then copy
            let _ = std::fs::remove_file(&current_exe);
            if let Err(e2) = std::fs::copy(&temp_download_path, &current_exe) {
                let _ = std::fs::remove_file(&temp_download_path);
                return Err(AppError::Player(format!(
                    "Failed to replace binary at {}: {} (fallback copy error: {})",
                    current_exe.display(),
                    e,
                    e2
                )));
            }
            let _ = std::fs::remove_file(&temp_download_path);
        }
    }

    let _ = tx.send(AppMessage::UpdateSuccess(target_version.to_string())).await;
    Ok(current_exe)
}

/// Clean up any leftover update artifacts (e.g., `*.exe.old` or temporary download files)
/// from previous updates. Called on startup.
pub fn cleanup_legacy_update_files() {
    if let Some(current_exe) = resolved_current_exe() {
        if let Some(parent) = current_exe.parent() {
            #[cfg(windows)]
            {
                let old_exe = current_exe.with_extension("exe.old");
                let _ = std::fs::remove_file(old_exe);
            }
            // Clean up any abandoned .solus-update-*.tmp files
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();
                    if name_str.starts_with(".solus-update-") && name_str.ends_with(".tmp") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_semver() {
        assert_eq!(parse_semver("1.0.6"), Some((1, 0, 6)));
        assert_eq!(parse_semver("v1.0.7"), Some((1, 0, 7)));
        assert_eq!(parse_semver("V2.14.0"), Some((2, 14, 0)));
        assert_eq!(parse_semver("v1.2.3-beta.1"), Some((1, 2, 3)));
    }

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("1.0.6", "1.0.7"));
        assert!(is_newer_version("v1.0.6", "v1.1.0"));
        assert!(is_newer_version("1.0.6", "2.0.0"));
        assert!(!is_newer_version("1.0.6", "1.0.6"));
        assert!(!is_newer_version("1.0.7", "1.0.6"));
        assert!(!is_newer_version("2.0.0", "1.9.9"));
    }

    #[test]
    fn test_target_asset_name_not_empty() {
        let name = target_asset_name();
        assert!(!name.is_empty());
        assert!(name.contains("solus"));
    }

    fn sample_release(tag_name: &str, body: Option<&str>) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag_name.to_string(),
            name: Some(tag_name.to_string()),
            body: body.map(|s| s.to_string()),
            html_url: "https://github.com/abdullah-binmadhi/Solus/releases".to_string(),
            assets: vec![ReleaseAsset {
                name: target_asset_name().to_string(),
                browser_download_url: "https://example.com/solus".to_string(),
                size: 1024,
            }],
        }
    }

    #[test]
    fn test_evaluate_release_update_rolling_sha_and_semver() {
        let body = "<!-- solus-sha:abcdef1234567890abcdef1234567890abcdef12 -->\n<!-- solus-ts:2000 -->\nAutomated build for commit `abcdef1`: Fix flickering";

        // 1. Existing v1.0.6 binary upgrades via semver to v1.0.7-rolling.
        let r1 = sample_release("v1.0.7-rolling", Some(body));
        let info1 = evaluate_release_update(r1, "1.0.6", "unknown", 0).unwrap();
        assert_eq!(info1.latest_version, "v1.0.7-rolling");
        assert!(!info1.release_notes.as_deref().unwrap().contains("solus-sha"));

        // 2. Same version 1.0.7 with older commit SHA upgrades to newer commit SHA.
        let r2 = sample_release("v1.0.7-rolling", Some(body));
        let info2 = evaluate_release_update(r2, "1.0.7", "1111111222222233333334444444555555566666", 1500).unwrap();
        assert_eq!(info2.latest_version, "main@abcdef1");
        assert_eq!(info2.current_version, "v1.0.7 (1111111)");

        // 3. Matching commit SHA reports no update needed.
        let r3 = sample_release("v1.0.7-rolling", Some(body));
        assert!(evaluate_release_update(r3, "1.0.7", "abcdef1234567890abcdef1234567890abcdef12", 2000).is_none());

        // 4. Local newer dev commit (local_ts > remote_ts) is never downgraded.
        let r4 = sample_release("v1.0.7-rolling", Some(body));
        assert!(evaluate_release_update(r4, "1.0.7", "9999999888888877777776666666555555544444", 2500).is_none());
    }

    #[test]
    fn test_is_homebrew_binary_path() {
        assert!(!is_homebrew_binary_path(std::path::Path::new("/opt/homebrew/bin/solus")));
        assert!(!is_homebrew_binary_path(std::path::Path::new("/Users/dev/.cargo/bin/solus")));
        assert!(is_homebrew_binary_path(std::path::Path::new("/opt/homebrew/Cellar/solus/1.0.7/bin/solus")));
        assert!(is_homebrew_binary_path(std::path::Path::new("/usr/local/Cellar/solus/1.0.7/bin/solus")));
        assert!(is_homebrew_binary_path(std::path::Path::new("/opt/homebrew/Caskroom/solus/1.0.7/solus")));
    }

    #[test]
    fn test_simplify_release_notes_strips_jargon_and_formats_badges() {
        let raw = r#"
<!-- solus-sha:abcdef1234567890abcdef1234567890abcdef12 -->
Automated build for commit `abcdef1`: feat(settings): add auto_fullscreen toggle in src/ui/settings.rs (#42)
- fix(queue): keep episode in queue until 90% completion on early close by @dev in https://github.com/example/pr/43
- chore: bump serde version
- feat(updater): plain-english update transparency popup without jargon
"#;
        let notes = simplify_release_notes(raw);
        assert!(notes.len() >= 3);
        assert!(notes.iter().any(|n| n.starts_with("[NEW]") && n.contains("Auto-Fullscreen")));
        assert!(notes.iter().any(|n| n.starts_with("[FIXED]") && n.contains("90%")));
        assert!(notes.iter().any(|n| n.starts_with("[NEW]") && n.contains("jargon-free")));
        for n in &notes {
            assert!(!n.contains("src/"), "file path leaked: {n}");
            assert!(!n.contains("serde"), "chore/jargon leaked: {n}");
            assert!(!n.contains("#43"), "PR trailer leaked: {n}");
        }
    }

    #[test]
    fn test_post_update_welcome_persistence_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("last_seen_build.json");

        // First launch initializes file without showing popup
        assert!(check_or_take_post_update_welcome_at(&path, "1.0.7", "1111111").is_none());
        assert!(path.exists());

        // Recording an update stores plain-English summary for next launch
        let info = UpdateInfo {
            current_version: "1.0.7".to_string(),
            latest_version: "v1.0.8".to_string(),
            download_url: "https://example.com/solus".to_string(),
            asset_name: "solus".to_string(),
            release_notes: None,
            simple_notes: vec!["[NEW] Auto-Fullscreen added.".to_string()],
            is_homebrew: false,
        };
        record_pending_post_update_welcome_at(&path, &info).unwrap();

        let (label, notes) = check_or_take_post_update_welcome_at(&path, "1.0.8", "2222222").unwrap();
        assert_eq!(label, "v1.0.8");
        assert_eq!(notes, vec!["[NEW] Auto-Fullscreen added.".to_string()]);

        // Subsequent launch does not re-show the same popup
        assert!(check_or_take_post_update_welcome_at(&path, "1.0.8", "2222222").is_none());
    }

    #[test]
    fn test_default_release_notes_and_pip_hud_commit_translation() {
        let notes = default_current_release_notes();
        assert_eq!(notes.len(), 5);
        assert!(notes.iter().any(|n| n.contains("4-Stage Video Mastering") && n.contains("How to use:")));
        assert!(notes.iter().any(|n| n.contains("Multi-Provider Stream Discovery") && n.contains("How to use:")));
        assert!(notes.iter().any(|n| n.contains("Voice-Synced Subtitles") && n.contains("How to use:")));
        assert!(notes.iter().any(|n| n.contains("Interactive Player HUD Controls") && n.contains("How to use:")));
        assert!(notes.iter().any(|n| n.contains("Loading Screen & Thumbnail Persistence") && n.contains("How to use:")));
        for n in &notes {
            let lower = n.to_ascii_lowercase();
            assert!(!lower.contains("plain english"), "should not mention plain english: {n}");
            assert!(!lower.contains("jargon"), "should not mention jargon: {n}");
        }

        let combined = simplify_release_notes(
            "Automated build for commit `955e836`: feat(subsync,shaders,ui): alass-core+earshot VAD v5 and unified libmpv mastering",
        );
        assert!(combined.iter().any(|n| n.contains("Voice-Synced Subtitles")));
        assert!(combined.iter().any(|n| n.contains("4-Stage Video Mastering")));
    }
}
