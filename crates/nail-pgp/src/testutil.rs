//! Test helpers: fixtures from `corpus/crypto/pgp/`.

use nail_testkit::{CryptoFixtures, crypto_fixture};

/// A fixture's bytes (panics when missing).
pub(crate) fn fixture(name: &str) -> Vec<u8> {
    crypto_fixture(CryptoFixtures::Pgp, name)
        .unwrap_or_else(|| panic!("missing OpenPGP fixture {name}"))
}

/// A fixture as text.
pub(crate) fn fixture_text(name: &str) -> String {
    String::from_utf8(fixture(name)).expect("fixture is UTF-8")
}
