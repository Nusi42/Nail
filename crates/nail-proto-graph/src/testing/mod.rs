//! A mock Microsoft Graph server for tests and the demo-multi seed
//! (feature `testing`; plan M3c WP91, contract ADR 0072).
//!
//! [`MockGraphServer`] binds 127.0.0.1 only (ADR 0057) and serves an
//! in-memory tenant ([`Tenant`]) under `/v1.0`: `/me` and `/users/{upn}`
//! (shared mailboxes, denied and on-premises ones), mailbox settings and
//! automatic replies, mail folders with `delta`, messages with per-folder
//! `delta` (paging, delta tokens, expiry → `410`), `$value`, `PATCH`,
//! `move` / `copy` / `permanentDelete`, MIME creates, attachments,
//! `sendMail` (MIME), Focused Inbox overrides, master categories, inbox
//! rules, calendars with `calendarView/delta` (series show as
//! occurrences), events, instances (daily / weekly / monthly patterns),
//! exceptions, cancellations, meeting answers, contacts and contact
//! folders with `delta`, To Do lists, tasks with `delta`, checklist items
//! and linked resources, places (rooms, room lists; `403` without
//! consent), `getSchedule`, `findMeetingTimes` and `$batch` (20 parts).
//!
//! Tests drive it with mutation helpers (server-side changes), injected
//! errors ([`InjectedError`], answered with recorded Graph error bodies),
//! a request budget (throttling with `Retry-After`), token checks and a
//! request log. Tenants load from recorded-style fixtures ([`fixtures`]).

use std::net::SocketAddr;
use std::sync::MutexGuard;

use nail_testkit_net::api_mock::{ApiRequest, MockApi, MockReply, MockService};
use serde_json::Value;

pub mod fixtures;
mod server;
mod tenant;
mod upload;

use server::{Budget, Req, State, dispatch};
pub use tenant::{
    CompletedUpload, IdGen, JsonDraft, Mailbox, MeetingAction, MockError, MockFolder, MockMessage,
    MockResult, MockTask, MockUpload, SentMail, Tenant, WELL_KNOWN,
};

nail_testkit_net::injected_error!(server::recorded_status);

impl InjectedError {
    /// `429 TooManyRequests` (recorded), optionally with `Retry-After`.
    pub fn throttled(retry_after_secs: Option<u32>) -> Self {
        Self {
            retry_after: retry_after_secs.map(|s| s.to_string()),
            ..Self::status(429)
        }
    }

    /// `503 ServiceUnavailable` (recorded), optionally with `Retry-After`.
    pub fn unavailable(retry_after_secs: Option<u32>) -> Self {
        Self {
            retry_after: retry_after_secs.map(|s| s.to_string()),
            ..Self::status(503)
        }
    }

    /// `500` (recorded).
    pub fn server_error() -> Self {
        Self::status(500)
    }

    /// `410 SyncStateNotFound` (recorded).
    pub fn sync_state_not_found() -> Self {
        Self::status(410)
    }

    /// `404 ErrorItemNotFound` (recorded).
    pub fn not_found() -> Self {
        Self::status(404)
    }

    /// `403 ErrorAccessDenied` (recorded).
    pub fn access_denied() -> Self {
        Self::status(403)
    }

    /// `401 InvalidAuthenticationToken` (recorded).
    pub fn invalid_token() -> Self {
        Self::status(401)
    }
}

/// A request the server saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    /// Method.
    pub method: String,
    /// Path (percent-encoded as sent, with `/v1.0`).
    pub path: String,
    /// Query string.
    pub query: String,
    /// A part of a `$batch`.
    pub in_batch: bool,
    /// The bearer token.
    pub token: Option<String>,
    /// The `Prefer` header.
    pub prefer: Option<String>,
    /// The `If-Match` header.
    pub if_match: Option<String>,
    /// The `Content-Type` header.
    pub content_type: Option<String>,
    /// The body as text.
    pub body: String,
}

/// A running mock Graph server; stops when dropped.
#[derive(Debug)]
pub struct MockGraphServer {
    api: MockApi<State>,
}

/// Largest request body (a 64 MB upload range or MIME message).
const BODY_LIMIT: usize = 64 * 1024 * 1024;

impl MockService for State {
    fn answer(&mut self, request: &ApiRequest) -> MockReply {
        let req = Req::new(
            &request.method,
            &request.path,
            &request.query,
            request.headers.clone(),
            request.body.clone(),
        );
        dispatch(self, &req, false)
    }
}

nail_testkit_net::api_mock_controls!(MockGraphServer);

impl MockGraphServer {
    /// Starts a server on a free loopback port with an empty mailbox for
    /// `email` (well-known folders, a default calendar, the default
    /// contacts folder, To Do's default and flagged-email lists). Any
    /// non-empty bearer token is accepted until [`Self::set_token`].
    pub async fn start(email: &str) -> std::io::Result<Self> {
        Self::start_with(Tenant::new(email)).await
    }

