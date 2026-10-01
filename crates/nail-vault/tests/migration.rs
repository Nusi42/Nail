//! Encryption at rest end to end: a real data directory (SQLite file, blob
//! store, tantivy index) is encrypted and decrypted, interrupted at many
//! points (panics standing in for crashes, pauses, errors), and must come
//! out intact, readable and without plaintext on disk.

use std::ops::ControlFlow;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nail_api::engine::{EngineError, MemorySecretStore, SecretStore};
use nail_api::security::{VaultPhase, VaultState, VaultStatus};
use nail_api::{
    Account, AccountKind, Address, Draft, ErrorCode, Flags, Folder, FolderRole, MessageSummary,
    SecretRef,
};
use nail_search::{CommitPolicy, IndexDocument, SearchIndex, query::TextField};
use nail_store::{BlobHash, BlobStore, SEALED_MAGIC, Store, StoredMessage};
use nail_vault::{INDEX_VERSION_FILE, Vault, VaultError};

const MESSAGES: usize = 12;
const DRAFTS: usize = 3;

struct DataDir {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    secrets: Arc<MemorySecretStore>,
    hashes: Vec<BlobHash>,
}

impl DataDir {
    fn db(&self) -> PathBuf {
        self.root.join("nail.sqlite3")
    }
    fn blob_dir(&self) -> PathBuf {
        self.root.join("blobs")
    }
    fn index_dir(&self) -> PathBuf {
        self.root.join("search")
    }

    /// Opens the directory the way the core does at start.
    fn open(&self) -> (Store, BlobStore, Vault) {
        let store = Store::open(self.db()).unwrap();
        let blobs = BlobStore::open(self.blob_dir()).unwrap();
        let vault = Vault::new(
            store.clone(),
            blobs.clone(),
            Some(self.index_dir()),
            self.secrets.clone(),
        );
        (store, blobs, vault)
    }
}

fn raw_message(i: usize) -> Vec<u8> {
    format!(
        "From: Ada <ada@example.org>\r\nTo: bob@example.org\r\nSubject: Plan {i}\r\n\r\n\
         secret-body-{i} {}\r\n",
        "padding ".repeat(50 + i * 40)
    )
    .into_bytes()
}

fn seed() -> DataDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_owned();
    let data = DataDir {
        _tmp: tmp,
        root,
        secrets: Arc::new(MemorySecretStore::new()),
        hashes: Vec::new(),
    };
    let (store, blobs, _vault) = data.open();
    store
        .upsert_account(&Account {
            id: "a1".into(),
            name: "Ada".into(),
            email: "ada@example.org".into(),
            kind: AccountKind::Imap,
        })
        .unwrap();
    store
        .upsert_folder(&Folder {
            id: "inbox".into(),
            account_id: "a1".into(),
            parent_id: None,
            name: "INBOX".into(),
            path: "INBOX".into(),
            role: FolderRole::Inbox,
            unread: 0,
            total: 0,
        })
        .unwrap();
    let mut hashes = Vec::new();
    let index = SearchIndex::open(data.index_dir(), CommitPolicy::default()).unwrap();
    for i in 0..MESSAGES {
        let raw = raw_message(i);
        let hash = blobs.put(&raw).unwrap();
        let mut m = StoredMessage::new(MessageSummary {
            id: format!("m{i}").as_str().into(),
            folder_id: "inbox".into(),
            account_id: "a1".into(),
            subject: format!("Plan {i}"),
            from: Some(Address {
                name: Some("Ada".into()),
                email: "ada@example.org".into(),
            }),
            to: Vec::new(),
            date: "2026-09-01T10:00:00Z".into(),
            flags: Flags::default(),
            has_attachments: false,
            preview: format!("secret-preview-{i}"),
            thread_id: None,
            tags: Vec::new(),
            duplicates: Vec::new(),
            headers_only: false,
            gmail: None,
        });
        m.blob_hash = Some(hash.clone());
        store.upsert_message(&m).unwrap();
        index
            .add(&[IndexDocument::from_raw(
                format!("m{i}").as_str().into(),
                "a1".into(),
                &raw,
            )])
            .unwrap();
        hashes.push(hash);
        // Several commits make several segments.
        if i % 4 == 3 {
            index.commit().unwrap();
        }
    }
    index.commit().unwrap();
    drop(index);
    for i in 0..DRAFTS {
        let draft: Draft = serde_json::from_value(serde_json::json!({
            "identity_id": "ada@example.org",
            "subject": format!("Draft {i}"),
            "text": format!("secret-draft-{i}"),
        }))
        .unwrap();
        store.save_draft(&draft).unwrap();
    }
    DataDir { hashes, ..data }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Everything is readable through the normal APIs.
