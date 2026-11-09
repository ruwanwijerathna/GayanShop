//! Reports (doc §5). Everything needs `REPORT_SALES`. Costs, profit, write-offs and expenses also need
//! `REPORT_FINANCIAL` and `VIEW_COST` (doc §5 "Report permissions"); without them those fields come back
//! empty. `rebuild_derived` (Settings → Maintenance) needs `SETTINGS_MANAGE`.

use tauri::{AppHandle, State};

use super::{emit_changed, session, write_ctx};
use crate::auth::guard::require;
use crate::db::repo::reports::{
    self, PeriodReport, RebuildReport, ReportDevice, SalesByRow, SalesDimension, TrendPoint, VoidReport,
};
use crate::domain::period::{Granularity, Period};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// `REPORT_SALES`, and whether the viewer may see costs.
fn viewer(state: &AppState) -> AppResult<bool> {
    require(&state.auth, state.clock.as_ref(), Perm::REPORT_SALES, "reports", None)?;
    let perms = session(state)?.perms;
    Ok(perms.has(Perm::REPORT_FINANCIAL) && perms.has(Perm::VIEW_COST))
}

#[tauri::command]
#[specta::specta]
pub async fn report_summary(
    state: State<'_, AppState>,
    period: Period,
    device_id: Option<String>,
    compare: bool,
) -> AppResult<PeriodReport> {
    let costs = viewer(&state)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let r = reports::report(&state.db.pool, &identity.calendar, &period, device_id.as_deref(), compare).await?;
    Ok(if costs {
        r
    } else {
        PeriodReport { current: r.current.without_costs(), previous: r.previous.map(|p| p.without_costs()) }
    })
}

#[tauri::command]
#[specta::specta]
pub async fn report_trend(
    state: State<'_, AppState>,
    period: Period,
    device_id: Option<String>,
    granularity: Granularity,
) -> AppResult<Vec<TrendPoint>> {
    let costs = viewer(&state)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let mut rows =
        reports::trend(&state.db.pool, &identity.calendar, &period, device_id.as_deref(), granularity).await?;
    if !costs {
        for r in &mut rows {
            r.sales_cost = None;
            r.expenses = None;
        }
    }
    Ok(rows)
}

#[tauri::command]
#[specta::specta]
pub async fn report_sales_by(
    state: State<'_, AppState>,
    period: Period,
    device_id: Option<String>,
    dimension: SalesDimension,
) -> AppResult<Vec<SalesByRow>> {
    let costs = viewer(&state)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let mut rows =
        reports::sales_by(&state.db.pool, &identity.calendar, &period, device_id.as_deref(), dimension).await?;
    if !costs {
        for r in &mut rows {
            r.cost = None;
        }
    }
    Ok(rows)
}

#[tauri::command]
#[specta::specta]
pub async fn report_voids(
    state: State<'_, AppState>,
    period: Period,
    device_id: Option<String>,
) -> AppResult<VoidReport> {
    viewer(&state)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    reports::voids(&state.db.pool, &identity.calendar, &period, device_id.as_deref()).await
}

#[tauri::command]
#[specta::specta]
pub async fn report_devices(state: State<'_, AppState>) -> AppResult<Vec<ReportDevice>> {
    viewer(&state)?;
    reports::devices(&state.db.pool).await
}

/// Settings → Maintenance: recompute every derived table from the base tables.
#[tauri::command]
#[specta::specta]
pub async fn rebuild_derived(app: AppHandle, state: State<'_, AppState>) -> AppResult<RebuildReport> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::SETTINGS_MANAGE, "maintenance", None)?;
    let r = reports::rebuild_derived(&state.db, &write_ctx(&state)?, &actor).await?;
    emit_changed(&app, &["orders", "products", "stock_movements", "customers", "quick_sale_links", "audit_logs"]);
    Ok(r)
}
