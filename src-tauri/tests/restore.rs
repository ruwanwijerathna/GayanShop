//! Block 4.1: automatic backups and restore with recovery (doc §8 Phase 4). Rotation at fixed timestamps; the
//! hourly run; the refusals (another store, another counter, a newer schema, a corrupt file); a crash in the
//! middle of the swap; the freeze while a restore is pending; and both recovery drills against an
//! in-process sync server (`support/hub.rs`): online (this counter's own post-backup rows come back) and
//! offline (the identity is retired, the backup's queued ops still go under the old one, and the old
//! identity's history comes back as another counter's). Plus the local path of a store that never synced.

mod common;
#[path = "support/hub.rs"]
mod hub;
#[path = "support/term.rs"]
mod term;

use std::path::{Path, PathBuf};

use chrono::{Duration, TimeZone, Utc};
use gayanshop_lib::auth::Auth;
use gayanshop_lib::auth::pin::PinHasher;
use gayanshop_lib::backup::{self, NoFaults};
use gayanshop_lib::clock::Clock;
use gayanshop_lib::db::Db;
use gayanshop_lib::domain::business_date::StoreCalendar;
use gayanshop_lib::error::AppError;
use gayanshop_lib::restore::{self, RecoveryMode, StageInput};
use gayanshop_lib::sync::engine;
use term::{Term, derived, hub_for, line, rebuilt, t01};

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } | AppError::Invariant { code, .. } => code,
        other => panic!("expected a coded error, got {other:?}"),
    }
}

/// A verified "Backup Database Now" copy of `t` into `dir`.
async fn backup_into(t: &Term, dir: &Path, n: u32) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let name = format!("GayanShop-{}-test{n}.db", t.code);
    let out = backup::backup_now(&t.db, &t.ctx(), &t.actor(), "0.1.0", dir, &name, &NoFaults).await.unwrap();
    PathBuf::from(out.path)
}

async fn stage(t: &Term, source: &Path) -> Result<restore::Inspected, AppError> {
    let input = StageInput { data_dir: t._dir.path(), source, app_version: "0.1.0", tz: chrono_tz::Asia::Colombo };
    restore::stage(&t.db, &t.ctx(), &t.actor(), &input, &NoFaults).await
}

/// What the app does at its next start: swap the staged file in before the database opens, then finish.
async fn restart(t: &mut Term) {
    t.db.pool.close().await;
    let dir = t._dir.path().to_path_buf();
    assert!(restore::apply_staged(&dir, &dir.join("pos.db")).unwrap());
    t.db = Db::open(&dir.join("pos.db"), &dir.join("backups")).await.unwrap();
    restore::finish_swap(&t.db, &dir).await.unwrap();
}

fn colombo() -> StoreCalendar {
    StoreCalendar::colombo_default()
}

#[test]
fn rotation_keeps_48_hourlies_and_one_per_business_day_for_30_days() {
    let cal = colombo();
    // Forty days of hourly backups, ending 2026-11-09 12:00 Colombo (06:30 UTC).
    let end = Utc.with_ymd_and_hms(2026, 11, 9, 6, 30, 0).unwrap();
    let all: Vec<(String, chrono::DateTime<Utc>)> = (0..40 * 24)
        .map(|h| {
            let at = end - Duration::hours(h);
            (backup::auto_name("T01", at, cal.timezone), at)
        })
        .collect();
    let keep = backup::rotation_keep(&all, &cal);
    let newest = &all[0].0;
    assert!(keep.contains(newest), "never the newest");
    for (name, _) in all.iter().take(48) {
        assert!(keep.contains(name), "the 48 newest are kept");
    }
    let days: std::collections::BTreeSet<_> =
        all.iter().filter(|(n, _)| keep.contains(n)).map(|(_, at)| cal.business_date(*at)).collect();
    assert_eq!(days.len(), 30, "30 business days are represented");
    // Beyond the hourlies, exactly one per business day, the newest of that day.
    let older: Vec<_> = all.iter().skip(48).filter(|(n, _)| keep.contains(n)).collect();
    let older_days: std::collections::BTreeSet<_> = older.iter().map(|(_, at)| cal.business_date(*at)).collect();
    assert_eq!(older.len(), older_days.len());
    assert!(keep.len() < 48 + 30, "{} kept", keep.len());

    // The business day turns at 04:00 Colombo, not midnight: 03:30 belongs to the day before.
    let before = Utc.with_ymd_and_hms(2026, 11, 8, 22, 0, 0).unwrap(); // 03:30 on the 9th
    let after = Utc.with_ymd_and_hms(2026, 11, 8, 23, 0, 0).unwrap(); // 04:30 on the 9th
    let pair = [
        (backup::auto_name("T01", before, cal.timezone), before),
        (backup::auto_name("T01", after, cal.timezone), after),
    ];
    assert_ne!(cal.business_date(before), cal.business_date(after));
    assert_eq!(backup::auto_taken_at(&pair[0].0, cal.timezone), Some(before));
}

