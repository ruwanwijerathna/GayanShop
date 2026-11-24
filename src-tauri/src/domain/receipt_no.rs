//! Offline-unique document numbers and in-store codes (ARCHITECTURE §6.4, §7.4).

use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodeError {
    #[error("device code must be 2–4 uppercase letters/digits ending in a digit 1–9 (e.g. T01)")]
    BadDeviceCode,
    #[error("in-store code sequence exhausted for this device")]
    SequenceExhausted,
    #[error("not a valid EAN check digit")]
    BadCheckDigit,
}

/// Device codes are short uppercase identifiers such as `T01`.
pub fn is_valid_device_code(code: &str) -> bool {
    (2..=4).contains(&code.len()) && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// `T01-261108004` (decision #31): the counter, a dash, the business date as `YYMMDD`, then that day's bill
/// number, at least three digits (it simply grows past 999: the date is always six digits, so it can't be misread).
/// Bills made before v4.8 keep `T01-20261108-0042`.
pub fn receipt_no(device_code: &str, business_date: NaiveDate, seq: u32) -> String {
    format!("{device_code}-{}{seq:03}", business_date.format("%y%m%d"))
}

/// Does a scanned or typed code look like a bill number of this store (either format)? Product codes are digits
/// only, so a `T0x-` prefix can never be a product. This is the shape only; the lookup says whether it exists.
pub fn looks_like_bill_no(code: &str) -> bool {
    let up = code.trim().to_uppercase();
    let Some(rest) = up.strip_prefix('T') else { return false };
    let Some((counter, tail)) = rest.split_once('-') else { return false };
    let counter_ok = counter.len() == 2 && counter.starts_with('0') && counter.bytes().all(|b| b.is_ascii_digit());
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    let new_format = digits(tail) && tail.len() >= 9;
    let old_format = tail.split_once('-').is_some_and(|(d, n)| d.len() == 8 && digits(d) && n.len() >= 4 && digits(n));
    counter_ok && (new_format || old_format)
}

/// Turn what a cashier typed into the full bill number to look up. It accepts any case, a space or a dash
/// anywhere a dash goes, `T1` for `T01`, an unpadded daily number, and, for today's bills, just the counter and
/// the daily number (`T01-4` is bill 4 of `today`). Anything it doesn't recognise comes back upper-cased and trimmed.
pub fn normalize_bill_query(input: &str, today: NaiveDate) -> String {
    let up = input.trim().to_uppercase();
    let tokens: Vec<&str> = up.split(|c: char| c == '-' || c.is_whitespace()).filter(|t| !t.is_empty()).collect();
    let all_digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    let Some((first, rest)) = tokens.split_first() else { return up };
    let counter = first
        .strip_prefix('T')
        .filter(|d| (1..=3).contains(&d.len()) && all_digits(d))
        .and_then(|d| d.parse::<u8>().ok())
        .filter(|n| (1..=9).contains(n));
    let Some(counter) = counter else { return up };
    let code = format!("T0{counter}");
    let day_and_seq = |date: &str, seq: &str| -> Option<String> {
        let n = seq.parse::<u32>().ok()?;
        Some(format!("{code}-{date}{n:03}"))
    };
    let found = match rest {
        // `T01-4`: bill 4 of today.
        [seq] if all_digits(seq) && seq.len() <= 5 => day_and_seq(&today.format("%y%m%d").to_string(), seq),
        // `T01-261108004` (or the daily number unpadded): six date digits, then the number.
        [num] if all_digits(num) && num.len() >= 7 => day_and_seq(&num[..6], &num[6..]),
        // `T01-261108-004`.
        [date, seq] if date.len() == 6 && all_digits(date) && all_digits(seq) => day_and_seq(date, seq),
        // Old format `T01-20261108-0042`: leave it as it was issued.
        [date, seq] if date.len() == 8 && all_digits(date) && all_digits(seq) => Some(format!("{code}-{date}-{seq}")),
        _ => None,
    };
    found.unwrap_or(up)
}

/// The device digit used in SKUs and in-store barcodes: the last digit of the device code (T01 → 1).
/// Supports up to nine counters per store (doc: 2–3 planned).
pub fn device_digit(device_code: &str) -> Result<u8, CodeError> {
    if !is_valid_device_code(device_code) {
        return Err(CodeError::BadDeviceCode);
    }
    let digits: String = device_code.chars().filter(char::is_ascii_digit).collect();
    match digits.parse::<u32>() {
        Ok(n @ 1..=9) => u8::try_from(n).map_err(|_| CodeError::BadDeviceCode),
        _ => Err(CodeError::BadDeviceCode),
    }
}

/// 6-digit in-store short code `{device_digit}{seq:05}` — fits a 1″×1″ label at 203 dpi (doc §7.4).
pub fn sku_code(device_digit: u8, seq: u32) -> Result<String, CodeError> {
    if !(1..=9).contains(&device_digit) {
        return Err(CodeError::BadDeviceCode);
    }
    if seq == 0 || seq > 99_999 {
        return Err(CodeError::SequenceExhausted);
    }
    Ok(format!("{device_digit}{seq:05}"))
}

/// EAN-13 check digit for the first 12 digits.
pub fn ean13_check_digit(first12: &str) -> Result<u8, CodeError> {
    if first12.len() != 12 || !first12.chars().all(|c| c.is_ascii_digit()) {
        return Err(CodeError::BadCheckDigit);
    }
    let sum: u32 =
        first12.bytes().enumerate().map(|(i, b)| u32::from(b - b'0') * if i.is_multiple_of(2) { 1 } else { 3 }).sum();
    u8::try_from((10 - sum % 10) % 10).map_err(|_| CodeError::BadCheckDigit)
}

/// EAN-8 check digit for the first 7 digits (weights 3, 1, 3, … from the left).
pub fn ean8_check_digit(first7: &str) -> Result<u8, CodeError> {
    if first7.len() != 7 || !first7.chars().all(|c| c.is_ascii_digit()) {
        return Err(CodeError::BadCheckDigit);
    }
    let sum: u32 =
        first7.bytes().enumerate().map(|(i, b)| u32::from(b - b'0') * if i.is_multiple_of(2) { 3 } else { 1 }).sum();
    u8::try_from((10 - sum % 10) % 10).map_err(|_| CodeError::BadCheckDigit)
}

pub fn is_valid_ean8(code: &str) -> bool {
    code.len() == 8
        && code.get(..7).and_then(|f| ean8_check_digit(f).ok()).is_some_and(|c| code.ends_with(&c.to_string()))
}

pub fn is_valid_ean13(code: &str) -> bool {
    code.len() == 13
        && code.get(..12).and_then(|f| ean13_check_digit(f).ok()).is_some_and(|c| code.ends_with(&c.to_string()))
}

/// The 8-digit in-store barcode `2{device_digit}{seq:05}{check}` (doc §7.4): prefix `2` (in-store use), the
/// counter's digit, the same 5-digit sequence as its SKUs, and an EAN-8 check digit so a cashier's mistyped
/// number almost never lands on another product. Printed as Code128-C (four digit pairs), it fits a 30 × 16 mm
/// label at a 2-dot module. It can never equal a generated SKU, which is 6 digits.
pub fn in_store_barcode(device_digit: u8, seq: u32) -> Result<String, CodeError> {
    if !(1..=9).contains(&device_digit) {
        return Err(CodeError::BadDeviceCode);
    }
    if seq == 0 || seq > 99_999 {
        return Err(CodeError::SequenceExhausted);
    }
    let first7 = format!("2{device_digit}{seq:05}");
    Ok(format!("{first7}{}", ean8_check_digit(&first7)?))
}

/// SQL that gives a bill's daily number as an integer, in either format: `T01-261108004` carries it from
/// character 11, an older `T01-20261108-0042` from character 14.
pub fn seq_sql(column: &str) -> String {
    format!("CAST(substr({column}, CASE WHEN instr(substr({column}, 5), '-') > 0 THEN 14 ELSE 11 END) AS INTEGER)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::business_date::parse_date;

    #[test]
    fn receipt_numbers() {
        let d = parse_date("2026-11-08").unwrap();
        assert_eq!(receipt_no("T01", d, 4), "T01-261108004");
        assert_eq!(receipt_no("T01", d, 42), "T01-261108042");
        assert_eq!(receipt_no("T02", d, 999), "T02-261108999");
        assert_eq!(receipt_no("T01", d, 1_000), "T01-2611081000", "past 999 the number just grows");
    }

    #[test]
    fn bill_numbers_are_told_apart_from_product_codes() {
        for yes in ["T01-261121004", "t01-261121004", "T03-2611211000", "T01-20261121-0004", " T02-261121004 "] {
            assert!(looks_like_bill_no(yes), "{yes}");
        }
        // Product barcodes and SKUs are digits only; short or malformed codes are not bills.
        for no in
            ["20000012", "100003", "8901234567890", "T01-4", "T01-26112", "T1-261121004", "X01-261121004", "T01", ""]
        {
            assert!(!looks_like_bill_no(no), "{no}");
        }
    }

    #[test]
    fn typed_bill_numbers_are_completed_and_cleaned() {
        let today = parse_date("2026-11-21").unwrap();
        let n = |s: &str| normalize_bill_query(s, today);
        assert_eq!(n("T01-261121004"), "T01-261121004");
        assert_eq!(n("t01-261121004"), "T01-261121004", "any case");
        assert_eq!(n("T01-261121-004"), "T01-261121004", "a dash after the date is fine");
        assert_eq!(n("t01 261121 004"), "T01-261121004", "so are spaces");
        assert_eq!(n("T1-261121004"), "T01-261121004", "T1 means T01");
        assert_eq!(n("T01-2611214"), "T01-261121004", "an unpadded daily number");
        assert_eq!(n("T01-4"), "T01-261121004", "the counter and the number alone: today's bill");
        assert_eq!(n("t1 42"), "T01-261121042");
        assert_eq!(n("T02-12345"), "T02-261121".to_owned() + "12345");
        assert_eq!(n("T01-261121-1000"), "T01-2611211000");
        assert_eq!(n("T01-20261108-0042"), "T01-20261108-0042", "an old bill stays as issued");
        assert_eq!(n("t01-20261108-42"), "T01-20261108-42");
        // Not recognised: returned upper-cased, for the lookup to say "not found".
        assert_eq!(n("T01-123456"), "T01-123456", "six digits is a date without a number");
        assert_eq!(n("T10-4"), "T10-4");
        assert_eq!(n("A1234"), "A1234");
        assert_eq!(n("  "), "");
    }

    #[test]
    fn device_codes() {
        assert!(is_valid_device_code("T01"));
        assert!(!is_valid_device_code("t01") && !is_valid_device_code("T") && !is_valid_device_code("T-01"));
        assert_eq!(device_digit("T01"), Ok(1));
        assert_eq!(device_digit("T03"), Ok(3));
        assert_eq!(device_digit("T10"), Err(CodeError::BadDeviceCode));
        assert_eq!(device_digit("T00"), Err(CodeError::BadDeviceCode));
    }

    #[test]
    fn sku_codes() {
        assert_eq!(sku_code(1, 4_523).as_deref(), Ok("104523"));
        assert_eq!(sku_code(1, 0), Err(CodeError::SequenceExhausted));
        assert_eq!(sku_code(1, 100_000), Err(CodeError::SequenceExhausted));
        assert_eq!(sku_code(0, 1), Err(CodeError::BadDeviceCode));
    }

    #[test]
    fn ean13() {
        assert_eq!(ean13_check_digit("400638133393"), Ok(1)); // 4006381333931 — published example
        assert!(is_valid_ean13("4006381333931"));
        assert!(!is_valid_ean13("4006381333932"));
        assert_eq!(ean13_check_digit("12345"), Err(CodeError::BadCheckDigit));
    }

    #[test]
    fn in_store_barcodes_are_8_digits_with_an_ean8_check_digit() {
        assert_eq!(ean8_check_digit("9638507"), Ok(4)); // 96385074, the published EAN-8 example
        assert!(is_valid_ean8("96385074"));
        assert!(!is_valid_ean8("96385075"));
        // Counter 1, sequence 42 (SKU 100042): 2·3 + 1 + 0 + 0 + 0 + 4 + 2·3 = 17 ⇒ check 3.
        assert_eq!(in_store_barcode(1, 42).as_deref(), Ok("21000423"));
        assert_eq!(in_store_barcode(9, 99_999).as_deref(), Ok("29999996"));
        for seq in [1, 42, 4_523, 99_999] {
            let code = in_store_barcode(2, seq).unwrap();
            assert_eq!(code.len(), 8);
            assert!(is_valid_ean8(&code), "{code}");
            assert_ne!(code, sku_code(2, seq).unwrap());
        }
        assert_eq!(in_store_barcode(1, 0), Err(CodeError::SequenceExhausted));
        assert_eq!(in_store_barcode(1, 100_000), Err(CodeError::SequenceExhausted));
        assert_eq!(in_store_barcode(0, 1), Err(CodeError::BadDeviceCode));
        assert_eq!(ean8_check_digit("12"), Err(CodeError::BadCheckDigit));
    }
}
