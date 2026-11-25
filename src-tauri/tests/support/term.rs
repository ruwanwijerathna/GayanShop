//! A terminal for the sync tests (Blocks 3.4-3.6): its own database, identity and manual clock, selling
//! through the real checkout, void and back-office code. `t01()` sets up the store; `joined()` adds a
//! counter the way decision #18 does and bootstraps it from a sync server.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use gayanshop_lib::auth::guard::Actor;
use gayanshop_lib::auth::pin::PinHasher;
use gayanshop_lib::clock::{Clock, ManualClock, iso};
use gayanshop_lib::db::Db;
use gayanshop_lib::db::repo::WriteCtx;
use gayanshop_lib::db::repo::batches::{self, AdHocPurchase};
use gayanshop_lib::db::repo::checkout::{self, CheckoutLine, NoFaults, Seller};
use gayanshop_lib::db::repo::products::{self, CodeChoice, ProductInput, ProductKind};
use gayanshop_lib::db::repo::void::{self, VoidInput, VoidReason, Voider};
use gayanshop_lib::db::repo::{cash, reports};
use gayanshop_lib::domain::permissions::PermMask;
use gayanshop_lib::setup::{SetupInput, first_run};
use gayanshop_lib::sync::api::SyncApi;
use gayanshop_lib::sync::config::ServerConfig;
use gayanshop_lib::sync::engine::{self, PullOptions};
use gayanshop_lib::sync::registration;

use super::common;
use super::hub::{Hub, foreign_keys};

pub const SETUP_AT: &str = "2026-11-08T03:30:00.000Z";

pub fn at(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc)
}

/// One counter: its own database, identity and clock.
pub struct Term {
    pub _dir: tempfile::TempDir,
    pub db: Db,
    pub store_id: String,
    pub device_id: String,
    pub code: String,
    pub owner: String,
    pub clock: ManualClock,
}

impl Term {
    pub fn ctx(&self) -> WriteCtx {
        let now = iso(self.clock.now());
        WriteCtx {
            store_id: self.store_id.clone(),
            device_id: self.device_id.clone(),
            device_code: self.code.clone(),
            business_date: now[..10].to_owned(),
            now,
        }
    }

    pub fn actor(&self) -> Actor {
        Actor { staff_id: self.owner.clone(), approved_by: None }
    }

    pub fn tick(&self, secs: i64) {
        self.clock.advance(chrono::Duration::seconds(secs));
    }

    pub async fn seller(&self) -> Seller {
        let (perms, max_discount_bp, max_override_bp): (i64, i64, i64) =
            sqlx::query_as("SELECT perms, max_discount_bp, max_override_bp FROM v_staff_effective WHERE staff_id = ?1")
                .bind(&self.owner)
                .fetch_one(&self.db.pool)
                .await
                .unwrap();
        Seller { staff_id: self.owner.clone(), perms: PermMask(perms), max_discount_bp, max_override_bp }
    }

    pub async fn open_session(&self) {
        cash::open(&self.db, &self.ctx(), &self.actor(), 0).await.unwrap();
    }

    pub async fn product(&self, name: &str, price: i64, barcode: Option<&str>, stock: Option<(i64, i64)>) -> String {
        let input = ProductInput {
            kind: ProductKind::Standard,
            name: name.into(),
            name_si: None,
            sku: CodeChoice::Generate,
            barcode: barcode.map_or(CodeChoice::None, |b| CodeChoice::Manual { value: b.into() }),
            category_id: None,
            default_cost: Some(0),
            price,
            min_price: None,
            track_stock: true,
            allow_negative_stock: true,
            attributes: std::collections::BTreeMap::new(),
            is_active: true,
        };
        let id = products::create(&self.db, &self.ctx(), &self.actor(), &input, true).await.unwrap().id;
        if let Some((qty, unit_cost)) = stock {
            let buy = AdHocPurchase { product_id: id.clone(), qty, unit_cost, supplier_name: None, note: None };
            batches::ad_hoc_purchase(&self.db, &self.ctx(), &self.actor(), &buy).await.unwrap();
        }
        id
    }

    pub async fn sell(&self, token: &str, lines: Vec<CheckoutLine>, phone: Option<&str>) -> String {
        self.tick(30);
        let total: i64 = lines.iter().map(|l| l.unit_price * l.qty).sum();
        let mut input = common::cart(token, lines, vec![common::cash(total, None)]);
        input.customer_phone = phone.map(str::to_owned);
        checkout::checkout(&self.db, &self.ctx(), &self.seller().await, &input, None, &NoFaults).await.unwrap().order_id
    }

