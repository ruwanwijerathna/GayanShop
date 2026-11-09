//! Block 2.6 — expenses (doc §5.1): categories (create, rename, retire, unique names), entry with
//! validation, the drawer pay-out integration (a `pay_out` in the open session that the Z-report counts, and
//! its reversal on delete), editing rules for drawer-paid expenses, the period list with totals, and the
//! `EXPENSE_MANAGE` / `CASH_PAY_IN_OUT` guard.

mod common;

use common::{CASHIER, CASHIER_PIN, SUP, SUP_PIN, Till, ctx_at, till};
use gayanshop_lib::auth::guard::require;
use gayanshop_lib::db::repo::cash;
use gayanshop_lib::db::repo::expenses::{self, ExpenseCategoryInput, ExpenseInput};
use gayanshop_lib::domain::business_date::StoreCalendar;
use gayanshop_lib::domain::payments::PaymentMethod;
use gayanshop_lib::domain::period::Period;
use gayanshop_lib::domain::permissions::Perm;
use gayanshop_lib::error::AppError;

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

async fn category(t: &Till, name: &str) -> String {
    sqlx::query_scalar("SELECT id FROM expense_categories WHERE name = ?1")
        .bind(name)
        .fetch_one(t.pool())
        .await
        .unwrap()
}

fn input(category_id: &str, amount: i64, date: Option<&str>, drawer: bool) -> ExpenseInput {
    ExpenseInput {
        category_id: category_id.into(),
        amount,
        business_date: date.map(str::to_owned),
        payment_method: PaymentMethod::Cash,
        paid_from_drawer: drawer,
        payee: Some("CEB".into()),
        note: None,
    }
}

async fn expected_cash(t: &Till) -> i64 {
    let mut conn = t.pool().acquire().await.unwrap();
    cash::expected_cash(&mut conn, &t.session_id).await.unwrap()
}

#[tokio::test]
async fn categories_are_seeded_unique_and_retired_not_removed() {
    let t = till().await;
    let cats = expenses::categories(t.pool(), &t.s.store_id).await.unwrap();
    assert_eq!(cats.len(), 8, "the §11 seed list");
    assert!(cats.iter().all(|c| c.is_active && c.used == 0));

    let id = expenses::save_category(
        &t.t.db,
        &t.ctx(),
        &t.owner(),
        None,
        &ExpenseCategoryInput { name: " Security ".into(), is_active: true },
    )
    .await
    .unwrap();
    let dup = ExpenseCategoryInput { name: "security".into(), is_active: true };
    assert_eq!(
        code(expenses::save_category(&t.t.db, &t.ctx(), &t.owner(), None, &dup).await.unwrap_err()),
        "EXPENSE_CATEGORY_EXISTS"
    );
    let blank = ExpenseCategoryInput { name: "  ".into(), is_active: true };
    assert_eq!(
        code(expenses::save_category(&t.t.db, &t.ctx(), &t.owner(), None, &blank).await.unwrap_err()),
        "EXPENSE_CATEGORY_NAME_INVALID"
    );
    // Rename and retire; a retired category refuses new expenses but keeps its old ones.
    expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&id, 50_000, None, false)).await.unwrap();
    let retire = ExpenseCategoryInput { name: "Night security".into(), is_active: false };
    expenses::save_category(&t.t.db, &t.ctx(), &t.owner(), Some(&id), &retire).await.unwrap();
    let c = expenses::categories(t.pool(), &t.s.store_id).await.unwrap().into_iter().find(|c| c.id == id).unwrap();
    assert_eq!((c.name.as_str(), c.is_active, c.used), ("Night security", false, 1));
    assert_eq!(
        code(expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&id, 1_000, None, false)).await.unwrap_err()),
        "EXPENSE_CATEGORY_INACTIVE"
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'SETTINGS_CHANGE'").await, 2);
    assert!(matches!(
        expenses::save_category(&t.t.db, &t.ctx(), &t.owner(), Some("nope"), &retire).await.unwrap_err(),
        AppError::NotFound { .. }
    ));
}

