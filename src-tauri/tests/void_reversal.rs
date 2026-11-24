//! Block 1.5 — same-session void: whole order, exact slice reversal into the original layers, rollups and
//! customer stats recomputed, VOID slip + drawer kick, approvals for cashiers, the reason list, and
//! "closed session ⇒ refused for every role". `v_integrity_void_unreversed` stays empty throughout.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};

use common::{CASHIER, CASHIER_PIN, SUP, SUP_PIN, Till, cart, cash, line, till};
use gayanshop_lib::auth::{ApprovalRequest, request_approval};
use gayanshop_lib::commands::void::void;
use gayanshop_lib::db::repo::cash::{self, CloseInput};
use gayanshop_lib::db::repo::checkout::{self, CheckoutResult, Checkpoint, NoFaults};
use gayanshop_lib::db::repo::void::{self as voids, VoidInput, VoidReason, Voider};
use gayanshop_lib::domain::permissions::Perm;
use gayanshop_lib::error::{AppError, AppResult};

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

async fn voider(t: &Till, staff: &str) -> Voider {
    let s = t.seller(staff).await;
    Voider { staff_id: s.staff_id, perms: s.perms }
}

fn void_input(order_id: &str, reason: VoidReason) -> VoidInput {
    VoidInput { order_id: order_id.into(), reason, note: None, approval_id: None }
}

async fn unreversed(t: &Till) -> i64 {
    t.count("SELECT COUNT(*) FROM v_integrity_void_unreversed").await
}

/// A cash sale of 3 shoes across two layers (2 @ Rs 800, 1 @ Rs 900) to a known customer.
async fn sale(t: &Till, token: &str) -> (String, CheckoutResult) {
    let shoe = t.stocked("Shoe", 150_000, &[(2, 80_000), (5, 90_000)]).await;
    let mut input = cart(token, vec![line(&shoe, 150_000, 3)], vec![cash(450_000, Some(500_000))]);
    input.customer_phone = Some("0771234567".into());
    let r = checkout::checkout(&t.t.db, &t.ctx(), &t.seller(CASHIER).await, &input, None, &NoFaults).await.unwrap();
    (shoe, r)
}

