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
use std::sync::MutexGuard;

use nail_testkit_net::api_mock::{ApiRequest, MockApi, MockReply, MockService};

pub mod fixtures;
mod mailbox;
mod server;

pub use mailbox::{Fixture, FixtureMessage, Mailbox, MockError, MockMessage};
use server::{Quota, State, dispatch};

use crate::wire::Label;

nail_testkit_net::injected_error!(server::generic_error);

impl InjectedError {
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

/// A running mock Gmail server; stops when dropped.
#[derive(Debug)]
pub struct MockGmailServer {
    api: MockApi<State>,
}

/// Largest request body (an 80 MB upload).
const BODY_LIMIT: usize = 80 * 1024 * 1024;

impl MockService for State {
    fn answer(&mut self, request: &ApiRequest) -> MockReply {
        dispatch(
            self,
            &request.method,
            &request.path,
            &request.query,
            &request.headers,
            &request.body,
            false,
        )
    }
}

nail_testkit_net::api_mock_controls!(MockGmailServer);

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
        let state = State {
            mailbox,
            token: None,
            errors: Vec::new(),
            requests: Vec::new(),
            quota: None,
        };
        Ok(Self {
            api: MockApi::start(state, BODY_LIMIT).await?,
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.api.lock()
    }

    /// The server's address (127.0.0.1 and its port).
    pub fn addr(&self) -> SocketAddr {
        self.api.addr()
    }

    /// The API base URL (`http://127.0.0.1:<port>/gmail/v1/users/me`).
    pub fn api_base(&self) -> String {
        crate::api_base("127.0.0.1", self.api.addr().port(), false)
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
