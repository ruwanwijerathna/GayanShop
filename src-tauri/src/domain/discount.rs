//! Bill-discount apportionment and the role discount-limit check (ARCHITECTURE §3.7).

use super::money::Money;

/// Split `total` across `weights` in proportion, using the largest-remainder method so the
/// shares sum to `total` exactly. Ties go to the lower index. All-zero weights ⇒ `None`.
pub fn apportion(total: Money, weights: &[Money]) -> Option<Vec<Money>> {
    assert!(total >= 0, "apportion total must be non-negative");
    assert!(weights.iter().all(|w| *w >= 0), "weights must be non-negative");
    let sum: i128 = weights.iter().map(|w| *w as i128).sum();
    if weights.is_empty() || (sum == 0 && total != 0) {
        return None;
    }
    if total == 0 {
        return Some(vec![0; weights.len()]);
    }
    let mut shares = Vec::with_capacity(weights.len());
    let mut remainders = Vec::with_capacity(weights.len());
    for (i, w) in weights.iter().enumerate() {
        let n = total as i128 * *w as i128;
        shares.push((n / sum) as Money);
        remainders.push((n % sum, i));
    }
    let mut leftover = total - shares.iter().sum::<Money>();
    // Largest remainder first; stable on index for ties.
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, i) in remainders {
        if leftover == 0 {
            break;
        }
        shares[i] += 1;
        leftover -= 1;
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
    discount as i128 * 10_000 <= limit_bp as i128 * gross as i128
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_sum_exactly() {
        let shares = apportion(1000, &[1, 1, 1]).unwrap();
        assert_eq!(shares, vec![334, 333, 333]);
        assert_eq!(shares.iter().sum::<i64>(), 1000);
    }

    #[test]
    fn proportional_split() {
        // 5% bill discount on 450.00 + 1500.00 + 50.00 = 2000.00 ⇒ 100.00
        let shares = apportion(10_000, &[45_000, 150_000, 5_000]).unwrap();
        assert_eq!(shares, vec![2_250, 7_500, 250]);
    }

    #[test]
    fn many_odd_splits_always_reconcile() {
        for total in [1, 7, 99, 1001, 123_457] {
            for weights in [vec![3, 5, 7], vec![1, 0, 2], vec![9_999, 1], vec![1; 13]] {
                let s = apportion(total, &weights).unwrap();
                assert_eq!(s.iter().sum::<i64>(), total, "total {total} weights {weights:?}");
                assert!(s.iter().all(|x| *x >= 0));
                // zero-weight lines never receive a share
                for (w, share) in weights.iter().zip(&s) {
                    if *w == 0 {
                        assert_eq!(*share, 0);
                    }
                }
            }
        }
    }

    #[test]
    fn zero_weights_rejected() {
        assert!(apportion(10, &[0, 0]).is_none());
        assert_eq!(apportion(0, &[0, 0]), Some(vec![0, 0]));
    }

    #[test]
    fn five_percent_limit() {
        assert!(within_limit(500, 10_000, 500)); // exactly 5%
        assert!(!within_limit(501, 10_000, 500)); // 5.01%
        assert!(within_limit(0, 0, 0));
        assert!(!within_limit(1, 0, 10_000));
        // 2.5% item + 2.5% bill share = 5% combined: allowed. 3% + 3%: not.
        assert!(within_limit(250 + 250, 10_000, 500));
        assert!(!within_limit(300 + 300, 10_000, 500));
    }
}
