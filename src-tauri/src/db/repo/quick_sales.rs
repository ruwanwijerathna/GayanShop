//! Quick-sale review and promotion (doc §3.2, `QUICK_SALE_PROMOTE`). The review lists every quick-sale key
//! from the derived `quick_sale_stats`, most sold first, with its decision (new / promoted / ignored).
//!
//! - **Promote** runs one transaction: create the product (latest spelling, last price, the manager's cost),
//!   optionally a layer with the shelf count (in a chosen received batch, else in `ADJUSTMENTS`), the
//!   `quick_sale_links` row, `QUICK_SALE_PROMOTE` audit, outbox.
//! - **Ignore** hides one-off noise.
//!
//! Promotion is forward-only: past quick-sale lines keep their attribution. `quick_sale_links.id =
//! UUIDv5(store, custom_key)`, so two managers promoting the same key offline converge on one row.

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use super::products::{self, CodeChoice, ProductInput, ProductKind};
use super::stock::{self, Movement, StockReason};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::clock::{new_id, stable_id};
use crate::db::{Db, outbox};
use crate::domain::quick_sale::custom_key;
use crate::error::{AppError, AppResult};

const SHELF_MAX: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuickSaleStatus {
    New,
    Promoted,
    Ignored,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct QuickSaleRow {
    pub custom_key: String,
    /// The latest spelling typed at the till.
    pub display_name: String,
    #[specta(type = specta_typescript::Number)]
    pub times_sold: i64,
    #[specta(type = specta_typescript::Number)]
    pub units_sold: i64,
    #[specta(type = specta_typescript::Number)]
    pub revenue_net: i64,
    #[specta(type = specta_typescript::Number)]
    pub last_price: i64,
    #[specta(type = specta_typescript::Number)]
    pub min_price: i64,
    #[specta(type = specta_typescript::Number)]
    pub max_price: i64,
    pub first_sold_at: String,
    pub last_sold_at: String,
    pub status: QuickSaleStatus,
    /// The product it became, when promoted.
    pub product_id: Option<String>,
}

#[derive(FromRow)]
struct Row {
    custom_key: String,
    display_name: String,
    times_sold: i64,
    units_sold: i64,
    revenue_net: i64,
    last_price: i64,
    min_price: i64,
    max_price: i64,
    first_sold_at: String,
    last_sold_at: String,
    action: Option<String>,
    product_id: Option<String>,
}

fn status(action: Option<&str>) -> QuickSaleStatus {
    match action {
        Some("promoted") => QuickSaleStatus::Promoted,
        Some("ignored") => QuickSaleStatus::Ignored,
        _ => QuickSaleStatus::New,
    }
}

/// The review table, most sold first. `show` narrows to one status (`None` = everything).
pub async fn review(pool: &SqlitePool, store_id: &str, show: Option<QuickSaleStatus>) -> AppResult<Vec<QuickSaleRow>> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT s.custom_key, s.display_name, s.times_sold, s.units_sold, s.revenue_net, s.last_price, s.min_price,
                s.max_price, s.first_sold_at, s.last_sold_at, l.action, l.product_id
         FROM quick_sale_stats s
         LEFT JOIN quick_sale_links l ON l.custom_key = s.custom_key AND l.store_id = ?1 AND l.deleted_at IS NULL
         ORDER BY s.times_sold DESC, s.last_sold_at DESC, s.custom_key",
    )
    .bind(store_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| QuickSaleRow {
            status: status(r.action.as_deref()),
            custom_key: r.custom_key,
            display_name: r.display_name,
            times_sold: r.times_sold,
            units_sold: r.units_sold,
            revenue_net: r.revenue_net,
            last_price: r.last_price,
            min_price: r.min_price,
            max_price: r.max_price,
            first_sold_at: r.first_sold_at,
            last_sold_at: r.last_sold_at,
            product_id: r.product_id,
        })
        .filter(|r| show.is_none_or(|s| r.status == s))
        .collect())
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CounterSales {
    pub device_code: String,
    #[specta(type = specta_typescript::Number)]
    pub times_sold: i64,
    #[specta(type = specta_typescript::Number)]
    pub units: i64,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct QuickSaleDetail {
    pub custom_key: String,
    /// Median unit price over the completed sales of this key.
    #[specta(type = specta_typescript::Number)]
    pub median_price: i64,
    pub counters: Vec<CounterSales>,
    /// Every spelling typed for it, most recent first.
    pub spellings: Vec<String>,
}

/// One key opened in the review: median price and which counters sold it, computed on demand from
/// `order_items` (via `ix_items_custom`), completed orders only.
pub async fn detail(pool: &SqlitePool, key: &str) -> AppResult<QuickSaleDetail> {
    let prices: Vec<i64> = sqlx::query_scalar(
        "SELECT oi.unit_price FROM order_items oi JOIN orders o ON o.id = oi.order_id
         WHERE oi.is_custom = 1 AND oi.custom_key = ?1 AND o.status = 'completed' ORDER BY oi.unit_price",
    )
    .bind(key)
    .fetch_all(pool)
    .await?;
    if prices.is_empty() {
        return Err(AppError::not_found("quick sale"));
    }
    let n = prices.len();
    let median = if n % 2 == 1 { prices[n / 2] } else { (prices[n / 2 - 1] + prices[n / 2]) / 2 };
    let counters: Vec<(String, i64, i64)> = sqlx::query_as(
        // Receipt numbers start with the counter code (`T01-20261108-0042`).
        "SELECT substr(o.receipt_no, 1, instr(o.receipt_no, '-') - 1) AS code, COUNT(*), SUM(oi.qty)
         FROM order_items oi JOIN orders o ON o.id = oi.order_id
         WHERE oi.is_custom = 1 AND oi.custom_key = ?1 AND o.status = 'completed'
         GROUP BY code ORDER BY COUNT(*) DESC, code",
    )
    .bind(key)
    .fetch_all(pool)
    .await?;
    let spellings: Vec<String> = sqlx::query_scalar(
        "SELECT oi.name FROM order_items oi JOIN orders o ON o.id = oi.order_id
         WHERE oi.is_custom = 1 AND oi.custom_key = ?1
         GROUP BY oi.name ORDER BY MAX(oi.created_at) DESC LIMIT 10",
    )
    .bind(key)
    .fetch_all(pool)
    .await?;
    Ok(QuickSaleDetail {
        custom_key: key.to_owned(),
        median_price: median,
        counters: counters
            .into_iter()
            .map(|(device_code, times_sold, units)| CounterSales { device_code, times_sold, units })
            .collect(),
        spellings,
    })
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct PromoteInput {
    pub custom_key: String,
    /// The product name (defaults in the UI to the latest spelling; editable).
    pub name: String,
    #[specta(type = specta_typescript::Number)]
    pub price: i64,
    #[specta(type = specta_typescript::Number)]
    pub default_cost: i64,
    pub category_id: Option<String>,
    /// Give the new product an in-store barcode (for labels) as well as its SKU.
    pub generate_barcode: bool,
    /// Units on the shelf right now (`None` = none counted).
    #[specta(type = Option<specta_typescript::Number>)]
    pub shelf_qty: Option<i64>,
    /// A received batch the shelf stock belongs to; `None` = the `ADJUSTMENTS` batch at `default_cost`.
    pub batch_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct PromoteResult {
    pub product: products::Product,
    #[specta(type = specta_typescript::Number)]
    pub shelf_qty: i64,
}

fn audit_ctx(ctx: &WriteCtx) -> AuditCtx<'_> {
    AuditCtx { store_id: &ctx.store_id, device_id: &ctx.device_id, business_date: &ctx.business_date, now: &ctx.now }
}

async fn existing_link(conn: &mut SqliteConnection, store_id: &str, key: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT action FROM quick_sale_links WHERE store_id = ?1 AND custom_key = ?2 AND deleted_at IS NULL",
    )
    .bind(store_id)
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?)
}

