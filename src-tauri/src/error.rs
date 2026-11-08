//! Application error type (AGENT.md §2, ARCHITECTURE §1.8).
//!
//! Serialised to the UI as `{ kind: "SCREAMING_CASE", ...fields }`. **Rust never produces display
//! text for the UI**: the UI translates `kind` (and `code` for validation/invariant errors) through the
//! `errors.<KIND>[.<code>]` i18n keys. `detail` fields are English diagnostics for logs and the support
//! bundle only.

use serde::Serialize;
use specta::Type;

use crate::domain::permissions::Perm;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Type)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppError {
    #[error("this terminal has not been set up yet")]
    NotInitialized,
    #[error("this terminal is already set up")]
    AlreadyInitialized,
    #[error("no staff member is logged in")]
    NotLoggedIn,
    #[error("session locked after inactivity")]
    SessionLocked,
    #[error("not permitted: {perm:?}")]
    Forbidden { perm: Perm },
    #[error("supervisor approval required: {perm:?}")]
    ApprovalRequired { perm: Perm },
    #[error("approval is invalid, expired or already used")]
    InvalidApproval,
    #[error("incorrect PIN ({attempts_left} attempts left)")]
    InvalidPin { attempts_left: u32 },
    #[error("account locked until {until}")]
    AccountLocked { until: String },
    /// Bad input. `code` is a stable SCREAMING_SNAKE identifier, e.g. `PIN_TOO_SHORT`.
    #[error("validation failed: {code}: {detail}")]
    Validation { code: String, detail: String },
    /// A business invariant would be broken, e.g. `LAST_OWNER`.
    #[error("invariant violated: {code}: {detail}")]
    Invariant { code: String, detail: String },
    #[error("not found: {entity}")]
    NotFound { entity: String },
    #[error("database error: {detail}")]
    Database { detail: String },
    #[error("internal error: {detail}")]
    Internal { detail: String },
}

impl AppError {
    pub fn validation(code: &str, detail: impl Into<String>) -> Self {
        AppError::Validation { code: code.to_owned(), detail: detail.into() }
    }

    pub fn invariant(code: &str, detail: impl Into<String>) -> Self {
        AppError::Invariant { code: code.to_owned(), detail: detail.into() }
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        AppError::Internal { detail: detail.into() }
    }

    pub fn not_found(entity: impl Into<String>) -> Self {
        AppError::NotFound { entity: entity.into() }
    }

    /// The i18n key the UI uses for this error (mirrors `src/db/client.ts`).
    pub fn i18n_key(&self) -> String {
        let kind = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
            .unwrap_or_else(|| "INTERNAL".to_owned());
        match self {
            AppError::Validation { code, .. } | AppError::Invariant { code, .. } => format!("errors.{kind}.{code}"),
            _ => format!("errors.{kind}"),
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Database { detail: e.to_string() }
    }
}

impl From<sqlx::migrate::MigrateError> for AppError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        AppError::Database { detail: e.to_string() }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::internal(format!("json: {e}"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::internal(format!("io: {e}"))
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::internal(format!("tauri: {e}"))
    }
}

impl<T> From<std::sync::PoisonError<T>> for AppError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        AppError::internal("a lock was poisoned by an earlier panic")
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialises_as_tagged_codes() {
        assert_eq!(serde_json::to_value(AppError::NotLoggedIn).unwrap(), json!({ "kind": "NOT_LOGGED_IN" }));
        assert_eq!(
            serde_json::to_value(AppError::ApprovalRequired { perm: Perm::VOID_ORDER }).unwrap(),
            json!({ "kind": "APPROVAL_REQUIRED", "perm": "VOID_ORDER" })
        );
        assert_eq!(
            serde_json::to_value(AppError::validation("PIN_TOO_SHORT", "needs 6 digits")).unwrap(),
            json!({ "kind": "VALIDATION", "code": "PIN_TOO_SHORT", "detail": "needs 6 digits" })
        );
    }

    #[test]
    fn i18n_keys() {
        assert_eq!(AppError::NotLoggedIn.i18n_key(), "errors.NOT_LOGGED_IN");
        assert_eq!(AppError::validation("PIN_TOO_SHORT", "x").i18n_key(), "errors.VALIDATION.PIN_TOO_SHORT");
        assert_eq!(AppError::invariant("LAST_OWNER", "x").i18n_key(), "errors.INVARIANT.LAST_OWNER");
        assert_eq!(AppError::Forbidden { perm: Perm::USER_MANAGE }.i18n_key(), "errors.FORBIDDEN");
    }

    #[test]
    fn poisoned_lock_becomes_internal() {
        let m = std::sync::Mutex::new(1);
        let _ = std::panic::catch_unwind(|| {
            let _g = m.lock().unwrap();
            panic!("poison");
        });
        let err: AppError = m.lock().map(|_| ()).unwrap_err().into();
        assert!(matches!(err, AppError::Internal { .. }));
    }
}
