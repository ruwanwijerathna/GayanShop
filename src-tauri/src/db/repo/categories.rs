//! Product categories (LWW-synced, doc §6.3). A category is soft-deleted, and only when nothing uses it.

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use crate::clock::new_id;
use crate::db::{Db, outbox};
use crate::error::{AppError, AppResult};

const NAME_MAX: usize = 60;

#[derive(Debug, Clone, Serialize, Type, FromRow, PartialEq, Eq)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    /// Live products in this category (not counting sub-categories).
    #[specta(type = specta_typescript::Number)]
    pub product_count: i64,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CategoryInput {
    pub name: String,
    pub parent_id: Option<String>,
}

pub async fn list(pool: &SqlitePool, store_id: &str) -> AppResult<Vec<Category>> {
    Ok(sqlx::query_as(
        "SELECT c.id, c.name, c.parent_id,
                (SELECT COUNT(*) FROM products p WHERE p.category_id = c.id AND p.deleted_at IS NULL) AS product_count
         FROM categories c
         WHERE c.store_id = ?1 AND c.deleted_at IS NULL
         ORDER BY c.name COLLATE NOCASE, c.id",
    )
    .bind(store_id)
    .fetch_all(pool)
    .await?)
}

async fn get(conn: &mut SqliteConnection, store_id: &str, id: &str) -> AppResult<Category> {
    sqlx::query_as(
        "SELECT c.id, c.name, c.parent_id,
                (SELECT COUNT(*) FROM products p WHERE p.category_id = c.id AND p.deleted_at IS NULL) AS product_count
         FROM categories c WHERE c.store_id = ?1 AND c.id = ?2 AND c.deleted_at IS NULL",
    )
    .bind(store_id)
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| AppError::not_found("category"))
}

/// Live category ids reachable upwards from `start` (inclusive), used for the cycle check.
async fn ancestors(conn: &mut SqliteConnection, store_id: &str, start: &str) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "WITH RECURSIVE up(id, parent_id, depth) AS (
           SELECT id, parent_id, 0 FROM categories WHERE store_id = ?1 AND id = ?2 AND deleted_at IS NULL
           UNION ALL
           SELECT c.id, c.parent_id, up.depth + 1 FROM categories c JOIN up ON c.id = up.parent_id
           WHERE c.deleted_at IS NULL AND up.depth < 64
         )
         SELECT id FROM up",
    )
    .bind(store_id)
    .bind(start)
    .fetch_all(&mut *conn)
    .await?)
}

async fn validate(
    conn: &mut SqliteConnection,
    store_id: &str,
    self_id: Option<&str>,
    input: &CategoryInput,
) -> AppResult<(String, Option<String>)> {
    let name = input.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(AppError::validation("CATEGORY_NAME_INVALID", format!("1–{NAME_MAX} characters")));
    }
    let parent_id = input.parent_id.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned);
    if let Some(parent) = parent_id.as_deref() {
        let chain = ancestors(conn, store_id, parent).await?;
        if chain.is_empty() {
            return Err(AppError::validation("CATEGORY_PARENT_INVALID", "parent category not found"));
        }
        if self_id.is_some_and(|me| chain.iter().any(|c| c == me)) {
            return Err(AppError::validation("CATEGORY_CYCLE", "a category cannot sit inside itself"));
        }
    }
    let taken: Option<String> = sqlx::query_scalar(
        "SELECT id FROM categories
         WHERE store_id = ?1 AND deleted_at IS NULL AND name = ?2 COLLATE NOCASE
           AND parent_id IS ?3 AND id IS NOT ?4",
    )
    .bind(store_id)
    .bind(&name)
    .bind(parent_id.as_deref())
    .bind(self_id)
    .fetch_optional(&mut *conn)
    .await?;
    if taken.is_some() {
        return Err(AppError::validation("CATEGORY_NAME_TAKEN", name));
    }
    Ok((name, parent_id))
}

pub async fn create(db: &Db, ctx: &WriteCtx, input: &CategoryInput) -> AppResult<Category> {
    let mut tx = db.write().await?;
    let (name, parent_id) = validate(tx.conn(), &ctx.store_id, None, input).await?;
    let id = new_id();
    sqlx::query(
        "INSERT INTO categories (id, store_id, name, parent_id, created_at, updated_at, updated_by_device)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
    )
    .bind(&id)
    .bind(&ctx.store_id)
    .bind(&name)
    .bind(parent_id.as_deref())
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .execute(tx.conn())
    .await?;
    outbox::enqueue_upsert(tx.conn(), "categories", &id, &ctx.now).await?;
    let created = get(tx.conn(), &ctx.store_id, &id).await?;
    tx.commit().await?;
    Ok(created)
}

pub async fn update(db: &Db, ctx: &WriteCtx, id: &str, input: &CategoryInput) -> AppResult<Category> {
    let mut tx = db.write().await?;
    get(tx.conn(), &ctx.store_id, id).await?;
    let (name, parent_id) = validate(tx.conn(), &ctx.store_id, Some(id), input).await?;
    sqlx::query(
        "UPDATE categories SET name = ?1, parent_id = ?2, updated_at = ?3, updated_by_device = ?4, version = version + 1
         WHERE id = ?5 AND store_id = ?6 AND deleted_at IS NULL",
    )
    .bind(&name)
    .bind(parent_id.as_deref())
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .bind(&ctx.store_id)
    .execute(tx.conn())
    .await?;
    outbox::enqueue_upsert(tx.conn(), "categories", id, &ctx.now).await?;
    let updated = get(tx.conn(), &ctx.store_id, id).await?;
    tx.commit().await?;
    Ok(updated)
}

/// Soft delete. Refused while live products or live sub-categories still point at it.
pub async fn delete(db: &Db, ctx: &WriteCtx, id: &str) -> AppResult<()> {
    let mut tx = db.write().await?;
    let category = get(tx.conn(), &ctx.store_id, id).await?;
    let children: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM categories WHERE parent_id = ?1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
    if category.product_count > 0 || children > 0 {
        return Err(AppError::validation(
            "CATEGORY_IN_USE",
            format!("{} products, {children} sub-categories", category.product_count),
        ));
    }
    sqlx::query(
        "UPDATE categories SET deleted_at = ?1, updated_at = ?1, updated_by_device = ?2, version = version + 1
         WHERE id = ?3 AND store_id = ?4",
    )
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .bind(&ctx.store_id)
    .execute(tx.conn())
    .await?;
    outbox::enqueue_upsert(tx.conn(), "categories", id, &ctx.now).await?;
    tx.commit().await
}
