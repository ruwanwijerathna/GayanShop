//! Restore from a backup, with recovery (doc §8 Phase 4, Block 4.1). Owner-only (`BACKUP_RESTORE`).
//!
//! A restore never edits the live database in place. It is staged and swapped across a restart:
//!
//! 1. **Stage** (app running): the chosen file is inspected (integrity, schema not newer than this app, the
//!    same store *and the same counter*: restoring T02's backup onto T01 would give two counters one
//!    identity). The current database is backed up first (`backups/pre-restore`), the file is copied to
//!    `restore/staged.db`, and a marker (`restore.json`, phase `staged`) is written. The app restarts.
//! 2. **Swap** (startup, before the database opens): the staged copy is written to `pos.db.restoring`,
//!    flushed, the old `-wal`/`-shm` are deleted (SQLite would otherwise replay the old WAL into the restored
//!    file), and it is renamed over `pos.db`. The marker moves to `swapped`. Every step can be repeated, so a
//!    crash anywhere resumes cleanly at the next start.
//! 3. **Finish the swap** (after the database opens): `restore.pending` is recorded in the restored
//!    database, the backup's unprinted print jobs are abandoned (they belong to the past), a crash-recovery
//!    cart whose sale is already in the backup is dropped, and the marker is removed.
//! 4. **Recover** (before anyone logs in, authorised by the Owner's PIN, which writes nothing). While a restore
//!    is pending this counter writes nothing under its identity: no audit row and no outbox row (the
//!    [`frozen`] guard), so no sale, no login, and the sync worker stays paused. The restored audit chain is
//!    behind the cloud's, and any row written now would take a `device_seq` the cloud already holds.
//!    * **Online** (registered, reachable): pull this counter's own post-backup rows back (`include_own`)
//!      from the backup's cursor; check that the audit chain now ends where the cloud's does; reset the
//!      receipt counters from the orders; requeue the backup's outbox (acknowledged ops resolve as replays).
//!    * **Offline** (registered, unreachable): the counter can't know what it lost, so it **retires its
//!      identity**: a new device id and a code no counter has used (T01 → T04), so receipt numbers, SKU
//!      ranges and audit sequences can't collide. The backup's queued ops still belong to the old identity:
//!      the worker sends them under it (its token is still in the keyring) before anything else. The pull
//!      cursor restarts at 0, so once the new identity is registered it receives the old one's history.
//!    * **Local** (never registered, decision #21): nothing was ever copied anywhere, so the counter keeps
//!      its identity and its counters continue from the restored data.
//!
//!    Every mode ends with `rebuild_derived` (derived tables and the search index) and one `BACKUP_RESTORE`
//!    audit row, written last.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection, SqliteConnection};

use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::auth::pin::PinHasher;
use crate::backup::{self, BackupFaults};
use crate::clock::{Clock, iso, new_id};
use crate::db::Db;
use crate::db::repo::{WriteCtx, reports};
use crate::db::settings::{self, keys};
use crate::domain::business_date::{StoreCalendar, format_date};
use crate::error::{AppError, AppResult};
use crate::sync::api::SyncApi;
use crate::sync::engine::{self, PullOptions};

pub const MARKER: &str = "restore.json";
pub const PENDING_KEY: &str = "restore.pending";
/// The identity an offline restore retired, and the last outbox op that belongs to it.
pub const RETIRED_KEY: &str = "sync.retired_identity";
const STAGED_DIR: &str = "restore";
const STAGED_FILE: &str = "staged.db";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Staged,
    Swapped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Marker {
    phase: Phase,
    pending: RestorePending,
}

/// What a pending restore is: shown on the recovery screen, recorded in the `BACKUP_RESTORE` audit row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RestorePending {
    /// The file the owner chose.
    pub source: String,
    pub source_sha256: String,
    /// When the backup was taken (its manifest, else the file's time).
    pub taken_at: String,
    /// The copy of the database as it was just before the restore.
    pub pre_restore: String,
    pub requested_by: String,
    pub requested_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryMode {
    Online,
    Offline,
    Local,
}

