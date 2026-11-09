//! CSV export (doc §5: `DATA_EXPORT`, audited). Each kind also needs the permission that shows the same
//! data on screen: orders, order lines and sales by product `REPORT_SALES`; customers `CUSTOMER_VIEW`;
//! expenses `EXPENSE_MANAGE`; stock movements `REPORT_FINANCIAL`. Cost columns need `VIEW_COST`.

use std::path::PathBuf;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use super::{emit_changed, session, write_ctx};
use crate::auth::guard::{Actor, require};
use crate::domain::period::Period;
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::export::{self, ExportKind, ExportOutcome, ExportRequest};
use crate::state::AppState;

fn exporter(state: &AppState, kind: Option<ExportKind>) -> AppResult<(Actor, bool)> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::DATA_EXPORT, "export", None)?;
    let also = match kind {
        Some(ExportKind::Orders | ExportKind::OrderLines | ExportKind::SalesByProduct) => Some(Perm::REPORT_SALES),
        Some(ExportKind::Customers) => Some(Perm::CUSTOMER_VIEW),
        Some(ExportKind::Expenses) => Some(Perm::EXPENSE_MANAGE),
        Some(ExportKind::StockMovements) => Some(Perm::REPORT_FINANCIAL),
        Some(ExportKind::Products) | None => None,
    };
    if let Some(perm) = also {
        require(&state.auth, state.clock.as_ref(), perm, "export", None)?;
    }
    Ok((actor, session(state)?.perms.has(Perm::VIEW_COST)))
}

/// The native folder picker; `None` when the user cancels.
#[tauri::command]
#[specta::specta]
pub async fn export_pick_folder(app: AppHandle, state: State<'_, AppState>) -> AppResult<Option<String>> {
    exporter(&state, None)?;
    let (tx, rx) = oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder.and_then(|f| f.into_path().ok()));
    });
    let picked = rx.await.map_err(|_| AppError::internal("folder picker closed"))?;
    Ok(picked.map(|p| p.display().to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn export_csv(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: ExportKind,
    period: Period,
    dest_dir: String,
) -> AppResult<ExportOutcome> {
    let (actor, with_cost) = exporter(&state, Some(kind))?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let dir = PathBuf::from(dest_dir);
    let req = ExportRequest { kind, period: &period, with_cost, dir: &dir, at: state.clock.now() };
    let out = export::export(&state.db, &write_ctx(&state)?, &actor, &identity.calendar, &req).await?;
    emit_changed(&app, &["audit_logs"]);
    Ok(out)
}
