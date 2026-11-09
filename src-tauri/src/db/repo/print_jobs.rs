//! The local print queue (doc §7.5): a job is inserted in the SAME transaction as the thing it prints, so a
//! committed sale/close always has its job and a rolled-back one never prints. The worker
//! (`hardware::printer::worker`) prints them after commit; these are the queue's own writes.

use serde_json::{Value, json};
use sqlx::{SqliteConnection, SqlitePool};

use super::WriteCtx;
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::clock::new_id;
use crate::db::Db;
use crate::error::AppResult;

/// The receipt printer profile; the cash drawer hangs off it (RJ11, doc §7.3).
pub const RECEIPT_PRINTER: &str = "receipt";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintKind {
    Receipt,
    VoidSlip,
    ZReport,
    DrawerKick,
    /// Settings: "Print test page".
    Test,
}

impl PrintKind {
    fn code(self) -> &'static str {
        match self {
            PrintKind::Receipt => "receipt",
            PrintKind::VoidSlip => "void_slip",
            PrintKind::ZReport => "z_report",
            PrintKind::DrawerKick => "drawer_kick",
            PrintKind::Test => "test",
        }
    }
}

/// Queue a job for the receipt printer. `payload` is the JSON render input (re-rendered on retry).
pub async fn enqueue(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    kind: PrintKind,
    ref_id: Option<&str>,
    payload: &Value,
) -> AppResult<String> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO print_jobs (id, kind, ref_id, printer_key, payload, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
    )
    .bind(&id)
    .bind(kind.code())
    .bind(ref_id)
    .bind(RECEIPT_PRINTER)
    .bind(payload.to_string())
    .bind(&ctx.now)
    .execute(&mut *conn)
    .await?;
    Ok(id)
}

/// `(queued, failed)` jobs of one printer for the banner ("2 receipts waiting"), not counting `held` (the
/// startup recovery list, which has its own prompt).
pub async fn counts(pool: &SqlitePool, printer_key: &str, held: &[String]) -> AppResult<(i64, i64)> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, status FROM print_jobs WHERE printer_key = ?1 AND status IN ('queued', 'printing', 'failed')",
    )
    .bind(printer_key)
    .fetch_all(pool)
    .await?;
    let (mut queued, mut failed) = (0, 0);
    for status in rows.iter().filter(|(id, _)| !held.contains(id)).map(|(_, s)| s) {
        if status == "failed" {
            failed += 1;
        } else {
            queued += 1;
        }
    }
    Ok((queued, failed))
}

/// The banner's Retry: every failed job goes back to the queue with fresh attempts.
pub async fn retry_failed(db: &Db, ctx: &WriteCtx, printer_key: &str) -> AppResult<u64> {
    let mut tx = db.write().await?;
    let n = sqlx::query(
        "UPDATE print_jobs SET status = 'queued', attempts = 0, last_error = NULL, updated_at = ?1
         WHERE printer_key = ?2 AND status = 'failed'",
    )
    .bind(&ctx.now)
    .bind(printer_key)
    .execute(tx.conn())
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(n)
}

/// Startup recovery's "Print now": the listed jobs go back to the queue.
pub async fn requeue(db: &Db, ctx: &WriteCtx, ids: &[String]) -> AppResult<u64> {
    let mut tx = db.write().await?;
    let mut n = 0;
    for id in ids {
        n += sqlx::query(
            "UPDATE print_jobs SET status = 'queued', attempts = 0, last_error = NULL, updated_at = ?1
             WHERE id = ?2 AND status IN ('queued', 'printing', 'failed')",
        )
        .bind(&ctx.now)
        .bind(id)
        .execute(tx.conn())
        .await?
        .rows_affected();
    }
    tx.commit().await?;
    Ok(n)
}

/// "Skip" / "Reprint later" dismissal: the jobs are `abandoned` (never printed) and the dismissal is audited
/// once, with what was dropped.
pub async fn abandon(db: &Db, ctx: &WriteCtx, actor: &Actor, ids: &[String], reason: &str) -> AppResult<u64> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let mut dropped: Vec<serde_json::Value> = Vec::new();
    for id in ids {
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT kind, ref_id FROM print_jobs WHERE id = ?1 AND status IN ('queued', 'printing', 'failed')",
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
        let Some((kind, ref_id)) = row else { continue };
        sqlx::query("UPDATE print_jobs SET status = 'abandoned', updated_at = ?1 WHERE id = ?2")
            .bind(&ctx.now)
            .bind(id)
            .execute(&mut *conn)
            .await?;
        dropped.push(json!({ "job_id": id, "kind": kind, "ref_id": ref_id }));
    }
    let n = u64::try_from(dropped.len()).unwrap_or(0);
    if n > 0 {
        audit::record(
            conn,
            &AuditCtx {
                store_id: &ctx.store_id,
                device_id: &ctx.device_id,
                business_date: &ctx.business_date,
                now: &ctx.now,
            },
            AuditEntry {
                action: AuditAction::PRINT_JOB_ABANDON,
                staff_id: Some(&actor.staff_id),
                approved_by: actor.approved_by.as_deref(),
                entity_type: Some("print_job"),
                entity_id: None,
                metadata: json!({ "reason": reason, "jobs": dropped }),
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(n)
}

/// Every unprinted job of one printer (what the banner's "Reprint later" dismisses).
pub async fn failed_ids(pool: &SqlitePool, printer_key: &str) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT id FROM print_jobs WHERE printer_key = ?1 AND status = 'failed' ORDER BY created_at")
        .bind(printer_key)
        .fetch_all(pool)
        .await?)
}
