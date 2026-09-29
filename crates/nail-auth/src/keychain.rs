//! [`KeychainSecretStore`]: [`SecretStore`] on the OS keychain (Windows
//! Credential Manager, macOS Keychain, Secret Service) through [`keyring`].
//!
//! A [`SecretRef`] `keychain:<service>/<name>` is the keychain entry with
//! service `<service>` and user `<name>`. Values are stored as UTF-8 bytes.
//!
//! Windows limits one credential blob to 2560 bytes; Microsoft refresh
//! tokens and PEM client certificates can be longer. Values above
//! [`CHUNK_BYTES`] are therefore split: the main entry holds a marker with
//! the chunk count and `<name>#1` … `<name>#n` hold the parts. Chunks are
//! written before the marker, so a reader never sees a marker without its
//! chunks, and left-over chunks of a longer old value are removed.

use std::fmt;

use nail_api::ErrorCode;
use nail_api::accounts::SecretRef;
use nail_api::engine::{EngineError, SecretStore};

/// Largest value stored in one keychain entry.
pub const CHUNK_BYTES: usize = 2000;

/// Prefix of the main entry of a chunked value.
const MARKER: &str = "nail-chunked:v1:";

/// Raw keychain access by (service, user). Implemented by [`OsKeychain`];
/// tests use an in-memory backend.
pub trait KeychainBackend: Send + Sync + fmt::Debug {
    /// The stored bytes, `None` when there is no entry.
    fn get(&self, service: &str, user: &str) -> Result<Option<Vec<u8>>, EngineError>;
    /// Creates or replaces an entry.
    fn set(&self, service: &str, user: &str, value: &[u8]) -> Result<(), EngineError>;
    /// Deletes an entry; whether it existed.
    fn delete(&self, service: &str, user: &str) -> Result<bool, EngineError>;
}

/// The platform keychain.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsKeychain;

fn keyring_error(err: keyring::Error) -> EngineError {
    use keyring::Error as E;
    // Never include secret bytes: BadEncoding / BadDataFormat carry them.
    let message = match &err {
        E::BadEncoding(_) => "keychain entry is not valid UTF-8".to_owned(),
        E::BadDataFormat(_, _) => "keychain entry has an unexpected format".to_owned(),
        other => other.to_string(),
    };
    match err {
        E::NoStorageAccess(_) => EngineError::temporary(
            ErrorCode::Unavailable,
            format!("keychain locked: {message}"),
        ),
        E::PlatformFailure(_) => EngineError::temporary(ErrorCode::Internal, message),
        E::TooLong(_, _) | E::Invalid(_, _) => {
            EngineError::permanent(ErrorCode::InvalidRequest, message)
        }
        _ => EngineError::permanent(ErrorCode::Internal, message),
    }
}

impl KeychainBackend for OsKeychain {
    fn get(&self, service: &str, user: &str) -> Result<Option<Vec<u8>>, EngineError> {
        let entry = keyring::Entry::new(service, user).map_err(keyring_error)?;
        match entry.get_secret() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(keyring_error(err)),
        }
    }

    fn set(&self, service: &str, user: &str, value: &[u8]) -> Result<(), EngineError> {
        let entry = keyring::Entry::new(service, user).map_err(keyring_error)?;
        entry.set_secret(value).map_err(keyring_error)
    }

    fn delete(&self, service: &str, user: &str) -> Result<bool, EngineError> {
        let entry = keyring::Entry::new(service, user).map_err(keyring_error)?;
        match entry.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(err) => Err(keyring_error(err)),
        }
    }
}

/// [`SecretStore`] on a keychain backend (the OS keychain by default).
#[derive(Debug, Default)]
pub struct KeychainSecretStore<B: KeychainBackend = OsKeychain> {
    backend: B,
}

impl KeychainSecretStore<OsKeychain> {
    /// The OS keychain.
    pub fn new() -> Self {
        Self {
            backend: OsKeychain,
        }
    }
}

impl<B: KeychainBackend> KeychainSecretStore<B> {
    /// A store on another backend.
    pub fn with_backend(backend: B) -> Self {
        Self { backend }
    }

