//! User data: watch history, continue watching, watchlist.

use crate::config::AudioMode;
use crate::error::Result;
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, HashSet};

/// A single history entry: which episode of which show was watched.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HistoryEntry {
    #[allow(dead_code)]
    pub anime_id:   i64,
    #[allow(dead_code)]
    pub episode:    i64,
    #[allow(dead_code)]
    pub watched_at: i64, // unix timestamp
}

/// The "continue watching" row entry.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ContinueEntry {
    pub anime_id:     i64,
    #[allow(dead_code)]
    pub last_episode: i64,
    #[allow(dead_code)]
    pub last_watched: i64,
}

// ─── History ──────────────────────────────────────────────────────────────────

/// Batch ingest AniList user collection into SQLite in a single atomic transaction.
/// This collapses hundreds of individual fsync disk flushes into 1, dropping
/// Windows collection sync time from 25s down to 20ms.
pub async fn batch_sync_user_collection(
    pool: &SqlitePool,
    coll: &crate::api::anilist::UserCollection,
    dismissed_cw: &std::collections::HashSet<i64>,
    dismissed_wl: &std::collections::HashSet<i64>,
    now: i64,
) -> Result<()> {
    let mut tx = pool.begin().await?;

    for (anime, progress) in &coll.watching {
        crate::db::cache::execute_upsert_anime(&mut *tx, anime).await?;
        if !dismissed_cw.contains(&anime.id) {
            sqlx::query(
                "INSERT INTO history (anime_id, episode, watched_at)
                 VALUES (?, ?, ?)
                 ON CONFLICT(anime_id, episode) DO UPDATE SET watched_at = excluded.watched_at",
            )
            .bind(anime.id)
            .bind(*progress as i64)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                "INSERT INTO continue_watching (anime_id, last_episode, last_watched)
                 VALUES (?, ?, ?)
                 ON CONFLICT(anime_id) DO UPDATE SET
                     last_episode = excluded.last_episode,
                     last_watched = excluded.last_watched",
            )
            .bind(anime.id)
            .bind(*progress as i64)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            let _ = sqlx::query(
                "DELETE FROM dismissed_sync WHERE anime_id = ? AND list_type = 'continue_watching'",
            )
            .bind(anime.id)
            .execute(&mut *tx)
            .await;
        }
    }

    for anime in &coll.planning {
        crate::db::cache::execute_upsert_anime(&mut *tx, anime).await?;
        if !dismissed_wl.contains(&anime.id) {
            sqlx::query(
                "INSERT OR IGNORE INTO watchlist (anime_id, added_at) VALUES (?, ?)",
            )
            .bind(anime.id)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            let _ = sqlx::query(
                "DELETE FROM dismissed_sync WHERE anime_id = ? AND list_type = 'watchlist'",
            )
            .bind(anime.id)
            .execute(&mut *tx)
            .await;
        }
    }

    for anime in &coll.completed {
        crate::db::cache::execute_upsert_anime(&mut *tx, anime).await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Record that an episode was watched (upsert: re-watching updates timestamp).
pub async fn record_watched(
    pool:     &SqlitePool,
    anime_id: i64,
    episode:  i64,
    now:      i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO history (anime_id, episode, watched_at)
         VALUES (?, ?, ?)
         ON CONFLICT(anime_id, episode) DO UPDATE SET watched_at = excluded.watched_at",
    )
    .bind(anime_id)
    .bind(episode)
    .bind(now)
    .execute(pool)
    .await?;

    // Also update or insert continue_watching
    sqlx::query(
        "INSERT INTO continue_watching (anime_id, last_episode, last_watched)
         VALUES (?, ?, ?)
         ON CONFLICT(anime_id) DO UPDATE SET
             last_episode = excluded.last_episode,
             last_watched = excluded.last_watched",
    )
    .bind(anime_id)
    .bind(episode)
    .bind(now)
    .execute(pool)
    .await?;

    let _ = sqlx::query(
        "DELETE FROM dismissed_sync WHERE anime_id = ? AND list_type = 'continue_watching'",
    )
    .bind(anime_id)
    .execute(pool)
    .await;

    Ok(())
}

