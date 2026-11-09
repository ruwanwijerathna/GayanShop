//! Customers keyed by phone (doc §3.8): id = UUIDv5(store, E.164 phone), so offline counters converge on one
//! row. Orders store only `customer_id`, never the phone. An erased customer who returns is re-activated
//! under the same id with a fresh profile.

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{SqliteConnection, SqlitePool};

use super::WriteCtx;
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::clock::stable_id;
use crate::db::{Db, outbox};
use crate::domain::phone;
use crate::error::{AppError, AppResult};

const NAME_MAX: usize = 60;

/// The customer a sale is linked to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerRef {
    pub id: String,
    pub created: bool,
}

pub fn customer_id(store_id: &str, phone_e164: &str) -> String {
    stable_id("customer", store_id, phone_e164)
}

/// Normalise a typed phone (`PHONE_INVALID` otherwise) and an optional display name.
pub fn clean(phone_input: &str, name: Option<&str>) -> AppResult<(String, Option<String>)> {
    let e164 = phone::normalize_lk(phone_input).map_err(|e| AppError::validation("PHONE_INVALID", e.to_string()))?;
    let name = name.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    if name.as_ref().is_some_and(|n| n.chars().count() > NAME_MAX) {
        return Err(AppError::validation("CUSTOMER_NAME_INVALID", format!("max {NAME_MAX}")));
    }
    Ok((e164, name))
}

/// Link to the existing customer for this phone, or create (or re-activate) one. Runs inside the caller's
/// transaction, before the order insert (the order's `customer_id` is frozen once inserted).
pub async fn resolve_or_create(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    actor: &Actor,
    phone_e164: &str,
    name: Option<&str>,
) -> AppResult<CustomerRef> {
    let id = customer_id(&ctx.store_id, phone_e164);
    let existing: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT display_name, erased_at FROM customers WHERE id = ?1")
            .bind(&id)
            .fetch_optional(&mut *conn)
            .await?;
    let created = match existing {
        None => {
            sqlx::query(
                "INSERT INTO customers (id, store_id, phone_e164, display_name, first_seen_at, created_at, updated_at,
                                        updated_by_device)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?5, ?6)",
            )
            .bind(&id)
            .bind(&ctx.store_id)
            .bind(phone_e164)
            .bind(name)
            .bind(&ctx.now)
            .bind(&ctx.device_id)
            .execute(&mut *conn)
            .await?;
            true
        }
        Some((_, Some(_erased))) => {
            sqlx::query(
                "UPDATE customers SET phone_e164 = ?1, display_name = ?2, note = NULL, erased_at = NULL,
                        deleted_at = NULL, updated_at = ?3, updated_by_device = ?4, version = version + 1
                 WHERE id = ?5",
            )
            .bind(phone_e164)
            .bind(name)
            .bind(&ctx.now)
            .bind(&ctx.device_id)
            .bind(&id)
            .execute(&mut *conn)
            .await?;
            true
        }
        Some((stored_name, None)) => {
            if name.is_some() && name != stored_name.as_deref() {
                sqlx::query(
                    "UPDATE customers SET display_name = ?1, updated_at = ?2, updated_by_device = ?3,
                            version = version + 1
                     WHERE id = ?4",
                )
                .bind(name)
                .bind(&ctx.now)
                .bind(&ctx.device_id)
                .bind(&id)
                .execute(&mut *conn)
                .await?;
                outbox::enqueue_upsert(conn, "customers", &id, &ctx.now).await?;
            }
            false
        }
    };
    if created {
        outbox::enqueue_upsert(conn, "customers", &id, &ctx.now).await?;
        audit::record(
            conn,
            &AuditCtx {
                store_id: &ctx.store_id,
                device_id: &ctx.device_id,
                business_date: &ctx.business_date,
                now: &ctx.now,
            },
            AuditEntry {
                action: AuditAction::CUSTOMER_CREATE,
                staff_id: Some(&actor.staff_id),
                approved_by: None,
                entity_type: Some("customer"),
                entity_id: Some(&id),
                // Never the phone itself: the audit log is immutable and erasure must stay possible.
                metadata: json!({ "named": name.is_some() }),
            },
        )
        .await?;
    }
    Ok(CustomerRef { id, created })
}

/// What the till sees after 9–10 digits (doc §3.8): no history without `CUSTOMER_VIEW`.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CustomerLookup {
    /// `None` ⇒ a new number (or an erased profile): the customer is created at checkout.
    pub customer_id: Option<String>,
    pub phone_e164: String,
    /// `077 123 4567`, for the cart header.
    pub phone_display: String,
    pub display_name: Option<String>,
    /// Completed visits so far (voids excluded); the header shows "Visit #N+1".
    #[specta(type = specta_typescript::Number)]
    pub visit_count: i64,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CustomerLookupInput {
    pub phone: String,
}

/// Look a typed phone up by its UUIDv5 id (a primary-key read, so it is instant and offline). An erased
/// profile reads as new: its pre-erasure visits stay anonymous.
pub async fn lookup(pool: &SqlitePool, store_id: &str, phone_input: &str) -> AppResult<CustomerLookup> {
    let (e164, _) = clean(phone_input, None)?;
    let id = customer_id(store_id, &e164);
    let row: Option<(Option<String>, Option<String>, i64)> = sqlx::query_as(
        "SELECT c.display_name, c.erased_at, COALESCE(cs.visit_count, 0)
         FROM customers c LEFT JOIN customer_stats cs ON cs.customer_id = c.id
         WHERE c.id = ?1 AND c.deleted_at IS NULL",
    )
    .bind(&id)
    .fetch_optional(pool)
    .await?;
    let phone_display = phone::format_national(&e164).unwrap_or_else(|| e164.clone());
    Ok(match row {
        Some((display_name, None, visit_count)) => {
            CustomerLookup { customer_id: Some(id), phone_e164: e164, phone_display, display_name, visit_count }
        }
        _ => CustomerLookup { customer_id: None, phone_e164: e164, phone_display, display_name: None, visit_count: 0 },
    })
}

/// PDPA erasure (doc §3.8): clear the phone, name and note and stamp `erased_at`. The row and its id stay, so
/// past orders keep an anonymous `customer_id` and every total is unchanged; orders are never touched.
pub async fn erase(db: &Db, ctx: &WriteCtx, actor: &Actor, id: &str) -> AppResult<()> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let erased: Option<Option<String>> =
        sqlx::query_scalar("SELECT erased_at FROM customers WHERE id = ?1 AND store_id = ?2")
            .bind(id)
            .bind(&ctx.store_id)
            .fetch_optional(&mut *conn)
            .await?;
    match erased {
        None => return Err(AppError::not_found("customer")),
        Some(Some(_)) => return Err(AppError::validation("CUSTOMER_ALREADY_ERASED", id.to_owned())),
        Some(None) => {}
    }
    sqlx::query(
        "UPDATE customers SET phone_e164 = NULL, display_name = NULL, note = NULL, erased_at = ?1, updated_at = ?1,
                updated_by_device = ?2, version = version + 1
         WHERE id = ?3",
    )
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    audit::record(
        conn,
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::CUSTOMER_ERASE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("customer"),
            entity_id: Some(id),
            metadata: json!({}),
        },
    )
    .await?;
    outbox::supersede(conn, "customers", id).await?;
    outbox::enqueue_upsert(conn, "customers", id, &ctx.now).await?;
    tx.commit().await?;
    Ok(())
}
