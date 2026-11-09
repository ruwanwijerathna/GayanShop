//! Settings → Backup (doc §3.9): "Backup Database Now" to a folder the user picks (a USB drive), needing
//! `BACKUP_CREATE` (not approvable). The snapshot runs on a pooled read connection, so selling continues.

use std::path::PathBuf;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use super::{emit_changed, session, write_ctx};
use crate::auth::guard::require;
use crate::backup::{self, BackupOutcome, BackupStatus, NoFaults};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn backup_status(state: State<'_, AppState>) -> AppResult<BackupStatus> {
    session(&state)?;
    backup::status(&state.db, state.clock.now()).await
}

/// The native folder picker, opened at the last-used folder. `None` when the user cancels.
#[tauri::command]
#[specta::specta]
pub async fn backup_pick_folder(app: AppHandle, state: State<'_, AppState>) -> AppResult<Option<String>> {
    require(&state.auth, state.clock.as_ref(), Perm::BACKUP_CREATE, "backup", None)?;
    let last = backup::status(&state.db, state.clock.now()).await?.last_dir;
    let (tx, rx) = oneshot::channel();
    let mut picker = app.dialog().file();
    if let Some(dir) = last.map(PathBuf::from).filter(|d| d.is_dir()) {
        picker = picker.set_directory(dir);
    }
    picker.pick_folder(move |folder| {
        let _ = tx.send(folder.and_then(|f| f.into_path().ok()));
    });
    let picked = rx.await.map_err(|_| AppError::internal("folder picker closed"))?;
    Ok(picked.map(|p| p.display().to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn backup_now(app: AppHandle, state: State<'_, AppState>, dest_dir: String) -> AppResult<BackupOutcome> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::BACKUP_CREATE, "backup", None)?;
    let ctx = write_ctx(&state)?;
    let tz = state.identity()?.ok_or(AppError::NotInitialized)?.calendar.timezone;
    let name = backup::file_name(&ctx.device_code, state.clock.now(), tz);
    let version = app.package_info().version.to_string();
    let result =
        backup::backup_now(&state.db, &ctx, &actor, &version, &PathBuf::from(dest_dir), &name, &NoFaults).await;
    emit_changed(&app, &["audit_logs"]);
    result
}
