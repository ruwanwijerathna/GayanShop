//! Settings → Backup → Restore, and the recovery screen (doc §8 Phase 4, Block 4.1). Staging a restore is
//! Owner-only (`BACKUP_RESTORE`, not approvable). Recovery runs before anyone logs in, authorised by the
//! Owner's PIN, because nothing may be written under this counter's identity until it completes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use super::write_ctx;
use crate::auth::guard::require;
use crate::backup::{self, NoFaults};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use crate::restore::{self, Inspected, RecoveryMode, RecoveryOutcome, RestorePending, StageInput};
use crate::setup;
use crate::state::AppState;
use crate::sync::api::HttpApi;
use crate::sync::credentials::refresh_token_account;
use crate::sync::{config, engine::PULL_PAGE};

/// A backup this counter could be restored from.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct RestoreCandidate {
    pub path: String,
    pub taken_at: String,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: i64,
    /// `auto` (hourly), `manual` (the last "Backup Database Now") or `pre_restore`.
    pub kind: String,
}

fn candidate(path: PathBuf, kind: &str, tz: chrono_tz::Tz) -> Option<RestoreCandidate> {
    let meta = std::fs::metadata(&path).ok()?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    let taken_at = backup::auto_taken_at(&name, tz)
        .map(crate::clock::iso)
        .or_else(|| meta.modified().ok().map(|t| crate::clock::iso(chrono::DateTime::<chrono::Utc>::from(t))))?;
    Some(RestoreCandidate {
        path: path.display().to_string(),
        taken_at,
        size_bytes: i64::try_from(meta.len()).unwrap_or(i64::MAX),
        kind: kind.to_owned(),
    })
}

/// The automatic backups on this counter and the last manual one, newest first.
#[tauri::command]
#[specta::specta]
pub async fn restore_candidates(state: State<'_, AppState>) -> AppResult<Vec<RestoreCandidate>> {
    require(&state.auth, state.clock.as_ref(), Perm::BACKUP_RESTORE, "backup", None)?;
    let tz = state.identity()?.ok_or(AppError::NotInitialized)?.calendar.timezone;
    let mut out = Vec::new();
    for (dir, kind) in [("auto", "auto"), ("pre-restore", "pre_restore")] {
        if let Ok(entries) = std::fs::read_dir(state.data_dir.join("backups").join(dir)) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "db") {
                    out.extend(candidate(p, kind, tz));
                }
            }
        }
    }
    let last_manual: Option<String> = crate::db::settings::device_get(&state.db.pool, backup::LAST_PATH).await?;
    if let Some(p) = last_manual {
        out.extend(candidate(PathBuf::from(p), "manual", tz));
    }
    out.sort_by(|a, b| b.taken_at.cmp(&a.taken_at));
    Ok(out)
}

/// The native file picker for a backup file; `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn restore_pick_file(app: AppHandle, state: State<'_, AppState>) -> AppResult<Option<String>> {
    if state.identity()?.is_some() {
        require(&state.auth, state.clock.as_ref(), Perm::BACKUP_RESTORE, "backup", None)?;
    }
    let (tx, rx) = oneshot::channel();
    app.dialog().file().add_filter("GayanShop backup", &["db"]).pick_file(move |file| {
        let _ = tx.send(file.and_then(|f| f.into_path().ok()));
    });
    let picked = rx.await.map_err(|_| AppError::internal("file picker closed"))?;
    Ok(picked.map(|p| p.display().to_string()))
}

/// Restart shortly after answering, so the UI sees the result first.
fn restart_soon(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        app.request_restart();
    });
}

