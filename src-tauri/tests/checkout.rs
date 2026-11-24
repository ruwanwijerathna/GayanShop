//! Block 1.4 — `checkout_order`: one sealed transaction (doc §8 Phase 1 step order), idempotency, FIFO
//! slices, quick sales, customers, discount limits and approvals, price overrides, payments, and the
//! command-level supervisor approval flow.

mod common;

use common::{CASHIER, SUP, SUP_PIN, Till, cart, cash, line, quick, till};
use gayanshop_lib::auth::{ApprovalRequest, request_approval};
use gayanshop_lib::commands::checkout::sell;
use gayanshop_lib::db::repo::checkout::{self, CheckoutInput, CheckoutResult, NoFaults, PaymentLine};
use gayanshop_lib::db::repo::customers::customer_id;
use gayanshop_lib::domain::payments::PaymentMethod;
use gayanshop_lib::domain::permissions::Perm;
use gayanshop_lib::error::AppError;

async fn sell_as(
    t: &Till,
    staff: &str,
    input: &CheckoutInput,
    approver: Option<&str>,
) -> Result<CheckoutResult, AppError> {
    checkout::checkout(&t.t.db, &t.ctx(), &t.seller(staff).await, input, approver, &NoFaults).await
}

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

async fn nothing_written(t: &Till) {
    for table in ["orders", "order_items", "order_payments", "receipt_counters"] {
        assert_eq!(t.count(&format!("SELECT COUNT(*) FROM {table}")).await, 0, "{table}");
    }
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE kind IN ('receipt', 'drawer_kick')").await, 0);
    assert_eq!(t.count("SELECT COUNT(*) FROM stock_movements WHERE reason = 'sale'").await, 0);
}

#[tokio::test]
async fn a_cash_sale_is_one_sealed_transaction() {
    let t = till().await;
    let bucket = t.stocked("Blue Bucket", 45_000, &[(2, 20_000), (5, 22_000)]).await;
    let mug = t.product("Mug", 25_000, false, 9_000, None).await; // not stock-tracked, known cost
    exec_held_cart(&t).await;

    let mut input = cart(
        "tok-1",
        vec![line(&bucket, 45_000, 3), line(&mug, 25_000, 1), quick("Plastic  Basin 10L", 30_000, 2)],
        vec![cash(220_000, Some(250_000))],
    );
    input.note = Some("gift".into());
    let r = sell_as(&t, CASHIER, &input, None).await.unwrap();
    assert_eq!(
        (r.receipt_no.as_str(), r.grand_total, r.change_due, r.replayed),
        ("T01-261108001", 220_000, 30_000, false)
    );

    // Header = Σ lines; the SQL seal accepted it, so every identity in §13.5 holds.
    let (status, lines, items, net, cost, quick_net, unknown): (String, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT status, line_count, item_count, net_sales, cost_total, quick_sale_net, unknown_cost_net FROM orders",
    )
    .fetch_one(t.pool())
    .await
    .unwrap();
    // FIFO: 2 units at Rs 200 from the older layer, 1 at Rs 220 from the newer; mug at its default cost.
    assert_eq!((status.as_str(), lines, items, net, cost), ("completed", 3, 6, 220_000, 62_000 + 9_000));
    assert_eq!((quick_net, unknown), (60_000, 60_000), "quick sale without cost = unknown-cost revenue");
    let slices: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT qty_delta, unit_cost, revenue_net FROM stock_movements WHERE reason = 'sale' ORDER BY unit_cost",
    )
    .fetch_all(t.pool())
    .await
    .unwrap();
    assert_eq!(slices, vec![(-2, 20_000, 90_000), (-1, 22_000, 45_000)]);
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{bucket}'")).await, 4);
    let key: String =
        sqlx::query_scalar("SELECT custom_key FROM order_items WHERE is_custom = 1").fetch_one(t.pool()).await.unwrap();
    assert_eq!(key, "plastic basin 10l");

    // Steps 8–10: the crash-recovery cart is gone (named holds stay), receipt + drawer queued, outbox.
    assert_eq!(t.count("SELECT COUNT(*) FROM held_carts WHERE id = '__current__'").await, 0);
    assert_eq!(t.count("SELECT COUNT(*) FROM held_carts WHERE id = 'h1'").await, 1);
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE kind IN ('receipt','drawer_kick')").await, 2);
    for (table, n) in [("orders", 1), ("order_items", 3), ("order_payments", 1)] {
        assert_eq!(
            t.count(&format!("SELECT COUNT(*) FROM sync_outbox WHERE table_name = '{table}'")).await,
            n,
            "{table}"
        );
    }
    let outbox_status: String =
        sqlx::query_scalar("SELECT json_extract(payload, '$.status') FROM sync_outbox WHERE table_name = 'orders'")
            .fetch_one(t.pool())
            .await
            .unwrap();
    assert_eq!(outbox_status, "completed", "the synced snapshot is the sealed order");
    // Derived rollups were maintained by the seal.
    assert_eq!(t.count("SELECT sales_count FROM daily_sales_summary").await, 1);
    assert_eq!(t.count("SELECT amount FROM daily_payment_summary WHERE method = 'cash'").await, 220_000);

    // Receipt numbers count up per business day.
    let r2 = sell_as(&t, CASHIER, &cart("tok-2", vec![line(&mug, 25_000, 1)], vec![cash(25_000, None)]), None)
        .await
        .unwrap();
    assert_eq!(r2.receipt_no, "T01-261108002");
}

