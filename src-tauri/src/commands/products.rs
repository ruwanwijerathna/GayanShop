//! Catalog commands: categories, products, the scanner code lookup and type-ahead search.
//! Reads need a logged-in session; writes need `PRODUCT_MANAGE`; cost needs `VIEW_COST` (doc §3.5).

use tauri::{AppHandle, State};

use super::{emit_changed, session, store_id, write_ctx};
use crate::auth::guard::{Actor, require};
use crate::db::repo::categories::{self, Category, CategoryInput};
use crate::db::repo::products::{self, Product, ProductInput, ProductSummary};
use crate::domain::permissions::Perm;
use crate::error::AppResult;
use crate::state::AppState;

/// `PRODUCT_MANAGE` is not approvable, so this is a plain allow/deny.
fn manager(state: &AppState) -> AppResult<(Actor, bool)> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::PRODUCT_MANAGE, "products", None)?;
    Ok((actor, session(state)?.perms.has(Perm::VIEW_COST)))
}

#[tauri::command]
#[specta::specta]
pub async fn list_categories(state: State<'_, AppState>) -> AppResult<Vec<Category>> {
    session(&state)?;
    categories::list(&state.db.pool, &store_id(&state)?).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_category(app: AppHandle, state: State<'_, AppState>, input: CategoryInput) -> AppResult<Category> {
    manager(&state)?;
    let created = categories::create(&state.db, &write_ctx(&state)?, &input).await?;
    emit_changed(&app, &["categories"]);
    Ok(created)
}

#[tauri::command]
#[specta::specta]
pub async fn update_category(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: CategoryInput,
) -> AppResult<Category> {
    manager(&state)?;
    let updated = categories::update(&state.db, &write_ctx(&state)?, &id, &input).await?;
    emit_changed(&app, &["categories"]);
    Ok(updated)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_category(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    manager(&state)?;
    categories::delete(&state.db, &write_ctx(&state)?, &id).await?;
    emit_changed(&app, &["categories"]);
    Ok(())
}

/// Every live product for the back-office list (cost masked without `VIEW_COST`).
#[tauri::command]
#[specta::specta]
pub async fn list_products(state: State<'_, AppState>) -> AppResult<Vec<Product>> {
    let view_cost = session(&state)?.perms.has(Perm::VIEW_COST);
    products::list(&state.db.pool, &store_id(&state)?, view_cost).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_product(state: State<'_, AppState>, id: String) -> AppResult<Product> {
    let view_cost = session(&state)?.perms.has(Perm::VIEW_COST);
    products::get(&state.db.pool, &store_id(&state)?, &id, view_cost).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_product(app: AppHandle, state: State<'_, AppState>, input: ProductInput) -> AppResult<Product> {
    let (actor, view_cost) = manager(&state)?;
    let created = products::create(&state.db, &write_ctx(&state)?, &actor, &input, view_cost).await?;
    emit_changed(&app, &["products", "audit_logs"]);
    Ok(created)
}

#[tauri::command]
#[specta::specta]
pub async fn update_product(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: ProductInput,
) -> AppResult<Product> {
    let (actor, view_cost) = manager(&state)?;
    let updated = products::update(&state.db, &write_ctx(&state)?, &actor, &id, &input, view_cost).await?;
    emit_changed(&app, &["products", "audit_logs"]);
    Ok(updated)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_product(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    let (actor, _) = manager(&state)?;
    products::delete(&state.db, &write_ctx(&state)?, &actor, &id).await?;
    emit_changed(&app, &["products", "audit_logs"]);
    Ok(())
}

/// Type-ahead search (English names, SKUs, barcodes; Sinhala names by substring).
#[tauri::command]
#[specta::specta]
pub async fn search_products(state: State<'_, AppState>, query: String, limit: u32) -> AppResult<Vec<ProductSummary>> {
    session(&state)?;
    products::search(&state.db.pool, &store_id(&state)?, &query, limit).await
}

/// Scanner lookup: every active product with this barcode (variants may share one), else the in-store SKU.
/// Empty = unknown code, one = add it, several = the till asks which.
#[tauri::command]
#[specta::specta]
pub async fn lookup_product_code_matches(state: State<'_, AppState>, code: String) -> AppResult<Vec<ProductSummary>> {
    session(&state)?;
    products::lookup_code_matches(&state.db.pool, &store_id(&state)?, &code).await
}
