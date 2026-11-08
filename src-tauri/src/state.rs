//! Process-wide application state managed by Tauri (`app.manage(AppState)`).
//! Session, approvals and device identity join in Blocks 0.6–0.7.

use std::path::PathBuf;
use std::sync::Arc;

use crate::clock::Clock;
use crate::db::Db;

pub struct AppState {
    pub db: Db,
    pub clock: Arc<dyn Clock>,
    /// `%APPDATA%\com.gayanshop.pos` — holds `pos.db` and `backups\`.
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn new(db: Db, clock: Arc<dyn Clock>, data_dir: PathBuf) -> Self {
        Self { db, clock, data_dir }
    }
}
