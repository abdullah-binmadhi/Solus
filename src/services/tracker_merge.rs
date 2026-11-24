//! Unified multi-tracker crosswalk resolution and deterministic state reconciliation.
//!
//! Guarantees zero duplicate anime entries across Home, Watchlist, Detail,
//! Calendar, and Analytics by resolving every MyAnimeList `mal_id` to its
//! canonical AniList `anime.id` inside SQLite and reconciling `tracker_entries`
//! into unified `history`, `continue_watching`, and `watchlist` tables.

use crate::api::anilist::{AniListClient, AniListEntryRecord};
use crate::api::mal::{normalize_mal_status, MalListEntry};
use crate::config::{MergePolicy, TrackerProvider};
use crate::db::cache::{
    execute_upsert_anime, get_anime, lookup_anilist_by_mal_ids, save_crosswalk_links, Anime,
};
use crate::db::user::{get_all_tracker_entries, get_dismissed_ids, TrackerEntryRow};
use crate::error::Result;
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Normalized snapshot row ready to write into `tracker_entries` + `anime` cache.
#[derive(Debug, Clone)]
pub struct TrackerSnapshotRecord {
    pub anime: Anime,
    pub provider: TrackerProvider,
    /// Normalized status: `"CURRENT" | "PLANNING" | "COMPLETED" | "PAUSED" | "DROPPED"`.
    pub status: String,
    pub progress: u32,
    /// Normalized 0..100 scale score.
    pub score: Option<i64>,
    pub remote_updated_at: Option<i64>,
}

/// Result of resolving a pair of AniList / MAL tracker rows under a `MergePolicy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEntry {
    pub winner_provider: TrackerProvider,
    pub status: String,
    pub progress: u32,
    pub score: Option<i64>,
    pub best_updated_at: i64,
}

/// Cross-tracker disagreement surfaced in the `[2] Merge & Conflicts` view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeDiffItem {
    pub anime_id: i64,
    pub mal_id: Option<i64>,
    pub title: String,
    pub al_status: Option<String>,
    pub al_progress: Option<u32>,
    pub mal_status: Option<String>,
    pub mal_progress: Option<u32>,
    pub winner_provider: TrackerProvider,
    pub resolved_status: String,
    pub resolved_progress: u32,
    pub needs_push_to_al: bool,
    pub needs_push_to_mal: bool,
}

/// Summary of the most recent tracker reconciliation or SQLite audit.
#[derive(Debug, Clone, Default)]
pub struct MergeReport {
    pub anilist_count: usize,
    pub mal_count: usize,
    pub unified_unique_count: usize,
    pub overlaps_count: usize,
    pub conflict_diffs: Vec<MergeDiffItem>,
    pub unresolved_mal_titles: Vec<String>,
}

fn status_priority(status: &str) -> u8 {
    match status {
        "COMPLETED" => 4,
        "CURRENT" => 3,
        "PAUSED" => 2,
        "DROPPED" => 1,
        _ => 0,
    }
}

