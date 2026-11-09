//! Block 2.1 — batch intake and lifecycle (doc §3.1): the three allocation methods with the live variance,
//! receive, reopen (only before anything moved), late items at cost 0, close (only when empty), and the batch
//! P&L ("did this batch make money?").

mod common;

use common::{CASHIER, Till, cart, cash, line, till};
use gayanshop_lib::db::repo::batches::{
    self, AllocationMethod, BatchDetail, BatchInput, BatchItemInput, BatchStatus, LateItemInput,
};
use gayanshop_lib::db::repo::checkout::{self, NoFaults};
use gayanshop_lib::error::AppError;

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

fn header(name: &str, total: i64, extra: i64, method: AllocationMethod) -> BatchInput {
    BatchInput {
        name: name.into(),
        supplier_name: Some("Pettah wholesale".into()),
        supplier_ref: None,
        purchased_at: None,
        total_cost: total,
        extra_costs: extra,
        allocation_method: Some(method),
        notes: None,
    }
}

fn item(product: &str, qty: i64, unit_cost: i64, retail: Option<i64>) -> BatchItemInput {
    BatchItemInput { product_id: product.into(), qty, unit_cost, retail_price: retail }
}

/// A draft with the given lines, ready to receive.
async fn draft(t: &Till, total: i64, extra: i64, method: AllocationMethod, lines: &[(&str, i64, i64, i64)]) -> String {
    let b = batches::create(&t.t.db, &t.ctx(), &t.owner(), &header("Shoe batch", total, extra, method)).await.unwrap();
    for (product, qty, cost, retail) in lines {
        batches::set_item(&t.t.db, &t.ctx(), &b.batch.id, &item(product, *qty, *cost, Some(*retail))).await.unwrap();
    }
    b.batch.id
}

async fn receive(t: &Till, id: &str) -> BatchDetail {
    batches::receive(&t.t.db, &t.ctx(), &t.owner(), id).await.unwrap()
}

fn costs(d: &BatchDetail) -> Vec<(i64, i64)> {
    d.items.iter().map(|i| (i.qty_received, i.unit_cost)).collect()
}

fn allocated(d: &BatchDetail) -> i64 {
    d.items.iter().map(|i| i.qty_received * i.unit_cost).sum()
}

async fn sell(t: &Till, token: &str, product: &str, price: i64, qty: i64) {
    let input = cart(token, vec![line(product, price, qty)], vec![cash(price * qty, None)]);
    checkout::checkout(&t.t.db, &t.ctx(), &t.seller(CASHIER).await, &input, None, &NoFaults).await.unwrap();
}

#[tokio::test]
async fn even_per_unit_splits_the_landed_cost_exactly_with_a_sub_unit_variance() {
    let t = till().await;
    let shoe = t.product("Shoe", 150_000, true, 0, None).await;
    let sandal = t.product("Sandal", 90_000, true, 0, None).await;
    // Rs 1,000.01 + Rs 0.00 extra over 10 units.
    let id =
        draft(&t, 100_001, 0, AllocationMethod::EvenPerUnit, &[(&shoe, 3, 0, 150_000), (&sandal, 7, 0, 90_000)]).await;
    let preview = batches::preview(t.pool(), &t.s.store_id, &id).await.unwrap();
    assert_eq!((preview.landed_cost, preview.problem.as_deref()), (100_001, None));
    assert!((0..10).contains(&preview.variance), "0 ≤ v < Σqty: {}", preview.variance);

    let d = receive(&t, &id).await;
    assert_eq!(d.batch.status, BatchStatus::Received);
    assert_eq!(d.batch.allocation_method, AllocationMethod::EvenPerUnit);
    assert_eq!(allocated(&d) + d.batch.allocation_variance, 100_001, "to the cent");
    assert_eq!(d.batch.allocation_variance, preview.variance, "the preview is what receive fixes");
    assert_eq!(costs(&d).iter().map(|c| c.1).collect::<Vec<_>>(), vec![10_000, 10_000]);
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{shoe}'")).await, 3);
    let audit = t
        .count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BATCH_RECEIVE' AND metadata LIKE '%even_per_unit%'")
        .await;
    assert_eq!(audit, 1);
}

