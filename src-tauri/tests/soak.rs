//! Block 4.5 — Pilot readiness and soak test (doc §8 Phase 4):
//! - 8-hour scripted selling at 3 counters (T01, T02, T03)
//! - Offline sales periods, reconnection, and sync convergence
//! - Catalog and quick-sale lines, a same-session void
//! - Cash sessions closed on every counter at shift end (Z-report, zero variance)
//! - Post-soak convergence, `rebuild_derived` equality, integrity, audit chain and support bundle export

mod common;
#[path = "support/hub.rs"]
mod hub;
#[path = "support/term.rs"]
mod term;

use std::time::Instant;

use gayanshop_lib::audit;
use gayanshop_lib::clock::Clock;
use gayanshop_lib::db::integrity;
use gayanshop_lib::db::repo::cash::{self, CloseInput};
use gayanshop_lib::db::repo::reports;
use gayanshop_lib::support;
use term::{Term, derived, hub_for, joined, line, outbox, rebuilt, t01};

const QUICK_SALE: &str = "Gift Wrap";

/// Closes the counter's open session with the drawer counted at exactly the expected cash.
async fn close_shift(name: &str, t: &Term) -> cash::ZReport {
    let session = cash::current(&t.db.pool, &t.device_id).await.unwrap().expect("open session at shift end");
    let mut conn = t.db.pool.acquire().await.unwrap();
    let expected = cash::expected_cash(&mut conn, &session.id).await.unwrap();
    drop(conn);
    let input = CloseInput { counted_cash: expected, denominations: None, note: Some(format!("{name} end of shift")) };
    let z = cash::close(&t.db, &t.ctx(), &t.actor(), &input).await.unwrap();
    assert_eq!(z.session.status, "closed", "{name}: session closed");
    assert_eq!(z.expected_cash, expected, "{name}: Z-report expected cash");
    assert_eq!(z.variance, Some(0), "{name}: drawer balances");
    assert!(cash::current(&t.db.pool, &t.device_id).await.unwrap().is_none(), "{name}: no open session left");
    z
}

