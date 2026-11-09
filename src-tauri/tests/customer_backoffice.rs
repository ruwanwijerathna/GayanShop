//! Block 2.5 — customer back office (doc §3.8): the list (search by phone or name, sort by spend, visits,
//! last visit or name), the profile (totals, average basket, top products and categories), keyset purchase
//! history with receipt drill-down, analytics for §5 periods (top spenders, new vs returning, attached
//! share), editing the name and note, PDPA erasure, and the `CUSTOMER_VIEW` guard.

mod common;

use common::{CASHIER, SUP, SUP_PIN, Till, cart, cash, ctx_at, line, quick, till};
use gayanshop_lib::auth::guard::require;
use gayanshop_lib::clock::ManualClock;
use gayanshop_lib::db::repo::checkout::{self, CheckoutLine, NoFaults};
use gayanshop_lib::db::repo::customers::{self, CustomerSort, CustomerUpdate, customer_id};
use gayanshop_lib::db::repo::void::{self, VoidInput, VoidReason, Voider};
use gayanshop_lib::domain::business_date::StoreCalendar;
use gayanshop_lib::domain::period::{Granularity, Period};
use gayanshop_lib::domain::permissions::Perm;
use gayanshop_lib::error::AppError;

const A: &str = "0771234567";
const B: &str = "0712345678";
const C: &str = "0112345678";

struct Shop {
    t: Till,
    a: String,
    b: String,
    c: String,
    a_void: String,
}

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

async fn sell(
    t: &Till,
    at: &str,
    token: &str,
    lines: Vec<CheckoutLine>,
    phone: Option<&str>,
    name: Option<&str>,
) -> String {
    let total: i64 = lines.iter().map(|l| l.unit_price * l.qty).sum();
    let mut input = cart(token, lines, vec![cash(total, None)]);
    input.customer_phone = phone.map(str::to_owned);
    input.customer_name = name.map(str::to_owned);
    checkout::checkout(&t.t.db, &ctx_at(&t.s, at), &t.seller(CASHIER).await, &input, None, &NoFaults)
        .await
        .unwrap()
        .order_id
}

/// A: 27 Oct (2 × p1), 3 Nov (p1 + quick mat), 8 Nov (p2) and a voided 8 Nov order → 3 visits, Rs 1,250.
/// B: 4 Nov (3 × p2) → Rs 1,200. C: 28 Oct (p1), no name → Rs 250. One anonymous sale on 8 Nov.
async fn shop() -> Shop {
    let t = till().await;
    let p1 = t.product("Plastic chair", 25_000, false, 10_000, None).await;
    let p2 = t.product("Rice cooker", 40_000, false, 30_000, None).await;
    sell(&t, "2026-10-27T09:00:00.000Z", "a1", vec![line(&p1, 25_000, 2)], Some(A), Some("Nimal")).await;
    sell(&t, "2026-10-28T09:00:00.000Z", "c1", vec![line(&p1, 25_000, 1)], Some(C), None).await;
    sell(&t, "2026-11-03T09:00:00.000Z", "a2", vec![line(&p1, 25_000, 1), quick("Door mat", 10_000, 1)], Some(A), None)
        .await;
    sell(&t, "2026-11-04T09:00:00.000Z", "b1", vec![line(&p2, 40_000, 3)], Some(B), Some("Kamala")).await;
    sell(&t, "2026-11-08T09:00:00.000Z", "a3", vec![line(&p2, 40_000, 1)], Some(A), None).await;
    sell(&t, "2026-11-08T09:10:00.000Z", "anon", vec![line(&p1, 25_000, 1)], None, None).await;
    let a_void = sell(&t, "2026-11-08T09:20:00.000Z", "a4", vec![line(&p2, 40_000, 2)], Some(A), None).await;
    let sup = t.seller(SUP).await;
    let input = VoidInput { order_id: a_void.clone(), reason: VoidReason::WrongQty, note: None, approval_id: None };
    void::void_order(&t.t.db, &t.ctx(), &Voider { staff_id: SUP.into(), perms: sup.perms }, &input, None, &NoFaults)
        .await
        .unwrap();
    let id = |p: &str| customer_id(&t.s.store_id, &gayanshop_lib::domain::phone::normalize_lk(p).unwrap());
    Shop { a: id(A), b: id(B), c: id(C), a_void, t }
}