async fn exec_held_cart(t: &Till) {
    common::exec(
        t.pool(),
        "INSERT INTO held_carts (id, payload, created_at, updated_at) VALUES ('__current__', '{}', 'x', 'x'), ('h1', '{}', 'x', 'x')",
    )
    .await;
    assert_eq!(t.count("SELECT COUNT(*) FROM held_carts").await, 2);
}

#[tokio::test]
async fn the_same_token_returns_the_original_sale() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let input = cart("tok-dup", vec![line(&mug, 25_000, 1)], vec![cash(25_000, None)]);
    let first = sell_as(&t, CASHIER, &input, None).await.unwrap();
    let audit_before = t.count("SELECT COUNT(*) FROM audit_logs").await;
    // Even a different cart under the same token replays the original: nothing new is written.
    let changed = cart("tok-dup", vec![line(&mug, 25_000, 4)], vec![cash(100_000, None)]);
    let again = sell_as(&t, CASHIER, &changed, None).await.unwrap();
    assert!(again.replayed);
    assert_eq!(
        (again.order_id.as_str(), again.receipt_no.as_str()),
        (first.order_id.as_str(), first.receipt_no.as_str())
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM orders").await, 1);
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs").await, 2);
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs").await, audit_before);
}

#[tokio::test]
async fn customers_are_linked_by_phone_without_storing_it_on_the_order() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let mut input = cart("c1", vec![line(&mug, 25_000, 1)], vec![cash(25_000, None)]);
    input.customer_phone = Some("077 123 4567".into());
    input.customer_name = Some("Saman".into());
    let r = sell_as(&t, CASHIER, &input, None).await.unwrap();
    let expected = customer_id(&t.s.store_id, "+94771234567");
    assert_eq!(r.customer_id.as_deref(), Some(expected.as_str()), "UUIDv5 of the store and E.164 phone");
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'CUSTOMER_CREATE'").await, 1);
    assert_eq!(
        t.count("SELECT COUNT(*) FROM audit_logs WHERE metadata LIKE '%771234567%'").await,
        0,
        "no phone in audit"
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM orders WHERE note LIKE '%771234567%'").await, 0);

    // Same number in another format: same customer, no new row or audit; visit count rises.
    let mut again = cart("c2", vec![line(&mug, 25_000, 1)], vec![cash(25_000, None)]);
    again.customer_phone = Some("+94771234567".into());
    assert_eq!(sell_as(&t, CASHIER, &again, None).await.unwrap().customer_id, r.customer_id);
    assert_eq!(t.count("SELECT COUNT(*) FROM customers").await, 1);
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'CUSTOMER_CREATE'").await, 1);
    assert_eq!(t.count("SELECT visit_count FROM customer_stats").await, 2);

    // An erased customer who returns is re-activated under the same id.
    common::exec(t.pool(), "UPDATE customers SET phone_e164 = NULL, display_name = NULL, erased_at = 'x'").await;
    let mut back = cart("c3", vec![line(&mug, 25_000, 1)], vec![cash(25_000, None)]);
    back.customer_phone = Some("0771234567".into());
    assert_eq!(sell_as(&t, CASHIER, &back, None).await.unwrap().customer_id, r.customer_id);
    assert_eq!(
        t.count("SELECT COUNT(*) FROM customers WHERE erased_at IS NULL AND phone_e164 = '+94771234567'").await,
        1
    );

    let mut bad = cart("c4", vec![line(&mug, 25_000, 1)], vec![cash(25_000, None)]);
    bad.customer_phone = Some("12345".into());
    assert_eq!(code(sell_as(&t, CASHIER, &bad, None).await.unwrap_err()), "PHONE_INVALID");
}

