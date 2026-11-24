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
use crate::db::repo::exchange::{self, BillSuggestion, ExchangeSource};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const SALE_TABLES: [&str; 10] = [
    "orders",
    "order_items",
    "order_payments",
    "order_returns",
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

/// Exchange step 1 (`EXCHANGE_PROCESS`): find the sale by its receipt number and report what can still come
/// back, and whether the 7-day window has passed (a late exchange needs a supervisor or owner PIN at checkout).
#[tauri::command]
#[specta::specta]
pub async fn exchange_lookup(state: State<'_, AppState>, receipt_no: String) -> AppResult<ExchangeSource> {
    require(&state.auth, state.clock.as_ref(), Perm::EXCHANGE_PROCESS, "exchange", None)?;
    let ctx = write_ctx(&state)?;
    exchange::lookup(&state.db.pool, &ctx.store_id, &receipt_no, &ctx.business_date).await
}

/// Bills whose number contains what the cashier has typed so far, offered while they type in the exchange dialog.
#[tauri::command]
#[specta::specta]
pub async fn exchange_suggest(state: State<'_, AppState>, typed: String) -> AppResult<Vec<BillSuggestion>> {
    require(&state.auth, state.clock.as_ref(), Perm::EXCHANGE_PROCESS, "exchange", None)?;
    let ctx = write_ctx(&state)?;
    exchange::suggest(&state.db.pool, &ctx.store_id, &typed).await
}
