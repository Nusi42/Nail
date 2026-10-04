//! Test helpers: the checked-in vectors of `corpus/crypto/smime/`.

use nail_testkit::{CryptoFixtures, crypto_fixture};

use crate::cert::Cert;
use crate::chain::FixedTrustAnchors;
use crate::keys::SoftwareKey;

/// A fixed "now" for validation: 2026-09-21, inside every good test
/// certificate's validity (2020–2125) and after carol's (2020–2021).
pub(crate) const AT: i64 = 1_790_000_000;

/// The PKCS#12 password of the vectors.
pub(crate) const P12_PASSWORD: &str = "nail-test";

/// The bytes of a vector.
pub(crate) fn fixture(name: &str) -> Vec<u8> {
    crypto_fixture(CryptoFixtures::Smime, name)
        .unwrap_or_else(|| panic!("missing S/MIME test vector {name}"))
}

/// A PEM certificate vector.
pub(crate) fn fixture_cert(name: &str) -> Cert {
    Cert::parse_many(&fixture(name))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .remove(0)
}

/// A PEM PKCS#8 private key vector (`<name>.key`).
pub(crate) fn fixture_key(name: &str) -> SoftwareKey {
    let pem = String::from_utf8(fixture(name)).unwrap();
    let (label, der) = der::pem::decode_vec(pem.as_bytes()).unwrap();
    assert_eq!(label, "PRIVATE KEY");
    SoftwareKey::from_pkcs8_der(&der).unwrap()
}

/// Trusts the test root only.
pub(crate) fn test_anchors() -> FixedTrustAnchors {
    FixedTrustAnchors::new(vec![fixture_cert("root-ca.crt")])
}
