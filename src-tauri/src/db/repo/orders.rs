//! Order history for the back office (doc §1.4 "unbounded lists"): keyset pages over
//! `ix_orders_keyset (completed_at DESC, id DESC)`, newest first, filtered by period, counter, status and
//! receipt number, with drill-down to the receipt. Sealed orders are never pending outside a checkout.

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{FromRow, SqlitePool};

use crate::domain::business_date::StoreCalendar;
use crate::domain::period::Period;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct OrderCursor {
    pub completed_at: String,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatusFilter {
    Completed,
    Voided,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct OrderFilter {
    pub period: Period,
    pub device_id: Option<String>,
    pub status: Option<OrderStatusFilter>,
    /// A receipt number, or the start of one (`T01-2611`).
    pub receipt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct OrderRow {
    pub id: String,
    pub receipt_no: String,
    pub business_date: String,
    pub completed_at: String,
    pub status: String,
    pub cashier: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub item_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub grand_total: i64,
    pub has_customer: bool,
    pub void_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct OrderPage {
    pub rows: Vec<OrderRow>,
    pub next_cursor: Option<OrderCursor>,
}

/// One keyset page, newest first: walks `ix_orders_keyset` and stops after `LIMIT` matching rows. The unary
/// `+` on `store_id` keeps SQLite from choosing `ux_orders_receipt (store_id, …)` and sorting every order.
pub const SQL_HISTORY: &str =
    "SELECT o.id, o.receipt_no, o.business_date, o.completed_at, o.status, s.display_name AS cashier,
                o.item_count, o.grand_total, o.customer_id IS NOT NULL AS has_customer, o.void_reason
         FROM orders o LEFT JOIN staff s ON s.id = o.staff_id
         WHERE +o.store_id = ?1 AND o.status IN ('completed', 'voided')
           AND (?2 IS NULL OR o.business_date >= ?2) AND (?3 IS NULL OR o.business_date <= ?3)
           AND (?4 IS NULL OR o.device_id = ?4) AND (?5 IS NULL OR o.status = ?5)
           AND (?6 IS NULL OR o.receipt_no LIKE ?6 ESCAPE '\\')
           AND (?7 IS NULL OR (o.completed_at, o.id) < (?7, ?8))
         ORDER BY o.completed_at DESC, o.id DESC LIMIT ?9";

pub async fn history(
    pool: &SqlitePool,
    store_id: &str,
    cal: &StoreCalendar,
    filter: &OrderFilter,
    cursor: Option<&OrderCursor>,
    limit: u32,
) -> AppResult<OrderPage> {
    let range = filter.period.resolve(cal)?;
    let limit = i64::from(limit.clamp(1, 500));
    let status = filter.status.map(|s| match s {
        OrderStatusFilter::Completed => "completed",
        OrderStatusFilter::Voided => "voided",
    });
    let receipt = filter
        .receipt
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(|r| format!("{}%", r.to_uppercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    let mut rows: Vec<OrderRow> = sqlx::query_as(SQL_HISTORY)
        .bind(store_id)
        .bind(range.from.as_deref())
        .bind(range.to.as_deref())
        .bind(filter.device_id.as_deref())
        .bind(status)
        .bind(receipt.as_deref())
        .bind(cursor.map(|c| c.completed_at.as_str()))
        .bind(cursor.map(|c| c.id.as_str()))
        .bind(limit + 1)
        .fetch_all(pool)
        .await?;
    let next_cursor = if i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit {
        rows.truncate(usize::try_from(limit).unwrap_or(0));
        rows.last().map(|r| OrderCursor { completed_at: r.completed_at.clone(), id: r.id.clone() })
    } else {
        None
    };
    Ok(OrderPage { rows, next_cursor })
}
