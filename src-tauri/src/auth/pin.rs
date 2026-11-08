//! Staff PIN hashing (argon2id) and validation (ARCHITECTURE §3.5).

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand_core::OsRng;

use crate::error::{AppError, AppResult};

pub const MAX_FAILED_ATTEMPTS: u32 = 5;
pub const LOCKOUT_MINUTES: i64 = 5;
pub const MAX_PIN_LEN: usize = 8;

#[derive(Clone)]
pub struct PinHasher {
    params: Params,
}

impl PinHasher {
    /// ~100 ms on counter hardware: 19 MiB, 2 passes (OWASP argon2id baseline).
    pub fn production() -> Self {
        Self { params: Params::new(19 * 1024, 2, 1, None).unwrap_or_default() }
    }

    /// Deliberately weak parameters so tests stay fast. Never use outside tests.
    pub fn fast_for_tests() -> Self {
        Self { params: Params::new(256, 1, 1, None).unwrap_or_default() }
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
        PasswordHash::new(stored).is_ok_and(|parsed| Argon2::default().verify_password(pin.as_bytes(), &parsed).is_ok())
    }
}

/// PINs are digits only, `min_len`–8 long (min 4; Manager/Owner roles need 6).
pub fn validate_pin(pin: &str, min_len: usize) -> AppResult<()> {
    let min_len = min_len.max(4);
    if !pin.chars().all(|c| c.is_ascii_digit()) || pin.is_empty() {
        return Err(AppError::validation("PIN_NOT_DIGITS", "PIN must contain digits only"));
    }
    if pin.len() < min_len {
        return Err(AppError::validation("PIN_TOO_SHORT", format!("PIN needs at least {min_len} digits")));
    }
    if pin.len() > MAX_PIN_LEN {
        return Err(AppError::validation("PIN_TOO_LONG", format!("PIN allows at most {MAX_PIN_LEN} digits")));
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
        assert_ne!(stored, h.hash("123456").unwrap(), "salts differ");
    }

    #[test]
    fn production_params_are_the_owasp_baseline() {
        let stored = PinHasher::production().hash("2468").unwrap();
        assert!(stored.contains("m=19456,t=2,p=1"), "{stored}");
    }

    #[test]
    fn pin_rules() {
        assert!(validate_pin("1234", 4).is_ok());
        assert_eq!(validate_pin("123", 4).unwrap_err().i18n_key(), "errors.VALIDATION.PIN_TOO_SHORT");
        assert_eq!(validate_pin("12345", 6).unwrap_err().i18n_key(), "errors.VALIDATION.PIN_TOO_SHORT");
        assert_eq!(validate_pin("123456789", 4).unwrap_err().i18n_key(), "errors.VALIDATION.PIN_TOO_LONG");
        assert_eq!(validate_pin("12a4", 4).unwrap_err().i18n_key(), "errors.VALIDATION.PIN_NOT_DIGITS");
        assert_eq!(validate_pin("", 4).unwrap_err().i18n_key(), "errors.VALIDATION.PIN_NOT_DIGITS");
    }
}
