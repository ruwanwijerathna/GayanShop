//! `#[tauri::command]` IPC surface. Commands stay thin: guard → transaction → repo → audit → outbox →
//! commit → emit (ARCHITECTURE §2). Every command and DTO is exported to TypeScript via tauri-specta.

// The expansion of `#[tauri::command]` on async commands taking `State` invokes the `unreachable`
// macro. That code is macro-generated; a hand-written call anywhere in src-tauri/src is still banned
// by `pnpm guard` (AGENT.md §2), so this allow cannot hide our own code.
#![allow(clippy::unreachable)]

pub mod audit;
pub mod auth;
pub mod prefs;
pub mod products;
pub mod setup;
pub mod system;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Payload of `db://changed`: the tables a committed write touched (the UI invalidates matching queries).
#[derive(Debug, Clone, Serialize)]
pub struct DbChanged {
    pub tables: Vec<&'static str>,
}

/// Emit `db://changed` after a commit. Best effort: the write is already durable, and the UI also
/// refreshes on its own mutation results, so a failed emit is only logged.
pub fn emit_changed(app: &AppHandle, tables: &[&'static str]) {
    if let Err(e) = app.emit("db://changed", DbChanged { tables: tables.to_vec() }) {
        log::warn!("db://changed emit failed: {e}");
    }
}
