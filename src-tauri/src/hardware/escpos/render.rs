//! Print-job kinds → ESC/POS bytes. Every render reads from the database at print time (a job stores only its
//! render input), so a retry or reprint matches the sealed records exactly.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::SqlitePool;

use super::{Align, EscPos, Layout, drawer_kick, wrap};
use crate::db::repo::receipts::{self, ReceiptData};
use crate::db::settings;
use crate::domain::money::format_lkr;
use crate::error::{AppError, AppResult};

/// `1,234.50` (the column figures; the total line carries the `Rs.`).
fn amt(cents: i64) -> String {
    format_lkr(cents).replacen("Rs ", "", 1)
}

fn rs(cents: i64) -> String {
    format_lkr(cents).replacen("Rs ", "Rs. ", 1)
}

/// `2026-11-08 14:30` in the store's timezone.
fn local(iso: &str, tz: Tz) -> String {
    DateTime::parse_from_rfc3339(iso)
        .map(|t| t.with_timezone(&Utc).with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| iso.to_owned())
}

fn method_label(m: &str) -> &'static str {
    match m {
        "cash" => "Cash",
        "card" => "Card",
        "bank_transfer" => "Bank transfer",
        "qr" => "QR payment",
        _ => "Other",
    }
}

fn header(p: &mut EscPos, store: &str) {
    let half = p.columns() / 2;
    p.align(Align::Center).bold(true).double(true).wrapped(store, half).double(false).bold(false);
}

/// The sale receipt (doc §6.4): lines, totals, payments, masked customer phone, QR of the order id, the
/// "All sales are final" footer, then the cut. Never the drawer kick (that's its own job, sent first).
pub fn receipt(r: &ReceiptData, layout: Layout, tz: Tz) -> Vec<u8> {
    let mut p = EscPos::new(layout);
    let cols = p.columns();
    header(&mut p, &r.store_name);
    if r.status == "voided" {
        p.bold(true).line("*** VOIDED - NOT A VALID RECEIPT ***").bold(false);
    }
    p.align(Align::Left);
    p.pair("Receipt", &r.receipt_no);
    p.pair(&local(&r.completed_at, tz), &format!("Cashier: {}", r.cashier));
    p.rule('-');
    for l in &r.lines {
        p.wrapped(&l.name, cols);
        p.pair(&format!("  {} x {}", l.qty, amt(l.unit_price)), &amt(l.line_gross));
        if l.unit_price < l.list_price {
            p.line(&format!("  (was {})", amt(l.list_price)));
        }
        if l.line_discount > 0 {
            p.pair("  Discount", &format!("-{}", amt(l.line_discount)));
        }
    }
    p.rule('-');
    p.pair(&format!("Items: {}", r.item_count), "");
    p.pair("Subtotal", &amt(r.gross_sales));
    if r.discount_total > 0 {
        p.pair("Discount", &format!("-{}", amt(r.discount_total)));
    }
    p.bold(true).pair("TOTAL", &rs(r.grand_total)).bold(false);
    for pay in &r.payments {
        match pay.tendered {
            Some(t) if t > pay.amount => p.pair(method_label(&pay.method), &amt(t)),
            _ => p.pair(method_label(&pay.method), &amt(pay.amount)),
        };
    }
    if r.change_due > 0 {
        p.pair("Change", &amt(r.change_due));
    }
    if let Some(c) = &r.customer_masked {
        p.line(&format!("Customer: {c}"));
    }
    p.rule('-').align(Align::Center).qr(&r.order_id);
    for l in wrap(&r.footer, cols) {
        p.line(&l);
    }
    p.align(Align::Left).cut();
    p.finish()
}

