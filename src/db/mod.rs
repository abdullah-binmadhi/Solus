pub mod cache;
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
    }

    #[tokio::test]
    async fn test_migrations_are_idempotent() {
        // Running migrations twice on the same DB should not error
        let pool = init(":memory:").await.unwrap();
        let result = run_migrations(&pool).await;
        assert!(result.is_ok(), "Second migration run should be idempotent");
    }
}
