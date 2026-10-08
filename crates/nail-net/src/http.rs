//! The HTTP plumbing the API clients (Gmail, Microsoft Graph, EWS, CalDAV /
//! CardDAV) and the discovery / Autodiscover fetchers share: the URL policy,
//! the inline retry policy with `Retry-After`, bounded answer bodies,
//! transport error classes, bearer sign-in, request routing for mock
//! servers and the status codes they act on.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use nail_api::ErrorCode;
#[doc(hidden)]
pub use nail_api::ErrorCode as __ErrorCode;
use nail_api::accounts::SecretRef;
#[doc(hidden)]
pub use nail_api::engine::EngineError as __EngineError;
use nail_api::engine::{AccessTokenProvider, EngineError};
#[doc(hidden)]
pub use nail_api::localized::LocalizedMessage as __LocalizedMessage;
use nail_api::localized::LocalizedMessage;
use reqwest::header::HeaderMap;
#[doc(hidden)]
pub use serde_json as __serde_json;
use url::Url;

/// The status codes the clients act on and the redirect test (defined in
/// [`crate::status`], which needs no HTTP dependency).
pub use crate::status::{self, is_redirect};

/// Largest JSON / XML answer an API client reads (a 35 MB Gmail message
/// as base64url, a large CardDAV multiget with photos).
pub const MAX_ANSWER: usize = 64 * 1024 * 1024;

/// Largest answer of the Exchange APIs (Graph, EWS), which return whole
/// messages raw or as base64 (Exchange accepts messages up to 150 MB).
pub const MAX_EXCHANGE_ANSWER: usize = 160 * 1024 * 1024;

/// Most redirects followed for one request.
pub const MAX_REDIRECTS: usize = 5;

/// Whether a client may talk to `url`: `https:`, or `http:` to a loopback
/// host (mock servers; ADR 0057 / 0065 / 0072).
#[must_use]
pub fn is_allowed_url(url: &Url) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => match url.host() {
            Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        },
        _ => false,
    }
}

/// Whether two URLs share scheme, host and port (the origin a login or a
/// bearer token belongs to).
#[must_use]
pub fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme().eq_ignore_ascii_case(b.scheme())
        && a.host_str().map(str::to_ascii_lowercase) == b.host_str().map(str::to_ascii_lowercase)
        && a.port_or_known_default() == b.port_or_known_default()
}

/// The URL `scheme://host[:port]path` of an API on `host` (`https`, or
/// `http` when `tls` is false: loopback test servers only); the port is
/// left out when it is the scheme's default, an IPv6 literal gets its
/// brackets.
#[must_use]
pub fn origin_url(host: &str, port: u16, tls: bool, path: &str) -> String {
    let (scheme, default_port) = if tls {
        ("https", crate::ports::HTTPS)
    } else {
        ("http", crate::ports::HTTP)
    };
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    if port == default_port {
        format!("{scheme}://{host}{path}")
    } else {
        format!("{scheme}://{host}:{port}{path}")
    }
}

/// An API base URL without query, fragment and trailing empty segment
/// (`https://host/v1.0/` → `https://host/v1.0`).
#[must_use]
pub fn normalized_base(mut base: Url) -> Url {
    base.set_query(None);
    base.set_fragment(None);
    if let Ok(mut segments) = base.path_segments_mut() {
        segments.pop_if_empty();
    }
    base
}

/// Appends `query` pairs to `url`.
pub fn append_query(url: &mut Url, query: &[(&str, String)]) {
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in query {
            pairs.append_pair(k, v);
        }
    }
}

/// Inline retries of throttling and server errors: exponential backoff
/// from `base_delay` up to `max_delay` (with a little jitter), or the
/// server's `Retry-After`. A wait longer than `max_inline_wait` is not
/// slept inline: the error goes to the caller with the server's wait (the
/// core's loops wait instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt.
    pub max_retries: u32,
    /// The first backoff.
    pub base_delay: Duration,
    /// The longest backoff.
    pub max_delay: Duration,
    /// The longest wait slept inline.
    pub max_inline_wait: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 4,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(16),
            max_inline_wait: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// Millisecond backoffs, for tests.
    #[must_use]
    pub fn fast() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(2),
            max_delay: Duration::from_millis(10),
            max_inline_wait: Duration::from_secs(2),
        }
    }

    /// No inline retries at all.
    #[must_use]
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            ..Self::fast()
        }
    }

    /// The backoff before retry number `attempt` (0-based): doubling from
    /// the base, capped, plus up to a quarter of jitter.
    #[must_use]
    pub fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt.min(20)).unwrap_or(u32::MAX);
        let delay = self.base_delay.saturating_mul(factor).min(self.max_delay);
        let nanos = u32::try_from(nail_util::time::unix_now_nanos() % 1_000_000_000).unwrap_or(0);
        let jitter = delay / 4 * (nanos % 1000) / 1000;
        (delay + jitter).min(self.max_delay.max(delay))
    }

    /// The wait before retry `attempt` after an answer with `retry_after`
    /// seconds (or none); `None` when it is longer than the inline limit.
    #[must_use]
    pub fn wait(&self, attempt: u32, retry_after: Option<u32>) -> Option<Duration> {
        let wait = retry_after.map_or_else(
            || self.backoff(attempt),
            |s| Duration::from_secs(u64::from(s)),
        );
        (wait <= self.max_inline_wait).then_some(wait)
    }
}

