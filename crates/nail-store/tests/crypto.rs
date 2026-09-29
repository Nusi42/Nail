//! Schema v5 (ADR 0029): keyring, trust decisions, Autocrypt peers,
//! encryption-at-rest state, cached message security and settings.

mod common;

use common::{message, seeded_store};
use nail_api::security::{
    AuthMethodResult, AuthResults, AuthVerdict, AutocryptPreference, CryptoTechnology,
    DecryptionState, EncryptionInfo, HeaderProtection, KeySource, SecretStorage, SecuritySettings,
    SecurityStatus, TrustDecision, VaultPhase, VaultState,
};
use nail_store::{
    AutocryptPeerRecord, Error, KeyData, MessageSecurityRecord, SECURITY_SETTINGS_KEY,
    SecretKeyData, Store, VaultRecord,
};
use pretty_assertions::assert_eq;

fn pgp_key(fingerprint: &str, email: &str) -> KeyData {
    KeyData {
        technology: CryptoTechnology::OpenPgp,
        fingerprint: fingerprint.into(),
        user_ids: vec![format!("Someone <{email}>")],
        emails: vec![email.into()],
        algorithm: "Ed25519".into(),
        created_at: 1_700_000_000,
        expires_at: None,
        revoked: false,
        can_sign: true,
        can_encrypt: true,
        source: KeySource::Imported,
        public_data: vec![0x99, 1, 2, 3],
        secret: None,
        issuer: None,
        serial: None,
    }
}

const FPR_A: &str = "0123456789ABCDEF0123456789ABCDEF01234567";
const FPR_B: &str = "89ABCDEF0123456789ABCDEF0123456789ABCDEF";

#[test]
fn keys_are_upserted_by_fingerprint() {
    let store = Store::open_in_memory().unwrap();
    // Spaces and lower case are normalized.
    let (bob, created) = store
        .upsert_key(&pgp_key(
            "0123 4567 89ab cdef 0123 4567 89ab cdef 0123 4567",
            "Bob@Example.org",
        ))
        .unwrap();
    assert!(created);
    assert_eq!(bob.id.as_str(), "key-1");
    assert_eq!(bob.data.fingerprint, FPR_A);
    assert_eq!(bob.data.emails, ["bob@example.org"]);
    assert_eq!(bob.decision, None);
    assert!(!bob.is_own());

    let mut updated = pgp_key(FPR_A, "bob@example.org");
    updated.user_ids.push("Bob <bob@work.example>".into());
    updated.emails.push("bob@work.example".into());
    updated.revoked = true;
    let (again, created) = store.upsert_key(&updated).unwrap();
    assert!(!created);
    assert_eq!(again.id, bob.id);
    assert!(again.data.revoked);
    assert_eq!(again.data.emails, ["bob@example.org", "bob@work.example"]);
    assert_eq!(store.get_key(&bob.id).unwrap(), again);
    assert_eq!(
        store
            .find_key(CryptoTechnology::OpenPgp, &FPR_A.to_lowercase())
            .unwrap()
            .map(|k| k.id),
        Some(bob.id.clone())
    );
    assert_eq!(
        store.find_key(CryptoTechnology::Smime, FPR_A).unwrap(),
        None
    );

    assert!(
        store
            .upsert_key(&pgp_key("not hex", "x@example.org"))
            .is_err()
    );
    let mut empty = pgp_key(FPR_B, "x@example.org");
    empty.public_data.clear();
    assert!(matches!(
        store.upsert_key(&empty),
        Err(Error::InvalidData(_))
    ));
    assert!(store.get_key(&"key-99".into()).unwrap_err().is_not_found());
    assert!(store.get_key(&"bogus".into()).unwrap_err().is_not_found());
}