    pub async fn void(&self, order_id: &str) {
        self.tick(30);
        let voider = Voider { staff_id: self.owner.clone(), perms: self.seller().await.perms };
        let input =
            VoidInput { order_id: order_id.into(), reason: VoidReason::WrongItem, note: None, approval_id: None };
        void::void_order(&self.db, &self.ctx(), &voider, &input, None, &NoFaults).await.unwrap();
    }

    pub async fn round(&self, hub: &Arc<Hub>) -> engine::RoundOutcome {
        self.tick(10);
        engine::round(&self.db, &hub.client(&self.device_id), &self.clock, &self.device_id).await.unwrap()
    }

    /// A round against any server (the chaos suite's real one).
    pub async fn round_with(&self, api: &dyn SyncApi) -> Result<engine::RoundOutcome, engine::RoundError> {
        self.tick(10);
        engine::round(&self.db, api, &self.clock, &self.device_id).await
    }

    pub async fn text(&self, sql: &str) -> String {
        sqlx::query_scalar(sql).fetch_one(&self.db.pool).await.unwrap()
    }

    /// Change a product's price through the back office.
    pub async fn edit_price(&self, id: &str, price: i64) {
        self.tick(60);
        let p = products::get(&self.db.pool, &self.store_id, id, true).await.unwrap();
        let input = ProductInput {
            kind: ProductKind::Standard,
            name: p.name.clone(),
            name_si: p.name_si.clone(),
            sku: p.sku.clone().map_or(CodeChoice::None, |value| CodeChoice::Manual { value }),
            barcode: p.barcode.clone().map_or(CodeChoice::None, |value| CodeChoice::Manual { value }),
            category_id: p.category_id.clone(),
            default_cost: p.default_cost,
            price,
            min_price: p.min_price,
            track_stock: p.track_stock,
            allow_negative_stock: p.allow_negative_stock,
            attributes: p.attributes.clone(),
            is_active: p.is_active,
        };
        products::update(&self.db, &self.ctx(), &self.actor(), id, &input, true).await.unwrap();
    }

    /// A batch received, then reopened and re-costed, then received again: its layers end up edited after the
    /// intake movements that reference them (newer server_seqs than their children). Returns the batch id.
    pub async fn reworked_batch(&self, product: &str, qty: i64, unit_cost: i64) -> String {
        use gayanshop_lib::db::repo::batches::{AllocationMethod, BatchInput, BatchItemInput};
        let header = BatchInput {
            name: "Reworked batch".into(),
            supplier_name: None,
            supplier_ref: None,
            purchased_at: None,
            total_cost: 0,
            extra_costs: 0,
            allocation_method: Some(AllocationMethod::Manual),
            notes: None,
        };
        let b = batches::create(&self.db, &self.ctx(), &self.actor(), &header).await.unwrap().batch.id;
        let item = |cost| BatchItemInput { product_id: product.into(), qty, unit_cost: cost, retail_price: None };
        batches::set_item(&self.db, &self.ctx(), &b, &item(unit_cost)).await.unwrap();
        batches::receive(&self.db, &self.ctx(), &self.actor(), &b).await.unwrap();
        self.tick(60);
        batches::reopen(&self.db, &self.ctx(), &self.actor(), &b).await.unwrap();
        batches::set_item(&self.db, &self.ctx(), &b, &item(unit_cost + 500)).await.unwrap();
        batches::receive(&self.db, &self.ctx(), &self.actor(), &b).await.unwrap();
        b
    }

    /// An empty terminal with a fresh device id and no identity yet.
    pub async fn empty() -> Term {
        let dir = tempfile::tempdir().unwrap();
        let db =
            Db::open(&dir.path().join(gayanshop_lib::db::DB_FILE_NAME), &dir.path().join("backups")).await.unwrap();
        Term {
            _dir: dir,
            db,
            store_id: String::new(),
            device_id: gayanshop_lib::clock::new_id(),
            code: String::new(),
            owner: String::new(),
            clock: ManualClock::new(at("2026-11-08T03:45:00.000Z")),
        }
    }

    pub async fn count(&self, sql: &str) -> i64 {
        common::scalar_i64(&self.db.pool, sql).await
    }
}

