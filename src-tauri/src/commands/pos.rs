//! Till cart persistence (doc §1.4): the debounced crash-recovery cart and hold / recall (F8 / F11).
//! Local to this counter and never audited (nothing is sold until `checkout_order`); a login is required.

use serde::Deserialize;
use specta::Type;
use tauri::State;

use super::session;
use crate::clock::iso;
use crate::db::repo::held_carts::{self, HeldCart};
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CartInput {
    /// The till's cart JSON.
    pub payload: String,
    pub checkout_token: Option<String>,
    /// Hold only: a short name such as "Lady in red".
    pub label: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn cart_save_current(state: State<'_, AppState>, input: CartInput) -> AppResult<()> {
    session(&state)?;
    held_carts::save_current(&state.db, &iso(state.clock.now()), &input.payload, input.checkout_token.as_deref()).await
}

#[tauri::command]
#[specta::specta]
pub async fn cart_discard_current(state: State<'_, AppState>) -> AppResult<()> {
    session(&state)?;
    held_carts::discard_current(&state.db).await
}

/// "Recover cart?" after a restart.
#[tauri::command]
#[specta::specta]
pub async fn cart_current(state: State<'_, AppState>) -> AppResult<Option<HeldCart>> {
    session(&state)?;
    held_carts::current(&state.db.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn cart_hold(state: State<'_, AppState>, input: CartInput) -> AppResult<HeldCart> {
    session(&state)?;
    let now = iso(state.clock.now());
    held_carts::hold(&state.db, &now, input.label.as_deref(), &input.payload, input.checkout_token.as_deref()).await
}

#[tauri::command]
#[specta::specta]
pub async fn held_carts(state: State<'_, AppState>) -> AppResult<Vec<HeldCart>> {
    session(&state)?;
    held_carts::list(&state.db.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn cart_recall(state: State<'_, AppState>, id: String) -> AppResult<HeldCart> {
    session(&state)?;
    held_carts::recall(&state.db, &id).await
}
