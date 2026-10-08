//! The HTTP side: the API URL policy, OAuth2 bearer tokens (one forced
//! refresh on `401`), quota / `429` / `5xx` backoff with `Retry-After`,
//! response size limits and error mapping.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::SecretRef;
use nail_api::engine::AccessTokenProvider;
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::GmailError;
use crate::wire::{ErrorBody, Profile};

/// Largest answer read (a 35 MB message as base64url in JSON stays below).
pub const MAX_RESPONSE: usize = 64 * 1024 * 1024;

/// How the client signs in.
#[derive(Clone)]
pub enum GmailAuth {
    /// A fixed bearer token (tests).
    Bearer(String),
    /// OAuth2 bearer tokens from the core's token manager, refreshed once
    /// when the API answers `401`.
    OAuth2 {
        /// The token source.
        tokens: Arc<dyn AccessTokenProvider>,
        /// The OAuth2 provider id (`google`).
        provider: String,
        /// Where the refresh token is stored.
        secret: SecretRef,
    },
}

impl fmt::Debug for GmailAuth {
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

/// Inline retries of quota, rate-limit and server errors: exponential
/// backoff from `base_delay` up to `max_delay` (with a little jitter), or
/// the server's `Retry-After`. A wait longer than `max_inline_wait` is not
/// slept here: the error goes to the caller with
/// [`GmailError::retry_after_secs`] (the core's poll loop waits instead).
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

/// A request.
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub method: Method,
    pub url: Url,
    pub body: Body,
    /// Safe to repeat after a server error or a broken connection (reads,
    /// label changes); `send` / `insert` / creates are not.
    pub idempotent: bool,
    /// What is attempted (for errors and logs).
    pub what: String,
}

/// A successful answer.
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

/// A Gmail API client for one mailbox (`…/gmail/v1/users/me`).
#[derive(Debug, Clone)]
pub struct GmailClient {
    http: reqwest::Client,
    base: Url,
    auth: GmailAuth,
    retry: RetryPolicy,
    /// The connection log (ADR 0190).
    tap: nail_connlog::HttpTap,
}

/// Whether the client may talk to `url`: `https:`, or `http:` to a
/// loopback host (the mock server; ADR 0065).
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

impl GmailClient {
    /// A client on Nail's TLS configuration (rustls with ring and the
    /// platform verifier, no redirects) for the API base URL `api_base`
    /// (see [`crate::api_base`]).
    pub fn new(api_base: &str, auth: GmailAuth) -> Result<Self, GmailError> {
        let http = nail_auth::http_client().map_err(|e| GmailError::Tls(e.to_string()))?;
        Self::with_http(http, api_base, auth)
    }