#[tokio::test]
async fn the_list_sorts_by_spend_visits_last_visit_and_name() {
    let s = shop().await;
    let ids = |rows: Vec<customers::CustomerRow>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
    let list = |sort| customers::list(s.t.pool(), &s.t.s.store_id, None, sort, None);
    let spend = list(CustomerSort::Spend).await.unwrap();
    assert!(!spend.truncated);
    let a = &spend.rows[0];
    assert_eq!(
        (a.id.as_str(), a.visit_count, a.total_spend, a.avg_basket, a.display_name.as_deref()),
        (s.a.as_str(), 3, 125_000, 41_666, Some("Nimal")),
        "the void is excluded; the name typed on the first visit stays"
    );
    assert_eq!(a.phone_display.as_deref(), Some("077 123 4567"));
    assert_eq!((a.first_visit_date.as_deref(), a.last_visit_date.as_deref()), (Some("2026-10-27"), Some("2026-11-08")));
    assert_eq!(ids(spend.rows), vec![s.a.clone(), s.b.clone(), s.c.clone()]);
    assert_eq!(ids(list(CustomerSort::Visits).await.unwrap().rows), vec![s.a.clone(), s.b.clone(), s.c.clone()]);
    assert_eq!(ids(list(CustomerSort::LastVisit).await.unwrap().rows), vec![s.a.clone(), s.b.clone(), s.c.clone()]);
    assert_eq!(
        ids(list(CustomerSort::Name).await.unwrap().rows),
        vec![s.b.clone(), s.a.clone(), s.c.clone()],
        "unnamed customers last"
    );
    let capped = customers::list(s.t.pool(), &s.t.s.store_id, None, CustomerSort::Spend, Some(2)).await.unwrap();
    assert_eq!((capped.rows.len(), capped.truncated), (2, true));
}

#[tokio::test]
async fn the_list_searches_by_any_phone_format_or_name() {
    let s = shop().await;
    let find = |q: &'static str| {
        let pool = s.t.pool().clone();
        let store = s.t.s.store_id.clone();
        async move {
            customers::list(&pool, &store, Some(q), CustomerSort::Spend, None)
                .await
                .unwrap()
                .rows
                .into_iter()
                .map(|r| r.id)
                .collect::<Vec<_>>()
        }
    };
    for q in ["0771234567", "077 123", "771234567", "+94 77 123 4567", "94771234567", "77123"] {
        assert_eq!(find(q).await, vec![s.a.clone()], "{q}");
    }
    assert_eq!(find("+9471").await, vec![s.b.clone()]);
    assert_eq!(find("011").await, vec![s.c.clone()]);
    assert_eq!(find("1234567").await.len(), 3, "bare digits match anywhere in the number");
    assert_eq!(find("kam").await, vec![s.b.clone()], "case-insensitive name search");
    assert_eq!(find("NIMAL").await, vec![s.a.clone()]);
    assert!(find("100%").await.is_empty(), "LIKE wildcards are literal");
    assert_eq!(find("   ").await.len(), 3, "a blank search lists everyone");
}

#[tokio::test]
async fn the_profile_shows_totals_and_top_products_and_categories() {
    let s = shop().await;
    let p = customers::profile(s.t.pool(), &s.t.s.store_id, &s.a).await.unwrap();
    assert_eq!((p.customer.visit_count, p.customer.total_spend, p.customer.total_net), (3, 125_000, 125_000));
    assert_eq!(p.first_seen_at, "2026-10-27T09:00:00.000Z");
    assert_eq!(p.erased_at, None);
    let top: Vec<(Option<&str>, i64, i64)> = p.top_products.iter().map(|i| (i.name.as_deref(), i.qty, i.net)).collect();
    assert_eq!(
        top,
        vec![(Some("Plastic chair"), 3, 75_000), (Some("Rice cooker"), 1, 40_000), (None, 1, 10_000)],
        "voided lines excluded; quick sales grouped as one row"
    );
    assert_eq!(p.top_categories.len(), 1);
    assert_eq!((p.top_categories[0].id.as_deref(), p.top_categories[0].net), (None, 125_000));
    assert!(matches!(
        customers::profile(s.t.pool(), &s.t.s.store_id, "nope").await.unwrap_err(),
        AppError::NotFound { .. }
    ));
}

#[tokio::test]
async fn history_pages_newest_first_and_drills_down_to_the_masked_receipt() {
    let s = shop().await;
    let first = customers::history(s.t.pool(), &s.a, None, 3).await.unwrap();
    assert_eq!(first.rows.len(), 3);
    assert_eq!(first.rows[0].id, s.a_void, "newest first, voids included and flagged");
    assert_eq!((first.rows[0].status.as_str(), first.rows[1].status.as_str()), ("voided", "completed"));
    let cursor = first.next_cursor.clone().unwrap();
    let second = customers::history(s.t.pool(), &s.a, Some(&cursor), 3).await.unwrap();
    assert_eq!(second.rows.len(), 1);
    assert_eq!((second.rows[0].business_date.as_str(), second.rows[0].grand_total), ("2026-10-27", 50_000));
    assert_eq!(second.next_cursor, None);

    let receipt = customers::order_receipt(s.t.pool(), &s.a, &second.rows[0].id).await.unwrap();
    assert_eq!((receipt.grand_total, receipt.lines.len()), (50_000, 1));
    assert_eq!(receipt.customer_masked.as_deref(), Some("07X XXX X567"), "never the full phone");
    let b_order = customers::history(s.t.pool(), &s.b, None, 10).await.unwrap().rows[0].id.clone();
    assert!(matches!(
        customers::order_receipt(s.t.pool(), &s.a, &b_order).await.unwrap_err(),
        AppError::NotFound { .. }
    ));
}

