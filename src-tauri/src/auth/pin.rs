//! Staff PIN hashing (argon2id) and validation.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::error::{AppError, AppResult};

pub const MAX_FAILED_ATTEMPTS: u32 = 5;
pub const LOCKOUT_MINUTES: i64 = 5;

#[derive(Clone)]
pub struct PinHasher {
    params: Params,
}

impl PinHasher {
    /// ~100 ms on counter hardware: 19 MiB, 2 passes (OWASP argon2id baseline).
    pub fn production() -> Self {
        Self { params: Params::new(19 * 1024, 2, 1, None).expect("valid argon2 params") }
    }

    /// Deliberately weak parameters so tests stay fast. Never use outside tests.
    pub fn fast_for_tests() -> Self {
        Self { params: Params::new(256, 1, 1, None).expect("valid argon2 params") }
    }

    fn argon(&self) -> Argon2<'static> {
        Argon2::new(Algorithm::Argon2id, Version::V0x13, self.params.clone())
    }

    pub fn hash(&self, pin: &str) -> AppResult<String> {
        let salt = SaltString::generate(&mut OsRng);
        self.argon()
            .hash_password(pin.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| AppError::internal(format!("pin hash: {e}")))
    }

    /// Verifies with the parameters embedded in the stored PHC string.
    pub fn verify(&self, pin: &str, stored: &str) -> bool {
        match PasswordHash::new(stored) {
            Ok(parsed) => Argon2::default().verify_password(pin.as_bytes(), &parsed).is_ok(),
            Err(_) => false,
        }
    }
}

/// PINs are 4–8 digits; Manager/Owner roles need at least 6.
pub fn validate_pin(pin: &str, min_len: usize) -> AppResult<()> {
    let ok_chars = pin.chars().all(|c| c.is_ascii_digit());
    if !ok_chars || pin.len() < min_len.max(4) || pin.len() > 8 {
        return Err(AppError::validation(format!("PIN must be {}–8 digits", min_len.max(4))));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let h = PinHasher::fast_for_tests();
        let stored = h.hash("123456").unwrap();
        assert!(stored.starts_with("$argon2id$"));
        assert!(h.verify("123456", &stored));
        assert!(!h.verify("123457", &stored));
        assert!(!h.verify("123456", "garbage"));
        // salts differ
        assert_ne!(stored, h.hash("123456").unwrap());
    }

    #[test]
    fn pin_rules() {
        assert!(validate_pin("1234", 4).is_ok());
        assert!(validate_pin("123", 4).is_err());
        assert!(validate_pin("12345", 6).is_err());
        assert!(validate_pin("123456789", 4).is_err());
        assert!(validate_pin("12a4", 4).is_err());
    }
}
