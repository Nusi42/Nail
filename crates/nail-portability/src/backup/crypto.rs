//! The encryption layer of `.nailbackup` files:
//!
//! ```text
//! "NAILBAK1" | header length (u32 LE) | header JSON | ciphertext
//! ```
//!
//! The ciphertext is XChaCha20-Poly1305 in the STREAM construction
//! (`aead::stream::StreamBE32`: 19-byte nonce prefix, 32-bit big-endian
//! chunk counter, last-chunk flag) over 64 KiB plaintext chunks, every
//! chunk authenticating the whole prefix (magic, length, header) as
//! associated data. The key is Argon2id(passphrase, salt).
//!
//! Reading is random access ([`DecryptingReader`] implements `Seek`):
//! the zip inside is read through its central directory, and each chunk
//! is authenticated as it is decrypted, so nothing unauthenticated is ever
//! returned and nothing is decrypted to disk.

use std::io::{self, Read, Seek, SeekFrom, Write};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use nail_api::backup::{BACKUP_FORMAT_VERSION, BACKUP_MAGIC};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{PortabilityError, Result};

/// Plaintext bytes per chunk, as the header records it.
const CHUNK_SIZE_U32: u32 = 64 * 1024;

/// Plaintext bytes per chunk.
pub const CHUNK_SIZE: usize = CHUNK_SIZE_U32 as usize;

/// Poly1305 tag bytes per chunk.
pub const TAG_SIZE: usize = 16;

/// STREAM nonce prefix bytes (24 − 4 counter − 1 flag).
pub const NONCE_PREFIX_SIZE: usize = 19;

/// The cipher name in the header.
pub const CIPHER: &str = "xchacha20poly1305-stream-be32";

/// The KDF name in the header.
pub const KDF: &str = "argon2id";

/// Longest header accepted.
const MAX_HEADER_BYTES: u32 = 64 * 1024;

/// Argon2id cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_kib: u32,
    /// Passes.
    pub t: u32,
    /// Lanes.
    pub p: u32,
}

impl KdfParams {
    /// What new backups use: 64 MiB, 3 passes, 1 lane (the contract's
    /// minimum).
    pub const STANDARD: Self = Self {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    };

    /// Cheap parameters for tests only (refused by
    /// [`KdfParams::check_for_writing`] outside tests).
    #[cfg(any(test, feature = "testing"))]
    pub const fn insecure_for_tests() -> Self {
        Self {
            m_kib: 64,
            t: 1,
            p: 1,
        }
    }

    /// Whether a new backup may use these (at least
    /// [`KdfParams::STANDARD`]; any cost in test builds).
    pub fn check_for_writing(&self) -> Result<()> {
        let allowed = cfg!(any(test, feature = "testing"))
            || (self.m_kib >= Self::STANDARD.m_kib && self.t >= Self::STANDARD.t && self.p >= 1);
        if allowed && self.p <= 16 {
            Ok(())
        } else {
            Err(PortabilityError::InvalidOptions(format!(
                "Argon2id parameters below the minimum: {self:?}"
            )))
        }
    }

    /// Whether an archive's parameters are sane to run (a crafted header
    /// must not make Nail allocate gigabytes or spin for hours).
    fn check_for_reading(&self) -> Result<()> {
        let sane = (8..=1024 * 1024).contains(&self.m_kib)
            && (1..=16).contains(&self.t)
            && (1..=16).contains(&self.p)
            && self.m_kib >= 8 * self.p;
        if sane {
            Ok(())
        } else {
            Err(PortabilityError::Corrupt(format!(
                "unsupported Argon2id parameters {self:?}"
            )))
        }
    }
}

/// The header: what a reader needs to derive the key and decrypt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupHeader {
    /// [`BACKUP_FORMAT_VERSION`] of the writer.
    pub format_version: u32,
    /// `argon2id`.
    pub kdf: String,
    /// Its cost.
    pub kdf_params: KdfParams,
    /// The salt (hex, 16 bytes).
    pub salt: String,
    /// [`CIPHER`].
    pub cipher: String,
    /// The STREAM nonce prefix (hex, 19 bytes).
    pub nonce_prefix: String,
    /// Plaintext bytes per chunk.
    pub chunk_size: u32,
}

/// A derived key (wiped when dropped).
pub struct DerivedKey(Zeroizing<[u8; 32]>);

/// Never prints the key.
impl std::fmt::Debug for DerivedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DerivedKey(<redacted>)")
    }
}

