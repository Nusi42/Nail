//! The HTTP side: authentication, URL policy, redirects, size limits,
//! error mapping.

use std::net::SocketAddr;
use std::sync::Arc;

use nail_api::accounts::SecretRef;
use nail_api::engine::AccessTokenProvider;
use nail_net::http;
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use url::Url;

use crate::error::DavError;

/// Largest response body accepted (a big address book with photos in one
/// multiget batch stays well below).
pub use nail_net::http::MAX_ANSWER as MAX_RESPONSE;
/// Most redirects followed for one request.
pub use nail_net::http::MAX_REDIRECTS;
/// The URL policy (`https:`, or `http:` to a loopback host) and the origin
/// a login belongs to.
pub use nail_net::http::{is_allowed_url, same_origin};

/// How the client signs in.
#[derive(Clone, Default)]
pub enum DavAuth {
    /// No credentials (public resources, tests).
    #[default]
    None,
    /// HTTP Basic: only ever sent over TLS (or to a loopback host).
    Basic {
        /// User name.
        username: String,
        /// Password or app password.
        password: String,
    },
    /// A fixed OAuth2 bearer token.
    Bearer(String),
    /// OAuth2 bearer tokens from a token provider (Google), refreshed once
    /// when the server answers `401`.
    OAuth2 {
        /// The token source (the core's token manager).
        tokens: Arc<dyn AccessTokenProvider>,
        /// The OAuth2 provider id (`google`).
        provider: String,
        /// Where the refresh token is stored.
        secret: SecretRef,
    },
}

nail_net::login_debug!(DavAuth);

/// `Depth` of a PROPFIND / REPORT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// The resource itself.
    Zero,
    /// The resource and its members.
    One,
}

impl Depth {
    fn as_str(self) -> &'static str {
        match self {
            Self::Zero => "0",
            Self::One => "1",
        }
    }
}

/// An HTTP answer.
#[derive(Debug, Clone)]
pub struct Reply {
    /// Status code.
    pub status: u16,
    /// The URL that answered (after redirects).
    pub url: Url,
    /// Headers.
    pub headers: HeaderMap,
    /// Body.
    pub body: Vec<u8>,
}

impl Reply {
    /// A header as text.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// `Retry-After` in seconds (the delta form only).
    pub fn retry_after(&self) -> Option<u64> {
        self.header("retry-after")
            .and_then(|v| v.trim().parse::<u64>().ok())
    }

    /// The body as text (lossy).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// `2xx`.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The error for this (unsuccessful) answer to `what`.
    pub fn error(&self, what: &str) -> DavError {
        DavError::from_status(self.status, self.retry_after(), what)
    }
}

/// Parses and checks a server URL ([`is_allowed_url`]).
pub fn parse_server_url(text: &str) -> Result<Url, DavError> {
    let url = Url::parse(text.trim()).map_err(|e| DavError::InsecureUrl(format!("{text}: {e}")))?;
    if !is_allowed_url(&url) || url.host_str().is_none_or(str::is_empty) {
        return Err(DavError::InsecureUrl(text.to_owned()));
    }
    Ok(url)
}

/// A CalDAV / CardDAV HTTP client.
#[derive(Debug, Clone)]
pub struct DavClient {
    http: reqwest::Client,
    auth: DavAuth,
    route_all_to: Option<SocketAddr>,
    /// The connection log (ADR 0190).
    tap: nail_connlog::HttpTap,
}

impl DavClient {
    /// A client on Nail's TLS configuration (rustls with ring and the
    /// platform verifier; no automatic redirects: they are followed here,
    /// with the URL policy applied to each hop).
    pub fn new(auth: DavAuth) -> Result<Self, DavError> {
        let http = nail_auth::http_client().map_err(|e| DavError::Tls(e.to_string()))?;
        Ok(Self::with_http(http, auth))
    }

    /// A client trusting only `roots` (DER) — for tests against local TLS
    /// servers and the interop suite.
    pub fn with_roots(roots: &[Vec<u8>], auth: DavAuth) -> Result<Self, DavError> {
        let tls =
            nail_auth::tls_config_with_roots(roots).map_err(|e| DavError::Tls(e.to_string()))?;
        let http = nail_auth::http_client_builder(tls)
            .build()
            .map_err(|e| DavError::Tls(e.to_string()))?;
        Ok(Self::with_http(http, auth))
    }

