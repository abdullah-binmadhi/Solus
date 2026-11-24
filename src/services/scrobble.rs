//! Unified dual-tracker writeback dispatcher for AniList + MyAnimeList.
//!
//! Coordinates concurrent progress scrobbling, interactive live Watchlist
//! additions/deletions, cross-tracker parity pushes, and automatic MAL
//! OAuth2 refresh token renewal.

use crate::api::anilist::AniListClient;
use crate::api::mal::MalClient;
use crate::config::{AuthConfig, TrackerProvider, TrackersConfig, WatchStatus, WatchlistTarget};
use crate::db::cache::{get_mal_id_for_anime, save_crosswalk_links, upsert_anime, Anime};
use crate::db::user::{
    add_to_watchlist_with_status, delete_tracker_entry, get_active_profile_id,
    get_anime_watchlist_source, remove_from_watchlist, upsert_tracker_entry, WatchlistSource,
};
use crate::error::Result;
use crate::services::tracker_merge::MergeDiffItem;
use sqlx::SqlitePool;

/// Human-readable outcome from a live dual-tracker write operation.
#[derive(Debug, Clone, Default)]
pub struct WritebackOutcome {
    pub summary: String,
    pub anilist_synced: bool,
    pub mal_synced: bool,
    pub warnings: Vec<String>,
}

impl WritebackOutcome {
    fn finalize(mut self, action_label: &str) -> Self {
        let mut targets = Vec::new();
        if self.anilist_synced {
            targets.push("AniList ✓");
        }
        if self.mal_synced {
            targets.push("MAL ✓");
        }
        self.summary = if !targets.is_empty() {
            if self.warnings.is_empty() {
                format!("{action_label} ({})", targets.join(" + "))
            } else {
                format!("{action_label} ({} | ⚠ {})", targets.join(" + "), self.warnings.join("; "))
            }
        } else if !self.warnings.is_empty() {
            format!("{action_label} — {}", self.warnings.join("; "))
        } else {
            action_label.to_string()
        };
        self
    }
}

/// Automatically refresh the MyAnimeList OAuth2 token when nearing expiry.
pub async fn maybe_refresh_mal_token(
    auth: &mut AuthConfig,
    mal_client_id: Option<&str>,
    now: i64,
) -> bool {
    let Some(client_id) = mal_client_id.filter(|s| !s.trim().is_empty()) else {
        return false;
    };
    let (Some(_token), Some(refresh_tok), expires_at) = (
        auth.mal.token.as_ref(),
        auth.mal.refresh_token.clone(),
        auth.mal.expires_at,
    ) else {
        return false;
    };

    let needs_refresh = expires_at.map(|exp| exp <= now + 120).unwrap_or(false);
    if !needs_refresh {
        return false;
    }

    let client = MalClient::new(Some(client_id.to_string()), None);
    match client.refresh_access_token(client_id, &refresh_tok).await {
        Ok(refreshed) => {
            auth.mal.token = Some(refreshed.access_token);
            if let Some(new_rt) = refreshed.refresh_token {
                auth.mal.refresh_token = Some(new_rt);
            }
            auth.mal.expires_at = refreshed.expires_in.map(|secs| now + secs.max(0));
            let _ = auth.save();
            true
        }
        Err(_) => false,
    }
}

async fn resolve_canonical_mal_id(
    pool: &SqlitePool,
    anilist: &AniListClient,
    anime: &Anime,
    now: i64,
) -> Option<i64> {
    if let Some(mid) = anime.mal_id.filter(|&id| id > 0) {
        return Some(mid);
    }
    if anime.id < 0 {
        return Some((-anime.id).max(1));
    }
    if let Ok(Some(mid)) = get_mal_id_for_anime(pool, anime.id).await {
        return Some(mid);
    }
    if anime.id > 0 {
        if let Some(mid) = anilist.lookup_mal_id(anime.id).await {
            let _ = save_crosswalk_links(pool, &[(mid, anime.id, "anilist_idmal")], now).await;
            return Some(mid);
        }
    }
    None
}

