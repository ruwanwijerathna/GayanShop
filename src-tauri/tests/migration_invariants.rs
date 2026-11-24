//! Block 0.3 — the schema's invariants (ARCHITECTURE §4.3, §13.5), proven against the bundled SQLite.
//! Each scenario is its own test; every rejection asserts its specific error message.

mod common;

use common::{BD, META, NOW, Sale, checkout, exec, open, rejects, rejects_sale, scalar_i64, seed};
use gayanshop_lib::db::{Db, integrity};

#[tokio::test]
async fn pragmas_and_schema_version() {
    let t = open().await;
    let p = t.pool();
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode").fetch_one(p).await.unwrap();
    assert_eq!(mode, "wal");
    assert_eq!(scalar_i64(p, "PRAGMA synchronous").await, 2, "synchronous must be FULL (§13.2)");
    assert_eq!(scalar_i64(p, "PRAGMA foreign_keys").await, 1);
    assert_eq!(
        t.db.schema_version().await.unwrap(),
        5,
        "0001_init + 0002_report_indexes + 0003_shared_barcodes + 0004_salesperson + 0005_exchanges"
    );
    assert_eq!(scalar_i64(p, "SELECT applying FROM sync_ctx WHERE id = 1").await, 0);
}

#[tokio::test]
async fn existing_data_is_snapshotted_before_a_pending_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(gayanshop_lib::db::DB_FILE_NAME);
    {
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        exec(&pool, "CREATE TABLE legacy (x INTEGER)").await;
        pool.close().await;
    }
    let backups = dir.path().join("backups");
    let db = Db::open(&path, &backups).await.unwrap();
    let snaps: Vec<_> = std::fs::read_dir(&backups).unwrap().filter_map(Result::ok).collect();
    assert_eq!(snaps.len(), 1);
    assert!(snaps[0].file_name().to_string_lossy().starts_with("pre-migration-"));
    db.pool.close().await;
    // Re-opening with nothing pending takes no further snapshot.
    let again = Db::open(&path, &backups).await.unwrap();
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 1);
    again.pool.close().await;
}

#[tokio::test]
async fn happy_path_checkout_updates_every_derived_table() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    checkout(p, &Sale::new("o1")).await;
    let row: (i64, i64, i64, i64) =
        sqlx::query_as("SELECT sales_count, net_sales, cogs, customer_orders FROM daily_sales_summary")
            .fetch_one(p)
            .await
            .unwrap();
    assert_eq!(row, (1, 45000, 20000, 1));
    let pay: (String, i64, i64) =
        sqlx::query_as("SELECT method, amount, txn_count FROM daily_payment_summary").fetch_one(p).await.unwrap();
    assert_eq!(pay, ("cash".into(), 45000, 1));
    let cust: (i64, i64, String) = sqlx::query_as(
        "SELECT visit_count, total_spend, last_visit_date FROM customer_stats WHERE customer_id = 'cu1'",
    )
    .fetch_one(p)
    .await
    .unwrap();
    assert_eq!(cust, (1, 45000, BD.to_owned()));
    let stock: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT on_hand, sold_qty, sold_revenue, sold_cost FROM batch_item_stock WHERE batch_item_id = 'bi1'",
    )
    .fetch_one(p)
    .await
    .unwrap();
    assert_eq!(stock, (1, 1, 45000, 20000));
    assert_eq!(scalar_i64(p, "SELECT on_hand FROM stock_levels WHERE product_id = 'p1'").await, 1);
}

#[tokio::test]
async fn negative_payments_are_impossible() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    let mut sale = Sale::new("o1").statements();
    sale[3] = format!(
        "INSERT INTO order_payments (id, store_id, device_id, order_id, method, amount, created_at) VALUES ('x', 's', 'dev1', 'o1', 'cash', -5, '{NOW}')"
    );
    let refs: Vec<&str> = sale.iter().map(String::as_str).collect();
    rejects(p, &refs, "CHECK constraint failed: amount >= 0").await;
}