#[tokio::test]
async fn cashier_discounts_up_to_five_percent_and_shares_sum_exactly() {
    let t = till().await;
    let a = t.product("A", 10_001, false, 0, None).await;
    let b = t.product("B", 20_003, false, 0, None).await;
    let c = t.product("C", 30_007, false, 0, None).await;
    // Bill discount 5% of Rs 600.11 = Rs 30.00 (rounded down to stay within every line's limit).
    let mut input =
        cart("d1", vec![line(&a, 10_001, 1), line(&b, 20_003, 1), line(&c, 30_007, 1)], vec![cash(57_011, None)]);
    input.bill_discount = 3_000;
    sell_as(&t, CASHIER, &input, None).await.unwrap();
    let shares: Vec<i64> = sqlx::query_scalar("SELECT order_discount_share FROM order_items ORDER BY line_no")
        .fetch_all(t.pool())
        .await
        .unwrap();
    assert_eq!(shares.iter().sum::<i64>(), 3_000, "largest remainder: Σ shares = bill discount");
    assert_eq!(t.count("SELECT discount_total FROM orders").await, 3_000);

    // Item discount of exactly 5% is fine; 6% needs a supervisor.
    let a = t.product("Hundred", 10_000, false, 0, None).await;
    let mut five = cart("d2", vec![line(&a, 10_000, 1)], vec![cash(9_500, None)]);
    five.lines[0].line_discount = 500;
    sell_as(&t, CASHIER, &five, None).await.unwrap();
    let mut six = cart("d3", vec![line(&a, 10_000, 1)], vec![cash(9_400, None)]);
    six.lines[0].line_discount = 600;
    assert_eq!(
        sell_as(&t, CASHIER, &six, None).await.unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::LINE_DISCOUNT }
    );
    let mut six_bill = cart("d4", vec![line(&a, 10_000, 1)], vec![cash(9_400, None)]);
    six_bill.bill_discount = 600;
    assert_eq!(
        sell_as(&t, CASHIER, &six_bill, None).await.unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::ORDER_DISCOUNT }
    );

    // With the supervisor (limit 20%) it goes through, audited, with the approver on the order.
    let r = sell_as(&t, CASHIER, &six_bill, Some(SUP)).await.unwrap();
    let approved: Option<String> = sqlx::query_scalar("SELECT approved_by FROM orders WHERE id = ?1")
        .bind(&r.order_id)
        .fetch_one(t.pool())
        .await
        .unwrap();
    assert_eq!(approved.as_deref(), Some(SUP));
    assert_eq!(
        t.count(&format!("SELECT COUNT(*) FROM audit_logs WHERE action = 'ORDER_DISCOUNT' AND approved_by = '{SUP}'"))
            .await,
        1
    );

    // 30% is beyond the supervisor too; the cashier can't approve themselves.
    let mut thirty = cart("d5", vec![line(&a, 10_000, 1)], vec![cash(7_000, None)]);
    thirty.bill_discount = 3_000;
    assert_eq!(code(sell_as(&t, CASHIER, &thirty, Some(SUP)).await.unwrap_err()), "APPROVER_LIMIT_TOO_LOW");
    let self_approved = CheckoutInput { checkout_token: "d7".into(), ..six_bill.clone() };
    assert_eq!(code(sell_as(&t, CASHIER, &self_approved, Some(CASHIER)).await.unwrap_err()), "SELF_APPROVAL");
    // A discount larger than the line is invalid outright.
    let mut huge = cart("d6", vec![line(&a, 10_000, 1)], vec![cash(0, None)]);
    huge.lines[0].line_discount = 10_001;
    assert_eq!(code(sell_as(&t, SUP, &huge, None).await.unwrap_err()), "LINE_DISCOUNT_INVALID");
}

