//! The vault recovery key (M4c WP122, ADR 0095; text form in
//! [`nail_api::recovery`]).
//!
//! 32 random bytes, shown to the user once as 56 characters with check
//! characters, never stored. A wrapping key derived from it seals the data
//! key; the sealed data key goes into the vault file beside the database
//! (readable while the database file is encrypted). With the recovery key
//! the data key can be put back into the keychain after the keychain entry
//! was lost (new Windows profile, reinstalled system, wiped credentials).

use nail_api::recovery::{
    RECOVERY_KEY_BYTES, RecoveryKeyStatus, format_recovery_key, parse_recovery_key,
};
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::error::VaultError;
use crate::format::{self, DataKey};
use crate::key::{KeyStore, derive, verify_key_check};
use crate::vaultfile::{RecoveryRecord, VaultFile};
use nail_api::SecretRef;

/// Context of the wrapping-key derivation.
const WRAPPING_KEY_CONTEXT: &[u8] = b"nail/vault/recovery-key/v1";
/// Associated data of the wrapped data key.
const WRAPPED_KEY_CONTEXT: &[u8] = b"nail/vault/recovery-wrapped-data-key";

/// A recovery key in memory (zeroed on drop, never printed).
#[derive(Clone)]
pub struct RecoveryKey(Zeroizing<[u8; RECOVERY_KEY_BYTES]>);

impl std::fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryKey(..)")
    }
}

impl RecoveryKey {
    /// A fresh random key (256 bits from the OS generator).
    pub fn generate() -> Self {
        let mut bytes = Zeroizing::new([0u8; RECOVERY_KEY_BYTES]);
        OsRng.fill_bytes(bytes.as_mut());
        Self(bytes)
    }

    /// Parses the typed text form (forgiving case, spaces, dashes and
    /// look-alike characters).
    pub fn parse(text: &str) -> Result<Self, VaultError> {
        parse_recovery_key(text)
            .map(|bytes| Self(Zeroizing::new(bytes)))
            .map_err(VaultError::RecoveryKeyInvalid)
    }

    /// The text form shown to the user (14 groups of 4 characters).
    pub fn to_text(&self) -> Zeroizing<String> {
        Zeroizing::new(format_recovery_key(&self.0))
    }

    fn wrapping_key(&self) -> DataKey {
        DataKey::from_bytes(*derive(&self.0, WRAPPING_KEY_CONTEXT))
    }

    /// Seals the data key with this recovery key (hex for the vault file).
    pub(crate) fn wrap(&self, data_key: &DataKey) -> Result<String, VaultError> {
        let sealed = format::seal(&self.wrapping_key(), WRAPPED_KEY_CONTEXT, data_key.bytes())?;
        Ok(hex::encode(sealed))
    }

    /// Opens a wrapped data key; [`VaultError::RecoveryKeyWrong`] when this
    /// recovery key did not wrap it.
    pub(crate) fn unwrap(&self, wrapped: &str) -> Result<DataKey, VaultError> {
        let sealed = hex::decode(wrapped)
            .map_err(|_| VaultError::VaultFile("the wrapped data key is malformed".into()))?;
        let plain = Zeroizing::new(
            format::open(&self.wrapping_key(), WRAPPED_KEY_CONTEXT, &sealed)
                .map_err(|_| VaultError::RecoveryKeyWrong)?,
        );
        let bytes: [u8; 32] = plain
            .as_slice()
            .try_into()
            .map_err(|_| VaultError::VaultFile("the wrapped data key has a wrong length".into()))?;
        Ok(DataKey::from_bytes(bytes))
    }
}

/// The recovery part of [`nail_api::security::VaultStatus`].
pub(crate) fn status_of(file: &VaultFile) -> RecoveryKeyStatus {
    match &file.recovery {
        Some(record) => RecoveryKeyStatus {
            configured: true,
            created_at: Some(record.created_at.clone()),
            verified_at: record.verified_at.clone(),
        },
        None => RecoveryKeyStatus::default(),
    }
}