#[tokio::test]
async fn an_expense_is_validated_audited_and_queued_for_sync() {
    let t = till().await;
    let rent = category(&t, "Rent").await;
    let e = expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&rent, 4_500_000, Some("2026-11-01"), false))
        .await
        .unwrap();
    assert_eq!(
        (e.category_name.as_str(), e.amount, e.business_date.as_str(), e.paid_from_drawer, e.cash_session_id),
        ("Rent", 4_500_000, "2026-11-01", false, None)
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'EXPENSE_CREATE'").await, 1);
    assert_eq!(t.count(&format!("SELECT COUNT(*) FROM sync_outbox WHERE row_id = '{}'", e.id)).await, 1);
    assert_eq!(t.count("SELECT COUNT(*) FROM cash_movements").await, 0, "not from the drawer: no cash moved");

    let bad = |amount, date: Option<&str>| input(&rent, amount, date, false);
    for (i, want) in [
        (bad(0, None), "AMOUNT_INVALID"),
        (bad(-5, None), "AMOUNT_INVALID"),
        (bad(100, Some("2026-11-09")), "EXPENSE_DATE_INVALID"),
        (bad(100, Some("09/11/2026")), "EXPENSE_DATE_INVALID"),
    ] {
        assert_eq!(code(expenses::create(&t.t.db, &t.ctx(), &t.owner(), &i).await.unwrap_err()), want);
    }
    let mut long = bad(100, None);
    long.note = Some("x".repeat(201));
    assert_eq!(
        code(expenses::create(&t.t.db, &t.ctx(), &t.owner(), &long).await.unwrap_err()),
        "EXPENSE_TEXT_TOO_LONG"
    );
    assert!(matches!(
        expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input("nope", 100, None, false)).await.unwrap_err(),
        AppError::NotFound { .. }
    ));
}

#[tokio::test]
async fn paid_from_the_drawer_is_a_pay_out_the_z_report_reconciles() {
    let t = till().await;
    let tea = category(&t, "Tea & sundries").await;
    let before = expected_cash(&t).await;
    let e = expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&tea, 75_000, None, true)).await.unwrap();
    assert_eq!((e.paid_from_drawer, e.cash_session_id.as_deref()), (true, Some(t.session_id.as_str())));
    assert_eq!(expected_cash(&t).await, before - 75_000, "the drawer expects Rs 750 less");
    let (kind, amount, reason): (String, i64, String) =
        sqlx::query_as("SELECT type, amount, reason FROM cash_movements").fetch_one(t.pool()).await.unwrap();
    assert_eq!((kind.as_str(), amount, reason.as_str()), ("pay_out", 75_000, "Expense: Tea & sundries – CEB"));
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE kind = 'drawer_kick'").await, 1, "the drawer opens");
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action IN ('CASH_PAY_OUT', 'EXPENSE_CREATE')").await, 2);
    let mut conn = t.pool().acquire().await.unwrap();
    let z = cash::z_report(&mut conn, &t.session_id, common::NOW).await.unwrap();
    assert_eq!(z.pay_outs, 75_000);

    // Drawer pay-outs are cash, today, in an open session on this counter.
    let mut card = input(&tea, 1_000, None, true);
    card.payment_method = PaymentMethod::Card;
    assert_eq!(
        code(expenses::create(&t.t.db, &t.ctx(), &t.owner(), &card).await.unwrap_err()),
        "EXPENSE_DRAWER_CASH_ONLY"
    );
    assert_eq!(
        code(
            expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&tea, 1_000, Some("2026-11-07"), true))
                .await
                .unwrap_err()
        ),
        "EXPENSE_DRAWER_TODAY_ONLY"
    );
    let close = cash::CloseInput { counted_cash: expected_cash(&t).await.max(0), denominations: None, note: None };
    cash::close(&t.t.db, &t.ctx(), &t.owner(), &close).await.unwrap();
    let no_session = expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&tea, 1_000, None, true)).await;
    assert!(no_session.is_err(), "no open session: no drawer pay-out");
    assert_eq!(t.count("SELECT COUNT(*) FROM expenses").await, 1, "nothing half-written");
}

