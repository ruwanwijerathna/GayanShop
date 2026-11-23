//! Reports engine (ARCHITECTURE §5): the §5.1 metrics for every §5.2 period, with an optional counter filter
//! and the previous equivalent period for comparison; trend buckets for the charts; sales by product,
//! category and batch; voids by reason and approver; and `rebuild_derived`, which recomputes every derived
//! table from the base tables.
//!
//! Sales and payment metrics read the rollups (`daily_sales_summary`, `daily_payment_summary`), so a yearly
//! report is a few hundred rows. Allocation variance, write-offs and expenses are store-wide (they belong to
//! no counter), so the counter filter applies to sales metrics only.

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::db::Db;
use crate::domain::business_date::{StoreCalendar, parse_date};
use crate::domain::period::{DateRange, Granularity, Period, bucket_of, bucket_sql};
use crate::domain::receipt_no;
use crate::error::AppResult;

/// Inclusive business-date bounds for SQL: an open end becomes a sentinel, so every filter is a plain
/// `BETWEEN` that SQLite serves from the date indexes (`ix_mov_date_reason`, `ix_expenses_bdate`,
/// `ix_orders_bdate`, the rollups' primary keys) instead of an `?1 IS NULL OR …` scan.
pub fn bounds(range: &DateRange) -> (&str, &str) {
    (range.from.as_deref().unwrap_or("0000-01-01"), range.to.as_deref().unwrap_or("9999-12-31"))
}

/// Sales metrics for a date range and optional counter (rollup rows only).
pub const SQL_SALES_SUMS: &str =
    "SELECT COALESCE(SUM(sales_count), 0) AS sales_count, COALESCE(SUM(void_count), 0) AS void_count,
                COALESCE(SUM(void_value), 0) AS void_value, COALESCE(SUM(gross_sales), 0) AS gross_sales,
                COALESCE(SUM(discounts), 0) AS discounts, COALESCE(SUM(net_sales), 0) AS net_sales,
                COALESCE(SUM(tax_total), 0) AS tax_total, COALESCE(SUM(cogs), 0) AS cogs,
                COALESCE(SUM(quick_sale_net), 0) AS quick_sale_net,
                COALESCE(SUM(unknown_cost_net), 0) AS unknown_cost_net,
                COALESCE(SUM(customer_orders), 0) AS customer_orders
         FROM daily_sales_summary
         WHERE business_date BETWEEN ?1 AND ?2
           AND (?3 IS NULL OR device_id = ?3)";

/// Payments by method from the payment rollup.
pub const SQL_PAYMENTS: &str =
    "SELECT method, SUM(amount) AS amount, SUM(txn_count) AS count FROM daily_payment_summary
         WHERE business_date BETWEEN ?1 AND ?2
           AND (?3 IS NULL OR device_id = ?3)
         GROUP BY method HAVING SUM(txn_count) > 0 ORDER BY SUM(amount) DESC, method";

/// Allocation variance of batches received in the range.
pub const SQL_VARIANCE: &str = "SELECT COALESCE(SUM(allocation_variance), 0) FROM batches
         WHERE status <> 'draft' AND deleted_at IS NULL
           AND received_business_date BETWEEN ?1 AND ?2";

/// Operating expenses in the range (`ix_expenses_bdate`).
pub const SQL_OPEX: &str = "SELECT COALESCE(SUM(amount), 0) FROM expenses
         WHERE deleted_at IS NULL AND business_date BETWEEN ?1 AND ?2";

/// Stock write-offs (§5.1: damage, count corrections and adjustments, net of found stock) in the range
/// (`ix_mov_date_reason`).
pub const SQL_WRITE_OFFS: &str = "SELECT COALESCE(SUM(-qty_delta * unit_cost), 0) FROM stock_movements
         WHERE reason IN ('damage','count_correction','adjustment')
           AND business_date BETWEEN ?1 AND ?2";

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct MethodTotal {
    pub method: String,
    #[specta(type = specta_typescript::Number)]
    pub amount: i64,
    #[specta(type = specta_typescript::Number)]
    pub count: i64,
}

