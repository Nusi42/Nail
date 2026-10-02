//! The sealed-data format: chunked XChaCha20-Poly1305 (version 1).
//!
//! Everything the vault encrypts — blob files, search-index files, sealed
//! database values and the key check value — uses this one format, so
//! there is one parser to review.
//!
//! ```text
//! offset  size  field
//! 0       4     magic "NLV\x01" (nail_store::SEALED_MAGIC)
//! 4       1     algorithm: 1 = XChaCha20-Poly1305, chunked
//! 5       1     log2 of the chunk size (10..=20; 15 = 32 KiB by default)
//! 6       2     reserved, zero
//! 8       16    random nonce prefix (per file / value)
//! 24      ...   chunks: ciphertext of up to `chunk size` bytes + 16-byte tag
//! ```
//!
//! * Chunk `i` uses the 24-byte nonce `prefix ‖ i` (u64 big-endian). The
//!   random prefix makes nonce reuse across files negligible (2^-64 per
//!   pair at 2^32 files); the counter makes it impossible within a file.
//! * Associated data of every chunk: the 24-byte header, the length of the
//!   context (u64 LE), the context, and a final-chunk flag. The context
//!   binds data to where it belongs (a blob hash, an index file name, a
//!   column), the header binds the chunk size, and the flag detects
//!   truncation at a chunk boundary.
//! * All chunks but the last hold exactly `chunk size` bytes; the last
//!   holds 0..=`chunk size` bytes. So the plaintext length follows from the
//!   file length, and any byte range can be read by decrypting only the
//!   chunks it touches ([`SealedLayout`]).

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::error::VaultError;

/// The magic every sealed item starts with.
pub const MAGIC: [u8; 4] = nail_store::SEALED_MAGIC;
/// Header length.
pub const HEADER_LEN: usize = 24;
/// Authentication tag length per chunk.
pub const TAG_LEN: usize = 16;
/// Algorithm id of version 1.
const ALGORITHM_XCHACHA_CHUNKED: u8 = 1;
/// Default chunk size exponent (32 KiB).
pub const DEFAULT_CHUNK_LOG2: u8 = 15;
const MIN_CHUNK_LOG2: u8 = 10;
const MAX_CHUNK_LOG2: u8 = 20;

/// A 256-bit data key. Zeroed when dropped; never printed.
#[derive(Clone)]
pub struct DataKey(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DataKey(..)")
    }
}

impl DataKey {
    /// A fresh random key from the OS generator.
    pub fn generate() -> Self {
        let mut bytes = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(bytes.as_mut());
        Self(bytes)
    }

    /// A key from its 32 bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// The key as lowercase hex (for the keychain). The caller must not
    /// keep the string longer than needed; it is zeroed on drop.
    pub fn to_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(hex::encode(self.0.as_ref()))
    }

    /// Parses [`DataKey::to_hex`].
    pub fn from_hex(text: &str) -> Result<Self, VaultError> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        hex::decode_to_slice(text.trim(), bytes.as_mut())
            .map_err(|_| VaultError::Crypto("the stored data key is malformed".into()))?;
        Ok(Self(bytes))
    }

    /// The raw key bytes (for key derivation inside this crate).
    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(Key::from_slice(self.0.as_ref()))
    }
}

/// The header fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    chunk_log2: u8,
    prefix: [u8; 16],
}

impl Header {
    fn new(chunk_log2: u8) -> Self {
        let mut prefix = [0u8; 16];
        OsRng.fill_bytes(&mut prefix);
        Self { chunk_log2, prefix }
    }

    fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[..4].copy_from_slice(&MAGIC);
        out[4] = ALGORITHM_XCHACHA_CHUNKED;
        out[5] = self.chunk_log2;
        out[8..24].copy_from_slice(&self.prefix);
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self, VaultError> {
        if bytes.len() < HEADER_LEN || bytes[..4] != MAGIC {
            return Err(VaultError::Crypto("not sealed data".into()));
        }
        if bytes[4] != ALGORITHM_XCHACHA_CHUNKED || bytes[6] != 0 || bytes[7] != 0 {
            return Err(VaultError::Crypto(format!(
                "unsupported sealed-data format {}",
                bytes[4]
            )));
        }
        let chunk_log2 = bytes[5];
        if !(MIN_CHUNK_LOG2..=MAX_CHUNK_LOG2).contains(&chunk_log2) {
            return Err(VaultError::Crypto("invalid chunk size".into()));
        }
        let mut prefix = [0u8; 16];
        prefix.copy_from_slice(&bytes[8..24]);
        Ok(Self { chunk_log2, prefix })
    }