#[tokio::test]
async fn a_drawer_paid_expense_keeps_its_cash_facts() {
    let t = till().await;
    let tea = category(&t, "Tea & sundries").await;
    let repairs = category(&t, "Repairs").await;
    let e = expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&tea, 75_000, None, true)).await.unwrap();
    // Category, payee and note can change …
    let mut edit = input(&repairs, 75_000, None, true);
    edit.payee = Some("Sunil".into());
    let after = expenses::update(&t.t.db, &t.ctx(), &t.owner(), &e.id, &edit).await.unwrap();
    assert_eq!((after.category_name.as_str(), after.payee.as_deref()), ("Repairs", Some("Sunil")));
    // … the amount, method and drawer link can't (the pay-out is append-only).
    for broken in [
        input(&repairs, 80_000, None, true),
        input(&repairs, 75_000, None, false),
        ExpenseInput { payment_method: PaymentMethod::Card, ..input(&repairs, 75_000, None, true) },
        input(&repairs, 75_000, Some("2026-11-01"), true),
    ] {
        assert_eq!(
            code(expenses::update(&t.t.db, &t.ctx(), &t.owner(), &e.id, &broken).await.unwrap_err()),
            "EXPENSE_DRAWER_LOCKED"
        );
    }
    // A plain expense is fully editable but can't move into the drawer afterwards.
    let plain = expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&repairs, 10_000, Some("2026-11-02"), false))
        .await
        .unwrap();
    let moved =
        expenses::update(&t.t.db, &t.ctx(), &t.owner(), &plain.id, &input(&tea, 12_000, Some("2026-11-03"), false))
            .await
            .unwrap();
    assert_eq!(
        (moved.amount, moved.business_date.as_str(), moved.category_name.as_str()),
        (12_000, "2026-11-03", "Tea & sundries")
    );
    assert_eq!(
        code(
            expenses::update(&t.t.db, &t.ctx(), &t.owner(), &plain.id, &input(&tea, 12_000, None, true))
                .await
                .unwrap_err()
        ),
        "EXPENSE_DRAWER_LOCKED"
    );
    let meta: String =
        sqlx::query_scalar("SELECT metadata FROM audit_logs WHERE action = 'EXPENSE_UPDATE' AND entity_id = ?1")
            .bind(&plain.id)
            .fetch_one(t.pool())
            .await
            .unwrap();
    assert!(meta.contains("10000") && meta.contains("12000"), "before and after: {meta}");
}

#[tokio::test]
async fn deleting_a_drawer_expense_puts_the_cash_back_while_the_session_is_open() {
    let t = till().await;
    let tea = category(&t, "Tea & sundries").await;
    let before = expected_cash(&t).await;
    let e = expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&tea, 75_000, None, true)).await.unwrap();
    expenses::delete(&t.t.db, &t.ctx(), &t.owner(), &e.id).await.unwrap();
    assert_eq!(expected_cash(&t).await, before, "the pay-in returns the cash");
    let kinds: Vec<String> = sqlx::query_scalar("SELECT type FROM cash_movements ORDER BY created_at, rowid")
        .fetch_all(t.pool())
        .await
        .unwrap();
    assert_eq!(kinds, vec!["pay_out", "pay_in"], "append-only: the pay-out stays");
    assert_eq!(t.count("SELECT COUNT(*) FROM expenses WHERE deleted_at IS NOT NULL").await, 1);
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'EXPENSE_DELETE'").await, 1);
    assert!(matches!(
        expenses::delete(&t.t.db, &t.ctx(), &t.owner(), &e.id).await.unwrap_err(),
        AppError::NotFound { .. }
    ));

    // After the session closes the drawer facts are final.
    let kept = expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&tea, 20_000, None, true)).await.unwrap();
    let close = cash::CloseInput { counted_cash: expected_cash(&t).await.max(0), denominations: None, note: None };
    cash::close(&t.t.db, &t.ctx(), &t.owner(), &close).await.unwrap();
    assert_eq!(
        code(expenses::delete(&t.t.db, &t.ctx(), &t.owner(), &kept.id).await.unwrap_err()),
        "EXPENSE_DRAWER_LOCKED"
    );
    // A plain expense deletes any time, with no cash movement.
    let rent = category(&t, "Rent").await;
    let plain = expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&rent, 9_000, None, false)).await.unwrap();
    let moves = t.count("SELECT COUNT(*) FROM cash_movements").await;
    expenses::delete(&t.t.db, &t.ctx(), &t.owner(), &plain.id).await.unwrap();
    assert_eq!(t.count("SELECT COUNT(*) FROM cash_movements").await, moves);
}

