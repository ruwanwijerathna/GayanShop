//! Item-for-item exchanges (ARCHITECTURE §3.3, decision #26). The replacement goods go through the normal
//! checkout; this module owns the returned side: looking up the sale being exchanged, valuing the returned
//! units, planning the stock slices that put them back on the shelf, and writing both inside the checkout's
//! transaction.
//!
//! * Returned goods are valued at what the customer actually paid (after line and bill discounts), prorated per
//!   unit with cumulative flooring (`domain::exchange::portion`), so returning every unit of a line over any
//!   number of exchanges credits exactly the line's total.
//! * Every returned unit goes straight back into available stock, into the cost layer it was sold from: a
//!   `return` slice mirrors the original `sale` slice (same layer, same unit cost) with the revenue reversed.
//! * Only stock-tracked catalog lines can be returned: a quick-sale or untracked line has no layer to restock.

use std::collections::HashSet;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use super::stock::{self, Movement, StockReason};
use crate::clock::new_id;
use crate::db::outbox;
use crate::domain::business_date::parse_date;
use crate::domain::exchange::{self, DEFAULT_WINDOW_DAYS, MAX_WINDOW_DAYS};
use crate::domain::receipt_no::normalize_bill_query;
use crate::error::{AppError, AppResult};

const MAX_RETURN_LINES: usize = 100;
const QTY_MAX: i64 = 100_000;
/// The store setting holding the exchange window in days.
pub const WINDOW_SETTING: &str = "exchange.window_days";

/// One returned line: units of an earlier order's line.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct ExchangeReturn {
    pub item_id: String,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
}

/// The returned side of an exchange; the replacement goods are the checkout's ordinary lines.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct ExchangeInput {
    pub returns: Vec<ExchangeReturn>,
}

