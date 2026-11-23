//! Block 2.9 — back-office tables and CSV export (doc §5: `DATA_EXPORT`, audited): CSV that Excel opens
//! cleanly (BOM, CRLF, RFC 4180 quoting, decimal money, formula-injection guard), every export kind's rows
//! matching the database, cost columns only with `VIEW_COST`, atomic writes with one `DATA_EXPORT` audit
//! row (no personal data), and the keyset order history behind the Orders table.

mod common;

use common::{CASHIER, SUP, Till, cart, cash, ctx_at, line, quick, till};
use gayanshop_lib::db::repo::checkout::{self, NoFaults};
use gayanshop_lib::db::repo::expenses::{self, ExpenseInput};
use gayanshop_lib::db::repo::orders::{self, OrderFilter, OrderStatusFilter};
use gayanshop_lib::db::repo::void::{self, VoidInput, VoidReason, Voider};
use gayanshop_lib::domain::business_date::StoreCalendar;
use gayanshop_lib::domain::payments::PaymentMethod;
use gayanshop_lib::domain::period::{DateRange, Period};
use gayanshop_lib::error::AppError;
use gayanshop_lib::export::{self, Cell, ExportKind, ExportRequest};

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

fn cal() -> StoreCalendar {
    StoreCalendar::colombo_default()
}

/// A tiny RFC 4180 reader for the assertions (quotes, doubled quotes, CRLF).
fn parse(csv: &str) -> Vec<Vec<String>> {
    let body = csv.strip_prefix('\u{feff}').expect("BOM first");
    let (mut rows, mut row, mut cell, mut quoted, mut chars) =
        (Vec::new(), Vec::new(), String::new(), false, body.chars().peekable());
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => row.push(std::mem::take(&mut cell)),
            ('\r', false) => {
                assert_eq!(chars.next(), Some('\n'), "CRLF line ends");
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            (c, _) => cell.push(c),
        }
    }
    assert!(row.is_empty() && cell.is_empty(), "ends with CRLF");
    rows
}

/// Five sales over two days (one voided), odd names, a customer, an expense.
async fn shop() -> Till {
    let t = till().await;
    let bucket = t.stocked("Bucket, \"blue\" 10L", 45_000, &[(50, 20_000)]).await;
    let mat = t.product("=HYPERLINK(\"x\")", 10_000, false, 4_000, None).await;
    let seller = t.seller(CASHIER).await;
    for (n, (at, lines)) in [
        ("2026-11-07T09:00:00.000Z", vec![line(&bucket, 45_000, 2)]),
        ("2026-11-07T10:00:00.000Z", vec![line(&mat, 10_000, 1), quick("Door mat", 5_000, 1)]),
        ("2026-11-08T09:00:00.000Z", vec![line(&bucket, 45_000, 1)]),
        ("2026-11-08T09:30:00.000Z", vec![line(&mat, 10_000, 3)]),
        ("2026-11-08T09:40:00.000Z", vec![line(&bucket, 45_000, 1)]),
    ]
    .into_iter()
    .enumerate()
    {
        let total: i64 = lines.iter().map(|l| l.unit_price * l.qty).sum();
        let mut input = cart(&format!("x{n}"), lines, vec![cash(total, None)]);
        if n == 0 {
            input.customer_phone = Some("0771234567".into());
            input.customer_name = Some("Nimal".into());
        }
        checkout::checkout(&t.t.db, &ctx_at(&t.s, at), &seller, &input, None, &NoFaults).await.unwrap();
    }
    let last: String = sqlx::query_scalar("SELECT id FROM orders ORDER BY completed_at DESC LIMIT 1")
        .fetch_one(t.pool())
        .await
        .unwrap();
    let sup = t.seller(SUP).await;
    let input = VoidInput { order_id: last, reason: VoidReason::DuplicateScan, note: None, approval_id: None };
    void::void_order(&t.t.db, &t.ctx(), &Voider { staff_id: SUP.into(), perms: sup.perms }, &input, None, &NoFaults)
        .await
        .unwrap();
    let rent: String =
        sqlx::query_scalar("SELECT id FROM expense_categories WHERE name = 'Rent'").fetch_one(t.pool()).await.unwrap();
    let e = ExpenseInput {
        category_id: rent,
        amount: 4_500_050,
        business_date: Some("2026-11-01".into()),
        payment_method: PaymentMethod::BankTransfer,
        paid_from_drawer: false,
        payee: Some("-Landlord".into()),
        note: Some("Line one\nline two".into()),
    };
    expenses::create(&t.t.db, &t.ctx(), &t.owner(), &e).await.unwrap();
    t
}

