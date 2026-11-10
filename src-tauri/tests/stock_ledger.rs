//! Block 1.2 — minimal batches and the stock ledger: draft → receive with manual costs, opening stock,
//! ad-hoc purchases, the derived `batch_item_stock` / `stock_levels`, FIFO layer selection, and rebuild.

mod common;

use std::collections::BTreeMap;

use common::{Sale, checkout, ctx_at, exec, open, scalar_i64, seed, store};
use gayanshop_lib::auth::guard::Actor;
use gayanshop_lib::db::repo::WriteCtx;
use gayanshop_lib::db::repo::batches::{self, AdHocPurchase, BatchInput, BatchItemInput, BatchStatus};
use gayanshop_lib::db::repo::products::{self, CodeChoice, ProductInput, ProductKind};
use gayanshop_lib::db::repo::stock::{self, Movement, StockReason};
use gayanshop_lib::error::AppError;
use sqlx::SqlitePool;

/// (batch_item_id, received, on_hand, sold_qty, sold_revenue, sold_cost, written_off_qty, written_off_cost)
type LayerSnapshot = (String, i64, i64, i64, i64, i64, i64, i64);

const T1: &str = "2026-11-08T09:00:00.000Z";
const T2: &str = "2026-11-08T10:00:00.000Z";
const T3: &str = "2026-11-08T11:00:00.000Z";

fn owner(s: &gayanshop_lib::setup::SetupResult) -> Actor {
    Actor { staff_id: s.owner_staff_id.clone(), approved_by: None }
}

async fn product(t: &common::TestDb, ctx: &WriteCtx, actor: &Actor, name: &str, cost: i64, tracked: bool) -> String {
    let input = ProductInput {
        kind: ProductKind::Standard,
        name: name.into(),
        name_si: None,
        sku: CodeChoice::Generate,
        barcode: CodeChoice::None,
        category_id: None,
        default_cost: Some(cost),
        price: 45_000,
        min_price: None,
        track_stock: tracked,
        allow_negative_stock: true,
        attributes: BTreeMap::new(),
        is_active: true,
    };
    products::create(&t.db, ctx, actor, &input, true).await.unwrap().id
}

fn header(name: &str, total: i64, extra: i64) -> BatchInput {
    BatchInput {
        name: name.into(),
        supplier_name: Some("Pettah wholesale".into()),
        supplier_ref: Some("INV-77".into()),
        purchased_at: None,
        total_cost: total,
        extra_costs: extra,
        allocation_method: None,
        notes: None,
    }
}

fn item(product_id: &str, qty: i64, unit_cost: i64) -> BatchItemInput {
    BatchItemInput { product_id: product_id.into(), qty, unit_cost, retail_price: None }
}

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

async fn on_hand(pool: &SqlitePool, product_id: &str) -> i64 {
    scalar_i64(
        pool,
        &format!("SELECT COALESCE((SELECT on_hand FROM stock_levels WHERE product_id = '{product_id}'), 0)"),
    )
    .await
}

