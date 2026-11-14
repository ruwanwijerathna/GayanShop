//! Block 1.1 — catalog behaviour: products CRUD with audit + outbox, in-store code generation, validation
//! failures, cost masking, the scanner lookup, Latin type-ahead search, and categories.

mod common;

use std::collections::BTreeMap;

use common::{BD, NOW, exec, open, scalar_i64};
use gayanshop_lib::auth::guard::Actor;
use gayanshop_lib::db::repo::WriteCtx;
use gayanshop_lib::db::repo::categories::{self, CategoryInput};
use gayanshop_lib::db::repo::products::{self, CodeChoice, ProductInput, ProductKind};
use gayanshop_lib::domain::receipt_no::is_valid_ean13;
use gayanshop_lib::error::AppError;
use serde_json::Value;
use sqlx::SqlitePool;

fn ctx() -> WriteCtx {
    WriteCtx {
        store_id: "s".into(),
        device_id: "dev1".into(),
        device_code: "T01".into(),
        now: NOW.into(),
        business_date: BD.into(),
    }
}

fn owner() -> Actor {
    Actor { staff_id: "owner".into(), approved_by: None }
}

fn input(name: &str, price: i64) -> ProductInput {
    ProductInput {
        kind: ProductKind::Standard,
        name: name.into(),
        name_si: None,
        sku: CodeChoice::None,
        barcode: CodeChoice::None,
        category_id: None,
        default_cost: Some(20_000),
        price,
        min_price: None,
        track_stock: true,
        allow_negative_stock: true,
        attributes: BTreeMap::new(),
        is_active: true,
    }
}

fn manual(v: &str) -> CodeChoice {
    CodeChoice::Manual { value: v.into() }
}

fn validation_code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

async fn audit_rows(pool: &SqlitePool, action: &str) -> Vec<Value> {
    let rows: Vec<String> = sqlx::query_scalar("SELECT metadata FROM audit_logs WHERE action = ?1 ORDER BY device_seq")
        .bind(action)
        .fetch_all(pool)
        .await
        .unwrap();
    rows.iter().map(|m| serde_json::from_str(m).unwrap()).collect()
}

#[tokio::test]
async fn generated_codes_are_device_prefixed_sequential_and_valid() {
    let t = open().await;
    let mut a = input("Blue Bucket 10L", 45_000);
    a.sku = CodeChoice::Generate;
    a.barcode = CodeChoice::Generate;
    let a = products::create(&t.db, &ctx(), &owner(), &a, true).await.unwrap();
    assert_eq!(a.sku.as_deref(), Some("100001"));
    let a_barcode = a.barcode.clone().unwrap();
    assert!(a_barcode.starts_with("210000000001") && is_valid_ean13(&a_barcode), "{a_barcode}");

    let mut b = input("Red Bucket 10L", 45_000);
    b.sku = CodeChoice::Generate;
    let b = products::create(&t.db, &ctx(), &owner(), &b, true).await.unwrap();
    assert_eq!((b.sku.as_deref(), b.barcode.as_deref()), (Some("100002"), None));

    // A hand-typed code that happens to be the next number is skipped, never duplicated.
    let mut c = input("Green Bucket", 45_000);
    c.sku = manual("100003");
    products::create(&t.db, &ctx(), &owner(), &c, true).await.unwrap();
    let mut d = input("Yellow Bucket", 45_000);
    d.sku = CodeChoice::Generate;
    assert_eq!(products::create(&t.db, &ctx(), &owner(), &d, true).await.unwrap().sku.as_deref(), Some("100004"));

    // Losing the counter (reinstall/restore) never reissues a code already on a product.
    exec(t.pool(), "DELETE FROM sku_counters").await;
    let mut e = input("Black Bucket", 45_000);
    e.sku = CodeChoice::Generate;
    e.barcode = CodeChoice::Generate;
    let e = products::create(&t.db, &ctx(), &owner(), &e, true).await.unwrap();
    assert_eq!(e.sku.as_deref(), Some("100005"));
    assert!(e.barcode.unwrap().starts_with("210000000005"));

    // Another counter uses its own digit (T02 → 2xxxxx), so offline counters never collide.
    let mut t02 = ctx();
    t02.device_code = "T02".into();
    let mut f = input("White Bucket", 45_000);
    f.sku = CodeChoice::Generate;
    assert_eq!(products::create(&t.db, &t02, &owner(), &f, true).await.unwrap().sku.as_deref(), Some("200001"));
}

