//! Test helpers (feature `test-support`): a raw HTTP/1.1 upstream on
//! loopback that records every request, and a scripted DNS resolver.
//! Used by this crate's and `nail-server`'s tests; never in release builds.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::fetch::{Lookup, Resolver};

/// A canned upstream answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Status code.
    pub status: u16,
    /// Headers (besides `Content-Length` / `Connection`).
    pub headers: Vec<(String, String)>,
    /// Body.
    pub body: Vec<u8>,
    /// Send `Content-Length` (else the body ends with the connection).
    pub content_length: bool,
    /// Wait before answering.
    pub delay: Option<Duration>,
}

impl Reply {
    /// `200` with `content_type` and `body`.
    pub fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), content_type.into())],
            body: body.into(),
            content_length: true,
            delay: None,
        }
    }

    /// A redirect to `location`.
    pub fn redirect(status: u16, location: &str) -> Self {
        Self {
            status,
            headers: vec![("Location".into(), location.into())],
            body: Vec::new(),
            content_length: true,
            delay: None,
        }
    }

    /// An empty answer with `status`.
    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            content_length: true,
            delay: None,
        }
    }

    /// Adds a header.
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Omits `Content-Length` (body delimited by closing the connection).
    #[must_use]
    pub fn without_length(mut self) -> Self {
        self.content_length = false;
        self
    }

    /// Delays the answer.
    #[must_use]
    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
}

/// A request the upstream received: request line and headers, lowercase
/// header names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// `GET /path HTTP/1.1`.
    pub request_line: String,
    /// Path with query.
    pub path: String,
    /// Headers in order.
    pub headers: Vec<(String, String)>,
}

impl Recorded {
    /// The first value of header `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Default)]
struct State {
    routes: Mutex<HashMap<String, Reply>>,
    requests: Mutex<Vec<Recorded>>,
    connections: AtomicUsize,
}

/// A loopback HTTP/1.1 server answering from a route table. Unknown paths
/// get `404`. Stops when dropped.
#[derive(Debug)]
pub struct Upstream {
    addr: SocketAddr,
    state: Arc<State>,
    task: JoinHandle<()>,
}

impl Upstream {
    /// Starts on a free port of `127.0.0.1`.
    pub async fn start() -> io::Result<Self> {
        Self::start_on("127.0.0.1:0".parse().unwrap_or_else(|_| unreachable!())).await
    }

    /// Starts on `addr` (e.g. `[::1]:0`).
    pub async fn start_on(addr: SocketAddr) -> io::Result<Self> {
        Self::listen(TcpListener::bind(addr).await?, None)
    }

    /// Starts an HTTPS upstream on a free port of `127.0.0.1` with `tls`.
    pub async fn start_tls(tls: Arc<rustls::ServerConfig>) -> io::Result<Self> {
        Self::listen(
            TcpListener::bind(("127.0.0.1", 0)).await?,
            Some(tokio_rustls::TlsAcceptor::from(tls)),
        )
    }

    /// Serves every connection to `listener`, through `tls` when given.
    fn listen(listener: TcpListener, tls: Option<tokio_rustls::TlsAcceptor>) -> io::Result<Self> {
        let addr = listener.local_addr()?;
        let state = Arc::new(State::default());
        let task = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    state.connections.fetch_add(1, Ordering::SeqCst);
                    let state = Arc::clone(&state);
                    let tls = tls.clone();
                    tokio::spawn(async move {
                        match tls {
                            None => drop(serve_inner(stream, &state).await),
                            Some(acceptor) => {
                                if let Ok(stream) = acceptor.accept(stream).await {
                                    drop(serve_inner(stream, &state).await);
                                }
                            }
                        }
                    });
                }
            })
        };
        Ok(Self { addr, state, task })
    }

    /// The listening address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Answers `path` (with query) with `reply`.
    pub fn route(&self, path: &str, reply: Reply) {
        lock(&self.state.routes).insert(path.to_owned(), reply);
    }

    /// Every request so far.
    pub fn requests(&self) -> Vec<Recorded> {
        lock(&self.state.requests).clone()
    }

    /// TCP connections accepted so far.
    pub fn connections(&self) -> usize {
        self.state.connections.load(Ordering::SeqCst)
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Serves one request per connection (the fetcher keeps no pool).
async fn serve_inner<S>(mut stream: S, state: &State) -> io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).await?;
        if n == 0 || buf.len() > 64 * 1024 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("").to_owned();
    let path = request_line.split(' ').nth(1).unwrap_or("/").to_owned();
    let headers = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    lock(&state.requests).push(Recorded {
        request_line,
        path: path.clone(),
        headers,
    });
    let reply = lock(&state.routes)
        .get(&path)
        .cloned()
        .unwrap_or_else(|| Reply::status(404));
    if let Some(delay) = reply.delay {
        tokio::time::sleep(delay).await;
    }
    let mut head = format!("HTTP/1.1 {} X\r\nConnection: close\r\n", reply.status);
    for (name, value) in &reply.headers {
        // Writing to a String cannot fail.
        let _ = write!(head, "{name}: {value}\r\n");
    }
    if reply.content_length {
        let _ = write!(head, "Content-Length: {}\r\n", reply.body.len());
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&reply.body).await?;
    stream.flush().await?;
    stream.shutdown().await
}

/// A resolver with scripted answers per name: each lookup takes the next
/// answer; the last one repeats. Unknown names fail. Lookups are counted.
#[derive(Debug, Default)]
pub struct ScriptedResolver {
    answers: Mutex<HashMap<String, Vec<Vec<IpAddr>>>>,
    lookups: Mutex<Vec<String>>,
}

impl ScriptedResolver {
    /// No names.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// `name` answers `answers[0]`, then `answers[1]`, … (the last repeats).
    pub fn script(&self, name: &str, answers: Vec<Vec<IpAddr>>) {
        lock(&self.answers).insert(name.to_ascii_lowercase(), answers);
    }

    /// `name` always answers `addrs`.
    ///
    /// # Panics
    ///
    /// When an entry of `addrs` is not an IP address (a bug in the test).
    pub fn set(&self, name: &str, addrs: &[&str]) {
        let addrs = addrs
            .iter()
            .map(|a| a.parse().unwrap_or_else(|_| panic!("bad address {a}")))
            .collect();
        self.script(name, vec![addrs]);
    }

    /// Names looked up so far, in order.
    pub fn lookups(&self) -> Vec<String> {
        lock(&self.lookups).clone()
    }
}

impl Resolver for ScriptedResolver {
    fn lookup<'a>(&'a self, host: &'a str) -> Lookup<'a> {
        Box::pin(async move {
            let host = host.to_ascii_lowercase();
            lock(&self.lookups).push(host.clone());
            let mut answers = lock(&self.answers);
            let script = answers
                .get_mut(&host)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such name"))?;
            if script.len() > 1 {
                Ok(script.remove(0))
            } else {
                Ok(script.first().cloned().unwrap_or_default())
            }
        })
    }
}