    /// A client on an existing [`reqwest::Client`]; it must not follow
    /// redirects itself (build it with `nail_auth::http_client_builder`).
    pub fn with_http(http: reqwest::Client, auth: DavAuth) -> Self {
        Self {
            http,
            auth,
            route_all_to: None,
            tap: nail_connlog::HttpTap::off(),
        }
    }

    /// Records every request into `tap` (ADR 0190).
    #[must_use]
    pub fn with_tap(mut self, tap: nail_connlog::HttpTap) -> Self {
        self.tap = tap;
        self
    }

    /// Sends every request over plain HTTP to `addr`, keeping the original
    /// host in the `Host` header (like `nail_discovery::ReqwestFetch`): for
    /// discovery tests and end-to-end runs against a mock server.
    #[must_use]
    pub fn route_all_to(mut self, addr: SocketAddr) -> Self {
        self.route_all_to = Some(addr);
        self
    }

    /// The configured sign-in.
    pub fn auth(&self) -> &DavAuth {
        &self.auth
    }

    async fn authorization(&self, force_refresh: bool) -> Result<Option<String>, DavError> {
        Ok(match &self.auth {
            DavAuth::None => None,
            DavAuth::Basic { username, password } => {
                self.tap.add_secret(password);
                Some(http::basic(username, password))
            }
            DavAuth::Bearer(token) => Some(http::bearer(token)),
            DavAuth::OAuth2 {
                tokens,
                provider,
                secret,
            } => Some(
                http::oauth2_bearer(tokens.as_ref(), provider, secret, force_refresh)
                    .await
                    .map_err(|e| DavError::Token(Box::new(e)))?,
            ),
        })
    }

    fn target(&self, url: &Url) -> (Url, Option<String>) {
        http::route(url, self.route_all_to, true)
    }

    /// Sends one request; follows redirects for safe methods (`GET`,
    /// `PROPFIND`, `REPORT`, `OPTIONS`); retries once with a fresh token
    /// when an OAuth2 token was refused. Answers with any status are
    /// returned; only transport problems are errors.
    pub async fn send(
        &self,
        method: Method,
        url: &Url,
        depth: Option<Depth>,
        headers: &[(HeaderName, String)],
        body: Option<(&str, Vec<u8>)>,
    ) -> Result<Reply, DavError> {
        let follow = matches!(
            method.as_str(),
            "GET" | "HEAD" | "PROPFIND" | "REPORT" | "OPTIONS"
        );
        let mut current = url.clone();
        let mut refreshed = false;
        let mut hops = 0;
        loop {
            if !is_allowed_url(&current) {
                return Err(DavError::InsecureUrl(current.to_string()));
            }
            // The login goes only to the origin of the requested URL, never
            // to another server a redirect names (like curl and browsers).
            let authorize = same_origin(&current, url);
            let reply = self
                .send_once(
                    &method,
                    &current,
                    depth,
                    headers,
                    body.as_ref(),
                    authorize.then_some(refreshed),
                )
                .await?;
            if reply.status == 401 && !authorize && !matches!(self.auth, DavAuth::None) {
                return Err(DavError::RedirectNeedsLogin(
                    current.host_str().unwrap_or_default().to_owned(),
                ));
            }
            if reply.status == 401 && !refreshed && matches!(self.auth, DavAuth::OAuth2 { .. }) {
                refreshed = true;
                continue;
            }
            if follow && http::is_redirect(reply.status) {
                let Some(location) = reply.header("location") else {
                    return Ok(reply);
                };
                hops += 1;
                if hops > MAX_REDIRECTS {
                    return Err(DavError::Protocol("too many redirects".to_owned()));
                }
                let next = current
                    .join(location)
                    .map_err(|e| DavError::Protocol(format!("bad redirect {location:?}: {e}")))?;
                if current.scheme() == "https" && next.scheme() != "https" {
                    return Err(DavError::InsecureUrl(next.to_string()));
                }
                tracing::debug!(from = %current, to = %next, "following a DAV redirect");
                current = next;
                continue;
            }
            return Ok(reply);
        }
    }

