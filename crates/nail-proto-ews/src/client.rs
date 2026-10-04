//! The HTTP side of EWS: the URL policy, the sign-in (Basic, OAuth2 bearer
//! with one refresh, the Negotiate handshake bound to one kept-alive
//! connection), `RequestServerVersion` with a fallback for older servers,
//! `X-AnchorMailbox`, throttling (`ErrorServerBusy` with
//! `BackOffMilliseconds`, HTTP `429` / `503` with `Retry-After`), response
//! size limits and the long-running `GetStreamingEvents` answers.
//!
//! Requests go through **lanes**: one HTTP client with at most one idle
//! connection each. With Negotiate (NTLM binds the login to its TCP
//! connection) a lane's requests are serialized and the handshake is
//! repeated when the server asks again (`401` on a fresh connection). The
//! main lane carries ordinary calls; streaming notifications get their own
//! lane so a waiting `GetStreamingEvents` never blocks sync.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use nail_api::ErrorCode;
use nail_api::exchange::engine::{NegotiateStep, http_spn};
use reqwest::header::{HeaderMap, HeaderValue};
use tokio::sync::OwnedMutexGuard;
use url::Url;

use crate::auth::{EwsAuth, HeaderSource, Password, negotiate_challenge};
use crate::error::EwsError;
use crate::soap::{
    self, Answer, DEFAULT_SERVER_VERSION, FALLBACK_SERVER_VERSION, ResponseMessage, ServerVersion,
    SoapResponse,
};

/// Largest answer read (a 100 MB message as base64 `MimeContent` stays
/// below).
pub const MAX_RESPONSE: usize = 160 * 1024 * 1024;

/// How long an ordinary call may take.
pub const CALL_TIMEOUT: Duration = Duration::from_mins(2);

/// Inline retries of throttling and server errors: exponential backoff
/// from `base_delay` up to `max_delay`, or the server's
/// `BackOffMilliseconds` / `Retry-After`. A wait longer than
/// `max_inline_wait` is not slept here: the error goes to the caller with
/// [`EwsError::retry_after_secs`] (the core's loop waits instead).
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
    pub fn fast() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(2),
            max_delay: Duration::from_millis(10),
            max_inline_wait: Duration::from_secs(2),
        }
    }

    /// No inline retries at all.
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            ..Self::fast()
        }
    }

    /// The backoff before retry number `attempt` (0-based): doubling from
    /// the base, capped, plus up to a quarter of jitter.
    pub fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt.min(20)).unwrap_or(u32::MAX);
        let delay = self.base_delay.saturating_mul(factor).min(self.max_delay);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let jitter = delay / 4 * (nanos % 1000) / 1000;
        (delay + jitter).min(self.max_delay.max(delay))
    }
}

/// Whether the client may talk to `url`: `https:`, or `http:` to a
/// loopback host (the mock server; ADR 0057 / 0072).
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

/// One EWS operation as the client sends it.
#[derive(Debug, Clone)]
pub struct Operation<'a> {
    /// The operation element (`<m:GetItem>…</m:GetItem>`).
    pub body: String,
    /// `X-AnchorMailbox`: the mailbox the call is about (another mailbox;
    /// `None` = the account's own address, if known).
    pub anchor: Option<&'a str>,
    /// Safe to repeat after a broken connection or a server error (reads,
    /// absolute flag writes).
    pub idempotent: bool,
    /// The operation's name, for errors and logs.
    pub what: &'static str,
    /// `TimeZoneContext` (a Windows zone id) for wall times in the request.
    pub time_zone: Option<&'a str>,
}

impl<'a> Operation<'a> {
    /// A read (idempotent) operation.
    pub fn read(what: &'static str, body: String) -> Self {
        Self {
            body,
            anchor: None,
            idempotent: true,
            what,
            time_zone: None,
        }
    }

    /// A write (not repeated after a broken connection).
    pub fn write(what: &'static str, body: String) -> Self {
        Self {
            idempotent: false,
            ..Self::read(what, body)
        }
    }

    /// About another mailbox.
    #[must_use]
    pub fn anchored(mut self, mailbox: Option<&'a str>) -> Self {
        self.anchor = mailbox;
        self
    }

    /// With a `TimeZoneContext`.
    #[must_use]
    pub fn in_zone(mut self, zone: Option<&'a str>) -> Self {
        self.time_zone = zone;
        self
    }
}