/// One stock slice that puts returned units back into the layer they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnSlice {
    pub batch_item_id: String,
    pub unit_cost: i64,
    pub qty: i64,
    /// Revenue reversed by this slice (positive; stored negated on the ledger row).
    pub revenue_net: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedReturn {
    pub id: String,
    pub source_item_id: String,
    pub source_receipt: String,
    pub name: String,
    pub product_id: String,
    pub qty: i64,
    pub gross: i64,
    pub discount: i64,
    pub total: i64,
    pub net: i64,
    pub tax: i64,
    pub cost: i64,
    pub slices: Vec<ReturnSlice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangePlan {
    pub returns: Vec<PlannedReturn>,
    /// What the returned goods were sold for: credited against the replacement.
    pub credit: i64,
    pub items: i64,
    pub gross: i64,
    pub discount: i64,
    pub net: i64,
    pub tax: i64,
    pub cost: i64,
    /// True when any returned line was sold more than the window ago: needs `EXCHANGE_LATE`.
    pub late: bool,
    pub window_days: i64,
}

#[derive(FromRow)]
struct SourceRow {
    id: String,
    name: String,
    product_id: Option<String>,
    is_custom: bool,
    qty: i64,
    line_gross: i64,
    line_total: i64,
    line_net: i64,
    tax_amount: i64,
    status: String,
    business_date: String,
    receipt_no: String,
    tracked: bool,
    returned: i64,
}

const SOURCE_SQL: &str = "SELECT oi.id, oi.name, oi.product_id, oi.is_custom, oi.qty, oi.line_gross,
        oi.line_total, oi.line_net, oi.tax_amount, o.status, o.business_date, o.receipt_no,
        COALESCE(p.track_stock, 0) AS tracked,
        COALESCE((SELECT SUM(r.qty) FROM order_returns r WHERE r.source_item_id = oi.id), 0) AS returned
     FROM order_items oi JOIN orders o ON o.id = oi.order_id LEFT JOIN products p ON p.id = oi.product_id";

/// The exchange window in days (`exchange.window_days`, default 7).
pub async fn window_days(conn: &mut SqliteConnection) -> AppResult<i64> {
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM store_settings WHERE key = ?1")
        .bind(WINDOW_SETTING)
        .fetch_optional(&mut *conn)
        .await?;
    window_from(raw)
}

fn window_from(raw: Option<String>) -> AppResult<i64> {
    match raw {
        None => Ok(DEFAULT_WINDOW_DAYS),
        Some(r) => match serde_json::from_str::<i64>(&r) {
            Ok(d) if (0..=MAX_WINDOW_DAYS).contains(&d) => Ok(d),
            _ => Err(AppError::internal(format!("setting {WINDOW_SETTING}: {r}"))),
        },
    }
}

fn today_of(ctx: &WriteCtx) -> AppResult<NaiveDate> {
    parse_date(&ctx.business_date).ok_or_else(|| AppError::internal("bad business date"))
}

/// Validate the returned lines, value them and plan the restocking slices. Writes nothing.
pub async fn plan(conn: &mut SqliteConnection, ctx: &WriteCtx, input: &ExchangeInput) -> AppResult<ExchangePlan> {
    if input.returns.is_empty() || input.returns.len() > MAX_RETURN_LINES {
        return Err(AppError::validation("RETURN_INVALID", format!("1–{MAX_RETURN_LINES} returned lines")));
    }
    let today = today_of(ctx)?;
    let window = window_days(conn).await?;
    let mut seen = HashSet::new();
    let mut returns = Vec::with_capacity(input.returns.len());
    let mut late = false;
    for r in &input.returns {
        if !(1..=QTY_MAX).contains(&r.qty) || !seen.insert(r.item_id.as_str()) {
            return Err(AppError::validation("RETURN_INVALID", r.item_id.clone()));
        }
        let src: SourceRow = sqlx::query_as(&format!("{SOURCE_SQL} WHERE oi.id = ?1 AND o.store_id = ?2"))
            .bind(&r.item_id)
            .bind(&ctx.store_id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| AppError::validation("RETURN_ITEM_UNKNOWN", r.item_id.clone()))?;
        if src.status != "completed" {
            return Err(AppError::validation("RETURN_ORDER_VOIDED", src.receipt_no));
        }
        let Some(product_id) = src.product_id.clone().filter(|_| src.tracked && !src.is_custom) else {
            return Err(AppError::validation("RETURN_NOT_STOCKED", src.name));
        };
        if r.qty > src.qty - src.returned {
            return Err(AppError::validation(
                "RETURN_QTY_EXCEEDS",
                format!("{}: {} left", src.name, src.qty - src.returned),
            ));
        }
        let sold_on = parse_date(&src.business_date).ok_or_else(|| AppError::internal("bad business date"))?;
        late |= exchange::is_late(sold_on, today, window);

        let (from, to) = (src.returned, src.returned + r.qty);
        let gross = exchange::portion(src.line_gross, src.qty, from, to);
        let total = exchange::portion(src.line_total, src.qty, from, to);
        let net = exchange::portion(src.line_net, src.qty, from, to);
        let tax = exchange::portion(src.tax_amount, src.qty, from, to);
        let slices = slices_for(conn, &src, from, to).await?;
        let cost = slices.iter().map(|s| s.qty * s.unit_cost).sum();
        returns.push(PlannedReturn {
            id: new_id(),
            source_item_id: src.id,
            source_receipt: src.receipt_no,
            name: src.name,
            product_id,
            qty: r.qty,
            gross,
            discount: gross - total,
            total,
            net,
            tax,
            cost,
            slices,
        });
    }
    Ok(ExchangePlan {
        credit: returns.iter().map(|r| r.total).sum(),
        items: returns.iter().map(|r| r.qty).sum(),
        gross: returns.iter().map(|r| r.gross).sum(),
        discount: returns.iter().map(|r| r.discount).sum(),
        net: returns.iter().map(|r| r.net).sum(),
        tax: returns.iter().map(|r| r.tax).sum(),
        cost: returns.iter().map(|r| r.cost).sum(),
        returns,
        late,
        window_days: window,
    })
}

/// Allocate units `from..to` of the source line over its original sale slices, in the order they were written,
/// so each return slice mirrors the layer (and cost) its units were sold from.
async fn slices_for(conn: &mut SqliteConnection, src: &SourceRow, from: i64, to: i64) -> AppResult<Vec<ReturnSlice>> {
    let sold: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT batch_item_id, unit_cost, -qty_delta FROM stock_movements
         WHERE order_item_id = ?1 AND reason = 'sale' ORDER BY rowid",
    )
    .bind(&src.id)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::new();
    let mut start = 0;
    for (batch_item_id, unit_cost, qty) in sold {
        let (lo, hi) = (start.max(from), (start + qty).min(to));
        if lo < hi {
            out.push(ReturnSlice {
                batch_item_id,
                unit_cost,
                qty: hi - lo,
                revenue_net: exchange::portion(src.line_net, src.qty, lo, hi),
            });
        }
        start += qty;
    }
    if out.iter().map(|s| s.qty).sum::<i64>() != to - from {
        return Err(AppError::internal(format!("sale slices of line {} do not cover its quantity", src.id)));
    }
    Ok(out)
}