/// The §5.1 metrics for one period. Cost-derived fields are `None` when the viewer may not see costs
/// (`REPORT_FINANCIAL` + `VIEW_COST`; see [`FinancialSummary::without_costs`]).
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct FinancialSummary {
    pub range: DateRange,
    /// `None` = the whole store.
    pub device_id: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub sales_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub void_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub void_value: i64,
    #[specta(type = specta_typescript::Number)]
    pub gross_sales: i64,
    #[specta(type = specta_typescript::Number)]
    pub discounts: i64,
    /// Total revenue, ex tax, completed orders only.
    #[specta(type = specta_typescript::Number)]
    pub net_sales: i64,
    #[specta(type = specta_typescript::Number)]
    pub tax_collected: i64,
    #[specta(type = specta_typescript::Number)]
    pub customer_orders: i64,
    #[specta(type = specta_typescript::Number)]
    pub customer_attached_bp: i64,
    /// `net_sales / sales_count`, rounded down.
    #[specta(type = specta_typescript::Number)]
    pub avg_basket: i64,
    #[specta(type = specta_typescript::Number)]
    pub quick_sale_net: i64,
    #[specta(type = specta_typescript::Number)]
    pub unknown_cost_net: i64,
    pub payments: Vec<MethodTotal>,
    /// Sale-slice cost of the period's sales (before allocation variance).
    #[specta(type = Option<specta_typescript::Number>)]
    pub sales_cost: Option<i64>,
    /// Σ `allocation_variance` of batches received in the period (store-wide).
    #[specta(type = Option<specta_typescript::Number>)]
    pub allocation_variance: Option<i64>,
    /// `sales_cost + allocation_variance`.
    #[specta(type = Option<specta_typescript::Number>)]
    pub cogs: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub gross_profit: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub gross_margin_bp: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub stock_write_offs: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub operating_expenses: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub net_profit: Option<i64>,
}

impl FinancialSummary {
    /// Sales-only view for `REPORT_SALES` without costs (doc §5 report permissions).
    pub fn without_costs(mut self) -> Self {
        self.sales_cost = None;
        self.allocation_variance = None;
        self.cogs = None;
        self.gross_profit = None;
        self.gross_margin_bp = None;
        self.stock_write_offs = None;
        self.operating_expenses = None;
        self.net_profit = None;
        self
    }
}

fn bp(part: i64, whole: i64) -> i64 {
    if whole == 0 { 0 } else { part * 10_000 / whole }
}

#[derive(FromRow)]
struct SalesSums {
    sales_count: i64,
    void_count: i64,
    void_value: i64,
    gross_sales: i64,
    discounts: i64,
    net_sales: i64,
    tax_total: i64,
    cogs: i64,
    quick_sale_net: i64,
    unknown_cost_net: i64,
    customer_orders: i64,
}

/// The P&L of doc §5.3 for inclusive bounds (`None` = open-ended) and an optional counter.
pub async fn summary_for(pool: &SqlitePool, range: &DateRange, device_id: Option<&str>) -> AppResult<FinancialSummary> {
    let (from, to) = bounds(range);
    let s: SalesSums = sqlx::query_as(SQL_SALES_SUMS).bind(from).bind(to).bind(device_id).fetch_one(pool).await?;
    let payments: Vec<MethodTotal> =
        sqlx::query_as(SQL_PAYMENTS).bind(from).bind(to).bind(device_id).fetch_all(pool).await?;
    let variance: i64 = sqlx::query_scalar(SQL_VARIANCE).bind(from).bind(to).fetch_one(pool).await?;
    let write_offs: i64 = sqlx::query_scalar(SQL_WRITE_OFFS).bind(from).bind(to).fetch_one(pool).await?;
    let opex: i64 = sqlx::query_scalar(SQL_OPEX).bind(from).bind(to).fetch_one(pool).await?;
    let cogs = s.cogs + variance;
    let gross_profit = s.net_sales - cogs;
    Ok(FinancialSummary {
        range: range.clone(),
        device_id: device_id.map(str::to_owned),
        sales_count: s.sales_count,
        void_count: s.void_count,
        void_value: s.void_value,
        gross_sales: s.gross_sales,
        discounts: s.discounts,
        net_sales: s.net_sales,
        tax_collected: s.tax_total,
        customer_orders: s.customer_orders,
        customer_attached_bp: bp(s.customer_orders, s.sales_count),
        avg_basket: if s.sales_count > 0 { s.net_sales / s.sales_count } else { 0 },
        quick_sale_net: s.quick_sale_net,
        unknown_cost_net: s.unknown_cost_net,
        payments,
        sales_cost: Some(s.cogs),
        allocation_variance: Some(variance),
        cogs: Some(cogs),
        gross_profit: Some(gross_profit),
        gross_margin_bp: Some(bp(gross_profit, s.net_sales)),
        stock_write_offs: Some(write_offs),
        operating_expenses: Some(opex),
        net_profit: Some(gross_profit - write_offs - opex),
    })
}