#[tokio::test]
async fn sealed_orders_accept_no_new_lines_or_payments() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    checkout(p, &Sale::new("o1")).await;
    rejects(p, &[&format!("INSERT INTO order_payments (id, store_id, device_id, order_id, method, amount, created_at) VALUES ('x', 's', 'dev1', 'o1', 'cash', 5, '{NOW}')")], "cannot add payments to a sealed order").await;
    rejects(p, &[&format!("INSERT INTO order_items (id, store_id, device_id, order_id, line_no, product_id, name, list_price, unit_price, qty, line_gross, line_total, line_net, created_at) VALUES ('x', 's', 'dev1', 'o1', 2, 'p1', 'B', 1, 1, 1, 1, 1, 1, '{NOW}')")], "cannot add lines to a sealed order").await;
    rejects(p, &[&format!("INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, order_item_id, unit_cost, revenue_net, business_date, created_at) VALUES ('x', 's', 'dev1', 'p1', 'bi1', -1, 'sale', 'o1i', 20000, 1, '{BD}', '{NOW}')")], "sale slices only inside a pending checkout").await;
}

#[tokio::test]
async fn completed_orders_are_immutable_and_undeletable() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    checkout(p, &Sale::new("o1")).await;
    rejects(p, &["UPDATE orders SET grand_total = 1, paid_total = 1 WHERE id = 'o1'"], "illegal state transition")
        .await;
    rejects(p, &["UPDATE orders SET customer_id = NULL WHERE id = 'o1'"], "illegal state transition").await;
    rejects(p, &["UPDATE orders SET status = 'pending' WHERE id = 'o1'"], "illegal state transition").await;
    rejects(p, &["DELETE FROM orders WHERE id = 'o1'"], "orders cannot be deleted").await;
    rejects(p, &["UPDATE order_items SET qty = 2 WHERE id = 'o1i'"], "order_items are immutable").await;
    rejects(p, &["DELETE FROM order_items WHERE id = 'o1i'"], "order_items are immutable").await;
    rejects(p, &["UPDATE order_payments SET amount = 1 WHERE id = 'o1p'"], "order_payments are append-only").await;
    rejects(p, &["DELETE FROM stock_movements WHERE id = 'o1m'"], "stock_movements are append-only").await;
}

#[tokio::test]
async fn local_orders_must_start_pending_and_tokens_are_unique() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    rejects(p, &[&format!("INSERT INTO orders (id, store_id, device_id, cash_session_id, receipt_no, checkout_token, status, staff_id, line_count, item_count, gross_sales, net_sales, grand_total, paid_total, business_date, completed_at, created_at, updated_at, updated_by_device) VALUES ('o9', 's', 'dev1', 'cs1', 'R9', 't9', 'completed', 'cashier', 1, 1, 1, 1, 1, 1, '{BD}', '{NOW}', {META})")], "local inserts must start as pending").await;
    checkout(p, &Sale::new("o1")).await;
    let dup = Sale { token: "o1", ..Sale::new("o2") };
    rejects_sale(p, &dup, "UNIQUE constraint failed: orders.checkout_token").await;
}

#[tokio::test]
async fn pending_header_cannot_be_edited_and_seal_verifies_payments() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    let stmts = Sale::new("o1").statements();
    rejects(
        p,
        &[&stmts[0], "UPDATE orders SET grand_total = 2, paid_total = 2 WHERE id = 'o1'"],
        "illegal state transition",
    )
    .await;
    let short_pay = [
        stmts[0].as_str(),
        stmts[1].as_str(),
        stmts[2].as_str(),
        &format!(
            "INSERT INTO order_payments (id, store_id, device_id, order_id, method, amount, created_at) VALUES ('o1p', 's', 'dev1', 'o1', 'card', 40000, '{NOW}')"
        ),
        stmts[4].as_str(),
    ];
    rejects(p, &short_pay, "payments plus exchange credit must equal grand_total").await;
    let no_slice = [stmts[0].as_str(), stmts[1].as_str(), stmts[3].as_str(), stmts[4].as_str()];
    rejects(p, &no_slice, "stock slices must cover each tracked line").await;
}

