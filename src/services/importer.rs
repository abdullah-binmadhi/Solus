//! Solus Library Importer — zero-token JSON archive ingestion and database hydration.
//!
//! Supports:
//! 1. AniList official exports and community JSON dumps (`MediaListCollection` schema).
//! 2. Flat anime list JSON arrays.
//! 3. Auto-discovery of exported files in `~/Downloads` and user directories.
//! 4. 1-click database hydration into SQLite cache, watchlist, and watched history.

use anyhow::Result;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

use crate::{
    db::{cache::{self, Anime}, user},
    error::AppError,
};

/// Normalized anime record extracted from external archives or APIs.
#[derive(Debug, Clone)]
pub struct ImportedAnimeEntry {
    pub id: i64,
    pub title_romaji: String,
    pub title_english: Option<String>,
    pub title_native: Option<String>,
    pub status: String, // "CURRENT", "COMPLETED", "PLANNING", "DROPPED", "PAUSED"
    pub progress: u32,
    pub total_episodes: Option<i64>,
    pub score: Option<i64>,
    pub format: Option<String>,
    pub season: Option<String>,
    pub season_year: Option<i64>,
    pub genres: Vec<String>,
    pub cover_url: Option<String>,
}

impl ImportedAnimeEntry {
    pub fn from_anime(anime: Anime, progress: u32, status: &str) -> Self {
        let genres = anime.genre_list();
        Self {
            id: anime.id,
            title_romaji: anime.title_romaji,
            title_english: anime.title_english,
            title_native: anime.title_native,
            status: status.to_string(),
            progress,
            total_episodes: anime.episodes,
            score: anime.score,
            format: anime.format,
            season: anime.season,
            season_year: anime.season_year,
            genres,
            cover_url: anime.cover_url,
        }
    }
}

/// Convert AniList UserCollection into normalized ImportedAnimeEntry records.
pub fn collection_to_entries(coll: crate::api::anilist::UserCollection) -> Vec<ImportedAnimeEntry> {
    let mut entries = Vec::new();
    for (anime, progress) in coll.watching {
        entries.push(ImportedAnimeEntry::from_anime(anime, progress, "CURRENT"));
    }
    for anime in coll.completed {
        let total = anime.episodes.unwrap_or(1) as u32;
        entries.push(ImportedAnimeEntry::from_anime(anime, total, "COMPLETED"));
    }
    for anime in coll.planning {
        entries.push(ImportedAnimeEntry::from_anime(anime, 0, "PLANNING"));
    }
    for anime in coll.dropped {
        entries.push(ImportedAnimeEntry::from_anime(anime, 0, "DROPPED"));
    }
    entries
}

/// Statistics reported after hydrating the SQLite database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub total_anime: usize,
    pub watching_count: usize,
    pub completed_count: usize,
    pub planning_count: usize,
    pub dropped_count: usize,
    pub episodes_logged: usize,
}

/// Scan `~/Downloads` and user home directory for exported anime lists.
pub fn detect_export_files() -> Vec<PathBuf> {
    let mut detected = Vec::new();

    let candidate_dirs = [
        directories::UserDirs::new().and_then(|u| u.download_dir().map(|d| d.to_path_buf())),
        directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()),
    ];

    for dir_opt in candidate_dirs.into_iter().flatten() {
        if let Ok(entries) = std::fs::read_dir(&dir_opt) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                    if ext.eq_ignore_ascii_case("json") {
                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                        if name.contains("animelist")
                            || name.contains("anilist")
                            || name.contains("anime_list")
                            || name.contains("mal_export")
                            || name.contains("solus_backup")
                        {
                            detected.push(path);
                        }
                    }
                }
            }
        }
    }

    // Sort by modification time descending (most recently created/downloaded first)
    detected.sort_by(|a, b| {
        let meta_a = a.metadata().and_then(|m| m.modified()).ok();
        let meta_b = b.metadata().and_then(|m| m.modified()).ok();
        meta_b.cmp(&meta_a)
    });

    detected.truncate(6);
    detected
}