    /// A client on an existing [`reqwest::Client`] (it must not follow
    /// redirects; build it with `nail_auth::http_client_builder`).
    pub fn with_http(
        http: reqwest::Client,
        api_base: &str,
        auth: GmailAuth,
    ) -> Result<Self, GmailError> {
        let mut base =
            Url::parse(api_base).map_err(|_| GmailError::InsecureUrl(api_base.to_owned()))?;
        if !is_allowed_url(&base) || base.host_str().is_none_or(str::is_empty) {
            return Err(GmailError::InsecureUrl(api_base.to_owned()));
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
            tap: nail_connlog::HttpTap::off(),
        })
    }

    /// Records every request into `tap` (ADR 0190).
    #[must_use]
    pub fn with_tap(mut self, tap: nail_connlog::HttpTap) -> Self {
        self.tap = tap;
        self
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

    /// The API base URL (`…/gmail/v1/users/me`).
    pub fn api_base(&self) -> &Url {
        &self.base
    }

    /// The sign-in.
    pub fn auth(&self) -> &GmailAuth {
        &self.auth
    }

    /// The URL of `segments` below the base, with `query`.
    pub(crate) fn url(&self, segments: &[&str], query: &[(&str, String)]) -> Url {
        let mut url = self.base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.extend(segments);
        }
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        url
    }

    /// The batch endpoint (`<origin>/batch/gmail/v1`).
    pub(crate) fn batch_url(&self) -> Url {
        let mut url = self.base.clone();
        url.set_path("/batch/gmail/v1");
        url
    }

    async fn authorization(&self, force_refresh: bool) -> Result<String, GmailError> {
        Ok(match &self.auth {
            GmailAuth::Bearer(token) => {
                self.tap.add_secret(token);
                format!("Bearer {token}")
            }
            GmailAuth::OAuth2 {
                tokens,
                provider,
                secret,
            } => {
                let token = tokens
                    .access_token(provider, secret, force_refresh)
                    .await
                    .map_err(|e| GmailError::Token(Box::new(e)))?;
                self.tap.add_secret(&token);
                format!("Bearer {token}")
            }
        })
    }

    /// Sends `call`: one forced token refresh on `401`, inline retries per
    /// the [`RetryPolicy`]. Non-2xx answers become [`GmailError`]s.
    pub(crate) async fn execute(&self, call: &Call) -> Result<Reply, GmailError> {
        if !is_allowed_url(&call.url) {
            return Err(GmailError::InsecureUrl(call.url.to_string()));
        }
        let mut refreshed = false;
        let mut attempt = 0u32;
        loop {
            let (status, headers, body) = match self.send_once(call, refreshed).await {
                Ok(answer) => answer,
                Err(e) => {
                    let transport = matches!(e, GmailError::Network(_) | GmailError::Timeout);
                    if transport && call.idempotent && attempt < self.retry.max_retries {
                        tokio::time::sleep(self.retry.backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(e);
                }
            };
            if (200..300).contains(&status) {
                return Ok(Reply { headers, body });
            }
            if status == 401 && !refreshed && matches!(self.auth, GmailAuth::OAuth2 { .. }) {
                tracing::debug!(what = %call.what, "Gmail refused the token, refreshing it");
                refreshed = true;
                continue;
            }
            let error_body: Option<ErrorBody> = serde_json::from_slice(&body).ok();
            let reason = error_body.as_ref().and_then(ErrorBody::reason);
            let retry_after = retry_after_secs(&headers);
            let rate_limit =
                status == 429 || (status == 403 && GmailError::is_transient(403, reason));
            if GmailError::is_transient(status, reason)
                && (call.idempotent || rate_limit)
                && attempt < self.retry.max_retries
            {
                let wait = retry_after.map_or_else(
                    || self.retry.backoff(attempt),
                    |s| Duration::from_secs(u64::from(s)),
                );
                if wait <= self.retry.max_inline_wait {
                    tracing::debug!(what = %call.what, status, ?wait, "Gmail asks to retry");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                    continue;
                }
            }
            return Err(GmailError::from_status(
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
    ) -> Result<(u16, HeaderMap, Vec<u8>), GmailError> {
        let mut request = self.http.request(call.method.clone(), call.url.clone());
        let mut value = HeaderValue::from_str(&self.authorization(force_refresh).await?)
            .map_err(|_| GmailError::Protocol("the token is not valid header text".into()))?;
        value.set_sensitive(true);
        request = request
            .header(reqwest::header::AUTHORIZATION, value)
            .header(reqwest::header::ACCEPT, "application/json");
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
        let sent = match &call.body {
            Body::None => None,
            Body::Json(bytes) | Body::Raw { bytes, .. } => {
                Some(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            }
        };
        let exchange = self
            .tap
            .request(call.method.as_str(), call.url.as_str(), true, sent);
        let mut response = request.send().await.map_err(|e| {
            let err = transport_error(e);
            exchange.failed(&err.to_string());
            err
        })?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        if response
            .content_length()
            .is_some_and(|l| l > MAX_RESPONSE as u64)
        {
            exchange.failed("answer too large");
            return Err(GmailError::TooLarge(format!("answer to {}", call.what)));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if body.len() + chunk.len() > MAX_RESPONSE {
                exchange.failed("answer too large");
                return Err(GmailError::TooLarge(format!("answer to {}", call.what)));
            }
            body.extend_from_slice(&chunk);
        }
        exchange.response(
            status,
            Some(u64::try_from(body.len()).unwrap_or(u64::MAX)),
            Some(&body),
        );
        Ok((status, headers, body))
    }

    /// A JSON request below the base URL.
    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, String)],
        body: Option<&(impl Serialize + ?Sized)>,
        idempotent: bool,
        what: &str,
    ) -> Result<T, GmailError> {
        let reply = self
            .call(method, segments, query, body, idempotent, what)
            .await?;
        parse_json(&reply.body, what)
    }

    /// A request below the base URL whose answer body is not needed.
    pub(crate) async fn call(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, String)],
        body: Option<&(impl Serialize + ?Sized)>,
        idempotent: bool,
        what: &str,
    ) -> Result<Reply, GmailError> {
        let body = match body {
            Some(b) => {
                Body::Json(serde_json::to_vec(b).map_err(|e| GmailError::Protocol(e.to_string()))?)
            }
            None => Body::None,
        };
        let call = Call {
            method,
            url: self.url(segments, query),
            body,
            idempotent,
            what: what.to_owned(),
        };
        self.execute(&call).await
    }

    /// `users.getProfile`: the address and the current history id.
    pub async fn profile(&self) -> Result<Profile, GmailError> {
        self.json(
            Method::GET,
            &["profile"],
            &[],
            None::<&()>,
            true,
            "users.getProfile",
        )
        .await
    }
}

