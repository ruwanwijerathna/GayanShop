//! Block 2.3 — stock operations (doc §3.1, §5): damage and adjustments (FIFO out, newest layer in), stock
//! counts producing `count_correction` movements, batch write-off before close, movement history, valuation
//! now and as of a date (by batch and by category), and oversold layers — all as ledger rows only.

mod common;

use common::{CASHIER, NOW, SUP, Till, cart, cash, ctx_at, line, till};
use gayanshop_lib::auth::guard::require;
use gayanshop_lib::db::repo::batches;
use gayanshop_lib::db::repo::checkout::{self, NoFaults};
use gayanshop_lib::db::repo::inventory::{
    self, AdjustmentInput, CountInput, CountLine, DamageInput, DamageReason, MovementCursor,
};
use gayanshop_lib::db::repo::stock;
use gayanshop_lib::domain::permissions::Perm;
use gayanshop_lib::error::AppError;

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

fn damage(product: &str, qty: i64, reason: DamageReason, note: Option<&str>) -> DamageInput {
    DamageInput { product_id: product.into(), qty, reason, note: note.map(str::to_owned) }
}

fn adjust(product: &str, qty_delta: i64, note: &str) -> AdjustmentInput {
    AdjustmentInput { product_id: product.into(), qty_delta, note: note.into() }
}

async fn on_hand(t: &Till, product: &str) -> i64 {
    t.count(&format!("SELECT COALESCE((SELECT on_hand FROM stock_levels WHERE product_id = '{product}'), 0)")).await
}

/// (qty, unit_cost) of each movement of a reason, oldest first.
async fn moves(t: &Till, product: &str, reason: &str) -> Vec<(i64, i64)> {
    sqlx::query_as(
        "SELECT qty_delta, unit_cost FROM stock_movements WHERE product_id = ?1 AND reason = ?2 ORDER BY created_at, id",
    )
    .bind(product)
    .bind(reason)
    .fetch_all(t.pool())
    .await
    .unwrap()
}

async fn sell(t: &Till, token: &str, product: &str, price: i64, qty: i64) {
    let input = cart(token, vec![line(product, price, qty)], vec![cash(price * qty, None)]);
    checkout::checkout(&t.t.db, &t.ctx(), &t.seller(CASHIER).await, &input, None, &NoFaults).await.unwrap();
}

#[tokio::test]
async fn damage_takes_units_fifo_at_layer_cost_and_shows_in_the_batch_pnl() {
    let t = till().await;
    let bucket = t.stocked("Bucket", 45_000, &[(2, 20_000), (5, 22_000)]).await;
    let c = inventory::damage(&t.t.db, &t.ctx(), &t.owner(), &damage(&bucket, 3, DamageReason::Broken, None))
        .await
        .unwrap();
    assert_eq!((c.qty_delta, c.cost_delta, c.on_hand_after), (-3, -(2 * 20_000 + 22_000), 4));
    assert_eq!(moves(&t, &bucket, "damage").await, vec![(-2, 20_000), (-1, 22_000)]);
    let written: i64 =
        t.count(&format!("SELECT SUM(written_off_cost) FROM batch_item_stock WHERE product_id = '{bucket}'")).await;
    assert_eq!(written, 62_000);
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'STOCK_DAMAGE'").await, 1);

    let too_many =
        inventory::damage(&t.t.db, &t.ctx(), &t.owner(), &damage(&bucket, 5, DamageReason::Broken, None)).await;
    assert_eq!(code(too_many.unwrap_err()), "STOCK_INSUFFICIENT");
    let no_note =
        inventory::damage(&t.t.db, &t.ctx(), &t.owner(), &damage(&bucket, 1, DamageReason::Other, None)).await;
    assert_eq!(code(no_note.unwrap_err()), "STOCK_NOTE_REQUIRED");
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let untracked =
        inventory::damage(&t.t.db, &t.ctx(), &t.owner(), &damage(&mug, 1, DamageReason::Broken, None)).await;
    assert_eq!(code(untracked.unwrap_err()), "PRODUCT_NOT_TRACKED");
    assert_eq!(on_hand(&t, &bucket).await, 4, "refusals wrote nothing");
}

