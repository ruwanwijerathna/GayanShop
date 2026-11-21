//! The Bills page (doc §1.4 "unbounded lists"): keyset pages over `ix_orders_keyset (completed_at DESC,
//! id DESC)`, newest first, filtered by period, counter, status, receipt number, a customer's phone, payment
//! method and exchange/returns, with drill-down to the receipt. Sealed orders are never pending outside a
//! checkout.

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{FromRow, SqlitePool};

use crate::domain::business_date::StoreCalendar;
use crate::domain::payments::PaymentMethod;
use crate::domain::period::Period;
use crate::error::{AppError, AppResult};

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

/// Exchange bills (replacement sales that took returned goods in) or the original sales those goods came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BillKind {
    Exchange,
    WithReturns,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct OrderFilter {
    pub period: Period,
    pub device_id: Option<String>,
    pub status: Option<OrderStatusFilter>,
    /// A receipt number, or the start of one (`T01-2611`).
    pub receipt: Option<String>,
    /// A customer's phone, or its last digits (at least four): `0771234567`, `771234567`, `34567`.
    #[serde(default)]
    pub phone: Option<String>,
    /// Bills paid at least partly with this method.
    #[serde(default)]
    pub payment: Option<PaymentMethod>,
    #[serde(default)]
    pub kind: Option<BillKind>,
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
    /// Value of returned goods this (exchange) bill took in; 0 for an ordinary sale.
    #[specta(type = specta_typescript::Number)]
    pub exchange_credit: i64,
    /// Some of this sale's goods were later returned in an exchange.
    pub has_returns: bool,
    /// The payment methods used, comma separated and sorted (`card,cash`).
    pub methods: String,
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
                o.item_count, o.grand_total, o.customer_id IS NOT NULL AS has_customer, o.void_reason,
                o.exchange_credit,
                EXISTS (SELECT 1 FROM order_returns r JOIN order_items i ON i.id = r.source_item_id
                        WHERE i.order_id = o.id) AS has_returns,
                COALESCE((SELECT group_concat(method) FROM
                            (SELECT DISTINCT method FROM order_payments WHERE order_id = o.id ORDER BY method)), '')
                  AS methods
         FROM orders o LEFT JOIN staff s ON s.id = o.staff_id
         WHERE +o.store_id = ?1 AND o.status IN ('completed', 'voided')
           AND (?2 IS NULL OR o.business_date >= ?2) AND (?3 IS NULL OR o.business_date <= ?3)
           AND (?4 IS NULL OR o.device_id = ?4) AND (?5 IS NULL OR o.status = ?5)
           AND (?6 IS NULL OR o.receipt_no LIKE ?6 ESCAPE '\\')
           AND (?7 IS NULL OR (o.completed_at, o.id) < (?7, ?8))
           AND (?10 IS NULL OR o.customer_id IN (SELECT id FROM customers WHERE phone_e164 LIKE ?10))
           AND (?11 IS NULL OR EXISTS (SELECT 1 FROM order_payments p WHERE p.order_id = o.id AND p.method = ?11))
           AND (?12 IS NULL
                OR (?12 = 'exchange' AND o.exchange_credit > 0)
                OR (?12 = 'with_returns' AND EXISTS (SELECT 1 FROM order_returns r
                      JOIN order_items i ON i.id = r.source_item_id WHERE i.order_id = o.id)))
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
    let phone = phone_pattern(filter.phone.as_deref())?;
    let kind = filter.kind.map(|k| match k {
        BillKind::Exchange => "exchange",
        BillKind::WithReturns => "with_returns",
    });
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
        .bind(phone.as_deref())
        .bind(filter.payment.map(PaymentMethod::as_db))
        .bind(kind)
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

/// `LIKE` pattern for the end of a stored `+94…` number from whatever the staff member typed: spaces, dashes,
/// a leading `0`, `94` or `+94` are ignored. Fewer than four digits would match half the shop.
fn phone_pattern(input: Option<&str>) -> AppResult<Option<String>> {
    let Some(raw) = input.map(str::trim).filter(|p| !p.is_empty()) else { return Ok(None) };
    let mut digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    if digits.len() > 9 && digits.starts_with("94") {
        digits.drain(..2);
    } else if digits.starts_with('0') {
        digits.remove(0);
    }
    if digits.len() < 4 {
        return Err(AppError::validation("PHONE_SEARCH_TOO_SHORT", raw.to_owned()));
    }
    Ok(Some(format!("%{digits}")))
}

/// Who sees which bills: `REPORT_SALES` keeps the filter as asked (any counter); everyone else is held to this
/// counter, whatever counter the request named.
pub fn visible_to(mut filter: OrderFilter, own_device: &str, all_counters: bool) -> OrderFilter {
    if !all_counters {
        filter.device_id = Some(own_device.to_owned());
    }
    filter
}

/// The counter that made this bill, if it exists.
pub async fn device_of(pool: &SqlitePool, order_id: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query_scalar("SELECT device_id FROM orders WHERE id = ?1").bind(order_id).fetch_optional(pool).await?)
}