/// The identity an offline restore retired: its ops up to `upto_seq` are sent under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetiredIdentity {
    pub device_id: String,
    pub device_code: String,
    pub upto_seq: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RecoveryOutcome {
    pub mode: RecoveryMode,
    pub device_code: String,
    /// Rows the recovery pull brought back (online).
    #[specta(type = specta_typescript::Number)]
    pub rows_recovered: i64,
}

/// A backup file, checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Inspected {
    pub path: String,
    pub store_id: String,
    pub device_id: String,
    pub device_code: String,
    #[specta(type = specta_typescript::Number)]
    pub schema_version: i64,
    pub taken_at: String,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: i64,
    pub sha256: String,
}

fn io(code: &str, e: impl std::fmt::Display) -> AppError {
    AppError::validation(code, e.to_string())
}

/// Check a backup before it may replace this counter's database.
/// `expect` is this counter's (store_id, device_id); `None` on a fresh install restoring onto a new PC.
pub async fn inspect(path: &Path, expect: Option<(&str, &str)>, max_schema: i64) -> AppResult<Inspected> {
    if !path.is_file() {
        return Err(AppError::validation("RESTORE_FILE_MISSING", path.display().to_string()));
    }
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .connect()
        .await
        .map_err(|e| io("RESTORE_CORRUPT", e))?;
    let check: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut conn)
        .await
        .map_err(|e| io("RESTORE_CORRUPT", e))?;
    if check != "ok" {
        return Err(AppError::validation("RESTORE_CORRUPT", check));
    }
    let schema: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(&mut conn)
        .await
        .map_err(|e| io("RESTORE_NOT_A_BACKUP", e))?;
    if schema > max_schema {
        return Err(AppError::validation(
            "RESTORE_NEWER_SCHEMA",
            format!("schema {schema}, this app has {max_schema}"),
        ));
    }
    let setting = |key: &'static str| {
        sqlx::query_scalar::<_, String>("SELECT value FROM device_settings WHERE key = ?1").bind(key)
    };
    let read = |raw: Option<String>| raw.and_then(|v| serde_json::from_str::<String>(&v).ok());
    let b_store =
        read(setting(keys::STORE_ID).fetch_optional(&mut conn).await.map_err(|e| io("RESTORE_NOT_A_BACKUP", e))?);
    let b_device =
        read(setting(keys::DEVICE_ID).fetch_optional(&mut conn).await.map_err(|e| io("RESTORE_NOT_A_BACKUP", e))?);
    let b_code =
        read(setting(keys::DEVICE_CODE).fetch_optional(&mut conn).await.map_err(|e| io("RESTORE_NOT_A_BACKUP", e))?);
    conn.close().await.map_err(|e| io("RESTORE_CORRUPT", e))?;
    let (Some(b_store), Some(b_device), Some(b_code)) = (b_store, b_device, b_code) else {
        return Err(AppError::validation("RESTORE_NOT_A_BACKUP", "no counter identity in the file"));
    };
    if let Some((store_id, device_id)) = expect {
        if b_store != store_id {
            return Err(AppError::validation("RESTORE_OTHER_STORE", b_store));
        }
        if b_device != device_id {
            return Err(AppError::validation("RESTORE_OTHER_COUNTER", b_code));
        }
    }
    let (sha256, size) = backup::sha256_file(path).map_err(|e| io("RESTORE_CORRUPT", e))?;
    Ok(Inspected {
        path: path.display().to_string(),
        store_id: b_store,
        device_id: b_device,
        device_code: b_code,
        schema_version: schema,
        taken_at: taken_at(path),
        size_bytes: i64::try_from(size).unwrap_or(i64::MAX),
        sha256,
    })
}

/// When a backup was taken: its manifest's `created_at`, else the file's modification time.
fn taken_at(path: &Path) -> String {
    let manifest = PathBuf::from(format!("{}.json", path.display()));
    if let Ok(text) = std::fs::read_to_string(&manifest)
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(at) = v.get("created_at").and_then(|a| a.as_str())
    {
        return at.to_owned();
    }
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| iso(chrono::DateTime::<chrono::Utc>::from(t)))
        .unwrap_or_default()
}

