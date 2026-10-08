//! Stream plumbing of the line-based protocol clients (IMAP, POP3,
//! ManageSieve) and the mock servers: `AsyncRead` / `AsyncWrite` delegation
//! for stream enums and wrappers, bounded line reading, an idle timeout
//! that fires only while the peer is silent, and the I/O error kinds that
//! mean the peer went away.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::Poll;
use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite};

#[doc(hidden)]
pub use tokio as __tokio;

/// Longest line a line-based client or mock server accepts (protocol
/// lines are short; SASL responses and capability lists can be long).
pub const MAX_LINE: u64 = 64 * 1024;

/// The largest message a line-protocol client downloads (one response with
/// its literal; a guard against runaway servers).
pub const MAX_MESSAGE: usize = 512 * 1024 * 1024;

/// Implements `AsyncRead` and `AsyncWrite` for an enum whose variants each
/// hold one stream (a box of one works too), optionally with one empty
/// variant whose reads / writes fail with `$err` and whose flush / shutdown
/// answer `$flush`:
///
/// ```text
/// nail_net::delegate_stream_enum!(Stream { Plain, Tls });
/// nail_net::delegate_stream_enum!(Stream { Plain, Tls }; Closed => closed(), Ok(()));
/// ```
#[macro_export]
macro_rules! delegate_stream_enum {
    ($ty:ident { $($variant:ident),+ $(,)? } $(; $empty:ident => $err:expr, $flush:expr)?) => {
        impl $crate::io::__tokio::io::AsyncRead for $ty {
            $crate::__read_method!(__variant_call { [$($variant),+] $(, $empty, $err, $flush)? });
        }

        impl $crate::io::__tokio::io::AsyncWrite for $ty {
            $crate::__write_methods!(__variant_call { [$($variant),+] $(, $empty, $err, $flush)? });
        }
    };
}

/// The `AsyncWrite` methods of a wrapper that writes straight through to
/// its field `$field` (use inside `impl AsyncWrite for Wrapper`; the
/// wrapper must be `Unpin`).
#[macro_export]
macro_rules! forward_async_write {
    ($field:ident) => {
        $crate::__write_methods!(__field_call { $field });
    };
}

/// The `AsyncRead` method of a wrapper that reads straight from its field
/// `$field` (use inside `impl AsyncRead for Wrapper`; the wrapper must be
/// `Unpin`).
#[macro_export]
macro_rules! forward_async_read {
    ($field:ident) => {
        $crate::__read_method!(__field_call { $field });
    };
}

