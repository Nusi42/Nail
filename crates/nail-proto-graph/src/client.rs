//! The HTTP side: the API URL policy, OAuth2 bearer tokens (one forced
//! refresh on `401`), throttling (`429` / `503` / `504` with
//! `Retry-After`) and server-error backoff, paging links that must stay on
//! the API's origin, response size limits and error mapping.

use nail_net::api::{ApiCall, ApiClient, ApiReply};
use nail_net::http::{self, Body, Retries, TransportFailure as _};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::error::GraphError;
use crate::wire::{ErrorBody, User};

/// Largest answer read (a 150 MB message is beyond Exchange Online's
/// limits; `$value` answers are raw MIME).
pub use nail_net::http::MAX_EXCHANGE_ANSWER as MAX_RESPONSE;

/// How the client signs in: a fixed bearer token (tests) or OAuth2 tokens
/// from the core's token manager (provider `microsoft`), refreshed once
/// when Graph answers `401`.
pub type GraphAuth = http::ApiAuth;

/// Inline retries of throttling and server errors (backoff or the
/// server's `Retry-After`; a longer wait goes to the caller with
/// [`GraphError::retry_after_secs`], the core's poll loop waits instead).
pub use nail_net::http::RetryPolicy;

/// Whether the client may talk to `url`: `https:`, or `http:` to a
/// loopback host (the mock server; ADR 0072).
pub use nail_net::http::is_allowed_url;

/// Largest answer to an upload range (a small JSON status).
const UPLOAD_ANSWER_LIMIT: usize = 1024 * 1024;

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

/// A request (extra headers: `Prefer`, `If-Match`; reads, `PATCH`es and
/// deletes are idempotent, `sendMail`, creates and `move` are not).
pub(crate) type Call = ApiCall;

/// A successful answer.
pub(crate) type Reply = ApiReply;

/// A Microsoft Graph client (`https://graph.microsoft.com/v1.0`).
#[derive(Debug, Clone)]
pub struct GraphClient {
    api: ApiClient,
}

nail_net::api_client_wrapper!(GraphClient, GraphError, MAX_RESPONSE);

impl GraphClient {
    /// A client on Nail's TLS configuration (rustls with ring and the
    /// platform verifier, no redirects) for the API base URL `api_base`
    /// (see [`crate::api_base`]).
    pub fn new(api_base: &str, auth: GraphAuth) -> Result<Self, GraphError> {
        let http = nail_auth::http_client().map_err(|e| GraphError::Tls(e.to_string()))?;
        Self::with_http(http, api_base, auth)
    }

    /// The URL of `path` (starting with `/`, segments already encoded with
    /// [`crate::path_segment`]) below the base, with `query`.
    pub(crate) fn url(&self, path: &str, query: &[(&str, String)]) -> Result<Url, GraphError> {
        let text = format!("{}{path}", self.api_base().as_str().trim_end_matches('/'));
        let mut url =
            Url::parse(&text).map_err(|e| GraphError::Protocol(format!("{path}: {e}")))?;
        http::append_query(&mut url, query);
        Ok(url)
    }

    /// Whether `url` is on the API's origin (paging and delta links must
    /// be: the bearer token never goes anywhere else).
    pub fn same_origin(&self, url: &Url) -> bool {
        self.api.same_origin(url)
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

    /// Sends `call` (its headers with [`IMMUTABLE_IDS`]): one forced token
    /// refresh on `401`, inline retries per the [`RetryPolicy`]. Non-2xx
    /// answers become [`GraphError`]s.
    pub(crate) async fn execute(&self, call: &Call) -> Result<Reply, GraphError> {
        self.api
            .execute(call, &with_immutable_ids(&call.headers))
            .await
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
            call.body = Body::json(body).map_err(|e| GraphError::Protocol(e.to_string()))?;
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

    /// `PUT` of one byte range to an upload session's pre-authenticated URL
    /// (`Content-Range: bytes start-end/total`). The URL may be on another
    /// host than the API (Outlook's upload service) but must pass
    /// [`is_allowed_url`]; the bearer token is **never** sent there (the
    /// URL carries its own authorization). Ranges may be repeated, so
    /// transient failures are retried per the [`RetryPolicy`]. Answers the
    /// status (`200` = more wanted, `201` = complete) and the JSON body.
    pub(crate) async fn put_upload_range(
        &self,
        upload_url: &str,
        bytes: &[u8],
        start: u64,
        total: u64,
        what: &str,
    ) -> Result<(u16, Value), GraphError> {
        let url =
            Url::parse(upload_url).map_err(|e| GraphError::Protocol(format!("upload URL: {e}")))?;
        if !is_allowed_url(&url) {
            return Err(GraphError::InsecureUrl(url.origin().ascii_serialization()));
        }
        if bytes.is_empty() || start + bytes.len() as u64 > total {
            return Err(GraphError::Protocol(format!(
                "{what}: range {start}+{} of {total}",
                bytes.len()
            )));
        }
        let end = start + bytes.len() as u64 - 1;
        let mut tries = Retries::new(self.retry_policy());
        loop {
            let sent = self
                .api
                .http()
                .put(url.clone())
                .header(
                    reqwest::header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{total}"),
                )
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(bytes.to_vec())
                .send()
                .await
                .map_err(|e| GraphError::from(http::transport_error(e)));
            let mut response = match sent {
                Ok(r) => r,
                Err(e) if e.is_transport() && tries.left() => {
                    tracing::debug!(what, error = %e, "upload range failed, retrying");
                    tries.backoff().await;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            let body = http::read_limited(&mut response, UPLOAD_ANSWER_LIMIT)
                .await
                .map_err(|e| GraphError::from_body(e, &format!("answer to {what}")))?;
            if (200..300).contains(&status) {
                let value = if body.is_empty() {
                    Value::Null
                } else {
                    parse_json(&body, what)?
                };
                return Ok((status, value));
            }
            let retry_after = http::retry_after_secs(&headers);
            if GraphError::is_transient(status)
                && let Some(wait) = tries.wait(retry_after)
            {
                tries.sleep(wait).await;
                continue;
            }
            let error_body: Option<ErrorBody> = serde_json::from_slice(&body).ok();
            return Err(GraphError::from_status(
                status,
                retry_after,
                error_body.as_ref(),
                what,
            ));
        }
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
    http::parse_json(body, what).map_err(GraphError::Protocol)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(base: &str) -> Result<GraphClient, GraphError> {
        GraphClient::with_http(
            nail_auth::http_client().unwrap(),
            base,
            GraphAuth::Bearer("t".into()),
        )
    }

    #[test]
    fn url_policy() {
        assert!(client("https://graph.microsoft.com/v1.0").is_ok());
        assert!(client("http://localhost/v1.0/").is_ok());
        let insecure = client("http://graph.microsoft.com/v1.0");
        assert!(matches!(insecure, Err(GraphError::InsecureUrl(_))));
        assert!(matches!(
            client("ftp://graph.microsoft.com/"),
            Err(GraphError::InsecureUrl(_))
        ));
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
}
