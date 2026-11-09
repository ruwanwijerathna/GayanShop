//! Batches and their cost layers (doc §3.1): `draft → received → closed`.
//! - Draft: items, quantities, retail prices and the allocation method change freely; the allocation is
//!   previewed live (`preview`), with its variance.
//! - Receive: one transaction allocates the landed cost (`even_per_unit`, `by_retail_value` or `manual`), locks
//!   every layer's cost and writes one `batch_intake` movement per layer.
//! - Reopen (received → draft): only while nothing but intake has moved; the intake is reversed.
//! - Late item: a forgotten product added to a received batch at unit cost 0 (the cost is already allocated).
//! - Close: when every layer is empty (or written off); read-only afterwards.
//!
//! Plus the `OPENING-STOCK` go-live batch and one-item ad-hoc purchases (Block 1.2).

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use super::stock::{self, Movement, StockReason};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::clock::new_id;
use crate::db::{Db, outbox};
use crate::domain::allocation::{self, AllocationError, AllocationLine, CostAllocation};
use crate::domain::receipt_no;
use crate::error::{AppError, AppResult};

const NAME_MAX: usize = 80;
const TEXT_MAX: usize = 120;
const QTY_MAX: i64 = 1_000_000;
const MONEY_MAX: i64 = 10_000_000_000;
pub const OPENING_STOCK: &str = "OPENING-STOCK";

/// How a batch's landed cost is spread over its layers (doc §3.1).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AllocationMethod {
    /// Every unit carries the same cost.
    EvenPerUnit,
    /// Weighted by retail price × qty: expensive items carry more cost.
    ByRetailValue,
    /// The owner types each unit cost.
    #[default]
    Manual,
}