impl DerivedKey {
    /// Argon2id(passphrase, salt).
    pub fn derive(passphrase: &str, salt: &[u8], params: KdfParams) -> Result<Self> {
        let params = Params::new(params.m_kib, params.t, params.p, Some(32))
            .map_err(|e| PortabilityError::Corrupt(format!("Argon2id parameters: {e}")))?;
        let mut key = Zeroizing::new([0u8; 32]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
            .map_err(|e| PortabilityError::Corrupt(format!("Argon2id: {e}")))?;
        Ok(Self(key))
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(Key::from_slice(self.0.as_ref()))
    }
}

fn nonce(prefix: &[u8; NONCE_PREFIX_SIZE], counter: u32, last: bool) -> XNonce {
    let mut nonce = [0u8; 24];
    nonce[..NONCE_PREFIX_SIZE].copy_from_slice(prefix);
    nonce[NONCE_PREFIX_SIZE..NONCE_PREFIX_SIZE + 4].copy_from_slice(&counter.to_be_bytes());
    nonce[23] = u8::from(last);
    XNonce::clone_from_slice(&nonce)
}

/// The bytes before the ciphertext (also the associated data).
fn prefix_bytes(header: &BackupHeader) -> Result<Vec<u8>> {
    let header_error = |e: &dyn std::fmt::Display| {
        PortabilityError::io("backup header", io::Error::other(e.to_string()))
    };
    let json = serde_json::to_vec(header).map_err(|e| header_error(&e))?;
    let len = u32::try_from(json.len()).map_err(|e| header_error(&e))?;
    let mut out = Vec::with_capacity(12 + json.len());
    out.extend_from_slice(BACKUP_MAGIC);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&json);
    Ok(out)
}

/// Encrypts into `inner`: writes the prefix at once, then chunks.
pub struct EncryptingWriter<W: Write> {
    inner: W,
    cipher: XChaCha20Poly1305,
    prefix: [u8; NONCE_PREFIX_SIZE],
    aad: Vec<u8>,
    buffer: Zeroizing<Vec<u8>>,
    counter: u32,
    written: u64,
}

/// Never prints the key or plaintext.
impl<W: Write> std::fmt::Debug for EncryptingWriter<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptingWriter")
            .field("chunks", &self.counter)
            .field("written", &self.written)
            .finish_non_exhaustive()
    }
}

impl<W: Write> EncryptingWriter<W> {
    /// Starts an archive: a fresh salt and nonce prefix, the key from the
    /// passphrase.
    pub fn new(mut inner: W, passphrase: &str, params: KdfParams) -> Result<Self> {
        params.check_for_writing()?;
        let mut salt = [0u8; 16];
        let mut prefix = [0u8; NONCE_PREFIX_SIZE];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        rand::rngs::OsRng.fill_bytes(&mut prefix);
        let header = BackupHeader {
            format_version: BACKUP_FORMAT_VERSION,
            kdf: KDF.to_owned(),
            kdf_params: params,
            salt: hex::encode(salt),
            cipher: CIPHER.to_owned(),
            nonce_prefix: hex::encode(prefix),
            chunk_size: CHUNK_SIZE_U32,
        };
        let key = DerivedKey::derive(passphrase, &salt, params)?;
        let aad = prefix_bytes(&header)?;
        inner
            .write_all(&aad)
            .map_err(|e| PortabilityError::io("backup header", e))?;
        Ok(Self {
            inner,
            cipher: key.cipher(),
            prefix,
            written: aad.len() as u64,
            aad,
            buffer: Zeroizing::new(Vec::with_capacity(CHUNK_SIZE + TAG_SIZE)),
            counter: 0,
        })
    }

    fn emit(&mut self, last: bool) -> io::Result<()> {
        let nonce = nonce(&self.prefix, self.counter, last);
        self.cipher
            .encrypt_in_place(&nonce, &self.aad, &mut *self.buffer)
            .map_err(|_| io::Error::other("encryption failed"))?;
        self.inner.write_all(&self.buffer)?;
        self.written += self.buffer.len() as u64;
        self.buffer.clear();
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or_else(|| io::Error::other("the backup is too large"))?;
        Ok(())
    }

    /// Bytes written to the file so far.
    pub fn bytes_written(&self) -> u64 {
        self.written
    }

    /// Writes the last chunk and returns the inner writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.emit(true)?;
        self.inner.flush()?;
        Ok(self.inner)
    }
}

impl<W: Write> Write for EncryptingWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        // A full chunk is only written once more data shows it is not the
        // last one.
        if self.buffer.len() == CHUNK_SIZE {
            self.emit(false)?;
        }
        let take = data.len().min(CHUNK_SIZE - self.buffer.len());
        self.buffer.extend_from_slice(&data[..take]);
        Ok(take)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Reads the header of an archive (no key needed).