/// Concurrently scrobble watched episode progress to connected AniList and MyAnimeList accounts.
pub async fn sync_episode_watched_all(
    pool: &SqlitePool,
    anime: &Anime,
    episode: u32,
    auth: &mut AuthConfig,
    trackers: &TrackersConfig,
    now: i64,
) -> WritebackOutcome {
    let mal_cid = trackers.resolved_mal_client_id();
    let _ = maybe_refresh_mal_token(auth, mal_cid.as_deref(), now).await;

    let normalized_status = match anime.episodes {
        Some(total) if total > 0 && episode as i64 >= total => "COMPLETED",
        _ => "CURRENT",
    };

    let al_tok = auth.anilist_token().filter(|_| anime.id > 0);
    let do_mal = auth.mal.has_write_token()
        && (auth.auto_scrobble_both || !auth.anilist.has_write_token());
    let mal_tok = if do_mal { auth.mal_token() } else { None };

    let mut outcome = WritebackOutcome::default();

    let public_al = AniListClient::new();
    let mal_id = if mal_tok.is_some() {
        resolve_canonical_mal_id(pool, &public_al, anime, now).await
    } else {
        None
    };

    let al_fut = async {
        if let Some(tok) = al_tok {
            let client = AniListClient::with_token(Some(tok));
            Some(client.save_progress(anime.id, episode).await)
        } else {
            None
        }
    };

    let mal_fut = async {
        if let Some(tok) = mal_tok {
            if let Some(mid) = mal_id {
                let client = MalClient::with_token(mal_cid.clone(), Some(tok));
                Some(client.update_list_status(mid, normalized_status, Some(episode), None).await)
            } else {
                Some(Err(crate::error::AppError::Parse(
                    "No MAL ID mapping found for this show".into(),
                )))
            }
        } else {
            None
        }
    };

    let (al_res, mal_res) = tokio::join!(al_fut, mal_fut);

    if let Some(res) = al_res {
        match res {
            Ok(()) => {
                outcome.anilist_synced = true;
                let _ = upsert_tracker_entry(
                    pool,
                    anime.id,
                    TrackerProvider::AniList,
                    normalized_status,
                    episode,
                    None,
                    Some(now),
                    now,
                )
                .await;
            }
            Err(e) => outcome.warnings.push(format!("AniList: {e}")),
        }
    }

    if let Some(res) = mal_res {
        match res {
            Ok(()) => {
                outcome.mal_synced = true;
                let _ = upsert_tracker_entry(
                    pool,
                    anime.id,
                    TrackerProvider::Mal,
                    normalized_status,
                    episode,
                    None,
                    Some(now),
                    now,
                )
                .await;
            }
            Err(e) => outcome.warnings.push(format!("MAL: {e}")),
        }
    }

    outcome.finalize(&format!("Synced EP {episode}"))
}

/// Add an anime to Solus Watchlist AND push `PLANNING` live to the chosen tracker DB(s).
#[allow(dead_code)]
pub async fn add_watchlist_to_target(
    pool: &SqlitePool,
    anime: &Anime,
    target: WatchlistTarget,
    auth: &mut AuthConfig,
    trackers: &TrackersConfig,
    now: i64,
) -> Result<WritebackOutcome> {
    set_watchlist_status_on_target(pool, anime, WatchStatus::Planning, target, auth, trackers, now)
        .await
}