#[test]
fn own_keys_keep_their_secret_and_list_first() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_key(&pgp_key(FPR_A, "bob@example.org"))
        .unwrap();
    let mut own = pgp_key(FPR_B, "ada@example.org");
    own.source = KeySource::Generated;
    own.secret = Some(SecretKeyData {
        storage: SecretStorage::Keychain,
        data: Some(vec![9, 9, 9]),
        reference: Some("keychain:nail/pgp/ada".into()),
    });
    let (own, _) = store.upsert_key(&own).unwrap();
    assert!(own.is_own());

    // Importing the public part again keeps the secret.
    let (public_again, created) = store
        .upsert_key(&pgp_key(FPR_B, "ada@example.org"))
        .unwrap();
    assert!(!created);
    assert_eq!(public_again.data.secret, own.data.secret);

    let all = store.list_keys(None, false).unwrap();
    assert_eq!(
        all.iter()
            .map(|k| k.data.fingerprint.as_str())
            .collect::<Vec<_>>(),
        [FPR_B, FPR_A]
    );
    assert_eq!(store.list_keys(None, true).unwrap().len(), 1);
    assert!(
        store
            .list_keys(Some(CryptoTechnology::Smime), false)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .keys_for_email("ADA@example.org", Some(CryptoTechnology::OpenPgp))
            .unwrap()
            .len(),
        1
    );
    assert!(
        store
            .keys_for_email("nobody@example.org", None)
            .unwrap()
            .is_empty()
    );

    // Inconsistent secret storage is refused.
    let mut bad = pgp_key("AAAA", "c@example.org");
    bad.secret = Some(SecretKeyData {
        storage: SecretStorage::WindowsStore,
        data: Some(vec![1]),
        reference: None,
    });
    assert!(matches!(store.upsert_key(&bad), Err(Error::InvalidData(_))));

    // Deleting an own key needs the explicit flag.
    assert!(matches!(
        store.delete_key(&own.id, false),
        Err(Error::Conflict(_))
    ));
    assert!(store.delete_key(&own.id, true).unwrap());
    assert!(!store.delete_key(&own.id, true).unwrap());
    assert_eq!(store.list_keys(None, false).unwrap().len(), 1);
}

#[test]
fn trust_decisions_survive_reimport() {
    let store = Store::open_in_memory().unwrap();
    let (key, _) = store
        .upsert_key(&pgp_key(FPR_A, "bob@example.org"))
        .unwrap();
    store
        .set_trust_decision(CryptoTechnology::OpenPgp, FPR_A, TrustDecision::Verified)
        .unwrap();
    assert_eq!(
        store.get_key(&key.id).unwrap().decision,
        Some(TrustDecision::Verified)
    );
    store.delete_key(&key.id, false).unwrap();
    let (again, _) = store
        .upsert_key(&pgp_key(FPR_A, "bob@example.org"))
        .unwrap();
    assert_eq!(again.decision, Some(TrustDecision::Verified));
    store
        .set_trust_decision(CryptoTechnology::OpenPgp, FPR_A, TrustDecision::Rejected)
        .unwrap();
    assert_eq!(
        store
            .trust_decision(CryptoTechnology::OpenPgp, FPR_A)
            .unwrap(),
        Some(TrustDecision::Rejected)
    );
    store
        .set_trust_decision(CryptoTechnology::OpenPgp, FPR_A, TrustDecision::Unset)
        .unwrap();
    assert_eq!(
        store
            .trust_decision(CryptoTechnology::OpenPgp, FPR_A)
            .unwrap(),
        None
    );
}

