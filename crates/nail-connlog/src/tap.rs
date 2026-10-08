//! Recording: protocol clients hold a [`WireTap`] (one per log scope) and
//! open a [`ConnTap`] per connection or an [`HttpExchange`] per HTTP request.
//! Every text goes through the protocol's rules and [`crate::redact::scrub`]
//! here; protocol crates never format log lines themselves.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use nail_api::connlog::{CONNECTION_LOG_LINE_BYTES, WireDirection, WireLevel, WireProtocol};

use crate::Part;
use crate::imap::ImapState;
use crate::lines::{Pop3State, SieveState, SmtpState};
use crate::redact::{Secrets, cut, scrub};

/// One redacted entry on its way to a [`WireSink`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRecord {
    /// The protocol.
    pub protocol: WireProtocol,
    /// The connection or HTTP exchange.
    pub connection: u32,
    /// Sent, received or a note.
    pub direction: WireDirection,
    /// Normal, warning or error.
    pub level: WireLevel,
    /// The redacted text.
    pub text: String,
}

/// Where a [`WireTap`]'s records go (the core's per-scope ring).
pub trait WireSink: Send + Sync {
    /// Stores one record.
    fn record(&self, record: WireRecord);
}

struct Shared {
    sink: Arc<dyn WireSink>,
    secrets: Mutex<Secrets>,
}

/// A log scope's recorder (cheap to clone). [`WireTap::off`] records
/// nothing and costs nothing.
#[derive(Clone, Default)]
pub struct WireTap {
    shared: Option<Arc<Shared>>,
}

/// Taps are equal when they record into the same place (configurations
/// that hold one stay comparable).
impl PartialEq for WireTap {
    fn eq(&self, other: &Self) -> bool {
        match (&self.shared, &other.shared) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Eq for WireTap {}

impl fmt::Debug for WireTap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WireTap")
            .field("on", &self.is_on())
            .finish_non_exhaustive()
    }
}

/// Connection ids: unique while the process runs.
static NEXT_CONNECTION: AtomicU32 = AtomicU32::new(1);