    fn chunk_size(&self) -> usize {
        1usize << self.chunk_log2
    }

    fn nonce(&self, index: u64) -> XNonce {
        let mut nonce = [0u8; 24];
        nonce[..16].copy_from_slice(&self.prefix);
        nonce[16..].copy_from_slice(&index.to_be_bytes());
        XNonce::from(nonce)
    }

    fn aad(&self, context: &[u8], last: bool) -> Vec<u8> {
        let mut aad = Vec::with_capacity(HEADER_LEN + 9 + context.len());
        aad.extend_from_slice(&self.encode());
        aad.extend_from_slice(&(context.len() as u64).to_le_bytes());
        aad.extend_from_slice(context);
        aad.push(u8::from(last));
        aad
    }
}

/// Encrypts chunk by chunk into any writer ([`SealingEncoder::push`] then
/// [`SealingEncoder::finish`]). Keeps at most one chunk in memory.
pub struct SealingEncoder {
    cipher: XChaCha20Poly1305,
    header: Header,
    context: Vec<u8>,
    buffer: Zeroizing<Vec<u8>>,
    index: u64,
    header_written: bool,
}

impl std::fmt::Debug for SealingEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealingEncoder")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl SealingEncoder {
    /// A new encoder with a fresh random nonce prefix.
    pub fn new(key: &DataKey, context: &[u8], chunk_log2: u8) -> Self {
        let chunk_log2 = chunk_log2.clamp(MIN_CHUNK_LOG2, MAX_CHUNK_LOG2);
        Self {
            cipher: key.cipher(),
            header: Header::new(chunk_log2),
            context: context.to_vec(),
            buffer: Zeroizing::new(Vec::new()),
            index: 0,
            header_written: false,
        }
    }

    fn seal_chunk(
        &mut self,
        plain: &[u8],
        last: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), VaultError> {
        let aad = self.header.aad(&self.context, last);
        let sealed = self
            .cipher
            .encrypt(
                &self.header.nonce(self.index),
                Payload {
                    msg: plain,
                    aad: &aad,
                },
            )
            .map_err(|_| VaultError::Crypto("encryption failed".into()))?;
        self.index += 1;
        out.extend_from_slice(&sealed);
        Ok(())
    }

    fn take_header(&mut self, out: &mut Vec<u8>) {
        if !self.header_written {
            out.extend_from_slice(&self.header.encode());
            self.header_written = true;
        }
    }

    /// Adds plaintext; returns sealed bytes that are ready to be written
    /// (the header and every complete chunk that is certainly not the
    /// last one).
    pub fn push(&mut self, plain: &[u8]) -> Result<Vec<u8>, VaultError> {
        let mut out = Vec::new();
        self.take_header(&mut out);
        self.buffer.extend_from_slice(plain);
        let size = self.header.chunk_size();
        // Keep the last (possibly full) chunk back: only `finish` knows it
        // is the last one.
        while self.buffer.len() > size {
            let rest = Zeroizing::new(self.buffer.split_off(size));
            let chunk = std::mem::replace(&mut self.buffer, rest);
            self.seal_chunk(&chunk, false, &mut out)?;
        }
        Ok(out)
    }

    /// Seals the final chunk; returns the remaining bytes to write.
    pub fn finish(mut self) -> Result<Vec<u8>, VaultError> {
        let mut out = Vec::new();
        self.take_header(&mut out);
        let last = std::mem::take(&mut *self.buffer);
        let last = Zeroizing::new(last);
        self.seal_chunk(&last, true, &mut out)?;
        Ok(out)
    }
}

