//! Block 3.4: the Rust sync worker (doc §6). Three terminal databases sell through the real checkout, void
//! and back-office code, and sync through an in-process stand-in for the server (`support/hub.rs`). The
//! tests cover pull apply (topological order, deferred foreign keys, the applying flag, idempotency, no echo,
//! no audit), the merge rules on the terminal (orders and voids, last-write-wins, barcode collisions,
//! customers and erasure), the outbox push (ok, dead, retry, network loss mid-push, crash recovery), and the
//! worker's status. The server's own rules are pgTAP-tested; the real stack runs in the chaos suite.

mod common;
#[path = "support/hub.rs"]
mod hub;
#[path = "support/term.rs"]
mod term;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gayanshop_lib::clock::{Clock, iso};
use gayanshop_lib::db::Db;
use gayanshop_lib::db::repo::customers;
use gayanshop_lib::db::repo::products::{self, CodeChoice, ProductInput, ProductKind};
use gayanshop_lib::sync::api::SyncApi;
use gayanshop_lib::sync::apply;
use gayanshop_lib::sync::config::{self, ServerConfig};
use gayanshop_lib::sync::credentials::{MemoryStore, SecretStore, refresh_token_account};
use gayanshop_lib::sync::engine::{self, PullOptions, RoundError};
use gayanshop_lib::sync::wire::{PullPage, PulledRow};
use gayanshop_lib::sync::worker::{Connector, SyncEvents, SyncHandle, SyncState, SyncStatus, Worker};
use serde_json::Value;
use term::{SETUP_AT, Term, derived, hub_for, joined, line, outbox, rebuilt, t01};

#[tokio::test]
async fn three_counters_sell_offline_and_converge() {
    let a = t01().await;
    let shoes = a.product("Shoe size 42", 250_000, Some("4790000000013"), Some((6, 120_000))).await;
    let mat = a.product("Door mat", 45_000, None, Some((20, 15_000))).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    assert_eq!(outbox(&a.db).await, 0, "everything T01 wrote is in the cloud");
    let b = joined(&hub, &a.store_id, "T02").await;
    let c = joined(&hub, &a.store_id, "T03").await;
    b.round(&hub).await;
    c.round(&hub).await;

    // Offline: each counter sells; T01 and T02 capture the same new phone number; T03 sells a quick item.
    let mut voided = None;
    for i in 0..8 {
        let o = a.sell(&format!("a{i}"), vec![line(&mat, 45_000, 1)], (i == 0).then_some("0771234567")).await;
        if i == 3 {
            voided = Some(o);
        }
        b.sell(&format!("b{i}"), vec![line(&mat, 45_000, 2)], (i == 0).then_some("077 123 4567")).await;
        c.sell(&format!("c{i}"), vec![line(&shoes, 250_000, 1), common::quick("Gift wrap", 5_000, 1)], None).await;
    }
    a.void(voided.as_deref().unwrap()).await;

    // Reconnect in any order; two rounds each reach a fixed point.
    for _ in 0..2 {
        for t in [&c, &a, &b] {
            t.round(&hub).await;
        }
    }

    let expected = derived(&a.db).await;
    assert!(!expected.is_empty());
    assert_eq!(derived(&b.db).await, expected, "T02 shows the same store-wide numbers as T01");
    assert_eq!(derived(&c.db).await, expected, "and so does T03");
    assert_eq!(rebuilt(&a.db).await, expected, "the incremental rollups equal a rebuild from base rows");
    for t in [&a, &b, &c] {
        assert_eq!(outbox(&t.db).await, 0, "{}: nothing left to push", t.code);
        assert_eq!(t.count("SELECT COUNT(*) FROM orders").await, 24, "{}: every order everywhere", t.code);
        assert_eq!(t.count("SELECT COUNT(*) FROM orders WHERE status = 'voided'").await, 1);
        // Oversold offline (§13.1): shoes 6 received, T03 sold 8 → −2; mats 20 received, T01 sold 7 (net of
        // the void) and T02 sold 16 → −3. Both layers show it identically everywhere, flagged.
        assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{shoes}'")).await, -2);
        assert_eq!(t.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{mat}'")).await, -3);
        assert_eq!(t.count("SELECT COUNT(*) FROM batch_item_stock WHERE on_hand < 0").await, 2);
        // The same phone captured on two counters is one customer with both visits.
        assert_eq!(t.count("SELECT COUNT(*) FROM customers").await, 1, "{}", t.code);
        assert_eq!(t.count("SELECT visit_count FROM customer_stats").await, 2, "{}", t.code);
        assert!(
            gayanshop_lib::audit::verify(&t.db.pool).await.unwrap().iter().all(|r| r.ok),
            "{}: chains intact",
            t.code
        );
    }
    assert_eq!(hub.count("orders"), 24, "the cloud holds each order exactly once");
}

#[tokio::test]
async fn pulled_rows_never_echo_and_write_no_audit() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    a.sell("s1", vec![line(&p, 45_000, 2)], Some("0712345678")).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = joined(&hub, &a.store_id, "T02").await;
    let before_outbox = outbox(&b.db).await;
    let own_audit = b.count(&format!("SELECT COUNT(*) FROM audit_logs WHERE device_id = '{}'", b.device_id)).await;
    assert_eq!(
        b.count(&format!("SELECT COUNT(*) FROM audit_logs WHERE device_id = '{}'", a.device_id)).await,
        a.count("SELECT COUNT(*) FROM audit_logs").await,
        "T01's audit rows arrive as they are"
    );
    // Pull everything again from the start: nothing is applied twice and nothing is queued or audited.
    let snapshot = derived(&b.db).await;
    let opts = PullOptions { page_size: 3, max_pages: 10_000, include_own: true };
    let wipe_cursor = "UPDATE sync_state SET last_server_seq = 0";
    sqlx::query(wipe_cursor).execute(&b.db.pool).await.unwrap();
    engine::pull(&b.db, &hub.client(&b.device_id), &b.clock, &b.device_id, opts, &|_| {}).await.unwrap();
    assert_eq!(derived(&b.db).await, snapshot, "applying the same rows twice changes nothing");
    assert_eq!(outbox(&b.db).await, before_outbox, "pulled rows are never echoed back to the outbox");
    assert_eq!(
        b.count(&format!("SELECT COUNT(*) FROM audit_logs WHERE device_id = '{}'", b.device_id)).await,
        own_audit,
        "applying writes no audit rows"
    );
    assert_eq!(b.count("SELECT applying FROM sync_ctx").await, 0);
}