impl AllocationMethod {
    fn code(self) -> &'static str {
        match self {
            AllocationMethod::EvenPerUnit => "even_per_unit",
            AllocationMethod::ByRetailValue => "by_retail_value",
            AllocationMethod::Manual => "manual",
        }
    }

    fn parse(code: &str) -> AppResult<Self> {
        match code {
            "even_per_unit" => Ok(AllocationMethod::EvenPerUnit),
            "by_retail_value" => Ok(AllocationMethod::ByRetailValue),
            "manual" => Ok(AllocationMethod::Manual),
            other => Err(AppError::internal(format!("unknown allocation method {other}"))),
        }
    }

    fn domain(self) -> CostAllocation {
        match self {
            AllocationMethod::EvenPerUnit => CostAllocation::EvenPerUnit,
            AllocationMethod::ByRetailValue => CostAllocation::ByRetailValue,
            AllocationMethod::Manual => CostAllocation::Manual,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Draft,
    Received,
    Closed,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct Batch {
    pub id: String,
    pub code: String,
    pub name: String,
    pub supplier_name: Option<String>,
    pub supplier_ref: Option<String>,
    pub purchased_at: String,
    pub received_at: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub total_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub extra_costs: i64,
    /// total_cost + extra_costs.
    #[specta(type = specta_typescript::Number)]
    pub landed_cost: i64,
    pub allocation_method: AllocationMethod,
    #[specta(type = specta_typescript::Number)]
    pub allocation_variance: i64,
    pub status: BatchStatus,
    pub is_system: bool,
    pub notes: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub item_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub total_units: i64,
    /// Σ unit_cost × qty over the layers (manual costs while still a draft).
    #[specta(type = specta_typescript::Number)]
    pub items_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct BatchItem {
    pub id: String,
    pub product_id: String,
    pub product_name: String,
    pub product_sku: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub qty_received: i64,
    /// While a draft: the typed (manual) cost; once received: the locked layer cost.
    #[specta(type = specta_typescript::Number)]
    pub unit_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub retail_price_at_intake: i64,
    pub added_after_receipt: bool,
    #[specta(type = specta_typescript::Number)]
    pub on_hand: i64,
    #[specta(type = specta_typescript::Number)]
    pub sold_qty: i64,
}

/// "Did this batch make money?" (doc §3.1 batch metrics), from the trigger-maintained `batch_item_stock`.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, Default)]
pub struct BatchPnl {
    #[specta(type = specta_typescript::Number)]
    pub landed_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub units_received: i64,
    #[specta(type = specta_typescript::Number)]
    pub units_sold: i64,
    #[specta(type = specta_typescript::Number)]
    pub units_remaining: i64,
    /// Net sales of the batch's units (ex tax, after discounts).
    #[specta(type = specta_typescript::Number)]
    pub revenue_net: i64,
    /// What the sold units cost.
    #[specta(type = specta_typescript::Number)]
    pub cogs: i64,
    #[specta(type = specta_typescript::Number)]
    pub written_off_qty: i64,
    #[specta(type = specta_typescript::Number)]
    pub written_off_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub allocation_variance: i64,
    /// Σ on_hand × unit cost (positive layers only).
    #[specta(type = specta_typescript::Number)]
    pub remaining_value_cost: i64,
    /// Σ on_hand × the product's current price.
    #[specta(type = specta_typescript::Number)]
    pub remaining_value_retail: i64,
    /// revenue − cogs − written-off cost − allocation variance.
    #[specta(type = specta_typescript::Number)]
    pub realised_profit: i64,
    /// revenue − landed cost: negative until the batch has paid for itself.
    #[specta(type = specta_typescript::Number)]
    pub payback: i64,
    /// payback + what the rest would bring at today's prices.
    #[specta(type = specta_typescript::Number)]
    pub projected_profit: i64,
    /// units sold / units received, in basis points.
    #[specta(type = specta_typescript::Number)]
    pub sell_through_bp: i64,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct BatchDetail {
    pub batch: Batch,
    pub items: Vec<BatchItem>,
    /// `None` for a draft (nothing received yet).
    pub pnl: Option<BatchPnl>,
}

/// One layer of the live allocation preview.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct AllocationPreviewLine {
    pub item_id: String,
    pub product_name: String,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
    #[specta(type = specta_typescript::Number)]
    pub retail_price: i64,
    #[specta(type = specta_typescript::Number)]
    pub unit_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub line_cost: i64,
}

/// What "Receive" would fix, shown live while the batch is a draft (doc §3.1 "variance is shown live").
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct AllocationPreview {
    pub method: AllocationMethod,
    #[specta(type = specta_typescript::Number)]
    pub landed_cost: i64,
    /// Σ unit_cost × qty.
    #[specta(type = specta_typescript::Number)]
    pub allocated: i64,
    /// landed − allocated: 0 ≤ v < Σqty for even / by-value; anything for manual.
    #[specta(type = specta_typescript::Number)]
    pub variance: i64,
    pub lines: Vec<AllocationPreviewLine>,
    /// Why Receive would be refused right now (a translatable code), if it would.
    pub problem: Option<String>,
}

/// A product forgotten at intake, added to a received batch (unit cost 0).
#[derive(Debug, Clone, Deserialize, Type)]
pub struct LateItemInput {
    pub product_id: String,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
    /// `None` = the product's current price.
    #[specta(type = Option<specta_typescript::Number>)]
    pub retail_price: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct BatchInput {
    pub name: String,
    pub supplier_name: Option<String>,
    pub supplier_ref: Option<String>,
    /// ISO date-time of the purchase; `None` = now.
    pub purchased_at: Option<String>,
    /// Invoice total. 0 with no extra costs ⇒ the batch costs exactly Σ(unit cost × qty).
    #[specta(type = specta_typescript::Number)]
    pub total_cost: i64,
    #[specta(type = specta_typescript::Number)]
    pub extra_costs: i64,
    /// `None` = manual (the Phase 1 default).
    #[serde(default)]
    pub allocation_method: Option<AllocationMethod>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct BatchItemInput {
    pub product_id: String,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
    #[specta(type = specta_typescript::Number)]
    pub unit_cost: i64,
    /// `None` = the product's current price.
    #[specta(type = Option<specta_typescript::Number>)]
    pub retail_price: Option<i64>,
}

/// A one-item purchase received immediately ("bought 12 umbrellas at the market").
#[derive(Debug, Clone, Deserialize, Type)]
pub struct AdHocPurchase {
    pub product_id: String,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
    #[specta(type = specta_typescript::Number)]
    pub unit_cost: i64,
    pub supplier_name: Option<String>,
    pub note: Option<String>,
}

#[derive(FromRow)]
struct BatchRow {
    id: String,
    code: String,
    name: String,
    supplier_name: Option<String>,
    supplier_ref: Option<String>,
    purchased_at: String,
    received_at: Option<String>,
    total_cost: i64,
    extra_costs: i64,
    allocation_method: String,
    allocation_variance: i64,
    status: String,
    is_system: bool,
    notes: Option<String>,
    item_count: i64,
    total_units: i64,
    items_cost: i64,
    version: i64,
}

impl BatchRow {
    fn into_batch(self) -> AppResult<Batch> {
        let status = match self.status.as_str() {
            "draft" => BatchStatus::Draft,
            "received" => BatchStatus::Received,
            "closed" => BatchStatus::Closed,
            other => return Err(AppError::internal(format!("unknown batch status {other}"))),
        };
        Ok(Batch {
            id: self.id,
            code: self.code,
            name: self.name,
            supplier_name: self.supplier_name,
            supplier_ref: self.supplier_ref,
            purchased_at: self.purchased_at,
            received_at: self.received_at,
            total_cost: self.total_cost,
            extra_costs: self.extra_costs,
            landed_cost: self.total_cost.saturating_add(self.extra_costs),
            allocation_method: AllocationMethod::parse(&self.allocation_method)?,
            allocation_variance: self.allocation_variance,
            status,
            is_system: self.is_system,
            notes: self.notes,
            item_count: self.item_count,
            total_units: self.total_units,
            items_cost: self.items_cost,
            version: self.version,
        })
    }
}

const BATCH_SELECT: &str =
    "SELECT b.id, b.code, b.name, b.supplier_name, b.supplier_ref, b.purchased_at, b.received_at,
       b.total_cost, b.extra_costs, b.allocation_method, b.allocation_variance, b.status, b.is_system, b.notes, b.version,
       (SELECT COUNT(*) FROM batch_items bi WHERE bi.batch_id = b.id AND bi.deleted_at IS NULL) AS item_count,
       (SELECT COALESCE(SUM(bi.qty_received), 0) FROM batch_items bi
         WHERE bi.batch_id = b.id AND bi.deleted_at IS NULL) AS total_units,
       (SELECT COALESCE(SUM(bi.qty_received * CASE WHEN b.status = 'draft' THEN COALESCE(bi.manual_unit_cost, 0)
                                                   ELSE bi.unit_cost END), 0)
          FROM batch_items bi WHERE bi.batch_id = b.id AND bi.deleted_at IS NULL) AS items_cost
     FROM batches b";

// ───────── reads ─────────

/// Drafts first, then received batches newest first.
pub async fn list(pool: &SqlitePool, store_id: &str) -> AppResult<Vec<Batch>> {
    let rows: Vec<BatchRow> = sqlx::query_as(&format!(
        "{BATCH_SELECT} WHERE b.store_id = ?1 AND b.deleted_at IS NULL
         ORDER BY b.status = 'draft' DESC, COALESCE(b.received_at, b.created_at) DESC, b.id DESC"
    ))
    .bind(store_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(BatchRow::into_batch).collect()
}

async fn get_row(conn: &mut SqliteConnection, store_id: &str, id: &str) -> AppResult<Batch> {
    let row: BatchRow =
        sqlx::query_as(&format!("{BATCH_SELECT} WHERE b.store_id = ?1 AND b.id = ?2 AND b.deleted_at IS NULL"))
            .bind(store_id)
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| AppError::not_found("batch"))?;
    row.into_batch()
}

async fn items(conn: &mut SqliteConnection, batch_id: &str, draft: bool) -> AppResult<Vec<BatchItem>> {
    Ok(sqlx::query_as(
        "SELECT bi.id, bi.product_id, p.name AS product_name, p.sku AS product_sku, bi.qty_received,
                CASE WHEN ?2 THEN COALESCE(bi.manual_unit_cost, 0) ELSE bi.unit_cost END AS unit_cost,
                bi.retail_price_at_intake, bi.added_after_receipt,
                COALESCE(bis.on_hand, 0) AS on_hand, COALESCE(bis.sold_qty, 0) AS sold_qty
         FROM batch_items bi
         JOIN products p ON p.id = bi.product_id
         LEFT JOIN batch_item_stock bis ON bis.batch_item_id = bi.id
         WHERE bi.batch_id = ?1 AND bi.deleted_at IS NULL
         ORDER BY p.name COLLATE NOCASE, bi.id",
    )
    .bind(batch_id)
    .bind(draft)
    .fetch_all(&mut *conn)
    .await?)
}

#[derive(FromRow)]
struct PnlRow {
    units_received: i64,
    units_sold: i64,
    on_hand: i64,
    revenue_net: i64,
    cogs: i64,
    written_off_qty: i64,
    written_off_cost: i64,
    remaining_value_cost: i64,
    remaining_value_retail: i64,
}

async fn pnl(conn: &mut SqliteConnection, batch: &Batch) -> AppResult<BatchPnl> {
    let r: PnlRow = sqlx::query_as(
        "SELECT COALESCE(SUM(bis.received_qty), 0) AS units_received, COALESCE(SUM(bis.sold_qty), 0) AS units_sold,
                COALESCE(SUM(bis.on_hand), 0) AS on_hand, COALESCE(SUM(bis.sold_revenue), 0) AS revenue_net,
                COALESCE(SUM(bis.sold_cost), 0) AS cogs, COALESCE(SUM(bis.written_off_qty), 0) AS written_off_qty,
                COALESCE(SUM(bis.written_off_cost), 0) AS written_off_cost,
                COALESCE(SUM(MAX(bis.on_hand, 0) * bi.unit_cost), 0) AS remaining_value_cost,
                COALESCE(SUM(MAX(bis.on_hand, 0) * p.price), 0) AS remaining_value_retail
         FROM batch_item_stock bis
         JOIN batch_items bi ON bi.id = bis.batch_item_id
         JOIN products p ON p.id = bis.product_id
         WHERE bis.batch_id = ?1",
    )
    .bind(&batch.id)
    .fetch_one(&mut *conn)
    .await?;
    let payback = r.revenue_net - batch.landed_cost;
    Ok(BatchPnl {
        landed_cost: batch.landed_cost,
        units_received: r.units_received,
        units_sold: r.units_sold,
        units_remaining: r.on_hand,
        revenue_net: r.revenue_net,
        cogs: r.cogs,
        written_off_qty: r.written_off_qty,
        written_off_cost: r.written_off_cost,
        allocation_variance: batch.allocation_variance,
        remaining_value_cost: r.remaining_value_cost,
        remaining_value_retail: r.remaining_value_retail,
        realised_profit: r.revenue_net - r.cogs - r.written_off_cost - batch.allocation_variance,
        payback,
        projected_profit: payback + r.remaining_value_retail,
        sell_through_bp: if r.units_received > 0 { r.units_sold * 10_000 / r.units_received } else { 0 },
    })
}

async fn detail_conn(conn: &mut SqliteConnection, store_id: &str, id: &str) -> AppResult<BatchDetail> {
    let batch = get_row(conn, store_id, id).await?;
    let items = items(conn, id, batch.status == BatchStatus::Draft).await?;
    let pnl = if batch.status == BatchStatus::Draft { None } else { Some(pnl(conn, &batch).await?) };
    Ok(BatchDetail { batch, items, pnl })
}

pub async fn detail(pool: &SqlitePool, store_id: &str, id: &str) -> AppResult<BatchDetail> {
    let mut conn = pool.acquire().await?;
    detail_conn(&mut conn, store_id, id).await
}

// ───────── validation helpers ─────────

fn opt_text(v: Option<&str>, max: usize, code: &str) -> AppResult<Option<String>> {
    let v = v.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    if v.as_ref().is_some_and(|s| s.chars().count() > max) {
        return Err(AppError::validation(code, format!("max {max}")));
    }
    Ok(v)
}

struct CleanHeader {
    name: String,
    supplier_name: Option<String>,
    supplier_ref: Option<String>,
    purchased_at: String,
    notes: Option<String>,
}

fn clean_header(input: &BatchInput, now: &str) -> AppResult<CleanHeader> {
    let name = input.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(AppError::validation("BATCH_NAME_INVALID", format!("1–{NAME_MAX} characters")));
    }
    if !(0..=MONEY_MAX).contains(&input.total_cost) || !(0..=MONEY_MAX).contains(&input.extra_costs) {
        return Err(AppError::validation(
            "BATCH_COST_INVALID",
            format!("{} / {}", input.total_cost, input.extra_costs),
        ));
    }
    let purchased_at = match input.purchased_at.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(at) => {
            chrono::DateTime::parse_from_rfc3339(at)
                .map_err(|_| AppError::validation("BATCH_DATE_INVALID", at.to_owned()))?;
            at.to_owned()
        }
        None => now.to_owned(),
    };
    Ok(CleanHeader {
        name,
        supplier_name: opt_text(input.supplier_name.as_deref(), TEXT_MAX, "BATCH_SUPPLIER_INVALID")?,
        supplier_ref: opt_text(input.supplier_ref.as_deref(), TEXT_MAX, "BATCH_SUPPLIER_INVALID")?,
        purchased_at,
        notes: opt_text(input.notes.as_deref(), 500, "BATCH_NOTES_INVALID")?,
    })
}

async fn require_draft(conn: &mut SqliteConnection, store_id: &str, id: &str) -> AppResult<Batch> {
    let batch = get_row(conn, store_id, id).await?;
    if batch.status != BatchStatus::Draft {
        return Err(AppError::validation("BATCH_NOT_DRAFT", batch.code));
    }
    Ok(batch)
}

/// `B-2026-1001`: business year, this counter's digit, then a sequence — unique across offline counters.
async fn next_code(conn: &mut SqliteConnection, ctx: &WriteCtx) -> AppResult<String> {
    let digit = receipt_no::device_digit(&ctx.device_code)
        .map_err(|e| AppError::validation("DEVICE_CODE_INVALID", e.to_string()))?;
    let year = ctx.business_date.get(..4).unwrap_or("0000");
    let prefix = format!("B-{year}-{digit}");
    let max: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(CAST(substr(code, ?2) AS INTEGER)) FROM batches WHERE store_id = ?1 AND code GLOB ?3",
    )
    .bind(&ctx.store_id)
    .bind(i64::try_from(prefix.len() + 1).unwrap_or(i64::MAX))
    .bind(format!("{prefix}[0-9][0-9][0-9]*"))
    .fetch_one(&mut *conn)
    .await?;
    Ok(format!("{prefix}{:03}", max.unwrap_or(0) + 1))
}

fn audit_ctx(ctx: &WriteCtx) -> AuditCtx<'_> {
    AuditCtx { store_id: &ctx.store_id, device_id: &ctx.device_id, business_date: &ctx.business_date, now: &ctx.now }
}

// ───────── writes (connection-level, so ad-hoc purchase can compose them in one transaction) ─────────

async fn create_conn(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    actor: &Actor,
    input: &BatchInput,
) -> AppResult<String> {
    let h = clean_header(input, &ctx.now)?;
    let id = new_id();
    let code = next_code(conn, ctx).await?;
    sqlx::query(
        "INSERT INTO batches (id, store_id, code, name, supplier_name, supplier_ref, purchased_at, total_cost,
                              extra_costs, allocation_method, notes, created_at, updated_at, updated_by_device)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?13, ?10, ?11, ?11, ?12)",
    )
    .bind(&id)
    .bind(&ctx.store_id)
    .bind(&code)
    .bind(&h.name)
    .bind(h.supplier_name.as_deref())
    .bind(h.supplier_ref.as_deref())
    .bind(&h.purchased_at)
    .bind(input.total_cost)
    .bind(input.extra_costs)
    .bind(h.notes.as_deref())
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(input.allocation_method.unwrap_or_default().code())
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "batches", &id, &ctx.now).await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::BATCH_CREATE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("batch"),
            entity_id: Some(&id),
            metadata: json!({ "code": code, "name": h.name, "total_cost": input.total_cost,
                              "extra_costs": input.extra_costs,
                              "allocation_method": input.allocation_method.unwrap_or_default().code() }),
        },
    )
    .await?;
    Ok(id)
}

