//! Money is always an integer count of minor units (LKR cents). Never a float.

pub type Money = i64;

/// `a × b / d`, rounded half away from zero, computed in i128 so the product cannot overflow.
/// Returns `None` for `d == 0` or if the result does not fit in an `i64`.
pub fn mul_div_round(a: i64, b: i64, d: i64) -> Option<i64> {
    if d == 0 {
        return None;
    }
    let n = i128::from(a) * i128::from(b);
    let d = i128::from(d);
    let q = n / d;
    let r = n % d;
    let q = if r.abs() * 2 >= d.abs() { if (n < 0) ^ (d < 0) { q - 1 } else { q + 1 } } else { q };
    i64::try_from(q).ok()
}

/// `a × b / d`, truncated toward zero (floor for non-negative inputs). `None` on `d == 0` or overflow.
pub fn mul_div_floor(a: i64, b: i64, d: i64) -> Option<i64> {
    if d == 0 {
        return None;
    }
    i64::try_from(i128::from(a) * i128::from(b) / i128::from(d)).ok()
}

/// Parse a major-unit amount typed by a person ("450", "450.5", "1,250.75") into minor units.
pub fn parse_major(input: &str) -> Option<Money> {
    let s: String = input.trim().chars().filter(|c| *c != ',').collect();
    if s.is_empty() || s.starts_with('-') || s.starts_with('+') {
        return None;
    }
    let (whole, frac) = s.split_once('.').unwrap_or((s.as_str(), ""));
    if (whole.is_empty() && frac.is_empty())
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !frac.chars().all(|c| c.is_ascii_digit())
        || frac.len() > 2
    {
        return None;
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let frac: i64 = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 10,
        _ => frac.parse().ok()?,
    };
    whole.checked_mul(100)?.checked_add(frac)
}

/// "Rs 1,234.50" (LKR, exponent 2). Receipts and screens both use Latin digits.
pub fn format_lkr(cents: Money) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let whole = (abs / 100).to_string();
    let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
    for (i, ch) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}Rs {grouped}.{:02}", abs % 100)
}

/// Label price: "Rs. 1,250" when whole rupees, "Rs. 1,250.50" otherwise (doc §7.4).
pub fn format_label_price(cents: Money) -> String {
    let full = format_lkr(cents).replacen("Rs ", "Rs. ", 1);
    full.strip_suffix(".00").map(str::to_owned).unwrap_or(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn rounding_half_away_from_zero() {
        assert_eq!(mul_div_round(5, 1, 2), Some(3));
        assert_eq!(mul_div_round(-5, 1, 2), Some(-3));
        assert_eq!(mul_div_round(4, 1, 3), Some(1));
        assert_eq!(mul_div_round(i64::MAX, 2, 2), Some(i64::MAX));
        assert_eq!(mul_div_round(1, 1, 0), None);
        assert_eq!(mul_div_round(i64::MAX, 2, 1), None);
    }

    #[test]
    fn floor_division() {
        assert_eq!(mul_div_floor(1_000, 1, 3), Some(333));
        assert_eq!(mul_div_floor(1, 1, 0), None);
    }

    #[test]
    fn parses_major_units() {
        assert_eq!(parse_major("450"), Some(45_000));
        assert_eq!(parse_major("450.5"), Some(45_050));
        assert_eq!(parse_major("1,250.75"), Some(125_075));
        assert_eq!(parse_major(".5"), Some(50));
        assert_eq!(parse_major("10L"), None);
        assert_eq!(parse_major("1.234"), None);
        assert_eq!(parse_major("-5"), None);
        assert_eq!(parse_major(""), None);
        assert_eq!(parse_major("."), None);
        assert_eq!(parse_major("99999999999999999999"), None);
    }

    #[test]
    fn formats_lkr() {
        assert_eq!(format_lkr(0), "Rs 0.00");
        assert_eq!(format_lkr(123_450), "Rs 1,234.50");
        assert_eq!(format_lkr(100_000_000), "Rs 1,000,000.00");
        assert_eq!(format_lkr(-505), "-Rs 5.05");
    }

    #[test]
    fn formats_label_price() {
        assert_eq!(format_label_price(125_000), "Rs. 1,250");
        assert_eq!(format_label_price(125_050), "Rs. 1,250.50");
        assert_eq!(format_label_price(45_000), "Rs. 450");
    }

    proptest! {
        #[test]
        fn parse_roundtrips_formatting(cents in 0i64..10_000_000_000) {
            let shown = format_lkr(cents);
            let typed = shown.trim_start_matches("Rs ");
            prop_assert_eq!(parse_major(typed), Some(cents));
        }
    }
}
