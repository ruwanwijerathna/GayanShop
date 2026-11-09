//! Block 1.1 — §13.4 search and lookup budgets.
//!
//! - The plan assertions always run: on a small dataset, every hot query must use its index and never
//!   `SCAN` products or customers. That catches a missing or unusable index even without timing.
//! - The timed budgets are `#[ignore]` by design (they need an optimised build and a quiet machine):
//!   `cargo test --release --test perf_budgets -- --ignored search`.
//!   They seed the gate dataset (50,000 products, 20,000 customers) and assert p95 per query.

#[path = "perf/seed.rs"]
mod seed;

use std::time::{Duration, Instant};

use gayanshop_lib::db::Db;
use gayanshop_lib::db::repo::products;
use seed::Lcg;
use sqlx::SqlitePool;
use tempfile::TempDir;

/// Phone → customer (the `customer_lookup` repo arrives in Block 1.6; this is the indexed query it uses).
const SQL_PHONE: &str = "SELECT id, display_name FROM customers
     WHERE store_id = ?1 AND phone_e164 = ?2 AND deleted_at IS NULL";

async fn open_seeded(products: usize, customers: usize) -> (TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("pos.db"), &dir.path().join("backups")).await.unwrap();
    seed::products(&db.pool, products).await;
    seed::customers(&db.pool, customers).await;
    (dir, db)
}

/// `EXPLAIN QUERY PLAN` details for `sql` (parameters bound to plausible values).
async fn plan(pool: &SqlitePool, sql: &str, binds: &[&str]) -> Vec<String> {
    let explain = format!("EXPLAIN QUERY PLAN {sql}");
    let mut q = sqlx::query_as::<_, (i64, i64, i64, String)>(&explain);
    for b in binds {
        q = q.bind(*b);
    }
    q.fetch_all(pool).await.unwrap().into_iter().map(|(_, _, _, detail)| detail).collect()
}

/// No full scan of a real table; virtual-table (FTS) access is reported as `SCAN f VIRTUAL TABLE INDEX …`
/// and is index-driven by the MATCH.
fn assert_indexed(name: &str, details: &[String], must_use: &str) {
    for d in details {
        let full_scan = d.starts_with("SCAN") && !d.contains("VIRTUAL TABLE");
        assert!(!full_scan, "{name}: full table scan in plan {details:?}");
    }
    assert!(details.iter().any(|d| d.contains(must_use)), "{name}: expected {must_use} in plan {details:?}");
}

#[tokio::test]
async fn hot_queries_use_their_indexes() {
    let (_dir, db) = open_seeded(2_000, 1_000).await;
    let p = &db.pool;
    let b = seed::barcode(42);
    let s = seed::sku(42);
    assert_indexed("barcode", &plan(p, &products::SQL_LOOKUP_BARCODE, &["s", &b]).await, "ux_products_barcode");
    assert_indexed("sku", &plan(p, &products::SQL_LOOKUP_SKU, &["s", &s]).await, "ux_products_sku");
    let latin = plan(p, &products::SQL_SEARCH_LATIN, &["\"blu\"* \"buc\"*", "s", "20"]).await;
    assert_indexed("latin search", &latin, "VIRTUAL TABLE INDEX");
    assert!(latin.iter().any(|d| d.contains("sqlite_autoindex_products_1")), "join by PK: {latin:?}");
    let sinhala = plan(p, &products::SQL_SEARCH_SINHALA, &["\"බාල්දි\"", "s", "20"]).await;
    assert_indexed("sinhala search", &sinhala, "VIRTUAL TABLE INDEX");
    assert!(sinhala.iter().any(|d| d.contains("sqlite_autoindex_products_1")), "join by PK: {sinhala:?}");
    assert_indexed("phone", &plan(p, SQL_PHONE, &["s", &seed::phone(7)]).await, "ux_customers_phone");

    // The seeded data is what the plans assume: lookups and searches return the expected rows.
    assert_eq!(products::lookup_code(p, "s", &b).await.unwrap().unwrap().id, "p000042");
    assert_eq!(products::lookup_code(p, "s", &s).await.unwrap().unwrap().id, "p000042");
    assert!(!products::search(p, "s", "blu buc", 20).await.unwrap().is_empty());
    assert!(!products::search(p, "s", "බාල්දි", 20).await.unwrap().is_empty());
}

fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    let idx = (samples.len() * 95).div_ceil(100).saturating_sub(1);
    samples[idx]
}

async fn time_all<F, Fut>(inputs: &[String], mut run: F) -> Duration
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    for q in inputs.iter().take(20) {
        run(q.clone()).await; // warm the page cache and statement cache
    }
    let mut samples = Vec::with_capacity(inputs.len());
    for q in inputs {
        let start = Instant::now();
        run(q.clone()).await;
        samples.push(start.elapsed());
    }
    p95(samples)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "timed budget: run with --release -- --ignored search"]
async fn search_and_lookup_budgets() {
    let (_dir, db) = open_seeded(seed::GATE_PRODUCTS, seed::GATE_CUSTOMERS).await;
    let p = db.pool.clone();
    let mut rng = Lcg::new(7);

    // Barcode scan → product (barcode hit), and SKU scan (barcode miss, then SKU hit).
    let barcodes: Vec<String> = (0..1_000).map(|_| seed::barcode(rng.below(seed::GATE_PRODUCTS))).collect();
    let skus: Vec<String> = (0..1_000).map(|_| seed::sku(rng.below(seed::GATE_PRODUCTS))).collect();
    let scan = |code: String| {
        let p = p.clone();
        async move {
            products::lookup_code(&p, "s", &code).await.unwrap();
        }
    };
    let barcode_p95 = time_all(&barcodes, scan).await;
    let sku_p95 = time_all(&skus, scan).await;

    // Type-ahead "blu* buc*"-style: two 3-letter prefixes, LIMIT 20.
    let mut latin: Vec<String> = vec!["blu buc".into()];
    latin.extend((0..300).map(|_| {
        let a = &seed::ADJECTIVES[rng.below(20)][..3];
        let n = &seed::NOUNS[rng.below(40)][..3];
        format!("{a} {n}")
    }));
    let latin_p95 = time_all(&latin, |q| {
        let p = p.clone();
        async move {
            products::search(&p, "s", &q, 20).await.unwrap();
        }
    })
    .await;

    // Sinhala substring (trigram), 3–4 characters from inside a word.
    let sinhala: Vec<String> = (0..300)
        .map(|_| {
            let word: Vec<char> = seed::SI_NOUNS[rng.below(20)].chars().collect();
            let len = 3 + rng.below(2).min(word.len().saturating_sub(3));
            let start = rng.below(word.len() - len + 1);
            word[start..start + len].iter().collect()
        })
        .collect();
    let sinhala_p95 = time_all(&sinhala, |q| {
        let p = p.clone();
        async move {
            products::search(&p, "s", &q, 20).await.unwrap();
        }
    })
    .await;

    // Phone → customer.
    let phones: Vec<String> = (0..1_000).map(|_| seed::phone(rng.below(seed::GATE_CUSTOMERS))).collect();
    let phone_p95 = time_all(&phones, |ph| {
        let p = p.clone();
        async move {
            let row: Option<(String, Option<String>)> =
                sqlx::query_as(SQL_PHONE).bind("s").bind(&ph).fetch_optional(&p).await.unwrap();
            assert!(row.is_some());
        }
    })
    .await;

    println!(
        "§13.4 p95 @ {} products / {} customers: barcode {barcode_p95:?}, sku {sku_p95:?}, latin {latin_p95:?}, \
         sinhala {sinhala_p95:?}, phone {phone_p95:?}",
        seed::GATE_PRODUCTS,
        seed::GATE_CUSTOMERS
    );
    assert!(barcode_p95 < Duration::from_millis(2), "barcode scan p95 {barcode_p95:?} ≥ 2 ms");
    assert!(sku_p95 < Duration::from_millis(2), "SKU scan p95 {sku_p95:?} ≥ 2 ms");
    assert!(latin_p95 < Duration::from_millis(15), "type-ahead p95 {latin_p95:?} ≥ 15 ms");
    assert!(sinhala_p95 < Duration::from_millis(25), "Sinhala search p95 {sinhala_p95:?} ≥ 25 ms");
    assert!(phone_p95 < Duration::from_millis(2), "phone lookup p95 {phone_p95:?} ≥ 2 ms");
}
