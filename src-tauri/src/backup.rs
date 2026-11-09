//! "Backup Database Now" (doc §3.9). Until Phase 3 sync this is the shop's only off-machine copy.
//!
//! 1. Pre-checks: the folder exists and is writable, has ≥ 1.2 × the database size free, and the target name
//!    `GayanShop-{device_code}-{YYYYMMDD-HHmmss}.db` is new.
//! 2. Snapshot: `VACUUM INTO '<name>.db.partial'` on a pooled READ connection, never the writer lock. Under WAL
//!    that is a consistent snapshot while the till keeps selling. The live `pos.db`/`-wal`/`-shm` files are
//!    never copied.
//! 3. Verify: open the copy read-only, `PRAGMA integrity_check` = `ok`, the schema version matches, SHA-256.
//! 4. Finalise: rename `.partial` → `.db`, write the `….db.json` manifest.
//! 5. Record: audit `BACKUP_CREATE` (path, size, hash) and remember `backup.last_manual_at` / `_path`.
//! 6. Failure: delete the `.partial`, audit `BACKUP_FAILED` with a reason code, return a translatable error.
//!    Sales are never affected.

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use specta::Type;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};

use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::db::repo::WriteCtx;
use crate::db::{Db, settings};
use crate::error::{AppError, AppResult};

pub const LAST_AT: &str = "backup.last_manual_at";
pub const LAST_PATH: &str = "backup.last_manual_path";
pub const LAST_DIR: &str = "backup.last_dir";

/// Tables whose row counts go into the manifest (a quick "is this the right shop?" check on restore).
const COUNTED: [&str; 8] =
    ["products", "batches", "orders", "order_items", "stock_movements", "customers", "cash_sessions", "audit_logs"];

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct BackupOutcome {
    pub path: String,
    pub manifest_path: String,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: i64,
    pub sha256: String,
    pub created_at: String,
}

/// Failure injection for tests (USB pulled mid-write, a corrupt copy); production passes [`NoFaults`].
pub trait BackupFaults: Send + Sync {
    fn after_snapshot(&self, _partial: &Path) {}
}

pub struct NoFaults;
impl BackupFaults for NoFaults {}

struct Failure {
    code: &'static str,
    detail: String,
}

impl Failure {
    fn new(code: &'static str, detail: impl std::fmt::Display) -> Self {
        Failure { code, detail: detail.to_string() }
    }
}

pub fn file_name(device_code: &str, at: DateTime<Utc>, tz: chrono_tz::Tz) -> String {
    format!("GayanShop-{device_code}-{}.db", at.with_timezone(&tz).format("%Y%m%d-%H%M%S"))
}

/// Free bytes on the volume holding `dir`; `None` when the platform can't tell (the write itself still fails
/// cleanly on a full disk).
#[cfg(windows)]
#[allow(unsafe_code)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call; the out-pointer is a live u64.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
fn free_space(_dir: &Path) -> Option<u64> {
    None
}

fn sha256_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((hex::encode(hasher.finalize()), size))
}

async fn db_size(db: &Db) -> AppResult<u64> {
    let (pages, page_size): (i64, i64) =
        sqlx::query_as("SELECT page_count, page_size FROM pragma_page_count(), pragma_page_size()")
            .fetch_one(&db.pool)
            .await?;
    Ok(u64::try_from(pages.saturating_mul(page_size)).unwrap_or(0))
}

