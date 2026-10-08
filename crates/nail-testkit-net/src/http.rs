//! A mock HTTP server for autoconfig, ISPDB and Autodiscover documents.
//!
//! Routes match on method, the `Host` header (port ignored) and the path, so
//! one server can stand in for `autoconfig.example.org`, `example.org`,
//! `autoconfig.thunderbird.net` and `autodiscover.example.org` at once: the
//! client under test sends every request to [`Served::addr`](crate::api_mock::Served::addr) and
//! keeps the original host name in the `Host` header. Unmatched requests get
//! `404`. Every request is recorded.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use crate::MockNetError;
use crate::api_mock::{self, ApiRequest, Served};
use crate::server::lock;

/// A canned response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockResponse {
    /// HTTP status.
    pub status: u16,
    /// `Content-Type` header.
    pub content_type: String,
    /// Body.
    pub body: Vec<u8>,
    /// Extra headers (e.g. `Location`).
    pub headers: Vec<(String, String)>,
    /// Wait this long before answering (timeouts).
    pub delay: Option<Duration>,
}

impl MockResponse {
    /// `200 OK` with an XML body.
    pub fn xml(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            content_type: "text/xml; charset=utf-8".to_owned(),
            body: body.into(),
            headers: Vec::new(),
            delay: None,
        }
    }

    /// An empty response with `status`.
    pub fn status(status: u16) -> Self {
        Self {
            status,
            content_type: "text/plain".to_owned(),
            body: Vec::new(),
            headers: Vec::new(),
            delay: None,
        }
    }

    /// Adds a header.
    #[must_use]
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Delays the answer.
    #[must_use]
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
}

/// A request the server received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    /// `GET`, `POST`, …
    pub method: String,
    /// `Host` header without port, lower case.
    pub host: String,
    /// Path (without query).
    pub path: String,
    /// Raw query string, if any.
    pub query: Option<String>,
    /// Request body.
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Route {
    method: Option<String>,
    host: Option<String>,
    path: String,
    response: MockResponse,
}

#[derive(Debug, Default)]
struct Inner {
    routes: Mutex<Vec<Route>>,
    requests: Mutex<Vec<RecordedRequest>>,
}

/// A running mock HTTP server on `127.0.0.1`. Stops when dropped.
#[derive(Debug)]
pub struct MockHttpServer {
    served: Served,
    inner: Arc<Inner>,
}

/// The address and base URL of the server.
impl std::ops::Deref for MockHttpServer {
    type Target = Served;

    fn deref(&self) -> &Served {
        &self.served
    }
}

impl MockHttpServer {
    /// Starts a server without routes on a free loopback port.
    pub async fn start() -> Result<Self, MockNetError> {
        let inner = Arc::new(Inner::default());
        let app = Router::new()
            .fallback(handle)
            .with_state(Arc::clone(&inner));
        Ok(Self {
            served: Served::start(app).await?,
            inner,
        })
    }

    /// Answers `GET <path>` for `host` (`None` = any host) with `response`.
    /// Later routes win over earlier ones for the same request.
    pub fn get(&self, host: Option<&str>, path: &str, response: MockResponse) {
        self.route(Some("GET"), host, path, response);
    }

    /// Answers `POST <path>` for `host` with `response`.
    pub fn post(&self, host: Option<&str>, path: &str, response: MockResponse) {
        self.route(Some("POST"), host, path, response);
    }

    /// Adds a route; `method: None` matches any method.
    pub fn route(
        &self,
        method: Option<&str>,
        host: Option<&str>,
        path: &str,
        response: MockResponse,
    ) {
        lock(&self.inner.routes).push(Route {
            method: method.map(str::to_ascii_uppercase),
            host: host.map(str::to_ascii_lowercase),
            path: path.to_owned(),
            response,
        });
    }

    /// Every request received, in order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        lock(&self.inner.requests).clone()
    }
}

async fn handle(
    State(inner): State<Arc<Inner>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = ApiRequest::new(&method, &uri, &headers, &body);
    let host = request
        .header("host")
        .map(|h| {
            // Strip the port (IPv6 literals keep their brackets).
            match h.rfind(':') {
                Some(i) if !h[i..].contains(']') => &h[..i],
                _ => h,
            }
            .to_ascii_lowercase()
        })
        .unwrap_or_default();
    let ApiRequest {
        method,
        path,
        query,
        body,
        ..
    } = request;
    lock(&inner.requests).push(RecordedRequest {
        method: method.clone(),
        host: host.clone(),
        path: path.clone(),
        query: (!query.is_empty()).then_some(query),
        body,
    });
    let matched = lock(&inner.routes)
        .iter()
        .rev()
        .find(|r| {
            r.path == path
                && r.method.as_deref().is_none_or(|m| m == method)
                && r.host.as_deref().is_none_or(|h| h == host)
        })
        .map(|r| r.response.clone());
    let Some(response) = matched else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Some(delay) = response.delay {
        tokio::time::sleep(delay).await;
    }
    let content_type = ("content-type".to_owned(), response.content_type);
    api_mock::response(
        response.status,
        std::iter::once(content_type).chain(response.headers),
        response.body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn raw(addr: SocketAddr, request: &str) -> String {
        let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        tcp.write_all(request.as_bytes()).await.unwrap();
        let mut out = String::new();
        tcp.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn routes_by_host_and_records() {
        let server = MockHttpServer::start().await.unwrap();
        server.get(
            Some("autoconfig.example.org"),
            "/mail/config-v1.1.xml",
            MockResponse::xml("<a/>"),
        );
        server.post(
            None,
            "/autodiscover/autodiscover.xml",
            MockResponse::status(401),
        );
        let ok = raw(
            server.addr(),
            "GET /mail/config-v1.1.xml?emailaddress=a%40example.org HTTP/1.1\r\nHost: autoconfig.example.org:443\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.ends_with("<a/>"));
        let other = raw(
            server.addr(),
            "GET /mail/config-v1.1.xml HTTP/1.1\r\nHost: example.org\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(other.starts_with("HTTP/1.1 404"));
        let post = raw(
            server.addr(),
            "POST /autodiscover/autodiscover.xml HTTP/1.1\r\nHost: autodiscover.example.org\r\nContent-Length: 3\r\nConnection: close\r\n\r\nxyz",
        )
        .await;
        assert!(post.starts_with("HTTP/1.1 401"));
        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].host, "autoconfig.example.org");
        assert_eq!(
            requests[0].query.as_deref(),
            Some("emailaddress=a%40example.org")
        );
        assert_eq!(requests[2].body, b"xyz");
        assert_eq!(requests[2].method, "POST");
    }
}