#[tokio::test]
async fn adjustments_put_found_stock_in_the_newest_layer_and_need_a_reason() {
    let t = till().await;
    let bucket = t.stocked("Bucket", 45_000, &[(2, 20_000), (5, 22_000)]).await;
    let found =
        inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&bucket, 4, "found behind the shelf")).await.unwrap();
    assert_eq!((found.cost_delta, found.on_hand_after), (4 * 22_000, 11), "newest layer, its cost");
    let lost =
        inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&bucket, -3, "miscount at intake")).await.unwrap();
    assert_eq!((lost.cost_delta, lost.on_hand_after), (-(2 * 20_000 + 22_000), 8), "FIFO out");
    assert_eq!(moves(&t, &bucket, "adjustment").await, vec![(4, 22_000), (-2, 20_000), (-1, 22_000)]);

    // No layer yet: found stock goes into ADJUSTMENTS at the default cost.
    let basin = t.product("Basin", 30_000, true, 12_500, None).await;
    let c = inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&basin, 3, "opening miss")).await.unwrap();
    assert_eq!(c.cost_delta, 37_500);
    let batch: String = sqlx::query_scalar(
        "SELECT b.code FROM stock_movements m JOIN batch_items bi ON bi.id = m.batch_item_id
         JOIN batches b ON b.id = bi.batch_id WHERE m.product_id = ?1",
    )
    .bind(&basin)
    .fetch_one(t.pool())
    .await
    .unwrap();
    assert_eq!(batch, "ADJUSTMENTS");

    assert_eq!(
        code(inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&bucket, 2, "  ")).await.unwrap_err()),
        "STOCK_NOTE_REQUIRED"
    );
    assert_eq!(
        code(inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&bucket, 0, "x")).await.unwrap_err()),
        "QTY_INVALID"
    );
    assert_eq!(
        code(inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&bucket, -9, "x")).await.unwrap_err()),
        "STOCK_INSUFFICIENT"
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'STOCK_ADJUSTMENT'").await, 3);
}

#[tokio::test]
async fn a_stock_count_corrects_every_product_in_one_audited_transaction() {
    let t = till().await;
    let a = t.stocked("A", 10_000, &[(7, 4_000)]).await;
    let b = t.product("B", 9_000, true, 3_000, None).await;
    let c = t.stocked("C", 5_000, &[(4, 2_000)]).await;
    let input = CountInput {
        lines: vec![
            CountLine { product_id: a.clone(), counted: 5 },
            CountLine { product_id: b.clone(), counted: 3 },
            CountLine { product_id: c.clone(), counted: 4 },
        ],
        note: Some("Monthly count".into()),
    };
    let r = inventory::count(&t.t.db, &t.ctx(), &t.owner(), &input).await.unwrap();
    assert_eq!(r.unchanged, 1);
    assert_eq!(
        r.changes.iter().map(|c| (c.product_id.as_str(), c.qty_delta, c.cost_delta)).collect::<Vec<_>>(),
        vec![(a.as_str(), -2, -8_000), (b.as_str(), 3, 9_000)]
    );
    assert_eq!((on_hand(&t, &a).await, on_hand(&t, &b).await, on_hand(&t, &c).await), (5, 3, 4));
    assert_eq!(t.count("SELECT COUNT(*) FROM stock_movements WHERE reason = 'count_correction'").await, 2);
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'STOCK_COUNT'").await, 1);

    let dup = CountInput {
        lines: vec![CountLine { product_id: a.clone(), counted: 1 }, CountLine { product_id: a.clone(), counted: 2 }],
        note: None,
    };
    assert_eq!(code(inventory::count(&t.t.db, &t.ctx(), &t.owner(), &dup).await.unwrap_err()), "COUNT_DUPLICATE");
    let empty = CountInput { lines: vec![], note: None };
    assert_eq!(code(inventory::count(&t.t.db, &t.ctx(), &t.owner(), &empty).await.unwrap_err()), "COUNT_EMPTY");
    // A bad line refuses the whole count.
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let mixed = CountInput {
        lines: vec![CountLine { product_id: a.clone(), counted: 0 }, CountLine { product_id: mug, counted: 1 }],
        note: None,
    };
    assert_eq!(code(inventory::count(&t.t.db, &t.ctx(), &t.owner(), &mixed).await.unwrap_err()), "PRODUCT_NOT_TRACKED");
    assert_eq!(on_hand(&t, &a).await, 5, "all or nothing");
}

