//! The Bills page query: search by receipt or a customer's phone, payment method, exchange / returns kinds,
//! the flags and payment words on every row, and the sale-then-exchange history it has to tell apart.

mod common;

use common::{CASHIER, Till, cart, cash, line, till};
use gayanshop_lib::db::repo::checkout::{self, CheckoutInput, NoFaults, PaymentLine};
use gayanshop_lib::db::repo::exchange::{ExchangeInput, ExchangeReturn};
use gayanshop_lib::db::repo::orders::{self, BillKind, OrderFilter, OrderRow};
use gayanshop_lib::domain::business_date::StoreCalendar;
use gayanshop_lib::domain::payments::PaymentMethod;
use gayanshop_lib::domain::period::Period;
use gayanshop_lib::error::AppError;

const T1: &str = "2026-11-08T09:00:00.000Z";
const T2: &str = "2026-11-08T09:05:00.000Z";
const T3: &str = "2026-11-08T09:10:00.000Z";
const T4: &str = "2026-11-08T09:15:00.000Z";

fn card(amount: i64) -> PaymentLine {
    PaymentLine { method: PaymentMethod::Card, amount, tendered: None, reference: Some("slip 1".into()) }
}

async fn sell_at(t: &Till, input: &CheckoutInput, now: &str) -> String {
    let ctx = common::ctx_at(&t.s, now);
    checkout::checkout(&t.t.db, &ctx, &t.seller(CASHIER).await, input, None, &NoFaults).await.unwrap().order_id
}

fn all() -> OrderFilter {
    OrderFilter {
        period: Period::AllTime,
        device_id: None,
        status: None,
        receipt: None,
        phone: None,
        payment: None,
        kind: None,
    }
}

async fn find(t: &Till, filter: OrderFilter) -> Vec<OrderRow> {
    orders::history(t.pool(), &t.s.store_id, &StoreCalendar::colombo_default(), &filter, None, 50).await.unwrap().rows
}

struct Shop {
    t: Till,
    /// cash sale of 3 buckets, customer 077 123 4567
    cash_sale: String,
    /// card sale
    card_sale: String,
    /// cash + card
    split_sale: String,
    /// exchange that took 1 bucket back
    exchange: String,
}

async fn shop() -> Shop {
    let t = till().await;
    let bucket = t.stocked("Bucket", 45_000, &[(10, 20_000)]).await;
    let mug = t.stocked("Mug", 25_000, &[(30, 9_000)]).await;

    let mut a = cart("bill-a", vec![line(&bucket, 45_000, 3)], vec![cash(135_000, None)]);
    a.customer_phone = Some("0771234567".into());
    let cash_sale = sell_at(&t, &a, T1).await;

    let card_sale = sell_at(&t, &cart("bill-b", vec![line(&mug, 25_000, 2)], vec![card(50_000)]), T2).await;
    let split_sale =
        sell_at(&t, &cart("bill-c", vec![line(&mug, 25_000, 4)], vec![cash(40_000, None), card(60_000)]), T3).await;

    // One of the three buckets comes back for 2 mugs (500.00 ≥ the 450.00 credit): pay the 50.00 difference.
    let item: String = sqlx::query_scalar("SELECT id FROM order_items WHERE order_id = ?1 LIMIT 1")
        .bind(&cash_sale)
        .fetch_one(t.pool())
        .await
        .unwrap();
    let mut ex = cart("bill-d", vec![line(&mug, 25_000, 2)], vec![cash(5_000, None)]);
    ex.exchange = Some(ExchangeInput { returns: vec![ExchangeReturn { item_id: item, qty: 1 }] });
    let exchange = sell_at(&t, &ex, T4).await;
    Shop { t, cash_sale, card_sale, split_sale, exchange }
}

fn ids(rows: &[OrderRow]) -> Vec<&str> {
    rows.iter().map(|r| r.id.as_str()).collect()
}

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn every_row_says_how_it_was_paid_and_what_kind_of_bill_it_is() {
    let s = shop().await;
    let rows = find(&s.t, all()).await;
    assert_eq!(ids(&rows), [&s.exchange, &s.split_sale, &s.card_sale, &s.cash_sale], "newest first");
    let by = |id: &str| rows.iter().find(|r| r.id == id).unwrap();

    assert_eq!(by(&s.cash_sale).methods, "cash");
    assert_eq!(by(&s.card_sale).methods, "card");
    assert_eq!(by(&s.split_sale).methods, "card,cash");

    assert_eq!(by(&s.exchange).exchange_credit, 45_000);
    assert!(!by(&s.exchange).has_returns);
    assert!(by(&s.cash_sale).has_returns, "its bucket came back in the exchange");
    assert_eq!(by(&s.cash_sale).exchange_credit, 0);
    assert!(!by(&s.card_sale).has_returns && !by(&s.split_sale).has_returns);
}