#[test]
fn autocrypt_peers_per_account() {
    let store = Store::open_in_memory().unwrap();
    let peer = AutocryptPeerRecord {
        account_id: "work".into(),
        email: "Bob@Example.org".into(),
        last_seen: 100,
        autocrypt_timestamp: Some(100),
        key_fingerprint: Some(FPR_A.into()),
        prefer_encrypt: AutocryptPreference::Mutual,
        gossip_timestamp: None,
        gossip_fingerprint: None,
    };
    store.put_autocrypt_peer(&peer).unwrap();
    let got = store
        .autocrypt_peer(&"work".into(), "bob@example.org")
        .unwrap()
        .unwrap();
    assert_eq!(got.email, "bob@example.org");
    assert_eq!(got.prefer_encrypt, AutocryptPreference::Mutual);
    assert_eq!(
        store
            .autocrypt_peer(&"home".into(), "bob@example.org")
            .unwrap(),
        None
    );
    store
        .put_autocrypt_peer(&AutocryptPeerRecord {
            last_seen: 200,
            prefer_encrypt: AutocryptPreference::NoPreference,
            ..peer.clone()
        })
        .unwrap();
    let got = store
        .autocrypt_peer(&"work".into(), "BOB@example.org")
        .unwrap()
        .unwrap();
    assert_eq!(
        (got.last_seen, got.prefer_encrypt),
        (200, AutocryptPreference::NoPreference)
    );
    assert!(
        store
            .put_autocrypt_peer(&AutocryptPeerRecord {
                email: " ".into(),
                ..peer.clone()
            })
            .is_err()
    );
    // Across accounts, by address.
    store
        .put_autocrypt_peer(&AutocryptPeerRecord {
            account_id: "home".into(),
            ..peer.clone()
        })
        .unwrap();
    let all = store.autocrypt_peers_for_email(" BOB@example.org").unwrap();
    assert_eq!(
        all.iter()
            .map(|p| p.account_id.as_str())
            .collect::<Vec<_>>(),
        ["home", "work"]
    );
    assert!(store.autocrypt_peers_for_email("x@y").unwrap().is_empty());
    assert_eq!(store.delete_autocrypt_peers(&"work".into()).unwrap(), 1);
}

#[test]
fn vault_state_round_trips() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.vault_state().unwrap(), VaultRecord::default());
    let running = VaultRecord {
        state: VaultState::Encrypting,
        phase: Some(VaultPhase::Blobs),
        done: 10,
        total: 42,
        key_ref: Some("keychain:nail/vault".into()),
        key_check: Some(vec![1, 2, 3]),
        started_at: Some(5),
        finished_at: None,
        error: Some("disk full".into()),
    };
    store.set_vault_state(&running).unwrap();
    assert_eq!(store.vault_state().unwrap(), running);
}

#[test]
fn message_security_is_cached_and_follows_the_message() {
    let store = seeded_store();
    store
        .upsert_message(&message("m1", "a1", "inbox", "2026-09-29T08:15:00Z"))
        .unwrap();
    assert_eq!(store.message_security(&"m1".into()).unwrap(), None);
    let record = MessageSecurityRecord {
        message_id: "m1".into(),
        status: Some(SecurityStatus {
            technology: CryptoTechnology::OpenPgp,
            signatures: vec![],
            encryption: Some(EncryptionInfo {
                state: DecryptionState::NoKey,
                key_id: None,
                recipients: vec!["0x1234".into()],
                algorithm: None,
            }),
            header_protection: HeaderProtection::None,
            partial: false,
            inline_pgp: false,
        }),
        auth_results: Some(AuthResults {
            authserv_id: "mx.example.net".into(),
            dkim: vec![AuthMethodResult {
                verdict: AuthVerdict::Pass,
                domain: Some("example.org".into()),
                selector: Some("s1".into()),
                aligned: Some(true),
                reason: None,
            }],
            spf: None,
            dmarc: None,
            arc: None,
        }),
        checked_at: 77,
    };
    store.put_message_security(&record).unwrap();
    assert_eq!(store.message_security(&"m1".into()).unwrap(), Some(record));
    assert!(
        store
            .put_message_security(&MessageSecurityRecord {
                message_id: "missing".into(),
                status: None,
                auth_results: None,
                checked_at: 1,
            })
            .is_err()
    );
    assert_eq!(store.clear_message_security().unwrap(), 1);
}

#[test]
fn settings_are_json() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(
        store
            .setting::<SecuritySettings>(SECURITY_SETTINGS_KEY)
            .unwrap(),
        None
    );
    let settings = SecuritySettings {
        index_decrypted: vec!["work".into()],
        ..SecuritySettings::default()
    };
    store.set_setting(SECURITY_SETTINGS_KEY, &settings).unwrap();
    assert_eq!(
        store.setting(SECURITY_SETTINGS_KEY).unwrap(),
        Some(settings)
    );
    store.set_setting("other", &5u32).unwrap();
    assert_eq!(store.setting::<u32>("other").unwrap(), Some(5));
}
