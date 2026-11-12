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
}

/// Query local SQLite watch history and anime metadata to generate personalized analytics.
pub async fn compute_user_analytics(pool: &SqlitePool) -> Result<UserAnalytics> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // 1. Total distinct episodes watched
    let total_episodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    // 2. Total duration in seconds: exact position sum from playback_progress
    // plus estimated 24 mins (1440s) for watched episodes without progress rows
    let tracked_seconds: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(position_secs), 0.0) FROM playback_progress"
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);

    let tracked_episodes_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM playback_progress"
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let untracked_episodes = (total_episodes - tracked_episodes_count).max(0);
    let total_seconds = tracked_seconds + (untracked_episodes as f64 * 24.0 * 60.0);
    let total_hours = total_seconds / 3600.0;

    // 3. Watched anime statistics (genres, eras, formats, scores)
    let rows = sqlx::query(
        "SELECT a.id, a.genres, a.season_year, a.format, a.score, a.episodes,
                COUNT(h.episode) as watched_count
         FROM history h
         JOIN anime a ON h.anime_id = a.id
         GROUP BY a.id"
    )
    .fetch_all(pool)
    .await?;

    let mut completed_shows = 0usize;
    let mut in_progress_shows = 0usize;
    let mut total_score = 0.0f64;
    let mut scored_count = 0usize;

    let mut genre_counts: HashMap<String, usize> = HashMap::new();
    let mut total_genre_tags = 0usize;

    let mut pre_2000 = 0usize;
    let mut era_2000s = 0usize;
    let mut era_2010s = 0usize;
    let mut era_2020s = 0usize;
    let mut total_dated_shows = 0usize;

    let mut format_counts: HashMap<String, usize> = HashMap::new();

    for row in &rows {
        let watched: i64 = row.try_get("watched_count").unwrap_or(0);
        let ep_opt: Option<i64> = row.try_get("episodes").unwrap_or(None);
        let score_opt: Option<i64> = row.try_get("score").unwrap_or(None);
        let year_opt: Option<i64> = row.try_get("season_year").unwrap_or(None);
        let format_opt: Option<String> = row.try_get("format").unwrap_or(None);
        let genres_raw: String = row.try_get("genres").unwrap_or_else(|_| "[]".to_string());

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
            }
        }

        if let Some(year) = year_opt {
            total_dated_shows += 1;
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

        let fmt = format_opt.unwrap_or_else(|| "TV".to_string());
        *format_counts.entry(fmt).or_insert(0) += 1;

        if let Ok(genres) = serde_json::from_str::<Vec<String>>(&genres_raw) {
            for g in genres {
                *genre_counts.entry(g).or_insert(0) += 1;
                total_genre_tags += 1;
            }
        }
    }

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
    top_genres.truncate(8);

    let dated_div = total_dated_shows.max(1) as f64;
    let era_distribution = vec![
        ("Pre-2000 (Golden Age)", pre_2000, (pre_2000 as f64 / dated_div) * 100.0),
        ("2000s (Classic Era)",   era_2000s, (era_2000s as f64 / dated_div) * 100.0),
        ("2010s (Modern Era)",    era_2010s, (era_2010s as f64 / dated_div) * 100.0),
        ("2020s (Current Era)",   era_2020s, (era_2020s as f64 / dated_div) * 100.0),
    ];

    let mut format_distribution: Vec<(String, usize)> = format_counts.into_iter().collect();
    format_distribution.sort_by_key(|a| std::cmp::Reverse(a.1));

    // 4. Activity across last 14 days
    let day_seconds = 86400i64;
    let mut recent_activity = vec![0u64; 14];
    let start_14_days_ago = now - (14 * day_seconds);

    let history_timestamps: Vec<i64> = sqlx::query_scalar(
        "SELECT watched_at FROM history WHERE watched_at >= ?"
    )
    .bind(start_14_days_ago)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for ts in history_timestamps {
        let diff = now - ts;
        if diff >= 0 {
            let day_idx = (diff / day_seconds) as usize;
            if day_idx < 14 {
                recent_activity[13 - day_idx] += 1;
            }
        }
    }

    // 5. Determine personality archetype based on leading genre
    let taste_archetype = match top_genres.first().map(|(g, _, _)| g.as_str()) {
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
        recent_activity,
        taste_archetype,
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

        let analytics = compute_user_analytics(&pool).await.unwrap();
        assert_eq!(analytics.total_episodes_watched, 14);
        assert!(analytics.total_hours_watched > 5.0);
        assert_eq!(analytics.completed_shows, 1);
        assert_eq!(analytics.in_progress_shows, 1);
        assert_eq!(analytics.top_genres[0].0, "Psychological");
        assert_eq!(analytics.top_genres[0].1, 2);
        assert_eq!(analytics.taste_archetype, "✦ The Narrative Philosopher");
        assert_eq!(analytics.era_distribution[0].0, "Pre-2000 (Golden Age)");
        assert!(analytics.era_distribution[0].1 >= 1);
    }
}