#[tokio::test]
async fn receive_locks_costs_and_writes_one_intake_per_layer() {
    let t = open().await;
    let s = store(&t).await;
    let (ctx, a) = (ctx_at(&s, T1), owner(&s));
    let bucket = product(&t, &ctx, &a, "Blue Bucket", 15_000, true).await;
    let basin = product(&t, &ctx, &a, "Red Basin", 9_000, true).await;

    let draft = batches::create(&t.db, &ctx, &a, &header("Pettah 8 Nov", 0, 0)).await.unwrap();
    assert_eq!(draft.batch.status, BatchStatus::Draft);
    assert!(draft.batch.code.starts_with("B-2026-1"), "{}", draft.batch.code);
    let id = draft.batch.id;
    batches::set_item(&t.db, &ctx, &id, &item(&bucket, 10, 20_000)).await.unwrap();
    batches::set_item(&t.db, &ctx, &id, &item(&basin, 4, 12_500)).await.unwrap();
    // Editing a line in a draft replaces it (one layer per product per batch).
    let d = batches::set_item(&t.db, &ctx, &id, &item(&basin, 5, 12_500)).await.unwrap();
    assert_eq!((d.items.len(), d.batch.total_units, d.batch.items_cost), (2, 15, 262_500));
    assert_eq!(on_hand(t.pool(), &bucket).await, 0, "a draft holds no stock");

    let r = batches::receive(&t.db, &ctx, &a, &id).await.unwrap();
    assert_eq!(r.batch.status, BatchStatus::Received);
    assert_eq!((r.batch.total_cost, r.batch.allocation_variance), (262_500, 0), "no invoice total ⇒ Σ layers");
    let layer = r.items.iter().find(|i| i.product_id == bucket).unwrap();
    assert_eq!((layer.unit_cost, layer.qty_received, layer.on_hand), (20_000, 10, 10));
    assert_eq!(on_hand(t.pool(), &bucket).await, 10);
    assert_eq!(on_hand(t.pool(), &basin).await, 5);

    let p = t.pool();
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM stock_movements WHERE reason = 'batch_intake'").await, 2);
    assert_eq!(scalar_i64(p, "SELECT SUM(qty_delta * unit_cost) FROM stock_movements").await, 262_500);
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'stock_movements'").await, 2);
    assert_eq!(
        scalar_i64(p, "SELECT COUNT(*) FROM audit_logs WHERE action IN ('BATCH_CREATE','BATCH_RECEIVE')").await,
        2
    );
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM batch_item_stock WHERE received_qty = on_hand").await, 2);

    // Costs and quantities are locked (SQL trigger), and the batch can't be edited or received again.
    let lock = sqlx::query(&format!("UPDATE batch_items SET unit_cost = 1 WHERE id = '{}'", layer.id)).execute(p).await;
    assert!(lock.unwrap_err().to_string().contains("locked after receive"));
    assert_eq!(code(batches::set_item(&t.db, &ctx, &id, &item(&bucket, 1, 1)).await.unwrap_err()), "BATCH_NOT_DRAFT");
    assert_eq!(code(batches::receive(&t.db, &ctx, &a, &id).await.unwrap_err()), "BATCH_NOT_DRAFT");
    assert_eq!(code(batches::remove_item(&t.db, &ctx, &id, &layer.id).await.unwrap_err()), "BATCH_NOT_DRAFT");
}

#[tokio::test]
async fn invoice_total_differences_land_in_allocation_variance() {
    let t = open().await;
    let s = store(&t).await;
    let (ctx, a) = (ctx_at(&s, T1), owner(&s));
    let mug = product(&t, &ctx, &a, "Mug", 0, true).await;
    // Paid Rs 1,000 + Rs 50 transport for 3 mugs typed at Rs 333.33: landed − Σ = 105,000 − 99,999 = 5,001.
    let b = batches::create(&t.db, &ctx, &a, &header("Mugs", 100_000, 5_000)).await.unwrap();
    batches::set_item(&t.db, &ctx, &b.batch.id, &item(&mug, 3, 33_333)).await.unwrap();
    let r = batches::receive(&t.db, &ctx, &a, &b.batch.id).await.unwrap();
    assert_eq!((r.batch.total_cost, r.batch.extra_costs, r.batch.allocation_variance), (100_000, 5_000, 5_001));
}