fn write_marker(data_dir: &Path, marker: &Marker) -> AppResult<()> {
    let path = data_dir.join(MARKER);
    let tmp = data_dir.join(format!("{MARKER}.tmp"));
    let text = serde_json::to_string_pretty(marker)?;
    let mut f = std::fs::File::create(&tmp).map_err(|e| io("RESTORE_WRITE_FAILED", e))?;
    f.write_all(text.as_bytes()).map_err(|e| io("RESTORE_WRITE_FAILED", e))?;
    f.sync_all().map_err(|e| io("RESTORE_WRITE_FAILED", e))?;
    drop(f);
    retrying(|| std::fs::rename(&tmp, &path)).map_err(|e| io("RESTORE_WRITE_FAILED", e))
}

fn read_marker(data_dir: &Path) -> AppResult<Option<Marker>> {
    match std::fs::read_to_string(data_dir.join(MARKER)) {
        Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io("RESTORE_WRITE_FAILED", e)),
    }
}

/// Windows briefly holds a file that was just written or closed (an antivirus scan, the indexer): retry a
/// file operation refused that way for up to ~15 s before giving up (a just-installed app is scanned hardest).
fn retrying<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut tries = 0;
    loop {
        match op() {
            // Access denied, or a sharing / lock violation (Windows errors 32 and 33).
            Err(e)
                if (e.kind() == std::io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32 | 33)))
                    && tries < 150 =>
            {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            other => return other,
        }
    }
}

fn copy_flushed(from: &Path, to: &Path) -> AppResult<()> {
    retrying(|| std::fs::copy(from, to)).map_err(|e| io("RESTORE_WRITE_FAILED", e))?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(to)
        .and_then(|f| f.sync_all())
        .map_err(|e| io("RESTORE_WRITE_FAILED", e))
}

/// Name the swap step that failed (the support bundle shows it).
fn step(what: &str, e: AppError) -> AppError {
    match e {
        AppError::Validation { code, detail } => AppError::Validation { code, detail: format!("{what}: {detail}") },
        other => other,
    }
}

fn remove_if_present(path: &Path) -> AppResult<()> {
    match retrying(|| std::fs::remove_file(path)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io("RESTORE_WRITE_FAILED", e)),
    }
}

/// What to restore, and where this counter keeps its files.
pub struct StageInput<'a> {
    pub data_dir: &'a Path,
    pub source: &'a Path,
    pub app_version: &'a str,
    pub tz: chrono_tz::Tz,
}

/// Step 1: check the file, back up the current database, stage the copy, write the marker. The caller
/// restarts the app.
pub async fn stage(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    input: &StageInput<'_>,
    faults: &dyn BackupFaults,
) -> AppResult<Inspected> {
    let checked = inspect(input.source, Some((&ctx.store_id, &ctx.device_id)), db.schema_version().await?).await?;
    let pre_dir = input.data_dir.join("backups").join("pre-restore");
    std::fs::create_dir_all(&pre_dir).map_err(|e| io("RESTORE_WRITE_FAILED", e))?;
    let now = crate::clock::parse_iso(&ctx.now).unwrap_or_else(chrono::Utc::now);
    let name = format!("pre-restore-{}", backup::file_name(&ctx.device_code, now, input.tz));
    let pre = backup::backup_now(db, ctx, actor, input.app_version, &pre_dir, &name, faults).await?;
    let pending = RestorePending {
        source: checked.path.clone(),
        source_sha256: checked.sha256.clone(),
        taken_at: checked.taken_at.clone(),
        pre_restore: pre.path,
        requested_by: actor.staff_id.clone(),
        requested_at: ctx.now.clone(),
    };
    put_staged(input.data_dir, input.source, &checked, pending)?;
    Ok(checked)
}