#[tokio::test]
async fn a_page_applies_whatever_its_row_order() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    a.sell("s1", vec![line(&p, 45_000, 2)], None).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    // Every row the cloud holds, children first: the terminal sorts parents first and defers foreign keys.
    let all = hub.client("someone-else").pull(0, 100_000, true).await.unwrap();
    let mut rows = all.rows.clone();
    rows.reverse();
    let page = PullPage { rows, cursor: all.cursor, has_more: false, watermark: all.watermark };
    let dir = tempfile::tempdir().unwrap();
    let db =
        Db::open(&dir.path().join(gayanshop_lib::db::DB_FILE_NAME), &dir.path().join("backups")).await.unwrap();
    let applied = apply::apply_page(&db, "t09", "2026-11-08T05:00:00.000Z", &page).await.unwrap();
    assert_eq!(applied.received, all.rows.len());
    assert_eq!(derived(&db).await, derived(&a.db).await);
    assert_eq!(apply::cursor(&db).await.unwrap(), all.cursor, "the cursor advanced with the page");
}

#[tokio::test]
async fn a_bad_page_is_not_applied_at_all() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    a.sell("s1", vec![line(&p, 45_000, 2)], None).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = joined(&hub, &a.store_id, "T02").await;
    let cursor = apply::cursor(&b.db).await.unwrap();
    let mut product = hub.row("products", &p).unwrap();
    product["updated_at"] = Value::from("2026-11-09T00:00:00.000Z");
    product["price"] = Value::from(1);
    let mut bad_line = hub.row("order_items", &a.text("SELECT id FROM order_items").await).unwrap();
    bad_line["id"] = Value::from(gayanshop_lib::clock::new_id());
    bad_line["line_gross"] = Value::from(7); // breaks CHECK (line_gross = unit_price × qty)
    let page = PullPage {
        rows: vec![
            PulledRow { seq: cursor + 1, table: "products".into(), row: product },
            PulledRow { seq: cursor + 2, table: "order_items".into(), row: bad_line },
        ],
        cursor: cursor + 2,
        has_more: false,
        watermark: cursor + 2,
    };
    assert!(apply::apply_page(&b.db, &b.device_id, "2026-11-09T00:00:01.000Z", &page).await.is_err());
    assert_eq!(apply::cursor(&b.db).await.unwrap(), cursor, "the cursor did not move");
    assert_eq!(b.count(&format!("SELECT price FROM products WHERE id = '{p}'")).await, 45_000, "nor any row");
    assert_eq!(b.count("SELECT applying FROM sync_ctx").await, 0, "and the applying flag is back to 0");
}