/// The `AsyncRead::poll_read` of a wrapper with its own logic: `$body`
/// sees the wrapper as `&mut Self` (`$this`), the context and the buffer
/// (use inside `impl AsyncRead for Wrapper`; the wrapper must be `Unpin`).
#[macro_export]
macro_rules! poll_read_with {
    (|$this:ident, $cx:ident, $buf:ident| $body:block) => {
        fn poll_read(
            self: ::std::pin::Pin<&mut Self>,
            $cx: &mut ::std::task::Context<'_>,
            $buf: &mut $crate::io::__tokio::io::ReadBuf<'_>,
        ) -> ::std::task::Poll<::std::io::Result<()>> {
            let $this = self.get_mut();
            $body
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __read_method {
    ($dispatch:ident { $($args:tt)* }) => {
        fn poll_read(
            self: ::std::pin::Pin<&mut Self>,
            cx: &mut ::std::task::Context<'_>,
            buf: &mut $crate::io::__tokio::io::ReadBuf<'_>,
        ) -> ::std::task::Poll<::std::io::Result<()>> {
            $crate::$dispatch!(self, poll_read(cx, buf), error, $($args)*)
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __write_methods {
    ($dispatch:ident { $($args:tt)* }) => {
        fn poll_write(
            self: ::std::pin::Pin<&mut Self>,
            cx: &mut ::std::task::Context<'_>,
            buf: &[u8],
        ) -> ::std::task::Poll<::std::io::Result<usize>> {
            $crate::$dispatch!(self, poll_write(cx, buf), error, $($args)*)
        }

        fn poll_flush(
            self: ::std::pin::Pin<&mut Self>,
            cx: &mut ::std::task::Context<'_>,
        ) -> ::std::task::Poll<::std::io::Result<()>> {
            $crate::$dispatch!(self, poll_flush(cx), flush, $($args)*)
        }

        fn poll_shutdown(
            self: ::std::pin::Pin<&mut Self>,
            cx: &mut ::std::task::Context<'_>,
        ) -> ::std::task::Poll<::std::io::Result<()>> {
            $crate::$dispatch!(self, poll_shutdown(cx), flush, $($args)*)
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __field_call {
    ($this:ident, $method:ident $args:tt, $kind:ident, $field:ident) => {
        ::std::pin::Pin::new(&mut $this.get_mut().$field).$method $args
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __variant_call {
    (
        $this:ident, $method:ident $args:tt, $kind:ident,
        [$($variant:ident),+] $(, $empty:ident, $err:expr, $flush:expr)?
    ) => {
        match $this.get_mut() {
            $(Self::$variant(s) => ::std::pin::Pin::new(s).$method $args,)+
            $(Self::$empty => ::std::task::Poll::Ready($crate::__empty_result!($kind, $err, $flush)),)?
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __empty_result {
    (error, $err:expr, $flush:expr) => {
        Err($err)
    };
    (flush, $err:expr, $flush:expr) => {
        $flush
    };
}

/// One line read by [`read_line_limited`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineRead {
    /// The peer closed the stream before sending anything.
    Eof,
    /// No line feed within the limit.
    TooLong,
    /// A line, without its final line feed (a carriage return before it
    /// stays: protocols differ in how strictly they strip it).
    Line(Vec<u8>),
}

/// Reads one `\n`-terminated line of at most `limit` bytes.
pub async fn read_line_limited<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: u64,
) -> std::io::Result<LineRead> {
    let mut line = Vec::new();
    let n = reader.take(limit).read_until(b'\n', &mut line).await?;
    if n == 0 {
        return Ok(LineRead::Eof);
    }
    if line.pop() != Some(b'\n') {
        return Ok(LineRead::TooLong);
    }
    Ok(LineRead::Line(line))
}

/// The English diagnostic of a response line longer than [`MAX_LINE`].
pub const LINE_TOO_LONG: &str = "response line too long";

/// Reads one response line of a line protocol (at most [`MAX_LINE`]
/// bytes) without its CRLF; the end of the stream is `eof()`, an over-long
/// line `protocol(`[`LINE_TOO_LONG`]`)`.
pub async fn read_response_line<R, E>(
    reader: &mut R,
    eof: impl FnOnce() -> E,
    protocol: impl FnOnce(String) -> E,
) -> Result<Vec<u8>, E>
where
    R: AsyncBufRead + Unpin,
    E: From<std::io::Error>,
{
    match read_line_limited(reader, MAX_LINE).await? {
        LineRead::Eof => Err(eof()),
        LineRead::TooLong => Err(protocol(LINE_TOO_LONG.to_owned())),
        LineRead::Line(mut line) => {
            if line.ends_with(b"\r") {
                line.pop();
            }
            Ok(line)
        }
    }
}

/// Runs `fut` for at most `timeout`; `expired` is the error when it takes
/// longer.
pub async fn with_timeout<T, E>(
    timeout: Duration,
    fut: impl Future<Output = Result<T, E>>,
    expired: impl FnOnce(Duration) -> E,
) -> Result<T, E> {
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| expired(timeout))?
}

/// Whether an I/O error means the peer closed or dropped the connection.
#[must_use]
pub fn is_disconnect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
    )
}

/// The idle timeout of an [`IdleTimeout`] stream, shared with the code that
/// drives it so it can be switched off (IMAP `IDLE`).
#[derive(Debug, Clone)]
pub struct ReadTimeout(Arc<AtomicU64>);

impl ReadTimeout {
    /// A timeout of `timeout`.
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        let this = Self(Arc::new(AtomicU64::new(0)));
        this.set(Some(timeout));
        this
    }

    /// `None` switches the timeout off.
    pub fn set(&self, timeout: Option<Duration>) {
        let millis = timeout.map_or(0, |t| {
            u64::try_from(t.as_millis()).unwrap_or(u64::MAX).max(1)
        });
        self.0.store(millis, Ordering::Relaxed);
    }

    /// The current timeout (`None` = off).
    #[must_use]
    pub fn get(&self) -> Option<Duration> {
        match self.0.load(Ordering::Relaxed) {
            0 => None,
            millis => Some(Duration::from_millis(millis)),
        }
    }
}

/// A stream whose reads fail with [`std::io::ErrorKind::TimedOut`] when the
/// peer sends nothing for the [`ReadTimeout`] while a read is waiting.
///
/// Unlike a timeout around a whole command, this never fires while data is
/// flowing, so a `FETCH` of large bodies over a slow link can take as long
/// as it needs, while a silent server is still detected.
#[derive(Debug)]
pub struct IdleTimeout<S> {
    inner: S,
    timeout: ReadTimeout,
    sleep: Pin<Box<tokio::time::Sleep>>,
    /// Whether `sleep` is set for the current wait.
    armed: bool,
}

impl<S> IdleTimeout<S> {
    /// Wraps `inner`.
    pub fn new(inner: S, timeout: ReadTimeout) -> Self {
        Self {
            inner,
            timeout,
            sleep: Box::pin(tokio::time::sleep(Duration::ZERO)),
            armed: false,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for IdleTimeout<S> {
    crate::poll_read_with!(|this, cx, buf| {
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(result) => {
                this.armed = false;
                Poll::Ready(result)
            }
            Poll::Pending => {
                let Some(idle) = this.timeout.get() else {
                    this.armed = false;
                    return Poll::Pending;
                };
                if !this.armed {
                    let deadline = tokio::time::Instant::now() + idle;
                    this.sleep.as_mut().reset(deadline);
                    this.armed = true;
                }
                match this.sleep.as_mut().poll(cx) {
                    Poll::Ready(()) => {
                        this.armed = false;
                        Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            format!("no data from the server for {idle:?}"),
                        )))
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    });
}

impl<S: AsyncWrite + Unpin> AsyncWrite for IdleTimeout<S> {
    crate::forward_async_write!(inner);
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncWriteExt, BufReader};

    use super::*;

    enum Either {
        Left(tokio::io::DuplexStream),
        Right(Box<tokio::io::DuplexStream>),
        Gone,
    }

    fn gone() -> std::io::Error {
        std::io::Error::other("gone")
    }

    delegate_stream_enum!(Either { Left, Right }; Gone => gone(), Ok(()));

    #[tokio::test]
    async fn enums_delegate_to_their_stream() {
        let (a, mut b) = tokio::io::duplex(64);
        let mut left = Either::Left(a);
        left.write_all(b"hi").await.unwrap();
        left.flush().await.unwrap();
        let mut buf = [0u8; 2];
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hi");
        let (c, mut d) = tokio::io::duplex(64);
        let mut right = Either::Right(Box::new(c));
        d.write_all(b"yo").await.unwrap();
        right.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"yo");
        right.shutdown().await.unwrap();
        let mut empty = Either::Gone;
        assert!(empty.write_all(b"x").await.is_err());
        assert!(empty.read_u8().await.is_err());
        empty.flush().await.unwrap();
        empty.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn lines_are_bounded() {
        let mut reader = BufReader::new(&b"one\r\ntwo\nthree-is-long\n"[..]);
        assert_eq!(
            read_line_limited(&mut reader, 64).await.unwrap(),
            LineRead::Line(b"one\r".to_vec())
        );
        assert_eq!(
            read_line_limited(&mut reader, 64).await.unwrap(),
            LineRead::Line(b"two".to_vec())
        );
        assert_eq!(
            read_line_limited(&mut reader, 4).await.unwrap(),
            LineRead::TooLong
        );
        let mut empty = BufReader::new(&b""[..]);
        assert_eq!(
            read_line_limited(&mut empty, 4).await.unwrap(),
            LineRead::Eof
        );
        let mut partial = BufReader::new(&b"no newline"[..]);
        assert_eq!(
            read_line_limited(&mut partial, 64).await.unwrap(),
            LineRead::TooLong
        );
    }

    #[derive(Debug, PartialEq, Eq)]
    enum LineError {
        Eof,
        Protocol(String),
        Io,
    }

    impl From<std::io::Error> for LineError {
        fn from(_: std::io::Error) -> Self {
            Self::Io
        }
    }

    #[tokio::test]
    async fn response_lines_lose_their_crlf() {
        let long = vec![b'x'; usize::try_from(MAX_LINE).unwrap() + 1];
        let mut input = b"+OK ready\r\nbare\n".to_vec();
        input.extend_from_slice(&long);
        let mut reader = BufReader::new(&input[..]);
        let mut next =
            async || read_response_line(&mut reader, || LineError::Eof, LineError::Protocol).await;
        assert_eq!(next().await, Ok(b"+OK ready".to_vec()));
        assert_eq!(next().await, Ok(b"bare".to_vec()));
        assert_eq!(
            next().await,
            Err(LineError::Protocol(LINE_TOO_LONG.to_owned()))
        );
        let mut empty = BufReader::new(&b""[..]);
        assert_eq!(
            read_response_line(&mut empty, || LineError::Eof, LineError::Protocol).await,
            Err(LineError::Eof)
        );
    }

    #[tokio::test]
    async fn timeouts() {
        let quick = with_timeout(Duration::from_secs(5), async { Ok::<_, String>(1) }, |_| {
            "late".to_owned()
        });
        assert_eq!(quick.await, Ok(1));
        let slow = with_timeout(
            Duration::from_millis(10),
            async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok::<u8, String>(1)
            },
            |after| format!("late after {after:?}"),
        );
        assert_eq!(slow.await, Err("late after 10ms".to_owned()));
    }

    #[test]
    fn disconnect_kinds() {
        for kind in [
            std::io::ErrorKind::UnexpectedEof,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::BrokenPipe,
        ] {
            assert!(is_disconnect(&kind.into()), "{kind:?}");
        }
        assert!(!is_disconnect(&std::io::ErrorKind::TimedOut.into()));
    }

    #[tokio::test]
    async fn idle_timeout_fires_only_when_the_peer_is_silent() {
        let idle = Duration::from_millis(150);
        let (client, mut server) = tokio::io::duplex(64);
        let timeout = ReadTimeout::new(idle);
        let mut stream = IdleTimeout::new(client, timeout.clone());

        // A slow but steady sender: 12 bytes over ~600 ms, far longer than
        // `idle` in total, but never silent for that long.
        let sender = tokio::spawn(async move {
            for _ in 0..12 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                server.write_all(b"x").await.unwrap();
            }
            server
        });
        let mut buf = [0u8; 12];
        stream.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"xxxxxxxxxxxx");

        // Now the peer stays connected but silent.
        let mut server = sender.await.unwrap();
        let started = std::time::Instant::now();
        let err = stream.read_u8().await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= idle);

        // Switched off (as while waiting in IDLE), a silent peer is fine.
        timeout.set(None);
        assert_eq!(timeout.get(), None);
        let silent = tokio::time::timeout(idle * 3, stream.read_u8()).await;
        assert!(silent.is_err(), "the read must still be pending");

        // Writes go straight through.
        stream.write_all(b"ok").await.unwrap();
        stream.flush().await.unwrap();
        let mut back = [0u8; 2];
        server.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"ok");
    }
}
