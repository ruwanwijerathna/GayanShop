//! Blocks 1.1 and 2.10 — the §13.4 database scale budget.
//!
//! - The plan assertions always run: on a small dataset, every hot query must use its index and never
//!   `SCAN` products or customers. That catches a missing or unusable index even without timing.
//! - The timed budgets are `#[ignore]` by design (they need an optimised build and a quiet machine):
//!   `cargo test --release --test perf_budgets -- --ignored search`.
//!   They seed the gate dataset (50,000 products, 20,000 customers) and assert p95 per query.
//! - Block 2.10 adds the rest of the table on a first-run store seeded with 100,000 order items (≈ 35k orders
//!   over a year at three counters): checkout, P&L, order history, customer list, quick-sale review, batch
//!   report, stock valuation and the audit page; plus the headroom run at 1,000,000 order items, which must
//!   stay within 2× of every budget. `cargo test --release --test perf_budgets -- --ignored` runs them all.

#[path = "common/mod.rs"]
mod common;
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
    let db = Db::open(&dir.path().join(gayanshop_lib::db::DB_FILE_NAME), &dir.path().join("backups")).await.unwrap();
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
    assert_indexed("barcode", &plan(p, &products::SQL_LOOKUP_BARCODE, &["s", &b]).await, "idx_products_barcode");
    assert_indexed("sku", &plan(p, &products::SQL_LOOKUP_SKU, &["s", &s]).await, "ux_products_sku");
    let latin = plan(p, &products::SQL_SEARCH_LATIN, &["\"blu\"* \"buc\"*", "s", "20"]).await;
    assert_indexed("latin search", &latin, "VIRTUAL TABLE INDEX");
    assert!(latin.iter().any(|d| d.contains("sqlite_autoindex_products_1")), "join by PK: {latin:?}");
    let sinhala = plan(p, &products::SQL_SEARCH_SINHALA, &["\"බාල්දි\"", "s", "20"]).await;
    assert_indexed("sinhala search", &sinhala, "VIRTUAL TABLE INDEX");
    assert!(sinhala.iter().any(|d| d.contains("sqlite_autoindex_products_1")), "join by PK: {sinhala:?}");
    assert_indexed("phone", &plan(p, SQL_PHONE, &["s", &seed::phone(7)]).await, "ux_customers_phone");

    // The seeded data is what the plans assume: lookups and searches return the expected rows.
    assert_eq!(products::lookup_code_matches(p, "s", &b).await.unwrap()[0].id, "p000042");
    assert_eq!(products::lookup_code_matches(p, "s", &s).await.unwrap()[0].id, "p000042");
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
            products::lookup_code_matches(&p, "s", &code).await.unwrap();
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

// ───────── Block 2.10: the rest of the §13.4 table (orders, reports, customers, stock, audit) ─────────

/// One row of the §13.4 table: what was measured, its p95 and the budget.
struct Budget {
    name: &'static str,
    p95: Duration,
    target: Duration,
}

/// No full scan of the big tables; an index-ordered walk (`SCAN x USING INDEX …`) is what keyset paging
/// does and stops at `LIMIT`, so only an un-indexed `SCAN` fails.
fn assert_no_scan(name: &str, details: &[String], big: &[&str], must_use: &str) {
    for d in details {
        if let Some(rest) = d.strip_prefix("SCAN ") {
            let table = rest.split_whitespace().next().unwrap_or("");
            let indexed = rest.contains("USING INDEX") || rest.contains("USING COVERING INDEX");
            assert!(!(big.contains(&table) && !indexed), "{name}: full scan of {table} in plan {details:?}");
        }
    }
    assert!(details.iter().any(|d| d.contains(must_use)), "{name}: expected {must_use} in plan {details:?}");
}

async fn scaled(products: usize, customers: usize, items: usize) -> (common::Till, seed::Scale) {
    let t = common::till().await;
    let scale =
        seed::scale(t.pool(), &t.s.store_id, &t.s.device_id, &t.s.owner_staff_id, products, customers, items).await;
    (t, scale)
}

