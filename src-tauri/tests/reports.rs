//! Block 2.7 — reports engine (doc §5): on randomised data (two counters, three months across a year
//! boundary, voids, discounts, split payments, customers, quick sales, expenses, damage and a batch with
//! allocation variance) the rollups equal raw sums over the base rows; `rebuild_derived` reproduces every
//! derived table exactly; monthly == Σ daily (and week, year, custom and all time likewise); the P&L
//! identities hold; sales by product / category / batch and voids by reason / approver reconcile.

mod common;

use common::{SUP, Till, ctx_at, till};
use gayanshop_lib::db::repo::batches::{self, AllocationMethod, BatchInput, BatchItemInput};
use gayanshop_lib::db::repo::checkout::{self, CheckoutLine, NoFaults, PaymentLine};
use gayanshop_lib::db::repo::expenses::{self, ExpenseInput};
use gayanshop_lib::db::repo::inventory::{self, DamageInput, DamageReason};
use gayanshop_lib::db::repo::reports::{self, FinancialSummary, SalesDimension};
use gayanshop_lib::db::repo::void::{self, VoidInput, VoidReason, Voider};
use gayanshop_lib::db::repo::{WriteCtx, cash};
use gayanshop_lib::domain::business_date::{StoreCalendar, format_date, parse_date};
use gayanshop_lib::domain::payments::PaymentMethod;
use gayanshop_lib::domain::period::{DateRange, Granularity, Period};
use sqlx::SqlitePool;

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> i64 {
        i64::try_from(self.next() % n).unwrap()
    }
}

const FIRST: &str = "2025-12-10";
const DAYS: i64 = 75; // → 2026-02-22
const DEV2: &str = "dev-two";

fn day(offset: i64) -> String {
    format_date(parse_date(FIRST).unwrap() + chrono::Duration::days(offset))
}

fn ctx_on(t: &Till, date: &str, device: Option<&str>, n: i64) -> WriteCtx {
    let mut c = ctx_at(&t.s, &format!("{date}T{:02}:{:02}:00.000Z", 4 + (n / 60) % 12, n % 60));
    if let Some(d) = device {
        c.device_id = d.into();
        c.device_code = "T02".into();
    }
    c
}

struct Data {
    t: Till,
}

