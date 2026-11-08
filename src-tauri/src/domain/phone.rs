//! Sri Lankan phone numbers (ARCHITECTURE §3.8): normalise to E.164 (`+94` + 9 national digits),
//! mask for receipts (`07X XXX X123`), format for screens (`077 123 4567`).

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PhoneError {
    #[error("phone number must contain only digits, spaces, dashes, brackets or a leading +")]
    InvalidCharacters,
    #[error("not a Sri Lankan phone number (expected 0XXXXXXXXX or +94XXXXXXXXX)")]
    NotSriLankan,
}

/// Accepts `0771234567`, `771234567`, `94771234567`, `+94 77 123 4567`, `(077) 123-4567`.
/// Mobile (07X) and landline (0XX) numbers are both valid: 9 national digits, the first non-zero.
pub fn normalize_lk(input: &str) -> Result<String, PhoneError> {
    let trimmed = input.trim();
    let mut digits = String::with_capacity(trimmed.len());
    for (i, c) in trimmed.chars().enumerate() {
        match c {
            '0'..='9' => digits.push(c),
            ' ' | '-' | '(' | ')' => {}
            '+' if i == 0 => {}
            _ => return Err(PhoneError::InvalidCharacters),
        }
    }
    let had_plus = trimmed.starts_with('+');
    let national = match digits.len() {
        11 if digits.starts_with("94") => &digits[2..],
        10 if !had_plus && digits.starts_with('0') => &digits[1..],
        9 if !had_plus => digits.as_str(),
        _ => return Err(PhoneError::NotSriLankan),
    };
    if national.len() != 9 || national.starts_with('0') {
        return Err(PhoneError::NotSriLankan);
    }
    Ok(format!("+94{national}"))
}

fn national_digits(e164: &str) -> Option<&str> {
    let n = e164.strip_prefix("+94")?;
    (n.len() == 9 && n.chars().all(|c| c.is_ascii_digit())).then_some(n)
}

/// Receipt mask, owner format `07X XXX X123`: keep the first two national-format digits and the last three.
pub fn mask_for_receipt(e164: &str) -> Option<String> {
    let n = national_digits(e164)?;
    let first = n.get(0..1)?;
    let last3 = n.get(6..9)?;
    Some(format!("0{first}X XXX X{last3}"))
}

/// Screen format `077 123 4567`.
pub fn format_national(e164: &str) -> Option<String> {
    let n = national_digits(e164)?;
    Some(format!("0{} {} {}", n.get(0..2)?, n.get(2..5)?, n.get(5..9)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_common_forms() {
        for input in ["0771234567", "771234567", "94771234567", "+94771234567", "+94 77 123 4567", "(077) 123-4567"] {
            assert_eq!(normalize_lk(input).as_deref(), Ok("+94771234567"), "{input}");
        }
        assert_eq!(normalize_lk("0112345678").as_deref(), Ok("+94112345678")); // Colombo landline
    }

    #[test]
    fn rejects_invalid() {
        assert_eq!(normalize_lk("077-ABC-4567"), Err(PhoneError::InvalidCharacters));
        assert_eq!(normalize_lk("07712345"), Err(PhoneError::NotSriLankan));
        assert_eq!(normalize_lk("+447700900123"), Err(PhoneError::NotSriLankan));
        assert_eq!(normalize_lk("0071234567"), Err(PhoneError::NotSriLankan));
        assert_eq!(normalize_lk("+0771234567"), Err(PhoneError::NotSriLankan));
        assert_eq!(normalize_lk("1+771234567"), Err(PhoneError::InvalidCharacters));
        assert_eq!(normalize_lk(""), Err(PhoneError::NotSriLankan));
    }

    #[test]
    fn masks_and_formats() {
        assert_eq!(mask_for_receipt("+94771234123").as_deref(), Some("07X XXX X123"));
        assert_eq!(format_national("+94771234567").as_deref(), Some("077 123 4567"));
        assert_eq!(mask_for_receipt("+4477"), None);
    }
}