/// A received 30-item batch (the size of a real container), for the batch-report row.
async fn thirty_item_batch(t: &common::Till) -> String {
    use gayanshop_lib::db::repo::batches::{self, AllocationMethod, BatchInput, BatchItemInput};
    let input = BatchInput {
        name: "Container 30".into(),
        supplier_name: None,
        supplier_ref: None,
        purchased_at: None,
        total_cost: 0,
        extra_costs: 0,
        allocation_method: Some(AllocationMethod::Manual),
        notes: None,
    };
    let b = batches::create(&t.t.db, &t.ctx(), &t.owner(), &input).await.unwrap();
    for i in 0..30 {
        let item =
            BatchItemInput { product_id: format!("sp{:06}", i * 7), qty: 20, unit_cost: 10_000, retail_price: None };
        batches::set_item(&t.t.db, &t.ctx(), &b.batch.id, &item).await.unwrap();
    }
    batches::receive(&t.t.db, &t.ctx(), &t.owner(), &b.batch.id).await.unwrap();
    b.batch.id
}

#[tokio::test]
async fn report_order_stock_and_audit_queries_use_their_indexes() {
    use gayanshop_lib::audit;
    use gayanshop_lib::db::repo::{orders, reports};
    let (t, scale) = scaled(2_000, 500, 3_000).await;
    let p = t.pool();
    assert!(scale.orders > 900 && scale.order_items >= 3_000);
    let plan_of = |sql: &'static str, binds: Vec<String>| {
        let p = p.clone();
        async move {
            let refs: Vec<&str> = binds.iter().map(String::as_str).collect();
            plan(&p, sql, &refs).await
        }
    };
    let (from, to) = (scale.first_day.clone(), scale.last_day.clone());
    let s = t.s.store_id.clone();
    assert_no_scan(
        "order history page",
        &plan_of(
            orders::SQL_HISTORY,
            vec![s.clone(), "".into(), "".into(), "".into(), "".into(), "".into(), "".into(), "".into(), "500".into()],
        )
        .await,
        &["o"],
        "ix_orders_keyset",
    );
    assert_no_scan(
        "audit page",
        &plan_of(audit::SQL_PAGE, vec![String::new(), String::new(), "200".into()]).await,
        &["a"],
        "ix_audit_keyset",
    );
    assert_no_scan(
        "P&L write-offs",
        &plan_of(reports::SQL_WRITE_OFFS, vec![from.clone(), to.clone()]).await,
        &["stock_movements"],
        "ix_mov_writeoffs",
    );
    // A day's expenses come from the date index; over a whole year a scan of a few hundred rows is cheaper,
    // and SQLite rightly picks it.
    assert_no_scan(
        "P&L expenses (day)",
        &plan_of(reports::SQL_OPEX, vec![to.clone(), to.clone()]).await,
        &["expenses"],
        "ix_expenses_bdate",
    );
    let sums = plan_of(reports::SQL_SALES_SUMS, vec![from.clone(), to.clone(), String::new()]).await;
    assert_no_scan("P&L sales", &sums, &["orders", "order_items"], "daily_sales_summary");
    assert!(sums.iter().all(|d| !d.contains("orders")), "P&L reads the rollup only: {sums:?}");
    let pays = plan_of(reports::SQL_PAYMENTS, vec![from.clone(), to.clone(), String::new()]).await;
    assert!(pays.iter().all(|d| !d.contains("order_payments")), "payments from the rollup only: {pays:?}");

    // The seeded history went through the real triggers: the rollups reconcile with the base rows.
    let rollup: i64 =
        sqlx::query_scalar("SELECT SUM(sales_count) FROM daily_sales_summary").fetch_one(p).await.unwrap();
    let completed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM orders WHERE status = 'completed'").fetch_one(p).await.unwrap();
    assert_eq!(rollup, completed);
    let stats: i64 = sqlx::query_scalar("SELECT SUM(visit_count) FROM customer_stats").fetch_one(p).await.unwrap();
    let with_customer: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM orders WHERE status = 'completed' AND customer_id IS NOT NULL")
            .fetch_one(p)
            .await
            .unwrap();
    assert_eq!(stats, with_customer);
    let sold: i64 = sqlx::query_scalar("SELECT SUM(sold_qty) FROM batch_item_stock").fetch_one(p).await.unwrap();
    let raw: i64 = sqlx::query_scalar("SELECT -SUM(qty_delta) FROM stock_movements WHERE reason = 'sale'")
        .fetch_one(p)
        .await
        .unwrap();
    assert_eq!(sold, raw);
    assert!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM quick_sale_stats").fetch_one(p).await.unwrap() > 0);
}

