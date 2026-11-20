//! Block 0.7 — first-run setup seeds every owner-confirmed value exactly, in one transaction.

mod common;

use chrono::{TimeZone, Utc};
use common::{TestDb, open, scalar_i64};
use gayanshop_lib::auth::pin::PinHasher;
use gayanshop_lib::auth::{Auth, DeviceCtx, login};
use gayanshop_lib::clock::ManualClock;
use gayanshop_lib::domain::permissions::{PermMask, default_roles};
use gayanshop_lib::error::AppError;
use gayanshop_lib::setup::{RECEIPT_FOOTER, SetupInput, SetupResult, first_run, is_initialized, load_identity};
use serde_json::{Value, json};

fn input() -> SetupInput {
    SetupInput {
        store_name: "  GayanShop  ".into(),
        owner_name: "Gayan".into(),
        owner_pin: "246810".into(),
        device_code: "t01".into(),
        ui_language: "si".into(),
    }
}

fn clock() -> ManualClock {
    // 01:30 Colombo on 9 Nov ⇒ business day 8 Nov (04:00 cutoff)
    ManualClock::new(Utc.with_ymd_and_hms(2026, 11, 8, 20, 0, 0).unwrap())
}

async fn setup() -> (TestDb, SetupResult) {
    let t = open().await;
    let result = first_run(&t.db, &PinHasher::fast_for_tests(), &clock(), &input()).await.unwrap();
    (t, result)
}

