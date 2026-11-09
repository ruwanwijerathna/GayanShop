//! The receipt as data (doc §6.4, §7.2): everything the ESC/POS builder prints, loaded from the sealed order
//! at print time (a print job stores only the order id, so a reprint always matches the database).
//! English-first; the customer's phone is **always masked** (`07X XXX X123`, doc §3.8) and never leaves the
//! `customers` row in full.

use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::db::settings;
use crate::domain::phone;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, FromRow, PartialEq, Eq)]
pub struct ReceiptLine {
    pub name: String,
    pub qty: i64,
    pub list_price: i64,
    pub unit_price: i64,
    pub line_gross: i64,
    pub line_discount: i64,
    pub line_total: i64,
}

#[derive(Debug, Clone, Serialize, FromRow, PartialEq, Eq)]
pub struct ReceiptPayment {
    pub method: String,
    pub amount: i64,
    pub tendered: Option<i64>,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReceiptData {
    pub order_id: String,
    pub store_name: String,
    pub receipt_no: String,
    /// `completed` or `voided` (a voided order's reprint carries a VOID banner).
    pub status: String,
    pub business_date: String,
    pub completed_at: String,
    pub cashier: String,
    pub lines: Vec<ReceiptLine>,
    pub item_count: i64,
    pub gross_sales: i64,
    pub discount_total: i64,
    pub grand_total: i64,
    pub payments: Vec<ReceiptPayment>,
    pub change_due: i64,
    /// `07X XXX X123`, or `None` when no customer was attached (or the profile was erased).
    pub customer_masked: Option<String>,
    pub footer: String,
}

#[derive(FromRow)]
struct Head {
    receipt_no: String,
    status: String,
    business_date: String,
    completed_at: String,
    cashier: String,
    item_count: i64,
    gross_sales: i64,
    discount_total: i64,
    grand_total: i64,
    change_due: i64,
    customer_phone: Option<String>,
}

pub async fn load(pool: &SqlitePool, order_id: &str) -> AppResult<ReceiptData> {
    let head: Head = sqlx::query_as(
        "SELECT o.receipt_no, o.status, o.business_date, o.completed_at, s.display_name AS cashier, o.item_count,
                o.gross_sales, o.discount_total, o.grand_total, o.change_due, c.phone_e164 AS customer_phone
         FROM orders o
         JOIN staff s ON s.id = o.staff_id
         LEFT JOIN customers c ON c.id = o.customer_id
         WHERE o.id = ?1 AND o.status IN ('completed', 'voided')",
    )
    .bind(order_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("order"))?;
    let lines: Vec<ReceiptLine> = sqlx::query_as(
        "SELECT name, qty, list_price, unit_price, line_gross, line_discount, line_total
         FROM order_items WHERE order_id = ?1 ORDER BY line_no",
    )
    .bind(order_id)
    .fetch_all(pool)
    .await?;
    let payments: Vec<ReceiptPayment> = sqlx::query_as(
        "SELECT method, amount, tendered, reference FROM order_payments WHERE order_id = ?1 ORDER BY created_at, id",
    )
    .bind(order_id)
    .fetch_all(pool)
    .await?;
    let store_name: String = settings::store_get(pool, settings::keys::STORE_NAME).await?.unwrap_or_default();
    let footer: String = settings::store_get(pool, "receipt.footer").await?.unwrap_or_default();
    Ok(ReceiptData {
        order_id: order_id.to_owned(),
        store_name,
        receipt_no: head.receipt_no,
        status: head.status,
        business_date: head.business_date,
        completed_at: head.completed_at,
        cashier: head.cashier,
        lines,
        item_count: head.item_count,
        gross_sales: head.gross_sales,
        discount_total: head.discount_total,
        grand_total: head.grand_total,
        payments,
        change_due: head.change_due,
        customer_masked: head.customer_phone.as_deref().and_then(phone::mask_for_receipt),
        footer,
    })
}
