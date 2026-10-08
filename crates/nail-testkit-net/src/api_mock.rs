//! What the in-memory mock API servers of the engine crates (Gmail, Graph,
//! EWS, Autodiscover; their `testing` features) share: serving an [`axum`]
//! router on a free loopback port, turning their replies into responses,
//! the request headers as pairs, and injected errors
//! ([`crate::injected_error!`]).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, Method, Response, StatusCode, Uri};
use tokio::task::JoinHandle;

#[doc(hidden)]
pub use serde_json::Value as __Value;

/// The first token a scripted (fake) Negotiate client sends to a mock
/// server (EWS, Autodiscover).
pub const NEGOTIATE_TOKEN: &[u8] = b"nail-negotiate";
/// The scripted client's answer to the mock's challenge, which completes
/// the fake handshake.
pub const AUTHENTICATE_TOKEN: &[u8] = b"nail-authenticate";

/// A request as a mock API server sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequest {
    /// The method (`GET`, …).
    pub method: String,
    /// The path, percent-encoded as sent.
    pub path: String,
    /// The query string (empty without one).
    pub query: String,
    /// The headers as (name, value) pairs.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

impl ApiRequest {
    /// The request of an axum handler's parts.
    pub fn new(method: &Method, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> Self {
        Self {
            method: method.as_str().to_owned(),
            path: uri.path().to_owned(),
            query: uri.query().unwrap_or_default().to_owned(),
            headers: header_pairs(headers),
            body: body.to_vec(),
        }
    }

    /// The first value of a header (case-insensitive name).
    pub fn header(&self, name: &str) -> Option<&str> {
        nail_util::named::first(&self.headers, name)
    }

    /// The path with the query (`/a?b=c`; the path alone without one).
    pub fn path_and_query(&self) -> String {
        if self.query.is_empty() {
            self.path.clone()
        } else {
            format!("{}?{}", self.path, self.query)
        }
    }
}

/// A mock API server's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockReply {
    /// The status.
    pub status: u16,
    /// The headers (name, value).
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

impl MockReply {
    /// An answer with `body` of `content_type` (`None`: no body headers).
    pub fn with_body(status: u16, content_type: Option<&str>, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: content_type
                .map(|t| ("content-type".to_owned(), t.to_owned()))
                .into_iter()
                .collect(),
            body,
        }
    }

    /// A JSON answer of the service's JSON `content_type`.
    pub fn json_as(status: u16, value: &serde_json::Value, content_type: &str) -> Self {
        Self::with_body(status, Some(content_type), value.to_string().into_bytes())
    }
}

/// The state of a mock API server: answers one request at a time.
pub trait MockService: Send + 'static {
    /// The answer to `request`.
    fn answer(&mut self, request: &ApiRequest) -> MockReply;
}

/// The controls every mock API server with a bearer-token check, injected
/// errors and a request log offers (`set_token`, `inject`, `requests`,
/// `clear_requests`); its `lock()` gives a state with the fields `token`,
/// `errors` and `requests`.
#[macro_export]
macro_rules! api_mock_controls {
    ($ty:ident) => {
        impl $ty {
            /// Accepts only this bearer token (`None` = any non-empty one).
            pub fn set_token(&self, token: Option<&str>) {
                self.lock().token = token.map(str::to_owned);
            }

            /// Answers the next matching request(s) with `error`.
            pub fn inject(&self, error: InjectedError) {
                self.lock().errors.push(error);
            }

            /// The requests so far.
            pub fn requests(&self) -> Vec<RecordedRequest> {
                self.lock().requests.clone()
            }

            /// Forgets the request log.
            pub fn clear_requests(&self) {
                self.lock().requests.clear();
            }
        }
    };
}

/// A running mock API server on a free loopback port (ADR 0057) with its
/// state; stops when dropped.
#[derive(Debug)]
pub struct MockApi<S> {
    addr: SocketAddr,
    state: Arc<Mutex<S>>,
    task: JoinHandle<()>,
}

impl<S: MockService> MockApi<S> {
    /// Starts serving `state`, accepting request bodies of up to
    /// `body_limit` bytes.
    pub async fn start(state: S, body_limit: usize) -> std::io::Result<Self> {
        let state = Arc::new(Mutex::new(state));
        let app = Router::new()
            .fallback(handle::<S>)
            .layer(DefaultBodyLimit::max(body_limit))
            .with_state(Arc::clone(&state));
        let (addr, task) = serve(app).await?;
        Ok(Self { addr, state, task })
    }

