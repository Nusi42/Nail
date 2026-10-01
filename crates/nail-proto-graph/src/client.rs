//! The HTTP side: the API URL policy, OAuth2 bearer tokens (one forced
//! refresh on `401`), throttling (`429` / `503` / `504` with
//! `Retry-After`) and server-error backoff, paging links that must stay on
//! the API's origin, response size limits and error mapping.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::SecretRef;
use nail_api::engine::AccessTokenProvider;
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::error::GraphError;
use crate::wire::{ErrorBody, User};

/// Largest answer read (a 150 MB message is beyond Exchange Online's
/// limits; `$value` answers are raw MIME).
pub const MAX_RESPONSE: usize = 160 * 1024 * 1024;

/// How the client signs in.
#[derive(Clone)]
pub enum GraphAuth {
    /// A fixed bearer token (tests).
    Bearer(String),
    /// OAuth2 bearer tokens from the core's token manager, refreshed once
    /// when Graph answers `401`.
    OAuth2 {
        /// The token source.
        tokens: Arc<dyn AccessTokenProvider>,
        /// The OAuth2 provider id (`microsoft`).
        provider: String,
        /// Where the refresh token is stored.
        secret: SecretRef,
    },
}

impl fmt::Debug for GraphAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer(_) => f.write_str("Bearer(..)"),
            Self::OAuth2 { provider, .. } => f
                .debug_struct("OAuth2")
                .field("provider", provider)
                .finish_non_exhaustive(),
        }
    }
}

/// Inline retries of throttling and server errors: exponential backoff
/// from `base_delay` up to `max_delay` (with a little jitter), or the
/// server's `Retry-After`. A wait longer than `max_inline_wait` is not
/// slept here: the error goes to the caller with
/// [`GraphError::retry_after_secs`] (the core's poll loop waits instead).
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

    /// The wait before retry `attempt` after an answer with `retry_after`
    /// seconds (or none); `None` when it is longer than the inline limit.
    pub fn wait(&self, attempt: u32, retry_after: Option<u32>) -> Option<Duration> {
        let wait = retry_after.map_or_else(
            || self.backoff(attempt),
            |s| Duration::from_secs(u64::from(s)),
        );
        (wait <= self.max_inline_wait).then_some(wait)
    }
}

/// The body of a request.
#[derive(Debug, Clone)]
pub(crate) enum Body {
    None,
    Json(Vec<u8>),
    Raw {
        content_type: String,
        bytes: Vec<u8>,
    },
}

/// The `Prefer` value asking for immutable ids: a message keeps its id when
/// it is moved (by Nail, another client or a server rule), so a move is not
/// seen as a delete plus a new message (ADR 0078).
pub const IMMUTABLE_IDS: &str = "IdType=\"ImmutableId\"";

/// `headers` with [`IMMUTABLE_IDS`] merged into their `Prefer` header (one
/// header, preferences separated by commas).
pub(crate) fn with_immutable_ids(headers: &[(String, String)]) -> Vec<(String, String)> {
    let mut prefer = vec![IMMUTABLE_IDS.to_owned()];
    let mut out = Vec::with_capacity(headers.len() + 1);
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("prefer") {
            prefer.extend(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty() && !p.starts_with("IdType="))
                    .map(str::to_owned),
            );
        } else {
            out.push((name.clone(), value.clone()));
        }
    }
    out.push(("Prefer".to_owned(), prefer.join(", ")));
    out
}

/// A request.
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub method: Method,
    pub url: Url,
    pub body: Body,
    /// Extra headers (`Prefer`, `If-Match`).
    pub headers: Vec<(String, String)>,
    /// Safe to repeat after a server error or a broken connection (reads,
    /// `PATCH`es, deletes); `sendMail`, creates and `move` are not.
    pub idempotent: bool,
    /// What is attempted (for errors and logs).
    pub what: String,
}

/// A successful answer.
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// A Microsoft Graph client (`https://graph.microsoft.com/v1.0`).
#[derive(Debug, Clone)]
pub struct GraphClient {
    http: reqwest::Client,
    base: Url,
    auth: GraphAuth,
    retry: RetryPolicy,
}

/// Whether the client may talk to `url`: `https:`, or `http:` to a
/// loopback host (the mock server; ADR 0072).
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

