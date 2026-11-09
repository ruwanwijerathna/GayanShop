//! Block 1.9 — held carts: the debounced crash-recovery cart, hold (F8) and recall (F11), and a sold cart
//! never being offered back.

mod common;

use common::{CASHIER, cart, cash, line, till};
use gayanshop_lib::db::repo::checkout::{self, NoFaults};
use gayanshop_lib::db::repo::held_carts;
use gayanshop_lib::error::AppError;

const NOW: &str = "2026-11-08T09:00:00.000Z";

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn the_current_cart_is_saved_replaced_and_offered_back() {
    let t = till().await;
    assert_eq!(held_carts::current(t.pool()).await.unwrap(), None);
    held_carts::save_current(&t.t.db, NOW, r#"{"lines":[1]}"#, Some("tok-a")).await.unwrap();
    held_carts::save_current(&t.t.db, NOW, r#"{"lines":[1,2]}"#, Some("tok-a")).await.unwrap();
    let c = held_carts::current(t.pool()).await.unwrap().unwrap();
    assert_eq!((c.payload.as_str(), c.checkout_token.as_deref()), (r#"{"lines":[1,2]}"#, Some("tok-a")));
    assert_eq!(t.count("SELECT COUNT(*) FROM held_carts").await, 1, "one recovery row, upserted");
    held_carts::discard_current(&t.t.db).await.unwrap();
    assert_eq!(held_carts::current(t.pool()).await.unwrap(), None);
}

#[tokio::test]
async fn a_sold_cart_is_never_offered_back() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    held_carts::save_current(&t.t.db, NOW, r#"{"lines":[]}"#, Some("sold-1")).await.unwrap();
    let input = cart("sold-1", vec![line(&mug, 25_000, 1)], vec![cash(25_000, None)]);
    checkout::checkout(&t.t.db, &t.ctx(), &t.seller(CASHIER).await, &input, None, &NoFaults).await.unwrap();
    assert_eq!(held_carts::current(t.pool()).await.unwrap(), None, "the checkout deleted it");

    // Even a stale save that lands after the sale (a debounce racing the commit) is not offered.
    held_carts::save_current(&t.t.db, NOW, r#"{"lines":[]}"#, Some("sold-1")).await.unwrap();
    assert_eq!(held_carts::current(t.pool()).await.unwrap(), None);
}

#[tokio::test]
async fn hold_and_recall() {
    let t = till().await;
    held_carts::save_current(&t.t.db, NOW, r#"{"n":1}"#, Some("t1")).await.unwrap();
    let held = held_carts::hold(&t.t.db, NOW, Some("  Lady in red "), r#"{"n":1}"#, Some("t1")).await.unwrap();
    assert_eq!(held.label.as_deref(), Some("Lady in red"));
    assert_eq!(held_carts::current(t.pool()).await.unwrap(), None, "holding parks the cart in progress");
    held_carts::hold(&t.t.db, "2026-11-08T09:05:00.000Z", None, r#"{"n":2}"#, Some("t2")).await.unwrap();
    let list = held_carts::list(t.pool()).await.unwrap();
    assert_eq!(list.iter().map(|c| c.checkout_token.as_deref()).collect::<Vec<_>>(), vec![Some("t2"), Some("t1")]);

    let back = held_carts::recall(&t.t.db, &held.id).await.unwrap();
    assert_eq!(back.payload, r#"{"n":1}"#);
    assert_eq!(held_carts::list(t.pool()).await.unwrap().len(), 1);
    assert!(matches!(held_carts::recall(&t.t.db, &held.id).await.unwrap_err(), AppError::NotFound { .. }));
    assert!(matches!(held_carts::recall(&t.t.db, "__current__").await.unwrap_err(), AppError::NotFound { .. }));
}

#[tokio::test]
async fn payloads_are_bounded_json_objects() {
    let t = till().await;
    for bad in ["", "[1]", "not json", "42"] {
        assert_eq!(code(held_carts::save_current(&t.t.db, NOW, bad, None).await.unwrap_err()), "CART_INVALID");
    }
    let huge = format!(r#"{{"x":"{}"}}"#, "a".repeat(held_carts::PAYLOAD_MAX));
    assert_eq!(code(held_carts::save_current(&t.t.db, NOW, &huge, None).await.unwrap_err()), "CART_TOO_LARGE");
    let long = "x".repeat(41);
    assert_eq!(code(held_carts::hold(&t.t.db, NOW, Some(&long), "{}", None).await.unwrap_err()), "TEXT_TOO_LONG");
    for _ in 0..held_carts::HELD_MAX {
        held_carts::hold(&t.t.db, NOW, None, "{}", None).await.unwrap();
    }
    assert_eq!(code(held_carts::hold(&t.t.db, NOW, None, "{}", None).await.unwrap_err()), "TOO_MANY_HELD");
}
