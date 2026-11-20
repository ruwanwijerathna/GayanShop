//! Block 1.6 — customers at the till (doc §3.8): `customer_lookup` by phone, create-at-checkout under
//! `UUIDv5(store, E.164)`, the masked phone on receipts, and the erasure-safe design (the phone lives only on
//! `customers`, so erasing it never touches an immutable order).

mod common;

use common::{CASHIER, SUP, Till, cart, cash, line, till};
use gayanshop_lib::db::repo::checkout::{self, CheckoutInput, CheckoutResult, NoFaults};
use gayanshop_lib::db::repo::customers::{self, customer_id};
use gayanshop_lib::db::repo::receipts;
use gayanshop_lib::db::repo::void::{self, VoidInput, VoidReason, Voider};
use gayanshop_lib::error::AppError;

const NATIONAL: &str = "771234567";

async fn sell(t: &Till, input: &CheckoutInput) -> CheckoutResult {
    checkout::checkout(&t.t.db, &t.ctx(), &t.seller(CASHIER).await, input, None, &NoFaults).await.unwrap()
}

fn sale(token: &str, product: &str, phone: Option<&str>, name: Option<&str>) -> CheckoutInput {
    let mut input = cart(token, vec![line(product, 25_000, 1)], vec![cash(25_000, None)]);
    input.customer_phone = phone.map(str::to_owned);
    input.customer_name = name.map(str::to_owned);
    input
}

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

/// Every row of every table except `customers` (and, unless `with_sync`, the queued sync snapshots of the
/// customers row itself), searched for the national digits of the phone.
async fn tables_holding_the_phone(t: &Till, with_sync: bool) -> Vec<String> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> 'customers'
           AND sql NOT LIKE 'CREATE VIRTUAL%' AND name NOT LIKE '%fts%'",
    )
    .fetch_all(t.pool())
    .await
    .unwrap();
    let mut hits = Vec::new();
    for table in tables {
        let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?1)")
            .bind(&table)
            .fetch_all(t.pool())
            .await
            .unwrap();
        let cond = cols.iter().map(|c| format!("CAST(\"{c}\" AS TEXT) LIKE '%{NATIONAL}%'")).collect::<Vec<_>>();
        let scope = if table == "sync_outbox" && !with_sync { " AND table_name <> 'customers'" } else { "" };
        let n = t.count(&format!("SELECT COUNT(*) FROM \"{table}\" WHERE ({}){scope}", cond.join(" OR "))).await;
        if n > 0 {
            hits.push(table);
        }
    }
    hits
}

#[tokio::test]
async fn lookup_reads_a_new_number_as_new_and_rejects_bad_input() {
    let t = till().await;
    let l = customers::lookup(t.pool(), &t.s.store_id, "077 123 4567").await.unwrap();
    assert_eq!(
        (l.customer_id, l.phone_e164.as_str(), l.phone_display.as_str(), l.display_name, l.visit_count),
        (None, "+94771234567", "077 123 4567", None, 0)
    );
    for bad in ["12345", "0771234", "+44 20 7946 0958", "07712345678", "abc"] {
        let e = customers::lookup(t.pool(), &t.s.store_id, bad).await.unwrap_err();
        assert_eq!(code(e), "PHONE_INVALID", "{bad}");
    }
    assert_eq!(t.count("SELECT COUNT(*) FROM customers").await, 0, "a lookup never writes");
}

#[tokio::test]
async fn a_customer_is_created_at_checkout_and_known_at_the_next_visit() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let first = sell(&t, &sale("s1", &mug, Some("0771234567"), Some("Saman"))).await;
    let id = customer_id(&t.s.store_id, "+94771234567");
    assert_eq!(first.customer_id.as_deref(), Some(id.as_str()));

    // Every accepted spelling of the number is the same customer.
    for typed in ["0771234567", "771234567", "94771234567", "+94 77 123 4567", "077-123-4567"] {
        let l = customers::lookup(t.pool(), &t.s.store_id, typed).await.unwrap();
        assert_eq!(
            (l.customer_id.as_deref(), l.display_name.as_deref(), l.visit_count),
            (Some(id.as_str()), Some("Saman"), 1),
            "{typed}"
        );
    }

    // A second visit without a name keeps the stored name; a void removes the visit again.
    let second = sell(&t, &sale("s2", &mug, Some("+94771234567"), None)).await;
    assert_eq!(second.customer_id.as_deref(), Some(id.as_str()));
    assert_eq!(customers::lookup(t.pool(), &t.s.store_id, NATIONAL).await.unwrap().visit_count, 2);
    let input = VoidInput {
        order_id: second.order_id.clone(),
        reason: VoidReason::CustomerCancelled,
        note: None,
        approval_id: None,
    };
    let sup = t.seller(SUP).await;
    void::void_order(&t.t.db, &t.ctx(), &Voider { staff_id: SUP.into(), perms: sup.perms }, &input, None, &NoFaults)
        .await
        .unwrap();
    let l = customers::lookup(t.pool(), &t.s.store_id, NATIONAL).await.unwrap();
    assert_eq!((l.visit_count, l.display_name.as_deref()), (1, Some("Saman")));
    assert_eq!(t.count("SELECT COUNT(*) FROM customers").await, 1);

    // A sale without a phone is the default: no customer at all.
    let anon = sell(&t, &sale("s3", &mug, None, None)).await;
    assert_eq!(anon.customer_id, None);
    let blank = sell(&t, &sale("s4", &mug, Some("   "), Some("ignored"))).await;
    assert_eq!(blank.customer_id, None);
    assert_eq!(t.count("SELECT COUNT(*) FROM customers").await, 1);
}