/// One HTTP client with its own connection (see the module docs).
#[derive(Debug)]
struct Lane {
    http: reqwest::Client,
    /// Negotiate: whether the lane's connection is signed in. Locked for
    /// the whole request so the handshake and the call share the
    /// connection.
    negotiated: Arc<tokio::sync::Mutex<bool>>,
}

impl Lane {
    fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            negotiated: Arc::new(tokio::sync::Mutex::new(false)),
        }
    }
}

#[derive(Debug)]
struct State {
    version: &'static str,
    server_version: Option<ServerVersion>,
}

#[derive(Debug)]
struct Inner {
    url: Url,
    spn: String,
    auth: EwsAuth,
    headers: HeaderSource,
    retry: RetryPolicy,
    main: Lane,
    stream: Lane,
    own_address: Option<String>,
    state: Mutex<State>,
}

/// An EWS client for one account (cheap to clone).
#[derive(Debug, Clone)]
pub struct EwsClient {
    inner: Arc<Inner>,
}

type Sent = (reqwest::Response, Option<OwnedMutexGuard<bool>>);

impl EwsClient {
    /// A client on Nail's TLS configuration (rustls with ring and the
    /// platform verifier, no redirects) for the endpoint `url`
    /// (`https://mail.corp.example/EWS/Exchange.asmx`).
    pub fn new(url: &str, auth: EwsAuth) -> Result<Self, EwsError> {
        let main = tls_lane(CALL_TIMEOUT)?;
        let stream = tls_lane(Duration::from_mins(40))?;
        Self::with_http(main, stream, url, auth)
    }

    /// A client on existing HTTP clients (they must not follow redirects;
    /// build them with `nail_auth::http_client_builder`). `stream` serves
    /// streaming notifications and needs a timeout of at least half an
    /// hour.
    pub fn with_http(
        main: reqwest::Client,
        stream: reqwest::Client,
        url: &str,
        auth: EwsAuth,
    ) -> Result<Self, EwsError> {
        let mut parsed = Url::parse(url).map_err(|_| EwsError::InsecureUrl(url.to_owned()))?;
        if !is_allowed_url(&parsed) || parsed.host_str().is_none_or(str::is_empty) {
            return Err(EwsError::InsecureUrl(url.to_owned()));
        }
        parsed.set_fragment(None);
        let spn = http_spn(parsed.host_str().unwrap_or_default());
        Ok(Self {
            inner: Arc::new(Inner {
                url: parsed,
                spn,
                auth,
                headers: HeaderSource::default(),
                retry: RetryPolicy::default(),
                main: Lane::new(main),
                stream: Lane::new(stream),
                own_address: None,
                state: Mutex::new(State {
                    version: DEFAULT_SERVER_VERSION,
                    server_version: None,
                }),
            }),
        })
    }

    fn rebuild(self, change: impl FnOnce(&mut Inner)) -> Self {
        let mut inner = Arc::try_unwrap(self.inner).unwrap_or_else(|shared| {
            let state = {
                let current = shared.state();
                State {
                    version: current.version,
                    server_version: current.server_version.clone(),
                }
            };
            Inner {
                url: shared.url.clone(),
                spn: shared.spn.clone(),
                auth: shared.auth.clone(),
                headers: HeaderSource::default(),
                retry: shared.retry,
                main: Lane::new(shared.main.http.clone()),
                stream: Lane::new(shared.stream.http.clone()),
                own_address: shared.own_address.clone(),
                state: Mutex::new(state),
            }
        });
        change(&mut inner);
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Replaces the retry policy.
    #[must_use]
    pub fn with_retry(self, retry: RetryPolicy) -> Self {
        self.rebuild(|i| i.retry = retry)
    }

    /// The account's own address: the default `X-AnchorMailbox` and the
    /// mailbox of settings calls.
    #[must_use]
    pub fn with_own_address(self, address: impl Into<String>) -> Self {
        let address = address.into();
        self.rebuild(|i| i.own_address = Some(address))
    }

    /// Starts with this `RequestServerVersion` instead of
    /// [`DEFAULT_SERVER_VERSION`].
    #[must_use]
    pub fn with_request_version(self, version: &'static str) -> Self {
        self.rebuild(|i| i.state().version = version)
    }

    /// The endpoint.
    pub fn url(&self) -> &Url {
        &self.inner.url
    }

    /// The sign-in.
    pub fn auth(&self) -> &EwsAuth {
        &self.inner.auth
    }

    /// The retry policy.
    pub fn retry_policy(&self) -> RetryPolicy {
        self.inner.retry
    }

    /// The account's own address, if set.
    pub fn own_address(&self) -> Option<&str> {
        self.inner.own_address.as_deref()
    }

    /// The `RequestServerVersion` in use.
    pub fn request_version(&self) -> &'static str {
        self.inner.state().version
    }

