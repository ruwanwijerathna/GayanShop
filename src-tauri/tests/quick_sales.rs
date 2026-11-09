//! Block 2.4 — quick-sale review and promotion (doc §3.2): the frequency table over `quick_sale_stats`,
//! the per-key detail (median price, counters), promote (product + optional batch layer + shelf count, one
//! transaction, forward-only), ignore, the offline-safe link id, and the till's "now a product" hint.

mod common;

use common::{CASHIER, Till, cart, cash, quick, till};
use gayanshop_lib::clock::stable_id;
use gayanshop_lib::db::repo::checkout::{self, NoFaults};
use gayanshop_lib::db::repo::products::lookup_code;
use gayanshop_lib::db::repo::quick_sales::{self, PromoteInput, QuickSaleStatus};
use gayanshop_lib::db::repo::void::{self, VoidInput, VoidReason, Voider};
use gayanshop_lib::error::AppError;

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

async fn sell_quick(t: &Till, token: &str, name: &str, price: i64, qty: i64) -> String {
    let input = cart(token, vec![quick(name, price, qty)], vec![cash(price * qty, None)]);
    checkout::checkout(&t.t.db, &t.ctx(), &t.seller(CASHIER).await, &input, None, &NoFaults).await.unwrap().order_id
}

fn promote_input(key: &str, shelf: Option<i64>, batch: Option<String>) -> PromoteInput {
    PromoteInput {
        custom_key: key.into(),
        name: "Blue Bucket 10L".into(),
        price: 45_000,
        default_cost: 20_000,
        category_id: None,
        generate_barcode: true,
        shelf_qty: shelf,
        batch_id: batch,
    }
}

