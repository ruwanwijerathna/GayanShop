//! Who made the sale: every staff member has a number (1, 2, 3 …), the number typed at the till is stored on the
//! order as `sold_by`, and the Staff screen's "Sales performance" reads a person's daily, weekly, monthly and
//! yearly totals from it.

mod common;

use common::{CASHIER, NOW, SUP, Till, cart, cash, ctx_at, line, till};
use gayanshop_lib::db::repo::checkout::{self, CheckoutResult, NoFaults};
use gayanshop_lib::db::repo::staff::{self, NewStaff, StaffInput};
use gayanshop_lib::db::repo::staff_sales::{self, CHART_DAYS};
use gayanshop_lib::db::repo::void::{self as voids, VoidInput, VoidReason, Voider};
use gayanshop_lib::domain::business_date::StoreCalendar;
use gayanshop_lib::error::AppError;

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

/// A cash sale of `price` rung up by the Cashier on the given ISO time, credited to staff number `no`.
async fn sell_at(
    t: &Till,
    at: &str,
    token: &str,
    product: &str,
    price: i64,
    no: i64,
) -> Result<CheckoutResult, AppError> {
    let mut input = cart(token, vec![line(product, price, 1)], vec![cash(price, None)]);
    input.sold_by_no = no;
    checkout::checkout(&t.t.db, &ctx_at(&t.s, at), &t.seller(CASHIER).await, &input, None, &NoFaults).await
}

async fn sales_of(t: &Till, staff_id: &str, today: &str) -> staff_sales::StaffSales {
    let cal = StoreCalendar::colombo_default();
    let day = gayanshop_lib::domain::business_date::parse_date(today).unwrap();
    staff_sales::sales(t.pool(), &cal, day, staff_id).await.unwrap()
}

#[tokio::test]
async fn staff_get_the_next_free_number_and_the_seeded_ones_are_numbered() {
    let t = till().await;
    let numbers: Vec<(String, i64)> =
        sqlx::query_as("SELECT id, staff_no FROM staff ORDER BY staff_no").fetch_all(t.pool()).await.unwrap();
    assert_eq!(numbers[0], (t.s.owner_staff_id.clone(), 1), "the Owner is number 1");

    let role = staff::roles(t.pool()).await.unwrap().into_iter().find(|r| r.name == "Cashier").unwrap().id;
    let input = StaffInput {
        display_name: "Nimal".into(),
        role_id: role,
        ui_language: "en".into(),
        grants: vec![],
        denies: vec![],
        max_discount_bp: None,
        is_active: true,
    };
    let new = NewStaff { staff: input, pin: "4321".into() };
    let a = staff::create(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, &new).await.unwrap();
    let b = staff::create(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, &new).await.unwrap();
    assert_eq!((a.staff_no, b.staff_no), (Some(4), Some(5)), "after the seeded 1, 2 and 3");
    let listed = staff::list(t.pool()).await.unwrap();
    assert!(listed.iter().all(|m| m.staff_no.is_some()));

    // Disabling a person never frees their number for the next hire.
    common::exec(t.pool(), &format!("UPDATE staff SET is_active = 0 WHERE id = '{}'", b.id)).await;
    let c = staff::create(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, &new).await.unwrap();
    assert_eq!(c.staff_no, Some(6));
}

#[tokio::test]
async fn a_sale_is_credited_to_the_number_typed_not_the_logged_in_cashier() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let r = sell_at(&t, NOW, "s1", &mug, 25_000, 3).await.unwrap();
    let (staff_id, sold_by): (String, String) = sqlx::query_as("SELECT staff_id, sold_by FROM orders WHERE id = ?1")
        .bind(&r.order_id)
        .fetch_one(t.pool())
        .await
        .unwrap();
    assert_eq!(staff_id, CASHIER, "the logged-in cashier still owns the shift and drawer");
    assert_eq!(sold_by, SUP, "staff number 3 is the Supervisor");
    let queued: String =
        sqlx::query_scalar("SELECT payload FROM sync_outbox WHERE table_name = 'orders' AND row_id = ?1")
            .bind(&r.order_id)
            .fetch_one(t.pool())
            .await
            .unwrap();
    assert!(queued.contains(&format!("\"sold_by\":\"{SUP}\"")), "the salesperson syncs with the order");
}

#[tokio::test]
async fn an_unknown_or_disabled_number_refuses_the_sale_and_writes_nothing() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    for (no, token) in [(0, "u0"), (99, "u99"), (-1, "u-1")] {
        assert_eq!(code(sell_at(&t, NOW, token, &mug, 25_000, no).await.unwrap_err()), "SALESPERSON_UNKNOWN");
    }
    common::exec(t.pool(), &format!("UPDATE staff SET is_active = 0 WHERE id = '{SUP}'")).await;
    assert_eq!(code(sell_at(&t, NOW, "ud", &mug, 25_000, 3).await.unwrap_err()), "SALESPERSON_INACTIVE");
    assert_eq!(t.count("SELECT COUNT(*) FROM orders").await, 0);
    assert_eq!(t.count("SELECT COUNT(*) FROM receipt_counters").await, 0, "no receipt number was burned");
}

