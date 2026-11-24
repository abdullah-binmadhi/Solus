//! Watch Session Queue, background `N+1` prefetcher, temp session cache, and portable JSON playlists.

use crate::{
    api::{
        player::PlayOptions,
        subtitles::{AudioTrackOption, SubtitleOption},
    },
    config::{Anime4KMode, Config},
    db::playlists::{PlaylistItemDraft, SavedPlaylistItem},
    error::{AppError, Result},
    services::{
        downloader::parse_episode_range,
        shaders::build_shader_arg_for_year,
        subsync::ensure_synchronized_subtitle,
        substyler::SubtitleStyleConfig,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    time::Duration,
};

/// Short 3-minute freshness window for near-EOF pre-resolved CDN stream links.
pub const PREFETCH_TTL_SECS: u64 = 180;

/// Episode completion ratio (90% watched / 10% remaining) required before an active
/// queued episode is considered finished and permanently removed from the queue.
pub const QUEUE_COMPLETION_THRESHOLD: f64 = 0.90;

/// Fixed monochrome status label for a queued item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchStatus {
    Ready,
    Resolving,
    Queued,
    Stale,
}

impl PrefetchStatus {
    pub fn badge(self) -> &'static str {
        match self {
            PrefetchStatus::Ready => "[ READY ]",
            PrefetchStatus::Resolving => "[WARMING]",
            PrefetchStatus::Queued => "[QUEUED ]",
            PrefetchStatus::Stale => "[ STALE ]",
        }
    }

    pub fn remote_badge(self) -> &'static str {
        match self {
            PrefetchStatus::Ready => "READY",
            PrefetchStatus::Resolving => "WARM",
            PrefetchStatus::Queued => "QUEUE",
            PrefetchStatus::Stale => "STALE",
        }
    }
}

/// Pre-resolved playback payload ready for instant `mpv`/`vlc` launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedPrefetch {
    pub options:       PlayOptions,
    pub resolved_at:   u64,
    #[serde(default)]
    pub sub_synced:    bool,
    #[serde(default)]
    pub master_url:    Option<String>,
    #[serde(default)]
    pub subtitles:     Vec<SubtitleOption>,
    #[serde(default)]
    pub audio_tracks:  Vec<AudioTrackOption>,
    #[serde(default)]
    pub dub_available: bool,
    #[serde(default)]
    pub ladder:        Option<crate::api::subtitles::ResolvedStreamLadder>,
}

impl ResolvedPrefetch {
    /// Returns true if the pre-resolved URL is within the conservative TTL window.
    pub fn is_fresh(&self, now_unix: u64) -> bool {
        now_unix >= self.resolved_at && now_unix.saturating_sub(self.resolved_at) < PREFETCH_TTL_SECS
    }

    /// Seconds remaining before cached link expires.
    pub fn remaining_secs(&self, now_unix: u64) -> u64 {
        let elapsed = now_unix.saturating_sub(self.resolved_at);
        PREFETCH_TTL_SECS.saturating_sub(elapsed)
    }
}

/// Single queued episode with durable intent + optional cached prefetch snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    #[serde(default)]
    pub uid:          u64,
    #[serde(default)]
    pub spec_version: u64,
    pub anime_id:     i64,
    #[serde(default)]
    pub mal_id:       Option<i64>,
    #[serde(default)]
    pub season_year:  Option<i64>,
    pub title:        String,
    pub query:        String,
    pub episode:      u32,
    pub quality:      String,
    pub dub:          bool,
    pub sub_lang:     String,
    pub player:       String,
    #[serde(skip)]
    pub resolving:    bool,
    #[serde(default)]
    pub prefetch:     Option<ResolvedPrefetch>,
}

impl QueueItem {
    pub fn new(
        anime_id: i64,
        mal_id: Option<i64>,
        season_year: Option<i64>,
        title: String,
        query: String,
        episode: u32,
        quality: String,
        dub: bool,
        sub_lang: String,
        player: String,
    ) -> Self {
        Self {
            uid: 0,
            spec_version: 1,
            anime_id,
            mal_id,
            season_year,
            title,
            query,
            episode,
            quality,
            dub,
            sub_lang,
            player,
            resolving: false,
            prefetch: None,
        }
    }

    pub fn from_saved(saved: &SavedPlaylistItem) -> Self {
        Self {
            uid: 0,
            spec_version: 1,
            anime_id: saved.anime_id,
            mal_id: saved.mal_id,
            season_year: saved.season_year,
            title: saved.title.clone(),
            query: saved.query.clone(),
            episode: saved.episode,
            quality: saved.quality.clone(),
            dub: saved.dub,
            sub_lang: saved.sub_lang.clone(),
            player: saved.player.clone(),
            resolving: false,
            prefetch: None,
        }
    }

    /// Invalidate any cached URL snapshot after modifying item playback configuration.
    pub fn bump_spec(&mut self) {
        self.spec_version = self.spec_version.saturating_add(1);
        self.prefetch = None;
        self.resolving = false;
    }

