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
use nail_net::http::{self, Retries, TransportFailure as _, status};
use reqwest::header::HeaderValue;
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
pub use nail_net::http::MAX_EXCHANGE_ANSWER as MAX_RESPONSE;

/// How long an ordinary call may take.
pub const CALL_TIMEOUT: Duration = Duration::from_mins(2);

/// Inline retries of throttling and server errors: exponential backoff, or
/// the server's `BackOffMilliseconds` / `Retry-After`. A longer wait goes
/// to the caller with [`EwsError::retry_after_secs`] (the core's loop
/// waits instead).
pub use nail_net::http::RetryPolicy;

/// Whether the client may talk to `url`: `https:`, or `http:` to a
/// loopback host (the mock server; ADR 0057 / 0072).
pub use nail_net::http::is_allowed_url;

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
    /// The connection log (ADR 0190).
    tap: nail_connlog::HttpTap,
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
                tap: nail_connlog::HttpTap::off(),
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
                tap: shared.tap.clone(),
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

    /// Records every request into `tap` (ADR 0190).
    #[must_use]
    pub fn with_tap(self, tap: nail_connlog::HttpTap) -> Self {
        self.rebuild(|i| i.tap = tap)
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
        let mut tries = Retries::new(self.inner.retry);
        let mut downgraded = false;
        loop {
            let envelope = soap::envelope_with(self.request_version(), op.time_zone, &op.body);
            let read = match self
                .send(&self.inner.main, &envelope, op, CALL_TIMEOUT)
                .await
            {
                Ok((response, guard)) => {
                    let status = response.status().as_u16();
                    let retry_after = http::retry_after_secs(response.headers());
                    let body = read_body(response, op.what).await;
                    drop(guard);
                    body.map(|text| (status, retry_after, text))
                }
                Err(e) => Err(e),
            };
            let (status, retry_after, text) = match read {
                Ok(answer) => answer,
                Err(e) if e.is_transport() && op.idempotent && tries.backoff().await => continue,
                Err(e) => return Err(e),
            };
            match status {
                200..=299 | 500 => {
                    let answer = match soap::parse_answer(&text) {
                        Ok(answer) => answer,
                        Err(_) if status == 500 => {
                            if op.idempotent && tries.backoff().await {
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
                                if tries.allows(wait) {
                                    tracing::debug!(what = op.what, ?wait, "EWS server busy");
                                    tries.sleep(wait).await;
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
                                if tries.allows(wait) {
                                    tracing::debug!(what = op.what, ?wait, "EWS server busy");
                                    tries.sleep(wait).await;
                                    continue;
                                }
                            } else if matches!(error, EwsError::Unavailable(..))
                                && op.idempotent
                                && tries.backoff().await
                            {
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
                status::CONTENT_TOO_LARGE => {
                    return Err(EwsError::TooLarge(format!("request of {}", op.what)));
                }
                status::TOO_MANY_REQUESTS | 502..=status::GATEWAY_TIMEOUT => {
                    let may_retry = status == status::TOO_MANY_REQUESTS || op.idempotent;
                    if may_retry && let Some(wait) = tries.wait(retry_after) {
                        tries.sleep(wait).await;
                        continue;
                    }
                    return Err(if status == status::TOO_MANY_REQUESTS {
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
            let retry_after = http::retry_after_secs(response.headers());
            drop(read_body(response, op.what).await);
            return Err(match status {
                401 => EwsError::AuthFailed,
                403 => EwsError::Forbidden(format!("HTTP 403 to {}", op.what)),
                status::TOO_MANY_REQUESTS => EwsError::RateLimited(retry_after),
                502..=status::GATEWAY_TIMEOUT => {
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
        let authorized = authorization.is_some();
        if let Some(value) = authorization {
            // The credential (token, Basic pair, Negotiate blob) never
            // reaches the log, also not inside a server's error text.
            http::hide_credential(&self.inner.tap, &value);
            request = http::with_authorization(request, &value).ok_or_else(|| {
                EwsError::Protocol("the credentials are not valid header text".into())
            })?;
        }
        if let Some(anchor) = anchor
            && let Ok(value) = HeaderValue::from_str(anchor)
        {
            request = request.header("X-AnchorMailbox", value);
        }
        let exchange = self.inner.tap.request(
            "POST",
            self.inner.url.as_str(),
            authorized,
            Some(u64::try_from(envelope.len()).unwrap_or(u64::MAX)),
        );
        match request.send().await {
            Ok(response) => {
                // The body (a SOAP answer, or a notification stream) is
                // read by the caller; the log gets the status.
                exchange.response(response.status().as_u16(), response.content_length(), None);
                Ok(response)
            }
            Err(err) => {
                let err = EwsError::from(http::transport_error(err));
                exchange.failed(&err.to_string());
                Err(err)
            }
        }
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
    let body = http::read_limited(&mut response, MAX_RESPONSE)
        .await
        .map_err(|e| EwsError::from_body(e, &format!("answer to {what}")))?;
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
            if let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| EwsError::from(http::transport_error(e)))?
            {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(client("https://mail.corp.example/EWS/Exchange.asmx").is_ok());
        assert!(client("http://[::1]:9/EWS/Exchange.asmx").is_ok());
        let insecure = client("http://mail.corp.example/EWS/Exchange.asmx");
        assert!(matches!(insecure, Err(EwsError::InsecureUrl(_))));
        assert!(matches!(client("not a url"), Err(EwsError::InsecureUrl(_))));
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
