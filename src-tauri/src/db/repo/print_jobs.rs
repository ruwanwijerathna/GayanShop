//! The local print queue (doc §7.5): a job is inserted in the SAME transaction as the thing it prints, so a
//! committed sale/close always has its job and a rolled-back one never prints. The worker arrives in 1.7.

use serde_json::Value;
use sqlx::SqliteConnection;

use super::WriteCtx;
use crate::clock::new_id;
use crate::error::AppResult;

/// The receipt printer profile; the cash drawer hangs off it (RJ11, doc §7.3).
pub const RECEIPT_PRINTER: &str = "receipt";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintKind {
    Receipt,
    VoidSlip,
    ZReport,
    DrawerKick,
}

impl PrintKind {
    fn code(self) -> &'static str {
        match self {
            PrintKind::Receipt => "receipt",
            PrintKind::VoidSlip => "void_slip",
            PrintKind::ZReport => "z_report",
            PrintKind::DrawerKick => "drawer_kick",
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
