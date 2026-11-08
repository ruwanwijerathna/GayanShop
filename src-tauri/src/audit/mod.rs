//! Append-only audit trail, written inside the action's own transaction (ARCHITECTURE §3.6).

pub mod chain;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use crate::clock::new_id;
use crate::db::outbox;
use crate::error::AppResult;
use chain::{ChainFields, GENESIS_HASH, canonical_json, compute_hash};

#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum AuditAction {
    STORE_INITIALIZED,
    AUTH_LOGIN,
    AUTH_LOGOUT,
    AUTH_FAILED,
    AUTH_LOCKOUT,
    APPROVAL_GRANTED,
    APPROVAL_DENIED,
    CASH_SESSION_OPEN,
    CASH_SESSION_CLOSE,
    CASH_SESSION_FORCE_CLOSE,
    DRAWER_KICK,
    CASH_PAY_IN,
    CASH_PAY_OUT,
    PRICE_OVERRIDE,
    LINE_DISCOUNT,
    ORDER_DISCOUNT,
    VOID_ORDER,
    EXCHANGE_ORDER,
    EXCHANGE_LATE_APPROVED,
    CREDIT_NOTE_ISSUE,
    REPRINT_RECEIPT,
    STOCK_ADJUSTMENT,
    STOCK_DAMAGE,
    STOCK_COUNT,
    BATCH_CREATE,
    BATCH_RECEIVE,
    BATCH_REOPEN,
    BATCH_CLOSE,
    BATCH_LATE_ITEM,
    PRODUCT_CREATE,
    PRODUCT_UPDATE,
    PRODUCT_DELETE,
    BARCODE_CONFLICT_RESOLVE,
    QUICK_SALE_PROMOTE,
    QUICK_SALE_IGNORE,
    EXPENSE_CREATE,
    EXPENSE_UPDATE,
    EXPENSE_DELETE,
    SETTINGS_CHANGE,
    ROLE_CREATE,
    ROLE_UPDATE,
    USER_CREATE,
    USER_UPDATE,
    USER_DISABLE,
    USER_PIN_RESET,
    BACKUP_CREATE,
    BACKUP_RESTORE,
    DATA_EXPORT,
    SYNC_DEAD_OP_RESOLVE,
    DEVICE_REGISTER,
}

impl AuditAction {
    pub fn as_str(self) -> String {
        serde_json::to_value(self).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
    }
}

/// Where and when an audited action happens.
pub struct AuditCtx<'a> {
    pub store_id: &'a str,
    pub device_id: &'a str,
    pub business_date: &'a str,
    pub now: &'a str,
}

pub struct AuditEntry<'a> {
    pub action: AuditAction,
    pub staff_id: Option<&'a str>,
    pub approved_by: Option<&'a str>,
    pub entity_type: Option<&'a str>,
    pub entity_id: Option<&'a str>,
    pub metadata: Value,
}

/// Append one audit row (and its outbox entry). Must be called inside the action's transaction;
/// writes are serialised by `Db::write`, so reading the chain head here is race-free.
pub async fn record(conn: &mut SqliteConnection, ctx: &AuditCtx<'_>, e: AuditEntry<'_>) -> AppResult<String> {
    let head: Option<(i64, String)> =
        sqlx::query_as("SELECT device_seq, hash FROM audit_logs WHERE device_id = ?1 ORDER BY device_seq DESC LIMIT 1")
            .bind(ctx.device_id)
            .fetch_optional(&mut *conn)
            .await?;
    let (seq, prev_hash) = match head {
        Some((s, h)) => (s + 1, h),
        None => (1, GENESIS_HASH.to_owned()),
    };
    let id = new_id();
    let action = e.action.as_str();
    let metadata = canonical_json(&e.metadata);
    let hash = compute_hash(
        &prev_hash,
        &ChainFields {
            id: &id,
            store_id: ctx.store_id,
            device_id: ctx.device_id,
            device_seq: seq,
            staff_id: e.staff_id,
            approved_by: e.approved_by,
            action: &action,
            entity_type: e.entity_type,
            entity_id: e.entity_id,
            metadata: &metadata,
            business_date: ctx.business_date,
            created_at: ctx.now,
        },
    );
    sqlx::query(
        "INSERT INTO audit_logs (id, store_id, device_id, device_seq, staff_id, approved_by, action, entity_type,
           entity_id, metadata, business_date, prev_hash, hash, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )
    .bind(&id)
    .bind(ctx.store_id)
    .bind(ctx.device_id)
    .bind(seq)
    .bind(e.staff_id)
    .bind(e.approved_by)
    .bind(&action)
    .bind(e.entity_type)
    .bind(e.entity_id)
    .bind(&metadata)
    .bind(ctx.business_date)
    .bind(&prev_hash)
    .bind(&hash)
    .bind(ctx.now)
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "audit_logs", &id, ctx.now).await?;
    Ok(id)
}

