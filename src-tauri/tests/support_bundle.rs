//! Block 4.4 — Observability and support bundle (doc §8 Phase 4):
//! - Rotating logs verification
//! - Support bundle: application logs, sync logs, integrity check, audit chain verify
//! - Exportable as a zip archive and as structured diagnostics

mod common;

use std::fs::File;
use std::io::Read;

use common::{open, till};
use gayanshop_lib::support;
use tempfile::tempdir;
use zip::ZipArchive;

#[tokio::test]
async fn integrity_check_reports_healthy_on_clean_till() {
    let t = till().await;
    let report = gayanshop_lib::db::integrity::check(t.pool()).await.unwrap();

    assert_eq!(report.quick_check, "ok");
    assert_eq!(report.pending_orders, 0);
    assert_eq!(report.unreversed_voids, 0);
    assert!(!report.stale_sync_flag);
    assert!(report.healthy());
}

#[tokio::test]
async fn count_tables_returns_counts_for_tracked_tables() {
    let t = till().await;
    let counts = support::count_tables(t.pool()).await;

    assert!(!counts.is_empty());
    let staff_count = counts.iter().find(|c| c.table_name == "staff").expect("staff table count");
    assert!(staff_count.count >= 1, "till has at least the owner staff member");
}

#[test]
fn collect_log_files_reads_and_truncates_correctly() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("app.log");
    std::fs::write(&log_path, "2026-11-10 [INFO] line 1\n2026-11-10 [INFO] line 2\n").unwrap();

    let logs = support::collect_log_files(dir.path(), 1024);
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].filename, "app.log");
    assert!(logs[0].snippet.contains("line 1"));
    assert!(logs[0].snippet.contains("line 2"));

    // Truncation check
    let small_logs = support::collect_log_files(dir.path(), 15);
    assert_eq!(small_logs.len(), 1);
    assert_eq!(small_logs[0].snippet.len(), 15);
}

#[tokio::test]
async fn build_support_bundle_assembles_diagnostics() {
    let t = till().await;
    let log_dir = tempdir().unwrap();
    std::fs::write(log_dir.path().join("app.log"), "startup ok\n").unwrap();

    let bundle = support::build_support_bundle(t.pool(), log_dir.path(), "0.1.0", Some(("store-1", "dev-1", "T01")))
        .await
        .unwrap();

    assert_eq!(bundle.app_version, "0.1.0");
    assert_eq!(bundle.store_id.as_deref(), Some("store-1"));
    assert_eq!(bundle.device_id.as_deref(), Some("dev-1"));
    assert_eq!(bundle.device_code.as_deref(), Some("T01"));
    assert!(bundle.integrity.healthy());
    assert!(bundle.audit_chain.all_valid);
    assert_eq!(bundle.logs.len(), 1);
    assert_eq!(bundle.logs[0].snippet, "startup ok\n");
}

#[tokio::test]
async fn export_support_bundle_zip_creates_valid_zip_with_diagnostics() {
    let t = open().await;
    let log_dir = tempdir().unwrap();
    std::fs::write(log_dir.path().join("app.log"), "log line 1\nlog line 2\n").unwrap();
    std::fs::write(log_dir.path().join("app.2026-11-09.log"), "previous day log\n").unwrap();

    let export_dir = tempdir().unwrap();
    let zip_dest = export_dir.path().join("bundle.zip");

    let result = support::export_support_bundle_zip(
        t.pool(),
        log_dir.path(),
        &zip_dest,
        "0.1.0",
        Some(("store-abc", "dev-xyz", "T02")),
    )
    .await
    .unwrap();

    assert!(result.size_bytes > 0);
    assert!(zip_dest.exists());
    assert!(result.files_included.contains(&"diagnostics.json".to_string()));

    // Verify zip contents by opening archive
    let file = File::open(&zip_dest).unwrap();
    let mut zip = ZipArchive::new(file).unwrap();

    // Check diagnostics.json
    let diag_content = {
        let mut diag_file = zip.by_name("diagnostics.json").unwrap();
        let mut content = String::new();
        diag_file.read_to_string(&mut content).unwrap();
        content
    };

    let parsed_bundle: serde_json::Value = serde_json::from_str(&diag_content).unwrap();
    assert_eq!(parsed_bundle["app_version"], "0.1.0");
    assert_eq!(parsed_bundle["store_id"], "store-abc");
    assert_eq!(parsed_bundle["device_code"], "T02");
    assert_eq!(parsed_bundle["integrity"]["quick_check"], "ok");

    // Check log files inside zip
    assert!(zip.by_name("logs/app.log").is_ok());
    assert!(zip.by_name("logs/app.2026-11-09.log").is_ok());
}
