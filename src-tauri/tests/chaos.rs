//! Block 3.6: the chaos suite (doc §8 Phase 3 exit, §13.1). Three terminal databases sync with the REAL local
//! Supabase stack (Postgres + PostgREST + GoTrue + the register-device function) through toxiproxy, which
//! cuts the network on demand. Every terminal is registered the way an owner would do it, and the cloud's own
//! views are read back with psql, so "cloud and every terminal agree" is checked against Postgres itself.
//!
//! Needs Docker and the local stack:
//!   pnpm exec supabase start -x realtime,storage-api,imgproxy,mailpit,postgres-meta,studio,logflare,vector,supavisor
//!   cargo test --manifest-path src-tauri/Cargo.toml --test chaos -- --ignored
//! It refuses to run against anything but this machine (127.0.0.1), reads the local stack's keys from
//! `supabase status` at run time, and fails loudly when the stack isn't up.

mod common;
#[path = "support/hub.rs"]
mod hub;
#[path = "support/term.rs"]
mod term;

use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use gayanshop_lib::auth::guard::Actor;
use gayanshop_lib::auth::pin::PinHasher;
use gayanshop_lib::db::Db;
use gayanshop_lib::db::repo::batches::{self, AdHocPurchase};
use gayanshop_lib::db::repo::staff::{self, NewStaff, StaffInput};
use gayanshop_lib::setup;
use gayanshop_lib::sync::api::HttpApi;
use gayanshop_lib::sync::apply::TABLES;
use gayanshop_lib::sync::bootstrap;
use gayanshop_lib::sync::config::ServerConfig;
use gayanshop_lib::sync::credentials::MemoryStore;
use gayanshop_lib::sync::registration::{self, RegisterInput};
use serde_json::json;
use term::{Term, derived, line, t01};
use tokio::sync::Mutex;

/// The tests share one proxy; they take turns.
static PROXY: Mutex<()> = Mutex::const_new(());

const PROXY_API: &str = "http://127.0.0.1:8474";
const PROXY_NAME: &str = "gayanshop";
const PROXY_PORT: u16 = 26543;
const OWNER_PASSWORD: &str = "Chaos-owner-pass-1";

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn run(cmd: &str) -> String {
    let out = Command::new("cmd").args(["/C", cmd]).current_dir(repo_root()).output().expect("run a command");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Stack {
    /// The API through toxiproxy: what the terminals use.
    url: String,
    /// The API directly (admin calls of the test itself).
    direct: String,
    anon: String,
    service: String,
}

/// The local stack's addresses and keys, from `supabase status` (never written to a file).
async fn stack() -> Stack {
    let env: HashMap<String, String> = run("pnpm exec supabase status -o env")
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().trim_matches('"').to_owned()))
        .collect();
    let direct = env.get("API_URL").cloned().unwrap_or_default();
    assert!(
        direct.starts_with("http://127.0.0.1:"),
        "the chaos suite runs only against the local Supabase stack; `supabase status` gave {direct:?}. Start it first."
    );
    let anon = env.get("ANON_KEY").cloned().expect("ANON_KEY from supabase status");
    let service = env.get("SERVICE_ROLE_KEY").cloned().expect("SERVICE_ROLE_KEY from supabase status");
    ensure_proxy(&direct).await;
    Stack { url: format!("http://127.0.0.1:{PROXY_PORT}"), direct, anon, service }
}