async fn store_setting(t: &TestDb, key: &str) -> Value {
    let raw: String = sqlx::query_scalar("SELECT value FROM store_settings WHERE key = ?1")
        .bind(key)
        .fetch_one(t.pool())
        .await
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

async fn device_setting(t: &TestDb, key: &str) -> Value {
    let raw: String = sqlx::query_scalar("SELECT value FROM device_settings WHERE key = ?1")
        .bind(key)
        .fetch_one(t.pool())
        .await
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[tokio::test]
async fn seeds_identity_and_device_settings() {
    let (t, r) = setup().await;
    assert!(is_initialized(t.pool()).await.unwrap());
    assert_eq!(r.device_code, "T01");
    let id = load_identity(t.pool()).await.unwrap().unwrap();
    assert_eq!(
        (id.store_id.as_str(), id.device_id.as_str(), id.device_code.as_str(), id.store_name.as_str()),
        (r.store_id.as_str(), r.device_id.as_str(), "T01", "GayanShop")
    );
    assert_eq!(uuid::Uuid::parse_str(&r.store_id).unwrap().get_version_num(), 7);
    assert_eq!(device_setting(&t, "ui.default_language").await, json!("si"));
    assert_eq!(device_setting(&t, "ui.theme").await, json!("light"));
    assert_eq!(device_setting(&t, "session.idle_lock_minutes").await, json!(5));
}

#[tokio::test]
async fn seeds_every_store_setting_exactly() {
    let (t, _) = setup().await;
    let expected = [
        ("store.name", json!("GayanShop")),
        ("store.currency", json!("LKR")),
        ("store.currency_exponent", json!(2)),
        ("store.timezone", json!("Asia/Colombo")),
        ("store.day_cutoff", json!("04:00")),
        ("store.week_start", json!("monday")),
        ("tax.inclusive", json!(true)),
        ("tax.default_rate_bp", json!(0)),
        ("discount.cashier_max_bp", json!(500)),
        ("sales.policy", json!("exchange_7d")),
        ("exchange.window_days", json!(7)),
        ("void.max_minutes", Value::Null),
        ("quick_sale.auto_tag_batch", json!(false)),
        (
            "receipt.footer",
            json!("Exchange within 7 days. Item for item only; no cash refunds. Thank you, come again!"),
        ),
        ("receipt.mask_customer_phone", json!(true)),
        ("customer.capture", json!("optional")),
        ("backup.reminder_days", json!(7)),
    ];
    for (key, value) in &expected {
        assert_eq!(&store_setting(&t, key).await, value, "{key}");
    }
    assert_eq!(
        scalar_i64(t.pool(), "SELECT COUNT(*) FROM store_settings").await,
        i64::try_from(expected.len()).unwrap()
    );
    assert_eq!(RECEIPT_FOOTER, "Exchange within 7 days. Item for item only; no cash refunds. Thank you, come again!");
}

#[tokio::test]
async fn seeds_roles_owner_categories_and_system_batches() {
    let (t, r) = setup().await;
    for seed in default_roles() {
        let (perms, disc, over, sys): (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT permissions, max_discount_bp, max_override_bp, is_system FROM roles WHERE name = ?1",
        )
        .bind(seed.name)
        .fetch_one(t.pool())
        .await
        .unwrap();
        assert_eq!(
            (perms, disc, over, sys),
            (seed.perms.0, seed.max_discount_bp, seed.max_override_bp, 1),
            "{}",
            seed.name
        );
    }
    assert_eq!(scalar_i64(t.pool(), "SELECT max_discount_bp FROM roles WHERE name = 'Cashier'").await, 500);
    assert_eq!(scalar_i64(t.pool(), "SELECT permissions FROM roles WHERE name = 'Owner'").await, PermMask::all().0);
    let owner: (String, String, String) = sqlx::query_as(
        "SELECT s.display_name, r.name, s.ui_language FROM staff s JOIN roles r ON r.id = s.role_id WHERE s.id = ?1",
    )
    .bind(&r.owner_staff_id)
    .fetch_one(t.pool())
    .await
    .unwrap();
    assert_eq!(owner, ("Gayan".into(), "Owner".into(), "si".into()));
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM expense_categories").await, 8);
    let batches: Vec<(String, String, Option<String>)> =
        sqlx::query_as("SELECT code, status, received_business_date FROM batches ORDER BY code")
            .fetch_all(t.pool())
            .await
            .unwrap();
    assert_eq!(
        batches,
        vec![
            ("ADJUSTMENTS".into(), "received".into(), Some("2026-11-08".into())),
            ("OPENING-STOCK".into(), "draft".into(), None)
        ]
    );
}

#[tokio::test]
async fn seeds_label_templates_at_203_dpi_with_3mm_gap() {
    let (t, _) = setup().await;
    let templates = device_setting(&t, "label.templates").await;
    let list = templates.as_array().unwrap();
    assert_eq!(list.len(), 2);
    for (tpl, id, cols, w, h, kinds) in [
        (&list[0], "two_up_50x25", 2, 50.0, 25.0, vec!["store_name", "barcode", "price"]),
        (&list[1], "three_up_30x16", 3, 30.0, 16.0, vec!["store_name", "barcode", "price"]),
    ] {
        assert_eq!(tpl["id"], json!(id));
        assert_eq!(tpl["columns"], json!(cols));
        assert_eq!(tpl["label_w_mm"], json!(w));
        assert_eq!(tpl["label_h_mm"], json!(h));
        assert_eq!(tpl["dpi"], json!(203));
        assert_eq!(tpl["row_gap_mm"], json!(3.0));
        let got: Vec<&str> = tpl["elements"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
        assert_eq!(got, kinds);
    }
    assert_eq!(list[1]["symbology"], json!("code128"), "3-up 1x1 prints the 6-digit SKU as Code128");
}

#[tokio::test]
async fn everything_is_outboxed_and_audited() {
    let (t, r) = setup().await;
    for (table, n) in [
        ("store_settings", 17),
        ("roles", 4),
        ("staff", 1),
        ("expense_categories", 8),
        ("batches", 2),
        ("audit_logs", 1),
    ] {
        let got = scalar_i64(t.pool(), &format!("SELECT COUNT(*) FROM sync_outbox WHERE table_name = '{table}'")).await;
        assert_eq!(got, n, "{table}");
    }
    let (action, staff, bd): (String, String, String) =
        sqlx::query_as("SELECT action, staff_id, business_date FROM audit_logs").fetch_one(t.pool()).await.unwrap();
    assert_eq!((action.as_str(), staff, bd.as_str()), ("STORE_INITIALIZED", r.owner_staff_id, "2026-11-08"));
    assert!(gayanshop_lib::audit::verify(t.pool()).await.unwrap()[0].ok);
}

#[tokio::test]
async fn runs_once_and_the_owner_can_log_in() {
    let (t, r) = setup().await;
    assert_eq!(
        first_run(&t.db, &PinHasher::fast_for_tests(), &clock(), &input()).await,
        Err(AppError::AlreadyInitialized)
    );
    let id = load_identity(t.pool()).await.unwrap().unwrap();
    let dev = DeviceCtx { store_id: id.store_id, device_id: id.device_id, calendar: id.calendar };
    let auth = Auth::new(PinHasher::fast_for_tests());
    let info = login(&t.db, &auth, &clock(), &dev, &r.owner_staff_id, "246810").await.unwrap();
    assert_eq!(info.permissions.len(), PermMask::all().list().len());
}

#[tokio::test]
async fn rejects_invalid_input_without_writing_anything() {
    let t = open().await;
    let hasher = PinHasher::fast_for_tests();
    let cases = [
        (SetupInput { owner_pin: "12345".into(), ..input() }, "errors.VALIDATION.PIN_TOO_SHORT"),
        (SetupInput { device_code: "T10".into(), ..input() }, "errors.VALIDATION.DEVICE_CODE_INVALID"),
        (SetupInput { store_name: "   ".into(), ..input() }, "errors.VALIDATION.STORE_NAME_INVALID"),
        (SetupInput { owner_name: String::new(), ..input() }, "errors.VALIDATION.OWNER_NAME_INVALID"),
        (SetupInput { ui_language: "fr".into(), ..input() }, "errors.VALIDATION.LANGUAGE_INVALID"),
    ];
    for (bad, key) in cases {
        assert_eq!(first_run(&t.db, &hasher, &clock(), &bad).await.unwrap_err().i18n_key(), key);
    }
    assert!(!is_initialized(t.pool()).await.unwrap());
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM roles").await, 0);
    assert!(load_identity(t.pool()).await.unwrap().is_none());
}