#[tokio::test]
async fn discount_limit_enforced_at_seal() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    rejects_sale(p, &Sale { disc: 2700, ..Sale::new("o1") }, "discount above the cashier limit").await; // 6%
    checkout(p, &Sale { disc: 2250, ..Sale::new("o2") }).await; // exactly 5%
    checkout(p, &Sale { disc: 2700, approved: Some("sup"), ..Sale::new("o3") }).await; // 6% with supervisor (20%)
}

#[tokio::test]
async fn price_override_requires_permission_or_an_approver() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    rejects_sale(p, &Sale { unit: 40000, ..Sale::new("o1") }, "price below list needs PRICE_OVERRIDE").await;
    rejects_sale(
        p,
        &Sale { unit: 40000, approved: Some("cashier"), ..Sale::new("o2") },
        "price below list needs PRICE_OVERRIDE",
    )
    .await;
    checkout(p, &Sale { unit: 40000, approved: Some("sup"), ..Sale::new("o3") }).await;
}

#[tokio::test]
async fn void_rules_and_exact_reversal() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    checkout(p, &Sale::new("o1")).await;
    checkout(p, &Sale::new("o2")).await;
    let void = |by: &str, approver: &str, reason: &str| {
        format!(
            "UPDATE orders SET status = 'voided', voided_at = '{NOW}', voided_by = '{by}', void_approved_by = {approver}, void_reason = '{reason}' WHERE id = 'o1'"
        )
    };
    rejects(p, &[&void("cashier", "NULL", "WRONG_ITEM")], "requires VOID_ORDER").await;
    rejects(p, &[&void("cashier", "'cashier'", "WRONG_ITEM")], "requires VOID_ORDER").await;
    rejects(p, &[&void("cashier", "'sup'", "OTHER")], "CHECK constraint failed").await; // OTHER needs a note
    exec(p, &void("cashier", "'sup'", "WRONG_ITEM")).await;
    let over = format!(
        "INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, order_item_id, unit_cost, revenue_net, business_date, created_at) VALUES ('v2', 's', 'dev1', 'p1', 'bi1', 2, 'void', 'o1i', 20000, -45000, '{BD}', '{NOW}')"
    );
    rejects(p, &[&over], "void slice must mirror a sale slice").await;
    let wrong_cost =
        over.replace("'v2'", "'v3'").replace(", 2, 'void'", ", 1, 'void'").replace("20000, -45000", "1, -45000");
    rejects(p, &[&wrong_cost], "void slice must mirror a sale slice").await;
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM v_integrity_void_unreversed").await, 1, "not yet reversed");
    exec(p, &format!("INSERT INTO stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, order_item_id, unit_cost, revenue_net, business_date, created_at) VALUES ('v1', 's', 'dev1', 'p1', 'bi1', 1, 'void', 'o1i', 20000, -45000, '{BD}', '{NOW}')")).await;
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM v_integrity_void_unreversed").await, 0);
    let roll: (i64, i64, i64, i64) =
        sqlx::query_as("SELECT sales_count, void_count, void_value, net_sales FROM daily_sales_summary")
            .fetch_one(p)
            .await
            .unwrap();
    assert_eq!(roll, (1, 1, 45000, 45000));
    assert_eq!(scalar_i64(p, "SELECT amount FROM daily_payment_summary WHERE method = 'cash'").await, 45000);
    let cust: (i64, i64) =
        sqlx::query_as("SELECT visit_count, total_spend FROM customer_stats WHERE customer_id = 'cu1'")
            .fetch_one(p)
            .await
            .unwrap();
    assert_eq!(cust, (1, 45000));
    assert_eq!(scalar_i64(p, "SELECT on_hand FROM batch_item_stock WHERE batch_item_id = 'bi1'").await, 1);
    rejects(p, &["UPDATE orders SET status = 'completed' WHERE id = 'o1'"], "illegal state transition").await;
}

