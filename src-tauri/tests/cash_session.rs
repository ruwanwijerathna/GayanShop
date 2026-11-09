//! Block 1.3 — cash sessions and the Z-report: open with a float, pay-in / pay-out / no-sale, close with a
//! count and variance, the Z-report numbers, and "a closed session can't reopen" (nor be voided into).

mod common;

use common::{BD, NOW, Sale, checkout, ctx_at, exec, open, scalar_i64, seed, store};
use gayanshop_lib::auth::guard::Actor;
use gayanshop_lib::db::repo::WriteCtx;
use gayanshop_lib::db::repo::cash::{self, CloseInput, DenominationCount, DrawerMovementKind};
use gayanshop_lib::error::AppError;

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

fn close_input(counted: i64) -> CloseInput {
    CloseInput { counted_cash: counted, denominations: None, note: None }
}

#[tokio::test]
async fn session_lifecycle_open_movements_close() {
    let t = open().await;
    let s = store(&t).await;
    let ctx = ctx_at(&s, NOW);
    let owner = Actor { staff_id: s.owner_staff_id.clone(), approved_by: None };
    let p = t.pool();

    assert!(cash::current(p, &s.device_id).await.unwrap().is_none());
    let mut conn = p.acquire().await.unwrap();
    assert_eq!(code(cash::require_open(&mut conn, &s.device_id).await.unwrap_err()), "NO_OPEN_SESSION");
    drop(conn);
    assert_eq!(
        code(cash::drawer_movement(&t.db, &ctx, &owner, DrawerMovementKind::PayIn, 100, "x").await.unwrap_err()),
        "NO_OPEN_SESSION",
        "drawer movements need an open session"
    );

    let session = cash::open(&t.db, &ctx, &owner, 500_000).await.unwrap();
    assert_eq!((session.status.as_str(), session.opening_float, session.business_date.as_str()), ("open", 500_000, BD));
    assert_eq!(code(cash::open(&t.db, &ctx, &owner, 0).await.unwrap_err()), "SESSION_ALREADY_OPEN");

    let x =
        cash::drawer_movement(&t.db, &ctx, &owner, DrawerMovementKind::PayIn, 100_000, "Float top-up").await.unwrap();
    assert_eq!(x.expected_cash, 600_000);
    let x = cash::drawer_movement(&t.db, &ctx, &owner, DrawerMovementKind::PayOut, 25_000, " petty cash – tea ")
        .await
        .unwrap();
    assert_eq!((x.expected_cash, x.pay_ins, x.pay_outs), (575_000, 100_000, 25_000));
    assert_eq!(x.drawer[1].reason, "petty cash – tea");
    cash::no_sale(&t.db, &ctx, &owner, "Change for a customer").await.unwrap();

    // Count by denomination: 5 × Rs 1,000 + 1 × Rs 500 + 2 × Rs 100 = Rs 5,700 against Rs 5,750 expected.
    let input = CloseInput {
        counted_cash: 570_000,
        denominations: Some(vec![
            DenominationCount { value: 100_000, count: 5 },
            DenominationCount { value: 50_000, count: 1 },
            DenominationCount { value: 10_000, count: 2 },
            DenominationCount { value: 5_000, count: 0 },
        ]),
        note: Some("Rs 50 short".into()),
    };
    let z = cash::close(&t.db, &ctx_at(&s, "2026-11-08T20:00:00.000Z"), &owner, &input).await.unwrap();
    assert_eq!((z.expected_cash, z.counted_cash, z.variance), (575_000, Some(570_000), Some(-5_000)));
    assert_eq!((z.session.status.as_str(), z.no_sales, z.sales_count), ("closed", 1, 0));
    let stored: String = sqlx::query_scalar("SELECT denominations FROM cash_sessions").fetch_one(p).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
        serde_json::json!({"100000": 5, "50000": 1, "10000": 2})
    );

    // Everything audited, synced and queued for the printer in the same transactions.
    for (action, n) in [
        ("CASH_SESSION_OPEN", 1),
        ("CASH_PAY_IN", 1),
        ("CASH_PAY_OUT", 1),
        ("DRAWER_KICK", 1),
        ("CASH_SESSION_CLOSE", 1),
    ] {
        assert_eq!(
            scalar_i64(p, &format!("SELECT COUNT(*) FROM audit_logs WHERE action = '{action}'")).await,
            n,
            "{action}"
        );
    }
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM print_jobs WHERE kind = 'drawer_kick'").await, 3);
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM print_jobs WHERE kind = 'z_report'").await, 1);
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'cash_movements'").await, 3);
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'cash_sessions'").await, 2);

    // Closed means closed: no more movements, and a new session can start.
    assert_eq!(code(cash::no_sale(&t.db, &ctx, &owner, "again").await.unwrap_err()), "NO_OPEN_SESSION");
    assert_eq!(code(cash::close(&t.db, &ctx, &owner, &close_input(0)).await.unwrap_err()), "NO_OPEN_SESSION");
    let next = cash::open(&t.db, &ctx_at(&s, "2026-11-09T08:00:00.000Z"), &owner, 0).await.unwrap();
    assert_ne!(next.id, session.id);
}

