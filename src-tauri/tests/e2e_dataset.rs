//! Block 2.10 — the data folder the Phase 2 e2e suite opens (tests/e2e/phase2.e2e.ts): a first-run store
//! (owner "Gayan", PIN 246810, counter T01) holding the §13.4 gate dataset — 50,000 products and 100,000 order
//! items over a year at three counters — so the real WebView2 scrolls the Products and Orders tables at scale.
//! The e2e config runs it before the suite:
//! `GAYANSHOP_E2E_SCALE_DIR=<folder> cargo test --release --test e2e_dataset -- --ignored`.

#[path = "perf/seed.rs"]
mod seed;

use std::path::PathBuf;

use gayanshop_lib::auth::pin::PinHasher;
use gayanshop_lib::clock::ManualClock;
use gayanshop_lib::db::Db;
use gayanshop_lib::setup::{SetupInput, first_run};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "writes the e2e scale data folder: set GAYANSHOP_E2E_SCALE_DIR, run with --release -- --ignored"]
async fn write_the_phase2_scale_dataset() {
    let dir = PathBuf::from(std::env::var("GAYANSHOP_E2E_SCALE_DIR").expect("GAYANSHOP_E2E_SCALE_DIR is not set"));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join(gayanshop_lib::db::DB_FILE_NAME);
    assert!(!db_path.exists(), "refusing to overwrite {}", db_path.display());
    let db = Db::open(&db_path, &dir.join("backups")).await.unwrap();
    // Set up before the year of history starts, so every seeded sale is after the store existed.
    let at = chrono::DateTime::parse_from_rfc3339("2025-11-01T03:00:00Z").unwrap().with_timezone(&chrono::Utc);
    let input = SetupInput {
        store_name: "GayanShop".into(),
        owner_name: "Gayan".into(),
        owner_pin: "246810".into(),
        device_code: "T01".into(),
        ui_language: "en".into(),
    };
    let s = first_run(&db, &PinHasher::fast_for_tests(), &ManualClock::new(at), &input).await.unwrap();
    let scale = seed::scale(
        &db.pool,
        &s.store_id,
        &s.device_id,
        &s.owner_staff_id,
        seed::GATE_PRODUCTS,
        seed::GATE_CUSTOMERS,
        seed::GATE_ORDER_ITEMS,
    )
    .await;
    let orders: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders").fetch_one(&db.pool).await.unwrap();
    let products: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products").fetch_one(&db.pool).await.unwrap();
    assert_eq!(usize::try_from(orders).unwrap(), scale.orders);
    assert_eq!(usize::try_from(products).unwrap(), seed::GATE_PRODUCTS);
    assert!(scale.order_items >= seed::GATE_ORDER_ITEMS && scale.orders > 30_000);
    // A clean, checkpointed file: the app opens it in WAL mode like any other day.
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&db.pool).await.unwrap();
    db.pool.close().await;
    println!(
        "e2e dataset: {} products, {} orders, {} order items → {}",
        products,
        orders,
        scale.order_items,
        dir.display()
    );
}