/// Step 1 on a fresh install (a new PC, or a reinstall): there is nothing to compare with or back up, so the
/// file is only checked and staged. Recovery still needs the Owner's PIN from the restored data.
pub async fn stage_fresh(db: &Db, data_dir: &Path, source: &Path, now: &str) -> AppResult<Inspected> {
    if crate::setup::is_initialized(&db.pool).await? {
        return Err(AppError::AlreadyInitialized);
    }
    let checked = inspect(source, None, db.schema_version().await?).await?;
    let pending = RestorePending {
        source: checked.path.clone(),
        source_sha256: checked.sha256.clone(),
        taken_at: checked.taken_at.clone(),
        pre_restore: String::new(),
        requested_by: String::new(),
        requested_at: now.to_owned(),
    };
    put_staged(data_dir, source, &checked, pending)?;
    Ok(checked)
}

fn put_staged(data_dir: &Path, source: &Path, checked: &Inspected, pending: RestorePending) -> AppResult<()> {
    let staged_dir = data_dir.join(STAGED_DIR);
    std::fs::create_dir_all(&staged_dir).map_err(|e| io("RESTORE_WRITE_FAILED", e))?;
    let staged = staged_dir.join(STAGED_FILE);
    copy_flushed(source, &staged)?;
    let (sha, _) = backup::sha256_file(&staged).map_err(|e| io("RESTORE_WRITE_FAILED", e))?;
    if sha != checked.sha256 {
        return Err(AppError::validation("RESTORE_WRITE_FAILED", "the staged copy differs from the backup"));
    }
    write_marker(data_dir, &Marker { phase: Phase::Staged, pending })
}

/// Step 2, at startup **before** the database opens: put the staged copy in place of `pos.db`. Returns
/// whether a restore is in progress.
pub fn apply_staged(data_dir: &Path, db_path: &Path) -> AppResult<bool> {
    let Some(mut marker) = read_marker(data_dir)? else { return Ok(false) };
    if marker.phase == Phase::Staged {
        let staged = data_dir.join(STAGED_DIR).join(STAGED_FILE);
        let restoring = PathBuf::from(format!("{}.restoring", db_path.display()));
        copy_flushed(&staged, &restoring).map_err(|e| step("copy the staged file", e))?;
        remove_if_present(&PathBuf::from(format!("{}-wal", db_path.display()))).map_err(|e| step("remove -wal", e))?;
        remove_if_present(&PathBuf::from(format!("{}-shm", db_path.display()))).map_err(|e| step("remove -shm", e))?;
        retrying(|| std::fs::rename(&restoring, db_path))
            .map_err(|e| io("RESTORE_WRITE_FAILED", format!("replace pos.db: {e}")))?;
        marker.phase = Phase::Swapped;
        write_marker(data_dir, &marker)?;
    }
    Ok(true)
}

/// Step 3, after the database opens: record the pending restore in it and clear what must not survive.
pub async fn finish_swap(db: &Db, data_dir: &Path) -> AppResult<()> {
    let Some(marker) = read_marker(data_dir)? else { return Ok(()) };
    if marker.phase != Phase::Swapped {
        return Ok(());
    }
    let mut tx = db.write().await?;
    let c = tx.conn();
    settings::device_set(c, PENDING_KEY, &marker.pending).await?;
    // The backup's unprinted jobs are receipts of the past; printing them now would be wrong.
    sqlx::query("UPDATE print_jobs SET status = 'abandoned' WHERE status IN ('queued', 'printing', 'failed')")
        .execute(&mut *c)
        .await?;
    // A crash-recovery cart whose sale is already in the backup must not be sold again.
    sqlx::query(
        "DELETE FROM held_carts WHERE id = '__current__'
           AND checkout_token IN (SELECT checkout_token FROM orders)",
    )
    .execute(&mut *c)
    .await?;
    tx.commit().await?;
    remove_if_present(&data_dir.join(MARKER))?;
    remove_if_present(&data_dir.join(STAGED_DIR).join(STAGED_FILE))
}

pub async fn pending(pool: &sqlx::SqlitePool) -> AppResult<Option<RestorePending>> {
    settings::device_get(pool, PENDING_KEY).await
}