/// Check the file, back up the current database, stage the restore and restart.
#[tauri::command]
#[specta::specta]
pub async fn restore_stage(app: AppHandle, state: State<'_, AppState>, path: String) -> AppResult<Inspected> {
    let actor = require(&state.auth, state.clock.as_ref(), Perm::BACKUP_RESTORE, "backup", None)?;
    let ctx = write_ctx(&state)?;
    let tz = state.identity()?.ok_or(AppError::NotInitialized)?.calendar.timezone;
    let version = app.package_info().version.to_string();
    let input = StageInput { data_dir: &state.data_dir, source: &PathBuf::from(path), app_version: &version, tz };
    let checked = restore::stage(&state.db, &ctx, &actor, &input, &NoFaults).await?;
    restart_soon(&app);
    Ok(checked)
}

/// First run on a new PC (or a reinstall): restore this counter from its backup file instead of setting up.
#[tauri::command]
#[specta::specta]
pub async fn restore_stage_fresh(app: AppHandle, state: State<'_, AppState>, path: String) -> AppResult<Inspected> {
    let now = crate::clock::iso(state.clock.now());
    let checked = restore::stage_fresh(&state.db, &state.data_dir, &PathBuf::from(path), &now).await?;
    restart_soon(&app);
    Ok(checked)
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct RestoreStatus {
    pub pending: Option<RestorePending>,
    /// The recovery paths open to this counter: online and offline once registered (online needs its
    /// sign-in token, which a new PC doesn't have), local for a store that never used the cloud.
    pub modes: Vec<RecoveryMode>,
    pub device_code: Option<String>,
}

async fn modes(state: &AppState) -> AppResult<Vec<RecoveryMode>> {
    let Some(identity) = state.identity()? else { return Ok(vec![]) };
    if config::load(&state.db.pool).await?.is_none() {
        return Ok(vec![RecoveryMode::Local]);
    }
    let token = state.secrets.get(&refresh_token_account(&identity.device_id))?;
    Ok(if token.is_some() { vec![RecoveryMode::Online, RecoveryMode::Offline] } else { vec![RecoveryMode::Offline] })
}

/// Whether a restore awaits recovery (asked before login).
#[tauri::command]
#[specta::specta]
pub async fn restore_status(state: State<'_, AppState>) -> AppResult<RestoreStatus> {
    let pending = restore::pending(&state.db.pool).await?;
    let modes = if pending.is_some() { modes(&state).await? } else { vec![] };
    Ok(RestoreStatus { pending, modes, device_code: state.identity()?.map(|i| i.device_code) })
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct RecoverInput {
    pub mode: RecoveryMode,
    pub staff_id: String,
    pub pin: String,
}

/// Finish a restore. Authorised by the Owner's PIN; nothing is audited until the recovery's own last step.
#[tauri::command]
#[specta::specta]
pub async fn restore_recover(
    app: AppHandle,
    state: State<'_, AppState>,
    input: RecoverInput,
) -> AppResult<RecoveryOutcome> {
    restore::verify_owner(&state.db, &state.auth.hasher, state.clock.as_ref(), &input.staff_id, &input.pin).await?;
    if !modes(&state).await?.contains(&input.mode) {
        return Err(AppError::validation("RESTORE_MODE_UNAVAILABLE", format!("{:?}", input.mode)));
    }
    let clock = state.clock.as_ref();
    let outcome = match input.mode {
        RecoveryMode::Online => {
            let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
            let cfg = config::load(&state.db.pool).await?.ok_or(AppError::NotInitialized)?;
            let api = HttpApi::new(cfg, &identity.device_id, state.secrets.clone())?;
            restore::recover_online(&state.db, &api, clock, &input.staff_id, PULL_PAGE).await?
        }
        RecoveryMode::Offline => restore::recover_offline(&state.db, clock, &input.staff_id).await?,
        RecoveryMode::Local => restore::recover_local(&state.db, clock, &input.staff_id).await?,
    };
    if let Some(identity) = setup::load_identity(&state.db.pool).await? {
        state.set_identity(identity)?;
    }
    state.sync.poke();
    if let Err(e) = app.emit("db://changed", super::DbChanged { tables: vec!["audit_logs", "orders", "products"] }) {
        log::warn!("db://changed emit failed: {e}");
    }
    Ok(outcome)
}