    /// Starts a server with the tenant of a fixture (JSON text).
    pub async fn start_fixture(json: &str) -> std::io::Result<Self> {
        Self::start_with(fixture_tenant(json)?).await
    }

    /// Starts a server with the demo tenant ([`fixtures::DEMO_TENANT`]),
    /// its automatic replies on around today
    /// ([`Tenant::schedule_replies_around`]).
    pub async fn start_demo() -> std::io::Result<Self> {
        let mut tenant = fixture_tenant(fixtures::DEMO_TENANT)?;
        tenant.schedule_replies_around(chrono::Utc::now());
        Self::start_with(tenant).await
    }

    /// Starts a server with `tenant`.
    pub async fn start_with(tenant: Tenant) -> std::io::Result<Self> {
        let state = State {
            tenant,
            token: None,
            errors: Vec::new(),
            requests: Vec::new(),
            budget: None,
            // Set below, once the port is known (no request can come before).
            origin: String::new(),
        };
        let api = MockApi::start(state, BODY_LIMIT).await?;
        api.lock().origin = format!("http://127.0.0.1:{}", api.addr().port());
        Ok(Self { api })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.api.lock()
    }

    /// The server's address (127.0.0.1 and its port).
    pub fn addr(&self) -> SocketAddr {
        self.api.addr()
    }

    /// The API base URL (`http://127.0.0.1:<port>/v1.0`).
    pub fn api_base(&self) -> String {
        crate::api_base("127.0.0.1", self.api.addr().port(), false)
    }

    /// The user's address.
    pub fn me(&self) -> String {
        self.lock().tenant.me.clone()
    }

    /// Allows `requests` more requests (each batch part counts); beyond
    /// that the server answers `429` with `Retry-After`. `None` lifts it.
    pub fn set_request_budget(&self, requests: Option<u64>, retry_after_secs: u32) {
        self.lock().budget = requests.map(|limit| Budget {
            limit,
            used: 0,
            retry_after: retry_after_secs,
        });
    }

    /// Runs `f` on the tenant (any server-side change or inspection).
    pub fn with_tenant<T>(&self, f: impl FnOnce(&mut Tenant) -> T) -> T {
        f(&mut self.lock().tenant)
    }