async fn set_item_conn(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    batch_id: &str,
    input: &BatchItemInput,
) -> AppResult<String> {
    require_draft(conn, &ctx.store_id, batch_id).await?;
    if !(1..=QTY_MAX).contains(&input.qty) {
        return Err(AppError::validation("BATCH_ITEM_QTY_INVALID", input.qty.to_string()));
    }
    if !(0..=MONEY_MAX).contains(&input.unit_cost) || input.retail_price.is_some_and(|p| !(0..=MONEY_MAX).contains(&p))
    {
        return Err(AppError::validation("BATCH_ITEM_COST_INVALID", input.unit_cost.to_string()));
    }
    let product: Option<(bool, i64)> = sqlx::query_as(
        "SELECT track_stock, price FROM products WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL",
    )
    .bind(&input.product_id)
    .bind(&ctx.store_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((tracked, price)) = product else {
        return Err(AppError::validation("BATCH_ITEM_PRODUCT_INVALID", input.product_id.clone()));
    };
    if !tracked {
        return Err(AppError::validation("PRODUCT_NOT_TRACKED", input.product_id.clone()));
    }
    let retail = input.retail_price.unwrap_or(price);
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM batch_items WHERE batch_id = ?1 AND product_id = ?2")
            .bind(batch_id)
            .bind(&input.product_id)
            .fetch_optional(&mut *conn)
            .await?;
    let id = match existing {
        Some(id) => {
            sqlx::query(
                "UPDATE batch_items SET qty_received = ?1, manual_unit_cost = ?2, retail_price_at_intake = ?3,
                        deleted_at = NULL, updated_at = ?4, updated_by_device = ?5, version = version + 1
                 WHERE id = ?6",
            )
            .bind(input.qty)
            .bind(input.unit_cost)
            .bind(retail)
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
                "INSERT INTO batch_items (id, store_id, batch_id, product_id, qty_received, manual_unit_cost,
                                          retail_price_at_intake, created_at, updated_at, updated_by_device)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
            )
            .bind(&id)
            .bind(&ctx.store_id)
            .bind(batch_id)
            .bind(&input.product_id)
            .bind(input.qty)
            .bind(input.unit_cost)
            .bind(retail)
            .bind(&ctx.now)
            .bind(&ctx.device_id)
            .execute(&mut *conn)
            .await?;
            id
        }
    };
    outbox::enqueue_upsert(conn, "batch_items", &id, &ctx.now).await?;
    Ok(id)
}

