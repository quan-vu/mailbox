//! Secret stores (MB-113, Desktop half). `KeychainStore` keeps secrets in the OS credential
//! store (macOS Keychain, Windows Credential Manager, Secret Service on Linux). Nothing here
//! writes a secret to disk, a log or an error message.
use mailbox_core::MailboxError;
use mailbox_core::secrets::{Secret, SecretStore, is_valid_reference};
use std::collections::HashMap;
use std::sync::Mutex;

/// Keychain "service" name. Entries are listed under it in the OS credential manager.
pub const SERVICE: &str = "app.mailbox.client";

fn check(reference: &str) -> Result<(), MailboxError> {
    if is_valid_reference(reference) {
        Ok(())
    } else {
        Err(MailboxError::Storage("secret_reference"))
    }
}

/// OS keychain store. The reference is used as the keychain account name.
#[derive(Debug, Clone)]
pub struct KeychainStore {
    service: String,
}

impl Default for KeychainStore {
    fn default() -> Self {
        Self {
            service: SERVICE.to_string(),
        }
    }
}

impl KeychainStore {
    /// A separate service name, so tests never touch real MailBox entries.
    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }

    fn entry(&self, reference: &str) -> Result<keyring::Entry, MailboxError> {
        check(reference)?;
        keyring::Entry::new(&self.service, reference)
            .map_err(|_| MailboxError::Storage("keychain_entry"))
    }
}

impl SecretStore for KeychainStore {
    fn put(&self, reference: &str, secret: &Secret) -> Result<(), MailboxError> {
        self.entry(reference)?
            .set_password(secret.expose())
            .map_err(|_| MailboxError::Storage("keychain_write"))
    }

    fn get(&self, reference: &str) -> Result<Secret, MailboxError> {
        match self.entry(reference)?.get_password() {
            Ok(value) => Ok(Secret::new(value)),
            Err(keyring::Error::NoEntry) => Err(MailboxError::NotFound),
            // The keyring error is dropped on purpose: some backends include item attributes in it.
            Err(_) => Err(MailboxError::Storage("keychain_read")),
        }
    }

    fn delete(&self, reference: &str) -> Result<(), MailboxError> {
        match self.entry(reference)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(MailboxError::Storage("keychain_delete")),
        }
    }
}

/// In-memory store for tests and for the demo, where no real secret exists.
#[derive(Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn put(&self, reference: &str, secret: &Secret) -> Result<(), MailboxError> {
        check(reference)?;
        let mut map = self
            .entries
            .lock()
            .map_err(|_| MailboxError::Storage("secret_lock"))?;
        map.insert(reference.to_string(), secret.expose().to_string());
        Ok(())
    }

    fn get(&self, reference: &str) -> Result<Secret, MailboxError> {
        check(reference)?;
        let map = self
            .entries
            .lock()
            .map_err(|_| MailboxError::Storage("secret_lock"))?;
        map.get(reference)
            .map(|v| Secret::new(v.clone()))
            .ok_or(MailboxError::NotFound)
    }

    fn delete(&self, reference: &str) -> Result<(), MailboxError> {
        check(reference)?;
        let mut map = self
            .entries
            .lock()
            .map_err(|_| MailboxError::Storage("secret_lock"))?;
        map.remove(reference);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract every secret store must satisfy.
    fn contract(store: &impl SecretStore, reference: &str) {
        let _ = store.delete(reference);
        assert!(matches!(store.get(reference), Err(MailboxError::NotFound)));

        store.put(reference, &Secret::new("first value")).unwrap();
        assert_eq!(store.get(reference).unwrap().expose(), "first value");

        // Rotation replaces the value.
        store
            .put(reference, &Secret::new("rotated ✓ value"))
            .unwrap();
        assert_eq!(store.get(reference).unwrap().expose(), "rotated ✓ value");

        // Delete is idempotent and leaves nothing behind.
        store.delete(reference).unwrap();
        store.delete(reference).unwrap();
        assert!(matches!(store.get(reference), Err(MailboxError::NotFound)));

        // Unsafe references are refused before reaching the backend.
        for bad in ["", "a/b", "two words"] {
            assert!(matches!(
                store.put(bad, &Secret::new("x")),
                Err(MailboxError::Storage("secret_reference"))
            ));
        }
    }

    #[test]
    fn memory_store_meets_the_contract() {
        contract(&MemoryStore::default(), "test:acc-1");
    }

    #[test]
    fn errors_never_contain_the_secret() {
        let store = MemoryStore::default();
        let err = store
            .put("bad ref", &Secret::new("hunter2-very-secret"))
            .unwrap_err();
        assert!(!format!("{err} {err:?}").contains("hunter2"));
    }

    /// Writes and removes one dummy entry in the real OS keychain under a test-only service.
    /// Run with: `cargo test -p mailbox-secrets -- --ignored`
    #[test]
    #[ignore = "touches the real OS keychain"]
    fn os_keychain_meets_the_contract() {
        contract(
            &KeychainStore::with_service("app.mailbox.client.test"),
            "test:contract-entry",
        );
    }
}