pub fn read_header<R: Read>(reader: &mut R) -> Result<(BackupHeader, Vec<u8>)> {
    let mut magic = [0u8; 8];
    reader
        .read_exact(&mut magic)
        .map_err(|_| PortabilityError::NotABackup("too short".into()))?;
    if &magic != BACKUP_MAGIC {
        return Err(PortabilityError::NotABackup("no NAILBAK1 magic".into()));
    }
    let mut len = [0u8; 4];
    reader
        .read_exact(&mut len)
        .map_err(|_| PortabilityError::Corrupt("header cut off".into()))?;
    let len = u32::from_le_bytes(len);
    if len == 0 || len > MAX_HEADER_BYTES {
        return Err(PortabilityError::Corrupt(format!("header of {len} bytes")));
    }
    let mut json = vec![0u8; len as usize];
    reader
        .read_exact(&mut json)
        .map_err(|_| PortabilityError::Corrupt("header cut off".into()))?;
    let header: BackupHeader = serde_json::from_slice(&json)
        .map_err(|e| PortabilityError::Corrupt(format!("header: {e}")))?;
    if header.format_version > BACKUP_FORMAT_VERSION {
        return Err(PortabilityError::NewerFormat(header.format_version));
    }
    if header.kdf != KDF || header.cipher != CIPHER || header.chunk_size as usize != CHUNK_SIZE {
        return Err(PortabilityError::Corrupt(format!(
            "unsupported {} / {} / {}",
            header.kdf, header.cipher, header.chunk_size
        )));
    }
    header.kdf_params.check_for_reading()?;
    let mut aad = Vec::with_capacity(12 + json.len());
    aad.extend_from_slice(&magic);
    aad.extend_from_slice(&len.to_le_bytes());
    aad.extend_from_slice(&json);
    Ok((header, aad))
}

/// Random-access plaintext over an archive's ciphertext.
pub struct DecryptingReader<R: Read + Seek> {
    inner: R,
    cipher: XChaCha20Poly1305,
    prefix: [u8; NONCE_PREFIX_SIZE],
    aad: Vec<u8>,
    data_start: u64,
    chunks: u64,
    plain_len: u64,
    position: u64,
    cached: Option<(u64, Zeroizing<Vec<u8>>)>,
}

/// Never prints the key or plaintext.
impl<R: Read + Seek> std::fmt::Debug for DecryptingReader<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecryptingReader")
            .field("chunks", &self.chunks)
            .field("plain_len", &self.plain_len)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek> DecryptingReader<R> {
    /// Opens an archive: reads the header, derives the key and decrypts
    /// the first chunk (a wrong passphrase fails here with
    /// [`PortabilityError::WrongPassphrase`]).
    pub fn open(mut inner: R, passphrase: &str) -> Result<(Self, BackupHeader)> {
        inner
            .seek(SeekFrom::Start(0))
            .map_err(|e| PortabilityError::io("backup", e))?;
        let (header, aad) = read_header(&mut inner)?;
        let salt =
            hex::decode(&header.salt).map_err(|_| PortabilityError::Corrupt("salt".into()))?;
        let prefix: [u8; NONCE_PREFIX_SIZE] = hex::decode(&header.nonce_prefix)
            .ok()
            .and_then(|p| p.try_into().ok())
            .ok_or_else(|| PortabilityError::Corrupt("nonce prefix".into()))?;
        if salt.len() < 16 {
            return Err(PortabilityError::Corrupt("salt too short".into()));
        }
        let key = DerivedKey::derive(passphrase, &salt, header.kdf_params)?;
        let data_start = aad.len() as u64;
        let end = inner
            .seek(SeekFrom::End(0))
            .map_err(|e| PortabilityError::io("backup", e))?;
        let cipher_len = end.saturating_sub(data_start);
        let full = (CHUNK_SIZE + TAG_SIZE) as u64;
        let chunks = cipher_len.div_ceil(full);
        let last_len = cipher_len - chunks.saturating_sub(1) * full;
        if chunks == 0 || last_len < TAG_SIZE as u64 {
            return Err(PortabilityError::Corrupt("no data".into()));
        }
        let plain_len = cipher_len - chunks * TAG_SIZE as u64;
        let mut reader = Self {
            inner,
            cipher: key.cipher(),
            prefix,
            aad,
            data_start,
            chunks,
            plain_len,
            position: 0,
            cached: None,
        };
        reader
            .load(0)
            .map_err(|_| PortabilityError::WrongPassphrase)?;
        Ok((reader, header))
    }

    /// Plaintext length.
    pub fn len(&self) -> u64 {
        self.plain_len
    }

    /// Whether there is no plaintext.
    pub fn is_empty(&self) -> bool {
        self.plain_len == 0
    }

    /// The plaintext of chunk `index` (decrypted once, then cached).
    fn load(&mut self, index: u64) -> io::Result<&[u8]> {
        if self.cached.as_ref().is_none_or(|(i, _)| *i != index) {
            let buffer = self.decrypt_chunk(index)?;
            self.cached = Some((index, buffer));
        }
        Ok(self
            .cached
            .as_ref()
            .map_or(&[][..], |(_, chunk)| chunk.as_slice()))
    }

    fn decrypt_chunk(&mut self, index: u64) -> io::Result<Zeroizing<Vec<u8>>> {
        let full = (CHUNK_SIZE + TAG_SIZE) as u64;
        let start = self.data_start + index * full;
        let len = if index + 1 == self.chunks {
            self.data_start + self.plain_len + self.chunks * TAG_SIZE as u64 - start
        } else {
            full
        };
        self.inner.seek(SeekFrom::Start(start))?;
        let len = usize::try_from(len).map_err(|_| io::Error::other("backup chunk too large"))?;
        let mut buffer = Zeroizing::new(vec![0u8; len]);
        self.inner.read_exact(&mut buffer)?;
        let counter =
            u32::try_from(index).map_err(|_| io::Error::other("chunk counter overflow"))?;
        let nonce = nonce(&self.prefix, counter, index + 1 == self.chunks);
        self.cipher
            .decrypt_in_place(&nonce, &self.aad, &mut *buffer)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "backup chunk failed authentication",
                )
            })?;
        Ok(buffer)
    }
}