#[tokio::test]
async fn a_closed_session_cannot_reopen() {
    let t = open().await;
    let s = store(&t).await;
    let ctx = ctx_at(&s, NOW);
    let owner = Actor { staff_id: s.owner_staff_id.clone(), approved_by: None };
    let session = cash::open(&t.db, &ctx, &owner, 0).await.unwrap();
    cash::drawer_movement(&t.db, &ctx, &owner, DrawerMovementKind::PayIn, 1_000, "Coins").await.unwrap();
    cash::close(&t.db, &ctx, &owner, &close_input(1_000)).await.unwrap();
    let reopen = sqlx::query(&format!("UPDATE cash_sessions SET status = 'open' WHERE id = '{}'", session.id))
        .execute(t.pool())
        .await;
    assert!(reopen.unwrap_err().to_string().contains("a closed cash session cannot be reopened"));
    // The drawer history behind the printed Z-report can't be edited or removed either.
    let edit = sqlx::query("UPDATE cash_movements SET amount = 1").execute(t.pool()).await;
    assert!(edit.unwrap_err().to_string().contains("append-only"));
    let delete = sqlx::query("DELETE FROM cash_movements").execute(t.pool()).await;
    assert!(delete.unwrap_err().to_string().contains("append-only"));
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM cash_sessions").fetch_one(t.pool()).await.unwrap(),
        "closed"
    );
}

#[tokio::test]
async fn invalid_input_is_rejected() {
    let t = open().await;
    let s = store(&t).await;
    let ctx = ctx_at(&s, NOW);
    let owner = Actor { staff_id: s.owner_staff_id.clone(), approved_by: None };
    assert_eq!(code(cash::open(&t.db, &ctx, &owner, -1).await.unwrap_err()), "FLOAT_INVALID");
    cash::open(&t.db, &ctx, &owner, 0).await.unwrap();
    let pay = |amount, reason: &'static str| {
        cash::drawer_movement(&t.db, &ctx, &owner, DrawerMovementKind::PayOut, amount, reason)
    };
    assert_eq!(code(pay(0, "tea").await.unwrap_err()), "AMOUNT_INVALID");
    assert_eq!(code(pay(-5, "tea").await.unwrap_err()), "AMOUNT_INVALID");
    assert_eq!(code(pay(100, "   ").await.unwrap_err()), "REASON_REQUIRED");
    assert_eq!(code(cash::no_sale(&t.db, &ctx, &owner, "").await.unwrap_err()), "REASON_REQUIRED");
    assert_eq!(code(cash::close(&t.db, &ctx, &owner, &close_input(-1)).await.unwrap_err()), "COUNT_INVALID");
    let bad_note = CloseInput {
        counted_cash: 1_000,
        denominations: Some(vec![DenominationCount { value: 1_000, count: 2 }]),
        note: None,
    };
    assert_eq!(code(cash::close(&t.db, &ctx, &owner, &bad_note).await.unwrap_err()), "DENOMINATIONS_MISMATCH");
    let odd = CloseInput {
        counted_cash: 300,
        denominations: Some(vec![DenominationCount { value: 300, count: 1 }]),
        note: None,
    };
    assert_eq!(code(cash::close(&t.db, &ctx, &owner, &odd).await.unwrap_err()), "DENOMINATION_INVALID");
    // Nothing was written by the refused calls.
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM cash_movements").await, 0);
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM cash_sessions WHERE status = 'open'").await, 1);
}

