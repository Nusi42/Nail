//! Encryption-at-rest hooks of the store: sealed columns, conversion both
//! ways, locked reads, and compaction that leaves no plaintext behind.

mod common;

use std::sync::Arc;

use common::{account, folder, message};
use nail_api::{Draft, FolderRole};
use nail_store::{
    CipherError, DataCipher, Error, SEALED_FIELD_PREFIX, SEALED_MAGIC, SealMode, Store,
};

/// A reversible stand-in cipher (the real one lives in nail-vault):
/// magic + context + bytes XOR key. It hides ASCII text from a byte search,
/// which is all these tests look for.
#[derive(Debug)]
struct TestCipher(u8);

impl DataCipher for TestCipher {
    fn seal(&self, context: &[u8], plain: &[u8]) -> Result<Vec<u8>, CipherError> {
        let mut out = SEALED_MAGIC.to_vec();
        out.push(u8::try_from(context.len()).unwrap());
        out.extend_from_slice(context);
        out.extend(plain.iter().map(|b| b ^ self.0));
        Ok(out)
    }

    fn open(&self, context: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CipherError> {
        let rest = sealed
            .strip_prefix(&SEALED_MAGIC)
            .ok_or_else(|| CipherError::Failed("magic".into()))?;
        let (&len, rest) = rest.split_first().unwrap();
        let (ctx, body) = rest.split_at(usize::from(len));
        if ctx != context {
            return Err(CipherError::Failed("context".into()));
        }
        Ok(body.iter().map(|b| b ^ self.0).collect())
    }
}

fn cipher() -> Option<Arc<dyn DataCipher>> {
    Some(Arc::new(TestCipher(0x77)))
}

fn seed(store: &Store) {
    store.upsert_account(&account("a1")).unwrap();
    store
        .upsert_folder(&folder("inbox", "a1", "INBOX", None, FolderRole::Inbox))
        .unwrap();
}

fn draft(body: &str) -> Draft {
    serde_json::from_value(serde_json::json!({
        "identity_id": "a1@example.org",
        "to": [{"name": null, "email": "bob@example.org"}],
        "subject": "Plans",
        "text": body,
    }))
    .unwrap()
}

fn raw_preview(store: &Store, dir: &std::path::Path, id: &str) -> String {
    // A second, plain connection sees what is on disk.
    let _ = store;
    let conn = rusqlite::Connection::open(dir.join("nail.sqlite3")).unwrap();
    conn.query_row("SELECT preview FROM messages WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .unwrap()
}

#[test]
fn sealed_columns_round_trip_and_hide_content_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("nail.sqlite3")).unwrap();
    seed(&store);
    store.set_cipher(cipher(), SealMode::Seal).unwrap();
    assert!(store.seals_writes());

    let mut m = message("m1", "a1", "inbox", "2026-09-01T10:00:00Z");
    m.summary.preview = "The launch code is 1234".into();
    store.upsert_message(&m).unwrap();
    let draft_id = store.save_draft(&draft("Meet at the harbour")).unwrap();

    // Readable through the store.
    assert_eq!(
        store.get_message(&"m1".into()).unwrap().summary.preview,
        "The launch code is 1234"
    );
    assert_eq!(
        store.message_summaries(&["m1".into()]).unwrap()[0].preview,
        "The launch code is 1234"
    );
    assert_eq!(
        store.get_draft(&draft_id).unwrap().text,
        "Meet at the harbour"
    );
    // Sealed on disk.
    let raw = raw_preview(&store, dir.path(), "m1");
    assert!(raw.starts_with(SEALED_FIELD_PREFIX), "{raw:?}");
    assert!(!raw.contains("launch"));

    let counts = store.sealed_field_counts().unwrap();
    assert_eq!((counts.sealed, counts.plain), (2, 0));

    // Without the cipher: locked, not garbage.
    store.set_cipher(None, SealMode::Plain).unwrap();
    assert!(matches!(
        store.get_message(&"m1".into()),
        Err(Error::Locked(_))
    ));
    assert!(matches!(store.get_draft(&draft_id), Err(Error::Locked(_))));
    let api: nail_api::ApiError = store.get_draft(&draft_id).unwrap_err().into();
    assert_eq!(api.code, nail_api::ErrorCode::Locked);

    // A wrong key is a crypto error.
    store
        .set_cipher(Some(Arc::new(WrongContext)), SealMode::Seal)
        .unwrap();
    assert!(matches!(
        store.get_message(&"m1".into()),
        Err(Error::Crypto(_))
    ));
}

/// A cipher that can never open anything.
#[derive(Debug)]
struct WrongContext;

impl DataCipher for WrongContext {
    fn seal(&self, _: &[u8], plain: &[u8]) -> Result<Vec<u8>, CipherError> {
        let mut out = SEALED_MAGIC.to_vec();
        out.extend_from_slice(plain);
        Ok(out)
    }
    fn open(&self, _: &[u8], _: &[u8]) -> Result<Vec<u8>, CipherError> {
        Err(CipherError::Failed("authentication failed".into()))
    }
}

