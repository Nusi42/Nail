//! Content-addressed blob store for raw messages (and later attachments).
//!
//! * A blob is addressed by the SHA-256 of its **uncompressed** bytes, written
//!   as 64 lowercase hex characters ([`BlobHash`]).
//! * On disk a blob lives at `<root>/<hh>/<rest>`, where `hh` are the first two
//!   hex characters and `rest` the remaining 62, compressed with zstd.
//! * Writes are atomic: the compressed bytes go to a temporary file in the
//!   target directory, are flushed to disk, then renamed into place. A reader
//!   never sees a half-written blob, and a crash leaves at most a stray
//!   `.tmp*` file.
//! * Identical content is stored once: [`BlobStore::put`] of existing content
//!   only computes the hash.
//! * [`BlobStore::get`] verifies the hash of the decompressed bytes, so bit rot
//!   or tampering is reported as [`Error::BlobCorrupt`] instead of returning
//!   wrong data.
//!
//! * Encryption at rest (M2a WP33): with a [`DataCipher`] installed
//!   ([`BlobStore::set_cipher`], [`SealMode::Seal`]) the compressed bytes
//!   are sealed before they are written (the file then starts with
//!   [`SEALED_MAGIC`] instead of the zstd magic), bound to the blob's hash.
//!   Reading accepts both forms, so a migration can convert file by file
//!   ([`BlobStore::convert`], atomic per file) while the store is in use.
//!   File names stay the plaintext hash (the database holds it anyway).
//!
//! * Removals are counted ([`BlobStore::removal_mark`] /
//!   [`BlobStore::removed_since`]), so a writer that stored blobs before the
//!   rows referencing them can tell cheaply whether a garbage collection
//!   may have deleted one in between (and only then check the files again).
//!
//! The API is synchronous (file I/O); async callers run it on
//! `tokio::task::spawn_blocking`.

use std::fmt;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::seal::{CipherSlot, DataCipher, SEALED_MAGIC, SealMode};

/// SHA-256 of a blob's uncompressed content, as 64 lowercase hex characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BlobHash(String);

impl BlobHash {
    /// Hashes `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(bytes)))
    }

    /// Parses and validates a hash (64 hex characters; upper case is
    /// normalized to lower case).
    pub fn parse(s: &str) -> Result<Self> {
        if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(Self(s.to_ascii_lowercase()))
        } else {
            Err(Error::InvalidHash(s.to_owned()))
        }
    }

    /// The hash as a hex string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BlobHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for BlobHash {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

impl TryFrom<String> for BlobHash {
    type Error = Error;

    fn try_from(s: String) -> Result<Self> {
        Self::parse(&s)
    }
}

impl From<BlobHash> for String {
    fn from(hash: BlobHash) -> Self {
        hash.0
    }
}

/// A directory of zstd-compressed, content-addressed blobs.
#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
    level: i32,
    /// Encryption at rest, shared by every clone.
    cipher: CipherSlot,
    /// Removal counters, shared by every clone.
    removals: Arc<RemovalCounters>,
}

/// Removals started and finished (see [`BlobStore::removal_mark`]).
#[derive(Debug, Default)]
struct RemovalCounters {
    started: AtomicU64,
    finished: AtomicU64,
}

/// The removal counters at one moment ([`BlobStore::removal_mark`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemovalMark {
    started: u64,
    /// No removal was running when the mark was taken.
    idle: bool,
}

/// Decompresses a blob's content and checks it against its hash
/// ([`Error::BlobCorrupt`] when either fails).
fn decompress_verified(hash: &BlobHash, compressed: &[u8]) -> Result<Vec<u8>> {
    let bytes = zstd::decode_all(compressed).map_err(|_| Error::BlobCorrupt(hash.clone()))?;
    if BlobHash::of(&bytes) == *hash {
        Ok(bytes)
    } else {
        Err(Error::BlobCorrupt(hash.clone()))
    }
}

fn blob_context(hash: &BlobHash) -> Vec<u8> {
    format!("nail/blob/{hash}").into_bytes()
}

/// Whether a blob file's bytes are sealed.
fn is_sealed(bytes: &[u8]) -> bool {
    bytes.starts_with(&SEALED_MAGIC)
}

