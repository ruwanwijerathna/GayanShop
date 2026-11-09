//! The sync protocol's JSON shapes (ARCHITECTURE §6). A row travels as the terminal's own SQLite row
//! (`json_object` over every column): integers as numbers, flags as 0/1, timestamps as ISO-8601 UTC text.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One outbox entry as `sync_push` takes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushOp {
    pub op_id: String,
    pub table: String,
    pub op: String,
    pub row: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpStatus {
    /// Applied, a replay of an applied op, or a stale write the newer row outlived.
    Ok,
    /// Breaks a merge rule or invariant: the op is dead (Settings → Sync).
    Rejected,
    /// A parent it references hasn't reached the cloud yet: send it again later.
    Retry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpResult {
    pub op_id: Value,
    pub status: OpStatus,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushResponse {
    pub results: Vec<OpResult>,
}

/// One row of a pull page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PulledRow {
    pub seq: i64,
    pub table: String,
    pub row: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullPage {
    pub rows: Vec<PulledRow>,
    /// Where the next pull starts.
    pub cursor: i64,
    pub has_more: bool,
    /// The store's newest server_seq when the page was taken (bootstrap progress).
    #[serde(default)]
    pub watermark: i64,
}

/// A device whose pushed audit rows did not extend its hash chain (§6.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainBreak {
    pub device_code: String,
    pub at: Option<String>,
    pub seq: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Health {
    pub ok: bool,
    pub server_time: String,
    pub device_code: String,
    /// This device's audit chain head as the cloud knows it (restore recovery checks it).
    #[serde(default)]
    pub last_audit_seq: i64,
    #[serde(default)]
    pub last_audit_hash: Option<String>,
    #[serde(default)]
    pub chain_broken: Vec<ChainBreak>,
}

/// The register-device function's request: the counter's own ids are adopted, never reassigned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub store_id: Option<String>,
    pub store_name: Option<String>,
    pub device_id: String,
    pub device_code: Option<String>,
    pub device_name: Option<String>,
    pub app_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub store_id: String,
    pub store_name: String,
    pub device_id: String,
    pub device_code: String,
    pub refresh_token: String,
    pub access_token: String,
    pub expires_in: i64,
}
