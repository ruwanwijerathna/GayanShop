//! Item-for-item exchanges (decision #26): returned goods are credited at what the customer paid, go straight
//! back into the cost layers they were sold from, the customer pays only the difference, nobody is ever paid
//! out, and an exchange after the 7-day window needs a supervisor / owner (`EXCHANGE_LATE`).

mod common;

use common::{CASHIER, CASHIER_PIN, SUP, SUP_PIN, Till, cart, cash, line, quick, till};
use gayanshop_lib::auth::{ApprovalRequest, request_approval};
use gayanshop_lib::commands::checkout::sell;
use gayanshop_lib::db::repo::cash as cash_repo;
use gayanshop_lib::db::repo::checkout::{self, CheckoutInput, CheckoutResult, NoFaults};
use gayanshop_lib::db::repo::exchange::{self, ExchangeInput, ExchangeReturn};
use gayanshop_lib::db::repo::reports;
use gayanshop_lib::db::settings;
use gayanshop_lib::domain::permissions::Perm;
use gayanshop_lib::error::AppError;

const SOLD_ON: &str = "2026-11-08T09:00:00.000Z";
const DAY_7: &str = "2026-11-15T09:00:00.000Z";
const DAY_8: &str = "2026-11-16T09:00:00.000Z";

fn exchanging(mut input: CheckoutInput, returns: &[(&str, i64)]) -> CheckoutInput {
    input.exchange = Some(ExchangeInput {
        returns: returns.iter().map(|(id, qty)| ExchangeReturn { item_id: (*id).into(), qty: *qty }).collect(),
    });
    input
}

async fn sell_at(
    t: &Till,
    staff: &str,
    input: &CheckoutInput,
    approver: Option<&str>,
    now: &str,
) -> Result<CheckoutResult, AppError> {
    let ctx = common::ctx_at(&t.s, now);
    checkout::checkout(&t.t.db, &ctx, &t.seller(staff).await, input, approver, &NoFaults).await
}

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

async fn first_item(t: &Till, order_id: &str) -> String {
    sqlx::query_scalar("SELECT id FROM order_items WHERE order_id = ?1 ORDER BY line_no LIMIT 1")
        .bind(order_id)
        .fetch_one(t.pool())
        .await
        .unwrap()
}

/// Every unit of a line in the order the sale slices were written: (layer, unit cost).
async fn units_sold(t: &Till, item_id: &str) -> Vec<(String, i64)> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT batch_item_id, unit_cost, -qty_delta FROM stock_movements
         WHERE order_item_id = ?1 AND reason = 'sale' ORDER BY rowid",
    )
    .bind(item_id)
    .fetch_all(t.pool())
    .await
    .unwrap();
    rows.into_iter().flat_map(|(layer, cost, qty)| std::iter::repeat_n((layer, cost), qty as usize)).collect()
}

/// Sell 3 buckets (layers of 2 @ 200 and 5 @ 220) with a 5% bill discount: the customer pays 1,282.50.
async fn bucket_sale(t: &Till) -> (String, String, String, CheckoutResult) {
    let bucket = t.stocked("Bucket", 45_000, &[(2, 20_000), (5, 22_000)]).await;
    let mug = t.stocked("Mug", 25_000, &[(10, 9_000)]).await;
    let mut input = cart("sale-1", vec![line(&bucket, 45_000, 3)], vec![cash(128_250, None)]);
    input.bill_discount = 6_750;
    let sold = sell_at(t, CASHIER, &input, None, SOLD_ON).await.unwrap();
    assert_eq!(sold.grand_total, 128_250);
    (bucket, mug, first_item(t, &sold.order_id).await, sold)
}