/// Parse any supported JSON string into normalized `ImportedAnimeEntry` records.
pub fn parse_anime_export_json(raw_json: &str) -> Result<Vec<ImportedAnimeEntry>, AppError> {
    let v: serde_json::Value = serde_json::from_str(raw_json)
        .map_err(|e| AppError::Parse(format!("Invalid JSON format: {e}")))?;

    let mut entries = Vec::new();

    // Check Format 1: GraphQL Root {"data": {"MediaListCollection": {"lists": [...]}}}
    // or {"MediaListCollection": {"lists": [...]}} or {"lists": [...]}
    let lists_opt = v
        .pointer("/data/MediaListCollection/lists")
        .or_else(|| v.pointer("/MediaListCollection/lists"))
        .or_else(|| v.get("lists"))
        .and_then(|l| l.as_array());

    if let Some(lists) = lists_opt {
        for list in lists {
            let list_status = list.get("status").and_then(|s| s.as_str()).unwrap_or("PLANNING");
            if let Some(raw_entries) = list.get("entries").and_then(|e| e.as_array()) {
                for entry in raw_entries {
                    if let Some(parsed) = parse_single_anilist_entry(entry, list_status) {
                        entries.push(parsed);
                    }
                }
            }
        }
        if !entries.is_empty() {
            return Ok(entries);
        }
    }

    // Check Format 2: Direct array of entries [ { "media": {...}, "status": ... }, ... ]
    if let Some(arr) = v.as_array() {
        for item in arr {
            if item.get("media").is_some() {
                if let Some(parsed) = parse_single_anilist_entry(item, "PLANNING") {
                    entries.push(parsed);
                }
            } else if let Some(parsed) = parse_flat_entry(item) {
                entries.push(parsed);
            }
        }
        if !entries.is_empty() {
            return Ok(entries);
        }
    }

    Err(AppError::Parse("Unrecognized JSON archive schema. Expected AniList export or media list array.".into()))
}