    async fn send_once(
        &self,
        method: &Method,
        url: &Url,
        depth: Option<Depth>,
        headers: &[(HeaderName, String)],
        body: Option<&(&str, Vec<u8>)>,
        // `Some(force_refresh)`: send the login (with a fresh token).
        login: Option<bool>,
    ) -> Result<Reply, DavError> {
        let (target, host) = self.target(url);
        let mut request = self.http.request(method.clone(), target);
        if let Some(host) = host {
            request = request.header(reqwest::header::HOST, host);
        }
        let authorization = match login {
            Some(force_refresh) => self.authorization(force_refresh).await?,
            None => None,
        };
        let authorized = authorization.is_some();
        if let Some(authorization) = authorization {
            http::hide_credential(&self.tap, &authorization);
            let mut value = HeaderValue::from_str(&authorization)
                .map_err(|_| DavError::Protocol("credentials are not valid header text".into()))?;
            value.set_sensitive(true);
            request = request.header(reqwest::header::AUTHORIZATION, value);
        }
        if let Some(depth) = depth {
            request = request.header("depth", depth.as_str());
        }
        for (name, value) in headers {
            request = request.header(name.clone(), value.as_str());
        }
        if let Some((content_type, bytes)) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, *content_type)
                .body(bytes.clone());
        }
        let exchange = self.tap.request(
            method.as_str(),
            url.as_str(),
            authorized,
            body.map(|(_, bytes)| http::logged_size(bytes)),
        );
        let (status, headers, body) = http::send_logged::<DavError>(
            request,
            &exchange,
            MAX_RESPONSE,
            &format!("answer from {url}"),
        )
        .await?;
        Ok(Reply {
            status,
            url: url.clone(),
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http() -> reqwest::Client {
        nail_auth::http_client().unwrap()
    }

    #[test]
    fn url_policy() {
        for ok in [
            "https://dav.example.org/",
            "http://127.0.0.1:5232/",
            "http://localhost/dav/",
            "http://[::1]:8080/",
        ] {
            assert!(parse_server_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://dav.example.org/",
            "ftp://dav.example.org/",
            "not a url",
            "http://10.0.0.1/",
            "file:///c:/x",
        ] {
            assert!(
                matches!(parse_server_url(bad), Err(DavError::InsecureUrl(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn auth_debug_hides_secrets() {
        let basic = DavAuth::Basic {
            username: "ada".into(),
            password: "hunter2".into(),
        };
        let text = format!("{basic:?}");
        assert!(text.contains("ada") && !text.contains("hunter2"));
        assert!(!format!("{:?}", DavAuth::Bearer("tok-123".into())).contains("tok-123"));
    }

    #[tokio::test]
    async fn routing_keeps_the_logical_host() {
        let client = DavClient::with_http(http(), DavAuth::None)
            .route_all_to("127.0.0.1:9".parse().unwrap());
        let (target, host) = client.target(&Url::parse("https://dav.example.org:8443/x/").unwrap());
        assert_eq!(target.as_str(), "http://127.0.0.1:9/x/");
        assert_eq!(host.as_deref(), Some("dav.example.org:8443"));
        let plain = DavClient::with_http(http(), DavAuth::None);
        let (target, host) = plain.target(&Url::parse("https://a.example/").unwrap());
        assert_eq!(target.as_str(), "https://a.example/");
        assert_eq!(host, None);
    }

    #[tokio::test]
    async fn basic_and_bearer_headers() {
        let client = DavClient::with_http(
            http(),
            DavAuth::Basic {
                username: "ada".into(),
                password: "secret".into(),
            },
        );
        assert_eq!(
            client.authorization(false).await.unwrap().as_deref(),
            Some("Basic YWRhOnNlY3JldA==")
        );
        let bearer = DavClient::with_http(http(), DavAuth::Bearer("t".into()));
        assert_eq!(
            bearer.authorization(false).await.unwrap().as_deref(),
            Some("Bearer t")
        );
        let none = DavClient::with_http(http(), DavAuth::None);
        assert_eq!(none.authorization(false).await.unwrap(), None);
    }
}