#[derive(FromRow)]
struct ReceiveLine {
    id: String,
    product_id: String,
    product_name: String,
    qty_received: i64,
    manual_unit_cost: Option<i64>,
    retail_price_at_intake: i64,
}

fn alloc_err(e: AllocationError) -> AppError {
    match e {
        AllocationError::BadManualCost(_) => AppError::validation("BATCH_ITEM_COST_INVALID", e.to_string()),
        AllocationError::Overflow => AppError::validation("BATCH_COST_INVALID", e.to_string()),
        AllocationError::NoWeight => AppError::validation("BATCH_NO_WEIGHT", e.to_string()),
        other => AppError::validation("BATCH_ITEM_QTY_INVALID", other.to_string()),
    }
}

async fn receive_lines(conn: &mut SqliteConnection, batch_id: &str) -> AppResult<Vec<ReceiveLine>> {
    Ok(sqlx::query_as(
        "SELECT bi.id, bi.product_id, p.name AS product_name, bi.qty_received, bi.manual_unit_cost,
                bi.retail_price_at_intake
         FROM batch_items bi JOIN products p ON p.id = bi.product_id
         WHERE bi.batch_id = ?1 AND bi.deleted_at IS NULL ORDER BY bi.id",
    )
    .bind(batch_id)
    .fetch_all(&mut *conn)
    .await?)
}