#[tokio::test]
async fn a_void_reaches_the_other_counters() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    let o = a.sell("s1", vec![line(&p, 45_000, 2)], Some("0712345678")).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = joined(&hub, &a.store_id, "T02").await;
    let sold = "SELECT COALESCE(SUM(sales_count), 0) FROM daily_sales_summary";
    assert_eq!(b.count(sold).await, 1);
    assert_eq!(b.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{p}'")).await, 8);
    a.void(&o).await;
    a.round(&hub).await;
    b.round(&hub).await;
    assert_eq!(b.count(&format!("SELECT status = 'voided' FROM orders WHERE id = '{o}'")).await, 1);
    assert_eq!(b.count(sold).await, 0, "the store-wide report on T02 excludes the void");
    assert_eq!(b.count("SELECT COALESCE(SUM(void_count), 0) FROM daily_sales_summary").await, 1);
    assert_eq!(
        b.count(&format!("SELECT on_hand FROM stock_levels WHERE product_id = '{p}'")).await,
        10,
        "stock restored"
    );
    assert_eq!(b.count("SELECT COALESCE(SUM(visit_count), 0) FROM customer_stats").await, 0);
    assert_eq!(derived(&b.db).await, derived(&a.db).await);
}

#[tokio::test]
async fn the_newer_edit_wins_on_every_counter() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, None).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = joined(&hub, &a.store_id, "T02").await;
    let edit = |t: &Term, price: i64| {
        let input = ProductInput {
            kind: ProductKind::Standard,
            name: "Bucket".into(),
            name_si: None,
            sku: CodeChoice::Generate,
            barcode: CodeChoice::None,
            category_id: None,
            default_cost: Some(0),
            price,
            min_price: None,
            track_stock: true,
            allow_negative_stock: true,
            attributes: std::collections::BTreeMap::new(),
            is_active: true,
        };
        let (db, ctx, actor, id) = (t.db.clone(), t.ctx(), t.actor(), p.clone());
        async move { products::update(&db, &ctx, &actor, &id, &input, true).await.unwrap() }
    };
    // Both edit offline; T02's edit is the later one, but T02 syncs first.
    a.tick(60);
    edit(&a, 46_000).await;
    b.tick(120);
    edit(&b, 47_000).await;
    b.round(&hub).await;
    a.round(&hub).await;
    b.round(&hub).await;
    let price = format!("SELECT price FROM products WHERE id = '{p}'");
    assert_eq!(a.count(&price).await, 47_000, "T01 takes T02's later edit");
    assert_eq!(b.count(&price).await, 47_000, "T02 keeps its own, T01's older edit loses");
}