/// Deterministically choose canonical status, progress, and score from AniList + MAL rows.
pub fn resolve_pair(
    al: Option<&TrackerEntryRow>,
    mal: Option<&TrackerEntryRow>,
    policy: MergePolicy,
    fallback_now: i64,
) -> Option<ResolvedEntry> {
    match (al, mal) {
        (Some(a), None) => Some(ResolvedEntry {
            winner_provider: TrackerProvider::AniList,
            status: a.status.clone(),
            progress: a.progress.max(0) as u32,
            score: a.score.filter(|&s| s > 0),
            best_updated_at: a.remote_updated_at.unwrap_or(fallback_now),
        }),
        (None, Some(m)) => Some(ResolvedEntry {
            winner_provider: TrackerProvider::Mal,
            status: m.status.clone(),
            progress: m.progress.max(0) as u32,
            score: m.score.filter(|&s| s > 0),
            best_updated_at: m.remote_updated_at.unwrap_or(fallback_now),
        }),
        (None, None) => None,
        (Some(a), Some(m)) => {
            let a_prog = a.progress.max(0) as u32;
            let m_prog = m.progress.max(0) as u32;
            let a_ts = a.remote_updated_at.unwrap_or(0);
            let m_ts = m.remote_updated_at.unwrap_or(0);

            let pick_al = match policy {
                MergePolicy::AniListPrimary => true,
                MergePolicy::MalPrimary => false,
                MergePolicy::LatestUpdated => match a_ts.cmp(&m_ts) {
                    std::cmp::Ordering::Greater => true,
                    std::cmp::Ordering::Less => false,
                    std::cmp::Ordering::Equal => highest_progress_picks_al(a, m, a_prog, m_prog),
                },
                MergePolicy::HighestProgress => highest_progress_picks_al(a, m, a_prog, m_prog),
            };

            let (winner, winner_provider, loser) = if pick_al {
                (a, TrackerProvider::AniList, m)
            } else {
                (m, TrackerProvider::Mal, a)
            };

            let score = winner
                .score
                .filter(|&s| s > 0)
                .or_else(|| loser.score.filter(|&s| s > 0));
            let best_updated_at = a_ts.max(m_ts);
            let best_updated_at = if best_updated_at > 0 {
                best_updated_at
            } else {
                fallback_now
            };

            Some(ResolvedEntry {
                winner_provider,
                status: winner.status.clone(),
                progress: winner.progress.max(0) as u32,
                score,
                best_updated_at,
            })
        }
    }
}

fn highest_progress_picks_al(
    a: &TrackerEntryRow,
    m: &TrackerEntryRow,
    a_prog: u32,
    m_prog: u32,
) -> bool {
    match a_prog.cmp(&m_prog) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            match status_priority(&a.status).cmp(&status_priority(&m.status)) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Less => false,
                std::cmp::Ordering::Equal => {
                    a.remote_updated_at.unwrap_or(0) >= m.remote_updated_at.unwrap_or(0)
                }
            }
        }
    }
}

/// Convert detailed AniList list entries into normalized snapshot records.
pub fn anilist_records_to_snapshots(entries: &[AniListEntryRecord]) -> Vec<TrackerSnapshotRecord> {
    entries
        .iter()
        .map(|entry| TrackerSnapshotRecord {
            anime: entry.anime.clone(),
            provider: TrackerProvider::AniList,
            status: entry.status.clone(),
            progress: entry.progress,
            score: entry.score.filter(|&s| s > 0),
            remote_updated_at: entry.updated_at,
        })
        .collect()
}

fn mal_entry_to_synthetic_anime(entry: &MalListEntry, synth_id: i64, now: i64) -> Anime {
    Anime {
        id: synth_id,
        title_english: Some(entry.title.clone()),
        title_romaji: entry.title.clone(),
        title_native: None,
        description: None,
        episodes: entry.total_episodes,
        status: None,
        season: None,
        season_year: None,
        score: entry.score.filter(|&s| s > 0).map(|s| if s <= 10 { s * 10 } else { s }),
        format: Some("TV".to_string()),
        genres: "[]".to_string(),
        synonyms: "[]".to_string(),
        cover_url: entry.cover_url.clone(),
        cover_blob: None,
        has_dub: 0,
        updated_at: now,
        next_airing_episode: None,
        mal_id: Some(entry.mal_id),
    }
}