/// A period's report and, on request, the previous equivalent period's (§5.2 comparison).
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct PeriodReport {
    pub current: FinancialSummary,
    pub previous: Option<FinancialSummary>,
}

pub async fn report(
    pool: &SqlitePool,
    cal: &StoreCalendar,
    period: &Period,
    device_id: Option<&str>,
    compare: bool,
) -> AppResult<PeriodReport> {
    let current = summary_for(pool, &period.resolve(cal)?, device_id).await?;
    let previous = match (compare, period.previous(cal)?) {
        (true, Some(prev)) => Some(summary_for(pool, &prev.resolve(cal)?, device_id).await?),
        _ => None,
    };
    Ok(PeriodReport { current, previous })
}

/// One bucket of the trend charts (revenue vs expenses, sales trend).
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct TrendPoint {
    /// The date, the week's first day, or `YYYY-MM`.
    pub bucket: String,
    #[specta(type = specta_typescript::Number)]
    pub sales_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub net_sales: i64,
    /// Sale-slice cost (allocation variance is a batch-level figure and stays in the summary).
    #[specta(type = Option<specta_typescript::Number>)]
    pub sales_cost: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub expenses: Option<i64>,
}

/// Buckets with any sales or expenses, oldest first. Expenses are store-wide.
pub async fn trend(
    pool: &SqlitePool,
    cal: &StoreCalendar,
    period: &Period,
    device_id: Option<&str>,
    granularity: Granularity,
) -> AppResult<Vec<TrendPoint>> {
    let range = period.resolve(cal)?;
    let sb = bucket_sql("business_date", granularity, cal);
    let rows: Vec<TrendPoint> = sqlx::query_as(&format!(
        "WITH s AS (
           SELECT {sb} AS bucket, SUM(sales_count) AS sales_count, SUM(net_sales) AS net_sales, SUM(cogs) AS cost
           FROM daily_sales_summary
           WHERE business_date BETWEEN ?1 AND ?2
             AND (?3 IS NULL OR device_id = ?3)
           GROUP BY bucket),
         e AS (
           SELECT {sb} AS bucket, SUM(amount) AS amount FROM expenses
           WHERE deleted_at IS NULL AND business_date BETWEEN ?1 AND ?2
           GROUP BY bucket),
         b AS (SELECT bucket FROM s UNION SELECT bucket FROM e)
         SELECT b.bucket, COALESCE(s.sales_count, 0) AS sales_count, COALESCE(s.net_sales, 0) AS net_sales,
                COALESCE(s.cost, 0) AS sales_cost, COALESCE(e.amount, 0) AS expenses
         FROM b LEFT JOIN s ON s.bucket = b.bucket LEFT JOIN e ON e.bucket = b.bucket
         WHERE COALESCE(s.sales_count, 0) <> 0 OR COALESCE(e.amount, 0) <> 0 OR COALESCE(s.net_sales, 0) <> 0
         ORDER BY b.bucket"
    ))
    .bind(bounds(&range).0)
    .bind(bounds(&range).1)
    .bind(device_id)
    .fetch_all(pool)
    .await?;
    Ok(densify(rows, &range, granularity, cal))
}