async fn known_key(conn: &mut SqliteConnection, key: &str) -> AppResult<String> {
    sqlx::query_scalar("SELECT display_name FROM quick_sale_stats WHERE custom_key = ?1")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::not_found("quick sale"))
}

/// A decision about one key.
struct Decision<'a> {
    key: &'a str,
    display_name: &'a str,
    action: &'a str,
    product_id: Option<&'a str>,
    batch_id: Option<&'a str>,
}

/// Upsert the decision row (one per key, id = UUIDv5(store, key)).
async fn link(conn: &mut SqliteConnection, ctx: &WriteCtx, actor: &Actor, d: &Decision<'_>) -> AppResult<String> {
    let (key, display_name, action, product_id, batch_id) = (d.key, d.display_name, d.action, d.product_id, d.batch_id);
    let id = stable_id("quick_sale", &ctx.store_id, key);
    sqlx::query(
        "INSERT INTO quick_sale_links (id, store_id, custom_key, display_name, action, product_id, batch_id,
                                       decided_by, decided_at, created_at, updated_at, updated_by_device)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name, action = excluded.action,
           product_id = excluded.product_id, batch_id = excluded.batch_id, decided_by = excluded.decided_by,
           decided_at = excluded.decided_at, updated_at = excluded.updated_at,
           updated_by_device = excluded.updated_by_device, deleted_at = NULL, version = version + 1",
    )
    .bind(&id)
    .bind(&ctx.store_id)
    .bind(key)
    .bind(display_name)
    .bind(action)
    .bind(product_id)
    .bind(batch_id)
    .bind(&actor.staff_id)
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "quick_sale_links", &id, &ctx.now).await?;
    Ok(id)
}