/// Set an anime's canonical `WatchStatus` in Solus SQLite AND push live to chosen tracker DB(s).
pub async fn set_watchlist_status_on_target(
    pool: &SqlitePool,
    anime: &Anime,
    status: WatchStatus,
    target: WatchlistTarget,
    auth: &mut AuthConfig,
    trackers: &TrackersConfig,
    now: i64,
) -> Result<WritebackOutcome> {
    upsert_anime(pool, anime).await?;
    add_to_watchlist_with_status(pool, anime.id, status, now).await?;

    let pid = get_active_profile_id(pool).await;
    match status {
        WatchStatus::Watching => {
            let _ = sqlx::query(
                "INSERT OR IGNORE INTO continue_watching (profile_id, anime_id, last_episode, last_watched)
                 VALUES (?, ?, 0, ?)",
            )
            .bind(&pid)
            .bind(anime.id)
            .bind(now)
            .execute(pool)
            .await;
            let _ = sqlx::query(
                "DELETE FROM dismissed_sync WHERE profile_id = ? AND anime_id = ? AND list_type = 'continue_watching'",
            )
            .bind(&pid)
            .bind(anime.id)
            .execute(pool)
            .await;
        }
        WatchStatus::Completed | WatchStatus::Dropped => {
            let _ = sqlx::query(
                "DELETE FROM continue_watching WHERE profile_id = ? AND anime_id = ?",
            )
            .bind(&pid)
            .bind(anime.id)
            .execute(pool)
            .await;
        }
        _ => {}
    }

    let mal_cid = trackers.resolved_mal_client_id();
    if matches!(target, WatchlistTarget::Both | WatchlistTarget::Mal) {
        let _ = maybe_refresh_mal_token(auth, mal_cid.as_deref(), now).await;
    }

    let mut outcome = WritebackOutcome::default();
    let want_al = matches!(target, WatchlistTarget::Both | WatchlistTarget::AniList);
    let want_mal = matches!(target, WatchlistTarget::Both | WatchlistTarget::Mal);

    let al_tok = if want_al && anime.id > 0 {
        auth.anilist_token()
    } else {
        None
    };
    if want_al && al_tok.is_none() {
        outcome.warnings.push(if auth.anilist.username.is_some() {
            "AniList read-only (connect OAuth in [L] to write)".into()
        } else {
            "AniList not connected".into()
        });
    }

    let mal_tok = if want_mal { auth.mal_token() } else { None };
    if want_mal && mal_tok.is_none() {
        outcome.warnings.push(if auth.mal.username.is_some() {
            "MAL read-only (press [p] Password Login in [L] to write)".into()
        } else {
            "MAL not connected".into()
        });
    }

    let public_al = AniListClient::new();
    let mal_id = if mal_tok.is_some() {
        resolve_canonical_mal_id(pool, &public_al, anime, now).await
    } else {
        None
    };

    let status_str = status.to_tracker_status();
    let completed_eps = if status == WatchStatus::Completed {
        anime.episodes.filter(|&e| e > 0).map(|e| e as u32)
    } else {
        None
    };

    let al_fut = async {
        if let Some(tok) = al_tok {
            let client = AniListClient::with_token(Some(tok));
            Some(client.save_list_entry(anime.id, status_str, completed_eps).await)
        } else {
            None
        }
    };

    let mal_fut = async {
        if let Some(tok) = mal_tok {
            if let Some(mid) = mal_id {
                let client = MalClient::with_token(mal_cid.clone(), Some(tok));
                Some(client.update_list_status(mid, status_str, completed_eps, None).await)
            } else {
                Some(Err(crate::error::AppError::Parse(
                    "Unable to resolve MAL ID for live update".into(),
                )))
            }
        } else {
            None
        }
    };

    let (al_res, mal_res) = tokio::join!(al_fut, mal_fut);
    let stored_prog = completed_eps.unwrap_or(0);

    if let Some(res) = al_res {
        match res {
            Ok(()) => {
                outcome.anilist_synced = true;
                upsert_tracker_entry(
                    pool,
                    anime.id,
                    TrackerProvider::AniList,
                    status_str,
                    stored_prog,
                    None,
                    Some(now),
                    now,
                )
                .await?;
            }
            Err(e) => outcome.warnings.push(format!("AniList: {e}")),
        }
    }

    if let Some(res) = mal_res {
        match res {
            Ok(()) => {
                outcome.mal_synced = true;
                upsert_tracker_entry(
                    pool,
                    anime.id,
                    TrackerProvider::Mal,
                    status_str,
                    stored_prog,
                    None,
                    Some(now),
                    now,
                )
                .await?;
            }
            Err(e) => outcome.warnings.push(format!("MAL: {e}")),
        }
    }

    let action_label = match target {
        WatchlistTarget::Local => format!("Labeled \"{}\" · {} (Local)", anime.display_title(), status.full_label()),
        _ => format!("Labeled \"{}\" · {}", anime.display_title(), status.full_label()),
    };
    Ok(outcome.finalize(&action_label))
}