fn assert_intact(data: &DataDir, store: &Store, blobs: &BlobStore, vault: &Vault) {
    for i in 0..MESSAGES {
        let m = store.get_message(&format!("m{i}").as_str().into()).unwrap();
        assert_eq!(m.summary.preview, format!("secret-preview-{i}"));
        let hash = m.blob_hash.unwrap();
        assert_eq!(blobs.get(&hash).unwrap(), raw_message(i));
    }
    let mut drafts: Vec<String> = store
        .list_drafts()
        .unwrap()
        .into_iter()
        .map(|d| d.text)
        .collect();
    drafts.sort();
    assert_eq!(
        drafts,
        (0..DRAFTS)
            .map(|i| format!("secret-draft-{i}"))
            .collect::<Vec<_>>()
    );
    let index = SearchIndex::open_with(
        data.index_dir(),
        CommitPolicy::default(),
        vault.index_wrapper().unwrap(),
    )
    .unwrap();
    assert!(!index.was_rebuilt(), "the index must survive the migration");
    assert_eq!(index.doc_count().unwrap(), MESSAGES as u64);
    let hits = index.search_text(TextField::Body, "secret-body-7").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, "m7".into());
}

/// Nothing sensitive is readable on disk.
fn assert_encrypted_on_disk(data: &DataDir) {
    for file in files_under(&data.root) {
        let bytes = std::fs::read(&file).unwrap();
        for needle in [&b"secret-preview"[..], b"secret-draft", b"secret-body"] {
            assert!(!contains(&bytes, needle), "plaintext in {file:?}");
        }
    }
    for hash in &data.hashes {
        let (dir, name) = hash.as_str().split_at(2);
        let bytes = std::fs::read(data.blob_dir().join(dir).join(name)).unwrap();
        assert!(bytes.starts_with(&SEALED_MAGIC), "blob {hash} not sealed");
    }
    for file in files_under(&data.index_dir()) {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with(".lock") || name == INDEX_VERSION_FILE {
            continue;
        }
        let bytes = std::fs::read(&file).unwrap();
        assert!(
            bytes.starts_with(&SEALED_MAGIC),
            "index file {name} not sealed"
        );
    }
}

fn assert_plain_on_disk(data: &DataDir) {
    for hash in &data.hashes {
        let (dir, name) = hash.as_str().split_at(2);
        let bytes = std::fs::read(data.blob_dir().join(dir).join(name)).unwrap();
        assert!(
            !bytes.starts_with(&SEALED_MAGIC),
            "blob {hash} still sealed"
        );
    }
    for file in files_under(&data.index_dir()) {
        let bytes = std::fs::read(&file).unwrap();
        assert!(!bytes.starts_with(&SEALED_MAGIC), "{file:?} still sealed");
    }
    let conn = rusqlite_free_check(&data.db());
    assert!(conn.iter().all(|v| v.starts_with("secret-")), "{conn:?}");
}

/// Reads the raw preview column with a separate SQLite connection (via the
/// store, with no cipher installed, sealed values would be `locked`).
fn rusqlite_free_check(db: &Path) -> Vec<String> {
    let store = Store::open(db).unwrap();
    let counts = store.sealed_field_counts().unwrap();
    assert_eq!(counts.sealed, 0, "sealed values left: {counts:?}");
    (0..MESSAGES)
        .map(|i| {
            store
                .get_message(&format!("m{i}").as_str().into())
                .unwrap()
                .summary
                .preview
        })
        .collect()
}

fn run_to_end(vault: &Vault) -> (VaultStatus, Vec<VaultStatus>) {
    let mut seen = Vec::new();
    let status = vault
        .migrate(&mut |s| {
            seen.push(s.clone());
            ControlFlow::Continue(())
        })
        .unwrap();
    (status, seen)
}