#[tokio::test]
async fn invalid_input_is_rejected_with_specific_codes() {
    let t = open().await;
    let mut base = input("Blue Bucket", 45_000);
    base.sku = manual("bb-10");
    base.barcode = manual("4792024015103");
    let first = products::create(&t.db, &ctx(), &owner(), &base, true).await.unwrap();
    assert_eq!(first.sku.as_deref(), Some("BB-10"), "SKUs are stored upper-case");

    let cases: Vec<(ProductInput, &str)> = vec![
        (input("  ", 100), "PRODUCT_NAME_REQUIRED"),
        (input(&"x".repeat(81), 100), "PRODUCT_NAME_TOO_LONG"),
        (input("සබන්", 100), "PRODUCT_NAME_NOT_PRINTABLE"),
        (input("Soap", -1), "PRICE_INVALID"),
        (ProductInput { min_price: Some(200), ..input("Soap", 100) }, "MIN_PRICE_INVALID"),
        (ProductInput { default_cost: Some(-5), ..input("Soap", 100) }, "COST_INVALID"),
        (ProductInput { sku: manual("BB-10"), ..input("Soap", 100) }, "SKU_TAKEN"),
        (ProductInput { sku: manual("a b"), ..input("Soap", 100) }, "SKU_INVALID"),
        (ProductInput { barcode: manual("4792024015108"), ..input("Soap", 100) }, "BARCODE_CHECK_DIGIT"),
        (ProductInput { barcode: manual("12"), ..input("Soap", 100) }, "BARCODE_INVALID"),
        (ProductInput { category_id: Some("nope".into()), ..input("Soap", 100) }, "CATEGORY_INVALID"),
        (
            ProductInput { attributes: BTreeMap::from([("size".into(), " ".into())]), ..input("Soap", 100) },
            "ATTRIBUTES_INVALID",
        ),
    ];
    for (bad, code) in cases {
        let err = products::create(&t.db, &ctx(), &owner(), &bad, true).await.unwrap_err();
        assert_eq!(validation_code(err), code);
    }
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM products").await, 1, "nothing partial was written");
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM audit_logs").await, 1);

    // Variants may share a manufacturer barcode: active and inactive products alike (SKUs stay unique).
    let twin = ProductInput { barcode: manual("4792024015103"), ..input("Blue Bucket Lime", 46_000) };
    let twin = products::create(&t.db, &ctx(), &owner(), &twin, true).await.unwrap();
    assert_eq!(twin.barcode.as_deref(), Some("4792024015103"));
    let parked = ProductInput { barcode: manual("4792024015103"), is_active: false, ..input("Old Bucket", 100) };
    let parked = products::create(&t.db, &ctx(), &owner(), &parked, true).await.unwrap();
    let reactivate = ProductInput { barcode: manual("4792024015103"), ..input("Old Bucket", 100) };
    assert!(products::update(&t.db, &ctx(), &owner(), &parked.id, &reactivate, true).await.unwrap().is_active);
}

#[tokio::test]
async fn generated_in_store_barcodes_skip_ones_already_in_use() {
    let t = open().await;
    let mut a = input("Blue Bucket", 45_000);
    a.barcode = CodeChoice::Generate;
    let a = products::create(&t.db, &ctx(), &owner(), &a, true).await.unwrap();
    // Someone typed in the barcode the next sequence number would produce.
    let digit = gayanshop_lib::domain::receipt_no::device_digit("T01").unwrap();
    let taken = gayanshop_lib::domain::receipt_no::in_store_ean13(digit, 2).unwrap();
    let squatter = ProductInput { barcode: manual(&taken), ..input("Squatter", 100) };
    products::create(&t.db, &ctx(), &owner(), &squatter, true).await.unwrap();
    let mut b = input("Red Bucket", 45_000);
    b.barcode = CodeChoice::Generate;
    let b = products::create(&t.db, &ctx(), &owner(), &b, true).await.unwrap();
    assert_ne!(b.barcode.as_deref(), Some(taken.as_str()), "skipped the code in use");
    assert_ne!(a.barcode, b.barcode);
}