#[tokio::test]
async fn the_id_is_offline_stable_per_store() {
    // Two counters registering the same new number offline compute the same row id (merge by LWW).
    let a = customer_id("store-a", "+94771234567");
    assert_eq!(a, customer_id("store-a", "+94771234567"));
    assert_ne!(a, customer_id("store-b", "+94771234567"));
    assert_ne!(a, customer_id("store-a", "+94771234568"));
    assert_eq!(uuid::Uuid::parse_str(&a).unwrap().get_version_num(), 5);
}

#[tokio::test]
async fn the_receipt_masks_the_phone_and_no_other_table_stores_it() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let r = sell(&t, &sale("m1", &mug, Some("077 123 4567"), Some("Saman"))).await;

    let receipt = receipts::load(t.pool(), &r.order_id).await.unwrap();
    assert_eq!(receipt.customer_masked.as_deref(), Some("07X XXX X567"));
    assert_eq!(
        (receipt.receipt_no.as_str(), receipt.store_name.as_str(), receipt.grand_total, receipt.lines.len()),
        (r.receipt_no.as_str(), "GayanShop", 25_000, 1)
    );
    assert_eq!(receipt.footer, "Exchange within 7 days. Item for item only; no cash refunds. Thank you, come again!");
    let rendered = serde_json::to_string(&receipt).unwrap();
    assert!(!rendered.contains(NATIONAL) && !rendered.contains("Saman"), "receipt shows only the mask");

    // The phone (and the name) exist only on the customers row: not on the order, its lines, the print job,
    // the audit log or another table's sync snapshot. (The customers row itself syncs, phone included.)
    assert_eq!(tables_holding_the_phone(&t, false).await, Vec::<String>::new());
    assert_eq!(tables_holding_the_phone(&t, true).await, vec!["sync_outbox".to_owned()]);
    assert_eq!(t.count("SELECT COUNT(*) FROM customers WHERE phone_e164 = '+94771234567'").await, 1);

    let none = sell(&t, &sale("m2", &mug, None, None)).await;
    assert_eq!(receipts::load(t.pool(), &none.order_id).await.unwrap().customer_masked, None);
}

#[tokio::test]
async fn erasure_clears_the_profile_and_leaves_every_order_and_total_intact() {
    let t = till().await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let r = sell(&t, &sale("e1", &mug, Some("0771234567"), Some("Saman"))).await;
    sell(&t, &sale("e2", &mug, Some("0771234567"), None)).await;
    let id = r.customer_id.clone().unwrap();
    let totals = "SELECT (SELECT SUM(grand_total) FROM orders) * 1000 + (SELECT total_spend FROM customer_stats) / 1000
                  + (SELECT SUM(gross_sales) FROM daily_sales_summary)";
    let before = t.count(totals).await;

    customers::erase(&t.t.db, &t.ctx(), &t.owner(), &id).await.unwrap();
    let (phone, name, note, erased): (Option<String>, Option<String>, Option<String>, Option<String>) =
        sqlx::query_as("SELECT phone_e164, display_name, note, erased_at FROM customers WHERE id = ?1")
            .bind(&id)
            .fetch_one(t.pool())
            .await
            .unwrap();
    assert_eq!((phone, name, note, erased.is_some()), (None, None, None, true));
    assert_eq!(t.count(&format!("SELECT COUNT(*) FROM orders WHERE customer_id = '{id}'")).await, 2, "orders keep it");
    assert_eq!(t.count(totals).await, before, "sales, customer stats and rollups unchanged");
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'CUSTOMER_ERASE'").await, 1);
    assert_eq!(
        t.count(&format!("SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'customers' AND row_id = '{id}'")).await,
        1,
        "the erasure syncs"
    );
    let in_db = format!("SELECT COUNT(*) FROM customers WHERE phone_e164 LIKE '%{NATIONAL}%'");
    assert_eq!(t.count(&in_db).await, 0);
    assert_eq!(tables_holding_the_phone(&t, true).await, Vec::<String>::new(), "no unsent copy survives");

    // The reprint shows no customer; the lookup reads the number as new; erasing twice is refused.
    assert_eq!(receipts::load(t.pool(), &r.order_id).await.unwrap().customer_masked, None);
    let l = customers::lookup(t.pool(), &t.s.store_id, NATIONAL).await.unwrap();
    assert_eq!((l.customer_id, l.display_name, l.visit_count), (None, None, 0));
    let again = customers::erase(&t.t.db, &t.ctx(), &t.owner(), &id).await.unwrap_err();
    assert_eq!(code(again), "CUSTOMER_ALREADY_ERASED");
    assert!(matches!(
        customers::erase(&t.t.db, &t.ctx(), &t.owner(), "nobody").await.unwrap_err(),
        AppError::NotFound { .. }
    ));

    // Returning later re-activates the same id with a fresh profile.
    let back = sell(&t, &sale("e3", &mug, Some("077 123 4567"), Some("Nimal"))).await;
    assert_eq!(back.customer_id.as_deref(), Some(id.as_str()));
    let l = customers::lookup(t.pool(), &t.s.store_id, NATIONAL).await.unwrap();
    assert_eq!((l.customer_id.as_deref(), l.display_name.as_deref()), (Some(id.as_str()), Some("Nimal")));
    assert_eq!(t.count("SELECT COUNT(*) FROM customers").await, 1);
}
