//! Exchange rules (ARCHITECTURE §3.3, decision #26). Pure: no IO.
//!
//! * Item-for-item only: the customer hands back goods and takes other goods. There is never a cash
//!   refund or a credit note, so the replacement must be worth at least what the returned goods were
//!   sold for, and the customer pays only the (non-negative) difference.
//! * The 7-day window counts from the original sale's business date. After it, a supervisor override
//!   (`EXCHANGE_LATE`) is required.
//! * Returned goods are valued at what the customer actually paid for them (after line and bill
//!   discounts), split over the returned units with cumulative flooring so that returning every unit of a
//!   line, in any number of exchanges, gives back exactly the line's total, with no rounding drift.

use chrono::NaiveDate;

/// Default exchange window in days (`exchange.window_days`).
pub const DEFAULT_WINDOW_DAYS: i64 = 7;
/// Largest window the owner can configure.
pub const MAX_WINDOW_DAYS: i64 = 365;

/// The part of `amount` that belongs to units `from..to` of a line of `qty` units:
/// `floor(amount·to/qty) − floor(amount·from/qty)`. Summed over a partition of `0..qty` it is exactly `amount`.
pub fn portion(amount: i64, qty: i64, from: i64, to: i64) -> i64 {
    if qty <= 0 || from < 0 || to > qty || from >= to {
        return 0;
    }
    let at = |n: i64| i128::from(amount) * i128::from(n) / i128::from(qty);
    i64::try_from(at(to) - at(from)).unwrap_or(0)
}

/// Whole days from the sale's business date to the exchange's business date (never negative).
pub fn days_since(sold_on: NaiveDate, today: NaiveDate) -> i64 {
    (today - sold_on).num_days().max(0)
}

/// An exchange needs the late override once more than `window_days` days have passed.
pub fn is_late(sold_on: NaiveDate, today: NaiveDate, window_days: i64) -> bool {
    days_since(sold_on, today) > window_days
}

/// Why an exchange cannot proceed on the money alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExchangeRuleError {
    #[error("the replacement ({replacement}) must be worth at least the returned goods ({returned})")]
    ReplacementBelowReturned { replacement: i64, returned: i64 },
}

/// What the customer pays at the till: replacement total minus the value of the returned goods.
pub fn amount_due(replacement_total: i64, returned_value: i64) -> Result<i64, ExchangeRuleError> {
    if replacement_total < returned_value {
        return Err(ExchangeRuleError::ReplacementBelowReturned {
            replacement: replacement_total,
            returned: returned_value,
        });
    }
    Ok(replacement_total - returned_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap_or_default()
    }

    #[test]
    fn portion_splits_exactly() {
        // 100 over 3 units: 33, 33, 34 in any partition order.
        assert_eq!(portion(100, 3, 0, 1), 33);
        assert_eq!(portion(100, 3, 1, 2), 33);
        assert_eq!(portion(100, 3, 2, 3), 34);
        assert_eq!(portion(100, 3, 0, 3), 100);
        assert_eq!(portion(100, 3, 1, 3), 67);
        assert_eq!(portion(0, 5, 0, 5), 0);
    }

    #[test]
    fn portion_rejects_bad_ranges() {
        assert_eq!(portion(100, 3, 2, 2), 0);
        assert_eq!(portion(100, 3, 2, 4), 0);
        assert_eq!(portion(100, 0, 0, 0), 0);
        assert_eq!(portion(100, 3, -1, 1), 0);
    }

    #[test]
    fn window_counts_from_the_sale_business_date() {
        let sold = d(2026, 11, 10);
        assert!(!is_late(sold, d(2026, 11, 10), 7));
        assert!(!is_late(sold, d(2026, 11, 17), 7), "day 7 is still inside the window");
        assert!(is_late(sold, d(2026, 11, 18), 7), "day 8 needs the override");
        assert_eq!(days_since(sold, d(2026, 11, 1)), 0, "never negative");
        assert!(is_late(sold, d(2026, 11, 11), 0));
    }

    #[test]
    fn customer_pays_only_the_difference_and_never_receives_money() {
        assert_eq!(amount_due(1_500, 1_000), Ok(500));
        assert_eq!(amount_due(1_000, 1_000), Ok(0));
        assert_eq!(
            amount_due(900, 1_000),
            Err(ExchangeRuleError::ReplacementBelowReturned { replacement: 900, returned: 1_000 })
        );
    }

    proptest! {
        #[test]
        fn any_partition_of_the_units_sums_to_the_amount(
            amount in 0i64..10_000_000_000,
            qty in 1i64..500,
            cuts in proptest::collection::vec(0i64..500, 0..6),
        ) {
            let mut points: Vec<i64> = cuts.into_iter().map(|c| c % (qty + 1)).collect();
            points.push(0);
            points.push(qty);
            points.sort_unstable();
            points.dedup();
            let sum: i64 = points.windows(2).map(|w| portion(amount, qty, w[0], w[1])).sum();
            prop_assert_eq!(sum, amount);
        }
    }
}