/// The tries of one request under a [`RetryPolicy`]: counts the retries
/// and sleeps the waits between them.
#[derive(Debug, Clone)]
pub struct Retries {
    policy: RetryPolicy,
    attempt: u32,
}

impl Retries {
    /// The first try of a request.
    #[must_use]
    pub fn new(policy: RetryPolicy) -> Self {
        Self { policy, attempt: 0 }
    }

    /// Retries done so far.
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Whether a retry is left.
    #[must_use]
    pub fn left(&self) -> bool {
        self.attempt < self.policy.max_retries
    }

    /// After a broken connection: sleeps the backoff and answers `true`
    /// when a retry is left, else `false` (nothing slept).
    pub async fn backoff(&mut self) -> bool {
        if !self.left() {
            return false;
        }
        let wait = self.policy.backoff(self.attempt);
        self.sleep(wait).await;
        true
    }

    /// After an answer asking to retry (`retry_after` seconds, or none):
    /// the wait before the next try when a retry is left and the wait is
    /// within the inline limit ([`RetryPolicy::wait`]).
    #[must_use]
    pub fn wait(&self, retry_after: Option<u32>) -> Option<Duration> {
        if !self.left() {
            return None;
        }
        self.policy.wait(self.attempt, retry_after)
    }

    /// Whether a retry is left and `wait` is within the inline limit.
    #[must_use]
    pub fn allows(&self, wait: Duration) -> bool {
        self.left() && wait <= self.policy.max_inline_wait
    }

    /// Sleeps `wait` and counts the retry.
    pub async fn sleep(&mut self, wait: Duration) {
        tokio::time::sleep(wait).await;
        self.attempt += 1;
    }
}

/// The body of an API request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// No body (`Content-Length: 0` on a `POST`).
    None,
    /// A JSON document.
    Json(Vec<u8>),
    /// Bytes of another type.
    Raw {
        /// The `Content-Type`.
        content_type: String,
        /// The bytes.
        bytes: Vec<u8>,
    },
}

impl Body {
    /// `value` as a JSON body.
    pub fn json<T: serde::Serialize + ?Sized>(value: &T) -> Result<Self, serde_json::Error> {
        serde_json::to_vec(value).map(Self::Json)
    }

    /// Sets the body and its `Content-Type` on `request` (`method` decides
    /// whether an empty `POST` needs `Content-Length: 0`).
    pub fn apply(
        &self,
        request: reqwest::RequestBuilder,
        method: &reqwest::Method,
    ) -> reqwest::RequestBuilder {
        match self {
            Self::None if *method == reqwest::Method::POST => {
                request.header(reqwest::header::CONTENT_LENGTH, "0")
            }
            Self::None => request,
            Self::Json(bytes) => request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(bytes.clone()),
            Self::Raw {
                content_type,
                bytes,
            } => request
                .header(reqwest::header::CONTENT_TYPE, content_type.as_str())
                .body(bytes.clone()),
        }
    }
}

/// `request` with a sensitive (never logged) `Authorization` header;
/// `None` when `authorization` is not valid header text.
#[must_use]
pub fn with_authorization(
    request: reqwest::RequestBuilder,
    authorization: &str,
) -> Option<reqwest::RequestBuilder> {
    let mut value = reqwest::header::HeaderValue::from_str(authorization).ok()?;
    value.set_sensitive(true);
    Some(request.header(reqwest::header::AUTHORIZATION, value))
}

/// [`with_authorization`] plus `Accept: application/json`.
#[must_use]
pub fn json_request(
    request: reqwest::RequestBuilder,
    authorization: &str,
) -> Option<reqwest::RequestBuilder> {
    with_authorization(request, authorization)
        .map(|r| r.header(reqwest::header::ACCEPT, "application/json"))
}

/// Parses a JSON answer; the error names `what` was asked.
pub fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8], what: &str) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|e| format!("{what}: {e}"))
}

/// The `Retry-After` header in seconds (delta seconds or an HTTP date).
#[must_use]
pub fn retry_after_secs(headers: &HeaderMap) -> Option<u32> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    parse_retry_after(value, chrono::Utc::now())
}

/// Parses a `Retry-After` value (delta seconds or an HTTP date) relative to
/// `now`; a date in the past is `0`.
#[must_use]
pub fn parse_retry_after(value: &str, now: chrono::DateTime<chrono::Utc>) -> Option<u32> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(u32::try_from(secs).unwrap_or(u32::MAX));
    }
    let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let secs = (date.with_timezone(&chrono::Utc) - now)
        .num_seconds()
        .max(0);
    Some(u32::try_from(secs).unwrap_or(u32::MAX))
}

/// Why a request got no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The request timed out.
    Timeout,
    /// The TLS handshake failed (untrusted certificate, wrong name); the
    /// error's source chain.
    Tls(String),
    /// Any other network failure; the error's source chain.
    Network(String),
}

/// The English diagnostic (the source chain; `timed out`).
impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("timed out"),
            Self::Tls(chain) | Self::Network(chain) => f.write_str(chain),
        }
    }
}

