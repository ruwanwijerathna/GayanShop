//! Settings → Sync (doc §1.3, §6): register this counter with the cloud (`SYNC_MANAGE`), what it is
//! registered with, the worker's status, "Sync now", dead ops and the sync log; and the first-run "Join
//! existing store" with its full-history bootstrap (decision #18). The owner's cloud password passes through once, to
//! the auth server, and is never kept.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter, State};

use super::{emit_changed, session, write_ctx};
use crate::auth::guard::require;
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::setup;
use crate::state::AppState;
use crate::sync::admin::{self, DeadOp, ResolveDeadOp, SyncLogRow};
use crate::sync::api::HttpApi;
use crate::sync::bootstrap::{self, BootstrapProgress};
use crate::sync::config;
use crate::sync::engine::PULL_PAGE;
use crate::sync::registration::{self, RegisterInput, Registered};
use crate::sync::worker::{self, SyncStatus};

#[derive(Debug, Clone, Serialize, Type)]
pub struct SyncInfo {
    /// A server and a device identity are configured.
    pub registered: bool,
    pub server_url: Option<String>,
    pub registered_at: Option<String>,
    pub device_code: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn sync_info(state: State<'_, AppState>) -> AppResult<SyncInfo> {
    session(&state)?;
    let identity = state.identity()?;
    Ok(SyncInfo {
        registered: registration::is_registered(&state.db, state.secrets.as_ref()).await?,
        server_url: config::load(&state.db.pool).await?.map(|c| c.url),
        registered_at: config::registered_at(&state.db.pool).await?,
        device_code: identity.map(|i| i.device_code),
    })
}

/// Register (or re-register) this counter, adopting its store, device id and code.
#[tauri::command]
#[specta::specta]
pub async fn sync_register(app: AppHandle, state: State<'_, AppState>, input: RegisterInput) -> AppResult<Registered> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::SYNC_MANAGE, "sync", None)?;
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let registered = registration::register_this_counter(
        &state.db,
        state.clock.as_ref(),
        state.secrets.as_ref(),
        &identity,
        &actor.staff_id,
        &input,
    )
    .await?;
    emit_changed(&app, &["audit_logs", "device_settings"]);
    Ok(registered)
}

/// The sync worker's latest status, with fresh outbox counts (the status pill reads it at start, then
/// follows `sync://status`).
#[tauri::command]
#[specta::specta]
pub async fn sync_status(state: State<'_, AppState>) -> AppResult<SyncStatus> {
    session(&state)?;
    let mut status = state.sync.status();
    let (pending, dead) = worker::outbox_counts(&state.db).await?;
    status.pending = pending;
    status.dead = dead;
    Ok(status)
}

/// "Sync now": run a round at once.
#[tauri::command]
#[specta::specta]
pub async fn sync_now(state: State<'_, AppState>) -> AppResult<()> {
    session(&state)?;
    state.sync.poke();
    Ok(())
}

/// What the first-run "Join existing store" screen sends.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct JoinInput {
    pub register: RegisterInput,
    /// `en` or `si`, this counter's default.
    pub ui_language: String,
}

/// First-run "Join existing store" (decision #18): register this new counter into the owner's store, then
/// pull the store's whole history. If the bootstrap is interrupted, the registration stands and
/// `sync_bootstrap` resumes it.
#[tauri::command]
#[specta::specta]
pub async fn setup_join_store(
    app: AppHandle,
    state: State<'_, AppState>,
    input: JoinInput,
) -> AppResult<BootstrapProgress> {
    registration::join_store(
        &state.db,
        state.clock.as_ref(),
        state.secrets.as_ref(),
        &input.register,
        &input.ui_language,
    )
    .await?;
    reload_identity(&state).await?;
    run_bootstrap(&app, &state).await
}

/// Resume an interrupted bootstrap. Allowed before anyone can log in, but only while one is pending.
#[tauri::command]
#[specta::specta]
pub async fn sync_bootstrap(app: AppHandle, state: State<'_, AppState>) -> AppResult<BootstrapProgress> {
    if !bootstrap::is_pending(&state.db).await? {
        return Err(AppError::validation("SYNC_NOT_PENDING", "this counter has already pulled its history"));
    }
    run_bootstrap(&app, &state).await
}

async fn reload_identity(state: &AppState) -> AppResult<()> {
    if let Some(identity) = setup::load_identity(&state.db.pool).await? {
        state.set_identity(identity)?;
    }
    Ok(())
}

/// Pull the store's history, reporting `sync://bootstrap` progress; then refresh the identity (store name,
/// calendar) and hand over to the worker.
async fn run_bootstrap(app: &AppHandle, state: &AppState) -> AppResult<BootstrapProgress> {
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let cfg = config::load(&state.db.pool).await?.ok_or(AppError::validation("SYNC_UNAUTHORIZED", "no server"))?;
    let api = HttpApi::new(cfg, &identity.device_id, state.secrets.clone())?;
    let events = app.clone();
    let report = move |p: &BootstrapProgress| {
        if let Err(e) = events.emit("sync://bootstrap", p) {
            log::warn!("sync://bootstrap emit failed: {e}");
        }
    };
    let done = bootstrap::run(&state.db, &api, state.clock.as_ref(), &identity.device_id, PULL_PAGE, &report).await?;
    reload_identity(state).await?;
    state.sync.poke();
    if let Err(e) = app.emit("db://changed", super::DbChanged { tables: ALL_TABLES.to_vec() }) {
        log::warn!("db://changed emit failed: {e}");
    }
    Ok(done)
}

/// Every table the UI shows, refreshed after a bootstrap.
const ALL_TABLES: &[&str] = &[
    "products",
    "categories",
    "audit_logs",
    "batches",
    "batch_items",
    "stock_movements",
    "cash_sessions",
    "cash_movements",
    "orders",
    "customers",
    "staff",
    "quick_sale_links",
    "expenses",
    "expense_categories",
];

/// The ops the cloud rejected (Settings → Sync).
#[tauri::command]
#[specta::specta]
pub async fn sync_dead_ops(state: State<'_, AppState>) -> AppResult<Vec<DeadOp>> {
    require(&state.auth, state.clock.as_ref(), Perm::SYNC_MANAGE, "sync", None)?;
    admin::dead_ops(&state.db).await
}

/// Retry (a fresh snapshot under a new op id) or discard a dead op; audited.
#[tauri::command]
#[specta::specta]
pub async fn sync_resolve_dead_op(app: AppHandle, state: State<'_, AppState>, input: ResolveDeadOp) -> AppResult<()> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::SYNC_MANAGE, "sync", None)?;
    admin::resolve(&state.db, &write_ctx(&state)?, &actor, &input).await?;
    emit_changed(&app, &["audit_logs"]);
    Ok(())
}

/// The latest sync log entries (pushes and pulls), newest first.
#[tauri::command]
#[specta::specta]
pub async fn sync_logs(state: State<'_, AppState>, limit: u32) -> AppResult<Vec<SyncLogRow>> {
    session(&state)?;
    admin::logs(&state.db, limit).await
}
