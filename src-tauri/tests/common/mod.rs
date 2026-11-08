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

/// A store set up through the real first-run path (roles, Owner, system batches, settings).
pub async fn store(t: &TestDb) -> gayanshop_lib::setup::SetupResult {
    use gayanshop_lib::auth::pin::PinHasher;
    use gayanshop_lib::clock::ManualClock;
    use gayanshop_lib::setup::{SetupInput, first_run};
    let at = chrono::DateTime::parse_from_rfc3339(NOW).unwrap().with_timezone(&chrono::Utc);
    let input = SetupInput {
        store_name: "GayanShop".into(),
        owner_name: "Gayan".into(),
        owner_pin: "246810".into(),
        device_code: "T01".into(),
        ui_language: "en".into(),
    };
    first_run(&t.db, &PinHasher::fast_for_tests(), &ManualClock::new(at), &input).await.unwrap()
}

/// Write context for `store()` at an ISO time (business date = its Colombo date; tests stay after 04:00).
pub fn ctx_at(s: &gayanshop_lib::setup::SetupResult, now: &str) -> gayanshop_lib::db::repo::WriteCtx {
    gayanshop_lib::db::repo::WriteCtx {
        store_id: s.store_id.clone(),
        device_id: s.device_id.clone(),
        device_code: s.device_code.clone(),
        now: now.into(),
        business_date: now[..10].into(),
    }
}

// ───────── a working till (Phase 1 checkout and void tests) ─────────

/// A first-run store with real Cashier and Supervisor staff (seeded roles, hashed PINs), an open cash
/// session on this counter, and an `Auth` for the command-level approval flow.
pub struct Till {
    pub t: TestDb,
    pub s: gayanshop_lib::setup::SetupResult,
    pub auth: gayanshop_lib::auth::Auth,
    pub clock: gayanshop_lib::clock::ManualClock,
    pub dev: gayanshop_lib::auth::DeviceCtx,
    pub session_id: String,
}

pub const CASHIER: &str = "cashier";
pub const SUP: &str = "sup";
pub const CASHIER_PIN: &str = "1111";
pub const SUP_PIN: &str = "2222";

pub async fn till() -> Till {
    use gayanshop_lib::auth::pin::PinHasher;
    use gayanshop_lib::auth::{Auth, DeviceCtx};
    use gayanshop_lib::clock::ManualClock;
    let t = open().await;
    let s = store(&t).await;
    let hasher = PinHasher::fast_for_tests();
    for (id, role, pin) in [(CASHIER, "Cashier", CASHIER_PIN), (SUP, "Supervisor", SUP_PIN)] {
        let hash = hasher.hash(pin).unwrap();
        exec(
            t.pool(),
            &format!(
                "INSERT INTO staff (id, store_id, display_name, role_id, pin_hash, created_at, updated_at, updated_by_device)
                 SELECT '{id}', '{store}', '{id}', id, '{hash}', '{NOW}', '{NOW}', '{dev}' FROM roles WHERE name = '{role}'",
                store = s.store_id,
                dev = s.device_id
            ),
        )
        .await;
    }
    let owner = gayanshop_lib::auth::guard::Actor { staff_id: s.owner_staff_id.clone(), approved_by: None };
    let session = gayanshop_lib::db::repo::cash::open(&t.db, &ctx_at(&s, NOW), &owner, 0).await.unwrap();
    let identity = gayanshop_lib::setup::load_identity(t.pool()).await.unwrap().unwrap();
    let at = chrono::DateTime::parse_from_rfc3339(NOW).unwrap().with_timezone(&chrono::Utc);
    Till {
        dev: DeviceCtx { store_id: identity.store_id, device_id: identity.device_id, calendar: identity.calendar },
        auth: Auth::new(hasher),
        clock: ManualClock::new(at),
        session_id: session.id,
        t,
        s,
    }
}

impl Till {
    pub fn pool(&self) -> &SqlitePool {
        self.t.pool()
    }

    pub fn ctx(&self) -> gayanshop_lib::db::repo::WriteCtx {
        ctx_at(&self.s, NOW)
    }

    pub fn owner(&self) -> gayanshop_lib::auth::guard::Actor {
        gayanshop_lib::auth::guard::Actor { staff_id: self.s.owner_staff_id.clone(), approved_by: None }
    }

