//! A mock EWS SOAP server for tests and the demo-multi seed (feature
//! `testing`; plan M3c WP92, contract ADR 0072).
//!
//! [`MockEwsServer`] binds 127.0.0.1 only (ADR 0057) and serves
//! `POST /EWS/Exchange.asmx` from an in-memory Exchange organization: the
//! own mailbox (folders, messages with MIME, calendar with recurring
//! series, occurrences and exceptions, contacts, tasks), a shared mailbox,
//! a delegate's mailbox (calendar only), a mailbox the user may not open,
//! sync states per folder and for the hierarchy, change keys,
//! streaming subscriptions with `GetStreamingEvents` (closing, breaking
//! and expiring on demand), automatic replies, inbox rules (with an
//! Outlook rule blob switch), the category list, room lists and rooms
//! (rooms accept invitations and become busy), free/busy, name
//! resolution and delegates.
//!
//! Logins: none, Basic, bearer tokens, and a scripted Negotiate handshake
//! bound to the TCP connection like NTLM ([`ScriptedNegotiate`] is the
//! matching client side). Tests change the server with mutation helpers
//! ([`MockEwsServer::deliver`], [`MockEwsServer::set_read`], …), inject
//! faults ([`InjectedFault`]: `ErrorServerBusy` with `BackOffMilliseconds`,
//! HTTP errors, response codes), replay recorded answers
//! ([`MockEwsServer::answer_with`], documents in [`fixtures`]) and read the
//! request log.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State as AxumState};
use axum::http::{HeaderMap, Response, StatusCode, header};
use axum::routing::post;
use base64::Engine as _;
use nail_api::ErrorCode;
use nail_api::engine::EngineError;
use nail_api::exchange::engine::{NegotiateAuth, NegotiateSession, NegotiateStep};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

pub mod fixtures;
mod model;
mod ops;
mod zones;

use model::State;
pub use model::{Access, MeetingMessage, MockFolder, MockItem, MockMailbox, Oof};

use crate::xml::{self, Element};

/// The own mailbox of the default organization.
pub const OWN_ADDRESS: &str = "ada@corp.example";
/// A shared mailbox the user has full access to.
pub const SHARED_ADDRESS: &str = "shared@corp.example";
/// A mailbox whose calendar, contacts and tasks the user may open (a
/// delegate).
pub const DELEGATE_ADDRESS: &str = "boss@corp.example";
/// A mailbox the user may not open.
pub const PRIVATE_ADDRESS: &str = "private@corp.example";
/// The room list of the default organization.
pub const ROOM_LIST: &str = "rooms-hq@corp.example";
/// The rooms of [`ROOM_LIST`].
pub const ROOMS: [&str; 2] = ["room1@corp.example", "room2@corp.example"];

/// The client token a [`ScriptedNegotiate`] sends first, and its answer to
/// the challenge (shared with the mock Autodiscover server).
pub use nail_testkit_net::api_mock::{AUTHENTICATE_TOKEN, NEGOTIATE_TOKEN};
/// The server's challenge.
pub const CHALLENGE_TOKEN: &[u8] = b"mock-challenge";
/// The server's last (mutual authentication) token.
pub const MUTUAL_TOKEN: &[u8] = b"mock-mutual";

/// The login the server wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockAuth {
    /// No login.
    None,
    /// HTTP Basic with this user and password.
    Basic {
        /// User.
        username: String,
        /// Password.
        password: String,
    },
    /// Bearer tokens (any of these).
    Bearer(Vec<String>),
    /// The scripted Negotiate handshake ([`ScriptedNegotiate`]), bound to
    /// the connection.
    Negotiate,
}

/// A fault the server answers instead of handling a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectedFault {
    /// Only this operation (`GetItem`, …); `None` = any.
    pub operation: Option<String>,
    /// What to answer.
    pub kind: FaultKind,
    /// How many requests it answers before it is gone.
    pub times: u32,
}

