//! An encrypting tantivy [`Directory`] for the search index.
//!
//! [`SealedDirectory`] wraps another directory (on disk: `MmapDirectory`)
//! and stores every file in the sealed format ([`crate::format`]), bound to
//! its file name:
//!
//! * `open_write` streams: plaintext is sealed chunk by chunk as tantivy
//!   writes, the final chunk on `terminate`;
//! * `get_file_handle` reads random ranges by decrypting only the chunks a
//!   range touches, with a small per-file cache of decrypted chunks;
//! * `atomic_write` / `atomic_read` (`meta.json`, `.managed.json`) seal
//!   whole files;
//! * locks and change notifications are delegated unchanged (lock files
//!   carry no content).
//!
//! During a migration the directory can read plain files too
//! ([`SealedDirectory::accept_plain`]) and can write plain files
//! ([`SealedDirectory::seal_writes`] off, while decrypting). Once
//! encrypted, plain files are refused, so a planted plain segment is not
//! read.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tantivy::HasLen;
use tantivy::directory::error::{DeleteError, LockError, OpenReadError, OpenWriteError};
use tantivy::directory::{
    AntiCallToken, Directory, DirectoryLock, FileHandle, Lock, OwnedBytes, TerminatingWrite,
    WatchCallback, WatchHandle, WritePtr,
};

use crate::format::{self, DEFAULT_CHUNK_LOG2, DataKey, HEADER_LEN, SealedLayout, SealingEncoder};

/// Decrypted chunks kept per open file.
const CACHE_CHUNKS: usize = 8;

fn context_of(path: &Path) -> Vec<u8> {
    format!("nail/index/{}", path.to_string_lossy().replace('\\', "/")).into_bytes()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// An encrypting wrapper around a tantivy directory.
#[derive(Clone)]
pub struct SealedDirectory {
    inner: Box<dyn Directory>,
    key: DataKey,
    seal_writes: bool,
    accept_plain: bool,
    chunk_log2: u8,
}

impl std::fmt::Debug for SealedDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealedDirectory")
            .field("inner", &self.inner)
            .field("seal_writes", &self.seal_writes)
            .field("accept_plain", &self.accept_plain)
            .finish_non_exhaustive()
    }
}

impl SealedDirectory {
    /// Wraps `inner`: writes sealed, reads only sealed files.
    pub fn new(inner: Box<dyn Directory>, key: DataKey) -> Self {
        Self {
            inner,
            key,
            seal_writes: true,
            accept_plain: false,
            chunk_log2: DEFAULT_CHUNK_LOG2,
        }
    }

    /// Whether new files are sealed (off while decrypting).
    #[must_use]
    pub fn seal_writes(mut self, seal: bool) -> Self {
        self.seal_writes = seal;
        self
    }

    /// Whether plain files are read (only during a migration).
    #[must_use]
    pub fn accept_plain(mut self, accept: bool) -> Self {
        self.accept_plain = accept;
        self
    }

    /// The chunk size exponent for new files (tests use small chunks).
    #[must_use]
    pub fn chunk_log2(mut self, log2: u8) -> Self {
        self.chunk_log2 = log2;
        self
    }

    fn plain_refused(path: &Path) -> io::Error {
        invalid(format!(
            "index file {} is not encrypted",
            path.to_string_lossy()
        ))
    }
}

impl Directory for SealedDirectory {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        let handle = self.inner.get_file_handle(path)?;
        let len = handle.len();
        let head = if len >= HEADER_LEN {
            handle
                .read_bytes(0..HEADER_LEN)
                .map_err(|e| OpenReadError::wrap_io_error(e, path.to_owned()))?
                .as_slice()
                .to_vec()
        } else {
            Vec::new()
        };
        if !format::is_sealed(&head) {
            if self.accept_plain {
                return Ok(handle);
            }
            return Err(OpenReadError::wrap_io_error(
                Self::plain_refused(path),
                path.to_owned(),
            ));
        }
        let layout = SealedLayout::parse(&head, len)
            .map_err(|e| OpenReadError::wrap_io_error(invalid(e.to_string()), path.to_owned()))?;
        Ok(Arc::new(SealedFileHandle {
            inner: handle,
            layout,
            key: self.key.clone(),
            context: context_of(path),
            path: path.to_owned(),
            cache: Mutex::new(VecDeque::new()),
        }))
    }

    fn delete(&self, path: &Path) -> Result<(), DeleteError> {
        self.inner.delete(path)
    }

    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        self.inner.exists(path)
    }

    fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        let out = self.inner.open_write(path)?;
        if !self.seal_writes {
            return Ok(out);
        }
        let writer = SealingWriter {
            out,
            encoder: Some(SealingEncoder::new(
                &self.key,
                &context_of(path),
                self.chunk_log2,
            )),
        };
        Ok(io::BufWriter::new(Box::new(writer)))
    }

    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        let data = self.inner.atomic_read(path)?;
        if format::is_sealed(&data) {
            format::open(&self.key, &context_of(path), &data)
                .map_err(|e| OpenReadError::wrap_io_error(invalid(e.to_string()), path.to_owned()))
        } else if self.accept_plain {
            Ok(data)
        } else {
            Err(OpenReadError::wrap_io_error(
                Self::plain_refused(path),
                path.to_owned(),
            ))
        }
    }

    fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        if self.seal_writes {
            let sealed =
                format::seal_with_chunk(&self.key, &context_of(path), data, self.chunk_log2)
                    .map_err(|e| invalid(e.to_string()))?;
            self.inner.atomic_write(path, &sealed)
        } else {
            self.inner.atomic_write(path, data)
        }
    }

    fn sync_directory(&self) -> io::Result<()> {
        self.inner.sync_directory()
    }

    fn acquire_lock(&self, lock: &Lock) -> Result<DirectoryLock, LockError> {
        self.inner.acquire_lock(lock)
    }

    fn watch(&self, watch_callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        self.inner.watch(watch_callback)
    }
}