#[tokio::test]
async fn the_list_covers_a_period_with_totals_by_category() {
    let t = till().await;
    let (rent, tea, wages) =
        (category(&t, "Rent").await, category(&t, "Tea & sundries").await, category(&t, "Wages").await);
    let at = |d: &str| ctx_at(&t.s, &format!("{d}T09:00:00.000Z"));
    for (d, cat, amount) in [
        ("2026-10-31", &rent, 4_000_000),
        ("2026-11-01", &rent, 4_500_000),
        ("2026-11-03", &tea, 25_000),
        ("2026-11-05", &wages, 1_500_000),
        ("2026-11-06", &tea, 30_000),
    ] {
        expenses::create(&t.t.db, &at(d), &t.owner(), &input(cat, amount, None, false)).await.unwrap();
    }
    expenses::create(&t.t.db, &t.ctx(), &t.owner(), &input(&tea, 10_000, None, true)).await.unwrap();
    let cal = StoreCalendar::colombo_default();
    let nov =
        expenses::list(t.pool(), &t.s.store_id, &cal, &Period::Month { year: 2026, month: 11 }, None).await.unwrap();
    assert_eq!(nov.rows.len(), 5, "31 October is outside November");
    assert_eq!(nov.rows[0].business_date, "2026-11-08", "newest first");
    assert_eq!((nov.total, nov.from_drawer), (4_500_000 + 25_000 + 1_500_000 + 30_000 + 10_000, 10_000));
    let totals: Vec<(&str, i64, i64)> = nov.by_category.iter().map(|c| (c.name.as_str(), c.amount, c.count)).collect();
    assert_eq!(totals, vec![("Rent", 4_500_000, 1), ("Wages", 1_500_000, 1), ("Tea & sundries", 65_000, 3)]);

    let week =
        expenses::list(t.pool(), &t.s.store_id, &cal, &Period::Week { any_date: "2026-11-04".into() }, Some(&tea))
            .await
            .unwrap();
    assert_eq!((week.rows.len(), week.total), (3, 65_000), "Monday 2 – Sunday 8 November, tea only");
    let all = expenses::list(t.pool(), &t.s.store_id, &cal, &Period::AllTime, None).await.unwrap();
    assert_eq!(all.rows.len(), 6);
    let day =
        expenses::list(t.pool(), &t.s.store_id, &cal, &Period::Day { date: "2026-11-02".into() }, None).await.unwrap();
    assert!(day.rows.is_empty() && day.total == 0);
}

#[tokio::test]
async fn expenses_need_expense_manage_and_drawer_cash_needs_pay_in_out() {
    let t = till().await;
    for (staff, pin) in [(CASHIER, CASHIER_PIN), (SUP, SUP_PIN)] {
        t.login(staff, pin).await;
        assert!(matches!(
            require(&t.auth, &t.clock, Perm::EXPENSE_MANAGE, "expenses", None).unwrap_err(),
            AppError::Forbidden { perm: Perm::EXPENSE_MANAGE }
        ));
    }
    assert!(Perm::CASH_PAY_IN_OUT.approvable(), "a supervisor PIN can lend the drawer");
    t.login(&t.s.owner_staff_id, "246810").await;
    assert!(require(&t.auth, &t.clock, Perm::EXPENSE_MANAGE, "expenses", None).is_ok());
    assert!(require(&t.auth, &t.clock, Perm::CASH_PAY_IN_OUT, "cash_drawer", None).is_ok());
}