#[test]
fn encrypt_and_decrypt_keep_everything_and_leave_no_plaintext() {
    let data = seed();
    let (store, blobs, vault) = data.open();
    assert_eq!(vault.open().unwrap().state, VaultState::Plain);
    assert!(vault.index_wrapper().unwrap().is_none());
    // Sanity: the plaintext is on disk before.
    let before = std::fs::read(data.db()).unwrap();
    let wal = std::fs::read(data.root.join("nail.sqlite3-wal")).unwrap_or_default();
    assert!(contains(&before, b"secret-preview") || contains(&wal, b"secret-preview"));

    let status = vault.set_enabled(true).unwrap();
    assert_eq!(status.state, VaultState::Encrypting);
    assert_eq!(status.phase, Some(VaultPhase::Database));
    assert!(status.key_in_keychain);
    assert_eq!(data.secrets.len(), 1);

    let (status, seen) = run_to_end(&vault);
    assert_eq!(status.state, VaultState::Encrypted);
    assert_eq!(status.phase, None);
    assert!(status.key_in_keychain);
    // Progress went through the phases in order and counted up to the total.
    let phases: Vec<VaultPhase> = seen.iter().filter_map(|s| s.phase).collect();
    let first_blob = phases.iter().position(|p| *p == VaultPhase::Blobs).unwrap();
    let first_index = phases.iter().position(|p| *p == VaultPhase::Index).unwrap();
    assert!(
        phases[..first_blob]
            .iter()
            .all(|p| *p == VaultPhase::Database)
    );
    assert!(first_blob < first_index);
    let last_db = seen
        .iter()
        .rfind(|s| s.phase == Some(VaultPhase::Database))
        .unwrap();
    assert_eq!(
        (last_db.done, last_db.total),
        ((MESSAGES + DRAFTS) as u64, (MESSAGES + DRAFTS) as u64)
    );
    let last_blob = seen
        .iter()
        .rfind(|s| s.phase == Some(VaultPhase::Blobs))
        .unwrap();
    assert_eq!(
        (last_blob.done, last_blob.total),
        (MESSAGES as u64, MESSAGES as u64)
    );

    assert_intact(&data, &store, &blobs, &vault);
    drop((store, blobs, vault));
    assert_encrypted_on_disk(&data);

    // A restart with the key in the keychain reads everything.
    let (store, blobs, vault) = data.open();
    assert_eq!(vault.open().unwrap().state, VaultState::Encrypted);
    assert!(!vault.needs_migration().unwrap());
    assert_intact(&data, &store, &blobs, &vault);

    // New writes while encrypted are sealed.
    let hash = blobs.put(b"secret-body-new").unwrap();
    assert!(blobs.is_sealed(&hash).unwrap());
    blobs.remove(&hash).unwrap();

    // And back.
    assert_eq!(
        vault.set_enabled(false).unwrap().state,
        VaultState::Decrypting
    );
    let (status, _) = run_to_end(&vault);
    assert_eq!(status.state, VaultState::Plain);
    assert!(!status.key_in_keychain);
    assert!(data.secrets.is_empty(), "the data key is deleted");
    assert!(vault.index_wrapper().unwrap().is_none());
    assert_intact(&data, &store, &blobs, &vault);
    drop((store, blobs, vault));
    assert_plain_on_disk(&data);
    let (store, blobs, vault) = data.open();
    assert_eq!(vault.open().unwrap().state, VaultState::Plain);
    assert_intact(&data, &store, &blobs, &vault);
}

/// Runs a migration and "crashes" (panics) at the `k`-th progress report.
/// Returns whether it crashed (false = it finished first).
fn crash_at(data: &DataDir, k: usize) -> bool {
    let (_store, _blobs, vault) = data.open();
    vault.open().unwrap();
    let mut n = 0;
    let result = catch_unwind(AssertUnwindSafe(|| {
        vault.migrate(&mut |_| {
            n += 1;
            assert!(n != k, "simulated crash");
            ControlFlow::Continue(())
        })
    }));
    result.is_err()
}