#[test]
fn convert_fields_both_ways_in_batches_and_compact_removes_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nail.sqlite3");
    let store = Store::open(&path).unwrap();
    seed(&store);
    for i in 0..25 {
        let mut m = message(&format!("m{i}"), "a1", "inbox", "2026-09-01T10:00:00Z");
        m.summary.preview = format!("confidential-preview-{i}");
        store.upsert_message(&m).unwrap();
    }
    for i in 0..3 {
        store
            .save_draft(&draft(&format!("confidential-draft-{i}")))
            .unwrap();
    }
    let counts = store.sealed_field_counts().unwrap();
    assert_eq!((counts.sealed, counts.plain, counts.total()), (0, 28, 28));

    // Converting needs a cipher.
    assert!(matches!(
        store.convert_fields(true, 10),
        Err(Error::Locked(_))
    ));

    // Encrypting: new writes sealed, old ones converted in batches.
    store.set_cipher(cipher(), SealMode::Seal).unwrap();
    let mut batches = 0;
    while store.convert_fields(true, 10).unwrap() > 0 {
        batches += 1;
    }
    assert_eq!(batches, 3);
    let counts = store.sealed_field_counts().unwrap();
    assert_eq!((counts.sealed, counts.plain), (28, 0));
    for i in 0..25 {
        assert_eq!(
            store
                .get_message(&format!("m{i}").as_str().into())
                .unwrap()
                .summary
                .preview,
            format!("confidential-preview-{i}")
        );
    }
    store.compact().unwrap();
    drop(store);
    let bytes = std::fs::read(&path).unwrap();
    let wal = std::fs::read(dir.path().join("nail.sqlite3-wal")).unwrap_or_default();
    for needle in [&b"confidential-preview"[..], b"confidential-draft"] {
        assert!(
            !bytes.windows(needle.len()).any(|w| w == needle),
            "plaintext left in the database file"
        );
        assert!(
            !wal.windows(needle.len()).any(|w| w == needle),
            "plaintext left in the WAL"
        );
    }

    // Decrypting back.
    let store = Store::open(&path).unwrap();
    store.set_cipher(cipher(), SealMode::Plain).unwrap();
    assert!(!store.seals_writes());
    while store.convert_fields(false, 7).unwrap() > 0 {}
    let counts = store.sealed_field_counts().unwrap();
    assert_eq!((counts.sealed, counts.plain), (0, 28));
    store.set_cipher(None, SealMode::Plain).unwrap();
    let drafts = store.list_drafts().unwrap();
    assert_eq!(drafts.len(), 3);
    assert!(
        drafts
            .iter()
            .all(|d| d.text.starts_with("confidential-draft-"))
    );
}

#[test]
fn plain_mode_with_cipher_reads_sealed_and_writes_plain() {
    let store = Store::open_in_memory().unwrap();
    seed(&store);
    store.set_cipher(cipher(), SealMode::Seal).unwrap();
    let mut m = message("m1", "a1", "inbox", "2026-09-01T10:00:00Z");
    m.summary.preview = "sealed first".into();
    store.upsert_message(&m).unwrap();
    // Decrypting: reads work, a new write is plain.
    store.set_cipher(cipher(), SealMode::Plain).unwrap();
    assert_eq!(
        store.get_message(&"m1".into()).unwrap().summary.preview,
        "sealed first"
    );
    let mut m2 = message("m2", "a1", "inbox", "2026-09-01T11:00:00Z");
    m2.summary.preview = "plain second".into();
    store.upsert_message(&m2).unwrap();
    let counts = store.sealed_field_counts().unwrap();
    assert_eq!((counts.sealed, counts.plain), (1, 1));
}

/// Values that merely look similar to the prefix are not taken for sealed
/// ones.
#[test]
fn prefix_lookalikes_stay_plain() {
    let store = Store::open_in_memory().unwrap();
    seed(&store);
    let mut m = message("m1", "a1", "inbox", "2026-09-01T10:00:00Z");
    m.summary.preview = "nlv1: not sealed".into();
    store.upsert_message(&m).unwrap();
    assert_eq!(
        store.get_message(&"m1".into()).unwrap().summary.preview,
        "nlv1: not sealed"
    );
    assert_eq!(store.sealed_field_counts().unwrap().plain, 1);
}

/// Fails after `n` successful seals: a crash in the middle of a batch.
#[derive(Debug)]
struct FailAfter(std::sync::atomic::AtomicUsize);

impl DataCipher for FailAfter {
    fn seal(&self, context: &[u8], plain: &[u8]) -> Result<Vec<u8>, CipherError> {
        let left = self.0.load(std::sync::atomic::Ordering::SeqCst);
        if left == 0 {
            return Err(CipherError::Failed("simulated failure".into()));
        }
        self.0.store(left - 1, std::sync::atomic::Ordering::SeqCst);
        TestCipher(0x77).seal(context, plain)
    }
    fn open(&self, context: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CipherError> {
        TestCipher(0x77).open(context, sealed)
    }
}

#[test]
fn a_failure_inside_a_batch_rolls_the_whole_batch_back() {
    let store = Store::open_in_memory().unwrap();
    seed(&store);
    for i in 0..10 {
        let mut m = message(&format!("m{i}"), "a1", "inbox", "2026-09-01T10:00:00Z");
        m.summary.preview = format!("p{i}");
        store.upsert_message(&m).unwrap();
    }
    store
        .set_cipher(
            Some(Arc::new(FailAfter(std::sync::atomic::AtomicUsize::new(4)))),
            SealMode::Seal,
        )
        .unwrap();
    let err = store.convert_fields(true, 10).unwrap_err();
    assert!(matches!(err, Error::Crypto(_)), "{err}");
    let counts = store.sealed_field_counts().unwrap();
    assert_eq!((counts.sealed, counts.plain), (0, 10), "nothing half-done");
    // Resuming with a working cipher completes.
    store.set_cipher(cipher(), SealMode::Seal).unwrap();
    while store.convert_fields(true, 4).unwrap() > 0 {}
    assert_eq!(store.sealed_field_counts().unwrap().sealed, 10);
}