#[tokio::test]
async fn price_overrides_need_authority_and_the_floor_always_needs_approval() {
    let t = till().await;
    let shoe = t.stocked("Shoe", 150_000, &[(3, 80_000)]).await;
    let mut over = cart("o1", vec![line(&shoe, 140_000, 1)], vec![cash(140_000, None)]);
    over.lines[0].override_reason = Some("Damaged box".into());
    assert_eq!(
        sell_as(&t, CASHIER, &over, None).await.unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::PRICE_OVERRIDE }
    );
    sell_as(&t, CASHIER, &over, Some(SUP)).await.unwrap();
    let (overridden, approver): (bool, Option<String>) =
        sqlx::query_as("SELECT price_overridden, approved_by FROM order_items").fetch_one(t.pool()).await.unwrap();
    assert!(overridden);
    assert_eq!(approver.as_deref(), Some(SUP));
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'PRICE_OVERRIDE'").await, 1);
    // The supervisor may override on their own within 20%…
    sell_as(&t, SUP, &cart("o2", vec![line(&shoe, 125_000, 1)], vec![cash(125_000, None)]), None).await.unwrap();
    // …but not 30%.
    let deep = cart("o3", vec![line(&shoe, 105_000, 1)], vec![cash(105_000, None)]);
    assert_eq!(
        sell_as(&t, SUP, &deep, None).await.unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::PRICE_OVERRIDE }
    );

    // Below min_price: even within the seller's own limit, someone else must approve.
    let bag = t.product("Bag", 100_000, false, 0, Some(95_000)).await;
    let floor = cart("o4", vec![line(&bag, 90_000, 1)], vec![cash(90_000, None)]);
    assert_eq!(
        sell_as(&t, SUP, &floor, None).await.unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::PRICE_OVERRIDE }
    );
    sell_as(&t, SUP, &floor, Some(&t.s.owner_staff_id)).await.unwrap();
}

#[tokio::test]
async fn payments_are_never_negative_and_must_match_exactly() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let pay = |method, amount, tendered| PaymentLine { method, amount, tendered, reference: None };
    let cases = [
        (vec![cash(26_000, None), cash(-1_000, None)], "PAYMENT_NEGATIVE"),
        (vec![cash(24_999, None)], "PAYMENT_MISMATCH"),
        (vec![pay(PaymentMethod::Card, 25_000, Some(30_000))], "PAYMENT_INVALID"),
        (vec![cash(25_000, Some(20_000))], "PAYMENT_INVALID"),
    ];
    for (i, (payments, expected)) in cases.into_iter().enumerate() {
        let input = cart(&format!("p{i}"), vec![line(&mug, 25_000, 1)], payments);
        assert_eq!(code(sell_as(&t, CASHIER, &input, None).await.unwrap_err()), expected);
    }
    nothing_written(&t).await;

    // Split: card + cash, change only from the cash tendered.
    let split = cart(
        "split",
        vec![line(&mug, 25_000, 2)],
        vec![pay(PaymentMethod::Card, 30_000, None), cash(20_000, Some(50_000))],
    );
    let r = sell_as(&t, CASHIER, &split, None).await.unwrap();
    assert_eq!((r.grand_total, r.change_due), (50_000, 30_000));

    // The SQL CHECK is the second line of defence (sync-apply mode skips the policy triggers, not CHECKs).
    let mut tx = t.pool().begin().await.unwrap();
    sqlx::query("UPDATE sync_ctx SET applying = 1").execute(&mut *tx).await.unwrap();
    let raw = sqlx::query(
        "INSERT INTO order_payments (id, store_id, device_id, order_id, method, amount, created_at)
         VALUES ('neg', 's', 'd', ?1, 'cash', -1, 't')",
    )
    .bind(&r.order_id)
    .execute(&mut *tx)
    .await;
    assert!(raw.unwrap_err().to_string().contains("CHECK constraint failed"));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn refusals_write_nothing() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let gone = t.product("Old", 1_000, false, 0, None).await;
    common::exec(t.pool(), &format!("UPDATE products SET is_active = 0 WHERE id = '{gone}'")).await;
    let ok_pay = vec![cash(25_000, None)];
    assert_eq!(code(sell_as(&t, CASHIER, &cart("r1", vec![], vec![]), None).await.unwrap_err()), "CART_EMPTY");
    assert_eq!(
        code(
            sell_as(&t, CASHIER, &cart("r2", vec![line(&gone, 1_000, 1)], vec![cash(1_000, None)]), None)
                .await
                .unwrap_err()
        ),
        "PRODUCT_UNAVAILABLE"
    );
    assert_eq!(
        code(sell_as(&t, CASHIER, &cart("r3", vec![line(&mug, 25_000, 0)], ok_pay.clone()), None).await.unwrap_err()),
        "QTY_INVALID"
    );
    assert_eq!(
        code(sell_as(&t, CASHIER, &cart("r4", vec![quick("සබන්", 25_000, 1)], ok_pay.clone()), None).await.unwrap_err()),
        "QUICK_SALE_NAME_INVALID"
    );
    assert_eq!(
        code(sell_as(&t, CASHIER, &cart(" ", vec![line(&mug, 25_000, 1)], ok_pay.clone()), None).await.unwrap_err()),
        "CHECKOUT_TOKEN_INVALID"
    );
    // No open session: the seal would refuse, so Rust refuses first.
    gayanshop_lib::db::repo::cash::close(
        &t.t.db,
        &t.ctx(),
        &t.owner(),
        &gayanshop_lib::db::repo::cash::CloseInput { counted_cash: 0, denominations: None, note: None },
    )
    .await
    .unwrap();
    assert_eq!(
        code(sell_as(&t, CASHIER, &cart("r5", vec![line(&mug, 25_000, 1)], ok_pay), None).await.unwrap_err()),
        "NO_OPEN_SESSION"
    );
    nothing_written(&t).await;
}