/// Time every §13.4 row that Block 2.10 adds on the seeded store.
async fn measure(t: &common::Till, scale: &seed::Scale, rounds: usize) -> Vec<Budget> {
    use gayanshop_lib::audit;
    use gayanshop_lib::db::repo::checkout::{self, NoFaults};
    use gayanshop_lib::db::repo::customers::{self, CustomerSort};
    use gayanshop_lib::db::repo::orders::{self, OrderFilter};
    use gayanshop_lib::db::repo::{batches, inventory, quick_sales, reports};
    use gayanshop_lib::domain::business_date::StoreCalendar;
    use gayanshop_lib::domain::period::Period;

    let p = t.pool().clone();
    let store = t.s.store_id.clone();
    let cal = StoreCalendar::colombo_default();
    let mut rng = Lcg::new(11);
    let mut out = Vec::new();
    let inputs: Vec<String> = (0..rounds).map(|i| i.to_string()).collect();

    // Checkout: 5 tracked lines, FIFO slices, the SQL seal, audit and outbox, FULL fsync.
    let seller = t.seller(common::CASHIER).await;
    let carts: Vec<String> = (0..rounds.min(200)).map(|i| format!("perf-{i}")).collect();
    let picks: Vec<Vec<usize>> = carts.iter().map(|_| (0..5).map(|_| rng.below(scale.products)).collect()).collect();
    let mut k = 0usize;
    let checkout_p95 = time_all(&carts, |_| {
        // A fresh token every call: a repeated token would replay the first checkout (idempotency).
        let token = format!("perf-checkout-{k}");
        let lines: Vec<_> = picks[k % picks.len()]
            .iter()
            .enumerate()
            .map(|(j, &i)| common::line(&format!("sp{:06}", (i + j) % scale.products), 0, 1))
            .collect();
        k += 1;
        let (db, ctx, seller) = (t.t.db.clone(), t.ctx(), seller.clone());
        async move {
            let mut lines = lines;
            for l in &mut lines {
                let id = l.product_id.clone().unwrap();
                l.unit_price = sqlx::query_scalar("SELECT price FROM products WHERE id = ?1")
                    .bind(&id)
                    .fetch_one(&db.pool)
                    .await
                    .unwrap();
            }
            let total: i64 = lines.iter().map(|l| l.unit_price).sum();
            let input = common::cart(&token, lines, vec![common::cash(total, None)]);
            checkout::checkout(&db, &ctx, &seller, &input, None, &NoFaults).await.unwrap();
        }
    })
    .await;
    out.push(Budget {
        name: "checkout (5 lines, FIFO, seal, FULL fsync)",
        p95: checkout_p95,
        target: Duration::from_millis(40),
    });

    // P&L for a day, a week, a month and a year (rollups + write-offs + expenses + variance).
    for (name, period) in [
        ("P&L day", Period::Day { date: scale.last_day.clone() }),
        ("P&L week", Period::Week { any_date: scale.last_day.clone() }),
        ("P&L month", Period::Month { year: 2026, month: 10 }),
        ("P&L year", Period::Year { year: 2026 }),
    ] {
        let range = period.resolve(&cal).unwrap();
        let p95 = time_all(&inputs, |_| {
            let (p, range) = (p.clone(), range.clone());
            async move {
                reports::summary_for(&p, &range, None).await.unwrap();
            }
        })
        .await;
        out.push(Budget { name, p95, target: Duration::from_millis(20) });
    }

    // Order history: the newest page and a page deep in the year (500 rows, keyset).
    let all = OrderFilter {
        period: Period::AllTime,
        device_id: None,
        status: None,
        receipt: None,
        phone: None,
        payment: None,
        kind: None,
    };
    let first = orders::history(&p, &store, &cal, &all, None, 500).await.unwrap();
    let mut deep = first.next_cursor.clone();
    for _ in 0..20 {
        deep = orders::history(&p, &store, &cal, &all, deep.as_ref(), 500).await.unwrap().next_cursor.or(deep);
    }
    let cursors = [None, deep];
    let mut c = 0usize;
    let history_p95 = time_all(&inputs, |_| {
        let cursor = cursors[c % 2].clone();
        c += 1;
        let (p, store, cal, all) = (p.clone(), store.clone(), cal.clone(), all.clone());
        async move {
            let page = orders::history(&p, &store, &cal, &all, cursor.as_ref(), 500).await.unwrap();
            assert_eq!(page.rows.len(), 500);
        }
    })
    .await;
    out.push(Budget { name: "order history page (500, keyset)", p95: history_p95, target: Duration::from_millis(20) });

    let customers_p95 = time_all(&inputs, |_| {
        let (p, store) = (p.clone(), store.clone());
        async move {
            let l = customers::list(&p, &store, None, CustomerSort::Spend, None).await.unwrap();
            assert_eq!(l.rows.len(), 500);
        }
    })
    .await;
    out.push(Budget { name: "customer list by spend", p95: customers_p95, target: Duration::from_millis(20) });

    let quick_p95 = time_all(&inputs, |_| {
        let (p, store) = (p.clone(), store.clone());
        async move {
            assert!(!quick_sales::review(&p, &store, None).await.unwrap().is_empty());
        }
    })
    .await;
    out.push(Budget { name: "quick-sale review by frequency", p95: quick_p95, target: Duration::from_millis(20) });

    let batch = thirty_item_batch(t).await;
    let batch_p95 = time_all(&inputs, |_| {
        let (p, store, batch) = (p.clone(), store.clone(), batch.clone());
        async move {
            let d = batches::detail(&p, &store, &batch).await.unwrap();
            assert_eq!(d.items.len(), 30);
        }
    })
    .await;
    out.push(Budget {
        name: "batch report / valuation (30-item batch)",
        p95: batch_p95,
        target: Duration::from_millis(30),
    });
    let valuation_p95 = time_all(&inputs, |_| {
        let (p, store) = (p.clone(), store.clone());
        async move {
            inventory::valuation(&p, &store, None).await.unwrap();
        }
    })
    .await;
    // Not a §13.4 row: the whole store's valuation (every layer, by batch and category) is an on-demand
    // report (doc §5.3). The bound guards against regressions such as shipping every layer to Rust.
    out.push(Budget {
        name: "store-wide valuation (on demand, guard)",
        p95: valuation_p95,
        target: Duration::from_millis(250),
    });

    let audit_p95 = time_all(&inputs, |_| {
        let p = p.clone();
        async move {
            assert_eq!(audit::page(&p, None, 200).await.unwrap().rows.len(), 200);
        }
    })
    .await;
    out.push(Budget { name: "audit log page (200)", p95: audit_p95, target: Duration::from_millis(20) });
    out
}

