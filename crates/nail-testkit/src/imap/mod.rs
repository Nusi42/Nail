//! An in-process mock IMAP4rev1 server for tests.
//!
//! Supported: `CAPABILITY`, `NOOP`, `CHECK`, `LOGOUT`, `STARTTLS`, `LOGIN`,
//! `AUTHENTICATE` (`PLAIN`, `XOAUTH2`, `OAUTHBEARER`; with or without
//! SASL-IR), `ENABLE` (`CONDSTORE`, `QRESYNC`), `ID`, `COMPRESS DEFLATE`,
//! `LIST` (with RFC 6154 SPECIAL-USE attributes), `STATUS`, `SELECT` /
//! `EXAMINE` (with `CONDSTORE` and `QRESYNC` parameters), `CLOSE`,
//! `UNSELECT`, `FETCH` / `UID FETCH` (`UID FLAGS INTERNALDATE RFC822.SIZE
//! ENVELOPE MODSEQ BODY[] BODY.PEEK[] BODY[HEADER] BODY[TEXT] RFC822*`, the
//! `ALL`/`FAST`/`FULL` macros, `CHANGEDSINCE` and `VANISHED` modifiers),
//! `SEARCH` / `UID SEARCH`, `STORE` / `UID STORE` (with `UNCHANGEDSINCE`),
//! `COPY` / `UID COPY`, `MOVE` / `UID MOVE`, `EXPUNGE` / `UID EXPUNGE`,
//! `APPEND` (with UIDPLUS results and `APPENDLIMIT`), `CREATE`, `DELETE`,
//! `RENAME`, `SUBSCRIBE`, `UNSUBSCRIBE` and `IDLE`.
//!
//! Every session keeps its own view of the selected mailbox. When the
//! [`MailStore`] changes (another session or a test helper), the session
//! reports the difference like a real server: `EXISTS`, `EXPUNGE` (or
//! `VANISHED` once QRESYNC is enabled) and `FETCH` flag updates, at the next
//! command or immediately while it is in `IDLE`.
//!
//! The advertised extensions are configurable ([`MockCapabilities`]), so
//! tests can exercise client fallbacks. Failure injection: see
//! [`MockImapServer::disconnect_on`], [`MockImapServer::fail_on`],
//! [`MockImapServer::disconnect_after`],
//! [`MockImapServer::drop_idle_connections`] and
//! [`MockImapServer::delay_responses`].

mod command;
mod deflate;
mod encode;
mod session;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{Notify, watch};
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

/// The IMAP extensions a [`MockImapServer`] advertises and implements.
/// `IMAP4rev1` and `AUTH=PLAIN` are always on; `AUTH=XOAUTH2` and
/// `AUTH=OAUTHBEARER` follow [`MockImapConfig::oauth_token`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct MockCapabilities {
    /// `IDLE` (RFC 2177).
    pub idle: bool,
    /// `CONDSTORE` (RFC 7162).
    pub condstore: bool,
    /// `QRESYNC` (RFC 7162; advertised only together with `condstore` and
    /// `enable`).
    pub qresync: bool,
    /// `ENABLE` (RFC 5161).
    pub enable: bool,
    /// `MOVE` (RFC 6851).
    pub move_: bool,
    /// `UIDPLUS` (RFC 4315): `APPENDUID` / `COPYUID` and `UID EXPUNGE`.
    pub uidplus: bool,
    /// `COMPRESS=DEFLATE` (RFC 4978).
    pub compress: bool,
    /// `ID` (RFC 2971).
    pub id: bool,
    /// `SASL-IR` (RFC 4959): initial responses in `AUTHENTICATE`.
    pub sasl_ir: bool,
    /// `LITERAL+` (RFC 7888): non-synchronizing literals.
    pub literal_plus: bool,
    /// `SPECIAL-USE` (RFC 6154).
    pub special_use: bool,
    /// Larger `APPEND`s fail with `[TOOBIG]` (a synchronizing literal is
    /// refused before it is sent).
    pub append_limit: Option<u64>,
    /// Advertise the limit as `APPENDLIMIT=<n>` (RFC 7889). `false` hides
    /// it, like servers that only enforce a size limit.
    pub advertise_append_limit: bool,
}

