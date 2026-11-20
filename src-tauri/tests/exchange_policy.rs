//! Exchange policy rollout: a store created before exchanges moves to the 7-day exchange policy once, keeps an
//! owner-edited footer, gains only the two new permission bits, and leaves one audit row and its outbox rows.

mod common;

use chrono::{TimeZone, Utc};
use common::{TestDb, open, scalar_i64};
use gayanshop_lib::auth::pin::PinHasher;
use gayanshop_lib::clock::ManualClock;
use gayanshop_lib::setup::{
    LEGACY_RECEIPT_FOOTER, RECEIPT_FOOTER, SetupInput, first_run, load_identity, upgrade_exchange_policy,
};
use serde_json::{Value, json};

const EXCHANGE_BITS: i64 = 64 | 128;

fn clock() -> ManualClock {
    ManualClock::new(Utc.with_ymd_and_hms(2026, 11, 20, 5, 0, 0).unwrap())
}

async fn legacy_store(footer: &str) -> TestDb {
    let t = open().await;
    let input = SetupInput {
        store_name: "GayanShop".into(),
        owner_name: "Gayan".into(),
        owner_pin: "246810".into(),
        device_code: "T01".into(),
        ui_language: "en".into(),
    };
    first_run(&t.db, &PinHasher::fast_for_tests(), &clock(), &input).await.unwrap();
    // Rewind to what a pre-exchange store holds.
    for (key, value) in [("sales.policy", json!("final")), ("receipt.footer", json!(footer))] {
        sqlx::query("UPDATE store_settings SET value = ?2 WHERE key = ?1")
            .bind(key)
            .bind(value.to_string())
            .execute(t.pool())
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM store_settings WHERE key = 'exchange.window_days'").execute(t.pool()).await.unwrap();
    sqlx::query("UPDATE roles SET permissions = permissions & ~?1 WHERE name != 'Owner'")
        .bind(EXCHANGE_BITS)
        .execute(t.pool())
        .await
        .unwrap();
    t
}

async fn setting(t: &TestDb, key: &str) -> Value {
    let raw: String = sqlx::query_scalar("SELECT value FROM store_settings WHERE key = ?1")
        .bind(key)
        .fetch_one(t.pool())
        .await
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

async fn role_bits(t: &TestDb, name: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT permissions & ?2 FROM roles WHERE name = ?1")
        .bind(name)
        .bind(EXCHANGE_BITS)
        .fetch_one(t.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn legacy_store_moves_to_the_exchange_policy_once() {
    let t = legacy_store(LEGACY_RECEIPT_FOOTER).await;
    let id = load_identity(t.pool()).await.unwrap().unwrap();
    let before_outbox = scalar_i64(t.pool(), "SELECT COUNT(*) FROM sync_outbox").await;

    assert!(upgrade_exchange_policy(&t.db, &clock(), &id).await.unwrap());
    assert_eq!(setting(&t, "sales.policy").await, json!("exchange_7d"));
    assert_eq!(setting(&t, "exchange.window_days").await, json!(7));
    assert_eq!(setting(&t, "receipt.footer").await, json!(RECEIPT_FOOTER));
    assert_eq!(role_bits(&t, "Cashier").await, 64, "cashier processes exchanges, never late ones");
    assert_eq!(role_bits(&t, "Supervisor").await, EXCHANGE_BITS);
    assert_eq!(role_bits(&t, "Manager").await, EXCHANGE_BITS);
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM audit_logs WHERE action = 'POLICY_UPGRADED'").await, 1);
    assert!(
        scalar_i64(t.pool(), "SELECT COUNT(*) FROM sync_outbox").await > before_outbox,
        "changes are queued for sync"
    );

    // A second start changes nothing.
    let after = scalar_i64(t.pool(), "SELECT COUNT(*) FROM audit_logs").await;
    assert!(!upgrade_exchange_policy(&t.db, &clock(), &id).await.unwrap());
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM audit_logs").await, after);
}

#[tokio::test]
async fn an_owner_edited_footer_is_kept() {
    let t = legacy_store("Thanks for shopping at Sale Center!").await;
    let id = load_identity(t.pool()).await.unwrap().unwrap();
    assert!(upgrade_exchange_policy(&t.db, &clock(), &id).await.unwrap());
    assert_eq!(setting(&t, "receipt.footer").await, json!("Thanks for shopping at Sale Center!"));
    assert_eq!(setting(&t, "sales.policy").await, json!("exchange_7d"));
}

#[tokio::test]
async fn a_fresh_store_needs_no_upgrade() {
    let t = open().await;
    let input = SetupInput {
        store_name: "GayanShop".into(),
        owner_name: "Gayan".into(),
        owner_pin: "246810".into(),
        device_code: "T01".into(),
        ui_language: "en".into(),
    };
    first_run(&t.db, &PinHasher::fast_for_tests(), &clock(), &input).await.unwrap();
    let id = load_identity(t.pool()).await.unwrap().unwrap();
    assert!(!upgrade_exchange_policy(&t.db, &clock(), &id).await.unwrap());
    assert_eq!(role_bits(&t, "Cashier").await, 64);
}