pub async fn t01() -> Term {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join(gayanshop_lib::db::DB_FILE_NAME), &dir.path().join("backups")).await.unwrap();
    let clock = ManualClock::new(at(SETUP_AT));
    let input = SetupInput {
        store_name: "GayanShop".into(),
        owner_name: "Gayan".into(),
        owner_pin: "246810".into(),
        device_code: "T01".into(),
        ui_language: "en".into(),
    };
    let s = first_run(&db, &PinHasher::fast_for_tests(), &clock, &input).await.unwrap();
    clock.advance(chrono::Duration::minutes(10));
    let t = Term {
        _dir: dir,
        db,
        store_id: s.store_id,
        device_id: s.device_id,
        code: "T01".into(),
        owner: s.owner_staff_id,
        clock,
    };
    t.open_session().await;
    t
}

/// A counter that joined the store (decision #18) and bootstrapped its history from the hub in pages of 7.
pub async fn joined(hub: &Arc<Hub>, store_id: &str, code: &str) -> Term {
    let t = Term::empty().await;
    let client = hub.client(&t.device_id);
    joined_via(t, &client, store_id, code).await
}

/// Join the empty terminal `t` to the store, bootstrapping from `api` in pages of 7.
pub async fn joined_via(t: Term, api: &dyn SyncApi, store_id: &str, code: &str) -> Term {
    let Term { _dir: dir, db, device_id, clock, .. } = t;
    let cfg = ServerConfig::parse("http://127.0.0.1:1", "anon").unwrap();
    registration::record_joined_identity(&db, &clock, store_id, &device_id, code, &cfg, "en").await.unwrap();
    let opts = PullOptions { page_size: 7, max_pages: 10_000, include_own: true };
    let out = engine::pull(&db, api, &clock, &device_id, opts, &|_| {}).await.unwrap();
    assert!(out.caught_up);
    let owner: String = sqlx::query_scalar(
        "SELECT s.id FROM staff s JOIN roles r ON r.id = s.role_id WHERE r.name = 'Owner' AND r.is_system = 1",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    let t = Term { _dir: dir, db, store_id: store_id.into(), device_id, code: code.into(), owner, clock };
    t.open_session().await;
    t
}

/// The derived read models a store-wide report reads, minus rows a rebuild drops (all zero).
pub async fn derived(db: &Db) -> Vec<String> {
    let queries = [
        "SELECT json_array('dss', business_date, device_id, sales_count, void_count, void_value, gross_sales, discounts,
                net_sales, tax_total, cogs, quick_sale_net, unknown_cost_net, customer_orders)
         FROM daily_sales_summary WHERE sales_count <> 0 OR void_count <> 0 ORDER BY business_date, device_id",
        "SELECT json_array('dps', business_date, device_id, method, amount, txn_count) FROM daily_payment_summary
         WHERE amount <> 0 OR txn_count <> 0 ORDER BY business_date, device_id, method",
        "SELECT json_array('sl', product_id, on_hand) FROM stock_levels ORDER BY product_id",
        "SELECT json_array('bis', batch_item_id, batch_id, product_id, received_qty, on_hand, sold_qty, sold_revenue,
                sold_cost, written_off_qty, written_off_cost) FROM batch_item_stock ORDER BY batch_item_id",
        "SELECT json_array('cs', customer_id, visit_count, total_spend, total_net, first_visit_date, last_visit_date)
         FROM customer_stats WHERE visit_count <> 0 ORDER BY customer_id",
        "SELECT json_array('qss', custom_key, display_name, times_sold, units_sold, revenue_net, last_price, min_price,
                max_price) FROM quick_sale_stats ORDER BY custom_key",
    ];
    let mut out = Vec::new();
    for q in queries {
        let rows: Vec<String> = sqlx::query_scalar(q).fetch_all(&db.pool).await.unwrap();
        out.extend(rows);
    }
    out
}

/// What `rebuild_derived` would produce, to prove the trigger-maintained values on a terminal are exact.
pub async fn rebuilt(db: &Db) -> Vec<String> {
    let mut tx = db.write().await.unwrap();
    reports::recompute(tx.conn()).await.unwrap();
    tx.commit().await.unwrap();
    derived(db).await
}

pub async fn outbox(db: &Db) -> i64 {
    common::scalar_i64(&db.pool, "SELECT COUNT(*) FROM sync_outbox").await
}

pub async fn hub_for(t: &Term) -> Arc<Hub> {
    Arc::new(Hub::new(foreign_keys(&t.db.pool).await))
}

pub fn line(product: &str, price: i64, qty: i64) -> CheckoutLine {
    common::line(product, price, qty)
}
