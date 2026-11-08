//! System information, status and health commands.

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::auth::session::SessionInfo;
use crate::db::integrity::{self, IntegrityReport};
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Type)]
pub struct AppInfo {
    pub app_version: String,
    /// Latest applied migration version.
    #[specta(type = specta_typescript::Number)]
    pub schema_version: i64,
    pub data_dir: String,
}

/// Version and storage information for the about box and the support bundle.
#[tauri::command]
#[specta::specta]
pub async fn app_info(state: State<'_, AppState>) -> AppResult<AppInfo> {
    Ok(AppInfo {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        schema_version: state.db.schema_version().await?,
        data_dir: state.data_dir.display().to_string(),
    })
}

/// SQLite quick_check plus the schema's integrity probes (ARCHITECTURE §13.2).
#[tauri::command]
#[specta::specta]
pub async fn integrity_check(state: State<'_, AppState>) -> AppResult<IntegrityReport> {
    integrity::check(&state.db.pool).await
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AppStatus {
    pub initialized: bool,
    pub store_name: Option<String>,
    pub device_code: Option<String>,
    pub session: Option<SessionInfo>,
}

/// What the shell needs to route: first-run wizard, login, or the app.
#[tauri::command]
#[specta::specta]
pub async fn app_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    let identity = state.identity()?;
    let session = state.auth.sessions.active(state.clock.now(), state.auth.idle_lock);
    Ok(AppStatus {
        initialized: identity.is_some(),
        store_name: identity.as_ref().map(|i| i.store_name.clone()),
        device_code: identity.as_ref().map(|i| i.device_code.clone()),
        session: session.as_ref().map(SessionInfo::from),
    })
}
