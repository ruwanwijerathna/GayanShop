//! `#[tauri::command]` IPC surface. Commands stay thin: guard → transaction → repo → audit → outbox →
//! commit → emit (ARCHITECTURE §2). Every command and DTO is exported to TypeScript via tauri-specta.

// The expansion of `#[tauri::command]` on async commands taking `State` invokes the `unreachable`
// macro. That code is macro-generated; a hand-written call anywhere in src-tauri/src is still banned
// by `pnpm guard` (AGENT.md §2), so this allow cannot hide our own code.
#![allow(clippy::unreachable)]

pub mod audit;
pub mod auth;
pub mod batches;
pub mod cash;
pub mod checkout;
pub mod customers;
pub mod prefs;
pub mod products;
pub mod setup;
pub mod system;
pub mod void;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::auth::session::Session;
use crate::clock::iso;
use crate::db::repo::WriteCtx;
use crate::domain::business_date::format_date;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// The logged-in session (counts as activity for the idle auto-lock). Reads that only need a login use this.
pub(crate) fn session(state: &AppState) -> AppResult<Session> {
    state.auth.sessions.touch(state.clock.now(), state.auth.idle_lock)
}

pub(crate) fn store_id(state: &AppState) -> AppResult<String> {
    Ok(state.identity()?.ok_or(AppError::NotInitialized)?.store_id)
}

/// Store, device, time and business date for a write happening now on this counter.
pub(crate) fn write_ctx(state: &AppState) -> AppResult<WriteCtx> {
    let identity = state.identity()?.ok_or(AppError::NotInitialized)?;
    let now = state.clock.now();
    Ok(WriteCtx {
        business_date: format_date(identity.calendar.business_date(now)),
        now: iso(now),
        store_id: identity.store_id,
        device_id: identity.device_id,
        device_code: identity.device_code,
    })
}

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
