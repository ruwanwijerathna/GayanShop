//! One sync round: push the outbox, then pull until caught up (ARCHITECTURE §6.1, §6.2). The worker
//! (`worker.rs`) schedules rounds; tests call them directly against a fake server.
//!
//! Push: up to 200 ready ops in `seq` order (parents before children), marked `in_flight` first so a crash
//! mid-push is recovered at the next start. The server answers per op:
//!   * `ok`       → deleted from the outbox;
//!   * `rejected` → `dead`, logged, and surfaced in Settings → Sync (`SYNC_MANAGE`);
//!   * `retry`    → `failed` with backoff (a parent it needs hasn't reached the cloud yet).
//!
//! A network or server failure marks the whole batch `failed` with backoff. Every op id is idempotent on the
//! server, so a batch whose answer was lost is simply sent again: no duplicates, no losses.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::json;
use specta::Type;

use super::api::{SyncApi, SyncError};
use super::apply::{self, Applied};
use super::wire::{OpStatus, PushOp};
use crate::clock::{Clock, iso, new_id};
use crate::db::Db;
use crate::error::AppError;

pub const PUSH_BATCH: i64 = 200;
pub const PULL_PAGE: i64 = 500;

/// A round failed locally (the database) or remotely (the server or the network).
#[derive(Debug, thiserror::Error)]
pub enum RoundError {
    #[error(transparent)]
    Local(#[from] AppError),
    #[error(transparent)]
    Remote(#[from] SyncError),
}

impl From<RoundError> for AppError {
    fn from(e: RoundError) -> Self {
        match e {
            RoundError::Local(e) => e,
            RoundError::Remote(e) => e.into(),
        }
    }
}

impl From<sqlx::Error> for RoundError {
    fn from(e: sqlx::Error) -> Self {
        RoundError::Local(e.into())
    }
}

/// Retry delay after `attempts` failures: 5 s doubling, capped at 5 minutes (doc §1.3).
pub fn backoff(attempts: i64) -> Duration {
    let exp = u32::try_from(attempts.clamp(1, 7) - 1).unwrap_or(0);
    Duration::seconds((5_i64 * 2_i64.pow(exp)).min(300))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Type)]
pub struct PushOutcome {
    #[specta(type = specta_typescript::Number)]
    pub sent: i64,
    #[specta(type = specta_typescript::Number)]
    pub ok: i64,
    #[specta(type = specta_typescript::Number)]
    pub rejected: i64,
    #[specta(type = specta_typescript::Number)]
    pub retried: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PullOutcome {
    pub pages: i64,
    pub rows: i64,
    pub cursor: i64,
    pub watermark: i64,
    pub caught_up: bool,
    pub tables: Vec<&'static str>,
}

struct Ready {
    seq: i64,
    op_id: String,
    table_name: String,
    row_id: String,
    op: String,
    payload: String,
    attempts: i64,
}

/// One `sync_logs` row (Settings → Sync shows the latest).
struct LogEntry<'a> {
    direction: &'a str,
    started: &'a str,
    result: &'a str,
    sent: i64,
    received: i64,
    rejected: i64,
    cursor_before: Option<i64>,
    cursor_after: Option<i64>,
    error: Option<&'a SyncError>,
    details: Option<serde_json::Value>,
}

async fn log(db: &Db, clock: &dyn Clock, device_id: &str, e: LogEntry<'_>) -> Result<(), RoundError> {
    let mut tx = db.write().await?;
    sqlx::query(
        "INSERT INTO sync_logs (id, device_id, direction, started_at, finished_at, result, rows_sent, rows_received,
           rows_rejected, cursor_before, cursor_after, error_code, error_message, details)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )
    .bind(new_id())
    .bind(device_id)
    .bind(e.direction)
    .bind(e.started)
    .bind(iso(clock.now()))
    .bind(e.result)
    .bind(e.sent)
    .bind(e.received)
    .bind(e.rejected)
    .bind(e.cursor_before)
    .bind(e.cursor_after)
    .bind(e.error.map(SyncError::code))
    .bind(e.error.map(ToString::to_string))
    .bind(e.details.map(|d| d.to_string()))
    .execute(tx.conn())
    .await?;
    tx.commit().await?;
    Ok(())
}

/// After a crash mid-push, `in_flight` ops go back to the queue (the server dedupes by op id).
pub async fn recover_in_flight(db: &Db) -> Result<u64, RoundError> {
    let mut tx = db.write().await?;
    let n = sqlx::query("UPDATE sync_outbox SET status = 'pending' WHERE status = 'in_flight'")
        .execute(tx.conn())
        .await?
        .rows_affected();
    tx.commit().await?;
    Ok(n)
}

/// Push one batch of ready ops. Returns `sent = 0` when nothing is ready.
pub async fn push_batch(
    db: &Db,
    api: &dyn SyncApi,
    clock: &dyn Clock,
    device_id: &str,
) -> Result<PushOutcome, RoundError> {
    push_range(db, api, clock, device_id, (0, i64::MAX)).await
}

