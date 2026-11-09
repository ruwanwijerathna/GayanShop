//! Barcode labels (doc §7.4): templates and the label printer (Settings → Labels, `SETTINGS_MANAGE`), the
//! WYSIWYG preview, and printing (`LABEL_PRINT`, audited once per job). Printing goes through the label
//! printer's own print worker, so a jammed label printer never touches the till.

use serde_json::json;
use tauri::{AppHandle, State};

use super::{emit_changed, store_id, write_ctx};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::require;
use crate::db::repo::print_jobs::{self, PrintKind};
use crate::db::settings;
use crate::domain::permissions::Perm;
use crate::error::AppResult;
use crate::hardware::label::layout::{LabelPreview, LabelTemplate};
use crate::hardware::label::{self, LABEL_PRINTER, LabelJob, LabelPrinterProfile, TemplateCalibration};
use crate::hardware::printer::profile_key;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn label_templates(state: State<'_, AppState>) -> AppResult<Vec<LabelTemplate>> {
    super::session(&state)?;
    label::templates(&state.db.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn label_printer_profile(state: State<'_, AppState>) -> AppResult<Option<LabelPrinterProfile>> {
    super::session(&state)?;
    label::load_profile(&state.db.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_label_printer_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: LabelPrinterProfile,
) -> AppResult<()> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::SETTINGS_MANAGE, "labels", None)?;
    profile.validate()?;
    let ctx = write_ctx(&state)?;
    let key = profile_key(LABEL_PRINTER);
    let before = label::load_profile(&state.db.pool).await?;
    let mut tx = state.db.write().await?;
    settings::device_set(tx.conn(), &key, &profile).await?;
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
            entity_id: Some(&key),
            metadata: json!({ "before": before, "after": profile }),
        },
    )
    .await?;
    tx.commit().await?;
    state.label_printer.notify();
    emit_changed(&app, &["audit_logs", "print_jobs"]);
    Ok(())
}

/// Correct a template's column gap, row gap and left margin for this counter's roll; `None` = reset to default.
#[tauri::command]
#[specta::specta]
pub async fn calibrate_label_template(
    state: State<'_, AppState>,
    id: String,
    calibration: Option<TemplateCalibration>,
) -> AppResult<LabelTemplate> {
    require(&state.auth, state.clock.as_ref(), Perm::SETTINGS_MANAGE, "labels", None)?;
    let mut tx = state.db.write().await?;
    let t = label::calibrate(tx.conn(), &state.db.pool, &id, calibration.as_ref()).await?;
    tx.commit().await?;
    Ok(t)
}

/// The Rust-computed layout the UI draws as SVG: exactly what will print, with warnings.
#[tauri::command]
#[specta::specta]
pub async fn label_preview(state: State<'_, AppState>, job: LabelJob) -> AppResult<LabelPreview> {
    // Seeing the layout needs only a login; printing it needs LABEL_PRINT (approvable).
    super::session(&state)?;
    label::preview(&state.db.pool, &store_id(&state)?, &job).await
}

/// Queue a label job for the label printer (audited `LABEL_PRINT`: template, products, labels).
#[tauri::command]
#[specta::specta]
pub async fn print_labels(
    app: AppHandle,
    state: State<'_, AppState>,
    job: LabelJob,
    approval_id: Option<String>,
) -> AppResult<LabelPreview> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::LABEL_PRINT, "labels", approval_id.as_deref())?;
    let ctx = write_ctx(&state)?;
    let preview = label::preview(&state.db.pool, &ctx.store_id, &job).await?;
    let labels = job.validate()?;
    let mut payload = serde_json::to_value(&job)?;
    payload["store_id"] = json!(ctx.store_id);
    let mut tx = state.db.write().await?;
    let conn = tx.conn();
    let job_id = print_jobs::enqueue_for(conn, &ctx, LABEL_PRINTER, PrintKind::Labels, None, &payload).await?;
    audit::record(
        conn,
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::LABEL_PRINT,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("print_job"),
            entity_id: Some(&job_id),
            metadata: json!({ "template": job.template_id, "products": job.items.len(), "labels": labels,
                              "warnings": preview.warnings.len() }),
        },
    )
    .await?;
    tx.commit().await?;
    state.label_printer.notify();
    emit_changed(&app, &["print_jobs", "audit_logs"]);
    Ok(preview)
}

/// Settings → Labels "Alignment test": every cell's outline, for calibrating gaps and margins.
#[tauri::command]
#[specta::specta]
pub async fn label_alignment_test(app: AppHandle, state: State<'_, AppState>, template_id: String) -> AppResult<()> {
    require(&state.auth, state.clock.as_ref(), Perm::SETTINGS_MANAGE, "labels", None)?;
    label::template(&state.db.pool, &template_id).await?;
    let ctx = write_ctx(&state)?;
    let mut tx = state.db.write().await?;
    print_jobs::enqueue_for(
        tx.conn(),
        &ctx,
        LABEL_PRINTER,
        PrintKind::Test,
        None,
        &json!({ "template_id": template_id }),
    )
    .await?;
    tx.commit().await?;
    state.label_printer.notify();
    emit_changed(&app, &["print_jobs"]);
    Ok(())
}