#[tokio::test]
async fn the_automatic_backup_runs_hourly_rotates_and_audits_failures_once() {
    let t = t01().await;
    let identity = gayanshop_lib::setup::load_identity(&t.db.pool).await.unwrap().unwrap();
    let backups = t._dir.path().join("backups");
    let auto = backups.join(backup::AUTO_DIR);
    std::fs::create_dir_all(&auto).unwrap();
    // Sixty days of older automatic backups already on disk (stand-ins), plus a file rotation must not touch.
    for d in 1..=60 {
        let at = t.clock.now() - Duration::days(d);
        std::fs::write(auto.join(backup::auto_name("T01", at, identity.calendar.timezone)), b"old").unwrap();
    }
    std::fs::write(auto.join("notes.txt"), b"keep me").unwrap();

    let first = backup::run_auto_if_due(&t.db, &t.clock, &backups, &identity, "0.1.0").await.unwrap();
    assert!(first.is_some(), "the first run takes a backup");
    let again = backup::run_auto_if_due(&t.db, &t.clock, &backups, &identity, "0.1.0").await.unwrap();
    assert!(again.is_none(), "not again within the hour");
    let autos = |dir: &Path| {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name().to_string_lossy().starts_with("auto-") && e.path().extension().is_some_and(|x| x == "db")
            })
            .count()
    };
    // 61 copies: rotation keeps the 48 newest (a count, so disk use stays bounded whatever the schedule);
    // the 30 newest business days are among them, so the 13 oldest go.
    assert_eq!(autos(&auto), 48);
    let oldest = backup::auto_name("T01", t.clock.now() - Duration::days(60), identity.calendar.timezone);
    let kept = backup::auto_name("T01", t.clock.now() - Duration::days(47), identity.calendar.timezone);
    assert!(!auto.join(oldest).exists() && auto.join(kept).exists());
    assert!(auto.join("notes.txt").exists(), "only automatic backups are ever deleted");
    let status = backup::status(&t.db, t.clock.now()).await.unwrap();
    assert!(status.last_auto_at.is_some());
    assert_eq!(
        t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BACKUP_CREATE'").await,
        0,
        "successes aren't audited"
    );

    // Two failures in a row: audited once. The next success clears the error.
    for _ in 0..2 {
        t.tick(3_601);
        let name = backup::auto_name("T01", t.clock.now(), identity.calendar.timezone);
        std::fs::write(auto.join(&name), b"in the way").unwrap();
        let err = backup::run_auto_if_due(&t.db, &t.clock, &backups, &identity, "0.1.0").await.unwrap_err();
        assert_eq!(code(err), "BACKUP_EXISTS");
    }
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BACKUP_FAILED'").await, 1);
    assert_eq!(backup::status(&t.db, t.clock.now()).await.unwrap().auto_error.as_deref(), Some("BACKUP_EXISTS"));
    t.tick(1);
    assert!(backup::run_auto_if_due(&t.db, &t.clock, &backups, &identity, "0.1.0").await.unwrap().is_some());
    assert_eq!(backup::status(&t.db, t.clock.now()).await.unwrap().auto_error, None);
}