#[test]
fn csv_is_excel_safe() {
    assert_eq!(export::money(123_456), "1234.56");
    assert_eq!(export::money(-5), "-0.05");
    assert_eq!(export::money(0), "0.00");
    let csv = export::render(
        &["name", "amount"],
        &[
            vec![Cell::Text("Bucket, \"blue\"".into()), Cell::Money(-1_250)],
            vec![Cell::Text("=SUM(A1)".into()), Cell::Int(-3)],
            vec![Cell::Text("+94 77".into()), Cell::Empty],
            vec![Cell::Text("@cmd".into()), Cell::Text("two\nlines".into())],
            vec![Cell::Text("සිංහල".into()), Cell::Text("-x".into())],
        ],
    );
    assert!(csv.starts_with('\u{feff}'), "UTF-8 BOM so Excel reads Sinhala");
    assert_eq!(
        parse(&csv),
        vec![
            vec!["name", "amount"],
            vec!["Bucket, \"blue\"", "-12.50"],
            vec!["'=SUM(A1)", "-3"],
            vec!["'+94 77", ""],
            vec!["'@cmd", "two\nlines"],
            vec!["සිංහල", "'-x"],
        ],
        "quoted, doubled quotes, formulas neutralised, numbers untouched"
    );
}

#[tokio::test]
async fn every_export_kind_matches_the_database() {
    let t = shop().await;
    let pool = t.pool();
    let all = Period::AllTime;
    let rows = |kind, with_cost| {
        let pool = pool.clone();
        let store = t.s.store_id.clone();
        let p = all.clone();
        async move { export::build(&pool, &store, &cal(), kind, &p, with_cost).await.unwrap().0 }
    };
    let o = rows(ExportKind::Orders, true).await;
    assert_eq!(o.rows.len(), 5, "completed and voided");
    let status = o.header.iter().position(|h| *h == "status").unwrap();
    assert_eq!(o.rows.iter().filter(|r| r[status] == Cell::Text("voided".into())).count(), 1);
    let reason = o.header.iter().position(|h| *h == "void_reason").unwrap();
    assert!(o.rows.iter().any(|r| r[reason] == Cell::Text("DUPLICATE_SCAN".into())));
    let payments = o.header.iter().position(|h| *h == "payments").unwrap();
    assert_eq!(o.rows[0][payments], Cell::Text("cash 900.00".into()));
    assert!(o.header.contains(&"cost"));
    assert!(!rows(ExportKind::Orders, false).await.header.contains(&"cost"), "no VIEW_COST ⇒ no cost column");

    let l = rows(ExportKind::OrderLines, true).await;
    assert_eq!(l.rows.len(), 6);
    let lines_no_cost = rows(ExportKind::OrderLines, false).await;
    assert!(!lines_no_cost.header.contains(&"cost") && !lines_no_cost.header.contains(&"cost_known"));
    assert_eq!(l.rows.iter().map(|r| r.len()).collect::<std::collections::BTreeSet<_>>().len(), 1, "rectangular");

    let p = rows(ExportKind::Products, true).await;
    assert_eq!(p.rows.len(), 2);
    let on_hand = p.header.iter().position(|h| *h == "on_hand").unwrap();
    let name = p.header.iter().position(|h| *h == "name").unwrap();
    let bucket = p.rows.iter().find(|r| r[name] == Cell::Text("Bucket, \"blue\" 10L".into())).unwrap();
    assert_eq!(bucket[on_hand], Cell::Int(50 - 2 - 1), "the voided sale's unit went back");
    assert!(!rows(ExportKind::Products, false).await.header.contains(&"default_cost"));

    let c = rows(ExportKind::Customers, false).await;
    assert_eq!(
        c.rows,
        vec![vec![
            Cell::Text("+94771234567".into()),
            Cell::Text("Nimal".into()),
            Cell::Empty,
            Cell::Text("2026-11-07T09:00:00.000Z".into()),
            Cell::Int(1),
            Cell::Money(90_000),
            Cell::Text("2026-11-07".into()),
            Cell::Text("2026-11-07".into()),
        ]]
    );

    let e = rows(ExportKind::Expenses, false).await;
    assert_eq!(e.rows.len(), 1);
    assert_eq!(e.rows[0][2], Cell::Money(4_500_050));

    let m = rows(ExportKind::StockMovements, true).await;
    let raw: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stock_movements").fetch_one(pool).await.unwrap();
    assert_eq!(i64::try_from(m.rows.len()).unwrap(), raw);
    assert!(!rows(ExportKind::StockMovements, false).await.header.contains(&"unit_cost"));

    let s = rows(ExportKind::SalesByProduct, true).await;
    let net: i64 = s
        .rows
        .iter()
        .map(|r| match r[2] {
            Cell::Money(v) => v,
            _ => 0,
        })
        .sum();
    let raw: i64 = sqlx::query_scalar(
        "SELECT SUM(oi.line_net) FROM order_items oi JOIN orders o ON o.id = oi.order_id WHERE o.status = 'completed'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(net, raw);
    assert!(s.rows.iter().any(|r| r[0] == Cell::Text("Quick sales".into())));

    // A period narrows the dated kinds.
    let day = Period::Day { date: "2026-11-07".into() };
    let (d, range) = export::build(pool, &t.s.store_id, &cal(), ExportKind::Orders, &day, false).await.unwrap();
    assert_eq!((d.rows.len(), range.from.as_deref()), (2, Some("2026-11-07")));
    let (_, snap) = export::build(pool, &t.s.store_id, &cal(), ExportKind::Products, &day, false).await.unwrap();
    assert_eq!(snap, DateRange { from: None, to: None }, "a product list is a snapshot");
}

#[tokio::test]
async fn an_export_is_written_atomically_and_audited_without_personal_data() {
    let t = shop().await;
    let dir = tempfile::tempdir().unwrap();
    let at = chrono::DateTime::parse_from_rfc3339("2026-11-08T04:00:00Z").unwrap().with_timezone(&chrono::Utc);
    let period = Period::Month { year: 2026, month: 11 };
    let req = ExportRequest { kind: ExportKind::Orders, period: &period, with_cost: false, dir: dir.path(), at };
    let out = export::export(&t.t.db, &t.ctx(), &t.owner(), &cal(), &req).await.unwrap();
    assert_eq!(out.rows, 5);
    let path = std::path::PathBuf::from(&out.path);
    assert_eq!(
        path.file_name().unwrap().to_str().unwrap(),
        "gayanshop-orders-2026-11-01_2026-11-30-T01-20261108-093000.csv",
        "store-local time in the name"
    );
    let entries: Vec<String> =
        std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(entries.len(), 1, "no .partial left behind");
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed = parse(&content);
    assert_eq!(parsed.len(), 6);
    assert_eq!(parsed[0][0], "receipt_no");

    let customers =
        ExportRequest { kind: ExportKind::Customers, period: &Period::AllTime, with_cost: false, dir: dir.path(), at };
    export::export(&t.t.db, &t.ctx(), &t.owner(), &cal(), &customers).await.unwrap();
    let audits: Vec<String> =
        sqlx::query_scalar("SELECT metadata FROM audit_logs WHERE action = 'DATA_EXPORT' ORDER BY device_seq")
            .fetch_all(t.pool())
            .await
            .unwrap();
    assert_eq!(audits.len(), 2);
    assert!(audits[0].contains("\"rows\":5") && audits[0].contains("2026-11-01"), "{}", audits[0]);
    assert!(audits[1].contains("customers") && !audits[1].contains("771234567") && !audits[1].contains("Nimal"));

    let missing = dir.path().join("unplugged-usb");
    let bad =
        ExportRequest { kind: ExportKind::Products, period: &Period::AllTime, with_cost: false, dir: &missing, at };
    assert_eq!(
        code(export::export(&t.t.db, &t.ctx(), &t.owner(), &cal(), &bad).await.unwrap_err()),
        "EXPORT_DIR_INVALID"
    );
    assert_eq!(
        t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'DATA_EXPORT'").await,
        2,
        "no audit for a failed export"
    );
    let bad_period = Period::Custom { from: "2026-11-09".into(), to: "2026-11-01".into() };
    let bad = ExportRequest { kind: ExportKind::Orders, period: &bad_period, with_cost: false, dir: dir.path(), at };
    assert_eq!(code(export::export(&t.t.db, &t.ctx(), &t.owner(), &cal(), &bad).await.unwrap_err()), "PERIOD_INVALID");
}

#[tokio::test]
async fn order_history_pages_newest_first_with_filters() {
    let t = shop().await;
    let pool = t.pool();
    let filter = |period: Period, status, receipt: Option<&str>| OrderFilter {
        period,
        device_id: None,
        status,
        receipt: receipt.map(str::to_owned),
        phone: None,
        payment: None,
        kind: None,
    };
    let all = filter(Period::AllTime, None, None);
    let first = orders::history(pool, &t.s.store_id, &cal(), &all, None, 2).await.unwrap();
    assert_eq!(first.rows.len(), 2);
    assert_eq!(first.rows[0].status, "voided", "newest first");
    let mut seen: Vec<String> = first.rows.iter().map(|r| r.receipt_no.clone()).collect();
    let mut cursor = first.next_cursor;
    while let Some(c) = cursor {
        let page = orders::history(pool, &t.s.store_id, &cal(), &all, Some(&c), 2).await.unwrap();
        seen.extend(page.rows.iter().map(|r| r.receipt_no.clone()));
        cursor = page.next_cursor;
    }
    assert_eq!(seen.len(), 5);
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 5, "no row twice across pages");

    let voided = orders::history(
        pool,
        &t.s.store_id,
        &cal(),
        &filter(Period::AllTime, Some(OrderStatusFilter::Voided), None),
        None,
        50,
    )
    .await
    .unwrap();
    assert_eq!((voided.rows.len(), voided.rows[0].void_reason.as_deref()), (1, Some("DUPLICATE_SCAN")));
    let day = orders::history(
        pool,
        &t.s.store_id,
        &cal(),
        &filter(Period::Day { date: "2026-11-07".into() }, None, None),
        None,
        50,
    )
    .await
    .unwrap();
    assert_eq!(day.rows.len(), 2);
    assert!(day.rows.iter().any(|r| r.has_customer));
    let receipt = seen[2].clone();
    let one = orders::history(
        pool,
        &t.s.store_id,
        &cal(),
        &filter(Period::AllTime, None, Some(&receipt.to_lowercase())),
        None,
        50,
    )
    .await
    .unwrap();
    assert_eq!(one.rows.iter().map(|r| r.receipt_no.clone()).collect::<Vec<_>>(), vec![receipt]);
    let prefix = orders::history(pool, &t.s.store_id, &cal(), &filter(Period::AllTime, None, Some("A")), None, 50)
        .await
        .unwrap();
    assert_eq!(prefix.rows.len(), 5);
    let other = OrderFilter { device_id: Some("another-counter".into()), ..all.clone() };
    assert!(orders::history(pool, &t.s.store_id, &cal(), &other, None, 50).await.unwrap().rows.is_empty());
}
