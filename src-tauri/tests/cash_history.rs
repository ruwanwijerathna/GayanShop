//! Cash drawer history: the shift list with filters and role-scoped visibility, plus the Z-report reprint
//! (a closed shift only, marked REPRINT, audited).

mod common;

use common::{Till, cart, cash, ctx_at, exec, line, till};
use gayanshop_lib::db::repo::cash::{self as cash_repo, CloseInput, HistoryFilter};
use gayanshop_lib::db::repo::checkout::{self, NoFaults};
use gayanshop_lib::error::AppError;

const DAY1_OPEN: &str = "2026-11-08T09:00:00.000Z";
const DAY1_CLOSE: &str = "2026-11-08T20:00:00.000Z";
const DAY2_OPEN: &str = "2026-11-09T09:00:00.000Z";
const DAY2_CLOSE: &str = "2026-11-09T20:00:00.000Z";

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

fn filter() -> HistoryFilter {
    HistoryFilter::default()
}

/// Day 1: sells one item for Rs 500 and closes Rs 20 short. Day 2: an open shift with no sales.
/// Another counter (`dev2`) has a closed shift on day 1 as well.
async fn two_days() -> Till {
    let t = till().await;
    let product = t.stocked("Soap", 50_000, &[(2, 20_000)]).await;
    let sold = checkout::checkout(
        &t.t.db,
        &ctx_at(&t.s, DAY1_OPEN),
        &t.seller(common::CASHIER).await,
        &cart("tok-1", vec![line(&product, 50_000, 1)], vec![cash(50_000, Some(50_000))]),
        None,
        &NoFaults,
    )
    .await
    .unwrap();
    assert!(!sold.receipt_no.is_empty());
    cash_repo::close(
        &t.t.db,
        &ctx_at(&t.s, DAY1_CLOSE),
        &t.owner(),
        &CloseInput { counted_cash: 48_000, denominations: None, note: Some("Rs 20 short".into()) },
    )
    .await
    .unwrap();
    cash_repo::open(&t.t.db, &ctx_at(&t.s, DAY2_OPEN), &t.owner(), 100_000).await.unwrap();
    exec(
        t.pool(),
        &format!(
            "INSERT INTO cash_sessions (id, store_id, device_id, business_date, status, opened_by, opened_at,
                opening_float, closed_by, closed_at, expected_cash, counted_cash, variance,
                created_at, updated_at, updated_by_device)
             VALUES ('other', '{store}', 'dev2', '2026-11-08', 'closed', '{owner}', '{DAY1_OPEN}', 0,
                '{owner}', '{DAY1_CLOSE}', 10000, 10000, 0, '{DAY1_OPEN}', '{DAY1_CLOSE}', 'dev2')",
            store = t.s.store_id,
            owner = t.s.owner_staff_id
        ),
    )
    .await;
    t
}

#[tokio::test]
async fn history_lists_shifts_newest_first_with_sales_and_variance() {
    let t = two_days().await;
    let h = cash_repo::history(t.pool(), &filter(), (&t.s.device_id, "T01"), true, 50).await.unwrap();
    assert_eq!(h.rows.len(), 3);
    assert!(h.all_counters && !h.truncated);
    // Newest first: day 2 (open), then day 1 of both counters.
    assert_eq!(h.rows[0].session.business_date, "2026-11-09");
    assert_eq!(h.rows[0].session.status, "open");
    let mine = h.rows.iter().find(|r| r.session.business_date == "2026-11-08" && r.session.device_id == t.s.device_id);
    let mine = mine.unwrap();
    assert_eq!((mine.sales_count, mine.sales_total, mine.session.variance), (1, 50_000, Some(-2_000)));
    assert_eq!(mine.device_code, "T01");
    assert_eq!(h.counters.len(), 2, "this counter and dev2");
}