impl<R: Read + Seek> Read for DecryptingReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.plain_len || out.is_empty() {
            return Ok(0);
        }
        let index = self.position / CHUNK_SIZE as u64;
        let offset = usize::try_from(self.position % CHUNK_SIZE as u64)
            .map_err(|_| io::Error::other("backup position out of range"))?;
        let chunk = self.load(index)?;
        let available = chunk.len().saturating_sub(offset);
        let n = available.min(out.len());
        out[..n].copy_from_slice(&chunk[offset..offset + n]);
        self.position += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for DecryptingReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(p) => i128::from(p),
            SeekFrom::End(d) => i128::from(self.plain_len) + i128::from(d),
            SeekFrom::Current(d) => i128::from(self.position) + i128::from(d),
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.position = u64::try_from(target).unwrap_or(u64::MAX);
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn encrypt(data: &[u8], passphrase: &str) -> Vec<u8> {
        let mut writer =
            EncryptingWriter::new(Vec::new(), passphrase, KdfParams::insecure_for_tests()).unwrap();
        writer.write_all(data).unwrap();
        writer.finish().unwrap()
    }

    fn decrypt(bytes: Vec<u8>, passphrase: &str) -> Result<Vec<u8>> {
        let (mut reader, _) = DecryptingReader::open(Cursor::new(bytes), passphrase)?;
        let mut out = Vec::new();
        reader
            .read_to_end(&mut out)
            .map_err(|e| PortabilityError::Corrupt(e.to_string()))?;
        Ok(out)
    }

    #[test]
    fn round_trips_every_chunk_boundary() {
        for len in [
            0,
            1,
            CHUNK_SIZE - 1,
            CHUNK_SIZE,
            CHUNK_SIZE + 1,
            3 * CHUNK_SIZE + 17,
        ] {
            let data: Vec<u8> = (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect();
            let bytes = encrypt(&data, "correct horse battery");
            assert!(bytes.starts_with(BACKUP_MAGIC));
            assert_eq!(
                decrypt(bytes, "correct horse battery").unwrap(),
                data,
                "length {len}"
            );
        }
    }

    #[test]
    fn wrong_passphrases_fail_on_the_first_chunk() {
        let bytes = encrypt(b"secret", "correct horse battery");
        assert!(matches!(
            decrypt(bytes, "wrong horse battery"),
            Err(PortabilityError::WrongPassphrase)
        ));
    }

    #[test]
    fn tampering_is_detected() {
        let data = vec![7u8; 3 * CHUNK_SIZE];
        let clean = encrypt(&data, "pass phrase 123");
        // A flipped bit in the second chunk.
        let mut flipped = clean.clone();
        let at = flipped.len() - CHUNK_SIZE - 40;
        flipped[at] ^= 1;
        assert!(decrypt(flipped, "pass phrase 123").is_err());
        // A truncated archive: the last chunk is gone (the new last one
        // was not written as last).
        let truncated = clean[..clean.len() - TAG_SIZE].to_vec();
        assert!(decrypt(truncated, "pass phrase 123").is_err());
        let cut = clean[..clean.len() - (TAG_SIZE + 10)].to_vec();
        assert!(decrypt(cut, "pass phrase 123").is_err());
        // The header is authenticated: a changed KDF field breaks it.
        let mut header_changed = clean.clone();
        let text = String::from_utf8_lossy(&header_changed[12..200]).into_owned();
        let at = 12 + text.find("\"t\":1").unwrap() + 4;
        header_changed[at] = b'2';
        assert!(decrypt(header_changed, "pass phrase 123").is_err());
    }

    #[test]
    fn matches_the_aead_stream_construction() {
        use chacha20poly1305::aead::stream::EncryptorBE32;
        let mut writer =
            EncryptingWriter::new(Vec::new(), "x", KdfParams::insecure_for_tests()).unwrap();
        let data = vec![1u8; CHUNK_SIZE + 5];
        writer.write_all(&data).unwrap();
        let prefix = writer.prefix;
        let aad = writer.aad.clone();
        let cipher = writer.cipher.clone();
        let bytes = writer.finish().unwrap();
        let mut expected = aad.clone();
        let mut stream = EncryptorBE32::from_aead(
            cipher,
            chacha20poly1305::aead::generic_array::GenericArray::from_slice(&prefix),
        );
        let first = stream
            .encrypt_next(chacha20poly1305::aead::Payload {
                msg: &data[..CHUNK_SIZE],
                aad: &aad,
            })
            .unwrap();
        expected.extend_from_slice(&first);
        let last = stream
            .encrypt_last(chacha20poly1305::aead::Payload {
                msg: &data[CHUNK_SIZE..],
                aad: &aad,
            })
            .unwrap();
        expected.extend_from_slice(&last);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn seeking() {
        let data: Vec<u8> = (0..(2 * CHUNK_SIZE + 100))
            .map(|i| u8::try_from(i % 256).unwrap())
            .collect();
        let bytes = encrypt(&data, "seek seek seek");
        let (mut reader, header) =
            DecryptingReader::open(Cursor::new(bytes), "seek seek seek").unwrap();
        assert_eq!(header.format_version, BACKUP_FORMAT_VERSION);
        assert_eq!(reader.len(), data.len() as u64);
        reader.seek(SeekFrom::Start(CHUNK_SIZE as u64 - 2)).unwrap();
        let mut four = [0u8; 4];
        reader.read_exact(&mut four).unwrap();
        assert_eq!(&four, &data[CHUNK_SIZE - 2..CHUNK_SIZE + 2]);
        reader.seek(SeekFrom::End(-3)).unwrap();
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, &data[data.len() - 3..]);
        assert!(reader.seek(SeekFrom::Current(-1_000_000)).is_err());
    }

    #[test]
    fn headers_are_checked() {
        assert!(matches!(
            read_header(&mut Cursor::new(b"PK\x03\x04 not a backup")),
            Err(PortabilityError::NotABackup(_))
        ));
        let mut newer = encrypt(b"x", "p");
        let text = String::from_utf8_lossy(&newer[12..60]).into_owned();
        let at = 12 + text.find("\"format_version\":1").unwrap() + 17;
        newer[at] = b'9';
        assert!(matches!(
            read_header(&mut Cursor::new(newer)),
            Err(PortabilityError::NewerFormat(9))
        ));
        let huge = KdfParams {
            m_kib: 4 * 1024 * 1024,
            t: 3,
            p: 1,
        };
        assert!(huge.check_for_reading().is_err());
        assert!(KdfParams::STANDARD.check_for_reading().is_ok());
        assert!(KdfParams::STANDARD.check_for_writing().is_ok());
    }

    #[test]
    fn standard_cost_derives_a_key() {
        // The real parameters (64 MiB, 3 passes) work.
        let key = DerivedKey::derive(
            "correct horse battery staple",
            &[1u8; 16],
            KdfParams::STANDARD,
        )
        .unwrap();
        let again = DerivedKey::derive(
            "correct horse battery staple",
            &[1u8; 16],
            KdfParams::STANDARD,
        )
        .unwrap();
        assert_eq!(*key.0, *again.0);
    }
}