#[tokio::test]
async fn a_customers_phone_is_found_by_any_way_of_typing_it_or_its_last_digits() {
    let s = shop().await;
    for typed in ["0771234567", "077 123 4567", "+94771234567", "94771234567", "771234567", "1234567", "4567"] {
        let rows = find(&s.t, OrderFilter { phone: Some(typed.into()), ..all() }).await;
        assert_eq!(ids(&rows), [s.cash_sale.as_str()], "typed {typed}");
    }
    let none = find(&s.t, OrderFilter { phone: Some("0779999999".into()), ..all() }).await;
    assert!(none.is_empty());

    // Fewer than four digits would match half the shop.
    for short in ["077", "+9", "12-3"] {
        let e = orders::history(
            s.t.pool(),
            &s.t.s.store_id,
            &StoreCalendar::colombo_default(),
            &OrderFilter { phone: Some(short.into()), ..all() },
            None,
            50,
        )
        .await
        .unwrap_err();
        assert_eq!(code(e), "PHONE_SEARCH_TOO_SHORT", "typed {short}");
    }
    let blank = find(&s.t, OrderFilter { phone: Some("   ".into()), ..all() }).await;
    assert_eq!(blank.len(), 4, "blank means no phone filter");
}

#[tokio::test]
async fn payment_method_and_kind_filters_narrow_the_list_and_combine() {
    let s = shop().await;
    let card_rows = find(&s.t, OrderFilter { payment: Some(PaymentMethod::Card), ..all() }).await;
    assert_eq!(ids(&card_rows), [s.split_sale.as_str(), s.card_sale.as_str()]);
    let cash_rows = find(&s.t, OrderFilter { payment: Some(PaymentMethod::Cash), ..all() }).await;
    assert_eq!(ids(&cash_rows), [s.exchange.as_str(), s.split_sale.as_str(), s.cash_sale.as_str()]);
    assert!(find(&s.t, OrderFilter { payment: Some(PaymentMethod::Qr), ..all() }).await.is_empty());

    let exchanges = find(&s.t, OrderFilter { kind: Some(BillKind::Exchange), ..all() }).await;
    assert_eq!(ids(&exchanges), [s.exchange.as_str()]);
    let with_returns = find(&s.t, OrderFilter { kind: Some(BillKind::WithReturns), ..all() }).await;
    assert_eq!(ids(&with_returns), [s.cash_sale.as_str()]);

    let both =
        find(&s.t, OrderFilter { kind: Some(BillKind::Exchange), payment: Some(PaymentMethod::Card), ..all() }).await;
    assert!(both.is_empty(), "the exchange was paid in cash only");

    let receipt: String = sqlx::query_scalar("SELECT receipt_no FROM orders WHERE id = ?1")
        .bind(&s.card_sale)
        .fetch_one(s.t.pool())
        .await
        .unwrap();
    let by_receipt =
        find(&s.t, OrderFilter { receipt: Some(receipt.to_lowercase()), payment: Some(PaymentMethod::Card), ..all() })
            .await;
    assert_eq!(ids(&by_receipt), [s.card_sale.as_str()], "receipt search ignores case and combines with the rest");
}

#[tokio::test]
async fn staff_without_report_sales_are_held_to_their_own_counter() {
    let s = shop().await;
    let own = s.t.s.device_id.clone();

    let seen = |f: OrderFilter| find(&s.t, f);
    let asked_for_elsewhere = OrderFilter { device_id: Some("another-counter".into()), ..all() };
    let held = orders::visible_to(asked_for_elsewhere.clone(), &own, false);
    assert_eq!(held.device_id.as_deref(), Some(own.as_str()), "the request's counter is overridden");
    assert_eq!(seen(held).await.len(), 4, "their own counter's bills are all there");

    let everything = orders::visible_to(asked_for_elsewhere, &own, true);
    assert_eq!(everything.device_id.as_deref(), Some("another-counter"), "REPORT_SALES keeps the request");
    assert!(seen(everything).await.is_empty());

    assert_eq!(orders::device_of(s.t.pool(), &s.card_sale).await.unwrap().as_deref(), Some(own.as_str()));
    assert_eq!(orders::device_of(s.t.pool(), "no-such-bill").await.unwrap(), None);
}
