//! UI preferences: per-device defaults (local-only) and per-staff language (synced LWW row).

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::SqlitePool;

use crate::db::{Db, outbox, settings};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum UiLanguage {
    En,
    Si,
}

impl UiLanguage {
    pub fn code(self) -> &'static str {
        match self {
            UiLanguage::En => "en",
            UiLanguage::Si => "si",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum UiTheme {
    Light,
    Dark,
}

impl UiTheme {
    pub fn code(self) -> &'static str {
        match self {
            UiTheme::Light => "light",
            UiTheme::Dark => "dark",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DevicePreferences {
    /// Language for the lock/login screens (before anyone logs in).
    pub default_language: UiLanguage,
    pub theme: UiTheme,
}

pub async fn device_preferences(pool: &SqlitePool) -> AppResult<DevicePreferences> {
    let lang: Option<String> = settings::device_get(pool, "ui.default_language").await?;
    let theme: Option<String> = settings::device_get(pool, "ui.theme").await?;
    Ok(DevicePreferences {
        default_language: if lang.as_deref() == Some("si") { UiLanguage::Si } else { UiLanguage::En },
        theme: if theme.as_deref() == Some("dark") { UiTheme::Dark } else { UiTheme::Light },
    })
}

pub async fn set_device_theme(db: &Db, theme: UiTheme) -> AppResult<()> {
    let mut tx = db.write().await?;
    settings::device_set(tx.conn(), "ui.theme", &theme.code()).await?;
    tx.commit().await
}

pub async fn set_device_language(db: &Db, language: UiLanguage) -> AppResult<()> {
    let mut tx = db.write().await?;
    settings::device_set(tx.conn(), "ui.default_language", &language.code()).await?;
    tx.commit().await
}

/// A staff member's own language (applied at their next login on any counter). Synced via the outbox.
pub async fn set_staff_language(
    db: &Db,
    staff_id: &str,
    device_id: &str,
    now: &str,
    language: UiLanguage,
) -> AppResult<()> {
    let mut tx = db.write().await?;
    let updated = sqlx::query(
        "UPDATE staff SET ui_language = ?1, updated_at = ?2, updated_by_device = ?3, version = version + 1
         WHERE id = ?4 AND deleted_at IS NULL",
    )
    .bind(language.code())
    .bind(now)
    .bind(device_id)
    .bind(staff_id)
    .execute(tx.conn())
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::not_found("staff"));
    }
    outbox::enqueue_upsert(tx.conn(), "staff", staff_id, now).await?;
    tx.commit().await
}
