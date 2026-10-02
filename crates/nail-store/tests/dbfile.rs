//! Full-file database encryption (M4c WP122, ADR 0095): keyed
//! opens, wrong / missing keys, nothing readable on disk (file and WAL),
//! crash-safe rewrites plain <-> encrypted <-> another key.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::{account, folder, message};
use nail_api::FolderRole;
use nail_store::dbfile::{REWRITE_SUFFIX, rewrite_temp_path};
use nail_store::{
    CipherError, DataCipher, DatabaseKey, Error, FileEncryption, SEALED_MAGIC, SealMode, Store,
    file_encryption,
};

const SECRET: &str = "Quarterly figures for the harbour project";

fn key(byte: u8) -> DatabaseKey {
    DatabaseKey::from_bytes([byte; 32])
}

fn seed(store: &Store) {
    store.upsert_account(&account("a1")).unwrap();
    store
        .upsert_folder(&folder("inbox", "a1", "INBOX", None, FolderRole::Inbox))
        .unwrap();
    let mut m = message("m1", "a1", "inbox", "2026-09-01T10:00:00Z");
    m.summary.subject = SECRET.into();
    store.upsert_message(&m).unwrap();
}

fn subject(store: &Store) -> String {
    store.get_message(&"m1".into()).unwrap().summary.subject
}

/// Every file of the database (main, WAL, shared memory) as one byte
/// string.
fn on_disk(path: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        if let Ok(bytes) = std::fs::read(&name) {
            out.extend(bytes);
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn a_new_file_is_created_encrypted_and_needs_its_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nail.sqlite3");
    {
        let store = Store::open_with_key(&path, Some(&key(1))).unwrap();
        assert!(store.is_file_encrypted());
        seed(&store);
        assert_eq!(subject(&store), SECRET);
        // While open, the WAL holds the latest pages — encrypted too.
        assert!(!contains(&on_disk(&path), SECRET));
        assert!(!contains(&on_disk(&path), "harbour"));
    }
    assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Encrypted);
    assert!(!contains(&on_disk(&path), SECRET));

    // No key: locked (not "damaged"); wrong key: crypto.
    assert!(matches!(Store::open(&path), Err(Error::Locked(_))));
    assert!(matches!(
        Store::open_with_key(&path, Some(&key(2))),
        Err(Error::Crypto(_))
    ));
    let api: nail_api::ApiError = Store::open(&path).unwrap_err().into();
    assert_eq!(api.code, nail_api::ErrorCode::Locked);

    let store = Store::open_with_key(&path, Some(&key(1))).unwrap();
    assert_eq!(subject(&store), SECRET);
    assert_eq!(store.schema_version().unwrap(), nail_store::SCHEMA_VERSION);
}

#[test]
fn a_plain_file_opens_plain_even_with_a_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nail.sqlite3");
    seed(&Store::open(&path).unwrap());
    let store = Store::open_with_key(&path, Some(&key(1))).unwrap();
    assert!(!store.is_file_encrypted());
    assert_eq!(subject(&store), SECRET);
    assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Plain);
}

#[test]
fn rewrite_encrypts_rekeys_and_decrypts_with_the_store_staying_usable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nail.sqlite3");
    let store = Store::open(&path).unwrap();
    let clone = store.clone();
    seed(&store);
    assert!(contains(&on_disk(&path), SECRET));

    // Plain -> encrypted.
    store.rewrite_file(Some(&key(1))).unwrap();
    assert!(store.is_file_encrypted());
    assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Encrypted);
    assert!(!contains(&on_disk(&path), SECRET));
    // Every clone uses the new connection; writes keep working.
    assert_eq!(subject(&clone), SECRET);
    let mut m2 = message("m2", "a1", "inbox", "2026-09-02T10:00:00Z");
    m2.summary.subject = "Second secret subject".into();
    clone.upsert_message(&m2).unwrap();
    assert!(!contains(&on_disk(&path), "Second secret"));
    assert!(!rewrite_temp_path(&path).exists());

    // Encrypted -> another key.
    store.rewrite_file(Some(&key(2))).unwrap();
    drop((store, clone));
    assert!(matches!(
        Store::open_with_key(&path, Some(&key(1))),
        Err(Error::Crypto(_))
    ));
    let store = Store::open_with_key(&path, Some(&key(2))).unwrap();
    assert_eq!(subject(&store), SECRET);
    assert_eq!(
        store
            .count_messages(&nail_store::MessageFilter::All)
            .unwrap(),
        2
    );

    // Encrypted -> plain.
    store.rewrite_file(None).unwrap();
    assert!(!store.is_file_encrypted());
    assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Plain);
    assert_eq!(subject(&store), SECRET);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store
            .count_messages(&nail_store::MessageFilter::All)
            .unwrap(),
        2
    );
}

