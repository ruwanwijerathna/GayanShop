//! Process-wide application state managed by Tauri (`app.manage(AppState)`).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::auth::{Auth, DeviceCtx};
use crate::clock::Clock;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::hardware::printer::worker::PrintHub;
use crate::setup::Identity;
use crate::sync::credentials::SecretStore;

pub struct AppState {
    pub db: Db,
    pub clock: Arc<dyn Clock>,
    /// `%APPDATA%\com.gayanshop.pos` — holds `pos.db` and `backups\`.
    pub data_dir: PathBuf,
    pub auth: Auth,
    /// `None` until first-run setup has completed on this terminal.
    identity: RwLock<Option<Identity>>,
    /// The receipt printer's worker (doc §7.5); commands ping it after committing a print job.
    pub printer: PrintHub,
    /// The barcode label printer's worker (doc §7.4): its own queue, so labels never hold up receipts.
    pub label_printer: PrintHub,
    /// Where the device identity's refresh token lives (Windows Credential Manager; doc §1.3).
    pub secrets: Arc<dyn SecretStore>,
}

/// The long-lived services the app starts beside the database.
pub struct Services {
    pub printer: PrintHub,
    pub label_printer: PrintHub,
    pub secrets: Arc<dyn SecretStore>,
}

impl AppState {
    pub fn new(
        db: Db,
        clock: Arc<dyn Clock>,
        data_dir: PathBuf,
        auth: Auth,
        identity: Option<Identity>,
        services: Services,
    ) -> Self {
        let Services { printer, label_printer, secrets } = services;
        Self { db, clock, data_dir, auth, identity: RwLock::new(identity), printer, label_printer, secrets }
    }

    pub fn identity(&self) -> AppResult<Option<Identity>> {
        Ok(self.identity.read()?.clone())
    }

    pub fn set_identity(&self, identity: Identity) -> AppResult<()> {
        *self.identity.write()? = Some(identity);
        Ok(())
    }

    /// The device context for audited actions; `NotInitialized` before setup.
    pub fn device_ctx(&self) -> AppResult<DeviceCtx> {
        let identity = self.identity()?.ok_or(AppError::NotInitialized)?;
        Ok(DeviceCtx { store_id: identity.store_id, device_id: identity.device_id, calendar: identity.calendar })
    }
}