    pub fn to_draft(&self) -> PlaylistItemDraft {
        PlaylistItemDraft {
            anime_id: self.anime_id,
            mal_id: self.mal_id,
            title: self.title.clone(),
            query: self.query.clone(),
            episode: self.episode,
            season_year: self.season_year,
            quality: self.quality.clone(),
            dub: self.dub,
            sub_lang: self.sub_lang.clone(),
            player: self.player.clone(),
        }
    }

    pub fn status(&self, now_unix: u64) -> PrefetchStatus {
        if self.resolving {
            PrefetchStatus::Resolving
        } else if let Some(ref pre) = self.prefetch {
            if pre.is_fresh(now_unix) {
                PrefetchStatus::Ready
            } else {
                PrefetchStatus::Stale
            }
        } else {
            PrefetchStatus::Queued
        }
    }
}

/// Metadata tracking the currently playing episode (whether started from queue or directly).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveNowItem {
    pub item:               QueueItem,
    #[serde(default)]
    pub used_prefetched:    bool,
    #[serde(default)]
    pub retried_once:       bool,
    /// True when this playback session was launched from the Session Queue / Playlist.
    #[serde(default)]
    pub from_queue:         bool,
    /// True once playback reached >= 90% (`QUEUE_COMPLETION_THRESHOLD`) or natural EOF.
    #[serde(default)]
    pub completion_reached: bool,
    /// Latest observed `pos / duration` ratio (`0.0 ..= 1.0`).
    #[serde(default)]
    pub progress_ratio:     f64,
}

/// Result returned when inserting an item into the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnqueueOutcome {
    pub position: usize,
    pub replaced: bool,
}

/// Session watch queue persisted temporarily in `<cache_dir>/active_queue_<profile>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchQueue {
    #[serde(default)]
    pub items:        VecDeque<QueueItem>,
    #[serde(default = "default_true")]
    pub auto_advance: bool,
    #[serde(default)]
    pub loop_mode:    bool,
    #[serde(skip)]
    pub manual_stop:  bool,
    #[serde(skip)]
    pub next_uid:     u64,
    #[serde(default)]
    pub active_item:  Option<ActiveNowItem>,
}

fn default_true() -> bool {
    true
}

impl Default for WatchQueue {
    fn default() -> Self {
        Self {
            items: VecDeque::new(),
            auto_advance: true,
            loop_mode: false,
            manual_stop: false,
            next_uid: 1,
            active_item: None,
        }
    }
}