/// Seeded random history. Returns the till; counter 1 orders can be voided (open session on this counter).
async fn history(seed: u64) -> Data {
    let t = till().await;
    let mut rng = Rng(seed);
    let owner = t.owner();
    // Counter 2 has its own open session.
    let mut c2 = ctx_on(&t, FIRST, Some(DEV2), 0);
    c2.business_date = FIRST.into();
    cash::open(&t.t.db, &c2, &owner, 0).await.unwrap();

    let tracked = [
        t.stocked("Bucket", 45_000, &[(20, 20_000), (500, 22_000)]).await,
        t.stocked("Basin", 30_000, &[(500, 12_000)]).await,
        t.stocked("Mug", 15_000, &[(5, 6_000), (500, 7_000)]).await,
    ];
    let untracked =
        [t.product("Gift wrap", 5_000, false, 1_000, None).await, t.product("Carry bag", 2_000, false, 0, None).await];
    // A batch with an allocation variance received inside the period (counts in COGS on its received date).
    let chair = t.product("Chair", 90_000, true, 0, None).await;
    let input = BatchInput {
        name: "Chairs".into(),
        supplier_name: None,
        supplier_ref: None,
        purchased_at: None,
        total_cost: 100_000,
        extra_costs: 5_000,
        allocation_method: Some(AllocationMethod::Manual),
        notes: None,
    };
    let recv_ctx = ctx_on(&t, &day(20), None, 1);
    let b = batches::create(&t.t.db, &recv_ctx, &owner, &input).await.unwrap();
    let item = BatchItemInput { product_id: chair.clone(), qty: 10, unit_cost: 9_000, retail_price: Some(90_000) };
    batches::set_item(&t.t.db, &recv_ctx, &b.batch.id, &item).await.unwrap();
    batches::receive(&t.t.db, &recv_ctx, &owner, &b.batch.id).await.unwrap();

    let seller = t.seller(&t.s.owner_staff_id).await;
    let phones = ["0771234567", "0712345678", "0759876543", "0112223334"];
    let quick_names = ["Door mat", "Hanger set", "Torch"];
    let mut mine = Vec::new();
    for n in 0..140 {
        let date = day(rng.below(u64::try_from(DAYS).unwrap()));
        let dev2 = rng.below(3) == 0;
        let ctx = ctx_on(&t, &date, dev2.then_some(DEV2), n);
        let mut lines = Vec::new();
        let mut used_quick = Vec::new();
        for _ in 0..=rng.below(3) {
            let qty = 1 + rng.below(3);
            let (product, name, price) = match rng.below(4) {
                0 | 1 => {
                    let p = &tracked[usize::try_from(rng.below(3)).unwrap()];
                    let price = [45_000, 30_000, 15_000][tracked.iter().position(|x| x == p).unwrap()];
                    (Some(p.clone()), None, price)
                }
                2 => {
                    let i = usize::try_from(rng.below(2)).unwrap();
                    (Some(untracked[i].clone()), None, [5_000, 2_000][i])
                }
                _ => {
                    let name = quick_names[usize::try_from(rng.below(3)).unwrap()];
                    if used_quick.contains(&name) {
                        continue;
                    }
                    used_quick.push(name);
                    (None, Some(name.to_owned()), 1_000 * (5 + rng.below(20)))
                }
            };
            if product.is_some() && lines.iter().any(|l: &CheckoutLine| l.product_id == product) {
                continue;
            }
            let gross = price * qty;
            lines.push(CheckoutLine {
                product_id: product,
                name,
                unit_price: price,
                qty,
                line_discount: gross * rng.below(6) / 100,
                unit_cost: None,
                override_reason: None,
            });
        }
        if lines.is_empty() {
            continue;
        }
        let subtotal: i64 = lines.iter().map(|l| l.unit_price * l.qty - l.line_discount).sum();
        let bill_discount = if rng.below(5) == 0 { subtotal / 50 } else { 0 };
        let total = subtotal - bill_discount;
        let payments = if rng.below(3) == 0 && total > 1 {
            let card = total / 2;
            vec![
                PaymentLine { method: PaymentMethod::Card, amount: card, tendered: None, reference: None },
                PaymentLine { method: PaymentMethod::Cash, amount: total - card, tendered: None, reference: None },
            ]
        } else {
            vec![PaymentLine { method: PaymentMethod::Cash, amount: total, tendered: None, reference: None }]
        };
        let mut input = common::cart(&format!("tok{n}"), lines, payments);
        input.bill_discount = bill_discount;
        if rng.below(2) == 0 {
            input.customer_phone = Some(phones[usize::try_from(rng.below(4)).unwrap()].to_owned());
        }
        let r = checkout::checkout(&t.t.db, &ctx, &seller, &input, None, &NoFaults).await.unwrap();
        if !dev2 {
            mine.push((r.order_id, ctx));
        }
    }
    // Void about one in eight of this counter's sales, with mixed reasons.
    let sup = t.seller(SUP).await;
    let reasons = [VoidReason::WrongItem, VoidReason::DuplicateScan, VoidReason::CustomerCancelled];
    for (i, (order_id, ctx)) in mine.iter().enumerate().filter(|(i, _)| i % 8 == 3) {
        let input = VoidInput { order_id: order_id.clone(), reason: reasons[i % 3], note: None, approval_id: None };
        void::void_order(&t.t.db, ctx, &Voider { staff_id: SUP.into(), perms: sup.perms }, &input, None, &NoFaults)
            .await
            .unwrap();
    }
    // Expenses and damage on random days.
    let cats: Vec<String> =
        sqlx::query_scalar("SELECT id FROM expense_categories ORDER BY name").fetch_all(t.pool()).await.unwrap();
    for n in 0..18 {
        let ctx = ctx_on(&t, &day(rng.below(u64::try_from(DAYS).unwrap())), None, 500 + n);
        let input = ExpenseInput {
            category_id: cats[usize::try_from(rng.below(8)).unwrap()].clone(),
            amount: 1_000 * (1 + rng.below(500)),
            business_date: None,
            payment_method: PaymentMethod::BankTransfer,
            paid_from_drawer: false,
            payee: None,
            note: None,
        };
        expenses::create(&t.t.db, &ctx, &owner, &input).await.unwrap();
    }
    for n in 0..4 {
        let ctx = ctx_on(&t, &day(rng.below(u64::try_from(DAYS).unwrap())), None, 600 + n);
        let input = DamageInput {
            product_id: tracked[usize::try_from(n % 3).unwrap()].clone(),
            qty: 1 + rng.below(2),
            reason: DamageReason::Broken,
            note: None,
        };
        inventory::damage(&t.t.db, &ctx, &owner, &input).await.unwrap();
    }
    Data { t }
}

