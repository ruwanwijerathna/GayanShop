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

/// Every audited action (doc §3.6). Stored and synced as the SCREAMING_SNAKE code.
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
    REPRINT_RECEIPT,
    PRINT_JOB_ABANDON,
    CUSTOMER_CREATE,
    CUSTOMER_UPDATE,
    CUSTOMER_ERASE,
    LABEL_PRINT,
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
    BACKUP_FAILED,
    BACKUP_RESTORE,
    DATA_EXPORT,
    SYNC_DEAD_OP_RESOLVE,
    DEVICE_REGISTER,
}

impl AuditAction {
    pub fn code(self) -> String {
        match serde_json::to_value(self) {
            Ok(Value::String(s)) => s,
            _ => format!("{self:?}"),
        }
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

/// Append one audit row and its outbox entry. Must be called inside the action's transaction (from
/// `Db::write()`); writes are serialised, so reading the chain head here is race-free. The SQL chain guard
/// re-checks the sequence and link.
pub async fn record(conn: &mut SqliteConnection, ctx: &AuditCtx<'_>, e: AuditEntry<'_>) -> AppResult<String> {
    // A pending restore freezes this counter's chain: its head is behind the cloud's (Block 4.1).
    if crate::restore::frozen(&mut *conn).await? {
        return Err(crate::restore::frozen_error());
    }
    let head: Option<(i64, String)> =
        sqlx::query_as("SELECT device_seq, hash FROM audit_logs WHERE device_id = ?1 ORDER BY device_seq DESC LIMIT 1")
            .bind(ctx.device_id)
            .fetch_optional(&mut *conn)
            .await?;
    let (seq, prev_hash) = head.map_or((1, GENESIS_HASH.to_owned()), |(s, h)| (s + 1, h));
    let id = new_id();
    let action = e.action.code();
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
    #[specta(type = specta_typescript::Number)]
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
}

/// Keyset page cursor: rows strictly older than `(created_at, id)`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AuditCursor {
    pub created_at: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AuditPage {
    pub rows: Vec<AuditLogRow>,
    pub next_cursor: Option<AuditCursor>,
}

/// One newest-first keyset page (walks `ix_audit_keyset`).
pub const SQL_PAGE: &str =
    "SELECT a.id, a.device_id, a.device_seq, a.staff_id, s.display_name AS staff_name, a.approved_by,
                ap.display_name AS approver_name, a.action, a.entity_type, a.entity_id, a.metadata,
                a.business_date, a.created_at
         FROM audit_logs a
         LEFT JOIN staff s ON s.id = a.staff_id
         LEFT JOIN staff ap ON ap.id = a.approved_by
         WHERE ?1 IS NULL OR (a.created_at, a.id) < (?1, ?2)
         ORDER BY a.created_at DESC, a.id DESC
         LIMIT ?3";

/// Newest-first keyset paging over `ix_audit_keyset` (ARCHITECTURE §1.4).
pub async fn page(pool: &SqlitePool, cursor: Option<&AuditCursor>, limit: u32) -> AppResult<AuditPage> {
    let limit = i64::from(limit.clamp(1, 500));
    let after_ts = cursor.map(|c| c.created_at.as_str());
    let after_id = cursor.map(|c| c.id.as_str());
    let rows: Vec<AuditLogRow> =
        sqlx::query_as(SQL_PAGE).bind(after_ts).bind(after_id).bind(limit).fetch_all(pool).await?;
    let next_cursor = if i64::try_from(rows.len()).unwrap_or(i64::MAX) == limit {
        rows.last().map(|r| AuditCursor { created_at: r.created_at.clone(), id: r.id.clone() })
    } else {
        None
    };
    Ok(AuditPage { rows, next_cursor })
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ChainBreak {
    #[specta(type = specta_typescript::Number)]
    pub device_seq: i64,
    pub reason: ChainBreakReason,
}

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChainBreakReason {
    SequenceGap,
    PrevHashMismatch,
    ContentMismatch,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ChainReport {
    pub device_id: String,
    #[specta(type = specta_typescript::Number)]
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

fn check_row(r: &ChainRow, expected_seq: i64, prev_hash: &str) -> Option<ChainBreakReason> {
    if r.device_seq != expected_seq {
        return Some(ChainBreakReason::SequenceGap);
    }
    if r.prev_hash != prev_hash {
        return Some(ChainBreakReason::PrevHashMismatch);
    }
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
    (recomputed != r.hash).then_some(ChainBreakReason::ContentMismatch)
}

/// Re-verify every device's chain: gapless sequence from 1, linked hashes, recomputed hashes.
pub async fn verify(pool: &SqlitePool) -> AppResult<Vec<ChainReport>> {
    let rows: Vec<ChainRow> =
        sqlx::query_as("SELECT * FROM audit_logs ORDER BY device_id, device_seq").fetch_all(pool).await?;
    let mut reports: Vec<ChainReport> = Vec::new();
    let mut current: Option<ChainReport> = None;
    let mut prev_hash = String::new();
    for r in &rows {
        if current.as_ref().is_none_or(|c| c.device_id != r.device_id) {
            if let Some(done) = current.take() {
                reports.push(done);
            }
            current =
                Some(ChainReport { device_id: r.device_id.clone(), rows_checked: 0, ok: true, first_break: None });
            prev_hash = GENESIS_HASH.to_owned();
        }
        if let Some(rep) = current.as_mut() {
            rep.rows_checked += 1;
            if rep.ok
                && let Some(reason) = check_row(r, rep.rows_checked, &prev_hash)
            {
                rep.ok = false;
                rep.first_break = Some(ChainBreak { device_seq: r.device_seq, reason });
            }
        }
        prev_hash.clone_from(&r.hash);
    }
    if let Some(done) = current {
        reports.push(done);
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_codes_are_screaming_snake() {
        assert_eq!(AuditAction::STORE_INITIALIZED.code(), "STORE_INITIALIZED");
        assert_eq!(AuditAction::VOID_ORDER.code(), "VOID_ORDER");
        assert_eq!(AuditAction::BACKUP_FAILED.code(), "BACKUP_FAILED");
    }
}
