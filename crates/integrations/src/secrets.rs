//! Secret storage. Production uses the Windows Credential Manager; secrets never go to plain files or logs.

use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
#[error("secret store failed: {0}")]
pub struct SecretStoreError(String);

/// One named secret slot (e.g. the broadcaster's Twitch tokens).
pub trait SecretStore: Send + Sync {
    fn read(&self) -> Result<Option<String>, SecretStoreError>;
    fn write(&self, value: &str) -> Result<(), SecretStoreError>;
    fn delete(&self) -> Result<(), SecretStoreError>;
}

const SERVICE: &str = "streamer-toolkit";

/// Windows Credential Manager entry `streamer-toolkit` / `<key>`.
#[cfg(windows)]
pub struct CredentialManager {
    entry: keyring_core::Entry,
}

#[cfg(windows)]
impl CredentialManager {
    /// Registers the Windows store as keyring-core's default once; must run before any entry is created.
    pub fn init() -> Result<(), SecretStoreError> {
        let store = windows_native_keyring_store::Store::new().map_err(err)?;
        keyring_core::set_default_store(store);
        Ok(())
    }

    pub fn new(key: &str) -> Result<Arc<Self>, SecretStoreError> {
        Ok(Arc::new(Self {
            entry: keyring_core::Entry::new(SERVICE, key).map_err(err)?,
        }))
    }
}

#[cfg(windows)]
impl SecretStore for CredentialManager {
    fn read(&self) -> Result<Option<String>, SecretStoreError> {
        match self.entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(err(e)),
        }
    }

    fn write(&self, value: &str) -> Result<(), SecretStoreError> {
        self.entry.set_password(value).map_err(err)
    }

    fn delete(&self) -> Result<(), SecretStoreError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(err(e)),
        }
    }
}

#[cfg(windows)]
fn err(e: keyring_core::Error) -> SecretStoreError {
    // Only the error kind is kept; keyring errors never contain the secret itself.
    SecretStoreError(e.to_string())
}

#[cfg(test)]
pub(crate) mod memory {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemoryStore {
        pub value: Mutex<Option<String>>,
        pub fail_writes: std::sync::atomic::AtomicBool,
    }

    impl SecretStore for MemoryStore {
        fn read(&self) -> Result<Option<String>, SecretStoreError> {
            Ok(self.value.lock().unwrap().clone())
        }
        fn write(&self, value: &str) -> Result<(), SecretStoreError> {
            if self.fail_writes.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(SecretStoreError("write disabled".into()));
            }
            *self.value.lock().unwrap() = Some(value.into());
            Ok(())
        }
        fn delete(&self) -> Result<(), SecretStoreError> {
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }
}
