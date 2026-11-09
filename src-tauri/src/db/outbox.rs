//! Transactional outbox (ARCHITECTURE §6.1): every synced write enqueues the full row, in the
//! same transaction as the write itself.

use sqlx::{Row, SqliteConnection};

use crate::clock::new_id;
use crate::error::{AppError, AppResult};

/// Tables that sync, with their primary-key column.
const SYNCED: &[(&str, &str)] = &[
    ("store_settings", "key"),
    ("roles", "id"),
    ("staff", "id"),
    ("categories", "id"),
    ("products", "id"),
    ("batches", "id"),
    ("batch_items", "id"),
    ("cash_sessions", "id"),
    ("cash_movements", "id"),
    ("customers", "id"),
    ("orders", "id"),
    ("order_items", "id"),
    ("order_payments", "id"),
    ("stock_movements", "id"),
    ("expense_categories", "id"),
    ("expenses", "id"),
    ("quick_sale_links", "id"),
    ("audit_logs", "id"),
];

fn pk_of(table: &str) -> AppResult<&'static str> {
    SYNCED
        .iter()
        .find(|(t, _)| *t == table)
        .map(|(_, pk)| *pk)
        .ok_or_else(|| AppError::internal(format!("{table} is not a synced table")))
}

/// Snapshot the current row as JSON and append an `upsert` op to the outbox.
pub async fn enqueue_upsert(conn: &mut SqliteConnection, table: &str, row_id: &str, now: &str) -> AppResult<()> {
    // Nothing is written under this counter's identity while a restore is pending (Block 4.1).
    if crate::restore::frozen(&mut *conn).await? {
        return Err(crate::restore::frozen_error());
    }
    let pk = pk_of(table)?;
    let cols: Vec<String> = sqlx::query("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
        .bind(table)
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|r| r.get::<String, _>(0))
        .collect();
    // Table and column names come from the whitelist above and the schema itself, never from input.
    let pairs = cols.iter().map(|c| format!("'{c}', \"{c}\"")).collect::<Vec<_>>().join(", ");
    let sql = format!("SELECT json_object({pairs}) FROM \"{table}\" WHERE \"{pk}\" = ?1");
    let payload: String = sqlx::query_scalar(&sql)
        .bind(row_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::internal(format!("outbox: {table}/{row_id} not found")))?;
    sqlx::query(
        "INSERT INTO sync_outbox (op_id, table_name, row_id, op, payload, status, next_attempt_at, created_at)
         VALUES (?1, ?2, ?3, 'upsert', ?4, 'pending', ?5, ?5)",
    )
    .bind(new_id())
    .bind(table)
    .bind(row_id)
    .bind(payload)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Drop the queued (not in-flight) snapshots of one row before enqueueing its newest one. An upsert carries
/// the whole row, so the newest snapshot supersedes them; used by PDPA erasure so no unsent copy of the
/// erased personal data stays on disk.
pub async fn supersede(conn: &mut SqliteConnection, table: &str, row_id: &str) -> AppResult<()> {
    pk_of(table)?;
    sqlx::query("DELETE FROM sync_outbox WHERE table_name = ?1 AND row_id = ?2 AND status <> 'in_flight'")
        .bind(table)
        .bind(row_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Move queued ops behind everything queued so far, keeping their snapshots (new op ids: none was sent).
/// Checkout uses it so a sealed order reaches the cloud before its lines, payments and stock slices, which
/// are written (and queued) before the order can be sealed: the server needs parents first (§6.1).
pub async fn requeue_after(conn: &mut SqliteConnection, seqs: &[i64]) -> AppResult<()> {
    for seq in seqs {
        sqlx::query(
            "INSERT INTO sync_outbox (op_id, table_name, row_id, op, payload, status, next_attempt_at, created_at)
             SELECT ?1, table_name, row_id, op, payload, status, next_attempt_at, created_at
             FROM sync_outbox WHERE seq = ?2 AND status = 'pending'",
        )
        .bind(new_id())
        .bind(seq)
        .execute(&mut *conn)
        .await?;
        sqlx::query("DELETE FROM sync_outbox WHERE seq = ?1 AND status = 'pending'")
            .bind(seq)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
