//! Customers at the till (doc §3.8). The lookup needs only a login: the cashier sees the name and visit
//! count, never history. The customer itself is created inside `checkout_order`.

use tauri::State;

use super::{session, store_id};
use crate::db::repo::customers::{self, CustomerLookup, CustomerLookupInput};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn customer_lookup(state: State<'_, AppState>, input: CustomerLookupInput) -> AppResult<CustomerLookup> {
    session(&state)?;
    customers::lookup(&state.db.pool, &store_id(&state)?, &input.phone).await
}