impl Default for MockCapabilities {
    /// A modern server: every extension on, no append limit.
    fn default() -> Self {
        Self {
            idle: true,
            condstore: true,
            qresync: true,
            enable: true,
            move_: true,
            uidplus: true,
            compress: true,
            id: true,
            sasl_ir: true,
            literal_plus: true,
            special_use: true,
            append_limit: None,
            advertise_append_limit: true,
        }
    }
}

impl MockCapabilities {
    /// A bare IMAP4rev1 server (plus SPECIAL-USE, so folder roles still
    /// work): no IDLE, CONDSTORE, QRESYNC, ENABLE, MOVE, UIDPLUS, COMPRESS,
    /// ID, SASL-IR or LITERAL+. Tests of client fallbacks start from here.
    pub fn imap4rev1() -> Self {
        Self {
            idle: false,
            condstore: false,
            qresync: false,
            enable: false,
            move_: false,
            uidplus: false,
            compress: false,
            id: false,
            sasl_ir: false,
            literal_plus: false,
            special_use: true,
            append_limit: None,
            advertise_append_limit: true,
        }
    }

    /// Whether QRESYNC is really offered (it needs CONDSTORE and ENABLE).
    pub(crate) fn qresync_offered(&self) -> bool {
        self.qresync && self.condstore && self.enable
    }
}

/// The parameters of one `ID` command (RFC 2971): key / value pairs,
/// `NIL` values as `None`.
pub type ClientId = Vec<(String, Option<String>)>;

/// Configuration of a [`MockImapServer`].
#[derive(Debug, Clone)]
pub struct MockImapConfig {
    /// Accepted user name.
    pub username: String,
    /// Accepted password.
    pub password: String,
    /// TLS mode.
    pub tls: MockTls,
    /// Advertised extensions.
    pub capabilities: MockCapabilities,
    /// The OAuth2 access token accepted by `AUTHENTICATE XOAUTH2` /
    /// `OAUTHBEARER` (for [`Self::username`]). `None` disables both
    /// mechanisms. Can be changed at runtime with
    /// [`MockImapServer::set_oauth_token`].
    pub oauth_token: Option<String>,
}

