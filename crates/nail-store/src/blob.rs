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
//! The API is synchronous (file I/O); async callers run it on
//! `tokio::task::spawn_blocking`.

use std::fmt;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

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
}

impl BlobStore {
    /// Opens (and creates, if needed) a blob store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            level: zstd::DEFAULT_COMPRESSION_LEVEL,
        })
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
    pub fn get(&self, hash: &BlobHash) -> Result<Vec<u8>> {
        let compressed = match std::fs::read(self.path_of(hash)) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return Err(Error::not_found("blob", hash));
            }
            Err(err) => return Err(err.into()),
        };
        let bytes = zstd::decode_all(compressed.as_slice())
            .map_err(|_| Error::BlobCorrupt(hash.clone()))?;
        if BlobHash::of(&bytes) != *hash {
            return Err(Error::BlobCorrupt(hash.clone()));
        }
        Ok(bytes)
    }

    /// Deletes a blob. Returns whether it existed. The caller is responsible
    /// for making sure no message references it any more.
    pub fn remove(&self, hash: &BlobHash) -> Result<bool> {
        match std::fs::remove_file(self.path_of(hash)) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
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