#[tokio::test]
async fn analytics_cover_top_spenders_new_vs_returning_and_the_attached_share() {
    let s = shop().await;
    let cal = StoreCalendar::colombo_default();
    let run = |p: Period, g: Granularity| {
        let pool = s.t.pool().clone();
        let store = s.t.s.store_id.clone();
        let cal = cal.clone();
        async move { customers::analytics(&pool, &store, &cal, &p, g, 10).await.unwrap() }
    };
    let nov = run(Period::Month { year: 2026, month: 11 }, Granularity::Week).await;
    assert_eq!((nov.range.from.as_deref(), nov.range.to.as_deref()), (Some("2026-11-01"), Some("2026-11-30")));
    let top: Vec<(&str, i64, i64)> = nov.top.iter().map(|c| (c.customer_id.as_str(), c.visits, c.spend)).collect();
    assert_eq!(top, vec![(s.b.as_str(), 1, 120_000), (s.a.as_str(), 2, 75_000)], "October visits excluded");
    assert_eq!((nov.sales_count, nov.customer_orders, nov.attached_bp), (4, 3, 7_500));

    let all = run(Period::AllTime, Granularity::Week).await;
    let top: Vec<(&str, i64)> = all.top.iter().map(|c| (c.customer_id.as_str(), c.spend)).collect();
    assert_eq!(top, vec![(s.a.as_str(), 125_000), (s.b.as_str(), 120_000), (s.c.as_str(), 25_000)]);
    let weeks: Vec<(&str, i64, i64)> =
        all.trend.iter().map(|w| (w.bucket.as_str(), w.new_customers, w.returning_customers)).collect();
    assert_eq!(weeks, vec![("2026-10-26", 2, 0), ("2026-11-02", 1, 1)], "Monday weeks; A returns in week 2");
    assert_eq!((all.sales_count, all.customer_orders), (6, 5));

    let months = run(Period::AllTime, Granularity::Month).await;
    let months: Vec<(&str, i64, i64)> =
        months.trend.iter().map(|m| (m.bucket.as_str(), m.new_customers, m.returning_customers)).collect();
    assert_eq!(months, vec![("2026-10", 2, 0), ("2026-11", 1, 1)]);

    let day = run(Period::Day { date: "2026-11-04".into() }, Granularity::Day).await;
    assert_eq!((day.top.len(), day.attached_bp), (1, 10_000));
    let bad = customers::analytics(
        s.t.pool(),
        &s.t.s.store_id,
        &cal,
        &Period::Custom { from: "2026-11-05".into(), to: "2026-11-01".into() },
        Granularity::Week,
        10,
    )
    .await;
    assert_eq!(code(bad.unwrap_err()), "PERIOD_INVALID");
}

#[tokio::test]
async fn editing_the_name_and_note_is_audited_without_personal_data() {
    let s = shop().await;
    let t = &s.t;
    let outbox_before = t.count(&format!("SELECT COUNT(*) FROM sync_outbox WHERE row_id = '{}'", s.c)).await;
    let input =
        CustomerUpdate { display_name: Some("  Sunil ".into()), note: Some("Prefers delivery on Fridays".into()) };
    customers::update(&t.t.db, &t.ctx(), &t.owner(), &s.c, &input).await.unwrap();
    let p = customers::profile(t.pool(), &t.s.store_id, &s.c).await.unwrap();
    assert_eq!(
        (p.customer.display_name.as_deref(), p.note.as_deref()),
        (Some("Sunil"), Some("Prefers delivery on Fridays"))
    );
    let meta: String = sqlx::query_scalar("SELECT metadata FROM audit_logs WHERE action = 'CUSTOMER_UPDATE'")
        .fetch_one(t.pool())
        .await
        .unwrap();
    assert!(meta.contains("display_name") && meta.contains("note"), "{meta}");
    assert!(!meta.contains("Sunil") && !meta.contains("Fridays"), "no personal data in the audit log: {meta}");
    assert_eq!(t.count(&format!("SELECT COUNT(*) FROM sync_outbox WHERE row_id = '{}'", s.c)).await, outbox_before + 1);

    // Unchanged ⇒ nothing written; blank clears; limits enforced.
    customers::update(&t.t.db, &t.ctx(), &t.owner(), &s.c, &input).await.unwrap();
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'CUSTOMER_UPDATE'").await, 1);
    let clear = CustomerUpdate { display_name: Some("Sunil".into()), note: Some("  ".into()) };
    customers::update(&t.t.db, &t.ctx(), &t.owner(), &s.c, &clear).await.unwrap();
    assert_eq!(customers::profile(t.pool(), &t.s.store_id, &s.c).await.unwrap().note, None);
    let long = CustomerUpdate { display_name: None, note: Some("x".repeat(501)) };
    assert_eq!(
        code(customers::update(&t.t.db, &t.ctx(), &t.owner(), &s.c, &long).await.unwrap_err()),
        "CUSTOMER_NOTE_INVALID"
    );
    let long = CustomerUpdate { display_name: Some("x".repeat(61)), note: None };
    assert_eq!(
        code(customers::update(&t.t.db, &t.ctx(), &t.owner(), &s.c, &long).await.unwrap_err()),
        "CUSTOMER_NAME_INVALID"
    );
    assert!(matches!(
        customers::update(&t.t.db, &t.ctx(), &t.owner(), "nope", &clear).await.unwrap_err(),
        AppError::NotFound { .. }
    ));
}

