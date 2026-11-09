//! Barcode encoders for the label engine (doc §7.4): Code128 (subset B, or C for numeric runs) and EAN-13.
//! They produce the bar/space module pattern, so the fit rules count real modules and the SVG preview draws
//! the same bars the printer will. Pure integer code.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Symbology {
    Code128,
    Ean13,
}

/// Code128 symbol patterns: bar, space, bar, space, bar, space widths (values 0–105, then STOP).
const CODE128: [[u8; 6]; 106] = [
    [2, 1, 2, 2, 2, 2],
    [2, 2, 2, 1, 2, 2],
    [2, 2, 2, 2, 2, 1],
    [1, 2, 1, 2, 2, 3],
    [1, 2, 1, 3, 2, 2],
    [1, 3, 1, 2, 2, 2],
    [1, 2, 2, 2, 1, 3],
    [1, 2, 2, 3, 1, 2],
    [1, 3, 2, 2, 1, 2],
    [2, 2, 1, 2, 1, 3],
    [2, 2, 1, 3, 1, 2],
    [2, 3, 1, 2, 1, 2],
    [1, 1, 2, 2, 3, 2],
    [1, 2, 2, 1, 3, 2],
    [1, 2, 2, 2, 3, 1],
    [1, 1, 3, 2, 2, 2],
    [1, 2, 3, 1, 2, 2],
    [1, 2, 3, 2, 2, 1],
    [2, 2, 3, 2, 1, 1],
    [2, 2, 1, 1, 3, 2],
    [2, 2, 1, 2, 3, 1],
    [2, 1, 3, 2, 1, 2],
    [2, 2, 3, 1, 1, 2],
    [3, 1, 2, 1, 3, 1],
    [3, 1, 1, 2, 2, 2],
    [3, 2, 1, 1, 2, 2],
    [3, 2, 1, 2, 2, 1],
    [3, 1, 2, 2, 1, 2],
    [3, 2, 2, 1, 1, 2],
    [3, 2, 2, 2, 1, 1],
    [2, 1, 2, 1, 2, 3],
    [2, 1, 2, 3, 2, 1],
    [2, 3, 2, 1, 2, 1],
    [1, 1, 1, 3, 2, 3],
    [1, 3, 1, 1, 2, 3],
    [1, 3, 1, 3, 2, 1],
    [1, 1, 2, 3, 1, 3],
    [1, 3, 2, 1, 1, 3],
    [1, 3, 2, 3, 1, 1],
    [2, 1, 1, 3, 1, 3],
    [2, 3, 1, 1, 1, 3],
    [2, 3, 1, 3, 1, 1],
    [1, 1, 2, 1, 3, 3],
    [1, 1, 2, 3, 3, 1],
    [1, 3, 2, 1, 3, 1],
    [1, 1, 3, 1, 2, 3],
    [1, 1, 3, 3, 2, 1],
    [1, 3, 3, 1, 2, 1],
    [3, 1, 3, 1, 2, 1],
    [2, 1, 1, 3, 3, 1],
    [2, 3, 1, 1, 3, 1],
    [2, 1, 3, 1, 1, 3],
    [2, 1, 3, 3, 1, 1],
    [2, 1, 3, 1, 3, 1],
    [3, 1, 1, 1, 2, 3],
    [3, 1, 1, 3, 2, 1],
    [3, 3, 1, 1, 2, 1],
    [3, 1, 2, 1, 1, 3],
    [3, 1, 2, 3, 1, 1],
    [3, 3, 2, 1, 1, 1],
    [3, 1, 4, 1, 1, 1],
    [2, 2, 1, 4, 1, 1],
    [4, 3, 1, 1, 1, 1],
    [1, 1, 1, 2, 2, 4],
    [1, 1, 1, 4, 2, 2],
    [1, 2, 1, 1, 2, 4],
    [1, 2, 1, 4, 2, 1],
    [1, 4, 1, 1, 2, 2],
    [1, 4, 1, 2, 2, 1],
    [1, 1, 2, 2, 1, 4],
    [1, 1, 2, 4, 1, 2],
    [1, 2, 2, 1, 1, 4],
    [1, 2, 2, 4, 1, 1],
    [1, 4, 2, 1, 1, 2],
    [1, 4, 2, 2, 1, 1],
    [2, 4, 1, 2, 1, 1],
    [2, 2, 1, 1, 1, 4],
    [4, 1, 3, 1, 1, 1],
    [2, 4, 1, 1, 1, 2],
    [1, 3, 4, 1, 1, 1],
    [1, 1, 1, 2, 4, 2],
    [1, 2, 1, 1, 4, 2],
    [1, 2, 1, 2, 4, 1],
    [1, 1, 4, 2, 1, 2],
    [1, 2, 4, 1, 1, 2],
    [1, 2, 4, 2, 1, 1],
    [4, 1, 1, 2, 1, 2],
    [4, 2, 1, 1, 1, 2],
    [4, 2, 1, 2, 1, 1],
    [2, 1, 2, 1, 4, 1],
    [2, 1, 4, 1, 2, 1],
    [4, 1, 2, 1, 2, 1],
    [1, 1, 1, 1, 4, 3],
    [1, 1, 1, 3, 4, 1],
    [1, 3, 1, 1, 4, 1],
    [1, 1, 4, 1, 1, 3],
    [1, 1, 4, 3, 1, 1],
    [4, 1, 1, 1, 1, 3],
    [4, 1, 1, 3, 1, 1],
    [1, 1, 3, 1, 4, 1],
    [1, 1, 4, 1, 3, 1],
    [3, 1, 1, 1, 4, 1],
    [4, 1, 1, 1, 3, 1],
    [2, 1, 1, 4, 1, 2],
    [2, 1, 1, 2, 1, 4],
    [2, 1, 1, 2, 3, 2],
];
const STOP: [u8; 7] = [2, 3, 3, 1, 1, 1, 2];
const START_B: u8 = 104;
const START_C: u8 = 105;
const CODE_B: u8 = 100;

