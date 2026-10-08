//! What the line-based mock servers (SMTP, POP3, ManageSieve) share: the
//! loopback listener with its accept loop and optional TLS, the injected
//! faults, and the poison-tolerant lock. Each server adds its protocol
//! session and its own recorded state.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nail_testkit::MockTls;
use nail_testkit::tls::TestTls;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use crate::stream::{Connection, MockStream};
use crate::{MockNetError, bind_loopback};

/// Locks a mutex, also after a panic of another holder (a failed test must
/// not hide the state it left).
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The listening side of a mock server on `127.0.0.1`: it accepts
/// connections, completes the implicit TLS handshake and runs one session
/// per connection. Stops when dropped.
#[derive(Debug)]
pub struct MockListener {
    addr: SocketAddr,
    ca_der: Option<Vec<u8>>,
    connections: Arc<AtomicUsize>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MockListener {
    /// Starts listening on a free loopback port. `session` gets each
    /// connection (TLS already accepted with [`MockTls::Implicit`]), the
    /// acceptor for a later `STARTTLS` and the connection's number (from 0).
    /// `protocol` names the server in logs.
    pub(crate) async fn start<F, Fut>(
        tls: MockTls,
        protocol: &'static str,
        session: F,
    ) -> Result<Self, MockNetError>
    where
        F: Fn(Connection, Option<TlsAcceptor>, usize) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::io::Result<()>> + Send + 'static,
    {
        let listener = bind_loopback().await?;
        let addr = listener.local_addr()?;
        let material = match tls {
            MockTls::None => None,
            MockTls::Implicit | MockTls::StartTls => Some(TestTls::generate()?),
        };
        let ca_der = material.as_ref().map(|t| t.ca_der.clone());
        let acceptor = material.map(|t| t.acceptor);
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&connections);
        let session = Arc::new(session);
        let (shutdown, mut stop) = watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.changed() => break,
                    accepted = listener.accept() => {
                        let Ok((tcp, _)) = accepted else { continue };
                        let n = counter.fetch_add(1, Ordering::SeqCst);
                        let session = Arc::clone(&session);
                        let acceptor = acceptor.clone();
                        tokio::spawn(async move {
                            drop(tcp.set_nodelay(true));
                            let opened = match (tls, &acceptor) {
                                (MockTls::Implicit, Some(a)) => Connection::accept_tls(tcp, a).await,
                                _ => Ok(Connection::new(MockStream::Plain(tcp))),
                            };
                            let result = match opened {
                                Ok(conn) => session(conn, acceptor, n).await,
                                Err(err) => Err(err),
                            };
                            if let Err(err) = result {
                                tracing::debug!(%err, "mock {protocol} session ended");
                            }
                        });
                    }
                }
            }
        });
        Ok(Self {
            addr,
            ca_der,
            connections,
            shutdown,
            task,
        })
    }

    /// [`Self::start`] for a session function that takes the server's
    /// shared state and its configuration ([`Session`]).
    pub(crate) async fn serve<S, C, F, Fut>(
        tls: MockTls,
        protocol: &'static str,
        shared: &Arc<S>,
        config: C,
        session: F,
    ) -> Result<Self, MockNetError>
    where
        S: Send + Sync + 'static,
        C: Send + Sync + 'static,
        F: Fn(Connection, Session<S, C>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::io::Result<()>> + Send + 'static,
    {
        let shared = Arc::clone(shared);
        let config = Arc::new(config);
        Self::start(tls, protocol, move |conn, acceptor, number| {
            let context = Session {
                shared: Arc::clone(&shared),
                config: Arc::clone(&config),
                acceptor,
                number,
            };
            session(conn, context)
        })
        .await
    }

    /// The listening address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The listening port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// DER of the CA that signed the server certificate (TLS modes only).
    pub fn ca_der(&self) -> Option<&[u8]> {
        self.ca_der.as_deref()
    }

    /// Number of accepted TCP connections.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

/// What one session of a mock server works with.
pub(crate) struct Session<S, C> {
    shared: Arc<S>,
    config: Arc<C>,
    acceptor: Option<TlsAcceptor>,
    /// The connection's number (from 0).
    pub(crate) number: usize,
}

impl<S, C> Session<S, C> {
    /// The server's shared state, its configuration and the acceptor for
    /// `STARTTLS`.
    pub(crate) fn parts(&self) -> (&S, &C, Option<&TlsAcceptor>) {
        (&self.shared, &self.config, self.acceptor.as_ref())
    }
}

impl Drop for MockListener {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

/// What a line-protocol mock records (command lines, successful logins)
/// and the faults it injects.
#[derive(Debug)]
pub(crate) struct Journal<F> {
    commands: Mutex<Vec<String>>,
    logins: AtomicUsize,
    /// The injected faults.
    pub(crate) faults: Faults<F>,
}

impl<F> Default for Journal<F> {
    fn default() -> Self {
        Self {
            commands: Mutex::new(Vec::new()),
            logins: AtomicUsize::new(0),
            faults: Faults::default(),
        }
    }
}

impl<F> Journal<F> {
    /// Records a command line (secrets already removed).
    pub(crate) fn command(&self, line: String) {
        lock(&self.commands).push(line);
    }

    /// Every command line, in order.
    pub(crate) fn commands(&self) -> Vec<String> {
        lock(&self.commands).clone()
    }

    /// Counts a successful login.
    pub(crate) fn login(&self) {
        self.logins.fetch_add(1, Ordering::SeqCst);
    }

    /// The successful logins so far.
    pub(crate) fn logins(&self) -> usize {
        self.logins.load(Ordering::SeqCst)
    }
}

/// `commands()` and `disconnect_on()` of a mock server whose shared state
/// has a [`Journal`] `journal` and whose fault type has `Disconnect`.
macro_rules! journal_controls {
    ($ty:ident) => {
        impl $ty {
            /// Every command received, in order (secrets removed).
            pub fn commands(&self) -> Vec<String> {
                self.shared.journal.commands()
            }

            /// Drops the connection without answering the next `command`.
            pub fn disconnect_on(&self, command: &str) {
                self.shared
                    .journal
                    .faults
                    .add(command, Fault::Disconnect, Some(1));
            }
        }
    };
}

/// `logins()` and `fail()` of a mock server with a [`Journal`] whose
/// error answers carry a text (`$fault(text)`, answered as `$answer`).
macro_rules! login_controls {
    ($ty:ident, $fault:path, $answer:literal) => {
        impl $ty {
            /// Number of successful logins.
            pub fn logins(&self) -> usize {
                self.shared.journal.logins()
            }

            #[doc = concat!("Answers the next `times` occurrences of `command` with ", $answer, ".")]
            pub fn fail(&self, command: &str, text: &str, times: usize) {
                self.shared
                    .journal
                    .faults
                    .add(command, $fault(text.to_owned()), Some(times));
            }
        }
    };
}

pub(crate) use {journal_controls, login_controls};

#[derive(Debug)]
struct FaultRule<F> {
    command: String,
    fault: F,
    /// `None` = forever.
    remaining: Option<usize>,
}

/// Faults injected per command (upper-case command names).
#[derive(Debug)]
pub(crate) struct Faults<F>(Mutex<Vec<FaultRule<F>>>);

impl<F> Default for Faults<F> {
    fn default() -> Self {
        Self(Mutex::new(Vec::new()))
    }
}

impl<F: Clone> Faults<F> {
    /// Answers the next `times` occurrences of `command` with `fault`
    /// (`None` = every time).
    pub(crate) fn add(&self, command: &str, fault: F, times: Option<usize>) {
        lock(&self.0).push(FaultRule {
            command: command.to_ascii_uppercase(),
            fault,
            remaining: times,
        });
    }

    /// The fault for this occurrence of `command`, if one is due.
    pub(crate) fn take(&self, command: &str) -> Option<F> {
        let mut faults = lock(&self.0);
        let index = faults.iter().position(|f| f.command == command)?;
        let fault = faults[index].fault.clone();
        match &mut faults[index].remaining {
            Some(n) if *n <= 1 => {
                faults.remove(index);
            }
            Some(n) => *n -= 1,
            None => {}
        }
        Some(fault)
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use super::*;

    #[test]
    fn faults_count_down() {
        let faults = Faults::default();
        faults.add("noop", 1, Some(2));
        faults.add("QUIT", 2, None);
        assert_eq!(faults.take("NOOP"), Some(1));
        assert_eq!(faults.take("NOOP"), Some(1));
        assert_eq!(faults.take("NOOP"), None);
        for _ in 0..3 {
            assert_eq!(faults.take("QUIT"), Some(2));
        }
    }

    #[tokio::test]
    async fn listener_runs_a_session_per_connection() {
        let listener =
            MockListener::start(MockTls::None, "test", |mut conn, acceptor, n| async move {
                assert!(acceptor.is_none());
                conn.line(&format!("hello {n}")).await
            })
            .await
            .unwrap();
        assert!(listener.addr().ip().is_loopback());
        assert_eq!(listener.port(), listener.addr().port());
        assert!(listener.ca_der().is_none());
        for n in 0..2 {
            let mut tcp = tokio::net::TcpStream::connect(listener.addr())
                .await
                .unwrap();
            let mut line = String::new();
            BufReader::new(&mut tcp).read_line(&mut line).await.unwrap();
            assert_eq!(line, format!("hello {n}\r\n"));
            drop(tcp.shutdown().await);
        }
        assert_eq!(listener.connections(), 2);
    }
}