#[tokio::test]
async fn the_salesperson_of_a_sold_order_cannot_be_changed() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let r = sell_at(&t, NOW, "im", &mug, 25_000, 3).await.unwrap();
    common::rejects(
        t.pool(),
        &[&format!("UPDATE orders SET sold_by = '{CASHIER}' WHERE id = '{}'", r.order_id)],
        "illegal state transition",
    )
    .await;
}

#[tokio::test]
async fn daily_week_month_and_year_totals_follow_the_salesperson_and_skip_voids() {
    let t = till().await;
    let mug = t.product("Mug", 1_000, false, 0, None).await;
    // Today is Sun 2026-11-08 (store week Mon 11-02 … Sun 11-08). Staff number 3 sells:
    //   today 100, Thu 11-05 200, Sun 11-01 (last week, this month) 400, 10-20 (last month) 800,
    //   2025-12-30 (last year) 1600 — plus a 3200 sale today that is voided. Staff number 2 sells 6400 today.
    let day = |d: &str| format!("{d}T09:00:00.000Z");
    sell_at(&t, &day("2026-11-08"), "a", &mug, 10_000, 3).await.unwrap();
    sell_at(&t, &day("2026-11-05"), "b", &mug, 20_000, 3).await.unwrap();
    sell_at(&t, &day("2026-11-01"), "c", &mug, 40_000, 3).await.unwrap();
    sell_at(&t, &day("2026-10-20"), "d", &mug, 80_000, 3).await.unwrap();
    sell_at(&t, &day("2025-12-30"), "e", &mug, 160_000, 3).await.unwrap();
    let voided = sell_at(&t, &day("2026-11-08"), "f", &mug, 320_000, 3).await.unwrap();
    sell_at(&t, &day("2026-11-08"), "g", &mug, 640_000, 2).await.unwrap();
    let s = t.seller(SUP).await;
    voids::void_order(
        &t.t.db,
        &ctx_at(&t.s, &day("2026-11-08")),
        &Voider { staff_id: s.staff_id, perms: s.perms },
        &VoidInput { order_id: voided.order_id, reason: VoidReason::WrongQty, note: None, approval_id: None },
        None,
        &NoFaults,
    )
    .await
    .unwrap();

    let r = sales_of(&t, SUP, "2026-11-08").await;
    assert_eq!((r.week.total, r.week.orders), (30_000, 2), "Mon 11-02 … Sun 11-08: today and Thursday");
    assert_eq!((r.month.total, r.month.orders), (70_000, 3), "adds Sunday 11-01");
    assert_eq!((r.year.total, r.year.orders), (150_000, 4), "adds October; December 2025 is last year");
    assert_eq!(r.daily.len(), usize::try_from(CHART_DAYS).unwrap());
    assert_eq!(r.daily.last().map(|d| (d.date.as_str(), d.total)), Some(("2026-11-08", 10_000)));
    assert_eq!(r.daily.first().map(|d| d.date.as_str()), Some("2026-10-10"), "30 days ending today");
    let by_date = |d: &str| r.daily.iter().find(|x| x.date == d).map(|x| (x.total, x.orders));
    assert_eq!(by_date("2026-11-05"), Some((20_000, 1)));
    assert_eq!(by_date("2026-11-06"), Some((0, 0)), "quiet days are zero, not missing");
    assert_eq!(by_date("2026-10-20"), Some((80_000, 1)));

    let other = sales_of(&t, CASHIER, "2026-11-08").await;
    assert_eq!((other.week.total, other.year.total), (640_000, 640_000), "each person sees only their own");

    // Early in January the chart reaches back into last year while the year card restarts.
    let jan = sales_of(&t, SUP, "2026-01-05").await;
    assert_eq!(jan.year.total, 0);
    assert_eq!(jan.daily.first().map(|d| d.date.as_str()), Some("2025-12-07"));
    assert_eq!(jan.daily.iter().find(|d| d.date == "2025-12-30").map(|d| d.total), Some(160_000));
}

#[tokio::test]
async fn an_unknown_staff_member_has_no_sales_page() {
    let t = till().await;
    let cal = StoreCalendar::colombo_default();
    let day = gayanshop_lib::domain::business_date::parse_date("2026-11-08").unwrap();
    let e = staff_sales::sales(t.pool(), &cal, day, "nobody").await.unwrap_err();
    assert_eq!(code(e), "STAFF_NOT_FOUND");
}
