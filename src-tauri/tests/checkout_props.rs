//! Block 1.4 — property tests: random carts ⇒ Rust totals == SQL seal == UI preview fixtures.
//!
//! - `random_carts_seal_with_rust_totals`: random catalog (tracked / untracked) and quick-sale lines, item and
//!   bill discounts, price overrides and split payments are sold through the real transaction. The SQL seal
//!   must accept every one (it re-derives all totals), and the stored header and lines must equal
//!   `domain::pricing` exactly.
//! - `ui_preview_fixtures_are_current`: `tests/fixtures/pricing_cases.json` holds golden carts priced by Rust;
//!   the till's TypeScript preview (Block 1.9) is tested against the same file. Regenerate with
//!   `UPDATE_FIXTURES=1 cargo test --test checkout_props ui_preview`.

mod common;

use std::path::Path;

use common::{Till, till};
use gayanshop_lib::db::repo::checkout::{self, CheckoutInput, CheckoutLine, NoFaults, PaymentLine};
use gayanshop_lib::domain::payments::PaymentMethod;
use gayanshop_lib::domain::pricing::{LineInput, price_lines};
use proptest::prelude::*;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
struct GenLine {
    kind: u8, // 0 tracked product, 1 untracked product, 2 quick sale
    price: i64,
    qty: i64,
    discount_bp: i64,
    override_bp: i64,
}

#[derive(Debug, Clone)]
struct GenCart {
    lines: Vec<GenLine>,
    bill_bp: i64,
    card_share_bp: i64,
    extra_tendered: i64,
}

fn gen_line() -> impl Strategy<Value = GenLine> {
    (0u8..3, 1i64..=500_000, 1i64..=5, 0i64..=3_000, prop_oneof![Just(0i64), 1i64..=2_000])
        .prop_map(|(kind, price, qty, discount_bp, override_bp)| GenLine { kind, price, qty, discount_bp, override_bp })
}

fn gen_cart() -> impl Strategy<Value = GenCart> {
    (prop::collection::vec(gen_line(), 1..6), 0i64..=3_000, 0i64..=10_000, 0i64..=100_000).prop_map(
        |(lines, bill_bp, card_share_bp, extra_tendered)| GenCart { lines, bill_bp, card_share_bp, extra_tendered },
    )
}

/// Build the cart (creating its products) and the expected pricing from the domain functions alone.
async fn build(t: &Till, n: usize, g: &GenCart) -> (CheckoutInput, gayanshop_lib::domain::pricing::PricedOrder) {
    let mut lines = Vec::new();
    let mut pricing = Vec::new();
    for (i, l) in g.lines.iter().enumerate() {
        let unit = if l.kind == 2 { l.price } else { l.price - l.price * l.override_bp / 10_000 };
        let gross = unit * l.qty;
        let disc = gross * l.discount_bp / 10_000;
        let product_id = match l.kind {
            2 => None,
            k => Some(t.product(&format!("P{n}-{i}"), l.price, k == 0, l.price / 3, None).await),
        };
        lines.push(CheckoutLine {
            product_id,
            name: (l.kind == 2).then(|| format!("Quick {n} {i}")),
            unit_price: unit,
            qty: l.qty,
            line_discount: disc,
            unit_cost: None,
            override_reason: None,
        });
        pricing.push(LineInput { unit_price: unit, qty: l.qty, line_discount: disc, tax_rate_bp: 0 });
    }
    let base: i64 = pricing.iter().map(|p| p.unit_price * p.qty - p.line_discount).sum();
    let bill = base * g.bill_bp / 10_000;
    let priced = price_lines(&pricing, bill, true).unwrap();
    let card = priced.grand_total * g.card_share_bp / 10_000;
    let cash_part = priced.grand_total - card;
    let mut payments = vec![PaymentLine {
        method: PaymentMethod::Cash,
        amount: cash_part,
        tendered: Some(cash_part + g.extra_tendered),
        reference: None,
    }];
    if card > 0 {
        payments.push(PaymentLine { method: PaymentMethod::Card, amount: card, tendered: None, reference: None });
    }
    let input = CheckoutInput {
        checkout_token: format!("prop-{n}"),
        lines,
        bill_discount: bill,
        payments,
        customer_phone: None,
        customer_name: None,
        note: None,
        sold_by_no: 1,
        approval_id: None,
        exchange: None,
    };
    (input, priced)
}