/// Parses a JSON answer.
pub(crate) fn parse_json<T: DeserializeOwned>(body: &[u8], what: &str) -> Result<T, GmailError> {
    serde_json::from_slice(body).map_err(|e| GmailError::Protocol(format!("{what}: {e}")))
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

/// Maps a reqwest error to a [`GmailError`] (TLS problems are told apart
/// by their source chain).
fn transport_error(error: reqwest::Error) -> GmailError {
    let error = error.without_url();
    if error.is_timeout() {
        return GmailError::Timeout;
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
        GmailError::Tls(chain)
    } else {
        GmailError::Network(chain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    fn client(base: &str) -> Result<GmailClient, GmailError> {
        GmailClient::with_http(
            nail_auth::http_client().unwrap(),
            base,
            GmailAuth::Bearer("t".into()),
        )
    }

    #[test]
    fn url_policy() {
        for ok in [
            "https://gmail.googleapis.com/gmail/v1/users/me",
            "http://127.0.0.1:8080/gmail/v1/users/me",
            "http://localhost/gmail/v1/users/me/",
            "http://[::1]:9/gmail/v1/users/me",
        ] {
            assert!(client(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://gmail.googleapis.com/gmail/v1/users/me",
            "http://10.0.0.1/gmail/v1/users/me",
            "ftp://gmail.googleapis.com/",
            "not a url",
        ] {
            assert!(
                matches!(client(bad), Err(GmailError::InsecureUrl(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn urls_are_built_below_the_base() {
        let c = client("http://127.0.0.1:1/gmail/v1/users/me/").unwrap();
        assert_eq!(c.api_base().path(), "/gmail/v1/users/me");
        let url = c.url(
            &["messages", "a/b"],
            &[
                ("format", "raw".into()),
                ("q", "after:1 in:anywhere".into()),
            ],
        );
        assert_eq!(url.path(), "/gmail/v1/users/me/messages/a%2Fb");
        assert_eq!(url.query(), Some("format=raw&q=after%3A1+in%3Aanywhere"));
        assert_eq!(c.batch_url().as_str(), "http://127.0.0.1:1/batch/gmail/v1");
    }

    #[test]
    fn auth_debug_hides_secrets() {
        let text = format!("{:?}", GmailAuth::Bearer("secret-token".into()));
        assert!(!text.contains("secret-token"));
    }

    #[test]
    fn retry_after_values() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
        assert_eq!(parse_retry_after("120", now), Some(120));
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
    fn backoff_doubles_up_to_the_cap() {
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
        assert_eq!(RetryPolicy::none().max_retries, 0);
    }
}