#[tokio::test]
async fn a_backup_of_another_store_counter_or_a_newer_app_or_a_corrupt_file_is_refused() {
    let a = t01().await;
    let other_store = t01().await;
    let usb = a._dir.path().join("usb");
    let foreign = backup_into(&other_store, &usb, 1).await;
    assert_eq!(code(stage(&a, &foreign).await.unwrap_err()), "RESTORE_OTHER_STORE");

    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = term::joined(&hub, &a.store_id, "T02").await;
    let t02 = backup_into(&b, &usb, 2).await;
    assert_eq!(
        code(stage(&a, &t02).await.unwrap_err()),
        "RESTORE_OTHER_COUNTER",
        "two counters would share one identity"
    );

    let mine = backup_into(&a, &usb, 3).await;
    let newer = usb.join("newer.db");
    std::fs::copy(&mine, &newer).unwrap();
    {
        let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", newer.display())).await.unwrap();
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
             VALUES (99999999999999, 'from the future', '2027-01-01', 1, x'00', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }
    assert_eq!(code(stage(&a, &newer).await.unwrap_err()), "RESTORE_NEWER_SCHEMA");

    let corrupt = usb.join("corrupt.db");
    std::fs::write(&corrupt, b"this is not a database").unwrap();
    assert_eq!(code(stage(&a, &corrupt).await.unwrap_err()), "RESTORE_CORRUPT");
    assert!(!a._dir.path().join(restore::MARKER).exists(), "nothing was staged");
    // None of these touched the live database.
    assert!(restore::pending(&a.db.pool).await.unwrap().is_none());
}

#[tokio::test]
async fn a_crash_in_the_middle_of_the_swap_resumes_and_writes_are_frozen_until_recovery() {
    let mut a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    a.sell("s1", vec![line(&p, 45_000, 1)], None).await;
    let b = backup_into(&a, &a._dir.path().join("usb"), 1).await;
    a.sell("s2", vec![line(&p, 45_000, 1)], None).await;
    stage(&a, &b).await.unwrap();
    assert!(
        a._dir.path().join("backups").join("pre-restore").read_dir().unwrap().count() >= 2,
        "the current DB is kept first"
    );
    // The power fails halfway through writing the restored copy: a torn .restoring file is left behind.
    std::fs::write(a._dir.path().join("pos.db.restoring"), b"torn").unwrap();
    restart(&mut a).await;
    assert_eq!(a.count("SELECT COUNT(*) FROM orders").await, 1, "the backup's data, not the torn copy or the old file");
    assert!(!a._dir.path().join(restore::MARKER).exists());
    let pending = restore::pending(&a.db.pool).await.unwrap().unwrap();
    assert!(pending.pre_restore.contains("pre-restore"));

    // Pending: nothing may be written under this counter's identity, a login included.
    let auth = Auth::new(PinHasher::fast_for_tests());
    let identity = gayanshop_lib::setup::load_identity(&a.db.pool).await.unwrap().unwrap();
    let dev = gayanshop_lib::auth::DeviceCtx {
        store_id: identity.store_id,
        device_id: identity.device_id,
        calendar: identity.calendar,
    };
    let err = gayanshop_lib::auth::login(&a.db, &auth, &a.clock, &dev, &a.owner, "246810").await.unwrap_err();
    assert_eq!(code(err), "RESTORE_PENDING");
    let mut input = common::cart("s3", vec![line(&p, 45_000, 1)], vec![common::cash(45_000, None)]);
    input.customer_phone = None;
    let seller = a.seller().await;
    let err = gayanshop_lib::db::repo::checkout::checkout(
        &a.db,
        &a.ctx(),
        &seller,
        &input,
        None,
        &gayanshop_lib::db::repo::checkout::NoFaults,
    )
    .await
    .unwrap_err();
    assert_eq!(code(err), "RESTORE_PENDING", "no sale until the restore is finished");
    // The Owner's PIN is checked without writing a word to the audit log.
    let before = a.count("SELECT COUNT(*) FROM audit_logs").await;
    let wrong = restore::verify_owner(&a.db, &auth.hasher, &a.clock, &a.owner, "000000").await.unwrap_err();
    assert!(matches!(wrong, AppError::InvalidPin { .. }));
    restore::verify_owner(&a.db, &auth.hasher, &a.clock, &a.owner, "246810").await.unwrap();
    assert_eq!(a.count("SELECT COUNT(*) FROM audit_logs").await, before);
}

#[tokio::test]
async fn online_recovery_brings_back_the_sales_made_after_the_backup() {
    let mut a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((50, 20_000))).await;
    let hub = hub_for(&a).await;
    for i in 0..3 {
        a.sell(&format!("before{i}"), vec![line(&p, 45_000, 1)], Some("0771234567")).await;
    }
    a.round(&hub).await;
    let b = backup_into(&a, &a._dir.path().join("usb"), 1).await;
    for i in 0..4 {
        a.sell(&format!("after{i}"), vec![line(&p, 45_000, 1)], None).await;
    }
    a.round(&hub).await;
    let expected = derived(&a.db).await;
    stage(&a, &b).await.unwrap();
    restart(&mut a).await;
    assert_eq!(a.count("SELECT COUNT(*) FROM orders").await, 3, "the backup is behind");

    let out = restore::recover_online(&a.db, &hub.client(&a.device_id), &a.clock, &a.owner, 7).await.unwrap();
    assert_eq!(out.mode, RecoveryMode::Online);
    assert!(out.rows_recovered > 0);
    assert_eq!(a.count("SELECT COUNT(*) FROM orders").await, 7, "this counter's own post-backup sales came back");
    assert_eq!(derived(&a.db).await, expected, "the same numbers as before the restore");
    assert_eq!(rebuilt(&a.db).await, expected);
    assert!(restore::pending(&a.db.pool).await.unwrap().is_none());
    // Selling continues without a receipt-number or audit-sequence collision.
    let o = a.sell("next", vec![line(&p, 45_000, 1)], None).await;
    let receipt = a.text(&format!("SELECT receipt_no FROM orders WHERE id = '{o}'")).await;
    assert!(receipt.ends_with("-0008"), "{receipt}");
    a.round(&hub).await;
    assert_eq!(a.count("SELECT COUNT(*) FROM sync_outbox").await, 0, "every op accepted, none dead");
    assert_eq!(hub.count("orders"), 8);
    assert!(gayanshop_lib::audit::verify(&a.db.pool).await.unwrap().iter().all(|r| r.ok), "the chain continues");
    assert_eq!(a.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'BACKUP_RESTORE'").await, 1);
}

#[tokio::test]
async fn online_recovery_refuses_a_chain_the_cloud_disagrees_with() {
    let mut a = t01().await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = backup_into(&a, &a._dir.path().join("usb"), 1).await;
    stage(&a, &b).await.unwrap();
    restart(&mut a).await;
    *hub.health_override.lock().unwrap() = Some((1, "f".repeat(64)));
    let err = restore::recover_online(&a.db, &hub.client(&a.device_id), &a.clock, &a.owner, 7).await.unwrap_err();
    assert_eq!(code(err), "RESTORE_CHAIN_MISMATCH");
    assert!(restore::pending(&a.db.pool).await.unwrap().is_some(), "still pending: the offline path remains");
    let out = restore::recover_offline(&a.db, &a.clock, &a.owner).await.unwrap();
    assert_eq!(out.mode, RecoveryMode::Offline);
}

#[tokio::test]
async fn offline_recovery_retires_the_identity_and_nothing_is_lost() {
    let mut a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((50, 20_000))).await;
    let hub = hub_for(&a).await;
    a.sell("synced", vec![line(&p, 45_000, 1)], None).await;
    a.round(&hub).await;
    // Two sales never reach the cloud before the backup is taken.
    a.sell("only-in-backup-1", vec![line(&p, 45_000, 1)], None).await;
    a.sell("only-in-backup-2", vec![line(&p, 45_000, 1)], None).await;
    let b = backup_into(&a, &a._dir.path().join("usb"), 1).await;
    // One more sale, then the counter dies without syncing again.
    a.sell("after-backup", vec![line(&p, 45_000, 1)], None).await;
    let after_id = a.text("SELECT id FROM orders WHERE checkout_token = 'after-backup'").await;
    let old_device = a.device_id.clone();
    stage(&a, &b).await.unwrap();
    restart(&mut a).await;

    let out = restore::recover_offline(&a.db, &a.clock, &a.owner).await.unwrap();
    assert_eq!(out.device_code, "T02", "a code no counter of this store has used");
    let identity = gayanshop_lib::setup::load_identity(&a.db.pool).await.unwrap().unwrap();
    assert_ne!(identity.device_id, old_device);
    assert_eq!(identity.device_code, "T02");
    a.device_id = identity.device_id.clone();
    a.code = identity.device_code.clone();
    a.open_session().await;
    let o = a.sell("new-identity", vec![line(&p, 45_000, 1)], None).await;
    let receipt = a.text(&format!("SELECT receipt_no FROM orders WHERE id = '{o}'")).await;
    assert!(receipt.starts_with("T02-") && receipt.ends_with("-0001"), "{receipt}");

    // The backup's queued ops go under the retired identity, the new ones under the new identity.
    assert!(!engine::drain_retired(&a.db, &hub.client(&old_device), &a.clock).await.unwrap(), "all sent");
    assert!(restore::retired(&a.db.pool).await.unwrap().is_none());
    assert_eq!(hub.count("orders"), 3, "the two that only the backup had reached the cloud");
    a.tick(10);
    engine::round(&a.db, &hub.client(&a.device_id), &a.clock, &a.device_id).await.unwrap();
    assert_eq!(hub.count("orders"), 4);
    assert_eq!(a.count("SELECT COUNT(*) FROM sync_outbox").await, 0, "nothing dead");
    assert!(
        gayanshop_lib::audit::verify(&a.db.pool).await.unwrap().iter().all(|r| r.ok),
        "both identities' chains verify"
    );
    assert_eq!(
        a.count(&format!("SELECT COUNT(*) FROM orders WHERE id = '{after_id}'")).await,
        0,
        "the sale made after the backup on the dead counter is lost: it never left it"
    );
}