fn next_connection() -> u32 {
    NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl WireTap {
    /// A tap that records nothing.
    pub fn off() -> Self {
        Self::default()
    }

    /// A tap recording into `sink`.
    pub fn new(sink: Arc<dyn WireSink>) -> Self {
        Self {
            shared: Some(Arc::new(Shared {
                sink,
                secrets: Mutex::new(Secrets::default()),
            })),
        }
    }

    /// Whether anything is recorded.
    pub fn is_on(&self) -> bool {
        self.shared.is_some()
    }

    /// Removes `secret` (a password, app password or token this scope signs
    /// in with) from everything recorded from now on.
    pub fn add_secret(&self, secret: &str) {
        if let Some(shared) = &self.shared {
            lock(&shared.secrets).add(secret);
        }
    }

    /// `text` as this scope would log it: registered secrets removed,
    /// secret-looking values and addresses redacted (for error details).
    pub fn scrub(&self, text: &str) -> String {
        match &self.shared {
            Some(shared) => scrub(text, &lock(&shared.secrets)),
            None => scrub(text, &Secrets::default()),
        }
    }

    /// Opens a connection's log: notes `connecting to host:port`.
    pub fn connect(&self, protocol: WireProtocol, host: &str, port: u16) -> ConnTap {
        let tap = ConnTap {
            tap: self.clone(),
            protocol,
            id: next_connection(),
            rules: Mutex::new(Rules::for_protocol(protocol)),
            started: Instant::now(),
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            last_sent: Mutex::new(None),
            last_received: Mutex::new(None),
            closed: AtomicBool::new(!self.is_on()),
        };
        tap.note(WireLevel::Info, &format!("connecting to {host}:{port}"));
        tap
    }

    /// The HTTP recorder of `protocol`.
    pub fn http(&self, protocol: WireProtocol) -> HttpTap {
        HttpTap {
            tap: self.clone(),
            protocol: Some(protocol),
        }
    }

    /// Scrubs, cuts and records one text; returns it as recorded.
    fn emit(
        &self,
        protocol: WireProtocol,
        connection: u32,
        direction: WireDirection,
        level: WireLevel,
        text: &str,
    ) -> Option<String> {
        let shared = self.shared.as_ref()?;
        let text = {
            let secrets = lock(&shared.secrets);
            cut(&scrub(text, &secrets), CONNECTION_LOG_LINE_BYTES)
        };
        shared.sink.record(WireRecord {
            protocol,
            connection,
            direction,
            level,
            text: text.clone(),
        });
        Some(text)
    }
}

/// The per-protocol line rules of a connection.
#[derive(Debug)]
enum Rules {
    Imap(ImapState),
    Smtp(SmtpState),
    Pop3(Pop3State),
    Sieve(SieveState),
    /// Protocols without line rules (HTTP): lines are not logged as text.
    Notes,
}

impl Rules {
    fn for_protocol(protocol: WireProtocol) -> Self {
        match protocol {
            WireProtocol::Imap => Self::Imap(ImapState::default()),
            WireProtocol::Smtp => Self::Smtp(SmtpState::default()),
            WireProtocol::Pop3 => Self::Pop3(Pop3State::default()),
            WireProtocol::ManageSieve => Self::Sieve(SieveState::default()),
            _ => Self::Notes,
        }
    }

    fn sent(&mut self, parts: &[Part<'_>]) -> String {
        match self {
            Self::Imap(state) => state.sent(parts),
            Self::Smtp(state) => state.sent(parts),
            Self::Pop3(state) => state.sent(parts),
            Self::Sieve(state) => state.sent(parts),
            Self::Notes => crate::imap::VALUE.to_owned(),
        }
    }

    fn received(&mut self, parts: &[Part<'_>]) -> String {
        match self {
            Self::Imap(state) => state.received(parts),
            Self::Smtp(state) => state.received(parts),
            Self::Pop3(state) => state.received(parts),
            Self::Sieve(state) => state.received(parts),
            Self::Notes => crate::imap::VALUE.to_owned(),
        }
    }
}

/// How notable a (redacted) response line is: refusals are warnings.
fn response_level(protocol: WireProtocol, text: &str) -> WireLevel {
    let mut words = text.split_whitespace();
    let first = words.next().unwrap_or_default();
    let refused = match protocol {
        // `BYE` is normal after `LOGOUT`; refusals are `NO` / `BAD`.
        WireProtocol::Imap => words
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case("NO") || s.eq_ignore_ascii_case("BAD")),
        WireProtocol::Smtp => first.starts_with('4') || first.starts_with('5'),
        WireProtocol::Pop3 => first.starts_with("-ERR"),
        WireProtocol::ManageSieve => first.eq_ignore_ascii_case("NO"),
        _ => false,
    };
    if refused {
        WireLevel::Warning
    } else {
        WireLevel::Info
    }
}

fn part_bytes(parts: &[Part<'_>]) -> u64 {
    parts
        .iter()
        .map(|p| match p {
            Part::Text(text) => u64::try_from(text.len()).unwrap_or(u64::MAX),
            Part::Bytes(n) => u64::try_from(*n).unwrap_or(u64::MAX),
        })
        .sum()
}

/// The log of one connection (IMAP, SMTP, POP3, ManageSieve). Logs its
/// close (duration, bytes) when dropped.
pub struct ConnTap {
    tap: WireTap,
    protocol: WireProtocol,
    id: u32,
    rules: Mutex<Rules>,
    started: Instant,
    sent: AtomicU64,
    received: AtomicU64,
    last_sent: Mutex<Option<String>>,
    last_received: Mutex<Option<String>>,
    closed: AtomicBool,
}

impl fmt::Debug for ConnTap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnTap")
            .field("protocol", &self.protocol)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl ConnTap {
    /// A connection log that records nothing.
    pub fn off(protocol: WireProtocol) -> Self {
        WireTap::off().connect(protocol, "", 0)
    }

    /// The connection id (`#12` in the log).
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Whether anything is recorded.
    pub fn is_on(&self) -> bool {
        self.tap.is_on()
    }

    /// Removes `secret` from everything recorded from now on (for the whole
    /// scope).
    pub fn add_secret(&self, secret: &str) {
        self.tap.add_secret(secret);
    }

    /// A line (or command with literals) Nail sent.
    pub fn sent(&self, parts: &[Part<'_>]) {
        if !self.is_on() {
            return;
        }
        self.sent.fetch_add(part_bytes(parts), Ordering::Relaxed);
        let text = lock(&self.rules).sent(parts);
        let logged = self.tap.emit(
            self.protocol,
            self.id,
            WireDirection::Sent,
            WireLevel::Info,
            &text,
        );
        *lock(&self.last_sent) = logged;
    }

    /// A response line (literals as [`Part::Bytes`]).
    pub fn received(&self, parts: &[Part<'_>]) {
        if !self.is_on() {
            return;
        }
        self.received
            .fetch_add(part_bytes(parts), Ordering::Relaxed);
        let text = lock(&self.rules).received(parts);
        let level = response_level(self.protocol, &text);
        let logged = self.tap.emit(
            self.protocol,
            self.id,
            WireDirection::Received,
            level,
            &text,
        );
        *lock(&self.last_received) = logged;
    }

    /// A note of Nail's own (scrubbed like everything else).
    pub fn note(&self, level: WireLevel, text: &str) {
        if self.is_on() {
            self.tap
                .emit(self.protocol, self.id, WireDirection::Info, level, text);
        }
    }

    /// The negotiated TLS parameters and the server certificate.
    pub fn tls(&self, version: &str, cipher: &str, peer_certificate: Option<&[u8]>) {
        if self.is_on() {
            self.note(
                WireLevel::Info,
                &crate::tls::note(version, cipher, peer_certificate),
            );
        }
    }

    /// The negotiated TLS version and cipher suite and the server
    /// certificate of a rustls session (IMAP, POP3, ManageSieve).
    pub fn tls_session(&self, session: &rustls::ClientConnection) {
        if !self.is_on() {
            return;
        }
        let version = session
            .protocol_version()
            .map(|v| format!("{v:?}"))
            .unwrap_or_default();
        let cipher = session
            .negotiated_cipher_suite()
            .map(|c| format!("{:?}", c.suite()))
            .unwrap_or_default();
        let certificate = session
            .peer_certificates()
            .and_then(|certs| certs.first())
            .map(AsRef::as_ref);
        self.tls(&version, &cipher, certificate);
    }

    /// The last line Nail sent, as logged (for error details).
    pub fn last_sent(&self) -> Option<String> {
        lock(&self.last_sent).clone()
    }

    /// The last line the server sent, as logged (for error details: the
    /// tagged `NO` / `BAD` of a failed command).
    pub fn last_received(&self) -> Option<String> {
        lock(&self.last_received).clone()
    }

    /// Notes the end of the connection (once): duration and bytes.
    pub fn close(&self, reason: Option<&str>) {
        if self.closed.swap(true, Ordering::Relaxed) {
            return;
        }
        let mut text = format!(
            "closed after {} ms, sent {} bytes, received {} bytes",
            self.started.elapsed().as_millis(),
            self.sent.load(Ordering::Relaxed),
            self.received.load(Ordering::Relaxed)
        );
        let level = if let Some(reason) = reason {
            text.push_str(": ");
            text.push_str(reason);
            WireLevel::Warning
        } else {
            WireLevel::Info
        };
        self.tap
            .emit(self.protocol, self.id, WireDirection::Info, level, &text);
    }
}

impl Drop for ConnTap {
    fn drop(&mut self) {
        self.close(None);
    }
}

/// The HTTP recorder of one protocol of a scope.
#[derive(Debug, Clone, Default)]
pub struct HttpTap {
    tap: WireTap,
    protocol: Option<WireProtocol>,
}

impl HttpTap {
    /// A recorder that records nothing.
    pub fn off() -> Self {
        Self::default()
    }

    /// Whether anything is recorded.
    pub fn is_on(&self) -> bool {
        self.tap.is_on() && self.protocol.is_some()
    }

    /// Removes `secret` (a token) from everything recorded from now on.
    pub fn add_secret(&self, secret: &str) {
        self.tap.add_secret(secret);
    }

    /// Logs a request line and returns the exchange to log its answer on.
    /// `authorization`: an `Authorization` header was sent (its value never
    /// is logged).
    pub fn request(
        &self,
        method: &str,
        url: &str,
        authorization: bool,
        body: Option<u64>,
    ) -> HttpExchange {
        let id = next_connection();
        let line = match self.protocol {
            Some(protocol) if self.tap.is_on() => {
                // Subscription URLs are secrets themselves (ADR 0190).
                let target = if protocol == WireProtocol::Feed {
                    crate::http::origin_only(url)
                } else {
                    url.to_owned()
                };
                self.tap.emit(
                    protocol,
                    id,
                    WireDirection::Sent,
                    WireLevel::Info,
                    &crate::http::request_line(method, &target, authorization, body),
                )
            }
            _ => None,
        };
        HttpExchange {
            tap: self.clone(),
            id,
            started: Instant::now(),
            line,
        }
    }

    /// A note of Nail's own (DNS lookups, redirects, retries).
    pub fn note(&self, level: WireLevel, text: &str) {
        if let Some(protocol) = self.protocol {
            self.tap.emit(
                protocol,
                next_connection(),
                WireDirection::Info,
                level,
                text,
            );
        }
    }
}

/// One HTTP request whose answer is still to be logged.
#[derive(Debug)]
pub struct HttpExchange {
    tap: HttpTap,
    id: u32,
    started: Instant,
    line: Option<String>,
}

impl HttpExchange {
    /// The request line as logged (for error details).
    pub fn request_line(&self) -> Option<&str> {
        self.line.as_deref()
    }

    /// Logs the answer: status, duration, body size and, for a failed
    /// answer, the error code / message of its body.
    pub fn response(&self, status: u16, body_len: Option<u64>, body: Option<&[u8]>) {
        let Some(protocol) = self.tap.protocol.filter(|_| self.tap.is_on()) else {
            return;
        };
        let error = if status >= 400 {
            body.and_then(crate::http::error_summary)
        } else {
            None
        };
        let line = crate::http::response_line(
            status,
            self.started.elapsed().as_millis(),
            body_len,
            error.as_deref(),
        );
        let level = match status {
            500.. => WireLevel::Error,
            400.. => WireLevel::Warning,
            _ => WireLevel::Info,
        };
        self.tap
            .tap
            .emit(protocol, self.id, WireDirection::Received, level, &line);
    }

    /// Logs a request that got no answer (connection, TLS, timeout).
    pub fn failed(&self, error: &str) {
        if let Some(protocol) = self.tap.protocol.filter(|_| self.tap.is_on()) {
            let text = format!(
                "failed after {} ms: {error}",
                self.started.elapsed().as_millis()
            );
            self.tap.tap.emit(
                protocol,
                self.id,
                WireDirection::Info,
                WireLevel::Error,
                &text,
            );
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Collects records.
    #[derive(Default)]
    pub(crate) struct Collect(pub(crate) Mutex<Vec<WireRecord>>);

    impl WireSink for Collect {
        fn record(&self, record: WireRecord) {
            lock(&self.0).push(record);
        }
    }

    impl Collect {
        pub(crate) fn texts(&self) -> Vec<String> {
            lock(&self.0).iter().map(|r| r.text.clone()).collect()
        }
    }

    #[test]
    fn a_connection_logs_lines_notes_and_its_close() {
        let sink = Arc::new(Collect::default());
        let tap = WireTap::new(sink.clone());
        tap.add_secret("hunter2-secret");
        {
            let conn = tap.connect(WireProtocol::Imap, "imap.example.org", 993);
            conn.tls("TLSv1_3", "TLS13_AES_256_GCM_SHA384", None);
            conn.received(&[Part::Text(
                "* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] ready\r\n",
            )]);
            conn.sent(&[Part::Text("A0001 LOGIN sam@example.net hunter2-secret")]);
            conn.received(&[Part::Text(
                "A0001 NO [AUTHENTICATIONFAILED] sam@example.net: hunter2-secret wrong",
            )]);
            assert_eq!(
                conn.last_sent().as_deref(),
                Some("A0001 LOGIN s***@example.net <redacted>")
            );
        }
        let texts = sink.texts();
        assert_eq!(texts[0], "connecting to imap.example.org:993");
        assert_eq!(texts[1], "TLS TLSv1_3 TLS13_AES_256_GCM_SHA384");
        assert_eq!(
            texts[4],
            "A0001 NO [AUTHENTICATIONFAILED] s***@example.net: <redacted> wrong"
        );
        assert!(texts[5].starts_with("closed after "), "{texts:?}");
        let records = lock(&sink.0);
        assert_eq!(records[4].level, WireLevel::Warning);
        assert_eq!(records[2].direction, WireDirection::Received);
        assert!(
            records
                .iter()
                .all(|r| r.connection == records[0].connection)
        );
    }

    #[test]
    fn http_exchanges() {
        let sink = Arc::new(Collect::default());
        let tap = WireTap::new(sink.clone()).http(WireProtocol::OAuth);
        tap.add_secret("1//refresh-token-value");
        let exchange = tap.request(
            "POST",
            "https://oauth2.googleapis.com/token",
            false,
            Some(300),
        );
        exchange.response(
            400,
            Some(80),
            Some(br#"{"error":"invalid_grant","error_description":"Bad 1//refresh-token-value"}"#),
        );
        let failed = tap.request("GET", "https://x.example/", true, None);
        failed.failed("connection refused");
        let texts = sink.texts();
        assert_eq!(
            texts[0],
            "POST https://oauth2.googleapis.com/token {300 bytes}"
        );
        assert!(texts[1].starts_with("400 · "), "{texts:?}");
        assert!(
            texts[1].ends_with("invalid_grant: Bad <redacted>"),
            "{texts:?}"
        );
        assert!(texts[3].contains("connection refused"));
        assert_eq!(exchange.request_line(), Some(texts[0].as_str()));
    }

    #[test]
    fn off_records_nothing() {
        let conn = ConnTap::off(WireProtocol::Smtp);
        conn.sent(&[Part::Text("AUTH PLAIN x")]);
        assert_eq!(conn.last_sent(), None);
        assert!(!HttpTap::off().is_on());
        HttpTap::off()
            .request("GET", "https://x", false, None)
            .response(200, None, None);
    }
}