impl BlobStore {
    /// Opens (and creates, if needed) a blob store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            level: zstd::DEFAULT_COMPRESSION_LEVEL,
            cipher: CipherSlot::default(),
            removals: Arc::default(),
        })
    }

    /// Marks the current point in the blob store's removals. Take it before
    /// storing blobs that rows will reference; [`Self::removed_since`] then
    /// says whether a removal may have deleted one of them meanwhile.
    pub fn removal_mark(&self) -> RemovalMark {
        // `finished` first: a removal that starts in between makes the
        // counters differ, so the mark is not idle (conservative).
        let finished = self.removals.finished.load(Ordering::SeqCst);
        let started = self.removals.started.load(Ordering::SeqCst);
        RemovalMark {
            started,
            idle: started == finished,
        }
    }

    /// Whether a removal ran (or was running) since `mark`: only then can a
    /// blob stored after the mark be gone again.
    pub fn removed_since(&self, mark: RemovalMark) -> bool {
        !mark.idle || self.removals.started.load(Ordering::SeqCst) != mark.started
    }

    /// Installs (or removes, with `None`) the data cipher. Every clone of
    /// this blob store sees the change. With [`SealMode::Seal`] new blobs
    /// are sealed; sealed blobs are readable whenever a cipher is installed.
    pub fn set_cipher(&self, cipher: Option<Arc<dyn DataCipher>>, mode: SealMode) {
        self.cipher.set(cipher, mode);
    }

    /// The root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file a blob is (or would be) stored in.
    pub fn path_of(&self, hash: &BlobHash) -> PathBuf {
        let (dir, file) = hash.as_str().split_at(2);
        self.root.join(dir).join(file)
    }

    /// Whether a blob with this hash is stored.
    pub fn contains(&self, hash: &BlobHash) -> bool {
        self.path_of(hash).is_file()
    }

    /// Stores `bytes` and returns their hash. Storing content that already
    /// exists is a no-op apart from hashing.
    pub fn put(&self, bytes: &[u8]) -> Result<BlobHash> {
        let hash = BlobHash::of(bytes);
        let path = self.path_of(&hash);
        if path.is_file() {
            tracing::trace!(%hash, "blob already stored");
            return Ok(hash);
        }
        let dir = path.parent().unwrap_or(&self.root);
        std::fs::create_dir_all(dir)?;

        let compressed = zstd::encode_all(bytes, self.level)?;
        let compressed = match self
            .cipher
            .seal_if_active(&blob_context(&hash), &compressed)?
        {
            Some(sealed) => sealed,
            None => compressed,
        };
        let mut tmp = tempfile::Builder::new().prefix(".tmp").tempfile_in(dir)?;
        tmp.write_all(&compressed)?;
        tmp.as_file().sync_all()?;
        match tmp.persist_noclobber(&path) {
            Ok(_) => {}
            // A concurrent writer stored the same content first; same hash,
            // same bytes, so theirs is as good as ours (the temp file is
            // deleted when the error drops).
            Err(err) if err.error.kind() == ErrorKind::AlreadyExists || path.is_file() => {}
            Err(err) => return Err(err.error.into()),
        }
        tracing::debug!(%hash, size = bytes.len(), stored = compressed.len(), "blob stored");
        Ok(hash)
    }

    /// Reads, decompresses and verifies a blob.
    ///
    /// Returns [`Error::NotFound`] when the blob does not exist and
    /// [`Error::BlobCorrupt`] when it cannot be decompressed or its content
    /// does not match `hash`.
    ///
    /// Returns [`Error::Locked`] for a sealed blob without an installed
    /// cipher and [`Error::Crypto`] when it cannot be opened.
    pub fn get(&self, hash: &BlobHash) -> Result<Vec<u8>> {
        let stored = self.read_file(hash)?;
        let compressed = if is_sealed(&stored) {
            self.cipher.open(&blob_context(hash), &stored)?
        } else {
            stored
        };
        let bytes = decompress_verified(hash, &compressed)?;
        Ok(bytes)
    }

    fn read_file(&self, hash: &BlobHash) -> Result<Vec<u8>> {
        match std::fs::read(self.path_of(hash)) {
            Ok(bytes) => Ok(bytes),
            Err(err) if err.kind() == ErrorKind::NotFound => Err(Error::not_found("blob", hash)),
            Err(err) => Err(err.into()),
        }
    }

    /// Whether a stored blob is sealed.
    pub fn is_sealed(&self, hash: &BlobHash) -> Result<bool> {
        use std::io::Read as _;
        let mut file = match std::fs::File::open(self.path_of(hash)) {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return Err(Error::not_found("blob", hash));
            }
            Err(err) => return Err(err.into()),
        };
        let mut head = [0u8; SEALED_MAGIC.len()];
        let mut read = 0;
        while read < head.len() {
            match file.read(&mut head[read..])? {
                0 => break,
                n => read += n,
            }
        }
        Ok(is_sealed(&head[..read]))
    }

    /// Every stored blob, in no particular order (temporary files and
    /// foreign names are skipped).
    pub fn hashes(&self) -> Result<Vec<BlobHash>> {
        let mut out = Vec::new();
        for dir in std::fs::read_dir(&self.root)? {
            let dir = dir?;
            let prefix = dir.file_name().to_string_lossy().into_owned();
            if prefix.len() != 2 || !dir.file_type()?.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(dir.path())? {
                let file = file?;
                let rest = file.file_name().to_string_lossy().into_owned();
                if let Ok(hash) = BlobHash::parse(&format!("{prefix}{rest}"))
                    && file.file_type()?.is_file()
                {
                    out.push(hash);
                }
            }
        }
        Ok(out)
    }

    /// Rewrites one blob sealed (`seal = true`) or plain. Returns whether
    /// the file changed (`false` when it already had that form). The new
    /// content is written to a temporary file, synced and renamed over the
    /// old one, so a crash leaves either form, never a torn file; the
    /// content is verified against its hash before the old file is
    /// replaced. Needs an installed cipher.
    pub fn convert(&self, hash: &BlobHash, seal: bool) -> Result<bool> {
        let stored = self.read_file(hash)?;
        if is_sealed(&stored) == seal {
            return Ok(false);
        }
        let context = blob_context(hash);
        let compressed = if seal {
            stored
        } else {
            self.cipher.open(&context, &stored)?
        };
        decompress_verified(hash, &compressed)?;
        let new = if seal {
            self.cipher.seal(&context, &compressed)?
        } else {
            compressed
        };
        let path = self.path_of(hash);
        let dir = path.parent().unwrap_or(&self.root);
        let mut tmp = tempfile::Builder::new().prefix(".tmp").tempfile_in(dir)?;
        tmp.write_all(&new)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&path).map_err(|err| err.error)?;
        Ok(true)
    }

    /// Deletes leftover temporary files (`.tmp*`) of interrupted writes
    /// that are at least `min_age` old (by modification time). A running
    /// write's temporary file is young, so a minimum age of a few minutes
    /// keeps concurrent writes safe; `Duration::ZERO` removes all (only
    /// when nothing else writes). Returns how many were removed.
    pub fn remove_temp_files(&self, min_age: std::time::Duration) -> Result<usize> {
        let mut removed = 0;
        for dir in std::fs::read_dir(&self.root)? {
            let dir = dir?;
            if !dir.file_type()?.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(dir.path())? {
                let file = file?;
                let old_enough = || -> std::io::Result<bool> {
                    let modified = file.metadata()?.modified()?;
                    Ok(modified.elapsed().unwrap_or_default() >= min_age)
                };
                if file.file_name().to_string_lossy().starts_with(".tmp") && old_enough()? {
                    match std::fs::remove_file(file.path()) {
                        Ok(()) => removed += 1,
                        Err(err) if err.kind() == ErrorKind::NotFound => {}
                        Err(err) => return Err(err.into()),
                    }
                }
            }
        }
        Ok(removed)
    }

    /// Deletes a blob. Returns whether it existed. The caller is responsible
    /// for making sure no message references it any more.
    pub fn remove(&self, hash: &BlobHash) -> Result<bool> {
        self.removals.started.fetch_add(1, Ordering::SeqCst);
        let result = match std::fs::remove_file(self.path_of(hash)) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        };
        self.removals.finished.fetch_add(1, Ordering::SeqCst);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, BlobStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path().join("blobs")).unwrap();
        (dir, store)
    }

    #[test]
    fn removal_marks_see_removals_of_every_clone() {
        let (_dir, store) = store();
        let hash = store.put(b"kept").unwrap();
        let mark = store.removal_mark();
        assert!(!store.removed_since(mark));
        store.put(b"more").unwrap();
        store.get(&hash).unwrap();
        assert!(
            !store.removed_since(mark),
            "writes and reads are no removals"
        );
        let clone = store.clone();
        clone.remove(&hash).unwrap();
        assert!(store.removed_since(mark));
        // Removing a missing blob still counts (it may race a writer).
        let mark = store.removal_mark();
        assert!(!clone.remove(&hash).unwrap());
        assert!(store.removed_since(mark));
        let after = store.removal_mark();
        assert!(!store.removed_since(after));
    }

    #[test]
    fn a_mark_taken_during_a_removal_is_not_idle() {
        let (_dir, store) = store();
        store.removals.started.fetch_add(1, Ordering::SeqCst);
        let mark = store.removal_mark();
        store.removals.finished.fetch_add(1, Ordering::SeqCst);
        assert!(store.removed_since(mark));
    }

    #[test]
    fn hash_is_sha256_hex() {
        assert_eq!(
            BlobHash::of(b"abc").as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn parse_validates_and_normalizes() {
        let upper = "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";
        assert_eq!(BlobHash::parse(upper).unwrap(), BlobHash::of(b"abc"));
        assert!(matches!(BlobHash::parse("abc"), Err(Error::InvalidHash(_))));
        assert!(BlobHash::parse(&"g".repeat(64)).is_err());
        // Path traversal attempts are not valid hashes.
        assert!(BlobHash::parse(&format!("..{}", "a".repeat(62))).is_err());
        let json = serde_json::to_string(&BlobHash::of(b"abc")).unwrap();
        let back: BlobHash = serde_json::from_str(&json).unwrap();
        assert_eq!(back, BlobHash::of(b"abc"));
        assert!(serde_json::from_str::<BlobHash>("\"nope\"").is_err());
    }

    #[test]
    fn put_get_round_trip_and_layout() {
        let (_dir, blobs) = store();
        let content = b"From: ada@example.org\r\nSubject: hi\r\n\r\nHello\r\n".repeat(50);
        let hash = blobs.put(&content).unwrap();
        assert_eq!(hash, BlobHash::of(&content));

        let path = blobs.path_of(&hash);
        assert_eq!(
            path,
            blobs
                .root()
                .join(&hash.as_str()[..2])
                .join(&hash.as_str()[2..])
        );
        assert!(blobs.contains(&hash));
        // Stored compressed.
        let on_disk = std::fs::read(&path).unwrap();
        assert!(on_disk.len() < content.len());
        assert_eq!(blobs.get(&hash).unwrap(), content);
    }

    #[test]
    fn empty_blob_round_trips() {
        let (_dir, blobs) = store();
        let hash = blobs.put(b"").unwrap();
        assert_eq!(blobs.get(&hash).unwrap(), b"");
    }

    #[test]
    fn put_deduplicates() {
        let (_dir, blobs) = store();
        let a = blobs.put(b"same").unwrap();
        let modified = std::fs::metadata(blobs.path_of(&a))
            .unwrap()
            .modified()
            .unwrap();
        let b = blobs.put(b"same").unwrap();
        assert_eq!(a, b);
        assert_eq!(
            std::fs::metadata(blobs.path_of(&a))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
        let c = blobs.put(b"other").unwrap();
        assert_ne!(a, c);

        // Exactly two blob files and no temp files left behind.
        let mut files = Vec::new();
        for dir in std::fs::read_dir(blobs.root()).unwrap() {
            for file in std::fs::read_dir(dir.unwrap().path()).unwrap() {
                files.push(file.unwrap().file_name());
            }
        }
        assert_eq!(files.len(), 2, "{files:?}");
    }

    #[test]
    fn missing_blob_is_not_found() {
        let (_dir, blobs) = store();
        let err = blobs.get(&BlobHash::of(b"never stored")).unwrap_err();
        assert!(err.is_not_found(), "{err}");
        assert!(!blobs.remove(&BlobHash::of(b"never stored")).unwrap());
    }

    #[test]
    fn corruption_is_detected() {
        let (_dir, blobs) = store();

        // Garbage that is not zstd at all.
        let hash = blobs.put(b"original content").unwrap();
        std::fs::write(blobs.path_of(&hash), b"not zstd").unwrap();
        assert!(matches!(blobs.get(&hash), Err(Error::BlobCorrupt(h)) if h == hash));

        // Valid zstd, but different content than the hash says.
        let hash = blobs.put(b"second original").unwrap();
        let forged = zstd::encode_all(&b"tampered"[..], 3).unwrap();
        std::fs::write(blobs.path_of(&hash), forged).unwrap();
        assert!(matches!(blobs.get(&hash), Err(Error::BlobCorrupt(h)) if h == hash));
    }

    #[test]
    fn remove_deletes() {
        let (_dir, blobs) = store();
        let hash = blobs.put(b"bye").unwrap();
        assert!(blobs.remove(&hash).unwrap());
        assert!(!blobs.contains(&hash));
        assert!(blobs.get(&hash).unwrap_err().is_not_found());
    }

    fn xor() -> Arc<dyn DataCipher> {
        Arc::new(crate::seal::tests::XOR)
    }

    #[test]
    fn sealed_blobs_round_trip_and_convert_both_ways() {
        let (_dir, blobs) = store();
        let plain_hash = blobs.put(b"written before encryption").unwrap();
        assert!(!blobs.is_sealed(&plain_hash).unwrap());

        // A clone made before installing sees the cipher too.
        let clone = blobs.clone();
        blobs.set_cipher(Some(xor()), SealMode::Seal);
        let content = b"Subject: secret\r\n\r\nThe launch code is 1234".repeat(20);
        let sealed_hash = clone.put(&content).unwrap();
        assert!(blobs.is_sealed(&sealed_hash).unwrap());
        let on_disk = std::fs::read(blobs.path_of(&sealed_hash)).unwrap();
        assert!(on_disk.starts_with(&SEALED_MAGIC));
        assert_eq!(blobs.get(&sealed_hash).unwrap(), content);
        // Plain blobs stay readable.
        assert_eq!(
            blobs.get(&plain_hash).unwrap(),
            b"written before encryption"
        );

        let mut all = blobs.hashes().unwrap();
        all.sort();
        let mut expected = vec![plain_hash.clone(), sealed_hash.clone()];
        expected.sort();
        assert_eq!(all, expected);

        assert!(blobs.convert(&plain_hash, true).unwrap());
        assert!(!blobs.convert(&plain_hash, true).unwrap(), "idempotent");
        assert!(blobs.is_sealed(&plain_hash).unwrap());
        assert_eq!(
            blobs.get(&plain_hash).unwrap(),
            b"written before encryption"
        );

        assert!(blobs.convert(&sealed_hash, false).unwrap());
        assert!(!blobs.is_sealed(&sealed_hash).unwrap());
        assert_eq!(blobs.get(&sealed_hash).unwrap(), content);

        // Without a cipher, sealed blobs are locked, plain ones readable.
        blobs.set_cipher(None, SealMode::Plain);
        assert!(matches!(blobs.get(&plain_hash), Err(Error::Locked(_))));
        assert!(matches!(
            blobs.convert(&plain_hash, false),
            Err(Error::Locked(_))
        ));
        assert_eq!(blobs.get(&sealed_hash).unwrap(), content);
    }

    #[test]
    fn sealed_blob_is_bound_to_its_hash() {
        let (_dir, blobs) = store();
        blobs.set_cipher(Some(xor()), SealMode::Seal);
        let a = blobs.put(b"first").unwrap();
        let b = blobs.put(b"second").unwrap();
        // Swap the files: the context (hash) no longer matches.
        std::fs::copy(blobs.path_of(&a), blobs.path_of(&b)).unwrap();
        assert!(matches!(blobs.get(&b), Err(Error::Crypto(_))));
        assert!(matches!(blobs.convert(&b, false), Err(Error::Crypto(_))));
    }

    #[test]
    fn plain_mode_with_cipher_writes_plain_and_temp_files_are_cleaned() {
        let (_dir, blobs) = store();
        blobs.set_cipher(Some(xor()), SealMode::Plain);
        let hash = blobs.put(b"decrypting: new writes are plain").unwrap();
        assert!(!blobs.is_sealed(&hash).unwrap());
        let dir = blobs.path_of(&hash).parent().unwrap().to_owned();
        std::fs::write(dir.join(".tmpABC"), b"torn").unwrap();
        assert_eq!(blobs.hashes().unwrap(), vec![hash.clone()]);
        let hour = std::time::Duration::from_hours(1);
        assert_eq!(blobs.remove_temp_files(hour).unwrap(), 0, "too young");
        assert_eq!(
            blobs.remove_temp_files(std::time::Duration::ZERO).unwrap(),
            1
        );
        assert_eq!(
            blobs.remove_temp_files(std::time::Duration::ZERO).unwrap(),
            0
        );
        assert!(
            blobs
                .is_sealed(&BlobHash::of(b"none"))
                .unwrap_err()
                .is_not_found()
        );
    }

    #[test]
    fn concurrent_puts_of_same_content_succeed() {
        let (_dir, blobs) = store();
        let content = vec![7u8; 100_000];
        let hashes: Vec<BlobHash> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| blobs.put(&content).unwrap()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(hashes.windows(2).all(|w| w[0] == w[1]));
        assert_eq!(blobs.get(&hashes[0]).unwrap(), content);
    }
}
