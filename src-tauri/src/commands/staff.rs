//! Staff management (doc §3.5). Everything here needs `USER_MANAGE` (Owner by default; not approvable).

use tauri::{AppHandle, State};

use super::{emit_changed, write_ctx};
use crate::auth::guard::{Actor, require};
use crate::db::repo::staff::{self, NewStaff, Role, StaffInput, StaffMember};
use crate::db::repo::staff_sales::{self, StaffSales};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const STAFF_TABLES: [&str; 2] = ["staff", "audit_logs"];

fn manager(state: &AppState) -> AppResult<Actor> {
    require(&state.auth, state.clock.as_ref(), Perm::USER_MANAGE, "staff", None)
}

#[tauri::command]
#[specta::specta]
pub async fn list_staff(state: State<'_, AppState>) -> AppResult<Vec<StaffMember>> {
    manager(&state)?;
    staff::list(&state.db.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_roles(state: State<'_, AppState>) -> AppResult<Vec<Role>> {
    manager(&state)?;
    staff::roles(&state.db.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_staff(app: AppHandle, state: State<'_, AppState>, input: NewStaff) -> AppResult<StaffMember> {
    let actor = manager(&state)?;
    let m = staff::create(&state.db, &write_ctx(&state)?, &actor, &state.auth.hasher, &input).await?;
    emit_changed(&app, &STAFF_TABLES);
    Ok(m)
}

#[tauri::command]
#[specta::specta]
pub async fn update_staff(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: StaffInput,
) -> AppResult<StaffMember> {
    let actor = manager(&state)?;
    let m = staff::update(&state.db, &write_ctx(&state)?, &actor, &id, &input).await?;
    emit_changed(&app, &STAFF_TABLES);
    Ok(m)
}

#[tauri::command]
#[specta::specta]
pub async fn reset_staff_pin(app: AppHandle, state: State<'_, AppState>, id: String, pin: String) -> AppResult<()> {
    let actor = manager(&state)?;
    staff::reset_pin(&state.db, &write_ctx(&state)?, &actor, &state.auth.hasher, &id, &pin).await?;
    emit_changed(&app, &STAFF_TABLES);
    Ok(())
}

/// One person's daily sales for the chart plus their week, month and year totals (Staff → ⋮ → Sales performance).
#[tauri::command]
#[specta::specta]
pub async fn staff_sales(state: State<'_, AppState>, id: String) -> AppResult<StaffSales> {
    manager(&state)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let today = identity.calendar.business_date(state.clock.now());
    staff_sales::sales(&state.db.pool, &identity.calendar, today, &id).await
}
