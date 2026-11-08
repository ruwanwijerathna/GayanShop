//! Shared fixtures for the schema/DB integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use gayanshop_lib::db::Db;
use sqlx::SqlitePool;
use tempfile::TempDir;

pub const NOW: &str = "2026-11-08T09:00:00.000Z";
pub const BD: &str = "2026-11-08";
pub const DEV: &str = "dev1";
/// Row metadata columns for mutable (LWW) tables.
pub const META: &str = "'2026-11-08T09:00:00.000Z', '2026-11-08T09:00:00.000Z', 'dev1'";

pub struct TestDb {
    pub dir: TempDir,
    pub db: Db,
}

impl TestDb {
    pub fn pool(&self) -> &SqlitePool {
        &self.db.pool
    }
}

pub async fn open() -> TestDb {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Db::open(&dir.path().join("pos.db"), &dir.path().join("backups")).await.expect("open db");
    TestDb { dir, db }
}

pub async fn exec(pool: &SqlitePool, sql: &str) {
    sqlx::query(sql).execute(pool).await.unwrap_or_else(|e| panic!("statement failed: {e}\n{sql}"));
}

/// Run `stmts` in one transaction; assert one of them fails with an error containing `needle`;
/// always roll back so the database is unchanged.
pub async fn rejects(pool: &SqlitePool, stmts: &[&str], needle: &str) {
    let mut tx = pool.begin().await.expect("begin");
    let mut failure = None;
    for sql in stmts {
        if let Err(e) = sqlx::query(sql).execute(&mut *tx).await {
            failure = Some(e.to_string());
            break;
        }
    }
    tx.rollback().await.expect("rollback");
    let msg =
        failure.unwrap_or_else(|| panic!("expected rejection containing {needle:?}, but every statement succeeded"));
    assert!(msg.contains(needle), "expected error containing {needle:?}, got: {msg}");
}

pub async fn scalar_i64(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap_or_else(|e| panic!("{e}\n{sql}"))
}

/// Store with a Cashier (5% discount, no override/void), a Supervisor (override + void), a tracked
/// product with a received 2-unit layer at Rs 200.00 cost, an open cash session on `dev1`, a customer.
pub async fn seed(pool: &SqlitePool) {
    exec(pool, r#"INSERT INTO device_settings (key, value) VALUES ('device.id', '"dev1"')"#).await;
    exec(pool, &format!("INSERT INTO roles (id, store_id, name, permissions, max_discount_bp, created_at, updated_at, updated_by_device) VALUES ('r_cash', 's', 'Cashier', {}, 500, {META})", 1 | 2 | 8 | 16)).await;
    exec(pool, &format!("INSERT INTO roles (id, store_id, name, permissions, max_discount_bp, max_override_bp, created_at, updated_at, updated_by_device) VALUES ('r_sup', 's', 'Supervisor', {}, 2000, 2000, {META})", 1 | 4 | 32)).await;
    for (id, role) in [("cashier", "r_cash"), ("sup", "r_sup")] {
        exec(pool, &format!("INSERT INTO staff (id, store_id, display_name, role_id, pin_hash, created_at, updated_at, updated_by_device) VALUES ('{id}', 's', '{id}', '{role}', 'h', {META})")).await;
    }
    exec(pool, &format!("INSERT INTO products (id, store_id, name, price, created_at, updated_at, updated_by_device) VALUES ('p1', 's', 'Blue Bucket', 45000, {META})")).await;
    exec(pool, &format!("INSERT INTO batches (id, store_id, code, name, purchased_at, received_at, received_business_date, allocation_method, status, created_at, updated_at, updated_by_device) VALUES ('b1', 's', 'B1', 'Batch 1', '{NOW}', '{NOW}', '{BD}', 'manual', 'received', {META})")).await;
    exec(pool, &format!("INSERT INTO batch_items (id, store_id, batch_id, product_id, qty_received, unit_cost, created_at, updated_at, updated_by_device) VALUES ('bi1', 's', 'b1', 'p1', 2, 20000, {META})")).await;
    exec(pool, &format!("INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, unit_cost, business_date, created_at) VALUES ('m0', 's', 'dev1', 'p1', 'bi1', 2, 'batch_intake', 20000, '{BD}', '{NOW}')")).await;
    exec(pool, &format!("INSERT INTO cash_sessions (id, store_id, device_id, business_date, opened_by, opened_at, created_at, updated_at, updated_by_device) VALUES ('cs1', 's', 'dev1', '{BD}', 'cashier', '{NOW}', {META})")).await;
    exec(pool, &format!("INSERT INTO customers (id, store_id, phone_e164, first_seen_at, created_at, updated_at, updated_by_device) VALUES ('cu1', 's', '+94771234567', '{NOW}', {META})")).await;
}

/// SQL for a pending order with one tracked line (qty 1 unless given), one sale slice and one cash payment.
pub struct Sale<'a> {
    pub id: &'a str,
    pub token: &'a str,
    pub unit: i64,
    pub list: i64,
    pub disc: i64,
    pub approved: Option<&'a str>,
    pub customer: bool,
}

