//! Cash sessions (doc §3.4): one open session per counter; pay-in / pay-out / no-sale drawer movements; close
//! with a count, variance and the Z-report. Closing ends the void window for every order in the session, and
//! the SQL trigger `trg_cash_session_no_reopen` makes that permanent.

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use super::print_jobs::{self, PrintKind};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::clock::new_id;
use crate::db::{Db, outbox};
use crate::error::{AppError, AppResult};

const MONEY_MAX: i64 = 10_000_000_000;
const REASON_MAX: usize = 120;
/// Sri Lankan notes and coins in use, in cents (Rs 5,000 … Rs 1).
pub const LKR_DENOMINATIONS: [i64; 11] =
    [500_000, 200_000, 100_000, 50_000, 10_000, 5_000, 2_000, 1_000, 500, 200, 100];

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct CashSession {
    pub id: String,
    pub device_id: String,
    pub business_date: String,
    /// `open` or `closed`.
    pub status: String,
    pub opened_by: String,
    pub opened_by_name: String,
    pub opened_at: String,
    #[specta(type = specta_typescript::Number)]
    pub opening_float: i64,
    pub closed_by: Option<String>,
    pub closed_by_name: Option<String>,
    pub closed_at: Option<String>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub expected_cash: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub counted_cash: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub variance: Option<i64>,
    pub close_note: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DrawerMovementKind {
    PayIn,
    PayOut,
}

impl DrawerMovementKind {
    fn code(self) -> &'static str {
        match self {
            DrawerMovementKind::PayIn => "pay_in",
            DrawerMovementKind::PayOut => "pay_out",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct OpenSessionInput {
    #[specta(type = specta_typescript::Number)]
    pub opening_float: i64,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct DrawerMovementInput {
    pub kind: DrawerMovementKind,
    #[specta(type = specta_typescript::Number)]
    pub amount: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct DenominationCount {
    /// Face value in cents (one of `LKR_DENOMINATIONS`).
    #[specta(type = specta_typescript::Number)]
    pub value: i64,
    #[specta(type = specta_typescript::Number)]
    pub count: i64,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CloseInput {
    #[specta(type = specta_typescript::Number)]
    pub counted_cash: i64,
    /// Optional count by note/coin; when given it must add up to `counted_cash`.
    pub denominations: Option<Vec<DenominationCount>>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct PaymentTotal {
    pub method: String,
    #[specta(type = specta_typescript::Number)]
    pub count: i64,
    #[specta(type = specta_typescript::Number)]
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct VoidReasonTotal {
    pub reason: String,
    #[specta(type = specta_typescript::Number)]
    pub count: i64,
    #[specta(type = specta_typescript::Number)]
    pub value: i64,
}

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct VoidLine {
    pub receipt_no: String,
    #[specta(type = specta_typescript::Number)]
    pub value: i64,
    pub reason: String,
    pub note: Option<String>,
    pub voided_by: String,
    /// The supervisor who approved it; `None` when the voider held VOID_ORDER themselves.
    pub approved_by: Option<String>,
    pub voided_at: String,
}

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct DrawerMovement {
    /// `pay_in`, `pay_out` or `no_sale`.
    pub kind: String,
    #[specta(type = specta_typescript::Number)]
    pub amount: i64,
    pub reason: String,
    pub staff: String,
    pub approved_by: Option<String>,
    pub created_at: String,
}

/// Everything the Z-report prints for one session (also the X-report while it is still open).
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ZReport {
    pub session: CashSession,
    pub generated_at: String,
    #[specta(type = specta_typescript::Number)]
    pub sales_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub items_sold: i64,
    #[specta(type = specta_typescript::Number)]
    pub gross_sales: i64,
    #[specta(type = specta_typescript::Number)]
    pub discount_total: i64,
    #[specta(type = specta_typescript::Number)]
    pub net_sales: i64,
    /// What the till actually took: Σ (grand total − exchange credit), i.e. the payments.
    #[specta(type = specta_typescript::Number)]
    pub sales_total: i64,
    /// Exchanges in this session: item-for-item swaps, with the value of the goods taken back.
    #[specta(type = specta_typescript::Number)]
    pub exchanges_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub exchange_credit: i64,
    #[specta(type = specta_typescript::Number)]
    pub returned_items: i64,
    pub payments: Vec<PaymentTotal>,
    #[specta(type = specta_typescript::Number)]
    pub change_given: i64,
    #[specta(type = specta_typescript::Number)]
    pub overrides_count: i64,
    /// Σ (list − sold price) × qty on overridden lines.
    #[specta(type = specta_typescript::Number)]
    pub overrides_value: i64,
    #[specta(type = specta_typescript::Number)]
    pub quick_sale_lines: i64,
    #[specta(type = specta_typescript::Number)]
    pub quick_sale_net: i64,
    #[specta(type = specta_typescript::Number)]
    pub voids_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub voids_value: i64,
    pub void_reasons: Vec<VoidReasonTotal>,
    pub voids: Vec<VoidLine>,
    pub drawer: Vec<DrawerMovement>,
    #[specta(type = specta_typescript::Number)]
    pub pay_ins: i64,
    #[specta(type = specta_typescript::Number)]
    pub pay_outs: i64,
    #[specta(type = specta_typescript::Number)]
    pub no_sales: i64,
    #[specta(type = specta_typescript::Number)]
    pub opening_float: i64,
    #[specta(type = specta_typescript::Number)]
    pub cash_sales: i64,
    #[specta(type = specta_typescript::Number)]
    pub expected_cash: i64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub counted_cash: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub variance: Option<i64>,
}

const SESSION_SELECT: &str = "SELECT s.id, s.device_id, s.business_date, s.status, s.opened_by,
       o.display_name AS opened_by_name, s.opened_at, s.opening_float, s.closed_by,
       c.display_name AS closed_by_name, s.closed_at, s.expected_cash, s.counted_cash, s.variance, s.close_note
     FROM cash_sessions s
     JOIN staff o ON o.id = s.opened_by
     LEFT JOIN staff c ON c.id = s.closed_by";

fn audit_ctx(ctx: &WriteCtx) -> AuditCtx<'_> {
    AuditCtx { store_id: &ctx.store_id, device_id: &ctx.device_id, business_date: &ctx.business_date, now: &ctx.now }
}

async fn audit_row(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    actor: &Actor,
    action: AuditAction,
    entity: (&str, &str),
    metadata: serde_json::Value,
) -> AppResult<()> {
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some(entity.0),
            entity_id: Some(entity.1),
            metadata,
        },
    )
    .await
    .map(|_| ())
}

// ───────── reads ─────────

pub async fn get(conn: &mut SqliteConnection, id: &str) -> AppResult<CashSession> {
    sqlx::query_as(&format!("{SESSION_SELECT} WHERE s.id = ?1 AND s.deleted_at IS NULL"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::not_found("cash session"))
}

/// This counter's open session, if any.
pub async fn current(pool: &SqlitePool, device_id: &str) -> AppResult<Option<CashSession>> {
    Ok(sqlx::query_as(&format!(
        "{SESSION_SELECT} WHERE s.device_id = ?1 AND s.status = 'open' AND s.deleted_at IS NULL"
    ))
    .bind(device_id)
    .fetch_optional(pool)
    .await?)
}

/// The open session id on this counter, or `NO_OPEN_SESSION` (sales and drawer movements need one).
pub async fn require_open(conn: &mut SqliteConnection, device_id: &str) -> AppResult<String> {
    sqlx::query_scalar("SELECT id FROM cash_sessions WHERE device_id = ?1 AND status = 'open' AND deleted_at IS NULL")
        .bind(device_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::validation("NO_OPEN_SESSION", "open a cash session first"))
}

/// This counter's sessions, newest first (bounded by `limit`).
pub async fn list(pool: &SqlitePool, device_id: &str, limit: u32) -> AppResult<Vec<CashSession>> {
    Ok(sqlx::query_as(&format!(
        "{SESSION_SELECT} WHERE s.device_id = ?1 AND s.deleted_at IS NULL ORDER BY s.opened_at DESC, s.id DESC LIMIT ?2"
    ))
    .bind(device_id)
    .bind(i64::from(limit.clamp(1, 500)))
    .fetch_all(pool)
    .await?)
}

/// float + Σ cash taken on completed (not voided) orders, net of change + pay-ins − pay-outs.
pub async fn expected_cash(conn: &mut SqliteConnection, session_id: &str) -> AppResult<i64> {
    sqlx::query_scalar(
        "SELECT s.opening_float
              + COALESCE((SELECT SUM(p.amount) FROM order_payments p JOIN orders o ON o.id = p.order_id
                          WHERE o.cash_session_id = s.id AND o.status = 'completed' AND p.method = 'cash'), 0)
              + COALESCE((SELECT SUM(amount) FROM cash_movements WHERE cash_session_id = s.id AND type = 'pay_in'), 0)
              - COALESCE((SELECT SUM(amount) FROM cash_movements WHERE cash_session_id = s.id AND type = 'pay_out'), 0)
         FROM cash_sessions s WHERE s.id = ?1",
    )
    .bind(session_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| AppError::not_found("cash session"))
}

#[derive(FromRow)]
struct SalesRow {
    sales_count: i64,
    items_sold: i64,
    gross_sales: i64,
    discount_total: i64,
    net_sales: i64,
    sales_total: i64,
    change_given: i64,
    exchanges_count: i64,
    exchange_credit: i64,
    returned_items: i64,
}

/// The Z-report for a session (an X-report if it is still open: same numbers, no count yet).
pub async fn z_report(conn: &mut SqliteConnection, session_id: &str, now: &str) -> AppResult<ZReport> {
    let session = get(conn, session_id).await?;
    let sales: SalesRow = sqlx::query_as(
        "SELECT COUNT(*) AS sales_count, COALESCE(SUM(item_count), 0) AS items_sold,
                COALESCE(SUM(gross_sales - returned_gross), 0) AS gross_sales,
                COALESCE(SUM(discount_total - returned_discount), 0) AS discount_total,
                COALESCE(SUM(net_sales - returned_net), 0) AS net_sales,
                COALESCE(SUM(grand_total - exchange_credit), 0) AS sales_total,
                COALESCE(SUM(change_due), 0) AS change_given,
                COALESCE(SUM(exchange_credit > 0), 0) AS exchanges_count,
                COALESCE(SUM(exchange_credit), 0) AS exchange_credit,
                COALESCE(SUM(returned_items), 0) AS returned_items
         FROM orders WHERE cash_session_id = ?1 AND status = 'completed'",
    )
    .bind(session_id)
    .fetch_one(&mut *conn)
    .await?;
    let payments: Vec<PaymentTotal> = sqlx::query_as(
        "SELECT p.method, COUNT(*) AS count, SUM(p.amount) AS amount
         FROM order_payments p JOIN orders o ON o.id = p.order_id
         WHERE o.cash_session_id = ?1 AND o.status = 'completed'
         GROUP BY p.method ORDER BY p.method",
    )
    .bind(session_id)
    .fetch_all(&mut *conn)
    .await?;
    let (overrides_count, overrides_value, quick_sale_lines, quick_sale_net): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(oi.price_overridden), 0),
                COALESCE(SUM(CASE WHEN oi.price_overridden = 1 THEN (oi.list_price - oi.unit_price) * oi.qty END), 0),
                COALESCE(SUM(oi.is_custom), 0),
                COALESCE(SUM(CASE WHEN oi.is_custom = 1 THEN oi.line_net END), 0)
         FROM order_items oi JOIN orders o ON o.id = oi.order_id
         WHERE o.cash_session_id = ?1 AND o.status = 'completed'",
    )
    .bind(session_id)
    .fetch_one(&mut *conn)
    .await?;
    let void_reasons: Vec<VoidReasonTotal> = sqlx::query_as(
        "SELECT void_reason AS reason, COUNT(*) AS count, SUM(grand_total) AS value
         FROM orders WHERE cash_session_id = ?1 AND status = 'voided'
         GROUP BY void_reason ORDER BY void_reason",
    )
    .bind(session_id)
    .fetch_all(&mut *conn)
    .await?;
    let voids: Vec<VoidLine> = sqlx::query_as(
        "SELECT o.receipt_no, o.grand_total AS value, o.void_reason AS reason, o.void_note AS note,
                v.display_name AS voided_by, a.display_name AS approved_by, o.voided_at
         FROM orders o
         JOIN staff v ON v.id = o.voided_by
         LEFT JOIN staff a ON a.id = o.void_approved_by
         WHERE o.cash_session_id = ?1 AND o.status = 'voided'
         ORDER BY o.voided_at, o.id",
    )
    .bind(session_id)
    .fetch_all(&mut *conn)
    .await?;
    let drawer: Vec<DrawerMovement> = sqlx::query_as(
        "SELECT m.type AS kind, m.amount, m.reason, s.display_name AS staff, a.display_name AS approved_by,
                m.created_at
         FROM cash_movements m
         JOIN staff s ON s.id = m.staff_id
         LEFT JOIN staff a ON a.id = m.approved_by
         WHERE m.cash_session_id = ?1
         ORDER BY m.created_at, m.id",
    )
    .bind(session_id)
    .fetch_all(&mut *conn)
    .await?;
    let sum_of = |kind: &str| drawer.iter().filter(|m| m.kind == kind).map(|m| m.amount).sum::<i64>();
    let (pay_ins, pay_outs) = (sum_of("pay_in"), sum_of("pay_out"));
    let no_sales = i64::try_from(drawer.iter().filter(|m| m.kind == "no_sale").count()).unwrap_or(i64::MAX);
    let cash_sales = payments.iter().filter(|p| p.method == "cash").map(|p| p.amount).sum::<i64>();
    let expected = expected_cash(conn, session_id).await?;
    Ok(ZReport {
        generated_at: now.to_owned(),
        sales_count: sales.sales_count,
        items_sold: sales.items_sold,
        gross_sales: sales.gross_sales,
        discount_total: sales.discount_total,
        net_sales: sales.net_sales,
        sales_total: sales.sales_total,
        exchanges_count: sales.exchanges_count,
        exchange_credit: sales.exchange_credit,
        returned_items: sales.returned_items,
        payments,
        change_given: sales.change_given,
        overrides_count,
        overrides_value,
        quick_sale_lines,
        quick_sale_net,
        voids_count: void_reasons.iter().map(|v| v.count).sum(),
        voids_value: void_reasons.iter().map(|v| v.value).sum(),
        void_reasons,
        voids,
        drawer,
        pay_ins,
        pay_outs,
        no_sales,
        opening_float: session.opening_float,
        cash_sales,
        expected_cash: session.expected_cash.unwrap_or(expected),
        counted_cash: session.counted_cash,
        variance: session.variance,
        session,
    })
}

// ───────── writes ─────────

fn clean_reason(reason: &str) -> AppResult<String> {
    let r = reason.trim().to_owned();
    if r.is_empty() || r.chars().count() > REASON_MAX {
        return Err(AppError::validation("REASON_REQUIRED", format!("1–{REASON_MAX} characters")));
    }
    Ok(r)
}

pub async fn open(db: &Db, ctx: &WriteCtx, actor: &Actor, opening_float: i64) -> AppResult<CashSession> {
    if !(0..=MONEY_MAX).contains(&opening_float) {
        return Err(AppError::validation("FLOAT_INVALID", opening_float.to_string()));
    }
    let mut tx = db.write().await?;
    if require_open(tx.conn(), &ctx.device_id).await.is_ok() {
        return Err(AppError::validation("SESSION_ALREADY_OPEN", "close the open session first"));
    }
    let id = new_id();
    sqlx::query(
        "INSERT INTO cash_sessions (id, store_id, device_id, business_date, opened_by, opened_at, opening_float,
                                    created_at, updated_at, updated_by_device)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?6, ?3)",
    )
    .bind(&id)
    .bind(&ctx.store_id)
    .bind(&ctx.device_id)
    .bind(&ctx.business_date)
    .bind(&actor.staff_id)
    .bind(&ctx.now)
    .bind(opening_float)
    .execute(tx.conn())
    .await?;
    outbox::enqueue_upsert(tx.conn(), "cash_sessions", &id, &ctx.now).await?;
    audit_row(
        tx.conn(),
        ctx,
        actor,
        AuditAction::CASH_SESSION_OPEN,
        ("cash_session", &id),
        json!({ "opening_float": opening_float }),
    )
    .await?;
    let session = get(tx.conn(), &id).await?;
    tx.commit().await?;
    Ok(session)
}

pub(crate) async fn insert_movement(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    actor: &Actor,
    session_id: &str,
    kind: &str,
    amount: i64,
    reason: &str,
) -> AppResult<String> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO cash_movements (id, store_id, device_id, cash_session_id, type, amount, reason, staff_id,
                                     approved_by, business_date, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(&id)
    .bind(&ctx.store_id)
    .bind(&ctx.device_id)
    .bind(session_id)
    .bind(kind)
    .bind(amount)
    .bind(reason)
    .bind(&actor.staff_id)
    .bind(actor.approved_by.as_deref())
    .bind(&ctx.business_date)
    .bind(&ctx.now)
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "cash_movements", &id, &ctx.now).await?;
    print_jobs::enqueue(conn, ctx, PrintKind::DrawerKick, Some(&id), &json!({ "reason": kind })).await?;
    Ok(id)
}

/// Pay-in (float top-up) or pay-out (petty cash). Opens the drawer; audited.
pub async fn drawer_movement(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    kind: DrawerMovementKind,
    amount: i64,
    reason: &str,
) -> AppResult<ZReport> {
    if !(1..=MONEY_MAX).contains(&amount) {
        return Err(AppError::validation("AMOUNT_INVALID", amount.to_string()));
    }
    let reason = clean_reason(reason)?;
    let mut tx = db.write().await?;
    let session_id = require_open(tx.conn(), &ctx.device_id).await?;
    let id = insert_movement(tx.conn(), ctx, actor, &session_id, kind.code(), amount, &reason).await?;
    let action = match kind {
        DrawerMovementKind::PayIn => AuditAction::CASH_PAY_IN,
        DrawerMovementKind::PayOut => AuditAction::CASH_PAY_OUT,
    };
    audit_row(tx.conn(), ctx, actor, action, ("cash_movement", &id), json!({ "amount": amount, "reason": reason }))
        .await?;
    let report = z_report(tx.conn(), &session_id, &ctx.now).await?;
    tx.commit().await?;
    Ok(report)
}

/// Open the drawer without a sale (DRAWER_NO_SALE). Audited as `DRAWER_KICK`.
pub async fn no_sale(db: &Db, ctx: &WriteCtx, actor: &Actor, reason: &str) -> AppResult<()> {
    let reason = clean_reason(reason)?;
    let mut tx = db.write().await?;
    let session_id = require_open(tx.conn(), &ctx.device_id).await?;
    let id = insert_movement(tx.conn(), ctx, actor, &session_id, "no_sale", 0, &reason).await?;
    audit_row(
        tx.conn(),
        ctx,
        actor,
        AuditAction::DRAWER_KICK,
        ("cash_movement", &id),
        json!({ "kind": "no_sale", "reason": reason }),
    )
    .await?;
    tx.commit().await
}

fn check_denominations(input: &CloseInput) -> AppResult<Option<String>> {
    let Some(list) = &input.denominations else {
        return Ok(None);
    };
    let mut total: i64 = 0;
    for d in list {
        if !LKR_DENOMINATIONS.contains(&d.value) || !(0..=100_000).contains(&d.count) {
            return Err(AppError::validation("DENOMINATION_INVALID", format!("{} × {}", d.count, d.value)));
        }
        total += d.value * d.count;
    }
    if total != input.counted_cash {
        return Err(AppError::validation("DENOMINATIONS_MISMATCH", format!("{total} ≠ {}", input.counted_cash)));
    }
    let map: serde_json::Map<String, serde_json::Value> =
        list.iter().filter(|d| d.count > 0).map(|d| (d.value.to_string(), json!(d.count))).collect();
    Ok(Some(serde_json::Value::Object(map).to_string()))
}

/// Count the drawer and close this counter's session; queues the Z-report. Ends the void window for good.
pub async fn close(db: &Db, ctx: &WriteCtx, actor: &Actor, input: &CloseInput) -> AppResult<ZReport> {
    if !(0..=MONEY_MAX).contains(&input.counted_cash) {
        return Err(AppError::validation("COUNT_INVALID", input.counted_cash.to_string()));
    }
    let denominations = check_denominations(input)?;
    let note = input.note.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    if note.as_ref().is_some_and(|n| n.chars().count() > 500) {
        return Err(AppError::validation("REASON_REQUIRED", "note too long"));
    }
    let mut tx = db.write().await?;
    let session_id = require_open(tx.conn(), &ctx.device_id).await?;
    let expected = expected_cash(tx.conn(), &session_id).await?;
    let variance = input.counted_cash - expected;
    sqlx::query(
        "UPDATE cash_sessions SET status = 'closed', closed_by = ?1, closed_at = ?2, expected_cash = ?3,
                counted_cash = ?4, variance = ?5, denominations = ?6, close_note = ?7, updated_at = ?2,
                updated_by_device = ?8, version = version + 1
         WHERE id = ?9 AND status = 'open'",
    )
    .bind(&actor.staff_id)
    .bind(&ctx.now)
    .bind(expected)
    .bind(input.counted_cash)
    .bind(variance)
    .bind(denominations.as_deref())
    .bind(note.as_deref())
    .bind(&ctx.device_id)
    .bind(&session_id)
    .execute(tx.conn())
    .await?;
    outbox::enqueue_upsert(tx.conn(), "cash_sessions", &session_id, &ctx.now).await?;
    audit_row(
        tx.conn(),
        ctx,
        actor,
        AuditAction::CASH_SESSION_CLOSE,
        ("cash_session", &session_id),
        json!({ "expected_cash": expected, "counted_cash": input.counted_cash, "variance": variance }),
    )
    .await?;
    let report = z_report(tx.conn(), &session_id, &ctx.now).await?;
    let payload = serde_json::to_value(&report).map_err(|e| AppError::internal(e.to_string()))?;
    print_jobs::enqueue(tx.conn(), ctx, PrintKind::ZReport, Some(&session_id), &payload).await?;
    tx.commit().await?;
    Ok(report)
}