#[tokio::test]
async fn oversold_layers_are_listed_and_a_count_brings_them_back_to_zero() {
    let t = till().await;
    let cup = t.stocked("Cup", 10_000, &[(1, 3_000)]).await;
    sell(&t, "o1", &cup, 10_000, 3).await; // two more than the layer held
    let over = inventory::oversold(t.pool(), &t.s.store_id).await.unwrap();
    assert_eq!(over.iter().map(|o| (o.product_name.as_str(), o.on_hand)).collect::<Vec<_>>(), vec![("Cup", -2)]);
    let v = inventory::valuation(t.pool(), &t.s.store_id, None).await.unwrap();
    assert_eq!((v.negative_layers, v.total_units), (1, 0), "negative layers are flagged, not valued");

    let fix = CountInput { lines: vec![CountLine { product_id: cup.clone(), counted: 0 }], note: None };
    let r = inventory::count(&t.t.db, &t.ctx(), &t.owner(), &fix).await.unwrap();
    assert_eq!(r.changes[0].qty_delta, 2);
    assert!(inventory::oversold(t.pool(), &t.s.store_id).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_batch_can_be_written_off_then_closed() {
    let t = till().await;
    let shoe = t.stocked("Shoe", 150_000, &[(3, 90_000)]).await;
    let batch_id: String = sqlx::query_scalar(
        "SELECT bi.batch_id FROM batch_items bi JOIN batches b ON b.id = bi.batch_id
         WHERE bi.product_id = ?1 AND b.is_system = 0",
    )
    .bind(&shoe)
    .fetch_one(t.pool())
    .await
    .unwrap();
    sell(&t, "w1", &shoe, 150_000, 1).await;
    let changes = inventory::write_off_batch(&t.t.db, &t.ctx(), &t.owner(), &batch_id, "end of season").await.unwrap();
    assert_eq!(changes.iter().map(|c| (c.qty_delta, c.cost_delta)).collect::<Vec<_>>(), vec![(-2, -180_000)]);
    let d = batches::close(&t.t.db, &t.ctx(), &t.owner(), &batch_id).await.unwrap();
    let p = d.pnl.unwrap();
    assert_eq!((p.written_off_qty, p.written_off_cost, p.realised_profit), (2, 180_000, 150_000 - 90_000 - 180_000));
    let opening = batches::opening_stock_id(t.pool(), &t.s.store_id).await.unwrap();
    assert_eq!(
        code(inventory::write_off_batch(&t.t.db, &t.ctx(), &t.owner(), &opening, "x").await.unwrap_err()),
        "BATCH_SYSTEM"
    );
}

#[tokio::test]
async fn valuation_now_and_as_of_a_date_by_batch_and_category() {
    let t = till().await;
    common::exec(
        t.pool(),
        &format!(
            "INSERT INTO categories (id, store_id, name, created_at, updated_at, updated_by_device)
             VALUES ('kitchen', '{}', 'Kitchen', '{NOW}', '{NOW}', 'dev')",
            t.s.store_id
        ),
    )
    .await;
    let pot = t.stocked("Pot", 50_000, &[(4, 30_000)]).await;
    let pen = t.stocked("Pen", 2_000, &[(10, 500)]).await;
    common::exec(t.pool(), &format!("UPDATE products SET category_id = 'kitchen' WHERE id = '{pot}'")).await;

    let now = inventory::valuation(t.pool(), &t.s.store_id, None).await.unwrap();
    assert_eq!((now.total_units, now.value_at_cost, now.value_at_retail), (14, 125_000, 220_000));
    assert_eq!(now.by_batch.len(), 2);
    let kitchen = now.by_category.iter().find(|g| g.id.as_deref() == Some("kitchen")).unwrap();
    assert_eq!((kitchen.label.as_str(), kitchen.units, kitchen.value_at_cost), ("Kitchen", 4, 120_000));
    let none = now.by_category.iter().find(|g| g.id.is_none()).unwrap();
    assert_eq!(none.value_at_cost, 5_000);

    // Two days later: damage 2 pots. "As of" the first day still sees all four.
    let later = ctx_at(&t.s, "2026-11-10T09:00:00.000Z");
    inventory::damage(&t.t.db, &later, &t.owner(), &damage(&pot, 2, DamageReason::Broken, None)).await.unwrap();
    let as_of = inventory::valuation(t.pool(), &t.s.store_id, Some("2026-11-08")).await.unwrap();
    assert_eq!((as_of.as_of.as_deref(), as_of.total_units, as_of.value_at_cost), (Some("2026-11-08"), 14, 125_000));
    let today = inventory::valuation(t.pool(), &t.s.store_id, Some("2026-11-10")).await.unwrap();
    assert_eq!((today.total_units, today.value_at_cost), (12, 65_000));
    assert_eq!(today, {
        let mut n = inventory::valuation(t.pool(), &t.s.store_id, None).await.unwrap();
        n.as_of = Some("2026-11-10".into());
        n
    });
    assert_eq!(
        code(inventory::valuation(t.pool(), &t.s.store_id, Some("10/11/2026")).await.unwrap_err()),
        "DATE_INVALID"
    );
    let _ = pen;
}

#[tokio::test]
async fn movement_history_pages_newest_first_and_filters_by_layer() {
    let t = till().await;
    let bucket = t.stocked("Bucket", 45_000, &[(2, 20_000), (5, 22_000)]).await;
    for i in 0..4 {
        let at = ctx_at(&t.s, &format!("2026-11-08T10:0{i}:00.000Z"));
        inventory::adjust(&t.t.db, &at, &t.owner(), &adjust(&bucket, 1, &format!("found {i}"))).await.unwrap();
    }
    let first = inventory::movements(t.pool(), &bucket, None, None, 3).await.unwrap();
    assert_eq!(first.rows.len(), 3);
    assert_eq!(first.rows[0].note.as_deref(), Some("found 3"), "newest first");
    let (created_at, id) = first.next_cursor.clone().unwrap();
    let rest =
        inventory::movements(t.pool(), &bucket, None, Some(&MovementCursor { created_at, id }), 3).await.unwrap();
    assert_eq!(rest.rows.len(), 3, "6 movements in all: 2 intakes + 4 adjustments");
    assert!(rest.next_cursor.is_none());
    assert!(first.rows.iter().chain(&rest.rows).all(|r| !r.batch_code.is_empty()));
    let layer = first.rows[0].batch_item_id.clone();
    let one = inventory::movements(t.pool(), &bucket, Some(&layer), None, 50).await.unwrap();
    assert!(one.rows.iter().all(|r| r.batch_item_id == layer));
    assert_eq!(one.rows[0].staff_name.as_deref(), Some("Gayan"));
}

#[tokio::test]
async fn the_ledger_still_rebuilds_to_the_same_derived_stock() {
    let t = till().await;
    let a = t.stocked("A", 10_000, &[(5, 4_000), (5, 4_500)]).await;
    sell(&t, "r1", &a, 10_000, 3).await;
    inventory::damage(&t.t.db, &t.ctx(), &t.owner(), &damage(&a, 2, DamageReason::Expired, None)).await.unwrap();
    inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&a, 1, "found")).await.unwrap();
    let count = CountInput { lines: vec![CountLine { product_id: a.clone(), counted: 4 }], note: None };
    inventory::count(&t.t.db, &t.ctx(), &t.owner(), &count).await.unwrap();
    let snapshot =
        "SELECT group_concat(batch_item_id || ':' || on_hand || ':' || written_off_qty || ':' || written_off_cost, ',')
                    FROM (SELECT * FROM batch_item_stock ORDER BY batch_item_id)";
    let before: String = sqlx::query_scalar(snapshot).fetch_one(t.pool()).await.unwrap();
    let mut tx = t.t.db.write().await.unwrap();
    stock::rebuild_derived(tx.conn()).await.unwrap();
    tx.commit().await.unwrap();
    let after: String = sqlx::query_scalar(snapshot).fetch_one(t.pool()).await.unwrap();
    assert_eq!(before, after);
    assert_eq!(on_hand(&t, &a).await, 4);
}

