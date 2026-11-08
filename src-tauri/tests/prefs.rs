//! Block 0.9 — UI preferences: device defaults are local; a staff member's language syncs (outbox).

mod common;

use common::{NOW, open, scalar_i64};
use gayanshop_lib::auth::pin::PinHasher;
use gayanshop_lib::clock::ManualClock;
use gayanshop_lib::error::AppError;
use gayanshop_lib::prefs::{self, DevicePreferences, UiLanguage, UiTheme};
use gayanshop_lib::setup::{SetupInput, first_run};

#[tokio::test]
async fn device_preferences_default_and_update() {
    let t = open().await;
    assert_eq!(
        prefs::device_preferences(t.pool()).await.unwrap(),
        DevicePreferences { default_language: UiLanguage::En, theme: UiTheme::Light }
    );
    prefs::set_device_theme(&t.db, UiTheme::Dark).await.unwrap();
    prefs::set_device_language(&t.db, UiLanguage::Si).await.unwrap();
    assert_eq!(
        prefs::device_preferences(t.pool()).await.unwrap(),
        DevicePreferences { default_language: UiLanguage::Si, theme: UiTheme::Dark }
    );
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM sync_outbox").await, 0, "device prefs never sync");
}

#[tokio::test]
async fn staff_language_is_saved_and_queued_for_sync() {
    let t = open().await;
    let clock = ManualClock::new(chrono::Utc::now());
    let setup = first_run(
        &t.db,
        &PinHasher::fast_for_tests(),
        &clock,
        &SetupInput {
            store_name: "GayanShop".into(),
            owner_name: "Gayan".into(),
            owner_pin: "246810".into(),
            device_code: "T01".into(),
            ui_language: "en".into(),
        },
    )
    .await
    .unwrap();
    let before = scalar_i64(t.pool(), "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'staff'").await;
    prefs::set_staff_language(&t.db, &setup.owner_staff_id, &setup.device_id, NOW, UiLanguage::Si).await.unwrap();
    let (lang, version): (String, i64) = sqlx::query_as("SELECT ui_language, version FROM staff WHERE id = ?1")
        .bind(&setup.owner_staff_id)
        .fetch_one(t.pool())
        .await
        .unwrap();
    assert_eq!((lang.as_str(), version), ("si", 2));
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'staff'").await, before + 1);
    assert_eq!(
        prefs::set_staff_language(&t.db, "nobody", &setup.device_id, NOW, UiLanguage::Si).await,
        Err(AppError::not_found("staff"))
    );
}
