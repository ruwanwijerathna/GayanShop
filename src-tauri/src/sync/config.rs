//! Which server this counter syncs with. Kept per device in `device_settings` and entered at registration,
//! never compiled into the binary: moving to a self-hosted server is a change of these two values.

use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use crate::db::settings;
use crate::error::{AppError, AppResult};

pub const SERVER_KEY: &str = "sync.server";
pub const REGISTERED_AT_KEY: &str = "sync.registered_at";

/// The sync endpoint: the server's base URL and its public (anon) API key. The anon key only identifies the
/// project to the API gateway; every row is still guarded by RLS on the device's own token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub anon_key: String,
}

impl ServerConfig {
    /// Trim and check: HTTPS, except plain HTTP to this machine (the local development stack).
    pub fn parse(url: &str, anon_key: &str) -> AppResult<Self> {
        let url = url.trim().trim_end_matches('/').to_owned();
        let anon_key = anon_key.trim().to_owned();
        let local = ["http://127.0.0.1:", "http://localhost:"].iter().any(|p| url.starts_with(p));
        let ok_scheme = url.starts_with("https://") || local;
        let host = url.split("://").nth(1).unwrap_or("");
        if !ok_scheme || host.is_empty() || url.contains(char::is_whitespace) {
            return Err(AppError::validation("SYNC_URL_INVALID", url));
        }
        if anon_key.is_empty() || anon_key.len() > 4096 || anon_key.contains(char::is_whitespace) {
            return Err(AppError::validation("SYNC_KEY_INVALID", "the project key is empty or malformed"));
        }
        Ok(Self { url, anon_key })
    }
}

pub async fn load(pool: &SqlitePool) -> AppResult<Option<ServerConfig>> {
    settings::device_get(pool, SERVER_KEY).await
}

pub async fn save(conn: &mut SqliteConnection, cfg: &ServerConfig, now: &str) -> AppResult<()> {
    settings::device_set(conn, SERVER_KEY, cfg).await?;
    settings::device_set(conn, REGISTERED_AT_KEY, &now).await
}

pub async fn registered_at(pool: &SqlitePool) -> AppResult<Option<String>> {
    settings::device_get(pool, REGISTERED_AT_KEY).await
}