    /// The server's address (127.0.0.1 and its port).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The state, locked.
    pub fn lock(&self) -> MutexGuard<'_, S> {
        lock(&self.state)
    }
}

impl<S> Drop for MockApi<S> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn handle<S: MockService>(
    State(state): State<Arc<Mutex<S>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let request = ApiRequest::new(&method, &uri, &headers, &body);
    let reply = lock(&state).answer(&request);
    response(reply.status, reply.headers, reply.body)
}

/// Locks a mock's state, also after a panic of another holder.
pub fn lock<T>(state: &Mutex<T>) -> MutexGuard<'_, T> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Serves `app` on a free loopback port (ADR 0057) until the task is
/// aborted; answers the address and the task.
pub async fn serve(app: Router) -> std::io::Result<(SocketAddr, JoinHandle<()>)> {
    let listener = crate::bind_loopback().await?;
    let addr = listener.local_addr()?;
    let task = tokio::spawn(async move {
        drop(axum::serve(listener, app).await);
    });
    Ok((addr, task))
}

/// A router served on a free loopback port (ADR 0057) until dropped
/// (graceful shutdown, then the task is aborted).
#[derive(Debug)]
pub struct Served {
    addr: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Served {
    /// Starts serving `app`.
    pub async fn start(app: Router) -> std::io::Result<Self> {
        let listener = crate::bind_loopback().await?;
        let addr = listener.local_addr()?;
        let (shutdown, mut stop) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            drop(
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        drop(stop.changed().await);
                    })
                    .await,
            );
        });
        Ok(Self {
            addr,
            shutdown,
            task,
        })
    }

    /// The listening address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

/// The request's headers as (name, value) pairs (values that are not text
/// are left out).
pub fn header_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(n, v)| {
            v.to_str()
                .ok()
                .map(|v| (n.as_str().to_owned(), v.to_owned()))
        })
        .collect()
}

/// A mock's reply as an HTTP response (an unknown status becomes `500`).
pub fn response(
    status: u16,
    headers: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
    body: Vec<u8>,
) -> Response<Body> {
    let mut response = Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR));
    for (name, value) in headers {
        response = response.header(name.as_ref(), value.as_ref());
    }
    response
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

