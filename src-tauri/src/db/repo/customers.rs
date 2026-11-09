//! Customers keyed by phone (doc §3.8): id = UUIDv5(store, E.164 phone), so offline counters converge on one
//! row. Orders store only `customer_id`, never the phone. An erased customer who returns is re-activated
//! under the same id with a fresh profile.

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{SqliteConnection, SqlitePool};

use super::WriteCtx;
use super::receipts::{self, ReceiptData};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::clock::stable_id;
use crate::db::{Db, outbox};
use crate::domain::business_date::StoreCalendar;
use crate::domain::period::{DateRange, Granularity, Period, bucket_sql};
use crate::domain::phone;
use crate::error::{AppError, AppResult};

const NAME_MAX: usize = 60;

/// The customer a sale is linked to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerRef {
    pub id: String,
    pub created: bool,
}

pub fn customer_id(store_id: &str, phone_e164: &str) -> String {
    stable_id("customer", store_id, phone_e164)
}

/// Normalise a typed phone (`PHONE_INVALID` otherwise) and an optional display name.
pub fn clean(phone_input: &str, name: Option<&str>) -> AppResult<(String, Option<String>)> {
    let e164 = phone::normalize_lk(phone_input).map_err(|e| AppError::validation("PHONE_INVALID", e.to_string()))?;
    let name = name.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    if name.as_ref().is_some_and(|n| n.chars().count() > NAME_MAX) {
        return Err(AppError::validation("CUSTOMER_NAME_INVALID", format!("max {NAME_MAX}")));
    }
    Ok((e164, name))
}

/// Link to the existing customer for this phone, or create (or re-activate) one. Runs inside the caller's
/// transaction, before the order insert (the order's `customer_id` is frozen once inserted).
pub async fn resolve_or_create(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    actor: &Actor,
    phone_e164: &str,
    name: Option<&str>,
) -> AppResult<CustomerRef> {
    let id = customer_id(&ctx.store_id, phone_e164);
    let existing: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT display_name, erased_at FROM customers WHERE id = ?1")
            .bind(&id)
            .fetch_optional(&mut *conn)
            .await?;
    let created = match existing {
        None => {
            sqlx::query(
                "INSERT INTO customers (id, store_id, phone_e164, display_name, first_seen_at, created_at, updated_at,
                                        updated_by_device)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?5, ?6)",
            )
            .bind(&id)
            .bind(&ctx.store_id)
            .bind(phone_e164)
            .bind(name)
            .bind(&ctx.now)
            .bind(&ctx.device_id)
            .execute(&mut *conn)
            .await?;
            true
        }
        Some((_, Some(_erased))) => {
            sqlx::query(
                "UPDATE customers SET phone_e164 = ?1, display_name = ?2, note = NULL, erased_at = NULL,
                        deleted_at = NULL, updated_at = ?3, updated_by_device = ?4, version = version + 1
                 WHERE id = ?5",
            )
            .bind(phone_e164)
            .bind(name)
            .bind(&ctx.now)
            .bind(&ctx.device_id)
            .bind(&id)
            .execute(&mut *conn)
            .await?;
            true
        }
        Some((stored_name, None)) => {
            if name.is_some() && name != stored_name.as_deref() {
                sqlx::query(
                    "UPDATE customers SET display_name = ?1, updated_at = ?2, updated_by_device = ?3,
                            version = version + 1
                     WHERE id = ?4",
                )
                .bind(name)
                .bind(&ctx.now)
                .bind(&ctx.device_id)
                .bind(&id)
                .execute(&mut *conn)
                .await?;
                outbox::enqueue_upsert(conn, "customers", &id, &ctx.now).await?;
            }
            false
        }
    };
    if created {
        outbox::enqueue_upsert(conn, "customers", &id, &ctx.now).await?;
        audit::record(
            conn,
            &AuditCtx {
                store_id: &ctx.store_id,
                device_id: &ctx.device_id,
                business_date: &ctx.business_date,
                now: &ctx.now,
            },
            AuditEntry {
                action: AuditAction::CUSTOMER_CREATE,
                staff_id: Some(&actor.staff_id),
                approved_by: None,
                entity_type: Some("customer"),
                entity_id: Some(&id),
                // Never the phone itself: the audit log is immutable and erasure must stay possible.
                metadata: json!({ "named": name.is_some() }),
            },
        )
        .await?;
    }
    Ok(CustomerRef { id, created })
}

