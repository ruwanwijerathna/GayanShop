//! Block 1.10 — "Backup Database Now" (doc §3.9): a verified `VACUUM INTO` copy with a manifest and audit,
//! taken from a read connection while the till keeps selling; every failure leaves no `.partial` behind and
//! is audited as `BACKUP_FAILED`.

mod common;

use std::path::{Path, PathBuf};

use common::{CASHIER, Till, cart, cash, line, till};
use gayanshop_lib::backup::{self, BackupFaults, NoFaults};
use gayanshop_lib::db::repo::checkout::{self, NoFaults as NoCheckoutFaults};
use gayanshop_lib::error::AppError;
use sha2::{Digest, Sha256};

const NAME: &str = "GayanShop-T01-20261108-143000.db";

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

async fn sell(t: &Till, token: &str, product: &str) {
    let input = cart(token, vec![line(product, 25_000, 1)], vec![cash(25_000, None)]);
    checkout::checkout(&t.t.db, &t.ctx(), &t.seller(CASHIER).await, &input, None, &NoCheckoutFaults).await.unwrap();
}

async fn open_ro(path: &Path) -> sqlx::SqlitePool {
    let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(path).read_only(true);
    sqlx::SqlitePool::connect_with(opts).await.unwrap()
}

async fn backup_into(t: &Till, dir: &Path, faults: &dyn BackupFaults) -> Result<backup::BackupOutcome, AppError> {
    backup::backup_now(&t.t.db, &t.ctx(), &t.owner(), "0.1.0", dir, NAME, faults).await
}

#[tokio::test]
async fn a_backup_is_a_verified_copy_with_manifest_and_audit() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    sell(&t, "b1", &mug).await;
    let dest = tempfile::tempdir().unwrap();

    let out = backup_into(&t, dest.path(), &NoFaults).await.unwrap();
    let file = dest.path().join(NAME);
    assert_eq!(out.path, file.display().to_string());
    let mut files = leftovers(dest.path());
    files.sort();
    assert_eq!(files, vec![NAME.to_owned(), format!("{NAME}.json")], "no .partial, no probe file left");

    // The hash and size are the file's.
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(out.sha256, hex::encode(Sha256::digest(&bytes)));
    assert_eq!(out.size_bytes, i64::try_from(bytes.len()).unwrap());

    // The copy opens on its own and holds the sale.
    let copy = open_ro(&file).await;
    let orders: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders").fetch_one(&copy).await.unwrap();
    let check: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&copy).await.unwrap();
    assert_eq!((orders, check.as_str()), (1, "ok"));
    copy.close().await;

    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dest.path().join(format!("{NAME}.json"))).unwrap()).unwrap();
    assert_eq!(manifest["sha256"], out.sha256.as_str());
    assert_eq!(manifest["device_code"], "T01");
    assert_eq!(manifest["store_id"], t.s.store_id.as_str());
    assert_eq!(manifest["row_counts"]["orders"], 1);
    assert_eq!(manifest["schema_version"], t.t.db.schema_version().await.unwrap());

    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BACKUP_CREATE'").await, 1);
    let st = backup::status(&t.t.db, chrono::Utc::now()).await.unwrap();
    assert_eq!(st.last_path.as_deref(), Some(out.path.as_str()));
    assert_eq!(st.last_dir.as_deref(), Some(dest.path().display().to_string().as_str()));

    // The same name again is refused rather than overwritten.
    assert_eq!(code(backup_into(&t, dest.path(), &NoFaults).await.unwrap_err()), "BACKUP_EXISTS");
}

#[tokio::test]
async fn a_concurrent_checkout_during_backup_is_not_blocked_and_the_copy_is_consistent() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    for i in 0..30 {
        sell(&t, &format!("pre-{i}"), &mug).await;
    }
    let dest = tempfile::tempdir().unwrap();
    let selling = async {
        for i in 0..20 {
            sell(&t, &format!("during-{i}"), &mug).await;
        }
    };
    let (out, ()) = tokio::join!(backup_into(&t, dest.path(), &NoFaults), selling);
    let out = out.unwrap();
    assert_eq!(t.count("SELECT COUNT(*) FROM orders").await, 50, "every sale during the backup committed");

    // The copy is one consistent snapshot: whole sales only, every seal identity intact.
    let copy = open_ro(Path::new(&out.path)).await;
    let orders: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders").fetch_one(&copy).await.unwrap();
    assert!((30..=50).contains(&orders), "{orders}");
    let torn: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM orders o WHERE o.status <> 'completed'
            OR o.grand_total <> (SELECT COALESCE(SUM(line_total), 0) FROM order_items i WHERE i.order_id = o.id)
            OR o.paid_total <> (SELECT COALESCE(SUM(amount), 0) FROM order_payments p WHERE p.order_id = o.id) + o.exchange_credit",
    )
    .fetch_one(&copy)
    .await
    .unwrap();
    assert_eq!(torn, 0);
    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM v_integrity_pending_orders").fetch_one(&copy).await.unwrap();
    assert_eq!(pending, 0);
    copy.close().await;
}

