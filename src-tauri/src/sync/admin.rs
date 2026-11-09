//! Settings → Sync for the owner (doc §6.1, `SYNC_MANAGE`): the ops the cloud rejected, and the sync log.
//!
//! A dead op can be **retried** or **discarded**, and both are audited (`SYNC_DEAD_OP_RESOLVE`). The cloud
//! remembers every op id it answered, rejections included, so resending the same op would only be rejected
//! again: a retry queues a fresh snapshot of the row as it is now, under a new op id. Discarding leaves the
//! row on this counter only.

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{FromRow, SqlitePool};

use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::db::repo::WriteCtx;
use crate::db::{Db, outbox};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct DeadOp {
    #[specta(type = specta_typescript::Number)]
    pub seq: i64,
    pub table_name: String,
    pub row_id: String,
    /// The cloud's reason, e.g. `COST_LOCKED: …` or `OWNER_INVARIANT: …`.
    pub last_error: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub attempts: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DeadOpAction {
    Retry,
    Discard,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct ResolveDeadOp {
    #[specta(type = specta_typescript::Number)]
    pub seq: i64,
    pub action: DeadOpAction,
}

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct SyncLogRow {
    pub id: String,
    pub direction: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub result: String,
    #[specta(type = specta_typescript::Number)]
    pub rows_sent: i64,
    #[specta(type = specta_typescript::Number)]
    pub rows_received: i64,
    #[specta(type = specta_typescript::Number)]
    pub rows_rejected: i64,
    pub error_code: Option<String>,
}

pub async fn dead_ops(db: &Db) -> AppResult<Vec<DeadOp>> {
    Ok(sqlx::query_as(
        "SELECT seq, table_name, row_id, last_error, attempts, created_at FROM sync_outbox
         WHERE status = 'dead' ORDER BY seq",
    )
    .fetch_all(&db.pool)
    .await?)
}

/// The latest sync log entries, newest first.
pub async fn logs_pool(pool: &SqlitePool, limit: u32) -> AppResult<Vec<SyncLogRow>> {
    Ok(sqlx::query_as(
        "SELECT id, direction, started_at, finished_at, result, rows_sent, rows_received, rows_rejected, error_code
         FROM sync_logs ORDER BY started_at DESC, id DESC LIMIT ?1",
    )
    .bind(i64::from(limit.clamp(1, 500)))
    .fetch_all(pool)
    .await?)
}

pub async fn logs(db: &Db, limit: u32) -> AppResult<Vec<SyncLogRow>> {
    logs_pool(&db.pool, limit).await
}

pub async fn resolve(db: &Db, ctx: &WriteCtx, actor: &Actor, input: &ResolveDeadOp) -> AppResult<()> {
    let mut tx = db.write().await?;
    let c = tx.conn();
    let op: Option<DeadOp> = sqlx::query_as(
        "SELECT seq, table_name, row_id, last_error, attempts, created_at FROM sync_outbox
         WHERE seq = ?1 AND status = 'dead'",
    )
    .bind(input.seq)
    .fetch_optional(&mut *c)
    .await?;
    let op = op.ok_or_else(|| AppError::not_found("dead sync op"))?;
    sqlx::query("DELETE FROM sync_outbox WHERE seq = ?1").bind(op.seq).execute(&mut *c).await?;
    if input.action == DeadOpAction::Retry {
        // A fresh snapshot of the row as it is now, under a new op id.
        outbox::enqueue_upsert(c, &op.table_name, &op.row_id, &ctx.now).await?;
    }
    audit::record(
        c,
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::SYNC_DEAD_OP_RESOLVE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some(&op.table_name),
            entity_id: Some(&op.row_id),
            metadata: json!({ "action": input.action, "reason": op.last_error, "attempts": op.attempts }),
        },
    )
    .await?;
    tx.commit().await
}
