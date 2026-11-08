//! Batch and stock-layer commands. Everything here shows or fixes costs, so all of it needs `BATCH_MANAGE`
//! (not approvable: Manager and Owner, doc §3.5).

use tauri::{AppHandle, State};

use super::{emit_changed, store_id, write_ctx};
use crate::auth::guard::{Actor, require};
use crate::db::repo::batches::{self, AdHocPurchase, Batch, BatchDetail, BatchInput, BatchItemInput};
use crate::db::repo::stock::{self, LayerStock};
use crate::domain::permissions::Perm;
use crate::error::AppResult;
use crate::state::AppState;

const STOCK_TABLES: [&str; 5] = ["batches", "batch_items", "stock_movements", "products", "audit_logs"];

fn batch_manager(state: &AppState) -> AppResult<Actor> {
    require(&state.auth, state.clock.as_ref(), Perm::BATCH_MANAGE, "batches", None)
}

#[tauri::command]
#[specta::specta]
pub async fn list_batches(state: State<'_, AppState>) -> AppResult<Vec<Batch>> {
    batch_manager(&state)?;
    batches::list(&state.db.pool, &store_id(&state)?).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_batch(state: State<'_, AppState>, id: String) -> AppResult<BatchDetail> {
    batch_manager(&state)?;
    batches::detail(&state.db.pool, &store_id(&state)?, &id).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_batch(app: AppHandle, state: State<'_, AppState>, input: BatchInput) -> AppResult<BatchDetail> {
    let actor = batch_manager(&state)?;
    let created = batches::create(&state.db, &write_ctx(&state)?, &actor, &input).await?;
    emit_changed(&app, &["batches", "audit_logs"]);
    Ok(created)
}

#[tauri::command]
#[specta::specta]
pub async fn update_batch(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: BatchInput,
) -> AppResult<BatchDetail> {
    batch_manager(&state)?;
    let updated = batches::update(&state.db, &write_ctx(&state)?, &id, &input).await?;
    emit_changed(&app, &["batches"]);
    Ok(updated)
}

#[tauri::command]
#[specta::specta]
pub async fn set_batch_item(
    app: AppHandle,
    state: State<'_, AppState>,
    batch_id: String,
    input: BatchItemInput,
) -> AppResult<BatchDetail> {
    batch_manager(&state)?;
    let detail = batches::set_item(&state.db, &write_ctx(&state)?, &batch_id, &input).await?;
    emit_changed(&app, &["batches", "batch_items"]);
    Ok(detail)
}

#[tauri::command]
#[specta::specta]
pub async fn remove_batch_item(
    app: AppHandle,
    state: State<'_, AppState>,
    batch_id: String,
    item_id: String,
) -> AppResult<BatchDetail> {
    batch_manager(&state)?;
    let detail = batches::remove_item(&state.db, &write_ctx(&state)?, &batch_id, &item_id).await?;
    emit_changed(&app, &["batches", "batch_items"]);
    Ok(detail)
}

/// Receive a draft: costs lock and stock arrives (one transaction).
#[tauri::command]
#[specta::specta]
pub async fn receive_batch(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<BatchDetail> {
    let actor = batch_manager(&state)?;
    let detail = batches::receive(&state.db, &write_ctx(&state)?, &actor, &id).await?;
    emit_changed(&app, &STOCK_TABLES);
    Ok(detail)
}

/// Buy and receive one item in one step.
#[tauri::command]
#[specta::specta]
pub async fn ad_hoc_purchase(
    app: AppHandle,
    state: State<'_, AppState>,
    input: AdHocPurchase,
) -> AppResult<BatchDetail> {
    let actor = batch_manager(&state)?;
    let detail = batches::ad_hoc_purchase(&state.db, &write_ctx(&state)?, &actor, &input).await?;
    emit_changed(&app, &STOCK_TABLES);
    Ok(detail)
}

/// A product's cost layers in FIFO order, with stock on hand per layer.
#[tauri::command]
#[specta::specta]
pub async fn product_layers(state: State<'_, AppState>, product_id: String) -> AppResult<Vec<LayerStock>> {
    batch_manager(&state)?;
    let mut conn = state.db.pool.acquire().await?;
    stock::product_layers(&mut conn, &product_id).await
}
