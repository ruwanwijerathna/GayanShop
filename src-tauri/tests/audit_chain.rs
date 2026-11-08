//! Block 0.5 — audit trail: record() inside a transaction, per-device chains, tamper detection, paging.

mod common;

use common::{BD, NOW, open, scalar_i64};
use gayanshop_lib::audit::{self, AuditAction, AuditCtx, AuditEntry, ChainBreakReason};
use serde_json::json;

fn ctx<'a>(device: &'a str, now: &'a str) -> AuditCtx<'a> {
    AuditCtx { store_id: "s", device_id: device, business_date: BD, now }
}

async fn record_n(db: &gayanshop_lib::db::Db, device: &str, n: usize) {
    for i in 0..n {
        let now = format!("2026-11-08T09:00:{i:02}.000Z");
        let mut tx = db.write().await.unwrap();
        audit::record(
            tx.conn(),
            &ctx(device, &now),
            AuditEntry {
                action: AuditAction::AUTH_LOGIN,
                staff_id: Some("staff-1"),
                approved_by: None,
                entity_type: Some("staff"),
                entity_id: Some("staff-1"),
                metadata: json!({ "i": i, "z": "last", "a": { "nested": true } }),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
}

#[tokio::test]
async fn records_a_gapless_verified_chain_with_outbox_rows() {
    let t = open().await;
    record_n(&t.db, "dev1", 5).await;
    let reports = audit::verify(t.pool()).await.unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].ok && reports[0].rows_checked == 5, "{reports:?}");
    assert_eq!(scalar_i64(t.pool(), "SELECT MAX(device_seq) FROM audit_logs").await, 5);
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'audit_logs'").await, 5);
    // metadata is stored canonically (sorted keys)
    let meta: String =
        sqlx::query_scalar("SELECT metadata FROM audit_logs WHERE device_seq = 1").fetch_one(t.pool()).await.unwrap();
    assert_eq!(meta, r#"{"a":{"nested":true},"i":0,"z":"last"}"#);
}

#[tokio::test]
async fn chains_are_independent_per_device() {
    let t = open().await;
    record_n(&t.db, "dev1", 3).await;
    // A pulled row from another counter arrives under the sync flag (its own chain, seq 1).
    let mut tx = t.db.write().await.unwrap();
    gayanshop_lib::db::sync_ctx::begin_applying(tx.conn()).await.unwrap();
    audit::record(
        tx.conn(),
        &ctx("dev2", NOW),
        AuditEntry {
            action: AuditAction::VOID_ORDER,
            staff_id: None,
            approved_by: None,
            entity_type: None,
            entity_id: None,
            metadata: json!({}),
        },
    )
    .await
    .unwrap();
    gayanshop_lib::db::sync_ctx::end_applying(tx.conn()).await.unwrap();
    tx.commit().await.unwrap();
    record_n(&t.db, "dev1", 1).await;
    let reports = audit::verify(t.pool()).await.unwrap();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.ok), "{reports:?}");
    assert_eq!(reports.iter().find(|r| r.device_id == "dev1").unwrap().rows_checked, 4);
}

#[tokio::test]
async fn tampering_is_detected() {
    let t = open().await;
    record_n(&t.db, "dev1", 4).await;
    // Someone with file access drops the append-only trigger and edits row 2.
    sqlx::query("DROP TRIGGER trg_audit_no_update").execute(t.pool()).await.unwrap();
    sqlx::query(r#"UPDATE audit_logs SET metadata = '{"i":99}' WHERE device_seq = 2"#).execute(t.pool()).await.unwrap();
    let report = &audit::verify(t.pool()).await.unwrap()[0];
    assert!(!report.ok);
    let brk = report.first_break.as_ref().unwrap();
    assert_eq!((brk.device_seq, brk.reason), (2, ChainBreakReason::ContentMismatch));
}

#[tokio::test]
async fn relinking_a_forged_row_is_still_detected() {
    let t = open().await;
    record_n(&t.db, "dev1", 3).await;
    sqlx::query("DROP TRIGGER trg_audit_no_update").execute(t.pool()).await.unwrap();
    // Forge row 3's prev_hash so it no longer links to row 2.
    sqlx::query("UPDATE audit_logs SET prev_hash = 'forged' WHERE device_seq = 3").execute(t.pool()).await.unwrap();
    let brk = audit::verify(t.pool()).await.unwrap()[0].first_break.clone().unwrap();
    assert_eq!((brk.device_seq, brk.reason), (3, ChainBreakReason::PrevHashMismatch));
}

#[tokio::test]
async fn a_failed_action_leaves_no_audit_row() {
    let t = open().await;
    {
        let mut tx = t.db.write().await.unwrap();
        audit::record(
            tx.conn(),
            &ctx("dev1", NOW),
            AuditEntry {
                action: AuditAction::SETTINGS_CHANGE,
                staff_id: None,
                approved_by: None,
                entity_type: None,
                entity_id: None,
                metadata: json!({}),
            },
        )
        .await
        .unwrap();
        // dropped without commit = rolled back, as when the business write fails
    }
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM audit_logs").await, 0);
    assert_eq!(scalar_i64(t.pool(), "SELECT COUNT(*) FROM sync_outbox").await, 0);
}

#[tokio::test]
async fn pages_newest_first_with_a_keyset_cursor() {
    let t = open().await;
    record_n(&t.db, "dev1", 5).await;
    let first = audit::page(t.pool(), None, 2).await.unwrap();
    assert_eq!(first.rows.iter().map(|r| r.device_seq).collect::<Vec<_>>(), vec![5, 4]);
    let second = audit::page(t.pool(), first.next_cursor.as_ref(), 2).await.unwrap();
    assert_eq!(second.rows.iter().map(|r| r.device_seq).collect::<Vec<_>>(), vec![3, 2]);
    let third = audit::page(t.pool(), second.next_cursor.as_ref(), 2).await.unwrap();
    assert_eq!(third.rows.iter().map(|r| r.device_seq).collect::<Vec<_>>(), vec![1]);
    assert!(third.next_cursor.is_none());
}
