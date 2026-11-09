//! Back-office order history (doc §1.4): keyset pages and the receipt drill-down, both `REPORT_SALES`.

use tauri::State;

use crate::auth::guard::require;
use crate::db::repo::orders::{self, OrderCursor, OrderFilter, OrderPage};
use crate::db::repo::receipts::{self, ReceiptData};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn order_history(
    state: State<'_, AppState>,
    filter: OrderFilter,
    cursor: Option<OrderCursor>,
    limit: u32,
) -> AppResult<OrderPage> {
    require(&state.auth, state.clock.as_ref(), Perm::REPORT_SALES, "orders", None)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    orders::history(&state.db.pool, &identity.store_id, &identity.calendar, &filter, cursor.as_ref(), limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn order_receipt(state: State<'_, AppState>, order_id: String) -> AppResult<ReceiptData> {
    require(&state.auth, state.clock.as_ref(), Perm::REPORT_SALES, "orders", None)?;
    receipts::load(&state.db.pool, &order_id).await
}