/// Every row of a table as text, sorted: equal snapshots ⇒ identical contents.
async fn snapshot(pool: &SqlitePool, table: &str, filter: &str) -> Vec<String> {
    let cols: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info(?1)").bind(table).fetch_all(pool).await.unwrap();
    let expr = cols.iter().map(|c| format!("quote(\"{c}\")")).collect::<Vec<_>>().join(" || '|' || ");
    sqlx::query_scalar(&format!("SELECT {expr} FROM {table} {filter} ORDER BY 1")).fetch_all(pool).await.unwrap()
}

const DERIVED: [(&str, &str); 7] = [
    ("daily_sales_summary", ""),
    // A payment method whose only sale was voided leaves an all-zero row behind the trigger; a rebuild drops it.
    ("daily_payment_summary", "WHERE amount <> 0 OR txn_count <> 0"),
    ("customer_stats", ""),
    ("quick_sale_stats", ""),
    ("stock_levels", ""),
    ("batch_item_stock", ""),
    ("products_fts", ""),
];

fn cal() -> StoreCalendar {
    StoreCalendar::colombo_default()
}

async fn summary(pool: &SqlitePool, p: Period, device: Option<&str>) -> FinancialSummary {
    reports::report(pool, &cal(), &p, device, false).await.unwrap().current
}

/// The additive fields of a summary (everything except ratios and the range).
fn additive(s: &FinancialSummary) -> Vec<i64> {
    vec![
        s.sales_count,
        s.void_count,
        s.void_value,
        s.gross_sales,
        s.discounts,
        s.net_sales,
        s.tax_collected,
        s.customer_orders,
        s.quick_sale_net,
        s.unknown_cost_net,
        s.sales_cost.unwrap(),
        s.allocation_variance.unwrap(),
        s.cogs.unwrap(),
        s.gross_profit.unwrap(),
        s.stock_write_offs.unwrap(),
        s.operating_expenses.unwrap(),
        s.net_profit.unwrap(),
        s.payments.iter().map(|p| p.amount).sum(),
        s.payments.iter().map(|p| p.count).sum(),
    ]
}

fn sum(parts: &[FinancialSummary]) -> Vec<i64> {
    parts.iter().map(additive).fold(vec![0; 19], |acc, v| acc.iter().zip(v).map(|(a, b)| a + b).collect())
}

async fn days(pool: &SqlitePool, from: &str, to: &str, device: Option<&str>) -> Vec<FinancialSummary> {
    let (mut d, end) = (parse_date(from).unwrap(), parse_date(to).unwrap());
    let mut out = Vec::new();
    while d <= end {
        out.push(summary(pool, Period::Day { date: format_date(d) }, device).await);
        d += chrono::Duration::days(1);
    }
    out
}