impl WatchQueue {
    fn alloc_uid(&mut self) -> u64 {
        let uid = self.next_uid.max(1);
        self.next_uid = uid.saturating_add(1);
        uid
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Re-assign monotonic unique IDs and clear transient flags after deserialization.
    /// If the previous session crashed mid-episode while playing a queued item before 90%
    /// completion, restore it to index `0` so the user can pick up right where they left off.
    pub fn normalize_uids(&mut self) {
        if let Some(active) = self.active_item.take() {
            if active.from_queue && !active.completion_reached {
                self.items.retain(|existing| {
                    !(existing.anime_id == active.item.anime_id
                        && existing.episode == active.item.episode)
                });
                let mut restored = active.item;
                restored.resolving = false;
                self.items.push_front(restored);
            }
        }
        self.next_uid = 1;
        for item in &mut self.items {
            item.uid = self.next_uid;
            self.next_uid += 1;
            item.spec_version = item.spec_version.max(1);
            item.resolving = false;
        }
        self.manual_stop = false;
    }

    /// Update live playback completion progress for the currently active episode.
    /// Returns `true` the first time the episode crosses `QUEUE_COMPLETION_THRESHOLD` (90%).
    pub fn record_active_progress(
        &mut self,
        anime_id: i64,
        episode: u32,
        pos: f64,
        duration: f64,
    ) -> bool {
        let Some(ref mut active) = self.active_item else {
            return false;
        };
        if active.item.anime_id != anime_id || active.item.episode != episode || duration < 60.0 {
            return false;
        }
        let ratio = (pos / duration).clamp(0.0, 1.0);
        if ratio > active.progress_ratio {
            active.progress_ratio = ratio;
        }
        if !active.completion_reached && active.progress_ratio >= QUEUE_COMPLETION_THRESHOLD {
            active.completion_reached = true;
            return true;
        }
        false
    }

    /// Mark the currently active item as completed (e.g., on natural EOF).
    pub fn mark_active_completed(&mut self) {
        if let Some(ref mut active) = self.active_item {
            active.completion_reached = true;
            active.progress_ratio = active.progress_ratio.max(1.0);
        }
    }

    /// If an episode launched from the queue closed early or crashed before reaching 90%
    /// completion, restore it to the front (`index 0`) of the queue so it can be resumed easily.
    pub fn restore_unfinished_active_to_front(&mut self) -> Option<QueueItem> {
        let active = self.active_item.take()?;
        if !active.from_queue || active.completion_reached {
            return None;
        }
        self.items.retain(|existing| {
            !(existing.anime_id == active.item.anime_id
                && existing.episode == active.item.episode)
        });
        let mut restored = active.item;
        restored.uid = self.alloc_uid();
        restored.spec_version = restored.spec_version.max(1);
        restored.resolving = false;
        self.items.push_front(restored.clone());
        Some(restored)
    }

    /// Append to tail unless an identical `(anime_id, episode)` is already queued (in which case refresh metadata).
    pub fn enqueue_back(&mut self, mut item: QueueItem) -> EnqueueOutcome {
        if let Some(pos) = self
            .items
            .iter()
            .position(|existing| existing.anime_id == item.anime_id && existing.episode == item.episode)
        {
            if let Some(existing) = self.items.get_mut(pos) {
                existing.quality = item.quality;
                existing.dub = item.dub;
                existing.sub_lang = item.sub_lang;
                existing.player = item.player;
                existing.bump_spec();
            }
            return EnqueueOutcome {
                position: pos + 1,
                replaced: true,
            };
        }
        item.uid = self.alloc_uid();
        item.spec_version = item.spec_version.max(1);
        self.items.push_back(item);
        EnqueueOutcome {
            position: self.items.len(),
            replaced: false,
        }
    }

    /// Update an item in-place by `uid`, bumping its `spec_version` and clearing stale cached URLs.
    pub fn update_item_by_uid(&mut self, uid: u64, mut f: impl FnMut(&mut QueueItem)) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.uid == uid) {
            f(item);
            item.bump_spec();
            true
        } else {
            false
        }
    }

    /// Insert (or promote) `(anime_id, episode)` at index `0` so it plays next.
    pub fn enqueue_next(&mut self, mut item: QueueItem) -> EnqueueOutcome {
        let mut replaced = false;
        if let Some(pos) = self
            .items
            .iter()
            .position(|existing| existing.anime_id == item.anime_id && existing.episode == item.episode)
        {
            self.items.remove(pos);
            replaced = true;
        }
        item.uid = self.alloc_uid();
        self.items.push_front(item);
        EnqueueOutcome {
            position: 1,
            replaced,
        }
    }

    /// Remove item at `idx`.
    pub fn remove_at(&mut self, idx: usize) -> Option<QueueItem> {
        self.items.remove(idx)
    }

    /// Swap item at `idx` toward the front; returns new cursor index.
    pub fn move_up(&mut self, idx: usize) -> usize {
        if idx > 0 && idx < self.items.len() {
            self.items.swap(idx, idx - 1);
            idx - 1
        } else {
            idx.min(self.items.len().saturating_sub(1))
        }
    }

    /// Swap item at `idx` toward the tail; returns new cursor index.
    pub fn move_down(&mut self, idx: usize) -> usize {
        if idx + 1 < self.items.len() {
            self.items.swap(idx, idx + 1);
            idx + 1
        } else {
            idx.min(self.items.len().saturating_sub(1))
        }
    }

    /// Clear all queued items (leaves `auto_advance`, `loop_mode`, and `active_item` intact).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Fisher-Yates shuffle without external RNG dependencies.
    pub fn shuffle(&mut self) {
        let len = self.items.len();
        if len <= 1 {
            return;
        }
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        seed ^= len as u64 ^ 0x9E37_79B9_7F4A_7C15;

        for i in (1..len).rev() {
            seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = seed;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let j = (z as usize) % (i + 1);
            self.items.swap(i, j);
        }
    }

    /// Pop the next episode to play; if `loop_mode` is active and the active item completed, rotate it to the tail.
    pub fn pop_next_for_advance(&mut self) -> Option<QueueItem> {
        if self.loop_mode {
            if let Some(finished) = self.active_item.take() {
                if finished.completion_reached {
                    let mut recycled = finished.item;
                    recycled.uid = self.alloc_uid();
                    recycled.resolving = false;
                    recycled.prefetch = None;
                    self.items.push_back(recycled);
                }
            }
        } else {
            self.active_item = None;
        }
        self.items.pop_front()
    }

    /// Return the next un-warmed or expired front item to background-prefetch.
    pub fn next_prefetch_target(&self, now_unix: u64) -> Option<QueueItem> {
        let head = self.items.front()?;
        if head.resolving {
            return None;
        }
        let is_fresh = head
            .prefetch
            .as_ref()
            .is_some_and(|p| p.is_fresh(now_unix));
        if is_fresh {
            None
        } else {
            Some(head.clone())
        }
    }

    /// Mark an item by `uid` as actively resolving or idle.
    pub fn set_resolving(&mut self, uid: u64, resolving: bool) {
        if let Some(item) = self.items.iter_mut().find(|i| i.uid == uid) {
            item.resolving = resolving;
        }
    }

    /// Store resolved prefetch payload onto matching `uid` item if its config version is unchanged.
    pub fn apply_prefetch_result(
        &mut self,
        uid: u64,
        expected_spec_version: u64,
        prefetch: Option<ResolvedPrefetch>,
    ) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.uid == uid) {
            item.resolving = false;
            if item.spec_version != expected_spec_version {
                return false;
            }
            if let Some(pre) = prefetch {
                item.prefetch = Some(pre);
                return true;
            }
        }
        false
    }

    /// Clear expired cached prefetches so UI status and workers re-warm accurately.
    pub fn invalidate_stale(&mut self, now_unix: u64) {
        for item in &mut self.items {
            if item
                .prefetch
                .as_ref()
                .is_some_and(|p| !p.is_fresh(now_unix))
            {
                item.prefetch = None;
            }
        }
    }
}