/// Charts read one point per bucket, so a bounded period gets every bucket from its first day to its last
/// (empty ones as zeros) and a previous-period overlay lines up point for point. All time stays sparse.
const DENSE_MAX: usize = 400;

fn densify(rows: Vec<TrendPoint>, range: &DateRange, g: Granularity, cal: &StoreCalendar) -> Vec<TrendPoint> {
    let (Some(from), Some(to)) = (range.from.as_deref().and_then(parse_date), range.to.as_deref().and_then(parse_date))
    else {
        return rows;
    };
    let mut buckets: Vec<String> = Vec::new();
    let mut d = from;
    while d <= to {
        let b = bucket_of(d, g, cal);
        if buckets.last() != Some(&b) {
            buckets.push(b);
            if buckets.len() > DENSE_MAX {
                return rows;
            }
        }
        d += chrono::Duration::days(1);
    }
    let zero =
        |bucket: String| TrendPoint { bucket, sales_count: 0, net_sales: 0, sales_cost: Some(0), expenses: Some(0) };
    let mut rows = rows.into_iter().peekable();
    buckets
        .into_iter()
        .map(|b| match rows.peek() {
            Some(r) if r.bucket == b => rows.next().unwrap_or_else(|| zero(b)),
            _ => zero(b),
        })
        .collect()
}

/// The trend and, on request, the previous equivalent period's, aligned bucket by bucket.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct TrendSeries {
    pub current: Vec<TrendPoint>,
    pub previous: Option<Vec<TrendPoint>>,
}

pub async fn trend_series(
    pool: &SqlitePool,
    cal: &StoreCalendar,
    period: &Period,
    device_id: Option<&str>,
    granularity: Granularity,
    compare: bool,
) -> AppResult<TrendSeries> {
    let current = trend(pool, cal, period, device_id, granularity).await?;
    let previous = match (compare, period.previous(cal)?) {
        (true, Some(prev)) => Some(trend(pool, cal, &prev, device_id, granularity).await?),
        _ => None,
    };
    Ok(TrendSeries { current, previous })
}

/// Batch sell-through (§1.4 chart): units received, sold, written off and left per received batch.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct BatchSellThrough {
    pub batch_id: String,
    pub code: String,
    pub name: String,
    pub status: String,
    #[specta(type = specta_typescript::Number)]
    pub received: i64,
    #[specta(type = specta_typescript::Number)]
    pub sold: i64,
    #[specta(type = specta_typescript::Number)]
    pub written_off: i64,
    #[specta(type = specta_typescript::Number)]
    pub on_hand: i64,
    /// `sold / received` in basis points.
    #[specta(type = specta_typescript::Number)]
    pub sell_through_bp: i64,
}