impl GraphClient {
    /// A client on Nail's TLS configuration (rustls with ring and the
    /// platform verifier, no redirects) for the API base URL `api_base`
    /// (see [`crate::api_base`]).
    pub fn new(api_base: &str, auth: GraphAuth) -> Result<Self, GraphError> {
        let http = nail_auth::http_client().map_err(|e| GraphError::Tls(e.to_string()))?;
        Self::with_http(http, api_base, auth)
    }

    /// A client on an existing [`reqwest::Client`] (it must not follow
    /// redirects; build it with `nail_auth::http_client_builder`).
    pub fn with_http(
        http: reqwest::Client,
        api_base: &str,
        auth: GraphAuth,
    ) -> Result<Self, GraphError> {
        let mut base =
            Url::parse(api_base).map_err(|_| GraphError::InsecureUrl(api_base.to_owned()))?;
        if !is_allowed_url(&base) || base.host_str().is_none_or(str::is_empty) {
            return Err(GraphError::InsecureUrl(api_base.to_owned()));
        }
        base.set_query(None);
        base.set_fragment(None);
        if let Ok(mut segments) = base.path_segments_mut() {
            segments.pop_if_empty();
        }
        Ok(Self {
            http,
            base,
            auth,
            retry: RetryPolicy::default(),
        })
    }

    /// Replaces the retry policy.
    #[must_use]
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// The retry policy.
    pub fn retry_policy(&self) -> RetryPolicy {
        self.retry
    }

    /// The API base URL (`…/v1.0`).
    pub fn api_base(&self) -> &Url {
        &self.base
    }

    /// The sign-in.
    pub fn auth(&self) -> &GraphAuth {
        &self.auth
    }

    /// The URL of `path` (starting with `/`, segments already encoded with
    /// [`crate::path_segment`]) below the base, with `query`.
    pub(crate) fn url(&self, path: &str, query: &[(&str, String)]) -> Result<Url, GraphError> {
        let text = format!("{}{path}", self.base.as_str().trim_end_matches('/'));
        let mut url =
            Url::parse(&text).map_err(|e| GraphError::Protocol(format!("{path}: {e}")))?;
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        Ok(url)
    }

    /// Whether `url` is on the API's origin (paging and delta links must
    /// be: the bearer token never goes anywhere else).
    pub fn same_origin(&self, url: &Url) -> bool {
        url.scheme() == self.base.scheme()
            && url.host_str() == self.base.host_str()
            && url.port_or_known_default() == self.base.port_or_known_default()
    }

    /// A paging or delta link the server sent, checked against the origin.
    pub(crate) fn link(&self, link: &str) -> Result<Url, GraphError> {
        let url =
            Url::parse(link).map_err(|e| GraphError::Protocol(format!("paging link: {e}")))?;
        if !self.same_origin(&url) {
            return Err(GraphError::Protocol(format!(
                "a paging link to another server: {}",
                url.origin().ascii_serialization()
            )));
        }
        Ok(url)
    }

    /// The `$batch` endpoint (`…/v1.0/$batch`).
    pub(crate) fn batch_url(&self) -> Result<Url, GraphError> {
        self.url("/$batch", &[])
    }

    pub(crate) async fn authorization(&self, force_refresh: bool) -> Result<String, GraphError> {
        Ok(match &self.auth {
            GraphAuth::Bearer(token) => format!("Bearer {token}"),
            GraphAuth::OAuth2 {
                tokens,
                provider,
                secret,
            } => {
                let token = tokens
                    .access_token(provider, secret, force_refresh)
                    .await
                    .map_err(|e| GraphError::Token(Box::new(e)))?;
                format!("Bearer {token}")
            }
        })
    }