async fn staff_name(pool: &SqlitePool, id: Option<&str>) -> AppResult<Option<String>> {
    let Some(id) = id else { return Ok(None) };
    Ok(sqlx::query_scalar("SELECT display_name FROM staff WHERE id = ?1").bind(id).fetch_optional(pool).await?)
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn n(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(Value::as_i64).unwrap_or(0)
}

/// The short VOID slip for the cash-up (doc §3.3): receipt no., amount, reason, who voided and approved.
pub async fn void_slip(pool: &SqlitePool, payload: &Value, layout: Layout, tz: Tz, now: &str) -> AppResult<Vec<u8>> {
    let store: String = settings::store_get(pool, settings::keys::STORE_NAME).await?.unwrap_or_default();
    let voided_by = staff_name(pool, payload.get("voided_by").and_then(Value::as_str)).await?;
    let approved_by = staff_name(pool, payload.get("approved_by").and_then(Value::as_str)).await?;
    let mut p = EscPos::new(layout);
    header(&mut p, &store);
    p.bold(true).line("VOID").bold(false).align(Align::Left);
    p.pair("Receipt", s(payload, "receipt_no"));
    p.pair("Amount", &rs(n(payload, "amount")));
    p.pair("Reason", s(payload, "reason"));
    if let Some(note) = payload.get("note").and_then(Value::as_str) {
        let cols = p.columns();
        p.wrapped(&format!("Note: {note}"), cols);
    }
    p.pair("Voided by", voided_by.as_deref().unwrap_or("-"));
    if let Some(a) = approved_by {
        p.pair("Approved by", &a);
    }
    p.pair("Printed", &local(now, tz));
    p.cut();
    Ok(p.finish())
}

/// The Z-report (doc §3.5) from the snapshot stored when the session closed.
pub fn z_report(store: &str, z: &Value, layout: Layout, tz: Tz) -> Vec<u8> {
    let null = Value::Null;
    let session = z.get("session").unwrap_or(&null);
    let mut p = EscPos::new(layout);
    header(&mut p, store);
    p.bold(true).line("Z-REPORT").bold(false).align(Align::Left);
    p.pair("Business date", s(session, "business_date"));
    p.pair("Opened", &local(s(session, "opened_at"), tz));
    p.pair("Opened by", s(session, "opened_by_name"));
    p.pair("Closed", &local(s(session, "closed_at"), tz));
    p.pair("Closed by", s(session, "closed_by_name"));
    p.rule('-');
    p.pair("Sales", &n(z, "sales_count").to_string());
    p.pair("Items sold", &n(z, "items_sold").to_string());
    p.pair("Gross sales", &amt(n(z, "gross_sales")));
    p.pair("Discounts", &format!("-{}", amt(n(z, "discount_total"))));
    p.bold(true).pair("Sales total", &rs(n(z, "sales_total"))).bold(false);
    p.rule('-').line("Payments");
    for pay in z.get("payments").and_then(Value::as_array).into_iter().flatten() {
        p.pair(&format!("  {} ({})", method_label(s(pay, "method")), n(pay, "count")), &amt(n(pay, "amount")));
    }
    p.pair("  Change given", &amt(n(z, "change_given")));
    p.rule('-');
    p.pair(&format!("Price overrides ({})", n(z, "overrides_count")), &amt(n(z, "overrides_value")));
    p.pair(&format!("Quick-sale lines ({})", n(z, "quick_sale_lines")), &amt(n(z, "quick_sale_net")));
    p.pair(&format!("Voids ({})", n(z, "voids_count")), &amt(n(z, "voids_value")));
    for v in z.get("voids").and_then(Value::as_array).into_iter().flatten() {
        p.pair(&format!("  {} {}", s(v, "receipt_no"), s(v, "reason")), &amt(n(v, "value")));
    }
    p.rule('-').line("Drawer");
    p.pair("  Opening float", &amt(n(session, "opening_float")));
    p.pair("  Cash sales", &amt(n(session, "cash_sales")));
    p.pair("  Pay-ins", &amt(n(z, "pay_ins")));
    p.pair("  Pay-outs", &format!("-{}", amt(n(z, "pay_outs"))));
    p.pair("  No-sale opens", &n(z, "no_sales").to_string());
    p.bold(true).pair("Expected cash", &rs(n(session, "expected_cash"))).bold(false);
    p.pair("Counted cash", &rs(n(session, "counted_cash")));
    p.bold(true).pair("Variance", &rs(n(session, "variance"))).bold(false);
    p.cut();
    p.finish()
}

/// Settings → "Print test page" (doc §7.2): column ruler, code-page sample, QR, cut, then the drawer kick.
pub fn test_page(store: &str, layout: Layout, printed_at: &str) -> Vec<u8> {
    let mut p = EscPos::new(layout);
    let cols = p.columns();
    header(&mut p, store);
    p.line("PRINTER TEST PAGE").align(Align::Left);
    p.line(&format!("{cols} columns:"));
    let ruler: String = (1..=cols).map(|i| char::from(b'0' + u8::try_from(i % 10).unwrap_or(0))).collect();
    p.line(&ruler);
    p.line("ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789");
    p.line("abcdefghijklmnopqrstuvwxyz .,:;!?-+/()");
    p.line("Code page: é ü £ ñ");
    p.pair("Total", &rs(125_000));
    p.align(Align::Center).qr("GAYANSHOP-TEST").line(printed_at).align(Align::Left);
    p.cut();
    if layout.open_drawer {
        p.kick();
    }
    p.finish()
}

/// Bytes for one queued job.
pub async fn job(
    pool: &SqlitePool,
    kind: &str,
    payload: &Value,
    layout: Layout,
    tz: Tz,
    now: &str,
) -> AppResult<Vec<u8>> {
    match kind {
        "receipt" => {
            let order_id = payload
                .get("order_id")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::internal("receipt job without order_id"))?;
            Ok(receipt(&receipts::load(pool, order_id).await?, layout, tz))
        }
        "drawer_kick" => Ok(drawer_kick(layout.drawer_pin)),
        "void_slip" => void_slip(pool, payload, layout, tz, now).await,
        "z_report" => {
            let store: String = settings::store_get(pool, settings::keys::STORE_NAME).await?.unwrap_or_default();
            Ok(z_report(&store, payload, layout, tz))
        }
        "test" => {
            let store: String = settings::store_get(pool, settings::keys::STORE_NAME).await?.unwrap_or_default();
            Ok(test_page(&store, layout, &local(now, tz)))
        }
        other => Err(AppError::internal(format!("not a receipt-printer job: {other}"))),
    }
}
