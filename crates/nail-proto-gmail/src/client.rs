//! The HTTP side: the API URL policy, OAuth2 bearer tokens (one forced
//! refresh on `401`), quota / `429` / `5xx` backoff with `Retry-After`,
//! response size limits and error mapping.

use nail_net::api::{ApiCall, ApiClient, ApiReply};
use nail_net::http::{self, Body};
use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::GmailError;
use crate::wire::Profile;

/// Largest answer read (a 35 MB message as base64url in JSON stays below).
pub use nail_net::http::MAX_ANSWER as MAX_RESPONSE;

/// How the client signs in: a fixed bearer token (tests) or OAuth2 tokens
/// from the core's token manager (provider `google`), refreshed once when
/// the API answers `401`.
pub type GmailAuth = http::ApiAuth;

/// Inline retries of quota, rate-limit and server errors (backoff or the
/// server's `Retry-After`; a longer wait goes to the caller with
/// [`GmailError::retry_after_secs`], the core's poll loop waits instead).
pub use nail_net::http::RetryPolicy;

/// Whether the client may talk to `url`: `https:`, or `http:` to a
/// loopback host (the mock server; ADR 0065).
pub use nail_net::http::is_allowed_url;

/// A request (no extra headers; reads and label changes are idempotent,
/// `send` / `insert` / creates are not).
pub(crate) type Call = ApiCall;

/// A successful answer.
pub(crate) type Reply = ApiReply;

/// A Gmail API client for one mailbox (`…/gmail/v1/users/me`).
#[derive(Debug, Clone)]
pub struct GmailClient {
    api: ApiClient,
}

nail_net::api_client_wrapper!(GmailClient, GmailError, MAX_RESPONSE);

impl GmailClient {
    /// A client on Nail's TLS configuration (rustls with ring and the
    /// platform verifier, no redirects) for the API base URL `api_base`
    /// (see [`crate::api_base`]).
    pub fn new(api_base: &str, auth: GmailAuth) -> Result<Self, GmailError> {
        let http = nail_auth::http_client().map_err(|e| GmailError::Tls(e.to_string()))?;
        Self::with_http(http, api_base, auth)
    }

    /// The URL of `segments` below the base, with `query`.
    pub(crate) fn url(&self, segments: &[&str], query: &[(&str, String)]) -> Url {
        let mut url = self.api.base().clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.extend(segments);
        }
        http::append_query(&mut url, query);
        url
    }

    /// The batch endpoint (`<origin>/batch/gmail/v1`).
    pub(crate) fn batch_url(&self) -> Url {
        let mut url = self.api.base().clone();
        url.set_path("/batch/gmail/v1");
        url
    }

    /// Sends `call`: one forced token refresh on `401`, inline retries per
    /// the [`RetryPolicy`]. Non-2xx answers become [`GmailError`]s.
    pub(crate) async fn execute(&self, call: &Call) -> Result<Reply, GmailError> {
        self.api.execute(call, &call.headers).await
    }

    /// A JSON request below the base URL (`T = IgnoredAny` when the answer
    /// does not matter).
    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, String)],
        body: Option<&(impl Serialize + ?Sized)>,
        idempotent: bool,
        what: &str,
    ) -> Result<T, GmailError> {
        let body = match body {
            Some(b) => Body::json(b).map_err(|e| GmailError::Protocol(e.to_string()))?,
            None => Body::None,
        };
        let call = Call {
            method,
            url: self.url(segments, query),
            body,
            headers: Vec::new(),
            idempotent,
            what: what.to_owned(),
        };
        parse_json(&self.execute(&call).await?.body, what)
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

/// Parses a JSON answer; an empty body reads as `null` (answers without
/// a body, read as `()`).
pub(crate) fn parse_json<T: DeserializeOwned>(body: &[u8], what: &str) -> Result<T, GmailError> {
    let body = if body.iter().all(u8::is_ascii_whitespace) {
        b"null"
    } else {
        body
    };
    http::parse_json(body, what).map_err(GmailError::Protocol)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(base: &str) -> Result<GmailClient, GmailError> {
        GmailClient::with_http(
            nail_auth::http_client().unwrap(),
            base,
            GmailAuth::Bearer("t".into()),
        )
    }

    #[test]
    fn url_policy() {
        assert!(client("https://gmail.googleapis.com/gmail/v1/users/me").is_ok());
        assert!(client("http://127.0.0.1:8080/gmail/v1/users/me").is_ok());
        let insecure = client("http://gmail.googleapis.com/gmail/v1/users/me");
        assert!(matches!(insecure, Err(GmailError::InsecureUrl(_))));
        assert!(matches!(
            client("not a url"),
            Err(GmailError::InsecureUrl(_))
        ));
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
}