/// The most recently received batches (system batches left out), newest first, from `batch_item_stock`.
pub async fn batch_sell_through(pool: &SqlitePool, store_id: &str, limit: u32) -> AppResult<Vec<BatchSellThrough>> {
    Ok(sqlx::query_as(
        "SELECT b.id AS batch_id, b.code, b.name, b.status,
                COALESCE(SUM(bis.received_qty), 0) AS received, COALESCE(SUM(bis.sold_qty), 0) AS sold,
                COALESCE(SUM(bis.written_off_qty), 0) AS written_off, COALESCE(SUM(bis.on_hand), 0) AS on_hand,
                CASE WHEN COALESCE(SUM(bis.received_qty), 0) > 0
                     THEN SUM(bis.sold_qty) * 10000 / SUM(bis.received_qty) ELSE 0 END AS sell_through_bp
         FROM batches b LEFT JOIN batch_item_stock bis ON bis.batch_id = b.id
         WHERE b.store_id = ?1 AND b.is_system = 0 AND b.status <> 'draft' AND b.deleted_at IS NULL
         GROUP BY b.id ORDER BY b.received_at DESC, b.id LIMIT ?2",
    )
    .bind(store_id)
    .bind(i64::from(limit.clamp(1, 50)))
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SalesDimension {
    Product,
    Category,
    Batch,
}

/// One row of "sales by product / category / batch".
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct SalesByRow {
    /// Product, category or batch id. `None` = quick sales (by product), uncategorised (by category), or
    /// sales from no batch layer (by batch: untracked products and untagged quick sales).
    pub id: Option<String>,
    pub label: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
    #[specta(type = specta_typescript::Number)]
    pub net: i64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub cost: Option<i64>,
}

/// Completed sales in the period grouped by product, category or batch layer, by revenue. Every row set sums
/// to the period's net sales and sale cost. By batch, revenue and cost come from the sale slices (and tagged
/// quick sales count towards their batch); the rest is one "no batch" row.
pub async fn sales_by(
    pool: &SqlitePool,
    cal: &StoreCalendar,
    period: &Period,
    device_id: Option<&str>,
    dimension: SalesDimension,
) -> AppResult<Vec<SalesByRow>> {
    let range = period.resolve(cal)?;
    const SCOPE: &str =
        "o.status = 'completed' AND o.business_date BETWEEN ?1 AND ?2 AND (?3 IS NULL OR o.device_id = ?3)";
    let sql = match dimension {
        // Returned goods are netted off in the period of the exchange that took them back (like the daily rollup).
        SalesDimension::Product => format!(
            "WITH lines AS (
               SELECT oi.product_id AS pid, oi.qty AS qty, oi.line_net AS net, oi.cost_total AS cost
               FROM order_items oi JOIN orders o ON o.id = oi.order_id WHERE {SCOPE}
               UNION ALL
               SELECT src.product_id, -r.qty, -r.line_net, -r.cost_total
               FROM order_returns r JOIN orders o ON o.id = r.order_id
               JOIN order_items src ON src.id = r.source_item_id WHERE {SCOPE})
             SELECT l.pid AS id, CASE WHEN l.pid IS NULL THEN NULL ELSE MAX(p.name) END AS label,
                    SUM(l.qty) AS qty, SUM(l.net) AS net, SUM(l.cost) AS cost
             FROM lines l LEFT JOIN products p ON p.id = l.pid
             GROUP BY l.pid ORDER BY net DESC, qty DESC, id"
        ),
        SalesDimension::Category => format!(
            "WITH lines AS (
               SELECT oi.product_id AS pid, oi.qty AS qty, oi.line_net AS net, oi.cost_total AS cost
               FROM order_items oi JOIN orders o ON o.id = oi.order_id WHERE {SCOPE}
               UNION ALL
               SELECT src.product_id, -r.qty, -r.line_net, -r.cost_total
               FROM order_returns r JOIN orders o ON o.id = r.order_id
               JOIN order_items src ON src.id = r.source_item_id WHERE {SCOPE})
             SELECT p.category_id AS id, MAX(c.name) AS label, SUM(l.qty) AS qty, SUM(l.net) AS net,
                    SUM(l.cost) AS cost
             FROM lines l LEFT JOIN products p ON p.id = l.pid
             LEFT JOIN categories c ON c.id = p.category_id
             GROUP BY p.category_id ORDER BY net DESC, qty DESC, id"
        ),
        SalesDimension::Batch => format!(
            "WITH slices AS (
               SELECT bi.batch_id AS batch_id, -m.qty_delta AS qty, COALESCE(m.revenue_net, 0) AS net,
                      -m.qty_delta * m.unit_cost AS cost
               FROM stock_movements m
               JOIN order_items oi ON oi.id = m.order_item_id JOIN orders o ON o.id = oi.order_id
               JOIN batch_items bi ON bi.id = m.batch_item_id
               WHERE m.reason = 'sale' AND {SCOPE}
               UNION ALL
               SELECT bi.batch_id, -m.qty_delta, COALESCE(m.revenue_net, 0), -m.qty_delta * m.unit_cost
               FROM stock_movements m
               JOIN orders o ON o.id = m.ref_id
               JOIN batch_items bi ON bi.id = m.batch_item_id
               WHERE m.reason = 'return' AND {SCOPE}),
             tagged AS (
               SELECT oi.batch_id AS batch_id, oi.qty AS qty, oi.line_net AS net, oi.cost_total AS cost
               FROM order_items oi JOIN orders o ON o.id = oi.order_id
               WHERE oi.is_custom = 1 AND oi.batch_id IS NOT NULL AND {SCOPE}),
             layered AS (SELECT * FROM slices UNION ALL SELECT * FROM tagged),
             total AS (
               SELECT COALESCE(SUM(x.qty), 0) AS qty, COALESCE(SUM(x.net), 0) AS net, COALESCE(SUM(x.cost), 0) AS cost
               FROM (SELECT oi.qty AS qty, oi.line_net AS net, oi.cost_total AS cost
                     FROM order_items oi JOIN orders o ON o.id = oi.order_id WHERE {SCOPE}
                     UNION ALL
                     SELECT -r.qty, -r.line_net, -r.cost_total
                     FROM order_returns r JOIN orders o ON o.id = r.order_id WHERE {SCOPE}) x),
             by_batch AS (
               SELECT l.batch_id AS id, MAX(b.code || ' · ' || b.name) AS label, SUM(l.qty) AS qty,
                      SUM(l.net) AS net, SUM(l.cost) AS cost
               FROM layered l JOIN batches b ON b.id = l.batch_id GROUP BY l.batch_id)
             SELECT id, label, qty, net, cost FROM by_batch
             UNION ALL
             SELECT NULL, NULL, t.qty - COALESCE((SELECT SUM(qty) FROM by_batch), 0),
                    t.net - COALESCE((SELECT SUM(net) FROM by_batch), 0),
                    t.cost - COALESCE((SELECT SUM(cost) FROM by_batch), 0)
             FROM total t
             WHERE t.qty - COALESCE((SELECT SUM(qty) FROM by_batch), 0) <> 0
                OR t.net - COALESCE((SELECT SUM(net) FROM by_batch), 0) <> 0
             ORDER BY net DESC, qty DESC, id"
        ),
    };
    Ok(sqlx::query_as(&sql).bind(bounds(&range).0).bind(bounds(&range).1).bind(device_id).fetch_all(pool).await?)
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct VoidGroup {
    /// The void reason code, or the approving staff id.
    pub key: String,
    pub label: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub count: i64,
    #[specta(type = specta_typescript::Number)]
    pub value: i64,
}