/// Sanitize a profile ID into a filesystem-safe token.
fn safe_profile_token(profile_id: &str) -> String {
    let cleaned: String = profile_id
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "default".to_string()
    } else {
        cleaned
    }
}

/// Path to the profile's temporary session queue file inside the OS cache directory.
pub fn active_queue_path(profile_id: &str) -> Result<PathBuf> {
    let token = safe_profile_token(profile_id);
    Ok(Config::cache_dir()?.join(format!("active_queue_{token}.json")))
}

/// Best-effort save of the temp session queue to `<cache_dir>/active_queue_<profile>.json`.
pub fn save_active_queue(profile_id: &str, queue: &WatchQueue) {
    let Ok(path) = active_queue_path(profile_id) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(queue) {
        let _ = std::fs::write(path, json);
    }
}

/// Load temp session queue for `profile_id`, purging stale links and resetting transient flags.
pub fn load_active_queue(profile_id: &str, now_unix: u64) -> WatchQueue {
    let Ok(path) = active_queue_path(profile_id) else {
        return WatchQueue::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return WatchQueue::default();
    };
    let Ok(mut queue) = serde_json::from_str::<WatchQueue>(&raw) else {
        return WatchQueue::default();
    };
    queue.normalize_uids();
    queue.invalidate_stale(now_unix);
    queue
}

