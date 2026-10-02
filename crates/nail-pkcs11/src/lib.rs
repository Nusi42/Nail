//! PKCS#11 smartcards for Nail (plan M4c WP122; contract ADR 0094,
//! `nail_api::smartcard`).
//!
//! Loads the configured modules ([`nail_api::smartcard::SmartcardSettings`])
//! with `cryptoki`, lists tokens and their keys / certificates
//! ([`nail_api::smartcard::SmartcardToken`]), and signs / decrypts on the
//! card, asking for the PIN through [`nail_api::smartcard::PinSource`].
//! `nail-pgp` and `nail-smime` use it for keys whose secret storage is
//! [`nail_api::security::SecretStorage::Smartcard`].
//!
//! Slot of WP122: this crate is a stub until then.

use nail_api::smartcard::SmartcardError;

/// Tries to load a PKCS#11 module (stub: only says whether the file can
/// be loaded at all).
pub fn probe_module(path: &str) -> Result<(), SmartcardError> {
    if !std::path::Path::new(path).is_file() {
        return Err(SmartcardError::ModuleNotFound);
    }
    cryptoki::context::Pkcs11::new(path)
        .map(|_| ())
        .map_err(|_| SmartcardError::ModuleLoadFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_broken_modules_are_refused() {
        assert_eq!(
            probe_module(r"C:\does\not\exist\pkcs11.dll"),
            Err(SmartcardError::ModuleNotFound)
        );
        // A file that is not a DLL: libloading fails, nothing panics.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake-pkcs11.dll");
        std::fs::write(&fake, b"not a module").unwrap();
        assert_eq!(
            probe_module(fake.to_str().unwrap()),
            Err(SmartcardError::ModuleLoadFailed)
        );
    }
}
