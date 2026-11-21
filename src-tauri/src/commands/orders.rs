//! The Bills page (doc §1.4): keyset pages and the receipt drill-down. `REPORT_SALES` sees every counter;
//! any other staff member sees this counter's own bills (so a cashier can find a bill to reprint).

use tauri::State;

use super::session;
use crate::db::repo::orders::{self, OrderCursor, OrderFilter, OrderPage};
use crate::db::repo::receipts::{self, ReceiptData};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Whether the logged-in staff member may read this bill: `REPORT_SALES` (any counter) or it is this counter's.
pub(super) async fn can_view_order(state: &AppState, order_id: &str) -> AppResult<()> {
    let s = session(state)?;
    if s.perms.has(Perm::REPORT_SALES) {
        return Ok(());
    }
    let own = state.identity()?.ok_or(AppError::NotInitialized)?.device_id;
    match orders::device_of(&state.db.pool, order_id).await? {
        Some(device) if device == own => Ok(()),
        Some(_) => Err(AppError::Forbidden { perm: Perm::REPORT_SALES }),
        None => Err(AppError::not_found("order")),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn order_history(
    state: State<'_, AppState>,
    filter: OrderFilter,
    cursor: Option<OrderCursor>,
    limit: u32,
) -> AppResult<OrderPage> {
    let s = session(&state)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let filter = orders::visible_to(filter, &identity.device_id, s.perms.has(Perm::REPORT_SALES));
    orders::history(&state.db.pool, &identity.store_id, &identity.calendar, &filter, cursor.as_ref(), limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn order_receipt(state: State<'_, AppState>, order_id: String) -> AppResult<ReceiptData> {
    can_view_order(&state, &order_id).await?;
    receipts::load(&state.db.pool, &order_id).await
}