/// Voids for control (§5.1): shown, never counted in revenue.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct VoidReport {
    #[specta(type = specta_typescript::Number)]
    pub count: i64,
    #[specta(type = specta_typescript::Number)]
    pub value: i64,
    pub by_reason: Vec<VoidGroup>,
    /// By the supervisor who approved (or the voider, when their own role holds `VOID_ORDER`).
    pub by_approver: Vec<VoidGroup>,
}

pub async fn voids(
    pool: &SqlitePool,
    cal: &StoreCalendar,
    period: &Period,
    device_id: Option<&str>,
) -> AppResult<VoidReport> {
    let range = period.resolve(cal)?;
    const SCOPE: &str =
        "o.status = 'voided' AND o.business_date BETWEEN ?1 AND ?2 AND (?3 IS NULL OR o.device_id = ?3)";
    let by_reason: Vec<VoidGroup> = sqlx::query_as(&format!(
        "SELECT o.void_reason AS key, NULL AS label, COUNT(*) AS count, SUM(o.grand_total) AS value
         FROM orders o WHERE {SCOPE} GROUP BY o.void_reason ORDER BY count DESC, value DESC, key"
    ))
    .bind(bounds(&range).0)
    .bind(bounds(&range).1)
    .bind(device_id)
    .fetch_all(pool)
    .await?;
    let by_approver: Vec<VoidGroup> = sqlx::query_as(&format!(
        "SELECT COALESCE(o.void_approved_by, o.voided_by) AS key, MAX(s.display_name) AS label, COUNT(*) AS count,
                SUM(o.grand_total) AS value
         FROM orders o LEFT JOIN staff s ON s.id = COALESCE(o.void_approved_by, o.voided_by)
         WHERE {SCOPE} GROUP BY key ORDER BY count DESC, value DESC, key"
    ))
    .bind(bounds(&range).0)
    .bind(bounds(&range).1)
    .bind(device_id)
    .fetch_all(pool)
    .await?;
    Ok(VoidReport {
        count: by_reason.iter().map(|g| g.count).sum(),
        value: by_reason.iter().map(|g| g.value).sum(),
        by_reason,
        by_approver,
    })
}