#[tokio::test]
async fn eight_hour_scripted_selling_at_three_counters() {
    let soak_start = Instant::now();

    // 1. Initial setup: Counter T01 with catalog (t01 already opens its initial cash session)
    let a = t01().await;
    let milk = a.product("Fresh Milk 1L", 42_000, Some("4790000000020"), Some((100, 32_000))).await;
    let sugar = a.product("White Sugar 1kg", 26_000, Some("4790000000037"), Some((200, 21_000))).await;
    let tea = a.product("Ceylon Tea 400g", 85_000, Some("4790000000044"), Some((80, 65_000))).await;
    let soap = a.product("Soap Bar 100g", 18_000, Some("4790000000051"), Some((150, 13_000))).await;

    // Connect to central sync Hub
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    assert_eq!(outbox(&a.db).await, 0, "T01 catalog synced to cloud");

    // 2. Counters T02 and T03 join the store (they automatically bootstrap and open initial cash sessions)
    let b = joined(&hub, &a.store_id, "T02").await;
    let c = joined(&hub, &a.store_id, "T03").await;
    b.round(&hub).await;
    c.round(&hub).await;

    // 3. Simulate 8 hours of trading across all 3 counters: 8 loops of one clock hour each
    // (8 × 3,600 s = 28,800 s), offline between the two-hourly sync rounds.
    let mut order_idx = 0;
    let mut void_target = None;
    let trading_start = a.clock.now();

    for hour in 1..=8 {
        // Advance every counter's clock by 1 hour (3,600 seconds)
        a.tick(3600);
        b.tick(3600);
        c.tick(3600);

        // Counter T01 sales
        let _o1 = a
            .sell(&format!("a-h{hour}-1"), vec![line(&milk, 42_000, 2), line(&sugar, 26_000, 1)], Some("0771112233"))
            .await;

        let _o2 = a.sell(&format!("a-h{hour}-2"), vec![line(&tea, 85_000, 1), line(&soap, 18_000, 3)], None).await;

        // Counter T02 sales (one catalog line plus a quick-sale line)
        let _o3 = b
            .sell(
                &format!("b-h{hour}-1"),
                vec![line(&sugar, 26_000, 3), common::quick(QUICK_SALE, 5_000, 1)],
                Some("0719998877"),
            )
            .await;

        let _o4 = b.sell(&format!("b-h{hour}-2"), vec![line(&milk, 42_000, 1), line(&soap, 18_000, 1)], None).await;

        // Counter T03 sales
        let o5 = c.sell(&format!("c-h{hour}-1"), vec![line(&tea, 85_000, 2)], Some("0754443322")).await;

        if hour == 2 {
            // Keep one order from T03 to void later during midday
            void_target = Some(o5);
        }

        order_idx += 5;

        // Midday void on T03 (Hour 3)
        if let Some(target) = void_target.as_ref().filter(|_| hour == 3) {
            c.void(target).await;
        }

        // Periodic sync rounds every 2 hours (simulating reconnection after offline periods)
        if hour % 2 == 0 {
            a.round(&hub).await;
            b.round(&hub).await;
            c.round(&hub).await;
        }
    }

    let traded = a.clock.now() - trading_start;
    assert!(traded >= chrono::Duration::hours(8), "the scripted day covers 8 full hours, got {traded}");

    // 4. Shift end: close the cash session on all 3 counters (Z-report with a balanced drawer)
    let za = close_shift("T01", &a).await;
    let zb = close_shift("T02", &b).await;
    let zc = close_shift("T03", &c).await;
    assert_eq!(za.sales_count, 16, "T01: 2 sales per hour");
    assert_eq!(zb.sales_count, 16, "T02: 2 sales per hour");
    assert_eq!(zb.quick_sale_lines, 8, "T02: one quick-sale line per hour");
    assert_eq!(zb.quick_sale_net, 8 * 5_000, "T02: quick-sale revenue");
    assert_eq!(zc.voids_count, 1, "T03: the midday void");

    // Final end-of-day sync: all counters flush to cloud, then pull everyone else's closing rows
    for _ in 0..2 {
        a.round(&hub).await;
        b.round(&hub).await;
        c.round(&hub).await;
    }

    // 5. Post-soak assertions: convergence & zero outbox backlog
    assert_eq!(outbox(&a.db).await, 0, "T01 outbox drained");
    assert_eq!(outbox(&b.db).await, 0, "T02 outbox drained");
    assert_eq!(outbox(&c.db).await, 0, "T03 outbox drained");

    // 6. Convergence: every counter reports the same store-wide numbers, and a full
    //    `rebuild_derived` recompute from the base rows reproduces them exactly.
    let expected = derived(&a.db).await;
    assert!(!expected.is_empty(), "derived read models populated");
    let quick_row = format!("\"{}\"", gayanshop_lib::domain::quick_sale::custom_key(QUICK_SALE));
    assert!(
        expected.iter().any(|r| r.starts_with("[\"qss\"") && r.contains(&quick_row)),
        "quick-sale stats present: {expected:?}"
    );
    for (name, term) in [("T01", &a), ("T02", &b), ("T03", &c)] {
        assert_eq!(derived(&term.db).await, expected, "{name}: store-wide numbers converge");
        assert_eq!(rebuilt(&term.db).await, expected, "{name}: rebuild_derived reproduces the ledger");
        let open: i64 = term.count("SELECT COUNT(*) FROM cash_sessions WHERE status = 'open'").await;
        assert_eq!(open, 0, "{name}: no open session anywhere in the store");
        let closed: i64 = term.count("SELECT COUNT(*) FROM cash_sessions WHERE status = 'closed'").await;
        assert_eq!(closed, 3, "{name}: all 3 counters' closed sessions synced");
    }

    // The audited `rebuild_derived` entry point the back office calls (Settings → Rebuild reports).
    let report = reports::rebuild_derived(&a.db, &a.ctx(), &a.actor()).await.unwrap();
    assert!(report.daily_sales_summary > 0, "rebuild recomputed daily sales rows");
    assert!(report.daily_payment_summary > 0, "rebuild recomputed payment rows");
    assert!(report.customer_stats > 0, "rebuild recomputed customer stats");
    assert!(report.quick_sale_stats > 0, "rebuild recomputed quick-sale stats");
    assert_eq!(derived(&a.db).await, expected, "T01: ledger unchanged by rebuild_derived");

    // 7. Post-soak assertions: database integrity on all 3 nodes
    for (name, term) in [("T01", &a), ("T02", &b), ("T03", &c)] {
        let rep = integrity::check(&term.db.pool).await.unwrap();
        assert_eq!(rep.quick_check, "ok", "{name}: SQLite quick_check must be ok");
        assert_eq!(rep.pending_orders, 0, "{name}: no pending orders");
        assert_eq!(rep.unreversed_voids, 0, "{name}: no unreversed voids");
        assert!(rep.healthy(), "{name}: database must be healthy");

        // Audit hash chain verification
        let chain_reports = audit::verify(&term.db.pool).await.unwrap();
        assert!(!chain_reports.is_empty(), "{name}: audit chain present");
        assert!(chain_reports.iter().all(|r| r.ok), "{name}: all audit chain entries must verify valid");
    }

    // 8. Post-soak support bundle generation and zip export drill
    let temp_log_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_log_dir.path().join("app.log"),
        format!("Soak test complete: {order_idx} orders processed across 3 counters\n"),
    )
    .unwrap();

    let export_dir = tempfile::tempdir().unwrap();
    let zip_dest = export_dir.path().join("soak-support-bundle.zip");

    let export_result = support::export_support_bundle_zip(
        &a.db.pool,
        temp_log_dir.path(),
        &zip_dest,
        "0.1.0",
        Some((&a.store_id, &a.device_id, "T01")),
    )
    .await
    .unwrap();

    assert!(export_result.size_bytes > 0);
    assert!(zip_dest.exists());
    assert!(export_result.files_included.contains(&"diagnostics.json".to_string()));

    println!("8-hour soak test passed: {order_idx} orders simulated in {:?}", soak_start.elapsed());
}