#[tokio::test]
async fn rollups_equal_raw_sums_over_the_base_rows() {
    let d = history(0x5EED_0001).await;
    let pool = d.t.pool();
    assert!(d.t.count("SELECT COUNT(*) FROM orders WHERE status = 'voided'").await >= 5, "voids exercised");
    assert!(d.t.count("SELECT COUNT(DISTINCT device_id) FROM orders").await == 2, "two counters");
    // Per day and counter: the rollup equals item-level sums (independent of the order header totals).
    let mismatches: i64 = sqlx::query_scalar(
        "WITH raw AS (
           SELECT o.business_date AS d, o.device_id AS dev, COUNT(DISTINCT o.id) AS n,
                  SUM(oi.line_gross) AS gross, SUM(oi.line_discount + oi.order_discount_share) AS disc,
                  SUM(oi.line_net) AS net, SUM(oi.tax_amount) AS tax, SUM(oi.cost_total) AS cost,
                  SUM(CASE WHEN oi.is_custom = 1 THEN oi.line_net ELSE 0 END) AS qs,
                  SUM(CASE WHEN oi.cost_known = 0 THEN oi.line_net ELSE 0 END) AS unk
           FROM orders o JOIN order_items oi ON oi.order_id = o.id
           WHERE o.status = 'completed' GROUP BY o.business_date, o.device_id)
         SELECT COUNT(*) FROM raw r LEFT JOIN daily_sales_summary s ON s.business_date = r.d AND s.device_id = r.dev
         WHERE s.sales_count IS NOT r.n OR s.gross_sales IS NOT r.gross OR s.discounts IS NOT r.disc
            OR s.net_sales IS NOT r.net OR s.tax_total IS NOT r.tax OR s.cogs IS NOT r.cost
            OR s.quick_sale_net IS NOT r.qs OR s.unknown_cost_net IS NOT r.unk",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(mismatches, 0, "daily_sales_summary == Σ order items");
    let stray: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM daily_sales_summary s WHERE s.sales_count <> 0 AND NOT EXISTS
           (SELECT 1 FROM orders o WHERE o.status = 'completed' AND o.business_date = s.business_date
              AND o.device_id = s.device_id)",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(stray, 0);
    let voids: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM orders WHERE status = 'voided') - SUM(void_count),
                (SELECT COALESCE(SUM(grand_total), 0) FROM orders WHERE status = 'voided') - SUM(void_value)
         FROM daily_sales_summary",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(voids, (0, 0), "voids counted for control, by value");
    let pay: i64 = sqlx::query_scalar(
        "WITH raw AS (SELECT o.business_date d, o.device_id dev, p.method m, SUM(p.amount) a, COUNT(*) n
                      FROM order_payments p JOIN orders o ON o.id = p.order_id WHERE o.status = 'completed'
                      GROUP BY 1, 2, 3)
         SELECT (SELECT COUNT(*) FROM raw r LEFT JOIN daily_payment_summary s
                   ON s.business_date = r.d AND s.device_id = r.dev AND s.method = r.m
                 WHERE s.amount IS NOT r.a OR s.txn_count IS NOT r.n)
              + (SELECT COUNT(*) FROM daily_payment_summary s WHERE (s.amount <> 0 OR s.txn_count <> 0)
                   AND NOT EXISTS (SELECT 1 FROM raw r WHERE r.d = s.business_date AND r.dev = s.device_id AND r.m = s.method))",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(pay, 0, "daily_payment_summary == Σ payments of completed orders");
    // The all-time summary equals raw totals too.
    let all = summary(pool, Period::AllTime, None).await;
    let raw_net: i64 = sqlx::query_scalar(
        "SELECT SUM(oi.line_net) FROM order_items oi JOIN orders o ON o.id = oi.order_id WHERE o.status = 'completed'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(all.net_sales, raw_net);
    assert_eq!(
        all.payments.iter().map(|p| p.amount).sum::<i64>(),
        all.gross_sales - all.discounts,
        "prices include tax"
    );
}

