//! Customers (doc §3.8). At the till the lookup needs only a login: the cashier sees the name and visit
//! count, never history, and the customer itself is created inside `checkout_order`. The back office (list,
//! profile, history, analytics, edit, erasure) needs `CUSTOMER_VIEW`, which is not approvable: §3.5 gives
//! it to Manager and Owner, the same roles §3.8 names for erasure.

use tauri::{AppHandle, State};

use super::{emit_changed, session, store_id, write_ctx};
use crate::auth::guard::{Actor, require};
use crate::db::repo::customers::{
    self, CustomerAnalytics, CustomerList, CustomerLookup, CustomerLookupInput, CustomerProfile, CustomerSort,
    CustomerUpdate, HistoryCursor, HistoryPage,
};
use crate::db::repo::receipts::ReceiptData;
use crate::domain::period::{Granularity, Period};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

fn viewer(state: &AppState, target: &str) -> AppResult<Actor> {
    require(&state.auth, state.clock.as_ref(), Perm::CUSTOMER_VIEW, target, None)
}

#[tauri::command]
#[specta::specta]
pub async fn customer_lookup(state: State<'_, AppState>, input: CustomerLookupInput) -> AppResult<CustomerLookup> {
    session(&state)?;
    customers::lookup(&state.db.pool, &store_id(&state)?, &input.phone).await
}

#[tauri::command]
#[specta::specta]
pub async fn customer_list(
    state: State<'_, AppState>,
    search: Option<String>,
    sort: Option<CustomerSort>,
) -> AppResult<CustomerList> {
    viewer(&state, "customers")?;
    customers::list(&state.db.pool, &store_id(&state)?, search.as_deref(), sort.unwrap_or_default(), None).await
}

#[tauri::command]
#[specta::specta]
pub async fn customer_profile(state: State<'_, AppState>, id: String) -> AppResult<CustomerProfile> {
    viewer(&state, &id)?;
    customers::profile(&state.db.pool, &store_id(&state)?, &id).await
}

#[tauri::command]
#[specta::specta]
pub async fn customer_history(
    state: State<'_, AppState>,
    id: String,
    cursor: Option<HistoryCursor>,
    limit: u32,
) -> AppResult<HistoryPage> {
    viewer(&state, &id)?;
    customers::history(&state.db.pool, &id, cursor.as_ref(), limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn customer_order_receipt(
    state: State<'_, AppState>,
    customer_id: String,
    order_id: String,
) -> AppResult<ReceiptData> {
    viewer(&state, &customer_id)?;
    customers::order_receipt(&state.db.pool, &customer_id, &order_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn customer_analytics(
    state: State<'_, AppState>,
    period: Period,
    granularity: Granularity,
    top_n: u32,
) -> AppResult<CustomerAnalytics> {
    viewer(&state, "customers")?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    customers::analytics(&state.db.pool, &identity.store_id, &identity.calendar, &period, granularity, top_n).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_customer(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: CustomerUpdate,
) -> AppResult<()> {
    let actor = viewer(&state, &id)?;
    customers::update(&state.db, &write_ctx(&state)?, &actor, &id, &input).await?;
    emit_changed(&app, &["customers", "audit_logs"]);
    Ok(())
}

/// PDPA erasure on request (doc §3.8).
#[tauri::command]
#[specta::specta]
pub async fn erase_customer(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    let actor = viewer(&state, &id)?;
    customers::erase(&state.db, &write_ctx(&state)?, &actor, &id).await?;
    emit_changed(&app, &["customers", "audit_logs"]);
    Ok(())
}
