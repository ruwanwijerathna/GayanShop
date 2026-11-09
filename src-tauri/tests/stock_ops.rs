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