#[tokio::test]
async fn goods_come_back_at_what_was_paid_into_their_original_layers_and_the_customer_pays_the_difference() {
    let t = till().await;
    let (bucket, mug, item, sold) = bucket_sale(&t).await;
    let units = units_sold(&t, &item).await;

    // Return 2 of the 3 buckets for 4 mugs (1,000.00). Credit = 2/3 of what was PAID, 855.00 → pay 145.00.
    let input = exchanging(cart("ex-1", vec![line(&mug, 25_000, 4)], vec![cash(14_500, Some(20_000))]), &[(&item, 2)]);
    let r = sell_at(&t, CASHIER, &input, None, DAY_7).await.unwrap();
    assert_eq!((r.grand_total, r.exchange_credit, r.change_due), (100_000, 85_500, 5_500));

    let (credit, items, gross, discount, net, cost, paid_total): (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT exchange_credit, returned_items, returned_gross, returned_discount, returned_net, returned_cost, paid_total
         FROM orders WHERE id = ?1",
    )
    .bind(&r.order_id)
    .fetch_one(t.pool())
    .await
    .unwrap();
    let returned_cost = units[0].1 + units[1].1;
    assert_eq!((credit, items, gross, discount, net), (85_500, 2, 90_000, 4_500, 85_500));
    assert_eq!((cost, paid_total), (returned_cost, 100_000), "credit counts as paid: paid_total = grand_total");

    // The returned units went back as `return` slices into the layers they were sold from, at their cost.
    let slices: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT batch_item_id, unit_cost, qty_delta, revenue_net FROM stock_movements WHERE reason = 'return'",
    )
    .fetch_all(t.pool())
    .await
    .unwrap();
    assert_eq!(slices.iter().map(|s| s.2).sum::<i64>(), 2);
    assert_eq!(slices.iter().map(|s| s.3).sum::<i64>(), -85_500, "revenue reversed exactly");
    for (layer, unit_cost, ..) in &slices {
        assert!(units[..2].contains(&(layer.clone(), *unit_cost)), "a return slice mirrors a sold unit");
    }
    // Available for resale at once: 7 − 3 sold + 2 returned.
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{bucket}'")).await, 6);
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{mug}'")).await, 6);
    // Batch margins net out: only the one bucket still sold counts as sold.
    let (sold_qty, sold_rev, sold_cost): (i64, i64, i64) = sqlx::query_as(
        "SELECT SUM(sold_qty), SUM(sold_revenue), SUM(sold_cost) FROM batch_item_stock WHERE product_id = ?1",
    )
    .bind(&bucket)
    .fetch_one(t.pool())
    .await
    .unwrap();
    assert_eq!((sold_qty, sold_rev, sold_cost), (1, 42_750, units[2].1));

    // The ledger of the day: sales net of what came back, only real money in the payment mix.
    let (count, net_sales, cogs): (i64, i64, i64) =
        sqlx::query_as("SELECT SUM(sales_count), SUM(net_sales), SUM(cogs) FROM daily_sales_summary")
            .fetch_one(t.pool())
            .await
            .unwrap();
    let bucket_cost: i64 = units.iter().map(|u| u.1).sum();
    assert_eq!((count, net_sales), (2, 128_250 + 100_000 - 85_500));
    assert_eq!(cogs, bucket_cost + 4 * 9_000 - returned_cost);
    assert_eq!(t.count("SELECT SUM(amount) FROM daily_payment_summary").await, 128_250 + 14_500);

    // Sync: the returns and their slices are queued behind the exchange order (parents first).
    let order_seq = t
        .count(&format!("SELECT MAX(seq) FROM sync_outbox WHERE table_name = 'orders' AND row_id = '{}'", r.order_id))
        .await;
    assert_eq!(
        t.count(&format!(
            "SELECT COUNT(*) FROM sync_outbox q WHERE q.seq <= {order_seq} AND (q.table_name = 'order_returns'
               OR (q.table_name = 'stock_movements' AND q.row_id IN (SELECT id FROM stock_movements WHERE reason = 'return')))"
        ))
        .await,
        0,
        "no child is queued ahead of its order"
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'order_returns'").await, 1);
    assert_eq!(
        t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'EXCHANGE'").await,
        1,
        "every exchange is audited"
    );
    let meta: String = sqlx::query_scalar("SELECT metadata FROM audit_logs WHERE action = 'EXCHANGE'")
        .fetch_one(t.pool())
        .await
        .unwrap();
    assert!(meta.contains(&sold.receipt_no) && meta.contains("85500"), "{meta}");

    // What the till reports: the session took 1,427.50 (not the 1,855.00 of replacement + original), 1 exchange.
    let mut tx = t.t.db.write().await.unwrap();
    let z = cash_repo::z_report(tx.conn(), &t.session_id, SOLD_ON).await.unwrap();
    assert_eq!((z.sales_count, z.sales_total), (2, 128_250 + 14_500));
    assert_eq!((z.exchanges_count, z.exchange_credit, z.returned_items), (1, 85_500, 2));
    assert_eq!(z.net_sales, 128_250 + 100_000 - 85_500);
}

