//! The stock ledger (append-only `stock_movements`), FIFO layer selection for sales, and the rebuild of the
//! derived `stock_levels` / `batch_item_stock` caches (doc §3.1, §4.3).

use std::collections::HashMap;

use serde::Serialize;
use specta::Type;
use sqlx::{FromRow, SqliteConnection};

use super::WriteCtx;
use crate::clock::new_id;
use crate::db::outbox;
use crate::domain::fifo::{self, FifoError, Layer, Slice};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StockReason {
    BatchIntake,
    BatchReopen,
    Sale,
    Void,
    Return,
    Damage,
    CountCorrection,
    Adjustment,
}

impl StockReason {
    pub fn code(self) -> &'static str {
        match self {
            StockReason::BatchIntake => "batch_intake",
            StockReason::BatchReopen => "batch_reopen",
            StockReason::Sale => "sale",
            StockReason::Void => "void",
            StockReason::Return => "return",
            StockReason::Damage => "damage",
            StockReason::CountCorrection => "count_correction",
            StockReason::Adjustment => "adjustment",
        }
    }
}

/// One ledger row to append.
#[derive(Debug, Clone)]
pub struct Movement<'a> {
    pub product_id: &'a str,
    pub batch_item_id: &'a str,
    pub qty_delta: i64,
    pub reason: StockReason,
    pub ref_type: Option<&'a str>,
    pub ref_id: Option<&'a str>,
    pub order_item_id: Option<&'a str>,
    pub unit_cost: i64,
    pub revenue_net: Option<i64>,
    pub staff_id: Option<&'a str>,
    pub approved_by: Option<&'a str>,
    pub note: Option<&'a str>,
}

/// Append a movement and its outbox row. The derive trigger updates `stock_levels` and `batch_item_stock`;
/// the SQL guards enforce sale/void rules. Returns the movement id.
pub async fn record(conn: &mut SqliteConnection, ctx: &WriteCtx, m: &Movement<'_>) -> AppResult<String> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, ref_type,
                                      ref_id, order_item_id, unit_cost, revenue_net, business_date, staff_id,
                                      approved_by, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )
    .bind(&id)
    .bind(&ctx.store_id)
    .bind(&ctx.device_id)
    .bind(m.product_id)
    .bind(m.batch_item_id)
    .bind(m.qty_delta)
    .bind(m.reason.code())
    .bind(m.ref_type)
    .bind(m.ref_id)
    .bind(m.order_item_id)
    .bind(m.unit_cost)
    .bind(m.revenue_net)
    .bind(&ctx.business_date)
    .bind(m.staff_id)
    .bind(m.approved_by)
    .bind(m.note)
    .bind(&ctx.now)
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "stock_movements", &id, &ctx.now).await?;
    Ok(id)
}

#[derive(FromRow)]
struct LayerRow {
    batch_item_id: String,
    unit_cost: i64,
    on_hand: i64,
}

/// A product's cost layers in FIFO order: received (or closed) batches by receive time, then layer id.
pub async fn layers(conn: &mut SqliteConnection, product_id: &str) -> AppResult<Vec<Layer>> {
    let rows: Vec<LayerRow> = sqlx::query_as(
        "SELECT bi.id AS batch_item_id, bi.unit_cost, COALESCE(bis.on_hand, 0) AS on_hand
         FROM batch_items bi
         JOIN batches b ON b.id = bi.batch_id
         LEFT JOIN batch_item_stock bis ON bis.batch_item_id = bi.id
         WHERE bi.product_id = ?1 AND bi.deleted_at IS NULL AND b.deleted_at IS NULL
           AND b.status IN ('received', 'closed')
         ORDER BY b.received_at, bi.id",
    )
    .bind(product_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Layer { batch_item_id: r.batch_item_id, on_hand: r.on_hand, unit_cost: r.unit_cost })
        .collect())
}