/// Promote a quick-sale key to a real product (one transaction).
pub async fn promote(db: &Db, ctx: &WriteCtx, actor: &Actor, input: &PromoteInput) -> AppResult<PromoteResult> {
    let shelf = input.shelf_qty.unwrap_or(0);
    if !(0..=SHELF_MAX).contains(&shelf) {
        return Err(AppError::validation("QTY_INVALID", shelf.to_string()));
    }
    let mut tx = db.write().await?;
    let conn = tx.conn();
    known_key(conn, &input.custom_key).await?;
    if existing_link(conn, &ctx.store_id, &input.custom_key).await?.as_deref() == Some("promoted") {
        return Err(AppError::validation("QUICK_SALE_ALREADY_PROMOTED", input.custom_key.clone()));
    }
    let batch = match input.batch_id.as_deref() {
        Some(b) => {
            let ok: Option<(String, bool)> = sqlx::query_as(
                "SELECT status, is_system FROM batches WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL",
            )
            .bind(b)
            .bind(&ctx.store_id)
            .fetch_optional(&mut *conn)
            .await?;
            match ok {
                Some((s, false)) if s == "received" => Some(b),
                _ => return Err(AppError::validation("BATCH_NOT_RECEIVED", b.to_owned())),
            }
        }
        None => None,
    };
    let product_input = ProductInput {
        kind: ProductKind::Standard,
        name: input.name.clone(),
        name_si: None,
        sku: CodeChoice::Generate,
        barcode: if input.generate_barcode { CodeChoice::Generate } else { CodeChoice::None },
        category_id: input.category_id.clone(),
        default_cost: Some(input.default_cost),
        price: input.price,
        min_price: None,
        track_stock: true,
        allow_negative_stock: true,
        attributes: std::collections::BTreeMap::new(),
        is_active: true,
    };
    let product_id = products::create_conn(conn, ctx, actor, &product_input, true).await?;

    if shelf > 0 {
        let layer = match batch {
            Some(b) => {
                // A layer in the chosen batch at the manager's cost (added after receipt).
                let id = new_id();
                sqlx::query(
                    "INSERT INTO batch_items (id, store_id, batch_id, product_id, qty_received, unit_cost,
                                              retail_price_at_intake, manual_unit_cost, added_after_receipt, note,
                                              created_at, updated_at, updated_by_device)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, 1, 'promoted quick sale', ?8, ?8, ?9)",
                )
                .bind(&id)
                .bind(&ctx.store_id)
                .bind(b)
                .bind(&product_id)
                .bind(shelf)
                .bind(input.default_cost)
                .bind(input.price)
                .bind(&ctx.now)
                .bind(&ctx.device_id)
                .execute(&mut *conn)
                .await?;
                outbox::enqueue_upsert(conn, "batch_items", &id, &ctx.now).await?;
                (id, input.default_cost, StockReason::BatchIntake)
            }
            None => {
                let l = stock::newest_layer(conn, ctx, &product_id).await?;
                (l.batch_item_id, l.unit_cost, StockReason::CountCorrection)
            }
        };
        stock::record(
            conn,
            ctx,
            &Movement {
                product_id: &product_id,
                batch_item_id: &layer.0,
                qty_delta: shelf,
                reason: layer.2,
                ref_type: Some("quick_sale_promote"),
                ref_id: Some(&input.custom_key),
                order_item_id: None,
                unit_cost: layer.1,
                revenue_net: None,
                staff_id: Some(&actor.staff_id),
                approved_by: None,
                note: Some("shelf count at promotion"),
            },
        )
        .await?;
    }
    let decision = Decision {
        key: &input.custom_key,
        display_name: &input.name,
        action: "promoted",
        product_id: Some(&product_id),
        batch_id: batch,
    };
    link(conn, ctx, actor, &decision).await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::QUICK_SALE_PROMOTE,
            staff_id: Some(&actor.staff_id),
            approved_by: None,
            entity_type: Some("product"),
            entity_id: Some(&product_id),
            metadata: json!({ "custom_key": input.custom_key, "name": input.name, "price": input.price,
                              "default_cost": input.default_cost, "shelf_qty": shelf, "batch_id": batch }),
        },
    )
    .await?;
    let product = products::get_on(conn, &ctx.store_id, &product_id, true).await?;
    tx.commit().await?;
    Ok(PromoteResult { product, shelf_qty: shelf })
}