/// Seals what tantivy writes.
struct SealingWriter {
    out: WritePtr,
    /// `None` after `terminate`.
    encoder: Option<SealingEncoder>,
}

impl Write for SealingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let encoder = self
            .encoder
            .as_mut()
            .ok_or_else(|| io::Error::other("write after terminate"))?;
        let sealed = encoder.push(buf).map_err(|e| invalid(e.to_string()))?;
        self.out.write_all(&sealed)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // Complete chunks are already written; the final one only exists
        // after `terminate`.
        self.out.flush()
    }
}

impl TerminatingWrite for SealingWriter {
    fn terminate_ref(&mut self, token: AntiCallToken) -> io::Result<()> {
        if let Some(encoder) = self.encoder.take() {
            let last = encoder.finish().map_err(|e| invalid(e.to_string()))?;
            self.out.write_all(&last)?;
        }
        self.out.terminate_ref(token)
    }
}

/// Random-access reads of a sealed file.
struct SealedFileHandle {
    inner: Arc<dyn FileHandle>,
    layout: SealedLayout,
    key: DataKey,
    context: Vec<u8>,
    path: PathBuf,
    cache: Mutex<VecDeque<(usize, Arc<Vec<u8>>)>>,
}

impl std::fmt::Debug for SealedFileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealedFileHandle")
            .field("path", &self.path)
            .field("len", &self.layout.plain_len())
            .finish_non_exhaustive()
    }
}

impl SealedFileHandle {
    fn chunk(&self, index: usize) -> io::Result<Arc<Vec<u8>>> {
        {
            let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((_, chunk)) = cache.iter().find(|(i, _)| *i == index) {
                return Ok(Arc::clone(chunk));
            }
        }
        let sealed = self.inner.read_bytes(self.layout.sealed_range(index))?;
        let plain = self
            .layout
            .open_chunk(&self.key, &self.context, index, sealed.as_slice())
            .map_err(|e| invalid(format!("index file {}: {e}", self.path.to_string_lossy())))?;
        let plain = Arc::new(plain);
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= CACHE_CHUNKS {
            cache.pop_front();
        }
        cache.push_back((index, Arc::clone(&plain)));
        Ok(plain)
    }
}

impl HasLen for SealedFileHandle {
    fn len(&self) -> usize {
        self.layout.plain_len()
    }
}

