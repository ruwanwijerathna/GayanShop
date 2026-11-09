//! Applying a pull page (ARCHITECTURE §6.2, §6.3). One transaction through the single writer:
//!
//! * `sync_ctx.applying = 1` for its duration, so the **policy** triggers stand down for rows already
//!   validated on their origin and by the server. Structural and derived triggers keep running: the
//!   rollups, stock caches and customer stats update exactly as for a local sale.
//! * `PRAGMA defer_foreign_keys = ON`, and tables applied parents-first, so order within a page doesn't
//!   matter; the server's parent closure guarantees every foreign key resolves by commit.
//! * Insert-only rows are inserted once; orders are inserted once and may then move completed → voided;
//!   mutable rows are upserted by last-write-wins on `(updated_at, updated_by_device)`, except that an
//!   erasure always wins and the cloud's version replaces a local change the cloud refused.
//! * Nothing is enqueued to the outbox (no echo) and no audit row is written; pulled audit rows are stored
//!   as they are. The one exception is a barcode collision (§6.3): the local product that loses its barcode
//!   is deactivated, flagged, and pushed like any local edit.
//!
//! The cursor advances in the same transaction, so a page is applied exactly once or not at all.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::{Row, SqliteConnection};

use super::wire::PullPage;
use crate::db::{Db, outbox, sync_ctx};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Same id ⇒ no-op.
    InsertOnly,
    /// Inserted once; the only later change is completed → voided.
    Order,
    /// Whole-row last-write-wins by `(updated_at, updated_by_device)`.
    Lww,
}

/// The synced tables in apply order (parents first), with their merge kind and primary key. Mirrors
/// `pos.synced_tables` in the cloud schema.
pub const TABLES: &[(&str, Kind, &str)] = &[
    ("store_settings", Kind::Lww, "key"),
    ("roles", Kind::Lww, "id"),
    ("staff", Kind::Lww, "id"),
    ("categories", Kind::Lww, "id"),
    ("products", Kind::Lww, "id"),
    ("batches", Kind::Lww, "id"),
    ("batch_items", Kind::Lww, "id"),
    ("cash_sessions", Kind::Lww, "id"),
    ("cash_movements", Kind::InsertOnly, "id"),
    ("customers", Kind::Lww, "id"),
    ("orders", Kind::Order, "id"),
    ("order_items", Kind::InsertOnly, "id"),
    ("order_payments", Kind::InsertOnly, "id"),
    ("stock_movements", Kind::InsertOnly, "id"),
    ("expense_categories", Kind::Lww, "id"),
    ("expenses", Kind::Lww, "id"),
    ("quick_sale_links", Kind::Lww, "id"),
    ("audit_logs", Kind::InsertOnly, "id"),
];

fn table_meta(table: &str) -> Option<(usize, Kind, &'static str, &'static str)> {
    TABLES.iter().enumerate().find(|(_, (t, _, _))| *t == table).map(|(i, (t, k, pk))| (i, *k, *t, *pk))
}

/// A pulled row that could not be stored because it collides with a different local row on a unique key
/// other than its id (two counters created the same role name or batch code offline). The row is skipped,
/// the page still applies, and the collision is recorded in `sync_logs` for the owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Collision {
    pub table: String,
    pub id: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Applied {
    /// Rows in the page.
    pub received: usize,
    pub collisions: Vec<Collision>,
    /// The tables the page touched (the UI refreshes them).
    pub tables: Vec<&'static str>,
    /// Local products deactivated because a pulled product took their barcode.
    pub barcode_conflicts: usize,
}

fn bind_value<'q>(
    q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    v: &Value,
) -> AppResult<sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>> {
    Ok(match v {
        Value::Null => q.bind(None::<i64>),
        Value::Bool(b) => q.bind(i64::from(*b)),
        Value::Number(n) => {
            let i = n.as_i64().ok_or_else(|| AppError::internal(format!("sync: non-integer number {n} in a row")))?;
            q.bind(i)
        }
        Value::String(s) => q.bind(s.clone()),
        other => q.bind(other.to_string()),
    })
}

/// The columns of a local table, in table order.
async fn local_columns(conn: &mut SqliteConnection, table: &str) -> AppResult<Vec<String>> {
    let rows =
        sqlx::query("SELECT name FROM pragma_table_info(?1) ORDER BY cid").bind(table).fetch_all(&mut *conn).await?;
    Ok(rows.iter().map(|r| r.get::<String, _>(0)).collect())
}

fn text<'a>(row: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.message().contains("UNIQUE constraint failed"))
}

/// `(updated_at, updated_by_device)` of the local copy, if there is one.
async fn local_version(
    conn: &mut SqliteConnection,
    table: &str,
    pk: &str,
    id: &str,
) -> AppResult<Option<(String, String)>> {
    let sql = format!("SELECT updated_at, updated_by_device FROM \"{table}\" WHERE \"{pk}\" = ?1");
    Ok(sqlx::query_as(&sql).bind(id).fetch_optional(&mut *conn).await?)
}