#[tokio::test]
async fn returning_a_line_in_pieces_credits_exactly_what_was_paid_and_never_more_than_was_sold() {
    let t = till().await;
    let (_, mug, item, _) = bucket_sale(&t).await;
    // 128,250 over 3 units: 42,750 each, so 1 + 2 returned over two exchanges credit the whole line.
    let first = exchanging(cart("ex-a", vec![line(&mug, 25_000, 2)], vec![cash(7_250, None)]), &[(&item, 1)]);
    assert_eq!(sell_at(&t, CASHIER, &first, None, SOLD_ON).await.unwrap().exchange_credit, 42_750);
    let second = exchanging(cart("ex-b", vec![line(&mug, 25_000, 4)], vec![cash(14_500, None)]), &[(&item, 2)]);
    assert_eq!(sell_at(&t, CASHIER, &second, None, SOLD_ON).await.unwrap().exchange_credit, 85_500);
    assert_eq!(t.count("SELECT SUM(exchange_credit) FROM orders").await, 128_250, "the whole line, to the cent");

    // Nothing left to return.
    let third = exchanging(cart("ex-c", vec![line(&mug, 25_000, 2)], vec![cash(7_250, None)]), &[(&item, 1)]);
    assert_eq!(code(sell_at(&t, CASHIER, &third, None, SOLD_ON).await.unwrap_err()), "RETURN_QTY_EXCEEDS");
    // …and the lookup says so.
    let src = exchange::lookup(t.pool(), &t.s.store_id, " t01-1 ", "2026-11-08").await.unwrap();
    assert_eq!((src.lines[0].returned_qty, src.lines[0].returnable_qty, src.lines[0].eligible), (3, 0, false));
    assert_eq!(t.count("SELECT COUNT(*) FROM v_integrity_over_returned").await, 0);
}

#[tokio::test]
async fn never_a_refund_the_replacement_must_be_worth_at_least_the_returned_goods() {
    let t = till().await;
    let (_, mug, item, _) = bucket_sale(&t).await;
    let before = t.count("SELECT COUNT(*) FROM orders").await;
    // 2 buckets = 855.00 back, but the replacement is only 1 mug (250.00).
    let short = exchanging(cart("ex-short", vec![line(&mug, 25_000, 1)], vec![]), &[(&item, 2)]);
    assert_eq!(code(sell_at(&t, CASHIER, &short, None, SOLD_ON).await.unwrap_err()), "EXCHANGE_REPLACEMENT_TOO_LOW");
    // Paying the whole replacement instead of the difference is a mismatch, not a quiet overpayment.
    let over = exchanging(cart("ex-over", vec![line(&mug, 25_000, 4)], vec![cash(100_000, None)]), &[(&item, 2)]);
    assert_eq!(code(sell_at(&t, CASHIER, &over, None, SOLD_ON).await.unwrap_err()), "PAYMENT_MISMATCH");
    assert_eq!(t.count("SELECT COUNT(*) FROM orders").await, before, "refused exchanges write nothing");
    assert_eq!(t.count("SELECT COUNT(*) FROM stock_movements WHERE reason = 'return'").await, 0);
    // An even swap costs nothing: one bucket back (427.50 paid) for a 427.50 pail.
    let pail = t.stocked("Pail", 42_750, &[(3, 10_000)]).await;
    let even = exchanging(cart("ex-even", vec![line(&pail, 42_750, 1)], vec![]), &[(&item, 1)]);
    let r = sell_at(&t, CASHIER, &even, None, SOLD_ON).await.unwrap();
    assert_eq!((r.grand_total, r.exchange_credit, r.change_due), (42_750, 42_750, 0));
    assert_eq!(t.count("SELECT COUNT(*) FROM order_payments WHERE order_id = (SELECT id FROM orders WHERE receipt_no = 'T01-261108002')").await, 0);
}

async fn second_cashier(t: &Till) {
    common::exec(
        t.pool(),
        &format!(
            "INSERT INTO staff (id, store_id, display_name, role_id, pin_hash, created_at, updated_at, updated_by_device, staff_no)
             SELECT 'cashier2', '{}', 'cashier2', id, 'x', '{}', '{}', '{}', 4 FROM roles WHERE name = 'Cashier'",
            t.s.store_id,
            common::NOW,
            common::NOW,
            t.s.device_id
        ),
    )
    .await;
}