#[tokio::test]
async fn stock_writes_need_their_permission_or_a_supervisor() {
    let t = till().await;
    t.login(CASHIER, common::CASHIER_PIN).await;
    assert!(matches!(
        require(&t.auth, &t.clock, Perm::STOCK_DAMAGE, "stock", None).unwrap_err(),
        AppError::ApprovalRequired { perm: Perm::STOCK_DAMAGE }
    ));
    assert!(matches!(
        require(&t.auth, &t.clock, Perm::REPORT_FINANCIAL, "reports", None).unwrap_err(),
        AppError::Forbidden { .. }
    ));
    t.login(SUP, common::SUP_PIN).await;
    assert!(require(&t.auth, &t.clock, Perm::STOCK_DAMAGE, "stock", None).is_ok(), "supervisors record damage");
}

#[tokio::test]
async fn a_product_with_stock_on_hand_can_be_deleted_writing_off_its_stock() {
    use gayanshop_lib::db::repo::products;
    let t = till().await;
    let bucket = t.stocked("Bucket", 45_000, &[(3, 20_000)]).await;
    assert_eq!(on_hand(&t, &bucket).await, 3);

    // Deleting product with stock writes off remaining units and soft-deletes the product
    products::delete(&t.t.db, &t.ctx(), &t.owner(), &bucket).await.unwrap();
    assert_eq!(on_hand(&t, &bucket).await, 0, "the stock was written off to zero");
    assert_eq!(
        t.count(&format!("SELECT COUNT(*) FROM products WHERE id = '{bucket}' AND deleted_at IS NOT NULL")).await,
        1
    );
}

