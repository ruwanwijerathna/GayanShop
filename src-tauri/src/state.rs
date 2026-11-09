//! Process-wide application state managed by Tauri (`app.manage(AppState)`).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::auth::{Auth, DeviceCtx};
use crate::clock::Clock;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::hardware::printer::worker::PrintHub;
use crate::setup::Identity;

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
}

impl AppState {
    pub fn new(
        db: Db,
        clock: Arc<dyn Clock>,
        data_dir: PathBuf,
        auth: Auth,
        identity: Option<Identity>,
        printer: PrintHub,
    ) -> Self {
        Self { db, clock, data_dir, auth, identity: RwLock::new(identity), printer }
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
