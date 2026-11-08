//! Exchange / no-cash-refund rules (ARCHITECTURE §3.3).

use chrono::NaiveDate;

use super::money::{Money, mul_div_floor};

pub const DEFAULT_WINDOW_DAYS: i64 = 7;

/// Day 7 is included: a sale on 10 Nov can be exchanged up to and including 17 Nov.
pub fn within_window(sold_on: NaiveDate, exchanged_on: NaiveDate, window_days: i64) -> bool {
    let days = (exchanged_on - sold_on).num_days();
    (0..=window_days).contains(&days)
}

/// Value of returning `k` more units of an original line, valued at what the customer paid.
/// The return that completes the line gets the exact remainder, so full returns reconcile.
pub fn return_value(line_total: Money, line_qty: i64, returned_qty: i64, returned_value: Money, k: i64) -> Option<Money> {
    if line_qty <= 0 || k <= 0 || returned_qty < 0 || returned_qty + k > line_qty {
        return None;
    }
    if returned_qty + k == line_qty {
        Some(line_total - returned_value)
    } else {
        Some(mul_div_floor(line_total, k, line_qty))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    CustomerPays(Money),
    Even,
    IssueCreditNote(Money),
}

pub fn settle(balance: Money) -> Settlement {
    match balance {
        b if b > 0 => Settlement::CustomerPays(b),
        0 => Settlement::Even,
        b => Settlement::IssueCreditNote(-b),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tender {
    Cash,
    Card,
    BankTransfer,
    Qr,
    Other,
    CreditNote,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaymentRuleError {
    #[error("no cash refunds: {0:?} payments cannot be negative")]
    NegativeNonCreditNote(Tender),
    #[error("a credit note may only be issued for a negative exchange balance, for exactly that amount")]
    BadCreditNoteIssue,
    #[error("payments ({paid}) do not cover the amount due ({due})")]
    Underpaid { paid: Money, due: Money },
    #[error("only cash can be over-tendered; non-cash payments exceed the amount due")]
    NonCashOverpayment,
}

/// Enforce the money-out rules for an order's payment rows. Returns the cash change due.
///
/// - Only a credit-note *issue* row may be negative, and only to settle a negative balance exactly.
/// - Non-cash tenders (including credit-note redemption) never exceed what is due; cash may (change).
pub fn validate_payments(grand_total: Money, payments: &[(Tender, Money)]) -> Result<Money, PaymentRuleError> {
    for (t, amt) in payments {
        if *amt < 0 && *t != Tender::CreditNote {
            return Err(PaymentRuleError::NegativeNonCreditNote(*t));
        }
    }
    let issued: Money = payments.iter().filter(|(t, a)| *t == Tender::CreditNote && *a < 0).map(|(_, a)| *a).sum();
    if issued != 0 && (grand_total >= 0 || issued != grand_total || payments.len() != 1) {
        return Err(PaymentRuleError::BadCreditNoteIssue);
    }
    if grand_total < 0 && issued == 0 {
        return Err(PaymentRuleError::BadCreditNoteIssue);
    }
    if grand_total <= 0 {
        return Ok(0);
    }
    let non_cash: Money = payments.iter().filter(|(t, _)| *t != Tender::Cash).map(|(_, a)| *a).sum();
    let cash: Money = payments.iter().filter(|(t, _)| *t == Tender::Cash).map(|(_, a)| *a).sum();
    if non_cash > grand_total {
        return Err(PaymentRuleError::NonCashOverpayment);
    }
    let paid = non_cash + cash;
    if paid < grand_total {
        return Err(PaymentRuleError::Underpaid { paid, due: grand_total });
    }
    Ok(paid - grand_total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::business_date::parse_date;

    #[test]
    fn seven_day_window_inclusive() {
        let sold = parse_date("2026-11-10").unwrap();
        assert!(within_window(sold, parse_date("2026-11-10").unwrap(), 7));
        assert!(within_window(sold, parse_date("2026-11-17").unwrap(), 7));
        assert!(!within_window(sold, parse_date("2026-11-18").unwrap(), 7));
        assert!(!within_window(sold, parse_date("2026-11-09").unwrap(), 7));
    }

    #[test]
    fn partial_returns_reconcile_to_the_cent() {
        // 3 units sold for Rs 10.00 total after discounts.
        let first = return_value(1_000, 3, 0, 0, 1).unwrap();
        let second = return_value(1_000, 3, 1, first, 1).unwrap();
        let third = return_value(1_000, 3, 2, first + second, 1).unwrap();
        assert_eq!((first, second, third), (333, 333, 334));
        assert_eq!(return_value(1_000, 3, 3, 1_000, 1), None);
        assert_eq!(return_value(1_000, 3, 0, 0, 3), Some(1_000));
    }

    #[test]
    fn settlement() {
        assert_eq!(settle(500), Settlement::CustomerPays(500));
        assert_eq!(settle(0), Settlement::Even);
        assert_eq!(settle(-250), Settlement::IssueCreditNote(250));
    }

    #[test]
    fn no_cash_refunds() {
        assert_eq!(validate_payments(-500, &[(Tender::Cash, -500)]), Err(PaymentRuleError::NegativeNonCreditNote(Tender::Cash)));
        assert_eq!(validate_payments(-500, &[(Tender::Card, -500)]), Err(PaymentRuleError::NegativeNonCreditNote(Tender::Card)));
        assert_eq!(validate_payments(-500, &[]), Err(PaymentRuleError::BadCreditNoteIssue));
        assert_eq!(validate_payments(-500, &[(Tender::CreditNote, -400)]), Err(PaymentRuleError::BadCreditNoteIssue));
        assert_eq!(validate_payments(-500, &[(Tender::CreditNote, -500)]), Ok(0));
        assert_eq!(validate_payments(500, &[(Tender::CreditNote, -500)]), Err(PaymentRuleError::BadCreditNoteIssue));
    }

    #[test]
    fn change_and_overpayment() {
        assert_eq!(validate_payments(1_250, &[(Tender::Cash, 2_000)]), Ok(750));
        assert_eq!(validate_payments(1_250, &[(Tender::CreditNote, 1_000), (Tender::Cash, 500)]), Ok(250));
        assert_eq!(validate_payments(1_250, &[(Tender::CreditNote, 1_500)]), Err(PaymentRuleError::NonCashOverpayment));
        assert_eq!(validate_payments(1_250, &[(Tender::Card, 1_000)]), Err(PaymentRuleError::Underpaid { paid: 1_000, due: 1_250 }));
        assert_eq!(validate_payments(0, &[]), Ok(0));
    }
}
