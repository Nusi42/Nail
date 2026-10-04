//! The HTTP seam of the common layer (Autodiscover, Windows sign-in).
//!
//! [`ExchangeHttp`] sends one request and never follows redirects (the
//! callers check every hop: credentials must not leak to another host,
//! ADR 0075). [`ReqwestHttp`] is the real implementation on a
//! `nail_auth` client (rustls + ring, platform verifier, no redirects); it
//! can route every request to a loopback mock server
//! ([`ReqwestHttp::route_all_to`]) the way `nail-discovery` does, keeping
//! the original host in the `Host` header. Tests use scripted fakes.

use std::fmt;
use std::net::SocketAddr;

use nail_api::engine::BoxFuture;
use url::Url;

/// Largest response body read (Autodiscover answers are a few KB).
pub const MAX_BODY: usize = 1024 * 1024;

/// An HTTP method the common layer uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
}

impl Method {
    /// The method's name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// A request.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Method.
    pub method: Method,
    /// Target.
    pub url: Url,
    /// Extra headers (name, value).
    pub headers: Vec<(String, String)>,
    /// Body (empty for `GET`).
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpRequest {
    // Never print `Authorization` values (tokens, Basic passwords).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<String> = self
            .headers
            .iter()
            .map(|(name, value)| {
                if name.eq_ignore_ascii_case("authorization") {
                    format!("{name}: <redacted>")
                } else {
                    format!("{name}: {value}")
                }
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url.as_str())
            .field("headers", &headers)
            .field("body_len", &self.body.len())
            .finish()
    }
}

impl HttpRequest {
    /// A `GET`.
    pub fn get(url: Url) -> Self {
        Self {
            method: Method::Get,
            url,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// A `POST` with a body of `content_type`.
    pub fn post(url: Url, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            method: Method::Post,
            url,
            headers: vec![("Content-Type".to_owned(), content_type.to_owned())],
            body,
        }
    }

    /// Adds a header.
    #[must_use]
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    /// The request with `Authorization` set (replacing an earlier one).
    #[must_use]
    pub fn with_authorization(mut self, value: String) -> Self {
        self.headers
            .retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
        self.headers.push(("Authorization".to_owned(), value));
        self
    }

    /// Whether the request carries credentials.
    pub fn has_authorization(&self) -> bool {
        self.headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
    }
}

/// A response.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Headers (name, value), in order; a header sent several times
    /// appears several times.
    pub headers: Vec<(String, String)>,
    /// Body (at most [`MAX_BODY`]).
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// A response with a status and body.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// Adds a header.
    #[must_use]
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    /// The first value of a header (case-insensitive name).
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Every value of a header.
    pub fn headers_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.headers
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Whether this is a redirect (`301`, `302`, `303`, `307`, `308`).
    pub fn is_redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308)
    }
}

/// A failed request (no response): English diagnostic, never secrets.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct HttpError {
    /// What went wrong.
    pub message: String,
    /// The TLS handshake failed (untrusted certificate, wrong name).
    pub tls: bool,
}

impl HttpError {
    /// A network failure.
    pub fn network(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            tls: false,
        }
    }
}

/// Sends HTTP requests without following redirects.
pub trait ExchangeHttp: Send + Sync + fmt::Debug {
    /// Sends `request` and answers the response (any status).
    fn send<'a>(
        &'a self,
        request: &'a HttpRequest,
    ) -> BoxFuture<'a, Result<HttpResponse, HttpError>>;
}

/// [`ExchangeHttp`] on a [`reqwest::Client`] (build it with
/// `nail_auth::http_client()`: rustls, platform verifier, no redirects).
#[derive(Debug, Clone)]
pub struct ReqwestHttp {
    client: reqwest::Client,
    route_all_to: Option<SocketAddr>,
}

impl ReqwestHttp {
    /// Sends with `client` (which must not follow redirects).
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            route_all_to: None,
        }
    }

    /// A client with Nail's defaults (`nail_auth::http_client`).
    pub fn system() -> Result<Self, HttpError> {
        let client = nail_auth::http_client().map_err(|e| HttpError::network(e.to_string()))?;
        Ok(Self::new(client))
    }

    /// Sends every request over plain HTTP to `addr` (must be loopback)
    /// instead, keeping the original host in the `Host` header and the
    /// path and query as they are: for tests and end-to-end runs against
    /// `testing::MockAutodiscoverServer` (feature `testing`).
    #[must_use]
    pub fn route_all_to(mut self, addr: SocketAddr) -> Self {
        debug_assert!(addr.ip().is_loopback(), "route_all_to needs loopback");
        self.route_all_to = Some(addr).filter(|a| a.ip().is_loopback());
        self
    }

    fn target(&self, url: &Url) -> (Url, Option<String>) {
        match self.route_all_to {
            None => (url.clone(), None),
            Some(addr) => {
                let mut routed = url.clone();
                let host = match url.port() {
                    Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
                    None => url.host_str().unwrap_or_default().to_owned(),
                };
                // The setters only fail for URLs that cannot have a host.
                let _ = routed.set_scheme("http");
                let _ = routed.set_ip_host(addr.ip());
                let _ = routed.set_port(Some(addr.port()));
                (routed, Some(host))
            }
        }
    }
}