/// Write the returned lines and their restocking slices into the pending exchange order.
pub async fn write(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    order_id: &str,
    staff_id: &str,
    approved_by: Option<&str>,
    plan: &ExchangePlan,
) -> AppResult<()> {
    for r in &plan.returns {
        sqlx::query(
            "INSERT INTO order_returns (id, store_id, device_id, order_id, source_item_id, qty, line_gross,
                                        line_discount, line_total, line_net, tax_amount, cost_total, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )
        .bind(&r.id)
        .bind(&ctx.store_id)
        .bind(&ctx.device_id)
        .bind(order_id)
        .bind(&r.source_item_id)
        .bind(r.qty)
        .bind(r.gross)
        .bind(r.discount)
        .bind(r.total)
        .bind(r.net)
        .bind(r.tax)
        .bind(r.cost)
        .bind(&ctx.now)
        .execute(&mut *conn)
        .await?;
        outbox::enqueue_upsert(conn, "order_returns", &r.id, &ctx.now).await?;
        for s in &r.slices {
            stock::record(
                conn,
                ctx,
                &Movement {
                    product_id: &r.product_id,
                    batch_item_id: &s.batch_item_id,
                    qty_delta: s.qty,
                    reason: StockReason::Return,
                    ref_type: Some("order"),
                    ref_id: Some(order_id),
                    order_item_id: Some(&r.source_item_id),
                    unit_cost: s.unit_cost,
                    revenue_net: Some(-s.revenue_net),
                    staff_id: Some(staff_id),
                    approved_by,
                    note: None,
                },
            )
            .await?;
        }
    }
    Ok(())
}

// ───────── lookup (the till's "Exchange" dialog) ─────────

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ExchangeLine {
    pub item_id: String,
    pub name: String,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
    /// What the customer paid for the whole line (after discounts).
    #[specta(type = specta_typescript::Number)]
    pub line_total: i64,
    #[specta(type = specta_typescript::Number)]
    pub returned_qty: i64,
    #[specta(type = specta_typescript::Number)]
    pub returnable_qty: i64,
    /// False for quick-sale and untracked lines (nothing to put back on the shelf) and fully returned lines.
    pub eligible: bool,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ExchangeSource {
    pub order_id: String,
    pub receipt_no: String,
    pub business_date: String,
    #[specta(type = specta_typescript::Number)]
    pub days_since: i64,
    #[specta(type = specta_typescript::Number)]
    pub window_days: i64,
    /// Past the window: completing the exchange needs a supervisor / owner PIN (`EXCHANGE_LATE`).
    pub late: bool,
    pub lines: Vec<ExchangeLine>,
}

/// A bill offered while the cashier is still typing its number.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct BillSuggestion {
    pub receipt_no: String,
    pub business_date: String,
    #[specta(type = specta_typescript::Number)]
    pub item_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub grand_total: i64,
}

