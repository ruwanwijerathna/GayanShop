//! CSV export (doc §2.9, §5 "Exporting … needs DATA_EXPORT and is audited"): orders, order lines, products,
//! customers, expenses, stock movements and sales by product, for a §5 period where it applies.
//!
//! The files open cleanly in Excel: UTF-8 with a byte-order mark (Sinhala names survive), CRLF line ends,
//! RFC 4180 quoting, money as plain decimals (`1234.50`, from integer cents, no floats). Text cells that a
//! spreadsheet would run as a formula (`=`, `+`, `-`, `@`, tab, CR) get a leading `'` (CSV injection).
//! Cost columns are left out unless the viewer may see costs (`VIEW_COST`). The file is written as
//! `.partial`, flushed and renamed, then one `DATA_EXPORT` audit row records what was exported (never the
//! data itself).

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{Row, SqlitePool};

use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::db::Db;
use crate::db::repo::WriteCtx;
use crate::db::repo::reports::{self, SalesDimension};
use crate::domain::business_date::StoreCalendar;
use crate::domain::period::{DateRange, Period};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportKind {
    Orders,
    OrderLines,
    Products,
    Customers,
    Expenses,
    StockMovements,
    SalesByProduct,
}

impl ExportKind {
    pub fn code(self) -> &'static str {
        match self {
            ExportKind::Orders => "orders",
            ExportKind::OrderLines => "order-lines",
            ExportKind::Products => "products",
            ExportKind::Customers => "customers",
            ExportKind::Expenses => "expenses",
            ExportKind::StockMovements => "stock-movements",
            ExportKind::SalesByProduct => "sales-by-product",
        }
    }

    /// Products and customers are a snapshot; the rest belong to a period.
    pub fn has_period(self) -> bool {
        !matches!(self, ExportKind::Products | ExportKind::Customers)
    }
}

/// One CSV cell. Numbers are never guarded (a negative amount must stay a number).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    Text(String),
    Int(i64),
    Money(i64),
    Empty,
}

impl Cell {
    fn text(v: Option<String>) -> Cell {
        v.map_or(Cell::Empty, Cell::Text)
    }
}

/// Integer cents as a plain decimal: `-123456` → `-1234.56`.
pub fn money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

fn escape(raw: &str) -> String {
    let guarded = if raw.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{raw}") } else { raw.to_owned() };
    if guarded.contains([',', '"', '\n', '\r']) { format!("\"{}\"", guarded.replace('"', "\"\"")) } else { guarded }
}

