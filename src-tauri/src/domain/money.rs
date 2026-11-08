//! Money is always an integer count of minor units (LKR cents). Never a float.

pub type Money = i64;

/// `a × b / d`, rounded half away from zero, computed in i128 so it cannot overflow.
pub fn mul_div_round(a: i64, b: i64, d: i64) -> i64 {
    assert!(d != 0, "division by zero");
    let n = a as i128 * b as i128;
    let d = d as i128;
    let q = n / d;
    let r = n % d;
    let round_up = (r.abs() * 2) >= d.abs();
    let q = if round_up {
        if (n < 0) ^ (d < 0) { q - 1 } else { q + 1 }
    } else {
        q
    };
    q as i64
}

/// `a × b / d`, rounded toward zero (floor for non-negative inputs).
pub fn mul_div_floor(a: i64, b: i64, d: i64) -> i64 {
    assert!(d != 0, "division by zero");
    (a as i128 * b as i128 / d as i128) as i64
}

/// Parse a major-unit amount typed by a person ("450", "450.5", "1,250.75") into minor units.
pub fn parse_major(input: &str) -> Option<Money> {
    let s: String = input.trim().chars().filter(|c| *c != ',').collect();
    if s.is_empty() || s.starts_with('-') {
        return None;
    }
    let (whole, frac) = match s.split_once('.') {
        Some((w, f)) => (w, f),
        None => (s.as_str(), ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) || frac.len() > 2 {
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

/// "Rs 1,234.50" (LKR, exponent 2).
pub fn format_lkr(cents: Money) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let whole = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}Rs {grouped}.{:02}", abs % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_half_away_from_zero() {
        assert_eq!(mul_div_round(5, 1, 2), 3);
        assert_eq!(mul_div_round(-5, 1, 2), -3);
        assert_eq!(mul_div_round(4, 1, 3), 1);
        assert_eq!(mul_div_round(i64::MAX, 2, 2), i64::MAX);
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
    }

    #[test]
    fn formats_lkr() {
        assert_eq!(format_lkr(0), "Rs 0.00");
        assert_eq!(format_lkr(123_450), "Rs 1,234.50");
        assert_eq!(format_lkr(100_000_000), "Rs 1,000,000.00");
        assert_eq!(format_lkr(-505), "-Rs 5.05");
    }
}
