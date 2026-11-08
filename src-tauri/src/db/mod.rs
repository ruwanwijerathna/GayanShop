//! SQLite pool, PRAGMAs, migrations and the single-writer transaction helper (ARCHITECTURE §1.1).

pub mod integrity;
pub mod outbox;
pub mod repo;
pub mod settings;
pub mod sync_ctx;

use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqliteConnection, SqlitePool, Transaction};
use tokio::sync::{Mutex, MutexGuard};

use crate::error::{AppError, AppResult};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub struct Db {
    pub pool: SqlitePool,
    /// Every write goes through one transaction at a time. This avoids SQLITE_BUSY on deferred
    /// transaction lock upgrades and keeps the per-device audit sequence gapless.
    write_lock: Mutex<()>,
    path: PathBuf,
}

/// A write transaction that holds the process-wide writer lock until commit (or drop = rollback).
pub struct WriteTx<'a> {
    tx: Transaction<'static, Sqlite>,
    _guard: MutexGuard<'a, ()>,
}

impl WriteTx<'_> {
    pub fn conn(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }

    pub async fn commit(self) -> AppResult<()> {
        self.tx.commit().await?;
        Ok(())
    }
}

fn connect_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        // FULL, not NORMAL: under WAL, NORMAL can lose the last committed sale on power loss (§13.2).
        .synchronous(SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        .pragma("cache_size", "-65536")
        .pragma("temp_store", "MEMORY")
        .pragma("mmap_size", "268435456")
}

impl Db {
    /// Open (creating if needed) the database at `path` and apply pending migrations.
    /// If the file already holds data and a migration is pending, a `VACUUM INTO` snapshot is taken
    /// into `backup_dir` first (§13.2), so a failed migration can always be rolled back by hand.
    pub async fn open(path: &Path, backup_dir: &Path) -> AppResult<Self> {
        let pool = SqlitePoolOptions::new().max_connections(4).connect_with(connect_options(path)).await?;
        let db = Self { pool, write_lock: Mutex::new(()), path: path.to_path_buf() };
        if db.has_pending_migrations().await? && db.has_user_tables().await? {
            db.snapshot_before_migration(backup_dir).await?;
        }
        MIGRATOR.run(&db.pool).await?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Start the one-and-only write transaction.
    pub async fn write(&self) -> AppResult<WriteTx<'_>> {
        let guard = self.write_lock.lock().await;
        let tx = self.pool.begin().await?;
        Ok(WriteTx { tx, _guard: guard })
    }

    async fn has_user_tables(&self) -> AppResult<bool> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(n > 0)
    }

    async fn has_pending_migrations(&self) -> AppResult<bool> {
        let has_table: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'")
                .fetch_one(&self.pool)
                .await?;
        let applied: Vec<i64> = if has_table == 0 {
            Vec::new()
        } else {
            sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success = 1").fetch_all(&self.pool).await?
        };
        Ok(MIGRATOR.iter().any(|m| !applied.contains(&m.version)))
    }

    async fn snapshot_before_migration(&self, backup_dir: &Path) -> AppResult<()> {
        std::fs::create_dir_all(backup_dir).map_err(|e| AppError::internal(format!("backup dir: {e}")))?;
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let target = backup_dir.join(format!("pre-migration-{stamp}.db"));
        let target = target.to_str().ok_or_else(|| AppError::internal("backup path is not valid UTF-8"))?;
        sqlx::query("VACUUM INTO ?1").bind(target).execute(&self.pool).await?;
        Ok(())
    }

    /// Latest applied migration version (used by backups and the support bundle).
    pub async fn schema_version(&self) -> AppResult<i64> {
        Ok(sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&self.pool)
            .await?)
    }
}