#[tokio::test]
async fn invalid_batches_are_rejected_without_partial_writes() {
    let t = open().await;
    let s = store(&t).await;
    let (ctx, a) = (ctx_at(&s, T1), owner(&s));
    let tracked = product(&t, &ctx, &a, "Bucket", 0, true).await;
    let service = product(&t, &ctx, &a, "Gift Wrap", 0, false).await;

    assert_eq!(code(batches::create(&t.db, &ctx, &a, &header(" ", 0, 0)).await.unwrap_err()), "BATCH_NAME_INVALID");
    assert_eq!(code(batches::create(&t.db, &ctx, &a, &header("x", -1, 0)).await.unwrap_err()), "BATCH_COST_INVALID");
    let bad_date = BatchInput { purchased_at: Some("yesterday".into()), ..header("x", 0, 0) };
    assert_eq!(code(batches::create(&t.db, &ctx, &a, &bad_date).await.unwrap_err()), "BATCH_DATE_INVALID");

    let b = batches::create(&t.db, &ctx, &a, &header("Empty", 0, 0)).await.unwrap().batch.id;
    assert_eq!(code(batches::receive(&t.db, &ctx, &a, &b).await.unwrap_err()), "BATCH_EMPTY");
    for (input, expected) in [
        (item(&tracked, 0, 100), "BATCH_ITEM_QTY_INVALID"),
        (item(&tracked, 1, -1), "BATCH_ITEM_COST_INVALID"),
        (item(&service, 1, 100), "PRODUCT_NOT_TRACKED"),
        (item("nope", 1, 100), "BATCH_ITEM_PRODUCT_INVALID"),
    ] {
        assert_eq!(code(batches::set_item(&t.db, &ctx, &b, &input).await.unwrap_err()), expected);
    }
    let d = batches::set_item(&t.db, &ctx, &b, &item(&tracked, 2, 100)).await.unwrap();
    let d = batches::remove_item(&t.db, &ctx, &b, &d.items[0].id).await.unwrap();
    assert!(d.items.is_empty());
    assert_eq!(code(batches::receive(&t.db, &ctx, &a, &b).await.unwrap_err()), "BATCH_EMPTY");
    // A removed line can be added back (the soft-deleted layer row is revived).
    let d = batches::set_item(&t.db, &ctx, &b, &item(&tracked, 3, 100)).await.unwrap();
    assert_eq!(d.batch.total_units, 3);

    // An ad-hoc purchase that fails validation leaves no batch behind.
    let before = scalar_i64(t.pool(), "SELECT COUNT(*) FROM batches").await;
    let bad = AdHocPurchase { product_id: service.clone(), qty: 1, unit_cost: 1, supplier_name: None, note: None };
    assert_eq!(code(batches::ad_hoc_purchase(&t.db, &ctx, &a, &bad).await.unwrap_err()), "PRODUCT_NOT_TRACKED");
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM batches").await, before);
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM stock_movements").await, 0);
}

#[tokio::test]
async fn opening_stock_and_ad_hoc_purchases_create_layers() {
    let t = open().await;
    let s = store(&t).await;
    let (ctx, a) = (ctx_at(&s, T1), owner(&s));
    let umbrella = product(&t, &ctx, &a, "Umbrella", 0, true).await;
    let opening = batches::opening_stock_id(t.pool(), &s.store_id).await.unwrap();
    let d = batches::detail(t.pool(), &s.store_id, &opening).await.unwrap();
    assert_eq!((d.batch.status, d.batch.is_system), (BatchStatus::Draft, true));

    batches::set_item(&t.db, &ctx, &opening, &item(&umbrella, 6, 55_000)).await.unwrap();
    // System batches keep their name.
    let renamed = batches::update(&t.db, &ctx, &opening, &header("Something else", 0, 0)).await.unwrap();
    assert_eq!((renamed.batch.code.as_str(), renamed.batch.name.as_str()), ("OPENING-STOCK", "Opening stock"));
    batches::receive(&t.db, &ctx, &a, &opening).await.unwrap();

    let buy = AdHocPurchase {
        product_id: umbrella.clone(),
        qty: 12,
        unit_cost: 48_000,
        supplier_name: Some("Market".into()),
        note: None,
    };
    let r = batches::ad_hoc_purchase(&t.db, &ctx_at(&s, T2), &a, &buy).await.unwrap();
    assert_eq!(
        (r.batch.status, r.batch.name.as_str(), r.batch.total_cost),
        (BatchStatus::Received, "Umbrella", 576_000)
    );
    assert_eq!(on_hand(t.pool(), &umbrella).await, 18);

    let mut conn = t.pool().acquire().await.unwrap();
    let layers = stock::product_layers(&mut conn, &umbrella).await.unwrap();
    let codes: Vec<&str> = layers.iter().map(|l| l.batch_code.as_str()).collect();
    assert_eq!(codes[0], "OPENING-STOCK", "opening stock is the oldest layer");
    assert_eq!(layers.iter().map(|l| (l.unit_cost, l.on_hand)).collect::<Vec<_>>(), vec![(55_000, 6), (48_000, 12)]);
}

