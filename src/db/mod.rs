pub mod cache;
pub mod playlists;
pub mod sync;
pub mod user;

use crate::error::Result;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

/// Initialize the SQLite connection pool and run all migrations.
pub async fn init(db_path: &str) -> Result<SqlitePool> {
    // Ensure the parent directory exists
    if db_path != ":memory:" {
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&format!("sqlite://{}?mode=rwc", db_path))
        .await?;

    // High performance SQLite configuration:
    // - WAL mode enables concurrent readers while writing, removing read/write contention.
    // - NORMAL synchronous reduces disk fsync overhead by ~95% on Windows NTFS and macOS APFS.
    // - busy_timeout avoids spurious lock failures under multithreaded loads.
    let _ = sqlx::query("PRAGMA journal_mode = WAL;").execute(&pool).await;
    let _ = sqlx::query("PRAGMA synchronous = NORMAL;").execute(&pool).await;
    let _ = sqlx::query("PRAGMA busy_timeout = 5000;").execute(&pool).await;
    let _ = sqlx::query("PRAGMA temp_store = MEMORY;").execute(&pool).await;

    run_migrations(&pool).await?;
    Ok(pool)
}

/// Run embedded SQL migrations in order.
async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    sqlx::query(include_str!("../../migrations/001_initial.sql"))
        .execute(pool)
        .await?;
    let _ = sqlx::query("ALTER TABLE anime ADD COLUMN next_airing_episode INTEGER;")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE anime ADD COLUMN mal_id INTEGER;")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE anime ADD COLUMN synonyms TEXT NOT NULL DEFAULT '[]';")
        .execute(pool)
        .await;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_anime_mal_id ON anime(mal_id);")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS id_crosswalk (
            mal_id      INTEGER PRIMARY KEY,
            anilist_id  INTEGER NOT NULL,
            resolved_at INTEGER NOT NULL,
            source      TEXT    NOT NULL DEFAULT 'anilist_idmal'
        );"
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_crosswalk_anilist ON id_crosswalk(anilist_id);")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tracker_entries (
            anime_id          INTEGER NOT NULL REFERENCES anime(id),
            provider          TEXT    NOT NULL,
            status            TEXT    NOT NULL,
            progress          INTEGER NOT NULL DEFAULT 0,
            score             INTEGER,
            remote_updated_at INTEGER,
            last_synced_at    INTEGER NOT NULL,
            PRIMARY KEY (anime_id, provider)
        );"
    )
    .execute(pool)
    .await?;
    let _ = sqlx::query(
        "DELETE FROM sync_meta WHERE key IN ('seasonal', 'trending') AND EXISTS (SELECT 1 FROM anime WHERE status = 'RELEASING' AND next_airing_episode IS NULL);"
    )
    .execute(pool)
    .await;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS dismissed_sync (
            anime_id     INTEGER NOT NULL,
            list_type    TEXT    NOT NULL,
            dismissed_at INTEGER NOT NULL,
            PRIMARY KEY (anime_id, list_type)
        );"
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playback_progress (
            anime_id        INTEGER NOT NULL,
            episode         INTEGER NOT NULL,
            position_secs   REAL NOT NULL,
            duration_secs   REAL NOT NULL,
            updated_at      INTEGER NOT NULL,
            PRIMARY KEY (anime_id, episode)
        );"
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS anime_fillers (
            anime_id        INTEGER NOT NULL,
            episode         INTEGER NOT NULL,
            filler_type     TEXT NOT NULL,
            PRIMARY KEY (anime_id, episode)
        );"
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS offline_episodes (
            anime_id        INTEGER NOT NULL,
            episode         INTEGER NOT NULL,
            file_path       TEXT NOT NULL,
            file_size       INTEGER NOT NULL,
            downloaded_at   INTEGER NOT NULL,
            PRIMARY KEY (anime_id, episode)
        );"
    )
    .execute(pool)
    .await?;

    // Multi-user profile registry & active profile pointer
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS profiles (
            id              TEXT    PRIMARY KEY,
            display_name    TEXT    NOT NULL,
            avatar_icon     TEXT    NOT NULL DEFAULT '◈',
            theme_index     INTEGER NOT NULL DEFAULT 0,
            audio_mode      TEXT    NOT NULL DEFAULT 'sub',
            created_at      INTEGER NOT NULL,
            last_active_at  INTEGER NOT NULL
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS active_profile_state (
            id          INTEGER PRIMARY KEY CHECK (id = 1),
            profile_id  TEXT    NOT NULL DEFAULT 'default'
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT OR IGNORE INTO profiles (id, display_name, avatar_icon, theme_index, audio_mode, created_at, last_active_at)
         VALUES ('default', 'Default', '◈', 0, 'sub', strftime('%s','now'), strftime('%s','now'));",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT OR IGNORE INTO active_profile_state (id, profile_id) VALUES (1, 'default');",
    )
    .execute(pool)
    .await?;

    // One-time idempotent migration from legacy single-user tables to profile-scoped composite keys
    let has_profile_col: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('history') WHERE name = 'profile_id'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    if has_profile_col == 0 {
        let mut tx = pool.begin().await?;

        sqlx::query(
            "CREATE TABLE history_v2 (
                profile_id  TEXT    NOT NULL DEFAULT 'default',
                anime_id    INTEGER NOT NULL REFERENCES anime(id),
                episode     INTEGER NOT NULL,
                watched_at  INTEGER NOT NULL,
                PRIMARY KEY (profile_id, anime_id, episode)
            );",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO history_v2 (profile_id, anime_id, episode, watched_at)
             SELECT 'default', anime_id, episode, watched_at FROM history;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE history;")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE history_v2 RENAME TO history;")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "CREATE TABLE continue_watching_v2 (
                profile_id   TEXT    NOT NULL DEFAULT 'default',
                anime_id     INTEGER NOT NULL REFERENCES anime(id),
                last_episode INTEGER NOT NULL,
                last_watched INTEGER NOT NULL,
                PRIMARY KEY (profile_id, anime_id)
            );",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO continue_watching_v2 (profile_id, anime_id, last_episode, last_watched)
             SELECT 'default', anime_id, last_episode, last_watched FROM continue_watching;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE continue_watching;")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE continue_watching_v2 RENAME TO continue_watching;")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "CREATE TABLE watchlist_v2 (
                profile_id TEXT    NOT NULL DEFAULT 'default',
                anime_id   INTEGER NOT NULL REFERENCES anime(id),
                added_at   INTEGER NOT NULL,
                PRIMARY KEY (profile_id, anime_id)
            );",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO watchlist_v2 (profile_id, anime_id, added_at)
             SELECT 'default', anime_id, added_at FROM watchlist;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE watchlist;")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE watchlist_v2 RENAME TO watchlist;")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "CREATE TABLE playback_progress_v2 (
                profile_id    TEXT    NOT NULL DEFAULT 'default',
                anime_id      INTEGER NOT NULL,
                episode       INTEGER NOT NULL,
                position_secs REAL    NOT NULL,
                duration_secs REAL    NOT NULL,
                updated_at    INTEGER NOT NULL,
                PRIMARY KEY (profile_id, anime_id, episode)
            );",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO playback_progress_v2 (profile_id, anime_id, episode, position_secs, duration_secs, updated_at)
             SELECT 'default', anime_id, episode, position_secs, duration_secs, updated_at FROM playback_progress;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE playback_progress;")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE playback_progress_v2 RENAME TO playback_progress;")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "CREATE TABLE tracker_entries_v2 (
                profile_id        TEXT    NOT NULL DEFAULT 'default',
                anime_id          INTEGER NOT NULL REFERENCES anime(id),
                provider          TEXT    NOT NULL,
                status            TEXT    NOT NULL,
                progress          INTEGER NOT NULL DEFAULT 0,
                score             INTEGER,
                remote_updated_at INTEGER,
                last_synced_at    INTEGER NOT NULL,
                PRIMARY KEY (profile_id, anime_id, provider)
            );",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO tracker_entries_v2 (profile_id, anime_id, provider, status, progress, score, remote_updated_at, last_synced_at)
             SELECT 'default', anime_id, provider, status, progress, score, remote_updated_at, last_synced_at FROM tracker_entries;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE tracker_entries;")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE tracker_entries_v2 RENAME TO tracker_entries;")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "CREATE TABLE dismissed_sync_v2 (
                profile_id   TEXT    NOT NULL DEFAULT 'default',
                anime_id     INTEGER NOT NULL,
                list_type    TEXT    NOT NULL,
                dismissed_at INTEGER NOT NULL,
                PRIMARY KEY (profile_id, anime_id, list_type)
            );",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO dismissed_sync_v2 (profile_id, anime_id, list_type, dismissed_at)
             SELECT 'default', anime_id, list_type, dismissed_at FROM dismissed_sync;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE dismissed_sync;")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE dismissed_sync_v2 RENAME TO dismissed_sync;")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "CREATE TABLE playback_prefs_v2 (
                profile_id TEXT    NOT NULL DEFAULT 'default',
                anime_id   INTEGER NOT NULL REFERENCES anime(id),
                query      TEXT    NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (profile_id, anime_id)
            );",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO playback_prefs_v2 (profile_id, anime_id, query, updated_at)
             SELECT 'default', anime_id, query, updated_at FROM playback_prefs;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE playback_prefs;")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE playback_prefs_v2 RENAME TO playback_prefs;")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "CREATE TABLE audio_prefs_v2 (
                profile_id TEXT    NOT NULL DEFAULT 'default',
                anime_id   INTEGER NOT NULL REFERENCES anime(id),
                audio_mode TEXT    NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (profile_id, anime_id)
            );",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audio_prefs_v2 (profile_id, anime_id, audio_mode, updated_at)
             SELECT 'default', anime_id, audio_mode, updated_at FROM audio_prefs;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE audio_prefs;")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE audio_prefs_v2 RENAME TO audio_prefs;")
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
    }

    // Schema enhancement: store canonical 5-state list status directly on watchlist entries
    let _ = sqlx::query(
        "ALTER TABLE watchlist ADD COLUMN status TEXT NOT NULL DEFAULT 'PLANNING';",
    )
    .execute(pool)
    .await;

    // Performance indexes for instant local browsing, scrobbling, and history aggregation
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_history_profile_anime ON history(profile_id, anime_id);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_continue_watching_profile_last ON continue_watching(profile_id, last_watched DESC);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_category_entries_cat_pos ON category_entries(category, position);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_playback_progress_profile_updated ON playback_progress(profile_id, updated_at DESC);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tracker_entries_profile_anime ON tracker_entries(profile_id, anime_id);")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_anime_status ON anime(status);")
        .execute(pool)
        .await?;

    // Schema v3: Profile-scoped Watch Session Saved Playlists
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playlists (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            profile_id   TEXT    NOT NULL DEFAULT 'default',
            name         TEXT    NOT NULL,
            created_at   INTEGER NOT NULL,
            updated_at   INTEGER NOT NULL,
            loop_default INTEGER NOT NULL DEFAULT 0
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playlist_items (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            position    INTEGER NOT NULL,
            anime_id    INTEGER NOT NULL,
            mal_id      INTEGER,
            title       TEXT    NOT NULL,
            query       TEXT    NOT NULL,
            episode     INTEGER NOT NULL,
            season_year INTEGER,
            quality     TEXT    NOT NULL,
            dub         INTEGER NOT NULL DEFAULT 0,
            sub_lang    TEXT    NOT NULL,
            player      TEXT    NOT NULL
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_playlists_profile ON playlists(profile_id, updated_at DESC);",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_playlist_items_order ON playlist_items(playlist_id, position ASC);",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS profile_playback_settings (
            profile_id        TEXT PRIMARY KEY,
            dub               INTEGER NOT NULL DEFAULT 0,
            subtitle_code     TEXT    NOT NULL DEFAULT 'eng',
            subtitle_delay_ms INTEGER NOT NULL DEFAULT 0,
            quality           TEXT    NOT NULL DEFAULT '1080p',
            preferred_player  TEXT    NOT NULL DEFAULT 'auto-best',
            updated_at        INTEGER NOT NULL DEFAULT 0
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS anime_subtitle_offsets (
            profile_id TEXT    NOT NULL,
            anime_id   INTEGER NOT NULL,
            delay_ms   INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (profile_id, anime_id)
        );",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS anime_provider_sub_delays (
            profile_id  TEXT    NOT NULL,
            anime_id    INTEGER NOT NULL,
            provider_id TEXT    NOT NULL,
            delay_ms    INTEGER NOT NULL DEFAULT 0,
            updated_at  INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (profile_id, anime_id, provider_id)
        );",
    )
    .execute(pool)
    .await?;

    let _ = sqlx::query("DELETE FROM anime_provider_sub_delays WHERE ABS(delay_ms) >= 90000;")
        .execute(pool)
        .await;
    let _ = sqlx::query("PRAGMA user_version = 7;").execute(pool).await;

    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_creates_tables() {
        let pool = init(":memory:").await.expect("DB init failed");

        // Verify all tables exist by querying sqlite_master
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert!(tables.contains(&"anime".to_string()));
        assert!(tables.contains(&"category_entries".to_string()));
        assert!(tables.contains(&"genre_entries".to_string()));
        assert!(tables.contains(&"history".to_string()));
        assert!(tables.contains(&"continue_watching".to_string()));
        assert!(tables.contains(&"watchlist".to_string()));
        assert!(tables.contains(&"playback_prefs".to_string()));
        assert!(tables.contains(&"audio_prefs".to_string()));
        assert!(tables.contains(&"sync_meta".to_string()));
        assert!(tables.contains(&"id_crosswalk".to_string()));
        assert!(tables.contains(&"tracker_entries".to_string()));
        assert!(tables.contains(&"playlists".to_string()));
        assert!(tables.contains(&"playlist_items".to_string()));
        assert!(tables.contains(&"profile_playback_settings".to_string()));
        assert!(tables.contains(&"anime_subtitle_offsets".to_string()));
    }

    #[tokio::test]
    async fn test_migrations_are_idempotent() {
        // Running migrations twice on the same DB should not error
        let pool = init(":memory:").await.unwrap();
        let result = run_migrations(&pool).await;
        assert!(result.is_ok(), "Second migration run should be idempotent");
    }
}
