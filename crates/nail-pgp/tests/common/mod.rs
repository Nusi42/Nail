//! Shared helpers of the integration tests.
#![allow(dead_code)]

use mail_parser::MessageParser;
use nail_api::KeyId;
use nail_api::engine::MemorySecretStore;
use nail_api::security::{KeyEncoding, SecurityStatus};
use nail_pgp::keyring::{Keyring, UnlockCache};
use nail_pgp::read::{ReadOutcome, read_message};
use nail_store::Store;
use nail_testkit::{CryptoFixtures, crypto_fixture};

/// 2026-09-29: after every fixture key's creation, dave's key expired.
pub const NOW: i64 = 1_790_000_000;

pub fn fixture(name: &str) -> Vec<u8> {
    crypto_fixture(CryptoFixtures::Pgp, name)
        .unwrap_or_else(|| panic!("missing OpenPGP fixture {name}"))
}

pub fn fixture_text(name: &str) -> String {
    String::from_utf8(fixture(name)).unwrap()
}

/// A store, keychain and unlock cache.
pub struct Env {
    pub store: Store,
    pub secrets: MemorySecretStore,
    pub cache: UnlockCache,
}

impl Env {
    pub fn new() -> Self {
        Self {
            store: Store::open_in_memory().unwrap(),
            secrets: MemorySecretStore::new(),
            cache: UnlockCache::new(),
        }
    }

    /// With these fixture keys imported.
    pub fn with(names: &[&str]) -> Self {
        let env = Self::new();
        for name in names {
            env.import(name);
        }
        env
    }

    pub fn keyring(&self) -> Keyring<'_> {
        Keyring::new(&self.store, &self.secrets, &self.cache)
    }

    /// Imports a fixture; returns the id of its (first) key.
    pub fn import(&self, name: &str) -> KeyId {
        let result = self
            .keyring()
            .import(&fixture_text(name), KeyEncoding::Text)
            .unwrap();
        assert!(result.issues.is_empty(), "{name}: {:?}", result.issues);
        result.keys[0].id.clone()
    }

    pub fn read(&self, raw: &[u8]) -> ReadOutcome {
        read_message(&self.keyring(), raw).unwrap()
    }

    pub fn status(&self, raw: &[u8]) -> SecurityStatus {
        self.read(raw).status.expect("a security status")
    }
}

/// Subject and first text body of a display message.
pub fn subject_and_text(raw: &[u8]) -> (String, String) {
    let message = MessageParser::default().parse(raw).unwrap();
    (
        message.subject().unwrap_or_default().to_owned(),
        message.body_text(0).unwrap_or_default().into_owned(),
    )
}