impl Default for MockImapConfig {
    fn default() -> Self {
        Self {
            username: "sam@nail.example".to_owned(),
            password: "secret".to_owned(),
            tls: MockTls::None,
            capabilities: MockCapabilities::default(),
            oauth_token: None,
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

/// Shared runtime controls of a server (failure injection, counters,
/// recordings).
#[derive(Debug, Default)]
pub(crate) struct Control {
    /// Upper-case command name (`"UID FETCH"`, `"EXAMINE"`, ...) on which the
    /// server drops the connection without answering.
    disconnect_on: Mutex<Option<String>>,
    /// Upper-case command name the server answers with a tagged `NO`.
    fail_on: Mutex<Option<String>>,
    /// Upper-case substring of a command line and how many matching commands
    /// still pass before the server drops the connection (once).
    disconnect_after: Mutex<Option<(String, usize)>>,
    /// Mailboxes that `SELECT`/`EXAMINE` refuse (as if forbidden by ACLs).
    denied_mailboxes: Mutex<Vec<String>>,
    /// Delay before answering commands (all, or one upper-case name).
    delay: Mutex<Option<(Option<String>, Duration)>>,
    /// Wakes sessions in `IDLE` to drop their connection.
    pub(crate) idle_kill: Notify,
    /// Sessions currently in `IDLE`.
    pub(crate) idling: AtomicUsize,
    /// Wakes [`MockImapServer::wait_for_idle`] when `idling` changes.
    pub(crate) idle_changed: Notify,
    /// Every command received (normalized names such as `"UID STORE"`,
    /// `"AUTHENTICATE XOAUTH2"`).
    commands: Mutex<Vec<String>>,
    /// Parameters of every `ID` command received.
    client_ids: Mutex<Vec<ClientId>>,
    /// Sessions that switched on `COMPRESS=DEFLATE`.
    pub(crate) compressed: AtomicUsize,
    /// Accepted OAuth2 access token.
    oauth_token: Mutex<Option<String>>,
    connections: AtomicUsize,
    /// Simulated outage: new connections are closed at once and sessions
    /// close on their next command.
    pub(crate) offline: std::sync::atomic::AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

impl Control {
    pub(crate) fn should_disconnect(&self, command: &str) -> bool {
        matches_command(&self.disconnect_on, command)
    }

    pub(crate) fn should_fail(&self, command: &str) -> bool {
        matches_command(&self.fail_on, command)
    }

    /// Counts `line` against [`MockImapServer::disconnect_after`].
    pub(crate) fn should_disconnect_line(&self, line: &[u8]) -> bool {
        let mut guard = lock(&self.disconnect_after);
        let Some((pattern, remaining)) = guard.as_mut() else {
            return false;
        };
        let line = String::from_utf8_lossy(line).to_ascii_uppercase();
        if !line.contains(pattern.as_str()) {
            return false;
        }
        if *remaining == 0 {
            *guard = None;
            return true;
        }
        *remaining -= 1;
        false
    }

    pub(crate) fn is_denied(&self, mailbox: &str) -> bool {
        lock(&self.denied_mailboxes).iter().any(|m| m == mailbox)
    }

    pub(crate) fn delay_for(&self, command: &str) -> Option<Duration> {
        match &*lock(&self.delay) {
            Some((None, delay)) => Some(*delay),
            Some((Some(name), delay)) if name == command => Some(*delay),
            _ => None,
        }
    }

    pub(crate) fn record_command(&self, command: String) {
        lock(&self.commands).push(command);
    }

    pub(crate) fn record_client_id(&self, id: ClientId) {
        lock(&self.client_ids).push(id);
    }

    pub(crate) fn oauth_token(&self) -> Option<String> {
        lock(&self.oauth_token).clone()
    }

    pub(crate) fn set_idling(&self, delta: isize) {
        if delta >= 0 {
            self.idling
                .fetch_add(delta.unsigned_abs(), Ordering::SeqCst);
        } else {
            self.idling
                .fetch_sub(delta.unsigned_abs(), Ordering::SeqCst);
        }
        self.idle_changed.notify_waiters();
    }
}

fn matches_command(slot: &Mutex<Option<String>>, command: &str) -> bool {
    lock(slot).as_deref().is_some_and(|c| c == command)
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
        let control = Arc::new(Control {
            oauth_token: Mutex::new(config.oauth_token.clone()),
            ..Control::default()
        });
        let config = Arc::new(config);
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

    /// The served mailbox state; changes are visible to connected clients
    /// (and pushed to sessions in `IDLE`).
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
        *lock(&self.control.disconnect_on) = command.map(str::to_ascii_uppercase);
    }

    /// Makes the server answer `command` (e.g. `"UID FETCH"`) with a tagged
    /// `NO` and no data. `None` switches failure injection off.
    pub fn fail_on(&self, command: Option<&str>) {
        *lock(&self.control.fail_on) = command.map(str::to_ascii_uppercase);
    }

    /// Lets `passing` commands whose line contains `pattern`
    /// (case-insensitive, e.g. `"BODY.PEEK[]"`) through, then drops the
    /// connection without answering the next one. Fires once.
    pub fn disconnect_after(&self, pattern: &str, passing: usize) {
        *lock(&self.control.disconnect_after) = Some((pattern.to_ascii_uppercase(), passing));
    }

    /// Makes `SELECT`/`EXAMINE` of these mailboxes fail with `NO [NOPERM]`
    /// (the mailboxes still appear in `LIST`, like folders an ACL forbids).
    pub fn deny_mailboxes(&self, mailboxes: &[&str]) {
        *lock(&self.control.denied_mailboxes) = mailboxes.iter().map(|m| (*m).to_owned()).collect();
    }

    /// A slow server: waits `delay` before answering `command`
    /// (case-insensitive, e.g. `"UID FETCH"`), or every command when
    /// `command` is `None`. [`Self::clear_delay`] switches it off.
    pub fn delay_responses(&self, command: Option<&str>, delay: Duration) {
        *lock(&self.control.delay) = Some((command.map(str::to_ascii_uppercase), delay));
    }

    /// Switches [`Self::delay_responses`] off.
    pub fn clear_delay(&self) {
        *lock(&self.control.delay) = None;
    }

    /// Drops every connection that is currently in `IDLE`, without a `BYE`
    /// (like a NAT timeout or a server restart). Returns how many sessions
    /// were idling.
    pub fn drop_idle_connections(&self) -> usize {
        let idling = self.idle_sessions();
        self.control.idle_kill.notify_waiters();
        idling
    }

    /// Number of sessions currently in `IDLE`.
    pub fn idle_sessions(&self) -> usize {
        self.control.idling.load(Ordering::SeqCst)
    }

    /// Waits until at least `count` sessions are in `IDLE`; `false` on
    /// timeout.
    pub async fn wait_for_idle(&self, count: usize, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, async {
            loop {
                let changed = self.control.idle_changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if self.idle_sessions() >= count {
                    return;
                }
                changed.await;
            }
        })
        .await
        .is_ok()
    }

    /// Replaces the accepted OAuth2 access token (`None`: reject all), e.g.
    /// to simulate an expired token.
    pub fn set_oauth_token(&self, token: Option<&str>) {
        *lock(&self.control.oauth_token) = token.map(str::to_owned);
    }

    /// Every command received so far, in order, as normalized names
    /// (`"LOGIN"`, `"AUTHENTICATE XOAUTH2"`, `"UID MOVE"`, `"IDLE"`, ...).
    pub fn commands(&self) -> Vec<String> {
        lock(&self.control.commands).clone()
    }

    /// How often `command` (case-insensitive) was received.
    pub fn command_count(&self, command: &str) -> usize {
        lock(&self.control.commands)
            .iter()
            .filter(|c| c.eq_ignore_ascii_case(command))
            .count()
    }

    /// Forgets the recorded commands.
    pub fn clear_commands(&self) {
        lock(&self.control.commands).clear();
    }

    /// The parameters of every `ID` command received (`NIL` values as `None`).
    pub fn client_ids(&self) -> Vec<ClientId> {
        lock(&self.control.client_ids).clone()
    }

    /// Number of sessions that switched on `COMPRESS=DEFLATE`.
    pub fn compressed_sessions(&self) -> usize {
        self.control.compressed.load(Ordering::SeqCst)
    }

    /// Number of accepted connections so far.
    pub fn connection_count(&self) -> usize {
        self.control.connections.load(Ordering::SeqCst)
    }

    /// Simulates an outage (`true`) or its end (`false`): while offline,
    /// new connections are closed right after they are accepted, sessions
    /// close on their next command and sessions in `IDLE` are dropped.
    pub fn set_offline(&self, offline: bool) {
        self.control.offline.store(offline, Ordering::SeqCst);
        if offline {
            self.control.idle_kill.notify_waiters();
        }
    }

    /// Whether [`Self::set_offline`] is on.
    pub fn is_offline(&self) -> bool {
        self.control.offline.load(Ordering::SeqCst)
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
                if shared.control.offline.load(Ordering::SeqCst) {
                    tracing::debug!(%peer, "mock IMAP offline: closing the connection");
                    drop(stream);
                    continue;
                }
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