/// Resolve a list of MyAnimeList entries to canonical AniList `Anime` records using:
/// 1) SQLite crosswalk / `anime.mal_id` cache (0 HTTP calls),
/// 2) Batched AniList GraphQL `idMal_in` lookup (50 IDs per request),
/// 3) Title-search fallback for rare unlinked entries,
/// 4) Deterministic negative synthetic ID (`-mal_id`) as a non-colliding last resort.
pub async fn resolve_mal_entries_to_canonical(
    pool: &SqlitePool,
    anilist: &AniListClient,
    entries: &[MalListEntry],
    now: i64,
) -> Result<(Vec<TrackerSnapshotRecord>, Vec<String>)> {
    if entries.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let mal_ids: Vec<i64> = entries.iter().map(|e| e.mal_id).collect();
    let cached_map = lookup_anilist_by_mal_ids(pool, &mal_ids).await?;

    let mut resolved_anime_by_mal: HashMap<i64, Anime> = HashMap::new();
    let mut missing_mal_ids: Vec<i64> = Vec::new();

    for entry in entries {
        if let Some(&al_id) = cached_map.get(&entry.mal_id) {
            if let Some(mut anime) = get_anime(pool, al_id).await? {
                if anime.mal_id.is_none() {
                    anime.mal_id = Some(entry.mal_id);
                }
                resolved_anime_by_mal.insert(entry.mal_id, anime);
                continue;
            }
        }
        missing_mal_ids.push(entry.mal_id);
    }

    missing_mal_ids.sort_unstable();
    missing_mal_ids.dedup();

    let mut links_to_save: Vec<(i64, i64, &str)> = Vec::new();

    if !missing_mal_ids.is_empty() {
        let fetched = anilist.resolve_by_mal_ids(&missing_mal_ids, now).await?;
        for anime in fetched {
            if let Some(mid) = anime.mal_id {
                links_to_save.push((mid, anime.id, "anilist_idmal"));
                resolved_anime_by_mal.insert(mid, anime);
            }
        }
    }

    let mut fallback_links: Vec<(i64, i64, &str)> = Vec::new();
    let mut unresolved_titles: Vec<String> = Vec::new();
    let mut fallback_budget = 25usize;

    for entry in entries {
        if resolved_anime_by_mal.contains_key(&entry.mal_id) {
            continue;
        }
        let mut matched: Option<Anime> = None;
        if fallback_budget > 0 && !entry.title.trim().is_empty() {
            fallback_budget -= 1;
            if let Ok(Some(mut candidate)) = anilist.search_candidate_for_mal(&entry.title, now).await {
                if candidate.mal_id.is_none() || candidate.mal_id == Some(entry.mal_id) {
                    candidate.mal_id = Some(entry.mal_id);
                    fallback_links.push((entry.mal_id, candidate.id, "title_fallback"));
                    matched = Some(candidate);
                }
            }
        }

        if let Some(anime) = matched {
            resolved_anime_by_mal.insert(entry.mal_id, anime);
        } else {
            let synth_id = -entry.mal_id.abs().max(1);
            unresolved_titles.push(entry.title.clone());
            resolved_anime_by_mal.insert(entry.mal_id, mal_entry_to_synthetic_anime(entry, synth_id, now));
        }
    }

    if !links_to_save.is_empty() {
        save_crosswalk_links(pool, &links_to_save, now).await?;
    }
    if !fallback_links.is_empty() {
        save_crosswalk_links(pool, &fallback_links, now).await?;
    }

    let mut snapshots = Vec::with_capacity(entries.len());
    let mut seen_ids: HashSet<(i64, &'static str)> = HashSet::new();

    for entry in entries {
        if let Some(mut anime) = resolved_anime_by_mal.get(&entry.mal_id).cloned() {
            if anime.cover_url.is_none() {
                anime.cover_url = entry.cover_url.clone();
            }
            if anime.episodes.is_none() {
                anime.episodes = entry.total_episodes;
            }
            if anime.mal_id.is_none() {
                anime.mal_id = Some(entry.mal_id);
            }
            // Prevent duplicate snapshot rows inside a single MAL export payload.
            if !seen_ids.insert((anime.id, TrackerProvider::Mal.as_str())) {
                continue;
            }
            snapshots.push(TrackerSnapshotRecord {
                anime,
                provider: TrackerProvider::Mal,
                status: normalize_mal_status(&entry.status).to_string(),
                progress: entry.progress,
                score: entry.score.filter(|&s| s > 0),
                remote_updated_at: entry.updated_at,
            });
        }
    }

    Ok((snapshots, unresolved_titles))
}

/// Persist tracker snapshots for `replaced_providers`, then deterministically
/// reconcile ALL stored `tracker_entries` into `history`, `continue_watching`,
/// and `watchlist` without creating duplicates.
pub async fn merge_and_reconcile(
    pool: &SqlitePool,
    replaced_providers: &[TrackerProvider],
    records: &[TrackerSnapshotRecord],
    policy: MergePolicy,
    unresolved_mal_titles: Vec<String>,
    now: i64,
) -> Result<MergeReport> {
    let pid = crate::db::user::get_active_profile_id(pool).await;
    let dismissed_cw = get_dismissed_ids(pool, "continue_watching").await?;
    let dismissed_wl = get_dismissed_ids(pool, "watchlist").await?;

    let mut tx = pool.begin().await?;

    // Track shows that belonged to replaced providers so if they
    // were removed on the website itself, they are cleanly evicted locally too.
    let mut prev_replaced_tracked: HashSet<i64> = HashSet::new();
    for provider in replaced_providers {
        let prev_ids: Vec<i64> = sqlx::query_scalar(
            "SELECT anime_id FROM tracker_entries WHERE profile_id = ? AND provider = ?",
        )
        .bind(&pid)
        .bind(provider.as_str())
        .fetch_all(&mut *tx)
        .await?;
        prev_replaced_tracked.extend(prev_ids);

        sqlx::query("DELETE FROM tracker_entries WHERE profile_id = ? AND provider = ?")
            .bind(&pid)
            .bind(provider.as_str())
            .execute(&mut *tx)
            .await?;
    }

    for rec in records {
        execute_upsert_anime(&mut *tx, &rec.anime).await?;
        sqlx::query(
            "INSERT INTO tracker_entries
                 (profile_id, anime_id, provider, status, progress, score, remote_updated_at, last_synced_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(profile_id, anime_id, provider) DO UPDATE SET
                 status            = excluded.status,
                 progress          = excluded.progress,
                 score             = COALESCE(excluded.score, tracker_entries.score),
                 remote_updated_at = COALESCE(excluded.remote_updated_at, tracker_entries.remote_updated_at),
                 last_synced_at    = excluded.last_synced_at",
        )
        .bind(&pid)
        .bind(rec.anime.id)
        .bind(rec.provider.as_str())
        .bind(&rec.status)
        .bind(rec.progress as i64)
        .bind(rec.score)
        .bind(rec.remote_updated_at)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    let all_rows = sqlx::query_as::<_, TrackerEntryRow>(
        "SELECT anime_id, provider, status, progress, score, remote_updated_at, last_synced_at
         FROM tracker_entries
         WHERE profile_id = ?
         ORDER BY anime_id ASC, provider ASC",
    )
    .bind(&pid)
    .fetch_all(&mut *tx)
    .await?;

    let title_rows = sqlx::query(
        "SELECT id, mal_id, COALESCE(NULLIF(TRIM(title_english), ''), title_romaji) AS display_title
         FROM anime
         WHERE id IN (SELECT DISTINCT anime_id FROM tracker_entries WHERE profile_id = ?)",
    )
    .bind(&pid)
    .fetch_all(&mut *tx)
    .await?;

    let mut meta_map: HashMap<i64, (Option<i64>, String)> = HashMap::new();
    for r in title_rows {
        let id: i64 = r.try_get("id").unwrap_or_default();
        let mal_id: Option<i64> = r.try_get("mal_id").ok().flatten();
        let title: String = r.try_get("display_title").unwrap_or_default();
        meta_map.insert(id, (mal_id, title));
    }

    let mut grouped: BTreeMap<i64, (Option<TrackerEntryRow>, Option<TrackerEntryRow>)> =
        BTreeMap::new();
    let mut anilist_count = 0usize;
    let mut mal_count = 0usize;

    for row in all_rows {
        let entry = grouped.entry(row.anime_id).or_default();
        if row.provider == TrackerProvider::AniList.as_str() {
            anilist_count += 1;
            entry.0 = Some(row);
        } else if row.provider == TrackerProvider::Mal.as_str() {
            mal_count += 1;
            entry.1 = Some(row);
        }
    }

    let mut overlaps_count = 0usize;
    let mut conflict_diffs = Vec::new();
    let mut tracked_ids = HashSet::with_capacity(grouped.len());

    for (&anime_id, (al_opt, mal_opt)) in &grouped {
        tracked_ids.insert(anime_id);
        let Some(resolved) = resolve_pair(al_opt.as_ref(), mal_opt.as_ref(), policy, now) else {
            continue;
        };

        if let (Some(al), Some(mal)) = (al_opt, mal_opt) {
            overlaps_count += 1;
            let al_p = al.progress.max(0) as u32;
            let mal_p = mal.progress.max(0) as u32;
            let needs_push_to_al = al.status != resolved.status || al_p != resolved.progress;
            let needs_push_to_mal = mal.status != resolved.status || mal_p != resolved.progress;
            if needs_push_to_al || needs_push_to_mal {
                let (mal_id, title) = meta_map
                    .get(&anime_id)
                    .cloned()
                    .unwrap_or((None, format!("Anime #{anime_id}")));
                conflict_diffs.push(MergeDiffItem {
                    anime_id,
                    mal_id,
                    title,
                    al_status: Some(al.status.clone()),
                    al_progress: Some(al_p),
                    mal_status: Some(mal.status.clone()),
                    mal_progress: Some(mal_p),
                    winner_provider: resolved.winner_provider,
                    resolved_status: resolved.status.clone(),
                    resolved_progress: resolved.progress,
                    needs_push_to_al,
                    needs_push_to_mal,
                });
            }
        }

        if !dismissed_wl.contains(&anime_id) {
            sqlx::query(
                "INSERT INTO watchlist (profile_id, anime_id, added_at, status)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(profile_id, anime_id) DO UPDATE SET
                     status   = excluded.status,
                     added_at = MAX(watchlist.added_at, excluded.added_at)",
            )
            .bind(&pid)
            .bind(anime_id)
            .bind(resolved.best_updated_at.max(now))
            .bind(&resolved.status)
            .execute(&mut *tx)
            .await?;
        }

        match resolved.status.as_str() {
            "CURRENT" => {
                if resolved.progress > 0 {
                    sqlx::query(
                        "INSERT INTO history (profile_id, anime_id, episode, watched_at)
                         VALUES (?, ?, ?, ?)
                         ON CONFLICT(profile_id, anime_id, episode) DO UPDATE SET watched_at = excluded.watched_at",
                    )
                    .bind(&pid)
                    .bind(anime_id)
                    .bind(resolved.progress as i64)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
                }

                if !dismissed_cw.contains(&anime_id) {
                    sqlx::query(
                        "INSERT INTO continue_watching (profile_id, anime_id, last_episode, last_watched)
                         VALUES (?, ?, ?, ?)
                         ON CONFLICT(profile_id, anime_id) DO UPDATE SET
                             last_episode = excluded.last_episode,
                             last_watched = excluded.last_watched",
                    )
                    .bind(&pid)
                    .bind(anime_id)
                    .bind(resolved.progress as i64)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;

                    let _ = sqlx::query(
                        "DELETE FROM dismissed_sync WHERE profile_id = ? AND anime_id = ? AND list_type = 'continue_watching'",
                    )
                    .bind(&pid)
                    .bind(anime_id)
                    .execute(&mut *tx)
                    .await;
                }
            }
            "COMPLETED" | "DROPPED" => {
                sqlx::query("DELETE FROM continue_watching WHERE profile_id = ? AND anime_id = ?")
                    .bind(&pid)
                    .bind(anime_id)
                    .execute(&mut *tx)
                    .await?;
            }
            _ => {}
        }
    }

    // Evict shows that vanished completely from a synced provider's website
    // unless another connected provider still holds them or they are purely Local.
    for old_wl_id in prev_replaced_tracked {
        if !tracked_ids.contains(&old_wl_id) {
            sqlx::query("DELETE FROM watchlist WHERE profile_id = ? AND anime_id = ?")
                .bind(&pid)
                .bind(old_wl_id)
                .execute(&mut *tx)
                .await?;
        }
    }

    tx.commit().await?;

    Ok(MergeReport {
        anilist_count,
        mal_count,
        unified_unique_count: grouped.len(),
        overlaps_count,
        conflict_diffs,
        unresolved_mal_titles,
    })
}

/// Compute the live `MergeReport` purely from stored SQLite `tracker_entries`.
pub async fn build_merge_audit(pool: &SqlitePool, policy: MergePolicy) -> Result<MergeReport> {
    let pid = crate::db::user::get_active_profile_id(pool).await;
    let all_rows = get_all_tracker_entries(pool).await?;
    let title_rows = sqlx::query(
        "SELECT id, mal_id, COALESCE(NULLIF(TRIM(title_english), ''), title_romaji) AS display_title
         FROM anime
         WHERE id IN (SELECT DISTINCT anime_id FROM tracker_entries WHERE profile_id = ?)",
    )
    .bind(&pid)
    .fetch_all(pool)
    .await?;

    let mut meta_map: HashMap<i64, (Option<i64>, String)> = HashMap::new();
    for r in title_rows {
        let id: i64 = r.try_get("id").unwrap_or_default();
        let mal_id: Option<i64> = r.try_get("mal_id").ok().flatten();
        let title: String = r.try_get("display_title").unwrap_or_default();
        meta_map.insert(id, (mal_id, title));
    }

    let mut grouped: BTreeMap<i64, (Option<TrackerEntryRow>, Option<TrackerEntryRow>)> =
        BTreeMap::new();
    let mut anilist_count = 0usize;
    let mut mal_count = 0usize;

    for row in all_rows {
        let slot = grouped.entry(row.anime_id).or_default();
        if row.provider == TrackerProvider::AniList.as_str() {
            anilist_count += 1;
            slot.0 = Some(row);
        } else if row.provider == TrackerProvider::Mal.as_str() {
            mal_count += 1;
            slot.1 = Some(row);
        }
    }

    let mut overlaps_count = 0usize;
    let mut conflict_diffs = Vec::new();
    for (&anime_id, (al_opt, mal_opt)) in &grouped {
        let Some(resolved) = resolve_pair(al_opt.as_ref(), mal_opt.as_ref(), policy, 0) else {
            continue;
        };
        if let (Some(al), Some(mal)) = (al_opt, mal_opt) {
            overlaps_count += 1;
            let al_p = al.progress.max(0) as u32;
            let mal_p = mal.progress.max(0) as u32;
            let needs_push_to_al = al.status != resolved.status || al_p != resolved.progress;
            let needs_push_to_mal = mal.status != resolved.status || mal_p != resolved.progress;
            if needs_push_to_al || needs_push_to_mal {
                let (mal_id, title) = meta_map
                    .get(&anime_id)
                    .cloned()
                    .unwrap_or((None, format!("Anime #{anime_id}")));
                conflict_diffs.push(MergeDiffItem {
                    anime_id,
                    mal_id,
                    title,
                    al_status: Some(al.status.clone()),
                    al_progress: Some(al_p),
                    mal_status: Some(mal.status.clone()),
                    mal_progress: Some(mal_p),
                    winner_provider: resolved.winner_provider,
                    resolved_status: resolved.status,
                    resolved_progress: resolved.progress,
                    needs_push_to_al,
                    needs_push_to_mal,
                });
            }
        }
    }

    Ok(MergeReport {
        anilist_count,
        mal_count,
        unified_unique_count: grouped.len(),
        overlaps_count,
        conflict_diffs,
        unresolved_mal_titles: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{cache::upsert_anime, init, user::get_continue_watching};
    use crate::db::user::get_watchlist;

    fn sample_anime(id: i64, mal_id: Option<i64>, title: &str) -> Anime {
        Anime {
            id,
            title_english: Some(title.into()),
            title_romaji: title.into(),
            title_native: None,
            description: None,
            episodes: Some(24),
            status: Some("FINISHED".into()),
            season: Some("FALL".into()),
            season_year: Some(2023),
            score: Some(88),
            format: Some("TV".into()),
            genres: "[\"Action\"]".into(),
            synonyms: "[]".into(),
            cover_url: None,
            cover_blob: None,
            has_dub: 0,
            updated_at: 1_000,
            next_airing_episode: None,
            mal_id,
        }
    }

    fn snap(
        anime: Anime,
        provider: TrackerProvider,
        status: &str,
        progress: u32,
        updated_at: Option<i64>,
    ) -> TrackerSnapshotRecord {
        TrackerSnapshotRecord {
            anime,
            provider,
            status: status.into(),
            progress,
            score: Some(80),
            remote_updated_at: updated_at,
        }
    }

    #[tokio::test]
    async fn test_merge_produces_zero_duplicates_across_al_then_mal_and_mal_then_al() {
        let pool_a = init(":memory:").await.unwrap();
        let pool_b = init(":memory:").await.unwrap();
        let frieren = sample_anime(154587, Some(52991), "Frieren");
        let steins = sample_anime(9253, Some(9253), "Steins;Gate");
        let monster = sample_anime(19, Some(19), "Monster");

        // Order A: AniList then MAL
        merge_and_reconcile(
            &pool_a,
            &[TrackerProvider::AniList],
            &[
                snap(frieren.clone(), TrackerProvider::AniList, "CURRENT", 4, Some(100)),
                snap(steins.clone(), TrackerProvider::AniList, "PLANNING", 0, Some(90)),
            ],
            MergePolicy::HighestProgress,
            vec![],
            1_000,
        )
        .await
        .unwrap();

        let report_a = merge_and_reconcile(
            &pool_a,
            &[TrackerProvider::Mal],
            &[
                snap(frieren.clone(), TrackerProvider::Mal, "CURRENT", 9, Some(110)),
                snap(monster.clone(), TrackerProvider::Mal, "PLANNING", 0, Some(95)),
            ],
            MergePolicy::HighestProgress,
            vec![],
            1_005,
        )
        .await
        .unwrap();

        // Order B: MAL then AniList
        merge_and_reconcile(
            &pool_b,
            &[TrackerProvider::Mal],
            &[
                snap(frieren.clone(), TrackerProvider::Mal, "CURRENT", 9, Some(110)),
                snap(monster.clone(), TrackerProvider::Mal, "PLANNING", 0, Some(95)),
            ],
            MergePolicy::HighestProgress,
            vec![],
            1_000,
        )
        .await
        .unwrap();

        let report_b = merge_and_reconcile(
            &pool_b,
            &[TrackerProvider::AniList],
            &[
                snap(frieren.clone(), TrackerProvider::AniList, "CURRENT", 4, Some(100)),
                snap(steins.clone(), TrackerProvider::AniList, "PLANNING", 0, Some(90)),
            ],
            MergePolicy::HighestProgress,
            vec![],
            1_005,
        )
        .await
        .unwrap();

        assert_eq!(report_a.unified_unique_count, 3);
        assert_eq!(report_b.unified_unique_count, 3);
        assert_eq!(report_a.overlaps_count, 1);
        assert_eq!(report_a.conflict_diffs.len(), 1);
        assert_eq!(report_a.conflict_diffs[0].resolved_progress, 9);
        assert!(report_a.conflict_diffs[0].needs_push_to_al);
        assert!(!report_a.conflict_diffs[0].needs_push_to_mal);

        let cw_a = get_continue_watching(&pool_a).await.unwrap();
        let cw_b = get_continue_watching(&pool_b).await.unwrap();
        assert_eq!(cw_a.len(), 1);
        assert_eq!(cw_b.len(), 1);
        assert_eq!(cw_a[0].last_episode, 9);
        assert_eq!(cw_b[0].last_episode, 9);

        let mut wl_a = get_watchlist(&pool_a).await.unwrap();
        let mut wl_b = get_watchlist(&pool_b).await.unwrap();
        wl_a.sort();
        wl_b.sort();
        assert_eq!(wl_a, vec![19, 9253, 154587]);
        assert_eq!(wl_b, vec![19, 9253, 154587]);
        let statuses_a = crate::db::user::get_watchlist_statuses(&pool_a).await.unwrap();
        assert_eq!(statuses_a.get(&154587), Some(&crate::config::WatchStatus::Watching));
        assert_eq!(statuses_a.get(&19), Some(&crate::config::WatchStatus::Planning));
        assert_eq!(statuses_a.get(&9253), Some(&crate::config::WatchStatus::Planning));
    }

    #[tokio::test]
    async fn test_resolve_pair_policies() {
        let al = TrackerEntryRow {
            anime_id: 1,
            provider: "anilist".into(),
            status: "CURRENT".into(),
            progress: 4,
            score: Some(80),
            remote_updated_at: Some(200),
            last_synced_at: 500,
        };
        let mal = TrackerEntryRow {
            anime_id: 1,
            provider: "mal".into(),
            status: "CURRENT".into(),
            progress: 7,
            score: Some(90),
            remote_updated_at: Some(150),
            last_synced_at: 500,
        };

        let hp = resolve_pair(Some(&al), Some(&mal), MergePolicy::HighestProgress, 999).unwrap();
        assert_eq!(hp.winner_provider, TrackerProvider::Mal);
        assert_eq!(hp.progress, 7);

        let lu = resolve_pair(Some(&al), Some(&mal), MergePolicy::LatestUpdated, 999).unwrap();
        assert_eq!(lu.winner_provider, TrackerProvider::AniList);
        assert_eq!(lu.progress, 4);

        let ap = resolve_pair(Some(&al), Some(&mal), MergePolicy::AniListPrimary, 999).unwrap();
        assert_eq!(ap.winner_provider, TrackerProvider::AniList);
        assert_eq!(ap.progress, 4);

        let mp = resolve_pair(Some(&al), Some(&mal), MergePolicy::MalPrimary, 999).unwrap();
        assert_eq!(mp.winner_provider, TrackerProvider::Mal);
        assert_eq!(mp.progress, 7);
    }

    #[tokio::test]
    async fn test_mal_crosswalk_uses_sqlite_cache_with_zero_network() {
        let pool = init(":memory:").await.unwrap();
        let anime = sample_anime(154587, Some(52991), "Frieren");
        upsert_anime(&pool, &anime).await.unwrap();
        save_crosswalk_links(&pool, &[(52991, 154587, "anilist_idmal")], 1_000)
            .await
            .unwrap();

        let client = AniListClient::new();
        let mal_entries = vec![MalListEntry {
            mal_id: 52991,
            title: "Sousou no Frieren".into(),
            title_english: None,
            status: "watching".into(),
            progress: 11,
            total_episodes: Some(28),
            score: Some(90),
            format: Some("TV".into()),
            season: None,
            season_year: None,
            genres: vec![],
            updated_at: Some(1_700_000),
            cover_url: None,
        }];

        let (snaps, unresolved) = resolve_mal_entries_to_canonical(&pool, &client, &mal_entries, 2_000)
            .await
            .unwrap();
        assert!(unresolved.is_empty());
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].anime.id, 154587);
        assert_eq!(snaps[0].status, "CURRENT");
        assert_eq!(snaps[0].progress, 11);
        assert_eq!(snaps[0].score, Some(90));
    }
}
