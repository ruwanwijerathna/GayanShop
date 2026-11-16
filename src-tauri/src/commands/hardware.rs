//! Receipt printer and drawer (doc §7.2–§7.5): the status behind the till banner, the startup recovery
//! prompt ("Print now / Skip"), Retry / Reprint later, and Settings → Printer with its test page.
//! Reading the status and acting on stuck receipts only needs a login; the profile needs `SETTINGS_MANAGE`.

use serde::Serialize;
use serde_json::json;
use specta::Type;
use tauri::{AppHandle, State};

use super::{emit_changed, session, write_ctx};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::{Actor, require};
use crate::db::repo::print_jobs::{self, PrintKind, RECEIPT_PRINTER};
use crate::db::settings;
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::hardware::escpos::{Logo, render};
use crate::hardware::printer::worker::PrinterHealth;
use crate::hardware::printer::{self, ReceiptPrinterProfile, transport_spooler};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct PrinterStatusView {
    pub health: PrinterHealth,
    /// `PAPER_OUT`, `COVER_OPEN`, `TIMEOUT`, `CONNECT_FAILED`, … for the banner.
    pub fault: Option<String>,
    /// Waiting to print (or printing).
    #[specta(type = specta_typescript::Number)]
    pub queued: i64,
    /// Failed and waiting for Retry / Reprint later.
    #[specta(type = specta_typescript::Number)]
    pub failed: i64,
    /// Unprinted jobs from before the restart, waiting for "Print now / Skip".
    #[specta(type = specta_typescript::Number)]
    pub recovered: i64,
}

fn changed(app: &AppHandle, state: &AppState) {
    state.printer.notify();
    emit_changed(app, &["print_jobs"]);
}

#[tauri::command]
#[specta::specta]
pub async fn printer_status(state: State<'_, AppState>) -> AppResult<PrinterStatusView> {
    session(&state)?;
    let h = state.printer.health()?;
    let held = state.printer.held()?;
    let (queued, failed) = print_jobs::counts(&state.db.pool, RECEIPT_PRINTER, &held).await?;
    let recovered = i64::try_from(held.len()).unwrap_or(i64::MAX);
    Ok(PrinterStatusView { health: h.health, fault: h.fault, queued, failed, recovered })
}

/// The printers Windows has installed (Settings → printer dropdown and its Refresh button). Enumeration
/// runs off the async thread under a timeout, because a frozen spooler can hang it.
#[tauri::command]
#[specta::specta]
pub async fn list_system_printers(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    session(&state)?;
    let listing = tokio::task::spawn_blocking(transport_spooler::list_printers);
    match tokio::time::timeout(std::time::Duration::from_secs(5), listing).await {
        Ok(Ok(Ok(names))) => Ok(names),
        Ok(Ok(Err(fault))) => Err(AppError::internal(format!("{}: {}", fault.code, fault.detail))),
        Ok(Err(e)) => Err(AppError::internal(format!("printer listing task: {e}"))),
        Err(_) => Err(AppError::internal("printer listing timed out")),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn printer_profile(state: State<'_, AppState>) -> AppResult<Option<ReceiptPrinterProfile>> {
    session(&state)?;
    printer::load_profile(&state.db.pool, RECEIPT_PRINTER).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_printer_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: ReceiptPrinterProfile,
) -> AppResult<()> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::SETTINGS_MANAGE, "printer", None)?;
    printer::save_profile(&state.db, &write_ctx(&state)?, &actor, RECEIPT_PRINTER, &profile).await?;
    emit_changed(&app, &["audit_logs"]);
    changed(&app, &state);
    Ok(())
}

/// Ruler, code-page sample, QR and cut, then the drawer kick (doc §7.2).
#[tauri::command]
#[specta::specta]
pub async fn print_test_page(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    require(&state.auth, state.clock.as_ref(), Perm::SETTINGS_MANAGE, "printer", None)?;
    let ctx = write_ctx(&state)?;
    let mut tx = state.db.write().await?;
    print_jobs::enqueue(tx.conn(), &ctx, PrintKind::Test, None, &serde_json::json!({})).await?;
    tx.commit().await?;
    changed(&app, &state);
    Ok(())
}