/// The valuation recomputed straight from the ledger (not the derived caches): each live, tracked product's
/// layers, Σ max(layer on hand, 0) × the layer's cost and × today's price, plus the negative layers.
async fn ledger_valuation(t: &Till) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT COALESCE(SUM(MAX(l.on_hand, 0)), 0), COALESCE(SUM(MAX(l.on_hand, 0) * bi.unit_cost), 0),
                COALESCE(SUM(MAX(l.on_hand, 0) * p.price), 0), COALESCE(SUM(l.on_hand < 0), 0)
         FROM (SELECT batch_item_id, SUM(qty_delta) AS on_hand FROM stock_movements GROUP BY batch_item_id) l
         JOIN batch_items bi ON bi.id = l.batch_item_id
         JOIN products p ON p.id = bi.product_id
         WHERE p.deleted_at IS NULL AND p.track_stock = 1",
    )
    .fetch_one(t.pool())
    .await
    .unwrap()
}

async fn valued(t: &Till) -> (i64, i64, i64, i64) {
    let v = inventory::valuation(t.pool(), &t.s.store_id, None).await.unwrap();
    let sum = |f: fn(&inventory::ValuationGroup) -> i64, g: &[inventory::ValuationGroup]| g.iter().map(f).sum::<i64>();
    for groups in [&v.by_batch, &v.by_category] {
        assert_eq!(
            (sum(|g| g.units, groups), sum(|g| g.value_at_cost, groups), sum(|g| g.value_at_retail, groups)),
            (v.total_units, v.value_at_cost, v.value_at_retail),
            "each breakdown adds up to the totals"
        );
    }
    (v.total_units, v.value_at_cost, v.value_at_retail, v.negative_layers)
}

/// Σ `stock_levels` over the products the Products & Stock list shows with a stock figure.
async fn listed_units(t: &Till) -> i64 {
    t.count(
        "SELECT COALESCE(SUM(sl.on_hand), 0) FROM products p JOIN stock_levels sl ON sl.product_id = p.id
         WHERE p.deleted_at IS NULL AND p.track_stock = 1",
    )
    .await
}

