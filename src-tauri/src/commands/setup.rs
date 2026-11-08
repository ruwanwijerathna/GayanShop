//! First-run setup command.

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::setup::{self, SetupInput, SetupResult};
use crate::state::AppState;

/// Create this store/terminal: identity, roles, the Owner and every seed, in one transaction.
#[tauri::command]
#[specta::specta]
pub async fn first_run_setup(state: State<'_, AppState>, input: SetupInput) -> AppResult<SetupResult> {
    let result = setup::first_run(&state.db, &state.auth.hasher, state.clock.as_ref(), &input).await?;
    let identity = setup::load_identity(&state.db.pool).await?.ok_or(AppError::NotInitialized)?;
    state.set_identity(identity)?;
    Ok(result)
}