    /// Runs `f` on a mailbox (`None` = the user's) with the id generator.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn with_mailbox<T>(
        &self,
        mailbox: Option<&str>,
        f: impl FnOnce(&mut Mailbox, &mut IdGen) -> T,
    ) -> T {
        self.with_tenant(|t| {
            let address = mailbox.map_or_else(|| t.me.clone(), str::to_ascii_lowercase);
            let ids = &mut t.ids;
            let mb = t
                .mailboxes
                .get_mut(&address)
                .unwrap_or_else(|| panic!("no mailbox {address}"));
            f(mb, ids)
        })
    }

    /// The id of a folder (well-known name or id) of a mailbox.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn folder_id(&self, mailbox: Option<&str>, folder: &str) -> String {
        self.with_mailbox(mailbox, |mb, _| mb.folder_id(folder))
            .unwrap_or_else(|| panic!("no folder {folder}"))
    }

    /// Delivers a new message into a folder; returns its id.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn deliver(&self, mailbox: Option<&str>, folder: &str, raw: &[u8]) -> String {
        self.with_mailbox(mailbox, |mb, ids| {
            mb.add_message(ids, folder, raw.to_vec(), false, false)
        })
        .unwrap_or_else(|e| panic!("deliver: {e:?}"))
    }

    /// Changes a message on the server.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn change_message(
        &self,
        mailbox: Option<&str>,
        id: &str,
        f: impl FnOnce(&mut MockMessage),
    ) {
        self.with_mailbox(mailbox, |mb, _| mb.change_message(id, f))
            .unwrap_or_else(|e| panic!("change_message: {e:?}"));
    }

    /// Moves a message on the server; returns the new id.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn move_message(&self, mailbox: Option<&str>, id: &str, folder: &str) -> String {
        self.with_mailbox(mailbox, |mb, ids| mb.move_message(ids, id, folder, false))
            .unwrap_or_else(|e| panic!("move_message: {e:?}"))
    }

    /// Deletes a message on the server.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn delete_message(&self, mailbox: Option<&str>, id: &str) {
        self.with_mailbox(mailbox, |mb, _| mb.remove_message(id))
            .unwrap_or_else(|e| panic!("delete_message: {e:?}"));
    }

    /// Adds a folder on the server; returns its id.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn add_folder(&self, mailbox: Option<&str>, parent: Option<&str>, name: &str) -> String {
        self.with_mailbox(mailbox, |mb, ids| mb.add_folder(ids, parent, name))
            .unwrap_or_else(|e| panic!("add_folder: {e:?}"))
    }

    /// Forgets the change log of a mailbox: older delta links answer
    /// `410`.
    pub fn expire_deltas(&self, mailbox: Option<&str>) {
        self.with_mailbox(mailbox, |mb, _| mb.expire_deltas());
    }

    /// A message as stored.
    pub fn message(&self, mailbox: Option<&str>, id: &str) -> Option<MockMessage> {
        self.with_mailbox(mailbox, |mb, _| mb.messages.get(id).cloned())
    }

    /// The messages of a folder.
    pub fn messages_in(&self, mailbox: Option<&str>, folder: &str) -> Vec<MockMessage> {
        let folder = self.folder_id(mailbox, folder);
        self.with_mailbox(mailbox, |mb, _| {
            mb.messages
                .values()
                .filter(|m| m.folder == folder)
                .cloned()
                .collect()
        })
    }

    /// Messages received through `sendMail`.
    pub fn sent(&self) -> Vec<SentMail> {
        self.with_tenant(|t| t.sent.clone())
    }

    /// Attachment uploads that finished (M4c upload sessions).
    pub fn completed_uploads(&self) -> Vec<CompletedUpload> {
        self.with_tenant(|t| t.completed_uploads.clone())
    }

    /// Upload sessions still open.
    pub fn open_uploads(&self) -> usize {
        self.with_tenant(|t| t.uploads.len())
    }

    /// Meeting answers and cancellations received.
    pub fn meeting_actions(&self) -> Vec<MeetingAction> {
        self.with_tenant(|t| t.meeting_actions.clone())
    }

    /// Adds an event to a calendar of the user's mailbox (`None` = the
    /// default calendar); returns its id.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn add_event(&self, calendar: Option<&str>, event: Value) -> String {
        self.with_mailbox(None, |mb, ids| mb.add_event(ids, calendar, event))
            .unwrap_or_else(|e| panic!("add_event: {e:?}"))
    }

    /// Changes an event on the server (a `PATCH`).
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn update_event(&self, id: &str, patch: &Value) {
        self.with_mailbox(None, |mb, _| {
            let event = mb
                .events
                .get_mut(id)
                .unwrap_or_else(|| panic!("no event {id}"));
            tenant::merge(event, patch);
            mb.bump_event(id);
            mb.touch_event(id);
        });
    }

    /// Deletes an event on the server.
    pub fn delete_event(&self, id: &str) {
        self.with_mailbox(None, |mb, _| {
            mb.events.remove(id);
            mb.touch_event(id);
            mb.event_calendar.remove(id);
        });
    }

    /// An event as stored.
    pub fn event(&self, id: &str) -> Option<Value> {
        self.with_mailbox(None, |mb, _| mb.events.get(id).cloned())
    }

    /// The events of the user's mailbox (id → JSON).
    pub fn events(&self) -> Vec<(String, Value)> {
        self.with_mailbox(None, |mb, _| {
            mb.events
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
    }

    /// Adds a contact (`None` = the default folder); returns its id.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn add_contact(&self, folder: Option<&str>, contact: Value) -> String {
        self.with_mailbox(None, |mb, ids| mb.add_contact(ids, folder, contact))
            .unwrap_or_else(|e| panic!("add_contact: {e:?}"))
    }

    /// A contact as stored.
    pub fn contact(&self, id: &str) -> Option<Value> {
        self.with_mailbox(None, |mb, _| mb.contacts.get(id).cloned())
    }

    /// Adds a To Do task (`None` = the default list); returns its id.
    ///
    /// # Panics
    ///
    /// When the mailbox or item does not exist, or the mock rejects the change.
    pub fn add_task(&self, list: Option<&str>, task: Value) -> String {
        self.with_mailbox(None, |mb, ids| mb.add_task(ids, list, task, Vec::new()))
            .unwrap_or_else(|e| panic!("add_task: {e:?}"))
    }

    /// A task as stored.
    pub fn task(&self, id: &str) -> Option<MockTask> {
        self.with_mailbox(None, |mb, _| mb.tasks.get(id).cloned())
    }

    /// Sets the busy items `getSchedule` answers for `address`.
    pub fn set_schedule(&self, address: &str, items: Vec<Value>) {
        self.with_tenant(|t| {
            t.schedules.insert(address.to_ascii_lowercase(), items);
        });
    }

    /// Makes places answer `403` (no `Place.Read.All` consent), or not.
    pub fn forbid_places(&self, forbidden: bool) {
        self.with_tenant(|t| t.places_forbidden = forbidden);
    }
}

/// [`Tenant::from_fixture`] with an I/O error, as the servers start.
fn fixture_tenant(json: &str) -> std::io::Result<Tenant> {
    Tenant::from_fixture(json).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