/// An error and its sources as one text (`outer: inner: …`).
#[must_use]
pub fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut chain = String::new();
    let mut source: Option<&dyn std::error::Error> = Some(error);
    while let Some(e) = source {
        chain.push_str(&e.to_string());
        chain.push_str(": ");
        source = e.source();
    }
    chain.trim_end_matches(": ").to_owned()
}

/// Whether an error text names a TLS problem (certificate, handshake).
#[must_use]
pub fn mentions_tls(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("certificate") || lower.contains("tls") || lower.contains("handshake")
}

/// Classifies a reqwest error (the URL is left out; TLS problems are told
/// apart by the source chain).
#[must_use]
pub fn transport_error(error: reqwest::Error) -> TransportError {
    let error = error.without_url();
    if error.is_timeout() {
        return TransportError::Timeout;
    }
    let chain = error_chain(&error);
    if mentions_tls(&chain) {
        TransportError::Tls(chain)
    } else {
        TransportError::Network(chain)
    }
}

/// An API client's error type seen from the transport: the errors for a
/// broken connection ([`TransportError`]) and a too large answer. Implement
/// it with [`crate::transport_errors!`].
pub trait TransportFailure: From<TransportError> {
    /// The answer is larger than the client reads (`what` names it).
    fn too_large(what: String) -> Self;

    /// Whether the connection broke or timed out (no answer): worth a
    /// retry of an idempotent request.
    fn is_transport(&self) -> bool;

    /// The error for an answer body that could not be read (`what` names
    /// the answer).
    #[must_use]
    fn from_body(error: BodyError, what: &str) -> Self {
        match error {
            BodyError::TooLarge => Self::too_large(what.to_owned()),
            BodyError::Transport(e) => e.into(),
        }
    }
}

/// Implements `From<TransportError>` and [`TransportFailure`] for an API
/// client's error type with the variants `Timeout`, `Tls(String)`,
/// `Network(String)` and `TooLarge(String)`.
#[macro_export]
macro_rules! transport_errors {
    ($ty:ident) => {
        impl From<$crate::http::TransportError> for $ty {
            fn from(error: $crate::http::TransportError) -> Self {
                match error {
                    $crate::http::TransportError::Timeout => Self::Timeout,
                    $crate::http::TransportError::Tls(chain) => Self::Tls(chain),
                    $crate::http::TransportError::Network(chain) => Self::Network(chain),
                }
            }
        }

        impl $crate::http::TransportFailure for $ty {
            fn too_large(what: String) -> Self {
                Self::TooLarge(what)
            }

            fn is_transport(&self) -> bool {
                matches!(self, Self::Network(_) | Self::Timeout)
            }
        }
    };
}

/// Whether failures of this class are worth trying again later (a broken
/// connection, throttling, an unavailable server) — the rule of the API
/// clients' errors.
#[must_use]
pub fn retryable_class(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Network | ErrorCode::RateLimited | ErrorCode::Unavailable
    )
}

/// The [`EngineError`] of a client error: its class, English text,
/// retry hint, the server's wait and the user-facing text.
#[must_use]
pub fn engine_error(
    code: ErrorCode,
    message: String,
    retryable: bool,
    retry_after_secs: Option<u32>,
    localized: LocalizedMessage,
) -> EngineError {
    EngineError {
        code,
        message,
        retryable,
        localized: Some(Box::new(localized)),
        retry_after_secs,
    }
}

/// The user-facing text of an access-token error: the token manager's own
/// text, else `fallback`.
#[must_use]
pub fn token_text(error: &EngineError, fallback: &'static str) -> LocalizedMessage {
    error
        .localized
        .as_deref()
        .cloned()
        .unwrap_or_else(|| LocalizedMessage::new(fallback))
}