/// While a restore is pending, nothing may be written under this counter's identity (the sync pull, which
/// runs with `sync_ctx.applying = 1`, is the exception). `audit::record` and the outbox call this.
pub async fn frozen(conn: &mut SqliteConnection) -> AppResult<bool> {
    let frozen: i64 = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM device_settings WHERE key = ?1)
                AND (SELECT applying FROM sync_ctx WHERE id = 1) = 0",
    )
    .bind(PENDING_KEY)
    .fetch_one(&mut *conn)
    .await?;
    Ok(frozen == 1)
}

pub fn frozen_error() -> AppError {
    AppError::validation("RESTORE_PENDING", "this counter is finishing a restore")
}

/// The Owner authorises the recovery with their PIN. Nothing is audited (the chain is frozen); wrong PINs are
/// counted in the local, unsynced lockout table like any login.
pub async fn verify_owner(db: &Db, hasher: &PinHasher, clock: &dyn Clock, staff_id: &str, pin: &str) -> AppResult<()> {
    let staff = crate::auth::staff::load(&db.pool, staff_id).await?;
    let now = iso(clock.now());
    let locked: Option<String> = sqlx::query_scalar("SELECT locked_until FROM auth_attempts WHERE staff_id = ?1")
        .bind(staff_id)
        .fetch_optional(&db.pool)
        .await?
        .flatten();
    if let Some(until) = locked
        && until > now
    {
        return Err(AppError::AccountLocked { until });
    }
    let owner = staff.is_active && staff.is_owner_role();
    if owner && hasher.verify(pin, &staff.pin_hash) {
        let mut tx = db.write().await?;
        sqlx::query("DELETE FROM auth_attempts WHERE staff_id = ?1").bind(staff_id).execute(tx.conn()).await?;
        tx.commit().await?;
        return Ok(());
    }
    let mut tx = db.write().await?;
    let failed: i64 = sqlx::query_scalar(
        "INSERT INTO auth_attempts (staff_id, failed_count, last_failed_at) VALUES (?1, 1, ?2)
         ON CONFLICT(staff_id) DO UPDATE SET failed_count = failed_count + 1, last_failed_at = ?2
         RETURNING failed_count",
    )
    .bind(staff_id)
    .bind(&now)
    .fetch_one(tx.conn())
    .await?;
    if failed >= 5 {
        let until = iso(clock.now() + chrono::Duration::minutes(5));
        sqlx::query("UPDATE auth_attempts SET locked_until = ?2, failed_count = 0 WHERE staff_id = ?1")
            .bind(staff_id)
            .bind(&until)
            .execute(tx.conn())
            .await?;
    }
    tx.commit().await?;
    if !owner {
        return Err(AppError::validation("RESTORE_OWNER_ONLY", "only the Owner can finish a restore"));
    }
    Err(AppError::InvalidPin { attempts_left: u32::try_from((5 - failed).max(0)).unwrap_or(0) })
}

/// Set each business day's receipt counter to the highest receipt this counter issued that day
/// (`{code}-{YYYYMMDD}-{seq}`).
async fn reset_receipt_counters(c: &mut SqliteConnection, device_id: &str) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO receipt_counters (day, last_seq)
         SELECT business_date, MAX(CAST(substr(receipt_no, 14) AS INTEGER)) FROM orders
         WHERE device_id = ?1 GROUP BY business_date
         ON CONFLICT(day) DO UPDATE SET last_seq = MAX(last_seq, excluded.last_seq)",
    )
    .bind(device_id)
    .execute(&mut *c)
    .await?;
    Ok(())
}

/// The audit chain head of a device: (highest `device_seq`, its hash).
async fn chain_head(pool: &sqlx::SqlitePool, device_id: &str) -> AppResult<(i64, Option<String>)> {
    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT device_seq, hash FROM audit_logs WHERE device_id = ?1 ORDER BY device_seq DESC LIMIT 1")
            .bind(device_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map_or((0, None), |(s, h)| (s, Some(h))))
}

struct Finish<'a> {
    mode: RecoveryMode,
    staff_id: &'a str,
    pending: &'a RestorePending,
    extra: serde_json::Value,
}

