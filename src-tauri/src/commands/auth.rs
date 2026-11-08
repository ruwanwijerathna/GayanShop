//! Login, logout, session and supervisor-approval commands.

use serde::Serialize;
use specta::Type;
use sqlx::FromRow;
use tauri::State;

use crate::auth::guard::require;
use crate::auth::session::SessionInfo;
use crate::auth::{self, ApprovalGrant, ApprovalRequest};
use crate::domain::permissions::Perm;
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Type, FromRow)]
pub struct LoginStaff {
    pub id: String,
    pub display_name: String,
    pub role_name: String,
}

/// Active staff for the login picker (names and roles only).
#[tauri::command]
#[specta::specta]
pub async fn list_login_staff(state: State<'_, AppState>) -> AppResult<Vec<LoginStaff>> {
    Ok(sqlx::query_as(
        "SELECT s.id, s.display_name, r.name AS role_name FROM staff s JOIN roles r ON r.id = s.role_id
         WHERE s.is_active = 1 AND s.deleted_at IS NULL ORDER BY s.display_name COLLATE NOCASE",
    )
    .fetch_all(&state.db.pool)
    .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn login(state: State<'_, AppState>, staff_id: String, pin: String) -> AppResult<SessionInfo> {
    let dev = state.device_ctx()?;
    auth::login(&state.db, &state.auth, state.clock.as_ref(), &dev, &staff_id, &pin).await
}

#[tauri::command]
#[specta::specta]
pub async fn logout(state: State<'_, AppState>) -> AppResult<()> {
    let dev = state.device_ctx()?;
    auth::logout(&state.db, &state.auth, state.clock.as_ref(), &dev).await
}

/// The active session, if any (idle sessions are reported as logged out).
#[tauri::command]
#[specta::specta]
pub async fn current_session(state: State<'_, AppState>) -> AppResult<Option<SessionInfo>> {
    let now = state.clock.now();
    Ok(state.auth.sessions.active(now, state.auth.idle_lock).as_ref().map(SessionInfo::from))
}

/// A supervisor enters their PIN to authorise `perm` on `target_ref` for the logged-in staff member.
#[tauri::command]
#[specta::specta]
pub async fn request_approval(
    state: State<'_, AppState>,
    perm: Perm,
    target_ref: String,
    approver_id: String,
    pin: String,
) -> AppResult<ApprovalGrant> {
    let dev = state.device_ctx()?;
    auth::request_approval(
        &state.db,
        &state.auth,
        state.clock.as_ref(),
        &dev,
        ApprovalRequest { perm, target_ref: &target_ref, approver_id: &approver_id, pin: &pin },
    )
    .await
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct Authorization {
    pub staff_id: String,
    pub approved_by: Option<String>,
}

/// Run the RBAC guard for `perm` (consuming `approval_id` if given). The UI calls this before opening a
/// privileged screen; each mutating command still runs the guard itself.
#[tauri::command]
#[specta::specta]
pub async fn authorize(
    state: State<'_, AppState>,
    perm: Perm,
    target_ref: String,
    approval_id: Option<String>,
) -> AppResult<Authorization> {
    let actor = require(&state.auth, state.clock.as_ref(), perm, &target_ref, approval_id.as_deref())?;
    Ok(Authorization { staff_id: actor.staff_id, approved_by: actor.approved_by })
}
