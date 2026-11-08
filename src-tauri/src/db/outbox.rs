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
    ("orders", "id"),
    ("order_items", "id"),
    ("credit_notes", "id"),
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