#[tokio::test]
async fn after_seven_days_an_exchange_needs_a_supervisor_or_owner() {
    let t = till().await;
    second_cashier(&t).await;
    let (_, mug, item, _) = bucket_sale(&t).await;
    // One bucket back (427.50) for two mugs (500.00) each time: pay 72.50.
    let swap =
        |token: &str| exchanging(cart(token, vec![line(&mug, 25_000, 2)], vec![cash(7_250, None)]), &[(&item, 1)]);

    // Day 7 after the sale is still inside the window: any cashier.
    assert!(sell_at(&t, CASHIER, &swap("in-window"), None, DAY_7).await.is_ok());

    // Day 8 needs the override: asked for, never taken on trust.
    let late = swap("late-1");
    assert_eq!(
        sell_at(&t, CASHIER, &late, None, DAY_8).await.unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::EXCHANGE_LATE }
    );
    assert_eq!(code(sell_at(&t, CASHIER, &late, Some(CASHIER), DAY_8).await.unwrap_err()), "SELF_APPROVAL");
    assert_eq!(
        code(sell_at(&t, CASHIER, &late, Some("cashier2"), DAY_8).await.unwrap_err()),
        "APPROVER_LIMIT_TOO_LOW",
        "a cashier cannot lift the window"
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM orders WHERE exchange_credit > 0").await, 1, "refusals wrote nothing");

    // A supervisor's (or the owner's) PIN lifts it, and the order records who.
    let owner = t.s.owner_staff_id.clone();
    let approved = sell_at(&t, CASHIER, &late, Some(&owner), DAY_8).await.unwrap();
    let by: Option<String> = sqlx::query_scalar("SELECT late_approved_by FROM orders WHERE id = ?1")
        .bind(&approved.order_id)
        .fetch_one(t.pool())
        .await
        .unwrap();
    assert_eq!(by, Some(owner.clone()));
    let audit: (Option<String>, String) = sqlx::query_as(
        "SELECT approved_by, metadata FROM audit_logs WHERE action = 'EXCHANGE' ORDER BY device_seq DESC LIMIT 1",
    )
    .fetch_one(t.pool())
    .await
    .unwrap();
    assert_eq!(audit.0, Some(owner));
    assert!(audit.1.contains("\"late\":true"), "{}", audit.1);

    // A supervisor at the till holds EXCHANGE_LATE themselves.
    assert!(sell_at(&t, SUP, &swap("late-2"), None, DAY_8).await.is_ok());
}

#[tokio::test]
async fn the_window_is_a_store_setting_counted_from_the_sale_business_date() {
    let t = till().await;
    let (_, mug, item, _) = bucket_sale(&t).await;
    let swap =
        |token: &str| exchanging(cart(token, vec![line(&mug, 25_000, 2)], vec![cash(7_250, None)]), &[(&item, 1)]);
    let src = exchange::lookup(t.pool(), &t.s.store_id, "T01-261108001", "2026-11-15").await.unwrap();
    assert_eq!((src.days_since, src.window_days, src.late), (7, 7, false));
    let src = exchange::lookup(t.pool(), &t.s.store_id, "T01-261108001", "2026-11-16").await.unwrap();
    assert_eq!((src.days_since, src.late), (8, true));

    let mut tx = t.t.db.write().await.unwrap();
    settings::store_set(
        tx.conn(),
        &t.s.store_id,
        &t.s.device_id,
        DAY_8,
        "exchange.window_days",
        &serde_json::json!(30),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(sell_at(&t, CASHIER, &swap("ok-30"), None, DAY_8).await.is_ok(), "30-day window: day 8 is fine");
    let src = exchange::lookup(t.pool(), &t.s.store_id, "T01-261108001", "2026-11-16").await.unwrap();
    assert_eq!((src.window_days, src.late), (30, false));
}

#[tokio::test]
async fn the_command_flow_asks_a_supervisor_for_a_single_use_late_approval() {
    let t = till().await;
    let (_, mug, item, _) = bucket_sale(&t).await;
    t.login(CASHIER, CASHIER_PIN).await;
    let ctx = common::ctx_at(&t.s, DAY_8);
    let mut input = exchanging(cart("cmd-late", vec![line(&mug, 25_000, 2)], vec![cash(7_250, None)]), &[(&item, 1)]);
    assert_eq!(
        sell(&t.auth, &t.clock, &t.t.db, &ctx, &input).await.unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::EXCHANGE_LATE }
    );
    let grant = request_approval(
        &t.t.db,
        &t.auth,
        &t.clock,
        &t.dev,
        ApprovalRequest { perm: Perm::EXCHANGE_LATE, target_ref: "cmd-late", approver_id: SUP, pin: SUP_PIN },
    )
    .await
    .unwrap();
    input.approval_id = Some(grant.approval_id);
    let done = sell(&t.auth, &t.clock, &t.t.db, &ctx, &input).await.unwrap();
    assert_eq!(done.exchange_credit, 42_750);
}

