//! Deterministic benchmark dataset (doc §13.4). Block 1.1 seeds what the search and lookup rows need:
//! products (with Sinhala names, SKUs, EAN-13 barcodes) and customers. Block 2.10 adds the 100,000 order
//! items for the order, report and stock rows ([`scale`]).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use gayanshop_lib::domain::receipt_no::ean13_check_digit;
use sqlx::SqlitePool;

pub const GATE_PRODUCTS: usize = 50_000;
pub const GATE_CUSTOMERS: usize = 20_000;
pub const GATE_ORDER_ITEMS: usize = 100_000;
pub const HEADROOM_ORDER_ITEMS: usize = 1_000_000;
const CHUNK: usize = 500;
const META: &str = "'2026-11-08T09:00:00.000Z', '2026-11-08T09:00:00.000Z', 'dev1'";

pub const ADJECTIVES: [&str; 20] = [
    "Blue", "Red", "Green", "Black", "White", "Yellow", "Pink", "Navy", "Grey", "Brown", "Large", "Small", "Kids",
    "Ladies", "Mens", "Plastic", "Steel", "Glass", "Cotton", "Rubber",
];
pub const NOUNS: [&str; 40] = [
    "Bucket", "Basin", "Cup", "Plate", "Spoon", "Bowl", "Jug", "Bottle", "Towel", "Sheet", "Pillow", "Mat", "Bag",
    "Wallet", "Belt", "Sandal", "Slipper", "Shoe", "Sock", "Shirt", "Frock", "Skirt", "Trouser", "Cap", "Umbrella",
    "Brush", "Comb", "Mirror", "Clock", "Lamp", "Candle", "Basket", "Box", "Tray", "Pan", "Pot", "Knife", "Ladle",
    "Hanger", "Broom",
];
/// Sinhala nouns (unrelated to the English name: only the index shape matters for the budget).
pub const SI_NOUNS: [&str; 20] = [
    "බාල්දිය",
    "බේසම",
    "කෝප්පය",
    "පිඟාන",
    "හැන්ද",
    "බෝතලය",
    "තුවාය",
    "ඇතිරිල්ල",
    "බෑගය",
    "පාවහන්",
    "සෙරෙප්පු",
    "කමිසය",
    "ගවුම",
    "කුඩය",
    "බුරුසුව",
    "පනාව",
    "කණ්ණාඩිය",
    "පහන",
    "කූඩය",
    "පිහිය",
];
pub const SI_ADJECTIVES: [&str; 10] = ["නිල්", "රතු", "කොළ", "කළු", "සුදු", "කහ", "ලොකු", "පොඩි", "ළමා", "කාන්තා"];

/// Small deterministic PRNG (64-bit LCG), so every run benchmarks the same data.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    pub fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next()).unwrap() % n
    }
}

pub fn sku(i: usize) -> String {
    format!("1{i:05}")
}

pub fn barcode(i: usize) -> String {
    let first12 = format!("479{i:09}");
    format!("{first12}{}", ean13_check_digit(&first12).unwrap())
}

pub fn phone(i: usize) -> String {
    format!("+9477{i:07}")
}