#[tokio::test]
async fn rebuild_derived_reproduces_every_derived_table() {
    let d = history(0x5EED_0002).await;
    let pool = d.t.pool();
    let mut before = Vec::new();
    for (table, filter) in DERIVED {
        before.push(snapshot(pool, table, filter).await);
    }
    // Damage the caches the way a bad restore or a bug would.
    for sql in [
        "UPDATE daily_sales_summary SET net_sales = 0, sales_count = sales_count + 7",
        "DELETE FROM daily_payment_summary WHERE method = 'card'",
        "UPDATE customer_stats SET total_spend = 1",
        "DELETE FROM quick_sale_stats",
        "UPDATE stock_levels SET on_hand = -999",
        "UPDATE batch_item_stock SET sold_cost = 0",
        "DELETE FROM products_fts",
    ] {
        common::exec(pool, sql).await;
    }
    let r = reports::rebuild_derived(&d.t.t.db, &d.t.ctx(), &d.t.owner()).await.unwrap();
    assert!(r.daily_sales_summary > 0 && r.batch_item_stock > 0 && r.products_fts > 0);
    for ((table, filter), want) in DERIVED.iter().zip(before) {
        assert_eq!(snapshot(pool, table, filter).await, want, "{table} rebuilt exactly");
    }
    assert_eq!(
        d.t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'SETTINGS_CHANGE' AND entity_id = 'rebuild_derived'")
            .await,
        1
    );
    // Search works again from the rebuilt index.
    let hits = gayanshop_lib::db::repo::products::search(pool, &d.t.s.store_id, "buck", 20).await.unwrap();
    assert!(hits.iter().any(|p| p.name == "Bucket"));
}

#[tokio::test]
async fn months_years_weeks_and_ranges_equal_the_sum_of_their_days() {
    let d = history(0x5EED_0003).await;
    let pool = d.t.pool();
    for (year, month, from, to) in [
        (2025, 12, "2025-12-01", "2025-12-31"),
        (2026, 1, "2026-01-01", "2026-01-31"),
        (2026, 2, "2026-02-01", "2026-02-28"),
    ] {
        let m = summary(pool, Period::Month { year, month }, None).await;
        assert_eq!(additive(&m), sum(&days(pool, from, to, None).await), "{year}-{month} == Σ daily");
        let m2 = summary(pool, Period::Month { year, month }, Some(DEV2)).await;
        assert_eq!(additive(&m2)[..10], sum(&days(pool, from, to, Some(DEV2)).await)[..10], "counter 2: sales metrics");
    }
    let y2025 = summary(pool, Period::Year { year: 2025 }, None).await;
    let y2026 = summary(pool, Period::Year { year: 2026 }, None).await;
    let months: Vec<FinancialSummary> = vec![
        summary(pool, Period::Month { year: 2025, month: 12 }, None).await,
        summary(pool, Period::Month { year: 2026, month: 1 }, None).await,
        summary(pool, Period::Month { year: 2026, month: 2 }, None).await,
    ];
    assert_eq!(additive(&y2025), additive(&months[0]), "2025 holds only December");
    assert_eq!(additive(&y2026), sum(&months[1..]));
    let all = summary(pool, Period::AllTime, None).await;
    assert_eq!(additive(&all), sum(&[y2025, y2026]), "all time == Σ years");
    let week = summary(pool, Period::Week { any_date: "2026-01-01".into() }, None).await;
    assert_eq!(week.range, DateRange { from: Some("2025-12-29".into()), to: Some("2026-01-04".into()) });
    assert_eq!(additive(&week), sum(&days(pool, "2025-12-29", "2026-01-04", None).await), "a week across the new year");
    let custom = summary(pool, Period::Custom { from: "2025-12-20".into(), to: "2026-01-10".into() }, None).await;
    assert_eq!(additive(&custom), sum(&days(pool, "2025-12-20", "2026-01-10", None).await));
    // Counters partition the store's sales metrics (expenses, write-offs and variance are store-wide).
    let c1 = summary(pool, Period::AllTime, Some(&d.t.s.device_id)).await;
    let c2 = summary(pool, Period::AllTime, Some(DEV2)).await;
    for i in 0..10 {
        assert_eq!(additive(&all)[i], additive(&c1)[i] + additive(&c2)[i], "field {i}");
    }
    assert_eq!(c2.operating_expenses, all.operating_expenses, "store-wide on any counter");
    // Trend buckets add up to the period too.
    for g in [Granularity::Day, Granularity::Week, Granularity::Month] {
        let pts = reports::trend(pool, &cal(), &Period::AllTime, None, g).await.unwrap();
        assert_eq!(pts.iter().map(|p| p.net_sales).sum::<i64>(), all.net_sales, "{g:?}");
        assert_eq!(pts.iter().map(|p| p.expenses.unwrap()).sum::<i64>(), all.operating_expenses.unwrap());
        assert_eq!(pts.iter().map(|p| p.sales_cost.unwrap()).sum::<i64>(), all.sales_cost.unwrap());
        assert!(pts.windows(2).all(|w| w[0].bucket < w[1].bucket), "oldest first");
    }
    let monthly = reports::trend(pool, &cal(), &Period::AllTime, None, Granularity::Month).await.unwrap();
    assert_eq!(monthly.iter().map(|p| p.bucket.as_str()).collect::<Vec<_>>(), vec!["2025-12", "2026-01", "2026-02"]);
}