/// Defines `InjectedError` in a mock API module: an error the server
/// answers instead of handling a request, with the request filter
/// (`on`, `method`, `times`) and `status(status)` whose JSON body comes
/// from `$body(status)`. The module adds its own named constructors.
#[macro_export]
macro_rules! injected_error {
    ($body:expr) => {
        /// An error the server answers instead of handling a request.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct InjectedError {
            /// Only this method (`GET`, `POST`, …); `None` = any.
            pub method: Option<String>,
            /// Only requests whose path + query contain this; `None` = any.
            pub path_contains: Option<String>,
            /// HTTP status.
            pub status: u16,
            /// The JSON body.
            pub body: $crate::api_mock::__Value,
            /// A `Retry-After` header.
            pub retry_after: Option<String>,
            /// How many requests it answers before it is gone.
            pub times: u32,
        }

        impl InjectedError {
            /// An error with `status` and the service's body for it.
            pub fn status(status: u16) -> Self {
                Self {
                    method: None,
                    path_contains: None,
                    status,
                    body: $body(status),
                    retry_after: None,
                    times: 1,
                }
            }

            /// Only for requests whose path + query contain `text`.
            #[must_use]
            pub fn on(mut self, text: &str) -> Self {
                self.path_contains = Some(text.to_owned());
                self
            }

            /// Only for `method`.
            #[must_use]
            pub fn method(mut self, method: &str) -> Self {
                self.method = Some(method.to_ascii_uppercase());
                self
            }

            /// Answers `n` requests (at least 1).
            #[must_use]
            pub fn times(mut self, n: u32) -> Self {
                self.times = n.max(1);
                self
            }

            /// With a `Retry-After` of `secs` (if any).
            #[must_use]
            pub fn retry_after(mut self, secs: Option<u32>) -> Self {
                self.retry_after = secs.map(|s| s.to_string());
                self
            }

            fn matches(&self, method: &str, target: &str) -> bool {
                self.method.as_deref().is_none_or(|m| m == method)
                    && self
                        .path_contains
                        .as_deref()
                        .is_none_or(|p| target.contains(p))
            }

            /// The error due for `method` on `target` (path + query), counted
            /// down and dropped when used up.
            pub(crate) fn take_due(
                errors: &mut Vec<Self>,
                method: &str,
                target: &str,
            ) -> Option<Self> {
                let at = errors.iter().position(|e| e.matches(method, target))?;
                let error = &mut errors[at];
                let due = error.clone();
                error.times = error.times.saturating_sub(1);
                if error.times == 0 {
                    errors.remove(at);
                }
                Some(due)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    fn body_for(status: u16) -> serde_json::Value {
        serde_json::json!({ "status": status })
    }

    #[expect(
        unreachable_pub,
        reason = "the macro defines a public type for the mock crates that use it"
    )]
    mod injected {
        crate::injected_error!(super::body_for);
    }

    use injected::InjectedError;

    #[test]
    fn injected_errors_filter_and_count_down() {
        let mut errors = vec![
            InjectedError::status(429)
                .on("/messages")
                .method("get")
                .times(2)
                .retry_after(Some(3)),
        ];
        assert_eq!(errors[0].body, serde_json::json!({"status": 429}));
        assert!(InjectedError::take_due(&mut errors, "POST", "/messages?").is_none());
        assert!(InjectedError::take_due(&mut errors, "GET", "/labels?").is_none());
        let first = InjectedError::take_due(&mut errors, "GET", "/messages?q=x").unwrap();
        assert_eq!(first.retry_after.as_deref(), Some("3"));
        assert!(InjectedError::take_due(&mut errors, "GET", "/messages?").is_some());
        assert_eq!(errors.len(), 0);
        assert_eq!(InjectedError::status(500).times(0).times, 1);
    }

    #[derive(Debug, Default)]
    struct Echo {
        seen: Vec<ApiRequest>,
    }

    impl MockService for Echo {
        fn answer(&mut self, request: &ApiRequest) -> MockReply {
            self.seen.push(request.clone());
            let mut reply = MockReply::with_body(202, None, request.body.clone());
            reply
                .headers
                .push(("x-path".to_owned(), request.path.clone()));
            reply
        }
    }

    #[test]
    fn replies() {
        let json = MockReply::json_as(200, &serde_json::json!([1]), "application/json");
        assert_eq!(json.body, b"[1]");
        assert_eq!(
            json.headers,
            [("content-type".to_owned(), "application/json".to_owned())]
        );
        assert_eq!(MockReply::with_body(204, None, Vec::new()).headers.len(), 0);
    }

    #[tokio::test]
    async fn mock_api_serves_its_state() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let server = MockApi::start(Echo::default(), 1024).await.unwrap();
        assert!(server.addr().ip().is_loopback());
        let mut tcp = tokio::net::TcpStream::connect(server.addr()).await.unwrap();
        tcp.write_all(b"POST /a%20b?x=1 HTTP/1.1\r\nHost: m\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi")
            .await
            .unwrap();
        let mut answer = String::new();
        tcp.read_to_string(&mut answer).await.unwrap();
        assert!(answer.starts_with("HTTP/1.1 202"), "{answer}");
        assert!(answer.contains("x-path: /a%20b"), "{answer}");
        assert!(answer.ends_with("hi"), "{answer}");
        let seen = server.lock().seen.clone();
        assert_eq!(seen[0].header("HOST"), Some("m"));
        assert_eq!(seen[0].path_and_query(), "/a%20b?x=1");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].query, "x=1");
        assert!(seen[0].headers.iter().any(|(n, v)| n == "host" && v == "m"));
    }

    #[tokio::test]
    async fn responses_and_headers() {
        let reply = response(201, [("x-a", "1")], b"hi".to_vec());
        assert_eq!(reply.status(), StatusCode::CREATED);
        assert_eq!(reply.headers()["x-a"], "1");
        let body = to_bytes(reply.into_body(), 16).await.unwrap();
        assert_eq!(&body[..], b"hi");
        assert_eq!(
            response(1000, Vec::<(&str, &str)>::new(), Vec::new()).status(),
            500
        );
        let mut headers = HeaderMap::new();
        headers.insert("x-b", "2".parse().unwrap());
        assert_eq!(header_pairs(&headers), [("x-b".to_owned(), "2".to_owned())]);
        let (addr, task) = serve(Router::new()).await.unwrap();
        assert!(addr.ip().is_loopback());
        task.abort();
        assert_eq!(*lock(&Mutex::new(5)), 5);
    }
}