#[tokio::test]
async fn z_report_totals_sales_payments_voids_and_reconciles_cash() {
    let t = open().await;
    let p = t.pool();
    seed(p).await; // session cs1 open on dev1 (float 0); cashier + supervisor; p1 with a 2-unit layer
    let ctx = WriteCtx {
        store_id: "s".into(),
        device_id: "dev1".into(),
        device_code: "T01".into(),
        now: NOW.into(),
        business_date: BD.into(),
    };
    let cashier = Actor { staff_id: "cashier".into(), approved_by: None };
    let sup = Actor { staff_id: "sup".into(), approved_by: None };

    // o1: Rs 450 cash (tendered Rs 455). o2: overridden to Rs 400 with the supervisor's approval.
    // o3: Rs 450 with a Rs 5 discount, then voided by the cashier with the supervisor's approval.
    checkout(p, &Sale::new("o1")).await;
    checkout(p, &Sale { unit: 40_000, approved: Some("sup"), ..Sale::new("o2") }).await;
    checkout(p, &Sale { disc: 500, ..Sale::new("o3") }).await;
    exec(p, &format!("UPDATE orders SET status = 'voided', voided_at = '{NOW}', voided_by = 'cashier', void_approved_by = 'sup', void_reason = 'WRONG_ITEM' WHERE id = 'o3'")).await;

    cash::drawer_movement(&t.db, &ctx, &sup, DrawerMovementKind::PayIn, 10_000, "Coins").await.unwrap();
    cash::drawer_movement(
        &t.db,
        &ctx,
        &Actor { approved_by: Some("sup".into()), ..cashier.clone() },
        DrawerMovementKind::PayOut,
        3_000,
        "Tea",
    )
    .await
    .unwrap();

    let mut conn = p.acquire().await.unwrap();
    let x = cash::z_report(&mut conn, "cs1", NOW).await.unwrap();
    drop(conn);
    assert_eq!((x.sales_count, x.items_sold, x.sales_total), (2, 2, 85_000), "voided o3 is excluded");
    assert_eq!((x.gross_sales, x.discount_total, x.net_sales), (85_000, 0, 85_000));
    assert_eq!(x.payments.len(), 1);
    assert_eq!((x.payments[0].method.as_str(), x.payments[0].count, x.payments[0].amount), ("cash", 2, 85_000));
    assert_eq!(x.change_given, 1_000, "Rs 5 change on each completed sale");
    assert_eq!((x.overrides_count, x.overrides_value), (1, 5_000));
    assert_eq!((x.voids_count, x.voids_value), (1, 44_500));
    assert_eq!(x.void_reasons[0].reason, "WRONG_ITEM");
    assert_eq!((x.voids[0].voided_by.as_str(), x.voids[0].approved_by.as_deref()), ("cashier", Some("sup")));
    assert_eq!((x.pay_ins, x.pay_outs), (10_000, 3_000));
    assert_eq!(x.drawer[1].approved_by.as_deref(), Some("sup"), "approver of a cashier's pay-out is reported");
    // expected = float 0 + cash sales 85,000 + pay-in 10,000 − pay-out 3,000.
    assert_eq!((x.cash_sales, x.expected_cash, x.counted_cash), (85_000, 92_000, None));

    let z = cash::close(&t.db, &ctx, &sup, &close_input(92_000)).await.unwrap();
    assert_eq!((z.variance, z.session.closed_by_name.as_deref()), (Some(0), Some("sup")), "expected equals counted");
    assert_eq!(scalar_i64(p, "SELECT expected_cash FROM cash_sessions WHERE id = 'cs1'").await, 92_000);

    // Closing ended the void window: o1 can no longer be voided by anyone.
    let late = sqlx::query(&format!(
        "UPDATE orders SET status = 'voided', voided_at = '{NOW}', voided_by = 'sup', void_reason = 'WRONG_ITEM' WHERE id = 'o1'"
    ))
    .execute(p)
    .await;
    assert!(late.unwrap_err().to_string().contains("sales are final"));
}
