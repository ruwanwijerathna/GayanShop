//! Quick-sale review and promotion (doc §3.2). Review, promote and ignore need `QUICK_SALE_PROMOTE` (not
//! approvable); the till's "now a product" hint needs only a login.

use tauri::{AppHandle, State};

use super::{emit_changed, session, store_id, write_ctx};
use crate::auth::guard::require;
use crate::db::repo::quick_sales::{
    self, PromoteInput, PromoteResult, PromotedHint, QuickSaleDetail, QuickSaleRow, QuickSaleStatus,
};
use crate::domain::permissions::Perm;
use crate::error::AppResult;
use crate::state::AppState;

fn reviewer(state: &AppState) -> AppResult<crate::auth::guard::Actor> {
    require(&state.auth, state.clock.as_ref(), Perm::QUICK_SALE_PROMOTE, "quick_sales", None)
}

#[tauri::command]
#[specta::specta]
pub async fn quick_sale_review(
    state: State<'_, AppState>,
    show: Option<QuickSaleStatus>,
) -> AppResult<Vec<QuickSaleRow>> {
    reviewer(&state)?;
    quick_sales::review(&state.db.pool, &store_id(&state)?, show).await
}

#[tauri::command]
#[specta::specta]
pub async fn quick_sale_detail(state: State<'_, AppState>, custom_key: String) -> AppResult<QuickSaleDetail> {
    reviewer(&state)?;
    quick_sales::detail(&state.db.pool, &custom_key).await
}

#[tauri::command]
#[specta::specta]
pub async fn promote_quick_sale(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PromoteInput,
) -> AppResult<PromoteResult> {
    let actor = reviewer(&state)?;
    let r = quick_sales::promote(&state.db, &write_ctx(&state)?, &actor, &input).await?;
    emit_changed(&app, &["products", "quick_sale_links", "batches", "stock_movements", "audit_logs"]);
    Ok(r)
}

#[tauri::command]
#[specta::specta]
pub async fn ignore_quick_sale(app: AppHandle, state: State<'_, AppState>, custom_key: String) -> AppResult<()> {
    let actor = reviewer(&state)?;
    quick_sales::ignore(&state.db, &write_ctx(&state)?, &actor, &custom_key).await?;
    emit_changed(&app, &["quick_sale_links", "audit_logs"]);
    Ok(())
}

/// "Now a product: Blue Bucket 10L (scan or ↓ Enter)" under the till's Quick Sale bar.
#[tauri::command]
#[specta::specta]
pub async fn quick_sale_hint(state: State<'_, AppState>, name: String) -> AppResult<Option<PromotedHint>> {
    session(&state)?;
    quick_sales::hint(&state.db.pool, &store_id(&state)?, &name).await
}