/// What the till sees after 9–10 digits (doc §3.8): no history without `CUSTOMER_VIEW`.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CustomerLookup {
    /// `None` ⇒ a new number (or an erased profile): the customer is created at checkout.
    pub customer_id: Option<String>,
    pub phone_e164: String,
    /// `077 123 4567`, for the cart header.
    pub phone_display: String,
    pub display_name: Option<String>,
    /// Completed visits so far (voids excluded); the header shows "Visit #N+1".
    #[specta(type = specta_typescript::Number)]
    pub visit_count: i64,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CustomerLookupInput {
    pub phone: String,
}

/// Look a typed phone up by its UUIDv5 id (a primary-key read, so it is instant and offline). An erased
/// profile reads as new: its pre-erasure visits stay anonymous.
pub async fn lookup(pool: &SqlitePool, store_id: &str, phone_input: &str) -> AppResult<CustomerLookup> {
    let (e164, _) = clean(phone_input, None)?;
    let id = customer_id(store_id, &e164);
    let row: Option<(Option<String>, Option<String>, i64)> = sqlx::query_as(
        "SELECT c.display_name, c.erased_at, COALESCE(cs.visit_count, 0)
         FROM customers c LEFT JOIN customer_stats cs ON cs.customer_id = c.id
         WHERE c.id = ?1 AND c.deleted_at IS NULL",
    )
    .bind(&id)
    .fetch_optional(pool)
    .await?;
    let phone_display = phone::format_national(&e164).unwrap_or_else(|| e164.clone());
    Ok(match row {
        Some((display_name, None, visit_count)) => {
            CustomerLookup { customer_id: Some(id), phone_e164: e164, phone_display, display_name, visit_count }
        }
        _ => CustomerLookup { customer_id: None, phone_e164: e164, phone_display, display_name: None, visit_count: 0 },
    })
}

/// PDPA erasure (doc §3.8): clear the phone, name and note and stamp `erased_at`. The row and its id stay, so
/// past orders keep an anonymous `customer_id` and every total is unchanged; orders are never touched.
pub async fn erase(db: &Db, ctx: &WriteCtx, actor: &Actor, id: &str) -> AppResult<()> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let erased: Option<Option<String>> =
        sqlx::query_scalar("SELECT erased_at FROM customers WHERE id = ?1 AND store_id = ?2")
            .bind(id)
            .bind(&ctx.store_id)
            .fetch_optional(&mut *conn)
            .await?;
    match erased {
        None => return Err(AppError::not_found("customer")),
        Some(Some(_)) => return Err(AppError::validation("CUSTOMER_ALREADY_ERASED", id.to_owned())),
        Some(None) => {}
    }
    sqlx::query(
        "UPDATE customers SET phone_e164 = NULL, display_name = NULL, note = NULL, erased_at = ?1, updated_at = ?1,
                updated_by_device = ?2, version = version + 1
         WHERE id = ?3",
    )
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    audit::record(
        conn,
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::CUSTOMER_ERASE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("customer"),
            entity_id: Some(id),
            metadata: json!({}),
        },
    )
    .await?;
    outbox::supersede(conn, "customers", id).await?;
    outbox::enqueue_upsert(conn, "customers", id, &ctx.now).await?;
    tx.commit().await?;
    Ok(())
}

// ───────── back office (Block 2.5, `CUSTOMER_VIEW`) ─────────

const NOTE_MAX: usize = 500;
const LIST_MAX: i64 = 500;
const TOP_N: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CustomerSort {
    #[default]
    Spend,
    Visits,
    LastVisit,
    Name,
}

/// One customer in the back-office list (figures from `customer_stats`: completed orders only).
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CustomerRow {
    pub id: String,
    pub phone_e164: Option<String>,
    /// `077 123 4567`.
    pub phone_display: Option<String>,
    pub display_name: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub visit_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub total_spend: i64,
    #[specta(type = specta_typescript::Number)]
    pub total_net: i64,
    /// `total_spend / visit_count`, rounded down; 0 with no visits.
    #[specta(type = specta_typescript::Number)]
    pub avg_basket: i64,
    pub first_visit_date: Option<String>,
    pub last_visit_date: Option<String>,
}

#[derive(sqlx::FromRow)]
struct RowDb {
    id: String,
    phone_e164: Option<String>,
    display_name: Option<String>,
    visit_count: i64,
    total_spend: i64,
    total_net: i64,
    first_visit_date: Option<String>,
    last_visit_date: Option<String>,
}

