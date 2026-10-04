//! A mock Gmail REST server for tests and the demo-multi seed (feature
//! `testing`; plan M3b WP82, ADR 0065).
//!
//! [`MockGmailServer`] binds 127.0.0.1 only (ADR 0057) and serves an
//! in-memory Gmail ([`Mailbox`]) under `/gmail/v1/users/me`: profile,
//! messages (list with the search terms Nail uses, get in every format,
//! modify, batchModify / batchDelete, trash, untrash, delete, insert /
//! import, send, attachments), media uploads (`/upload/…`), drafts.create,
//! history (with expiry → `404`), labels (palette and name checks), threads
//! and HTTP batch requests (`/batch/gmail/v1`). Tests drive it with
//! mutation helpers (server-side changes), injected errors
//! ([`InjectedError`], answered with recorded Gmail error bodies), a quota
//! in Gmail's units, token checks and a request log. Mailboxes load from
//! recorded-style fixtures ([`fixtures`]).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State as AxumState};
use axum::http::{HeaderMap, Method, Response, StatusCode, Uri};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub mod fixtures;
mod mailbox;
mod server;

pub use mailbox::{Fixture, FixtureMessage, Mailbox, MockError, MockMessage};
use server::{Quota, State, dispatch};

use crate::wire::Label;

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
    pub body: Value,
    /// A `Retry-After` header.
    pub retry_after: Option<String>,
    /// How many requests it answers before it is gone.
    pub times: u32,
}

impl InjectedError {
    /// An error with `status` and a body like Gmail's.
    pub fn status(status: u16) -> Self {
        Self {
            method: None,
            path_contains: None,
            status,
            body: server::generic_error(status),
            retry_after: None,
            times: 1,
        }
    }

    /// `429 rateLimitExceeded` (recorded), optionally with `Retry-After`.
    pub fn rate_limit(retry_after_secs: Option<u32>) -> Self {
        Self {
            body: server::recorded_error("429"),
            retry_after: retry_after_secs.map(|s| s.to_string()),
            ..Self::status(429)
        }
    }

    /// `403 userRateLimitExceeded` (recorded).
    pub fn user_rate_limit() -> Self {
        Self {
            body: server::recorded_error("403-rate"),
            ..Self::status(403)
        }
    }

    /// `500 backendError` (recorded).
    pub fn backend_error() -> Self {
        Self {
            body: server::recorded_error("500"),
            ..Self::status(500)
        }
    }

    /// `403` with insufficient scopes (recorded).
    pub fn insufficient_scope() -> Self {
        Self {
            body: server::recorded_error("403-scope"),
            ..Self::status(403)
        }
    }