#[tokio::test]
async fn offline_recovery_brings_back_the_old_identitys_history_from_the_cloud() {
    let mut a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((50, 20_000))).await;
    let hub = hub_for(&a).await;
    a.round(&hub).await;
    let b = backup_into(&a, &a._dir.path().join("usb"), 1).await;
    for i in 0..3 {
        a.sell(&format!("after{i}"), vec![line(&p, 45_000, 1)], None).await;
    }
    a.round(&hub).await;
    let old_device = a.device_id.clone();
    stage(&a, &b).await.unwrap();
    restart(&mut a).await;
    restore::recover_offline(&a.db, &a.clock, &a.owner).await.unwrap();
    let identity = gayanshop_lib::setup::load_identity(&a.db.pool).await.unwrap().unwrap();
    a.device_id = identity.device_id;
    engine::drain_retired(&a.db, &hub.client(&old_device), &a.clock).await.unwrap();
    a.tick(10);
    engine::round(&a.db, &hub.client(&a.device_id), &a.clock, &a.device_id).await.unwrap();
    // The pull restarted at 0: the retired identity's post-backup sales are back, as another counter's.
    assert_eq!(a.count(&format!("SELECT COUNT(*) FROM orders WHERE device_id = '{old_device}'")).await, 3);
    assert_eq!(a.count("SELECT COUNT(*) FROM sync_outbox").await, 0);
    assert_eq!(rebuilt(&a.db).await, derived(&a.db).await);
}