/// Remove an anime from the selected live tracker database(s) AND update Solus SQLite cleanly.
///
/// If an item lived in BOTH AniList + MAL and the user removes only one side, Solus deletes
/// that remote entry + snapshot row and preserves the show cleanly under the remaining divider.
pub async fn remove_watchlist_from_target(
    pool: &SqlitePool,
    anime: &Anime,
    target: WatchlistTarget,
    auth: &mut AuthConfig,
    trackers: &TrackersConfig,
    now: i64,
) -> Result<WritebackOutcome> {
    if target == WatchlistTarget::Local {
        remove_from_watchlist(pool, anime.id).await?;
        return Ok(WritebackOutcome::default().finalize("Hidden from Solus Watchlist"));
    }

    let mal_cid = trackers.resolved_mal_client_id();
    if matches!(target, WatchlistTarget::Both | WatchlistTarget::Mal) {
        let _ = maybe_refresh_mal_token(auth, mal_cid.as_deref(), now).await;
    }

    let mut outcome = WritebackOutcome::default();
    let rm_al = matches!(target, WatchlistTarget::Both | WatchlistTarget::AniList);
    let rm_mal = matches!(target, WatchlistTarget::Both | WatchlistTarget::Mal);

    let al_tok = if rm_al && anime.id > 0 {
        auth.anilist_token()
    } else {
        None
    };
    let mal_tok = if rm_mal { auth.mal_token() } else { None };
    let al_uid = auth.anilist.user_id;

    let public_al = AniListClient::new();
    let mal_id = if mal_tok.is_some() {
        resolve_canonical_mal_id(pool, &public_al, anime, now).await
    } else {
        None
    };

    let al_fut = async {
        if let Some(tok) = al_tok {
            let client = AniListClient::with_token(Some(tok));
            Some(client.delete_media_list_entry(al_uid, anime.id).await)
        } else {
            None
        }
    };

    let mal_fut = async {
        if let Some(tok) = mal_tok {
            if let Some(mid) = mal_id {
                let client = MalClient::with_token(mal_cid.clone(), Some(tok));
                Some(client.delete_list_entry(mid).await)
            } else {
                Some(Err(crate::error::AppError::Parse(
                    "Unable to resolve MAL ID for remote delete".into(),
                )))
            }
        } else {
            None
        }
    };

    let (al_res, mal_res) = tokio::join!(al_fut, mal_fut);

    if let Some(res) = al_res {
        match res {
            Ok(()) => {
                outcome.anilist_synced = true;
                delete_tracker_entry(pool, anime.id, TrackerProvider::AniList).await?;
            }
            Err(e) => outcome.warnings.push(format!("AniList: {e}")),
        }
    } else if rm_al {
        // If user was in read-only/public mode or cached, still remove local snapshot link cleanly
        if auth.anilist.username.is_some() {
            outcome
                .warnings
                .push("AniList read-only: remote website entry unchanged".into());
        }
        delete_tracker_entry(pool, anime.id, TrackerProvider::AniList).await?;
    }

    if let Some(res) = mal_res {
        match res {
            Ok(()) => {
                outcome.mal_synced = true;
                delete_tracker_entry(pool, anime.id, TrackerProvider::Mal).await?;
            }
            Err(e) => outcome.warnings.push(format!("MAL: {e}")),
        }
    } else if rm_mal {
        if auth.mal.username.is_some() {
            outcome
                .warnings
                .push("MAL read-only: remote website entry unchanged".into());
        }
        delete_tracker_entry(pool, anime.id, TrackerProvider::Mal).await?;
    }

    let remaining = get_anime_watchlist_source(pool, anime.id).await?;
    let keep_on_remaining = match target {
        WatchlistTarget::AniList => remaining == WatchlistSource::Mal,
        WatchlistTarget::Mal => remaining == WatchlistSource::AniList,
        WatchlistTarget::Both | WatchlistTarget::Local => false,
    };

    if !keep_on_remaining {
        remove_from_watchlist(pool, anime.id).await?;
    }

    let label = if keep_on_remaining {
        format!("Moved to {}", remaining.divider_label())
    } else {
        "Removed from Watchlist".to_string()
    };

    Ok(outcome.finalize(&label))
}