impl From<RowDb> for CustomerRow {
    fn from(r: RowDb) -> Self {
        CustomerRow {
            phone_display: r.phone_e164.as_deref().and_then(phone::format_national),
            avg_basket: if r.visit_count > 0 { r.total_spend / r.visit_count } else { 0 },
            id: r.id,
            phone_e164: r.phone_e164,
            display_name: r.display_name,
            visit_count: r.visit_count,
            total_spend: r.total_spend,
            total_net: r.total_net,
            first_visit_date: r.first_visit_date,
            last_visit_date: r.last_visit_date,
        }
    }
}

const ROW_SELECT: &str = "SELECT c.id, c.phone_e164, c.display_name, COALESCE(cs.visit_count, 0) AS visit_count,
        COALESCE(cs.total_spend, 0) AS total_spend, COALESCE(cs.total_net, 0) AS total_net,
        cs.first_visit_date, cs.last_visit_date
 FROM customers c LEFT JOIN customer_stats cs ON cs.customer_id = c.id";

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CustomerList {
    pub rows: Vec<CustomerRow>,
    /// More customers match than were returned: narrow the search.
    pub truncated: bool,
}

/// `0771234567`, `771234567` and `+94 77…` all search the same national digits; anything else searches the
/// name (case-insensitive). Returns `(is_phone, LIKE pattern)`.
fn search_pattern(search: &str) -> Option<(bool, String)> {
    let s = search.trim();
    if s.is_empty() {
        return None;
    }
    let compact: String = s.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    let international = compact.starts_with('+');
    let digits = compact.trim_start_matches('+');
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        // A number typed from its start (`07…`, `+94…`, a full `94XXXXXXXXX`) matches the start of the 9
        // national digits; bare digits (`4567`) match anywhere in them.
        let anchored = match digits.strip_prefix("94") {
            Some(rest) if international || digits.len() >= 11 => Some(rest),
            _ => digits.strip_prefix('0'),
        };
        return Some((
            true,
            match anchored {
                Some(national) => format!("+94{national}%"),
                None => format!("+94%{digits}%"),
            },
        ));
    }
    let escaped = s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    Some((false, format!("%{escaped}%")))
}