/// Steps 1–4; `partial` is set as soon as a `.partial` file may exist, so the caller can clean it up.
async fn snapshot_and_verify(
    db: &Db,
    ctx: &WriteCtx,
    app_version: &str,
    dest_dir: &Path,
    name: &str,
    partial: &mut Option<PathBuf>,
    faults: &dyn BackupFaults,
) -> Result<BackupOutcome, Failure> {
    // 1. Pre-checks.
    let meta = std::fs::metadata(dest_dir).map_err(|e| Failure::new("BACKUP_DEST_MISSING", e))?;
    if !meta.is_dir() {
        return Err(Failure::new("BACKUP_DEST_MISSING", dest_dir.display()));
    }
    let probe = dest_dir.join(format!(".gayanshop-write-test-{}", crate::clock::new_id()));
    std::fs::write(&probe, b"ok").map_err(|e| Failure::new("BACKUP_DEST_READ_ONLY", e))?;
    let _ = std::fs::remove_file(&probe);
    let size = db_size(db).await.map_err(|e| Failure::new("BACKUP_FAILED", e))?;
    if let Some(free) = free_space(dest_dir)
        && free < size.saturating_mul(6) / 5
    {
        return Err(Failure::new("BACKUP_DISK_FULL", format!("{free} bytes free, {size} needed")));
    }
    let target = dest_dir.join(name);
    let tmp = dest_dir.join(format!("{name}.partial"));
    let manifest_path = dest_dir.join(format!("{name}.json"));
    if target.exists() || tmp.exists() || manifest_path.exists() {
        return Err(Failure::new("BACKUP_EXISTS", target.display()));
    }

    // 2. Snapshot on a pooled read connection (VACUUM INTO never writes the live database).
    *partial = Some(tmp.clone());
    let tmp_str = tmp.to_str().ok_or_else(|| Failure::new("BACKUP_DEST_MISSING", "path is not valid text"))?;
    sqlx::query("VACUUM INTO ?1")
        .bind(tmp_str)
        .execute(&db.pool)
        .await
        .map_err(|e| Failure::new("BACKUP_WRITE_FAILED", e))?;
    faults.after_snapshot(&tmp);

    // 3. Verify the copy read-only, then close it before touching the file again (Windows locks).
    let expected_schema = db.schema_version().await.map_err(|e| Failure::new("BACKUP_FAILED", e))?;
    let mut counts = serde_json::Map::new();
    {
        if !tmp.exists() {
            return Err(Failure::new("BACKUP_WRITE_FAILED", "the copy disappeared"));
        }
        let mut conn = SqliteConnectOptions::new()
            .filename(&tmp)
            .read_only(true)
            .create_if_missing(false)
            .connect()
            .await
            .map_err(|e| Failure::new("BACKUP_VERIFY_FAILED", e))?;
        let check: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&mut conn)
            .await
            .map_err(|e| Failure::new("BACKUP_VERIFY_FAILED", e))?;
        if check != "ok" {
            return Err(Failure::new("BACKUP_VERIFY_FAILED", check));
        }
        let schema: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
            .fetch_one(&mut conn)
            .await
            .map_err(|e| Failure::new("BACKUP_VERIFY_FAILED", e))?;
        if schema != expected_schema {
            return Err(Failure::new("BACKUP_VERIFY_FAILED", format!("schema {schema}, expected {expected_schema}")));
        }
        for table in COUNTED {
            // Table names come from the constant list above, never from input.
            let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{table}\""))
                .fetch_one(&mut conn)
                .await
                .map_err(|e| Failure::new("BACKUP_VERIFY_FAILED", e))?;
            counts.insert(table.to_owned(), json!(n));
        }
        conn.close().await.map_err(|e| Failure::new("BACKUP_VERIFY_FAILED", e))?;
    }
    let (sha256, bytes) = sha256_file(&tmp).map_err(|e| Failure::new("BACKUP_VERIFY_FAILED", e))?;

    // 4. Finalise.
    std::fs::rename(&tmp, &target).map_err(|e| Failure::new("BACKUP_WRITE_FAILED", e))?;
    *partial = None;
    let size_bytes = i64::try_from(bytes).unwrap_or(i64::MAX);
    let manifest = json!({
        "app_version": app_version,
        "schema_version": expected_schema,
        "store_id": ctx.store_id,
        "device_id": ctx.device_id,
        "device_code": ctx.device_code,
        "created_at": ctx.now,
        "file": name,
        "size_bytes": size_bytes,
        "sha256": sha256,
        "row_counts": counts,
    });
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| Failure::new("BACKUP_FAILED", e))?;
    if let Err(e) = std::fs::write(&manifest_path, text) {
        // A backup without its manifest is not a finished backup.
        let _ = std::fs::remove_file(&target);
        return Err(Failure::new("BACKUP_WRITE_FAILED", e));
    }
    Ok(BackupOutcome {
        path: target.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        size_bytes,
        sha256,
        created_at: ctx.now.clone(),
    })
}