impl FileHandle for SealedFileHandle {
    fn read_bytes(&self, range: Range<usize>) -> io::Result<OwnedBytes> {
        if range.end > self.layout.plain_len() || range.start > range.end {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "range {range:?} outside index file {} ({} bytes)",
                    self.path.to_string_lossy(),
                    self.layout.plain_len()
                ),
            ));
        }
        if range.is_empty() {
            return Ok(OwnedBytes::empty());
        }
        let size = self.layout.chunk_size();
        let first = range.start / size;
        let last = (range.end - 1) / size;
        let mut out = Vec::with_capacity(range.len());
        for index in first..=last {
            let chunk = self.chunk(index)?;
            let chunk_start = index * size;
            let from = range.start.max(chunk_start) - chunk_start;
            let to = range.end.min(chunk_start + chunk.len()) - chunk_start;
            out.extend_from_slice(&chunk[from..to]);
        }
        Ok(OwnedBytes::new(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::directory::RamDirectory;

    fn key() -> DataKey {
        DataKey::from_bytes([3; 32])
    }

    fn write(dir: &dyn Directory, path: &str, pieces: &[&[u8]]) {
        let mut w = dir.open_write(Path::new(path)).unwrap();
        for piece in pieces {
            w.write_all(piece).unwrap();
        }
        w.terminate().unwrap();
    }

    #[test]
    fn streamed_files_read_back_at_any_range() {
        let ram = RamDirectory::create();
        let dir = SealedDirectory::new(Box::new(ram.clone()), key()).chunk_log2(10);
        let content: Vec<u8> = (0..5000u32).map(|i| (i % 253) as u8).collect();
        write(
            &dir,
            "seg.idx",
            &[&content[..100], &content[100..3000], &content[3000..]],
        );

        // On the inner directory it is sealed and longer.
        let raw = ram.open_read(Path::new("seg.idx")).unwrap();
        let raw = raw.read_bytes().unwrap();
        assert!(format::is_sealed(raw.as_slice()));
        assert!(raw.len() > content.len());

        let slice = dir.open_read(Path::new("seg.idx")).unwrap();
        assert_eq!(slice.len(), 5000);
        assert_eq!(slice.read_bytes().unwrap().as_slice(), &content[..]);
        for range in [0..0, 0..1, 1023..1025, 1024..2048, 4999..5000, 17..4321] {
            assert_eq!(
                slice.read_bytes_slice(range.clone()).unwrap().as_slice(),
                &content[range.clone()],
                "{range:?}"
            );
        }
        let handle = dir.get_file_handle(Path::new("seg.idx")).unwrap();
        assert!(handle.read_bytes(4000..5001).is_err());
    }

    #[test]
    fn atomic_files_are_sealed_and_bound_to_their_name() {
        let ram = RamDirectory::create();
        let dir = SealedDirectory::new(Box::new(ram.clone()), key());
        dir.atomic_write(Path::new("meta.json"), b"{\"segments\":[]}")
            .unwrap();
        assert!(format::is_sealed(
            &ram.atomic_read(Path::new("meta.json")).unwrap()
        ));
        assert_eq!(
            dir.atomic_read(Path::new("meta.json")).unwrap(),
            b"{\"segments\":[]}"
        );
        // Copy the sealed bytes under another name: refused.
        let sealed = ram.atomic_read(Path::new("meta.json")).unwrap();
        ram.atomic_write(Path::new(".managed.json"), &sealed)
            .unwrap();
        assert!(dir.atomic_read(Path::new(".managed.json")).is_err());
        write(&dir, "a.pos", &[b"segment a"]);
        let a = ram
            .open_read(Path::new("a.pos"))
            .unwrap()
            .read_bytes()
            .unwrap();
        write(&ram, "b.pos", &[a.as_slice()]);
        let b = dir.open_read(Path::new("b.pos")).unwrap();
        assert!(b.read_bytes().is_err());
    }

    #[test]
    fn plain_files_only_during_migration_and_plain_writes_while_decrypting() {
        let ram = RamDirectory::create();
        write(&ram, "old.idx", &[b"plain segment"]);
        ram.atomic_write(Path::new("meta.json"), b"{}").unwrap();

        let strict = SealedDirectory::new(Box::new(ram.clone()), key());
        assert!(strict.open_read(Path::new("old.idx")).is_err());
        assert!(strict.atomic_read(Path::new("meta.json")).is_err());

        let migrating = strict.clone().accept_plain(true);
        assert_eq!(
            migrating
                .open_read(Path::new("old.idx"))
                .unwrap()
                .read_bytes()
                .unwrap()
                .as_slice(),
            b"plain segment"
        );
        assert_eq!(
            migrating.atomic_read(Path::new("meta.json")).unwrap(),
            b"{}"
        );

        let decrypting = migrating.seal_writes(false);
        write(&decrypting, "new.idx", &[b"written plain"]);
        let raw = ram
            .open_read(Path::new("new.idx"))
            .unwrap()
            .read_bytes()
            .unwrap();
        assert_eq!(raw.as_slice(), b"written plain");
    }

    #[test]
    fn tampered_chunk_fails_the_read() {
        let ram = RamDirectory::create();
        let dir = SealedDirectory::new(Box::new(ram.clone()), key()).chunk_log2(10);
        write(&dir, "x.store", &[&[9u8; 3000]]);
        let mut raw = ram
            .open_read(Path::new("x.store"))
            .unwrap()
            .read_bytes()
            .unwrap()
            .as_slice()
            .to_vec();
        raw[HEADER_LEN + 1500] ^= 0x80;
        ram.delete(Path::new("x.store")).unwrap();
        write(&ram, "x.store", &[&raw]);
        let slice = dir.open_read(Path::new("x.store")).unwrap();
        // The first chunk is intact, the second is not.
        assert!(slice.read_bytes_slice(0..100).is_ok());
        assert!(slice.read_bytes_slice(1100..1200).is_err());
    }
}