#[tokio::test]
async fn valuation_matches_the_ledger_and_the_product_list_through_sales_returns_adjustments_and_imports() {
    use gayanshop_lib::db::repo::exchange::{ExchangeInput, ExchangeReturn};
    let t = till().await;
    let bucket = t.stocked("Bucket", 45_000, &[(2, 20_000), (5, 22_000)]).await;
    let mug = t.stocked("Mug", 25_000, &[(10, 9_000)]).await;
    assert_eq!(valued(&t).await, (17, 240_000, 565_000, 0));

    // A sale takes the oldest layer first: 2 @ 200 + 1 @ 220 leave 4 @ 220.
    let sale = cart("v-sale", vec![line(&bucket, 45_000, 3)], vec![cash(135_000, None)]);
    let sold = checkout::checkout(&t.t.db, &t.ctx(), &t.seller(CASHIER).await, &sale, None, &NoFaults).await.unwrap();
    assert_eq!(valued(&t).await, (14, 178_000, 430_000, 0));

    // An exchange: one bucket comes back into the layer it was sold from, two mugs go out.
    let item: String = sqlx::query_scalar("SELECT id FROM order_items WHERE order_id = ?1")
        .bind(&sold.order_id)
        .fetch_one(t.pool())
        .await
        .unwrap();
    let mut ex = cart("v-ex", vec![line(&mug, 25_000, 2)], vec![cash(5_000, None)]);
    ex.exchange = Some(ExchangeInput { returns: vec![ExchangeReturn { item_id: item, qty: 1 }] });
    checkout::checkout(&t.t.db, &t.ctx(), &t.seller(CASHIER).await, &ex, None, &NoFaults).await.unwrap();

    // Found stock goes into the newest layer; a write-down comes out FIFO.
    inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&bucket, 2, "found")).await.unwrap();
    inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&mug, -1, "miscount")).await.unwrap();

    // A CSV import: a new product at its cost, then its quantity as an adjustment (the ADJUSTMENTS layer at
    // that cost).
    let towel = t.product("Towel", 12_000, true, 7_000, None).await;
    inventory::adjust(&t.t.db, &t.ctx(), &t.owner(), &adjust(&towel, 6, "opening stock (import)")).await.unwrap();

    let (units, cost, retail, negative) = valued(&t).await;
    assert_eq!((units, cost, retail, negative), ledger_valuation(&t).await, "the derived caches agree with the ledger");
    assert_eq!(units, listed_units(&t).await, "with no oversold layer, units = the Products & Stock list");
    assert_eq!(units, (7 - 3 + 1 + 2) + (10 - 2 - 1) + 6);
    let towel_cost: i64 = t
        .count(&format!(
            "SELECT bi.unit_cost FROM batch_items bi JOIN batch_item_stock s ON s.batch_item_id = bi.id
             WHERE bi.product_id = '{towel}'"
        ))
        .await;
    assert_eq!(towel_cost, 7_000, "imported stock is valued at the imported cost");
    // Every movement was written at its layer's cost, so the layer cost is the cost of what's on the shelf.
    let off_cost = t
        .count(
            "SELECT COUNT(*) FROM stock_movements m JOIN batch_items bi ON bi.id = m.batch_item_id
             WHERE m.unit_cost <> bi.unit_cost",
        )
        .await;
    assert_eq!(off_cost, 0);
}

#[tokio::test]
async fn a_deleted_product_leaves_valuation_and_the_oversold_list_even_after_an_oversell() {
    let t = till().await;
    // Sold before any stock arrived: an ADJUSTMENTS layer at −3. Then 10 arrive in a new layer.
    let cup = t.product("Cup", 10_000, true, 0, None).await;
    sell(&t, "d1", &cup, 10_000, 3).await;
    let buy =
        batches::AdHocPurchase { product_id: cup.clone(), qty: 10, unit_cost: 3_000, supplier_name: None, note: None };
    batches::ad_hoc_purchase(&t.t.db, &t.ctx(), &t.owner(), &buy).await.unwrap();
    // The documented rule (doc §5): positive layers at cost, the oversold one flagged and left out.
    assert_eq!(valued(&t).await, (10, 30_000, 100_000, 1));
    assert_eq!(on_hand(&t, &cup).await, 7);

    // Deleting writes off the 7 FIFO from the positive layer, leaving +3 / −3 behind in the ledger.
    gayanshop_lib::db::repo::products::delete(&t.t.db, &t.ctx(), &t.owner(), &cup).await.unwrap();
    assert_eq!(on_hand(&t, &cup).await, 0);
    assert_eq!(t.count(&format!("SELECT MAX(on_hand) FROM batch_item_stock WHERE product_id = '{cup}'")).await, 3);
    assert_eq!(valued(&t).await, (0, 0, 0, 0), "nothing of a deleted product is valued or flagged");
    assert!(inventory::oversold(t.pool(), &t.s.store_id).await.unwrap().is_empty());
}

#[tokio::test]
async fn inactive_stock_is_still_valued_but_an_untracked_product_is_not() {
    let t = till().await;
    let pen = t.stocked("Pen", 2_000, &[(10, 500)]).await;
    let pot = t.stocked("Pot", 50_000, &[(2, 30_000)]).await;
    // Hidden from the till, still on the shelf.
    common::exec(t.pool(), &format!("UPDATE products SET is_active = 0 WHERE id = '{pen}'")).await;
    assert_eq!(valued(&t).await, (12, 65_000, 120_000, 0));
    // No longer tracked: the list shows a dash, so valuation leaves its old layers out too.
    common::exec(t.pool(), &format!("UPDATE products SET track_stock = 0 WHERE id = '{pot}'")).await;
    assert_eq!(valued(&t).await, (10, 5_000, 20_000, 0));
    // "As of" reads history as it stood: the pot was tracked stock on that date.
    let as_of = inventory::valuation(t.pool(), &t.s.store_id, Some("2026-11-08")).await.unwrap();
    assert_eq!(as_of.total_units, 12);
}