/// Defines `code()` (the error class for the UI) and `localized()` (the
/// user-facing text, Fluent ids `<prefix>-…`) of a client error with the
/// variants every HTTP client error has — `Network(_)`, `Timeout`,
/// `Protocol(_)`, `Tls(_)`, `AuthFailed`, `Forbidden(_)`, `NotFound(_)`,
/// `TooLarge(_)`, `Status { status, .. }`, `InsecureUrl(_)` and the token
/// variant `$token(Box<EngineError>)` — plus the arms for the client's own
/// variants.
#[macro_export]
macro_rules! client_error_classes {
    (
        $ty:ident, $prefix:literal, $token:ident, retry by class,
        codes { $($cpat:pat => $code:expr),* $(,)? }
        texts { $($tpat:pat => $text:expr),* $(,)? }
    ) => {
        $crate::client_error_classes!($ty, $prefix, $token,
            codes { $($cpat => $code),* }
            texts { $($tpat => $text),* }
        );

        impl $ty {
            /// Whether trying again later may succeed: the access-token
            /// error says so itself, others by their class
            /// ([`retryable_class`]($crate::http::retryable_class)).
            pub fn retryable(&self) -> bool {
                match self {
                    Self::$token(e) => e.retryable,
                    _ => $crate::http::retryable_class(self.code()),
                }
            }
        }
    };
    (
        $ty:ident, $prefix:literal, $token:ident,
        codes { $($cpat:pat => $code:expr),* $(,)? }
        texts { $($tpat:pat => $text:expr),* $(,)? }
    ) => {
        impl $ty {
            /// The error class for the UI.
            pub fn code(&self) -> $crate::http::__ErrorCode {
                match self {
                    Self::Network(_) | Self::Timeout | Self::Protocol(_) => {
                        $crate::http::__ErrorCode::Network
                    }
                    Self::Tls(_) => $crate::http::__ErrorCode::Tls,
                    Self::AuthFailed => $crate::http::__ErrorCode::AuthFailed,
                    Self::Forbidden(_) => $crate::http::__ErrorCode::Forbidden,
                    Self::NotFound(_) => $crate::http::__ErrorCode::NotFound,
                    Self::TooLarge(_) => $crate::http::__ErrorCode::TooLarge,
                    Self::Status { status, .. } if *status >= 500 => {
                        $crate::http::__ErrorCode::Unavailable
                    }
                    Self::Status { .. } => $crate::http::__ErrorCode::Rejected,
                    Self::InsecureUrl(_) => $crate::http::__ErrorCode::Config,
                    Self::$token(e) => e.code,
                    $($cpat => $code,)*
                }
            }

            /// The user-facing text.
            pub fn localized(&self) -> $crate::http::__LocalizedMessage {
                use $crate::http::__LocalizedMessage as Text;
                match self {
                    Self::Network(_) => Text::new(concat!($prefix, "-network")),
                    Self::Timeout => Text::new(concat!($prefix, "-timeout")),
                    Self::Tls(_) => Text::new(concat!($prefix, "-tls")),
                    Self::AuthFailed => Text::new(concat!($prefix, "-auth-failed")),
                    Self::Forbidden(_) => Text::new(concat!($prefix, "-forbidden")),
                    Self::NotFound(_) => Text::new(concat!($prefix, "-missing")),
                    Self::TooLarge(_) => Text::new(concat!($prefix, "-too-large")),
                    Self::Status { status, .. } => Text::new(concat!($prefix, "-http-status"))
                        .arg("status", i64::from(*status)),
                    Self::Protocol(_) => Text::new(concat!($prefix, "-bad-response")),
                    Self::InsecureUrl(_) => Text::new(concat!($prefix, "-insecure-url")),
                    $($tpat => $text,)*
                }
            }
        }
    };
}

/// Implements `From<$ty> for EngineError` for a client error with the
/// methods `code()`, `retryable()`, `localized()` and a variant
/// `$token(Box<EngineError>)` (an access-token error, passed on as it
/// is); `$retry_after(&err)` is the server's wait.
#[macro_export]
macro_rules! engine_error_from {
    ($ty:ident, $token:ident, $retry_after:expr) => {
        impl From<$ty> for $crate::http::__EngineError {
            fn from(err: $ty) -> Self {
                if let $ty::$token(inner) = err {
                    return *inner;
                }
                $crate::http::engine_error(
                    err.code(),
                    err.to_string(),
                    err.retryable(),
                    $retry_after(&err),
                    err.localized(),
                )
            }
        }
    };
}

/// Why an answer body could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyError {
    /// The body is larger than the limit (announced or while reading).
    TooLarge,
    /// The connection failed while reading.
    Transport(TransportError),
}

