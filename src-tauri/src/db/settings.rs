//! Device (local-only) and store (synced) settings. Values are JSON.

use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::{SqliteConnection, SqlitePool};

use super::outbox;
use crate::error::AppResult;

pub mod keys {
    pub const DEVICE_ID: &str = "device.id";
    pub const DEVICE_CODE: &str = "device.code";
    pub const STORE_ID: &str = "store.id";
    pub const IDLE_LOCK_MINUTES: &str = "session.idle_lock_minutes";

    pub const STORE_NAME: &str = "store.name";
    pub const TIMEZONE: &str = "store.timezone";
    pub const DAY_CUTOFF: &str = "store.day_cutoff";
    pub const WEEK_START: &str = "store.week_start";
}

pub async fn device_get<T: DeserializeOwned>(pool: &SqlitePool, key: &str) -> AppResult<Option<T>> {
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM device_settings WHERE key = ?1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(match raw {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    })
}

pub async fn device_set<T: Serialize>(conn: &mut SqliteConnection, key: &str, value: &T) -> AppResult<()> {
    sqlx::query("INSERT INTO device_settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(serde_json::to_string(value)?)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn store_get<T: DeserializeOwned>(pool: &SqlitePool, key: &str) -> AppResult<Option<T>> {
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM store_settings WHERE key = ?1 AND deleted_at IS NULL")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(match raw {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    })
}

/// Upsert a store setting (LWW row) and enqueue it for sync.
pub async fn store_set(
    conn: &mut SqliteConnection,
    store_id: &str,
    device_id: &str,
    now: &str,
    key: &str,
    value: &serde_json::Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO store_settings (key, store_id, value, created_at, updated_at, updated_by_device)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at,
           updated_by_device = excluded.updated_by_device, version = version + 1, deleted_at = NULL",
    )
    .bind(key)
    .bind(store_id)
    .bind(serde_json::to_string(value)?)
    .bind(now)
    .bind(device_id)
    .execute(&mut *conn)
    .await?;
    outbox::enqueue_upsert(conn, "store_settings", key, now).await
}