struct Planned {
    total_cost: i64,
    landed: i64,
    allocation: allocation::Allocation,
}

/// The allocation "Receive" would fix, or the reason it can't.
fn plan(batch: &Batch, lines: &[ReceiveLine]) -> AppResult<Planned> {
    if lines.iter().all(|l| l.qty_received <= 0) {
        return Err(AppError::validation("BATCH_EMPTY", batch.code.clone()));
    }
    let alloc_lines: Vec<AllocationLine> = lines
        .iter()
        .map(|l| AllocationLine {
            qty: l.qty_received,
            retail_price: l.retail_price_at_intake,
            manual_unit_cost: l.manual_unit_cost,
        })
        .collect();
    let total_cost = match batch.allocation_method {
        AllocationMethod::Manual if batch.total_cost == 0 && batch.extra_costs == 0 => {
            // No invoice total typed (opening stock, quick purchases): the batch costs exactly its layers.
            lines
                .iter()
                .map(|l| l.qty_received.checked_mul(l.manual_unit_cost.unwrap_or(0)))
                .try_fold(0i64, |acc, x| x.and_then(|x| acc.checked_add(x)))
                .ok_or_else(|| AppError::validation("BATCH_COST_INVALID", "overflow"))?
        }
        AllocationMethod::Manual => batch.total_cost,
        _ if batch.total_cost == 0 && batch.extra_costs == 0 => {
            return Err(AppError::validation("BATCH_TOTAL_REQUIRED", batch.code.clone()));
        }
        _ => batch.total_cost,
    };
    let landed = total_cost + batch.extra_costs;
    let allocation = allocation::allocate(landed, batch.allocation_method.domain(), &alloc_lines).map_err(alloc_err)?;
    Ok(Planned { total_cost, landed, allocation })
}

async fn receive_conn(conn: &mut SqliteConnection, ctx: &WriteCtx, actor: &Actor, batch_id: &str) -> AppResult<()> {
    let batch = require_draft(conn, &ctx.store_id, batch_id).await?;
    let lines = receive_lines(conn, batch_id).await?;
    let Planned { total_cost, landed, allocation } = plan(&batch, &lines)?;

    // Fix each layer's cost while the batch is still a draft (the cost-lock trigger fires after receive).
    for (line, cost) in lines.iter().zip(&allocation.unit_costs) {
        sqlx::query(
            "UPDATE batch_items SET unit_cost = ?1, updated_at = ?2, updated_by_device = ?3, version = version + 1
             WHERE id = ?4",
        )
        .bind(cost)
        .bind(&ctx.now)
        .bind(&ctx.device_id)
        .bind(&line.id)
        .execute(&mut *conn)
        .await?;
        outbox::enqueue_upsert(conn, "batch_items", &line.id, &ctx.now).await?;
    }
    // A reopened batch's late items are ordinary layers again once it is re-received.
    sqlx::query("UPDATE batch_items SET added_after_receipt = 0 WHERE batch_id = ?1 AND added_after_receipt = 1")
        .bind(batch_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE batches SET status = 'received', received_at = ?1, received_business_date = ?2, received_by = ?3,
                total_cost = ?4, allocation_variance = ?5, updated_at = ?1, updated_by_device = ?6,
                version = version + 1
         WHERE id = ?7",
    )
    .bind(&ctx.now)
    .bind(&ctx.business_date)
    .bind(&actor.staff_id)
    .bind(total_cost)
    .bind(allocation.variance)
    .bind(&ctx.device_id)
    .bind(batch_id)
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "batches", batch_id, &ctx.now).await?;

    for (line, cost) in lines.iter().zip(&allocation.unit_costs) {
        if line.qty_received <= 0 {
            continue;
        }
        stock::record(
            conn,
            ctx,
            &Movement {
                product_id: &line.product_id,
                batch_item_id: &line.id,
                qty_delta: line.qty_received,
                reason: StockReason::BatchIntake,
                ref_type: Some("batch"),
                ref_id: Some(batch_id),
                order_item_id: None,
                unit_cost: *cost,
                revenue_net: None,
                staff_id: Some(&actor.staff_id),
                approved_by: actor.approved_by.as_deref(),
                note: None,
            },
        )
        .await?;
    }
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::BATCH_RECEIVE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("batch"),
            entity_id: Some(batch_id),
            metadata: json!({
                "code": batch.code,
                "layers": lines.iter().filter(|l| l.qty_received > 0).count(),
                "units": lines.iter().map(|l| l.qty_received).sum::<i64>(),
                "landed_cost": landed,
                "allocation_method": batch.allocation_method.code(),
                "allocation_variance": allocation.variance,
            }),
        },
    )
    .await?;
    Ok(())
}