    /// `404 notFound` (recorded).
    pub fn not_found() -> Self {
        Self {
            body: server::recorded_error("404"),
            ..Self::status(404)
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

    fn matches(&self, method: &str, target: &str) -> bool {
        self.method.as_deref().is_none_or(|m| m == method)
            && self
                .path_contains
                .as_deref()
                .is_none_or(|p| target.contains(p))
    }
}

/// A request the server saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    /// Method.
    pub method: String,
    /// Path (percent-encoded as sent).
    pub path: String,
    /// Query string.
    pub query: String,
    /// A part of a batch request.
    pub in_batch: bool,
    /// The bearer token.
    pub token: Option<String>,
}

type Shared = Arc<Mutex<State>>;

/// A running mock Gmail server; stops when dropped.
#[derive(Debug)]
pub struct MockGmailServer {
    addr: SocketAddr,
    state: Shared,
    task: JoinHandle<()>,
}

async fn handle(
    AxumState(state): AxumState<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let headers: Vec<(String, String)> = headers
        .iter()
        .filter_map(|(n, v)| {
            v.to_str()
                .ok()
                .map(|v| (n.as_str().to_owned(), v.to_owned()))
        })
        .collect();
    let reply = {
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        dispatch(
            &mut state,
            method.as_str(),
            uri.path(),
            uri.query().unwrap_or_default(),
            &headers,
            &body,
            false,
        )
    };
    let mut response = Response::builder()
        .status(StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR));
    for (name, value) in reply.headers {
        response = response.header(name, value);
    }
    response
        .body(Body::from(reply.body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

impl MockGmailServer {
    /// Starts a server on a free loopback port with an empty mailbox for
    /// `email` (Gmail's system labels, history id 1000). Any non-empty
    /// bearer token is accepted until [`Self::set_token`].
    pub async fn start(email: &str) -> std::io::Result<Self> {
        Self::start_with(Mailbox::new(email)).await
    }

    /// Starts a server with the mailbox of a fixture (JSON text).
    pub async fn start_fixture(json: &str) -> std::io::Result<Self> {
        let mailbox = Mailbox::from_fixture(json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Self::start_with(mailbox).await
    }

    /// Starts a server with the demo mailbox ([`fixtures::DEMO_MAILBOX`]).
    pub async fn start_demo() -> std::io::Result<Self> {
        Self::start_fixture(fixtures::DEMO_MAILBOX).await
    }

    /// Starts a server with `mailbox`.
    pub async fn start_with(mailbox: Mailbox) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let state: Shared = Arc::new(Mutex::new(State {
            mailbox,
            token: None,
            errors: Vec::new(),
            requests: Vec::new(),
            quota: None,
        }));
        let app = Router::new()
            .fallback(handle)
            .layer(DefaultBodyLimit::max(80 * 1024 * 1024))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            drop(axum::serve(listener, app).await);
        });
        Ok(Self { addr, state, task })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The server's address (127.0.0.1 and its port).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The API base URL (`http://127.0.0.1:<port>/gmail/v1/users/me`).
    pub fn api_base(&self) -> String {
        crate::api_base("127.0.0.1", self.addr.port(), false)
    }

    /// Accepts only this bearer token (`None` = any non-empty one).
    pub fn set_token(&self, token: Option<&str>) {
        self.lock().token = token.map(str::to_owned);
    }

    /// Answers the next matching request(s) with `error`.
    pub fn inject(&self, error: InjectedError) {
        self.lock().errors.push(error);
    }

    /// Limits the requests to `units` of Gmail's quota units from now on;
    /// beyond it the server answers `429` with `Retry-After`.
    pub fn set_quota(&self, units: Option<u64>, retry_after_secs: u32) {
        self.lock().quota = units.map(|limit| Quota {
            limit,
            used: 0,
            retry_after: retry_after_secs,
        });
    }

    /// The requests so far.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.lock().requests.clone()
    }

    /// Forgets the request log.
    pub fn clear_requests(&self) {
        self.lock().requests.clear();
    }

    /// Runs `f` on the mailbox (any server-side change or inspection).
    pub fn with_mailbox<T>(&self, f: impl FnOnce(&mut Mailbox) -> T) -> T {
        f(&mut self.lock().mailbox)
    }

    /// Delivers a new message with `labels` (e.g. `["INBOX", "UNREAD"]`);
    /// returns its id.
    ///
    /// # Panics
    ///
    /// When the mock mailbox rejects the change (a mistake in the test).
    pub fn deliver(&self, raw: &[u8], labels: &[&str]) -> String {
        let labels: Vec<String> = labels.iter().map(|l| (*l).to_owned()).collect();
        self.with_mailbox(|m| m.add_message(raw.to_vec(), &labels, None))
            .unwrap_or_else(|e| panic!("deliver: {e:?}"))
    }

    /// Changes a message's labels on the server.
    ///
    /// # Panics
    ///
    /// When the mock mailbox rejects the change (a mistake in the test).
    pub fn modify(&self, id: &str, add: &[&str], remove: &[&str]) {
        let add: Vec<String> = add.iter().map(|l| (*l).to_owned()).collect();
        let remove: Vec<String> = remove.iter().map(|l| (*l).to_owned()).collect();
        self.with_mailbox(|m| m.modify(id, &add, &remove).map(drop))
            .unwrap_or_else(|e| panic!("modify: {e:?}"));
    }

    /// Deletes a message permanently on the server.
    ///
    /// # Panics
    ///
    /// When the mock mailbox rejects the change (a mistake in the test).
    pub fn delete(&self, id: &str) {
        self.with_mailbox(|m| m.delete(id))
            .unwrap_or_else(|e| panic!("delete: {e:?}"));
    }

    /// Creates a user label on the server; returns its id.
    ///
    /// # Panics
    ///
    /// When the mock mailbox rejects the change (a mistake in the test).
    pub fn create_label(&self, name: &str) -> String {
        self.with_mailbox(|m| {
            m.create_label(Label {
                name: name.to_owned(),
                ..Label::default()
            })
        })
        .unwrap_or_else(|e| panic!("create_label: {e:?}"))
        .id
    }

    /// Renames a user label on the server.
    ///
    /// # Panics
    ///
    /// When the mock mailbox rejects the change (a mistake in the test).
    pub fn rename_label(&self, id: &str, name: &str) {
        self.with_mailbox(|m| m.update_label(id, &serde_json::json!({ "name": name }), false))
            .unwrap_or_else(|e| panic!("rename_label: {e:?}"));
    }

    /// Forgets the history so far (the next `history.list` from an older
    /// id answers `404`).
    pub fn expire_history(&self) {
        self.with_mailbox(Mailbox::expire_history);
    }

    /// A message as stored.
    pub fn message(&self, id: &str) -> Option<MockMessage> {
        self.with_mailbox(|m| m.message(id).cloned())
    }

    /// The labels of a message.
    pub fn labels_of(&self, id: &str) -> Vec<String> {
        self.message(id).map(|m| m.label_ids).unwrap_or_default()
    }

    /// Every label.
    pub fn labels(&self) -> Vec<Label> {
        self.with_mailbox(|m| m.labels())
    }

    /// The current history id.
    pub fn history_id(&self) -> u64 {
        self.with_mailbox(|m| m.history_id())
    }

    /// Messages received through `messages.send`, as sent.
    pub fn sent(&self) -> Vec<Vec<u8>> {
        self.with_mailbox(|m| m.sent().to_vec())
    }
}

impl Drop for MockGmailServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