#[tokio::test]
async fn a_void_reverses_every_slice_into_its_own_layer() {
    let t = till().await;
    let (shoe, sold) = sale(&t, "v1").await;
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{shoe}'")).await, 4);
    assert_eq!(t.count("SELECT visit_count FROM customer_stats").await, 1);

    let r = voids::void_order(
        &t.t.db,
        &t.ctx(),
        &voider(&t, SUP).await,
        &void_input(&sold.order_id, VoidReason::WrongQty),
        None,
        &NoFaults,
    )
    .await
    .unwrap();
    assert_eq!((r.grand_total, r.cash_returned, r.receipt_no.as_str()), (450_000, 450_000, sold.receipt_no.as_str()));

    // Each sale slice has a mirror: same layer, unit cost and order line; +qty and −revenue.
    let pairs: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT batch_item_id, unit_cost, SUM(qty_delta), SUM(revenue_net) FROM stock_movements
         WHERE reason IN ('sale', 'void') GROUP BY batch_item_id, unit_cost ORDER BY unit_cost",
    )
    .fetch_all(t.pool())
    .await
    .unwrap();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.iter().all(|(_, _, qty, revenue)| *qty == 0 && *revenue == 0), "{pairs:?}");
    assert_eq!(t.count("SELECT COUNT(*) FROM stock_movements WHERE reason = 'void'").await, 2);
    assert_eq!(unreversed(&t).await, 0);
    // Stock and every layer are exactly as before the sale.
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{shoe}'")).await, 7);
    assert_eq!(
        t.count("SELECT COUNT(*) FROM batch_item_stock WHERE sold_qty <> 0 OR sold_revenue <> 0 OR sold_cost <> 0")
            .await,
        0
    );

    // Rollups: out of sales and payments, into voids; the customer's stats drop the visit.
    let (sales, voided, value): (i64, i64, i64) =
        sqlx::query_as("SELECT sales_count, void_count, void_value FROM daily_sales_summary")
            .fetch_one(t.pool())
            .await
            .unwrap();
    assert_eq!((sales, voided, value), (0, 1, 450_000));
    assert_eq!(t.count("SELECT COALESCE(SUM(amount), 0) FROM daily_payment_summary").await, 0);
    assert_eq!(t.count("SELECT COALESCE(SUM(visit_count), 0) FROM customer_stats").await, 0);

    // The drawer: expected cash no longer counts the sale; slip + kick queued; audited; synced as voided.
    let mut conn = t.pool().acquire().await.unwrap();
    assert_eq!(cash::expected_cash(&mut conn, &t.session_id).await.unwrap(), 0);
    let z = cash::z_report(&mut conn, &t.session_id, common::NOW).await.unwrap();
    drop(conn);
    assert_eq!((z.sales_count, z.voids_count, z.voids_value), (0, 1, 450_000));
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE kind = 'void_slip'").await, 1);
    assert_eq!(
        t.count(&format!(
            "SELECT COUNT(*) FROM print_jobs WHERE kind = 'drawer_kick' AND ref_id = '{}'",
            sold.order_id
        ))
        .await,
        2
    );
    let meta: String = sqlx::query_scalar("SELECT metadata FROM audit_logs WHERE action = 'VOID_ORDER'")
        .fetch_one(t.pool())
        .await
        .unwrap();
    let meta: serde_json::Value = serde_json::from_str(&meta).unwrap();
    assert_eq!((meta["reason"].as_str(), meta["amount"].as_i64()), (Some("WRONG_QTY"), Some(450_000)));
    let synced: String = sqlx::query_scalar(
        "SELECT json_extract(payload, '$.status') FROM sync_outbox WHERE table_name = 'orders' ORDER BY rowid DESC LIMIT 1",
    )
    .fetch_one(t.pool())
    .await
    .unwrap();
    assert_eq!(synced, "voided");
    assert_eq!(t.count("SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'stock_movements' AND json_extract(payload, '$.reason') = 'void'").await, 2);

    // Whole order, once.
    let again = voids::void_order(
        &t.t.db,
        &t.ctx(),
        &voider(&t, SUP).await,
        &void_input(&sold.order_id, VoidReason::WrongQty),
        None,
        &NoFaults,
    )
    .await;
    assert_eq!(code(again.unwrap_err()), "ORDER_ALREADY_VOIDED");
    // The slices can't be over-reversed by hand either (SQL guard).
    let extra = sqlx::query(
        "INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, order_item_id, unit_cost, revenue_net, business_date, created_at)
         SELECT 'x', store_id, device_id, product_id, batch_item_id, 1, 'void', order_item_id, unit_cost, 0, business_date, created_at
         FROM stock_movements WHERE reason = 'void' LIMIT 1",
    )
    .execute(t.pool())
    .await;
    assert!(extra.unwrap_err().to_string().contains("not exceed"));
}