/// Hide a one-off key from the review.
pub async fn ignore(db: &Db, ctx: &WriteCtx, actor: &Actor, key: &str) -> AppResult<()> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let name = known_key(conn, key).await?;
    if existing_link(conn, &ctx.store_id, key).await?.as_deref() == Some("promoted") {
        return Err(AppError::validation("QUICK_SALE_ALREADY_PROMOTED", key.to_owned()));
    }
    let decision = Decision { key, display_name: &name, action: "ignored", product_id: None, batch_id: None };
    link(conn, ctx, actor, &decision).await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::QUICK_SALE_IGNORE,
            staff_id: Some(&actor.staff_id),
            approved_by: None,
            entity_type: Some("quick_sale"),
            entity_id: Some(key),
            metadata: json!({ "custom_key": key }),
        },
    )
    .await?;
    tx.commit().await
}

/// What the till's Quick Sale bar shows when the typed name is a promoted key: "Now a product: …".
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct PromotedHint {
    pub product: products::ProductSummary,
}

pub async fn hint(pool: &SqlitePool, store_id: &str, typed_name: &str) -> AppResult<Option<PromotedHint>> {
    let key = custom_key(typed_name);
    if key.is_empty() {
        return Ok(None);
    }
    let product_id: Option<String> = sqlx::query_scalar(
        "SELECT l.product_id FROM quick_sale_links l JOIN products p ON p.id = l.product_id
         WHERE l.store_id = ?1 AND l.custom_key = ?2 AND l.action = 'promoted' AND l.deleted_at IS NULL
           AND p.deleted_at IS NULL AND p.is_active = 1",
    )
    .bind(store_id)
    .bind(&key)
    .fetch_optional(pool)
    .await?;
    match product_id {
        Some(id) => Ok(products::summary(pool, store_id, &id).await?.map(|product| PromotedHint { product })),
        None => Ok(None),
    }
}