/// Reads an answer body of at most `max` bytes, in chunks, so an endless
/// chunked answer cannot fill the memory.
pub async fn read_limited(
    response: &mut reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, BodyError> {
    if response
        .content_length()
        .is_some_and(|l| l > u64::try_from(max).unwrap_or(u64::MAX))
    {
        return Err(BodyError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| BodyError::Transport(transport_error(e)))?
    {
        if body.len() + chunk.len() > max {
            return Err(BodyError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Keeps the credential of an `Authorization` value (`Bearer <token>`,
/// `Basic <pair>`, `Negotiate <blob>`) out of the connection log, also out
/// of a server's error text that repeats it (ADR 0190).
pub fn hide_credential(tap: &nail_connlog::HttpTap, authorization: &str) {
    if let Some((_, credential)) = authorization.split_once(' ') {
        tap.add_secret(credential);
    }
}

/// A sent request's answer: status, headers and body.
pub type Answer = (u16, HeaderMap, Vec<u8>);

/// Sends `request` and reads its answer body (at most `max` bytes; `what`
/// names the answer in errors), logging the answer or the failure on
/// `exchange` (the connection log, ADR 0190).
pub async fn send_logged<E: TransportFailure + fmt::Display>(
    request: reqwest::RequestBuilder,
    exchange: &nail_connlog::HttpExchange,
    max: usize,
    what: &str,
) -> Result<Answer, E> {
    let failed = |err: E| {
        exchange.failed(&err.to_string());
        err
    };
    let mut response = request
        .send()
        .await
        .map_err(|e| failed(E::from(transport_error(e))))?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = read_limited(&mut response, max)
        .await
        .map_err(|e| failed(E::from_body(e, what)))?;
    exchange.response(
        status,
        Some(u64::try_from(body.len()).unwrap_or(u64::MAX)),
        Some(&body),
    );
    Ok((status, headers, body))
}

/// The size the connection log shows for a request body.
#[must_use]
pub fn logged_size(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// Where a request for `url` goes: unchanged, or (with `route_all_to`)
/// over plain HTTP to that address with the original host for the `Host`
/// header ([`routed_to`]).
#[must_use]
pub fn route(
    url: &Url,
    route_all_to: Option<SocketAddr>,
    with_port: bool,
) -> (Url, Option<String>) {
    match route_all_to {
        None => (url.clone(), None),
        Some(addr) => {
            let (routed, host) = routed_to(url, addr, with_port);
            (routed, Some(host))
        }
    }
}

/// `url` sent over plain HTTP to `addr` instead (path and query kept), and
/// the original host for the `Host` header (`host:port` when the URL names
/// a port and `with_port`). For tests and end-to-end runs against mock
/// servers that route by host.
#[must_use]
pub fn routed_to(url: &Url, addr: SocketAddr, with_port: bool) -> (Url, String) {
    let mut routed = url.clone();
    let host = match url.port() {
        Some(port) if with_port => format!("{}:{port}", url.host_str().unwrap_or_default()),
        _ => url.host_str().unwrap_or_default().to_owned(),
    };
    // The setters only fail for URLs that cannot have a host.
    let _ = routed.set_scheme("http");
    let _ = routed.set_ip_host(addr.ip());
    let _ = routed.set_port(Some(addr.port()));
    (routed, host)
}

/// A response's headers as (lowercase name, value) pairs; values that are
/// not visible ASCII are left out.
#[must_use]
pub fn header_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.as_str().to_owned(), v.to_owned()))
        })
        .collect()
}

/// Appends a header to a list of (name, value) pairs (the builders of
/// request / response values).
pub fn push_header(headers: &mut Vec<(String, String)>, name: &str, value: impl Into<String>) {
    headers.push((name.to_owned(), value.into()));
}

/// `Authorization: Basic <base64(username:password)>` (RFC 7617).
#[must_use]
pub fn basic(username: &str, password: &str) -> String {
    use base64::Engine as _;
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    format!("Basic {encoded}")
}

/// `Authorization: Bearer <token>`.
#[must_use]
pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// A bearer `Authorization` value from a token provider (`force_refresh`
/// asks for a new token after the server refused the last one).
pub async fn oauth2_bearer(
    tokens: &dyn AccessTokenProvider,
    provider: &str,
    secret: &SecretRef,
    force_refresh: bool,
) -> Result<String, EngineError> {
    let token = tokens.access_token(provider, secret, force_refresh).await?;
    Ok(bearer(&token))
}

/// How an OAuth2 API client (Gmail, Microsoft Graph) signs in.
#[derive(Clone)]
pub enum ApiAuth {
    /// A fixed bearer token (tests).
    Bearer(String),
    /// OAuth2 bearer tokens from the core's token manager, refreshed once
    /// when the API answers `401`.
    OAuth2 {
        /// The token source.
        tokens: Arc<dyn AccessTokenProvider>,
        /// The OAuth2 provider id (`google`, `microsoft`).
        provider: String,
        /// Where the refresh token is stored.
        secret: SecretRef,
    },
}

impl ApiAuth {
    /// Whether a refused token is worth one forced refresh.
    #[must_use]
    pub fn can_refresh(&self) -> bool {
        matches!(self, Self::OAuth2 { .. })
    }

    /// The `Authorization` value.
    pub async fn authorization(&self, force_refresh: bool) -> Result<String, EngineError> {
        match self {
            Self::Bearer(token) => Ok(bearer(token)),
            Self::OAuth2 {
                tokens,
                provider,
                secret,
            } => oauth2_bearer(tokens.as_ref(), provider, secret, force_refresh).await,
        }
    }
}

/// Never prints a token.
impl fmt::Debug for ApiAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer(_) => debug_bearer(f),
            Self::OAuth2 { provider, .. } => debug_oauth2(f, provider),
        }
    }
}

/// Implements `Debug` for a login enum with the variants `None`,
/// `Basic { username, .. }`, `Bearer(_)` and `OAuth2 { provider, .. }`
/// (plus the extra arms given): secrets never show.
#[macro_export]
macro_rules! login_debug {
    ($ty:ident $(, $pattern:pat => $text:expr)* $(,)?) => {
        impl ::std::fmt::Debug for $ty {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    Self::None => f.write_str("None"),
                    Self::Basic { username, .. } => $crate::http::debug_basic(f, username),
                    Self::Bearer(_) => $crate::http::debug_bearer(f),
                    Self::OAuth2 { provider, .. } => $crate::http::debug_oauth2(f, provider),
                    $($pattern => $text(f),)*
                }
            }
        }
    };
}

/// The text of a failed answer to `what`: `what: <the service's message>`,
/// or `what` alone when the service sent no (or an empty) message.
#[must_use]
pub fn answer_text(what: &str, message: Option<&str>) -> String {
    match message.filter(|m| !m.is_empty()) {
        Some(m) => format!("{what}: {m}"),
        None => what.to_owned(),
    }
}

/// The `Debug` text of a bearer-token login (the token never shows).
pub fn debug_bearer(f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("Bearer(..)")
}

/// The `Debug` text of an OAuth2 login (only the provider shows).
pub fn debug_oauth2(f: &mut fmt::Formatter<'_>, provider: &str) -> fmt::Result {
    f.debug_struct("OAuth2")
        .field("provider", &provider)
        .finish_non_exhaustive()
}

