//! Application error type. Serialised to the UI as `{ kind: "...", ... }`.

use serde::Serialize;
use specta::Type;

use crate::domain::permissions::Perm;

#[derive(Debug, thiserror::Error, Serialize, Type)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppError {
    #[error("this terminal has not been set up yet")]
    NotInitialized,
    #[error("this terminal is already set up")]
    AlreadyInitialized,
    #[error("no staff member is logged in")]
    NotLoggedIn,
    #[error("session locked after inactivity; log in again")]
    SessionLocked,
    #[error("not permitted: {perm:?}")]
    Forbidden { perm: Perm },
    #[error("supervisor approval required: {perm:?}")]
    ApprovalRequired { perm: Perm },
    #[error("approval is invalid, expired or already used")]
    InvalidApproval,
    #[error("incorrect PIN")]
    InvalidPin { attempts_left: u32 },
    #[error("account locked until {until}")]
    AccountLocked { until: String },
    #[error("{message}")]
    Validation { message: String },
    #[error("{message}")]
    Invariant { message: String },
    #[error("not found: {what}")]
    NotFound { what: String },
    #[error("database error: {message}")]
    Database { message: String },
    #[error("internal error: {message}")]
    Internal { message: String },
}

impl AppError {
    pub fn validation(msg: impl Into<String>) -> Self {
        AppError::Validation { message: msg.into() }
    }
    pub fn invariant(msg: impl Into<String>) -> Self {
        AppError::Invariant { message: msg.into() }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal { message: msg.into() }
    }
    pub fn not_found(what: impl Into<String>) -> Self {
        AppError::NotFound { what: what.into() }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Database { message: e.to_string() }
    }
}

impl From<sqlx::migrate::MigrateError> for AppError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        AppError::Database { message: e.to_string() }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::internal(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