/// The last step of every mode: unfreeze, rebuild the derived tables and the search index, and audit the
/// restore (now the chain may continue).
async fn finish(c: &mut SqliteConnection, ctx: &WriteCtx, f: Finish<'_>) -> AppResult<()> {
    reports::recompute(&mut *c).await?;
    sqlx::query("DELETE FROM device_settings WHERE key = ?1").bind(PENDING_KEY).execute(&mut *c).await?;
    let mut metadata = json!({
        "mode": f.mode, "source": f.pending.source, "sha256": f.pending.source_sha256,
        "taken_at": f.pending.taken_at, "pre_restore": f.pending.pre_restore,
    });
    if let (Some(m), Some(extra)) = (metadata.as_object_mut(), f.extra.as_object()) {
        m.extend(extra.clone());
    }
    audit::record(
        c,
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::BACKUP_RESTORE,
            staff_id: Some(f.staff_id),
            approved_by: None,
            entity_type: Some("backup"),
            entity_id: None,
            metadata,
        },
    )
    .await?;
    Ok(())
}

async fn require_pending(db: &Db) -> AppResult<RestorePending> {
    pending(&db.pool).await?.ok_or_else(|| AppError::validation("RESTORE_NOT_PENDING", "no restore to finish"))
}

fn ctx_for(
    store_id: &str,
    device_id: &str,
    device_code: &str,
    clock: &dyn Clock,
    calendar: &StoreCalendar,
) -> WriteCtx {
    let now = clock.now();
    WriteCtx {
        store_id: store_id.to_owned(),
        device_id: device_id.to_owned(),
        device_code: device_code.to_owned(),
        business_date: format_date(calendar.business_date(now)),
        now: iso(now),
    }
}

async fn identity(db: &Db) -> AppResult<crate::setup::Identity> {
    crate::setup::load_identity(&db.pool).await?.ok_or(AppError::NotInitialized)
}

/// Online recovery: bring this counter's own post-backup rows back from the cloud, then continue.
pub async fn recover_online(
    db: &Db,
    api: &dyn SyncApi,
    clock: &dyn Clock,
    staff_id: &str,
    page_size: i64,
) -> AppResult<RecoveryOutcome> {
    let pending = require_pending(db).await?;
    let me = identity(db).await?;
    let health = api.health().await?;
    let opts = PullOptions { page_size, max_pages: i64::MAX, include_own: true };
    let pulled = engine::pull(db, api, clock, &me.device_id, opts, &|_| {}).await?;
    // The chain must now end where the cloud's does (or beyond it, with rows the cloud hasn't received yet).
    if health.last_audit_seq > 0 {
        let at: Option<String> =
            sqlx::query_scalar("SELECT hash FROM audit_logs WHERE device_id = ?1 AND device_seq = ?2")
                .bind(&me.device_id)
                .bind(health.last_audit_seq)
                .fetch_optional(&db.pool)
                .await?;
        if at.is_none() || at != health.last_audit_hash {
            let (local, _) = chain_head(&db.pool, &me.device_id).await?;
            return Err(AppError::invariant(
                "RESTORE_CHAIN_MISMATCH",
                format!("the cloud's chain ends at {}, this counter's at {local}", health.last_audit_seq),
            ));
        }
    }
    let ctx = ctx_for(&me.store_id, &me.device_id, &me.device_code, clock, &me.calendar);
    let mut tx = db.write().await?;
    let c = tx.conn();
    reset_receipt_counters(c, &me.device_id).await?;
    // The backup's queued ops go again; the cloud answers the ones it already has as replays.
    sqlx::query(
        "UPDATE sync_outbox SET status = 'pending', next_attempt_at = ?1 WHERE status IN ('in_flight', 'failed')",
    )
    .bind(&ctx.now)
    .execute(&mut *c)
    .await?;
    let extra = json!({ "rows_recovered": pulled.rows });
    finish(c, &ctx, Finish { mode: RecoveryMode::Online, staff_id, pending: &pending, extra }).await?;
    tx.commit().await?;
    Ok(RecoveryOutcome { mode: RecoveryMode::Online, device_code: me.device_code, rows_recovered: pulled.rows })
}