#[test]
fn a_crash_at_any_point_resumes_to_intact_data_in_both_directions() {
    // Count the progress reports of an uninterrupted run first.
    let probe = seed();
    let (_s, _b, vault) = probe.open();
    vault.set_enabled(true).unwrap();
    let (_, seen) = run_to_end(&vault);
    let steps = seen.len();
    assert!(steps > MESSAGES, "{steps}");
    drop(vault);

    let mut crashes = 0;
    let mut crashed_phases = std::collections::BTreeSet::new();
    for enable in [true, false] {
        for k in (1..=steps + 1).step_by(3) {
            let data = seed();
            if !enable {
                // Start from an encrypted directory.
                let (_s, _b, vault) = data.open();
                vault.set_enabled(true).unwrap();
                run_to_end(&vault);
            }
            {
                let (_s, _b, vault) = data.open();
                vault.open().unwrap();
                vault.set_enabled(enable).unwrap();
            }
            let crashed = crash_at(&data, k);
            if crashed {
                crashes += 1;
                let phase = data.open().2.status().unwrap().phase;
                crashed_phases.insert(phase.map(VaultPhase::as_str));
                // A second crash on the resumed run, a bit later.
                crash_at(&data, 2);
            }
            // The next start resumes and completes.
            let (store, blobs, vault) = data.open();
            let status = vault.open().unwrap();
            if crashed {
                assert!(
                    matches!(
                        status.state,
                        VaultState::Encrypting | VaultState::Decrypting
                    ),
                    "a crash leaves the migration pending: {status:?}"
                );
                assert!(vault.needs_migration().unwrap());
            }
            let (status, _) = run_to_end(&vault);
            let expected = if enable {
                VaultState::Encrypted
            } else {
                VaultState::Plain
            };
            assert_eq!(status.state, expected, "enable={enable} k={k}");
            assert_intact(&data, &store, &blobs, &vault);
            drop((store, blobs, vault));
            if enable {
                assert_encrypted_on_disk(&data);
            } else {
                assert_plain_on_disk(&data);
                assert!(data.secrets.is_empty());
            }
        }
    }
    // The loop really interrupted migrations, in every phase.
    assert!(
        crashes >= 2 * (steps / 3),
        "{crashes} crashes of {steps} steps"
    );
    assert_eq!(
        crashed_phases.into_iter().flatten().collect::<Vec<_>>(),
        ["blobs", "database", "index"]
    );
}

