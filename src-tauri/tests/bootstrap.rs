//! Block 3.5: full-history bootstrap of a new counter (doc §6.2, decision #18). T01's history is built so
//! that parents were edited *after* the rows that reference them: a batch received, reopened, re-costed and
//! received again (its layers and the batch itself end up newer than the first intake movements), a product
//! re-priced after its sales, orders voided after their payments reached the cloud. A joining counter then
//! pulls everything in pages of 7 rows. No page may trip a foreign key, the derived tables must equal a
//! rebuild and T01's, and an interrupted bootstrap resumes from its cursor. Without the parent closure the
//! same bootstrap fails, which shows the closure is what makes it work.

mod common;
#[path = "support/hub.rs"]
mod hub;
#[path = "support/term.rs"]
mod term;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use gayanshop_lib::sync::apply;
use gayanshop_lib::sync::bootstrap::{self, BootstrapProgress};
use gayanshop_lib::sync::config::ServerConfig;
use gayanshop_lib::sync::engine::RoundError;
use gayanshop_lib::sync::registration::record_joined_identity;
use hub::Hub;
use term::{Term, derived, hub_for, line, rebuilt, t01};

async fn history() -> (Term, Arc<Hub>) {
    let a = t01().await;
    let bucket = a.product("Bucket", 45_000, None, None).await;
    let mat = a.product("Door mat", 15_000, None, Some((30, 5_000))).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let batch = a.reworked_batch(&bucket, 20, 20_000).await;
    let mut voids = Vec::new();
    for i in 0..12 {
        let phone = (i % 3 == 0).then_some("0771234567");
        let o = a.sell(&format!("h{i}"), vec![line(&bucket, 45_000, 1), line(&mat, 15_000, 2)], phone).await;
        if i % 5 == 0 {
            voids.push(o);
        }
    }
    a.round(&hub).await;
    // After the cloud has the sales and their payments: void some, re-price the product.
    for o in &voids {
        a.void(o).await;
    }
    a.edit_price(&bucket, 47_000).await;
    a.round(&hub).await;
    assert_eq!(a.count(&format!("SELECT COUNT(*) FROM batch_items WHERE batch_id = '{batch}'")).await, 1);
    (a, hub)
}

/// A counter that has registered into the store but not pulled anything yet.
async fn joining(store_id: &str) -> Term {
    let t = Term::empty().await;
    let cfg = ServerConfig::parse("http://127.0.0.1:1", "anon").unwrap();
    record_joined_identity(&t.db, &t.clock, store_id, &t.device_id, "T02", &cfg, "si").await.unwrap();
    t
}

#[tokio::test]
async fn a_new_counter_bootstraps_the_whole_history_in_pages_of_seven() {
    let (a, hub) = history().await;
    let b = joining(&a.store_id).await;
    assert!(bootstrap::is_pending(&b.db).await.unwrap(), "the till stays closed until the history is in");
    let seen: Arc<Mutex<Vec<BootstrapProgress>>> = Arc::new(Mutex::new(vec![]));
    let log = seen.clone();
    let done = bootstrap::run(&b.db, &hub.client(&b.device_id), &b.clock, &b.device_id, 7, &move |p| {
        log.lock().unwrap().push(p.clone());
    })
    .await
    .unwrap();

    assert!(done.done && done.pages > 10, "{} pages of 7", done.pages);
    let progress = seen.lock().unwrap().clone();
    assert!(progress.windows(2).all(|w| w[0].permille <= w[1].permille), "progress only moves forward");
    assert_eq!(progress.last().map(|p| (p.permille, p.done)), Some((1000, true)));
    assert!(!bootstrap::is_pending(&b.db).await.unwrap(), "the till may open");

    let expected = derived(&a.db).await;
    assert_eq!(derived(&b.db).await, expected, "the new counter's reports equal T01's");
    assert_eq!(rebuilt(&b.db).await, expected, "and a rebuild from base rows");
    let dangling: Vec<(String,)> =
        sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check").fetch_all(&b.db.pool).await.unwrap();
    assert!(dangling.is_empty(), "no dangling reference: {dangling:?}");
    assert_eq!(b.count("SELECT COUNT(*) FROM orders WHERE status = 'voided'").await, 3);
    // The identity is complete: name and calendar from the pulled settings, the staff to log in with.
    let identity = gayanshop_lib::setup::load_identity(&b.db.pool).await.unwrap().unwrap();
    assert_eq!((identity.store_name.as_str(), identity.device_code.as_str()), ("GayanShop", "T02"));
    assert_eq!(b.count("SELECT COUNT(*) FROM staff").await, a.count("SELECT COUNT(*) FROM staff").await);
    assert!(gayanshop_lib::audit::verify(&b.db.pool).await.unwrap().iter().all(|r| r.ok), "every chain verifies");
}

#[tokio::test]
async fn without_the_parent_closure_the_same_bootstrap_breaks() {
    let (a, hub) = history().await;
    hub.no_closure.store(true, Ordering::SeqCst);
    let b = joining(&a.store_id).await;
    let err = bootstrap::run(&b.db, &hub.client(&b.device_id), &b.clock, &b.device_id, 7, &|_| {}).await.unwrap_err();
    assert!(matches!(err, RoundError::Local(_)), "a page whose parents are newer than its rows can't commit: {err:?}");
    assert!(bootstrap::is_pending(&b.db).await.unwrap());
}

#[tokio::test]
async fn an_interrupted_bootstrap_resumes_where_it_stopped() {
    let (a, hub) = history().await;
    let b = joining(&a.store_id).await;
    // The network drops after five pages.
    let cut = hub.clone();
    let err = bootstrap::run(&b.db, &hub.client(&b.device_id), &b.clock, &b.device_id, 7, &move |p| {
        if p.pages == 5 {
            cut.fail_pulls.store(1, Ordering::SeqCst);
        }
    })
    .await
    .unwrap_err();
    assert!(matches!(err, RoundError::Remote(_)));
    assert!(bootstrap::is_pending(&b.db).await.unwrap(), "still pending: the till stays closed");
    let cursor = apply::cursor(&b.db).await.unwrap();
    assert!(cursor > 0, "the five applied pages are kept");
    let pulls = hub.pulls();
    let done = bootstrap::run(&b.db, &hub.client(&b.device_id), &b.clock, &b.device_id, 7, &|_| {}).await.unwrap();
    assert!(done.done);
    let first: Vec<i64> =
        sqlx::query_scalar("SELECT cursor_before FROM sync_logs WHERE direction = 'pull' AND result <> 'error'")
            .fetch_all(&b.db.pool)
            .await
            .unwrap();
    assert!(first.contains(&cursor), "the second run started from the saved cursor: {first:?}");
    assert!(hub.pulls() > pulls);
    assert_eq!(derived(&b.db).await, derived(&a.db).await);
}
