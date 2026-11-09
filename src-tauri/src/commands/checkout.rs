//! `checkout_order`: the till's one write path for a sale (`SALE_CREATE`). See `db::repo::checkout` for the
//! transaction. Supervisor step-up: a first attempt that needs authority fails with `ApprovalRequired`; the
//! UI gets a single-use approval bound to (perm, `checkout_token`, cashier) and retries with its id.

use tauri::{AppHandle, State};

use super::{emit_changed, write_ctx};
use crate::auth::Auth;
use crate::auth::guard::require;
use crate::clock::Clock;
use crate::db::Db;
use crate::db::repo::WriteCtx;
use crate::db::repo::checkout::{self, CheckoutInput, CheckoutResult, NoFaults, Seller};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const SALE_TABLES: [&str; 9] = [
    "orders",
    "order_items",
    "order_payments",
    "stock_movements",
    "customers",
    "cash_sessions",
    "products",
    "batches",
    "audit_logs",
];

/// Guard, then sell; on `ApprovalRequired` with an approval id in the input, consume that approval for the
/// required perm (bound to this checkout token and cashier) and sell again with the approver.
pub async fn sell(
    auth: &Auth,
    clock: &dyn Clock,
    db: &Db,
    ctx: &WriteCtx,
    input: &CheckoutInput,
) -> AppResult<CheckoutResult> {
    require(auth, clock, Perm::SALE_CREATE, "sale", None)?;
    let session = auth.sessions.touch(clock.now(), auth.idle_lock)?;
    let seller = Seller {
        staff_id: session.staff_id,
        perms: session.perms,
        max_discount_bp: session.max_discount_bp,
        max_override_bp: session.max_override_bp,
    };
    match checkout::checkout(db, ctx, &seller, input, None, &NoFaults).await {
        Err(AppError::ApprovalRequired { perm }) => {
            let Some(approval_id) = input.approval_id.as_deref() else {
                return Err(AppError::ApprovalRequired { perm });
            };
            let approver = auth
                .approvals
                .consume(approval_id, perm, input.checkout_token.trim(), &seller.staff_id, clock.now())
                .ok_or(AppError::InvalidApproval)?;
            checkout::checkout(db, ctx, &seller, input, Some(&approver), &NoFaults).await
        }
        other => other,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn checkout_order(
    app: AppHandle,
    state: State<'_, AppState>,
    input: CheckoutInput,
) -> AppResult<CheckoutResult> {
    let ctx = write_ctx(&state)?;
    let result = sell(&state.auth, state.clock.as_ref(), &state.db, &ctx, &input).await?;
    if !result.replayed {
        state.printer.notify();
        emit_changed(&app, &SALE_TABLES);
    }
    Ok(result)
}