#[tokio::test]
async fn the_same_barcode_on_two_counters_parks_one_product_and_rejects_neither() {
    let a = t01().await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = joined(&hub, &a.store_id, "T02").await;
    let pa = a.product("Bucket (T01)", 45_000, Some("4791234567891"), None).await;
    let pb = b.product("Bucket (T02)", 46_000, Some("4791234567891"), None).await;
    // T01 first; T02 is parked in the cloud, steps aside on its own pull, and pushes that change.
    a.round(&hub).await;
    b.round(&hub).await;
    b.round(&hub).await;
    a.round(&hub).await;
    for t in [&a, &b] {
        let active = t
            .count("SELECT COUNT(*) FROM products WHERE barcode = '4791234567891' AND is_active = 1 AND deleted_at IS NULL")
            .await;
        assert_eq!(active, 1, "{}: one live product holds the barcode", t.code);
        assert_eq!(t.count(&format!("SELECT is_active FROM products WHERE id = '{pa}'")).await, 1, "{}", t.code);
        assert_eq!(
            t.count(&format!("SELECT barcode_conflict FROM products WHERE id = '{pb}'")).await,
            1,
            "{}: the later one is flagged for the owner",
            t.code
        );
        assert_eq!(outbox(&t.db).await, 0, "{}: nothing was rejected", t.code);
    }
}

#[tokio::test]
async fn an_erasure_wins_over_a_later_edit() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    a.sell("s1", vec![line(&p, 45_000, 1)], Some("0712345678")).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = joined(&hub, &a.store_id, "T02").await;
    let id: String = sqlx::query_scalar("SELECT id FROM customers").fetch_one(&a.db.pool).await.unwrap();
    customers::erase(&a.db, &a.ctx(), &a.actor(), &id).await.unwrap();
    // T02 edits the name later, while offline.
    b.tick(600);
    let update = customers::CustomerUpdate { display_name: Some("Kamal".into()), note: None };
    customers::update(&b.db, &b.ctx(), &b.actor(), &id, &update).await.unwrap();
    a.round(&hub).await;
    b.round(&hub).await;
    a.round(&hub).await;
    for t in [&a, &b] {
        assert_eq!(
            t.count(&format!("SELECT erased_at IS NOT NULL AND phone_e164 IS NULL AND display_name IS NULL FROM customers WHERE id = '{id}'")).await,
            1,
            "{}: the customer stays erased", t.code
        );
    }
}

#[tokio::test]
async fn push_outcomes_ok_dead_and_retry() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    let hub = hub_for(&a).await;
    hub.reject_tables.lock().unwrap().insert("expense_categories".into());
    // An op whose parent the cloud doesn't have: retried, not dead.
    let orphan = serde_json::json!({
        "id": gayanshop_lib::clock::new_id(), "store_id": a.store_id, "device_id": a.device_id,
        "order_id": "0196f0c2-0000-7000-8000-00000000dead", "method": "cash", "amount": 100, "tendered": null,
        "reference": null, "created_at": "2026-11-08T03:41:00.000Z"
    });
    sqlx::query(
        "INSERT INTO sync_outbox (op_id, table_name, row_id, op, payload, next_attempt_at, created_at)
         VALUES (?1, 'order_payments', ?2, 'upsert', ?3, ?4, ?4)",
    )
    .bind(gayanshop_lib::clock::new_id())
    .bind(orphan["id"].as_str().unwrap())
    .bind(orphan.to_string())
    .bind(SETUP_AT)
    .execute(&a.db.pool)
    .await
    .unwrap();
    let out = a.round(&hub).await;
    assert_eq!(out.pushed.rejected, 8, "the eight seeded expense categories are rejected");
    assert_eq!(out.pushed.retried, 1);
    assert_eq!(out.pushed.ok, out.pushed.sent - 9);
    assert_eq!(
        a.count("SELECT COUNT(*) FROM sync_outbox WHERE status = 'dead' AND table_name = 'expense_categories'").await,
        8
    );
    let (status, attempts, next): (String, i64, String) =
        sqlx::query_as("SELECT status, attempts, next_attempt_at FROM sync_outbox WHERE table_name = 'order_payments'")
            .fetch_one(&a.db.pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), attempts), ("failed", 1));
    assert_eq!(next, iso(a.clock.now() + chrono::Duration::seconds(5)), "retried after 5 s, doubling after that");
    let details: String = sqlx::query_scalar(
        "SELECT details FROM sync_logs WHERE direction = 'push' AND result = 'partial' ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_one(&a.db.pool)
    .await
    .unwrap();
    assert!(details.contains("expense_categories"), "the rejections are logged for Settings → Sync");
    assert!(hub.row("products", &p).is_some());
    assert_eq!(engine::backoff(1).num_seconds(), 5);
    assert_eq!(engine::backoff(3).num_seconds(), 20);
    assert_eq!(engine::backoff(40).num_seconds(), 300, "capped at 5 minutes");
}

