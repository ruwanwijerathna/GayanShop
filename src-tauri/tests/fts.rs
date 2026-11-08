//! Block 0.3 — FTS5 tokenizers used by the till search exist in the bundled SQLite (§13.4).

mod common;

use common::{META, exec, open, scalar_i64};

async fn ids(pool: &sqlx::SqlitePool, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(pool).await.unwrap()
}

#[tokio::test]
async fn latin_prefix_search() {
    let t = open().await;
    let p = t.pool();
    exec(p, &format!("INSERT INTO products (id, store_id, name, sku, barcode, price, created_at, updated_at, updated_by_device) VALUES ('p1', 's', 'Blue Bucket 10L', '104523', '2100000123459', 45000, {META})")).await;
    exec(p, &format!("INSERT INTO products (id, store_id, name, price, created_at, updated_at, updated_by_device) VALUES ('p2', 's', 'Black Sandal', 150000, {META})")).await;
    assert_eq!(ids(p, "SELECT id FROM products_fts WHERE products_fts MATCH 'blu* buc*'").await, vec!["p1"]);
    assert_eq!(ids(p, "SELECT id FROM products_fts WHERE products_fts MATCH 'b*' ORDER BY id").await, vec!["p1", "p2"]);
    assert_eq!(ids(p, "SELECT id FROM products_fts WHERE products_fts MATCH '1045*'").await, vec!["p1"]);
}

#[tokio::test]
async fn sinhala_trigram_search_and_maintenance() {
    let t = open().await;
    let p = t.pool();
    exec(p, &format!("INSERT INTO products (id, store_id, name, name_si, price, created_at, updated_at, updated_by_device) VALUES ('p1', 's', 'Bucket', 'නිල් බාල්දිය', 45000, {META})")).await;
    assert_eq!(ids(p, "SELECT id FROM products_fts_si WHERE products_fts_si MATCH 'බාල්දි'").await, vec!["p1"]);
    exec(p, "UPDATE products SET name_si = 'රතු බේසම' WHERE id = 'p1'").await;
    assert!(ids(p, "SELECT id FROM products_fts_si WHERE products_fts_si MATCH 'බාල්දි'").await.is_empty());
    assert_eq!(ids(p, "SELECT id FROM products_fts_si WHERE products_fts_si MATCH 'බේසම'").await, vec!["p1"]);
    exec(p, "UPDATE products SET name_si = NULL WHERE id = 'p1'").await;
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM products_fts_si").await, 0);
    exec(p, "DELETE FROM products WHERE id = 'p1'").await;
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM products_fts").await, 0);
}
