//! Stock operations (doc §3.1, §5): damage (`STOCK_DAMAGE`), adjustments and counts (`STOCK_ADJUST`), both
//! approvable with a supervisor's PIN; movement history, valuation and oversold layers need `REPORT_FINANCIAL`
//! (they show cost).

use tauri::{AppHandle, State};

use super::{emit_changed, store_id, write_ctx};
use crate::auth::guard::require;
use crate::db::repo::inventory::{
    self, AdjustmentInput, CountInput, CountResult, DamageInput, MovementCursor, MovementPage, OversoldLayer,
    StockChange, StockValuation,
};
use crate::domain::permissions::Perm;
use crate::error::AppResult;
use crate::state::AppState;

/// The approval target for stock writes (what the supervisor approves).
pub const STOCK_TARGET: &str = "stock";
const STOCK_TABLES: [&str; 4] = ["stock_movements", "batches", "batch_items", "audit_logs"];

#[tauri::command]
#[specta::specta]
pub async fn record_damage(
    app: AppHandle,
    state: State<'_, AppState>,
    input: DamageInput,
    approval_id: Option<String>,
) -> AppResult<StockChange> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::STOCK_DAMAGE, STOCK_TARGET, approval_id.as_deref())?;
    let change = inventory::damage(&state.db, &write_ctx(&state)?, &actor, &input).await?;
    emit_changed(&app, &STOCK_TABLES);
    Ok(change)
}

#[tauri::command]
#[specta::specta]
pub async fn adjust_stock(
    app: AppHandle,
    state: State<'_, AppState>,
    input: AdjustmentInput,
    approval_id: Option<String>,
) -> AppResult<StockChange> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::STOCK_ADJUST, STOCK_TARGET, approval_id.as_deref())?;
    let change = inventory::adjust(&state.db, &write_ctx(&state)?, &actor, &input).await?;
    emit_changed(&app, &STOCK_TABLES);
    Ok(change)
}

#[tauri::command]
#[specta::specta]
pub async fn record_stock_count(
    app: AppHandle,
    state: State<'_, AppState>,
    input: CountInput,
    approval_id: Option<String>,
) -> AppResult<CountResult> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::STOCK_ADJUST, STOCK_TARGET, approval_id.as_deref())?;
    let result = inventory::count(&state.db, &write_ctx(&state)?, &actor, &input).await?;
    emit_changed(&app, &STOCK_TABLES);
    Ok(result)
}

/// Write off what a received batch still holds, so it can be closed (`STOCK_ADJUST` and `BATCH_MANAGE`).
#[tauri::command]
#[specta::specta]
pub async fn write_off_batch(
    app: AppHandle,
    state: State<'_, AppState>,
    batch_id: String,
    note: String,
) -> AppResult<Vec<StockChange>> {
    require(&state.auth, state.clock.as_ref(), Perm::BATCH_MANAGE, "batches", None)?;
    let actor = require(&state.auth, state.clock.as_ref(), Perm::STOCK_ADJUST, STOCK_TARGET, None)?;
    let changes = inventory::write_off_batch(&state.db, &write_ctx(&state)?, &actor, &batch_id, &note).await?;
    emit_changed(&app, &STOCK_TABLES);
    Ok(changes)
}

#[tauri::command]
#[specta::specta]
pub async fn stock_movements(
    state: State<'_, AppState>,
    product_id: String,
    batch_item_id: Option<String>,
    cursor: Option<MovementCursor>,
    limit: u32,
) -> AppResult<MovementPage> {
    require(&state.auth, state.clock.as_ref(), Perm::REPORT_FINANCIAL, "reports", None)?;
    inventory::movements(&state.db.pool, &product_id, batch_item_id.as_deref(), cursor.as_ref(), limit).await
}

/// `as_of` = a business date (`YYYY-MM-DD`), or `None` for now.
#[tauri::command]
#[specta::specta]
pub async fn stock_valuation(state: State<'_, AppState>, as_of: Option<String>) -> AppResult<StockValuation> {
    require(&state.auth, state.clock.as_ref(), Perm::REPORT_FINANCIAL, "reports", None)?;
    inventory::valuation(&state.db.pool, &store_id(&state)?, as_of.as_deref()).await
}

#[tauri::command]
#[specta::specta]
pub async fn oversold_layers(state: State<'_, AppState>) -> AppResult<Vec<OversoldLayer>> {
    require(&state.auth, state.clock.as_ref(), Perm::REPORT_FINANCIAL, "reports", None)?;
    inventory::oversold(&state.db.pool, &store_id(&state)?).await
}