#[tokio::test]
async fn fifo_takes_oldest_layers_first_then_oversells_the_newest() {
    let t = open().await;
    let s = store(&t).await;
    let a = owner(&s);
    let ctx = ctx_at(&s, T1);
    let shoe = product(&t, &ctx, &a, "Shoe", 0, true).await;
    for (at, qty, cost) in [(T1, 2, 10_000), (T2, 3, 12_000), (T3, 1, 15_000)] {
        let buy = AdHocPurchase { product_id: shoe.clone(), qty, unit_cost: cost, supplier_name: None, note: None };
        batches::ad_hoc_purchase(&t.db, &ctx_at(&s, at), &a, &buy).await.unwrap();
    }
    let mut conn = t.pool().acquire().await.unwrap();
    // 4 units, Rs 1,000.01 net: 2 from the oldest layer, 2 from the next; revenue split by qty exactly.
    let slices = stock::plan_sale(&mut conn, &ctx, &shoe, 4, 100_001).await.unwrap();
    let view: Vec<(i64, i64, i64)> = slices.iter().map(|x| (x.qty, x.unit_cost, x.revenue_net)).collect();
    assert_eq!(view, vec![(2, 10_000, 50_000), (2, 12_000, 50_001)]);
    // 9 units: everything on hand (6), then 3 more from the NEWEST layer, which goes negative.
    let slices = stock::plan_sale(&mut conn, &ctx, &shoe, 9, 0).await.unwrap();
    let view: Vec<(i64, i64)> = slices.iter().map(|x| (x.qty, x.unit_cost)).collect();
    assert_eq!(view, vec![(2, 10_000), (3, 12_000), (4, 15_000)]);
    assert_eq!(code(stock::plan_sale(&mut conn, &ctx, &shoe, 0, 0).await.unwrap_err()), "QTY_INVALID");
}

#[tokio::test]
async fn a_product_without_layers_gets_an_adjustments_layer_at_default_cost() {
    let t = open().await;
    let s = store(&t).await;
    let a = owner(&s);
    let ctx = ctx_at(&s, T1);
    let comb = product(&t, &ctx, &a, "Comb", 3_500, true).await;
    let mut conn = t.pool().acquire().await.unwrap();
    let slices = stock::plan_sale(&mut conn, &ctx, &comb, 2, 9_000).await.unwrap();
    assert_eq!(slices.len(), 1);
    assert_eq!((slices[0].qty, slices[0].unit_cost), (2, 3_500));
    let layers = stock::product_layers(&mut conn, &comb).await.unwrap();
    assert_eq!((layers.len(), layers[0].batch_code.as_str()), (1, "ADJUSTMENTS"));
    // Asking again reuses the same layer.
    stock::plan_sale(&mut conn, &ctx, &comb, 1, 0).await.unwrap();
    assert_eq!(stock::product_layers(&mut conn, &comb).await.unwrap().len(), 1);
}