#[tokio::test]
async fn the_pnl_identities_hold_and_match_raw_costs() {
    let d = history(0x5EED_0004).await;
    let pool = d.t.pool();
    let s = summary(pool, Period::AllTime, None).await;
    let (gp, cogs) = (s.gross_profit.unwrap(), s.cogs.unwrap());
    assert_eq!(cogs, s.sales_cost.unwrap() + s.allocation_variance.unwrap());
    assert_eq!(s.allocation_variance, Some(105_000 - 90_000), "the chair batch's variance");
    assert_eq!(gp, s.net_sales - cogs);
    assert_eq!(s.net_profit.unwrap(), gp - s.stock_write_offs.unwrap() - s.operating_expenses.unwrap());
    let opex: i64 =
        sqlx::query_scalar("SELECT SUM(amount) FROM expenses WHERE deleted_at IS NULL").fetch_one(pool).await.unwrap();
    assert_eq!(s.operating_expenses, Some(opex));
    let wo: i64 = sqlx::query_scalar("SELECT SUM(-qty_delta * unit_cost) FROM stock_movements WHERE reason = 'damage'")
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(wo > 0);
    assert_eq!(s.stock_write_offs, Some(wo));
    assert_eq!(s.gross_margin_bp, Some(gp * 10_000 / s.net_sales));
    assert_eq!(s.customer_attached_bp, s.customer_orders * 10_000 / s.sales_count);
    assert!(s.quick_sale_net > 0 && s.unknown_cost_net >= s.quick_sale_net, "quick sales have unknown cost");
    // The variance lands on its received date only.
    let recv = summary(pool, Period::Day { date: day(20) }, None).await;
    assert_eq!(recv.allocation_variance, Some(15_000));
    // Sales-only viewers see no costs.
    let masked = s.clone().without_costs();
    assert_eq!((masked.cogs, masked.net_profit, masked.operating_expenses), (None, None, None));
    assert_eq!(masked.net_sales, s.net_sales);
    // The previous equivalent period comes back on request.
    let r = reports::report(pool, &cal(), &Period::Month { year: 2026, month: 1 }, None, true).await.unwrap();
    let prev = r.previous.unwrap();
    assert_eq!(prev.range.from.as_deref(), Some("2025-12-01"));
    assert_eq!(additive(&prev), additive(&summary(pool, Period::Month { year: 2025, month: 12 }, None).await));
    assert!(reports::report(pool, &cal(), &Period::AllTime, None, true).await.unwrap().previous.is_none());
}

