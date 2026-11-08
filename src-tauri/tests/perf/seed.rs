//! Deterministic benchmark dataset (doc §13.4). Block 1.1 seeds what the search and lookup rows need:
//! products (with Sinhala names, SKUs, EAN-13 barcodes) and customers. Block 2.10 adds the 100,000 order
//! items for the order, report and stock rows.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use gayanshop_lib::domain::receipt_no::ean13_check_digit;
use sqlx::SqlitePool;

pub const GATE_PRODUCTS: usize = 50_000;
pub const GATE_CUSTOMERS: usize = 20_000;
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
