//! An in-process mock IMAP4rev1 server for tests.
//!
//! Supported: `CAPABILITY`, `NOOP`, `LOGOUT`, `STARTTLS`, `LOGIN`,
//! `AUTHENTICATE PLAIN` (with or without initial response), `LIST` (with
//! RFC 6154 SPECIAL-USE attributes), `SELECT`, `EXAMINE`, `CLOSE`, `FETCH` /
//! `UID FETCH` (`UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE BODY[]
//! BODY.PEEK[] BODY[HEADER] BODY[TEXT] RFC822*` and the `ALL`/`FAST`/`FULL`
//! macros) and `SEARCH` / `UID SEARCH` (`ALL`, `UID <set>`, `SEEN`, `UNSEEN`).
//!
//! The server listens on `127.0.0.1:0`; mailboxes come from a shared
//! [`MailStore`]. Tests can inject failures with
//! [`MockImapServer::disconnect_on`].

mod command;
mod encode;
mod session;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::mailstore::MailStore;
use crate::tls::{TestTls, TestTlsError};

/// How the mock server offers TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MockTls {
    /// Plaintext only (tests only).
    #[default]
    None,
    /// TLS from the first byte (like port 993).
    Implicit,
    /// Plaintext greeting, `STARTTLS` required before authentication
    /// (`LOGINDISABLED` is advertised until TLS is active).
    StartTls,
}

/// Configuration of a [`MockImapServer`].
#[derive(Debug, Clone)]
pub struct MockImapConfig {
    /// Accepted user name.
    pub username: String,
    /// Accepted password.
    pub password: String,
    /// TLS mode.
    pub tls: MockTls,
}

impl Default for MockImapConfig {
    fn default() -> Self {
        Self {
            username: "sam@nail.example".to_owned(),
            password: "secret".to_owned(),
            tls: MockTls::None,
        }
    }
}

/// Errors when starting the mock server.
#[derive(Debug, thiserror::Error)]
pub enum MockServerError {
    /// Binding the listener failed.
    #[error("binding the mock IMAP server: {0}")]
    Io(#[from] std::io::Error),
    /// Generating TLS material failed.
    #[error(transparent)]
    Tls(#[from] TestTlsError),
}

/// Shared runtime controls of a server (failure injection, counters).
#[derive(Debug, Default)]
pub(crate) struct Control {
    /// Upper-case command name (`"UID FETCH"`, `"EXAMINE"`, ...) on which the
    /// server drops the connection without answering.
    disconnect_on: Mutex<Option<String>>,
    /// Upper-case command name the server answers with a tagged `NO`.
    fail_on: Mutex<Option<String>>,
    connections: AtomicUsize,
}

impl Control {
    pub(crate) fn should_disconnect(&self, command: &str) -> bool {
        matches_command(&self.disconnect_on, command)
    }

    pub(crate) fn should_fail(&self, command: &str) -> bool {
        matches_command(&self.fail_on, command)
    }
}

fn matches_command(slot: &Mutex<Option<String>>, command: &str) -> bool {
    let guard = slot.lock().unwrap_or_else(|p| p.into_inner());
    guard.as_deref().is_some_and(|c| c == command)
}

/// A running mock IMAP server. Stops when dropped.
#[derive(Debug)]
pub struct MockImapServer {
    addr: SocketAddr,
    store: MailStore,
    config: Arc<MockImapConfig>,
    ca_der: Option<Vec<u8>>,
    control: Arc<Control>,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl MockImapServer {
    /// Starts a server on `127.0.0.1:0` serving `store`.
    pub async fn start(config: MockImapConfig, store: MailStore) -> Result<Self, MockServerError> {
        let tls = match config.tls {
            MockTls::None => None,
            MockTls::Implicit | MockTls::StartTls => Some(TestTls::generate()?),
        };
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let config = Arc::new(config);
        let control = Arc::new(Control::default());
        let (shutdown, shutdown_rx) = watch::channel(false);
        let ca_der = tls.as_ref().map(|t| t.ca_der.clone());

        let task = tokio::spawn(accept_loop(
            listener,
            session::Shared {
                store: store.clone(),
                config: Arc::clone(&config),
                control: Arc::clone(&control),
                tls,
            },
            shutdown_rx,
        ));

        Ok(Self {
            addr,
            store,
            config,
            ca_der,
            control,
            shutdown,
            task: Some(task),
        })
    }

    /// Starts a plaintext server with default credentials serving the demo corpus.
    pub async fn start_demo() -> Result<Self, MockServerError> {
        Self::start(MockImapConfig::default(), MailStore::demo()).await
    }

    /// The listening address (`127.0.0.1:<port>`).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The listening port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// The host to connect to (`127.0.0.1`; the TLS certificate covers it).
    pub fn host(&self) -> &'static str {
        "127.0.0.1"
    }

    /// The served mailbox state; changes are visible to connected clients.
    pub fn store(&self) -> &MailStore {
        &self.store
    }

    /// The server configuration.
    pub fn config(&self) -> &MockImapConfig {
        &self.config
    }

    /// DER of the CA certificate that signed the server certificate, when TLS
    /// is enabled. Clients must trust it as a root.
    pub fn ca_certificate_der(&self) -> Option<&[u8]> {
        self.ca_der.as_deref()
    }

    /// Makes the server drop the connection (no response) when a client sends
    /// `command` (case-insensitive, e.g. `"UID FETCH"`, `"EXAMINE"`).
    /// `None` switches failure injection off.
    pub fn disconnect_on(&self, command: Option<&str>) {
        let mut guard = self
            .control
            .disconnect_on
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *guard = command.map(str::to_ascii_uppercase);
    }

    /// Makes the server answer `command` (e.g. `"UID FETCH"`) with a tagged
    /// `NO` and no data. `None` switches failure injection off.
    pub fn fail_on(&self, command: Option<&str>) {
        let mut guard = self
            .control
            .fail_on
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *guard = command.map(str::to_ascii_uppercase);
    }

    /// Number of accepted connections so far.
    pub fn connection_count(&self) -> usize {
        self.control.connections.load(Ordering::SeqCst)
    }

    /// Stops accepting and closes all connections.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for MockImapServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

async fn accept_loop(
    listener: TcpListener,
    shared: session::Shared,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut sessions = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                shared.control.connections.fetch_add(1, Ordering::SeqCst);
                tracing::debug!(%peer, "mock IMAP connection");
                let shared = shared.clone();
                let shutdown = shutdown.clone();
                sessions.spawn(session::run(stream, shared, shutdown));
            }
        }
    }
    sessions.shutdown().await;
}
