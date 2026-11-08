//! Same-session void (doc §3.3). A cashier's attempt fails with `ApprovalRequired { VOID_ORDER }`; the UI gets
//! a supervisor's single-use approval bound to the order id and retries with it.

use tauri::{AppHandle, State};

use super::{emit_changed, session, write_ctx};
use crate::auth::Auth;
use crate::clock::Clock;
use crate::db::Db;
use crate::db::repo::WriteCtx;
use crate::db::repo::checkout::NoFaults;
use crate::db::repo::void::{self, SessionOrder, VoidInput, VoidResult, Voider};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const VOID_TABLES: [&str; 6] = ["orders", "stock_movements", "cash_sessions", "customers", "batches", "audit_logs"];

/// Void as the logged-in staff member; on `ApprovalRequired` with an approval id, consume it (bound to this
/// order) and void again with the approver.
pub async fn void(auth: &Auth, clock: &dyn Clock, db: &Db, ctx: &WriteCtx, input: &VoidInput) -> AppResult<VoidResult> {
    let s = auth.sessions.touch(clock.now(), auth.idle_lock)?;
    let voider = Voider { staff_id: s.staff_id, perms: s.perms };
    match void::void_order(db, ctx, &voider, input, None, &NoFaults).await {
        Err(AppError::ApprovalRequired { perm }) => {
            let Some(approval_id) = input.approval_id.as_deref() else {
                return Err(AppError::ApprovalRequired { perm });
            };
            let approver = auth
                .approvals
                .consume(approval_id, Perm::VOID_ORDER, &input.order_id, &voider.staff_id, clock.now())
                .ok_or(AppError::InvalidApproval)?;
            void::void_order(db, ctx, &voider, input, Some(&approver), &NoFaults).await
        }
        other => other,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn void_order(app: AppHandle, state: State<'_, AppState>, input: VoidInput) -> AppResult<VoidResult> {
    let ctx = write_ctx(&state)?;
    let result = void(&state.auth, state.clock.as_ref(), &state.db, &ctx, &input).await?;
    emit_changed(&app, &VOID_TABLES);
    Ok(result)
}

/// The open session's orders on this counter (the void list).
#[tauri::command]
#[specta::specta]
pub async fn session_orders(state: State<'_, AppState>) -> AppResult<Vec<SessionOrder>> {
    session(&state)?;
    let device_id = state.identity()?.ok_or(AppError::NotInitialized)?.device_id;
    void::session_orders(&state.db.pool, &device_id).await
}