    /// The backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    fn parts(secret: &SecretRef) -> Result<(&str, &str), EngineError> {
        let (service, name) = (secret.service(), secret.name());
        if service.is_empty() || name.is_empty() {
            return Err(EngineError::permanent(
                ErrorCode::InvalidRequest,
                format!("malformed keychain reference {secret}"),
            ));
        }
        Ok((service, name))
    }

    /// The chunk count of a stored main entry (0 = not chunked).
    fn chunk_count(main: &[u8]) -> Option<usize> {
        std::str::from_utf8(main)
            .ok()?
            .strip_prefix(MARKER)?
            .parse()
            .ok()
    }
}

fn chunk_name(name: &str, index: usize) -> String {
    format!("{name}#{index}")
}

impl<B: KeychainBackend> SecretStore for KeychainSecretStore<B> {
    fn get(&self, secret: &SecretRef) -> Result<Option<String>, EngineError> {
        let (service, name) = Self::parts(secret)?;
        let Some(main) = self.backend.get(service, name)? else {
            return Ok(None);
        };
        let bytes = match Self::chunk_count(&main) {
            None => main,
            Some(count) => {
                let mut out = Vec::new();
                for index in 1..=count {
                    let chunk = self
                        .backend
                        .get(service, &chunk_name(name, index))?
                        .ok_or_else(|| {
                            EngineError::permanent(
                                ErrorCode::Internal,
                                format!("keychain entry {secret} is incomplete"),
                            )
                        })?;
                    out.extend(chunk);
                }
                out
            }
        };
        String::from_utf8(bytes).map(Some).map_err(|_| {
            EngineError::permanent(
                ErrorCode::Internal,
                format!("keychain entry {secret} is not UTF-8"),
            )
        })
    }

    fn set(&self, secret: &SecretRef, value: &str) -> Result<(), EngineError> {
        let (service, name) = Self::parts(secret)?;
        let old_count = self
            .backend
            .get(service, name)?
            .and_then(|main| Self::chunk_count(&main))
            .unwrap_or(0);
        let bytes = value.as_bytes();
        let new_count = if bytes.len() <= CHUNK_BYTES && !value.starts_with(MARKER) {
            self.backend.set(service, name, bytes)?;
            0
        } else {
            let chunks: Vec<&[u8]> = bytes.chunks(CHUNK_BYTES).collect();
            for (i, chunk) in chunks.iter().enumerate() {
                self.backend.set(service, &chunk_name(name, i + 1), chunk)?;
            }
            self.backend.set(
                service,
                name,
                format!("{MARKER}{}", chunks.len()).as_bytes(),
            )?;
            chunks.len()
        };
        for index in new_count + 1..=old_count {
            self.backend.delete(service, &chunk_name(name, index))?;
        }
        Ok(())
    }