#[tokio::test]
async fn a_store_that_never_synced_restores_and_keeps_its_identity() {
    let mut a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    a.sell("s1", vec![line(&p, 45_000, 1)], None).await;
    let b = backup_into(&a, &a._dir.path().join("usb"), 1).await;
    a.sell("s2", vec![line(&p, 45_000, 1)], None).await;
    stage(&a, &b).await.unwrap();
    restart(&mut a).await;
    let out = restore::recover_local(&a.db, &a.clock, &a.owner).await.unwrap();
    assert_eq!((out.mode, out.device_code.as_str()), (RecoveryMode::Local, "T01"));
    let o = a.sell("s3", vec![line(&p, 45_000, 1)], None).await;
    let receipt = a.text(&format!("SELECT receipt_no FROM orders WHERE id = '{o}'")).await;
    assert!(receipt.starts_with("T01-") && receipt.ends_with("-0002"), "continues from the restored data: {receipt}");
    assert!(gayanshop_lib::audit::verify(&a.db.pool).await.unwrap().iter().all(|r| r.ok));
    assert_eq!(rebuilt(&a.db).await, derived(&a.db).await);
    let err = restore::recover_local(&a.db, &a.clock, &a.owner).await.unwrap_err();
    assert_eq!(code(err), "RESTORE_NOT_PENDING");
}

#[tokio::test]
async fn a_fresh_install_restores_a_counter_from_its_backup_file() {
    let a = t01().await;
    let p = a.product("Bucket", 45_000, None, Some((10, 20_000))).await;
    a.sell("s1", vec![line(&p, 45_000, 1)], None).await;
    let b = backup_into(&a, &a._dir.path().join("usb"), 1).await;
    // A new PC: an empty data folder.
    let mut fresh = Term::empty().await;
    let checked = restore::stage_fresh(&fresh.db, fresh._dir.path(), &b, "2026-11-09T05:00:00.000Z").await.unwrap();
    assert_eq!(checked.device_code, "T01");
    restart(&mut fresh).await;
    let identity = gayanshop_lib::setup::load_identity(&fresh.db.pool).await.unwrap().unwrap();
    assert_eq!((identity.store_id.as_str(), identity.device_code.as_str()), (a.store_id.as_str(), "T01"));
    assert!(restore::pending(&fresh.db.pool).await.unwrap().is_some(), "recovery still to do");
    assert_eq!(fresh.count("SELECT COUNT(*) FROM orders").await, 1);
    // A set-up counter can't use the first-run path.
    let err = restore::stage_fresh(&a.db, a._dir.path(), &b, "2026-11-09T05:00:00.000Z").await.unwrap_err();
    assert!(matches!(err, AppError::AlreadyInitialized));
}