/// Push reconciled winner states back to any lagging tracker across all cross-tracker diffs.
pub async fn push_parity_diffs(
    pool: &SqlitePool,
    diffs: &[MergeDiffItem],
    auth: &mut AuthConfig,
    trackers: &TrackersConfig,
    now: i64,
) -> WritebackOutcome {
    let mal_cid = trackers.resolved_mal_client_id();
    let _ = maybe_refresh_mal_token(auth, mal_cid.as_deref(), now).await;

    let al_client = auth
        .anilist_token()
        .map(|tok| AniListClient::with_token(Some(tok)));
    let mal_client = auth
        .mal_token()
        .map(|tok| MalClient::with_token(mal_cid.clone(), Some(tok)));
    let public_al = AniListClient::new();

    let mut outcome = WritebackOutcome::default();
    let mut pushed_al = 0usize;
    let mut pushed_mal = 0usize;

    for diff in diffs {
        if diff.needs_push_to_al {
            if let Some(ref client) = al_client {
                if diff.anime_id > 0 {
                    match client
                        .save_list_entry(
                            diff.anime_id,
                            &diff.resolved_status,
                            Some(diff.resolved_progress),
                        )
                        .await
                    {
                        Ok(()) => {
                            pushed_al += 1;
                            outcome.anilist_synced = true;
                            let _ = upsert_tracker_entry(
                                pool,
                                diff.anime_id,
                                TrackerProvider::AniList,
                                &diff.resolved_status,
                                diff.resolved_progress,
                                None,
                                Some(now),
                                now,
                            )
                            .await;
                        }
                        Err(e) => outcome
                            .warnings
                            .push(format!("{}: AL {e}", diff.title)),
                    }
                }
            }
        }

        if diff.needs_push_to_mal {
            if let Some(ref client) = mal_client {
                let mid = if let Some(m) = diff.mal_id.filter(|&x| x > 0) {
                    Some(m)
                } else if let Ok(Some(m)) = get_mal_id_for_anime(pool, diff.anime_id).await {
                    Some(m)
                } else if diff.anime_id > 0 {
                    public_al.lookup_mal_id(diff.anime_id).await
                } else {
                    None
                };

                if let Some(mal_id) = mid {
                    match client
                        .update_list_status(
                            mal_id,
                            &diff.resolved_status,
                            Some(diff.resolved_progress),
                            None,
                        )
                        .await
                    {
                        Ok(()) => {
                            pushed_mal += 1;
                            outcome.mal_synced = true;
                            let _ = upsert_tracker_entry(
                                pool,
                                diff.anime_id,
                                TrackerProvider::Mal,
                                &diff.resolved_status,
                                diff.resolved_progress,
                                None,
                                Some(now),
                                now,
                            )
                            .await;
                        }
                        Err(e) => outcome
                            .warnings
                            .push(format!("{}: MAL {e}", diff.title)),
                    }
                } else {
                    outcome
                        .warnings
                        .push(format!("{}: missing MAL ID", diff.title));
                }
            }
        }
    }

    if (diffs.iter().any(|d| d.needs_push_to_al) && al_client.is_none())
        || (diffs.iter().any(|d| d.needs_push_to_mal) && mal_client.is_none())
    {
        outcome
            .warnings
            .push("Connect OAuth on lagging tracker to push updates".into());
    }

    let action = format!("Parity push: AL {}, MAL {}", pushed_al, pushed_mal);
    outcome.finalize(&action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{init, user::{add_to_watchlist, is_in_watchlist}};

    fn sample_anime(id: i64, mal_id: Option<i64>, title: &str) -> Anime {
        Anime {
            id,
            title_english: Some(title.into()),
            title_romaji: title.into(),
            title_native: None,
            description: None,
            episodes: Some(12),
            status: Some("FINISHED".into()),
            season: None,
            season_year: None,
            score: None,
            format: Some("TV".into()),
            genres: "[]".into(),
            synonyms: "[]".into(),
            cover_url: None,
            cover_blob: None,
            has_dub: 0,
            updated_at: 100,
            next_airing_episode: None,
            mal_id,
        }
    }

    #[tokio::test]
    async fn test_remove_watchlist_partial_keeps_remaining_tracker_and_deletes_when_both() {
        let pool = init(":memory:").await.unwrap();
        let anime = sample_anime(100, Some(200), "Vinland Saga");
        upsert_anime(&pool, &anime).await.unwrap();
        add_to_watchlist(&pool, 100, 1_000).await.unwrap();
        upsert_tracker_entry(&pool, 100, TrackerProvider::AniList, "PLANNING", 0, None, None, 1_000)
            .await
            .unwrap();
        upsert_tracker_entry(&pool, 100, TrackerProvider::Mal, "PLANNING", 0, None, None, 1_000)
            .await
            .unwrap();

        let mut auth = AuthConfig::default();
        let trackers = TrackersConfig::default();

        // Removing MAL side from a Both show keeps it in AniList Watchlist cleanly
        let res = remove_watchlist_from_target(
            &pool,
            &anime,
            WatchlistTarget::Mal,
            &mut auth,
            &trackers,
            2_000,
        )
        .await
        .unwrap();
        assert!(res.summary.contains("ANILIST"));
        assert!(is_in_watchlist(&pool, 100).await.unwrap());
        assert_eq!(get_anime_watchlist_source(&pool, 100).await.unwrap(), WatchlistSource::AniList);

        // Removing remaining AniList side evicts from Watchlist completely
        let _ = remove_watchlist_from_target(
            &pool,
            &anime,
            WatchlistTarget::AniList,
            &mut auth,
            &trackers,
            2_100,
        )
        .await
        .unwrap();
        assert!(!is_in_watchlist(&pool, 100).await.unwrap());
    }

    #[tokio::test]
    async fn test_add_watchlist_local_creates_local_entry_without_remote() {
        let pool = init(":memory:").await.unwrap();
        let anime = sample_anime(300, None, "Local Show");
        let mut auth = AuthConfig::default();
        let trackers = TrackersConfig::default();

        let outcome = add_watchlist_to_target(
            &pool,
            &anime,
            WatchlistTarget::Local,
            &mut auth,
            &trackers,
            1_000,
        )
        .await
        .unwrap();
        assert!(!outcome.anilist_synced);
        assert!(!outcome.mal_synced);
        assert!(is_in_watchlist(&pool, 300).await.unwrap());
        assert_eq!(get_anime_watchlist_source(&pool, 300).await.unwrap(), WatchlistSource::Local);
    }
}