    /// Sends `call`: one forced token refresh on `401`, inline retries per
    /// the [`RetryPolicy`]. Non-2xx answers become [`GraphError`]s.
    pub(crate) async fn execute(&self, call: &Call) -> Result<Reply, GraphError> {
        if !is_allowed_url(&call.url) || !self.same_origin(&call.url) {
            return Err(GraphError::InsecureUrl(call.url.to_string()));
        }
        let mut refreshed = false;
        let mut attempt = 0u32;
        loop {
            let (status, headers, body) = match self.send_once(call, refreshed).await {
                Ok(answer) => answer,
                Err(e) => {
                    let transport = matches!(e, GraphError::Network(_) | GraphError::Timeout);
                    if transport && call.idempotent && attempt < self.retry.max_retries {
                        tokio::time::sleep(self.retry.backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(e);
                }
            };
            if (200..300).contains(&status) {
                return Ok(Reply { status, body });
            }
            if status == 401 && !refreshed && matches!(self.auth, GraphAuth::OAuth2 { .. }) {
                tracing::debug!(what = %call.what, "Graph refused the token, refreshing it");
                refreshed = true;
                continue;
            }
            let error_body: Option<ErrorBody> = serde_json::from_slice(&body).ok();
            let retry_after = retry_after_secs(&headers);
            if GraphError::is_transient(status)
                && (call.idempotent || GraphError::is_throttle(status))
                && attempt < self.retry.max_retries
                && let Some(wait) = self.retry.wait(attempt, retry_after)
            {
                tracing::debug!(what = %call.what, status, ?wait, "Graph asks to retry");
                tokio::time::sleep(wait).await;
                attempt += 1;
                continue;
            }
            return Err(GraphError::from_status(
                status,
                retry_after,
                error_body.as_ref(),
                &call.what,
            ));
        }
    }

    async fn send_once(
        &self,
        call: &Call,
        force_refresh: bool,
    ) -> Result<(u16, HeaderMap, Vec<u8>), GraphError> {
        let mut request = self.http.request(call.method.clone(), call.url.clone());
        let mut value = HeaderValue::from_str(&self.authorization(force_refresh).await?)
            .map_err(|_| GraphError::Protocol("the token is not valid header text".into()))?;
        value.set_sensitive(true);
        request = request
            .header(reqwest::header::AUTHORIZATION, value)
            .header(reqwest::header::ACCEPT, "application/json");
        for (name, value) in with_immutable_ids(&call.headers) {
            request = request.header(name.as_str(), value.as_str());
        }
        match &call.body {
            Body::None => {
                if call.method == Method::POST {
                    request = request.header(reqwest::header::CONTENT_LENGTH, "0");
                }
            }
            Body::Json(bytes) => {
                request = request
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(bytes.clone());
            }
            Body::Raw {
                content_type,
                bytes,
            } => {
                request = request
                    .header(reqwest::header::CONTENT_TYPE, content_type.as_str())
                    .body(bytes.clone());
            }
        }
        let mut response = request.send().await.map_err(transport_error)?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        if response
            .content_length()
            .is_some_and(|l| l > MAX_RESPONSE as u64)
        {
            return Err(GraphError::TooLarge(format!("answer to {}", call.what)));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if body.len() + chunk.len() > MAX_RESPONSE {
                return Err(GraphError::TooLarge(format!("answer to {}", call.what)));
            }
            body.extend_from_slice(&chunk);
        }
        Ok((status, headers, body))
    }

    /// A request below the base URL.
    pub(crate) fn call(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        what: &str,
    ) -> Result<Call, GraphError> {
        let idempotent = method != Method::POST;
        Ok(Call {
            method,
            url: self.url(path, query)?,
            body: Body::None,
            headers: Vec::new(),
            idempotent,
            what: what.to_owned(),
        })
    }

    /// `GET path` as JSON.
    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
        headers: &[(&str, String)],
        what: &str,
    ) -> Result<T, GraphError> {
        let mut call = self.call(Method::GET, path, query, what)?;
        call.headers = owned(headers);
        let reply = self.execute(&call).await?;
        parse_json(&reply.body, what)
    }

    /// `GET` of a paging / delta link as JSON.
    pub(crate) async fn get_link_json<T: DeserializeOwned>(
        &self,
        link: &str,
        headers: &[(&str, String)],
        what: &str,
    ) -> Result<T, GraphError> {
        let call = Call {
            method: Method::GET,
            url: self.link(link)?,
            body: Body::None,
            headers: owned(headers),
            idempotent: true,
            what: what.to_owned(),
        };
        let reply = self.execute(&call).await?;
        parse_json(&reply.body, what)
    }

    /// `GET path` as bytes (`$value`).
    pub(crate) async fn get_bytes(&self, path: &str, what: &str) -> Result<Vec<u8>, GraphError> {
        let call = self.call(Method::GET, path, &[], what)?;
        Ok(self.execute(&call).await?.body)
    }

    /// A request with an optional JSON body; answers the parsed JSON body
    /// (`Value::Null` for `204`).
    pub(crate) async fn send_json(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        headers: &[(&str, String)],
        idempotent: bool,
        what: &str,
    ) -> Result<Value, GraphError> {
        let mut call = self.call(method, path, &[], what)?;
        call.idempotent = idempotent;
        call.headers = owned(headers);
        if let Some(body) = body {
            call.body = Body::Json(
                serde_json::to_vec(body).map_err(|e| GraphError::Protocol(e.to_string()))?,
            );
        }
        let reply = self.execute(&call).await?;
        if reply.body.is_empty() || reply.status == 204 {
            return Ok(Value::Null);
        }
        parse_json(&reply.body, what)
    }

    /// A `POST` of raw text (`text/plain` base64 MIME).
    pub(crate) async fn post_text(
        &self,
        path: &str,
        text: Vec<u8>,
        what: &str,
    ) -> Result<Value, GraphError> {
        let mut call = self.call(Method::POST, path, &[], what)?;
        call.body = Body::Raw {
            content_type: "text/plain".to_owned(),
            bytes: text,
        };
        let reply = self.execute(&call).await?;
        if reply.body.is_empty() {
            return Ok(Value::Null);
        }
        parse_json(&reply.body, what)
    }

    /// `GET /me` (or `/users/{mailbox}`): the signed-in user; also the
    /// account test.
    pub async fn me(&self, mailbox: Option<&str>) -> Result<User, GraphError> {
        self.get_json(
            &crate::mailbox_path(mailbox),
            &[(
                "$select",
                "id,displayName,mail,userPrincipalName".to_owned(),
            )],
            &[],
            "GET /me",
        )
        .await
    }
}

fn owned(headers: &[(&str, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(n, v)| ((*n).to_owned(), v.clone()))
        .collect()
}

/// Parses a JSON answer.
pub(crate) fn parse_json<T: DeserializeOwned>(body: &[u8], what: &str) -> Result<T, GraphError> {
    serde_json::from_slice(body).map_err(|e| GraphError::Protocol(format!("{what}: {e}")))
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

/// Maps a reqwest error to a [`GraphError`] (TLS problems are told apart
/// by their source chain).
fn transport_error(error: reqwest::Error) -> GraphError {
    let error = error.without_url();
    if error.is_timeout() {
        return GraphError::Timeout;
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
        GraphError::Tls(chain)
    } else {
        GraphError::Network(chain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    fn client(base: &str) -> Result<GraphClient, GraphError> {
        GraphClient::with_http(
            nail_auth::http_client().unwrap(),
            base,
            GraphAuth::Bearer("t".into()),
        )
    }

    #[test]
    fn url_policy() {
        for ok in [
            "https://graph.microsoft.com/v1.0",
            "http://127.0.0.1:8080/v1.0",
            "http://localhost/v1.0/",
            "http://[::1]:9/v1.0",
        ] {
            assert!(client(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://graph.microsoft.com/v1.0",
            "http://10.0.0.1/v1.0",
            "ftp://graph.microsoft.com/",
            "not a url",
        ] {
            assert!(
                matches!(client(bad), Err(GraphError::InsecureUrl(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn urls_and_links() {
        let c = client("http://127.0.0.1:1/v1.0/").unwrap();
        assert_eq!(c.api_base().path(), "/v1.0");
        let url = c
            .url(
                "/me/mailFolders/a%2Fb/messages/delta",
                &[("$select", "id,isRead".into())],
            )
            .unwrap();
        assert_eq!(url.path(), "/v1.0/me/mailFolders/a%2Fb/messages/delta");
        assert_eq!(url.query(), Some("%24select=id%2CisRead"));
        assert_eq!(
            c.batch_url().unwrap().as_str(),
            "http://127.0.0.1:1/v1.0/$batch"
        );
        assert!(
            c.link("http://127.0.0.1:1/v1.0/me/messages/delta?$deltatoken=x")
                .is_ok()
        );
        for foreign in [
            "http://127.0.0.1:2/v1.0/me/messages/delta",
            "https://evil.example/v1.0/me",
            "http://localhost:1/v1.0/me",
        ] {
            assert!(
                matches!(c.link(foreign), Err(GraphError::Protocol(_))),
                "{foreign}"
            );
        }
        let https = client("https://graph.microsoft.com/v1.0").unwrap();
        assert!(https.same_origin(&Url::parse("https://graph.microsoft.com:443/beta").unwrap()));
    }

    #[test]
    fn auth_debug_hides_secrets() {
        let text = format!("{:?}", GraphAuth::Bearer("secret-token".into()));
        assert!(!text.contains("secret-token"));
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
        assert_eq!(parse_retry_after("soon", now), None);
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
    }
}