/// A counter for the report's counter filter.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct ReportDevice {
    pub device_id: String,
    /// `T01`…, from its receipt numbers (`T01-…` or the letter of `A1234`).
    pub device_code: String,
}

pub async fn devices(pool: &SqlitePool) -> AppResult<Vec<ReportDevice>> {
    Ok(sqlx::query_as(&format!(
        "SELECT device_id, MIN({}) AS device_code
         FROM orders WHERE status <> 'pending' GROUP BY device_id ORDER BY device_code",
        receipt_no::device_code_sql("receipt_no"),
    ))
    .fetch_all(pool)
    .await?)
}

// ───────── rebuild_derived (doc §5.3) ─────────

/// Rows written per derived table by [`rebuild_derived`].
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct RebuildReport {
    #[specta(type = specta_typescript::Number)]
    pub daily_sales_summary: i64,
    #[specta(type = specta_typescript::Number)]
    pub daily_payment_summary: i64,
    #[specta(type = specta_typescript::Number)]
    pub customer_stats: i64,
    #[specta(type = specta_typescript::Number)]
    pub quick_sale_stats: i64,
    #[specta(type = specta_typescript::Number)]
    pub stock_levels: i64,
    #[specta(type = specta_typescript::Number)]
    pub batch_item_stock: i64,
    #[specta(type = specta_typescript::Number)]
    pub products_fts: i64,
}

async fn count(conn: &mut SqliteConnection, table: &str) -> AppResult<i64> {
    Ok(sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}")).fetch_one(&mut *conn).await?)
}