// ───────── public writes ─────────

pub async fn create(db: &Db, ctx: &WriteCtx, actor: &Actor, input: &BatchInput) -> AppResult<BatchDetail> {
    let mut tx = db.write().await?;
    let id = create_conn(tx.conn(), ctx, actor, input).await?;
    let detail = detail_conn(tx.conn(), &ctx.store_id, &id).await?;
    tx.commit().await?;
    Ok(detail)
}

/// Edit a draft's header. System batches keep their code and name.
pub async fn update(db: &Db, ctx: &WriteCtx, id: &str, input: &BatchInput) -> AppResult<BatchDetail> {
    let mut tx = db.write().await?;
    let batch = require_draft(tx.conn(), &ctx.store_id, id).await?;
    let h = clean_header(input, &batch.purchased_at)?;
    let name = if batch.is_system { batch.name.clone() } else { h.name };
    let method = if batch.is_system { AllocationMethod::Manual } else { input.allocation_method.unwrap_or_default() };
    sqlx::query(
        "UPDATE batches SET name = ?1, supplier_name = ?2, supplier_ref = ?3, purchased_at = ?4, total_cost = ?5,
                extra_costs = ?6, notes = ?7, updated_at = ?8, updated_by_device = ?9, version = version + 1,
                allocation_method = ?11
         WHERE id = ?10",
    )
    .bind(&name)
    .bind(h.supplier_name.as_deref())
    .bind(h.supplier_ref.as_deref())
    .bind(&h.purchased_at)
    .bind(input.total_cost)
    .bind(input.extra_costs)
    .bind(h.notes.as_deref())
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .bind(method.code())
    .execute(tx.conn())
    .await?;
    outbox::enqueue_upsert(tx.conn(), "batches", id, &ctx.now).await?;
    let detail = detail_conn(tx.conn(), &ctx.store_id, id).await?;
    tx.commit().await?;
    Ok(detail)
}

/// Add a product to a draft, or change its quantity / manual cost (one layer per product per batch).
pub async fn set_item(db: &Db, ctx: &WriteCtx, batch_id: &str, input: &BatchItemInput) -> AppResult<BatchDetail> {
    let mut tx = db.write().await?;
    set_item_conn(tx.conn(), ctx, batch_id, input).await?;
    let detail = detail_conn(tx.conn(), &ctx.store_id, batch_id).await?;
    tx.commit().await?;
    Ok(detail)
}

pub async fn remove_item(db: &Db, ctx: &WriteCtx, batch_id: &str, item_id: &str) -> AppResult<BatchDetail> {
    let mut tx = db.write().await?;
    require_draft(tx.conn(), &ctx.store_id, batch_id).await?;
    let done = sqlx::query(
        "UPDATE batch_items SET deleted_at = ?1, updated_at = ?1, updated_by_device = ?2, version = version + 1
         WHERE id = ?3 AND batch_id = ?4 AND deleted_at IS NULL",
    )
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(item_id)
    .bind(batch_id)
    .execute(tx.conn())
    .await?;
    if done.rows_affected() != 1 {
        return Err(AppError::not_found("batch item"));
    }
    outbox::enqueue_upsert(tx.conn(), "batch_items", item_id, &ctx.now).await?;
    let detail = detail_conn(tx.conn(), &ctx.store_id, batch_id).await?;
    tx.commit().await?;
    Ok(detail)
}

/// Receive a draft: lock costs, write the intake movements, audit `BATCH_RECEIVE` — all or nothing.
pub async fn receive(db: &Db, ctx: &WriteCtx, actor: &Actor, batch_id: &str) -> AppResult<BatchDetail> {
    let mut tx = db.write().await?;
    receive_conn(tx.conn(), ctx, actor, batch_id).await?;
    let detail = detail_conn(tx.conn(), &ctx.store_id, batch_id).await?;
    tx.commit().await?;
    Ok(detail)
}

