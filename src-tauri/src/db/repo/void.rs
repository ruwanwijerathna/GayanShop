//! Same-session void (doc §3.3): the only correction to a sale. Whole order only, on the counter that sold it,
//! while its cash session is still open; a reason from the fixed list (free text required for `OTHER`);
//! `VOID_ORDER` or a supervisor's approval bound to the order id. One transaction:
//!
//! 1. `completed → voided` (the only update the order state machine allows); the rollup triggers remove the
//!    order from the day's sales, payments and the customer's stats;
//! 2. a `void` movement mirroring every sale slice exactly (same layer, unit cost, negated revenue);
//! 3. `VOID_ORDER` audit; 4. VOID slip (+ drawer kick for cash) print jobs; 5. outbox.

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use super::checkout::Checkpoint;
use super::print_jobs::{self, PrintKind};
use super::stock::{self, Movement, StockReason};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::db::{Db, outbox};
use crate::domain::permissions::{Perm, PermMask};
use crate::error::{AppError, AppResult};

const NOTE_MAX: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VoidReason {
    WrongItem,
    WrongPrice,
    WrongQty,
    DuplicateScan,
    CustomerCancelled,
    Other,
}

impl VoidReason {
    pub fn code(self) -> &'static str {
        match self {
            VoidReason::WrongItem => "WRONG_ITEM",
            VoidReason::WrongPrice => "WRONG_PRICE",
            VoidReason::WrongQty => "WRONG_QTY",
            VoidReason::DuplicateScan => "DUPLICATE_SCAN",
            VoidReason::CustomerCancelled => "CUSTOMER_CANCELLED",
            VoidReason::Other => "OTHER",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct VoidInput {
    pub order_id: String,
    pub reason: VoidReason,
    /// Required for `OTHER`.
    pub note: Option<String>,
    /// A supervisor's single-use approval bound to (VOID_ORDER, this order id, this cashier).
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct VoidResult {
    pub order_id: String,
    pub receipt_no: String,
    #[specta(type = specta_typescript::Number)]
    pub grand_total: i64,
    /// Cash to hand back to the customer (the cash payments of the order, net of change).
    #[specta(type = specta_typescript::Number)]
    pub cash_returned: i64,
}

/// Who is voiding, with their effective permissions (from the session).
#[derive(Debug, Clone)]
pub struct Voider {
    pub staff_id: String,
    pub perms: PermMask,
}

/// An order of the counter's open session, for the till's void list (F10).
#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct SessionOrder {
    pub order_id: String,
    pub receipt_no: String,
    /// `completed` or `voided`.
    pub status: String,
    #[specta(type = specta_typescript::Number)]
    pub grand_total: i64,
    #[specta(type = specta_typescript::Number)]
    pub item_count: i64,
    pub staff_name: String,
    pub completed_at: String,
}

#[derive(FromRow)]
struct OrderRow {
    receipt_no: String,
    status: String,
    device_id: String,
    session_status: String,
    grand_total: i64,
}

#[derive(FromRow)]
struct SaleSlice {
    product_id: String,
    batch_item_id: String,
    order_item_id: String,
    qty_delta: i64,
    unit_cost: i64,
    revenue_net: i64,
}

/// The open session's orders on this counter, newest first (what F10 lists).
pub async fn session_orders(pool: &SqlitePool, device_id: &str) -> AppResult<Vec<SessionOrder>> {
    Ok(sqlx::query_as(
        "SELECT o.id AS order_id, o.receipt_no, o.status, o.grand_total, o.item_count, s.display_name AS staff_name,
                o.completed_at
         FROM orders o
         JOIN cash_sessions cs ON cs.id = o.cash_session_id
         JOIN staff s ON s.id = o.staff_id
         WHERE cs.device_id = ?1 AND cs.status = 'open' AND o.status IN ('completed', 'voided')
         ORDER BY o.completed_at DESC, o.id DESC",
    )
    .bind(device_id)
    .fetch_all(pool)
    .await?)
}

async fn has_perm(conn: &mut SqliteConnection, staff_id: &str, perm: Perm) -> AppResult<bool> {
    let perms: Option<i64> =
        sqlx::query_scalar("SELECT perms FROM v_staff_effective WHERE staff_id = ?1 AND is_active = 1")
            .bind(staff_id)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(perms.is_some_and(|p| PermMask(p).has(perm)))
}

/// Void a sale. `approver` is a supervisor already resolved from a consumed approval token.
pub async fn void_order(
    db: &Db,
    ctx: &WriteCtx,
    voider: &Voider,
    input: &VoidInput,
    approver: Option<&str>,
    cp: &dyn Checkpoint,
) -> AppResult<VoidResult> {
    let note = input.note.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    if note.as_ref().is_some_and(|n| n.chars().count() > NOTE_MAX) {
        return Err(AppError::validation("TEXT_TOO_LONG", format!("max {NOTE_MAX}")));
    }
    if input.reason == VoidReason::Other && note.is_none() {
        return Err(AppError::validation("VOID_NOTE_REQUIRED", "explain the reason"));
    }
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let order: OrderRow = sqlx::query_as(
        "SELECT o.receipt_no, o.status, o.device_id, cs.status AS session_status, o.grand_total
         FROM orders o JOIN cash_sessions cs ON cs.id = o.cash_session_id
         WHERE o.id = ?1 AND o.store_id = ?2",
    )
    .bind(&input.order_id)
    .bind(&ctx.store_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| AppError::not_found("order"))?;
    if order.status == "voided" {
        return Err(AppError::validation("ORDER_ALREADY_VOIDED", order.receipt_no));
    }
    if order.device_id != ctx.device_id {
        return Err(AppError::validation("VOID_OTHER_DEVICE", order.receipt_no));
    }
    if order.session_status != "open" {
        return Err(AppError::validation("VOID_SESSION_CLOSED", order.receipt_no));
    }

    // Authority: VOID_ORDER, or a different active staff member who holds it.
    let approved_by = if voider.perms.has(Perm::VOID_ORDER) {
        None
    } else {
        let Some(a) = approver else {
            return Err(AppError::ApprovalRequired { perm: Perm::VOID_ORDER });
        };
        if a == voider.staff_id {
            return Err(AppError::validation("SELF_APPROVAL", a.to_owned()));
        }
        if !has_perm(conn, a, Perm::VOID_ORDER).await? {
            return Err(AppError::validation("APPROVER_NOT_AUTHORISED", a.to_owned()));
        }
        Some(a)
    };
    cp.reached("validated")?;

    // 1. The state change (re-checked by trg_orders_void_verify); rollups follow by trigger.
    sqlx::query(
        "UPDATE orders SET status = 'voided', voided_at = ?1, voided_by = ?2, void_approved_by = ?3, void_reason = ?4,
                void_note = ?5, updated_at = ?1, updated_by_device = ?6, version = version + 1
         WHERE id = ?7 AND status = 'completed'",
    )
    .bind(&ctx.now)
    .bind(&voider.staff_id)
    .bind(approved_by)
    .bind(input.reason.code())
    .bind(note.as_deref())
    .bind(&ctx.device_id)
    .bind(&input.order_id)
    .execute(&mut *conn)
    .await?;
    cp.reached("voided")?;

    // 2. Mirror every sale slice back into exactly the layer it came from.
    let slices: Vec<SaleSlice> = sqlx::query_as(
        "SELECT m.product_id, m.batch_item_id, m.order_item_id, m.qty_delta, m.unit_cost,
                COALESCE(m.revenue_net, 0) AS revenue_net
         FROM stock_movements m JOIN order_items oi ON oi.id = m.order_item_id
         WHERE oi.order_id = ?1 AND m.reason = 'sale'
         ORDER BY oi.line_no, m.id",
    )
    .bind(&input.order_id)
    .fetch_all(&mut *conn)
    .await?;
    for s in &slices {
        stock::record(
            conn,
            ctx,
            &Movement {
                product_id: &s.product_id,
                batch_item_id: &s.batch_item_id,
                qty_delta: -s.qty_delta,
                reason: StockReason::Void,
                ref_type: Some("order"),
                ref_id: Some(&input.order_id),
                order_item_id: Some(&s.order_item_id),
                unit_cost: s.unit_cost,
                revenue_net: Some(-s.revenue_net),
                staff_id: Some(&voider.staff_id),
                approved_by,
                note: None,
            },
        )
        .await?;
    }
    cp.reached("reversed")?;

    // 3. Audit (receipt, amount, reason, approver).
    let cash_returned: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0) FROM order_payments WHERE order_id = ?1 AND method = 'cash'",
    )
    .bind(&input.order_id)
    .fetch_one(&mut *conn)
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
            action: AuditAction::VOID_ORDER,
            staff_id: Some(&voider.staff_id),
            approved_by,
            entity_type: Some("order"),
            entity_id: Some(&input.order_id),
            metadata: json!({ "receipt_no": order.receipt_no, "amount": order.grand_total,
                              "cash_returned": cash_returned, "reason": input.reason.code(), "note": note }),
        },
    )
    .await?;
    cp.reached("audit")?;

    // 4. VOID slip for the cash-up, and the drawer if cash goes back.
    let slip = json!({ "order_id": input.order_id, "receipt_no": order.receipt_no, "amount": order.grand_total,
                       "reason": input.reason.code(), "note": note, "voided_by": voider.staff_id,
                       "approved_by": approved_by });
    print_jobs::enqueue(conn, ctx, PrintKind::VoidSlip, Some(&input.order_id), &slip).await?;
    if cash_returned > 0 {
        print_jobs::enqueue(conn, ctx, PrintKind::DrawerKick, Some(&input.order_id), &json!({ "reason": "void" }))
            .await?;
    }
    cp.reached("print_jobs")?;

    // 5. The voided order for sync (the void slices were queued as written).
    outbox::enqueue_upsert(conn, "orders", &input.order_id, &ctx.now).await?;
    cp.reached("outbox")?;

    tx.commit().await?;
    Ok(VoidResult {
        order_id: input.order_id.clone(),
        receipt_no: order.receipt_no,
        grand_total: order.grand_total,
        cash_returned,
    })
}