/// `n` products: English name `"{adj} {noun} {size}"`, Sinhala name on 3 of every 5, SKU `1xxxxx`,
/// EAN-13 `479…`, every 20th inactive. Inserted through the real FTS triggers, then `ANALYZE`.
pub async fn products(pool: &SqlitePool, n: usize) {
    let mut rng = Lcg::new(0x00C0_FFEE);
    let mut tx = pool.begin().await.unwrap();
    for start in (0..n).step_by(CHUNK) {
        let rows: Vec<String> = (start..n.min(start + CHUNK))
            .map(|i| {
                let name = format!("{} {} {}", ADJECTIVES[rng.below(20)], NOUNS[rng.below(40)], 1 + rng.below(50));
                let name_si = if i % 5 < 3 {
                    format!("'{} {}'", SI_ADJECTIVES[rng.below(10)], SI_NOUNS[rng.below(20)])
                } else {
                    "NULL".to_owned()
                };
                let active = i64::from(i % 20 != 0);
                let price = 1_000 + rng.below(500_000);
                format!(
                    "('p{i:06}', 's', '{name}', {name_si}, '{}', '{}', {price}, {active}, {META})",
                    sku(i),
                    barcode(i)
                )
            })
            .collect();
        let sql = format!(
            "INSERT INTO products (id, store_id, name, name_si, sku, barcode, price, is_active, created_at, updated_at, updated_by_device) VALUES {}",
            rows.join(",")
        );
        sqlx::query(&sql).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    sqlx::query("ANALYZE").execute(pool).await.unwrap();
}

/// `n` customers with phones `+9477xxxxxxx`.
pub async fn customers(pool: &SqlitePool, n: usize) {
    let mut tx = pool.begin().await.unwrap();
    for start in (0..n).step_by(CHUNK) {
        let rows: Vec<String> = (start..n.min(start + CHUNK))
            .map(|i| format!("('c{i:06}', 's', '{}', 'Customer {i}', '2026-11-08T09:00:00.000Z', {META})", phone(i)))
            .collect();
        let sql = format!(
            "INSERT INTO customers (id, store_id, phone_e164, display_name, first_seen_at, created_at, updated_at, updated_by_device) VALUES {}",
            rows.join(",")
        );
        sqlx::query(&sql).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    sqlx::query("ANALYZE").execute(pool).await.unwrap();
}

// ───────── Block 2.10: the full §13.4 dataset (orders, stock, customers, audit) ─────────

/// What [`scale`] wrote, for the assertions and the e2e dataset.
pub struct Scale {
    pub products: usize,
    pub customers: usize,
    pub orders: usize,
    pub order_items: usize,
    pub first_day: String,
    pub last_day: String,
}

pub const DAYS: i64 = 365;
pub const QUICK_KEYS: usize = 200;

/// Business date `offset` days after 9 November 2025 (the year ends on 8 November 2026).
pub fn day(offset: i64) -> String {
    let d = chrono::NaiveDate::from_ymd_opt(2025, 11, 9).unwrap() + chrono::Duration::days(offset);
    d.format("%Y-%m-%d").to_string()
}

async fn insert_chunks(c: &mut sqlx::SqliteConnection, head: &str, rows: &[String]) {
    for chunk in rows.chunks(CHUNK) {
        sqlx::query(&format!("{head} VALUES {}", chunk.join(","))).execute(&mut *c).await.unwrap();
    }
}

const ORDER_COLS: &str = "INSERT INTO orders (id, store_id, device_id, cash_session_id, receipt_no, checkout_token, status, \
     staff_id, customer_id, line_count, item_count, gross_sales, discount_total, net_sales, tax_total, grand_total, paid_total, \
     change_due, cost_total, quick_sale_net, unknown_cost_net, business_date, completed_at, voided_at, voided_by, void_reason, \
     created_at, updated_at, updated_by_device)";
const ITEM_COLS: &str = "INSERT INTO order_items (id, store_id, device_id, order_id, line_no, product_id, is_custom, custom_key, \
     name, list_price, unit_price, qty, line_gross, line_discount, order_discount_share, line_total, line_net, cost_total, \
     cost_known, created_at)";
const PAY_COLS: &str = "INSERT INTO order_payments (id, store_id, device_id, order_id, method, amount, created_at)";
const SALE_COLS: &str = "INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, \
     order_item_id, unit_cost, revenue_net, business_date, created_at)";
const MOVE_COLS: &str = "INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, \
     unit_cost, business_date, created_at)";
const AUDIT_COLS: &str = "INSERT INTO audit_logs (id, store_id, device_id, device_seq, staff_id, action, entity_type, entity_id, \
     metadata, business_date, prev_hash, hash, created_at)";

/// The §13.4 dataset on a first-run store: `products_n` products each with a stock layer in one received
/// batch, `customers_n` customers, and `order_items` order lines (about 3 per order) over a year at three
/// counters, with payments, sale slices, quick sales, voids, customers on 30 % of sales, expenses, damage
/// and one pulled audit row per order. History arrives through the sync pull path (`sync_ctx.applying`),
/// so every derived table (rollups, customer and quick-sale stats, stock levels, layers) is maintained by
/// the real triggers. Deterministic: every run benchmarks the same rows.
pub async fn scale(
    pool: &SqlitePool,
    store: &str,
    device: &str,
    staff: &str,
    products_n: usize,
    customers_n: usize,
    order_items: usize,
) -> Scale {
    let mut rng = Lcg::new(0x5CA1E);
    let meta = |at: &str| format!("'{at}', '{at}', '{device}'");
    let opening = "2025-11-01T03:00:00.000Z";
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await.unwrap();
    let c = &mut *conn;

    // Products + one received batch with a layer per product (1,000 units each at 55 % of price).
    let mut prices: Vec<i64> = Vec::with_capacity(products_n);
    let mut rows = Vec::with_capacity(products_n);
    for i in 0..products_n {
        let name = format!("{} {} {}", ADJECTIVES[rng.below(20)], NOUNS[rng.below(40)], 1 + rng.below(50));
        let name_si = if i % 5 < 3 {
            format!("'{} {}'", SI_ADJECTIVES[rng.below(10)], SI_NOUNS[rng.below(20)])
        } else {
            "NULL".to_owned()
        };
        let price = i64::try_from(1_000 + rng.below(500_000)).unwrap();
        prices.push(price);
        rows.push(format!(
            "('sp{i:06}', '{store}', '{name}', {name_si}, '9{i:05}', '{}', {price}, {}, {})",
            barcode(500_000 + i),
            price * 55 / 100,
            meta(opening)
        ));
    }
    insert_chunks(
        c,
        "INSERT INTO products (id, store_id, name, name_si, sku, barcode, price, default_cost, created_at, updated_at, updated_by_device)",
        &rows,
    )
    .await;
    sqlx::query(&format!(
        "INSERT INTO batches (id, store_id, code, name, purchased_at, received_at, received_business_date, total_cost,
                              allocation_method, status, received_by, created_at, updated_at, updated_by_device)
         VALUES ('bulk', '{store}', 'B-2025-0001', 'Opening container', '{opening}', '{opening}', '2025-11-01', 0,
                 'manual', 'received', '{staff}', {})",
        meta(opening)
    ))
    .execute(&mut *c)
    .await
    .unwrap();
    let layers: Vec<String> = (0..products_n)
        .map(|i| {
            format!(
                "('sbi{i:06}', '{store}', 'bulk', 'sp{i:06}', 1000, {}, {}, {})",
                prices[i] * 55 / 100,
                prices[i],
                meta(opening)
            )
        })
        .collect();
    insert_chunks(
        c,
        "INSERT INTO batch_items (id, store_id, batch_id, product_id, qty_received, unit_cost, retail_price_at_intake, created_at, updated_at, updated_by_device)",
        &layers,
    )
    .await;
    let intake: Vec<String> = (0..products_n)
        .map(|i| {
            format!(
                "('smi{i:06}', '{store}', '{device}', 'sp{i:06}', 'sbi{i:06}', 1000, 'batch_intake', {}, '2025-11-01', '{opening}')",
                prices[i] * 55 / 100
            )
        })
        .collect();
    insert_chunks(c, MOVE_COLS, &intake).await;

    let customers: Vec<String> = (0..customers_n)
        .map(|i| format!("('sc{i:06}', '{store}', '+9471{i:07}', 'Customer {i}', '{opening}', {})", meta(opening)))
        .collect();
    insert_chunks(
        c,
        "INSERT INTO customers (id, store_id, phone_e164, display_name, first_seen_at, created_at, updated_at, updated_by_device)",
        &customers,
    )
    .await;

    // Pulled history: three counters, one closed session per counter per day.
    sqlx::query("UPDATE sync_ctx SET applying = 1 WHERE id = 1").execute(&mut *c).await.unwrap();
    let devices = [device.to_owned(), "dev-scale-2".to_owned(), "dev-scale-3".to_owned()];
    let mut sessions = Vec::new();
    for d in 0..DAYS {
        let bd = day(d);
        for (k, dev) in devices.iter().enumerate() {
            sessions.push(format!(
                "('ss{d:03}{k}', '{store}', '{dev}', '{bd}', 'closed', '{staff}', '{bd}T03:00:00.000Z', 0, '{staff}', '{bd}T15:00:00.000Z', {})",
                meta(&format!("{bd}T15:00:00.000Z"))
            ));
        }
    }
    insert_chunks(
        c,
        "INSERT INTO cash_sessions (id, store_id, device_id, business_date, status, opened_by, opened_at, opening_float, closed_by, closed_at, created_at, updated_at, updated_by_device)",
        &sessions,
    )
    .await;

    let (mut orders, mut items, mut pays, mut moves, mut audits) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut per_slot = vec![0usize; usize::try_from(DAYS).unwrap() * 3];
    let (mut written, mut n) = (0usize, 0usize);
    while written < order_items {
        let d = rng.below(usize::try_from(DAYS).unwrap());
        let k = rng.below(3);
        per_slot[d * 3 + k] += 1;
        let seq = per_slot[d * 3 + k];
        let bd = day(i64::try_from(d).unwrap());
        let at = format!("{bd}T{:02}:{:02}:{:02}.{:03}Z", 4 + (seq / 3600) % 14, (seq / 60) % 60, seq % 60, n % 1000);
        let dev = &devices[k];
        let voided = n % 50 == 7;
        let lines = 1 + rng.below(5);
        let (mut gross, mut cost, mut quick, mut units) = (0i64, 0i64, 0i64, 0i64);
        for l in 0..lines {
            let qty = i64::try_from(1 + rng.below(3)).unwrap();
            let id = format!("si{n:07}{l}");
            if rng.below(20) == 0 {
                let key = rng.below(QUICK_KEYS);
                let price = i64::try_from(500 + (key * 97) % 20_000).unwrap();
                let g = price * qty;
                items.push(format!(
                    "('{id}', '{store}', '{dev}', 'so{n:07}', {}, NULL, 1, 'quick item {key}', 'Quick item {key}', {price}, {price}, {qty}, {g}, 0, 0, {g}, {g}, 0, 0, '{at}')",
                    l + 1
                ));
                gross += g;
                quick += g;
            } else {
                let p = rng.below(products_n);
                let price = prices[p];
                let unit_cost = price * 55 / 100;
                let g = price * qty;
                items.push(format!(
                    "('{id}', '{store}', '{dev}', 'so{n:07}', {}, 'sp{p:06}', 0, NULL, 'Product {p}', {price}, {price}, {qty}, {g}, 0, 0, {g}, {g}, {}, 1, '{at}')",
                    l + 1,
                    unit_cost * qty
                ));
                if !voided {
                    moves.push(format!(
                        "('sm{n:07}{l}', '{store}', '{dev}', 'sp{p:06}', 'sbi{p:06}', {}, 'sale', '{id}', {unit_cost}, {g}, '{bd}', '{at}')",
                        -qty
                    ));
                }
                gross += g;
                cost += unit_cost * qty;
            }
            units += qty;
        }
        written += lines;
        let customer = if rng.below(10) < 3 { format!("'sc{:06}'", rng.below(customers_n)) } else { "NULL".to_owned() };
        let (status, void_cols) = if voided {
            ("voided", format!("'{at}', '{staff}', 'DUPLICATE_SCAN'"))
        } else {
            ("completed", "NULL, NULL, NULL".to_owned())
        };
        orders.push(format!(
            "('so{n:07}', '{store}', '{dev}', 'ss{d:03}{k}', 'S0{}-{}-{seq:04}', 'stok{n:07}', '{status}', '{staff}', {customer}, {lines}, {units}, {gross}, 0, {gross}, 0, {gross}, {gross}, 0, {cost}, {quick}, {quick}, '{bd}', '{at}', {void_cols}, {})",
            k + 1,
            bd.replace('-', ""),
            meta(&at)
        ));
        pays.push(format!("('sy{n:07}', '{store}', '{dev}', 'so{n:07}', 'cash', {gross}, '{at}')"));
        audits.push(format!(
            "('sa{n:07}', '{store}', 'dev-scale-audit', {}, '{staff}', 'SALE', 'order', 'so{n:07}', '{{}}', '{bd}', 'p{n}', 'h{n}', '{at}')",
            n + 1
        ));
        n += 1;
        // Parents first, children after: the pull-path triggers count a completed order at insert, and its
        // pulled payments and quick-sale lines as they arrive.
        if orders.len() >= CHUNK * 4 || written >= order_items {
            insert_chunks(c, ORDER_COLS, &orders).await;
            insert_chunks(c, ITEM_COLS, &items).await;
            insert_chunks(c, PAY_COLS, &pays).await;
            insert_chunks(c, SALE_COLS, &moves).await;
            insert_chunks(c, AUDIT_COLS, &audits).await;
            for v in [&mut orders, &mut items, &mut pays, &mut moves, &mut audits] {
                v.clear();
            }
        }
    }
    sqlx::query("UPDATE sync_ctx SET applying = 0 WHERE id = 1").execute(&mut *c).await.unwrap();

    // Expenses (two a day) and a damage write-off a day, across the year.
    let cats: Vec<String> =
        sqlx::query_scalar("SELECT id FROM expense_categories ORDER BY name").fetch_all(&mut *c).await.unwrap();
    let (mut exp, mut dmg) = (Vec::new(), Vec::new());
    for d in 0..DAYS {
        let bd = day(d);
        for j in 0..2 {
            exp.push(format!(
                "('se{d:03}{j}', '{store}', '{}', {}, '{bd}', 'cash', '{staff}', {})",
                cats[rng.below(cats.len())],
                1_000 * (1 + rng.below(900)),
                meta(&format!("{bd}T10:00:00.000Z"))
            ));
        }
        let p = rng.below(products_n);
        dmg.push(format!(
            "('sd{d:03}', '{store}', '{device}', 'sp{p:06}', 'sbi{p:06}', -1, 'damage', {}, '{bd}', '{bd}T11:00:00.000Z')",
            prices[p] * 55 / 100
        ));
    }
    insert_chunks(
        c,
        "INSERT INTO expenses (id, store_id, category_id, amount, business_date, payment_method, recorded_by, created_at, updated_at, updated_by_device)",
        &exp,
    )
    .await;
    insert_chunks(c, MOVE_COLS, &dmg).await;
    sqlx::query("COMMIT").execute(&mut *c).await.unwrap();
    drop(conn);
    sqlx::query("ANALYZE").execute(pool).await.unwrap();
    Scale {
        products: products_n,
        customers: customers_n,
        orders: n,
        order_items: written,
        first_day: day(0),
        last_day: day(DAYS - 1),
    }
}