/// One-item purchase, created and received in a single transaction.
pub async fn ad_hoc_purchase(db: &Db, ctx: &WriteCtx, actor: &Actor, input: &AdHocPurchase) -> AppResult<BatchDetail> {
    let mut tx = db.write().await?;
    let product_name: String =
        sqlx::query_scalar("SELECT name FROM products WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL")
            .bind(&input.product_id)
            .bind(&ctx.store_id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or_else(|| AppError::validation("BATCH_ITEM_PRODUCT_INVALID", input.product_id.clone()))?;
    let header = BatchInput {
        name: product_name.chars().take(NAME_MAX).collect(),
        supplier_name: input.supplier_name.clone(),
        supplier_ref: None,
        purchased_at: None,
        total_cost: 0,
        extra_costs: 0,
        allocation_method: Some(AllocationMethod::Manual),
        notes: input.note.clone(),
    };
    let id = create_conn(tx.conn(), ctx, actor, &header).await?;
    let item = BatchItemInput {
        product_id: input.product_id.clone(),
        qty: input.qty,
        unit_cost: input.unit_cost,
        retail_price: None,
    };
    set_item_conn(tx.conn(), ctx, &id, &item).await?;
    receive_conn(tx.conn(), ctx, actor, &id).await?;
    let detail = detail_conn(tx.conn(), &ctx.store_id, &id).await?;
    tx.commit().await?;
    Ok(detail)
}

/// The system `OPENING-STOCK` batch id (a draft until go-live is received).
pub async fn opening_stock_id(pool: &SqlitePool, store_id: &str) -> AppResult<String> {
    sqlx::query_scalar(
        "SELECT id FROM batches WHERE store_id = ?1 AND code = ?2 AND is_system = 1 AND deleted_at IS NULL",
    )
    .bind(store_id)
    .bind(OPENING_STOCK)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("batch"))
}

// ───────── Block 2.1: live allocation, reopen, late items, close ─────────

/// The allocation "Receive" would fix right now, for the intake wizard's live variance.
pub async fn preview(pool: &SqlitePool, store_id: &str, batch_id: &str) -> AppResult<AllocationPreview> {
    let mut conn = pool.acquire().await?;
    let batch = get_row(&mut conn, store_id, batch_id).await?;
    let lines = receive_lines(&mut conn, batch_id).await?;
    let (planned, problem) = match plan(&batch, &lines) {
        Ok(p) => (Some(p), None),
        Err(AppError::Validation { code, .. }) => (None, Some(code)),
        Err(e) => return Err(e),
    };
    let landed = planned.as_ref().map_or(batch.landed_cost, |p| p.landed);
    let costs = planned.as_ref().map(|p| p.allocation.unit_costs.clone());
    let lines: Vec<AllocationPreviewLine> = lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let unit_cost = costs.as_ref().and_then(|c| c.get(i).copied()).unwrap_or(0);
            AllocationPreviewLine {
                item_id: l.id,
                product_name: l.product_name,
                qty: l.qty_received,
                retail_price: l.retail_price_at_intake,
                unit_cost,
                line_cost: unit_cost.saturating_mul(l.qty_received),
            }
        })
        .collect();
    let allocated = lines.iter().map(|l| l.line_cost).sum::<i64>();
    Ok(AllocationPreview {
        method: batch.allocation_method,
        landed_cost: landed,
        allocated,
        variance: planned.map_or(landed - allocated, |p| p.allocation.variance),
        lines,
        problem,
    })
}

async fn require_status(
    conn: &mut SqliteConnection,
    store_id: &str,
    id: &str,
    status: BatchStatus,
    code: &str,
) -> AppResult<Batch> {
    let batch = get_row(conn, store_id, id).await?;
    if batch.status != status {
        return Err(AppError::validation(code, batch.code));
    }
    if batch.is_system {
        return Err(AppError::validation("BATCH_SYSTEM", batch.code));
    }
    Ok(batch)
}

/// Received → draft, to fix a mistake at intake. Only while nothing but intake has moved (no sale, void,
/// damage or count), so no history depends on the locked costs; the intake is reversed by `batch_reopen`.
pub async fn reopen(db: &Db, ctx: &WriteCtx, actor: &Actor, batch_id: &str) -> AppResult<BatchDetail> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let batch = require_status(conn, &ctx.store_id, batch_id, BatchStatus::Received, "BATCH_NOT_RECEIVED").await?;
    let moved: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM stock_movements m JOIN batch_items bi ON bi.id = m.batch_item_id
         WHERE bi.batch_id = ?1 AND m.reason NOT IN ('batch_intake', 'batch_reopen')",
    )
    .bind(batch_id)
    .fetch_one(&mut *conn)
    .await?;
    if moved > 0 {
        return Err(AppError::validation("BATCH_HAS_MOVEMENTS", batch.code));
    }
    let layers: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT bi.id, bi.product_id, bi.unit_cost, COALESCE(SUM(m.qty_delta), 0)
         FROM batch_items bi LEFT JOIN stock_movements m ON m.batch_item_id = bi.id
         WHERE bi.batch_id = ?1 GROUP BY bi.id",
    )
    .bind(batch_id)
    .fetch_all(&mut *conn)
    .await?;
    for (item_id, product_id, unit_cost, on_hand) in &layers {
        if *on_hand == 0 {
            continue;
        }
        stock::record(
            conn,
            ctx,
            &Movement {
                product_id,
                batch_item_id: item_id,
                qty_delta: -on_hand,
                reason: StockReason::BatchReopen,
                ref_type: Some("batch"),
                ref_id: Some(batch_id),
                order_item_id: None,
                unit_cost: *unit_cost,
                revenue_net: None,
                staff_id: Some(&actor.staff_id),
                approved_by: actor.approved_by.as_deref(),
                note: None,
            },
        )
        .await?;
    }
    sqlx::query(
        "UPDATE batches SET status = 'draft', received_at = NULL, received_business_date = NULL, received_by = NULL,
                allocation_variance = 0, updated_at = ?1, updated_by_device = ?2, version = version + 1
         WHERE id = ?3",
    )
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(batch_id)
    .execute(&mut *conn)
    .await?;
    // The locked costs become the editable manual costs again (a late item keeps its 0).
    sqlx::query(
        "UPDATE batch_items SET manual_unit_cost = COALESCE(manual_unit_cost, unit_cost), updated_at = ?1,
                updated_by_device = ?2, version = version + 1
         WHERE batch_id = ?3 AND deleted_at IS NULL",
    )
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(batch_id)
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "batches", batch_id, &ctx.now).await?;
    for (item_id, ..) in &layers {
        outbox::enqueue_upsert(conn, "batch_items", item_id, &ctx.now).await?;
    }
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::BATCH_REOPEN,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("batch"),
            entity_id: Some(batch_id),
            metadata: json!({ "code": batch.code, "units_reversed": layers.iter().map(|l| l.3).sum::<i64>() }),
        },
    )
    .await?;
    let detail = detail_conn(conn, &ctx.store_id, batch_id).await?;
    tx.commit().await?;
    Ok(detail)
}