/// Push one batch of the ready ops whose outbox seq is in `(after, upto]`. After an offline restore the ops
/// up to the retirement belong to the retired identity and go under it (Block 4.1); the rest go under the
/// counter's current identity.
pub async fn push_range(
    db: &Db,
    api: &dyn SyncApi,
    clock: &dyn Clock,
    device_id: &str,
    (after, upto): (i64, i64),
) -> Result<PushOutcome, RoundError> {
    let now_dt: DateTime<Utc> = clock.now();
    let now = iso(now_dt);
    let mut tx = db.write().await?;
    let ready: Vec<Ready> = sqlx::query_as::<_, (i64, String, String, String, String, String, i64)>(
        "SELECT seq, op_id, table_name, row_id, op, payload, attempts FROM sync_outbox
         WHERE status IN ('pending', 'failed') AND next_attempt_at <= ?1 AND seq > ?3 AND seq <= ?4
         ORDER BY seq LIMIT ?2",
    )
    .bind(&now)
    .bind(PUSH_BATCH)
    .bind(after)
    .bind(upto)
    .fetch_all(tx.conn())
    .await?
    .into_iter()
    .map(|(seq, op_id, table_name, row_id, op, payload, attempts)| Ready {
        seq,
        op_id,
        table_name,
        row_id,
        op,
        payload,
        attempts,
    })
    .collect();
    if ready.is_empty() {
        return Ok(PushOutcome::default());
    }
    for r in &ready {
        sqlx::query("UPDATE sync_outbox SET status = 'in_flight' WHERE seq = ?1")
            .bind(r.seq)
            .execute(tx.conn())
            .await?;
    }
    tx.commit().await?;

    let mut ops = Vec::with_capacity(ready.len());
    for r in &ready {
        let row = serde_json::from_str(&r.payload).map_err(AppError::from)?;
        ops.push(PushOp { op_id: r.op_id.clone(), table: r.table_name.clone(), op: r.op.clone(), row });
    }
    let sent = i64::try_from(ready.len()).unwrap_or(i64::MAX);

    let results = match api.push(device_id, &ops).await {
        Ok(results) => results,
        Err(e) => {
            let mut tx = db.write().await?;
            for r in &ready {
                let attempts = r.attempts + 1;
                sqlx::query(
                    "UPDATE sync_outbox SET status = 'failed', attempts = ?1, next_attempt_at = ?2, last_error = ?3
                     WHERE seq = ?4",
                )
                .bind(attempts)
                .bind(iso(now_dt + backoff(attempts)))
                .bind(e.code())
                .bind(r.seq)
                .execute(tx.conn())
                .await?;
            }
            tx.commit().await?;
            let entry = LogEntry {
                direction: "push",
                started: &now,
                result: "error",
                sent,
                received: 0,
                rejected: 0,
                cursor_before: None,
                cursor_after: None,
                error: Some(&e),
                details: None,
            };
            log(db, clock, device_id, entry).await?;
            return Err(e.into());
        }
    };

    let mut out = PushOutcome { sent, ..PushOutcome::default() };
    let mut rejections = Vec::new();
    let mut tx = db.write().await?;
    for (r, res) in ready.iter().zip(&results) {
        if res.op_id.as_str() != Some(r.op_id.as_str()) {
            return Err(SyncError::Protocol(format!("result for {} answered op {}", r.op_id, res.op_id)).into());
        }
        let attempts = r.attempts + 1;
        match res.status {
            OpStatus::Ok => {
                sqlx::query("DELETE FROM sync_outbox WHERE seq = ?1").bind(r.seq).execute(tx.conn()).await?;
                out.ok += 1;
            }
            OpStatus::Rejected => {
                sqlx::query("UPDATE sync_outbox SET status = 'dead', attempts = ?1, last_error = ?2 WHERE seq = ?3")
                    .bind(attempts)
                    .bind(res.reason.as_deref().unwrap_or("REJECTED"))
                    .bind(r.seq)
                    .execute(tx.conn())
                    .await?;
                rejections
                    .push(json!({ "op_id": r.op_id, "table": r.table_name, "row_id": r.row_id, "reason": res.reason }));
                out.rejected += 1;
            }
            OpStatus::Retry => {
                sqlx::query(
                    "UPDATE sync_outbox SET status = 'failed', attempts = ?1, next_attempt_at = ?2, last_error = ?3
                     WHERE seq = ?4",
                )
                .bind(attempts)
                .bind(iso(now_dt + backoff(attempts)))
                .bind(res.reason.as_deref().unwrap_or("RETRY"))
                .bind(r.seq)
                .execute(tx.conn())
                .await?;
                out.retried += 1;
            }
        }
    }
    tx.commit().await?;
    let result = if out.rejected == 0 && out.retried == 0 { "success" } else { "partial" };
    let details = (!rejections.is_empty()).then(|| json!({ "rejected": rejections }));
    let entry = LogEntry {
        direction: "push",
        started: &now,
        result,
        sent,
        received: 0,
        rejected: out.rejected,
        cursor_before: None,
        cursor_after: None,
        error: None,
        details,
    };
    log(db, clock, device_id, entry).await?;
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullOptions {
    pub page_size: i64,
    pub max_pages: i64,
    pub include_own: bool,
}

impl PullOptions {
    /// A routine round: normal pages, this device's own writes excluded.
    pub const ROUTINE: PullOptions = PullOptions { page_size: PULL_PAGE, max_pages: 200, include_own: false };
}

/// Pull pages until caught up (or `max_pages`). `include_own` is for bootstrap: a recovered terminal needs
/// its own past writes back too. `on_page` sees each applied page (bootstrap progress).
pub async fn pull(
    db: &Db,
    api: &dyn SyncApi,
    clock: &dyn Clock,
    device_id: &str,
    opts: PullOptions,
    on_page: &(dyn Fn(&PullOutcome) + Send + Sync),
) -> Result<PullOutcome, RoundError> {
    let PullOptions { page_size, max_pages, include_own } = opts;
    let started = iso(clock.now());
    let before = apply::cursor(db).await?;
    let mut out = PullOutcome { cursor: before, ..PullOutcome::default() };
    let mut collisions = Vec::new();
    while out.pages < max_pages {
        let page = match api.pull(out.cursor, page_size, include_own).await {
            Ok(p) => p,
            Err(e) => {
                let entry = LogEntry {
                    direction: "pull",
                    started: &started,
                    result: "error",
                    sent: 0,
                    received: out.rows,
                    rejected: 0,
                    cursor_before: Some(before),
                    cursor_after: Some(out.cursor),
                    error: Some(&e),
                    details: None,
                };
                log(db, clock, device_id, entry).await?;
                return Err(e.into());
            }
        };
        let applied: Applied = apply::apply_page(db, device_id, &iso(clock.now()), &page).await?;
        out.pages += 1;
        out.rows += i64::try_from(applied.received).unwrap_or(i64::MAX);
        out.cursor = page.cursor;
        out.watermark = page.watermark.max(page.cursor);
        for t in applied.tables {
            if !out.tables.contains(&t) {
                out.tables.push(t);
            }
        }
        collisions.extend(applied.collisions);
        on_page(&out);
        if !page.has_more {
            out.caught_up = true;
            break;
        }
    }
    if out.rows > 0 || !collisions.is_empty() {
        let result = if collisions.is_empty() { "success" } else { "partial" };
        let details = (!collisions.is_empty()).then(|| json!({ "collisions": collisions }));
        let rejected = i64::try_from(collisions.len()).unwrap_or(i64::MAX);
        let entry = LogEntry {
            direction: "pull",
            started: &started,
            result,
            sent: 0,
            received: out.rows,
            rejected,
            cursor_before: Some(before),
            cursor_after: Some(out.cursor),
            error: None,
            details,
        };
        log(db, clock, device_id, entry).await?;
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RoundOutcome {
    pub pushed: PushOutcome,
    pub pulled: PullOutcome,
}

/// One full round: push everything ready (in batches), then pull until caught up.
pub async fn round(db: &Db, api: &dyn SyncApi, clock: &dyn Clock, device_id: &str) -> Result<RoundOutcome, RoundError> {
    // Ops queued before an offline restore belong to the retired identity, never to this one.
    let after = crate::restore::retired(&db.pool).await?.map_or(0, |r| r.upto_seq);
    let mut pushed = PushOutcome::default();
    for _ in 0..50 {
        let b = push_range(db, api, clock, device_id, (after, i64::MAX)).await?;
        pushed.sent += b.sent;
        pushed.ok += b.ok;
        pushed.rejected += b.rejected;
        pushed.retried += b.retried;
        if b.sent < PUSH_BATCH || b.ok == 0 {
            break;
        }
    }
    let pulled = pull(db, api, clock, device_id, PullOptions::ROUTINE, &|_| {}).await?;
    Ok(RoundOutcome { pushed, pulled })
}

/// After an offline restore: send the retired identity's queued ops under that identity (its token is still
/// in the keyring), then forget it. Returns whether anything of it is still waiting.
pub async fn drain_retired(db: &Db, api: &dyn SyncApi, clock: &dyn Clock) -> Result<bool, RoundError> {
    let Some(retired) = crate::restore::retired(&db.pool).await? else { return Ok(false) };
    for _ in 0..50 {
        let b = push_range(db, api, clock, &retired.device_id, (0, retired.upto_seq)).await?;
        if b.sent < PUSH_BATCH || b.ok == 0 {
            break;
        }
    }
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_outbox WHERE seq <= ?1 AND status <> 'dead'")
        .bind(retired.upto_seq)
        .fetch_one(&db.pool)
        .await?;
    if left == 0 {
        let mut tx = db.write().await?;
        sqlx::query("DELETE FROM device_settings WHERE key = ?1")
            .bind(crate::restore::RETIRED_KEY)
            .execute(tx.conn())
            .await?;
        tx.commit().await?;
    }
    Ok(left > 0)
}
