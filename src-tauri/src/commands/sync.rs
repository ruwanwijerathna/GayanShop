//! Settings → Sync (doc §1.3, §6): register this counter with the cloud (`SYNC_MANAGE`), and what it is
//! registered with. The owner's cloud password passes through once, to the auth server, and is never kept.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};

use super::{emit_changed, session};
use crate::auth::guard::require;
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::sync::config;
use crate::sync::registration::{self, RegisterInput, Registered};

#[derive(Debug, Clone, Serialize, Type)]
pub struct SyncInfo {
    /// A server and a device identity are configured.
    pub registered: bool,
    pub server_url: Option<String>,
    pub registered_at: Option<String>,
    pub device_code: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn sync_info(state: State<'_, AppState>) -> AppResult<SyncInfo> {
    session(&state)?;
    let identity = state.identity()?;
    Ok(SyncInfo {
        registered: registration::is_registered(&state.db, state.secrets.as_ref()).await?,
        server_url: config::load(&state.db.pool).await?.map(|c| c.url),
        registered_at: config::registered_at(&state.db.pool).await?,
        device_code: identity.map(|i| i.device_code),
    })
}

/// Register (or re-register) this counter, adopting its store, device id and code.
#[tauri::command]
#[specta::specta]
pub async fn sync_register(app: AppHandle, state: State<'_, AppState>, input: RegisterInput) -> AppResult<Registered> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::SYNC_MANAGE, "sync", None)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let registered = registration::register_this_counter(
        &state.db,
        state.clock.as_ref(),
        state.secrets.as_ref(),
        &identity,
        &actor.staff_id,
        &input,
    )
    .await?;
    emit_changed(&app, &["audit_logs", "device_settings"]);
    Ok(registered)
}