/// Seals a whole buffer.
pub fn seal(key: &DataKey, context: &[u8], plain: &[u8]) -> Result<Vec<u8>, VaultError> {
    seal_with_chunk(key, context, plain, DEFAULT_CHUNK_LOG2)
}

/// Seals a whole buffer with a chosen chunk size (tests use small chunks).
pub fn seal_with_chunk(
    key: &DataKey,
    context: &[u8],
    plain: &[u8],
    chunk_log2: u8,
) -> Result<Vec<u8>, VaultError> {
    let mut encoder = SealingEncoder::new(key, context, chunk_log2);
    let mut out = encoder.push(plain)?;
    out.extend(encoder.finish()?);
    Ok(out)
}

/// Where the chunks of a sealed item are, computed from its total length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SealedLayout {
    header: Header,
    /// Total sealed length.
    sealed_len: usize,
    /// Number of chunks (at least one).
    chunks: usize,
    /// Plaintext length.
    plain_len: usize,
}

impl SealedLayout {
    /// Parses the header and checks that `sealed_len` is a possible length.
    pub fn parse(header: &[u8], sealed_len: usize) -> Result<Self, VaultError> {
        let header = Header::decode(header)?;
        let body = sealed_len
            .checked_sub(HEADER_LEN)
            .filter(|b| *b >= TAG_LEN)
            .ok_or_else(|| VaultError::Crypto("sealed data is truncated".into()))?;
        let full = header.chunk_size() + TAG_LEN;
        let chunks = body.div_ceil(full);
        let last = body - (chunks - 1) * full;
        if last < TAG_LEN {
            return Err(VaultError::Crypto("sealed data is truncated".into()));
        }
        Ok(Self {
            header,
            sealed_len,
            chunks,
            plain_len: body - chunks * TAG_LEN,
        })
    }

    /// Plaintext length.
    pub fn plain_len(&self) -> usize {
        self.plain_len
    }

    /// Number of chunks.
    pub fn chunks(&self) -> usize {
        self.chunks
    }

    /// Plaintext bytes per (non-final) chunk.
    pub fn chunk_size(&self) -> usize {
        self.header.chunk_size()
    }

    /// The sealed byte range of chunk `index`.
    pub fn sealed_range(&self, index: usize) -> std::ops::Range<usize> {
        let full = self.header.chunk_size() + TAG_LEN;
        let start = HEADER_LEN + index * full;
        start..(start + full).min(self.sealed_len)
    }

    /// Decrypts chunk `index` from its sealed bytes.
    pub fn open_chunk(
        &self,
        key: &DataKey,
        context: &[u8],
        index: usize,
        sealed: &[u8],
    ) -> Result<Vec<u8>, VaultError> {
        let last = index + 1 == self.chunks;
        let aad = self.header.aad(context, last);
        key.cipher()
            .decrypt(
                &self.header.nonce(index as u64),
                Payload {
                    msg: sealed,
                    aad: &aad,
                },
            )
            .map_err(|_| {
                VaultError::Crypto(
                    "sealed data failed authentication (damaged, truncated, another key or \
                     another place)"
                        .into(),
                )
            })
    }
}