/// What an [`InjectedFault`] answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultKind {
    /// A SOAP fault `ErrorServerBusy` with `BackOffMilliseconds` (HTTP
    /// 500, as Exchange throttles).
    ServerBusy {
        /// The back-off.
        back_off_ms: u64,
    },
    /// `ErrorServerBusy` as the response message of the operation.
    ServerBusyMessage {
        /// The back-off.
        back_off_ms: u64,
    },
    /// A plain HTTP status (with `Retry-After` seconds).
    Http(u16, Option<u32>),
    /// The operation's response message fails with this code.
    ResponseCode(String),
}

impl InjectedFault {
    /// `ErrorServerBusy` once for `operation`.
    pub fn busy(operation: Option<&str>, back_off_ms: u64) -> Self {
        Self {
            operation: operation.map(str::to_owned),
            kind: FaultKind::ServerBusy { back_off_ms },
            times: 1,
        }
    }

    /// A response code once for `operation`.
    pub fn code(operation: &str, code: &str) -> Self {
        Self {
            operation: Some(operation.to_owned()),
            kind: FaultKind::ResponseCode(code.to_owned()),
            times: 1,
        }
    }

    /// An HTTP status once.
    pub fn http(operation: Option<&str>, status: u16) -> Self {
        Self {
            operation: operation.map(str::to_owned),
            kind: FaultKind::Http(status, None),
            times: 1,
        }
    }

    /// Answers this many requests.
    #[must_use]
    pub fn times(mut self, times: u32) -> Self {
        self.times = times;
        self
    }
}

/// One logged request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedRequest {
    /// The operation (`GetItem`, …).
    pub operation: String,
    /// `RequestServerVersion`.
    pub version: String,
    /// `X-AnchorMailbox`.
    pub anchor: Option<String>,
    /// `TimeZoneContext`.
    pub time_zone: Option<String>,
    /// The `Authorization` scheme (`Basic`, `Bearer`, `Negotiate`).
    pub auth_scheme: Option<String>,
    /// The client's address (one per connection).
    pub peer: SocketAddr,
    /// The operation element's attributes and body, for assertions.
    pub body: String,
}

/// A message the server sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMessage {
    /// The MIME source as sent.
    pub raw: Vec<u8>,
    /// Whether a copy was saved in Sent Items.
    pub saved: bool,
    /// The `t:From` mailbox the request named (send as / on behalf; M4c).
    pub from: Option<String>,
    /// The mailbox whose Sent Items got the copy.
    pub saved_in: Option<String>,
}

/// The client side of the scripted Negotiate handshake (a stand-in for
/// SSPI in tests): `nail-negotiate` → challenge → `nail-authenticate`,
/// then the server's mutual token is checked.
#[derive(Debug, Clone, Default)]
pub struct ScriptedNegotiate {
    /// Make the handshake fail (wrong credentials).
    pub reject: bool,
}

#[derive(Debug)]
struct ScriptedSession {
    reject: bool,
    round: u8,
}

impl NegotiateSession for ScriptedSession {
    fn step(&mut self, server_token: Option<&[u8]>) -> Result<NegotiateStep, EngineError> {
        self.round += 1;
        match (self.round, server_token) {
            (1, None) => Ok(NegotiateStep::Continue(NEGOTIATE_TOKEN.to_vec())),
            (2, Some(CHALLENGE_TOKEN)) if !self.reject => {
                Ok(NegotiateStep::Done(Some(AUTHENTICATE_TOKEN.to_vec())))
            }
            (2, Some(CHALLENGE_TOKEN)) => Ok(NegotiateStep::Done(Some(b"wrong-password".to_vec()))),
            (3, Some(MUTUAL_TOKEN)) => Ok(NegotiateStep::Done(None)),
            _ => Err(EngineError::permanent(
                ErrorCode::AuthFailed,
                "unexpected Negotiate token",
            )),
        }
    }
}