fn audit_ctx(ctx: &WriteCtx) -> AuditCtx<'_> {
    AuditCtx { store_id: &ctx.store_id, device_id: &ctx.device_id, business_date: &ctx.business_date, now: &ctx.now }
}

/// Take a verified backup into `dest_dir`. `name` is [`file_name`] for now (tests pass their own).
pub async fn backup_now(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    app_version: &str,
    dest_dir: &Path,
    name: &str,
    faults: &dyn BackupFaults,
) -> AppResult<BackupOutcome> {
    let mut partial = None;
    let result = snapshot_and_verify(db, ctx, app_version, dest_dir, name, &mut partial, faults).await;
    let mut tx = db.write().await?;
    let conn = tx.conn();
    match result {
        Ok(outcome) => {
            audit::record(
                conn,
                &audit_ctx(ctx),
                AuditEntry {
                    action: AuditAction::BACKUP_CREATE,
                    staff_id: Some(&actor.staff_id),
                    approved_by: None,
                    entity_type: Some("backup"),
                    entity_id: None,
                    metadata: json!({ "path": outcome.path, "size_bytes": outcome.size_bytes, "sha256": outcome.sha256 }),
                },
            )
            .await?;
            settings::device_set(conn, LAST_AT, &outcome.created_at).await?;
            settings::device_set(conn, LAST_PATH, &outcome.path).await?;
            settings::device_set(conn, LAST_DIR, &dest_dir.display().to_string()).await?;
            tx.commit().await?;
            Ok(outcome)
        }
        Err(f) => {
            if let Some(p) = partial {
                let _ = std::fs::remove_file(&p);
            }
            log::warn!("backup failed: {} {}", f.code, f.detail);
            audit::record(
                conn,
                &audit_ctx(ctx),
                AuditEntry {
                    action: AuditAction::BACKUP_FAILED,
                    staff_id: Some(&actor.staff_id),
                    approved_by: None,
                    entity_type: Some("backup"),
                    entity_id: None,
                    metadata: json!({ "reason": f.code, "dest": dest_dir.display().to_string() }),
                },
            )
            .await?;
            tx.commit().await?;
            Err(AppError::validation(f.code, f.detail))
        }
    }
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct BackupStatus {
    pub last_at: Option<String>,
    pub last_path: Option<String>,
    /// The folder the picker should open in.
    pub last_dir: Option<String>,
    /// No manual backup within `backup.reminder_days` (7) and no cloud sync yet: show "Back up today".
    pub reminder_due: bool,
}

pub async fn status(db: &Db, now: DateTime<Utc>) -> AppResult<BackupStatus> {
    let last_at: Option<String> = settings::device_get(&db.pool, LAST_AT).await?;
    let last_path: Option<String> = settings::device_get(&db.pool, LAST_PATH).await?;
    let last_dir: Option<String> = settings::device_get(&db.pool, LAST_DIR).await?;
    let days: i64 = settings::store_get(&db.pool, "backup.reminder_days").await?.unwrap_or(7);
    let reminder_due = match last_at.as_deref().and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
        Some(at) => now.signed_duration_since(at.with_timezone(&Utc)) >= chrono::Duration::days(days),
        None => true,
    };
    Ok(BackupStatus { last_at, last_path, last_dir, reminder_due })
}
