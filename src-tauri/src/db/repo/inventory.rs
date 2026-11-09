//! Stock operations (doc §3.1, §5): damage, adjustments, stock counts, movement history, valuation (now and
//! as of a date, by batch and by category) and oversold layers.
//!
//! Every write is a ledger movement — never an `UPDATE` of stock:
//! - Removals (damage, a negative adjustment, a short count) take units FIFO, oldest layer with stock first,
//!   at each layer's cost.
//! - Found stock (a positive adjustment or count) goes into the product's **newest** layer at that layer's
//!   cost, or into the system `ADJUSTMENTS` batch at the product's `default_cost` if it has no layer.
//! - Write-offs show up in the batch P&L (`written_off_qty` / `written_off_cost`) and the store's COGS.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use super::stock::{self, Movement, StockReason};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::db::Db;
use crate::error::{AppError, AppResult};

const QTY_MAX: i64 = 1_000_000;
const NOTE_MAX: usize = 200;
const COUNT_LINES_MAX: usize = 2_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DamageReason {
    Broken,
    Expired,
    Stolen,
    Faulty,
    Other,
}

impl DamageReason {
    fn code(self) -> &'static str {
        match self {
            DamageReason::Broken => "broken",
            DamageReason::Expired => "expired",
            DamageReason::Stolen => "stolen",
            DamageReason::Faulty => "faulty",
            DamageReason::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct DamageInput {
    pub product_id: String,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
    pub reason: DamageReason,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct AdjustmentInput {
    pub product_id: String,
    /// Positive = found / added, negative = removed.
    #[specta(type = specta_typescript::Number)]
    pub qty_delta: i64,
    /// Required: an adjustment always says why.
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CountLine {
    pub product_id: String,
    #[specta(type = specta_typescript::Number)]
    pub counted: i64,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CountInput {
    pub lines: Vec<CountLine>,
    pub note: Option<String>,
}

/// What a write did: the units and their cost, per layer touched.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct StockChange {
    pub product_id: String,
    #[specta(type = specta_typescript::Number)]
    pub qty_delta: i64,
    /// Σ qty × layer cost (negative for a write-off).
    #[specta(type = specta_typescript::Number)]
    pub cost_delta: i64,
    #[specta(type = specta_typescript::Number)]
    pub on_hand_after: i64,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CountResult {
    pub changes: Vec<StockChange>,
    /// Lines whose count matched the system: nothing written for them.
    #[specta(type = specta_typescript::Number)]
    pub unchanged: i64,
}

fn audit_ctx(ctx: &WriteCtx) -> AuditCtx<'_> {
    AuditCtx { store_id: &ctx.store_id, device_id: &ctx.device_id, business_date: &ctx.business_date, now: &ctx.now }
}

fn clean_note(note: Option<&str>, required: bool) -> AppResult<Option<String>> {
    let n = note.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    if required && n.is_none() {
        return Err(AppError::validation("STOCK_NOTE_REQUIRED", "say why"));
    }
    if n.as_ref().is_some_and(|s| s.chars().count() > NOTE_MAX) {
        return Err(AppError::validation("TEXT_TOO_LONG", format!("max {NOTE_MAX}")));
    }
    Ok(n)
}

async fn tracked(conn: &mut SqliteConnection, store_id: &str, product_id: &str) -> AppResult<()> {
    let row: Option<bool> =
        sqlx::query_scalar("SELECT track_stock FROM products WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL")
            .bind(product_id)
            .bind(store_id)
            .fetch_optional(&mut *conn)
            .await?;
    match row {
        None => Err(AppError::not_found("product")),
        Some(false) => Err(AppError::validation("PRODUCT_NOT_TRACKED", product_id.to_owned())),
        Some(true) => Ok(()),
    }
}

async fn on_hand(conn: &mut SqliteConnection, product_id: &str) -> AppResult<i64> {
    Ok(sqlx::query_scalar("SELECT COALESCE((SELECT on_hand FROM stock_levels WHERE product_id = ?1), 0)")
        .bind(product_id)
        .fetch_one(&mut *conn)
        .await?)
}

struct Write<'a> {
    reason: StockReason,
    ref_type: &'a str,
    note: Option<&'a str>,
}

/// Apply a signed quantity change to one product as ledger movements; returns the change with its cost.
async fn apply(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    actor: &Actor,
    product_id: &str,
    qty_delta: i64,
    w: &Write<'_>,
) -> AppResult<StockChange> {
    let mut cost_delta = 0;
    if qty_delta < 0 {
        // FIFO, at each layer's cost (the sale planner with zero revenue).
        for s in stock::plan_sale(conn, ctx, product_id, -qty_delta, 0).await? {
            stock::record(
                conn,
                ctx,
                &Movement {
                    product_id,
                    batch_item_id: &s.batch_item_id,
                    qty_delta: -s.qty,
                    reason: w.reason,
                    ref_type: Some(w.ref_type),
                    ref_id: None,
                    order_item_id: None,
                    unit_cost: s.unit_cost,
                    revenue_net: None,
                    staff_id: Some(&actor.staff_id),
                    approved_by: actor.approved_by.as_deref(),
                    note: w.note,
                },
            )
            .await?;
            cost_delta -= s.qty * s.unit_cost;
        }
    } else if qty_delta > 0 {
        let layer = stock::newest_layer(conn, ctx, product_id).await?;
        stock::record(
            conn,
            ctx,
            &Movement {
                product_id,
                batch_item_id: &layer.batch_item_id,
                qty_delta,
                reason: w.reason,
                ref_type: Some(w.ref_type),
                ref_id: None,
                order_item_id: None,
                unit_cost: layer.unit_cost,
                revenue_net: None,
                staff_id: Some(&actor.staff_id),
                approved_by: actor.approved_by.as_deref(),
                note: w.note,
            },
        )
        .await?;
        cost_delta = qty_delta * layer.unit_cost;
    }
    Ok(StockChange {
        product_id: product_id.to_owned(),
        qty_delta,
        cost_delta,
        on_hand_after: on_hand(conn, product_id).await?,
    })
}

/// Damaged, expired, stolen or faulty units (`STOCK_DAMAGE`). Can't remove more than is on hand.
pub async fn damage(db: &Db, ctx: &WriteCtx, actor: &Actor, input: &DamageInput) -> AppResult<StockChange> {
    if !(1..=QTY_MAX).contains(&input.qty) {
        return Err(AppError::validation("QTY_INVALID", input.qty.to_string()));
    }
    let note = clean_note(input.note.as_deref(), input.reason == DamageReason::Other)?;
    let mut tx = db.write().await?;
    let conn = tx.conn();
    tracked(conn, &ctx.store_id, &input.product_id).await?;
    let have = on_hand(conn, &input.product_id).await?;
    if input.qty > have {
        return Err(AppError::validation("STOCK_INSUFFICIENT", have.to_string()));
    }
    let text = match &note {
        Some(n) => format!("{}: {n}", input.reason.code()),
        None => input.reason.code().to_owned(),
    };
    let w = Write { reason: StockReason::Damage, ref_type: "damage", note: Some(&text) };
    let change = apply(conn, ctx, actor, &input.product_id, -input.qty, &w).await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::STOCK_DAMAGE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("product"),
            entity_id: Some(&input.product_id),
            metadata: json!({ "qty": input.qty, "cost": -change.cost_delta, "reason": input.reason.code(), "note": note }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(change)
}

/// A one-off correction with a reason (`STOCK_ADJUST`): found stock in, or units out (not below zero).
pub async fn adjust(db: &Db, ctx: &WriteCtx, actor: &Actor, input: &AdjustmentInput) -> AppResult<StockChange> {
    if input.qty_delta == 0 || input.qty_delta.abs() > QTY_MAX {
        return Err(AppError::validation("QTY_INVALID", input.qty_delta.to_string()));
    }
    let note = clean_note(Some(&input.note), true)?;
    let mut tx = db.write().await?;
    let conn = tx.conn();
    tracked(conn, &ctx.store_id, &input.product_id).await?;
    let have = on_hand(conn, &input.product_id).await?;
    if input.qty_delta < 0 && -input.qty_delta > have {
        return Err(AppError::validation("STOCK_INSUFFICIENT", have.to_string()));
    }
    let w = Write { reason: StockReason::Adjustment, ref_type: "adjustment", note: note.as_deref() };
    let change = apply(conn, ctx, actor, &input.product_id, input.qty_delta, &w).await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::STOCK_ADJUSTMENT,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("product"),
            entity_id: Some(&input.product_id),
            metadata: json!({ "qty_delta": input.qty_delta, "cost_delta": change.cost_delta, "before": have,
                              "after": change.on_hand_after, "note": note }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(change)
}

/// A stock count (`STOCK_ADJUST`): each counted product is set to what's on the shelf with `count_correction`
/// movements, in one transaction, audited once as `STOCK_COUNT`. Matching lines write nothing.
pub async fn count(db: &Db, ctx: &WriteCtx, actor: &Actor, input: &CountInput) -> AppResult<CountResult> {
    if input.lines.is_empty() || input.lines.len() > COUNT_LINES_MAX {
        return Err(AppError::validation("COUNT_EMPTY", input.lines.len().to_string()));
    }
    let note = clean_note(input.note.as_deref(), false)?;
    let mut seen = BTreeMap::new();
    for l in &input.lines {
        if !(0..=QTY_MAX).contains(&l.counted) {
            return Err(AppError::validation("QTY_INVALID", l.counted.to_string()));
        }
        if seen.insert(l.product_id.clone(), l.counted).is_some() {
            return Err(AppError::validation("COUNT_DUPLICATE", l.product_id.clone()));
        }
    }
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let mut changes = Vec::new();
    let mut unchanged = 0;
    let mut lines_audit = Vec::new();
    let w = Write { reason: StockReason::CountCorrection, ref_type: "count", note: note.as_deref() };
    for l in &input.lines {
        tracked(conn, &ctx.store_id, &l.product_id).await?;
        let system = on_hand(conn, &l.product_id).await?;
        let delta = l.counted - system;
        lines_audit.push(json!({ "product_id": l.product_id, "system": system, "counted": l.counted }));
        if delta == 0 {
            unchanged += 1;
            continue;
        }
        changes.push(apply(conn, ctx, actor, &l.product_id, delta, &w).await?);
    }
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::STOCK_COUNT,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("stock_count"),
            entity_id: None,
            metadata: json!({ "lines": lines_audit, "corrected": changes.len(),
                              "cost_delta": changes.iter().map(|c| c.cost_delta).sum::<i64>(), "note": note }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(CountResult { changes, unchanged })
}

/// Write off whatever a received batch still holds (so it can be closed), as `adjustment` movements on its
/// own layers, audited as `STOCK_ADJUSTMENT`.
pub async fn write_off_batch(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    batch_id: &str,
    note: &str,
) -> AppResult<Vec<StockChange>> {
    let note = clean_note(Some(note), true)?;
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let status: Option<(String, bool)> =
        sqlx::query_as("SELECT status, is_system FROM batches WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL")
            .bind(batch_id)
            .bind(&ctx.store_id)
            .fetch_optional(&mut *conn)
            .await?;
    match status {
        None => return Err(AppError::not_found("batch")),
        Some((_, true)) => return Err(AppError::validation("BATCH_SYSTEM", batch_id.to_owned())),
        Some((s, _)) if s != "received" => return Err(AppError::validation("BATCH_NOT_RECEIVED", batch_id.to_owned())),
        _ => {}
    }
    let layers: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT bis.batch_item_id, bis.product_id, bi.unit_cost, bis.on_hand
         FROM batch_item_stock bis JOIN batch_items bi ON bi.id = bis.batch_item_id
         WHERE bis.batch_id = ?1 AND bis.on_hand > 0 ORDER BY bis.batch_item_id",
    )
    .bind(batch_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut changes = Vec::new();
    for (item, product, cost, qty) in &layers {
        stock::record(
            conn,
            ctx,
            &Movement {
                product_id: product,
                batch_item_id: item,
                qty_delta: -qty,
                reason: StockReason::Adjustment,
                ref_type: Some("batch_write_off"),
                ref_id: Some(batch_id),
                order_item_id: None,
                unit_cost: *cost,
                revenue_net: None,
                staff_id: Some(&actor.staff_id),
                approved_by: actor.approved_by.as_deref(),
                note: note.as_deref(),
            },
        )
        .await?;
        changes.push(StockChange {
            product_id: product.clone(),
            qty_delta: -qty,
            cost_delta: -qty * cost,
            on_hand_after: on_hand(conn, product).await?,
        });
    }
    if !changes.is_empty() {
        audit::record(
            conn,
            &audit_ctx(ctx),
            AuditEntry {
                action: AuditAction::STOCK_ADJUSTMENT,
                staff_id: Some(&actor.staff_id),
                approved_by: actor.approved_by.as_deref(),
                entity_type: Some("batch"),
                entity_id: Some(batch_id),
                metadata: json!({ "write_off": true, "units": changes.iter().map(|c| -c.qty_delta).sum::<i64>(),
                                  "cost": changes.iter().map(|c| -c.cost_delta).sum::<i64>(), "note": note }),
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(changes)
}

// ───────── reads ─────────

/// One ledger row, for a product's (or one layer's) movement history.
#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct MovementRow {
    pub id: String,
    pub created_at: String,
    pub business_date: String,
    pub reason: String,
    #[specta(type = specta_typescript::Number)]
    pub qty_delta: i64,
    #[specta(type = specta_typescript::Number)]
    pub unit_cost: i64,
    pub batch_item_id: String,
    pub batch_code: String,
    pub ref_type: Option<String>,
    pub ref_id: Option<String>,
    pub note: Option<String>,
    pub staff_name: Option<String>,
    pub approved_by_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct MovementCursor {
    pub created_at: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct MovementPage {
    pub rows: Vec<MovementRow>,
    pub next_cursor: Option<(String, String)>,
}

/// Newest first, keyset-paged (doc §1.4: unbounded lists page on the server). `layer` narrows to one layer.
pub async fn movements(
    pool: &SqlitePool,
    product_id: &str,
    layer: Option<&str>,
    cursor: Option<&MovementCursor>,
    limit: u32,
) -> AppResult<MovementPage> {
    let limit = i64::from(limit.clamp(1, 500));
    let rows: Vec<MovementRow> = sqlx::query_as(
        "SELECT m.id, m.created_at, m.business_date, m.reason, m.qty_delta, m.unit_cost, m.batch_item_id,
                b.code AS batch_code, m.ref_type, m.ref_id, m.note, s.display_name AS staff_name,
                a.display_name AS approved_by_name
         FROM stock_movements m
         JOIN batch_items bi ON bi.id = m.batch_item_id
         JOIN batches b ON b.id = bi.batch_id
         LEFT JOIN staff s ON s.id = m.staff_id
         LEFT JOIN staff a ON a.id = m.approved_by
         WHERE m.product_id = ?1 AND (?2 IS NULL OR m.batch_item_id = ?2)
           AND (?3 IS NULL OR (m.created_at, m.id) < (?3, ?4))
         ORDER BY m.created_at DESC, m.id DESC LIMIT ?5",
    )
    .bind(product_id)
    .bind(layer)
    .bind(cursor.map(|c| c.created_at.as_str()))
    .bind(cursor.map(|c| c.id.as_str()))
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;
    let mut rows = rows;
    let next_cursor = if i64::try_from(rows.len()).unwrap_or(0) > limit {
        rows.truncate(usize::try_from(limit).unwrap_or(0));
        rows.last().map(|r| (r.created_at.clone(), r.id.clone()))
    } else {
        None
    };
    Ok(MovementPage { rows, next_cursor })
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ValuationGroup {
    /// Batch id, or category id (`None` = uncategorised).
    pub id: Option<String>,
    pub label: String,
    #[specta(type = specta_typescript::Number)]
    pub units: i64,
    #[specta(type = specta_typescript::Number)]
    pub value_at_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub value_at_retail: i64,
}

/// What the stock is worth (doc §5): Σ max(on_hand, 0) × layer cost, and × today's price at retail.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct StockValuation {
    /// `None` = now.
    pub as_of: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub total_units: i64,
    #[specta(type = specta_typescript::Number)]
    pub value_at_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub value_at_retail: i64,
    pub by_batch: Vec<ValuationGroup>,
    pub by_category: Vec<ValuationGroup>,
    /// Oversold layers needing attention (excluded from the totals).
    #[specta(type = specta_typescript::Number)]
    pub negative_layers: i64,
}

#[derive(FromRow)]
struct LayerValue {
    batch_id: String,
    batch_code: String,
    batch_name: String,
    category_id: Option<String>,
    category_name: Option<String>,
    on_hand: i64,
    unit_cost: i64,
    price: i64,
}

/// Valuation now (from the derived `batch_item_stock`) or as of the end of a business date (summing the
/// ledger up to it: slower but exact, doc §5.3).
pub async fn valuation(pool: &SqlitePool, store_id: &str, as_of: Option<&str>) -> AppResult<StockValuation> {
    if let Some(d) = as_of
        && crate::domain::business_date::parse_date(d).is_none()
    {
        return Err(AppError::validation("DATE_INVALID", d.to_owned()));
    }
    let stock_source = if as_of.is_some() {
        "(SELECT batch_item_id, SUM(qty_delta) AS on_hand FROM stock_movements
          WHERE store_id = ?1 AND business_date <= ?2 GROUP BY batch_item_id)"
    } else {
        "(SELECT batch_item_id, on_hand FROM batch_item_stock WHERE ?2 IS NULL)"
    };
    let rows: Vec<LayerValue> = sqlx::query_as(&format!(
        "SELECT b.id AS batch_id, b.code AS batch_code, b.name AS batch_name, p.category_id,
                c.name AS category_name, s.on_hand, bi.unit_cost, p.price
         FROM {stock_source} s
         JOIN batch_items bi ON bi.id = s.batch_item_id
         JOIN batches b ON b.id = bi.batch_id
         JOIN products p ON p.id = bi.product_id
         LEFT JOIN categories c ON c.id = p.category_id
         WHERE b.store_id = ?1 AND s.on_hand <> 0"
    ))
    .bind(store_id)
    .bind(as_of)
    .fetch_all(pool)
    .await?;
    let mut by_batch: BTreeMap<String, ValuationGroup> = BTreeMap::new();
    let mut by_category: BTreeMap<Option<String>, ValuationGroup> = BTreeMap::new();
    let (mut units, mut cost, mut retail, mut negative) = (0, 0, 0, 0);
    for r in rows {
        if r.on_hand < 0 {
            negative += 1;
            continue;
        }
        let (c, p) = (r.on_hand * r.unit_cost, r.on_hand * r.price);
        units += r.on_hand;
        cost += c;
        retail += p;
        let b = by_batch.entry(r.batch_id.clone()).or_insert_with(|| ValuationGroup {
            id: Some(r.batch_id.clone()),
            label: format!("{} · {}", r.batch_code, r.batch_name),
            units: 0,
            value_at_cost: 0,
            value_at_retail: 0,
        });
        b.units += r.on_hand;
        b.value_at_cost += c;
        b.value_at_retail += p;
        let g = by_category.entry(r.category_id.clone()).or_insert_with(|| ValuationGroup {
            id: r.category_id.clone(),
            label: r.category_name.clone().unwrap_or_default(),
            units: 0,
            value_at_cost: 0,
            value_at_retail: 0,
        });
        g.units += r.on_hand;
        g.value_at_cost += c;
        g.value_at_retail += p;
    }
    let sorted = |m: Vec<ValuationGroup>| {
        let mut v = m;
        v.sort_by(|a, b| b.value_at_cost.cmp(&a.value_at_cost).then(a.label.cmp(&b.label)));
        v
    };
    Ok(StockValuation {
        as_of: as_of.map(str::to_owned),
        total_units: units,
        value_at_cost: cost,
        value_at_retail: retail,
        by_batch: sorted(by_batch.into_values().collect()),
        by_category: sorted(by_category.into_values().collect()),
        negative_layers: negative,
    })
}

/// A layer that sold more than it held (offline counters draining the same layer, or a sale before intake).
#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct OversoldLayer {
    pub batch_item_id: String,
    pub product_id: String,
    pub product_name: String,
    pub batch_id: String,
    pub batch_code: String,
    #[specta(type = specta_typescript::Number)]
    pub on_hand: i64,
    #[specta(type = specta_typescript::Number)]
    pub unit_cost: i64,
}

pub async fn oversold(pool: &SqlitePool, store_id: &str) -> AppResult<Vec<OversoldLayer>> {
    Ok(sqlx::query_as(
        "SELECT bis.batch_item_id, bis.product_id, p.name AS product_name, b.id AS batch_id, b.code AS batch_code,
                bis.on_hand, bi.unit_cost
         FROM batch_item_stock bis
         JOIN batch_items bi ON bi.id = bis.batch_item_id
         JOIN batches b ON b.id = bis.batch_id
         JOIN products p ON p.id = bis.product_id
         WHERE bis.on_hand < 0 AND b.store_id = ?1
         ORDER BY bis.on_hand, p.name COLLATE NOCASE",
    )
    .bind(store_id)
    .fetch_all(pool)
    .await?)
}