/// Delete a profile's temp session queue file when deleting that profile.
pub fn remove_active_queue_file(profile_id: &str) {
    if let Ok(path) = active_queue_path(profile_id) {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Portable JSON playlist export/share envelope (`profiles/<id>/playlists/<slug>.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortablePlaylist {
    #[serde(default = "default_schema")]
    pub schema:       u32,
    pub name:         String,
    #[serde(default)]
    pub loop_default: bool,
    pub items:        Vec<PlaylistItemDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistTransferKind {
    Export { playlist_id: i64, name: String, loop_default: bool },
    Import,
}

fn default_schema() -> u32 {
    1
}

/// Profile-scoped directory for exported JSON playlist packs.
pub fn playlists_export_dir(profile_id: &str) -> Result<PathBuf> {
    let token = safe_profile_token(profile_id);
    Ok(Config::config_dir()?
        .join("profiles")
        .join(token)
        .join("playlists"))
}

fn slugify_playlist_name(name: &str) -> String {
    let mut slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "playlist".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

pub fn clean_user_path(raw: &str) -> PathBuf {
    let mut text = raw.trim().to_string();
    if text.len() >= 2 {
        let bytes = text.as_bytes();
        if (bytes[0] == b'\'' && bytes[text.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[text.len() - 1] == b'"')
        {
            text = text[1..text.len() - 1].trim().to_string();
        }
    }
    #[cfg(unix)] {
        text = text.replace("\\ ", " ");
    }
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
        if let Some(base) = directories::BaseDirs::new() {
            return base.home_dir().join(rest);
        }
    }
    PathBuf::from(text)
}

fn slug_for_name(name: &str) -> String {
    let mut slug: String = name.to_lowercase().chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while slug.contains("--") { slug = slug.replace("--", "-"); }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() { "playlist".to_string() } else { slug.chars().take(48).collect() }
}

pub fn resolve_export_target(raw_input: &str, playlist_name: &str) -> Result<PathBuf> {
    let mut path = clean_user_path(raw_input);
    if path.as_os_str().is_empty() {
        path = PathBuf::from(format!("{}.json", slug_for_name(playlist_name)));
    } else {
        let input = raw_input.trim();
        let directory_hint = path.is_dir() || input.ends_with('/')
            || (cfg!(windows) && input.ends_with('\\'));
        if directory_hint {
            path = path.join(format!("{}.json", slug_for_name(playlist_name)));
        } else if path.extension().is_none()
            || path.extension().and_then(|e| e.to_str()).map(|e| !e.eq_ignore_ascii_case("json")).unwrap_or(false)
        {
            path.set_extension("json");
        }
    }
    Ok(path)
}

pub fn export_playlist_json_to_path(
    target: &Path,
    name: &str,
    loop_default: bool,
    items: &[PlaylistItemDraft],
) -> Result<PathBuf> {
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() { std::fs::create_dir_all(parent)?; }
    }
    let envelope = PortablePlaylist { schema: 1, name: name.to_string(), loop_default, items: items.to_vec() };
    let json = serde_json::to_string_pretty(&envelope)
        .map_err(|e| AppError::Config(format!("Failed to serialize playlist: {e}")))?;
    std::fs::write(target, json)?;
    Ok(target.to_path_buf())
}

pub fn scan_portable_playlists_in_dir(dir: &Path) -> Vec<(PathBuf, PortablePlaylist)> {
    let mut found = Vec::new();
    if dir.is_file() {
        if dir.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("json")) == Some(true) {
            if let Ok(pack) = import_playlist_json(dir) {
                if !pack.items.is_empty() { found.push((dir.to_path_buf(), pack)); }
            }
        }
        return found;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return found; };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.path().to_string_lossy().to_lowercase());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("json")) == Some(true) {
            if let Ok(pack) = import_playlist_json(&path) {
                if !pack.items.is_empty() { found.push((path, pack)); }
            }
        }
    }
    found
}

/// Export a saved playlist as a portable JSON file for sharing watch-party packs.
pub fn export_playlist_json(
    profile_id:   &str,
    name:         &str,
    loop_default: bool,
    items:        &[PlaylistItemDraft],
) -> Result<PathBuf> {
    let dir = playlists_export_dir(profile_id)?;
    std::fs::create_dir_all(&dir)?;
    let slug = slugify_playlist_name(name);
    let path = dir.join(format!("{slug}.json"));
    let envelope = PortablePlaylist {
        schema: 1,
        name: name.to_string(),
        loop_default,
        items: items.to_vec(),
    };
    let json = serde_json::to_string_pretty(&envelope)
        .map_err(|e| AppError::Config(format!("Failed to serialize playlist: {e}")))?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Read and parse a portable playlist JSON file from disk.
pub fn import_playlist_json(path: &Path) -> Result<PortablePlaylist> {
    let raw = std::fs::read_to_string(path)?;
    serde_json::from_str::<PortablePlaylist>(&raw)
        .map_err(|e| AppError::Config(format!("Invalid playlist JSON: {e}")))
}

/// Scan the profile's `playlists/` directory for `.json` watch-party packs.
pub fn scan_portable_playlists(profile_id: &str) -> Vec<PortablePlaylist> {
    let Ok(dir) = playlists_export_dir(profile_id) else { return Vec::new(); };
    scan_portable_playlists_in_dir(&dir).into_iter().map(|(_, pack)| pack).collect()
}

/// Parse single numbers or dash ranges (`"5"`, `"3-8"`) bounded to `max_aired`.
pub fn parse_batch_queue_input(input: &str, max_aired: u32) -> Vec<u32> {
    parse_episode_range(input, max_aired.max(1), &[])
}

async fn verify_stream_url_alive(url: &str, force_ipv4: bool) -> bool {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return true;
    }
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(3));
    if force_ipv4 {
        builder = builder.local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(_) => return true,
    };
    let head = client
        .head(url)
        .header("Referer", "https://zokoanime.video/")
        .send()
        .await;
    if let Ok(resp) = head {
        let code = resp.status().as_u16();
        if (200..400).contains(&code) {
            return true;
        }
        if code == 403 || code == 404 || code == 410 {
            return false;
        }
    }
    match client
        .get(url)
        .header("Referer", "https://zokoanime.video/")
        .header("Range", "bytes=0-128")
        .send()
        .await
    {
        Ok(resp) => {
            let code = resp.status().as_u16();
            (200..400).contains(&code)
        }
        Err(_) => false,
    }
}

/// Resolve stream URL, subtitle track (including optional auto-sync), and Era-Adaptive GLSL shaders
/// for a queued episode. Used by both near-EOF `N+1` prefetch and Just-In-Time queue launches.
pub async fn resolve_item_prefetch(
    item:         &QueueItem,
    anime4k_mode: Anime4KMode,
    sub_style:    SubtitleStyleConfig,
    network_mode: crate::config::NetworkMode,
    force_ipv4:   bool,
    resolved_at:  u64,
) -> Option<ResolvedPrefetch> {
    let stream_info = crate::api::subtitles::fetch_stream_info_with_network(
        item.anime_id,
        item.mal_id,
        &item.query,
        item.episode,
        item.dub,
        force_ipv4,
        network_mode.network_timeout_secs(),
    )
    .await;

    let desired_h = crate::api::subtitles::parse_height_from_label(&item.quality);
    let capped_h = match (desired_h, network_mode.max_height_cap()) {
        (Some(d), Some(c)) => Some(d.min(c)),
        (Some(d), None) => Some(d),
        (None, Some(c)) => Some(c),
        (None, None) => None,
    };

    let (selected_quality_label, stream_url) = if let Some(ref ladder) = stream_info.ladder {
        if let Some(variant) = ladder.select_best_variant(capped_h, network_mode) {
            (variant.label.clone(), Some(variant.url.clone()))
        } else {
            (
                item.quality.clone(),
                stream_info
                    .qualities
                    .iter()
                    .find(|q| q.label.eq_ignore_ascii_case(&item.quality))
                    .or_else(|| stream_info.qualities.first())
                    .map(|q| q.url.clone())
                    .filter(|u| !u.is_empty())
                    .or_else(|| stream_info.master_url.clone())
                    .filter(|u| !u.is_empty()),
            )
        }
    } else {
        let chosen_q = if let Some(cap) = capped_h {
            stream_info
                .qualities
                .iter()
                .find(|q| {
                    crate::api::subtitles::parse_height_from_label(&q.label).is_some_and(|h| h <= cap)
                })
                .or_else(|| stream_info.qualities.last())
        } else {
            stream_info
                .qualities
                .iter()
                .find(|q| q.label.eq_ignore_ascii_case(&item.quality))
                .or_else(|| stream_info.qualities.first())
        };
        (
            chosen_q.map(|q| q.label.clone()).unwrap_or_else(|| item.quality.clone()),
            chosen_q
                .map(|q| q.url.clone())
                .filter(|u| !u.is_empty())
                .or_else(|| stream_info.master_url.clone())
                .filter(|u| !u.is_empty()),
        )
    };

    // Do not cache empty fallback results as Ready; require an actual verified HTTP stream URL.
    let valid_url = match stream_url {
        Some(ref url) if verify_stream_url_alive(url, force_ipv4).await => Some(url.clone()),
        _ => return None,
    };

    let pref_lower = item.sub_lang.trim().to_lowercase();
    let subs_disabled = pref_lower == "none" || pref_lower == "off" || pref_lower == "no subtitles";
    let (chosen_sub, sub_lang, mut sub_url) = if subs_disabled {
        (None, Some("none".to_string()), None)
    } else {
        let mut subs_with_none = stream_info.subtitles.clone();
        crate::api::subtitles::SubtitleOption::ensure_none_option_first(&mut subs_with_none);
        let (idx, _) = crate::api::subtitles::SubtitleOption::select_index_with_english_fallback(
            &subs_with_none,
            &item.sub_lang,
        );
        let chosen = subs_with_none
            .into_iter()
            .nth(idx)
            .filter(|s| !s.is_none());

        let lang = chosen
            .as_ref()
            .map(|s| s.lang_keyword().to_string())
            .or_else(|| Some(item.sub_lang.clone()));
        let url = chosen.as_ref().map(|s| s.url.clone()).filter(|u| !u.is_empty());
        (chosen, lang, url)
    };
    let mut sub_synced = false;

    let needs_subsync = chosen_sub.is_some_and(|s| {
        s.stream_label.contains("[Auto-Synced]")
            && (s.url.starts_with("http://") || s.url.starts_with("https://"))
    });

    if needs_subsync {
        if let Some(ref raw_sub_url) = sub_url {
            let code = sub_lang.as_deref().unwrap_or("sub");
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default();
            let ref_sub_url = stream_info
                .subtitles
                .iter()
                .find(|s| {
                    !s.stream_label.contains("[Auto-Synced]")
                        && (s.url.starts_with("http://") || s.url.starts_with("https://"))
                })
                .map(|s| s.url.as_str());
            if let Ok((cached_path, res)) = tokio::time::timeout(
                Duration::from_secs(4),
                ensure_synchronized_subtitle(
                    &client,
                    raw_sub_url,
                    ref_sub_url,
                    valid_url.as_deref(),
                    item.anime_id,
                    item.episode,
                    code,
                ),
            )
            .await
            {
                sub_url = Some(cached_path);
                sub_synced = res.is_some_and(|r| r.applied);
            }
        }
    }

    let glsl_shaders = if anime4k_mode != Anime4KMode::Off {
        build_shader_arg_for_year(anime4k_mode, item.season_year)
    } else {
        None
    };

    let options = PlayOptions {
        title: item.query.clone(),
        episode: item.episode,
        quality: selected_quality_label,
        dub: item.dub,
        player: item.player.clone(),
        sub_lang,
        sub_url,
        stream_url: valid_url,
        ipc_socket: None,
        start_pos: None,
        glsl_shaders,
        sub_style: Some(sub_style),
        loading_image: None,
        sub_delay_ms: 0,
        subtitles_disabled: subs_disabled,
        network_mode,
        force_ipv4,
        auto_fullscreen: false,
        auto_pip: false,
        shader_mode: anime4k_mode,
        performance_mode: crate::config::PerformanceMode::Balanced,
        deband_mode: crate::config::DebandMode::Auto,
        motion_mode: crate::config::MotionMode::Off,
        referer: stream_info.providers.first().and_then(|p| p.referer.clone()),
    };

    Some(ResolvedPrefetch {
        options,
        resolved_at,
        sub_synced,
        master_url: stream_info.master_url,
        subtitles: stream_info.subtitles,
        audio_tracks: stream_info.audio_tracks,
        dub_available: stream_info.dub_available,
        ladder: stream_info.ladder,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_item(anime_id: i64, title: &str, ep: u32) -> QueueItem {
        QueueItem::new(
            anime_id,
            None,
            Some(2023),
            title.to_string(),
            title.to_string(),
            ep,
            "1080p".to_string(),
            false,
            "English".to_string(),
            "mpv".to_string(),
        )
    }

    fn mk_prefetch(resolved_at: u64) -> ResolvedPrefetch {
        ResolvedPrefetch {
            options: PlayOptions {
                title: "Frieren".to_string(),
                episode: 1,
                quality: "1080p".to_string(),
                dub: false,
                player: "mpv".to_string(),
                sub_lang: Some("English".to_string()),
                sub_url: None,
                stream_url: Some("https://cdn.example.com/1080.m3u8".to_string()),
                ipc_socket: None,
                start_pos: None,
                glsl_shaders: None,
                sub_style: None,
                loading_image: None,
                sub_delay_ms: 0,
                subtitles_disabled: false,
                network_mode: crate::config::NetworkMode::Normal,
                force_ipv4: false,
                auto_fullscreen: false,
                auto_pip: false,
                shader_mode: Anime4KMode::Off,
                performance_mode: crate::config::PerformanceMode::Balanced,
                deband_mode: crate::config::DebandMode::Auto,
                motion_mode: crate::config::MotionMode::Off,
                referer: None,
            },
            resolved_at,
            sub_synced: false,
            master_url: None,
            subtitles: Vec::new(),
            audio_tracks: Vec::new(),
            dub_available: false,
            ladder: None,
        }
    }

    #[test]
    fn test_enqueue_back_dedup_and_enqueue_next_priority() {
        let mut q = WatchQueue::default();
        assert_eq!(q.enqueue_back(mk_item(1, "Frieren", 1)), EnqueueOutcome { position: 1, replaced: false });
        assert_eq!(q.enqueue_back(mk_item(1, "Frieren", 2)), EnqueueOutcome { position: 2, replaced: false });
        // Duplicate replaces in-place instead of creating duplicate entries
        assert_eq!(q.enqueue_back(mk_item(1, "Frieren", 1)), EnqueueOutcome { position: 1, replaced: true });
        assert_eq!(q.len(), 2);

        // Queue Next promotes ep 2 to front
        let res = q.enqueue_next(mk_item(1, "Frieren", 2));
        assert_eq!(res, EnqueueOutcome { position: 1, replaced: true });
        assert_eq!(q.items[0].episode, 2);
        assert_eq!(q.items[1].episode, 1);
    }

    #[test]
    fn test_loop_mode_rotates_finished_item_and_advance() {
        let mut q = WatchQueue::default();
        q.loop_mode = true;
        q.enqueue_back(mk_item(1, "Bebop", 1));
        q.enqueue_back(mk_item(2, "Frieren", 2));

        let first = q.pop_next_for_advance().unwrap();
        assert_eq!(first.episode, 1);
        q.active_item = Some(ActiveNowItem {
            item: first,
            used_prefetched: false,
            retried_once: false,
            from_queue: true,
            completion_reached: true,
            progress_ratio: 1.0,
        });

        let second = q.pop_next_for_advance().unwrap();
        assert_eq!(second.episode, 2);
        assert_eq!(q.len(), 1);
        assert_eq!(q.items[0].episode, 1); // Active item rotated to tail
    }

    #[test]
    fn test_prefetch_ttl_expiration_and_status_badges() {
        let pre = mk_prefetch(1_000);
        assert!(pre.is_fresh(1_000));
        assert!(pre.is_fresh(1_000 + PREFETCH_TTL_SECS - 1));
        assert!(!pre.is_fresh(1_000 + PREFETCH_TTL_SECS));

        let mut item = mk_item(1, "Frieren", 1);
        let fresh_ts = 1_000 + PREFETCH_TTL_SECS.saturating_sub(10);
        let stale_ts = 1_000 + PREFETCH_TTL_SECS;
        assert_eq!(item.status(fresh_ts), PrefetchStatus::Queued);
        item.resolving = true;
        assert_eq!(item.status(fresh_ts), PrefetchStatus::Resolving);
        item.resolving = false;
        item.prefetch = Some(pre);
        assert_eq!(item.status(fresh_ts), PrefetchStatus::Ready);
        assert_eq!(item.status(stale_ts), PrefetchStatus::Stale);
    }

    #[test]
    fn test_move_up_down_and_json_roundtrip_clears_transient_flags() {
        let mut q = WatchQueue::default();
        q.enqueue_back(mk_item(1, "A", 1));
        q.enqueue_back(mk_item(2, "B", 2));
        q.enqueue_back(mk_item(3, "C", 3));
        q.items[0].resolving = true;
        q.manual_stop = true;

        assert_eq!(q.move_down(0), 1);
        assert_eq!(q.items[1].title, "A");
        assert_eq!(q.move_up(2), 1);
        assert_eq!(q.items[1].title, "C");

        let json = serde_json::to_string(&q).unwrap();
        let mut decoded: WatchQueue = serde_json::from_str(&json).unwrap();
        decoded.normalize_uids();
        assert!(!decoded.manual_stop);
        assert!(!decoded.items[1].resolving);
        assert_eq!(decoded.items.len(), 3);
    }

    #[test]
    fn test_clean_user_path_and_export_target_resolution() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        assert_eq!(clean_user_path("~/Desktop/mix.json"), PathBuf::from(&home).join("Desktop/mix.json"));
        assert_eq!(clean_user_path("'~/Downloads/My Mix.json'"), PathBuf::from(&home).join("Downloads/My Mix.json"));
        assert_eq!(clean_user_path("/tmp/My\\ Mix.json"), PathBuf::from("/tmp/My Mix.json"));

        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(resolve_export_target(&tmp.path().display().to_string(), "Test Mix").unwrap(), tmp.path().join("test-mix.json"));
        let extensionless = tmp.path().join("watch-party");
        assert_eq!(resolve_export_target(&extensionless.display().to_string(), "Unused").unwrap(), tmp.path().join("watch-party.json"));
        let explicit = tmp.path().join("watch-party.json");
        assert_eq!(resolve_export_target(&explicit.display().to_string(), "Unused").unwrap(), explicit);
    }

    #[test]
    fn test_custom_export_roundtrip_and_directory_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let pack_dir = tmp.path().join("packs");
        let target = pack_dir.join("party.json");
        let pack_item = mk_item(1, "Cowboy Bebop", 5).to_draft();
        export_playlist_json_to_path(&target, "Party Mix", true, &[pack_item.clone()]).unwrap();
        assert!(target.exists());
        assert_eq!(import_playlist_json(&target).unwrap().name, "Party Mix");
        let found = scan_portable_playlists_in_dir(&pack_dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1.items[0], pack_item);
    }

    #[test]
    fn test_batch_range_parser_and_portable_playlist_json() {
        assert_eq!(parse_batch_queue_input("3-6", 24), vec![3, 4, 5, 6]);
        assert_eq!(parse_batch_queue_input("10", 12), vec![10]);
        assert_eq!(parse_batch_queue_input("10-20", 12), vec![10, 11, 12]);

        let tmp = std::env::temp_dir().join("solus_test_portable_playlist.json");
        let pack = PortablePlaylist {
            schema: 1,
            name: "Party Mix".to_string(),
            loop_default: true,
            items: vec![mk_item(1, "Cowboy Bebop", 5).to_draft()],
        };
        std::fs::write(&tmp, serde_json::to_string_pretty(&pack).unwrap()).unwrap();
        let loaded = import_playlist_json(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(loaded, pack);
    }

    #[test]
    fn test_spec_version_rejects_stale_prefetch_after_item_edit() {
        let mut q = WatchQueue::default();
        q.enqueue_back(mk_item(1, "Frieren", 4));
        let uid = q.items[0].uid;
        let old_spec = q.items[0].spec_version;

        // Simulate user editing dub/sub settings while older prefetch was in flight
        assert!(q.update_item_by_uid(uid, |item| {
            item.dub = true;
            item.sub_lang = "Arabic".to_string();
        }));
        assert!(q.items[0].spec_version > old_spec);

        // Stale prefetch carrying old_spec must be rejected cleanly
        let applied_stale = q.apply_prefetch_result(uid, old_spec, Some(mk_prefetch(1_000)));
        assert!(!applied_stale);
        assert!(q.items[0].prefetch.is_none());

        // Matching current spec_version succeeds
        let cur_spec = q.items[0].spec_version;
        let applied_fresh = q.apply_prefetch_result(uid, cur_spec, Some(mk_prefetch(1_000)));
        assert!(applied_fresh);
        assert!(q.items[0].prefetch.is_some());
    }

    #[test]
    fn test_queue_retains_item_on_early_close_and_removes_at_90_pct() {
        let mut q = WatchQueue::default();
        q.enqueue_back(mk_item(1, "Frieren", 1));
        q.enqueue_back(mk_item(1, "Frieren", 2));

        let first = q.pop_next_for_advance().unwrap();
        q.active_item = Some(ActiveNowItem {
            item: first,
            used_prefetched: false,
            retried_once: false,
            from_queue: true,
            completion_reached: false,
            progress_ratio: 0.0,
        });

        // At 35%, episode is NOT finished; closing player restores Ep 1 to front of queue
        assert!(!q.record_active_progress(1, 1, 500.0, 1420.0));
        assert!(!q.active_item.as_ref().unwrap().completion_reached);
        let restored = q.restore_unfinished_active_to_front().unwrap();
        assert_eq!(restored.episode, 1);
        assert_eq!(q.items.len(), 2);
        assert_eq!(q.items[0].episode, 1);
        assert_eq!(q.items[1].episode, 2);

        // Relaunch Ep 1 and watch past 90% threshold (within 10% of completion)
        let first_again = q.pop_next_for_advance().unwrap();
        q.active_item = Some(ActiveNowItem {
            item: first_again,
            used_prefetched: false,
            retried_once: false,
            from_queue: true,
            completion_reached: false,
            progress_ratio: 0.0,
        });
        assert!(q.record_active_progress(1, 1, 1290.0, 1420.0));
        assert!(q.active_item.as_ref().unwrap().completion_reached);
        assert!(q.restore_unfinished_active_to_front().is_none());
        assert_eq!(q.items.len(), 1);
        assert_eq!(q.items[0].episode, 2);
    }

    #[test]
    fn test_queue_crash_recovery_restores_unfinished_active_item_on_load() {
        let mut q = WatchQueue::default();
        q.enqueue_back(mk_item(1, "Bebop", 5));
        q.enqueue_back(mk_item(1, "Bebop", 6));

        let active = q.pop_next_for_advance().unwrap();
        q.active_item = Some(ActiveNowItem {
            item: active,
            used_prefetched: false,
            retried_once: false,
            from_queue: true,
            completion_reached: false,
            progress_ratio: 0.42,
        });

        // Simulate app crash and reload from JSON
        let json = serde_json::to_string(&q).unwrap();
        let mut reloaded: WatchQueue = serde_json::from_str(&json).unwrap();
        reloaded.normalize_uids();

        assert!(reloaded.active_item.is_none());
        assert_eq!(reloaded.items.len(), 2);
        assert_eq!(reloaded.items[0].episode, 5);
        assert_eq!(reloaded.items[1].episode, 6);
    }
}