#[tokio::test]
async fn by_retail_value_puts_more_cost_on_expensive_items() {
    let t = till().await;
    let watch = t.product("Watch", 300_000, true, 0, None).await;
    let strap = t.product("Strap", 30_000, true, 0, None).await;
    // Rs 5,000 invoice + Rs 333.33 transport over 2 watches and 10 straps.
    let id = draft(
        &t,
        500_000,
        33_333,
        AllocationMethod::ByRetailValue,
        &[(&watch, 2, 0, 300_000), (&strap, 10, 0, 30_000)],
    )
    .await;
    let d = receive(&t, &id).await;
    let by: std::collections::HashMap<String, i64> =
        d.items.iter().map(|i| (i.product_name.clone(), i.unit_cost)).collect();
    // Weights 600,000 : 300,000 ⇒ the watches carry 2/3 of 533,333.
    assert_eq!(by["Watch"] * 2 + by["Strap"] * 10 + d.batch.allocation_variance, 533_333);
    assert!(by["Watch"] > by["Strap"] * 9, "{by:?}");
    assert!((0..12).contains(&d.batch.allocation_variance));
    assert_eq!(d.batch.landed_cost, 533_333);
}

#[tokio::test]
async fn manual_costs_show_the_variance_against_the_invoice_live() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, true, 0, None).await;
    let id = draft(&t, 100_000, 5_000, AllocationMethod::Manual, &[(&mug, 10, 9_000, 25_000)]).await;
    let p = batches::preview(t.pool(), &t.s.store_id, &id).await.unwrap();
    assert_eq!((p.landed_cost, p.allocated, p.variance), (105_000, 90_000, 15_000));
    // The owner corrects the typed cost; the preview follows.
    batches::set_item(&t.t.db, &t.ctx(), &id, &item(&mug, 10, 11_000, Some(25_000))).await.unwrap();
    let p = batches::preview(t.pool(), &t.s.store_id, &id).await.unwrap();
    assert_eq!(p.variance, -5_000, "manual may over-allocate; it's shown, not hidden");
    let d = receive(&t, &id).await;
    assert_eq!((d.batch.allocation_variance, costs(&d)), (-5_000, vec![(10, 11_000)]));
}

#[tokio::test]
async fn the_preview_explains_why_receive_would_be_refused() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, true, 0, None).await;
    let empty = draft(&t, 1_000, 0, AllocationMethod::EvenPerUnit, &[]).await;
    assert_eq!(
        batches::preview(t.pool(), &t.s.store_id, &empty).await.unwrap().problem.as_deref(),
        Some("BATCH_EMPTY")
    );
    assert_eq!(code(batches::receive(&t.t.db, &t.ctx(), &t.owner(), &empty).await.unwrap_err()), "BATCH_EMPTY");

    let no_total = draft(&t, 0, 0, AllocationMethod::EvenPerUnit, &[(&mug, 5, 0, 25_000)]).await;
    let p = batches::preview(t.pool(), &t.s.store_id, &no_total).await.unwrap();
    assert_eq!(p.problem.as_deref(), Some("BATCH_TOTAL_REQUIRED"));
    assert_eq!(
        code(batches::receive(&t.t.db, &t.ctx(), &t.owner(), &no_total).await.unwrap_err()),
        "BATCH_TOTAL_REQUIRED"
    );

    let free = t.product("Free sample", 0, true, 0, None).await;
    let no_weight = draft(&t, 1_000, 0, AllocationMethod::ByRetailValue, &[(&free, 5, 0, 0)]).await;
    assert_eq!(code(batches::receive(&t.t.db, &t.ctx(), &t.owner(), &no_weight).await.unwrap_err()), "BATCH_NO_WEIGHT");
    assert_eq!(t.count("SELECT COUNT(*) FROM stock_movements").await, 0, "nothing received");
}