#[test]
fn pausing_resumes_and_switching_back_mid_way_reverses() {
    let data = seed();
    let (store, blobs, vault) = data.open();
    vault.set_enabled(true).unwrap();
    let mut n = 0;
    let status = vault
        .migrate(&mut |_| {
            n += 1;
            if n == 8 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(status.state, VaultState::Encrypting);
    assert!(status.phase.is_some());
    assert!(vault.needs_migration().unwrap());
    // The app keeps working in the middle: mixed data is readable, new
    // writes are sealed.
    assert_intact(&data, &store, &blobs, &vault);
    let hash = blobs.put(b"written mid-way").unwrap();
    assert!(blobs.is_sealed(&hash).unwrap());

    // The user switches it off again before it finished.
    let status = vault.set_enabled(false).unwrap();
    assert_eq!(status.state, VaultState::Decrypting);
    let (status, _) = run_to_end(&vault);
    assert_eq!(status.state, VaultState::Plain);
    assert!(!blobs.is_sealed(&hash).unwrap());
    assert_eq!(blobs.get(&hash).unwrap(), b"written mid-way");
    assert_intact(&data, &store, &blobs, &vault);
    assert!(data.secrets.is_empty());
    // Switching to the current state is a no-op.
    assert_eq!(vault.set_enabled(false).unwrap().state, VaultState::Plain);
}

#[test]
fn the_index_phase_can_wait_until_the_index_is_closed() {
    let data = seed();
    let (store, blobs, vault) = data.open();
    vault.set_enabled(true).unwrap();
    // Database and blobs with the index open (plain files readable, new
    // segments sealed).
    let index = SearchIndex::open_with(
        data.index_dir(),
        CommitPolicy::default(),
        vault.index_wrapper().unwrap(),
    )
    .unwrap();
    let status = vault
        .migrate_until(Some(VaultPhase::Index), &mut |_| ControlFlow::Continue(()))
        .unwrap();
    assert_eq!(status.state, VaultState::Encrypting);
    assert_eq!(status.phase, Some(VaultPhase::Index));
    index
        .add(&[IndexDocument::from_raw(
            "m99".into(),
            "a1".into(),
            b"Subject: x\r\n\r\nsecret-body-99",
        )])
        .unwrap();
    index.commit().unwrap();
    assert_eq!(index.doc_count().unwrap(), MESSAGES as u64 + 1);
    drop(index);
    // Then the index phase with the index closed.
    let (status, _) = run_to_end(&vault);
    assert_eq!(status.state, VaultState::Encrypted);
    let index = SearchIndex::open_with(
        data.index_dir(),
        CommitPolicy::default(),
        vault.index_wrapper().unwrap(),
    )
    .unwrap();
    assert_eq!(
        index
            .search_text(TextField::Body, "secret-body-99")
            .unwrap()
            .len(),
        1
    );
    drop(index);
    let _ = (store, blobs);
    assert_encrypted_on_disk(&data);
}

#[test]
fn torn_temporary_files_from_a_power_loss_are_ignored() {
    let data = seed();
    // What a crash in the middle of a rename-based write leaves behind.
    let blob_dir = data.blob_dir().join(&data.hashes[0].as_str()[..2]);
    std::fs::write(blob_dir.join(".tmp1234"), b"half a blob").unwrap();
    std::fs::write(data.index_dir().join(".tmpABCD"), b"half an index file").unwrap();
    let (store, blobs, vault) = data.open();
    vault.set_enabled(true).unwrap();
    let (status, _) = run_to_end(&vault);
    assert_eq!(status.state, VaultState::Encrypted);
    assert_intact(&data, &store, &blobs, &vault);
    // Young temporary blob files are left alone (a write may be running).
    assert!(blob_dir.join(".tmp1234").exists());
    assert_eq!(
        blobs.remove_temp_files(std::time::Duration::ZERO).unwrap(),
        1
    );
}

#[test]
fn a_missing_key_is_locked_and_a_wrong_key_is_rejected() {
    let data = seed();
    {
        let (_s, _b, vault) = data.open();
        vault.set_enabled(true).unwrap();
        run_to_end(&vault);
    }
    let key_ref = {
        let store = Store::open(data.db()).unwrap();
        SecretRef(store.vault_state().unwrap().key_ref.unwrap())
    };
    let saved = data.secrets.get(&key_ref).unwrap().unwrap();

    // The keychain lost the key.
    data.secrets.delete(&key_ref).unwrap();
    let (store, _blobs, vault) = data.open();
    let err = vault.open().unwrap_err();
    assert!(matches!(err, VaultError::Locked(_)), "{err}");
    assert_eq!(err.code(), ErrorCode::Locked);
    assert!(!vault.status().unwrap().key_in_keychain);
    assert!(matches!(
        store.get_message(&"m1".into()),
        Err(nail_store::Error::Locked(_))
    ));

    // Another key under the same reference.
    data.secrets
        .set(&key_ref, &nail_vault::DataKey::generate().to_hex())
        .unwrap();
    let (_store, _blobs, vault) = data.open();
    assert!(matches!(vault.open(), Err(VaultError::Crypto(_))));

    // A locked migration records its error and resumes once the key is back.
    data.secrets.delete(&key_ref).unwrap();
    {
        let (_s, _b, vault) = data.open();
        // Decrypt needs the key: set_enabled fails, nothing changes.
        assert!(vault.set_enabled(false).is_err());
        assert_eq!(vault.status().unwrap().state, VaultState::Encrypted);
    }
    data.secrets.set(&key_ref, &saved).unwrap();
    let (store, blobs, vault) = data.open();
    vault.open().unwrap();
    vault.set_enabled(false).unwrap();
    data.secrets.delete(&key_ref).unwrap();
    let (_s2, _b2, vault2) = data.open();
    let err = vault2
        .migrate(&mut |_| ControlFlow::Continue(()))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Locked);
    let status = vault2.status().unwrap();
    assert_eq!(status.state, VaultState::Decrypting);
    assert_eq!(status.error, Some(ErrorCode::Locked));
    assert!(status.message.is_some());
    data.secrets.set(&key_ref, &saved).unwrap();
    let (status, _) = run_to_end(&vault2);
    assert_eq!(status.state, VaultState::Plain);
    assert_eq!(status.error, None);
    assert_intact(&data, &store, &blobs, &vault);
}

/// A keychain that refuses to store anything.
#[derive(Debug)]
struct ReadOnlyKeychain;

impl SecretStore for ReadOnlyKeychain {
    fn get(&self, _: &SecretRef) -> Result<Option<String>, EngineError> {
        Ok(None)
    }
    fn set(&self, _: &SecretRef, _: &str) -> Result<(), EngineError> {
        Err(EngineError::temporary(
            ErrorCode::Unavailable,
            "keychain locked",
        ))
    }
    fn delete(&self, _: &SecretRef) -> Result<bool, EngineError> {
        Ok(false)
    }
}

#[test]
fn a_failing_keychain_leaves_the_directory_plain() {
    let data = seed();
    let store = Store::open(data.db()).unwrap();
    let blobs = BlobStore::open(data.blob_dir()).unwrap();
    let vault = Vault::new(
        store.clone(),
        blobs.clone(),
        Some(data.index_dir()),
        Arc::new(ReadOnlyKeychain),
    );
    let err = vault.set_enabled(true).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Unavailable);
    let record = store.vault_state().unwrap();
    assert_eq!(record.state, VaultState::Plain);
    assert_eq!(record.key_ref, None);
    // Nothing is sealed.
    let hash = blobs.put(b"after the failure").unwrap();
    assert!(!blobs.is_sealed(&hash).unwrap());
}

#[test]
fn the_version_file_name_matches_nail_search() {
    assert_eq!(INDEX_VERSION_FILE, nail_search::VERSION_FILE_NAME);
}
