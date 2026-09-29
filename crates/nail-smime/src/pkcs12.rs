//! PKCS#12 (`.p12` / `.pfx`) import with the pure-Rust `p12-keystore`
//! (MIT / Apache-2.0): modern PBES2 / AES files (OpenSSL 3, Windows 10+)
//! and legacy 3DES / RC2 ones (older Windows and browser exports).

use p12_keystore::{KeyStore, KeyStoreEntry};
use zeroize::Zeroizing;

use crate::cert::Cert;
use crate::error::{Error, Result};
use crate::keys::SoftwareKey;

/// The own certificate, its private key and the chain from a PKCS#12 file.
pub struct Pkcs12Contents {
    /// The private key, unencrypted PKCS#8 DER (zeroized on drop).
    pub pkcs8: Zeroizing<Vec<u8>>,
    /// The parsed private key.
    pub key: SoftwareKey,
    /// The certificate of the key.
    pub cert: Cert,
    /// The other certificates in the file (intermediates, root).
    pub chain: Vec<Cert>,
}

impl std::fmt::Debug for Pkcs12Contents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pkcs12Contents")
            .field("cert", &self.cert)
            .field("chain", &self.chain)
            .finish_non_exhaustive()
    }
}

/// Opens a PKCS#12 file. A wrong password is [`Error::WrongPassphrase`];
/// a file without a private key is [`Error::NoKey`]. The first key entry
/// is used (files with several keys are rare for mail).
pub fn parse(data: &[u8], password: &str) -> Result<Pkcs12Contents> {
    let store = KeyStore::from_pkcs12(data, password).map_err(|e| match e {
        p12_keystore::error::Error::MacError(_) | p12_keystore::error::Error::UnpadError => {
            Error::WrongPassphrase
        }
        other => Error::malformed(format!("PKCS#12: {other}")),
    })?;
    let (_, chain) = store
        .private_key_chain()
        .ok_or_else(|| Error::NoKey("the PKCS#12 file holds no private key".into()))?;
    let mut certs = chain
        .chain()
        .iter()
        .map(|c| Cert::from_der(c.as_der()))
        .collect::<Result<Vec<_>>>()?;
    if certs.is_empty() {
        return Err(Error::NoKey("the PKCS#12 key has no certificate".into()));
    }
    let cert = certs.remove(0);
    // Loose certificates (e.g. a root without a key id) join the chain.
    for (_, entry) in store.entries() {
        if let KeyStoreEntry::Certificate(extra) = entry {
            let extra = Cert::from_der(extra.as_der())?;
            if extra != cert && !certs.contains(&extra) {
                certs.push(extra);
            }
        }
    }
    let pkcs8 = Zeroizing::new(chain.key().to_vec());
    let key = SoftwareKey::from_pkcs8_der(&pkcs8)?;
    if key.public_key() != cert.public_key()? {
        return Err(Error::Crypto(
            "the private key does not belong to the certificate".into(),
        ));
    }
    Ok(Pkcs12Contents {
        pkcs8,
        key,
        cert,
        chain: certs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{P12_PASSWORD, fixture, fixture_cert};

    #[test]
    fn opens_modern_and_legacy_files() {
        for file in ["alice.p12", "alice-legacy.p12"] {
            let p12 = parse(&fixture(file), P12_PASSWORD).unwrap();
            assert_eq!(p12.cert, fixture_cert("alice.crt"), "{file}");
            assert_eq!(
                p12.chain,
                vec![fixture_cert("intermediate-ca.crt")],
                "{file}"
            );
            assert_eq!(p12.key.public_key().algorithm_name(), "RSA 2048");
            assert!(!p12.pkcs8.is_empty());
            assert!(format!("{p12:?}").contains("Alice"));
        }
        let bob = parse(&fixture("bob.p12"), P12_PASSWORD).unwrap();
        assert_eq!(bob.cert, fixture_cert("bob.crt"));
    }

    #[test]
    fn wrong_password_is_reported_as_such() {
        for file in ["alice.p12", "alice-legacy.p12"] {
            assert!(matches!(
                parse(&fixture(file), "wrong"),
                Err(Error::WrongPassphrase)
            ));
        }
    }

    #[test]
    fn garbage_is_malformed() {
        assert!(matches!(parse(b"junk", "x"), Err(Error::Malformed(_))));
        assert_eq!(
            parse(b"junk", "x").unwrap_err().code(),
            nail_api::ErrorCode::Crypto
        );
    }
}
