//! Block 4.4 — Observability and support bundle (doc §8 Phase 4):
//! - Rotating logs
//! - Support bundle: application logs, sync logs, integrity check, audit chain verify
//! - Exportable as a zip archive and as structured diagnostics

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use specta::Type;
use sqlx::{Row, SqlitePool};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::audit::{self, ChainReport};
use crate::db::integrity::{self, IntegrityReport};
use crate::error::{AppError, AppResult};
use crate::sync::admin::{self, SyncLogRow};

#[derive(Debug, Clone, Serialize, Type)]
pub struct TableCount {
    pub table_name: String,
    #[specta(type = specta_typescript::Number)]
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AuditChainSummary {
    pub reports: Vec<ChainReport>,
    pub all_valid: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct SyncDiagnostic {
    pub last_logs: Vec<SyncLogRow>,
    #[specta(type = specta_typescript::Number)]
    pub pending_outbox_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub dead_ops_count: i64,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct LogFileEntry {
    pub filename: String,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: i64,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct SupportBundle {
    pub app_version: String,
    pub os_platform: String,
    pub generated_at: String,
    pub store_id: Option<String>,
    pub device_id: Option<String>,
    pub device_code: Option<String>,
    pub integrity: IntegrityReport,
    pub table_counts: Vec<TableCount>,
    pub audit_chain: AuditChainSummary,
    pub sync: SyncDiagnostic,
    pub logs: Vec<LogFileEntry>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct SupportBundleExportResult {
    pub path: String,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: i64,
    pub files_included: Vec<String>,
}

/// Core table counts for diagnostics.
pub async fn count_tables(pool: &SqlitePool) -> Vec<TableCount> {
    let tracked_tables = [
        "products",
        "batches",
        "orders",
        "order_items",
        "stock_movements",
        "staff",
        "audit_logs",
        "sync_outbox",
        "sync_inbox",
    ];

    let mut table_counts = Vec::with_capacity(tracked_tables.len());
    for tbl in tracked_tables {
        let count_row = sqlx::query(&format!("SELECT COUNT(*) FROM {tbl}")).fetch_one(pool).await;
        let count = match count_row {
            Ok(r) => r.try_get(0).unwrap_or(0),
            Err(_) => 0,
        };
        table_counts.push(TableCount { table_name: tbl.to_string(), count });
    }
    table_counts
}

/// Collects recent log files and reads the trailing lines.
pub fn collect_log_files(log_dir: &Path, max_tail_bytes: usize) -> Vec<LogFileEntry> {
    let mut entries = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(log_dir) else {
        return entries;
    };

    let mut files: Vec<PathBuf> = read_dir
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "log"))
        .collect();

    // Sort newest first
    files.sort_by_key(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok());
    files.reverse();

    for file_path in files.iter().take(5) {
        let filename = file_path.file_name().map_or("app.log".to_string(), |n| n.to_string_lossy().to_string());
        let Ok(bytes) = std::fs::read(file_path) else {
            continue;
        };
        let size_bytes = bytes.len() as i64;
        let snippet = if bytes.len() <= max_tail_bytes {
            String::from_utf8_lossy(&bytes).to_string()
        } else {
            let start = bytes.len() - max_tail_bytes;
            String::from_utf8_lossy(&bytes[start..]).to_string()
        };

        entries.push(LogFileEntry { filename, size_bytes, snippet });
    }

    entries
}

/// Builds the complete diagnostics support bundle.
pub async fn build_support_bundle(
    pool: &SqlitePool,
    log_dir: &Path,
    app_version: &str,
    identity: Option<(&str, &str, &str)>, // store_id, device_id, device_code
) -> AppResult<SupportBundle> {
    let integrity = integrity::check(pool).await?;
    let table_counts = count_tables(pool).await;

    let chain_reports = audit::verify(pool).await?;
    let all_valid = chain_reports.iter().all(|r| r.ok);
    let audit_chain = AuditChainSummary { reports: chain_reports, all_valid };

    let last_logs = admin::logs_pool(pool, 25).await.unwrap_or_default();
    let pending_outbox_row = sqlx::query("SELECT COUNT(*) FROM sync_outbox").fetch_one(pool).await;
    let pending_outbox_count = pending_outbox_row.map_or(0, |r| r.try_get(0).unwrap_or(0));

    let dead_ops_row =
        sqlx::query("SELECT COUNT(*) FROM sync_dead_ops WHERE resolved_at IS NULL").fetch_one(pool).await;
    let dead_ops_count = dead_ops_row.map_or(0, |r| r.try_get(0).unwrap_or(0));

    let sync = SyncDiagnostic { last_logs, pending_outbox_count, dead_ops_count };
    let logs = collect_log_files(log_dir, 64 * 1024);

    let (store_id, device_id, device_code) = match identity {
        Some((s, d, c)) => (Some(s.to_string()), Some(d.to_string()), Some(c.to_string())),
        None => (None, None, None),
    };

    Ok(SupportBundle {
        app_version: app_version.to_string(),
        os_platform: std::env::consts::OS.to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        store_id,
        device_id,
        device_code,
        integrity,
        table_counts,
        audit_chain,
        sync,
        logs,
    })
}

/// Exports the support bundle as a zip archive containing diagnostics.json and raw log files.
pub async fn export_support_bundle_zip(
    pool: &SqlitePool,
    log_dir: &Path,
    dest_path: &Path,
    app_version: &str,
    identity: Option<(&str, &str, &str)>,
) -> AppResult<SupportBundleExportResult> {
    let bundle = build_support_bundle(pool, log_dir, app_version, identity).await?;
    let bundle_json = serde_json::to_string_pretty(&bundle)
        .map_err(|e| AppError::internal(format!("serialize support bundle: {e}")))?;

    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(dest_path)?;
    let mut zip = ZipWriter::new(file);
    let options =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).unix_permissions(0o644);

    let mut files_included = Vec::new();

    // 1. diagnostics.json
    zip.start_file("diagnostics.json", options)
        .map_err(|e| AppError::internal(format!("zip write diagnostics.json: {e}")))?;
    zip.write_all(bundle_json.as_bytes())?;
    files_included.push("diagnostics.json".to_string());

    // 2. Add raw log files if present
    if let Ok(read_dir) = std::fs::read_dir(log_dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "log") {
                let filename = path.file_name().map_or("app.log".to_string(), |n| n.to_string_lossy().to_string());
                let zip_name = format!("logs/{filename}");
                if let Ok(bytes) = std::fs::read(&path) {
                    zip.start_file(&zip_name, options)
                        .map_err(|e| AppError::internal(format!("zip write {zip_name}: {e}")))?;
                    zip.write_all(&bytes)?;
                    files_included.push(zip_name);
                }
            }
        }
    }

    zip.finish().map_err(|e| AppError::internal(format!("zip finalize: {e}")))?;

    let size_bytes = std::fs::metadata(dest_path)?.len() as i64;

    Ok(SupportBundleExportResult { path: dest_path.display().to_string(), size_bytes, files_included })
}
