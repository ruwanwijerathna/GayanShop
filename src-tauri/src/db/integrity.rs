//! Startup integrity checks (ARCHITECTURE §1.1, §13.2): SQLite `quick_check`, the schema's integrity
//! probe views (which must be empty), and a stale sync-apply flag.

use serde::Serialize;
use specta::Type;
use sqlx::SqlitePool;

use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IntegrityReport {
    /// `"ok"` when SQLite's quick_check found nothing; otherwise its first message.
    pub quick_check: String,
    /// Orders committed while still `pending` (should be impossible; indicates a bug).
    pub pending_orders: u32,
    /// Voided order lines whose sale slices are not fully reversed.
    pub unreversed_voids: u32,
    /// `sync_ctx.applying` left at 1 outside a pull transaction.
    pub stale_sync_flag: bool,
}

impl IntegrityReport {
    pub fn healthy(&self) -> bool {
        self.quick_check == "ok" && self.pending_orders == 0 && self.unreversed_voids == 0 && !self.stale_sync_flag
    }
}

pub async fn check(pool: &SqlitePool) -> AppResult<IntegrityReport> {
    let quick_check: String = sqlx::query_scalar("PRAGMA quick_check").fetch_one(pool).await?;
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM v_integrity_pending_orders").fetch_one(pool).await?;
    let unreversed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM v_integrity_void_unreversed").fetch_one(pool).await?;
    let applying: i64 = sqlx::query_scalar("SELECT applying FROM sync_ctx WHERE id = 1").fetch_one(pool).await?;
    Ok(IntegrityReport {
        quick_check,
        pending_orders: u32::try_from(pending).unwrap_or(u32::MAX),
        unreversed_voids: u32::try_from(unreversed).unwrap_or(u32::MAX),
        stale_sync_flag: applying != 0,
    })
}