    /// The staff member's effective permissions and limits, as a seller.
    pub async fn seller(&self, staff_id: &str) -> gayanshop_lib::db::repo::checkout::Seller {
        let (perms, max_discount_bp, max_override_bp): (i64, i64, i64) =
            sqlx::query_as("SELECT perms, max_discount_bp, max_override_bp FROM v_staff_effective WHERE staff_id = ?1")
                .bind(staff_id)
                .fetch_one(self.pool())
                .await
                .unwrap();
        gayanshop_lib::db::repo::checkout::Seller {
            staff_id: staff_id.to_owned(),
            perms: gayanshop_lib::domain::permissions::PermMask(perms),
            max_discount_bp,
            max_override_bp,
        }
    }

    pub async fn login(&self, staff_id: &str, pin: &str) {
        gayanshop_lib::auth::login(&self.t.db, &self.auth, &self.clock, &self.dev, staff_id, pin).await.unwrap();
    }

    /// A tracked product received in one ad-hoc batch per `(qty, unit_cost)` layer, oldest first.
    pub async fn stocked(&self, name: &str, price: i64, layers: &[(i64, i64)]) -> String {
        let id = self.product(name, price, true, 0, None).await;
        for (qty, cost) in layers {
            let buy = gayanshop_lib::db::repo::batches::AdHocPurchase {
                product_id: id.clone(),
                qty: *qty,
                unit_cost: *cost,
                supplier_name: None,
                note: None,
            };
            gayanshop_lib::db::repo::batches::ad_hoc_purchase(&self.t.db, &self.ctx(), &self.owner(), &buy)
                .await
                .unwrap();
        }
        id
    }

    pub async fn product(&self, name: &str, price: i64, tracked: bool, cost: i64, min_price: Option<i64>) -> String {
        use gayanshop_lib::db::repo::products::{self, CodeChoice, ProductInput, ProductKind};
        let input = ProductInput {
            kind: ProductKind::Standard,
            name: name.into(),
            name_si: None,
            sku: CodeChoice::Generate,
            barcode: CodeChoice::None,
            category_id: None,
            default_cost: Some(cost),
            price,
            min_price,
            track_stock: tracked,
            allow_negative_stock: true,
            attributes: std::collections::BTreeMap::new(),
            is_active: true,
        };
        products::create(&self.t.db, &self.ctx(), &self.owner(), &input, true).await.unwrap().id
    }

    pub async fn count(&self, sql: &str) -> i64 {
        scalar_i64(self.pool(), sql).await
    }
}

/// A catalog line at list price.
pub fn line(product_id: &str, unit_price: i64, qty: i64) -> gayanshop_lib::db::repo::checkout::CheckoutLine {
    gayanshop_lib::db::repo::checkout::CheckoutLine {
        product_id: Some(product_id.into()),
        name: None,
        unit_price,
        qty,
        line_discount: 0,
        unit_cost: None,
        override_reason: None,
    }
}

/// A quick-sale line.
pub fn quick(name: &str, unit_price: i64, qty: i64) -> gayanshop_lib::db::repo::checkout::CheckoutLine {
    gayanshop_lib::db::repo::checkout::CheckoutLine {
        product_id: None,
        name: Some(name.into()),
        unit_price,
        qty,
        line_discount: 0,
        unit_cost: None,
        override_reason: None,
    }
}

pub fn cash(amount: i64, tendered: Option<i64>) -> gayanshop_lib::db::repo::checkout::PaymentLine {
    gayanshop_lib::db::repo::checkout::PaymentLine {
        method: gayanshop_lib::domain::payments::PaymentMethod::Cash,
        amount,
        tendered,
        reference: None,
    }
}

pub fn cart(
    token: &str,
    lines: Vec<gayanshop_lib::db::repo::checkout::CheckoutLine>,
    payments: Vec<gayanshop_lib::db::repo::checkout::PaymentLine>,
) -> gayanshop_lib::db::repo::checkout::CheckoutInput {
    gayanshop_lib::db::repo::checkout::CheckoutInput {
        checkout_token: token.into(),
        lines,
        bill_discount: 0,
        payments,
        customer_phone: None,
        customer_name: None,
        note: None,
        approval_id: None,
    }
}