/// Give a product with no cost layer one in the system `ADJUSTMENTS` batch, at its `default_cost` (doc §3.1:
/// every unit of tracked stock belongs to a layer). A sale must never fail for want of a batch; the layer
/// simply goes negative and shows up in "Oversold layers".
async fn ensure_adjustment_layer(conn: &mut SqliteConnection, ctx: &WriteCtx, product_id: &str) -> AppResult<()> {
    let batch_id: String = sqlx::query_scalar(
        "SELECT id FROM batches WHERE store_id = ?1 AND code = 'ADJUSTMENTS' AND is_system = 1 AND deleted_at IS NULL",
    )
    .bind(&ctx.store_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| AppError::internal("system batch ADJUSTMENTS is missing"))?;
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM batch_items WHERE batch_id = ?1 AND product_id = ?2")
            .bind(&batch_id)
            .bind(product_id)
            .fetch_optional(&mut *conn)
            .await?;
    let item_id = match existing {
        Some(id) => {
            sqlx::query(
                "UPDATE batch_items SET deleted_at = NULL, updated_at = ?1, updated_by_device = ?2, version = version + 1
                 WHERE id = ?3 AND deleted_at IS NOT NULL",
            )
            .bind(&ctx.now)
            .bind(&ctx.device_id)
            .bind(&id)
            .execute(&mut *conn)
            .await?;
            id
        }
        None => {
            let id = new_id();
            sqlx::query(
                "INSERT INTO batch_items (id, store_id, batch_id, product_id, qty_received, unit_cost,
                                          retail_price_at_intake, added_after_receipt, note,
                                          created_at, updated_at, updated_by_device)
                 SELECT ?1, ?2, ?3, p.id, 0, p.default_cost, p.price, 1, 'auto: no cost layer at first sale', ?4, ?4, ?5
                 FROM products p WHERE p.id = ?6",
            )
            .bind(&id)
            .bind(&ctx.store_id)
            .bind(&batch_id)
            .bind(&ctx.now)
            .bind(&ctx.device_id)
            .bind(product_id)
            .execute(&mut *conn)
            .await?;
            id
        }
    };
    outbox::enqueue_upsert(conn, "batch_items", &item_id, &ctx.now).await
}

/// The layer found stock goes into (doc §3.1): the product's newest layer at its cost, or a new one in the
/// system `ADJUSTMENTS` batch at the product's `default_cost` when it has none.
pub async fn newest_layer(conn: &mut SqliteConnection, ctx: &WriteCtx, product_id: &str) -> AppResult<Layer> {
    if let Some(l) = layers(conn, product_id).await?.pop() {
        return Ok(l);
    }
    ensure_adjustment_layer(conn, ctx, product_id).await?;
    layers(conn, product_id).await?.pop().ok_or_else(|| AppError::internal("no cost layer after creating one"))
}

fn fifo_err(e: FifoError) -> AppError {
    match e {
        FifoError::NonPositiveQty => AppError::validation("QTY_INVALID", e.to_string()),
        FifoError::NegativeRevenue => AppError::validation("AMOUNT_INVALID", e.to_string()),
        FifoError::NoLayers | FifoError::Overflow => AppError::internal(e.to_string()),
    }
}

/// FIFO slices for selling `qty` units of `product_id` with net revenue `revenue_net` (doc §3.1). Reads layer
/// stock inside the caller's write transaction, so concurrent checkouts on this counter can't double-count.
pub async fn plan_sale(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    product_id: &str,
    qty: i64,
    revenue_net: i64,
) -> AppResult<Vec<Slice>> {
    plan_sale_within(conn, ctx, product_id, qty, revenue_net, &mut HashMap::new()).await
}