#[test]
fn a_crash_before_the_swap_leaves_the_old_file_and_the_leftover_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nail.sqlite3");
    seed(&Store::open(&path).unwrap());
    // What an interrupted VACUUM INTO leaves: a partial copy (and journal).
    let tmp = rewrite_temp_path(&path);
    assert!(tmp.to_string_lossy().ends_with(REWRITE_SUFFIX));
    std::fs::write(&tmp, b"partial copy").unwrap();
    let mut journal = tmp.as_os_str().to_owned();
    journal.push("-journal");
    std::fs::write(&journal, b"x").unwrap();

    let store = Store::open(&path).unwrap();
    assert!(!tmp.exists());
    assert!(!Path::new(&journal).exists());
    assert_eq!(subject(&store), SECRET);
    // A rewrite also clears a leftover first.
    std::fs::write(&tmp, b"partial copy").unwrap();
    store.rewrite_file(Some(&key(3))).unwrap();
    assert_eq!(subject(&store), SECRET);
}

#[test]
fn a_crash_after_the_swap_opens_the_new_file_by_its_header() {
    // After the rename the header alone says what the file is: opening
    // with the key works and no state elsewhere is needed.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nail.sqlite3");
    {
        let store = Store::open(&path).unwrap();
        seed(&store);
        store.rewrite_file(Some(&key(4))).unwrap();
        // "Crash": the process ends without a clean shutdown of anything
        // else; the store is simply dropped.
    }
    assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Encrypted);
    let store = Store::open_with_key(&path, Some(&key(4))).unwrap();
    assert_eq!(subject(&store), SECRET);
}

#[test]
fn rewriting_keeps_sealed_columns_working() {
    #[derive(Debug)]
    struct Xor;
    impl DataCipher for Xor {
        fn seal(&self, context: &[u8], plain: &[u8]) -> Result<Vec<u8>, CipherError> {
            let mut out = SEALED_MAGIC.to_vec();
            out.push(u8::try_from(context.len()).unwrap());
            out.extend_from_slice(context);
            out.extend(plain.iter().map(|b| b ^ 0x33));
            Ok(out)
        }
        fn open(&self, context: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CipherError> {
            let rest = sealed.strip_prefix(&SEALED_MAGIC).unwrap();
            let (&len, rest) = rest.split_first().unwrap();
            let (_, body) = rest.split_at(usize::from(len));
            let _ = context;
            Ok(body.iter().map(|b| b ^ 0x33).collect())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nail.sqlite3");
    let store = Store::open(&path).unwrap();
    seed(&store);
    store
        .set_cipher(Some(Arc::new(Xor)), SealMode::Seal)
        .unwrap();
    store.rewrite_file(Some(&key(5))).unwrap();
    // The SQL functions are registered on the new connection.
    let mut m = message("m3", "a1", "inbox", "2026-09-03T10:00:00Z");
    m.summary.preview = "sealed preview".into();
    store.upsert_message(&m).unwrap();
    assert_eq!(
        store.get_message(&"m3".into()).unwrap().summary.preview,
        "sealed preview"
    );
    assert_eq!(store.sealed_field_counts().unwrap().sealed, 1);
}

#[test]
fn in_memory_stores_cannot_be_rewritten() {
    let store = Store::open_in_memory().unwrap();
    assert!(!store.is_file_encrypted());
    assert!(matches!(
        store.rewrite_file(Some(&key(1))),
        Err(Error::InvalidData(_))
    ));
}
