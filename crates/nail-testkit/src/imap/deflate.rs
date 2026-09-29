//! A byte stream compressed with raw DEFLATE in both directions
//! (`COMPRESS=DEFLATE`, RFC 4978).
//!
//! Reads go through a DEFLATE decoder, writes through an encoder; a flush
//! emits a sync-flush block so the peer can decode everything written so
//! far (the RFC requires a flush after every response).

use std::pin::Pin;
use std::task::{Context, Poll};

use async_compression::tokio::bufread::DeflateDecoder;
use async_compression::tokio::write::DeflateEncoder;
use tokio::io::{AsyncRead, AsyncWrite, BufReader, ReadBuf};

/// `T` wrapped in DEFLATE compression.
pub(crate) struct DeflateIo<T: AsyncRead + AsyncWrite + Unpin> {
    inner: DeflateDecoder<BufReader<DeflateEncoder<T>>>,
}

impl<T: AsyncRead + AsyncWrite + Unpin> DeflateIo<T> {
    pub(crate) fn new(stream: T) -> Self {
        Self {
            inner: DeflateDecoder::new(BufReader::new(DeflateEncoder::new(stream))),
        }
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for DeflateIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for DeflateIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
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
    }
}