impl<'a> Sale<'a> {
    pub fn new(id: &'a str) -> Self {
        Sale { id, token: id, unit: 45000, list: 45000, disc: 0, approved: None, customer: true }
    }

    pub fn statements(&self) -> Vec<String> {
        let id = self.id;
        let total = self.unit - self.disc;
        let overridden = i64::from(self.unit < self.list);
        let approved = self.approved.map_or("NULL".to_owned(), |a| format!("'{a}'"));
        let customer = if self.customer { "'cu1'" } else { "NULL" };
        vec![
            format!(
                "INSERT INTO orders (id, store_id, device_id, cash_session_id, receipt_no, checkout_token, status, staff_id, customer_id, line_count, item_count, gross_sales, discount_total, net_sales, grand_total, paid_total, change_due, cost_total, business_date, completed_at, created_at, updated_at, updated_by_device) VALUES ('{id}', 's', 'dev1', 'cs1', 'R-{id}', '{}', 'pending', 'cashier', {customer}, 1, 1, {}, {}, {total}, {total}, {total}, 500, 20000, '{BD}', '{NOW}', {META})",
                self.token, self.unit, self.disc
            ),
            format!(
                "INSERT INTO order_items (id, store_id, device_id, order_id, line_no, product_id, name, list_price, unit_price, qty, line_gross, line_discount, line_total, line_net, cost_total, price_overridden, approved_by, created_at) VALUES ('{id}i', 's', 'dev1', '{id}', 1, 'p1', 'Blue Bucket', {}, {}, 1, {}, {}, {total}, {total}, 20000, {overridden}, {approved}, '{NOW}')",
                self.list, self.unit, self.unit, self.disc
            ),
            format!(
                "INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, order_item_id, unit_cost, revenue_net, business_date, created_at) VALUES ('{id}m', 's', 'dev1', 'p1', 'bi1', -1, 'sale', '{id}i', 20000, {total}, '{BD}', '{NOW}')"
            ),
            format!(
                "INSERT INTO order_payments (id, store_id, device_id, order_id, method, amount, tendered, created_at) VALUES ('{id}p', 's', 'dev1', '{id}', 'cash', {total}, {}, '{NOW}')",
                total + 500
            ),
            format!("UPDATE orders SET status = 'completed' WHERE id = '{id}'"),
        ]
    }
}

pub async fn checkout(pool: &SqlitePool, sale: &Sale<'_>) {
    let mut tx = pool.begin().await.expect("begin");
    for sql in sale.statements() {
        sqlx::query(&sql).execute(&mut *tx).await.unwrap_or_else(|e| panic!("checkout failed: {e}\n{sql}"));
    }
    tx.commit().await.expect("commit");
}

pub async fn rejects_sale(pool: &SqlitePool, sale: &Sale<'_>, needle: &str) {
    let stmts = sale.statements();
    let refs: Vec<&str> = stmts.iter().map(String::as_str).collect();
    rejects(pool, &refs, needle).await;
}