#[tokio::test]
async fn losing_the_network_mid_push_neither_duplicates_nor_loses() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    for i in 0..5 {
        a.sell(&format!("s{i}"), vec![line(&p, 45_000, 1)], None).await;
    }
    let hub = hub_for(&a).await;
    let queued = outbox(&a.db).await;
    // The server applies the batch but the answer never arrives.
    hub.lose_push_answers.store(1, std::sync::atomic::Ordering::SeqCst);
    a.tick(10);
    let err = engine::round(&a.db, &hub.client(&a.device_id), &a.clock, &a.device_id).await.unwrap_err();
    assert!(matches!(err, RoundError::Remote(_)));
    assert_eq!(outbox(&a.db).await, queued, "nothing is dropped from the outbox without an answer");
    assert_eq!(
        a.count("SELECT COUNT(*) FROM sync_outbox WHERE status = 'failed' AND attempts = 1").await,
        queued.min(200)
    );
    // Down entirely for the next try.
    hub.fail_pushes.store(1, std::sync::atomic::Ordering::SeqCst);
    a.tick(10);
    assert!(engine::round(&a.db, &hub.client(&a.device_id), &a.clock, &a.device_id).await.is_err());
    // Back: the replay is answered from the idempotency ledger.
    a.tick(600);
    for _ in 0..3 {
        a.round(&hub).await;
    }
    assert_eq!(outbox(&a.db).await, 0);
    assert_eq!(hub.count("orders"), 5, "each order exactly once");
    assert_eq!(hub.count("order_payments"), 5);
    let errors = a.count("SELECT COUNT(*) FROM sync_logs WHERE direction = 'push' AND result = 'error'").await;
    assert_eq!(errors, 2, "both failures are logged");
}

#[tokio::test]
async fn ops_in_flight_at_a_crash_are_sent_again() {
    let a = t01().await;
    let hub = hub_for(&a).await;
    sqlx::query("UPDATE sync_outbox SET status = 'in_flight'").execute(&a.db.pool).await.unwrap();
    assert_eq!(a.round(&hub).await.pushed.sent, 0, "in-flight ops wait for recovery");
    let recovered = engine::recover_in_flight(&a.db).await.unwrap();
    assert!(recovered > 0);
    a.round(&hub).await;
    assert_eq!(outbox(&a.db).await, 0);
}

#[tokio::test]
async fn the_cursor_persists_and_a_caught_up_pull_is_empty() {
    let a = t01().await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = joined(&hub, &a.store_id, "T02").await;
    let cursor = apply::cursor(&b.db).await.unwrap();
    assert!(cursor > 0);
    let again = b.round(&hub).await;
    assert_eq!(again.pulled.rows, 0, "caught up: nothing to apply");
    assert!(again.pulled.caught_up);
    assert!(apply::cursor(&b.db).await.unwrap() >= cursor);
}