fn report(title: &str, scale: &seed::Scale, rows: &[Budget], factor: u32) {
    println!(
        "§13.4 {title}: {} products, {} customers, {} orders, {} order items (budget × {factor})",
        scale.products, scale.customers, scale.orders, scale.order_items
    );
    for r in rows {
        println!("  {:<46} p95 {:>9.3?}   budget {:?}", r.name, r.p95, r.target * factor);
    }
    for r in rows {
        assert!(r.p95 < r.target * factor, "{}: p95 {:?} ≥ {:?}", r.name, r.p95, r.target * factor);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "timed budget: run with --release -- --ignored"]
async fn order_report_stock_and_audit_budgets() {
    let (t, scale) = scaled(seed::GATE_PRODUCTS, seed::GATE_CUSTOMERS, seed::GATE_ORDER_ITEMS).await;
    assert!(scale.order_items >= seed::GATE_ORDER_ITEMS);
    let rows = measure(&t, &scale, 200).await;
    report("gate", &scale, &rows, 1);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "headroom run (1,000,000 order items): run with --release -- --ignored"]
async fn headroom_at_one_million_order_items() {
    let (t, scale) = scaled(seed::GATE_PRODUCTS, seed::GATE_CUSTOMERS, seed::HEADROOM_ORDER_ITEMS).await;
    assert!(scale.order_items >= seed::HEADROOM_ORDER_ITEMS);
    let rows = measure(&t, &scale, 100).await;
    report("headroom", &scale, &rows, 2);
}
