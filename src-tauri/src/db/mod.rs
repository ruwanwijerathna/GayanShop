//! SQLite pool, PRAGMAs, migrations, and the single-writer transaction helper.

pub mod outbox;
pub mod settings;

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqliteConnection, SqlitePool, Transaction};
use tokio::sync::{Mutex, MutexGuard};

use crate::error::AppResult;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub struct Db {
    pub pool: SqlitePool,
    /// Every write goes through one transaction at a time. This avoids SQLITE_BUSY on
    /// deferred-transaction lock upgrades and keeps the per-device audit sequence gapless.
    write_lock: Mutex<()>,
}

/// A write transaction holding the process-wide writer lock until commit/rollback.
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

impl Db {
    pub async fn open(path: &Path) -> AppResult<Self> {
        let url = format!("sqlite://{}", path.display());
        let opts = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool, write_lock: Mutex::new(()) })
    }

    pub async fn write(&self) -> AppResult<WriteTx<'_>> {
        let guard = self.write_lock.lock().await;
        let tx = self.pool.begin().await?;
        Ok(WriteTx { tx, _guard: guard })
    }
}
