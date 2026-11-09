//! Expenses (doc §5.1, Block 2.6). Everything needs `EXPENSE_MANAGE` (Manager, Owner; not approvable). An
//! expense paid from the drawer, or the deletion of one, also moves cash, so it also needs `CASH_PAY_IN_OUT`
//! (approvable: a supervisor PIN can lend it, bound to the drawer like any pay-out).

use tauri::{AppHandle, State};

use super::cash::DRAWER_TARGET;
use super::{emit_changed, write_ctx};
use crate::auth::guard::{Actor, require};
use crate::db::repo::expenses::{self, ExpenseCategory, ExpenseCategoryInput, ExpenseInput, ExpenseList, ExpenseRow};
use crate::domain::period::Period;
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

fn manager(state: &AppState) -> AppResult<Actor> {
    require(&state.auth, state.clock.as_ref(), Perm::EXPENSE_MANAGE, "expenses", None)
}

/// `EXPENSE_MANAGE`, plus `CASH_PAY_IN_OUT` (own or lent) when the drawer is involved.
fn drawer_actor(state: &AppState, drawer: bool, approval_id: Option<&str>) -> AppResult<Actor> {
    let actor = manager(state)?;
    if !drawer {
        return Ok(actor);
    }
    let cash = require(&state.auth, state.clock.as_ref(), Perm::CASH_PAY_IN_OUT, DRAWER_TARGET, approval_id)?;
    Ok(Actor { staff_id: actor.staff_id, approved_by: cash.approved_by })
}

#[tauri::command]
#[specta::specta]
pub async fn expense_categories(state: State<'_, AppState>) -> AppResult<Vec<ExpenseCategory>> {
    manager(&state)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    expenses::categories(&state.db.pool, &identity.store_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_expense_category(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    input: ExpenseCategoryInput,
) -> AppResult<String> {
    let actor = manager(&state)?;
    let id = expenses::save_category(&state.db, &write_ctx(&state)?, &actor, id.as_deref(), &input).await?;
    emit_changed(&app, &["expense_categories", "audit_logs"]);
    Ok(id)
}

#[tauri::command]
#[specta::specta]
pub async fn expense_list(
    state: State<'_, AppState>,
    period: Period,
    category_id: Option<String>,
) -> AppResult<ExpenseList> {
    manager(&state)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    expenses::list(&state.db.pool, &identity.store_id, &identity.calendar, &period, category_id.as_deref()).await
}

#[tauri::command]
#[specta::specta]
pub async fn record_expense(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ExpenseInput,
    approval_id: Option<String>,
) -> AppResult<ExpenseRow> {
    let actor = drawer_actor(&state, input.paid_from_drawer, approval_id.as_deref())?;
    let row = expenses::create(&state.db, &write_ctx(&state)?, &actor, &input).await?;
    if input.paid_from_drawer {
        state.printer.notify();
        emit_changed(&app, &["expenses", "cash_movements", "cash_sessions", "audit_logs"]);
    } else {
        emit_changed(&app, &["expenses", "audit_logs"]);
    }
    Ok(row)
}

#[tauri::command]
#[specta::specta]
pub async fn update_expense(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: ExpenseInput,
) -> AppResult<ExpenseRow> {
    let actor = manager(&state)?;
    let row = expenses::update(&state.db, &write_ctx(&state)?, &actor, &id, &input).await?;
    emit_changed(&app, &["expenses", "audit_logs"]);
    Ok(row)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_expense(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    approval_id: Option<String>,
) -> AppResult<()> {
    manager(&state)?;
    let drawer = expenses::paid_from_drawer(&state.db.pool, &id).await?;
    let actor = drawer_actor(&state, drawer, approval_id.as_deref())?;
    expenses::delete(&state.db, &write_ctx(&state)?, &actor, &id).await?;
    if drawer {
        state.printer.notify();
        emit_changed(&app, &["expenses", "cash_movements", "cash_sessions", "audit_logs"]);
    } else {
        emit_changed(&app, &["expenses", "audit_logs"]);
    }
    Ok(())
}