/// The CSV text (with BOM) for a header and rows.
pub fn render(header: &[&str], rows: &[Vec<Cell>]) -> String {
    let mut out = String::from("\u{feff}");
    out.push_str(&header.iter().map(|h| escape(h)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
    for row in rows {
        let line: Vec<String> = row
            .iter()
            .map(|c| match c {
                Cell::Text(s) => escape(s),
                Cell::Int(n) => n.to_string(),
                Cell::Money(m) => money(*m),
                Cell::Empty => String::new(),
            })
            .collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out
}

/// A built export, before it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub header: Vec<&'static str>,
    pub rows: Vec<Vec<Cell>>,
}

fn s(row: &sqlx::sqlite::SqliteRow, col: &str) -> AppResult<Cell> {
    Ok(Cell::text(row.try_get::<Option<String>, _>(col)?))
}

fn i(row: &sqlx::sqlite::SqliteRow, col: &str) -> AppResult<Cell> {
    Ok(row.try_get::<Option<i64>, _>(col)?.map_or(Cell::Empty, Cell::Int))
}

fn m(row: &sqlx::sqlite::SqliteRow, col: &str) -> AppResult<Cell> {
    Ok(row.try_get::<Option<i64>, _>(col)?.map_or(Cell::Empty, Cell::Money))
}

fn yes_no(row: &sqlx::sqlite::SqliteRow, col: &str) -> AppResult<Cell> {
    Ok(Cell::Text(if row.try_get::<i64, _>(col)? != 0 { "yes" } else { "no" }.to_owned()))
}

const DEVICE_CODE: &str =
    "CASE WHEN instr(o.receipt_no, '-') > 0 THEN substr(o.receipt_no, 1, instr(o.receipt_no, '-') - 1) ELSE '' END";

/// Build the rows for one export. `with_cost` adds the cost columns (`VIEW_COST`).
pub async fn build(
    pool: &SqlitePool,
    store_id: &str,
    cal: &StoreCalendar,
    kind: ExportKind,
    period: &Period,
    with_cost: bool,
) -> AppResult<(Table, DateRange)> {
    let range = if kind.has_period() { period.resolve(cal)? } else { DateRange { from: None, to: None } };
    let (from, to) = (range.from.as_deref(), range.to.as_deref());
    let mut rows = Vec::new();
    let header: Vec<&'static str> = match kind {
        ExportKind::Orders => {
            let found = sqlx::query(&format!(
                "SELECT o.receipt_no, o.business_date, o.completed_at, {DEVICE_CODE} AS counter, o.status,
                        s.display_name AS cashier, o.item_count, o.gross_sales, o.discount_total, o.net_sales,
                        o.tax_total, o.grand_total, o.cost_total,
                        (SELECT group_concat(p.method || ' ' || printf('%d.%02d', p.amount / 100, p.amount % 100), '; ')
                         FROM order_payments p WHERE p.order_id = o.id) AS payments,
                        o.customer_id IS NOT NULL AS with_customer, o.void_reason, a.display_name AS void_approved_by
                 FROM orders o LEFT JOIN staff s ON s.id = o.staff_id
                 LEFT JOIN staff a ON a.id = COALESCE(o.void_approved_by, o.voided_by)
                 WHERE o.store_id = ?1 AND o.status IN ('completed', 'voided')
                   AND (?2 IS NULL OR o.business_date >= ?2) AND (?3 IS NULL OR o.business_date <= ?3)
                 ORDER BY o.completed_at, o.id"
            ))
            .bind(store_id)
            .bind(from)
            .bind(to)
            .fetch_all(pool)
            .await?;
            for r in &found {
                let mut row = vec![
                    s(r, "receipt_no")?,
                    s(r, "business_date")?,
                    s(r, "completed_at")?,
                    s(r, "counter")?,
                    s(r, "status")?,
                    s(r, "cashier")?,
                    i(r, "item_count")?,
                    m(r, "gross_sales")?,
                    m(r, "discount_total")?,
                    m(r, "net_sales")?,
                    m(r, "tax_total")?,
                    m(r, "grand_total")?,
                ];
                if with_cost {
                    row.push(m(r, "cost_total")?);
                }
                row.extend([
                    s(r, "payments")?,
                    yes_no(r, "with_customer")?,
                    s(r, "void_reason")?,
                    s(r, "void_approved_by")?,
                ]);
                rows.push(row);
            }
            let mut h = vec![
                "receipt_no",
                "business_date",
                "completed_at",
                "counter",
                "status",
                "cashier",
                "items",
                "gross_sales",
                "discounts",
                "net_sales",
                "tax",
                "total",
            ];
            if with_cost {
                h.push("cost");
            }
            h.extend(["payments", "customer_attached", "void_reason", "void_approved_by"]);
            h
        }
        ExportKind::OrderLines => {
            let found = sqlx::query(
                "SELECT o.receipt_no, o.business_date, o.status, oi.line_no, oi.name, p.sku, oi.is_custom, oi.qty,
                        oi.list_price, oi.unit_price, oi.line_discount, oi.order_discount_share, oi.line_total,
                        oi.line_net, oi.cost_total, oi.cost_known, b.code AS batch
                 FROM order_items oi JOIN orders o ON o.id = oi.order_id
                 LEFT JOIN products p ON p.id = oi.product_id LEFT JOIN batches b ON b.id = oi.batch_id
                 WHERE o.store_id = ?1 AND o.status IN ('completed', 'voided')
                   AND (?2 IS NULL OR o.business_date >= ?2) AND (?3 IS NULL OR o.business_date <= ?3)
                 ORDER BY o.completed_at, o.id, oi.line_no",
            )
            .bind(store_id)
            .bind(from)
            .bind(to)
            .fetch_all(pool)
            .await?;
            for r in &found {
                let mut row = vec![
                    s(r, "receipt_no")?,
                    s(r, "business_date")?,
                    s(r, "status")?,
                    i(r, "line_no")?,
                    s(r, "name")?,
                    s(r, "sku")?,
                    yes_no(r, "is_custom")?,
                    i(r, "qty")?,
                    m(r, "list_price")?,
                    m(r, "unit_price")?,
                    m(r, "line_discount")?,
                    m(r, "order_discount_share")?,
                    m(r, "line_total")?,
                    m(r, "line_net")?,
                ];
                if with_cost {
                    row.extend([m(r, "cost_total")?, yes_no(r, "cost_known")?]);
                }
                row.push(s(r, "batch")?);
                rows.push(row);
            }
            let mut h = vec![
                "receipt_no",
                "business_date",
                "status",
                "line",
                "name",
                "sku",
                "quick_sale",
                "qty",
                "list_price",
                "unit_price",
                "line_discount",
                "bill_discount_share",
                "line_total",
                "line_net",
            ];
            if with_cost {
                h.extend(["cost", "cost_known"]);
            }
            h.push("batch");
            h
        }
        ExportKind::Products => {
            let found = sqlx::query(
                "SELECT p.sku, p.barcode, p.name, p.name_si, c.name AS category, p.price, p.default_cost,
                        p.track_stock, COALESCE(sl.on_hand, 0) AS on_hand, p.is_active
                 FROM products p LEFT JOIN categories c ON c.id = p.category_id
                 LEFT JOIN stock_levels sl ON sl.product_id = p.id
                 WHERE p.store_id = ?1 AND p.deleted_at IS NULL
                 ORDER BY p.name COLLATE NOCASE, p.id",
            )
            .bind(store_id)
            .fetch_all(pool)
            .await?;
            for r in &found {
                let mut row = vec![
                    s(r, "sku")?,
                    s(r, "barcode")?,
                    s(r, "name")?,
                    s(r, "name_si")?,
                    s(r, "category")?,
                    m(r, "price")?,
                ];
                if with_cost {
                    row.push(m(r, "default_cost")?);
                }
                row.extend([yes_no(r, "track_stock")?, i(r, "on_hand")?, yes_no(r, "is_active")?]);
                rows.push(row);
            }
            let mut h = vec!["sku", "barcode", "name", "name_si", "category", "price"];
            if with_cost {
                h.push("default_cost");
            }
            h.extend(["track_stock", "on_hand", "active"]);
            h
        }
        ExportKind::Customers => {
            let found = sqlx::query(
                "SELECT c.phone_e164, c.display_name, c.note, c.first_seen_at, COALESCE(cs.visit_count, 0) AS visits,
                        COALESCE(cs.total_spend, 0) AS spend, cs.first_visit_date, cs.last_visit_date
                 FROM customers c LEFT JOIN customer_stats cs ON cs.customer_id = c.id
                 WHERE c.store_id = ?1 AND c.erased_at IS NULL AND c.deleted_at IS NULL
                 ORDER BY spend DESC, c.id",
            )
            .bind(store_id)
            .fetch_all(pool)
            .await?;
            for r in &found {
                rows.push(vec![
                    s(r, "phone_e164")?,
                    s(r, "display_name")?,
                    s(r, "note")?,
                    s(r, "first_seen_at")?,
                    i(r, "visits")?,
                    m(r, "spend")?,
                    s(r, "first_visit_date")?,
                    s(r, "last_visit_date")?,
                ]);
            }
            vec!["phone", "name", "note", "first_seen_at", "visits", "total_spend", "first_visit", "last_visit"]
        }
        ExportKind::Expenses => {
            let found = sqlx::query(
                "SELECT e.business_date, c.name AS category, e.amount, e.payment_method, e.paid_from_drawer, e.payee,
                        e.note, s.display_name AS recorded_by
                 FROM expenses e JOIN expense_categories c ON c.id = e.category_id
                 LEFT JOIN staff s ON s.id = e.recorded_by
                 WHERE e.store_id = ?1 AND e.deleted_at IS NULL
                   AND (?2 IS NULL OR e.business_date >= ?2) AND (?3 IS NULL OR e.business_date <= ?3)
                 ORDER BY e.business_date, e.created_at, e.id",
            )
            .bind(store_id)
            .bind(from)
            .bind(to)
            .fetch_all(pool)
            .await?;
            for r in &found {
                rows.push(vec![
                    s(r, "business_date")?,
                    s(r, "category")?,
                    m(r, "amount")?,
                    s(r, "payment_method")?,
                    yes_no(r, "paid_from_drawer")?,
                    s(r, "payee")?,
                    s(r, "note")?,
                    s(r, "recorded_by")?,
                ]);
            }
            vec!["business_date", "category", "amount", "method", "from_drawer", "payee", "note", "recorded_by"]
        }
        ExportKind::StockMovements => {
            let found = sqlx::query(
                "SELECT m.business_date, m.created_at, p.name AS product, p.sku, b.code AS batch, m.reason, m.qty_delta,
                        m.unit_cost, m.revenue_net, m.note, s.display_name AS staff
                 FROM stock_movements m JOIN products p ON p.id = m.product_id
                 JOIN batch_items bi ON bi.id = m.batch_item_id JOIN batches b ON b.id = bi.batch_id
                 LEFT JOIN staff s ON s.id = m.staff_id
                 WHERE m.store_id = ?1
                   AND (?2 IS NULL OR m.business_date >= ?2) AND (?3 IS NULL OR m.business_date <= ?3)
                 ORDER BY m.created_at, m.id",
            )
            .bind(store_id)
            .bind(from)
            .bind(to)
            .fetch_all(pool)
            .await?;
            for r in &found {
                let mut row = vec![
                    s(r, "business_date")?,
                    s(r, "created_at")?,
                    s(r, "product")?,
                    s(r, "sku")?,
                    s(r, "batch")?,
                    s(r, "reason")?,
                    i(r, "qty_delta")?,
                ];
                if with_cost {
                    row.push(m(r, "unit_cost")?);
                }
                row.extend([m(r, "revenue_net")?, s(r, "note")?, s(r, "staff")?]);
                rows.push(row);
            }
            let mut h = vec!["business_date", "created_at", "product", "sku", "batch", "reason", "qty"];
            if with_cost {
                h.push("unit_cost");
            }
            h.extend(["revenue_net", "note", "staff"]);
            h
        }
        ExportKind::SalesByProduct => {
            for r in reports::sales_by(pool, cal, period, None, SalesDimension::Product).await? {
                let mut row = vec![
                    Cell::Text(r.label.unwrap_or_else(|| "Quick sales".to_owned())),
                    Cell::Int(r.qty),
                    Cell::Money(r.net),
                ];
                if with_cost {
                    row.extend([
                        r.cost.map_or(Cell::Empty, Cell::Money),
                        r.cost.map_or(Cell::Empty, |c| Cell::Money(r.net - c)),
                    ]);
                }
                rows.push(row);
            }
            let mut h = vec!["product", "qty", "revenue_net"];
            if with_cost {
                h.extend(["cost", "gross_profit"]);
            }
            h
        }
    };
    Ok((Table { header, rows }, range))
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ExportOutcome {
    pub path: String,
    #[specta(type = specta_typescript::Number)]
    pub rows: i64,
}

/// `gayanshop-orders-2026-11-01_2026-11-30-T01-20261108-0930.csv` (all time / snapshot: no range).
pub fn file_name(
    kind: ExportKind,
    range: &DateRange,
    device_code: &str,
    at: DateTime<Utc>,
    tz: chrono_tz::Tz,
) -> String {
    let span = match (&range.from, &range.to) {
        (Some(f), Some(t)) if f == t => format!("-{f}"),
        (Some(f), Some(t)) => format!("-{f}_{t}"),
        _ => String::new(),
    };
    format!("gayanshop-{}{span}-{device_code}-{}.csv", kind.code(), at.with_timezone(&tz).format("%Y%m%d-%H%M%S"))
}

fn write_atomic(dir: &Path, name: &str, content: &str) -> AppResult<PathBuf> {
    if !dir.is_dir() {
        return Err(AppError::validation("EXPORT_DIR_INVALID", dir.display().to_string()));
    }
    let target = dir.join(name);
    let tmp = dir.join(format!("{name}.partial"));
    let written = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &target)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(AppError::validation("EXPORT_WRITE_FAILED", e.to_string()));
    }
    Ok(target)
}

