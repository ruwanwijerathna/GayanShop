//! Block 1.4 — crash mid-checkout ⇒ nothing or everything (doc §13.2).
//!
//! Two ways of killing a checkout half-way:
//! - `every_named_step`: the `Checkpoint` seam fails right after each step of the transaction.
//! - `every_await_point`: the checkout future is dropped after N polls, for every N until it completes —
//!   i.e. at every `.await` inside it (sqlx calls included), which is what a crash or power cut looks like
//!   to the transaction.
//!
//! After every interruption the database must be exactly as before (no order, line, payment, slice,
//! customer, auto-created layer, receipt-counter bump, audit row, outbox row or print job; the recovery
//! cart still there). The uninterrupted run must then write everything, once.

mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use common::{CASHIER, SUP, Till, cart, cash, line, quick, till};
use gayanshop_lib::db::repo::checkout::{self, CheckoutInput, Checkpoint, NoFaults};
use gayanshop_lib::error::{AppError, AppResult};

const TABLES: [&str; 13] = [
    "orders",
    "order_items",
    "order_payments",
    "stock_movements",
    "customers",
    "batch_items",
    "receipt_counters",
    "audit_logs",
    "sync_outbox",
    "print_jobs",
    "daily_sales_summary",
    "customer_stats",
    "held_carts",
];

async fn snapshot(t: &Till) -> Vec<i64> {
    let mut out = Vec::new();
    for table in TABLES {
        out.push(t.count(&format!("SELECT COUNT(*) FROM {table}")).await);
    }
    out.push(t.count("SELECT COALESCE(SUM(on_hand), 0) FROM stock_levels").await);
    out.push(t.count("SELECT COALESCE(SUM(last_seq), 0) FROM receipt_counters").await);
    out
}

/// Cart with everything: a FIFO line across two layers, a product sold before it has a layer (auto
/// ADJUSTMENTS layer), a quick sale, a new customer, a supervisor-approved override, cash with change.
async fn rich_cart(t: &Till) -> CheckoutInput {
    let shoe = t.stocked("Shoe", 150_000, &[(1, 80_000), (3, 90_000)]).await;
    let comb = t.product("Comb", 5_000, true, 1_500, None).await;
    common::exec(
        t.pool(),
        "INSERT INTO held_carts (id, payload, created_at, updated_at) VALUES ('__current__', '{}', 'x', 'x')",
    )
    .await;
    let mut input = cart(
        "crash-1",
        vec![line(&shoe, 140_000, 2), line(&comb, 5_000, 1), quick("Gift Wrap", 2_000, 1)],
        vec![cash(287_000, Some(300_000))],
    );
    input.customer_phone = Some("0771234567".into());
    input
}

struct FailAt {
    at: usize,
    seen: AtomicUsize,
}

impl Checkpoint for FailAt {
    fn reached(&self, step: &'static str) -> AppResult<()> {
        if self.seen.fetch_add(1, Ordering::SeqCst) == self.at {
            Err(AppError::internal(format!("injected crash after {step}")))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn every_named_step() {
    let t = till().await;
    let input = rich_cart(&t).await;
    let seller = t.seller(CASHIER).await;
    let before = snapshot(&t).await;
    let mut at = 0;
    loop {
        let cp = FailAt { at, seen: AtomicUsize::new(0) };
        match checkout::checkout(&t.t.db, &t.ctx(), &seller, &input, Some(SUP), &cp).await {
            Err(AppError::Internal { detail }) => {
                assert!(detail.starts_with("injected crash"), "{detail}");
                assert_eq!(snapshot(&t).await, before, "partial writes after: {detail}");
                at += 1;
            }
            Ok(r) => {
                assert!(!r.replayed);
                break;
            }
            Err(other) => panic!("unexpected error at step {at}: {other:?}"),
        }
        assert!(at < 100, "checkout never completed");
    }
    assert!(at >= 12, "only {at} checkpoints were exercised");
    assert_everything_written(&t, &before).await;
}

/// Polls the inner future at most `budget` times, then gives up and drops it (a crash at that await).
struct Interrupt<F> {
    inner: Pin<Box<F>>,
    budget: usize,
}

impl<F: Future> Future for Interrupt<F> {
    type Output = Option<F::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.budget == 0 {
            return Poll::Ready(None);
        }
        self.budget -= 1;
        self.inner.as_mut().poll(cx).map(Some)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_await_point() {
    let t = till().await;
    let input = rich_cart(&t).await;
    let seller = t.seller(CASHIER).await;
    let ctx = t.ctx();
    let before = snapshot(&t).await;
    let mut interrupted = 0;
    for budget in 1..10_000 {
        let run = Interrupt {
            inner: Box::pin(checkout::checkout(&t.t.db, &ctx, &seller, &input, Some(SUP), &NoFaults)),
            budget,
        };
        match run.await {
            None => {
                interrupted += 1;
                // Taking the writer lock proves the aborted transaction has been rolled back and released.
                drop(t.t.db.write().await.unwrap());
                if snapshot(&t).await != before {
                    // Cut at the very poll where COMMIT had already reached SQLite: the checkout is durable.
                    // That is "everything", never a part of it, which assert_everything_written checks exactly.
                    assert!(interrupted > 20, "only {interrupted} await points were cut");
                    assert_everything_written(&t, &before).await;
                    return;
                }
            }
            Some(result) => {
                assert!(!result.unwrap().replayed);
                break;
            }
        }
    }
    assert!(interrupted > 20, "only {interrupted} await points were cut");
    assert_everything_written(&t, &before).await;
}

async fn assert_everything_written(t: &Till, before: &[i64]) {
    let after = snapshot(t).await;
    let delta: Vec<(&str, i64)> = TABLES.iter().zip(after.iter().zip(before)).map(|(n, (a, b))| (*n, a - b)).collect();
    let expect = |table: &str| delta.iter().find(|(n, _)| *n == table).map(|(_, d)| *d).unwrap();
    assert_eq!(expect("orders"), 1);
    assert_eq!(expect("order_items"), 3);
    assert_eq!(expect("order_payments"), 1);
    assert_eq!(expect("stock_movements"), 3, "shoe across two layers + comb from its new ADJUSTMENTS layer");
    assert_eq!(expect("customers"), 1);
    assert_eq!(expect("batch_items"), 1, "the comb's ADJUSTMENTS layer");
    assert_eq!(expect("receipt_counters"), 1);
    assert_eq!(expect("print_jobs"), 2);
    assert_eq!(expect("held_carts"), -1);
    assert_eq!(expect("daily_sales_summary"), 1);
    assert!(expect("audit_logs") >= 2, "customer create + price override");
    assert!(expect("sync_outbox") >= 8);
    // Retrying the same cart after the crash-free run changes nothing.
    let seller = t.seller(CASHIER).await;
    // Only the token matters for a replay: it returns before the cart is even read.
    let replay = cart("crash-1", vec![], vec![]);
    let again = checkout::checkout(&t.t.db, &t.ctx(), &seller, &replay, Some(SUP), &NoFaults).await.unwrap();
    assert!(again.replayed);
    assert_eq!(snapshot(t).await, after);
}
