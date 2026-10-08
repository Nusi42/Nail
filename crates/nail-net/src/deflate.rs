//! A byte stream compressed with raw DEFLATE in both directions
//! (`COMPRESS=DEFLATE`, RFC 4978), for the IMAP client and the mock IMAP
//! server.
//!
//! Reads go through a DEFLATE decoder, writes through an encoder; a flush
//! emits a sync-flush block so the peer can decode everything written so
//! far (the RFC requires a flush after every command and response).

use async_compression::tokio::bufread::DeflateDecoder;
use async_compression::tokio::write::DeflateEncoder;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// `T` wrapped in DEFLATE compression.
pub struct DeflateIo<T: AsyncRead + AsyncWrite + Unpin> {
    inner: DeflateDecoder<BufReader<DeflateEncoder<T>>>,
}

impl<T: AsyncRead + AsyncWrite + Unpin> DeflateIo<T> {
    /// Compresses `stream`.
    pub fn new(stream: T) -> Self {
        Self {
            inner: DeflateDecoder::new(BufReader::new(DeflateEncoder::new(stream))),
        }
    }

    /// The compressed stream underneath.
    pub fn get_ref(&self) -> &T {
        self.inner.get_ref().get_ref().get_ref()
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> std::fmt::Debug for DeflateIo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeflateIo")
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for DeflateIo<T> {
    crate::forward_async_read!(inner);
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for DeflateIo<T> {
    crate::forward_async_write!(inner);
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn round_trip_with_flushes() {
        let (a, b) = tokio::io::duplex(1024);
        let mut a = DeflateIo::new(a);
        let mut b = DeflateIo::new(b);
        a.write_all(b"* OK hello\r\n").await.unwrap();
        a.flush().await.unwrap();
        let mut buf = [0u8; 12];
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"* OK hello\r\n");
        b.write_all(b"A1 NOOP\r\n").await.unwrap();
        b.flush().await.unwrap();
        let mut buf = [0u8; 9];
        a.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"A1 NOOP\r\n");
        let _: &tokio::io::DuplexStream = a.get_ref();
        assert_eq!(format!("{a:?}"), "DeflateIo");
        a.shutdown().await.unwrap();
    }
}