#[tokio::test]
async fn voiding_the_only_customer_order_resets_customer_stats() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    checkout(p, &Sale::new("o1")).await;
    exec(p, &format!("UPDATE orders SET status = 'voided', voided_at = '{NOW}', voided_by = 'sup', void_reason = 'DUPLICATE_SCAN' WHERE id = 'o1'")).await;
    let cust: (i64, i64, Option<String>) = sqlx::query_as(
        "SELECT visit_count, total_spend, last_visit_date FROM customer_stats WHERE customer_id = 'cu1'",
    )
    .fetch_one(p)
    .await
    .unwrap();
    assert_eq!(cust, (0, 0, None));
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM customer_stats WHERE customer_id IS NULL").await, 0);
}

#[tokio::test]
async fn a_closed_session_makes_its_sales_final() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    checkout(p, &Sale::new("o1")).await;
    exec(p, "UPDATE cash_sessions SET status = 'closed' WHERE id = 'cs1'").await;
    rejects(p, &[&format!("UPDATE orders SET status = 'voided', voided_at = '{NOW}', voided_by = 'sup', void_reason = 'WRONG_ITEM' WHERE id = 'o1'")], "cash session is closed").await;
    rejects(p, &["UPDATE cash_sessions SET status = 'open' WHERE id = 'cs1'"], "cannot be reopened").await;
    rejects_sale(p, &Sale::new("o2"), "cash session must be open").await;
}

#[tokio::test]
async fn voids_only_on_the_originating_device() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    checkout(p, &Sale::new("o1")).await;
    exec(p, r#"UPDATE device_settings SET value = '"dev2"' WHERE key = 'device.id'"#).await;
    rejects(p, &[&format!("UPDATE orders SET status = 'voided', voided_at = '{NOW}', voided_by = 'sup', void_reason = 'WRONG_ITEM' WHERE id = 'o1'")], "only on the device that made the sale").await;
}

#[tokio::test]
async fn pulled_rows_apply_parents_first_under_the_sync_flag() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    let mut tx = p.begin().await.unwrap();
    gayanshop_lib::db::sync_ctx::begin_applying(&mut tx).await.unwrap();
    for sql in [
        format!(
            "INSERT INTO cash_sessions (id, store_id, device_id, business_date, status, opened_by, opened_at, created_at, updated_at, updated_by_device) VALUES ('cs2', 's', 'dev2', '{BD}', 'closed', 'sup', '{NOW}', {META})"
        ),
        format!(
            "INSERT INTO orders (id, store_id, device_id, cash_session_id, receipt_no, checkout_token, status, staff_id, line_count, item_count, gross_sales, net_sales, grand_total, paid_total, business_date, completed_at, created_at, updated_at, updated_by_device) VALUES ('r1', 's', 'dev2', 'cs2', 'T02-1', 'u1', 'completed', 'sup', 1, 1, 1000, 1000, 1000, 1000, '{BD}', '{NOW}', '{NOW}', '{NOW}', 'dev2')"
        ),
        format!(
            "INSERT INTO order_items (id, store_id, device_id, order_id, line_no, is_custom, custom_key, name, list_price, unit_price, qty, line_gross, line_total, line_net, cost_known, created_at) VALUES ('r1i', 's', 'dev2', 'r1', 1, 1, 'mug', 'Mug', 1000, 1000, 1, 1000, 1000, 1000, 0, '{NOW}')"
        ),
        format!(
            "INSERT INTO order_payments (id, store_id, device_id, order_id, method, amount, created_at) VALUES ('r1p', 's', 'dev2', 'r1', 'qr', 1000, '{NOW}')"
        ),
    ] {
        sqlx::query(&sql).execute(&mut *tx).await.unwrap();
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT times_sold FROM quick_sale_stats WHERE custom_key = 'mug'")
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        1
    );
    sqlx::query(&format!("UPDATE orders SET status = 'voided', voided_at = '{NOW}', voided_by = 'sup', void_reason = 'DUPLICATE_SCAN', updated_by_device = 'dev2' WHERE id = 'r1'"))
        .execute(&mut *tx)
        .await
        .unwrap();
    gayanshop_lib::db::sync_ctx::end_applying(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    let roll: (i64, i64, i64) =
        sqlx::query_as("SELECT sales_count, void_count, net_sales FROM daily_sales_summary WHERE device_id = 'dev2'")
            .fetch_one(p)
            .await
            .unwrap();
    assert_eq!(roll, (0, 1, 0));
    let pay: (i64, i64) = sqlx::query_as(
        "SELECT amount, txn_count FROM daily_payment_summary WHERE device_id = 'dev2' AND method = 'qr'",
    )
    .fetch_one(p)
    .await
    .unwrap();
    assert_eq!(pay, (0, 0));
    assert_eq!(scalar_i64(p, "SELECT COUNT(*) FROM quick_sale_stats WHERE custom_key = 'mug'").await, 0);
    assert_eq!(scalar_i64(p, "SELECT applying FROM sync_ctx").await, 0);
    // Outside the flag the same pulled-style insert is refused by policy.
    rejects(p, &[&format!("INSERT INTO orders (id, store_id, device_id, cash_session_id, receipt_no, checkout_token, status, staff_id, line_count, item_count, gross_sales, net_sales, grand_total, paid_total, business_date, completed_at, created_at, updated_at, updated_by_device) VALUES ('r2', 's', 'dev2', 'cs2', 'T02-2', 'u2', 'completed', 'sup', 1, 1, 1, 1, 1, 1, '{BD}', '{NOW}', {META})")], "local inserts must start as pending").await;
}