#[tokio::test]
async fn history_filters_by_date_counter_and_variance() {
    let t = two_days().await;
    let own = (t.s.device_id.as_str(), "T01");
    let day2 = HistoryFilter { from: Some("2026-11-09".into()), ..filter() };
    assert_eq!(cash_repo::history(t.pool(), &day2, own, true, 50).await.unwrap().rows.len(), 1);
    let day1 = HistoryFilter { from: Some("2026-11-08".into()), to: Some("2026-11-08".into()), ..filter() };
    assert_eq!(cash_repo::history(t.pool(), &day1, own, true, 50).await.unwrap().rows.len(), 2);
    let dev2 = HistoryFilter { device_id: Some("dev2".into()), ..filter() };
    let only = cash_repo::history(t.pool(), &dev2, own, true, 50).await.unwrap();
    assert_eq!((only.rows.len(), only.rows[0].session.device_id.as_str()), (1, "dev2"));
    let off = HistoryFilter { variance_only: true, ..filter() };
    let rows = cash_repo::history(t.pool(), &off, own, true, 50).await.unwrap().rows;
    assert_eq!(rows.len(), 1, "only the shift that was Rs 20 short");
    assert_eq!(rows[0].session.variance, Some(-2_000));
}

#[tokio::test]
async fn history_rejects_bad_dates_and_reports_truncation() {
    let t = two_days().await;
    let own = (t.s.device_id.as_str(), "T01");
    let bad = HistoryFilter { from: Some("11/08/2026".into()), ..filter() };
    assert_eq!(code(cash_repo::history(t.pool(), &bad, own, true, 50).await.unwrap_err()), "DATE_INVALID");
    let backwards = HistoryFilter { from: Some("2026-11-09".into()), to: Some("2026-11-08".into()), ..filter() };
    assert_eq!(code(cash_repo::history(t.pool(), &backwards, own, true, 50).await.unwrap_err()), "DATE_INVALID");
    let h = cash_repo::history(t.pool(), &filter(), own, true, 2).await.unwrap();
    assert_eq!((h.rows.len(), h.truncated), (2, true));
}

#[tokio::test]
async fn without_report_permission_only_this_counter_is_visible() {
    let t = two_days().await;
    let own = (t.s.device_id.as_str(), "T01");
    // Even asking for the other counter explicitly: forced back to this one.
    let ask = HistoryFilter { device_id: Some("dev2".into()), ..filter() };
    let h = cash_repo::history(t.pool(), &ask, own, false, 50).await.unwrap();
    assert!(!h.all_counters);
    assert_eq!(h.rows.len(), 2);
    assert!(h.rows.iter().all(|r| r.session.device_id == t.s.device_id));
    assert_eq!(h.counters.len(), 1);
}

#[tokio::test]
async fn reprint_queues_a_marked_z_report_for_a_closed_shift_only() {
    let t = two_days().await;
    let ids: Vec<(String, String)> =
        sqlx::query_as("SELECT id, status FROM cash_sessions WHERE device_id = ?1 ORDER BY opened_at")
            .bind(&t.s.device_id)
            .fetch_all(t.pool())
            .await
            .unwrap();
    let (closed, open) = (&ids[0].0, &ids[1].0);
    assert_eq!(ids[1].1, "open");

    let ctx = ctx_at(&t.s, DAY2_CLOSE);
    assert_eq!(code(cash_repo::reprint_z(&t.t.db, &ctx, &t.owner(), open).await.unwrap_err()), "SESSION_NOT_CLOSED");
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE kind = 'z_report'").await, 1, "only the close job");

    cash_repo::reprint_z(&t.t.db, &ctx, &t.owner(), closed).await.unwrap();
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE kind = 'z_report'").await, 2);
    let payload: String = sqlx::query_scalar(
        "SELECT payload FROM print_jobs WHERE kind = 'z_report' ORDER BY created_at DESC, rowid DESC LIMIT 1",
    )
    .fetch_one(t.pool())
    .await
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(json["reprint"], true);
    assert_eq!(json["session"]["id"], closed.as_str());
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'Z_REPORT_REPRINT'").await, 1);
}

#[test]
fn a_reprinted_z_report_says_reprint_and_the_original_does_not() {
    use gayanshop_lib::hardware::escpos::{Layout, render};
    let z = serde_json::json!({ "session": { "business_date": "2026-11-08" } });
    let mut again = z.clone();
    again["reprint"] = serde_json::json!(true);
    let has = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).contains("*** REPRINT ***");
    assert!(!has(render::z_report("GayanShop", &z, Layout::default(), chrono_tz::Asia::Colombo)));
    assert!(has(render::z_report("GayanShop", &again, Layout::default(), chrono_tz::Asia::Colombo)));
}