/// An encoded barcode: alternating bar / space widths in modules, starting with a bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub symbology: Symbology,
    /// What the scanner reads (EAN-13 includes its check digit).
    pub data: String,
    pub widths: Vec<u8>,
}

impl Encoded {
    /// Total width in modules, without quiet zones.
    pub fn modules(&self) -> u32 {
        self.widths.iter().map(|w| u32::from(*w)).sum()
    }
}

/// Code128 symbol values: subset C for an all-digit code (with a final CODE B for an odd last digit), else B.
pub fn code128_values(data: &str) -> Option<Vec<u8>> {
    if data.is_empty() || !data.chars().all(|c| (' '..='~').contains(&c)) {
        return None;
    }
    let mut values = Vec::new();
    if data.len() >= 2 && data.bytes().all(|b| b.is_ascii_digit()) {
        values.push(START_C);
        let bytes = data.as_bytes();
        let pairs = bytes.len() / 2;
        for i in 0..pairs {
            values.push((bytes[2 * i] - b'0') * 10 + (bytes[2 * i + 1] - b'0'));
        }
        if bytes.len() % 2 == 1 {
            values.push(CODE_B);
            values.push(bytes[bytes.len() - 1] - b' ');
        }
    } else {
        values.push(START_B);
        values.extend(data.bytes().map(|b| b - b' '));
    }
    let check = values.iter().enumerate().fold(0u32, |acc, (i, v)| {
        let weight = if i == 0 { 1 } else { u32::try_from(i).unwrap_or(0) };
        acc + u32::from(*v) * weight
    }) % 103;
    values.push(u8::try_from(check).unwrap_or(0));
    Some(values)
}

pub fn code128(data: &str) -> Option<Encoded> {
    let values = code128_values(data)?;
    let mut widths = Vec::with_capacity(values.len() * 6 + 7);
    for v in values {
        widths.extend_from_slice(CODE128.get(usize::from(v))?);
    }
    widths.extend_from_slice(&STOP);
    Some(Encoded { symbology: Symbology::Code128, data: data.to_owned(), widths })
}

/// EAN-13 check digit for 12 digits.
pub fn ean13_check(first12: &str) -> Option<u8> {
    if first12.len() != 12 || !first12.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let sum: u32 = first12.bytes().enumerate().map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 }).sum();
    u8::try_from((10 - sum % 10) % 10).ok()
}

/// A valid 13-digit EAN/UPC (check digit included).
pub fn is_ean13(code: &str) -> bool {
    code.len() == 13
        && code.bytes().all(|b| b.is_ascii_digit())
        && ean13_check(&code[..12]) == Some(code.as_bytes()[12] - b'0')
}

