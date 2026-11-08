//! GayanShop POS — Rust core. See docs/ARCHITECTURE.md and AGENT.md.

// AGENT.md §2: no panicking shortcuts in production code, no float arithmetic (money is i64 cents).
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]
#![deny(clippy::float_arithmetic)]

/// Builds and runs the Tauri application. Errors are returned to `main`, never panicked.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), tauri::Error> {
    tauri::Builder::default().run(tauri::generate_context!())
}