/// Opens a whole sealed buffer.
pub fn open(key: &DataKey, context: &[u8], sealed: &[u8]) -> Result<Vec<u8>, VaultError> {
    let layout = SealedLayout::parse(sealed, sealed.len())?;
    let mut out = Vec::with_capacity(layout.plain_len());
    for index in 0..layout.chunks() {
        let chunk = layout.open_chunk(key, context, index, &sealed[layout.sealed_range(index)])?;
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Whether bytes start like sealed data.
pub fn is_sealed(bytes: &[u8]) -> bool {
    bytes.starts_with(&MAGIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> DataKey {
        DataKey::from_bytes([7; 32])
    }

    #[test]
    fn round_trips_all_sizes_around_chunk_boundaries() {
        let key = key();
        for len in [0usize, 1, 1023, 1024, 1025, 2048, 2049, 5000] {
            let plain: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let sealed = seal_with_chunk(&key, b"ctx", &plain, 10).unwrap();
            assert!(is_sealed(&sealed));
            let layout = SealedLayout::parse(&sealed, sealed.len()).unwrap();
            assert_eq!(layout.plain_len(), len, "len {len}");
            assert_eq!(open(&key, b"ctx", &sealed).unwrap(), plain, "len {len}");
        }
    }

    #[test]
    fn streaming_push_matches_layout() {
        let key = key();
        let mut encoder = SealingEncoder::new(&key, b"file", 10);
        let mut sealed = Vec::new();
        for piece in [&[1u8; 700][..], &[2u8; 700], &[3u8; 648]] {
            sealed.extend(encoder.push(piece).unwrap());
        }
        sealed.extend(encoder.finish().unwrap());
        let layout = SealedLayout::parse(&sealed, sealed.len()).unwrap();
        // 2048 bytes = exactly two full chunks, the second one final.
        assert_eq!((layout.chunks(), layout.plain_len()), (2, 2048));
        let plain = open(&key, b"file", &sealed).unwrap();
        assert_eq!(&plain[..700], &[1u8; 700][..]);
        assert_eq!(&plain[1400..], &[3u8; 648][..]);
    }

    #[test]
    fn nonces_differ_per_item() {
        let key = key();
        let a = seal(&key, b"c", b"same").unwrap();
        let b = seal(&key, b"c", b"same").unwrap();
        assert_ne!(a, b);
        assert_ne!(a[8..24], b[8..24]);
    }

    #[test]
    fn detects_tampering_truncation_reordering_and_wrong_context_or_key() {
        let key = key();
        let plain = vec![42u8; 3000];
        let sealed = seal_with_chunk(&key, b"ctx", &plain, 10).unwrap();

        // Bit flip in the body and in the header.
        for pos in [30, sealed.len() - 1, 5, 12] {
            let mut bad = sealed.clone();
            bad[pos] ^= 1;
            assert!(open(&key, b"ctx", &bad).is_err(), "flip at {pos}");
        }
        // Truncation at a chunk boundary (drops the final chunk).
        let boundary = HEADER_LEN + 2 * (1024 + TAG_LEN);
        assert!(open(&key, b"ctx", &sealed[..boundary]).is_err());
        // Truncation inside a chunk and below the header.
        assert!(open(&key, b"ctx", &sealed[..sealed.len() - 3]).is_err());
        assert!(open(&key, b"ctx", &sealed[..10]).is_err());
        assert!(open(&key, b"ctx", &sealed[..HEADER_LEN + 5]).is_err());
        // Swapped chunks.
        let mut swapped = sealed.clone();
        let (r0, r1) = (
            HEADER_LEN..HEADER_LEN + 1040,
            HEADER_LEN + 1040..HEADER_LEN + 2080,
        );
        let first = sealed[r0.clone()].to_vec();
        swapped[r0].copy_from_slice(&sealed[r1.clone()]);
        swapped[r1].copy_from_slice(&first);
        assert!(open(&key, b"ctx", &swapped).is_err());
        // Other context, other key.
        assert!(open(&key, b"other", &sealed).is_err());
        assert!(open(&DataKey::from_bytes([8; 32]), b"ctx", &sealed).is_err());
        // Plain data is not sealed data.
        assert!(open(&key, b"ctx", b"plain text that is long enough").is_err());
    }

    #[test]
    fn rejects_unknown_versions() {
        let key = key();
        let mut sealed = seal(&key, b"c", b"x").unwrap();
        sealed[4] = 2;
        assert!(matches!(
            open(&key, b"c", &sealed),
            Err(VaultError::Crypto(_))
        ));
        let mut sealed = seal(&key, b"c", b"x").unwrap();
        sealed[5] = 40;
        assert!(open(&key, b"c", &sealed).is_err());
    }

    #[test]
    fn key_hex_round_trip_and_debug_hides_it() {
        let key = DataKey::generate();
        let hex = key.to_hex();
        assert_eq!(hex.len(), 64);
        let back = DataKey::from_hex(&hex).unwrap();
        let sealed = seal(&key, b"c", b"x").unwrap();
        assert_eq!(open(&back, b"c", &sealed).unwrap(), b"x");
        assert!(DataKey::from_hex("zz").is_err());
        assert_eq!(format!("{key:?}"), "DataKey(..)");
    }
}
