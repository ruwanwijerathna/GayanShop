//! Audit log viewer commands (AUDIT_VIEW — not approvable, so a supervisor PIN cannot lend it).

use tauri::State;

use crate::audit::{self, AuditCursor, AuditPage, ChainReport};
use crate::auth::guard::require;
use crate::domain::permissions::Perm;
use crate::error::AppResult;
use crate::state::AppState;

/// Newest-first keyset page of the audit trail.
#[tauri::command]
#[specta::specta]
pub async fn audit_page(state: State<'_, AppState>, cursor: Option<AuditCursor>, limit: u32) -> AppResult<AuditPage> {
    require(&state.auth, state.clock.as_ref(), Perm::AUDIT_VIEW, "audit", None)?;
    audit::page(&state.db.pool, cursor.as_ref(), limit).await
}

/// Re-verify every device's hash chain.
#[tauri::command]
#[specta::specta]
pub async fn audit_verify(state: State<'_, AppState>) -> AppResult<Vec<ChainReport>> {
    require(&state.auth, state.clock.as_ref(), Perm::AUDIT_VIEW, "audit", None)?;
    audit::verify(&state.db.pool).await
}
