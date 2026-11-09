//! Full-history bootstrap of a new or recovered counter (ARCHITECTURE §6.2, Block 3.5): the ordinary pull,
//! from cursor 0, with this device's own past writes included, repeated until caught up. Every page is
//! applied in its own transaction and moves the cursor, so an interrupted bootstrap resumes where it
//! stopped. It ends with `rebuild_derived`'s recompute as cheap insurance, and clears
//! `sync.bootstrap_pending` so the till opens.

use serde::Serialize;
use specta::Type;

use super::api::SyncApi;
use super::engine::{self, PullOptions, RoundError};
use super::registration::BOOTSTRAP_PENDING_KEY;
use crate::clock::Clock;
use crate::db::Db;
use crate::db::repo::reports;
use crate::db::settings;
use crate::error::AppResult;

/// Progress of a bootstrap, for the first-run screen's progress bar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Type)]
pub struct BootstrapProgress {
    #[specta(type = specta_typescript::Number)]
    pub pages: i64,
    #[specta(type = specta_typescript::Number)]
    pub rows: i64,
    /// 0–1000: how far the cursor is towards the store's newest change.
    #[specta(type = specta_typescript::Number)]
    pub permille: i64,
    pub done: bool,
}

pub async fn is_pending(db: &Db) -> AppResult<bool> {
    Ok(settings::device_get::<bool>(&db.pool, BOOTSTRAP_PENDING_KEY).await?.unwrap_or(false))
}

/// Pull the whole store into this counter. `page_size` is 500 in the app (the tests use 7).
pub async fn run(
    db: &Db,
    api: &dyn SyncApi,
    clock: &dyn Clock,
    device_id: &str,
    page_size: i64,
    on_progress: &(dyn Fn(&BootstrapProgress) + Send + Sync),
) -> Result<BootstrapProgress, RoundError> {
    let start = super::apply::cursor(db).await?;
    let opts = PullOptions { page_size, max_pages: i64::MAX, include_own: true };
    let report = |p: &engine::PullOutcome| {
        let span = (p.watermark - start).max(1);
        let permille = ((p.cursor - start).clamp(0, span) * 1000) / span;
        on_progress(&BootstrapProgress { pages: p.pages, rows: p.rows, permille: permille.min(999), done: false });
    };
    let out = engine::pull(db, api, clock, device_id, opts, &report).await?;
    let mut tx = db.write().await?;
    reports::recompute(tx.conn()).await?;
    settings::device_set(tx.conn(), BOOTSTRAP_PENDING_KEY, &false).await?;
    tx.commit().await?;
    let done = BootstrapProgress { pages: out.pages, rows: out.rows, permille: 1000, done: true };
    on_progress(&done);
    Ok(done)
}