/// Last-write-wins: does `(at, dev)` beat `(cur_at, cur_dev)`? Timestamps share one fixed format
/// (`clock::iso`, which the server reproduces), so text order is time order. A row written twice by one
/// device within the same millisecond (create then receive a batch) arrives as two snapshots with the same
/// stamp: the later one wins, because one device's changes always arrive in order.
fn is_newer(at: &str, dev: &str, cur_at: &str, cur_dev: &str) -> bool {
    at > cur_at || (at == cur_at && dev >= cur_dev)
}

struct Applier<'c> {
    conn: &'c mut SqliteConnection,
    me: &'c str,
    now: &'c str,
    columns: HashMap<&'static str, Vec<String>>,
    out: Applied,
}

impl Applier<'_> {
    async fn columns_of(&mut self, table: &'static str) -> AppResult<Vec<String>> {
        if let Some(c) = self.columns.get(table) {
            return Ok(c.clone());
        }
        let c = local_columns(self.conn, table).await?;
        self.columns.insert(table, c.clone());
        Ok(c)
    }

    /// `INSERT INTO t (cols) VALUES (…)` over the row's columns that exist locally, plus `tail`.
    async fn insert(&mut self, table: &'static str, row: &Map<String, Value>, tail: &str) -> Result<u64, sqlx::Error> {
        let cols: Vec<String> = match self.columns_of(table).await {
            Ok(c) => c.into_iter().filter(|c| row.contains_key(c)).collect(),
            Err(e) => return Err(sqlx::Error::Protocol(e.to_string())),
        };
        let names = cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
        let marks = (1..=cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
        let tail = tail.replace(
            "{SET}",
            &cols.iter().map(|c| format!("\"{c}\" = excluded.\"{c}\"")).collect::<Vec<_>>().join(", "),
        );
        // Table and column names come from the TABLES whitelist and the local schema, never from the wire.
        let sql = format!("INSERT INTO \"{table}\" ({names}) VALUES ({marks}) {tail}");
        let mut q = sqlx::query(&sql);
        for c in &cols {
            q = bind_value(q, &row[c]).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        }
        Ok(q.execute(&mut *self.conn).await?.rows_affected())
    }

    async fn apply(
        &mut self,
        table: &'static str,
        kind: Kind,
        pk: &'static str,
        row: &Map<String, Value>,
    ) -> AppResult<()> {
        let id = text(row, pk).ok_or_else(|| AppError::internal(format!("sync: {table} row without {pk}")))?.to_owned();
        match kind {
            Kind::InsertOnly => {
                self.insert(table, row, &format!("ON CONFLICT(\"{pk}\") DO NOTHING")).await?;
            }
            Kind::Order => {
                self.insert(table, row, "ON CONFLICT(\"id\") DO NOTHING").await?;
                if text(row, "status") == Some("voided") {
                    // The one transition a sold order makes. The state-machine trigger still checks that
                    // nothing else changes; the derived triggers reverse the rollups.
                    let mut q = sqlx::query(
                        "UPDATE orders SET status = 'voided', voided_at = ?1, voided_by = ?2, void_approved_by = ?3,
                           void_reason = ?4, void_note = ?5, updated_at = ?6, updated_by_device = ?7, version = ?8
                         WHERE id = ?9 AND status = 'completed'",
                    );
                    for key in [
                        "voided_at",
                        "voided_by",
                        "void_approved_by",
                        "void_reason",
                        "void_note",
                        "updated_at",
                        "updated_by_device",
                        "version",
                    ] {
                        q = bind_value(q, row.get(key).unwrap_or(&Value::Null))?;
                    }
                    q.bind(&id).execute(&mut *self.conn).await?;
                }
            }
            Kind::Lww => self.apply_lww(table, pk, &id, row).await?,
        }
        Ok(())
    }

    async fn apply_lww(
        &mut self,
        table: &'static str,
        pk: &'static str,
        id: &str,
        row: &Map<String, Value>,
    ) -> AppResult<()> {
        let at = text(row, "updated_at").unwrap_or_default().to_owned();
        let dev = text(row, "updated_by_device").unwrap_or_default().to_owned();
        let local = local_version(self.conn, table, pk, id).await?;
        // An erasure wins over any concurrent edit, whichever is newer (§6.3): a pulled erasure always
        // applies, and a pulled edit never re-populates an erased customer.
        let mut erasure = false;
        if table == "customers" && local.is_some() {
            let erased: Option<String> = sqlx::query_scalar("SELECT erased_at FROM customers WHERE id = ?1")
                .bind(id)
                .fetch_one(&mut *self.conn)
                .await?;
            let erasing = !row.get("erased_at").is_none_or(Value::is_null);
            if erased.is_some() && !erasing {
                return Ok(());
            }
            erasure = erasing && erased.is_none();
        }
        // A local change the cloud refused (its op is dead) gives way to the cloud's version, which the
        // server re-sends when it refuses a change: this counter must not keep a row the cloud rejected.
        let refused = local.is_some() && {
            let n: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sync_outbox WHERE table_name = ?1 AND row_id = ?2 AND status = 'dead'",
            )
            .bind(table)
            .bind(id)
            .fetch_one(&mut *self.conn)
            .await?;
            n > 0
        };
        if !erasure
            && !refused
            && let Some((cur_at, cur_dev)) = &local
            && !is_newer(&at, &dev, cur_at, cur_dev)
        {
            return Ok(()); // the local copy is newer (or this is our own write coming back)
        }
        if table == "products"
            && row.get("is_active").and_then(Value::as_i64) == Some(1)
            && row.get("deleted_at").is_none_or(Value::is_null)
            && let Some(barcode) = text(row, "barcode")
        {
            self.yield_barcode(id, text(row, "store_id").unwrap_or_default(), barcode).await?;
        }
        // Savepoint per row: a collision on another unique key skips this row, not the page.
        sqlx::query("SAVEPOINT lww_row").execute(&mut *self.conn).await?;
        let tail = if erasure || refused {
            format!("ON CONFLICT(\"{pk}\") DO UPDATE SET {{SET}}")
        } else {
            format!(
                "ON CONFLICT(\"{pk}\") DO UPDATE SET {{SET}}
                 WHERE excluded.updated_at > \"{table}\".updated_at
                    OR (excluded.updated_at = \"{table}\".updated_at AND excluded.updated_by_device >= \"{table}\".updated_by_device)"
            )
        };
        match self.insert(table, row, &tail).await {
            Ok(_) => {
                sqlx::query("RELEASE lww_row").execute(&mut *self.conn).await?;
            }
            Err(e) if is_unique_violation(&e) => {
                sqlx::query("ROLLBACK TO lww_row").execute(&mut *self.conn).await?;
                sqlx::query("RELEASE lww_row").execute(&mut *self.conn).await?;
                self.out.collisions.push(Collision {
                    table: table.to_owned(),
                    id: id.to_owned(),
                    detail: e.to_string(),
                });
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    /// A pulled active product takes a barcode another local product holds (two counters registered it
    /// offline): the local one steps aside — inactive, flagged — and that change is pushed (§6.3).
    async fn yield_barcode(&mut self, id: &str, store_id: &str, barcode: &str) -> AppResult<()> {
        let others: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM products
             WHERE store_id = ?1 AND barcode = ?2 AND id <> ?3 AND is_active = 1 AND deleted_at IS NULL",
        )
        .bind(store_id)
        .bind(barcode)
        .bind(id)
        .fetch_all(&mut *self.conn)
        .await?;
        for other in others {
            sqlx::query(
                "UPDATE products SET is_active = 0, barcode_conflict = 1, updated_at = ?1, updated_by_device = ?2,
                   version = version + 1 WHERE id = ?3",
            )
            .bind(self.now)
            .bind(self.me)
            .bind(&other)
            .execute(&mut *self.conn)
            .await?;
            outbox::enqueue_upsert(self.conn, "products", &other, self.now).await?;
            self.out.barcode_conflicts += 1;
        }
        Ok(())
    }
}

pub const CURSOR_KEY: &str = "__global__";

pub async fn cursor(db: &Db) -> AppResult<i64> {
    let c: Option<i64> = sqlx::query_scalar("SELECT last_server_seq FROM sync_state WHERE table_name = ?1")
        .bind(CURSOR_KEY)
        .fetch_optional(&db.pool)
        .await?;
    Ok(c.unwrap_or(0))
}

/// Apply one page and advance the cursor, atomically. `me` is this device; `now` stamps the rare local
/// change a page causes (a barcode collision).
pub async fn apply_page(db: &Db, me: &str, now: &str, page: &PullPage) -> AppResult<Applied> {
    let mut rows = Vec::with_capacity(page.rows.len());
    for r in &page.rows {
        let (order, kind, table, pk) =
            table_meta(&r.table).ok_or_else(|| AppError::internal(format!("sync: unknown table {}", r.table)))?;
        let obj =
            r.row.as_object().ok_or_else(|| AppError::internal(format!("sync: {} row is not an object", r.table)))?;
        rows.push((order, r.seq, kind, table, pk, obj));
    }
    // Parents first; within a table, server order.
    rows.sort_by_key(|(order, seq, ..)| (*order, *seq));

    let mut tx = db.write().await?;
    let conn = tx.conn();
    sqlx::query("PRAGMA defer_foreign_keys = ON").execute(&mut *conn).await?;
    sync_ctx::begin_applying(conn).await?;
    let mut applier = Applier { conn, me, now, columns: HashMap::new(), out: Applied::default() };
    for (_, _, kind, table, pk, row) in &rows {
        applier.apply(table, *kind, pk, row).await?;
        if !applier.out.tables.contains(table) {
            applier.out.tables.push(table);
        }
    }
    let mut out = applier.out;
    out.received = rows.len();
    sqlx::query(
        "INSERT INTO sync_state (table_name, last_server_seq, last_pulled_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(table_name) DO UPDATE SET last_server_seq = excluded.last_server_seq,
           last_pulled_at = excluded.last_pulled_at",
    )
    .bind(CURSOR_KEY)
    .bind(page.cursor)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    sync_ctx::end_applying(conn).await?;
    tx.commit().await?;
    Ok(out)
}