const EAN_L: [[u8; 4]; 10] = [
    [3, 2, 1, 1],
    [2, 2, 2, 1],
    [2, 1, 2, 2],
    [1, 4, 1, 1],
    [1, 1, 3, 2],
    [1, 2, 3, 1],
    [1, 1, 1, 4],
    [1, 3, 1, 2],
    [1, 2, 1, 3],
    [3, 1, 1, 2],
];
/// Parity of digits 2–7 (L = false, G = true) chosen by the first digit.
const EAN_PARITY: [[bool; 6]; 10] = [
    [false, false, false, false, false, false],
    [false, false, true, false, true, true],
    [false, false, true, true, false, true],
    [false, false, true, true, true, false],
    [false, true, false, false, true, true],
    [false, true, true, false, false, true],
    [false, true, true, true, false, false],
    [false, true, false, true, false, true],
    [false, true, false, true, true, false],
    [false, true, true, false, true, false],
];

pub fn ean13(code: &str) -> Option<Encoded> {
    if !is_ean13(code) {
        return None;
    }
    let d: Vec<usize> = code.bytes().map(|b| usize::from(b - b'0')).collect();
    // Widths alternate bar/space starting with a bar: guard 1,1,1 then the left digits (space-first patterns).
    let mut widths: Vec<u8> = vec![1, 1, 1];
    let parity = EAN_PARITY[d[0]];
    for (i, digit) in d[1..7].iter().enumerate() {
        let l = EAN_L[*digit];
        // L codes start with a space; G codes are the mirror of R (= L widths reversed), also space-first.
        let pattern = if parity[i] { [l[3], l[2], l[1], l[0]] } else { l };
        widths.extend_from_slice(&pattern);
    }
    widths.extend_from_slice(&[1, 1, 1, 1, 1]); // centre guard: space, bar, space, bar, space
    for digit in &d[7..13] {
        widths.extend_from_slice(&EAN_L[*digit]); // R codes: bar-first, same widths as L
    }
    widths.extend_from_slice(&[1, 1, 1]);
    Some(Encoded { symbology: Symbology::Ean13, data: code.to_owned(), widths })
}

/// Encode with the template's symbology rule: EAN-13 for a valid 13-digit code under `auto`, else Code128.
pub fn encode(code: &str, preferred: Option<Symbology>) -> Option<Encoded> {
    match preferred {
        Some(Symbology::Ean13) => ean13(code),
        Some(Symbology::Code128) => code128(code),
        None if is_ean13(code) => ean13(code),
        None => code128(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code128_c_for_a_six_digit_sku_is_68_modules() {
        let e = code128("104523").unwrap();
        // START C, 10, 45, 23, check, STOP = 5 × 11 + 13.
        assert_eq!(e.modules(), 68);
        assert_eq!(code128_values("104523").unwrap(), vec![105, 10, 45, 23, 68]);
    }

    #[test]
    fn code128_b_and_odd_digit_runs() {
        let b = code128("AB-1").unwrap();
        assert_eq!(b.modules(), 11 * (1 + 4 + 1) + 13);
        let odd = code128("12345").unwrap();
        assert_eq!(code128_values("12345").unwrap()[..5], [105, 12, 34, 100, 21]);
        assert_eq!(odd.modules(), 11 * 6 + 13);
        assert!(code128("නිල්").is_none(), "only printable ASCII");
    }

    #[test]
    fn every_symbol_is_eleven_modules() {
        assert!(CODE128.iter().all(|p| p.iter().map(|w| u32::from(*w)).sum::<u32>() == 11));
        assert_eq!(STOP.iter().map(|w| u32::from(*w)).sum::<u32>(), 13);
    }

    #[test]
    fn ean13_is_95_modules_with_a_valid_check_digit() {
        assert_eq!(ean13_check("479123456789"), Some(1));
        assert!(is_ean13("4791234567891"));
        assert!(!is_ean13("4791234567895"));
        let e = ean13("4791234567891").unwrap();
        assert_eq!(e.modules(), 95);
        assert_eq!(e.widths.len(), 3 + 6 * 4 + 5 + 6 * 4 + 3);
        assert_eq!(encode("4791234567891", None).unwrap().symbology, Symbology::Ean13);
        assert_eq!(encode("104523", None).unwrap().symbology, Symbology::Code128);
    }
}