/// The smallest counter code no counter of this store is known to have used.
async fn free_code(pool: &sqlx::SqlitePool, current: &str) -> AppResult<String> {
    let used: Vec<String> = sqlx::query_scalar(
        "SELECT substr(receipt_no, 1, 3) FROM orders
         UNION SELECT json_extract(metadata, '$.device_code') FROM audit_logs
           WHERE action IN ('STORE_INITIALIZED', 'DEVICE_REGISTER') AND json_extract(metadata, '$.device_code') IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    (1..=9)
        .map(|n| format!("T0{n}"))
        .find(|c| c != current && !used.contains(c))
        .ok_or_else(|| AppError::validation("RESTORE_NO_FREE_CODE", "all nine counter codes are in use"))
}

/// Offline recovery: retire this counter's identity and continue as a new one.
pub async fn recover_offline(db: &Db, clock: &dyn Clock, staff_id: &str) -> AppResult<RecoveryOutcome> {
    let pending = require_pending(db).await?;
    let me = identity(db).await?;
    let code = free_code(&db.pool, &me.device_code).await?;
    let new_device = new_id();
    let ctx = ctx_for(&me.store_id, &new_device, &code, clock, &me.calendar);
    let mut tx = db.write().await?;
    let c = tx.conn();
    let upto: Option<i64> =
        sqlx::query_scalar("SELECT MAX(seq) FROM sync_outbox WHERE status <> 'dead'").fetch_one(&mut *c).await?;
    if let Some(upto_seq) = upto {
        let retired =
            RetiredIdentity { device_id: me.device_id.clone(), device_code: me.device_code.clone(), upto_seq };
        settings::device_set(c, RETIRED_KEY, &retired).await?;
        sqlx::query(
            "UPDATE sync_outbox SET status = 'pending', next_attempt_at = ?1 WHERE status IN ('in_flight', 'failed')",
        )
        .bind(&ctx.now)
        .execute(&mut *c)
        .await?;
    }
    settings::device_set(c, keys::DEVICE_ID, &new_device).await?;
    settings::device_set(c, keys::DEVICE_CODE, &code).await?;
    sqlx::query("DELETE FROM receipt_counters").execute(&mut *c).await?;
    // From the start once the new identity is registered: the old one's history comes back as another counter's.
    sqlx::query("DELETE FROM sync_state").execute(&mut *c).await?;
    sqlx::query("DELETE FROM device_settings WHERE key = ?1")
        .bind(crate::sync::config::REGISTERED_AT_KEY)
        .execute(&mut *c)
        .await?;
    let extra =
        json!({ "retired_device_id": me.device_id, "retired_device_code": me.device_code, "device_code": code });
    finish(c, &ctx, Finish { mode: RecoveryMode::Offline, staff_id, pending: &pending, extra }).await?;
    tx.commit().await?;
    Ok(RecoveryOutcome { mode: RecoveryMode::Offline, device_code: code, rows_recovered: 0 })
}

/// A store that never registered with the cloud: nothing to recover, the counter keeps its identity.
pub async fn recover_local(db: &Db, clock: &dyn Clock, staff_id: &str) -> AppResult<RecoveryOutcome> {
    let pending = require_pending(db).await?;
    let me = identity(db).await?;
    let ctx = ctx_for(&me.store_id, &me.device_id, &me.device_code, clock, &me.calendar);
    let mut tx = db.write().await?;
    let c = tx.conn();
    reset_receipt_counters(c, &me.device_id).await?;
    finish(c, &ctx, Finish { mode: RecoveryMode::Local, staff_id, pending: &pending, extra: json!({}) }).await?;
    tx.commit().await?;
    Ok(RecoveryOutcome { mode: RecoveryMode::Local, device_code: me.device_code, rows_recovered: 0 })
}

pub async fn retired(pool: &sqlx::SqlitePool) -> AppResult<Option<RetiredIdentity>> {
    settings::device_get(pool, RETIRED_KEY).await
}