/// The USB stick is pulled right after the snapshot was written.
struct PulledAfterSnapshot;
impl BackupFaults for PulledAfterSnapshot {
    fn after_snapshot(&self, partial: &Path) {
        std::fs::remove_file(partial).unwrap();
    }
}

/// The copy is corrupted on the way (a bad sector).
struct Corrupt;
impl BackupFaults for Corrupt {
    fn after_snapshot(&self, partial: &Path) {
        let mut bytes = std::fs::read(partial).unwrap();
        for b in bytes.iter_mut().skip(100).take(4096) {
            *b = 0xA5;
        }
        std::fs::write(partial, bytes).unwrap();
    }
}

#[tokio::test]
async fn destination_removed_mid_write_leaves_no_partial_and_is_audited() {
    let t = till().await;
    let dest = tempfile::tempdir().unwrap();
    let e = backup_into(&t, dest.path(), &PulledAfterSnapshot).await.unwrap_err();
    assert_eq!(code(e), "BACKUP_WRITE_FAILED");
    assert_eq!(leftovers(dest.path()), Vec::<String>::new(), "no .partial left");
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BACKUP_FAILED'").await, 1);
    let reason: String =
        sqlx::query_scalar("SELECT json_extract(metadata, '$.reason') FROM audit_logs WHERE action = 'BACKUP_FAILED'")
            .fetch_one(t.pool())
            .await
            .unwrap();
    assert_eq!(reason, "BACKUP_WRITE_FAILED");

    // The whole folder gone (drive unplugged before we start).
    let gone: PathBuf = dest.path().join("unplugged");
    assert_eq!(code(backup_into(&t, &gone, &NoFaults).await.unwrap_err()), "BACKUP_DEST_MISSING");
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BACKUP_CREATE'").await, 0);
}

#[tokio::test]
async fn a_corrupt_copy_fails_verification_and_is_deleted() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    for i in 0..40 {
        sell(&t, &format!("c{i}"), &mug).await;
    }
    let dest = tempfile::tempdir().unwrap();
    let e = backup_into(&t, dest.path(), &Corrupt).await.unwrap_err();
    assert_eq!(code(e), "BACKUP_VERIFY_FAILED");
    assert_eq!(leftovers(dest.path()), Vec::<String>::new());
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BACKUP_FAILED'").await, 1);
    // The till is untouched.
    let check: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(t.pool()).await.unwrap();
    assert_eq!(check, "ok");
}

#[tokio::test]
async fn the_reminder_is_due_after_seven_days_without_a_backup() {
    let t = till().await;
    let now = chrono::Utc::now();
    assert!(backup::status(&t.t.db, now).await.unwrap().reminder_due, "never backed up");
    let dest = tempfile::tempdir().unwrap();
    let out = backup_into(&t, dest.path(), &NoFaults).await.unwrap();
    let at = chrono::DateTime::parse_from_rfc3339(&out.created_at).unwrap().with_timezone(&chrono::Utc);
    assert!(!backup::status(&t.t.db, at + chrono::Duration::days(6)).await.unwrap().reminder_due);
    assert!(backup::status(&t.t.db, at + chrono::Duration::days(7)).await.unwrap().reminder_due);
}

#[test]
fn file_names_follow_the_doc_pattern() {
    let at = chrono::DateTime::parse_from_rfc3339("2026-11-08T09:00:05Z").unwrap().with_timezone(&chrono::Utc);
    assert_eq!(backup::file_name("T02", at, chrono_tz::Asia::Colombo), "GayanShop-T02-20261108-143005.db");
}
