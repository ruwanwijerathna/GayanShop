//! Support bundle IPC commands (doc §8 Phase 4: `BACKUP_CREATE` or `USER_MANAGE`).
//! Generates diagnostic bundles (logs, sync logs, integrity check, audit chain) and exports zip archives.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use super::session;
use crate::auth::guard::require;
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::support::{self, SupportBundle, SupportBundleExportResult};

fn support_actor(state: &AppState) -> AppResult<()> {
    let sess = session(state)?;
    if sess.role_name == "Owner"
        || sess.perms.has(Perm::BACKUP_CREATE)
        || sess.perms.has(Perm::USER_MANAGE)
        || sess.perms.has(Perm::REPORT_FINANCIAL)
    {
        Ok(())
    } else {
        require(&state.auth, state.clock.as_ref(), Perm::BACKUP_CREATE, "support", None)?;
        Ok(())
    }
}

fn log_directory(app: &AppHandle) -> PathBuf {
    app.path().app_log_dir().unwrap_or_else(|_| PathBuf::from("logs"))
}

#[tauri::command]
#[specta::specta]
pub async fn support_diagnostics(app: AppHandle, state: State<'_, AppState>) -> AppResult<SupportBundle> {
    support_actor(&state)?;
    let log_dir = log_directory(&app);
    let identity = state.identity().ok().flatten();
    let id_tuple = identity.as_ref().map(|i| (i.store_id.as_str(), i.device_id.as_str(), i.device_code.as_str()));
    let version = app.package_info().version.to_string();

    support::build_support_bundle(&state.db.pool, &log_dir, &version, id_tuple).await
}

#[tauri::command]
#[specta::specta]
pub async fn support_bundle_pick_folder(app: AppHandle, state: State<'_, AppState>) -> AppResult<Option<String>> {
    support_actor(&state)?;
    let (tx, rx) = oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder.and_then(|f| f.into_path().ok()));
    });
    let picked = rx.await.map_err(|_| AppError::internal("folder picker closed"))?;
    Ok(picked.map(|p| p.display().to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn support_bundle_export(
    app: AppHandle,
    state: State<'_, AppState>,
    destination_folder: Option<String>,
) -> AppResult<SupportBundleExportResult> {
    support_actor(&state)?;
    let log_dir = log_directory(&app);
    let identity = state.identity().ok().flatten();
    let id_tuple = identity.as_ref().map(|i| (i.store_id.as_str(), i.device_id.as_str(), i.device_code.as_str()));
    let version = app.package_info().version.to_string();

    let target_dir = match destination_folder {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => app.path().app_data_dir()?.join("support"),
    };

    let filename = format!("gayanshop-support-{}.zip", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    let dest_path = target_dir.join(filename);

    support::export_support_bundle_zip(&state.db.pool, &log_dir, &dest_path, &version, id_tuple).await
}