/// The banner's Retry: failed jobs go back to the queue.
#[tauri::command]
#[specta::specta]
pub async fn print_retry(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    session(&state)?;
    print_jobs::retry_failed(&state.db, &write_ctx(&state)?, RECEIPT_PRINTER).await?;
    changed(&app, &state);
    Ok(())
}

/// The banner's "Reprint later": failed jobs are dropped (abandoned, audited). The sales stand.
#[tauri::command]
#[specta::specta]
pub async fn print_dismiss_failed(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let s = session(&state)?;
    let ids = print_jobs::failed_ids(&state.db.pool, RECEIPT_PRINTER).await?;
    let actor = Actor { staff_id: s.staff_id, approved_by: None };
    print_jobs::abandon(&state.db, &write_ctx(&state)?, &actor, &ids, "reprint_later").await?;
    emit_changed(&app, &["audit_logs"]);
    changed(&app, &state);
    Ok(())
}

/// The startup prompt: print the jobs left from before the restart, or skip them (abandoned, audited).
#[tauri::command]
#[specta::specta]
pub async fn print_recovery_resolve(app: AppHandle, state: State<'_, AppState>, print_now: bool) -> AppResult<()> {
    let s = session(&state)?;
    let ids = state.printer.take_held()?;
    let ctx = write_ctx(&state)?;
    if print_now {
        print_jobs::requeue(&state.db, &ctx, &ids).await?;
    } else {
        let actor = Actor { staff_id: s.staff_id, approved_by: None };
        print_jobs::abandon(&state.db, &ctx, &actor, &ids, "skipped_after_restart").await?;
        emit_changed(&app, &["audit_logs"]);
    }
    changed(&app, &state);
    Ok(())
}

/// What the receipt preview draws: the header image, the store name and the footer as the printer prints them.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ReceiptHeaderView {
    pub logo: Option<Logo>,
    pub store_name: String,
    pub footer: String,
}

#[tauri::command]
#[specta::specta]
pub async fn receipt_header(state: State<'_, AppState>) -> AppResult<ReceiptHeaderView> {
    session(&state)?;
    let pool = &state.db.pool;
    Ok(ReceiptHeaderView {
        logo: render::load_logo(pool).await?,
        store_name: settings::store_get(pool, settings::keys::STORE_NAME).await?.unwrap_or_default(),
        footer: settings::store_get(pool, "receipt.footer").await?.unwrap_or_default(),
    })
}

/// Set (or, with `None`, remove) the receipt header image. Audited as a settings change; the log keeps the
/// image's size, not its bytes.
#[tauri::command]
#[specta::specta]
pub async fn set_receipt_logo(app: AppHandle, state: State<'_, AppState>, logo: Option<Logo>) -> AppResult<()> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::SETTINGS_MANAGE, "printer", None)?;
    if let Some(l) = &logo {
        l.validate()?;
    }
    let ctx = write_ctx(&state)?;
    let size = |l: &Option<Logo>| l.as_ref().map(|l| json!({ "width": l.width, "height": l.height }));
    let before = size(&render::load_logo(&state.db.pool).await?);
    let mut tx = state.db.write().await?;
    match &logo {
        Some(l) => settings::device_set(tx.conn(), render::LOGO_KEY, l).await?,
        None => settings::device_delete(tx.conn(), render::LOGO_KEY).await?,
    }
    audit::record(
        tx.conn(),
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::SETTINGS_CHANGE,
            staff_id: Some(&actor.staff_id),
            approved_by: None,
            entity_type: Some("device_setting"),
            entity_id: Some(render::LOGO_KEY),
            metadata: json!({ "before": before, "after": size(&logo) }),
        },
    )
    .await?;
    tx.commit().await?;
    emit_changed(&app, &["audit_logs"]);
    Ok(())
}
