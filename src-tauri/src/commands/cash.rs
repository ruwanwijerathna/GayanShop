//! Cash-session commands (doc §3.4). Open/close: `CASH_SESSION_MANAGE`. Pay-in/out and no-sale are approvable
//! (`CASH_PAY_IN_OUT`, `DRAWER_NO_SALE`): a cashier retries with a supervisor's approval id bound to
//! [`DRAWER_TARGET`]. History, X-reports and Z-reports need `REPORT_SALES` for any counter, and are open to staff for this counter's own shifts. Reprinting a Z-report needs `REPRINT_RECEIPT`.

use tauri::{AppHandle, State};

use super::{emit_changed, session, write_ctx};
use crate::auth::guard::require;
use crate::db::repo::cash::{
    self, CashSession, CloseInput, DrawerMovementInput, HistoryFilter, OpenSessionInput, SessionHistory, ZReport,
};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// The approval target for drawer actions (what the supervisor approves).
pub const DRAWER_TARGET: &str = "cash_drawer";

fn device_id(state: &AppState) -> AppResult<String> {
    Ok(state.identity()?.ok_or(AppError::NotInitialized)?.device_id)
}

/// This counter's open session, if any (any logged-in staff member: the till needs it).
#[tauri::command]
#[specta::specta]
pub async fn current_cash_session(state: State<'_, AppState>) -> AppResult<Option<CashSession>> {
    session(&state)?;
    cash::current(&state.db.pool, &device_id(&state)?).await
}

#[tauri::command]
#[specta::specta]
pub async fn open_cash_session(
    app: AppHandle,
    state: State<'_, AppState>,
    input: OpenSessionInput,
) -> AppResult<CashSession> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::CASH_SESSION_MANAGE, "cash_session", None)?;
    let opened = cash::open(&state.db, &write_ctx(&state)?, &actor, input.opening_float).await?;
    emit_changed(&app, &["cash_sessions", "audit_logs"]);
    Ok(opened)
}

/// Pay-in or pay-out with a reason; returns the updated X-report.
#[tauri::command]
#[specta::specta]
pub async fn cash_drawer_movement(
    app: AppHandle,
    state: State<'_, AppState>,
    input: DrawerMovementInput,
    approval_id: Option<String>,
) -> AppResult<ZReport> {
    let actor =
        require(&state.auth, state.clock.as_ref(), Perm::CASH_PAY_IN_OUT, DRAWER_TARGET, approval_id.as_deref())?;
    let ctx = write_ctx(&state)?;
    let report = cash::drawer_movement(&state.db, &ctx, &actor, input.kind, input.amount, &input.reason).await?;
    state.printer.notify();
    emit_changed(&app, &["cash_sessions", "cash_movements", "audit_logs"]);
    Ok(report)
}

/// Open the drawer without a sale.
#[tauri::command]
#[specta::specta]
pub async fn no_sale(
    app: AppHandle,
    state: State<'_, AppState>,
    reason: String,
    approval_id: Option<String>,
) -> AppResult<()> {
    let actor =
        require(&state.auth, state.clock.as_ref(), Perm::DRAWER_NO_SALE, DRAWER_TARGET, approval_id.as_deref())?;
    cash::no_sale(&state.db, &write_ctx(&state)?, &actor, &reason).await?;
    state.printer.notify();
    emit_changed(&app, &["cash_movements", "audit_logs"]);
    Ok(())
}

/// Count and close; returns the Z-report (also queued for printing).
#[tauri::command]
#[specta::specta]
pub async fn close_cash_session(app: AppHandle, state: State<'_, AppState>, input: CloseInput) -> AppResult<ZReport> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::CASH_SESSION_MANAGE, "cash_session", None)?;
    let report = cash::close(&state.db, &write_ctx(&state)?, &actor, &input).await?;
    state.printer.notify();
    emit_changed(&app, &["cash_sessions", "audit_logs"]);
    Ok(report)
}

/// This counter's recent sessions.
#[tauri::command]
#[specta::specta]
pub async fn list_cash_sessions(state: State<'_, AppState>, limit: u32) -> AppResult<Vec<CashSession>> {
    require(&state.auth, state.clock.as_ref(), Perm::REPORT_SALES, "reports", None)?;
    cash::list(&state.db.pool, &device_id(&state)?, limit).await
}

/// Shift history (Today | History). `REPORT_SALES` sees every counter; anyone else sees this counter's shifts.
#[tauri::command]
#[specta::specta]
pub async fn cash_history(state: State<'_, AppState>, filter: HistoryFilter, limit: u32) -> AppResult<SessionHistory> {
    let s = session(&state)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let all = s.perms.has(Perm::REPORT_SALES);
    cash::history(&state.db.pool, &filter, (&identity.device_id, &identity.device_code), all, limit).await
}

/// Whether the logged-in staff member may read this session: `REPORT_SALES` (any counter) or it is this
/// counter's own.
async fn can_view_session(state: &AppState, session_id: &str) -> AppResult<()> {
    let s = session(state)?;
    if s.perms.has(Perm::REPORT_SALES) {
        return Ok(());
    }
    let mut conn = state.db.pool.acquire().await?;
    let found = cash::get(&mut conn, session_id).await?;
    if found.device_id == device_id(state)? { Ok(()) } else { Err(AppError::Forbidden { perm: Perm::REPORT_SALES }) }
}

/// Z-report of a closed session, or X-report of the open one. Own counter, or any with `REPORT_SALES`.
#[tauri::command]
#[specta::specta]
pub async fn cash_session_report(state: State<'_, AppState>, session_id: String) -> AppResult<ZReport> {
    can_view_session(&state, &session_id).await?;
    let now = write_ctx(&state)?.now;
    let mut conn = state.db.pool.acquire().await?;
    cash::z_report(&mut conn, &session_id, &now).await
}

/// Print a closed session's Z-report again (`REPRINT_RECEIPT`; own counter, or any with `REPORT_SALES`).
#[tauri::command]
#[specta::specta]
pub async fn reprint_z_report(app: AppHandle, state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::REPRINT_RECEIPT, "z_report", None)?;
    can_view_session(&state, &session_id).await?;
    cash::reprint_z(&state.db, &write_ctx(&state)?, &actor, &session_id).await?;
    state.printer.notify();
    emit_changed(&app, &["audit_logs"]);
    Ok(())
}
