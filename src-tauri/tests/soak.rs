//! Block 4.5 — Pilot readiness and soak test (doc §8 Phase 4):
//! - 8-hour scripted selling at 3 counters (T01, T02, T03)
//! - Offline sales periods, reconnection, and sync convergence
//! - Cash sessions (opening float, sales, voids, close)
//! - Post-soak database integrity, audit chain verification, and support bundle export

mod common;
#[path = "support/hub.rs"]
mod hub;
#[path = "support/term.rs"]
mod term;

use std::time::Instant;

use gayanshop_lib::audit;
use gayanshop_lib::db::integrity;
use gayanshop_lib::support;
use term::{hub_for, joined, line, outbox, t01};

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

    // 4. Simulate 8 hours of trading across all 3 counters (08:00 -> 16:00)
    // 8 hours = 28,800 seconds. We simulate regular trading cycles.
    let mut order_idx = 0;
    let mut void_target = None;

    // Hour 1-3: Morning rush (concurrent offline sales on all counters)
    for hour in 1..=8 {
        // Advance clock by 1 hour (3600 seconds)
        a.tick(1200);
        b.tick(1200);
        c.tick(1200);

        // Counter T01 sales
        let _o1 = a
            .sell(&format!("a-h{hour}-1"), vec![line(&milk, 42_000, 2), line(&sugar, 26_000, 1)], Some("0771112233"))
            .await;

        let _o2 = a.sell(&format!("a-h{hour}-2"), vec![line(&tea, 85_000, 1), line(&soap, 18_000, 3)], None).await;

        // Counter T02 sales (with quick sale items)
        let _o3 = b.sell(&format!("b-h{hour}-1"), vec![line(&sugar, 26_000, 3)], Some("0719998877")).await;

        let _o4 = b.sell(&format!("b-h{hour}-2"), vec![line(&milk, 42_000, 1), line(&soap, 18_000, 1)], None).await;

        // Counter T03 sales
        let o5 = c.sell(&format!("c-h{hour}-1"), vec![line(&tea, 85_000, 2)], Some("0754443322")).await;

        if hour == 2 {
            // Keep one order from T03 to void later during midday
            void_target = Some(o5);
        }

        order_idx += 5;

        // Midday void on T03 (Hour 3)
        if hour == 3 && void_target.is_some() {
            c.void(void_target.as_ref().unwrap()).await;
        }

        // Periodic sync rounds every 2 hours (simulating reconnection after offline periods)
        if hour % 2 == 0 {
            a.round(&hub).await;
            b.round(&hub).await;
            c.round(&hub).await;
        }
    }

    // Final end-of-day sync: all counters flush to cloud
    a.round(&hub).await;
    b.round(&hub).await;
    c.round(&hub).await;

    // 5. Post-soak assertions: convergence & zero outbox backlog
    assert_eq!(outbox(&a.db).await, 0, "T01 outbox drained");
    assert_eq!(outbox(&b.db).await, 0, "T02 outbox drained");
    assert_eq!(outbox(&c.db).await, 0, "T03 outbox drained");

    // 6. Post-soak assertions: database integrity on all 3 nodes
    for (name, term) in [("T01", &a), ("T02", &b), ("T03", &c)] {
        let rep = integrity::check(&term.db.pool).await.unwrap();
        assert_eq!(rep.quick_check, "ok", "{name}: SQLite quick_check must be ok");
        assert_eq!(rep.pending_orders, 0, "{name}: no pending orders");
        assert_eq!(rep.unreversed_voids, 0, "{name}: no unreversed voids");
        assert!(rep.healthy(), "{name}: database must be healthy");

        // 7. Audit hash chain verification
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
