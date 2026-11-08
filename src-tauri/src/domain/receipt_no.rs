//! Offline-unique document numbers and in-store codes (ARCHITECTURE §6.4, §7.4).

use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodeError {
    #[error("device code must be 2–4 uppercase letters/digits ending in a digit 1–9 (e.g. T01)")]
    BadDeviceCode,
    #[error("in-store code sequence exhausted for this device")]
    SequenceExhausted,
    #[error("not a valid EAN-13 code")]
    BadEan13,
}

/// Device codes are short uppercase identifiers such as `T01`.
pub fn is_valid_device_code(code: &str) -> bool {
    (2..=4).contains(&code.len()) && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// `T01-20261108-0042` — the business date, not the calendar date.
pub fn receipt_no(device_code: &str, business_date: NaiveDate, seq: u32) -> String {
    format!("{device_code}-{}-{seq:04}", business_date.format("%Y%m%d"))
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
        return Err(CodeError::BadEan13);
    }
    let sum: u32 =
        first12.bytes().enumerate().map(|(i, b)| u32::from(b - b'0') * if i.is_multiple_of(2) { 1 } else { 3 }).sum();
    u8::try_from((10 - sum % 10) % 10).map_err(|_| CodeError::BadEan13)
}

pub fn is_valid_ean13(code: &str) -> bool {
    code.len() == 13
        && code.get(..12).and_then(|f| ean13_check_digit(f).ok()).is_some_and(|c| code.ends_with(&c.to_string()))
}

/// In-store EAN-13: prefix `2` (reserved for in-store use) + device digit + 10-digit sequence + check digit.
pub fn in_store_ean13(device_digit: u8, seq: u64) -> Result<String, CodeError> {
    if !(1..=9).contains(&device_digit) {
        return Err(CodeError::BadDeviceCode);
    }
    if seq == 0 || seq > 9_999_999_999 {
        return Err(CodeError::SequenceExhausted);
    }
    let first12 = format!("2{device_digit}{seq:010}");
    Ok(format!("{first12}{}", ean13_check_digit(&first12)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::business_date::parse_date;

    #[test]
    fn receipt_numbers() {
        let d = parse_date("2026-11-08").unwrap();
        assert_eq!(receipt_no("T01", d, 42), "T01-20261108-0042");
        assert_eq!(receipt_no("T01", d, 12_345), "T01-20261108-12345");
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
        let code = in_store_ean13(1, 12_345).unwrap();
        assert_eq!(code.len(), 13);
        assert!(code.starts_with("210000012345"));
        assert!(is_valid_ean13(&code));
        assert_eq!(ean13_check_digit("12345"), Err(CodeError::BadEan13));
    }
}