#[tokio::test]
async fn sales_by_dimension_and_voids_reconcile() {
    let d = history(0x5EED_0005).await;
    let pool = d.t.pool();
    for p in [Period::AllTime, Period::Month { year: 2026, month: 1 }] {
        let s = summary(pool, p.clone(), None).await;
        for dim in [SalesDimension::Product, SalesDimension::Category, SalesDimension::Batch] {
            let rows = reports::sales_by(pool, &cal(), &p, None, dim).await.unwrap();
            assert_eq!(rows.iter().map(|r| r.net).sum::<i64>(), s.net_sales, "{dim:?} revenue");
            assert_eq!(rows.iter().map(|r| r.cost.unwrap()).sum::<i64>(), s.sales_cost.unwrap(), "{dim:?} cost");
            assert!(rows.windows(2).all(|w| w[0].net >= w[1].net), "{dim:?} by revenue");
        }
        let c2 = reports::sales_by(pool, &cal(), &p, Some(DEV2), SalesDimension::Product).await.unwrap();
        assert_eq!(c2.iter().map(|r| r.net).sum::<i64>(), summary(pool, p.clone(), Some(DEV2)).await.net_sales);
    }
    let by_product = reports::sales_by(pool, &cal(), &Period::AllTime, None, SalesDimension::Product).await.unwrap();
    let quick = by_product.iter().find(|r| r.id.is_none()).unwrap();
    assert_eq!(quick.net, summary(pool, Period::AllTime, None).await.quick_sale_net, "quick sales are one row");
    let by_batch = reports::sales_by(pool, &cal(), &Period::AllTime, None, SalesDimension::Batch).await.unwrap();
    assert!(by_batch.iter().any(|r| r.id.is_some()), "tracked sales reach their layers' batches");

    let s = summary(pool, Period::AllTime, None).await;
    let v = reports::voids(pool, &cal(), &Period::AllTime, None).await.unwrap();
    assert_eq!((v.count, v.value), (s.void_count, s.void_value));
    assert_eq!(v.by_reason.iter().map(|g| g.count).sum::<i64>(), v.count);
    assert!(v.by_reason.len() >= 2, "mixed reasons");
    assert_eq!(v.by_approver.len(), 1);
    assert_eq!(
        (v.by_approver[0].key.as_str(), v.by_approver[0].count),
        (SUP, v.count),
        "the supervisor voided them all"
    );
    assert_eq!(
        reports::voids(pool, &cal(), &Period::AllTime, Some(DEV2)).await.unwrap().count,
        0,
        "counter 2 voided nothing"
    );

    let devices = reports::devices(pool).await.unwrap();
    assert_eq!(devices.iter().map(|d| d.device_code.as_str()).collect::<Vec<_>>(), vec!["T01", "T02"]);
}

#[tokio::test]
async fn trends_are_dense_for_charts_and_sell_through_matches_the_layers() {
    let d = history(0x5EED_0006).await;
    let pool = d.t.pool();
    let jan = Period::Month { year: 2026, month: 1 };
    let s = reports::trend_series(pool, &cal(), &jan, None, Granularity::Day, true).await.unwrap();
    assert_eq!(s.current.len(), 31, "one point per day, empty days as zeros");
    assert_eq!(s.current[0].bucket, "2026-01-01");
    assert_eq!(s.current.iter().map(|p| p.net_sales).sum::<i64>(), summary(pool, jan.clone(), None).await.net_sales);
    let prev = s.previous.unwrap();
    assert_eq!((prev.len(), prev[0].bucket.as_str()), (31, "2025-12-01"), "December overlays January");
    let weeks = reports::trend_series(pool, &cal(), &jan, None, Granularity::Week, false).await.unwrap();
    assert_eq!(weeks.current.first().map(|p| p.bucket.as_str()), Some("2025-12-29"), "the week holding 1 January");
    assert!(weeks.previous.is_none());
    let all = reports::trend_series(pool, &cal(), &Period::AllTime, None, Granularity::Day, true).await.unwrap();
    assert!(
        all.previous.is_none()
            && all.current.iter().all(|p| p.sales_count > 0 || p.expenses.unwrap() > 0 || p.net_sales > 0)
    );

    let st = reports::batch_sell_through(pool, &d.t.s.store_id, 12).await.unwrap();
    let raw: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT b.code, SUM(bis.received_qty), SUM(bis.sold_qty) FROM batch_item_stock bis
         JOIN batches b ON b.id = bis.batch_id WHERE b.is_system = 0 GROUP BY b.id ORDER BY b.code",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut got: Vec<(String, i64, i64)> = st.iter().map(|b| (b.code.clone(), b.received, b.sold)).collect();
    got.sort();
    assert_eq!(got, raw);
    for b in &st {
        assert_eq!(b.received - b.sold - b.written_off, b.on_hand, "{}", b.code);
        assert_eq!(b.sell_through_bp, if b.received > 0 { b.sold * 10_000 / b.received } else { 0 });
    }
    assert!(st.iter().any(|b| b.sold > 0));
}