/// [`plan_sale`] for one line of a multi-line checkout: `planned` (layer → units) holds what earlier lines
/// of the same checkout already took, since their slices aren't written yet. Updated with this line's slices.
pub async fn plan_sale_within(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    product_id: &str,
    qty: i64,
    revenue_net: i64,
    planned: &mut HashMap<String, i64>,
) -> AppResult<Vec<Slice>> {
    let mut found = layers(conn, product_id).await?;
    if found.is_empty() {
        ensure_adjustment_layer(conn, ctx, product_id).await?;
        found = layers(conn, product_id).await?;
    }
    for layer in &mut found {
        layer.on_hand -= planned.get(&layer.batch_item_id).copied().unwrap_or(0);
    }
    let slices = fifo::consume(&found, qty, revenue_net).map_err(fifo_err)?;
    for s in &slices {
        *planned.entry(s.batch_item_id.clone()).or_default() += s.qty;
    }
    Ok(slices)
}

/// Recompute `stock_levels` and `batch_item_stock` from the ledger (repair, restore, after a bootstrap). Must
/// match the incremental trigger exactly; `tests/stock_ledger.rs` checks that on random histories.
pub async fn rebuild_derived(conn: &mut SqliteConnection) -> AppResult<()> {
    sqlx::query("DELETE FROM stock_levels").execute(&mut *conn).await?;
    sqlx::query("DELETE FROM batch_item_stock").execute(&mut *conn).await?;
    sqlx::query(
        "INSERT INTO stock_levels (product_id, on_hand)
         SELECT product_id, SUM(qty_delta) FROM stock_movements GROUP BY product_id",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO batch_item_stock (batch_item_id, batch_id, product_id, received_qty, on_hand, sold_qty,
                                       sold_revenue, sold_cost, written_off_qty, written_off_cost)
         SELECT m.batch_item_id, bi.batch_id, m.product_id,
           SUM(CASE WHEN m.reason IN ('batch_intake','batch_reopen') THEN m.qty_delta ELSE 0 END),
           SUM(m.qty_delta),
           SUM(CASE WHEN m.reason IN ('sale','void','return') THEN -m.qty_delta ELSE 0 END),
           SUM(CASE WHEN m.reason IN ('sale','void','return') THEN COALESCE(m.revenue_net, 0) ELSE 0 END),
           SUM(CASE WHEN m.reason IN ('sale','void','return') THEN -m.qty_delta * m.unit_cost ELSE 0 END),
           SUM(CASE WHEN m.reason IN ('damage','count_correction','adjustment') THEN -m.qty_delta ELSE 0 END),
           SUM(CASE WHEN m.reason IN ('damage','count_correction','adjustment') THEN -m.qty_delta * m.unit_cost ELSE 0 END)
         FROM stock_movements m JOIN batch_items bi ON bi.id = m.batch_item_id
         GROUP BY m.batch_item_id",
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// One layer of a product's stock, for the product and batch screens.
#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct LayerStock {
    pub batch_item_id: String,
    pub batch_id: String,
    pub batch_code: String,
    pub received_at: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub unit_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub received_qty: i64,
    #[specta(type = specta_typescript::Number)]
    pub on_hand: i64,
    #[specta(type = specta_typescript::Number)]
    pub sold_qty: i64,
}

/// A product's layers (FIFO order) with their derived stock.
pub async fn product_layers(conn: &mut SqliteConnection, product_id: &str) -> AppResult<Vec<LayerStock>> {
    Ok(sqlx::query_as(
        "SELECT bi.id AS batch_item_id, b.id AS batch_id, b.code AS batch_code, b.received_at, bi.unit_cost,
                COALESCE(bis.received_qty, 0) AS received_qty, COALESCE(bis.on_hand, 0) AS on_hand,
                COALESCE(bis.sold_qty, 0) AS sold_qty
         FROM batch_items bi
         JOIN batches b ON b.id = bi.batch_id
         LEFT JOIN batch_item_stock bis ON bis.batch_item_id = bi.id
         WHERE bi.product_id = ?1 AND bi.deleted_at IS NULL AND b.deleted_at IS NULL
           AND b.status IN ('received', 'closed')
         ORDER BY b.received_at, bi.id",
    )
    .bind(product_id)
    .fetch_all(&mut *conn)
    .await?)
}