#[tokio::test]
async fn rebuild_reproduces_the_incremental_derived_tables() {
    let t = open().await;
    let p = t.pool();
    seed(p).await; // p1 with a received 2-unit layer bi1 (store "s"), an open session, roles
    exec(p, &format!("INSERT INTO products (id, store_id, name, price, created_at, updated_at, updated_by_device) VALUES ('p2', 's', 'Basin', 9000, {})", common::META)).await;
    exec(p, &format!("INSERT INTO batch_items (id, store_id, batch_id, product_id, qty_received, unit_cost, created_at, updated_at, updated_by_device) VALUES ('bi2', 's', 'b1', 'p2', 5, 7000, {})", common::META)).await;
    let ctx = WriteCtx {
        store_id: "s".into(),
        device_id: "dev1".into(),
        device_code: "T01".into(),
        now: T1.into(),
        business_date: common::BD.into(),
    };
    let mut conn = p.acquire().await.unwrap();
    let mv = |product: &'static str, layer: &'static str, qty: i64, reason: StockReason, cost: i64| Movement {
        product_id: product,
        batch_item_id: layer,
        qty_delta: qty,
        reason,
        ref_type: None,
        ref_id: None,
        order_item_id: None,
        unit_cost: cost,
        revenue_net: None,
        staff_id: None,
        approved_by: None,
        note: None,
    };
    stock::record(&mut conn, &ctx, &mv("p2", "bi2", 5, StockReason::BatchIntake, 7_000)).await.unwrap();
    stock::record(&mut conn, &ctx, &mv("p2", "bi2", -1, StockReason::Damage, 7_000)).await.unwrap();
    stock::record(&mut conn, &ctx, &mv("p2", "bi2", 2, StockReason::CountCorrection, 7_000)).await.unwrap();
    stock::record(&mut conn, &ctx, &mv("p1", "bi1", -1, StockReason::Adjustment, 20_000)).await.unwrap();
    drop(conn);
    checkout(p, &Sale::new("o1")).await;
    checkout(p, &Sale { disc: 500, ..Sale::new("o2") }).await; // oversells bi1 to −1

    let snapshot = |p: SqlitePool| async move {
        let levels: Vec<(String, i64)> =
            sqlx::query_as("SELECT product_id, on_hand FROM stock_levels ORDER BY product_id")
                .fetch_all(&p)
                .await
                .unwrap();
        let layers: Vec<LayerSnapshot> = sqlx::query_as(
            "SELECT batch_item_id, received_qty, on_hand, sold_qty, sold_revenue, sold_cost, written_off_qty,
                    written_off_cost FROM batch_item_stock ORDER BY batch_item_id",
        )
        .fetch_all(&p)
        .await
        .unwrap();
        (levels, layers)
    };
    let incremental = snapshot(p.clone()).await;
    assert_eq!(incremental.0, vec![("p1".to_owned(), -1), ("p2".to_owned(), 6)]);
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM batch_item_stock WHERE on_hand < 0").await, 1, "oversold layer");

    let mut conn = p.acquire().await.unwrap();
    stock::rebuild_derived(&mut conn).await.unwrap();
    drop(conn);
    assert_eq!(snapshot(p.clone()).await, incremental);
    // The ledger itself stays append-only.
    let upd = sqlx::query("UPDATE stock_movements SET qty_delta = 1").execute(p).await;
    assert!(upd.unwrap_err().to_string().contains("append-only"));
}

#[tokio::test]
async fn product_list_and_get_carry_units_on_hand() {
    let till = common::till().await;
    let pot = till.stocked("Pot", 50_000, &[(7, 20_000), (3, 21_000)]).await;
    let never_stocked = till.product("Mug", 10_000, true, 0, None).await;
    let untracked = till.product("Bag", 5_000, false, 0, None).await;
    let store = till.ctx().store_id;

    let list = products::list(till.pool(), &store, true).await.unwrap();
    let on_hand = |id: &str| list.iter().find(|p| p.id == id).map(|p| p.on_hand);
    assert_eq!(on_hand(&pot), Some(10), "both layers count");
    assert_eq!(on_hand(&never_stocked), Some(0), "no stock_levels row reads as 0");
    assert_eq!(on_hand(&untracked), Some(0));
    assert_eq!(list.len(), 3, "the stock join never duplicates or drops a product");
    assert_eq!(products::get(till.pool(), &store, &pot, false).await.unwrap().on_hand, 10);
}