struct Recorder(Mutex<Vec<SyncStatus>>, Mutex<Vec<&'static str>>);

impl SyncEvents for Recorder {
    fn status(&self, status: &SyncStatus) {
        self.0.lock().unwrap().push(status.clone());
    }

    fn applied(&self, tables: &[&'static str]) {
        self.1.lock().unwrap().extend_from_slice(tables);
    }
}

async fn wait_for(handle: &SyncHandle, state: SyncState) -> SyncStatus {
    wait_until(handle, |s| s.state == state).await
}

async fn wait_until(handle: &SyncHandle, ok: impl Fn(&SyncStatus) -> bool) -> SyncStatus {
    for _ in 0..200 {
        let s = handle.status();
        if ok(&s) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the worker never got there; last status {:?}", handle.status());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_worker_syncs_after_a_sale_and_reports_offline() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    let hub = hub_for(&a).await;
    let secrets = Arc::new(MemoryStore::new());
    let handle = SyncHandle::new();
    let events = Arc::new(Recorder(Mutex::new(vec![]), Mutex::new(vec![])));
    let connect_hub = hub.clone();
    let connect: Connector = Arc::new(move |_cfg: &ServerConfig, device: &str, _s: Arc<dyn SecretStore>| {
        Ok(Arc::new(connect_hub.client(device)) as Arc<dyn SyncApi>)
    });
    let worker = Worker {
        db: a.db.clone(),
        clock: Arc::new(gayanshop_lib::clock::SystemClock),
        secrets: secrets.clone(),
        events: events.clone(),
        handle: handle.clone(),
        connect,
    };
    let task = tokio::spawn(worker.run());
    // Unregistered: nothing is sent, the first run's rows wait in the outbox.
    wait_until(&handle, |s| s.state == SyncState::Unregistered && s.pending > 0).await;
    assert_eq!(hub.pushes(), 0);

    // Registered: the next round pushes everything.
    let mut tx = a.db.write().await.unwrap();
    config::save(tx.conn(), &ServerConfig::parse("http://127.0.0.1:1", "anon").unwrap(), SETUP_AT).await.unwrap();
    tx.commit().await.unwrap();
    secrets.set(&refresh_token_account(&a.device_id), "rt").unwrap();
    handle.poke();
    let synced = wait_for(&handle, SyncState::Synced).await;
    assert_eq!((synced.pending, synced.dead), (0, 0));
    assert!(synced.last_success_at.is_some());

    // A sale is pushed right away.
    a.sell("s1", vec![line(&p, 45_000, 1)], None).await;
    handle.poke();
    for _ in 0..200 {
        if hub.count("orders") == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(hub.count("orders"), 1, "the worker pushed the sale after the poke");

    // The network goes down: offline, and the sale stays queued.
    hub.fail_pushes.store(1_000, std::sync::atomic::Ordering::SeqCst);
    hub.fail_pulls.store(1_000, std::sync::atomic::Ordering::SeqCst);
    a.sell("s2", vec![line(&p, 45_000, 1)], None).await;
    handle.poke();
    let offline = wait_for(&handle, SyncState::Offline).await;
    assert!(offline.pending > 0);
    assert!(events.0.lock().unwrap().iter().any(|s| s.state == SyncState::Syncing));
    task.abort();
}

#[tokio::test]
async fn a_change_the_cloud_refuses_gives_way_to_the_cloud_version() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, None).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = joined(&hub, &a.store_id, "T02").await;
    b.edit_price(&p, 99_000).await;
    hub.reject_tables.lock().unwrap().insert("products".into());
    b.round(&hub).await;
    let price = format!("SELECT price FROM products WHERE id = '{p}'");
    assert_eq!(b.count("SELECT COUNT(*) FROM sync_outbox WHERE status = 'dead'").await, 1, "shown as refused");
    assert_eq!(b.count(&price).await, 45_000, "and the cloud's version replaced the refused edit");
}

#[tokio::test]
async fn a_newer_local_edit_after_a_refusal_is_not_overwritten() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, None).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = joined(&hub, &a.store_id, "T02").await;
    b.edit_price(&p, 99_000).await;
    hub.reject_tables.lock().unwrap().insert("products".into());
    b.round(&hub).await;
    hub.reject_tables.lock().unwrap().clear();
    // A new, legitimate edit on T02 that hasn't been pushed yet when the cloud's row comes round again.
    b.edit_price(&p, 50_000).await;
    hub.put("products", hub.row("products", &p).unwrap());
    let opts = PullOptions { page_size: 500, max_pages: 10, include_own: false };
    engine::pull(&b.db, &hub.client(&b.device_id), &b.clock, &b.device_id, opts, &|_| {}).await.unwrap();
    let price = format!("SELECT price FROM products WHERE id = '{p}'");
    assert_eq!(b.count(&price).await, 50_000, "the newer local edit stays");
    b.round(&hub).await;
    assert_eq!(hub.row("products", &p).unwrap()["price"], 50_000, "and reaches the cloud");
    a.round(&hub).await;
    assert_eq!(a.count(&price).await, 50_000, "and the other counters");
}