#[tokio::test]
async fn a_rolled_back_pull_restores_the_sync_flag() {
    let t = open().await;
    let p = t.pool();
    {
        let mut tx = p.begin().await.unwrap();
        gayanshop_lib::db::sync_ctx::begin_applying(&mut tx).await.unwrap();
        assert!(gayanshop_lib::db::sync_ctx::is_applying(&mut tx).await.unwrap());
        tx.rollback().await.unwrap();
    }
    assert_eq!(scalar_i64(p, "SELECT applying FROM sync_ctx").await, 0);
}

#[tokio::test]
async fn audit_chain_guard_and_append_only() {
    let t = open().await;
    let p = t.pool();
    let zeros = "0".repeat(64);
    exec(p, &format!("INSERT INTO audit_logs (id, store_id, device_id, device_seq, action, business_date, prev_hash, hash, created_at) VALUES ('a1', 's', 'dev1', 1, 'X', '{BD}', '{zeros}', 'h1', '{NOW}')")).await;
    rejects(p, &[&format!("INSERT INTO audit_logs (id, store_id, device_id, device_seq, action, business_date, prev_hash, hash, created_at) VALUES ('a3', 's', 'dev1', 3, 'X', '{BD}', 'h1', 'h3', '{NOW}')")], "device_seq must be the previous seq + 1").await;
    rejects(p, &[&format!("INSERT INTO audit_logs (id, store_id, device_id, device_seq, action, business_date, prev_hash, hash, created_at) VALUES ('a2', 's', 'dev1', 2, 'X', '{BD}', 'zz', 'h2', '{NOW}')")], "prev_hash must equal").await;
    rejects(p, &["UPDATE audit_logs SET action = 'Y'"], "audit_logs are append-only").await;
    rejects(p, &["DELETE FROM audit_logs"], "audit_logs are append-only").await;
}

#[tokio::test]
async fn cash_movements_append_only_and_batch_cost_locked() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    exec(p, &format!("INSERT INTO cash_movements (id, store_id, device_id, cash_session_id, type, amount, reason, staff_id, business_date, created_at) VALUES ('c1', 's', 'dev1', 'cs1', 'pay_out', 500, 'tea', 'sup', '{BD}', '{NOW}')")).await;
    rejects(p, &["UPDATE cash_movements SET amount = 1"], "cash_movements are append-only").await;
    rejects(p, &["DELETE FROM cash_movements"], "cash_movements are append-only").await;
    rejects(p, &["UPDATE batch_items SET unit_cost = 1 WHERE id = 'bi1'"], "locked after receive").await;
}

#[tokio::test]
async fn integrity_report_is_healthy_on_a_fresh_store() {
    let t = open().await;
    seed(t.pool()).await;
    checkout(t.pool(), &Sale::new("o1")).await;
    let report = integrity::check(t.pool()).await.unwrap();
    assert!(report.healthy(), "{report:?}");
}