/// Start toxiproxy (Docker) if needed and point a clean proxy at the local API.
async fn ensure_proxy(direct: &str) {
    if run("docker inspect -f {{.State.Running}} gayanshop-toxiproxy").trim() != "true" {
        run("docker rm -f gayanshop-toxiproxy");
        run(&format!(
            "docker run -d --name gayanshop-toxiproxy -p 8474:8474 -p {PROXY_PORT}:{PROXY_PORT} ghcr.io/shopify/toxiproxy:2.9.0"
        ));
        for _ in 0..50 {
            if reqwest::get(format!("{PROXY_API}/version")).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    let http = reqwest::Client::new();
    http.post(format!("{PROXY_API}/reset")).send().await.expect("toxiproxy reachable");
    let port = direct.rsplit(':').next().unwrap();
    let _ = http.delete(format!("{PROXY_API}/proxies/{PROXY_NAME}")).send().await;
    let res = http
        .post(format!("{PROXY_API}/proxies"))
        .json(&json!({ "name": PROXY_NAME, "listen": format!("0.0.0.0:{PROXY_PORT}"),
                       "upstream": format!("host.docker.internal:{port}"), "enabled": true }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "toxiproxy proxy: {}", res.status());
}

async fn network(up: bool) {
    let res = reqwest::Client::new()
        .post(format!("{PROXY_API}/proxies/{PROXY_NAME}"))
        .json(&json!({ "enabled": up }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
}

/// Requests reach the server but no answer comes back (the network drops mid-push).
async fn drop_answers(on: bool) {
    let http = reqwest::Client::new();
    if on {
        let res = http
            .post(format!("{PROXY_API}/proxies/{PROXY_NAME}/toxics"))
            .json(&json!({ "name": "drop_answers", "type": "timeout", "stream": "downstream",
                           "attributes": { "timeout": 0 } }))
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success());
    } else {
        http.delete(format!("{PROXY_API}/proxies/{PROXY_NAME}/toxics/drop_answers")).send().await.unwrap();
    }
}

/// A fresh owner account (as the owner would create it in the dashboard).
async fn owner_account(s: &Stack) -> String {
    let email = format!("owner-{}@chaos.test", gayanshop_lib::clock::new_id());
    let res = reqwest::Client::new()
        .post(format!("{}/auth/v1/admin/users", s.direct))
        .header("apikey", &s.service)
        .bearer_auth(&s.service)
        .json(&json!({ "email": email, "password": OWNER_PASSWORD, "email_confirm": true }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "create owner: {}", res.status());
    email
}

fn input(s: &Stack, email: &str) -> RegisterInput {
    RegisterInput {
        server_url: s.url.clone(),
        anon_key: s.anon.clone(),
        owner_email: email.into(),
        owner_password: OWNER_PASSWORD.into(),
        device_name: None,
    }
}

/// A terminal and its connection to the real server.
struct Counter {
    t: Term,
    api: HttpApi,
}

impl Counter {
    async fn round(&self) -> bool {
        match self.t.round_with(&self.api).await {
            Ok(o) => o.pushed.sent > 0 || o.pulled.rows > 0,
            Err(e) => panic!("{}: round failed: {e}", self.t.code),
        }
    }
}

fn api(s: &Stack, t: &Term, secrets: Arc<MemoryStore>) -> HttpApi {
    HttpApi::with_timeout(ServerConfig::parse(&s.url, &s.anon).unwrap(), &t.device_id, secrets, Duration::from_secs(4))
        .unwrap()
}

/// T01: set up offline, then registered by the owner, adopting its ids.
async fn first(s: &Stack, email: &str) -> Counter {
    let t = t01().await;
    let secrets = Arc::new(MemoryStore::new());
    let identity = setup::load_identity(&t.db.pool).await.unwrap().unwrap();
    registration::register_this_counter(&t.db, &t.clock, secrets.as_ref(), &identity, &t.owner, &input(s, email))
        .await
        .unwrap();
    let api = api(s, &t, secrets);
    Counter { t, api }
}

/// T02/T03: "Join an existing store" from first run, then the full-history bootstrap (pages of `page`).
async fn join(s: &Stack, email: &str, page: i64) -> Counter {
    let empty = Term::empty().await;
    let secrets = Arc::new(MemoryStore::new());
    let r = registration::join_store(&empty.db, &empty.clock, secrets.as_ref(), &input(s, email), "en").await.unwrap();
    let mut t = empty;
    t.device_id = r.device_id.clone();
    t.store_id = r.store_id.clone();
    t.code = r.device_code.clone();
    let api = api(s, &t, secrets);
    bootstrap::run(&t.db, &api, &t.clock, &t.device_id, page, &|_| {}).await.unwrap();
    t.owner = sqlx::query_scalar(
        "SELECT s.id FROM staff s JOIN roles r ON r.id = s.role_id WHERE r.name = 'Owner' AND r.is_system = 1
         ORDER BY s.created_at LIMIT 1",
    )
    .fetch_one(&t.db.pool)
    .await
    .unwrap();
    t.open_session().await;
    Counter { t, api }
}

/// Rounds until a whole pass over every counter moves nothing and no outbox still holds a queued op (an op
/// waiting out its backoff counts: the clocks jump past it).
async fn settle(counters: &[&Counter]) {
    for _ in 0..20 {
        let mut moved = false;
        let mut queued = 0;
        for c in counters {
            moved |= c.round().await;
            queued += c.t.count("SELECT COUNT(*) FROM sync_outbox WHERE status IN ('pending', 'failed')").await;
        }
        if !moved && queued == 0 {
            return;
        }
        for c in counters {
            c.t.tick(300);
        }
    }
    let mut stuck = Vec::new();
    for c in counters {
        let rows: Vec<(String, String, i64, Option<String>)> =
            sqlx::query_as("SELECT table_name, status, attempts, last_error FROM sync_outbox ORDER BY seq LIMIT 5")
                .fetch_all(&c.t.db.pool)
                .await
                .unwrap();
        stuck.push(format!("{}: {rows:?}", c.t.code));
    }
    panic!("sync did not settle: {stuck:?}");
}

/// Every synced row of a terminal, as canonical text: equal on two terminals ⇔ byte-identical rows.
async fn rows(db: &Db) -> Vec<String> {
    let mut out = Vec::new();
    for (table, _, pk) in TABLES {
        let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
            .bind(table)
            .fetch_all(&db.pool)
            .await
            .unwrap();
        let pairs = cols.iter().map(|c| format!("'{c}', \"{c}\"")).collect::<Vec<_>>().join(", ");
        let q = format!("SELECT '{table} ' || json_object({pairs}) FROM \"{table}\" ORDER BY \"{pk}\"");
        out.extend(sqlx::query_scalar::<_, String>(&q).fetch_all(&db.pool).await.unwrap());
    }
    out
}

fn psql(sql: &str) -> Vec<String> {
    let out = Command::new("docker")
        .args(["exec", "supabase_db_GayanShop", "psql", "-U", "postgres", "-At", "-c", sql])
        .output()
        .expect("docker exec psql");
    assert!(out.status.success(), "psql: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).filter(|l| !l.is_empty()).collect()
}

/// The store-wide numbers as the cloud's own views compute them, in the same shape as `local_numbers`.
fn cloud_numbers(store: &str) -> Vec<String> {
    let mut out = psql(&format!(
        "SELECT 'dss|' || business_date || '|' || device_id || '|' || sales_count || '|' || void_count || '|' ||
                net_sales || '|' || cogs || '|' || customer_orders
         FROM pos.daily_sales_summary WHERE store_id = '{store}' ORDER BY business_date, device_id"
    ));
    out.extend(psql(&format!(
        "SELECT 'sl|' || product_id || '|' || on_hand FROM pos.stock_levels WHERE store_id = '{store}' ORDER BY product_id"
    )));
    out.extend(psql(&format!(
        "SELECT 'bis|' || batch_item_id || '|' || on_hand || '|' || sold_qty || '|' || sold_revenue || '|' || sold_cost
         FROM pos.batch_item_stock WHERE store_id = '{store}' ORDER BY batch_item_id"
    )));
    out.extend(psql(&format!(
        "SELECT 'cs|' || customer_id || '|' || visit_count || '|' || total_spend FROM pos.customer_stats
         WHERE store_id = '{store}' ORDER BY customer_id"
    )));
    out
}

async fn local_numbers(db: &Db) -> Vec<String> {
    let q = [
        "SELECT 'dss|' || business_date || '|' || device_id || '|' || sales_count || '|' || void_count || '|' ||
                net_sales || '|' || cogs || '|' || customer_orders
         FROM daily_sales_summary WHERE sales_count <> 0 OR void_count <> 0 ORDER BY business_date, device_id",
        "SELECT 'sl|' || product_id || '|' || on_hand FROM stock_levels ORDER BY product_id",
        "SELECT 'bis|' || batch_item_id || '|' || on_hand || '|' || sold_qty || '|' || sold_revenue || '|' || sold_cost
         FROM batch_item_stock ORDER BY batch_item_id",
        "SELECT 'cs|' || customer_id || '|' || visit_count || '|' || total_spend FROM customer_stats
         WHERE visit_count <> 0 ORDER BY customer_id",
    ];
    let mut out = Vec::new();
    for sql in q {
        out.extend(sqlx::query_scalar::<_, String>(sql).fetch_all(&db.pool).await.unwrap());
    }
    out
}

/// Cloud and every terminal agree: identical derived numbers, equal to the cloud's views, and every synced row
/// byte-identical on every terminal.
async fn assert_agree(counters: &[&Counter]) {
    let expected = derived(&counters[0].t.db).await;
    let cloud = cloud_numbers(&counters[0].t.store_id);
    let identical = rows(&counters[0].t.db).await;
    for c in counters {
        assert_eq!(derived(&c.t.db).await, expected, "{}: the store-wide numbers", c.t.code);
        assert_eq!(local_numbers(&c.t.db).await, cloud, "{}: the same numbers as the cloud's views", c.t.code);
        let mine = rows(&c.t.db).await;
        if mine != identical {
            let diff: Vec<_> = mine.iter().zip(&identical).filter(|(a, b)| a != b).take(3).collect();
            panic!(
                "{}: synced rows differ from {}: {diff:?} ({} vs {})",
                c.t.code,
                counters[0].t.code,
                mine.len(),
                identical.len()
            );
        }
        assert_eq!(c.t.count("SELECT COUNT(*) FROM sync_outbox").await, 0, "{}: nothing left to push", c.t.code);
    }
}

#[tokio::test]
#[ignore = "chaos suite: needs the local Supabase stack and Docker"]
async fn two_hundred_offline_sales_per_terminal_reconcile() {
    let _turn = PROXY.lock().await;
    let s = stack().await;
    let email = owner_account(&s).await;
    let a = first(&s, &email).await;
    let bucket = a.t.product("Bucket", 45_000, None, Some((400, 20_000))).await;
    let mat = a.t.product("Door mat", 15_000, None, Some((300, 5_000))).await;
    a.round().await;
    let b = join(&s, &email, 500).await;
    let c = join(&s, &email, 500).await;
    settle(&[&a, &b, &c]).await;

    network(false).await;
    for i in 0..200 {
        let phone = (i % 25 == 0).then(|| format!("07712{:05}", i));
        a.t.sell(&format!("a{i}"), vec![line(&bucket, 45_000, 1)], phone.as_deref()).await;
        b.t.sell(&format!("b{i}"), vec![line(&mat, 15_000, 1)], phone.as_deref()).await;
        c.t.sell(&format!("c{i}"), vec![line(&bucket, 45_000, 1), common::quick("Gift wrap", 2_000, 1)], None).await;
    }
    // Offline, a round fails and keeps everything queued.
    assert!(a.t.round_with(&a.api).await.is_err());
    assert!(a.t.count("SELECT COUNT(*) FROM sync_outbox").await > 200);
    network(true).await;
    settle(&[&a, &b, &c]).await;

    assert_agree(&[&a, &b, &c]).await;
    for x in [&a, &b, &c] {
        assert_eq!(x.t.count("SELECT COUNT(*) FROM orders").await, 600);
        assert_eq!(x.t.count("SELECT COUNT(*) FROM customers").await, 8, "the same phones on two counters converge");
    }
    let store = &a.t.store_id;
    assert_eq!(psql(&format!("SELECT count(*) FROM pos.orders WHERE store_id = '{store}'")), vec!["600"]);
}

#[tokio::test]
#[ignore = "chaos suite: needs the local Supabase stack and Docker"]
async fn killing_the_network_mid_push_neither_duplicates_nor_loses() {
    let _turn = PROXY.lock().await;
    let s = stack().await;
    let email = owner_account(&s).await;
    let a = first(&s, &email).await;
    let p = a.t.product("Bucket", 45_000, None, Some((100, 20_000))).await;
    a.round().await;
    for i in 0..40 {
        a.t.sell(&format!("s{i}"), vec![line(&p, 45_000, 1)], None).await;
    }
    let before = a.t.count("SELECT COUNT(*) FROM sync_outbox").await;
    // The push reaches the server, the answers never come back.
    drop_answers(true).await;
    assert!(a.t.round_with(&a.api).await.is_err(), "the client times out");
    drop_answers(false).await;
    assert_eq!(
        a.t.count("SELECT COUNT(*) FROM sync_outbox").await,
        before,
        "nothing leaves the outbox without an answer"
    );
    assert!(before >= 40 * 4, "order, line, payment and slice per sale");
    // Retry after the backoff: answered from the idempotency ledger.
    a.t.tick(600);
    settle(&[&a]).await;
    let store = &a.t.store_id;
    assert_eq!(psql(&format!("SELECT count(*) FROM pos.orders WHERE store_id = '{store}'")), vec!["40"]);
    assert_eq!(psql(&format!("SELECT count(*) FROM pos.order_payments WHERE store_id = '{store}'")), vec!["40"]);
    let local_audit = a.t.count("SELECT COUNT(*) FROM audit_logs").await;
    assert_eq!(
        psql(&format!("SELECT count(*) FROM pos.audit_logs WHERE store_id = '{store}'")),
        vec![local_audit.to_string()]
    );
    assert_eq!(a.t.count("SELECT COUNT(*) FROM sync_outbox").await, 0);
    assert_eq!(local_numbers(&a.t.db).await, cloud_numbers(store));
}

#[tokio::test]
#[ignore = "chaos suite: needs the local Supabase stack and Docker"]
async fn three_counters_oversell_last_units() {
    let _turn = PROXY.lock().await;
    let s = stack().await;
    let email = owner_account(&s).await;
    let a = first(&s, &email).await;
    let shoe = a.t.product("Shoe size 42", 250_000, None, Some((2, 120_000))).await;
    a.round().await;
    let b = join(&s, &email, 500).await;
    let c = join(&s, &email, 500).await;
    settle(&[&a, &b, &c]).await;
    let layer: String = a.t.text(&format!("SELECT id FROM batch_items WHERE product_id = '{shoe}'")).await;

    network(false).await;
    a.t.sell("a1", vec![line(&shoe, 250_000, 1)], None).await;
    b.t.sell("b1", vec![line(&shoe, 250_000, 1)], None).await;
    c.t.sell("c1", vec![line(&shoe, 250_000, 1)], None).await;
    b.t.sell("b2", vec![line(&shoe, 250_000, 1)], None).await;
    network(true).await;
    settle(&[&a, &b, &c]).await;

    // Identical ledger on three nodes and the cloud: the layer shows −2, and it's flagged.
    for x in [&a, &b, &c] {
        assert_eq!(
            x.t.count(&format!("SELECT on_hand FROM batch_item_stock WHERE batch_item_id = '{layer}'")).await,
            -2
        );
        assert_eq!(x.t.count("SELECT COUNT(*) FROM batch_item_stock WHERE on_hand < 0").await, 1, "{}", x.t.code);
    }
    let store = &a.t.store_id;
    assert_eq!(
        psql(&format!(
            "SELECT on_hand FROM pos.batch_item_stock WHERE store_id = '{store}' AND batch_item_id = '{layer}'"
        )),
        vec!["-2"]
    );
    assert_agree(&[&a, &b, &c]).await;
    // The next sale draws from the next layer.
    let buy = AdHocPurchase { product_id: shoe.clone(), qty: 5, unit_cost: 125_000, supplier_name: None, note: None };
    batches::ad_hoc_purchase(&a.t.db, &a.t.ctx(), &a.t.actor(), &buy).await.unwrap();
    let o = a.t.sell("a2", vec![line(&shoe, 250_000, 1)], None).await;
    let used: String =
        a.t.text(&format!(
            "SELECT m.batch_item_id FROM stock_movements m JOIN order_items oi ON oi.id = m.order_item_id
             WHERE oi.order_id = '{o}' AND m.reason = 'sale'"
        ))
        .await;
    assert_ne!(used, layer, "FIFO moves on to the next layer");
}

#[tokio::test]
#[ignore = "chaos suite: needs the local Supabase stack and Docker"]
async fn a_void_and_a_shared_phone_reach_every_counter() {
    let _turn = PROXY.lock().await;
    let s = stack().await;
    let email = owner_account(&s).await;
    let a = first(&s, &email).await;
    let p = a.t.product("Bucket", 45_000, None, Some((50, 20_000))).await;
    a.round().await;
    let b = join(&s, &email, 500).await;
    let c = join(&s, &email, 500).await;
    settle(&[&a, &b, &c]).await;

    network(false).await;
    a.t.sell("a1", vec![line(&p, 45_000, 1)], Some("0779876543")).await;
    let o = b.t.sell("b1", vec![line(&p, 45_000, 2)], Some("077 987 6543")).await;
    network(true).await;
    settle(&[&a, &b, &c]).await;
    b.t.void(&o).await;
    settle(&[&a, &b, &c]).await;

    let cid: String = a.t.text("SELECT id FROM customers").await;
    for x in [&a, &b, &c] {
        assert_eq!(x.t.count("SELECT COUNT(*) FROM customers").await, 1, "{}: one customer", x.t.code);
        assert_eq!(x.t.count(&format!("SELECT status = 'voided' FROM orders WHERE id = '{o}'")).await, 1);
        assert_eq!(
            x.t.count("SELECT SUM(sales_count) FROM daily_sales_summary").await,
            1,
            "{}: the void is excluded",
            x.t.code
        );
        assert_eq!(
            x.t.count(&format!("SELECT COUNT(*) FROM orders WHERE customer_id = '{cid}'")).await,
            2,
            "{}: both orders are in the customer's history",
            x.t.code
        );
    }
    assert_agree(&[&a, &b, &c]).await;
}

#[tokio::test]
#[ignore = "chaos suite: needs the local Supabase stack and Docker"]
async fn a_day_of_clock_skew_on_one_counter_changes_nothing() {
    let _turn = PROXY.lock().await;
    let s = stack().await;
    let email = owner_account(&s).await;
    let a = first(&s, &email).await;
    let p = a.t.product("Bucket", 45_000, None, Some((50, 20_000))).await;
    a.round().await;
    let b = join(&s, &email, 500).await;
    let c = join(&s, &email, 500).await;
    c.t.tick(86_400); // T03 runs a day ahead
    settle(&[&a, &b, &c]).await;
    for i in 0..10 {
        a.t.sell(&format!("a{i}"), vec![line(&p, 45_000, 1)], None).await;
        c.t.sell(&format!("c{i}"), vec![line(&p, 45_000, 1)], None).await;
    }
    settle(&[&a, &b, &c]).await;
    assert_agree(&[&a, &b, &c]).await;
    assert_eq!(b.t.count("SELECT COUNT(*) FROM orders").await, 20);
}

#[tokio::test]
#[ignore = "chaos suite: needs the local Supabase stack and Docker"]
async fn bootstrap_in_pages_of_seven_after_parents_were_edited() {
    let _turn = PROXY.lock().await;
    let s = stack().await;
    let email = owner_account(&s).await;
    let a = first(&s, &email).await;
    let bucket = a.t.product("Bucket", 45_000, None, None).await;
    let mat = a.t.product("Door mat", 15_000, None, Some((30, 5_000))).await;
    a.round().await;
    a.t.reworked_batch(&bucket, 20, 20_000).await;
    let mut voids = Vec::new();
    for i in 0..12 {
        let o = a.t.sell(&format!("h{i}"), vec![line(&bucket, 45_000, 1), line(&mat, 15_000, 2)], None).await;
        if i % 4 == 0 {
            voids.push(o);
        }
    }
    a.round().await;
    for o in &voids {
        a.t.void(o).await;
    }
    a.t.edit_price(&bucket, 47_000).await;
    settle(&[&a]).await;
    // Batches and layers edited after their movements, orders voided after their payments: every page of 7
    // still applies, because the server ships each row's newer ancestors with it.
    let b = join(&s, &email, 7).await;
    let dangling: Vec<(String,)> =
        sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check").fetch_all(&b.t.db.pool).await.unwrap();
    assert!(dangling.is_empty(), "{dangling:?}");
    assert_eq!(derived(&b.t.db).await, derived(&a.t.db).await);
    assert_eq!(term::rebuilt(&b.t.db).await, derived(&a.t.db).await);
    settle(&[&a, &b]).await;
    assert_agree(&[&a, &b]).await;
}

#[tokio::test]
#[ignore = "chaos suite: needs the local Supabase stack and Docker"]
async fn concurrent_owner_demotions_are_rejected_by_the_invariant() {
    let _turn = PROXY.lock().await;
    let s = stack().await;
    let email = owner_account(&s).await;
    let a = first(&s, &email).await;
    let owner_role = a.t.text("SELECT id FROM roles WHERE name = 'Owner'").await;
    let cashier_role = a.t.text("SELECT id FROM roles WHERE name = 'Cashier'").await;
    let staff_input = |name: &str, role: &str| StaffInput {
        display_name: name.into(),
        role_id: role.into(),
        ui_language: "en".into(),
        grants: vec![],
        denies: vec![],
        max_discount_bp: None,
        is_active: true,
    };
    let second = staff::create(
        &a.t.db,
        &a.t.ctx(),
        &a.t.actor(),
        &PinHasher::fast_for_tests(),
        &NewStaff { staff: staff_input("Nimal", &owner_role), pin: "135790".into() },
    )
    .await
    .unwrap()
    .id;
    a.round().await;
    let b = join(&s, &email, 500).await;
    settle(&[&a, &b]).await;

    // Offline, each counter demotes a different Owner: each is fine locally (another Owner remains).
    network(false).await;
    a.t.tick(60);
    staff::update(&a.t.db, &a.t.ctx(), &a.t.actor(), &a.t.owner, &staff_input("Gayan", &cashier_role)).await.unwrap();
    b.t.tick(120);
    let nimal = Actor { staff_id: second.clone(), approved_by: None };
    staff::update(&b.t.db, &b.t.ctx(), &nimal, &second, &staff_input("Nimal", &cashier_role)).await.unwrap();
    network(true).await;
    a.round().await;
    b.round().await;
    // T02's demotion would leave no Owner: the cloud refuses it, T02 shows it as refused…
    assert_eq!(b.t.count("SELECT COUNT(*) FROM sync_outbox WHERE status = 'dead' AND table_name = 'staff'").await, 1);
    let reason = b.t.text("SELECT last_error FROM sync_outbox WHERE status = 'dead'").await;
    assert!(reason.starts_with("OWNER_INVARIANT"), "{reason}");
    // …and takes the cloud's version back, so every counter and the cloud keep exactly one active Owner.
    settle(&[&a, &b]).await;
    let owners = "SELECT COUNT(*) FROM staff s JOIN roles r ON r.id = s.role_id
                  WHERE r.name = 'Owner' AND s.is_active = 1 AND s.deleted_at IS NULL";
    for x in [&a, &b] {
        assert_eq!(x.t.count(owners).await, 1, "{}", x.t.code);
        assert_eq!(x.t.count(&format!("SELECT role_id = '{owner_role}' FROM staff WHERE id = '{second}'")).await, 1);
    }
    let store = &a.t.store_id;
    assert_eq!(
        psql(&format!(
            "SELECT count(*) FROM pos.staff s JOIN pos.roles r ON r.id = s.role_id
             WHERE s.store_id = '{store}' AND r.name = 'Owner' AND s.is_active AND s.deleted_at IS NULL"
        )),
        vec!["1"]
    );
}

#[tokio::test]
#[ignore = "chaos suite: needs the local Supabase stack and Docker"]
async fn editing_an_audit_row_in_a_terminal_file_is_flagged() {
    let _turn = PROXY.lock().await;
    let s = stack().await;
    let email = owner_account(&s).await;
    let a = first(&s, &email).await;
    let p = a.t.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    a.t.sell("s1", vec![line(&p, 45_000, 1)], None).await;
    // Someone with access to T01's database file rewrites an audit entry, before it syncs.
    let original = a.t.text("SELECT metadata FROM audit_logs WHERE device_seq = 2").await;
    sqlx::query("DROP TRIGGER trg_audit_no_update").execute(&a.t.db.pool).await.unwrap();
    sqlx::query(r#"UPDATE audit_logs SET metadata = '{"forged":true}' WHERE device_seq = 2"#)
        .execute(&a.t.db.pool)
        .await
        .unwrap();
    // T01's chain verification flags the edited entry.
    let own = gayanshop_lib::audit::verify(&a.t.db.pool).await.unwrap();
    let mine = own.iter().find(|r| r.device_id == a.t.device_id).unwrap();
    assert!(!mine.ok);
    assert_eq!(mine.first_break.as_ref().map(|b| b.device_seq), Some(2));
    // And the forgery doesn't spread: the outbox carries each entry as it was written, so the cloud and the
    // other counters keep the true record, whose chain verifies.
    a.round().await;
    let b = join(&s, &email, 500).await;
    let reports = gayanshop_lib::audit::verify(&b.t.db.pool).await.unwrap();
    assert!(reports.iter().find(|r| r.device_id == a.t.device_id).unwrap().ok);
    let theirs =
        b.t.text(&format!("SELECT metadata FROM audit_logs WHERE device_id = '{}' AND device_seq = 2", a.t.device_id))
            .await;
    assert_eq!(theirs, original);
    let store = &a.t.store_id;
    let cloud = psql(&format!(
        "SELECT metadata FROM pos.audit_logs WHERE store_id = '{store}' AND device_id = '{}' AND device_seq = 2",
        a.t.device_id
    ));
    assert_eq!(cloud, vec![original]);
}