/// A product forgotten at intake: a new layer at unit cost 0 (the batch cost is already fully allocated, so
/// its sales are pure recovery), received immediately.
pub async fn add_late_item(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    batch_id: &str,
    input: &LateItemInput,
) -> AppResult<BatchDetail> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let batch = require_status(conn, &ctx.store_id, batch_id, BatchStatus::Received, "BATCH_NOT_RECEIVED").await?;
    if !(1..=QTY_MAX).contains(&input.qty) {
        return Err(AppError::validation("BATCH_ITEM_QTY_INVALID", input.qty.to_string()));
    }
    let product: Option<(bool, i64)> = sqlx::query_as(
        "SELECT track_stock, price FROM products WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL",
    )
    .bind(&input.product_id)
    .bind(&ctx.store_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((tracked, price)) = product else {
        return Err(AppError::validation("BATCH_ITEM_PRODUCT_INVALID", input.product_id.clone()));
    };
    if !tracked {
        return Err(AppError::validation("PRODUCT_NOT_TRACKED", input.product_id.clone()));
    }
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM batch_items WHERE batch_id = ?1 AND product_id = ?2")
        .bind(batch_id)
        .bind(&input.product_id)
        .fetch_one(&mut *conn)
        .await?;
    if exists > 0 {
        return Err(AppError::validation("BATCH_ITEM_EXISTS", input.product_id.clone()));
    }
    let retail = input.retail_price.unwrap_or(price);
    if !(0..=MONEY_MAX).contains(&retail) {
        return Err(AppError::validation("BATCH_ITEM_COST_INVALID", retail.to_string()));
    }
    let id = new_id();
    sqlx::query(
        "INSERT INTO batch_items (id, store_id, batch_id, product_id, qty_received, unit_cost, retail_price_at_intake,
                                  manual_unit_cost, added_after_receipt, created_at, updated_at, updated_by_device)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, 0, 1, ?7, ?7, ?8)",
    )
    .bind(&id)
    .bind(&ctx.store_id)
    .bind(batch_id)
    .bind(&input.product_id)
    .bind(input.qty)
    .bind(retail)
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "batch_items", &id, &ctx.now).await?;
    stock::record(
        conn,
        ctx,
        &Movement {
            product_id: &input.product_id,
            batch_item_id: &id,
            qty_delta: input.qty,
            reason: StockReason::BatchIntake,
            ref_type: Some("batch"),
            ref_id: Some(batch_id),
            order_item_id: None,
            unit_cost: 0,
            revenue_net: None,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            note: Some("late item"),
        },
    )
    .await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::BATCH_LATE_ITEM,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("batch"),
            entity_id: Some(batch_id),
            metadata: json!({ "code": batch.code, "batch_item_id": id, "product_id": input.product_id,
                              "qty": input.qty }),
        },
    )
    .await?;
    let detail = detail_conn(conn, &ctx.store_id, batch_id).await?;
    tx.commit().await?;
    Ok(detail)
}

/// Received → closed: every layer is empty (or written off, Block 2.3). The batch is read-only afterwards and
/// its P&L is final.
pub async fn close(db: &Db, ctx: &WriteCtx, actor: &Actor, batch_id: &str) -> AppResult<BatchDetail> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let batch = require_status(conn, &ctx.store_id, batch_id, BatchStatus::Received, "BATCH_NOT_RECEIVED").await?;
    let left: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(on_hand), 0) FROM batch_item_stock WHERE batch_id = ?1 AND on_hand > 0",
    )
    .bind(batch_id)
    .fetch_one(&mut *conn)
    .await?;
    if left > 0 {
        return Err(AppError::validation("BATCH_HAS_STOCK", left.to_string()));
    }
    sqlx::query(
        "UPDATE batches SET status = 'closed', updated_at = ?1, updated_by_device = ?2, version = version + 1
         WHERE id = ?3",
    )
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(batch_id)
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "batches", batch_id, &ctx.now).await?;
    let detail = detail_conn(conn, &ctx.store_id, batch_id).await?;
    let pnl = detail.pnl.clone().unwrap_or_default();
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::BATCH_CLOSE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("batch"),
            entity_id: Some(batch_id),
            metadata: json!({ "code": batch.code, "revenue_net": pnl.revenue_net, "cogs": pnl.cogs,
                              "realised_profit": pnl.realised_profit, "payback": pnl.payback }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(detail)
}
