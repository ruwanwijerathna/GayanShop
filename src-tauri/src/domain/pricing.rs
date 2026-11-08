//! Authoritative line and order totals (ARCHITECTURE §3.7). The UI only previews these.

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
    #[error("line {0}: quantity must be positive for a sale line")]
    NonPositiveQty(usize),
    #[error("line {0}: price must not be negative")]
    NegativePrice(usize),
    #[error("line {0}: discount must be between 0 and the line amount")]
    BadLineDiscount(usize),
    #[error("bill discount must be between 0 and the bill amount after item discounts")]
    BadBillDiscount,
}

/// Tax contained in (inclusive) or added to (exclusive) an amount.
pub fn tax_for(amount: Money, rate_bp: i64, inclusive: bool) -> Money {
    if rate_bp == 0 || amount == 0 {
        return 0;
    }
    if inclusive {
        mul_div_round(amount, rate_bp, 10_000 + rate_bp)
    } else {
        mul_div_round(amount, rate_bp, 10_000)
    }
}

/// Price the new-item (qty > 0) lines of an order and apportion the bill discount across them.
pub fn price_lines(lines: &[LineInput], bill_discount: Money, tax_inclusive: bool) -> Result<PricedOrder, PricingError> {
    for (i, l) in lines.iter().enumerate() {
        if l.qty <= 0 {
            return Err(PricingError::NonPositiveQty(i));
        }
        if l.unit_price < 0 {
            return Err(PricingError::NegativePrice(i));
        }
        if l.line_discount < 0 || l.line_discount > l.unit_price * l.qty {
            return Err(PricingError::BadLineDiscount(i));
        }
    }
    let bases: Vec<Money> = lines.iter().map(|l| l.unit_price * l.qty - l.line_discount).collect();
    let base_sum: Money = bases.iter().sum();
    if bill_discount < 0 || bill_discount > base_sum {
        return Err(PricingError::BadBillDiscount);
    }
    let shares = if lines.is_empty() {
        vec![]
    } else {
        apportion(bill_discount, &bases).ok_or(PricingError::BadBillDiscount)?
    };

    let mut priced = Vec::with_capacity(lines.len());
    for (l, share) in lines.iter().zip(shares) {
        let line_gross = l.unit_price * l.qty;
        let line_total = line_gross - l.line_discount - share;
        let tax_amount = tax_for(line_total, l.tax_rate_bp, tax_inclusive);
        let line_net = if tax_inclusive { line_total - tax_amount } else { line_total };
        priced.push(PricedLine {
            line_gross,
            line_discount: l.line_discount,
            order_discount_share: share,
            line_total,
            tax_amount,
            line_net,
        });
    }
    let gross_sales = priced.iter().map(|p| p.line_gross).sum();
    let discount_total = priced.iter().map(|p| p.line_discount + p.order_discount_share).sum();
    let tax_total: Money = priced.iter().map(|p| p.tax_amount).sum();
    let net_sales = priced.iter().map(|p| p.line_net).sum();
    let lines_total: Money = priced.iter().map(|p| p.line_total).sum();
    let grand_total = if tax_inclusive { lines_total } else { lines_total + tax_total };
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

    fn line(price: Money, qty: i64, disc: Money) -> LineInput {
        LineInput { unit_price: price, qty, line_discount: disc, tax_rate_bp: 0 }
    }

    #[test]
    fn zero_tax_inclusive_order() {
        let o = price_lines(&[line(45_000, 1, 0), line(150_000, 1, 0), line(5_000, 1, 0)], 10_000, true).unwrap();
        assert_eq!(o.gross_sales, 200_000);
        assert_eq!(o.discount_total, 10_000);
        assert_eq!(o.grand_total, 190_000);
        assert_eq!(o.net_sales, 190_000);
        assert_eq!(o.tax_total, 0);
        assert_eq!(o.lines.iter().map(|l| l.order_discount_share).sum::<i64>(), 10_000);
        assert!(lines_over_discount_limit(&o, 500).is_empty()); // exactly 5% everywhere
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
        // Future VAT 18% on a Rs 118.00 tax-inclusive price ⇒ Rs 18.00 tax, Rs 100.00 net.
        let o = price_lines(&[LineInput { unit_price: 11_800, qty: 1, line_discount: 0, tax_rate_bp: 1_800 }], 0, true)
            .unwrap();
        assert_eq!(o.tax_total, 1_800);
        assert_eq!(o.net_sales, 10_000);
        assert_eq!(o.grand_total, 11_800);
    }

    #[test]
    fn exclusive_tax_is_added() {
        let o = price_lines(&[LineInput { unit_price: 10_000, qty: 1, line_discount: 0, tax_rate_bp: 1_800 }], 0, false)
            .unwrap();
        assert_eq!(o.tax_total, 1_800);
        assert_eq!(o.grand_total, 11_800);
        assert_eq!(o.net_sales, 10_000);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(price_lines(&[line(100, 0, 0)], 0, true), Err(PricingError::NonPositiveQty(0)));
        assert_eq!(price_lines(&[line(100, 1, 101)], 0, true), Err(PricingError::BadLineDiscount(0)));
        assert_eq!(price_lines(&[line(100, 1, 50)], 51, true), Err(PricingError::BadBillDiscount));
    }
}