#[tokio::test]
async fn erasure_hides_the_profile_but_keeps_every_total() {
    let s = shop().await;
    let t = &s.t;
    let stats_before = t.count("SELECT SUM(total_spend) FROM customer_stats").await;
    let orders_before = t.count("SELECT COUNT(*) FROM orders WHERE customer_id IS NOT NULL").await;
    customers::erase(&t.t.db, &t.ctx(), &t.owner(), &s.a).await.unwrap();

    let ids: Vec<String> = customers::list(t.pool(), &t.s.store_id, None, CustomerSort::Spend, None)
        .await
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, vec![s.b.clone(), s.c.clone()], "erased profiles leave the list");
    assert!(
        customers::list(t.pool(), &t.s.store_id, Some(A), CustomerSort::Spend, None).await.unwrap().rows.is_empty()
    );
    let p = customers::profile(t.pool(), &t.s.store_id, &s.a).await.unwrap();
    assert!(p.erased_at.is_some());
    assert_eq!((p.customer.phone_e164, p.customer.display_name, p.note), (None, None, None));
    assert_eq!(p.customer.total_spend, 125_000, "the anonymous visits still count");
    assert_eq!(t.count("SELECT SUM(total_spend) FROM customer_stats").await, stats_before);
    assert_eq!(t.count("SELECT COUNT(*) FROM orders WHERE customer_id IS NOT NULL").await, orders_before);

    let cal = StoreCalendar::colombo_default();
    let all =
        customers::analytics(t.pool(), &t.s.store_id, &cal, &Period::AllTime, Granularity::Month, 10).await.unwrap();
    assert_eq!(all.top[0].customer_id, s.a);
    assert_eq!((all.top[0].display_name.as_deref(), all.top[0].phone_display.as_deref()), (None, None));
    let receipt = customers::order_receipt(t.pool(), &s.a, &s.a_void).await.unwrap();
    assert_eq!(receipt.customer_masked, None, "no phone left to mask");

    let edit = CustomerUpdate { display_name: Some("Nimal".into()), note: None };
    assert_eq!(
        code(customers::update(&t.t.db, &t.ctx(), &t.owner(), &s.a, &edit).await.unwrap_err()),
        "CUSTOMER_ALREADY_ERASED"
    );
    assert_eq!(
        code(customers::erase(&t.t.db, &t.ctx(), &t.owner(), &s.a).await.unwrap_err()),
        "CUSTOMER_ALREADY_ERASED"
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'CUSTOMER_ERASE'").await, 1);
}

#[tokio::test]
async fn the_back_office_needs_customer_view_which_no_pin_can_lend() {
    let s = shop().await;
    let t = &s.t;
    let clock: &ManualClock = &t.clock;
    for (staff, pin) in [(CASHIER, common::CASHIER_PIN), (SUP, SUP_PIN)] {
        t.login(staff, pin).await;
        assert!(
            matches!(
                require(&t.auth, clock, Perm::CUSTOMER_VIEW, "customers", None).unwrap_err(),
                AppError::Forbidden { perm: Perm::CUSTOMER_VIEW }
            ),
            "{staff}: Forbidden, not ApprovalRequired"
        );
    }
    t.login(&t.s.owner_staff_id, "246810").await;
    assert_eq!(require(&t.auth, clock, Perm::CUSTOMER_VIEW, "customers", None).unwrap().staff_id, t.s.owner_staff_id);
    let manager_has_it: i64 = t.count("SELECT (permissions >> 29) & 1 FROM roles WHERE name = 'Manager'").await;
    assert_eq!(manager_has_it, 1);
}
