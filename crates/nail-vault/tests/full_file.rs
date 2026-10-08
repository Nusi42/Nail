//! M4c WP122 (ADR 0095): full-file database encryption through
//! [`nail_vault::open_data_dir`], interrupted migrations, the vault file,
//! and the recovery key (create, verify, rotate, remove, restore after the
//! keychain entry was lost).
#![cfg(test)]

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;

use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::recovery::RecoveryKeyIssue;
use nail_api::security::{VaultPhase, VaultState, VaultStatus};
use nail_api::{Address, ErrorCode, MessageSummary, SecretRef};
use nail_store::{FileEncryption, Store, StoredMessage, file_encryption};
use nail_vault::{
    OpenedDataDir, ReadyDataDir, VAULT_FILE_NAME, Vault, VaultError, VaultFile, open_data_dir,
};

const SUBJECT: &str = "Merger talks with Contoso";
const PREVIEW: &str = "secret-preview-text";

struct Dir {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    secrets: Arc<MemorySecretStore>,
}

impl Dir {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_owned();
        Self {
            _tmp: tmp,
            root,
            secrets: Arc::new(MemorySecretStore::new()),
        }
    }

    fn db(&self) -> PathBuf {
        self.root.join(nail_store::DB_FILE_NAME)
    }

    fn open(&self) -> OpenedDataDir {
        open_data_dir(
            &self.root,
            Some(self.root.join("search")),
            self.secrets.clone(),
        )
        .unwrap()
    }

    fn ready(&self) -> ReadyDataDir {
        match self.open() {
            OpenedDataDir::Ready(ready) => ready,
            OpenedDataDir::Locked(locked) => panic!("locked: {}", locked.error()),
        }
    }

    fn vault_file(&self) -> VaultFile {
        VaultFile::load(&self.root.join(VAULT_FILE_NAME)).unwrap()
    }

    /// Every byte of the database files.
    fn db_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for suffix in ["", "-wal", "-shm"] {
            let mut name = self.db().into_os_string();
            name.push(suffix);
            if let Ok(bytes) = std::fs::read(PathBuf::from(name)) {
                out.extend(bytes);
            }
        }
        out
    }

    fn forget_key(&self) {
        let key_ref = self.vault_file().key_ref.unwrap();
        assert!(self.secrets.delete(&SecretRef(key_ref)).unwrap());
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

fn seed(store: &Store) {
    nail_store::testing::add_inbox(store, "a1", "inbox");
    let m = StoredMessage::new(MessageSummary {
        subject: SUBJECT.into(),
        preview: PREVIEW.into(),
        from: Some(Address {
            name: Some("Ada".into()),
            email: "ada@example.org".into(),
        }),
        ..nail_api::testing::summary("m1", "a1", "inbox")
    });
    store.upsert_message(&m).unwrap();
}

fn assert_readable(store: &Store) {
    let m = store.get_message(&"m1".into()).unwrap();
    assert_eq!(m.summary.subject, SUBJECT);
    assert_eq!(m.summary.preview, PREVIEW);
}

fn run(vault: &Vault) -> VaultStatus {
    vault.migrate(&mut |_| ControlFlow::Continue(())).unwrap()
}

/// A seeded, fully encrypted data directory.
fn encrypted() -> Dir {
    let dir = Dir::new();
    let ready = dir.ready();
    seed(&ready.store);
    ready.vault.set_enabled(true).unwrap();
    assert_eq!(run(&ready.vault).state, VaultState::Encrypted);
    dir
}

#[test]
fn encrypting_encrypts_the_database_file_and_decrypting_restores_it() {
    let dir = Dir::new();
    let ready = dir.ready();
    assert!(ready.vault.full_file_encryption());
    assert!(ready.open_error.is_none());
    seed(&ready.store);
    assert!(contains(&dir.db_bytes(), SUBJECT));

    ready.vault.set_enabled(true).unwrap();
    // The vault file names the key before any data is converted.
    let file = dir.vault_file();
    assert!(file.key_ref.is_some() && file.key_check.is_some());
    let status = run(&ready.vault);
    assert_eq!(status.state, VaultState::Encrypted);
    assert!(status.key_in_keychain && !status.needs_recovery);
    assert_eq!(
        file_encryption(&dir.db()).unwrap(),
        FileEncryption::Encrypted
    );
    assert!(dir.vault_file().database_encrypted);
    // Subjects are not a sealed column: only the file encryption hides them.
    assert!(!contains(&dir.db_bytes(), SUBJECT));
    assert!(!contains(&dir.db_bytes(), "ada@example.org"));
    assert_readable(&ready.store);
    // Plain opening no longer works; open_data_dir does.
    drop(ready);
    assert!(matches!(
        Store::open(dir.db()),
        Err(nail_store::Error::Locked(_))
    ));
    let ready = dir.ready();
    assert!(ready.open_error.is_none());
    assert!(ready.store.is_file_encrypted());
    assert_readable(&ready.store);

    // Off again: the file becomes plain and the key is forgotten.
    let key_ref = dir.vault_file().key_ref.unwrap();
    ready.vault.set_enabled(false).unwrap();
    assert_eq!(run(&ready.vault).state, VaultState::Plain);
    assert_eq!(file_encryption(&dir.db()).unwrap(), FileEncryption::Plain);
    let file = dir.vault_file();
    assert_eq!((file.key_ref, file.database_encrypted), (None, false));
    assert!(dir.secrets.get(&SecretRef(key_ref)).unwrap().is_none());
    drop(ready);
    assert_readable(&Store::open(dir.db()).unwrap());
}

#[test]
fn an_interrupted_encryption_resumes_after_the_file_was_rewritten() {
    let dir = Dir::new();
    let ready = dir.ready();
    seed(&ready.store);
    ready.vault.set_enabled(true).unwrap();
    // A stray copy of an earlier crashed rewrite is cleaned up on the way.
    std::fs::write(nail_store::dbfile::rewrite_temp_path(&dir.db()), b"junk").unwrap();
    // Stop right after the database phase: the file is encrypted, the
    // state still says "encrypting".
    let status = ready
        .vault
        .migrate_until(Some(VaultPhase::Blobs), &mut |_| ControlFlow::Continue(()))
        .unwrap();
    assert_eq!(status.state, VaultState::Encrypting);
    assert_eq!(
        file_encryption(&dir.db()).unwrap(),
        FileEncryption::Encrypted
    );
    drop(ready); // "crash"

    let ready = dir.ready();
    assert!(ready.vault.needs_migration().unwrap());
    assert_readable(&ready.store);
    assert_eq!(run(&ready.vault).state, VaultState::Encrypted);
    assert!(!nail_store::dbfile::rewrite_temp_path(&dir.db()).exists());
}

#[test]
fn an_interrupted_decryption_resumes_with_the_key_from_the_row() {
    let dir = encrypted();
    let ready = dir.ready();
    ready.vault.set_enabled(false).unwrap();
    // Pause after the first progress report of the blob phase: the
    // database file is plain again, the key still recorded.
    let mut seen_blobs = false;
    let status = ready
        .vault
        .migrate(&mut |s| {
            if s.phase == Some(VaultPhase::Blobs) {
                seen_blobs = true;
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        })
        .unwrap();
    // No blobs in this directory: the phase may finish without a report.
    if seen_blobs {
        assert_eq!(status.state, VaultState::Decrypting);
    }
    assert_eq!(file_encryption(&dir.db()).unwrap(), FileEncryption::Plain);
    drop(ready);
    let ready = dir.ready();
    assert!(ready.open_error.is_none());
    assert_readable(&ready.store);
    assert_eq!(run(&ready.vault).state, VaultState::Plain);
}

#[test]
fn recovery_key_restores_the_vault_after_the_keychain_entry_is_lost() {
    let dir = encrypted();
    let ready = dir.ready();
    let (text, status) = ready.vault.create_recovery_key().unwrap();
    assert!(status.configured && status.created_at.is_some() && status.verified_at.is_none());
    assert_eq!(text.len(), 69);
    // The vault file holds the wrapped key, never the recovery key.
    let raw = std::fs::read_to_string(dir.root.join(VAULT_FILE_NAME)).unwrap();
    assert!(!raw.contains(text.as_str()));
    assert!(!raw.contains(&text.replace('-', "")));
    assert!(ready.vault.status().unwrap().recovery.configured);

    // Verify: right, wrong, mistyped.
    let (valid, status) = ready
        .vault
        .verify_recovery_key(&text.to_lowercase())
        .unwrap();
    assert!(valid && status.verified_at.is_some());
    let other = nail_vault::RecoveryKey::generate().to_text();
    assert!(!ready.vault.verify_recovery_key(&other).unwrap().0);
    assert!(matches!(
        ready.vault.verify_recovery_key("ABCD"),
        Err(VaultError::RecoveryKeyInvalid(
            RecoveryKeyIssue::WrongLength
        ))
    ));
    drop(ready);

    // The keychain entry disappears (new Windows profile).
    dir.forget_key();
    let OpenedDataDir::Locked(locked) = dir.open() else {
        panic!("an encrypted database without its key must stay locked");
    };
    assert_eq!(locked.error().code(), ErrorCode::Locked);
    let status = locked.status();
    assert_eq!(status.state, VaultState::Encrypted);
    assert!(status.needs_recovery && !status.key_in_keychain);
    assert!(status.recovery.configured);
    assert_eq!(status.error, Some(ErrorCode::Locked));
    assert!(locked.verify_recovery_key(&text).unwrap());
    assert!(!locked.verify_recovery_key(&other).unwrap());
    let err = locked.restore_key(&other).unwrap_err();
    assert!(matches!(err, VaultError::RecoveryKeyWrong));
    let api: nail_api::ApiError = err.into();
    assert_eq!(api.code, ErrorCode::AuthFailed);
    assert_eq!(api.localized.unwrap().id, "recovery-key-wrong");

    let status = locked.restore_key(&text).unwrap();
    assert!(status.key_in_keychain && !status.needs_recovery);
    assert!(status.error.is_none());
    drop(locked);
    let ready = dir.ready();
    assert!(ready.open_error.is_none());
    assert_readable(&ready.store);
    let status = ready.vault.status().unwrap();
    assert!(!status.needs_recovery && status.recovery.configured);
}

#[test]
fn a_wrong_key_in_the_keychain_locks_without_touching_the_data() {
    let dir = encrypted();
    let key_ref = dir.vault_file().key_ref.unwrap();
    dir.secrets
        .set(&SecretRef(key_ref), &"11".repeat(32))
        .unwrap();
    let OpenedDataDir::Locked(locked) = dir.open() else {
        panic!("a wrong key must not open the database");
    };
    assert_eq!(locked.error().code(), ErrorCode::Crypto);
    // The key is in the keychain, just the wrong one: no recovery prompt
    // by the keychain test, but the error says why.
    assert!(locked.status().key_in_keychain);
    assert_eq!(
        file_encryption(&dir.db()).unwrap(),
        FileEncryption::Encrypted
    );
}

#[test]
fn recovery_keys_rotate_and_can_be_removed() {
    let dir = encrypted();
    let ready = dir.ready();
    let (first, _) = ready.vault.create_recovery_key().unwrap();
    let (second, _) = ready.vault.create_recovery_key().unwrap();
    assert_ne!(first.as_str(), second.as_str());
    assert!(!ready.vault.verify_recovery_key(&first).unwrap().0);
    assert!(ready.vault.verify_recovery_key(&second).unwrap().0);
    let status = ready.vault.remove_recovery_key().unwrap();
    assert!(!status.recovery.configured);
    assert!(matches!(
        ready.vault.verify_recovery_key(&second),
        Err(VaultError::NoRecoveryKey)
    ));
    // Turning encryption off forgets a recovery key too.
    ready.vault.create_recovery_key().unwrap();
    ready.vault.set_enabled(false).unwrap();
    run(&ready.vault);
    assert!(dir.vault_file().recovery.is_none());
}

#[test]
fn a_failed_delivery_keeps_the_old_recovery_key() {
    let dir = encrypted();
    let ready = dir.ready();
    let (first, before) = ready.vault.create_recovery_key().unwrap();
    let mut offered = String::new();
    let err = ready
        .vault
        .create_recovery_key_with(|key, created| {
            assert!(!created.is_empty(), "{created:?}");
            offered = key.to_owned();
            Err(VaultError::VaultFile("the kit cannot be written".into()))
        })
        .unwrap_err();
    assert!(matches!(err, VaultError::VaultFile(_)));
    assert_ne!(offered, first.as_str());
    // The old key still opens the data key, the new one was never stored.
    assert!(ready.vault.verify_recovery_key(&first).unwrap().0);
    assert!(!ready.vault.verify_recovery_key(&offered).unwrap().0);
    assert_eq!(
        dir.vault_file().recovery.unwrap().created_at,
        before.created_at.unwrap()
    );
    // A delivery that works hands over exactly the stored key.
    let mut delivered = String::new();
    let (second, _) = ready
        .vault
        .create_recovery_key_with(|key, _| {
            delivered = key.to_owned();
            Ok::<(), VaultError>(())
        })
        .unwrap();
    assert_eq!(delivered, second.as_str());
    assert!(ready.vault.verify_recovery_key(&second).unwrap().0);
}

#[test]
fn a_plain_vault_has_no_recovery_key() {
    let dir = Dir::new();
    let ready = dir.ready();
    let err = ready.vault.create_recovery_key().unwrap_err();
    assert!(matches!(err, VaultError::NotEncrypted));
    let api: nail_api::ApiError = err.into();
    assert_eq!(api.code, ErrorCode::Conflict);
    assert_eq!(api.localized.unwrap().id, "vault-not-encrypted");
    let status = ready.vault.status().unwrap();
    assert!(!status.needs_recovery && !status.recovery.configured);
    assert!(matches!(
        ready
            .vault
            .restore_key(&nail_vault::RecoveryKey::generate().to_text()),
        Err(VaultError::NotEncrypted)
    ));
}

#[test]
fn column_only_vaults_recover_through_the_vault_itself() {
    // Vault::new keeps the M2a behaviour (no file encryption): the store
    // opens without a key, and a lost key only locks sealed data.
    let dir = Dir::new();
    let store = Store::open(dir.db()).unwrap();
    let blobs = nail_store::BlobStore::open(dir.root.join("blobs")).unwrap();
    seed(&store);
    let vault = Vault::new(store.clone(), blobs.clone(), None, dir.secrets.clone());
    assert!(!vault.full_file_encryption());
    vault.set_enabled(true).unwrap();
    assert_eq!(run(&vault).state, VaultState::Encrypted);
    assert_eq!(file_encryption(&dir.db()).unwrap(), FileEncryption::Plain);
    let (text, _) = vault.create_recovery_key().unwrap();
    drop((vault, store, blobs));

    dir.forget_key();
    let store = Store::open(dir.db()).unwrap();
    let blobs = nail_store::BlobStore::open(dir.root.join("blobs")).unwrap();
    let vault = Vault::new(store.clone(), blobs, None, dir.secrets.clone());
    assert!(matches!(vault.open(), Err(VaultError::Locked(_))));
    let status = vault.status().unwrap();
    assert!(status.needs_recovery && status.recovery.configured);
    assert!(store.get_message(&"m1".into()).is_err());
    let status = vault.restore_key(&text).unwrap();
    assert!(status.key_in_keychain && !status.needs_recovery);
    assert_readable(&store);
}

#[test]
fn an_older_vault_gets_its_vault_file_on_open() {
    // A directory encrypted before M4c has the key only in the row.
    let dir = Dir::new();
    {
        let store = Store::open(dir.db()).unwrap();
        let blobs = nail_store::BlobStore::open(dir.root.join("blobs")).unwrap();
        let vault = Vault::new(store, blobs, None, dir.secrets.clone());
        vault.set_enabled(true).unwrap();
        run(&vault);
    }
    std::fs::remove_file(dir.root.join(VAULT_FILE_NAME)).unwrap();
    let ready = dir.ready();
    assert!(ready.open_error.is_none());
    assert!(dir.vault_file().key_ref.is_some());
    // The database file is still plain; upgrading encrypts it once.
    assert_eq!(file_encryption(&dir.db()).unwrap(), FileEncryption::Plain);
    assert!(ready.vault.encrypt_database_file().unwrap());
    assert!(!ready.vault.encrypt_database_file().unwrap());
    assert_eq!(
        file_encryption(&dir.db()).unwrap(),
        FileEncryption::Encrypted
    );
    assert!(dir.vault_file().database_encrypted);
    drop(ready);
    let ready = dir.ready();
    assert!(ready.open_error.is_none());
    assert!(ready.store.is_file_encrypted());
    // Column-only vaults (Vault::new) never encrypt the file.
    let plain_dir = Dir::new();
    let store = Store::open(plain_dir.db()).unwrap();
    let blobs = nail_store::BlobStore::open(plain_dir.root.join("blobs")).unwrap();
    let vault = Vault::new(store, blobs, None, plain_dir.secrets.clone());
    vault.set_enabled(true).unwrap();
    run(&vault);
    assert!(!vault.encrypt_database_file().unwrap());
}

#[test]
fn a_crash_at_any_progress_point_resumes_in_both_directions() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    for enabled in [true, false] {
        for crash_at in 1..12 {
            let dir = if enabled { Dir::new() } else { encrypted() };
            {
                let ready = dir.ready();
                if enabled {
                    seed(&ready.store);
                    // Several sealed values make several progress reports.
                    for i in 0..5 {
                        let draft: nail_api::Draft = serde_json::from_value(serde_json::json!({
                            "identity_id": "ada@example.org",
                            "subject": format!("Draft {i}"),
                            "text": format!("secret-draft-{i}"),
                        }))
                        .unwrap();
                        ready.store.save_draft(&draft).unwrap();
                    }
                }
                ready.vault.set_enabled(enabled).unwrap();
                let mut calls = 0;
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    ready.vault.migrate(&mut |_| {
                        calls += 1;
                        assert!(calls != crash_at, "simulated crash");
                        ControlFlow::Continue(())
                    })
                }));
                if outcome.is_ok() {
                    // Fewer reports than `crash_at`: finished normally.
                    continue;
                }
            }
            let ready = dir.ready();
            assert!(ready.open_error.is_none(), "crash at {crash_at}");
            let status = run(&ready.vault);
            let want = if enabled {
                VaultState::Encrypted
            } else {
                VaultState::Plain
            };
            assert_eq!(status.state, want, "crash at {crash_at}");
            let form = if enabled {
                FileEncryption::Encrypted
            } else {
                FileEncryption::Plain
            };
            assert_eq!(file_encryption(&dir.db()).unwrap(), form);
            assert_readable(&ready.store);
            assert_eq!(contains(&dir.db_bytes(), SUBJECT), !enabled);
        }
    }
}
