//! Held carts (doc §1.4, §13.2): local-only, never synced. The reserved `__current__` row is the in-progress
//! cart, written by the till with a 500 ms debounce so a crash or power cut can offer "Recover cart?"; the
//! checkout transaction deletes it, so a sold cart is never offered back. "Hold sale" (F8) writes a named row
//! and "Recall" (F11) takes it back. The payload is the till's own cart JSON: Rust re-prices everything at
//! checkout, so it only checks that the payload is a bounded JSON object.

use serde::Serialize;
use specta::Type;
use sqlx::{FromRow, SqlitePool};

use super::checkout::CURRENT_CART;
use crate::clock::new_id;
use crate::db::Db;
use crate::error::{AppError, AppResult};

/// A cart is a few hundred lines at most; anything bigger is a bug, not a sale.
pub const PAYLOAD_MAX: usize = 256 * 1024;
const LABEL_MAX: usize = 40;
/// Held sales on one counter at once.
pub const HELD_MAX: i64 = 50;

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct HeldCart {
    pub id: String,
    pub label: Option<String>,
    /// The till's cart JSON.
    pub payload: String,
    pub checkout_token: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn check_payload(payload: &str) -> AppResult<()> {
    if payload.len() > PAYLOAD_MAX {
        return Err(AppError::validation("CART_TOO_LARGE", format!("max {PAYLOAD_MAX} bytes")));
    }
    match serde_json::from_str::<serde_json::Value>(payload) {
        Ok(serde_json::Value::Object(_)) => Ok(()),
        _ => Err(AppError::validation("CART_INVALID", "not a cart")),
    }
}

/// Write the in-progress cart (debounced by the till).
pub async fn save_current(db: &Db, now: &str, payload: &str, token: Option<&str>) -> AppResult<()> {
    check_payload(payload)?;
    let mut tx = db.write().await?;
    sqlx::query(
        "INSERT INTO held_carts (id, label, payload, checkout_token, created_at, updated_at)
         VALUES (?1, NULL, ?2, ?3, ?4, ?4)
         ON CONFLICT(id) DO UPDATE SET payload = excluded.payload, checkout_token = excluded.checkout_token,
           updated_at = excluded.updated_at",
    )
    .bind(CURRENT_CART)
    .bind(payload)
    .bind(token)
    .bind(now)
    .execute(tx.conn())
    .await?;
    tx.commit().await
}

/// The cart is empty again (cleared, or every line removed).
pub async fn discard_current(db: &Db) -> AppResult<()> {
    let mut tx = db.write().await?;
    sqlx::query("DELETE FROM held_carts WHERE id = ?1").bind(CURRENT_CART).execute(tx.conn()).await?;
    tx.commit().await
}

/// The cart to offer back after a restart, if any. A token that was already sold is never offered: the
/// checkout deleted the row in its own transaction, and this double-checks against the orders table.
pub async fn current(pool: &SqlitePool) -> AppResult<Option<HeldCart>> {
    Ok(sqlx::query_as(
        "SELECT id, label, payload, checkout_token, created_at, updated_at FROM held_carts h
         WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM orders o WHERE o.checkout_token = h.checkout_token)",
    )
    .bind(CURRENT_CART)
    .fetch_optional(pool)
    .await?)
}

/// F8: park the cart under a label; the till then starts a fresh one.
pub async fn hold(db: &Db, now: &str, label: Option<&str>, payload: &str, token: Option<&str>) -> AppResult<HeldCart> {
    check_payload(payload)?;
    let label = label.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    if label.as_ref().is_some_and(|l| l.chars().count() > LABEL_MAX) {
        return Err(AppError::validation("TEXT_TOO_LONG", format!("max {LABEL_MAX}")));
    }
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let held: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM held_carts WHERE id <> ?1")
        .bind(CURRENT_CART)
        .fetch_one(&mut *conn)
        .await?;
    if held >= HELD_MAX {
        return Err(AppError::validation("TOO_MANY_HELD", format!("max {HELD_MAX}")));
    }
    let cart = HeldCart {
        id: new_id(),
        label,
        payload: payload.to_owned(),
        checkout_token: token.map(str::to_owned),
        created_at: now.to_owned(),
        updated_at: now.to_owned(),
    };
    sqlx::query(
        "INSERT INTO held_carts (id, label, payload, checkout_token, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
    )
    .bind(&cart.id)
    .bind(cart.label.as_deref())
    .bind(&cart.payload)
    .bind(cart.checkout_token.as_deref())
    .bind(now)
    .execute(&mut *conn)
    .await?;
    // The parked cart is no longer the one in progress.
    sqlx::query("DELETE FROM held_carts WHERE id = ?1").bind(CURRENT_CART).execute(&mut *conn).await?;
    tx.commit().await?;
    Ok(cart)
}

/// F11's list, newest first (the crash-recovery row is not a held sale).
pub async fn list(pool: &SqlitePool) -> AppResult<Vec<HeldCart>> {
    Ok(sqlx::query_as(
        "SELECT id, label, payload, checkout_token, created_at, updated_at FROM held_carts
         WHERE id <> ?1 ORDER BY updated_at DESC, id DESC",
    )
    .bind(CURRENT_CART)
    .fetch_all(pool)
    .await?)
}

/// Take a held sale back: the row is removed and its cart becomes the one in progress.
pub async fn recall(db: &Db, id: &str) -> AppResult<HeldCart> {
    if id == CURRENT_CART {
        return Err(AppError::not_found("held cart"));
    }
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let cart: HeldCart = sqlx::query_as(
        "SELECT id, label, payload, checkout_token, created_at, updated_at FROM held_carts WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| AppError::not_found("held cart"))?;
    sqlx::query("DELETE FROM held_carts WHERE id = ?1").bind(id).execute(&mut *conn).await?;
    tx.commit().await?;
    Ok(cart)
}
