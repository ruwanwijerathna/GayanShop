//! Offline-unique document numbers and in-store codes (ARCHITECTURE §6.4, §7.4).

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

/// The `receipt_counters` row that holds a counter's running bill number. It never resets: a bill number is
/// unique for the life of the store, and short enough to type when a receipt's barcode is damaged.
pub const RECEIPT_COUNTER_KEY: &str = "receipt";

/// `A1234`: the counter's letter (T01 → A, T02 → B … T09 → I) and its running number. Bills made before v4.8 are
/// `T01-20261108-0042` and keep that number.
pub fn receipt_no(device_code: &str, seq: u32) -> Result<String, CodeError> {
    let digit = device_digit(device_code)?;
    let letter = char::from(b'A' + digit - 1);
    Ok(format!("{letter}{seq}"))
}

/// SQL that turns a `receipt_no` column into its counter code: `T01-…` bills carry it, `A1234` bills carry
/// the letter. Use it wherever a bill must be traced back to its counter.
pub fn device_code_sql(column: &str) -> String {
    format!(
        "CASE WHEN instr({column}, '-') > 0 THEN substr({column}, 1, instr({column}, '-') - 1)
              ELSE 'T0' || (unicode(substr({column}, 1, 1)) - 64) END"
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_numbers() {
        assert_eq!(receipt_no("T01", 1).as_deref(), Ok("A1"));
        assert_eq!(receipt_no("T01", 4_523).as_deref(), Ok("A4523"));
        assert_eq!(receipt_no("T02", 42).as_deref(), Ok("B42"));
        assert_eq!(receipt_no("T09", 123_456).as_deref(), Ok("I123456"));
        assert_eq!(receipt_no("T10", 1), Err(CodeError::BadDeviceCode));
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