#[tokio::test]
async fn writes_are_audited_and_queued_for_sync() {
    let t = open().await;
    let p = t.pool();
    let created = products::create(&t.db, &ctx(), &owner(), &input("Blue Bucket", 45_000), true).await.unwrap();
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'products'").await, 1);
    let create_meta = audit_rows(p, "PRODUCT_CREATE").await;
    assert_eq!(create_meta.len(), 1);
    assert_eq!(create_meta[0]["price"], 45_000);
    assert_eq!(create_meta[0]["default_cost"], 20_000);

    let changed = ProductInput { price: 47_500, default_cost: Some(21_000), ..input("Blue Bucket", 0) };
    let updated = products::update(&t.db, &ctx(), &owner(), &created.id, &changed, true).await.unwrap();
    assert_eq!((updated.price, updated.version), (47_500, 2));
    let upd = audit_rows(p, "PRODUCT_UPDATE").await;
    assert_eq!(upd.len(), 1);
    assert_eq!(upd[0]["changes"]["price"], serde_json::json!([45_000, 47_500]), "price carries old → new");
    assert_eq!(upd[0]["changes"]["default_cost"], serde_json::json!([20_000, 21_000]), "cost carries old → new");
    assert!(upd[0]["changes"].get("name").is_none(), "only changed fields are listed");

    // Saving without changes writes no audit row and no outbox row.
    products::update(&t.db, &ctx(), &owner(), &created.id, &changed, true).await.unwrap();
    assert_eq!(audit_rows(p, "PRODUCT_UPDATE").await.len(), 1);
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'products'").await, 2);

    products::delete(&t.db, &ctx(), &owner(), &created.id).await.unwrap();
    assert_eq!(audit_rows(p, "PRODUCT_DELETE").await[0]["name"], "Blue Bucket");
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'products'").await, 3);
    assert!(products::list(p, "s", true).await.unwrap().is_empty());
    assert!(products::search(p, "s", "blue", 20).await.unwrap().is_empty());
    assert_eq!(
        products::get(p, "s", &created.id, true).await.unwrap_err(),
        AppError::not_found("product"),
        "deleted products are gone from reads"
    );
    // The audit chain is intact after the mixed writes.
    let chains = gayanshop_lib::audit::verify(p).await.unwrap();
    assert!(chains.iter().all(|c| c.ok));
}

#[tokio::test]
async fn cost_is_hidden_and_immutable_without_view_cost() {
    let t = open().await;
    let p = t.pool();
    // A creator without VIEW_COST can't set a cost: it is stored as 0 and returned masked.
    let created = products::create(&t.db, &ctx(), &owner(), &input("Mug", 25_000), false).await.unwrap();
    assert_eq!(created.default_cost, None);
    assert_eq!(products::get(p, "s", &created.id, true).await.unwrap().default_cost, Some(0));
    assert!(audit_rows(p, "PRODUCT_CREATE").await[0].get("default_cost").is_none());

    exec(p, &format!("UPDATE products SET default_cost = 12000 WHERE id = '{}'", created.id)).await;
    let attempt = ProductInput { default_cost: Some(1), price: 26_000, ..input("Mug", 0) };
    products::update(&t.db, &ctx(), &owner(), &created.id, &attempt, false).await.unwrap();
    assert_eq!(products::get(p, "s", &created.id, true).await.unwrap().default_cost, Some(12_000));
    let upd = audit_rows(p, "PRODUCT_UPDATE").await;
    assert!(upd[0]["changes"].get("default_cost").is_none());
    assert!(products::list(p, "s", false).await.unwrap().iter().all(|x| x.default_cost.is_none()));
}