/// A new recovery record for `data_key`.
pub(crate) fn new_record(
    recovery: &RecoveryKey,
    data_key: &DataKey,
) -> Result<RecoveryRecord, VaultError> {
    Ok(RecoveryRecord {
        wrapped_key: recovery.wrap(data_key)?,
        created_at: nail_util::time::rfc3339_now(),
        verified_at: None,
    })
}

/// Opens the data key recorded in the vault file with a typed recovery
/// key and checks it against the key check value.
pub(crate) fn open_data_key(file: &VaultFile, text: &str) -> Result<DataKey, VaultError> {
    let recovery = RecoveryKey::parse(text)?;
    let record = file.recovery.as_ref().ok_or(VaultError::NoRecoveryKey)?;
    let key = recovery.unwrap(&record.wrapped_key)?;
    if let Some(check) = file.key_check_bytes()? {
        verify_key_check(&key, &check).map_err(|_| VaultError::RecoveryKeyWrong)?;
    }
    Ok(key)
}

/// Puts the data key opened with the recovery key back into the keychain
/// under the vault file's reference.
pub(crate) fn restore_into_keychain(
    file: &VaultFile,
    keys: &KeyStore,
    text: &str,
) -> Result<DataKey, VaultError> {
    let key = open_data_key(file, text)?;
    let key_ref = file
        .key_ref
        .as_deref()
        .ok_or_else(|| VaultError::VaultFile("the vault file names no data key".into()))?;
    keys.store(&SecretRef(key_ref.to_owned()), &key)?;
    tracing::info!("data key restored into the keychain with the recovery key");
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::key_check;
    use nail_api::recovery::RecoveryKeyIssue;

    #[test]
    fn text_round_trip_and_redaction() {
        let key = RecoveryKey::generate();
        let text = key.to_text();
        assert_eq!(text.len(), 56 + 13);
        let parsed = RecoveryKey::parse(&text.to_lowercase()).unwrap();
        assert_eq!(*parsed.0, *key.0);
        assert_eq!(format!("{key:?}"), "RecoveryKey(..)");
        assert_ne!(*RecoveryKey::generate().0, *key.0);
        assert!(matches!(
            RecoveryKey::parse(""),
            Err(VaultError::RecoveryKeyInvalid(RecoveryKeyIssue::Empty))
        ));
    }

    #[test]
    fn wrap_unwrap_and_wrong_keys() {
        let data = DataKey::generate();
        let recovery = RecoveryKey::generate();
        let wrapped = recovery.wrap(&data).unwrap();
        assert!(!wrapped.contains(data.to_hex().as_str()));
        assert_eq!(recovery.unwrap(&wrapped).unwrap().to_hex(), data.to_hex());
        assert!(matches!(
            RecoveryKey::generate().unwrap(&wrapped),
            Err(VaultError::RecoveryKeyWrong)
        ));
        assert!(matches!(
            recovery.unwrap("zz"),
            Err(VaultError::VaultFile(_))
        ));
    }

    #[test]
    fn open_checks_the_key_check_value() {
        let data = DataKey::generate();
        let recovery = RecoveryKey::generate();
        let mut file = VaultFile {
            key_ref: Some("keychain:nail/vault-0/data-key".into()),
            key_check: Some(hex::encode(key_check(&data).unwrap())),
            recovery: Some(new_record(&recovery, &data).unwrap()),
            ..VaultFile::default()
        };
        assert!(status_of(&file).configured);
        let opened = open_data_key(&file, &recovery.to_text()).unwrap();
        assert_eq!(opened.to_hex(), data.to_hex());
        // A recovery record of another data key (stale) is refused.
        file.recovery = Some(new_record(&recovery, &DataKey::generate()).unwrap());
        assert!(matches!(
            open_data_key(&file, &recovery.to_text()),
            Err(VaultError::RecoveryKeyWrong)
        ));
        file.recovery = None;
        assert!(matches!(
            open_data_key(&file, &recovery.to_text()),
            Err(VaultError::NoRecoveryKey)
        ));
        assert_eq!(status_of(&file), RecoveryKeyStatus::default());
    }
}