const SUGGESTION_LIMIT: i64 = 8;
const SUGGESTION_MIN_CHARS: usize = 2;

/// Completed bills whose number contains what was typed, newest first, those starting with it ahead of the
/// rest. Case, dashes, spaces and anything else that isn't a letter or digit are ignored on both sides, so
/// `261121154`, `t01 261121 154` and `1154` all find `T01-261121154`. Voided bills can't be exchanged, so they
/// are not offered.
pub async fn suggest(pool: &SqlitePool, store_id: &str, typed: &str) -> AppResult<Vec<BillSuggestion>> {
    let needle: String = typed.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_uppercase();
    if needle.len() < SUGGESTION_MIN_CHARS {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as(
        "SELECT receipt_no, business_date, item_count, grand_total FROM orders
         WHERE store_id = ?1 AND status = 'completed' AND instr(REPLACE(receipt_no, '-', ''), ?2) > 0
         ORDER BY instr(REPLACE(receipt_no, '-', ''), ?2) = 1 DESC, completed_at DESC, id DESC LIMIT ?3",
    )
    .bind(store_id)
    .bind(&needle)
    .bind(SUGGESTION_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(FromRow)]
struct LookupHead {
    id: String,
    receipt_no: String,
    status: String,
    business_date: String,
}

/// Find a completed sale by receipt number and report what can still be exchanged, and whether the window has passed.
pub async fn lookup(pool: &SqlitePool, store_id: &str, receipt_no: &str, today: &str) -> AppResult<ExchangeSource> {
    let today = parse_date(today).ok_or_else(|| AppError::internal("bad business date"))?;
    // What the cashier typed becomes the full bill number: `T01-4` is bill 4 of today (decision #31).
    let receipt = normalize_bill_query(receipt_no, today);
    let head: LookupHead = sqlx::query_as(
        "SELECT id, receipt_no, status, business_date FROM orders WHERE store_id = ?1 AND receipt_no = ?2",
    )
    .bind(store_id)
    .bind(&receipt)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("order"))?;
    if head.status != "completed" {
        return Err(AppError::validation("RETURN_ORDER_VOIDED", head.receipt_no));
    }
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM store_settings WHERE key = ?1")
        .bind(WINDOW_SETTING)
        .fetch_optional(pool)
        .await?;
    let window = window_from(raw)?;
    let sold_on = parse_date(&head.business_date).ok_or_else(|| AppError::internal("bad business date"))?;
    let rows: Vec<SourceRow> = sqlx::query_as(&format!("{SOURCE_SQL} WHERE oi.order_id = ?1 ORDER BY oi.line_no"))
        .bind(&head.id)
        .fetch_all(pool)
        .await?;
    let lines = rows
        .into_iter()
        .map(|r| {
            let returnable = r.qty - r.returned;
            ExchangeLine {
                eligible: r.tracked && !r.is_custom && returnable > 0,
                item_id: r.id,
                name: r.name,
                qty: r.qty,
                line_total: r.line_total,
                returned_qty: r.returned,
                returnable_qty: returnable,
            }
        })
        .collect();
    Ok(ExchangeSource {
        order_id: head.id,
        receipt_no: head.receipt_no,
        business_date: head.business_date,
        days_since: exchange::days_since(sold_on, today),
        window_days: window,
        late: exchange::is_late(sold_on, today, window),
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_setting_parses_and_rejects_garbage() {
        assert_eq!(window_from(None).ok(), Some(7));
        assert_eq!(window_from(Some("14".into())).ok(), Some(14));
        assert_eq!(window_from(Some("0".into())).ok(), Some(0));
        assert!(window_from(Some("-1".into())).is_err());
        assert!(window_from(Some("\"7\"".into())).is_err());
        assert!(window_from(Some("9999".into())).is_err());
    }
}
