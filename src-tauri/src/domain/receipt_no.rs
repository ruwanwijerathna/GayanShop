//! Offline-unique document numbers: `{device_code}-{YYYYMMDD}-{seq:04}` (ARCHITECTURE §6.4).

use chrono::NaiveDate;

/// Device codes are short uppercase identifiers such as `T01`.
pub fn is_valid_device_code(code: &str) -> bool {
    (2..=4).contains(&code.len()) && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

pub fn receipt_no(device_code: &str, business_date: NaiveDate, seq: u32) -> String {
    format!("{device_code}-{}-{seq:04}", business_date.format("%Y%m%d"))
}

pub fn credit_note_code(device_code: &str, business_date: NaiveDate, seq: u32) -> String {
    format!("CN-{}", receipt_no(device_code, business_date, seq))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::business_date::parse_date;

    #[test]
    fn formats() {
        let d = parse_date("2026-11-08").unwrap();
        assert_eq!(receipt_no("T01", d, 42), "T01-20261108-0042");
        assert_eq!(credit_note_code("T02", d, 3), "CN-T02-20261108-0003");
        assert_eq!(receipt_no("T01", d, 12_345), "T01-20261108-12345");
    }

    #[test]
    fn device_codes() {
        assert!(is_valid_device_code("T01"));
        assert!(!is_valid_device_code("t01"));
        assert!(!is_valid_device_code("T"));
        assert!(!is_valid_device_code("T-01"));
    }
}