/// Fetch full history for a show, ordered by episode ascending.
#[cfg_attr(not(test), allow(dead_code))]
pub async fn get_history(pool: &SqlitePool, anime_id: i64) -> Result<Vec<HistoryEntry>> {
    let rows = sqlx::query_as::<_, HistoryEntry>(
        "SELECT anime_id, episode, watched_at FROM history
         WHERE anime_id = ? ORDER BY episode ASC",
    )
    .bind(anime_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Check whether a specific episode has been watched.
#[cfg_attr(not(test), allow(dead_code))]
pub async fn is_watched(pool: &SqlitePool, anime_id: i64, episode: i64) -> Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM history WHERE anime_id = ? AND episode = ?")
            .bind(anime_id)
            .bind(episode)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

// ─── Continue Watching ────────────────────────────────────────────────────────

/// Fetch all continue-watching entries, ordered by most recently watched first.
pub async fn get_continue_watching(pool: &SqlitePool) -> Result<Vec<ContinueEntry>> {
    let rows = sqlx::query_as::<_, ContinueEntry>(
        "SELECT anime_id, last_episode, last_watched FROM continue_watching
         ORDER BY last_watched DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Fetch the continue entry for a single show (if any).
#[cfg_attr(not(test), allow(dead_code))]
pub async fn get_continue_entry(
    pool:     &SqlitePool,
    anime_id: i64,
) -> Result<Option<ContinueEntry>> {
    let row = sqlx::query_as::<_, ContinueEntry>(
        "SELECT anime_id, last_episode, last_watched FROM continue_watching WHERE anime_id = ?",
    )
    .bind(anime_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Remove a show from continue watching and purge its local episode watch history.
/// Also records a sync dismissal so startup/background AniList syncs do not re-add it.
pub async fn remove_continue_watching(pool: &SqlitePool, anime_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM continue_watching WHERE anime_id = ?")
        .bind(anime_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM history WHERE anime_id = ?")
        .bind(anime_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT OR REPLACE INTO dismissed_sync (anime_id, list_type, dismissed_at)
         VALUES (?, 'continue_watching', strftime('%s','now'))",
    )
    .bind(anime_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ─── Watchlist ────────────────────────────────────────────────────────────────

/// Add a show to the watchlist (and clear any prior sync dismissal).
pub async fn add_to_watchlist(pool: &SqlitePool, anime_id: i64, now: i64) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO watchlist (anime_id, added_at) VALUES (?, ?)",
    )
    .bind(anime_id)
    .bind(now)
    .execute(pool)
    .await?;
    let _ = sqlx::query(
        "DELETE FROM dismissed_sync WHERE anime_id = ? AND list_type = 'watchlist'",
    )
    .bind(anime_id)
    .execute(pool)
    .await;
    Ok(())
}

/// Remove a show from the watchlist and record a sync dismissal so AniList syncs do not re-add it.
pub async fn remove_from_watchlist(pool: &SqlitePool, anime_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM watchlist WHERE anime_id = ?")
        .bind(anime_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT OR REPLACE INTO dismissed_sync (anime_id, list_type, dismissed_at)
         VALUES (?, 'watchlist', strftime('%s','now'))",
    )
    .bind(anime_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Fetch all anime IDs that the user explicitly removed/dismissed from `list_type` ("watchlist" or "continue_watching").
pub async fn get_dismissed_ids(pool: &SqlitePool, list_type: &str) -> Result<HashSet<i64>> {
    let rows = sqlx::query_scalar::<_, i64>(
        "SELECT anime_id FROM dismissed_sync WHERE list_type = ?",
    )
    .bind(list_type)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Check if a show is in the watchlist.
pub async fn is_in_watchlist(pool: &SqlitePool, anime_id: i64) -> Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM watchlist WHERE anime_id = ?")
            .bind(anime_id)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

/// Get all distinct episode numbers watched for a specific anime, ordered ascending.
pub async fn get_watched_episodes(pool: &SqlitePool, anime_id: i64) -> Result<Vec<i64>> {
    let rows = sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT episode FROM history WHERE anime_id = ? ORDER BY episode ASC",
    )
    .bind(anime_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get watched-episode counts for multiple anime IDs.
pub async fn get_watched_counts(pool: &SqlitePool, anime_ids: &[i64]) -> Result<HashMap<i64, usize>> {
    let mut counts = HashMap::new();

    for &anime_id in anime_ids {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT episode) FROM history WHERE anime_id = ?",
        )
        .bind(anime_id)
        .fetch_one(pool)
        .await?;
        counts.insert(anime_id, count as usize);
    }

    Ok(counts)
}

/// Fetch all watchlist anime IDs, ordered by date added.
pub async fn get_watchlist(pool: &SqlitePool) -> Result<Vec<i64>> {
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT anime_id FROM watchlist ORDER BY added_at DESC")
            .fetch_all(pool)
            .await?;
    Ok(ids)
}

/// Get all anime IDs that have any watch history or are in continue watching.
pub async fn get_all_watched_anime_ids(pool: &SqlitePool) -> Result<Vec<i64>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT anime_id FROM history UNION SELECT anime_id FROM continue_watching"
    )
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

/// Fetch tracked show IDs (continue watching + watchlist) and watched episode sets for the Calendar.
pub async fn get_calendar_tracking_snapshot(
    pool: &SqlitePool,
) -> Result<(HashSet<i64>, HashMap<i64, HashSet<u32>>)> {
    let tracked_rows: Vec<i64> = sqlx::query_scalar(
        "SELECT anime_id FROM continue_watching UNION SELECT anime_id FROM watchlist",
    )
    .fetch_all(pool)
    .await?;
    let tracked_ids: HashSet<i64> = tracked_rows.into_iter().collect();

    let hist_rows = sqlx::query("SELECT anime_id, episode FROM history")
        .fetch_all(pool)
        .await?;
    let mut watched_map: HashMap<i64, HashSet<u32>> = HashMap::new();
    for r in hist_rows {
        let aid: i64 = r.try_get("anime_id").unwrap_or_default();
        let ep: i64 = r.try_get("episode").unwrap_or_default();
        if ep > 0 {
            watched_map.entry(aid).or_default().insert(ep as u32);
        }
    }

    Ok((tracked_ids, watched_map))
}

/// Save the preferred playback query for a show.
pub async fn set_playback_query(
    pool:     &SqlitePool,
    anime_id: i64,
    query:    &str,
    now:      i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO playback_prefs (anime_id, query, updated_at)
         VALUES (?, ?, ?)
         ON CONFLICT(anime_id) DO UPDATE SET
             query = excluded.query,
             updated_at = excluded.updated_at",
    )
    .bind(anime_id)
    .bind(query)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get the preferred playback query for a show, if one has been saved before.
pub async fn get_playback_query(pool: &SqlitePool, anime_id: i64) -> Result<Option<String>> {
    let query = sqlx::query_scalar::<_, String>(
        "SELECT query FROM playback_prefs WHERE anime_id = ?",
    )
    .bind(anime_id)
    .fetch_optional(pool)
    .await?;
    Ok(query)
}

/// Save the preferred audio mode for a show.
pub async fn set_audio_mode(
    pool:     &SqlitePool,
    anime_id: i64,
    audio:    AudioMode,
    now:      i64,
) -> Result<()> {
    let value = match audio {
        AudioMode::Sub => "sub",
        AudioMode::Dub => "dub",
    };

    sqlx::query(
        "INSERT INTO audio_prefs (anime_id, audio_mode, updated_at)
         VALUES (?, ?, ?)
         ON CONFLICT(anime_id) DO UPDATE SET
             audio_mode = excluded.audio_mode,
             updated_at = excluded.updated_at",
    )
    .bind(anime_id)
    .bind(value)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get the preferred audio mode for a show, if one has been saved before.
pub async fn get_audio_mode(pool: &SqlitePool, anime_id: i64) -> Result<Option<AudioMode>> {
    let audio = sqlx::query_scalar::<_, String>(
        "SELECT audio_mode FROM audio_prefs WHERE anime_id = ?",
    )
    .bind(anime_id)
    .fetch_optional(pool)
    .await?;

    Ok(audio.map(|value| {
        if value.eq_ignore_ascii_case("dub") {
            AudioMode::Dub
        } else {
            AudioMode::Sub
        }
    }))
}

/// Update or insert continue_watching without marking episode in history as completed yet.
pub async fn touch_continue_watching(
    pool:     &SqlitePool,
    anime_id: i64,
    episode:  i64,
    now:      i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO continue_watching (anime_id, last_episode, last_watched)
         VALUES (?, ?, ?)
         ON CONFLICT(anime_id) DO UPDATE SET
             last_episode = excluded.last_episode,
             last_watched = excluded.last_watched",
    )
    .bind(anime_id)
    .bind(episode)
    .bind(now)
    .execute(pool)
    .await?;

    let _ = sqlx::query(
        "DELETE FROM dismissed_sync WHERE anime_id = ? AND list_type = 'continue_watching'",
    )
    .bind(anime_id)
    .execute(pool)
    .await;

    Ok(())
}

/// Save live playback progress (second offset and duration) for an episode.
pub async fn save_playback_progress(
    pool:          &SqlitePool,
    anime_id:      i64,
    episode:       u32,
    position_secs: f64,
    duration_secs: f64,
    now:           i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO playback_progress (anime_id, episode, position_secs, duration_secs, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(anime_id, episode) DO UPDATE SET
             position_secs = excluded.position_secs,
             duration_secs = excluded.duration_secs,
             updated_at    = excluded.updated_at",
    )
    .bind(anime_id)
    .bind(episode as i64)
    .bind(position_secs)
    .bind(duration_secs)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

/// Retrieve the saved playback progress (position, duration) for an episode.
pub async fn get_playback_progress(
    pool:     &SqlitePool,
    anime_id: i64,
    episode:  u32,
) -> Result<Option<(f64, f64)>> {
    let row = sqlx::query_as::<_, (f64, f64)>(
        "SELECT position_secs, duration_secs FROM playback_progress WHERE anime_id = ? AND episode = ?",
    )
    .bind(anime_id)
    .bind(episode as i64)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Fetch playback progress for all episodes of an anime.
pub async fn get_all_progress_for_anime(
    pool:     &SqlitePool,
    anime_id: i64,
) -> Result<HashMap<u32, (f64, f64)>> {
    let rows = sqlx::query_as::<_, (i64, f64, f64)>(
        "SELECT episode, position_secs, duration_secs FROM playback_progress WHERE anime_id = ?",
    )
    .bind(anime_id)
    .fetch_all(pool)
    .await?;

    let mut map = HashMap::new();
    for (ep, pos, dur) in rows {
        map.insert(ep as u32, (pos, dur));
    }
    Ok(map)
}

/// Save filler classifications for an anime.
pub async fn save_anime_fillers(
    pool:     &SqlitePool,
    anime_id: i64,
    fillers:  &HashMap<u32, String>,
) -> Result<()> {
    for (&ep, ftype) in fillers {
        sqlx::query(
            "INSERT INTO anime_fillers (anime_id, episode, filler_type)
             VALUES (?, ?, ?)
             ON CONFLICT(anime_id, episode) DO UPDATE SET filler_type = excluded.filler_type",
        )
        .bind(anime_id)
        .bind(ep as i64)
        .bind(ftype)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Fetch cached filler classifications for an anime.
pub async fn get_anime_fillers(
    pool:     &SqlitePool,
    anime_id: i64,
) -> Result<HashMap<u32, String>> {
    let rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT episode, filler_type FROM anime_fillers WHERE anime_id = ?",
    )
    .bind(anime_id)
    .fetch_all(pool)
    .await?;

    let mut map = HashMap::new();
    for (ep, ftype) in rows {
        map.insert(ep as u32, ftype);
    }
    Ok(map)
}

/// Save or update an offline downloaded episode record.
pub async fn save_offline_episode(
    pool:      &SqlitePool,
    anime_id:  i64,
    episode:   u32,
    file_path: &str,
    file_size: i64,
    now:       i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO offline_episodes (anime_id, episode, file_path, file_size, downloaded_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(anime_id, episode) DO UPDATE SET
             file_path     = excluded.file_path,
             file_size     = excluded.file_size,
             downloaded_at = excluded.downloaded_at",
    )
    .bind(anime_id)
    .bind(episode as i64)
    .bind(file_path)
    .bind(file_size)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

/// Fetch all offline downloaded episodes for an anime, returning map of episode -> file_path.
pub async fn get_offline_episodes(
    pool:     &SqlitePool,
    anime_id: i64,
) -> Result<HashMap<u32, String>> {
    let rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT episode, file_path FROM offline_episodes WHERE anime_id = ?",
    )
    .bind(anime_id)
    .fetch_all(pool)
    .await?;

    let mut map = HashMap::new();
    for (ep, path) in rows {
        map.insert(ep as u32, path);
    }
    Ok(map)
}

/// Remove an offline episode record.
#[allow(dead_code)]
pub async fn delete_offline_episode(
    pool:     &SqlitePool,
    anime_id: i64,
    episode:  u32,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM offline_episodes WHERE anime_id = ? AND episode = ?",
    )
    .bind(anime_id)
    .bind(episode as i64)
    .execute(pool)
    .await?;

    Ok(())
}

// ─── Analytics / Personal Anime Wrapped ───────────────────────────────────────

/// Individual show summary in the user's top watched rotation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TopWatchedAnime {
    pub id: i64,
    pub title: String,
    pub episodes_watched: usize,
    pub total_episodes: Option<usize>,
    pub total_hours: f64,
    pub completion_pct: f64,
    pub season_year: Option<i64>,
    pub format: String,
    pub score: Option<i64>,
    pub genres: Vec<String>,
    pub canon_ratio_pct: Option<f64>,
    pub last_watched_at: i64,
}

/// Queued item promoted from Watchlist when rotation backlog is displayed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WatchlistQueueItem {
    pub id: i64,
    pub title: String,
    pub remaining_episodes: Option<usize>,
    pub est_hours: f64,
    pub completion_pct: f64,
    pub primary_genre: String,
}

/// Single recent episode event for chronological timeline logs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecentWatchEntry {
    pub anime_id: i64,
    pub title: String,
    pub episode: usize,
    pub watched_at: i64,
    pub formatted_time: String,
}

/// Aggregated telemetry and taste metrics for the user's viewing history.
#[derive(Debug, Clone, PartialEq)]
pub struct UserAnalytics {
    pub total_episodes_watched: usize,
    pub total_hours_watched: f64,
    pub completed_shows: usize,
    pub in_progress_shows: usize,
    pub completion_rate: f64,
    pub avg_score: f64,
    pub top_genres: Vec<(String, usize, f64)>, // (genre, count, percentage)
    pub era_distribution: Vec<(&'static str, usize, f64)>, // (era, count, percentage)
    pub format_distribution: Vec<(String, usize)>,
    pub recent_activity: Vec<u64>, // last 14 days activity counts
    pub taste_archetype: &'static str,
    pub top_shows: Vec<TopWatchedAnime>,
    pub viewing_cadence: &'static str,
    pub active_streak_days: usize,
    pub audio_preference: &'static str,
    pub sub_pct: f64,
    pub dub_pct: f64,
    pub hourly_distribution: [u64; 24],
    pub peak_hour: usize,
    pub weekday_distribution: [u64; 7], // Mon..Sun
    pub weekend_share_pct: f64,
    pub thermal_activity: Vec<u64>,     // 182 local days ending today
    pub thermal_month_labels: Vec<(usize, String)>, // (day_idx, month_abbr)
    pub today_weekday_idx: usize,       // 0=Mon .. 6=Sun
    pub longest_streak_days: usize,
    pub cumulative_hours_30d: Vec<f64>, // 30 days cumulative hours ending today
    pub recent_7d_hours: f64,
    pub velocity_delta_pct: Option<f64>,
    pub season_distribution: Vec<(&'static str, usize, f64)>,
    pub weighted_median_year: Option<i64>,
    pub year_span_label: String,
    pub score_buckets: [usize; 4], // [90+, 80-89, 70-79, <70]
    pub watchlist_queue: Vec<WatchlistQueueItem>,
    pub total_watchlist_shows: usize,
    pub total_watchlist_eps: usize,
    pub offline_cached_episodes: usize,
    pub offline_cached_bytes: u64,
    pub recent_watch_log: Vec<RecentWatchEntry>,
    pub weighted_canon_pct: Option<f64>,
}

impl Default for UserAnalytics {
    fn default() -> Self {
        Self {
            total_episodes_watched: 0,
            total_hours_watched: 0.0,
            completed_shows: 0,
            in_progress_shows: 0,
            completion_rate: 0.0,
            avg_score: 0.0,
            top_genres: Vec::new(),
            era_distribution: vec![
                ("Pre-2000 (Golden Age)", 0, 0.0),
                ("2000s (Classic Era)", 0, 0.0),
                ("2010s (Modern Era)", 0, 0.0),
                ("2020s (Current Era)", 0, 0.0),
            ],
            format_distribution: Vec::new(),
            recent_activity: vec![0; 14],
            taste_archetype: "✦ Eclectic Polymath",
            top_shows: Vec::new(),
            viewing_cadence: "✦ Afternoon Flow (12:00 - 18:00)",
            active_streak_days: 0,
            audio_preference: "Japanese Sub",
            sub_pct: 100.0,
            dub_pct: 0.0,
            hourly_distribution: [0; 24],
            peak_hour: 18,
            weekday_distribution: [0; 7],
            weekend_share_pct: 0.0,
            thermal_activity: vec![0; 182],
            thermal_month_labels: Vec::new(),
            today_weekday_idx: 0,
            longest_streak_days: 0,
            cumulative_hours_30d: vec![0.0; 30],
            recent_7d_hours: 0.0,
            velocity_delta_pct: None,
            season_distribution: vec![
                ("Winter", 0, 0.0),
                ("Spring", 0, 0.0),
                ("Summer", 0, 0.0),
                ("Fall", 0, 0.0),
            ],
            weighted_median_year: None,
            year_span_label: "Local Catalog".to_string(),
            score_buckets: [0; 4],
            watchlist_queue: Vec::new(),
            total_watchlist_shows: 0,
            total_watchlist_eps: 0,
            offline_cached_episodes: 0,
            offline_cached_bytes: 0,
            recent_watch_log: Vec::new(),
            weighted_canon_pct: None,
        }
    }
}

fn is_canonical_or_subgenre(g: &str) -> bool {
    const VALID_GENRES: &[&str] = &[
        // Canonical AniList genres
        "Action", "Adventure", "Comedy", "Drama", "Ecchi", "Fantasy",
        "Horror", "Mahou Shoujo", "Mecha", "Music", "Mystery", "Psychological",
        "Romance", "Sci-Fi", "Slice of Life", "Sports", "Supernatural", "Thriller",
        // Recognized major subgenres & demographics
        "Cyberpunk", "Seinen", "Shounen", "Shonen", "Shojo", "Josei", "Isekai",
        "Dystopian", "Post-Apocalyptic", "Crime", "Super Power", "Martial Arts",
        "Military", "Historical", "Space", "Football", "Basketball", "Baseball",
        "Volleyball", "Boxing", "Detective", "Philosophy", "Magic", "Steampunk",
        "Time Manipulation", "Survival", "Card Battle", "School"
    ];
    VALID_GENRES.iter().any(|&vg| vg.eq_ignore_ascii_case(g.trim()))
}

struct CalendarTelemetry {
    thermal_activity: Vec<u64>,
    thermal_month_labels: Vec<(usize, String)>,
    today_weekday_idx: usize,
    recent_activity: Vec<u64>,
    active_streak_days: usize,
    longest_streak_days: usize,
    velocity_delta_pct: Option<f64>,
    cumulative_hours_30d: Vec<f64>,
    recent_7d_hours: f64,
}

async fn load_calendar_telemetry(
    pool: &SqlitePool,
    avg_hours_per_ep: f64,
) -> CalendarTelemetry {
    let history_day_rows = sqlx::query(
        "SELECT date(watched_at, 'unixepoch', 'localtime') as day_str,
                CAST(julianday(date(watched_at, 'unixepoch', 'localtime')) AS INTEGER) as jday,
                COUNT(*) as cnt
         FROM history
         GROUP BY day_str
         ORDER BY jday ASC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut day_counts: HashMap<String, u64> = HashMap::new();
    let mut jdays: Vec<i64> = Vec::with_capacity(history_day_rows.len());
    for r in history_day_rows {
        let d: String = r.try_get("day_str").unwrap_or_default();
        let j: i64 = r.try_get("jday").unwrap_or(0);
        let c: i64 = r.try_get("cnt").unwrap_or(0);
        if c > 0 {
            day_counts.insert(d, c as u64);
            jdays.push(j);
        }
    }

    let mut longest_streak_days = 0usize;
    let mut run = 0usize;
    let mut prev_j: Option<i64> = None;
    for j in jdays {
        if prev_j == Some(j - 1) {
            run += 1;
        } else if prev_j != Some(j) {
            run = 1;
        }
        prev_j = Some(j);
        longest_streak_days = longest_streak_days.max(run);
    }

    // Single recursive CTE for the last 182 local calendar days (saves 181 round-trips)
    let cte_rows = sqlx::query(
        "WITH RECURSIVE dates(offset_day, day_str, dow, mon_abbr) AS (
             SELECT 181,
                    date('now', 'localtime', '-181 days'),
                    CAST(strftime('%w', date('now', 'localtime', '-181 days')) AS INTEGER),
                    CASE strftime('%m', date('now', 'localtime', '-181 days'))
                        WHEN '01' THEN 'Jan' WHEN '02' THEN 'Feb' WHEN '03' THEN 'Mar'
                        WHEN '04' THEN 'Apr' WHEN '05' THEN 'May' WHEN '06' THEN 'Jun'
                        WHEN '07' THEN 'Jul' WHEN '08' THEN 'Aug' WHEN '09' THEN 'Sep'
                        WHEN '10' THEN 'Oct' WHEN '11' THEN 'Nov' ELSE 'Dec'
                    END
             UNION ALL
             SELECT offset_day - 1,
                    date('now', 'localtime', '-' || (offset_day - 1) || ' days'),
                    CAST(strftime('%w', date('now', 'localtime', '-' || (offset_day - 1) || ' days')) AS INTEGER),
                    CASE strftime('%m', date('now', 'localtime', '-' || (offset_day - 1) || ' days'))
                        WHEN '01' THEN 'Jan' WHEN '02' THEN 'Feb' WHEN '03' THEN 'Mar'
                        WHEN '04' THEN 'Apr' WHEN '05' THEN 'May' WHEN '06' THEN 'Jun'
                        WHEN '07' THEN 'Jul' WHEN '08' THEN 'Aug' WHEN '09' THEN 'Sep'
                        WHEN '10' THEN 'Oct' WHEN '11' THEN 'Nov' ELSE 'Dec'
                    END
             FROM dates
             WHERE offset_day > 0
         )
         SELECT day_str, dow, mon_abbr FROM dates ORDER BY offset_day DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut thermal_activity = vec![0u64; 182];
    let mut thermal_month_labels: Vec<(usize, String)> = Vec::new();
    let mut last_mon = String::new();
    let mut today_weekday_idx = 0usize;

    for (idx, row) in cte_rows.into_iter().take(182).enumerate() {
        let day_str: String = row.try_get("day_str").unwrap_or_default();
        let dow_sun0: i64 = row.try_get("dow").unwrap_or(0);
        let mon: String = row.try_get("mon_abbr").unwrap_or_default();
        thermal_activity[idx] = day_counts.get(&day_str).copied().unwrap_or(0);
        if mon != last_mon && !mon.is_empty() {
            thermal_month_labels.push((idx, mon.clone()));
            last_mon = mon;
        }
        if idx == 181 {
            // Convert SQLite %w (0=Sun..6=Sat) to Mon=0..Sun=6
            today_weekday_idx = if dow_sun0 == 0 { 6 } else { (dow_sun0 - 1) as usize };
        }
    }

    let recent_activity = thermal_activity[182 - 14..].to_vec();

    let mut active_streak_days = 0usize;
    if recent_activity[13] > 0 || recent_activity[12] > 0 {
        let mut idx = if recent_activity[13] > 0 { 13 } else { 12 };
        while recent_activity[idx] > 0 {
            active_streak_days += 1;
            if idx == 0 {
                break;
            }
            idx -= 1;
        }
    }
    longest_streak_days = longest_streak_days.max(active_streak_days);

    let last_14: u64 = thermal_activity[182 - 14..].iter().sum();
    let prev_14: u64 = thermal_activity[182 - 28..182 - 14].iter().sum();
    let velocity_delta_pct = if prev_14 > 0 {
        Some(((last_14 as f64 - prev_14 as f64) / prev_14 as f64) * 100.0)
    } else {
        None
    };

    let ep_hours = avg_hours_per_ep.max(0.4);
    let total_all_eps: u64 = day_counts.values().sum();
    let last_30_eps: u64 = thermal_activity[182 - 30..].iter().sum();
    let base_hours = total_all_eps.saturating_sub(last_30_eps) as f64 * ep_hours;

    let mut cumulative_hours_30d = Vec::with_capacity(30);
    let mut acc = base_hours;
    for &c in &thermal_activity[182 - 30..] {
        acc += c as f64 * ep_hours;
        cumulative_hours_30d.push(acc);
    }
    let recent_7d_eps: u64 = thermal_activity[182 - 7..].iter().sum();
    let recent_7d_hours = recent_7d_eps as f64 * ep_hours;

    CalendarTelemetry {
        thermal_activity,
        thermal_month_labels,
        today_weekday_idx,
        recent_activity,
        active_streak_days,
        longest_streak_days,
        velocity_delta_pct,
        cumulative_hours_30d,
        recent_7d_hours,
    }
}

/// Query local SQLite watch history and anime metadata to generate personalized analytics.
pub async fn compute_user_analytics(pool: &SqlitePool) -> Result<UserAnalytics> {
    // 1. Total distinct episodes watched & duration calculation
    let total_episodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let tracked_seconds: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(position_secs), 0.0) FROM playback_progress",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);

    let tracked_episodes_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM playback_progress",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let untracked_episodes = (total_episodes - tracked_episodes_count).max(0);
    let total_seconds = tracked_seconds + (untracked_episodes as f64 * 24.0 * 60.0);
    let total_hours = total_seconds / 3600.0;

    // 2. Watched anime statistics (genres, eras, seasons, formats, scores, canon, rotation)
    let rows = sqlx::query(
        "SELECT a.id, a.title_english, a.title_romaji, a.genres, a.season, a.season_year,
                a.format, a.score, a.episodes,
                COUNT(h.episode) as watched_count, MAX(h.watched_at) as last_watched_at
         FROM history h
         JOIN anime a ON h.anime_id = a.id
         GROUP BY a.id",
    )
    .fetch_all(pool)
    .await?;

    let progress_rows = sqlx::query(
        "SELECT anime_id, COALESCE(SUM(position_secs), 0.0) as dur, COUNT(*) as cnt
         FROM playback_progress
         GROUP BY anime_id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut anime_durations: HashMap<i64, (f64, i64)> = HashMap::new();
    for r in progress_rows {
        let aid: i64 = r.try_get("anime_id").unwrap_or(0);
        let dur: f64 = r.try_get("dur").unwrap_or(0.0);
        let cnt: i64 = r.try_get("cnt").unwrap_or(0);
        anime_durations.insert(aid, (dur, cnt));
    }

    let filler_rows = sqlx::query(
        "SELECT h.anime_id,
                COUNT(*) as tagged_cnt,
                SUM(CASE WHEN f.filler_type != 'filler' THEN 1 ELSE 0 END) as canon_cnt
         FROM history h
         JOIN anime_fillers f ON h.anime_id = f.anime_id AND h.episode = f.episode
         GROUP BY h.anime_id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut anime_canon: HashMap<i64, (i64, i64)> = HashMap::new();
    let mut total_tagged_canon_eps = 0i64;
    let mut total_canon_eps = 0i64;
    for r in filler_rows {
        let aid: i64 = r.try_get("anime_id").unwrap_or(0);
        let tagged: i64 = r.try_get("tagged_cnt").unwrap_or(0);
        let canon: i64 = r.try_get("canon_cnt").unwrap_or(0);
        anime_canon.insert(aid, (canon, tagged));
        total_tagged_canon_eps += tagged;
        total_canon_eps += canon;
    }

    let weighted_canon_pct = if total_tagged_canon_eps > 0 {
        Some((total_canon_eps as f64 / total_tagged_canon_eps as f64) * 100.0)
    } else {
        None
    };

    let mut completed_shows = 0usize;
    let mut in_progress_shows = 0usize;
    let mut total_score = 0.0f64;
    let mut scored_count = 0usize;
    let mut score_buckets = [0usize; 4];

    let mut genre_counts: HashMap<String, usize> = HashMap::new();
    let mut total_genre_tags = 0usize;

    let mut pre_2000 = 0usize;
    let mut era_2000s = 0usize;
    let mut era_2010s = 0usize;
    let mut era_2020s = 0usize;
    let mut total_dated_shows = 0usize;
    let mut weighted_year_sum = 0i64;
    let mut weighted_year_eps = 0i64;
    let mut min_year: Option<i64> = None;
    let mut max_year: Option<i64> = None;

    let mut season_counts = [0usize; 4]; // [Winter, Spring, Summer, Fall]
    let mut total_seasonal_shows = 0usize;

    let mut format_counts: HashMap<String, usize> = HashMap::new();
    let mut top_shows: Vec<TopWatchedAnime> = Vec::new();

    for row in &rows {
        let aid: i64 = row.try_get("id").unwrap_or(0);
        let title_eng: Option<String> = row.try_get("title_english").unwrap_or(None);
        let title_rom: String = row.try_get("title_romaji").unwrap_or_else(|_| "Unknown".to_string());
        let display_title = title_eng.filter(|s| !s.trim().is_empty()).unwrap_or(title_rom);

        let watched: i64 = row.try_get("watched_count").unwrap_or(0);
        let last_watched_at: i64 = row.try_get("last_watched_at").unwrap_or(0);
        let ep_opt: Option<i64> = row.try_get("episodes").unwrap_or(None);
        let score_opt: Option<i64> = row.try_get("score").unwrap_or(None);
        let season_opt: Option<String> = row.try_get("season").unwrap_or(None);
        let year_opt: Option<i64> = row.try_get("season_year").unwrap_or(None);
        let format_opt: Option<String> = row.try_get("format").unwrap_or(None);
        let genres_raw: String = row.try_get("genres").unwrap_or_else(|_| "[]".to_string());

        let eps_watched = watched as usize;
        let total_eps = ep_opt.map(|e| e as usize);
        let completion_pct = match total_eps {
            Some(t) if t > 0 => ((eps_watched as f64 / t as f64) * 100.0).min(100.0),
            _ => 0.0,
        };

        let (tracked_sec, tracked_cnt) = anime_durations.get(&aid).copied().unwrap_or((0.0, 0));
        let untracked = watched.saturating_sub(tracked_cnt).max(0);
        let anime_total_sec = tracked_sec + (untracked as f64 * 1440.0);
        let anime_hours = anime_total_sec / 3600.0;

        let parsed_genres: Vec<String> = serde_json::from_str::<Vec<String>>(&genres_raw)
            .unwrap_or_default()
            .into_iter()
            .map(|g| g.trim().to_string())
            .filter(|g| is_canonical_or_subgenre(g))
            .collect();

        for g in &parsed_genres {
            *genre_counts.entry(g.clone()).or_insert(0) += eps_watched;
            total_genre_tags += eps_watched;
        }

        let fmt = format_opt.unwrap_or_else(|| "TV".to_string());
        *format_counts.entry(fmt.clone()).or_insert(0) += 1;

        let canon_ratio_pct = anime_canon.get(&aid).and_then(|&(c, t)| {
            (t > 0).then(|| (c as f64 / t as f64) * 100.0)
        });

        top_shows.push(TopWatchedAnime {
            id: aid,
            title: display_title,
            episodes_watched: eps_watched,
            total_episodes: total_eps,
            total_hours: anime_hours,
            completion_pct,
            season_year: year_opt,
            format: fmt,
            score: score_opt,
            genres: parsed_genres,
            canon_ratio_pct,
            last_watched_at,
        });

        if let Some(total_eps) = ep_opt {
            if total_eps > 0 && watched >= total_eps {
                completed_shows += 1;
            } else {
                in_progress_shows += 1;
            }
        } else if watched > 0 {
            in_progress_shows += 1;
        }

        if let Some(score) = score_opt {
            if score > 0 {
                total_score += score as f64;
                scored_count += 1;
                if score >= 90 {
                    score_buckets[0] += 1;
                } else if score >= 80 {
                    score_buckets[1] += 1;
                } else if score >= 70 {
                    score_buckets[2] += 1;
                } else {
                    score_buckets[3] += 1;
                }
            }
        }

        if let Some(year) = year_opt {
            total_dated_shows += 1;
            weighted_year_sum += year * watched.max(1);
            weighted_year_eps += watched.max(1);
            min_year = Some(min_year.map_or(year, |m| m.min(year)));
            max_year = Some(max_year.map_or(year, |m| m.max(year)));
            if year < 2000 {
                pre_2000 += 1;
            } else if year < 2010 {
                era_2000s += 1;
            } else if year < 2020 {
                era_2010s += 1;
            } else {
                era_2020s += 1;
            }
        }

        if let Some(s) = season_opt.as_deref() {
            match s.to_ascii_uppercase().as_str() {
                "WINTER" => { season_counts[0] += 1; total_seasonal_shows += 1; }
                "SPRING" => { season_counts[1] += 1; total_seasonal_shows += 1; }
                "SUMMER" => { season_counts[2] += 1; total_seasonal_shows += 1; }
                "FALL" | "AUTUMN" => { season_counts[3] += 1; total_seasonal_shows += 1; }
                _ => {}
            }
        }
    }

    top_shows.sort_by(|a, b| {
        b.episodes_watched
            .cmp(&a.episodes_watched)
            .then_with(|| b.total_hours.partial_cmp(&a.total_hours).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| b.last_watched_at.cmp(&a.last_watched_at))
    });
    top_shows.truncate(12);

    let total_shows = completed_shows + in_progress_shows;
    let completion_rate = if total_shows > 0 {
        (completed_shows as f64 / total_shows as f64) * 100.0
    } else {
        0.0
    };

    let avg_score = if scored_count > 0 {
        total_score / scored_count as f64
    } else {
        0.0
    };

    let mut top_genres: Vec<(String, usize, f64)> = genre_counts
        .into_iter()
        .map(|(g, count)| {
            let pct = if total_genre_tags > 0 {
                (count as f64 / total_genre_tags as f64) * 100.0
            } else {
                0.0
            };
            (g, count, pct)
        })
        .collect();
    top_genres.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top_genres.truncate(14);

    let dated_div = total_dated_shows.max(1) as f64;
    let era_distribution = vec![
        ("Pre-2000 (Golden Age)", pre_2000, (pre_2000 as f64 / dated_div) * 100.0),
        ("2000s (Classic Era)",   era_2000s, (era_2000s as f64 / dated_div) * 100.0),
        ("2010s (Modern Era)",    era_2010s, (era_2010s as f64 / dated_div) * 100.0),
        ("2020s (Current Era)",   era_2020s, (era_2020s as f64 / dated_div) * 100.0),
    ];

    let weighted_median_year = (weighted_year_eps > 0).then(|| weighted_year_sum / weighted_year_eps);
    let year_span_label = match (min_year, max_year) {
        (Some(lo), Some(hi)) if lo == hi => {
            let tag = if lo < 2000 { "Golden Age" } else if lo < 2010 { "Classic" } else if lo < 2020 { "Modern" } else { "Current" };
            format!("{} {}", lo, tag)
        }
        (Some(lo), Some(hi)) => format!("{}–{} Catalog", lo, hi),
        _ => "Local Catalog".to_string(),
    };

    let s_div = total_seasonal_shows.max(1) as f64;
    let season_distribution = vec![
        ("Winter", season_counts[0], (season_counts[0] as f64 / s_div) * 100.0),
        ("Spring", season_counts[1], (season_counts[1] as f64 / s_div) * 100.0),
        ("Summer", season_counts[2], (season_counts[2] as f64 / s_div) * 100.0),
        ("Fall",   season_counts[3], (season_counts[3] as f64 / s_div) * 100.0),
    ];

    let mut format_distribution: Vec<(String, usize)> = format_counts.into_iter().collect();
    format_distribution.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    // 3. Calendar telemetry (single CTE, streaks, cumulative hours)
    let avg_hours_per_ep = if total_episodes > 0 {
        total_hours / total_episodes as f64
    } else {
        0.4
    };
    let cal = load_calendar_telemetry(pool, avg_hours_per_ep).await;

    // 4. Local hour & weekday histograms
    let hour_rows = sqlx::query(
        "SELECT CAST(strftime('%H', watched_at, 'unixepoch', 'localtime') AS INTEGER) as hr,
                CAST(strftime('%w', watched_at, 'unixepoch', 'localtime') AS INTEGER) as dow,
                COUNT(*) as cnt
         FROM history
         GROUP BY hr, dow",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut hour_buckets = [0usize; 4]; // [Night (22-04), Morning (05-11), Afternoon (12-17), Prime (18-21)]
    let mut hourly_distribution = [0u64; 24];
    let mut weekday_distribution = [0u64; 7]; // Mon=0 .. Sun=6
    for r in hour_rows {
        let hr: i64 = r.try_get("hr").unwrap_or(0);
        let dow_sun0: i64 = r.try_get("dow").unwrap_or(0);
        let cnt: i64 = r.try_get("cnt").unwrap_or(0);
        let c = cnt.max(0) as u64;

        let hr_idx = hr.clamp(0, 23) as usize;
        hourly_distribution[hr_idx] += c;

        let mon_idx = if dow_sun0 == 0 { 6 } else { (dow_sun0 - 1).clamp(0, 6) as usize };
        weekday_distribution[mon_idx] += c;

        match hr {
            22..=23 | 0..=4 => hour_buckets[0] += c as usize,
            5..=11          => hour_buckets[1] += c as usize,
            12..=17         => hour_buckets[2] += c as usize,
            _               => hour_buckets[3] += c as usize,
        }
    }

    let peak_hour = hourly_distribution
        .iter()
        .enumerate()
        .max_by_key(|&(_, v)| v)
        .filter(|&(_, &v)| v > 0)
        .map(|(h, _)| h)
        .unwrap_or(18);

    let total_dow_eps: u64 = weekday_distribution.iter().sum();
    let weekend_share_pct = if total_dow_eps > 0 {
        let sat_sun = weekday_distribution[5] + weekday_distribution[6];
        (sat_sun as f64 / total_dow_eps as f64) * 100.0
    } else {
        0.0
    };

    let viewing_cadence = if hour_buckets[2] >= hour_buckets[0] && hour_buckets[2] >= hour_buckets[1] && hour_buckets[2] >= hour_buckets[3] {
        "✦ Afternoon Flow (12:00 - 18:00)"
    } else if hour_buckets[1] >= hour_buckets[0] && hour_buckets[1] >= hour_buckets[3] {
        "⬡ Dawn & Morning (05:00 - 12:00)"
    } else if hour_buckets[3] >= hour_buckets[0] {
        "◈ Prime Time (18:00 - 22:00)"
    } else {
        "❖ Night Owl (22:00 - 05:00)"
    };

    // 5. Real SUB/DUB counts from audio_prefs (fixed from legacy playback_prefs typo)
    let sub_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audio_prefs WHERE audio_mode = 'sub'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let dub_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audio_prefs WHERE audio_mode = 'dub'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let pref_total = sub_count + dub_count;
    let (sub_pct, dub_pct, audio_preference) = if pref_total > 0 {
        let sp = (sub_count as f64 / pref_total as f64) * 100.0;
        let dp = (dub_count as f64 / pref_total as f64) * 100.0;
        let label = if dub_count > sub_count { "English Dub" } else { "Japanese Sub" };
        (sp, dp, label)
    } else if total_episodes > 0 {
        (100.0, 0.0, "Japanese Sub")
    } else {
        (50.0, 50.0, "Sub / Dub")
    };

    // 6. Watchlist backlog horizon (sorted newest saved first)
    let wl_rows = sqlx::query(
        "SELECT a.id, a.title_english, a.title_romaji, a.episodes, a.genres,
                (SELECT COUNT(*) FROM history h WHERE h.anime_id = a.id) as watched_eps
         FROM watchlist w
         JOIN anime a ON w.anime_id = a.id
         ORDER BY w.added_at DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut watchlist_queue = Vec::new();
    let mut total_watchlist_eps = 0usize;
    let total_watchlist_shows = wl_rows.len();
    for r in wl_rows {
        let id: i64 = r.try_get("id").unwrap_or(0);
        let title_eng: Option<String> = r.try_get("title_english").unwrap_or(None);
        let title_rom: String = r.try_get("title_romaji").unwrap_or_else(|_| "Unknown".to_string());
        let title = title_eng.filter(|s| !s.trim().is_empty()).unwrap_or(title_rom);
        let total_ep: Option<i64> = r.try_get("episodes").unwrap_or(None);
        let watched_ep: i64 = r.try_get("watched_eps").unwrap_or(0);
        let genres_raw: String = r.try_get("genres").unwrap_or_else(|_| "[]".to_string());
        let primary_genre = serde_json::from_str::<Vec<String>>(&genres_raw)
            .ok()
            .and_then(|v| v.into_iter().find(|g| is_canonical_or_subgenre(g)))
            .unwrap_or_else(|| "Anime".to_string());

        let (rem_ep, est_hours, comp_pct) = match total_ep {
            Some(tot) if tot > 0 => {
                let rem = tot.saturating_sub(watched_ep) as usize;
                let pct = ((watched_ep as f64 / tot as f64) * 100.0).min(100.0);
                total_watchlist_eps += rem;
                (Some(rem), rem as f64 * 0.4, pct)
            }
            _ => (None, 4.8, 0.0),
        };
        if watchlist_queue.len() < 16 {
            watchlist_queue.push(WatchlistQueueItem {
                id,
                title,
                remaining_episodes: rem_ep,
                est_hours,
                completion_pct: comp_pct,
                primary_genre,
            });
        }
    }

    // 7. Offline footprint & chronological watch log (for Lens 3 / Pipeline KPI)
    let offline_stats = sqlx::query(
        "SELECT COUNT(*) as cnt, COALESCE(SUM(file_size), 0) as bytes FROM offline_episodes",
    )
    .fetch_one(pool)
    .await;
    let (offline_cached_episodes, offline_cached_bytes) = match offline_stats {
        Ok(r) => (
            r.try_get::<i64, _>("cnt").unwrap_or(0).max(0) as usize,
            r.try_get::<i64, _>("bytes").unwrap_or(0).max(0) as u64,
        ),
        Err(_) => (0, 0),
    };

    let log_rows = sqlx::query(
        "SELECT h.anime_id, h.episode, h.watched_at,
                a.title_english, a.title_romaji,
                strftime('%m-%d %H:%M', h.watched_at, 'unixepoch', 'localtime') as fmt_time
         FROM history h
         JOIN anime a ON h.anime_id = a.id
         ORDER BY h.watched_at DESC
         LIMIT 25",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let recent_watch_log = log_rows
        .into_iter()
        .map(|r| {
            let eng: Option<String> = r.try_get("title_english").unwrap_or(None);
            let rom: String = r.try_get("title_romaji").unwrap_or_else(|_| "Unknown".to_string());
            RecentWatchEntry {
                anime_id: r.try_get("anime_id").unwrap_or(0),
                title: eng.filter(|s| !s.trim().is_empty()).unwrap_or(rom),
                episode: r.try_get::<i64, _>("episode").unwrap_or(1).max(1) as usize,
                watched_at: r.try_get("watched_at").unwrap_or(0),
                formatted_time: r.try_get("fmt_time").unwrap_or_default(),
            }
        })
        .collect();

    // 8. Personality archetype from dominant genre
    let taste_archetype = match top_genres.first().map(|(g, _, _)| g.as_str()) {
        Some("Sports") | Some("Football") | Some("Basketball") | Some("Baseball") => "✦ Kinetic Tactician & Grit Striker",
        Some("Cyberpunk")     => "✦ High-Tech Neon Dystopian",
        Some("Seinen")        => "✦ Cerebral Realist & Narrative Analyst",
        Some("Psychological") => "✦ The Narrative Philosopher",
        Some("Sci-Fi")        => "✦ Cyberpunk Visionary & Futurist",
        Some("Action")        => "✦ Apex Kinetic Connoisseur",
        Some("Drama")         => "✦ Melancholic Humanist",
        Some("Fantasy")       => "✦ Myth Seeker & Worldbuilder",
        Some("Mystery")       => "✦ Labyrinth Mind & Enigma Decoder",
        Some("Comedy")        => "✦ Cathartic Absurdist",
        Some("Slice of Life") => "✦ Zen Pastoralist",
        Some("Romance")       => "✦ Romantic Idealist",
        Some("Horror")        => "✦ Gothic Surrealist",
        Some("Mecha")         => "✦ Heavy Iron Pilot",
        Some("Shounen") | Some("Shonen") => "✦ High-Spirited Champion",
        _                     => "✦ Eclectic Polymath",
    };

    Ok(UserAnalytics {
        total_episodes_watched: total_episodes as usize,
        total_hours_watched: total_hours,
        completed_shows,
        in_progress_shows,
        completion_rate,
        avg_score,
        top_genres,
        era_distribution,
        format_distribution,
        recent_activity: cal.recent_activity,
        taste_archetype,
        top_shows,
        viewing_cadence,
        active_streak_days: cal.active_streak_days,
        audio_preference,
        sub_pct,
        dub_pct,
        hourly_distribution,
        peak_hour,
        weekday_distribution,
        weekend_share_pct,
        thermal_activity: cal.thermal_activity,
        thermal_month_labels: cal.thermal_month_labels,
        today_weekday_idx: cal.today_weekday_idx,
        longest_streak_days: cal.longest_streak_days,
        cumulative_hours_30d: cal.cumulative_hours_30d,
        recent_7d_hours: cal.recent_7d_hours,
        velocity_delta_pct: cal.velocity_delta_pct,
        season_distribution,
        weighted_median_year,
        year_span_label,
        score_buckets,
        watchlist_queue,
        total_watchlist_shows,
        total_watchlist_eps,
        offline_cached_episodes,
        offline_cached_bytes,
        recent_watch_log,
        weighted_canon_pct,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{cache::{upsert_anime, Anime}, init};

    async fn setup() -> SqlitePool {
        let pool = init(":memory:").await.unwrap();
        // Insert a dummy anime so FK constraints pass
        let anime = Anime {
            id:            1,
            title_english: Some("Test Anime".into()),
            title_romaji:  "Test Anime".into(),
            title_native:  None,
            description:   None,
            episodes:      Some(12),
            status:        Some("FINISHED".into()),
            season:        None,
            season_year:   None,
            score:         Some(80),
            format:        Some("TV".into()),
            genres:        "[]".into(),
            cover_url:     None,
            cover_blob:    None,
            has_dub:       0,
            updated_at:    1_000_000,
            next_airing_episode: None,
        };
        upsert_anime(&pool, &anime).await.unwrap();
        pool
    }

    // ── History ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_record_watched_creates_history_entry() {
        let pool = setup().await;
        record_watched(&pool, 1, 3, 1_700_000_000).await.unwrap();

        let history = get_history(&pool, 1).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].episode, 3);
    }

    #[tokio::test]
    async fn test_record_watched_updates_timestamp() {
        let pool = setup().await;
        record_watched(&pool, 1, 1, 100).await.unwrap();
        record_watched(&pool, 1, 1, 200).await.unwrap(); // re-watch

        let history = get_history(&pool, 1).await.unwrap();
        assert_eq!(history.len(), 1); // still only one entry
        assert_eq!(history[0].watched_at, 200);
    }

    #[tokio::test]
    async fn test_is_watched_true_and_false() {
        let pool = setup().await;
        record_watched(&pool, 1, 5, 100).await.unwrap();

        assert!(is_watched(&pool, 1, 5).await.unwrap());
        assert!(!is_watched(&pool, 1, 6).await.unwrap());
    }

    // ── Continue Watching ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_record_watched_updates_continue_watching() {
        let pool = setup().await;
        record_watched(&pool, 1, 4, 1_000).await.unwrap();

        let entry = get_continue_entry(&pool, 1).await.unwrap().unwrap();
        assert_eq!(entry.last_episode, 4);
        assert_eq!(entry.last_watched, 1_000);
    }

    #[tokio::test]
    async fn test_continue_watching_ordered_by_recency() {
        let pool = init(":memory:").await.unwrap();

        // Insert two anime
        for id in [1i64, 2i64] {
            let anime = Anime {
                id,
                title_english: Some(format!("Anime {}", id)),
                title_romaji:  format!("Anime {}", id),
                title_native:  None,
                description:   None,
                episodes:      Some(12),
                status:        Some("FINISHED".into()),
                season:        None,
                season_year:   None,
                score:         Some(80),
                format:        Some("TV".into()),
                genres:        "[]".into(),
                cover_url:     None,
                cover_blob:    None,
                has_dub:       0,
                updated_at:    1_000_000,
                next_airing_episode: None,
            };
            upsert_anime(&pool, &anime).await.unwrap();
        }

        record_watched(&pool, 1, 1, 1_000).await.unwrap();
        record_watched(&pool, 2, 1, 2_000).await.unwrap(); // more recent

        let entries = get_continue_watching(&pool).await.unwrap();
        assert_eq!(entries[0].anime_id, 2); // most recent first
        assert_eq!(entries[1].anime_id, 1);
    }

    #[tokio::test]
    async fn test_remove_continue_watching() {
        let pool = setup().await;
        record_watched(&pool, 1, 2, 100).await.unwrap();
        assert!(get_continue_entry(&pool, 1).await.unwrap().is_some());
        assert!(!get_watched_episodes(&pool, 1).await.unwrap().is_empty());

        remove_continue_watching(&pool, 1).await.unwrap();
        assert!(get_continue_entry(&pool, 1).await.unwrap().is_none());
        assert!(get_watched_episodes(&pool, 1).await.unwrap().is_empty());
    }

    // ── Watchlist ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_add_to_watchlist() {
        let pool = setup().await;
        add_to_watchlist(&pool, 1, 100).await.unwrap();
        assert!(is_in_watchlist(&pool, 1).await.unwrap());
    }

    #[tokio::test]
    async fn test_remove_from_watchlist() {
        let pool = setup().await;
        add_to_watchlist(&pool, 1, 100).await.unwrap();
        remove_from_watchlist(&pool, 1).await.unwrap();
        assert!(!is_in_watchlist(&pool, 1).await.unwrap());
    }

    #[tokio::test]
    async fn test_add_watchlist_is_idempotent() {
        let pool = setup().await;
        add_to_watchlist(&pool, 1, 100).await.unwrap();
        add_to_watchlist(&pool, 1, 200).await.unwrap(); // should not error or duplicate
        let list = get_watchlist(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn test_get_watchlist_returns_ids() {
        let pool = setup().await;
        add_to_watchlist(&pool, 1, 100).await.unwrap();
        let list = get_watchlist(&pool).await.unwrap();
        assert!(list.contains(&1));
    }

    #[tokio::test]
    async fn test_not_in_watchlist_by_default() {
        let pool = setup().await;
        assert!(!is_in_watchlist(&pool, 1).await.unwrap());
    }

    // ── Watched episodes ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_watched_episodes_returns_distinct_episodes() {
        let pool = setup().await;
        record_watched(&pool, 1, 3, 1_000).await.unwrap();
        record_watched(&pool, 1, 5, 2_000).await.unwrap();
        record_watched(&pool, 1, 3, 3_000).await.unwrap(); // re-watch ep3 — still only 1 entry
        let eps = get_watched_episodes(&pool, 1).await.unwrap();
        assert_eq!(eps.len(), 2);
        assert!(eps.contains(&3));
        assert!(eps.contains(&5));
    }

    #[tokio::test]
    async fn test_get_watched_episodes_empty_for_unwatched() {
        let pool = setup().await;
        let eps = get_watched_episodes(&pool, 1).await.unwrap();
        assert!(eps.is_empty());
    }

    #[tokio::test]
    async fn test_get_watched_counts_returns_per_show_counts() {
        let pool = init(":memory:").await.unwrap();

        for id in [1i64, 2i64] {
            let anime = Anime {
                id,
                title_english: Some(format!("Anime {}", id)),
                title_romaji: format!("Anime {}", id),
                title_native: None,
                description: None,
                episodes: Some(12),
                status: Some("FINISHED".into()),
                season: None,
                season_year: None,
                score: Some(80),
                format: Some("TV".into()),
                genres: "[]".into(),
                cover_url: None,
                cover_blob: None,
                has_dub: 0,
                updated_at: 1_000_000,
                next_airing_episode: None,
            };
            upsert_anime(&pool, &anime).await.unwrap();
        }

        record_watched(&pool, 1, 1, 100).await.unwrap();
        record_watched(&pool, 1, 2, 200).await.unwrap();
        record_watched(&pool, 2, 1, 300).await.unwrap();

        let counts = get_watched_counts(&pool, &[1, 2]).await.unwrap();
        assert_eq!(counts.get(&1), Some(&2));
        assert_eq!(counts.get(&2), Some(&1));
    }

    #[tokio::test]
    async fn test_set_and_get_playback_query() {
        let pool = setup().await;
        set_playback_query(&pool, 1, "Attack on Titan Final Season", 123).await.unwrap();

        let query = get_playback_query(&pool, 1).await.unwrap();
        assert_eq!(query.as_deref(), Some("Attack on Titan Final Season"));
    }

    #[tokio::test]
    async fn test_set_playback_query_updates_existing() {
        let pool = setup().await;
        set_playback_query(&pool, 1, "Old Query", 100).await.unwrap();
        set_playback_query(&pool, 1, "New Query", 200).await.unwrap();

        let query = get_playback_query(&pool, 1).await.unwrap();
        assert_eq!(query.as_deref(), Some("New Query"));
    }

    #[tokio::test]
    async fn test_set_and_get_audio_mode() {
        let pool = setup().await;
        set_audio_mode(&pool, 1, AudioMode::Dub, 123).await.unwrap();

        let audio = get_audio_mode(&pool, 1).await.unwrap();
        assert_eq!(audio, Some(AudioMode::Dub));
    }

    #[tokio::test]
    async fn test_set_audio_mode_updates_existing() {
        let pool = setup().await;
        set_audio_mode(&pool, 1, AudioMode::Sub, 100).await.unwrap();
        set_audio_mode(&pool, 1, AudioMode::Dub, 200).await.unwrap();

        let audio = get_audio_mode(&pool, 1).await.unwrap();
        assert_eq!(audio, Some(AudioMode::Dub));
    }

    #[tokio::test]
    async fn test_dismissed_sync_persists_and_clears_on_readd() {
        let pool = setup().await;
        add_to_watchlist(&pool, 1, 100).await.unwrap();
        assert!(!get_dismissed_ids(&pool, "watchlist").await.unwrap().contains(&1));

        remove_from_watchlist(&pool, 1).await.unwrap();
        assert!(get_dismissed_ids(&pool, "watchlist").await.unwrap().contains(&1));

        add_to_watchlist(&pool, 1, 200).await.unwrap();
        assert!(!get_dismissed_ids(&pool, "watchlist").await.unwrap().contains(&1));

        record_watched(&pool, 1, 1, 100).await.unwrap();
        remove_continue_watching(&pool, 1).await.unwrap();
        assert!(get_dismissed_ids(&pool, "continue_watching").await.unwrap().contains(&1));

        record_watched(&pool, 1, 2, 200).await.unwrap();
        assert!(!get_dismissed_ids(&pool, "continue_watching").await.unwrap().contains(&1));
    }

    #[tokio::test]
    async fn test_playback_progress_round_trip() {
        let pool = setup().await;
        save_playback_progress(&pool, 1, 3, 142.5, 1420.0, 100).await.unwrap();

        let prog = get_playback_progress(&pool, 1, 3).await.unwrap();
        assert!(prog.is_some());
        let (pos, dur) = prog.unwrap();
        assert!((pos - 142.5).abs() < 1e-4);
        assert!((dur - 1420.0).abs() < 1e-4);

        let all = get_all_progress_for_anime(&pool, 1).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all.get(&3), Some(&(142.5, 1420.0)));
    }

    #[tokio::test]
    async fn test_anime_fillers_round_trip() {
        let pool = setup().await;
        let mut fillers = HashMap::new();
        fillers.insert(1, "manga_canon".to_string());
        fillers.insert(7, "mixed".to_string());
        fillers.insert(26, "filler".to_string());

        save_anime_fillers(&pool, 1, &fillers).await.unwrap();
        let loaded = get_anime_fillers(&pool, 1).await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.get(&1), Some(&"manga_canon".to_string()));
        assert_eq!(loaded.get(&7), Some(&"mixed".to_string()));
        assert_eq!(loaded.get(&26), Some(&"filler".to_string()));
    }

    #[tokio::test]
    async fn test_offline_episodes_round_trip() {
        let pool = setup().await;
        save_offline_episode(&pool, 1, 2, "/path/to/ep2.mkv", 450_000_000, 100).await.unwrap();

        let off = get_offline_episodes(&pool, 1).await.unwrap();
        assert_eq!(off.len(), 1);
        assert_eq!(off.get(&2), Some(&"/path/to/ep2.mkv".to_string()));

        delete_offline_episode(&pool, 1, 2).await.unwrap();
        let empty = get_offline_episodes(&pool, 1).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn test_touch_continue_watching() {
        let pool = setup().await;
        touch_continue_watching(&pool, 1, 5, 150).await.unwrap();
        let entries = get_continue_watching(&pool).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].anime_id, 1);
        assert_eq!(entries[0].last_episode, 5);
        // Does not add to watched history
        assert!(get_watched_episodes(&pool, 1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_compute_user_analytics() {
        let pool = setup().await;
        // Insert an anime with genres and metadata
        let anime = Anime {
            id:            99,
            title_english: Some("Serial Experiments Lain".into()),
            title_romaji:  "Serial Experiments Lain".into(),
            title_native:  None,
            description:   None,
            episodes:      Some(13),
            status:        Some("FINISHED".into()),
            season:        Some("SUMMER".into()),
            season_year:   Some(1998),
            score:         Some(85),
            format:        Some("TV".into()),
            genres:        "[\"Psychological\",\"Sci-Fi\",\"Mystery\"]".into(),
            cover_url:     None,
            cover_blob:    None,
            has_dub:       1,
            updated_at:    1_000_000,
            next_airing_episode: None,
        };
        upsert_anime(&pool, &anime).await.unwrap();

        // Mark 13 episodes watched
        for ep in 1..=13 {
            record_watched(&pool, 99, ep, 1_000 + ep).await.unwrap();
        }

        // Insert second anime with Psychological
        let anime2 = Anime {
            id:            100,
            title_english: Some("Monster".into()),
            title_romaji:  "Monster".into(),
            title_native:  None,
            description:   None,
            episodes:      Some(74),
            status:        Some("FINISHED".into()),
            season:        Some("SPRING".into()),
            season_year:   Some(2004),
            score:         Some(88),
            format:        Some("TV".into()),
            genres:        "[\"Psychological\",\"Thriller\"]".into(),
            cover_url:     None,
            cover_blob:    None,
            has_dub:       1,
            updated_at:    1_000_000,
            next_airing_episode: None,
        };
        upsert_anime(&pool, &anime2).await.unwrap();
        record_watched(&pool, 100, 1, 2_000).await.unwrap();

        // Also add Monster to watchlist & set explicit audio_prefs to test new Bento fields
        add_to_watchlist(&pool, 100, 3_000).await.unwrap();
        set_audio_mode(&pool, 99, AudioMode::Dub, 3_000).await.unwrap();
        set_audio_mode(&pool, 100, AudioMode::Sub, 3_000).await.unwrap();

        let analytics = compute_user_analytics(&pool).await.unwrap();
        assert_eq!(analytics.total_episodes_watched, 14);
        assert!(analytics.total_hours_watched > 5.0);
        assert_eq!(analytics.completed_shows, 1);
        assert_eq!(analytics.in_progress_shows, 1);
        assert_eq!(analytics.top_genres[0].0, "Psychological");
        assert_eq!(analytics.top_genres[0].1, 14);
        assert_eq!(analytics.taste_archetype, "✦ The Narrative Philosopher");
        assert_eq!(analytics.era_distribution[0].0, "Pre-2000 (Golden Age)");
        assert!(analytics.era_distribution[0].1 >= 1);
        assert_eq!(analytics.top_shows.len(), 2);
        assert_eq!(analytics.top_shows[0].title, "Serial Experiments Lain");
        assert_eq!(analytics.top_shows[0].episodes_watched, 13);
        assert!(!analytics.viewing_cadence.is_empty());
        assert_eq!(analytics.audio_preference, "Japanese Sub");
        assert!((analytics.sub_pct - 50.0).abs() < 0.1);
        assert!((analytics.dub_pct - 50.0).abs() < 0.1);
        assert_eq!(analytics.score_buckets, [0, 2, 0, 0]);
        assert_eq!(analytics.weighted_median_year, Some(1998));
        assert_eq!(analytics.thermal_activity.len(), 182);
        assert_eq!(analytics.cumulative_hours_30d.len(), 30);
        assert_eq!(analytics.total_watchlist_shows, 1);
        assert_eq!(analytics.watchlist_queue[0].title, "Monster");
        assert_eq!(analytics.watchlist_queue[0].remaining_episodes, Some(73));
        assert_eq!(analytics.recent_watch_log.len(), 14);
    }
}