/// The customer list (doc §3.8): erased profiles are left out; search by phone or name; sorted by spend,
/// visits, last visit or name; at most 500 rows (`truncated` says to narrow the search).
pub async fn list(
    pool: &SqlitePool,
    store_id: &str,
    search: Option<&str>,
    sort: CustomerSort,
    limit: Option<u32>,
) -> AppResult<CustomerList> {
    let limit = limit.map_or(LIST_MAX, |l| i64::from(l).clamp(1, LIST_MAX));
    let order = match sort {
        CustomerSort::Spend => "total_spend DESC, c.id",
        CustomerSort::Visits => "visit_count DESC, total_spend DESC, c.id",
        CustomerSort::LastVisit => "cs.last_visit_date DESC NULLS LAST, c.id",
        CustomerSort::Name => "c.display_name COLLATE NOCASE ASC NULLS LAST, c.id",
    };
    let pattern = search.and_then(search_pattern);
    // Unfiltered spend and last-visit lists start from `customer_stats` and walk its index, reading one page
    // instead of sorting every customer (doc §13.4: < 20 ms). Every customer gets a stats row at their first
    // checkout; anyone without one (never sold to) would have no spend and no visit, so they belong after
    // the page anyway and appear when searched for.
    if pattern.is_none() && matches!(sort, CustomerSort::Spend | CustomerSort::LastVisit) {
        let order = if sort == CustomerSort::Spend {
            "cs.total_spend DESC, cs.customer_id"
        } else {
            "cs.last_visit_date DESC, cs.customer_id"
        };
        let rows: Vec<RowDb> = sqlx::query_as(&format!(
            "SELECT c.id, c.phone_e164, c.display_name, cs.visit_count, cs.total_spend, cs.total_net,
                    cs.first_visit_date, cs.last_visit_date
             FROM customer_stats cs JOIN customers c ON c.id = cs.customer_id
             WHERE c.store_id = ?1 AND c.erased_at IS NULL AND c.deleted_at IS NULL
             ORDER BY {order} LIMIT ?2"
        ))
        .bind(store_id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?;
        let truncated = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
        let rows = rows.into_iter().take(usize::try_from(limit).unwrap_or(0)).map(CustomerRow::from).collect();
        return Ok(CustomerList { rows, truncated });
    }
    let filter = match &pattern {
        None => "",
        Some((true, _)) => " AND c.phone_e164 LIKE ?2",
        Some((false, _)) => " AND c.display_name LIKE ?2 ESCAPE '\\'",
    };
    let sql = format!(
        "{ROW_SELECT} WHERE c.store_id = ?1 AND c.erased_at IS NULL AND c.deleted_at IS NULL{filter}
         ORDER BY {order} LIMIT ?3"
    );
    let rows: Vec<RowDb> = sqlx::query_as(&sql)
        .bind(store_id)
        .bind(pattern.as_ref().map(|(_, p)| p.as_str()))
        .bind(limit + 1)
        .fetch_all(pool)
        .await?;
    let truncated = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
    let rows = rows.into_iter().take(usize::try_from(limit).unwrap_or(0)).map(CustomerRow::from).collect();
    Ok(CustomerList { rows, truncated })
}

/// A product or category in a customer's top list (completed orders only).
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, sqlx::FromRow)]
pub struct TopItem {
    /// Product or category id; `None` = quick sales (top products) or uncategorised (top categories).
    pub id: Option<String>,
    pub name: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
    #[specta(type = specta_typescript::Number)]
    pub net: i64,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CustomerProfile {
    pub customer: CustomerRow,
    pub note: Option<String>,
    pub first_seen_at: String,
    pub erased_at: Option<String>,
    pub top_products: Vec<TopItem>,
    pub top_categories: Vec<TopItem>,
}

pub async fn profile(pool: &SqlitePool, store_id: &str, id: &str) -> AppResult<CustomerProfile> {
    let row: Option<RowDb> = sqlx::query_as(&format!("{ROW_SELECT} WHERE c.id = ?1 AND c.store_id = ?2"))
        .bind(id)
        .bind(store_id)
        .fetch_optional(pool)
        .await?;
    let row = row.ok_or_else(|| AppError::not_found("customer"))?;
    let (note, first_seen_at, erased_at): (Option<String>, String, Option<String>) =
        sqlx::query_as("SELECT note, first_seen_at, erased_at FROM customers WHERE id = ?1")
            .bind(id)
            .fetch_one(pool)
            .await?;
    // Products by revenue; all quick sales together form one row (id NULL).
    let top_products: Vec<TopItem> = sqlx::query_as(
        "SELECT oi.product_id AS id, CASE WHEN oi.product_id IS NULL THEN NULL ELSE MAX(oi.name) END AS name,
                SUM(oi.qty) AS qty, SUM(oi.line_net) AS net
         FROM orders o JOIN order_items oi ON oi.order_id = o.id
         WHERE o.customer_id = ?1 AND o.status = 'completed'
         GROUP BY oi.product_id ORDER BY net DESC, qty DESC LIMIT ?2",
    )
    .bind(id)
    .bind(TOP_N)
    .fetch_all(pool)
    .await?;
    let top_categories: Vec<TopItem> = sqlx::query_as(
        "SELECT p.category_id AS id, MAX(cat.name) AS name, SUM(oi.qty) AS qty, SUM(oi.line_net) AS net
         FROM orders o JOIN order_items oi ON oi.order_id = o.id
         LEFT JOIN products p ON p.id = oi.product_id
         LEFT JOIN categories cat ON cat.id = p.category_id
         WHERE o.customer_id = ?1 AND o.status = 'completed'
         GROUP BY p.category_id ORDER BY net DESC, qty DESC LIMIT ?2",
    )
    .bind(id)
    .bind(TOP_N)
    .fetch_all(pool)
    .await?;
    Ok(CustomerProfile { customer: row.into(), note, first_seen_at, erased_at, top_products, top_categories })
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct HistoryCursor {
    pub business_date: String,
    pub id: String,
}

/// One order in a customer's purchase history (completed or voided; voids carry no spend).
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, sqlx::FromRow)]
pub struct HistoryOrder {
    pub id: String,
    pub receipt_no: String,
    pub business_date: String,
    pub completed_at: Option<String>,
    pub status: String,
    #[specta(type = specta_typescript::Number)]
    pub item_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub grand_total: i64,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct HistoryPage {
    pub rows: Vec<HistoryOrder>,
    pub next_cursor: Option<HistoryCursor>,
}

/// Purchase history, newest first, keyset-paged over `ix_orders_customer (customer_id, business_date)`.
pub async fn history(
    pool: &SqlitePool,
    customer_id: &str,
    cursor: Option<&HistoryCursor>,
    limit: u32,
) -> AppResult<HistoryPage> {
    let limit = i64::from(limit.clamp(1, 500));
    let mut rows: Vec<HistoryOrder> = sqlx::query_as(
        "SELECT id, receipt_no, business_date, completed_at, status, item_count, grand_total
         FROM orders
         WHERE customer_id = ?1 AND status IN ('completed', 'voided')
           AND (?2 IS NULL OR (business_date, id) < (?2, ?3))
         ORDER BY business_date DESC, id DESC LIMIT ?4",
    )
    .bind(customer_id)
    .bind(cursor.map(|c| c.business_date.as_str()))
    .bind(cursor.map(|c| c.id.as_str()))
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;
    let next_cursor = if i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit {
        rows.truncate(usize::try_from(limit).unwrap_or(0));
        rows.last().map(|r| HistoryCursor { business_date: r.business_date.clone(), id: r.id.clone() })
    } else {
        None
    };
    Ok(HistoryPage { rows, next_cursor })
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct TopCustomer {
    pub customer_id: String,
    /// `None` once erased (shown as an anonymous customer).
    pub display_name: Option<String>,
    pub phone_display: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub visits: i64,
    #[specta(type = specta_typescript::Number)]
    pub spend: i64,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct NewReturning {
    /// The week's first day or `YYYY-MM`.
    pub bucket: String,
    #[specta(type = specta_typescript::Number)]
    pub new_customers: i64,
    #[specta(type = specta_typescript::Number)]
    pub returning_customers: i64,
}

/// Customer analytics for a §5 period (doc §3.8): top customers by spend, new vs returning per week or
/// month, and the share of sales with a customer attached. Completed orders only.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CustomerAnalytics {
    pub range: DateRange,
    pub top: Vec<TopCustomer>,
    pub trend: Vec<NewReturning>,
    #[specta(type = specta_typescript::Number)]
    pub sales_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub customer_orders: i64,
    /// `customer_orders / sales_count` in basis points.
    #[specta(type = specta_typescript::Number)]
    pub attached_bp: i64,
}

#[derive(sqlx::FromRow)]
struct TopCustomerDb {
    customer_id: String,
    display_name: Option<String>,
    phone_e164: Option<String>,
    visits: i64,
    spend: i64,
}

/// A customer is "new" in the bucket holding their first completed visit, "returning" in every later one.
pub async fn analytics(
    pool: &SqlitePool,
    store_id: &str,
    cal: &StoreCalendar,
    period: &Period,
    granularity: Granularity,
    top_n: u32,
) -> AppResult<CustomerAnalytics> {
    let range = period.resolve(cal)?;
    let (from, to) = (range.from.as_deref(), range.to.as_deref());
    let top_n = i64::from(top_n.clamp(1, 50));
    // All time reads the derived table (`ix_customer_stats_spend`); a bounded period sums its orders.
    let top: Vec<TopCustomerDb> = if from.is_none() && to.is_none() {
        sqlx::query_as(
            "SELECT cs.customer_id, c.display_name, c.phone_e164, cs.visit_count AS visits, cs.total_spend AS spend
             FROM customer_stats cs JOIN customers c ON c.id = cs.customer_id
             WHERE c.store_id = ?1 AND cs.visit_count > 0
             ORDER BY cs.total_spend DESC, cs.customer_id LIMIT ?2",
        )
        .bind(store_id)
        .bind(top_n)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT o.customer_id, c.display_name, c.phone_e164, COUNT(*) AS visits, SUM(o.grand_total) AS spend
             FROM orders o JOIN customers c ON c.id = o.customer_id
             WHERE o.store_id = ?1 AND o.status = 'completed' AND o.customer_id IS NOT NULL
               AND o.business_date BETWEEN ?2 AND ?3
             GROUP BY o.customer_id ORDER BY spend DESC, o.customer_id LIMIT ?4",
        )
        .bind(store_id)
        .bind(from)
        .bind(to)
        .bind(top_n)
        .fetch_all(pool)
        .await?
    };
    // New vs returning is per week or month (doc §3.8); a day bucket would always read "returning".
    let g = if granularity == Granularity::Day { Granularity::Week } else { granularity };
    let bucket = bucket_sql("o.business_date", g, cal);
    let first_bucket = bucket_sql("cs.first_visit_date", g, cal);
    let trend: Vec<(String, i64, i64)> = sqlx::query_as(&format!(
        "SELECT b, SUM(fb = b), SUM(fb <> b) FROM (
           SELECT DISTINCT {bucket} AS b, o.customer_id, {first_bucket} AS fb
           FROM orders o JOIN customer_stats cs ON cs.customer_id = o.customer_id
           WHERE o.store_id = ?1 AND o.status = 'completed' AND o.customer_id IS NOT NULL
             AND (?2 IS NULL OR o.business_date >= ?2) AND (?3 IS NULL OR o.business_date <= ?3))
         GROUP BY b ORDER BY b"
    ))
    .bind(store_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let (sales_count, customer_orders): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(sales_count), 0), COALESCE(SUM(customer_orders), 0) FROM daily_sales_summary
         WHERE (?1 IS NULL OR business_date >= ?1) AND (?2 IS NULL OR business_date <= ?2)",
    )
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await?;
    Ok(CustomerAnalytics {
        range,
        top: top
            .into_iter()
            .map(|r| TopCustomer {
                phone_display: r.phone_e164.as_deref().and_then(phone::format_national),
                customer_id: r.customer_id,
                display_name: r.display_name,
                visits: r.visits,
                spend: r.spend,
            })
            .collect(),
        trend: trend
            .into_iter()
            .map(|(bucket, new_customers, returning_customers)| NewReturning {
                bucket,
                new_customers,
                returning_customers,
            })
            .collect(),
        sales_count,
        customer_orders,
        attached_bp: if sales_count > 0 { customer_orders * 10_000 / sales_count } else { 0 },
    })
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CustomerUpdate {
    pub display_name: Option<String>,
    pub note: Option<String>,
}

