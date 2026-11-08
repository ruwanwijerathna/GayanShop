//! Tender rules (ARCHITECTURE §3.3, §3.7). All sales are final: no payment is ever negative, the
//! applied amounts equal the bill exactly, and change can only come from cash tendered.
//! The SQL seal trigger enforces the same identities.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::money::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethod {
    Cash,
    Card,
    BankTransfer,
    Qr,
    Other,
}

impl PaymentMethod {
    /// The value stored in `order_payments.method`.
    pub fn as_db(self) -> &'static str {
        match self {
            PaymentMethod::Cash => "cash",
            PaymentMethod::Card => "card",
            PaymentMethod::BankTransfer => "bank_transfer",
            PaymentMethod::Qr => "qr",
            PaymentMethod::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaymentInput {
    pub method: PaymentMethod,
    /// Amount applied to the bill.
    pub amount: Money,
    /// Cash handed over (cash only). Change = tendered − amount.
    pub tendered: Option<Money>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaymentRuleError {
    #[error("all sales are final: payment {0} is negative")]
    Negative(usize),
    #[error("payment {0}: only cash can be tendered with change")]
    TenderedNonCash(usize),
    #[error("payment {0}: cash tendered is less than the amount applied")]
    TenderedBelowAmount(usize),
    #[error("payments ({paid}) must equal the amount due ({due}) exactly")]
    Mismatch { paid: Money, due: Money },
    #[error("the bill total must not be negative")]
    NegativeTotal,
    #[error("amount too large")]
    Overflow,
}

/// Validate the payment rows for a bill. Returns the cash change due.
pub fn validate_payments(grand_total: Money, payments: &[PaymentInput]) -> Result<Money, PaymentRuleError> {
    if grand_total < 0 {
        return Err(PaymentRuleError::NegativeTotal);
    }
    let mut paid: Money = 0;
    let mut change: Money = 0;
    for (i, p) in payments.iter().enumerate() {
        if p.amount < 0 {
            return Err(PaymentRuleError::Negative(i));
        }
        if let Some(tendered) = p.tendered {
            if p.method != PaymentMethod::Cash {
                return Err(PaymentRuleError::TenderedNonCash(i));
            }
            if tendered < p.amount {
                return Err(PaymentRuleError::TenderedBelowAmount(i));
            }
            change = change.checked_add(tendered - p.amount).ok_or(PaymentRuleError::Overflow)?;
        }
        paid = paid.checked_add(p.amount).ok_or(PaymentRuleError::Overflow)?;
    }
    if paid != grand_total {
        return Err(PaymentRuleError::Mismatch { paid, due: grand_total });
    }
    Ok(change)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cash(amount: Money, tendered: Option<Money>) -> PaymentInput {
        PaymentInput { method: PaymentMethod::Cash, amount, tendered }
    }

    #[test]
    fn cash_with_change() {
        assert_eq!(validate_payments(1_250, &[cash(1_250, Some(2_000))]), Ok(750));
        assert_eq!(validate_payments(1_250, &[cash(1_250, None)]), Ok(0));
    }

    #[test]
    fn split_tender() {
        let card = PaymentInput { method: PaymentMethod::Card, amount: 1_000, tendered: None };
        assert_eq!(validate_payments(1_250, &[card, cash(250, Some(500))]), Ok(250));
    }

    #[test]
    fn no_refunds_no_negatives() {
        assert_eq!(validate_payments(-500, &[]), Err(PaymentRuleError::NegativeTotal));
        assert_eq!(validate_payments(0, &[cash(-5, None), cash(5, None)]), Err(PaymentRuleError::Negative(0)));
    }

    #[test]
    fn exact_amount_required() {
        assert_eq!(
            validate_payments(1_250, &[cash(1_000, None)]),
            Err(PaymentRuleError::Mismatch { paid: 1_000, due: 1_250 })
        );
        assert_eq!(
            validate_payments(1_250, &[cash(1_300, None)]),
            Err(PaymentRuleError::Mismatch { paid: 1_300, due: 1_250 })
        );
        assert_eq!(validate_payments(0, &[]), Ok(0)); // a 100%-discounted bill needs no payment
    }

    #[test]
    fn tender_rules() {
        let card = PaymentInput { method: PaymentMethod::Card, amount: 100, tendered: Some(200) };
        assert_eq!(validate_payments(100, &[card]), Err(PaymentRuleError::TenderedNonCash(0)));
        assert_eq!(validate_payments(100, &[cash(100, Some(50))]), Err(PaymentRuleError::TenderedBelowAmount(0)));
    }

    #[test]
    fn db_values() {
        assert_eq!(PaymentMethod::BankTransfer.as_db(), "bank_transfer");
        assert_eq!(PaymentMethod::Qr.as_db(), "qr");
    }
}
