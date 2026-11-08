//! `#[tauri::command]` IPC surface. Commands stay thin: guard → transaction → repo → audit → outbox →
//! commit → emit (ARCHITECTURE §2). Every command and DTO is exported to TypeScript via tauri-specta.

// The expansion of `#[tauri::command]` on async commands taking `State` invokes the `unreachable`
// macro. That code is macro-generated; a hand-written call anywhere in src-tauri/src is still banned
// by `pnpm guard` (AGENT.md §2), so this allow cannot hide our own code.
#![allow(clippy::unreachable)]

pub mod audit;
pub mod auth;
pub mod prefs;
pub mod setup;
pub mod system;