/// Recompute every derived table from the base tables, in one transaction: run after a restore, after a
/// migration touching derived logic, and from Settings → Maintenance. The trigger-maintained values and a
/// rebuild agree exactly (tested on randomised data); a rebuild only drops all-zero leftovers (a payment
/// method whose only sale was voided).
pub async fn recompute(conn: &mut SqliteConnection) -> AppResult<RebuildReport> {
    for stmt in [
        "DELETE FROM daily_sales_summary",
        "INSERT INTO daily_sales_summary (business_date, device_id, sales_count, void_count, void_value, gross_sales,
                                          discounts, net_sales, tax_total, cogs, quick_sale_net, unknown_cost_net,
                                          customer_orders)
         SELECT business_date, device_id,
                SUM(status = 'completed'), SUM(status = 'voided'),
                SUM(CASE WHEN status = 'voided' THEN grand_total ELSE 0 END),
                SUM(CASE WHEN status = 'completed' THEN gross_sales - returned_gross ELSE 0 END),
                SUM(CASE WHEN status = 'completed' THEN discount_total - returned_discount ELSE 0 END),
                SUM(CASE WHEN status = 'completed' THEN net_sales - returned_net ELSE 0 END),
                SUM(CASE WHEN status = 'completed' THEN tax_total - returned_tax ELSE 0 END),
                SUM(CASE WHEN status = 'completed' THEN cost_total - returned_cost ELSE 0 END),
                SUM(CASE WHEN status = 'completed' THEN quick_sale_net ELSE 0 END),
                SUM(CASE WHEN status = 'completed' THEN unknown_cost_net ELSE 0 END),
                SUM(status = 'completed' AND customer_id IS NOT NULL)
         FROM orders WHERE status IN ('completed', 'voided')
         GROUP BY business_date, device_id",
        "DELETE FROM daily_payment_summary",
        "INSERT INTO daily_payment_summary (business_date, device_id, method, amount, txn_count)
         SELECT o.business_date, o.device_id, p.method, SUM(p.amount), COUNT(*)
         FROM order_payments p JOIN orders o ON o.id = p.order_id
         WHERE o.status = 'completed'
         GROUP BY o.business_date, o.device_id, p.method",
        "DELETE FROM customer_stats",
        "INSERT INTO customer_stats (customer_id, visit_count, total_spend, total_net, first_visit_date, last_visit_date)
         SELECT customer_id, SUM(status = 'completed'),
                SUM(CASE WHEN status = 'completed' THEN grand_total - exchange_credit ELSE 0 END),
                SUM(CASE WHEN status = 'completed' THEN net_sales - returned_net ELSE 0 END),
                MIN(CASE WHEN status = 'completed' THEN business_date END),
                MAX(CASE WHEN status = 'completed' THEN business_date END)
         FROM orders WHERE customer_id IS NOT NULL AND status IN ('completed', 'voided')
         GROUP BY customer_id",
        "DELETE FROM quick_sale_stats",
        "INSERT INTO quick_sale_stats (custom_key, display_name, times_sold, units_sold, revenue_net, last_price,
                                       min_price, max_price, first_sold_at, last_sold_at)
         SELECT oi.custom_key,
                (SELECT x.name FROM order_items x JOIN orders xo ON xo.id = x.order_id AND xo.status = 'completed'
                 WHERE x.custom_key = oi.custom_key AND x.is_custom = 1 ORDER BY x.created_at DESC LIMIT 1),
                COUNT(*), SUM(oi.qty), SUM(oi.line_net),
                (SELECT y.unit_price FROM order_items y JOIN orders yo ON yo.id = y.order_id AND yo.status = 'completed'
                 WHERE y.custom_key = oi.custom_key AND y.is_custom = 1 ORDER BY y.created_at DESC LIMIT 1),
                MIN(oi.unit_price), MAX(oi.unit_price), MIN(oi.created_at), MAX(oi.created_at)
         FROM order_items oi JOIN orders o ON o.id = oi.order_id AND o.status = 'completed'
         WHERE oi.is_custom = 1
         GROUP BY oi.custom_key",
        "DELETE FROM stock_levels",
        "INSERT INTO stock_levels (product_id, on_hand)
         SELECT product_id, SUM(qty_delta) FROM stock_movements GROUP BY product_id",
        "DELETE FROM batch_item_stock",
        "INSERT INTO batch_item_stock (batch_item_id, batch_id, product_id, received_qty, on_hand, sold_qty, sold_revenue,
                                       sold_cost, written_off_qty, written_off_cost)
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
        "DELETE FROM products_fts",
        "DELETE FROM products_fts_si",
        "INSERT INTO products_fts (id, name, sku, barcode) SELECT id, name, sku, barcode FROM products",
        "INSERT INTO products_fts_si (id, name_si) SELECT id, name_si FROM products WHERE name_si IS NOT NULL",
    ] {
        sqlx::query(stmt).execute(&mut *conn).await?;
    }
    Ok(RebuildReport {
        daily_sales_summary: count(conn, "daily_sales_summary").await?,
        daily_payment_summary: count(conn, "daily_payment_summary").await?,
        customer_stats: count(conn, "customer_stats").await?,
        quick_sale_stats: count(conn, "quick_sale_stats").await?,
        stock_levels: count(conn, "stock_levels").await?,
        batch_item_stock: count(conn, "batch_item_stock").await?,
        products_fts: count(conn, "products_fts").await?,
    })
}

/// Settings → Maintenance → "Rebuild reports": [`recompute`] plus a `SETTINGS_CHANGE` audit row.
pub async fn rebuild_derived(db: &Db, ctx: &WriteCtx, actor: &Actor) -> AppResult<RebuildReport> {
    let mut tx = db.write().await?;
    let report = recompute(tx.conn()).await?;
    audit::record(
        tx.conn(),
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::SETTINGS_CHANGE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("maintenance"),
            entity_id: Some("rebuild_derived"),
            metadata: json!(report),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(report)
}