#[tokio::test]
async fn only_stocked_catalog_goods_of_a_completed_sale_can_come_back() {
    let t = till().await;
    let tracked = t.stocked("Bucket", 45_000, &[(5, 20_000)]).await;
    let loose = t.product("Loose Tape", 10_000, false, 4_000, None).await;
    let mut input = cart(
        "mixed",
        vec![line(&tracked, 45_000, 1), line(&loose, 10_000, 1), quick("Odd Thing", 5_000, 1)],
        vec![cash(60_000, None)],
    );
    input.customer_phone = None;
    let sold = sell_at(&t, CASHIER, &input, None, SOLD_ON).await.unwrap();
    let items: Vec<(String, String)> =
        sqlx::query_as("SELECT name, id FROM order_items WHERE order_id = ?1 ORDER BY line_no")
            .bind(&sold.order_id)
            .fetch_all(t.pool())
            .await
            .unwrap();
    let id_of = |n: &str| items.iter().find(|i| i.0 == n).map(|i| i.1.clone()).unwrap();
    let replacement = |token: &str| cart(token, vec![line(&tracked, 45_000, 2)], vec![]);

    for name in ["Loose Tape", "Odd Thing"] {
        let r = exchanging(replacement("x"), &[(&id_of(name), 1)]);
        assert_eq!(code(sell_at(&t, CASHIER, &r, None, SOLD_ON).await.unwrap_err()), "RETURN_NOT_STOCKED", "{name}");
    }
    let src = exchange::lookup(t.pool(), &t.s.store_id, &sold.receipt_no, "2026-11-08").await.unwrap();
    assert_eq!(
        src.lines.iter().map(|l| (l.name.as_str(), l.eligible)).collect::<Vec<_>>(),
        [("Bucket", true), ("Loose Tape", false), ("Odd Thing", false)]
    );

    let bucket = id_of("Bucket");
    for (returns, expected) in [
        (vec![("no-such-line", 1)], "RETURN_ITEM_UNKNOWN"),
        (vec![(bucket.as_str(), 0)], "RETURN_INVALID"),
        (vec![(bucket.as_str(), 1), (bucket.as_str(), 1)], "RETURN_INVALID"),
        (vec![(bucket.as_str(), 2)], "RETURN_QTY_EXCEEDS"),
        (vec![], "RETURN_INVALID"),
    ] {
        let r = exchanging(replacement("y"), &returns);
        assert_eq!(code(sell_at(&t, CASHIER, &r, None, SOLD_ON).await.unwrap_err()), expected);
    }
    assert!(exchange::lookup(t.pool(), &t.s.store_id, "T99-nope", "2026-11-08").await.is_err());

    // A voided sale is gone: its goods cannot be exchanged.
    let voided =
        sell_at(&t, CASHIER, &cart("v", vec![line(&tracked, 45_000, 1)], vec![cash(45_000, None)]), None, SOLD_ON)
            .await
            .unwrap();
    let vitem = first_item(&t, &voided.order_id).await;
    let s = t.seller(SUP).await;
    gayanshop_lib::db::repo::void::void_order(
        &t.t.db,
        &t.ctx(),
        &gayanshop_lib::db::repo::void::Voider { staff_id: s.staff_id, perms: s.perms },
        &gayanshop_lib::db::repo::void::VoidInput {
            order_id: voided.order_id.clone(),
            reason: gayanshop_lib::db::repo::void::VoidReason::WrongItem,
            note: None,
            approval_id: None,
        },
        None,
        &NoFaults,
    )
    .await
    .unwrap();
    let r = exchanging(replacement("z"), &[(&vitem, 1)]);
    assert_eq!(code(sell_at(&t, CASHIER, &r, None, SOLD_ON).await.unwrap_err()), "RETURN_ORDER_VOIDED");
    assert_eq!(
        code(exchange::lookup(t.pool(), &t.s.store_id, &voided.receipt_no, "2026-11-08").await.unwrap_err()),
        "RETURN_ORDER_VOIDED"
    );
}

