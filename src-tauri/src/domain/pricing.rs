//! Authoritative line and order totals (ARCHITECTURE §3.7). The UI only previews these; the SQL seal
//! trigger re-verifies the same identities.

use super::discount::{apportion, within_limit};
use super::money::{Money, mul_div_round};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineInput {
    pub unit_price: Money,
    pub qty: i64,
    pub line_discount: Money,
    pub tax_rate_bp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedLine {
    pub line_gross: Money,
    pub line_discount: Money,
    pub order_discount_share: Money,
    pub line_total: Money,
    pub tax_amount: Money,
    pub line_net: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedOrder {
    pub lines: Vec<PricedLine>,
    pub gross_sales: Money,
    pub discount_total: Money,
    pub tax_total: Money,
    pub net_sales: Money,
    /// Σ line_total, plus tax when prices are tax-exclusive.
    pub grand_total: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PricingError {
    #[error("an order needs at least one line")]
    NoLines,
    #[error("line {0}: quantity must be positive")]
    NonPositiveQty(usize),
    #[error("line {0}: price must not be negative")]
    NegativePrice(usize),
    #[error("line {0}: tax rate must be between 0 and 10000 basis points")]
    BadTaxRate(usize),
    #[error("line {0}: discount must be between 0 and the line amount")]
    BadLineDiscount(usize),
    #[error("bill discount must be between 0 and the bill amount after item discounts")]
    BadBillDiscount,
    #[error("amount too large")]
    Overflow,
}

/// Tax contained in (inclusive) or added to (exclusive) an amount, rounded half away from zero.
pub fn tax_for(amount: Money, rate_bp: i64, inclusive: bool) -> Option<Money> {
    if rate_bp == 0 || amount == 0 {
        return Some(0);
    }
    if inclusive { mul_div_round(amount, rate_bp, 10_000 + rate_bp) } else { mul_div_round(amount, rate_bp, 10_000) }
}

/// Price the lines of an order and apportion the bill discount across them (largest remainder,
/// weighted by each line's amount after its own item discount).
pub fn price_lines(
    lines: &[LineInput],
    bill_discount: Money,
    tax_inclusive: bool,
) -> Result<PricedOrder, PricingError> {
    if lines.is_empty() {
        return Err(PricingError::NoLines);
    }
    let mut bases = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        if l.qty <= 0 {
            return Err(PricingError::NonPositiveQty(i));
        }
        if l.unit_price < 0 {
            return Err(PricingError::NegativePrice(i));
        }
        if !(0..=10_000).contains(&l.tax_rate_bp) {
            return Err(PricingError::BadTaxRate(i));
        }
        let gross = l.unit_price.checked_mul(l.qty).ok_or(PricingError::Overflow)?;
        if l.line_discount < 0 || l.line_discount > gross {
            return Err(PricingError::BadLineDiscount(i));
        }
        bases.push(gross - l.line_discount);
    }
    let base_sum = bases.iter().try_fold(0i64, |acc, b| acc.checked_add(*b)).ok_or(PricingError::Overflow)?;
    if bill_discount < 0 || bill_discount > base_sum {
        return Err(PricingError::BadBillDiscount);
    }
    let shares = apportion(bill_discount, &bases).ok_or(PricingError::BadBillDiscount)?;

    let mut priced = Vec::with_capacity(lines.len());
    for ((l, base), share) in lines.iter().zip(&bases).zip(shares) {
        let line_total = base - share;
        let tax_amount = tax_for(line_total, l.tax_rate_bp, tax_inclusive).ok_or(PricingError::Overflow)?;
        let line_net = if tax_inclusive { line_total - tax_amount } else { line_total };
        priced.push(PricedLine {
            line_gross: base + l.line_discount,
            line_discount: l.line_discount,
            order_discount_share: share,
            line_total,
            tax_amount,
            line_net,
        });
    }
    let sum = |f: fn(&PricedLine) -> Money| -> Result<Money, PricingError> {
        priced.iter().try_fold(0i64, |acc, p| acc.checked_add(f(p))).ok_or(PricingError::Overflow)
    };
    let gross_sales = sum(|p| p.line_gross)?;
    let discount_total = sum(|p| p.line_discount + p.order_discount_share)?;
    let tax_total = sum(|p| p.tax_amount)?;
    let net_sales = sum(|p| p.line_net)?;
    let lines_total = sum(|p| p.line_total)?;
    let grand_total =
        if tax_inclusive { lines_total } else { lines_total.checked_add(tax_total).ok_or(PricingError::Overflow)? };
    Ok(PricedOrder { lines: priced, gross_sales, discount_total, tax_total, net_sales, grand_total })
}

/// Indices of lines whose combined (item + bill share) discount exceeds `limit_bp`.
pub fn lines_over_discount_limit(order: &PricedOrder, limit_bp: i64) -> Vec<usize> {
    order
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !within_limit(l.line_discount + l.order_discount_share, l.line_gross, limit_bp))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn line(price: Money, qty: i64, disc: Money) -> LineInput {
        LineInput { unit_price: price, qty, line_discount: disc, tax_rate_bp: 0 }
    }

    #[test]
    fn zero_tax_inclusive_order() {
        let o = price_lines(&[line(45_000, 1, 0), line(150_000, 1, 0), line(5_000, 1, 0)], 10_000, true).unwrap();
        assert_eq!(
            (o.gross_sales, o.discount_total, o.grand_total, o.net_sales, o.tax_total),
            (200_000, 10_000, 190_000, 190_000, 0)
        );
        assert!(lines_over_discount_limit(&o, 500).is_empty()); // exactly 5% on every line
    }

    #[test]
    fn combined_item_and_bill_discount_checked_per_line() {
        // 4% item discount on line 0, then a 2% bill discount ⇒ line 0 is ~5.92% combined.
        let o = price_lines(&[line(10_000, 1, 400), line(10_000, 1, 0)], 392, true).unwrap();
        assert_eq!(lines_over_discount_limit(&o, 500), vec![0]);
        assert!(lines_over_discount_limit(&o, 2000).is_empty());
    }

    #[test]
    fn inclusive_tax_is_extracted() {
        let o = price_lines(&[LineInput { unit_price: 11_800, qty: 1, line_discount: 0, tax_rate_bp: 1_800 }], 0, true)
            .unwrap();
        assert_eq!((o.tax_total, o.net_sales, o.grand_total), (1_800, 10_000, 11_800));
    }

    #[test]
    fn exclusive_tax_is_added() {
        let o =
            price_lines(&[LineInput { unit_price: 10_000, qty: 1, line_discount: 0, tax_rate_bp: 1_800 }], 0, false)
                .unwrap();
        assert_eq!((o.tax_total, o.grand_total, o.net_sales), (1_800, 11_800, 10_000));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(price_lines(&[], 0, true), Err(PricingError::NoLines));
        assert_eq!(price_lines(&[line(100, 0, 0)], 0, true), Err(PricingError::NonPositiveQty(0)));
        assert_eq!(price_lines(&[line(-1, 1, 0)], 0, true), Err(PricingError::NegativePrice(0)));
        assert_eq!(price_lines(&[line(100, 1, 101)], 0, true), Err(PricingError::BadLineDiscount(0)));
        assert_eq!(price_lines(&[line(100, 1, 50)], 51, true), Err(PricingError::BadBillDiscount));
        assert_eq!(price_lines(&[line(i64::MAX, 2, 0)], 0, true), Err(PricingError::Overflow));
        let bad_tax = LineInput { unit_price: 1, qty: 1, line_discount: 0, tax_rate_bp: 10_001 };
        assert_eq!(price_lines(&[bad_tax], 0, true), Err(PricingError::BadTaxRate(0)));
    }

    proptest! {
        /// The identities the SQL seal trigger verifies (doc §4.3) hold for every valid cart.
        #[test]
        fn seal_identities_hold(
            raw in prop::collection::vec((0i64..2_000_000, 1i64..50, 0u16..=100, prop::sample::select(vec![0i64, 800, 1_500, 1_800])), 1..12),
            bill_pct in 0u16..=100,
            inclusive in any::<bool>(),
        ) {
            let lines: Vec<LineInput> = raw.iter().map(|(p, q, d, t)| LineInput {
                unit_price: *p, qty: *q, line_discount: p * q * i64::from(*d) / 100, tax_rate_bp: *t,
            }).collect();
            let base: i64 = lines.iter().map(|l| l.unit_price * l.qty - l.line_discount).sum();
            let bill = base * i64::from(bill_pct) / 100;
            let o = price_lines(&lines, bill, inclusive).unwrap();
            prop_assert_eq!(o.lines.iter().map(|l| l.order_discount_share).sum::<i64>(), bill);
            for (l, p) in lines.iter().zip(&o.lines) {
                prop_assert_eq!(p.line_gross, l.unit_price * l.qty);
                prop_assert_eq!(p.line_total, p.line_gross - p.line_discount - p.order_discount_share);
                prop_assert!(p.line_total >= 0 && p.line_net >= 0 && p.tax_amount >= 0);
            }
            prop_assert_eq!(o.gross_sales, o.lines.iter().map(|l| l.line_gross).sum::<i64>());
            prop_assert_eq!(o.discount_total, o.lines.iter().map(|l| l.line_discount + l.order_discount_share).sum::<i64>());
            let lines_total: i64 = o.lines.iter().map(|l| l.line_total).sum();
            let expected_grand = if inclusive { lines_total } else { lines_total + o.tax_total };
            prop_assert_eq!(o.grand_total, expected_grand);
        }
    }
}
