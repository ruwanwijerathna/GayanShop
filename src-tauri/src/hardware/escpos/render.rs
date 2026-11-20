//! Print-job kinds → ESC/POS bytes. Every render reads from the database at print time (a job stores only its
//! render input), so a retry or reprint matches the sealed records exactly.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::SqlitePool;

use super::{Align, EscPos, Layout, Logo, ReceiptImage, drawer_kick, wrap};
use crate::db::repo::receipts::{self, ReceiptData, ReceiptLine, ReceiptPayment};
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

fn date_time(iso: &str, tz: Tz) -> (String, String) {
    DateTime::parse_from_rfc3339(iso)
        .map(|t| {
            let l = t.with_timezone(&Utc).with_timezone(&tz);
            (l.format("%Y-%m-%d").to_string(), l.format("%H:%M").to_string())
        })
        .unwrap_or_else(|_| (iso.to_owned(), String::new()))
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
/// store footer, then the cut. Never the drawer kick (that's its own job, sent first).
pub fn receipt(r: &ReceiptData, layout: Layout, tz: Tz, logo: Option<&Logo>) -> Vec<u8> {
    receipt_with_banner(r, layout, tz, logo, None)
}

/// [`receipt`] with an optional bold line under the store name (the sample receipt marks itself as not a sale).
fn receipt_with_banner(r: &ReceiptData, layout: Layout, tz: Tz, logo: Option<&Logo>, banner: Option<&str>) -> Vec<u8> {
    let mut p = EscPos::new(layout);
    let cols = p.columns();
    if let Some(l) = logo {
        p.logo(l);
    } else {
        header(&mut p, &r.store_name);
    }
    if let Some(b) = banner {
        p.bold(true).wrapped(b, cols).bold(false);
    }
    if r.exchange_credit > 0 {
        p.bold(true).line("EXCHANGE").bold(false);
    }
    if r.status == "voided" {
        p.bold(true).line("*** VOIDED - NOT A VALID RECEIPT ***").bold(false);
    }
    let (date_str, time_str) = date_time(&r.completed_at, tz);
    p.align(Align::Left);
    p.pair("Bill No / Receipt:", &r.receipt_no);
    p.pair(&format!("Date: {date_str}"), &format!("Time: {time_str}"));
    let cashier_str = format!("Cashier: {}", r.cashier);
    if let Some(c) = &r.customer_masked {
        p.pair(&cashier_str, &format!("Customer: {c}"));
    } else {
        p.line(&cashier_str);
    }
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
    if r.exchange_credit > 0 {
        p.rule('-').line("Returned");
        for ret in &r.returns {
            p.wrapped(&ret.name, cols);
            p.pair(&format!("  {} x (sale {})", ret.qty, ret.original_receipt_no), &format!("-{}", amt(ret.value)));
        }
        p.pair("Exchange credit", &format!("-{}", amt(r.exchange_credit)));
        let due = r.grand_total - r.exchange_credit;
        p.bold(true).pair("TO PAY", &rs(due)).bold(false);
    }
    for pay in &r.payments {
        match pay.tendered {
            Some(t) if t > pay.amount => p.pair(method_label(&pay.method), &amt(t)),
            _ => p.pair(method_label(&pay.method), &amt(pay.amount)),
        };
    }
    if r.change_due > 0 {
        p.pair("Change", &amt(r.change_due));
    }
    p.rule('-');
    p.align(Align::Center);
    p.wrapped("Exchange within 7 days with bill and item.", cols);
    p.bold(true).line("Thank you! Come again.").bold(false);
    p.feed(1);
    p.align(Align::Center);
    if layout.qr {
        p.qr(&r.order_id);
    } else {
        p.code128(r.receipt_no.as_bytes());
    }
    p.align(Align::Center).line("Powered By BraceFrame - (076 9950 199)");
    if !r.footer.trim().is_empty() {
        for l in wrap(&r.footer, cols) {
            p.line(&l);
        }
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
    p.bold(true).line("Z-REPORT").bold(false);
    if z.get("reprint").and_then(Value::as_bool) == Some(true) {
        p.bold(true).line("*** REPRINT ***").bold(false);
    }
    p.align(Align::Left);
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
    if n(z, "exchanges_count") > 0 {
        p.pair(&format!("Exchanges ({})", n(z, "exchanges_count")), &amt(n(z, "exchange_credit")));
        p.pair("  Items returned", &n(z, "returned_items").to_string());
    }
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

/// Settings → "Print test page" (doc §7.2): a sample receipt (header image, store name, a few made-up lines,
/// totals, QR and the real footer) so the owner sees exactly how a bill will look, then the drawer kick.
pub fn test_page(store: &str, footer: &str, layout: Layout, tz: Tz, now: &str, logo: Option<&Logo>) -> Vec<u8> {
    let line = |name: &str, qty: i64, unit: i64| ReceiptLine {
        name: name.to_owned(),
        qty,
        list_price: unit,
        unit_price: unit,
        line_gross: qty * unit,
        line_discount: 0,
        line_total: qty * unit,
    };
    let lines =
        vec![line("Sample item A", 2, 45_000), line("Sample item B", 1, 125_000), line("Sample item C", 3, 9_500)];
    let total: i64 = lines.iter().map(|l| l.line_total).sum();
    let tendered = 300_000;
    let sample = ReceiptData {
        order_id: "SAMPLE-RECEIPT".into(),
        store_name: store.to_owned(),
        receipt_no: "T01-20261120-0001".into(),
        status: "completed".into(),
        business_date: String::new(),
        completed_at: now.to_owned(),
        cashier: "Cashier 01".into(),
        item_count: lines.iter().map(|l| l.qty).sum(),
        gross_sales: total,
        discount_total: 0,
        grand_total: total,
        payments: vec![ReceiptPayment {
            method: "cash".into(),
            amount: total,
            tendered: Some(tendered),
            reference: None,
        }],
        change_due: tendered - total,
        returns: Vec::new(),
        exchange_credit: 0,
        customer_masked: None,
        footer: footer.to_owned(),
        lines,
    };
    let mut bytes = receipt_with_banner(&sample, layout, tz, logo, Some("*** SAMPLE RECEIPT - NOT A SALE ***"));
    if layout.open_drawer {
        bytes.extend(drawer_kick(layout.drawer_pin));
    }
    bytes
}

/// A receipt the webview drew (Sinhala text cannot go through the printer's fonts): the picture, then the cut.
pub fn receipt_image(image: &ReceiptImage, layout: Layout) -> Vec<u8> {
    let mut p = EscPos::new(layout);
    p.image(image).cut();
    p.finish()
}

/// Device setting that holds the receipt header image.
pub const LOGO_KEY: &str = "receipt.logo";

/// This counter's receipt header image, if one was set.
pub async fn load_logo(pool: &SqlitePool) -> AppResult<Option<Logo>> {
    settings::device_get(pool, LOGO_KEY).await
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
        "receipt" if payload.get("image").is_some() => {
            let image: ReceiptImage = serde_json::from_value(payload["image"].clone())
                .map_err(|e| AppError::internal(format!("receipt job with a bad image: {e}")))?;
            image.validate()?;
            Ok(receipt_image(&image, layout))
        }
        "receipt" => {
            let order_id = payload
                .get("order_id")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::internal("receipt job without order_id"))?;
            let logo = load_logo(pool).await?;
            Ok(receipt(&receipts::load(pool, order_id).await?, layout, tz, logo.as_ref()))
        }
        "drawer_kick" => Ok(drawer_kick(layout.drawer_pin)),
        "void_slip" => void_slip(pool, payload, layout, tz, now).await,
        "z_report" => {
            let store: String = settings::store_get(pool, settings::keys::STORE_NAME).await?.unwrap_or_default();
            Ok(z_report(&store, payload, layout, tz))
        }
        "test" => {
            let store: String = settings::store_get(pool, settings::keys::STORE_NAME).await?.unwrap_or_default();
            let footer: String = settings::store_get(pool, "receipt.footer").await?.unwrap_or_default();
            let logo = load_logo(pool).await?;
            Ok(test_page(&store, &footer, layout, tz, now, logo.as_ref()))
        }
        other => Err(AppError::internal(format!("not a receipt-printer job: {other}"))),
    }
}