/// The `Debug` text of a Basic login (only the user name shows).
pub fn debug_basic(f: &mut fmt::Formatter<'_>, username: &str) -> fmt::Result {
    f.debug_struct("Basic")
        .field("username", &username)
        .finish_non_exhaustive()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    #[test]
    fn header_pairs_keep_text_values() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Retry-After",
            reqwest::header::HeaderValue::from_static("5"),
        );
        headers.insert(
            "x-bytes",
            reqwest::header::HeaderValue::from_bytes(b"\xff").unwrap(),
        );
        assert_eq!(
            header_pairs(&headers),
            [("retry-after".to_owned(), "5".to_owned())]
        );
    }

    #[test]
    fn answer_texts_name_the_call() {
        assert_eq!(answer_text("users.get", Some("gone")), "users.get: gone");
        assert_eq!(answer_text("users.get", Some("")), "users.get");
        assert_eq!(answer_text("users.get", None), "users.get");
    }

    #[test]
    fn origin_urls_leave_out_default_ports() {
        assert_eq!(
            origin_url("api.example", 443, true, "/v1"),
            "https://api.example/v1"
        );
        assert_eq!(
            origin_url("127.0.0.1", 8080, false, "/v1"),
            "http://127.0.0.1:8080/v1"
        );
        assert_eq!(origin_url("127.0.0.1", 80, false, ""), "http://127.0.0.1");
        assert_eq!(origin_url("::1", 4000, false, "/x"), "http://[::1]:4000/x");
        assert_eq!(origin_url("[::1]", 443, true, "/x"), "https://[::1]/x");
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ClientError {
        Timeout,
        Tls(String),
        Network(String),
        TooLarge(String),
    }

    crate::transport_errors!(ClientError);

    #[test]
    fn transport_errors_map_to_the_client_error() {
        assert_eq!(
            ClientError::from(TransportError::Timeout),
            ClientError::Timeout
        );
        assert_eq!(
            ClientError::from(TransportError::Tls("cert".into())),
            ClientError::Tls("cert".into())
        );
        let network = ClientError::from(TransportError::Network("reset".into()));
        assert!(network.is_transport() && ClientError::Timeout.is_transport());
        assert!(!ClientError::Tls(String::new()).is_transport());
        assert_eq!(
            ClientError::from_body(BodyError::TooLarge, "answer to GET"),
            ClientError::TooLarge("answer to GET".into())
        );
        assert_eq!(
            ClientError::from_body(BodyError::Transport(TransportError::Timeout), "x"),
            ClientError::Timeout
        );
    }

    #[test]
    fn url_policy() {
        for ok in [
            "https://graph.microsoft.com/v1.0",
            "http://127.0.0.1:8080/x",
            "http://localhost/x",
            "http://[::1]:9/x",
        ] {
            assert!(is_allowed_url(&Url::parse(ok).unwrap()), "{ok}");
        }
        for bad in [
            "http://graph.microsoft.com/v1.0",
            "http://10.0.0.1/x",
            "ftp://graph.microsoft.com/",
            "file:///c:/x",
        ] {
            assert!(!is_allowed_url(&Url::parse(bad).unwrap()), "{bad}");
        }
        let a = Url::parse("https://graph.microsoft.com/v1.0").unwrap();
        assert!(same_origin(
            &a,
            &Url::parse("https://graph.microsoft.com:443/beta").unwrap()
        ));
        assert!(!same_origin(
            &a,
            &Url::parse("http://graph.microsoft.com/v1.0").unwrap()
        ));
        let base = normalized_base(Url::parse("http://127.0.0.1:1/v1.0/?a=b#c").unwrap());
        assert_eq!(base.as_str(), "http://127.0.0.1:1/v1.0");
        let mut url = base;
        append_query(&mut url, &[("$select", "id,isRead".into())]);
        assert_eq!(url.query(), Some("%24select=id%2CisRead"));
    }

    #[test]
    fn retry_after_values() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
        assert_eq!(parse_retry_after("120", now), Some(120));
        assert_eq!(parse_retry_after(" 5 ", now), Some(5));
        assert_eq!(
            parse_retry_after("Thu, 01 Oct 2026 12:01:30 GMT", now),
            Some(90)
        );
        assert_eq!(
            parse_retry_after("Thu, 01 Oct 2026 11:00:00 GMT", now),
            Some(0)
        );
        assert_eq!(parse_retry_after("99999999999", now), Some(u32::MAX));
        assert_eq!(parse_retry_after("soon", now), None);
        let mut headers = HeaderMap::new();
        assert_eq!(retry_after_secs(&headers), None);
        headers.insert(reqwest::header::RETRY_AFTER, "7".parse().unwrap());
        assert_eq!(retry_after_secs(&headers), Some(7));
    }

    #[test]
    fn backoff_and_inline_waits() {
        let policy = RetryPolicy {
            max_retries: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
            max_inline_wait: Duration::from_secs(5),
        };
        let first = policy.backoff(0);
        assert!(first >= Duration::from_millis(100) && first <= Duration::from_millis(125));
        let third = policy.backoff(2);
        assert!(third >= Duration::from_millis(400) && third <= Duration::from_millis(500));
        assert_eq!(policy.backoff(10), Duration::from_secs(1));
        assert_eq!(policy.backoff(u32::MAX), Duration::from_secs(1));
        assert_eq!(policy.wait(0, Some(3)), Some(Duration::from_secs(3)));
        assert_eq!(policy.wait(0, Some(60)), None);
        assert!(policy.wait(1, None).is_some());
        assert_eq!(RetryPolicy::none().max_retries, 0);
        assert_eq!(RetryPolicy::default().max_retries, 4);
        assert_eq!(RetryPolicy::fast().max_retries, 3);
    }

    #[test]
    fn error_texts() {
        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl fmt::Display for Outer {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("request failed")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let error = Outer(std::io::Error::other("invalid peer certificate"));
        let chain = error_chain(&error);
        assert_eq!(chain, "request failed: invalid peer certificate");
        assert!(mentions_tls(&chain));
        assert!(mentions_tls("TLS handshake eof"));
        assert!(!mentions_tls("connection refused"));
        assert_eq!(TransportError::Timeout.to_string(), "timed out");
        assert_eq!(
            TransportError::Tls("bad cert".into()).to_string(),
            "bad cert"
        );
    }

    #[expect(dead_code, reason = "the password is never printed, so never read")]
    enum Login {
        None,
        Basic { username: String, password: String },
        Bearer(String),
        OAuth2 { provider: String },
        Other(u8),
    }

    crate::login_debug!(Login, Self::Other(n) => |f: &mut fmt::Formatter<'_>| write!(f, "Other({n})"));

    #[test]
    fn logins_never_print_secrets() {
        let basic = Login::Basic {
            username: "ada".into(),
            password: "hunter2".into(),
        };
        let text = format!("{basic:?}");
        assert!(text.contains("ada") && !text.contains("hunter2"), "{text}");
        assert_eq!(format!("{:?}", Login::Bearer("tok".into())), "Bearer(..)");
        assert_eq!(format!("{:?}", Login::None), "None");
        assert!(
            format!(
                "{:?}",
                Login::OAuth2 {
                    provider: "google".into()
                }
            )
            .contains("google")
        );
        assert_eq!(format!("{:?}", Login::Other(3)), "Other(3)");
    }

    #[test]
    fn routing_keeps_the_host() {
        let addr: SocketAddr = "127.0.0.1:4000".parse().unwrap();
        let url = Url::parse("https://mail.example.org:8443/a").unwrap();
        assert_eq!(route(&url, None, true), (url.clone(), None));
        let (routed, host) = route(&url, Some(addr), true);
        assert_eq!(routed.as_str(), "http://127.0.0.1:4000/a");
        assert_eq!(host.as_deref(), Some("mail.example.org:8443"));
    }

    #[test]
    fn routed_urls() {
        let addr: SocketAddr = "127.0.0.1:4000".parse().unwrap();
        let url = Url::parse("https://mail.example.org:8443/a?b=c").unwrap();
        let (routed, host) = routed_to(&url, addr, true);
        assert_eq!(routed.as_str(), "http://127.0.0.1:4000/a?b=c");
        assert_eq!(host, "mail.example.org:8443");
        assert_eq!(routed_to(&url, addr, false).1, "mail.example.org");
        let plain = Url::parse("https://mail.example.org/").unwrap();
        assert_eq!(routed_to(&plain, addr, true).1, "mail.example.org");
    }

    #[test]
    fn auth_debug_hides_secrets() {
        let text = format!("{:?}", ApiAuth::Bearer("secret-token".into()));
        assert!(!text.contains("secret-token"));
        assert_eq!(text, "Bearer(..)");
        assert!(!ApiAuth::Bearer(String::new()).can_refresh());
        assert_eq!(bearer("t"), "Bearer t");
        let mut headers = Vec::new();
        push_header(&mut headers, "Prefer", "x");
        assert_eq!(headers, [("Prefer".to_owned(), "x".to_owned())]);
        assert_eq!(
            basic("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }

    #[tokio::test]
    async fn retries_count_and_stop() {
        let mut tries = Retries::new(RetryPolicy::fast());
        assert!(tries.left());
        assert!(tries.wait(Some(1)).is_some());
        assert_eq!(tries.wait(Some(60)), None);
        assert!(tries.allows(Duration::from_secs(2)));
        assert!(!tries.allows(Duration::from_secs(3)));
        for _ in 0..3 {
            assert!(tries.backoff().await);
        }
        assert_eq!(tries.attempt(), 3);
        assert!(!tries.backoff().await);
        assert_eq!(tries.wait(None), None);
        let mut once = Retries::new(RetryPolicy::none());
        assert!(!once.backoff().await);
        once.sleep(Duration::ZERO).await;
        assert_eq!(once.attempt(), 1);
    }

    #[test]
    fn bodies_and_json() {
        assert_eq!(
            Body::json(&serde_json::json!({"a": 1})).unwrap(),
            Body::Json(br#"{"a":1}"#.to_vec())
        );
        let parsed: serde_json::Value = parse_json(b"[1]", "x").unwrap();
        assert_eq!(parsed, serde_json::json!([1]));
        let err = parse_json::<serde_json::Value>(b"{", "GET /me").unwrap_err();
        assert!(err.starts_with("GET /me: "), "{err}");
        let tls = crate::tls::client_config(&crate::tls::TlsTrust::WebPkiRoots).unwrap();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .build()
            .unwrap();
        let request = json_request(client.get("http://127.0.0.1:1/"), "Bearer t")
            .unwrap()
            .build()
            .unwrap();
        assert!(request.headers()[reqwest::header::AUTHORIZATION].is_sensitive());
        assert_eq!(
            request.headers()[reqwest::header::ACCEPT],
            "application/json"
        );
        assert!(json_request(client.get("http://127.0.0.1:1/"), "a\nb").is_none());
        let post = Body::None
            .apply(client.post("http://127.0.0.1:1/"), &reqwest::Method::POST)
            .build()
            .unwrap();
        assert_eq!(post.headers()[reqwest::header::CONTENT_LENGTH], "0");
        let raw = Body::Raw {
            content_type: "text/plain".into(),
            bytes: b"x".to_vec(),
        }
        .apply(client.post("http://127.0.0.1:1/"), &reqwest::Method::POST)
        .build()
        .unwrap();
        assert_eq!(raw.headers()[reqwest::header::CONTENT_TYPE], "text/plain");
    }

    #[expect(
        unreachable_pub,
        reason = "the macro defines public methods for the client crates that use it"
    )]
    mod failure {
        use super::{EngineError, ErrorCode, LocalizedMessage, token_text};

        #[expect(dead_code, reason = "the variants' texts are never read")]
        #[derive(Debug)]
        pub(super) enum Failure {
            Network(String),
            Timeout,
            Protocol(String),
            Tls(String),
            AuthFailed,
            Forbidden(String),
            NotFound(String),
            TooLarge(String),
            Status { status: u16, message: String },
            InsecureUrl(String),
            Token(Box<EngineError>),
            Busy,
        }

        crate::client_error_classes!(Failure, "x", Token,
            codes { Self::Busy => ErrorCode::RateLimited }
            texts {
                Self::Token(e) => token_text(e, "x-token"),
                Self::Busy => LocalizedMessage::new("x-busy"),
            }
        );
    }

    use failure::Failure;

    #[test]
    fn client_error_classes() {
        let cases = [
            (
                Failure::Network(String::new()),
                ErrorCode::Network,
                "x-network",
            ),
            (Failure::Timeout, ErrorCode::Network, "x-timeout"),
            (
                Failure::Protocol(String::new()),
                ErrorCode::Network,
                "x-bad-response",
            ),
            (Failure::Tls(String::new()), ErrorCode::Tls, "x-tls"),
            (Failure::AuthFailed, ErrorCode::AuthFailed, "x-auth-failed"),
            (
                Failure::Forbidden(String::new()),
                ErrorCode::Forbidden,
                "x-forbidden",
            ),
            (
                Failure::NotFound(String::new()),
                ErrorCode::NotFound,
                "x-missing",
            ),
            (
                Failure::TooLarge(String::new()),
                ErrorCode::TooLarge,
                "x-too-large",
            ),
            (
                Failure::InsecureUrl(String::new()),
                ErrorCode::Config,
                "x-insecure-url",
            ),
            (Failure::Busy, ErrorCode::RateLimited, "x-busy"),
        ];
        for (failure, code, id) in cases {
            assert_eq!(failure.code(), code, "{failure:?}");
            assert_eq!(failure.localized().id, id);
        }
        let server = Failure::Status {
            status: 503,
            message: String::new(),
        };
        assert_eq!(server.code(), ErrorCode::Unavailable);
        let text = server.localized();
        assert_eq!(text.id, "x-http-status");
        assert!(text.args.contains_key("status"));
        let client = Failure::Status {
            status: 418,
            message: String::new(),
        };
        assert_eq!(client.code(), ErrorCode::Rejected);
        let token = Failure::Token(Box::new(EngineError::permanent(ErrorCode::Config, "x")));
        assert_eq!(token.code(), ErrorCode::Config);
        assert_eq!(token.localized().id, "x-token");
        let own = EngineError::permanent(ErrorCode::Config, "x")
            .with_localized(LocalizedMessage::new("own"));
        assert_eq!(token_text(&own, "x-token").id, "own");
    }

    #[test]
    fn engine_errors() {
        assert!(retryable_class(ErrorCode::Network));
        assert!(retryable_class(ErrorCode::RateLimited));
        assert!(retryable_class(ErrorCode::Unavailable));
        assert!(!retryable_class(ErrorCode::AuthFailed));
        let error = engine_error(
            ErrorCode::RateLimited,
            "slow".into(),
            true,
            Some(3),
            LocalizedMessage::new("x-slow"),
        );
        assert_eq!(error.code, ErrorCode::RateLimited);
        assert_eq!(error.message, "slow");
        assert!(error.retryable);
        assert_eq!(error.retry_after_secs, Some(3));
        assert_eq!(error.localized.unwrap().id, "x-slow");
    }

    #[tokio::test]
    async fn fixed_bearer_authorization() {
        let auth = ApiAuth::Bearer("abc".into());
        assert_eq!(auth.authorization(true).await.unwrap(), "Bearer abc");
    }
}