impl NegotiateAuth for ScriptedNegotiate {
    fn start(&self, spn: &str) -> Result<Box<dyn NegotiateSession>, EngineError> {
        if !spn.starts_with("HTTP/") {
            return Err(EngineError::permanent(ErrorCode::Config, "bad SPN"));
        }
        Ok(Box::new(ScriptedSession {
            reject: self.reject,
            round: 0,
        }))
    }
}

#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) state: Mutex<State>,
    /// Wakes open streams after a change.
    pub(crate) wake: Notify,
    /// Bumped by [`MockEwsServer::close_streams`].
    pub(crate) close_generation: std::sync::atomic::AtomicU64,
}

impl Shared {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        nail_testkit_net::api_mock::lock(&self.state)
    }
}

/// A running mock EWS server; stops when dropped.
#[derive(Debug)]
pub struct MockEwsServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl MockEwsServer {
    /// Starts a server with the default organization
    /// ([`fixtures::organization`]) and no login.
    pub async fn start() -> std::io::Result<Self> {
        Self::start_with(MockAuth::None).await
    }

    /// Starts a server with the default organization and a login.
    pub async fn start_with(auth: MockAuth) -> std::io::Result<Self> {
        let mut state = fixtures::organization();
        state.auth = auth;
        let shared = Arc::new(Shared {
            state: Mutex::new(state),
            wake: Notify::new(),
            close_generation: std::sync::atomic::AtomicU64::new(0),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let app = Router::new()
            .route(nail_api::exchange::EWS_PATH, post(handle))
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
            .with_state(shared.clone());
        let task = tokio::spawn(async move {
            drop(
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await,
            );
        });
        Ok(Self { addr, shared, task })
    }

    /// The server's address (127.0.0.1 and its port).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The EWS endpoint (`http://127.0.0.1:<port>/EWS/Exchange.asmx`).
    pub fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}{}",
            self.addr.port(),
            nail_api::exchange::EWS_PATH
        )
    }

    /// A client of this server as the own user with `auth`, retrying
    /// without waits.
    pub fn client(&self, auth: crate::EwsAuth) -> Result<crate::EwsClient, crate::EwsError> {
        use crate::client::{CALL_TIMEOUT, lane_http};
        Ok(crate::EwsClient::with_http(
            lane_http(CALL_TIMEOUT)?,
            lane_http(CALL_TIMEOUT)?,
            &self.url(),
            auth,
        )?
        .with_own_address(OWN_ADDRESS)
        .with_retry(crate::RetryPolicy::fast()))
    }

    /// An engine of the account `account` on this server, without
    /// sign-in (see [`Self::client`]).
    pub fn engine(&self, account: &str) -> Result<crate::EwsEngine, crate::EwsError> {
        Ok(crate::EwsEngine::new(
            self.client(crate::EwsAuth::None)?,
            account.into(),
        ))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.shared.lock()
    }

    fn changed(&self) {
        self.shared.wake.notify_waiters();
    }

    // -- logins ---------------------------------------------------------

    /// Replaces the login the server wants (signed-in connections are
    /// forgotten).
    pub fn set_auth(&self, auth: MockAuth) {
        let mut state = self.state();
        state.auth = auth;
        state.authenticated.clear();
        state.challenged.clear();
    }

    /// Forgets Negotiate sign-ins of open connections (as if the server
    /// recycled them): the next request on them answers `401`.
    pub fn forget_connections(&self) {
        let mut state = self.state();
        state.authenticated.clear();
        state.challenged.clear();
    }

    // -- the log, faults, recorded answers -------------------------------

    /// Every request so far.
    pub fn requests(&self) -> Vec<LoggedRequest> {
        self.state().log.clone()
    }

    /// The operations so far, in order.
    pub fn operations(&self) -> Vec<String> {
        self.state()
            .log
            .iter()
            .map(|r| r.operation.clone())
            .collect()
    }

    /// Clears the request log.
    pub fn clear_requests(&self) {
        self.state().log.clear();
    }

    /// Injects a fault.
    pub fn inject(&self, fault: InjectedFault) {
        self.state().faults.push(fault);
    }

    /// Answers the next `times` requests of `operation` with a recorded
    /// SOAP document (see [`fixtures`]).
    pub fn answer_with(&self, operation: &str, document: &str, times: u32) {
        self.state().recorded.push(model::Recorded {
            operation: operation.to_owned(),
            document: document.to_owned(),
            times,
        });
    }

    /// Refuses `RequestServerVersion` values (an older server).
    pub fn refuse_versions(&self, versions: &[&str]) {
        self.state().refused_versions = versions.iter().map(|v| (*v).to_owned()).collect();
    }

    /// The `ServerVersionInfo` the server sends.
    pub fn set_server_version(&self, major: u32, minor: u32, build: u32, revision: u32) {
        self.state().version = (major, minor, build, revision);
    }

    // -- mailboxes --------------------------------------------------------

    /// A copy of a mailbox (`None` = own).
    pub fn mailbox(&self, address: Option<&str>) -> Option<MockMailbox> {
        let state = self.state();
        state
            .mailbox_index(address)
            .map(|i| state.mailboxes[i].clone())
    }

    /// The id of a distinguished folder (`inbox`, `calendar`, …) of a
    /// mailbox.
    pub fn folder_id(&self, mailbox: Option<&str>, distinguished: &str) -> Option<String> {
        let state = self.state();
        let m = state.mailbox_index(mailbox)?;
        state
            .distinguished(m, distinguished)
            .map(|f| state.mailboxes[m].folders[f].id.clone())
    }

    /// The id of a folder by display name.
    pub fn folder_named(&self, mailbox: Option<&str>, name: &str) -> Option<String> {
        let state = self.state();
        let m = state.mailbox_index(mailbox)?;
        state.mailboxes[m]
            .folders
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.id.clone())
    }

    /// The items of a folder.
    pub fn items_in(&self, folder: &str) -> Vec<MockItem> {
        let state = self.state();
        state
            .mailboxes
            .iter()
            .flat_map(|m| m.items.iter())
            .filter(|i| i.folder == folder)
            .cloned()
            .collect()
    }

    /// An item by id.
    pub fn item(&self, id: &str) -> Option<MockItem> {
        let state = self.state();
        state
            .item_location(id)
            .map(|(m, i)| state.mailboxes[m].items[i].clone())
    }

    /// Delivers a message into a folder (a server-side change); answers
    /// its id.
    ///
    /// # Panics
    ///
    /// When the named mailbox, folder or item does not exist, or the XML is malformed.
    pub fn deliver(&self, folder: &str, raw: &[u8], read: bool) -> String {
        let id = {
            let mut state = self.state();
            let (m, _) = state
                .folder_location(folder)
                .unwrap_or_else(|| panic!("no folder {folder}"));
            let props = fixtures::message_props(raw, read, &[]);
            state.add_item(m, folder, "Message", props, Some(raw.to_vec()))
        };
        self.changed();
        id
    }

    /// Adds a calendar item from its properties (`t:CalendarItem`
    /// children, UTC times); answers its id.
    ///
    /// # Panics
    ///
    /// When the named mailbox, folder or item does not exist, or the XML is malformed.
    pub fn add_calendar_item(&self, mailbox: Option<&str>, props_xml: &str) -> String {
        let id = {
            let mut state = self.state();
            let m = state
                .mailbox_index(mailbox)
                .unwrap_or_else(|| panic!("no mailbox {mailbox:?}"));
            let folder = state
                .distinguished(m, "calendar")
                .unwrap_or_else(|| panic!("no calendar folder"));
            let folder_id = state.mailboxes[m].folders[folder].id.clone();
            let props = xml::parse(&format!("<CalendarItem>{props_xml}</CalendarItem>"))
                .unwrap_or_else(|e| panic!("calendar item XML: {e:?}"));
            state.add_item(m, &folder_id, "CalendarItem", props, None)
        };
        self.changed();
        id
    }

    fn change_item(&self, id: &str, change: impl FnOnce(&mut MockItem), read_only: bool) {
        {
            let mut state = self.state();
            let (m, i) = state
                .item_location(id)
                .unwrap_or_else(|| panic!("no item {id}"));
            let key = state.new_key();
            let item = &mut state.mailboxes[m].items[i];
            change(item);
            if !read_only {
                item.change_key = key;
            }
            let folder = item.folder.clone();
            state.item_event("ModifiedEvent", id, &folder, None);
        }
        self.changed();
    }

    /// Marks an item read or unread on the server (a `ReadFlagChange`).
    pub fn set_read(&self, id: &str, read: bool) {
        self.change_item(
            id,
            |item| item.set("IsRead", if read { "true" } else { "false" }),
            true,
        );
    }

    /// Sets an item's categories on the server.
    pub fn set_categories(&self, id: &str, categories: &[&str]) {
        self.change_item(
            id,
            |item| {
                item.props.children.retain(|c| c.name != "Categories");
                if !categories.is_empty() {
                    let mut list = Element::new("Categories");
                    for c in categories {
                        let mut s = Element::new("String");
                        (*c).clone_into(&mut s.text);
                        list.children.push(s);
                    }
                    item.props.children.push(list);
                }
            },
            false,
        );
    }

    /// Changes a property of an item on the server (bumps its change
    /// key).
    pub fn set_property(&self, id: &str, name: &str, value: &str) {
        self.change_item(id, |item| item.set(name, value), false);
    }

    /// Moves an item on the server; answers its new id.
    ///
    /// # Panics
    ///
    /// When the named mailbox, folder or item does not exist, or the XML is malformed.
    pub fn move_item(&self, id: &str, folder: &str) -> String {
        let new = {
            let mut state = self.state();
            ops::move_item(&mut state, id, folder, false)
                .unwrap_or_else(|e| panic!("move {id} to {folder}: {e:?}"))
        };
        self.changed();
        new
    }

    /// Deletes an item on the server.
    pub fn delete_item(&self, id: &str) {
        {
            let mut state = self.state();
            if let Some((m, i)) = state.item_location(id) {
                let item = state.mailboxes[m].items.remove(i);
                state.item_event("DeletedEvent", id, &item.folder, None);
            }
        }
        self.changed();
    }

    /// Adds a folder below `parent`; answers its id.
    ///
    /// # Panics
    ///
    /// When the named mailbox, folder or item does not exist, or the XML is malformed.
    pub fn add_folder(&self, parent: &str, name: &str, class: &str) -> String {
        let mut state = self.state();
        let (m, _) = state
            .folder_location(parent)
            .unwrap_or_else(|| panic!("no folder {parent}"));
        let id = state.new_id("AQMkFolder");
        let key = state.new_key();
        state.mailboxes[m].folders.push(MockFolder {
            id: id.clone(),
            change_key: key,
            parent: Some(parent.to_owned()),
            name: name.to_owned(),
            class: class.to_owned(),
            distinguished: None,
        });
        id
    }

    /// Renames a folder.
    ///
    /// # Panics
    ///
    /// When the named mailbox, folder or item does not exist, or the XML is malformed.
    pub fn rename_folder(&self, id: &str, name: &str) {
        let mut state = self.state();
        let (m, f) = state
            .folder_location(id)
            .unwrap_or_else(|| panic!("no folder {id}"));
        let key = state.new_key();
        let folder = &mut state.mailboxes[m].folders[f];
        name.clone_into(&mut folder.name);
        folder.change_key = key;
    }

    /// Removes a folder and its items.
    pub fn remove_folder(&self, id: &str) {
        let mut state = self.state();
        if let Some((m, f)) = state.folder_location(id) {
            state.mailboxes[m].folders.remove(f);
            state.mailboxes[m].items.retain(|i| i.folder != id);
        }
    }

    /// Changes who may open a mailbox.
    pub fn set_access(&self, address: &str, access: Access) {
        let mut state = self.state();
        if let Some(m) = state.mailbox_index(Some(address)) {
            state.mailboxes[m].access = access;
        }
    }

    /// Forgets every sync state (`ErrorInvalidSyncStateData` next).
    pub fn expire_sync_states(&self) {
        let mut state = self.state();
        state.sync_states.clear();
        state.hierarchy_states.clear();
    }

    // -- streaming --------------------------------------------------------

    /// The open subscriptions (id, watched folder ids).
    pub fn subscriptions(&self) -> Vec<(String, Vec<String>)> {
        self.state()
            .subscriptions
            .iter()
            .map(|(id, s)| (id.clone(), s.folders.iter().cloned().collect()))
            .collect()
    }

    /// Forgets every subscription (open streams answer
    /// `ErrorSubscriptionNotFound`).
    pub fn expire_subscriptions(&self) {
        self.state().subscriptions.clear();
        self.changed();
    }

    /// Closes open streams with `ConnectionStatus` `Closed`.
    pub fn close_streams(&self) {
        self.shared
            .close_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.changed();
    }

    /// Breaks the next `count` streams (the connection is cut).
    pub fn break_streams(&self, count: u32) {
        self.state().break_streams += count;
        self.changed();
    }

    /// Streams close after this long (default 30 s).
    pub fn set_stream_close_after(&self, after: Duration) {
        self.state().stream_close_after = after;
    }

    // -- sent mail, settings, directory -----------------------------------

    /// Messages sent so far.
    pub fn sent(&self) -> Vec<SentMessage> {
        self.state().sent.clone()
    }

    /// Refuses (`false`) or allows `MoveItem` / `CopyItem` between
    /// mailboxes (`ErrorCrossMailboxMoveCopy`; M4c).
    pub fn allow_cross_mailbox_moves(&self, allow: bool) {
        self.state().cross_mailbox_moves = allow;
    }

    /// Meeting messages sent so far.
    pub fn meeting_messages(&self) -> Vec<MeetingMessage> {
        self.state().meeting_messages.clone()
    }

    /// The automatic replies as stored.
    pub fn oof(&self) -> Oof {
        self.state().oof.clone()
    }

    /// Replaces the automatic replies.
    pub fn set_oof(&self, oof: Oof) {
        self.state().oof = oof;
    }

    /// The inbox rules as stored.
    pub fn rules(&self) -> Vec<Element> {
        self.state().rules.clone()
    }

    /// Adds an inbox rule (a `t:Rule` element without namespace
    /// prefixes); answers its id.
    ///
    /// # Panics
    ///
    /// When the named mailbox, folder or item does not exist, or the XML is malformed.
    pub fn add_rule(&self, rule_xml: &str) -> String {
        let mut state = self.state();
        let mut rule = xml::parse(rule_xml).unwrap_or_else(|e| panic!("rule XML: {e:?}"));
        let id = state.new_id("R");
        rule.children.retain(|c| c.name != "RuleId");
        let mut rule_id = Element::new("RuleId");
        rule_id.text.clone_from(&id);
        rule.children.insert(0, rule_id);
        state.rules.push(rule);
        id
    }

    /// Whether Outlook keeps client-only rules.
    pub fn set_outlook_rule_blob(&self, exists: bool) {
        self.state().outlook_rule_blob = exists;
    }

    /// The category list document, if any.
    pub fn category_list(&self) -> Option<String> {
        self.state().categories.clone()
    }

    /// Replaces the category list document.
    pub fn set_category_list(&self, document: Option<&str>) {
        self.state().categories = document.map(str::to_owned);
    }

    /// Room lists answer `ErrorAccessDenied`.
    pub fn deny_rooms(&self, denied: bool) {
        self.state().rooms_denied = denied;
    }

    /// Busy periods of a person or room without a mailbox here (UTC,
    /// `Busy` / `Tentative` / `OOF`).
    pub fn set_free_busy(&self, address: &str, periods: &[(&str, &str, &str)]) {
        self.state().free_busy.insert(
            address.to_ascii_lowercase(),
            periods
                .iter()
                .map(|(s, e, k)| ((*s).to_owned(), (*e).to_owned(), (*k).to_owned()))
                .collect(),
        );
    }

    /// The busy periods recorded for a room or person.
    pub fn free_busy_of(&self, address: &str) -> Vec<(String, String, String)> {
        self.state()
            .free_busy
            .get(&address.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }
}