/// Bucket ×4 sales (prices 400/450/450/500), basin ×1, and a voided third key.
async fn history(t: &Till) {
    sell_quick(t, "q1", "Blue bucket 10L", 40_000, 1).await;
    sell_quick(t, "q2", "blue  BUCKET 10l", 45_000, 2).await;
    sell_quick(t, "q3", "Blue Bucket 10L", 45_000, 1).await;
    sell_quick(t, "q4", "Blue Bucket 10L ", 50_000, 1).await;
    sell_quick(t, "q5", "Plastic basin", 30_000, 1).await;
    let voided = sell_quick(t, "q6", "Mystery item", 9_900, 1).await;
    let sup = t.seller(common::SUP).await;
    let input = VoidInput { order_id: voided, reason: VoidReason::WrongItem, note: None, approval_id: None };
    void::void_order(
        &t.t.db,
        &t.ctx(),
        &Voider { staff_id: common::SUP.into(), perms: sup.perms },
        &input,
        None,
        &NoFaults,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn the_review_ranks_keys_by_frequency_with_prices_and_dates() {
    let t = till().await;
    history(&t).await;
    let rows = quick_sales::review(t.pool(), &t.s.store_id, None).await.unwrap();
    let keys: Vec<&str> = rows.iter().map(|r| r.custom_key.as_str()).collect();
    assert_eq!(keys, vec!["blue bucket 10l", "plastic basin"], "voided-only keys drop out");
    let b = &rows[0];
    assert_eq!((b.times_sold, b.units_sold, b.min_price, b.max_price, b.last_price), (4, 5, 40_000, 50_000, 50_000));
    assert_eq!(b.display_name, "Blue Bucket 10L", "the latest spelling");
    assert_eq!(b.status, QuickSaleStatus::New);
    assert_eq!(b.revenue_net, 40_000 + 90_000 + 45_000 + 50_000);

    let d = quick_sales::detail(t.pool(), "blue bucket 10l").await.unwrap();
    assert_eq!(d.median_price, 45_000);
    assert_eq!(
        d.counters.iter().map(|c| (c.device_code.as_str(), c.times_sold, c.units)).collect::<Vec<_>>(),
        vec![("T01", 4, 5)]
    );
    assert!(d.spellings.contains(&"blue  BUCKET 10l".to_owned()));
    assert!(matches!(quick_sales::detail(t.pool(), "nothing").await.unwrap_err(), AppError::NotFound { .. }));
}

#[tokio::test]
async fn promotion_creates_a_scannable_product_with_shelf_stock_in_one_transaction() {
    let t = till().await;
    history(&t).await;
    let r = quick_sales::promote(&t.t.db, &t.ctx(), &t.owner(), &promote_input("blue bucket 10l", Some(6), None))
        .await
        .unwrap();
    let p = &r.product;
    assert_eq!(
        (p.name.as_str(), p.price, p.default_cost, p.track_stock),
        ("Blue Bucket 10L", 45_000, Some(20_000), true)
    );
    let barcode = p.barcode.clone().unwrap();
    assert_eq!(lookup_code(t.pool(), &t.s.store_id, &barcode).await.unwrap().unwrap().id, p.id, "scannable at once");
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{}'", p.id)).await, 6);
    let layer_batch: String = sqlx::query_scalar(
        "SELECT b.code FROM stock_movements m JOIN batch_items bi ON bi.id = m.batch_item_id
         JOIN batches b ON b.id = bi.batch_id WHERE m.product_id = ?1",
    )
    .bind(&p.id)
    .fetch_one(t.pool())
    .await
    .unwrap();
    assert_eq!(layer_batch, "ADJUSTMENTS", "no batch chosen: shelf stock at the default cost");

    let rows = quick_sales::review(t.pool(), &t.s.store_id, Some(QuickSaleStatus::Promoted)).await.unwrap();
    assert_eq!((rows.len(), rows[0].product_id.as_deref()), (1, Some(p.id.as_str())));
    let link_id: String = sqlx::query_scalar("SELECT id FROM quick_sale_links WHERE custom_key = 'blue bucket 10l'")
        .fetch_one(t.pool())
        .await
        .unwrap();
    assert_eq!(link_id, stable_id("quick_sale", &t.s.store_id, "blue bucket 10l"), "UUIDv5: offline managers converge");
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'QUICK_SALE_PROMOTE'").await, 1);

    // Forward-only: past quick-sale lines keep their attribution.
    assert_eq!(
        t.count("SELECT COUNT(*) FROM order_items WHERE is_custom = 1 AND custom_key = 'blue bucket 10l'").await,
        4
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM order_items WHERE product_id IS NOT NULL").await, 0);

    let again =
        quick_sales::promote(&t.t.db, &t.ctx(), &t.owner(), &promote_input("blue bucket 10l", None, None)).await;
    assert_eq!(code(again.unwrap_err()), "QUICK_SALE_ALREADY_PROMOTED");
}

#[tokio::test]
async fn shelf_stock_can_join_a_received_batch() {
    let t = till().await;
    history(&t).await;
    let shoe = t.stocked("Shoe", 150_000, &[(2, 90_000)]).await;
    let batch_id: String = sqlx::query_scalar(
        "SELECT bi.batch_id FROM batch_items bi JOIN batches b ON b.id = bi.batch_id WHERE bi.product_id = ?1 AND b.is_system = 0",
    )
    .bind(&shoe)
    .fetch_one(t.pool())
    .await
    .unwrap();
    let r = quick_sales::promote(
        &t.t.db,
        &t.ctx(),
        &t.owner(),
        &promote_input("plastic basin", Some(3), Some(batch_id.clone())),
    )
    .await
    .unwrap();
    let (batch, cost, late): (String, i64, bool) =
        sqlx::query_as("SELECT batch_id, unit_cost, added_after_receipt FROM batch_items WHERE product_id = ?1")
            .bind(&r.product.id)
            .fetch_one(t.pool())
            .await
            .unwrap();
    assert_eq!((batch.as_str(), cost, late), (batch_id.as_str(), 20_000, true));
    assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{}'", r.product.id)).await, 3);
    let link: Option<String> =
        sqlx::query_scalar("SELECT batch_id FROM quick_sale_links WHERE custom_key = 'plastic basin'")
            .fetch_one(t.pool())
            .await
            .unwrap();
    assert_eq!(link.as_deref(), Some(batch_id.as_str()));

    let products_before = t.count("SELECT COUNT(*) FROM products").await;
    let draft = gayanshop_lib::db::repo::batches::opening_stock_id(t.pool(), &t.s.store_id).await.unwrap();
    let bad = PromoteInput { custom_key: "blue bucket 10l".into(), ..promote_input("x", Some(1), Some(draft)) };
    assert_eq!(
        code(quick_sales::promote(&t.t.db, &t.ctx(), &t.owner(), &bad).await.unwrap_err()),
        "BATCH_NOT_RECEIVED"
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM products").await, products_before, "nothing half-made");
}

#[tokio::test]
async fn ignore_hides_noise_and_unknown_keys_are_refused() {
    let t = till().await;
    history(&t).await;
    quick_sales::ignore(&t.t.db, &t.ctx(), &t.owner(), "plastic basin").await.unwrap();
    let new_only = quick_sales::review(t.pool(), &t.s.store_id, Some(QuickSaleStatus::New)).await.unwrap();
    assert_eq!(new_only.iter().map(|r| r.custom_key.as_str()).collect::<Vec<_>>(), vec!["blue bucket 10l"]);
    let ignored = quick_sales::review(t.pool(), &t.s.store_id, Some(QuickSaleStatus::Ignored)).await.unwrap();
    assert_eq!(ignored.len(), 1);
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'QUICK_SALE_IGNORE'").await, 1);
    // An ignored key can still be promoted later.
    quick_sales::promote(
        &t.t.db,
        &t.ctx(),
        &t.owner(),
        &PromoteInput { name: "Plastic Basin".into(), ..promote_input("plastic basin", None, None) },
    )
    .await
    .unwrap();
    assert!(matches!(
        quick_sales::ignore(&t.t.db, &t.ctx(), &t.owner(), "no such key").await.unwrap_err(),
        AppError::NotFound { .. }
    ));
    assert_eq!(
        code(quick_sales::ignore(&t.t.db, &t.ctx(), &t.owner(), "plastic basin").await.unwrap_err()),
        "QUICK_SALE_ALREADY_PROMOTED"
    );
}

#[tokio::test]
async fn the_till_hints_that_a_typed_name_is_now_a_product() {
    let t = till().await;
    history(&t).await;
    assert!(quick_sales::hint(t.pool(), &t.s.store_id, "Blue Bucket 10L").await.unwrap().is_none());
    let r = quick_sales::promote(&t.t.db, &t.ctx(), &t.owner(), &promote_input("blue bucket 10l", None, None))
        .await
        .unwrap();
    for typed in ["Blue Bucket 10L", "  blue   bucket 10l "] {
        let h = quick_sales::hint(t.pool(), &t.s.store_id, typed).await.unwrap().unwrap();
        assert_eq!((h.product.id.as_str(), h.product.price), (r.product.id.as_str(), 45_000), "{typed}");
    }
    assert!(quick_sales::hint(t.pool(), &t.s.store_id, "Plastic basin").await.unwrap().is_none());
    assert!(quick_sales::hint(t.pool(), &t.s.store_id, "   ").await.unwrap().is_none());
}