#[tokio::test]
async fn an_exchange_and_the_sale_it_returned_goods_from_can_no_longer_be_voided() {
    let t = till().await;
    let (_, mug, item, sold) = bucket_sale(&t).await;
    let exch = sell_at(
        &t,
        CASHIER,
        &exchanging(cart("ex", vec![line(&mug, 25_000, 2)], vec![cash(7_250, None)]), &[(&item, 1)]),
        None,
        SOLD_ON,
    )
    .await
    .unwrap();
    let s = t.seller(SUP).await;
    for order_id in [&sold.order_id, &exch.order_id] {
        let err = gayanshop_lib::db::repo::void::void_order(
            &t.t.db,
            &t.ctx(),
            &gayanshop_lib::db::repo::void::Voider { staff_id: s.staff_id.clone(), perms: s.perms },
            &gayanshop_lib::db::repo::void::VoidInput {
                order_id: order_id.clone(),
                reason: gayanshop_lib::db::repo::void::VoidReason::WrongItem,
                note: None,
                approval_id: None,
            },
            None,
            &NoFaults,
        )
        .await
        .unwrap_err();
        assert_eq!(code(err), "VOID_EXCHANGE");
    }
    // Defence in depth: the SQL trigger says the same to a hand-written UPDATE.
    for order_id in [&sold.order_id, &exch.order_id] {
        common::rejects(
            t.pool(),
            &[&format!(
                "UPDATE orders SET status = 'voided', voided_at = 'x', voided_by = '{SUP}', void_reason = 'WRONG_ITEM' WHERE id = '{order_id}'"
            )],
            "cannot be voided",
        )
        .await;
    }
}

#[tokio::test]
async fn returns_are_immutable_and_only_exist_inside_their_own_checkout() {
    let t = till().await;
    let (_, mug, item, _) = bucket_sale(&t).await;
    let exch = sell_at(
        &t,
        CASHIER,
        &exchanging(cart("ex", vec![line(&mug, 25_000, 2)], vec![cash(7_250, None)]), &[(&item, 1)]),
        None,
        SOLD_ON,
    )
    .await
    .unwrap();
    let p = t.pool();
    common::rejects(p, &["UPDATE order_returns SET qty = 2"], "immutable").await;
    common::rejects(p, &["DELETE FROM order_returns"], "immutable").await;
    common::rejects(
        p,
        &[&format!(
            "INSERT INTO order_returns (id, store_id, device_id, order_id, source_item_id, qty, line_gross, line_discount, line_total, line_net, tax_amount, cost_total, created_at)
             VALUES ('late', 's', 'd', '{}', '{item}', 1, 0, 0, 0, 0, 0, 0, 'x')",
            exch.order_id
        )],
        "cannot add returns to a sealed order",
    )
    .await;
    common::rejects(
        p,
        &[&format!(
            "INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, ref_type, ref_id, order_item_id, unit_cost, revenue_net, business_date, created_at)
             SELECT 'rogue', store_id, device_id, product_id, batch_item_id, 1, 'return', 'order', '{}', order_item_id, unit_cost, -1, business_date, created_at
             FROM stock_movements WHERE reason = 'return' LIMIT 1",
            exch.order_id
        )],
        "return slices only inside a pending exchange",
    )
    .await;
    common::rejects(
        p,
        &["UPDATE orders SET exchange_credit = 0 WHERE exchange_credit > 0"],
        "illegal state transition",
    )
    .await;
}