/// Parse a single entry from an AniList `MediaList` object.
fn parse_single_anilist_entry(entry: &serde_json::Value, fallback_status: &str) -> Option<ImportedAnimeEntry> {
    let status_raw = entry.get("status").and_then(|s| s.as_str()).unwrap_or(fallback_status);
    let progress = entry.get("progress").and_then(|p| p.as_u64()).unwrap_or(0) as u32;
    let score = entry.get("score").and_then(|s| s.as_f64()).map(|s| (s * 10.0).round() as i64);

    let media = entry.get("media")?;
    let id = media.get("id").and_then(|i| i.as_i64())
        .or_else(|| entry.get("mediaId").and_then(|i| i.as_i64()))?;

    let title_obj = media.get("title");
    let title_romaji = title_obj
        .and_then(|t| t.get("romaji"))
        .and_then(|r| r.as_str())
        .or_else(|| media.get("title_romaji").and_then(|t| t.as_str()))
        .unwrap_or("Unknown Title")
        .to_string();

    let title_english = title_obj
        .and_then(|t| t.get("english"))
        .and_then(|e| e.as_str())
        .or_else(|| media.get("title_english").and_then(|t| t.as_str()))
        .map(|s| s.to_string());

    let title_native = title_obj
        .and_then(|t| t.get("native"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    let total_episodes = media.get("episodes").and_then(|e| e.as_i64());
    let format = media.get("format").and_then(|f| f.as_str()).map(|s| s.to_string());
    let season = media.get("season").and_then(|s| s.as_str()).map(|s| s.to_string());
    let season_year = media.get("seasonYear").and_then(|y| y.as_i64());

    let mut genres = Vec::new();
    if let Some(genre_arr) = media.get("genres").and_then(|g| g.as_array()) {
        for g in genre_arr {
            if let Some(s) = g.as_str() {
                genres.push(s.to_string());
            }
        }
    }

    let cover_url = media
        .pointer("/coverImage/extraLarge")
        .or_else(|| media.pointer("/coverImage/large"))
        .or_else(|| media.pointer("/coverImage/medium"))
        .or_else(|| media.get("cover_url"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());

    Some(ImportedAnimeEntry {
        id,
        title_romaji,
        title_english,
        title_native,
        status: status_raw.to_uppercase(),
        progress,
        total_episodes,
        score,
        format,
        season,
        season_year,
        genres,
        cover_url,
    })
}

/// Parse flat entry `{ "id": 123, "title": "...", "status": "CURRENT", "progress": 5 }`.
fn parse_flat_entry(entry: &serde_json::Value) -> Option<ImportedAnimeEntry> {
    let id = entry.get("id").or_else(|| entry.get("media_id")).and_then(|i| i.as_i64())?;
    let title_romaji = entry.get("title")
        .or_else(|| entry.get("title_romaji"))
        .and_then(|t| t.as_str())
        .unwrap_or("Unknown Title")
        .to_string();

    let title_english = entry.get("title_english").and_then(|t| t.as_str()).map(|s| s.to_string());
    let status = entry.get("status").and_then(|s| s.as_str()).unwrap_or("PLANNING").to_uppercase();
    let progress = entry.get("progress").and_then(|p| p.as_u64()).unwrap_or(0) as u32;
    let total_episodes = entry.get("total_episodes").or_else(|| entry.get("episodes")).and_then(|e| e.as_i64());
    let score = entry.get("score").and_then(|s| s.as_i64());

    Some(ImportedAnimeEntry {
        id,
        title_romaji,
        title_english,
        title_native: None,
        status,
        progress,
        total_episodes,
        score,
        format: Some("TV".into()),
        season: None,
        season_year: None,
        genres: Vec::new(),
        cover_url: entry.get("cover_url").and_then(|c| c.as_str()).map(|s| s.to_string()),
    })
}

/// Hydrate parsed anime entries into local SQLite database.
pub async fn import_entries_to_db(
    pool: &SqlitePool,
    entries: &[ImportedAnimeEntry],
    now: i64,
) -> Result<ImportStats, AppError> {
    let mut stats = ImportStats::default();
    let dismissed_wl = user::get_dismissed_ids(pool, "watchlist").await.unwrap_or_default();
    let dismissed_cw = user::get_dismissed_ids(pool, "continue_watching").await.unwrap_or_default();

    for entry in entries {
        // 1. Build and upsert Anime metadata
        let genres_json = serde_json::to_string(&entry.genres).unwrap_or_else(|_| "[]".to_string());
        let anime = Anime {
            id: entry.id,
            title_english: entry.title_english.clone(),
            title_romaji: entry.title_romaji.clone(),
            title_native: entry.title_native.clone(),
            description: None,
            episodes: entry.total_episodes,
            status: Some(entry.status.clone()),
            season: entry.season.clone(),
            season_year: entry.season_year,
            score: entry.score,
            format: entry.format.clone(),
            genres: genres_json,
            cover_url: entry.cover_url.clone(),
            cover_blob: None,
            has_dub: 0,
            updated_at: now,
            next_airing_episode: None,
        };

        let _ = cache::upsert_anime(pool, &anime).await;
        stats.total_anime += 1;

        // 2. Hydrate user progress and library placement (respecting local user removals)
        match entry.status.as_str() {
            "CURRENT" => {
                if !dismissed_cw.contains(&entry.id) {
                    stats.watching_count += 1;
                    for ep in 1..=entry.progress {
                        let _ = user::record_watched(pool, entry.id, ep as i64, now).await;
                        stats.episodes_logged += 1;
                    }
                }
            }
            "COMPLETED" => {
                stats.completed_count += 1;
                if !dismissed_cw.contains(&entry.id) {
                    let max_ep = entry.total_episodes.unwrap_or(entry.progress as i64).max(entry.progress as i64);
                    let target_eps = if max_ep > 0 { max_ep } else { entry.progress as i64 };
                    for ep in 1..=target_eps {
                        let _ = user::record_watched(pool, entry.id, ep, now).await;
                        stats.episodes_logged += 1;
                    }
                }
            }
            "PLANNING" => {
                if !dismissed_wl.contains(&entry.id) {
                    stats.planning_count += 1;
                    let _ = user::add_to_watchlist(pool, entry.id, now).await;
                }
            }
            "DROPPED" | "PAUSED" => {
                stats.dropped_count += 1;
                if !dismissed_cw.contains(&entry.id) {
                    for ep in 1..=entry.progress {
                        let _ = user::record_watched(pool, entry.id, ep as i64, now).await;
                        stats.episodes_logged += 1;
                    }
                }
            }
            _ => {
                if !dismissed_wl.contains(&entry.id) {
                    stats.planning_count += 1;
                    let _ = user::add_to_watchlist(pool, entry.id, now).await;
                }
            }
        }
    }

    Ok(stats)
}

/// Export local Solus watchlist and history to a clean JSON file.
pub async fn export_library_to_json(pool: &SqlitePool, dest_path: &Path) -> Result<usize, AppError> {
    let watchlist_ids = user::get_watchlist(pool).await?;
    let continue_entries = user::get_continue_watching(pool).await?;

    let mut export_items = Vec::new();

    // Export Watchlist items
    for id in watchlist_ids {
        if let Ok(Some(anime)) = cache::get_anime(pool, id).await {
            export_items.push(serde_json::json!({
                "id": anime.id,
                "title_romaji": anime.title_romaji,
                "title_english": anime.title_english,
                "status": "PLANNING",
                "progress": 0,
                "episodes": anime.episodes,
                "cover_url": anime.cover_url,
            }));
        }
    }

    // Export Currently Watching / History items
    for entry in continue_entries {
        if let Ok(Some(anime)) = cache::get_anime(pool, entry.anime_id).await {
            export_items.push(serde_json::json!({
                "id": anime.id,
                "title_romaji": anime.title_romaji,
                "title_english": anime.title_english,
                "status": "CURRENT",
                "progress": entry.last_episode,
                "episodes": anime.episodes,
                "cover_url": anime.cover_url,
            }));
        }
    }

    let count = export_items.len();
    let content = serde_json::to_string_pretty(&export_items)
        .map_err(|e| AppError::Config(format!("Failed to serialize library export: {e}")))?;

    if let Some(parent) = dest_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(dest_path, content)?;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_anilist_graphql_export() {
        let sample = r#"{
            "data": {
                "MediaListCollection": {
                    "lists": [
                        {
                            "name": "Watching",
                            "status": "CURRENT",
                            "entries": [
                                {
                                    "mediaId": 178083,
                                    "status": "CURRENT",
                                    "progress": 6,
                                    "media": {
                                        "id": 178083,
                                        "title": { "romaji": "Tokyo Revengers: Santen Sensou-hen", "english": "Tokyo Revengers S3" },
                                        "episodes": 13,
                                        "coverImage": { "extraLarge": "https://cover.jpg" }
                                    }
                                }
                            ]
                        },
                        {
                            "name": "Planning",
                            "status": "PLANNING",
                            "entries": [
                                {
                                    "mediaId": 99999,
                                    "status": "PLANNING",
                                    "progress": 0,
                                    "media": {
                                        "id": 99999,
                                        "title": { "romaji": "Chainsaw Man Reze Arc", "english": null },
                                        "episodes": 1
                                    }
                                }
                            ]
                        }
                    ]
                }
            }
        }"#;

        let entries = parse_anime_export_json(sample).expect("Should parse GraphQL export");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, 178083);
        assert_eq!(entries[0].progress, 6);
        assert_eq!(entries[0].status, "CURRENT");
        assert_eq!(entries[1].id, 99999);
        assert_eq!(entries[1].status, "PLANNING");
    }

    #[test]
    fn test_parse_flat_array() {
        let sample = r#"[
            {
                "id": 1234,
                "title": "Sousou no Frieren",
                "title_english": "Frieren: Beyond Journey's End",
                "status": "COMPLETED",
                "progress": 28,
                "total_episodes": 28
            }
        ]"#;

        let entries = parse_anime_export_json(sample).expect("Should parse flat array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, 1234);
        assert_eq!(entries[0].title_romaji, "Sousou no Frieren");
        assert_eq!(entries[0].status, "COMPLETED");
        assert_eq!(entries[0].progress, 28);
    }
}
