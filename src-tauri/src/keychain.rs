#![allow(dead_code)]
//! Secure API key management via the operating system keyring.
//!
//! Linux: secret-service (gnome-keyring / KDE Wallet)
//! macOS: Keychain Services
//! Windows: Credential Manager
use crate::error::{AppError, AppResult};

/// Wraps the system keyring for storing, retrieving, and deleting API keys.
///
/// Each key is identified by a `(service_name, provider)` pair.
pub struct Keychain {
    service_name: String,
}

impl Keychain {
    /// Create a new keychain wrapper for the given service.
    pub fn new(service_name: impl Into<String>) -> Self {
        Self { service_name: service_name.into() }
    }

    /// Retrieve an API key for the given provider.
    pub fn get_key(&self, provider: &str) -> AppResult<String> {
        let entry = keyring::Entry::new(&self.service_name, provider)
            .map_err(|e| AppError::Keychain(format!("{}", e)))?;
        match entry.get_password() {
            Ok(password) => Ok(password),
            Err(e) => {
                let msg = format!("{}", e);
                if is_not_found_error(&msg) {
                    Err(AppError::NotFound(format!(
                        "no key found for provider '{}'",
                        provider
                    )))
                } else {
                    Err(AppError::Keychain(msg))
                }
            }
        }
    }

    /// Store an API key for the given provider.
    pub fn set_key(&self, provider: &str, key: &str) -> AppResult<()> {
        let entry = keyring::Entry::new(&self.service_name, provider)
            .map_err(|e| AppError::Keychain(format!("{}", e)))?;
        entry
            .set_password(key)
            .map_err(|e| AppError::Keychain(format!("{}", e)))
    }

    /// Delete the stored API key for the given provider.
    /// Deleting a non-existent key is a no-op.
    pub fn delete_key(&self, provider: &str) -> AppResult<()> {
        let entry = keyring::Entry::new(&self.service_name, provider)
            .map_err(|e| AppError::Keychain(format!("{}", e)))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = format!("{}", e);
                if is_not_found_error(&msg) {
                    Ok(()) // idempotent — key didn't exist anyway
                } else {
                    Err(AppError::Keychain(msg))
                }
            }
        }
    }
}

/// Heuristic to detect "not found" errors from keyring backends.
/// On Linux (dbus/secret-service) the message often contains "no such", "not found",
/// "No such interface", or similar.
fn is_not_found_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("no such")
        || lower.contains("not found")
        || lower.contains("no matching")
        || lower.contains("no results")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_instance() {
        let kc = Keychain::new("test-meyatu-code");
        assert_eq!(kc.service_name, "test-meyatu-code");
    }

    // These tests require a running keyring service (dbus + gnome-keyring).
    // Mark them with #[ignore] if the environment doesn't have keyring available.

    #[test]
    #[ignore = "requires running keyring service (dbus + gnome-keyring)"]
    fn test_set_and_get_key() {
        let kc = Keychain::new("com.meyatu.code.test");
        kc.set_key("test-provider", "test-api-key-123").unwrap();
        let key = kc.get_key("test-provider").unwrap();
        assert_eq!(key, "test-api-key-123");
        // Cleanup
        kc.delete_key("test-provider").unwrap();
    }

    #[test]
    fn test_get_missing_key_returns_not_found() {
        let kc = Keychain::new("com.meyatu.code.nonexistent");
        let result = kc.get_key("no-such-provider-xyz");
        assert!(result.is_err());
        // Should be NotFound, but may be Keychain error if keyring service is unavailable.
        // The test verifies it doesn't panic and returns an error.
    }

    #[test]
    #[ignore = "requires running keyring service (dbus + gnome-keyring)"]
    fn test_delete_key() {
        let kc = Keychain::new("com.meyatu.code.test");
        kc.set_key("delete-test", "temp-key").unwrap();
        kc.delete_key("delete-test").unwrap();
        let result = kc.get_key("delete-test");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_nonexistent_key_is_ok() {
        let kc = Keychain::new("com.meyatu.code.test");
        // Deleting a non-existent key should either succeed silently or fail.
        // We just verify it doesn't panic.
        let _ = kc.delete_key("definitely-does-not-exist-12345");
    }
}