    fn delete(&self, secret: &SecretRef) -> Result<bool, EngineError> {
        let (service, name) = Self::parts(secret)?;
        let count = self
            .backend
            .get(service, name)?
            .and_then(|main| Self::chunk_count(&main))
            .unwrap_or(0);
        let existed = self.backend.delete(service, name)?;
        for index in 1..=count {
            self.backend.delete(service, &chunk_name(name, index))?;
        }
        Ok(existed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// An in-memory backend with the Windows blob limit.
    #[derive(Debug, Default)]
    struct FakeKeychain(Mutex<BTreeMap<(String, String), Vec<u8>>>);

    impl KeychainBackend for FakeKeychain {
        fn get(&self, service: &str, user: &str) -> Result<Option<Vec<u8>>, EngineError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(service.into(), user.into()))
                .cloned())
        }
        fn set(&self, service: &str, user: &str, value: &[u8]) -> Result<(), EngineError> {
            if value.len() > 2560 {
                return Err(EngineError::permanent(
                    ErrorCode::InvalidRequest,
                    "too long",
                ));
            }
            self.0
                .lock()
                .unwrap()
                .insert((service.into(), user.into()), value.to_vec());
            Ok(())
        }
        fn delete(&self, service: &str, user: &str) -> Result<bool, EngineError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .remove(&(service.into(), user.into()))
                .is_some())
        }
    }

    fn entries(store: &KeychainSecretStore<FakeKeychain>) -> Vec<String> {
        store
            .backend()
            .0
            .lock()
            .unwrap()
            .keys()
            .map(|(s, u)| format!("{s}/{u}"))
            .collect()
    }

    #[test]
    fn short_values_use_one_entry() {
        let store = KeychainSecretStore::with_backend(FakeKeychain::default());
        let r = SecretRef::parse("keychain:nail/work/password").unwrap();
        assert_eq!(store.get(&r).unwrap(), None);
        store.set(&r, "hunter2 ✓").unwrap();
        assert_eq!(store.get(&r).unwrap().as_deref(), Some("hunter2 ✓"));
        assert_eq!(entries(&store), ["nail/work/password"]);
        assert!(store.delete(&r).unwrap());
        assert!(!store.delete(&r).unwrap());
        assert!(entries(&store).is_empty());
    }

    #[test]
    fn long_values_are_chunked_and_cleaned_up() {
        let store = KeychainSecretStore::with_backend(FakeKeychain::default());
        let r = SecretRef::parse("keychain:nail/ms/oauth2").unwrap();
        let long = "r".repeat(5000);
        store.set(&r, &long).unwrap();
        assert_eq!(store.get(&r).unwrap().as_deref(), Some(long.as_str()));
        assert_eq!(entries(&store).len(), 4);

        // Shrinking removes the extra chunks; growing back works.
        let medium = "m".repeat(2500);
        store.set(&r, &medium).unwrap();
        assert_eq!(store.get(&r).unwrap().as_deref(), Some(medium.as_str()));
        assert_eq!(entries(&store).len(), 3);
        store.set(&r, "short").unwrap();
        assert_eq!(entries(&store), ["nail/ms/oauth2"]);
        assert_eq!(store.get(&r).unwrap().as_deref(), Some("short"));

        store.set(&r, &long).unwrap();
        assert!(store.delete(&r).unwrap());
        assert!(entries(&store).is_empty());
    }

    #[test]
    fn values_looking_like_markers_round_trip() {
        let store = KeychainSecretStore::with_backend(FakeKeychain::default());
        let r = SecretRef::parse("keychain:nail/x/password").unwrap();
        store.set(&r, "nail-chunked:v1:3").unwrap();
        assert_eq!(store.get(&r).unwrap().as_deref(), Some("nail-chunked:v1:3"));
    }

    #[test]
    fn missing_chunks_and_bad_refs_are_errors() {
        let store = KeychainSecretStore::with_backend(FakeKeychain::default());
        let r = SecretRef::parse("keychain:nail/x/oauth2").unwrap();
        store.set(&r, &"z".repeat(3000)).unwrap();
        store.backend().delete("nail", "x/oauth2#2").unwrap();
        assert_eq!(store.get(&r).unwrap_err().code, ErrorCode::Internal);
        let bad = SecretRef("not-a-ref".into());
        assert_eq!(store.get(&bad).unwrap_err().code, ErrorCode::InvalidRequest);
        assert!(!format!("{:?}", store.get(&bad)).contains("zzz"));
    }

    /// One real round trip through the OS keychain under a throw-away
    /// service name, cleaned up afterwards.
    #[test]
    fn os_keychain_round_trip() {
        if keyring::Entry::store_status().is_err() {
            // No credential store on this platform/session (e.g. a headless
            // Linux CI without Secret Service): the error must surface.
            let err = OsKeychain.get("nail-test", "x").unwrap_err();
            assert!(!err.message.is_empty());
            return;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let service = format!("nail-test-{}-{nanos}", std::process::id());
        let store = KeychainSecretStore::new();
        let r = SecretRef::parse(&format!("keychain:{service}/acct/password")).unwrap();
        assert_eq!(store.get(&r).unwrap(), None);
        store.set(&r, "pässword").unwrap();
        assert_eq!(store.get(&r).unwrap().as_deref(), Some("pässword"));
        let long = "t".repeat(4100);
        store.set(&r, &long).unwrap();
        assert_eq!(store.get(&r).unwrap().as_deref(), Some(long.as_str()));
        assert!(store.delete(&r).unwrap());
        assert_eq!(store.get(&r).unwrap(), None);
        assert_eq!(OsKeychain.get(&service, "acct/password#1").unwrap(), None);
    }
}
