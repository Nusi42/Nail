//! S/MIME 4.0 for Nail (RFC 8551; M2a WP32, ADR 0031).
//!
//! Built on RustCrypto's [`cms`] and [`x509_cert`]; no OpenSSL.
//!
//! * [`mime`]: the MIME layer. [`mime::sign_entity`] (detached
//!   `multipart/signed` or opaque `signed-data`), [`mime::encrypt_entity`]
//!   (`authEnveloped-data` with AES-GCM, `enveloped-data` with AES-CBC for
//!   old clients) and [`mime::read`], which unwraps signed and encrypted
//!   layers and returns the innermost content with a
//!   [`nail_api::security::SecurityStatus`].
//! * [`signed`] / [`enveloped`]: CMS `SignedData`, `EnvelopedData` and
//!   `AuthEnvelopedData` (RSA key transport, ECDH key agreement on P-256 /
//!   P-384); [`ber`] normalizes streamed BER input.
//! * [`cert`] / [`chain`]: certificates, key usage and expiry checks, chain
//!   building and validation against a [`chain::TrustAnchors`] (the
//!   Windows trust store in the app, a fixed anchor in tests). Revocation
//!   is reported as "not checked" in M2.
//! * [`keys`]: public keys and [`keys::PrivateKeyOps`] for software keys
//!   and keys that stay in the Windows certificate store.
//! * [`pkcs12`]: `.p12` / `.pfx` import (pure Rust `p12-keystore`).
//! * [`smartcard`] (M4c WP122): keys on PKCS#11 smartcards
//!   ([`smartcard::SmartcardKey`] over `nail-pkcs11`), sign / decrypt on
//!   the card with PIN prompts.
//! * [`keyring`]: the store's keyring (`technology = smime`, fingerprint =
//!   SHA-256 of the DER certificate), private-key protection (OS keychain
//!   key or passphrase) and the protocol's `CryptoKey` view.
//! * [`windows`] (Windows only): listing and importing certificates of the
//!   "Personal" / "Other People" stores, CNG private-key operations and
//!   the system trust anchors. Its `unsafe` code lives in that module only.

#![warn(missing_docs)]

use nail_api::security::CryptoTechnology;

pub mod ber;
pub mod cert;
pub mod chain;
pub mod enveloped;
pub mod error;
pub mod keyring;
pub mod keys;
pub mod mime;
pub mod oids;
pub mod pkcs12;
pub mod signed;
pub mod smartcard;
pub mod time;
#[cfg(windows)]
pub mod windows;

#[cfg(test)]
mod testutil;

pub use cert::{Cert, Purpose};
pub use chain::{FixedTrustAnchors, TrustAnchors};
pub use enveloped::ContentCipher;
pub use error::{Error, Result};
pub use keys::{PrivateKeyOps, SoftwareKey};
pub use mime::{ReadContext, ReadOutcome, SignMode};
pub use signed::Signer;
pub use smartcard::SmartcardKey;

/// The technology this crate implements.
pub const TECHNOLOGY: CryptoTechnology = CryptoTechnology::Smime;

#[cfg(test)]
mod tests {
    #[test]
    fn technology_is_smime() {
        assert_eq!(super::TECHNOLOGY.as_str(), "smime");
    }
}