#[tokio::test]
async fn two_lines_of_one_product_draw_fifo_in_order() {
    let t = till().await;
    let cup = t.stocked("Cup", 5_000, &[(2, 1_000), (5, 1_200)]).await;
    let input = cart("f1", vec![line(&cup, 5_000, 1), line(&cup, 5_000, 2)], vec![cash(15_000, None)]);
    sell_as(&t, CASHIER, &input, None).await.unwrap();
    let per_line: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT oi.line_no, m.unit_cost, -m.qty_delta FROM stock_movements m JOIN order_items oi ON oi.id = m.order_item_id
         ORDER BY oi.line_no, m.unit_cost",
    )
    .fetch_all(t.pool())
    .await
    .unwrap();
    assert_eq!(
        per_line,
        vec![(1, 1_000, 1), (2, 1_000, 1), (2, 1_200, 1)],
        "the second line sees the first line's units"
    );
}

#[tokio::test]
async fn command_flow_asks_for_a_single_use_approval_bound_to_the_cart() {
    let t = till().await;
    let mug = t.product("Mug", 10_000, false, 0, None).await;
    t.login(CASHIER, common::CASHIER_PIN).await;
    let mut input = cart("cmd-1", vec![line(&mug, 10_000, 1)], vec![cash(9_000, None)]);
    input.bill_discount = 1_000; // 10%: above the cashier's 5%
    let ctx = t.ctx();
    assert_eq!(
        sell(&t.auth, &t.clock, &t.t.db, &ctx, &input).await.unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::ORDER_DISCOUNT }
    );

    let grant = request_approval(
        &t.t.db,
        &t.auth,
        &t.clock,
        &t.dev,
        ApprovalRequest { perm: Perm::ORDER_DISCOUNT, target_ref: "cmd-1", approver_id: SUP, pin: SUP_PIN },
    )
    .await
    .unwrap();
    input.approval_id = Some(grant.approval_id.clone());
    let sold = sell(&t.auth, &t.clock, &t.t.db, &ctx, &input).await.unwrap();
    assert!(!sold.replayed);

    // The approval was consumed and bound to "cmd-1": another cart can't reuse it.
    let mut other = cart("cmd-2", vec![line(&mug, 10_000, 1)], vec![cash(9_000, None)]);
    other.bill_discount = 1_000;
    other.approval_id = Some(grant.approval_id);
    assert_eq!(sell(&t.auth, &t.clock, &t.t.db, &ctx, &other).await.unwrap_err(), AppError::InvalidApproval);
    // Logged out: nothing sells.
    gayanshop_lib::auth::logout(&t.t.db, &t.auth, &t.clock, &t.dev).await.unwrap();
    assert_eq!(
        sell(&t.auth, &t.clock, &t.t.db, &ctx, &cart("cmd-3", vec![line(&mug, 10_000, 1)], vec![cash(10_000, None)]))
            .await
            .unwrap_err(),
        AppError::NotLoggedIn
    );
}