impl Drop for MockEwsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn soap_response(status: StatusCode, document: String) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/xml; charset=utf-8")
        .body(Body::from(document))
        .unwrap_or_default()
}

fn unauthorized(challenge: &[&str]) -> Response<Body> {
    let mut builder = Response::builder().status(StatusCode::UNAUTHORIZED);
    for value in challenge {
        builder = builder.header(header::WWW_AUTHENTICATE, *value);
    }
    builder.body(Body::empty()).unwrap_or_default()
}

/// Checks the login; `Err` = the answer to send, `Ok(Some(token))` = a
/// mutual-authentication token to add.
fn check_auth(
    state: &mut State,
    peer: SocketAddr,
    headers: &HeaderMap,
) -> Result<Option<String>, Box<Response<Body>>> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    match state.auth.clone() {
        MockAuth::None => Ok(None),
        MockAuth::Basic { username, password } => {
            if authorization.as_deref()
                == Some(crate::auth::basic_header(&username, &password).as_str())
            {
                Ok(None)
            } else {
                Err(Box::new(unauthorized(&["Basic realm=\"mock\""])))
            }
        }
        MockAuth::Bearer(tokens) => match authorization
            .as_deref()
            .and_then(|a| a.strip_prefix("Bearer "))
        {
            Some(token) if tokens.iter().any(|t| t == token) => Ok(None),
            _ => Err(Box::new(unauthorized(&["Bearer"]))),
        },
        MockAuth::Negotiate => {
            let token = authorization
                .as_deref()
                .and_then(|a| a.strip_prefix("Negotiate "))
                .and_then(|t| {
                    base64::engine::general_purpose::STANDARD
                        .decode(t.trim())
                        .ok()
                });
            match token {
                None if state.authenticated.contains(&peer) => Ok(None),
                None => Err(Box::new(unauthorized(&["Negotiate", "NTLM"]))),
                Some(t) if t == NEGOTIATE_TOKEN => {
                    state.challenged.insert(peer);
                    let challenge = format!(
                        "Negotiate {}",
                        base64::engine::general_purpose::STANDARD.encode(CHALLENGE_TOKEN)
                    );
                    Err(Box::new(unauthorized(&[challenge.as_str()])))
                }
                Some(t) if t == AUTHENTICATE_TOKEN && state.challenged.remove(&peer) => {
                    state.authenticated.insert(peer);
                    Ok(Some(format!(
                        "Negotiate {}",
                        base64::engine::general_purpose::STANDARD.encode(MUTUAL_TOKEN)
                    )))
                }
                Some(_) => {
                    state.challenged.remove(&peer);
                    Err(Box::new(unauthorized(&["Negotiate"])))
                }
            }
        }
    }
}

async fn handle(
    AxumState(shared): AxumState<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: String,
) -> Response<Body> {
    let mutual;
    let outcome = {
        let mut state = shared.lock();
        match check_auth(&mut state, peer, &headers) {
            Err(response) => return *response,
            Ok(token) => mutual = token,
        }
        ops::handle(&mut state, peer, &headers, &body)
    };
    let mut response = match outcome {
        ops::Outcome::Soap(status, document) => soap_response(status, document),
        ops::Outcome::Http(status, retry_after) => {
            let mut builder = Response::builder().status(status);
            if let Some(secs) = retry_after {
                builder = builder.header(header::RETRY_AFTER, secs.to_string());
            }
            builder.body(Body::empty()).unwrap_or_default()
        }
        ops::Outcome::Stream(subscriptions) => ops::stream(shared.clone(), subscriptions),
    };
    if let Some(token) = mutual
        && let Ok(value) = token.parse()
    {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    shared.wake.notify_waiters();
    response
}
