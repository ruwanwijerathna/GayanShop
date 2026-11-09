//! Where the device identity's refresh token lives (ARCHITECTURE §1.3): the Windows Credential Manager,
//! through the `keyring` crate. It never goes into `pos.db`, a backup or a log. Tests use the in-memory store.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::{AppError, AppResult};

/// The Credential Manager "service" every entry of this app is filed under.
pub const KEYRING_SERVICE: &str = "com.gayanshop.pos";

/// The credential holding a device's refresh token.
pub fn refresh_token_account(device_id: &str) -> String {
    format!("sync.refresh_token.{device_id}")
}

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> AppResult<Option<String>>;
    fn set(&self, account: &str, secret: &str) -> AppResult<()>;
    fn delete(&self, account: &str) -> AppResult<()>;
}

/// The operating system's credential store (Windows Credential Manager on the counters).
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: &str) -> Self {
        Self { service: service.to_owned() }
    }

    fn entry(&self, account: &str) -> AppResult<keyring::Entry> {
        keyring::Entry::new(&self.service, account).map_err(|e| AppError::internal(format!("keyring: {e}")))
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        match self.entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::internal(format!("keyring read: {e}"))),
        }
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        self.entry(account)?.set_password(secret).map_err(|e| AppError::internal(format!("keyring write: {e}")))
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::internal(format!("keyring delete: {e}"))),
        }
    }
}

/// A process-local store, for tests and drills.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        Ok(self.0.lock()?.get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        self.0.lock()?.insert(account.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        self.0.lock()?.remove(account);
        Ok(())
    }
}