/// What to export, and where.
#[derive(Debug, Clone)]
pub struct ExportRequest<'a> {
    pub kind: ExportKind,
    pub period: &'a Period,
    /// The viewer may see costs (`VIEW_COST`).
    pub with_cost: bool,
    pub dir: &'a Path,
    /// For the file name (store-local time).
    pub at: DateTime<Utc>,
}

/// Build, write and audit one export.
pub async fn export(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    cal: &StoreCalendar,
    req: &ExportRequest<'_>,
) -> AppResult<ExportOutcome> {
    let (kind, with_cost) = (req.kind, req.with_cost);
    let (table, range) = build(&db.pool, &ctx.store_id, cal, kind, req.period, with_cost).await?;
    let name = file_name(kind, &range, &ctx.device_code, req.at, cal.timezone);
    let path = write_atomic(req.dir, &name, &render(&table.header, &table.rows))?;
    let rows = i64::try_from(table.rows.len()).unwrap_or(i64::MAX);
    let mut tx = db.write().await?;
    audit::record(
        tx.conn(),
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::DATA_EXPORT,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("export"),
            entity_id: Some(kind.code()),
            metadata: json!({ "kind": kind.code(), "rows": rows, "from": range.from, "to": range.to,
                              "with_cost": with_cost, "file": name }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(ExportOutcome { path: path.display().to_string(), rows })
}