#[tokio::test]
async fn scanner_lookup_prefers_barcode_then_sku_and_ignores_inactive() {
    let t = open().await;
    let p = t.pool();
    let a = ProductInput { sku: manual("100200"), barcode: manual("4792024015103"), ..input("Blue Bucket", 45_000) };
    let a = products::create(&t.db, &ctx(), &owner(), &a, true).await.unwrap();
    // A product whose *barcode* equals another product's SKU wins the scan (barcode first, doc §7.4).
    let b = ProductInput { barcode: manual("100200"), ..input("Odd Label", 1_000) };
    let b = products::create(&t.db, &ctx(), &owner(), &b, true).await.unwrap();

    let ids = |hits: Vec<_>| {
        hits.into_iter().map(|h: gayanshop_lib::db::repo::products::ProductSummary| h.id).collect::<Vec<_>>()
    };
    assert_eq!(ids(products::lookup_code_matches(p, "s", "4792024015103").await.unwrap()), vec![a.id.clone()]);
    assert_eq!(ids(products::lookup_code_matches(p, "s", " 100200 ").await.unwrap()), vec![b.id.clone()]);
    assert!(products::lookup_code_matches(p, "s", "999999").await.unwrap().is_empty());
    assert!(products::lookup_code_matches(p, "s", "").await.unwrap().is_empty());

    exec(p, &format!("UPDATE products SET is_active = 0 WHERE id = '{}'", b.id)).await;
    assert_eq!(
        ids(products::lookup_code_matches(p, "s", "100200").await.unwrap()),
        vec![a.id.clone()],
        "falls back to SKU"
    );
    exec(p, &format!("UPDATE products SET is_active = 0 WHERE id = '{}'", a.id)).await;
    assert!(products::lookup_code_matches(p, "s", "100200").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_shared_barcode_returns_every_active_variant_by_name() {
    let t = open().await;
    let p = t.pool();
    let shared = "4792024015103";
    let mut made = Vec::new();
    for name in ["Lime Soap", "apple Soap", "Mint Soap"] {
        let v = ProductInput { barcode: manual(shared), ..input(name, 10_000) };
        made.push(products::create(&t.db, &ctx(), &owner(), &v, true).await.unwrap());
    }
    let names = |hits: Vec<gayanshop_lib::db::repo::products::ProductSummary>| {
        hits.into_iter().map(|h| h.name).collect::<Vec<_>>()
    };
    assert_eq!(
        names(products::lookup_code_matches(p, "s", shared).await.unwrap()),
        ["apple Soap", "Lime Soap", "Mint Soap"],
        "all variants, case-insensitive name order"
    );
    // Deactivating one drops it from the scan; deleting another too.
    exec(p, &format!("UPDATE products SET is_active = 0 WHERE id = '{}'", made[0].id)).await;
    assert_eq!(products::lookup_code_matches(p, "s", shared).await.unwrap().len(), 2);
    // The till's type-ahead puts every exact-code match first.
    let hits = products::search(p, "s", shared, 20).await.unwrap();
    assert_eq!(hits.iter().take(2).map(|h| h.name.as_str()).collect::<Vec<_>>(), ["apple Soap", "Mint Soap"]);
}

#[tokio::test]
async fn latin_type_ahead_search() {
    let t = open().await;
    let p = t.pool();
    let bucket = ProductInput { sku: manual("104523"), ..input("Blue Bucket 10L", 45_000) };
    let bucket = products::create(&t.db, &ctx(), &owner(), &bucket, true).await.unwrap();
    products::create(&t.db, &ctx(), &owner(), &input("Black Sandal", 150_000), true).await.unwrap();
    products::create(&t.db, &ctx(), &owner(), &input("Blue Basin", 30_000), true).await.unwrap();

    let names = |hits: Vec<products::ProductSummary>| hits.into_iter().map(|h| h.name).collect::<Vec<_>>();
    assert_eq!(names(products::search(p, "s", "blu buc", 20).await.unwrap()), vec!["Blue Bucket 10L"]);
    assert_eq!(products::search(p, "s", "BL", 20).await.unwrap().len(), 3, "case-insensitive prefix");
    assert_eq!(products::search(p, "s", "bl", 2).await.unwrap().len(), 2, "limit is honoured");
    // An exact code is listed first, even when it also matches by prefix.
    assert_eq!(products::search(p, "s", "104523", 20).await.unwrap()[0].id, bucket.id);
    // FTS syntax in the input is neutralised, not executed or rejected.
    for raw in ["\"", "blue OR", "name:blue", "*", "NEAR(", "bl* AND -x"] {
        products::search(p, "s", raw, 20).await.unwrap();
    }
    assert!(products::search(p, "s", "   ", 20).await.unwrap().is_empty());
    // Renames are re-indexed by the FTS triggers.
    let renamed = ProductInput { sku: manual("104523"), ..input("Navy Pail 10L", 45_000) };
    products::update(&t.db, &ctx(), &owner(), &bucket.id, &renamed, true).await.unwrap();
    assert!(products::search(p, "s", "bucket", 20).await.unwrap().is_empty());
    assert_eq!(names(products::search(p, "s", "pail", 20).await.unwrap()), vec!["Navy Pail 10L"]);
}

#[tokio::test]
async fn categories_lifecycle() {
    let t = open().await;
    let p = t.pool();
    let cat = |name: &str, parent: Option<&str>| CategoryInput { name: name.into(), parent_id: parent.map(Into::into) };
    let home = categories::create(&t.db, &ctx(), &cat("Home", None)).await.unwrap();
    let kitchen = categories::create(&t.db, &ctx(), &cat(" Kitchen ", Some(&home.id))).await.unwrap();
    assert_eq!(kitchen.name, "Kitchen");

    let err = categories::create(&t.db, &ctx(), &cat("kitchen", Some(&home.id))).await.unwrap_err();
    assert_eq!(validation_code(err), "CATEGORY_NAME_TAKEN");
    // The same name under a different parent is fine.
    categories::create(&t.db, &ctx(), &cat("Kitchen", None)).await.unwrap();
    let err = categories::create(&t.db, &ctx(), &cat("", None)).await.unwrap_err();
    assert_eq!(validation_code(err), "CATEGORY_NAME_INVALID");
    let err = categories::update(&t.db, &ctx(), &home.id, &cat("Home", Some(&kitchen.id))).await.unwrap_err();
    assert_eq!(validation_code(err), "CATEGORY_CYCLE");
    let err = categories::update(&t.db, &ctx(), &home.id, &cat("Home", Some(&home.id))).await.unwrap_err();
    assert_eq!(validation_code(err), "CATEGORY_CYCLE");

    let product = ProductInput { category_id: Some(kitchen.id.clone()), ..input("Ladle", 9_000) };
    let product = products::create(&t.db, &ctx(), &owner(), &product, true).await.unwrap();
    let listed = categories::list(p, "s").await.unwrap();
    assert_eq!(listed.iter().find(|c| c.id == kitchen.id).unwrap().product_count, 1);

    for (id, why) in [(&kitchen.id, "has a product"), (&home.id, "has a sub-category")] {
        let err = categories::delete(&t.db, &ctx(), id).await.unwrap_err();
        assert_eq!(validation_code(err), "CATEGORY_IN_USE", "{why}");
    }
    products::delete(&t.db, &ctx(), &owner(), &product.id).await.unwrap();
    categories::delete(&t.db, &ctx(), &kitchen.id).await.unwrap();
    categories::delete(&t.db, &ctx(), &home.id).await.unwrap();
    assert_eq!(categories::list(p, "s").await.unwrap().len(), 1);
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'categories'").await, 5);
    let err = categories::update(&t.db, &ctx(), &home.id, &cat("Home", None)).await.unwrap_err();
    assert_eq!(err, AppError::not_found("category"));
}
