//! UI preference commands (thin wrappers over `crate::prefs`).

use tauri::State;

use crate::clock::iso;
use crate::error::AppResult;
use crate::prefs::{self, DevicePreferences, UiLanguage, UiTheme};
use crate::state::AppState;

/// Device-level UI preferences (local to this counter).
#[tauri::command]
#[specta::specta]
pub async fn device_preferences(state: State<'_, AppState>) -> AppResult<DevicePreferences> {
    prefs::device_preferences(&state.db.pool).await
}

/// Set this counter's theme (local-only; each counter's screen and lighting differ).
#[tauri::command]
#[specta::specta]
pub async fn set_device_theme(state: State<'_, AppState>, theme: UiTheme) -> AppResult<()> {
    prefs::set_device_theme(&state.db, theme).await
}

/// Turn this counter's touch screen mode on or off (local-only: on-screen keypads for touch hardware).
#[tauri::command]
#[specta::specta]
pub async fn set_device_touch_mode(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    prefs::set_device_touch_mode(&state.db, enabled).await
}

/// Save the UI language. Logged in ⇒ the staff member's own preference (synced, applied at their next
/// login on any counter); otherwise ⇒ this counter's default for the lock and login screens.
#[tauri::command]
#[specta::specta]
pub async fn set_ui_language(state: State<'_, AppState>, language: UiLanguage) -> AppResult<()> {
    let now = state.clock.now();
    match state.auth.sessions.active(now, state.auth.idle_lock) {
        Some(s) => {
            let dev = state.device_ctx()?;
            prefs::set_staff_language(&state.db, &s.staff_id, &dev.device_id, &iso(now), language).await
        }
        None => prefs::set_device_language(&state.db, language).await,
    }
}