    /// The last `ServerVersionInfo` the server sent.
    pub fn server_version(&self) -> Option<ServerVersion> {
        self.inner.state().server_version.clone()
    }

    /// Sends one operation and answers its parsed response: inline
    /// retries per the [`RetryPolicy`], a `RequestServerVersion` fallback,
    /// faults and HTTP errors as [`EwsError`]s. Response messages are
    /// left to the caller (except `ErrorServerBusy`, which is retried).
    pub async fn call(&self, op: &Operation<'_>) -> Result<SoapResponse, EwsError> {
        let retry = self.inner.retry;
        let mut attempt = 0u32;
        let mut downgraded = false;
        loop {
            let envelope = soap::envelope_with(self.request_version(), op.time_zone, &op.body);
            let sent = self
                .send(&self.inner.main, &envelope, op, CALL_TIMEOUT)
                .await;
            let (status, headers, text) = match sent {
                Ok((response, guard)) => {
                    let status = response.status().as_u16();
                    let headers = response.headers().clone();
                    let body = read_body(response, op.what).await;
                    drop(guard);
                    match body {
                        Ok(text) => (status, headers, text),
                        Err(e) => {
                            if is_transport(&e) && op.idempotent && attempt < retry.max_retries {
                                tokio::time::sleep(retry.backoff(attempt)).await;
                                attempt += 1;
                                continue;
                            }
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    if is_transport(&e) && op.idempotent && attempt < retry.max_retries {
                        tokio::time::sleep(retry.backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(e);
                }
            };
            let retry_after = retry_after_secs(&headers);
            match status {
                200..=299 | 500 => {
                    let answer = match soap::parse_answer(&text) {
                        Ok(answer) => answer,
                        Err(_) if status == 500 => {
                            if op.idempotent && attempt < retry.max_retries {
                                tokio::time::sleep(retry.backoff(attempt)).await;
                                attempt += 1;
                                continue;
                            }
                            return Err(EwsError::Unavailable(
                                format!("HTTP 500 to {}", op.what),
                                retry_after,
                            ));
                        }
                        Err(e) => return Err(e),
                    };
                    match answer {
                        Answer::Response(response) => {
                            self.note_version(response.server_version.as_ref());
                            if let Some(ms) = busy_back_off(&response.body) {
                                let wait = Duration::from_millis(ms);
                                if wait <= retry.max_inline_wait && attempt < retry.max_retries {
                                    tracing::debug!(what = op.what, ?wait, "EWS server busy");
                                    tokio::time::sleep(wait).await;
                                    attempt += 1;
                                    continue;
                                }
                                return Err(EwsError::RateLimited(Some(
                                    crate::error::back_off_secs(ms),
                                )));
                            }
                            return Ok(response);
                        }
                        Answer::Fault(fault, version) => {
                            self.note_version(version.as_ref());
                            let code = fault.response_code.as_deref().unwrap_or_default();
                            if (code == "ErrorInvalidServerVersion"
                                || code == "ErrorInvalidRequestServerVersion")
                                && !downgraded
                                && self.request_version() != FALLBACK_SERVER_VERSION
                            {
                                tracing::info!(
                                    "the Exchange server refused {}, using {}",
                                    self.request_version(),
                                    FALLBACK_SERVER_VERSION
                                );
                                self.inner.state().version = FALLBACK_SERVER_VERSION;
                                downgraded = true;
                                continue;
                            }
                            let error = fault.error();
                            if let EwsError::RateLimited(secs) = &error {
                                let wait = fault.back_off_ms.map_or_else(
                                    || Duration::from_secs(u64::from(secs.unwrap_or(1))),
                                    Duration::from_millis,
                                );
                                if wait <= retry.max_inline_wait && attempt < retry.max_retries {
                                    tracing::debug!(what = op.what, ?wait, "EWS server busy");
                                    tokio::time::sleep(wait).await;
                                    attempt += 1;
                                    continue;
                                }
                            } else if matches!(error, EwsError::Unavailable(..))
                                && op.idempotent
                                && attempt < retry.max_retries
                            {
                                tokio::time::sleep(retry.backoff(attempt)).await;
                                attempt += 1;
                                continue;
                            }
                            return Err(error);
                        }
                    }
                }
                401 => return Err(EwsError::AuthFailed),
                403 => return Err(EwsError::Forbidden(format!("HTTP 403 to {}", op.what))),
                404 => {
                    return Err(EwsError::Status {
                        status,
                        message: format!("no EWS endpoint at {}", self.inner.url),
                    });
                }
                413 => return Err(EwsError::TooLarge(format!("request of {}", op.what))),
                429 | 502..=504 => {
                    let wait = retry_after.map_or_else(
                        || retry.backoff(attempt),
                        |s| Duration::from_secs(u64::from(s)),
                    );
                    let may_retry = status == 429 || op.idempotent;
                    if may_retry && wait <= retry.max_inline_wait && attempt < retry.max_retries {
                        tokio::time::sleep(wait).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(if status == 429 {
                        EwsError::RateLimited(retry_after)
                    } else {
                        EwsError::Unavailable(format!("HTTP {status} to {}", op.what), retry_after)
                    });
                }
                _ => {
                    return Err(EwsError::Status {
                        status,
                        message: op.what.to_owned(),
                    });
                }
            }
        }
    }

    /// Opens a `GetStreamingEvents` answer on the streaming lane; read it
    /// with [`EnvelopeStream::next`]. `timeout` bounds the whole answer.
    pub async fn open_stream(
        &self,
        op: &Operation<'_>,
        timeout: Duration,
    ) -> Result<EnvelopeStream, EwsError> {
        let envelope = soap::envelope_with(self.request_version(), op.time_zone, &op.body);
        let (response, guard) = self
            .send(&self.inner.stream, &envelope, op, timeout)
            .await?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) && status != 500 {
            let retry_after = retry_after_secs(response.headers());
            drop(read_body(response, op.what).await);
            return Err(match status {
                401 => EwsError::AuthFailed,
                403 => EwsError::Forbidden(format!("HTTP 403 to {}", op.what)),
                429 => EwsError::RateLimited(retry_after),
                502..=504 => {
                    EwsError::Unavailable(format!("HTTP {status} to {}", op.what), retry_after)
                }
                _ => EwsError::Status {
                    status,
                    message: op.what.to_owned(),
                },
            });
        }
        Ok(EnvelopeStream {
            client: self.clone(),
            response: Some(response),
            buffer: String::new(),
            bytes: Vec::new(),
            pending: VecDeque::new(),
            read: 0,
            _guard: guard,
        })
    }

    fn note_version(&self, version: Option<&ServerVersion>) {
        if let Some(version) = version {
            self.inner.state().server_version = Some(version.clone());
        }
    }

    /// Sends `envelope` on `lane` with the sign-in; answers the response
    /// (status not checked except `401`) and, for Negotiate, the lane lock.
    async fn send(
        &self,
        lane: &Lane,
        envelope: &str,
        op: &Operation<'_>,
        timeout: Duration,
    ) -> Result<Sent, EwsError> {
        let anchor = op.anchor.or(self.inner.own_address.as_deref());
        if let EwsAuth::Negotiate(negotiate) = &self.inner.auth {
            let mut guard = lane.negotiated.clone().lock_owned().await;
            if *guard {
                let response = self.post(lane, envelope, anchor, None, timeout).await?;
                if response.status().as_u16() != 401 {
                    return Ok((response, Some(guard)));
                }
                // The connection was replaced: sign in again.
                drop(response.bytes().await);
                *guard = false;
            }
            let mut session = negotiate.start(&self.inner.spn).map_err(negotiate_error)?;
            let mut token = match session.step(None).map_err(negotiate_error)? {
                NegotiateStep::Continue(t) => Some(t),
                NegotiateStep::Done(t) => t,
            };
            for _ in 0..6 {
                let header = token.take().map(|t| {
                    format!(
                        "Negotiate {}",
                        base64::engine::general_purpose::STANDARD.encode(t)
                    )
                });
                let response = self.post(lane, envelope, anchor, header, timeout).await?;
                let challenge = negotiate_challenge(
                    response
                        .headers()
                        .get_all(reqwest::header::WWW_AUTHENTICATE)
                        .iter()
                        .filter_map(|v| v.to_str().ok()),
                );
                if response.status().as_u16() == 401 {
                    drop(response.bytes().await);
                    let Some(Some(server_token)) = challenge else {
                        return Err(EwsError::AuthFailed);
                    };
                    match session.step(Some(&server_token)).map_err(negotiate_error)? {
                        NegotiateStep::Continue(t) | NegotiateStep::Done(Some(t)) => {
                            token = Some(t);
                        }
                        NegotiateStep::Done(None) => return Err(EwsError::AuthFailed),
                    }
                    continue;
                }
                if let Some(Some(server_token)) = challenge {
                    // Mutual authentication (Kerberos): the server's last
                    // token must verify.
                    session.step(Some(&server_token)).map_err(negotiate_error)?;
                }
                *guard = true;
                return Ok((response, Some(guard)));
            }
            return Err(EwsError::AuthFailed);
        }
        let auth = &self.inner.auth;
        let mut refreshed = false;
        loop {
            let header = self.inner.headers.header(auth, refreshed).await?;
            let response = self.post(lane, envelope, anchor, header, timeout).await?;
            if response.status().as_u16() == 401 {
                drop(response.bytes().await);
                let may_refresh = matches!(
                    auth,
                    EwsAuth::OAuth2 { .. }
                        | EwsAuth::Basic {
                            password: Password::Store { .. },
                            ..
                        }
                );
                if may_refresh && !refreshed {
                    tracing::debug!(what = op.what, "EWS refused the sign-in, refreshing it");
                    refreshed = true;
                    continue;
                }
                return Err(EwsError::AuthFailed);
            }
            return Ok((response, None));
        }
    }

    async fn post(
        &self,
        lane: &Lane,
        envelope: &str,
        anchor: Option<&str>,
        authorization: Option<String>,
        timeout: Duration,
    ) -> Result<reqwest::Response, EwsError> {
        let mut request = lane
            .http
            .post(self.inner.url.clone())
            .timeout(timeout)
            .header(reqwest::header::CONTENT_TYPE, "text/xml; charset=utf-8")
            .header(reqwest::header::ACCEPT, "text/xml")
            .body(envelope.to_owned());
        if let Some(value) = authorization {
            let mut value = HeaderValue::from_str(&value).map_err(|_| {
                EwsError::Protocol("the credentials are not valid header text".into())
            })?;
            value.set_sensitive(true);
            request = request.header(reqwest::header::AUTHORIZATION, value);
        }
        if let Some(anchor) = anchor
            && let Ok(value) = HeaderValue::from_str(anchor)
        {
            request = request.header("X-AnchorMailbox", value);
        }
        request.send().await.map_err(transport_error)
    }
}

impl Inner {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A lane client on Nail's TLS configuration: one idle connection, HTTP/1.1
/// (Negotiate does not work over HTTP/2), no redirects.
fn tls_lane(timeout: Duration) -> Result<reqwest::Client, EwsError> {
    let tls = nail_auth::tls_config().map_err(|e| EwsError::Tls(e.to_string()))?;
    nail_auth::http_client_builder(tls)
        .timeout(timeout)
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(Duration::from_secs(90))
        .http1_only()
        .build()
        .map_err(|e| EwsError::Tls(e.to_string()))
}

/// A lane client (Nail's TLS configuration, one connection, HTTP/1.1) with
/// a custom timeout, e.g. for tests against loopback servers.
pub fn lane_http(timeout: Duration) -> Result<reqwest::Client, EwsError> {
    tls_lane(timeout)
}

fn negotiate_error(error: nail_api::engine::EngineError) -> EwsError {
    if error.code == ErrorCode::AuthFailed {
        tracing::debug!(%error, "the Negotiate handshake failed");
        EwsError::AuthFailed
    } else {
        EwsError::Credentials(Box::new(error))
    }
}

fn is_transport(error: &EwsError) -> bool {
    matches!(error, EwsError::Network(_) | EwsError::Timeout)
}

/// `BackOffMilliseconds` when the answer's response messages say
/// `ErrorServerBusy`.
fn busy_back_off(body: &crate::xml::Element) -> Option<u64> {
    let direct = body
        .child("ResponseCode")
        .is_some()
        .then(|| ResponseMessage::read(body));
    soap::response_messages(body)
        .into_iter()
        .chain(direct)
        .find(|m| m.code == "ErrorServerBusy")
        .map(|m| m.back_off_ms().unwrap_or(1000))
}

async fn read_body(mut response: reqwest::Response, what: &str) -> Result<String, EwsError> {
    if response
        .content_length()
        .is_some_and(|l| l > MAX_RESPONSE as u64)
    {
        return Err(EwsError::TooLarge(format!("answer to {what}")));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if body.len() + chunk.len() > MAX_RESPONSE {
            return Err(EwsError::TooLarge(format!("answer to {what}")));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(match String::from_utf8(body) {
        Ok(text) => text,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    })
}

/// A `GetStreamingEvents` answer: one SOAP envelope per batch of
/// notifications, arriving over time on one HTTP answer.
#[derive(Debug)]
pub struct EnvelopeStream {
    client: EwsClient,
    response: Option<reqwest::Response>,
    buffer: String,
    bytes: Vec<u8>,
    pending: VecDeque<String>,
    read: usize,
    _guard: Option<OwnedMutexGuard<bool>>,
}

impl EnvelopeStream {
    /// The next envelope, parsed; `None` when the server ended the answer.
    pub async fn next(&mut self) -> Result<Option<Answer>, EwsError> {
        loop {
            if let Some(document) = self.pending.pop_front() {
                let answer = soap::parse_answer(&document)?;
                match &answer {
                    Answer::Response(r) => self.client.note_version(r.server_version.as_ref()),
                    Answer::Fault(_, v) => self.client.note_version(v.as_ref()),
                }
                return Ok(Some(answer));
            }
            let Some(response) = self.response.as_mut() else {
                return Ok(None);
            };
            if let Some(chunk) = response.chunk().await.map_err(transport_error)? {
                self.read += chunk.len();
                if self.read > MAX_RESPONSE {
                    return Err(EwsError::TooLarge("streaming answer".to_owned()));
                }
                self.bytes.extend_from_slice(&chunk);
                // Keep incomplete UTF-8 sequences for the next chunk.
                let valid = match std::str::from_utf8(&self.bytes) {
                    Ok(_) => self.bytes.len(),
                    Err(e) => e.valid_up_to(),
                };
                let rest = self.bytes.split_off(valid);
                let text = std::mem::replace(&mut self.bytes, rest);
                self.buffer.push_str(&String::from_utf8_lossy(&text));
                self.pending.extend(soap::split_envelopes(&mut self.buffer));
            } else {
                self.response = None;
                let rest = std::mem::take(&mut self.buffer);
                if !rest.trim().is_empty() {
                    self.pending.push_back(rest);
                }
            }
        }
    }
}

/// The `Retry-After` header in seconds (delta seconds or an HTTP date).
pub(crate) fn retry_after_secs(headers: &HeaderMap) -> Option<u32> {
    let value = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    parse_retry_after(value, chrono::Utc::now())
}

/// Parses a `Retry-After` value relative to `now`.
pub(crate) fn parse_retry_after(value: &str, now: chrono::DateTime<chrono::Utc>) -> Option<u32> {
    if let Ok(secs) = value.parse::<u64>() {
        return Some(u32::try_from(secs).unwrap_or(u32::MAX));
    }
    let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let secs = (date.with_timezone(&chrono::Utc) - now)
        .num_seconds()
        .max(0);
    Some(u32::try_from(secs).unwrap_or(u32::MAX))
}

/// Maps a reqwest error to an [`EwsError`] (TLS problems are told apart by
/// their source chain).
fn transport_error(error: reqwest::Error) -> EwsError {
    let error = error.without_url();
    if error.is_timeout() {
        return EwsError::Timeout;
    }
    let mut chain = String::new();
    let mut source: Option<&dyn std::error::Error> = Some(&error);
    while let Some(e) = source {
        chain.push_str(&e.to_string());
        chain.push_str(": ");
        source = e.source();
    }
    let chain = chain.trim_end_matches(": ").to_owned();
    let lower = chain.to_ascii_lowercase();
    if lower.contains("certificate") || lower.contains("tls") || lower.contains("handshake") {
        EwsError::Tls(chain)
    } else {
        EwsError::Network(chain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    fn client(url: &str) -> Result<EwsClient, EwsError> {
        EwsClient::with_http(
            lane_http(CALL_TIMEOUT).unwrap(),
            lane_http(CALL_TIMEOUT).unwrap(),
            url,
            EwsAuth::None,
        )
    }

    #[test]
    fn url_policy() {
        for ok in [
            "https://mail.corp.example/EWS/Exchange.asmx",
            "http://127.0.0.1:8080/EWS/Exchange.asmx",
            "http://localhost/EWS/Exchange.asmx",
            "http://[::1]:9/EWS/Exchange.asmx",
        ] {
            assert!(client(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://mail.corp.example/EWS/Exchange.asmx",
            "http://10.0.0.1/EWS/Exchange.asmx",
            "ftp://mail.corp.example/",
            "not a url",
        ] {
            assert!(
                matches!(client(bad), Err(EwsError::InsecureUrl(_))),
                "{bad}"
            );
        }
        let c = client("https://Mail.Corp.Example/EWS/Exchange.asmx#x").unwrap();
        assert_eq!(c.inner.spn, "HTTP/mail.corp.example");
        assert_eq!(c.url().fragment(), None);
    }

    #[test]
    fn builder_options() {
        let c = client("http://127.0.0.1:1/EWS/Exchange.asmx")
            .unwrap()
            .with_retry(RetryPolicy::none())
            .with_own_address("ada@corp.example")
            .with_request_version(FALLBACK_SERVER_VERSION);
        assert_eq!(c.retry_policy().max_retries, 0);
        assert_eq!(c.own_address(), Some("ada@corp.example"));
        assert_eq!(c.request_version(), FALLBACK_SERVER_VERSION);
        assert!(c.server_version().is_none());
        let shared = c.clone();
        let changed = shared.with_retry(RetryPolicy::fast());
        assert_eq!(changed.retry_policy(), RetryPolicy::fast());
        assert_eq!(c.retry_policy().max_retries, 0);
        assert_eq!(changed.own_address(), Some("ada@corp.example"));
    }

    #[test]
    fn retry_after_values() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
        assert_eq!(parse_retry_after("120", now), Some(120));
        assert_eq!(
            parse_retry_after("Thu, 01 Oct 2026 12:01:30 GMT", now),
            Some(90)
        );
        assert_eq!(parse_retry_after("soon", now), None);
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let policy = RetryPolicy {
            max_retries: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
            max_inline_wait: Duration::from_secs(5),
        };
        let first = policy.backoff(0);
        assert!(first >= Duration::from_millis(100) && first <= Duration::from_millis(125));
        assert_eq!(policy.backoff(10), Duration::from_secs(1));
        assert_eq!(policy.backoff(u32::MAX), Duration::from_secs(1));
    }

    #[test]
    fn busy_messages_are_found() {
        let body = crate::xml::parse(concat!(
            r#"<m:X xmlns:m="m" xmlns:t="t"><m:ResponseMessages><m:R ResponseClass="Error">"#,
            r#"<m:MessageText>busy</m:MessageText><m:ResponseCode>ErrorServerBusy</m:ResponseCode>"#,
            r#"<m:MessageXml><t:Value Name="BackOffMilliseconds">1500</t:Value></m:MessageXml>"#,
            r#"</m:R></m:ResponseMessages></m:X>"#
        ))
        .unwrap();
        assert_eq!(busy_back_off(&body), Some(1500));
        let direct = crate::xml::parse(
            r#"<m:GetInboxRulesResponse xmlns:m="m" ResponseClass="Error"><m:ResponseCode>ErrorServerBusy</m:ResponseCode></m:GetInboxRulesResponse>"#,
        )
        .unwrap();
        assert_eq!(busy_back_off(&direct), Some(1000));
        let fine = crate::xml::parse(r#"<m:X xmlns:m="m"><m:ResponseMessages/></m:X>"#).unwrap();
        assert_eq!(busy_back_off(&fine), None);
    }
}