fn tidy(v: Option<&str>) -> Option<String> {
    v.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// Edit the optional name and note (doc §3.8: phone, name and note are all we store). The phone is the
/// identity and never changes; an erased profile can't be edited.
pub async fn update(db: &Db, ctx: &WriteCtx, actor: &Actor, id: &str, input: &CustomerUpdate) -> AppResult<()> {
    let (name, note) = (tidy(input.display_name.as_deref()), tidy(input.note.as_deref()));
    if name.as_ref().is_some_and(|n| n.chars().count() > NAME_MAX) {
        return Err(AppError::validation("CUSTOMER_NAME_INVALID", format!("max {NAME_MAX}")));
    }
    if note.as_ref().is_some_and(|n| n.chars().count() > NOTE_MAX) {
        return Err(AppError::validation("CUSTOMER_NOTE_INVALID", format!("max {NOTE_MAX}")));
    }
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let current: Option<(Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT display_name, note, erased_at FROM customers WHERE id = ?1 AND store_id = ?2")
            .bind(id)
            .bind(&ctx.store_id)
            .fetch_optional(&mut *conn)
            .await?;
    let (old_name, old_note) = match current {
        None => return Err(AppError::not_found("customer")),
        Some((_, _, Some(_))) => return Err(AppError::validation("CUSTOMER_ALREADY_ERASED", id.to_owned())),
        Some((n, t, None)) => (n, t),
    };
    let mut changed = Vec::new();
    if old_name != name {
        changed.push("display_name");
    }
    if old_note != note {
        changed.push("note");
    }
    if changed.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE customers SET display_name = ?1, note = ?2, updated_at = ?3, updated_by_device = ?4,
                version = version + 1
         WHERE id = ?5",
    )
    .bind(&name)
    .bind(&note)
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    audit::record(
        conn,
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::CUSTOMER_UPDATE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("customer"),
            entity_id: Some(id),
            // Field names only: personal data never enters the immutable audit log.
            metadata: json!({ "changed": changed }),
        },
    )
    .await?;
    outbox::enqueue_upsert(conn, "customers", id, &ctx.now).await?;
    tx.commit().await?;
    Ok(())
}

/// Receipt drill-down from a customer's history: the order must belong to that customer.
pub async fn order_receipt(pool: &SqlitePool, customer_id: &str, order_id: &str) -> AppResult<ReceiptData> {
    let owner: Option<Option<String>> =
        sqlx::query_scalar("SELECT customer_id FROM orders WHERE id = ?1").bind(order_id).fetch_optional(pool).await?;
    if owner.flatten().as_deref() != Some(customer_id) {
        return Err(AppError::not_found("order"));
    }
    receipts::load(pool, order_id).await
}