#[tokio::test]
async fn a_cashier_needs_a_supervisor_who_holds_void_order() {
    let t = till().await;
    let (_, sold) = sale(&t, "v2").await;
    let input = void_input(&sold.order_id, VoidReason::DuplicateScan);
    let cashier = voider(&t, CASHIER).await;
    let attempt = voids::void_order(&t.t.db, &t.ctx(), &cashier, &input, None, &NoFaults).await;
    assert_eq!(attempt.unwrap_err(), AppError::ApprovalRequired { perm: Perm::VOID_ORDER });
    assert_eq!(
        code(voids::void_order(&t.t.db, &t.ctx(), &cashier, &input, Some(CASHIER), &NoFaults).await.unwrap_err()),
        "SELF_APPROVAL"
    );
    // Another cashier can't approve: they don't hold VOID_ORDER themselves.
    common::exec(t.pool(), &format!("INSERT INTO staff (id, store_id, display_name, role_id, pin_hash, created_at, updated_at, updated_by_device) SELECT 'cashier2', store_id, 'cashier2', role_id, pin_hash, created_at, updated_at, updated_by_device FROM staff WHERE id = '{CASHIER}'")).await;
    assert_eq!(
        code(voids::void_order(&t.t.db, &t.ctx(), &cashier, &input, Some("cashier2"), &NoFaults).await.unwrap_err()),
        "APPROVER_NOT_AUTHORISED"
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM orders WHERE status = 'voided'").await, 0);

    voids::void_order(&t.t.db, &t.ctx(), &cashier, &input, Some(SUP), &NoFaults).await.unwrap();
    let (by, approved): (String, Option<String>) =
        sqlx::query_as("SELECT voided_by, void_approved_by FROM orders").fetch_one(t.pool()).await.unwrap();
    assert_eq!((by.as_str(), approved.as_deref()), (CASHIER, Some(SUP)));
    assert_eq!(
        t.count(&format!("SELECT COUNT(*) FROM audit_logs WHERE action = 'VOID_ORDER' AND approved_by = '{SUP}'"))
            .await,
        1
    );
    assert_eq!(unreversed(&t).await, 0);
}

#[tokio::test]
async fn other_needs_a_note() {
    let t = till().await;
    let (_, sold) = sale(&t, "v3").await;
    let sup = voider(&t, SUP).await;
    let bare = void_input(&sold.order_id, VoidReason::Other);
    assert_eq!(
        code(voids::void_order(&t.t.db, &t.ctx(), &sup, &bare, None, &NoFaults).await.unwrap_err()),
        "VOID_NOTE_REQUIRED"
    );
    let blank = VoidInput { note: Some("   ".into()), ..bare.clone() };
    assert_eq!(
        code(voids::void_order(&t.t.db, &t.ctx(), &sup, &blank, None, &NoFaults).await.unwrap_err()),
        "VOID_NOTE_REQUIRED"
    );
    let explained = VoidInput { note: Some("Customer's card declined after print".into()), ..bare };
    voids::void_order(&t.t.db, &t.ctx(), &sup, &explained, None, &NoFaults).await.unwrap();
    let note: Option<String> = sqlx::query_scalar("SELECT void_note FROM orders").fetch_one(t.pool()).await.unwrap();
    assert_eq!(note.as_deref(), Some("Customer's card declined after print"));
}

#[tokio::test]
async fn a_closed_session_refuses_every_role() {
    let t = till().await;
    let (_, sold) = sale(&t, "v4").await;
    cash::close(&t.t.db, &t.ctx(), &t.owner(), &CloseInput { counted_cash: 450_000, denominations: None, note: None })
        .await
        .unwrap();
    let input = void_input(&sold.order_id, VoidReason::WrongItem);
    for (staff, approver) in [(CASHIER, Some(SUP)), (SUP, None), (t.s.owner_staff_id.as_str(), None)] {
        let v = voider(&t, staff).await;
        let refused = voids::void_order(&t.t.db, &t.ctx(), &v, &input, approver, &NoFaults).await;
        assert_eq!(code(refused.unwrap_err()), "VOID_SESSION_CLOSED", "{staff}");
    }
    // And the SQL policy says the same if Rust were bypassed.
    let direct = sqlx::query(&format!(
        "UPDATE orders SET status = 'voided', voided_at = 'x', voided_by = '{}', void_reason = 'WRONG_ITEM' WHERE id = '{}'",
        t.s.owner_staff_id, sold.order_id
    ))
    .execute(t.pool())
    .await;
    assert!(direct.unwrap_err().to_string().contains("sales are final"));
    assert_eq!(t.count("SELECT COUNT(*) FROM orders WHERE status = 'completed'").await, 1);
    assert_eq!(t.count("SELECT COUNT(*) FROM stock_movements WHERE reason = 'void'").await, 0);
}

#[tokio::test]
async fn only_the_selling_counter_can_void_and_unknown_orders_are_not_found() {
    let t = till().await;
    let (_, sold) = sale(&t, "v5").await;
    let sup = voider(&t, SUP).await;
    let mut elsewhere = t.ctx();
    elsewhere.device_id = "another-counter".into();
    let refused = voids::void_order(
        &t.t.db,
        &elsewhere,
        &sup,
        &void_input(&sold.order_id, VoidReason::WrongItem),
        None,
        &NoFaults,
    )
    .await;
    assert_eq!(code(refused.unwrap_err()), "VOID_OTHER_DEVICE");
    let missing =
        voids::void_order(&t.t.db, &t.ctx(), &sup, &void_input("nope", VoidReason::WrongItem), None, &NoFaults).await;
    assert_eq!(missing.unwrap_err(), AppError::not_found("order"));
}

struct FailAt(usize, AtomicUsize);

impl Checkpoint for FailAt {
    fn reached(&self, step: &'static str) -> AppResult<()> {
        if self.1.fetch_add(1, Ordering::SeqCst) == self.0 {
            Err(AppError::internal(format!("injected crash after {step}")))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn a_crash_mid_void_leaves_the_sale_untouched() {
    let t = till().await;
    let (shoe, sold) = sale(&t, "v6").await;
    let sup = voider(&t, SUP).await;
    let input = void_input(&sold.order_id, VoidReason::CustomerCancelled);
    for at in 0..6 {
        let cp = FailAt(at, AtomicUsize::new(0));
        assert!(matches!(
            voids::void_order(&t.t.db, &t.ctx(), &sup, &input, None, &cp).await,
            Err(AppError::Internal { .. })
        ));
        assert_eq!(t.count("SELECT COUNT(*) FROM orders WHERE status = 'completed'").await, 1, "step {at}");
        assert_eq!(t.count("SELECT COUNT(*) FROM stock_movements WHERE reason = 'void'").await, 0, "step {at}");
        assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE kind = 'void_slip'").await, 0, "step {at}");
        assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{shoe}'")).await, 4);
    }
    voids::void_order(&t.t.db, &t.ctx(), &sup, &input, None, &NoFaults).await.unwrap();
    assert_eq!(unreversed(&t).await, 0);
}

#[tokio::test]
async fn command_flow_binds_the_approval_to_the_order() {
    let t = till().await;
    let (_, first) = sale(&t, "v7").await;
    let mug = t.product("Mug", 10_000, false, 0, None).await;
    let second = checkout::checkout(
        &t.t.db,
        &t.ctx(),
        &t.seller(CASHIER).await,
        &cart("v8", vec![line(&mug, 10_000, 1)], vec![cash(10_000, None)]),
        None,
        &NoFaults,
    )
    .await
    .unwrap();
    t.login(CASHIER, CASHIER_PIN).await;
    let ctx = t.ctx();
    let mut input = void_input(&second.order_id, VoidReason::WrongItem);
    assert_eq!(
        void(&t.auth, &t.clock, &t.t.db, &ctx, &input).await.unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::VOID_ORDER }
    );

    // An approval for the FIRST order can't void the second one.
    let grant = request_approval(
        &t.t.db,
        &t.auth,
        &t.clock,
        &t.dev,
        ApprovalRequest { perm: Perm::VOID_ORDER, target_ref: &first.order_id, approver_id: SUP, pin: SUP_PIN },
    )
    .await
    .unwrap();
    input.approval_id = Some(grant.approval_id);
    assert_eq!(void(&t.auth, &t.clock, &t.t.db, &ctx, &input).await.unwrap_err(), AppError::InvalidApproval);

    let grant = request_approval(
        &t.t.db,
        &t.auth,
        &t.clock,
        &t.dev,
        ApprovalRequest { perm: Perm::VOID_ORDER, target_ref: &second.order_id, approver_id: SUP, pin: SUP_PIN },
    )
    .await
    .unwrap();
    input.approval_id = Some(grant.approval_id);
    let done = void(&t.auth, &t.clock, &t.t.db, &ctx, &input).await.unwrap();
    assert_eq!((done.cash_returned, done.grand_total), (10_000, 10_000));
    assert_eq!(unreversed(&t).await, 0, "untracked line: nothing to reverse, nothing left over");

    // The till's void list shows the session's orders with their status.
    let list = voids::session_orders(t.pool(), &t.s.device_id).await.unwrap();
    let statuses: Vec<(&str, &str)> = list.iter().map(|o| (o.receipt_no.as_str(), o.status.as_str())).collect();
    assert_eq!(statuses, vec![("T01-261108002", "voided"), ("T01-261108001", "completed")]);
}