async fn snapshot(t: &Till) -> Vec<String> {
    let mut out = Vec::new();
    for sql in [
        "SELECT 'dss', business_date, device_id, sales_count, void_count, gross_sales, discounts, net_sales, tax_total, cogs,
                customer_orders FROM daily_sales_summary ORDER BY business_date, device_id",
        "SELECT 'dps', business_date, device_id, method, amount, txn_count FROM daily_payment_summary ORDER BY 2, 3, 4",
        "SELECT 'cs', customer_id, visit_count, total_spend, total_net FROM customer_stats ORDER BY customer_id",
        "SELECT 'sl', product_id, on_hand FROM stock_levels ORDER BY product_id",
        "SELECT 'bis', batch_item_id, on_hand, received_qty, sold_qty, sold_revenue, sold_cost
         FROM batch_item_stock ORDER BY batch_item_id",
    ] {
        let rows = sqlx::query(sql).fetch_all(t.pool()).await.unwrap();
        for r in rows {
            use sqlx::{Row, ValueRef};
            let cells: Vec<String> = (0..r.columns().len())
                .map(|i| match r.try_get_raw(i) {
                    Ok(v) if v.is_null() => "∅".to_owned(),
                    _ => r
                        .try_get::<i64, _>(i)
                        .map(|n| n.to_string())
                        .or_else(|_| r.try_get::<String, _>(i))
                        .unwrap_or_default(),
                })
                .collect();
            out.push(cells.join("|"));
        }
    }
    out
}

#[tokio::test]
async fn the_rollups_a_rebuild_computes_from_the_base_rows_equal_the_ones_the_triggers_kept() {
    let t = till().await;
    let (_, mug, item, _) = bucket_sale(&t).await;
    // A customer-attached exchange, a partial return, and an ordinary sale after it.
    let mut a = exchanging(cart("ra", vec![line(&mug, 25_000, 4)], vec![cash(14_500, None)]), &[(&item, 2)]);
    a.customer_phone = Some("0771234567".into());
    sell_at(&t, CASHIER, &a, None, DAY_7).await.unwrap();
    let b = exchanging(cart("rb", vec![line(&mug, 25_000, 2)], vec![cash(7_250, None)]), &[(&item, 1)]);
    sell_at(&t, SUP, &b, None, DAY_8).await.unwrap();
    sell_at(&t, CASHIER, &cart("rc", vec![line(&mug, 25_000, 1)], vec![cash(25_000, None)]), None, DAY_8)
        .await
        .unwrap();

    let kept = snapshot(&t).await;
    assert!(kept.iter().any(|r| r.starts_with("cs|")), "the customer is counted");
    let mut tx = t.t.db.write().await.unwrap();
    reports::recompute(tx.conn()).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(snapshot(&t).await, kept);
    assert_eq!(t.count("SELECT COUNT(*) FROM v_integrity_over_returned").await, 0);
    assert_eq!(t.count("SELECT COUNT(*) FROM v_integrity_void_unreversed").await, 0);
}

#[tokio::test]
async fn typing_part_of_a_bill_number_suggests_the_completed_bills_that_contain_it() {
    let t = till().await;
    let (_, mug, _, first) = bucket_sale(&t).await;
    let second =
        sell_at(&t, CASHIER, &cart("sale-2", vec![line(&mug, 25_000, 1)], vec![cash(25_000, None)]), None, SOLD_ON)
            .await
            .unwrap();
    let (n1, n2) = (first.receipt_no, second.receipt_no);
    assert_eq!((n1.as_str(), n2.as_str()), ("T01-261108001", "T01-261108002"));
    let found = |typed: &'static str| {
        let (pool, store) = (t.pool().clone(), t.s.store_id.clone());
        async move {
            let rows = exchange::suggest(&pool, &store, typed).await.unwrap();
            rows.into_iter().map(|b| b.receipt_no).collect::<Vec<_>>()
        }
    };

    assert_eq!(found("t01 2611").await, [n2.clone(), n1.clone()], "newest first");
    assert_eq!(found("261108-002").await, std::slice::from_ref(&n2), "any part of the number, separators ignored");
    assert_eq!(found("8001").await, std::slice::from_ref(&n1));
    assert!(found("T").await.is_empty(), "too little typed");
    assert!(found("999999").await.is_empty(), "nothing contains it");

    // Voided bills can't be exchanged, so they are not offered.
    let s = t.seller(SUP).await;
    gayanshop_lib::db::repo::void::void_order(
        &t.t.db,
        &t.ctx(),
        &gayanshop_lib::db::repo::void::Voider { staff_id: s.staff_id, perms: s.perms },
        &gayanshop_lib::db::repo::void::VoidInput {
            order_id: second.order_id.clone(),
            reason: gayanshop_lib::db::repo::void::VoidReason::WrongItem,
            note: None,
            approval_id: None,
        },
        None,
        &NoFaults,
    )
    .await
    .unwrap();
    assert_eq!(found("t01 2611").await, [n1]);
}