#[derive(Debug, Clone, Serialize, Type, FromRow)]
pub struct AuditLogRow {
    pub id: String,
    pub device_id: String,
    pub device_seq: i64,
    pub staff_id: Option<String>,
    pub staff_name: Option<String>,
    pub approved_by: Option<String>,
    pub approver_name: Option<String>,
    pub action: String,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub metadata: String,
    pub business_date: String,
    pub created_at: String,
    pub hash: String,
}

pub async fn list(pool: &SqlitePool, limit: i64) -> AppResult<Vec<AuditLogRow>> {
    Ok(sqlx::query_as(
        "SELECT a.id, a.device_id, a.device_seq, a.staff_id, s.display_name AS staff_name, a.approved_by,
                ap.display_name AS approver_name, a.action, a.entity_type, a.entity_id, a.metadata,
                a.business_date, a.created_at, a.hash
         FROM audit_logs a
         LEFT JOIN staff s ON s.id = a.staff_id
         LEFT JOIN staff ap ON ap.id = a.approved_by
         ORDER BY a.created_at DESC, a.device_seq DESC
         LIMIT ?1",
    )
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ChainBreak {
    pub device_seq: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ChainReport {
    pub device_id: String,
    pub rows_checked: i64,
    pub ok: bool,
    pub first_break: Option<ChainBreak>,
}

#[derive(FromRow)]
struct ChainRow {
    id: String,
    store_id: String,
    device_id: String,
    device_seq: i64,
    staff_id: Option<String>,
    approved_by: Option<String>,
    action: String,
    entity_type: Option<String>,
    entity_id: Option<String>,
    metadata: String,
    business_date: String,
    prev_hash: String,
    hash: String,
    created_at: String,
}

/// Re-verify every device's chain: gapless sequence, linked hashes, recomputed hashes.
pub async fn verify(pool: &SqlitePool) -> AppResult<Vec<ChainReport>> {
    let rows: Vec<ChainRow> = sqlx::query_as("SELECT * FROM audit_logs ORDER BY device_id, device_seq").fetch_all(pool).await?;
    let mut reports: Vec<ChainReport> = Vec::new();
    let mut prev_hash = String::new();
    for r in &rows {
        if reports.last().map(|x| x.device_id != r.device_id).unwrap_or(true) {
            reports.push(ChainReport { device_id: r.device_id.clone(), rows_checked: 0, ok: true, first_break: None });
            prev_hash = GENESIS_HASH.to_owned();
        }
        let rep = reports.last_mut().expect("report exists");
        rep.rows_checked += 1;
        if !rep.ok {
            continue;
        }
        let expected_seq = rep.rows_checked;
        let reason = if r.device_seq != expected_seq {
            Some(format!("sequence gap: expected {expected_seq}, found {}", r.device_seq))
        } else if r.prev_hash != prev_hash {
            Some("prev_hash does not match the previous row".to_owned())
        } else {
            let recomputed = compute_hash(
                &r.prev_hash,
                &ChainFields {
                    id: &r.id,
                    store_id: &r.store_id,
                    device_id: &r.device_id,
                    device_seq: r.device_seq,
                    staff_id: r.staff_id.as_deref(),
                    approved_by: r.approved_by.as_deref(),
                    action: &r.action,
                    entity_type: r.entity_type.as_deref(),
                    entity_id: r.entity_id.as_deref(),
                    metadata: &r.metadata,
                    business_date: &r.business_date,
                    created_at: &r.created_at,
                },
            );
            (recomputed != r.hash).then(|| "row content does not match its hash (edited?)".to_owned())
        };
        if let Some(reason) = reason {
            rep.ok = false;
            rep.first_break = Some(ChainBreak { device_seq: r.device_seq, reason });
        }
        prev_hash = r.hash.clone();
    }
    Ok(reports)
}
