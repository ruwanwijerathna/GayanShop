//! Bill-discount apportionment and the role discount-limit check (ARCHITECTURE §3.7).

use super::money::Money;

/// Split `total` across `weights` in proportion, using the largest-remainder method so the shares
/// sum to `total` exactly. Ties go to the lower index.
///
/// Returns `None` if `total` or any weight is negative, if there are no weights, or if all weights are
/// zero while `total` is not (there is nothing to apportion against).
pub fn apportion(total: Money, weights: &[Money]) -> Option<Vec<Money>> {
    if total < 0 || weights.is_empty() || weights.iter().any(|w| *w < 0) {
        return None;
    }
    let sum: i128 = weights.iter().map(|w| i128::from(*w)).sum();
    if total == 0 {
        return Some(vec![0; weights.len()]);
    }
    if sum == 0 {
        return None;
    }
    let mut shares = Vec::with_capacity(weights.len());
    let mut remainders = Vec::with_capacity(weights.len());
    for (i, w) in weights.iter().enumerate() {
        let n = i128::from(total) * i128::from(*w);
        shares.push(i64::try_from(n / sum).ok()?);
        remainders.push((n % sum, i));
    }
    let mut leftover = total - shares.iter().sum::<Money>();
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, i) in remainders {
        if leftover == 0 {
            break;
        }
        if let Some(share) = shares.get_mut(i) {
            *share += 1;
            leftover -= 1;
        }
    }
    Some(shares)
}

/// True when `discount / gross ≤ limit_bp / 10000`, by exact integer cross-multiplication.
pub fn within_limit(discount: Money, gross: Money, limit_bp: i64) -> bool {
    if discount <= 0 {
        return true;
    }
    if gross <= 0 {
        return false;
    }
    i128::from(discount) * 10_000 <= i128::from(limit_bp) * i128::from(gross)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn shares_sum_exactly() {
        let shares = apportion(1000, &[1, 1, 1]).unwrap();
        assert_eq!(shares, vec![334, 333, 333]);
    }

    #[test]
    fn proportional_split() {
        // 5% bill discount on 450.00 + 1500.00 + 50.00 = 2000.00 ⇒ 100.00
        assert_eq!(apportion(10_000, &[45_000, 150_000, 5_000]).unwrap(), vec![2_250, 7_500, 250]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(apportion(10, &[0, 0]).is_none());
        assert!(apportion(10, &[]).is_none());
        assert!(apportion(-1, &[1]).is_none());
        assert!(apportion(1, &[1, -1]).is_none());
        assert_eq!(apportion(0, &[0, 0]), Some(vec![0, 0]));
    }

    #[test]
    fn five_percent_limit() {
        assert!(within_limit(500, 10_000, 500));
        assert!(!within_limit(501, 10_000, 500));
        assert!(within_limit(0, 0, 0));
        assert!(!within_limit(1, 0, 10_000));
        assert!(within_limit(250 + 250, 10_000, 500)); // 2.5% item + 2.5% bill share = 5%
        assert!(!within_limit(300 + 300, 10_000, 500));
    }

    proptest! {
        #[test]
        fn shares_always_reconcile(total in 0i64..1_000_000_000, weights in prop::collection::vec(0i64..10_000_000, 1..30)) {
            prop_assume!(total == 0 || weights.iter().any(|w| *w > 0));
            let shares = apportion(total, &weights).unwrap();
            prop_assert_eq!(shares.iter().sum::<i64>(), total);
            for (w, s) in weights.iter().zip(&shares) {
                prop_assert!(*s >= 0);
                if *w == 0 { prop_assert_eq!(*s, 0); }
            }
            // largest remainder: every share is within 1 of its exact proportional value
            let sum: i128 = weights.iter().map(|w| i128::from(*w)).sum();
            if sum > 0 {
                for (w, s) in weights.iter().zip(&shares) {
                    let exact_floor = i128::from(total) * i128::from(*w) / sum;
                    prop_assert!(i128::from(*s) == exact_floor || i128::from(*s) == exact_floor + 1);
                }
            }
        }
    }
}
