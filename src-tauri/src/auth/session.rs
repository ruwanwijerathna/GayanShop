//! The logged-in staff session for this terminal, with idle auto-lock (ARCHITECTURE §3.5).

use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use specta::Type;

use crate::domain::permissions::{Perm, PermMask};
use crate::error::{AppError, AppResult};

pub const DEFAULT_IDLE_LOCK_MINUTES: i64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub staff_id: String,
    pub display_name: String,
    pub role_name: String,
    pub perms: PermMask,
    pub max_discount_bp: i64,
    pub max_override_bp: i64,
    pub ui_language: String,
    pub logged_in_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub staff_no: Option<i64>,
}

/// What the UI receives — never the PIN hash; permissions as keys, not a mask.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct SessionInfo {
    pub staff_id: String,
    pub display_name: String,
    pub role_name: String,
    pub permissions: Vec<Perm>,
    #[specta(type = specta_typescript::Number)]
    pub max_discount_bp: i64,
    #[specta(type = specta_typescript::Number)]
    pub max_override_bp: i64,
    pub ui_language: String,
    #[specta(type = Option<specta_typescript::Number>)]
    pub staff_no: Option<i64>,
}

impl From<&Session> for SessionInfo {
    fn from(s: &Session) -> Self {
        SessionInfo {
            staff_id: s.staff_id.clone(),
            display_name: s.display_name.clone(),
            role_name: s.role_name.clone(),
            permissions: s.perms.list(),
            max_discount_bp: s.max_discount_bp,
            max_override_bp: s.max_override_bp,
            ui_language: s.ui_language.clone(),
            staff_no: s.staff_no,
        }
    }
}

#[derive(Default)]
pub struct SessionStore(Mutex<Option<Session>>);

impl SessionStore {
    fn slot(&self) -> MutexGuard<'_, Option<Session>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn start(&self, session: Session) {
        *self.slot() = Some(session);
    }

    pub fn end(&self) -> Option<Session> {
        self.slot().take()
    }

    /// The active session, refreshing its activity time. Idle longer than `idle` ⇒ locked (cleared).
    pub fn touch(&self, now: DateTime<Utc>, idle: Duration) -> AppResult<Session> {
        let mut slot = self.slot();
        let Some(session) = slot.as_mut() else {
            return Err(AppError::NotLoggedIn);
        };
        if now - session.last_activity > idle {
            *slot = None;
            return Err(AppError::SessionLocked);
        }
        session.last_activity = now;
        Ok(session.clone())
    }

    /// The active session without counting as activity (e.g. a status poll).
    pub fn peek(&self) -> Option<Session> {
        self.slot().clone()
    }

    /// Like `peek`, but applies the idle lock: an idle session is cleared and reported as `None`.
    pub fn active(&self, now: DateTime<Utc>, idle: Duration) -> Option<Session> {
        let mut slot = self.slot();
        if slot.as_ref().is_some_and(|s| now - s.last_activity > idle) {
            *slot = None;
        }
        slot.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn session(at: DateTime<Utc>) -> Session {
        Session {
            staff_id: "c".into(),
            display_name: "C".into(),
            role_name: "Cashier".into(),
            perms: PermMask::of(&[Perm::SALE_CREATE]),
            max_discount_bp: 500,
            max_override_bp: 0,
            ui_language: "si".into(),
            logged_in_at: at,
            last_activity: at,
            staff_no: Some(1),
        }
    }

    #[test]
    fn idle_lock_and_activity() {
        let t0 = Utc.with_ymd_and_hms(2026, 11, 8, 9, 0, 0).unwrap();
        let idle = Duration::minutes(DEFAULT_IDLE_LOCK_MINUTES);
        let s = SessionStore::default();
        assert_eq!(s.touch(t0, idle), Err(AppError::NotLoggedIn));
        s.start(session(t0));
        assert!(s.touch(t0 + Duration::minutes(4), idle).is_ok());
        assert!(s.touch(t0 + Duration::minutes(8), idle).is_ok(), "activity at +4 extended the window");
        assert_eq!(s.touch(t0 + Duration::minutes(14), idle), Err(AppError::SessionLocked));
        assert!(s.peek().is_none());
    }

    #[test]
    fn info_lists_permission_keys() {
        let t0 = Utc.with_ymd_and_hms(2026, 11, 8, 9, 0, 0).unwrap();
        let info = SessionInfo::from(&session(t0));
        assert_eq!(info.permissions, vec![Perm::SALE_CREATE]);
        assert_eq!(info.ui_language, "si");
    }
}