#[tokio::test]
async fn reopen_reverses_the_intake_only_while_nothing_else_has_moved() {
    let t = till().await;
    let shoe = t.product("Shoe", 150_000, true, 0, None).await;
    let id = draft(&t, 100_000, 0, AllocationMethod::EvenPerUnit, &[(&shoe, 10, 0, 150_000)]).await;
    receive(&t, &id).await;

    let d = batches::reopen(&t.t.db, &t.ctx(), &t.owner(), &id).await.unwrap();
    assert_eq!((d.batch.status, d.pnl.is_none()), (BatchStatus::Draft, true));
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{shoe}'")).await, 0);
    assert_eq!(
        t.count("SELECT COUNT(*) FROM stock_movements WHERE reason = 'batch_reopen' AND qty_delta = -10").await,
        1
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BATCH_REOPEN'").await, 1);

    // Fix the mistake (it was 8 pairs, at a dearer invoice) and receive again.
    batches::set_item(&t.t.db, &t.ctx(), &id, &item(&shoe, 8, 0, Some(150_000))).await.unwrap();
    let mut h = header("Shoe batch", 120_000, 0, AllocationMethod::EvenPerUnit);
    h.name = "Shoe batch (fixed)".into();
    batches::update(&t.t.db, &t.ctx(), &id, &h).await.unwrap();
    let d = receive(&t, &id).await;
    assert_eq!(costs(&d), vec![(8, 15_000)]);
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{shoe}'")).await, 8);
    assert_eq!(d.pnl.as_ref().map(|p| p.units_received), Some(8), "derived stock follows the reversal");

    // Once a unit sells, the costs are history: no more reopening.
    sell(&t, "s1", &shoe, 150_000, 1).await;
    let e = batches::reopen(&t.t.db, &t.ctx(), &t.owner(), &id).await.unwrap_err();
    assert_eq!(code(e), "BATCH_HAS_MOVEMENTS");
    let draft_id = draft(&t, 1_000, 0, AllocationMethod::EvenPerUnit, &[]).await;
    assert_eq!(
        code(batches::reopen(&t.t.db, &t.ctx(), &t.owner(), &draft_id).await.unwrap_err()),
        "BATCH_NOT_RECEIVED"
    );
    let opening = batches::opening_stock_id(t.pool(), &t.s.store_id).await.unwrap();
    let mug = t.product("Mug", 25_000, true, 0, None).await;
    batches::set_item(&t.t.db, &t.ctx(), &opening, &item(&mug, 2, 9_000, None)).await.unwrap();
    receive(&t, &opening).await;
    assert_eq!(code(batches::reopen(&t.t.db, &t.ctx(), &t.owner(), &opening).await.unwrap_err()), "BATCH_SYSTEM");
}

#[tokio::test]
async fn a_late_item_arrives_at_cost_zero_and_is_audited() {
    let t = till().await;
    let shoe = t.product("Shoe", 150_000, true, 0, None).await;
    let lace = t.product("Laces", 5_000, true, 0, None).await;
    let id = draft(&t, 100_000, 0, AllocationMethod::EvenPerUnit, &[(&shoe, 10, 0, 150_000)]).await;
    let late = LateItemInput { product_id: lace.clone(), qty: 20, retail_price: None };
    assert_eq!(
        code(batches::add_late_item(&t.t.db, &t.ctx(), &t.owner(), &id, &late).await.unwrap_err()),
        "BATCH_NOT_RECEIVED",
        "a draft just gets a normal item"
    );
    receive(&t, &id).await;

    let d = batches::add_late_item(&t.t.db, &t.ctx(), &t.owner(), &id, &late).await.unwrap();
    let laces = d.items.iter().find(|i| i.product_id == lace).unwrap();
    assert_eq!((laces.qty_received, laces.unit_cost, laces.added_after_receipt), (20, 0, true));
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{lace}'")).await, 20);
    assert_eq!(allocated(&d) + d.batch.allocation_variance, 100_000, "the batch cost didn't move");
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BATCH_LATE_ITEM'").await, 1);
    let again = LateItemInput { product_id: shoe.clone(), qty: 1, retail_price: None };
    assert_eq!(
        code(batches::add_late_item(&t.t.db, &t.ctx(), &t.owner(), &id, &again).await.unwrap_err()),
        "BATCH_ITEM_EXISTS"
    );

    // Reopened and re-received, the late item becomes an ordinary layer that shares the cost.
    batches::reopen(&t.t.db, &t.ctx(), &t.owner(), &id).await.unwrap();
    let d = receive(&t, &id).await;
    let laces = d.items.iter().find(|i| i.product_id == lace).unwrap();
    assert_eq!((laces.unit_cost > 0, laces.added_after_receipt), (true, false));
}

#[tokio::test]
async fn close_needs_an_empty_batch_and_freezes_it_with_its_pnl() {
    let t = till().await;
    let shoe = t.product("Shoe", 150_000, true, 0, None).await;
    let id = draft(&t, 200_000, 10_000, AllocationMethod::EvenPerUnit, &[(&shoe, 2, 0, 150_000)]).await;
    receive(&t, &id).await;
    assert_eq!(code(batches::close(&t.t.db, &t.ctx(), &t.owner(), &id).await.unwrap_err()), "BATCH_HAS_STOCK");

    sell(&t, "c1", &shoe, 150_000, 1).await;
    let mid = batches::detail(t.pool(), &t.s.store_id, &id).await.unwrap().pnl.unwrap();
    // Landed Rs 2,100 over 2 pairs = Rs 1,050 each; one sold at Rs 1,500.
    assert_eq!(
        (mid.landed_cost, mid.units_sold, mid.units_remaining, mid.revenue_net, mid.cogs),
        (210_000, 1, 1, 150_000, 105_000)
    );
    assert_eq!((mid.realised_profit, mid.payback), (45_000, -60_000), "profitable, not yet paid back");
    assert_eq!(
        (mid.remaining_value_cost, mid.remaining_value_retail, mid.projected_profit),
        (105_000, 150_000, 90_000)
    );
    assert_eq!(mid.sell_through_bp, 5_000);

    sell(&t, "c2", &shoe, 150_000, 1).await;
    let d = batches::close(&t.t.db, &t.ctx(), &t.owner(), &id).await.unwrap();
    let p = d.pnl.unwrap();
    assert_eq!(
        (d.batch.status, p.payback, p.realised_profit, p.sell_through_bp),
        (BatchStatus::Closed, 90_000, 90_000, 10_000)
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BATCH_CLOSE'").await, 1);

    // Read-only from now on.
    assert_eq!(
        code(batches::set_item(&t.t.db, &t.ctx(), &id, &item(&shoe, 3, 0, None)).await.unwrap_err()),
        "BATCH_NOT_DRAFT"
    );
    assert_eq!(code(batches::reopen(&t.t.db, &t.ctx(), &t.owner(), &id).await.unwrap_err()), "BATCH_NOT_RECEIVED");
    assert_eq!(code(batches::close(&t.t.db, &t.ctx(), &t.owner(), &id).await.unwrap_err()), "BATCH_NOT_RECEIVED");
    let late = LateItemInput { product_id: shoe.clone(), qty: 1, retail_price: None };
    assert_eq!(
        code(batches::add_late_item(&t.t.db, &t.ctx(), &t.owner(), &id, &late).await.unwrap_err()),
        "BATCH_NOT_RECEIVED"
    );
}

#[tokio::test]
async fn every_method_reconciles_to_the_cent_over_many_carts() {
    let t = till().await;
    let products: Vec<String> = {
        let mut v = Vec::new();
        for (i, price) in [9_999, 25_000, 131_313, 7].iter().enumerate() {
            v.push(t.product(&format!("P{i}"), *price, true, 0, None).await);
        }
        v
    };
    let mut seed: u64 = 0x5eed;
    let mut next = |m: u64| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % m
    };
    for round in 0..24 {
        let method = [AllocationMethod::EvenPerUnit, AllocationMethod::ByRetailValue][round % 2];
        let landed = i64::try_from(next(5_000_000) + 1).unwrap();
        let lines: Vec<(&str, i64, i64, i64)> = products
            .iter()
            .enumerate()
            .filter(|(i, _)| (round + i) % 3 != 0)
            .map(|(i, p)| (p.as_str(), i64::try_from(next(40) + 1).unwrap(), 0, [9_999, 25_000, 131_313, 7][i]))
            .collect();
        let id = draft(&t, landed, 0, method, &lines).await;
        let d = receive(&t, &id).await;
        let units: i64 = lines.iter().map(|l| l.1).sum();
        assert_eq!(allocated(&d) + d.batch.allocation_variance, landed, "round {round}");
        assert!((0..units).contains(&d.batch.allocation_variance), "round {round}: {}", d.batch.allocation_variance);
    }
}