#[test]
fn random_carts_seal_with_rust_totals() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let t = rt.block_on(till());
    // The Owner (no limits) sells, so every generated discount/override is allowed and the seal is the judge.
    let owner_id = t.s.owner_staff_id.clone();
    let seller = rt.block_on(t.seller(&owner_id));
    let counter = std::sync::atomic::AtomicUsize::new(0);
    let config = ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() };
    proptest!(config, |(g in gen_cart())| {
        let n = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let (input, priced) = rt.block_on(build(&t, n, &g));
        let r = rt.block_on(checkout::checkout(&t.t.db, &t.ctx(), &seller, &input, None, &NoFaults));
        let r = r.map_err(|e| TestCaseError::fail(format!("checkout refused: {e:?}")))?;
        prop_assert_eq!(r.grand_total, priced.grand_total);
        prop_assert_eq!(r.change_due, g.extra_tendered);

        let header: (String, i64, i64, i64, i64, i64) = rt.block_on(
            sqlx::query_as("SELECT status, gross_sales, discount_total, net_sales, grand_total, paid_total FROM orders WHERE id = ?1")
                .bind(&r.order_id)
                .fetch_one(t.pool()),
        ).unwrap();
        prop_assert_eq!(header, ("completed".to_owned(), priced.gross_sales, priced.discount_total, priced.net_sales, priced.grand_total, priced.grand_total));

        let stored: Vec<(i64, i64, i64, i64)> = rt.block_on(
            sqlx::query_as("SELECT line_gross, line_discount, order_discount_share, line_net FROM order_items WHERE order_id = ?1 ORDER BY line_no")
                .bind(&r.order_id)
                .fetch_all(t.pool()),
        ).unwrap();
        let expected: Vec<(i64, i64, i64, i64)> =
            priced.lines.iter().map(|p| (p.line_gross, p.line_discount, p.order_discount_share, p.line_net)).collect();
        prop_assert_eq!(stored, expected);
        prop_assert_eq!(priced.lines.iter().map(|p| p.order_discount_share).sum::<i64>(), input.bill_discount);

        // Tracked lines: slices carry exactly the line's revenue and quantity.
        let uncovered: i64 = rt.block_on(sqlx::query_scalar(
            "SELECT COUNT(*) FROM order_items oi JOIN products p ON p.id = oi.product_id
             WHERE oi.order_id = ?1 AND p.track_stock = 1
               AND (oi.line_net <> (SELECT SUM(revenue_net) FROM stock_movements WHERE order_item_id = oi.id)
                 OR oi.qty <> -(SELECT SUM(qty_delta) FROM stock_movements WHERE order_item_id = oi.id))",
        ).bind(&r.order_id).fetch_one(t.pool())).unwrap();
        prop_assert_eq!(uncovered, 0);
    });
}

/// Golden carts for the UI preview (tax 0 today, plus inclusive/exclusive tax to keep both paths honest).
fn fixture_cases() -> Vec<(&'static str, Vec<LineInput>, i64, bool)> {
    let l = |unit_price: i64, qty: i64, line_discount: i64, tax_rate_bp: i64| LineInput {
        unit_price,
        qty,
        line_discount,
        tax_rate_bp,
    };
    vec![
        ("single line", vec![l(45_000, 1, 0, 0)], 0, true),
        ("qty and item discount", vec![l(12_550, 3, 1_000, 0)], 0, true),
        (
            "bill discount split by largest remainder",
            vec![l(10_001, 1, 0, 0), l(20_003, 1, 0, 0), l(30_007, 1, 0, 0)],
            3_000,
            true,
        ),
        ("tie goes to the lower line", vec![l(100, 1, 0, 0), l(100, 1, 0, 0), l(100, 1, 0, 0)], 100, true),
        ("item and bill discount together", vec![l(99_900, 2, 9_990, 0), l(4_550, 4, 0, 0)], 5_000, true),
        ("free item", vec![l(0, 1, 0, 0), l(1_000, 1, 0, 0)], 0, true),
        ("whole bill discounted", vec![l(2_500, 2, 0, 0)], 5_000, true),
        ("inclusive 18% tax", vec![l(11_800, 1, 0, 1_800), l(5_000, 3, 500, 1_800)], 1_000, true),
        ("exclusive 18% tax", vec![l(10_000, 1, 0, 1_800), l(5_000, 3, 500, 1_800)], 1_000, false),
        ("mixed rates inclusive", vec![l(10_000, 1, 0, 0), l(10_000, 1, 0, 1_500)], 333, true),
    ]
}

fn line_json(l: &LineInput) -> Value {
    json!({ "unit_price": l.unit_price, "qty": l.qty, "line_discount": l.line_discount, "tax_rate_bp": l.tax_rate_bp })
}

#[test]
fn ui_preview_fixtures_are_current() {
    let cases: Vec<Value> = fixture_cases()
        .into_iter()
        .map(|(name, lines, bill, inclusive)| {
            let p = price_lines(&lines, bill, inclusive).unwrap();
            json!({
                "name": name,
                "input": { "lines": lines.iter().map(line_json).collect::<Vec<_>>(), "bill_discount": bill, "tax_inclusive": inclusive },
                "expected": {
                    "lines": p.lines.iter().map(|x| json!({
                        "line_gross": x.line_gross, "line_discount": x.line_discount,
                        "order_discount_share": x.order_discount_share, "line_total": x.line_total,
                        "tax_amount": x.tax_amount, "line_net": x.line_net,
                    })).collect::<Vec<_>>(),
                    "gross_sales": p.gross_sales, "discount_total": p.discount_total, "tax_total": p.tax_total,
                    "net_sales": p.net_sales, "grand_total": p.grand_total,
                },
            })
        })
        .collect();
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&json!({ "generated_by": "src-tauri/tests/checkout_props.rs", "cases": cases }))
            .unwrap()
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pricing_cases.json");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(committed, text, "pricing fixtures are stale: rerun with UPDATE_FIXTURES=1 and commit the file");
}