fn is_tls_error(error: &reqwest::Error) -> bool {
    let mut source: Option<&dyn std::error::Error> = Some(error);
    while let Some(e) = source {
        let text = e.to_string().to_ascii_lowercase();
        if text.contains("certificate") || text.contains("tls") || text.contains("handshake") {
            return true;
        }
        source = e.source();
    }
    false
}

impl ExchangeHttp for ReqwestHttp {
    fn send<'a>(
        &'a self,
        request: &'a HttpRequest,
    ) -> BoxFuture<'a, Result<HttpResponse, HttpError>> {
        Box::pin(async move {
            let (target, host) = self.target(&request.url);
            let method = match request.method {
                Method::Get => reqwest::Method::GET,
                Method::Post => reqwest::Method::POST,
            };
            let mut builder = self.client.request(method, target);
            if let Some(host) = host {
                builder = builder.header(reqwest::header::HOST, host);
            }
            for (name, value) in &request.headers {
                builder = builder.header(name.as_str(), value.as_str());
            }
            if request.method == Method::Post {
                builder = builder.body(request.body.clone());
            }
            let response = builder.send().await.map_err(|e| HttpError {
                tls: is_tls_error(&e),
                message: e.without_url().to_string(),
            })?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|v| (name.as_str().to_owned(), v.to_owned()))
                })
                .collect();
            let mut response = response;
            if response
                .content_length()
                .is_some_and(|l| l > MAX_BODY as u64)
            {
                return Err(HttpError::network("response too large"));
            }
            // Read in chunks and stop at the limit: Autodiscover talks to
            // untrusted hosts, and a chunked answer without an end must not
            // fill the memory (ADR 0078).
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| HttpError::network(e.without_url().to_string()))?
            {
                if body.len() + chunk.len() > MAX_BODY {
                    return Err(HttpError::network("response too large"));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(HttpResponse {
                status,
                headers,
                body,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_endless_answer_is_cut_at_the_limit() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            drop(socket.read(&mut request).await);
            drop(socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nTransfer-Encoding: chunked\r\n\r\n",
                )
                .await);
            // Chunks without an end.
            let chunk = format!("{:x}\r\n{}\r\n", 64 * 1024, "x".repeat(64 * 1024));
            while socket.write_all(chunk.as_bytes()).await.is_ok() {}
        });
        let http = ReqwestHttp::system().unwrap();
        let url = Url::parse(&format!("http://{addr}/autodiscover/autodiscover.xml")).unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            http.send(&HttpRequest::get(url)),
        )
        .await
        .expect("cut at the limit, not read until the end");
        assert_eq!(result.unwrap_err().message, "response too large");
    }

    #[test]
    fn requests_never_print_credentials() {
        let request = HttpRequest::get(Url::parse("https://mail.corp.example/a").unwrap())
            .with_authorization("Basic c2VjcmV0".into());
        let text = format!("{request:?}");
        assert!(!text.contains("c2VjcmV0"), "{text}");
        assert!(text.contains("<redacted>"));
        assert!(request.has_authorization());
        let replaced = request.with_authorization("Negotiate abc".into());
        assert_eq!(
            replaced
                .headers
                .iter()
                .filter(|(n, _)| n == "Authorization")
                .count(),
            1
        );
    }

    #[test]
    fn response_headers_are_case_insensitive() {
        let response = HttpResponse::new(401, "")
            .header("WWW-Authenticate", "Negotiate")
            .header("www-authenticate", "NTLM");
        assert_eq!(
            response
                .headers_named("Www-Authenticate")
                .collect::<Vec<_>>(),
            ["Negotiate", "NTLM"]
        );
        assert_eq!(response.header_value("location"), None);
        assert!(!response.is_redirect());
        assert!(HttpResponse::new(302, "").is_redirect());
    }

    #[test]
    fn routing_keeps_the_host() {
        let http = ReqwestHttp::system()
            .unwrap()
            .route_all_to("127.0.0.1:4000".parse().unwrap());
        let (target, host) =
            http.target(&Url::parse("https://autodiscover.corp.example/x?y=1").unwrap());
        assert_eq!(target.as_str(), "http://127.0.0.1:4000/x?y=1");
        assert_eq!(host.as_deref(), Some("autodiscover.corp.example"));
    }
}
