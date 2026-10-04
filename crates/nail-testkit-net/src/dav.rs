//! A mock CalDAV / CardDAV server (M3a WP72; contract ADR 0058) on
//! [`axum`], bound to 127.0.0.1 only (ADR 0057).
//!
//! It serves one user (Basic login and / or a bearer token):
//!
//! * `/.well-known/caldav` and `/.well-known/carddav` redirect (`301`) to
//!   the context path `/dav/` (switch off with
//!   [`MockDavConfig::well_known`]);
//! * the principal `/dav/principals/<user>/` with `calendar-home-set`
//!   `/dav/calendars/<user>/` and `addressbook-home-set`
//!   `/dav/addressbooks/<user>/`;
//! * `PROPFIND` (depth 0 / 1; `current-user-principal`, `resourcetype`,
//!   `displayname`, `getctag`, `sync-token`, `supported-report-set`,
//!   `supported-calendar-component-set`, `calendar-color`,
//!   `current-user-privilege-set`, `getetag`, `getcontenttype`; unknown
//!   properties answer `404` in their own propstat);
//! * `REPORT`: `calendar-query` (component and `time-range` filter; a
//!   recurring object always matches), `calendar-multiget`,
//!   `addressbook-query` (`prop-filter` / `text-match` contains),
//!   `addressbook-multiget`, `sync-collection` (RFC 6578, with `limit` →
//!   truncated answers with a `507` for the collection, and
//!   `valid-sync-token` errors for forgotten tokens);
//! * `GET`, conditional `PUT` / `DELETE` (`If-Match`, `If-None-Match: *`,
//!   `412` on mismatch), `MKCALENDAR`, extended `MKCOL` for address
//!   books, `PROPPATCH` (display name, colour), `OPTIONS`.
//!
//! Tests change the server's data directly ([`MockDavServer::put_object`],
//! [`MockDavServer::delete_object`], [`MockDavServer::add_calendar`], …)
//! to simulate other clients, read it back ([`MockDavServer::object`]),
//! inject failures ([`MockDavServer::fail_next`]), make old sync tokens
//! invalid ([`MockDavServer::forget_sync_history`]) and inspect every
//! request ([`MockDavServer::requests`]).
//!
//! Default collections (with [`MockDavConfig::seed`]): calendars
//! `personal` (events and tasks), `tasks` (tasks) and the read-only
//! `holidays`; the address book `contacts`.
//!
//! **Scheduling (RFC 6638; M3b WP83)**, on by default
//! ([`MockDavConfig::scheduling`]):
//!
//! * more principals on the same server ([`MockDavConfig::users`],
//!   [`MockDavServer::add_user`]), each with its own login, address
//!   (`calendar-user-address-set`), `calendar-user-type` (rooms and
//!   resources too), calendar home with a `personal` calendar, schedule
//!   inbox and outbox; `current-user-principal` follows the login;
//! * the principal's `schedule-inbox-URL` / `schedule-outbox-URL`, the
//!   inbox's `schedule-default-calendar-URL`;
//! * `POST` of a `VFREEBUSY` request to the own outbox: a
//!   `schedule-response` with one `VFREEBUSY` reply per known recipient,
//!   computed from the recipient's calendars (opaque, not cancelled,
//!   not declined events; tentative or unanswered ones as
//!   `BUSY-TENTATIVE`; recurrences are not expanded), `3.7` for unknown
//!   addresses and `3.8` for principals that hide their free/busy
//!   ([`MockDavServer::set_free_busy_visible`]);
//! * the schedule inbox: items can be listed, read, multiget and deleted
//!   (not written); tests deliver into it directly
//!   ([`MockDavServer::deliver`]);
//! * **auto-scheduling** ([`MockDavConfig::auto_schedule`],
//!   [`MockDavServer::set_auto_schedule`]; off by default): the `DAV`
//!   header then has `calendar-auto-schedule`, and writes schedule like a
//!   real server: an organizer's `PUT` sends `REQUEST`s to the attendees
//!   whose `SCHEDULE-AGENT` is `SERVER` (local ones get it in their inbox
//!   and their default calendar; others count as sent by mail), sets
//!   `SCHEDULE-STATUS` on them and then answers without an `ETag` (the
//!   stored object differs from the one sent); an organizer's `DELETE`
//!   sends `CANCEL`s; an attendee's `PUT` with a changed `PARTSTAT` (or
//!   `DELETE`, as declined) sends a `REPLY` to the organizer and updates
//!   the organizer's copy, unless the request carries `Schedule-Reply: F`.
//!   Every iTIP message is recorded ([`MockDavServer::scheduled_messages`]).

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use base64::Engine as _;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use crate::MockNetError;
use crate::stream::MockStream;

mod ical;
mod schedule;
mod xml;

use xml::{Element, escape};

/// The path the mock serves its principals under (`<user>/` follows).
pub const PRINCIPAL_PATH: &str = "/dav/principals/";
/// The context path `/.well-known/` redirects to.
pub const CONTEXT_PATH: &str = "/dav/";
/// Prefix of calendar homes (`<user>/` follows).
pub const CALENDARS_PATH: &str = "/dav/calendars/";
/// Prefix of address book homes (`<user>/` follows).
pub const ADDRESSBOOKS_PATH: &str = "/dav/addressbooks/";

const NS_DAV: &str = "DAV:";
const NS_CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const NS_CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";
const NS_CS: &str = "http://calendarserver.org/ns/";
const NS_APPLE: &str = "http://apple.com/ns/ical/";
const SYNC_TOKEN_PREFIX: &str = "http://nail.mock/sync/";
const MAX_BODY: usize = 16 * 1024 * 1024;

/// Configuration of a [`MockDavServer`].
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent feature switches of a mock server; each test turns single ones on or off"
)]
pub struct MockDavConfig {
    /// The user name (also the last segment of the principal and homes).
    pub username: String,
    /// The password for Basic authentication.
    pub password: String,
    /// A bearer token accepted instead of Basic (OAuth2), if any.
    pub bearer_token: Option<String>,
    /// Serve over TLS (a certificate for `localhost` / `127.0.0.1` from
    /// [`nail_testkit::tls`]); otherwise plain HTTP.
    pub tls: bool,
    /// Answer `/.well-known/caldav` / `carddav` with a redirect.
    pub well_known: bool,
    /// Offer `sync-collection` (RFC 6578) and the `sync-token` property.
    pub sync_collection: bool,
    /// Offer the CalendarServer `getctag` property.
    pub ctag: bool,
    /// Send `ETag` in answers to `PUT`.
    pub etag_on_put: bool,
    /// Most results per `sync-collection` answer before it is truncated
    /// (the client's own `limit` applies when lower).
    pub sync_page_size: Option<usize>,
    /// Create the default collections.
    pub seed: bool,
    /// The user's calendar-user address (`calendar-user-address-set`).
    pub email: String,
    /// Offer RFC 6638 scheduling: schedule inbox / outbox, free/busy
    /// `POST`, `calendar-schedule` in the `DAV` header.
    pub scheduling: bool,
    /// The server schedules itself (`calendar-auto-schedule`): see the
    /// module docs. Needs `scheduling`.
    pub auto_schedule: bool,
    /// More principals on the same server.
    pub users: Vec<MockDavUser>,
}

/// Another principal of a [`MockDavServer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockDavUser {
    /// User name (path segment and Basic login).
    pub username: String,
    /// Password for Basic authentication.
    pub password: String,
    /// Calendar-user address.
    pub email: String,
    /// `calendar-user-type`: `INDIVIDUAL`, `ROOM`, `RESOURCE`, `GROUP`.
    pub user_type: String,
    /// Display name of the principal.
    pub display_name: String,
}

impl MockDavUser {
    /// A person `username` with password `secret` and address
    /// `<username>@example.org`.
    pub fn new(username: &str) -> Self {
        Self {
            username: username.to_owned(),
            password: "secret".to_owned(),
            email: format!("{username}@example.org"),
            user_type: "INDIVIDUAL".to_owned(),
            display_name: username.to_owned(),
        }
    }

    /// A room `username` (`calendar-user-type` `ROOM`).
    pub fn room(username: &str, display_name: &str) -> Self {
        Self {
            user_type: "ROOM".to_owned(),
            display_name: display_name.to_owned(),
            ..Self::new(username)
        }
    }
}

/// An iTIP message the server scheduled (auto-scheduling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockScheduleMessage {
    /// `REQUEST`, `REPLY` or `CANCEL`.
    pub method: String,
    /// Sender address.
    pub from: String,
    /// Recipient address.
    pub to: String,
    /// The event's `UID`.
    pub uid: String,
    /// Delivered to a local inbox (`false`: an external address, which a
    /// real server would mail).
    pub local: bool,
    /// The iTIP text.
    pub data: String,
}

impl Default for MockDavConfig {
    fn default() -> Self {
        Self {
            username: "ada".to_owned(),
            password: "secret".to_owned(),
            bearer_token: None,
            tls: false,
            well_known: true,
            sync_collection: true,
            ctag: true,
            etag_on_put: true,
            sync_page_size: None,
            seed: true,
            email: "ada@example.org".to_owned(),
            scheduling: true,
            auto_schedule: false,
            users: Vec::new(),
        }
    }
}

/// What a collection holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockCollectionKind {
    /// A calendar with these components (`VEVENT`, `VTODO`).
    Calendar(Vec<String>),
    /// An address book.
    AddressBook,
    /// A schedule inbox (RFC 6638): iTIP messages delivered to the user.
    ScheduleInbox,
    /// A schedule outbox (RFC 6638): free/busy requests are `POST`ed here.
    ScheduleOutbox,
}

/// A stored calendar object or vCard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockObject {
    /// Its data.
    pub data: String,
    /// Its current ETag (quoted).
    pub etag: String,
    seq: u64,
}

/// A collection and its members.
#[derive(Debug, Clone)]
pub struct MockCollection {
    /// Calendar or address book.
    pub kind: MockCollectionKind,
    /// `displayname`.
    pub display_name: String,
    /// Apple `calendar-color`.
    pub color: Option<String>,
    /// Only `read` privilege: writes answer `403`.
    pub read_only: bool,
    /// Members by name (the last path segment).
    pub objects: BTreeMap<String, MockObject>,
    tombstones: BTreeMap<String, u64>,
    /// Sync tokens below this are invalid.
    floor: u64,
    changed: u64,
}

impl MockCollection {
    fn new(kind: MockCollectionKind, display_name: &str, seq: u64) -> Self {
        Self {
            kind,
            display_name: display_name.to_owned(),
            color: None,
            read_only: false,
            objects: BTreeMap::new(),
            tombstones: BTreeMap::new(),
            floor: 0,
            changed: seq,
        }
    }

    /// Holds iCalendar objects (a calendar or a schedule inbox).
    fn is_calendar(&self) -> bool {
        matches!(
            self.kind,
            MockCollectionKind::Calendar(_) | MockCollectionKind::ScheduleInbox
        )
    }

    fn is_schedule(&self) -> bool {
        matches!(
            self.kind,
            MockCollectionKind::ScheduleInbox | MockCollectionKind::ScheduleOutbox
        )
    }
}

/// A request the server received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DavRequest {
    /// `PROPFIND`, `PUT`, …
    pub method: String,
    /// Path (without query).
    pub path: String,
    /// `Depth` header.
    pub depth: Option<String>,
    /// `If-Match` header.
    pub if_match: Option<String>,
    /// `If-None-Match` header.
    pub if_none_match: Option<String>,
    /// `Authorization` header.
    pub authorization: Option<String>,
    /// `Content-Type` header.
    pub content_type: Option<String>,
    /// `Schedule-Reply` header (RFC 6638 §8.1).
    pub schedule_reply: Option<String>,
    /// The body as text.
    pub body: String,
}

#[derive(Debug)]
struct Data {
    seq: u64,
    collections: BTreeMap<String, MockCollection>,
    failures: VecDeque<(String, u16)>,
    /// Every principal by user name (the configured user included).
    users: BTreeMap<String, MockDavUser>,
    auto_schedule: bool,
    scheduled: Vec<MockScheduleMessage>,
    /// Users whose free/busy others may not see (`3.8`).
    hidden_free_busy: Vec<String>,
}

impl Data {
    fn next(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn token(&self) -> String {
        format!("{SYNC_TOKEN_PREFIX}{}", self.seq)
    }

    /// Stores `data` as `name` in `collection`; returns the new ETag.
    fn put(&mut self, collection: &str, name: &str, data: &str) -> Option<String> {
        let seq = self.next();
        let coll = self.collections.get_mut(collection)?;
        let etag = format!("\"e{seq}\"");
        coll.objects.insert(
            name.to_owned(),
            MockObject {
                data: data.to_owned(),
                etag: etag.clone(),
                seq,
            },
        );
        coll.tombstones.remove(name);
        coll.changed = seq;
        Some(etag)
    }

    fn delete(&mut self, collection: &str, name: &str) -> bool {
        let seq = self.next();
        let Some(coll) = self.collections.get_mut(collection) else {
            return false;
        };
        if coll.objects.remove(name).is_none() {
            return false;
        }
        coll.tombstones.insert(name.to_owned(), seq);
        coll.changed = seq;
        true
    }
}

#[derive(Debug)]
struct Inner {
    config: MockDavConfig,
    data: Mutex<Data>,
    requests: Mutex<Vec<DavRequest>>,
}

fn principal_of(user: &str) -> String {
    format!("{PRINCIPAL_PATH}{user}/")
}
fn calendar_home_of(user: &str) -> String {
    format!("{CALENDARS_PATH}{user}/")
}
fn addressbook_home_of(user: &str) -> String {
    format!("{ADDRESSBOOKS_PATH}{user}/")
}
fn inbox_of(user: &str) -> String {
    format!("{CALENDARS_PATH}{user}/inbox/")
}
fn outbox_of(user: &str) -> String {
    format!("{CALENDARS_PATH}{user}/outbox/")
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Inner {
    fn principal(&self) -> String {
        principal_of(&self.config.username)
    }
    fn calendar_home(&self) -> String {
        calendar_home_of(&self.config.username)
    }
    fn addressbook_home(&self) -> String {
        addressbook_home_of(&self.config.username)
    }
}

impl Data {
    /// Creates the principal's inbox / outbox (with scheduling) and, with
    /// `seed`, its `personal` calendar.
    fn add_user(&mut self, user: MockDavUser, scheduling: bool, seed: bool) {
        let name = user.username.clone();
        if scheduling {
            for (path, kind) in [
                (inbox_of(&name), MockCollectionKind::ScheduleInbox),
                (outbox_of(&name), MockCollectionKind::ScheduleOutbox),
            ] {
                let seq = self.next();
                self.collections
                    .entry(path)
                    .or_insert_with(|| MockCollection::new(kind, "", seq));
            }
        }
        if seed {
            let seq = self.next();
            self.collections
                .entry(format!("{}personal/", calendar_home_of(&name)))
                .or_insert_with(|| {
                    MockCollection::new(
                        MockCollectionKind::Calendar(vec!["VEVENT".into(), "VTODO".into()]),
                        "Personal",
                        seq,
                    )
                });
        }
        self.users.insert(name, user);
    }

    /// The user whose address is `email`.
    fn user_by_email(&self, email: &str) -> Option<&MockDavUser> {
        let email = ical::address(email);
        self.users
            .values()
            .find(|u| u.email.eq_ignore_ascii_case(&email))
    }

    /// Where the server puts invitations for `user`: `personal`, else the
    /// first event calendar in the home.
    fn default_calendar(&self, user: &str) -> Option<String> {
        let home = calendar_home_of(user);
        let personal = format!("{home}personal/");
        if self.collections.contains_key(&personal) {
            return Some(personal);
        }
        self.collections
            .iter()
            .find(|(p, c)| {
                p.starts_with(&home)
                    && matches!(&c.kind, MockCollectionKind::Calendar(comps) if comps.iter().any(|c| c == "VEVENT"))
            })
            .map(|(p, _)| p.clone())
    }

    /// The owner of a path under a calendar home.
    fn owner_of(&self, path: &str) -> Option<String> {
        let rest = path.strip_prefix(CALENDARS_PATH)?;
        let user = rest.split('/').next()?;
        self.users.contains_key(user).then(|| user.to_owned())
    }
}

/// A TLS listener for [`axum::serve`].
struct TlsListener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
}

impl axum::serve::Listener for TlsListener {
    type Io = MockStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let Ok((tcp, addr)) = self.tcp.accept().await else {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            };
            let handshake =
                tokio::time::timeout(Duration::from_secs(10), self.acceptor.accept(tcp)).await;
            if let Ok(Ok(tls)) = handshake {
                return (MockStream::Tls(Box::new(tls)), addr);
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

/// A running mock CalDAV / CardDAV server on `127.0.0.1`. Stops when
/// dropped.
#[derive(Debug)]
pub struct MockDavServer {
    addr: SocketAddr,
    tls: bool,
    ca_der: Option<Vec<u8>>,
    inner: Arc<Inner>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MockDavServer {
    /// Starts a server on a free loopback port.
    pub async fn start(config: MockDavConfig) -> Result<Self, MockNetError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let tls = config.tls;
        let mut data = Data {
            seq: 0,
            collections: BTreeMap::new(),
            failures: VecDeque::new(),
            users: BTreeMap::new(),
            auto_schedule: config.scheduling && config.auto_schedule,
            scheduled: Vec::new(),
            hidden_free_busy: Vec::new(),
        };
        data.add_user(
            MockDavUser {
                username: config.username.clone(),
                password: config.password.clone(),
                email: config.email.clone(),
                user_type: "INDIVIDUAL".to_owned(),
                display_name: config.username.clone(),
            },
            config.scheduling,
            false,
        );
        for user in &config.users {
            data.add_user(user.clone(), config.scheduling, true);
        }
        if config.seed {
            let cal_home = format!("{CALENDARS_PATH}{}/", config.username);
            let book_home = format!("{ADDRESSBOOKS_PATH}{}/", config.username);
            let seeds = [
                (
                    format!("{cal_home}personal/"),
                    MockCollectionKind::Calendar(vec!["VEVENT".into(), "VTODO".into()]),
                    "Personal",
                    Some("#3366ccff"),
                    false,
                ),
                (
                    format!("{cal_home}tasks/"),
                    MockCollectionKind::Calendar(vec!["VTODO".into()]),
                    "Tasks",
                    None,
                    false,
                ),
                (
                    format!("{cal_home}holidays/"),
                    MockCollectionKind::Calendar(vec!["VEVENT".into()]),
                    "Holidays",
                    None,
                    true,
                ),
                (
                    format!("{book_home}contacts/"),
                    MockCollectionKind::AddressBook,
                    "Contacts",
                    None,
                    false,
                ),
            ];
            for (path, kind, name, color, read_only) in seeds {
                let seq = data.next();
                let mut coll = MockCollection::new(kind, name, seq);
                coll.color = color.map(str::to_owned);
                coll.read_only = read_only;
                data.collections.insert(path, coll);
            }
        }
        let inner = Arc::new(Inner {
            config,
            data: Mutex::new(data),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .fallback(handle)
            .with_state(Arc::clone(&inner));
        let (shutdown, mut stop) = watch::channel(false);
        let stopped = async move {
            let _ = stop.changed().await;
        };
        let (task, ca_der) = if tls {
            let material = nail_testkit::tls::TestTls::generate()?;
            let listener = TlsListener {
                tcp: listener,
                acceptor: material.acceptor.clone(),
            };
            let task = tokio::spawn(async move {
                drop(
                    axum::serve(listener, app)
                        .with_graceful_shutdown(stopped)
                        .await,
                );
            });
            (task, Some(material.ca_der))
        } else {
            let task = tokio::spawn(async move {
                drop(
                    axum::serve(listener, app)
                        .with_graceful_shutdown(stopped)
                        .await,
                );
            });
            (task, None)
        };
        Ok(Self {
            addr,
            tls,
            ca_der,
            inner,
            shutdown,
            task,
        })
    }

    /// The listening address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>` (or `https://localhost:<port>` with TLS,
    /// matching the certificate).
    pub fn base_url(&self) -> String {
        if self.tls {
            format!("https://localhost:{}", self.addr.port())
        } else {
            format!("http://{}", self.addr)
        }
    }

    /// `base_url()` + `path`.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url())
    }

    /// DER of the CA that signed the server certificate (TLS only).
    pub fn ca_der(&self) -> Option<&[u8]> {
        self.ca_der.as_deref()
    }

    /// The principal's path.
    pub fn principal_path(&self) -> String {
        self.inner.principal()
    }

    /// The calendar home's path.
    pub fn calendar_home(&self) -> String {
        self.inner.calendar_home()
    }

    /// The address book home's path.
    pub fn addressbook_home(&self) -> String {
        self.inner.addressbook_home()
    }

    /// Path of the calendar `name` in the calendar home.
    pub fn calendar_path(&self, name: &str) -> String {
        format!("{}{name}/", self.inner.calendar_home())
    }

    /// Path of the address book `name` in the address book home.
    pub fn address_book_path(&self, name: &str) -> String {
        format!("{}{name}/", self.inner.addressbook_home())
    }

    /// Adds (or replaces) a calendar at `path` holding `components`.
    pub fn add_calendar(&self, path: &str, display_name: &str, components: &[&str]) {
        let mut data = lock(&self.inner.data);
        let seq = data.next();
        let kind =
            MockCollectionKind::Calendar(components.iter().map(|c| (*c).to_owned()).collect());
        data.collections.insert(
            path.to_owned(),
            MockCollection::new(kind, display_name, seq),
        );
    }

    /// Adds (or replaces) an address book at `path`.
    pub fn add_address_book(&self, path: &str, display_name: &str) {
        let mut data = lock(&self.inner.data);
        let seq = data.next();
        data.collections.insert(
            path.to_owned(),
            MockCollection::new(MockCollectionKind::AddressBook, display_name, seq),
        );
    }

    /// Removes a collection with its members.
    pub fn remove_collection(&self, path: &str) -> bool {
        lock(&self.inner.data).collections.remove(path).is_some()
    }

    /// Changes a collection's display name (another client's PROPPATCH).
    pub fn rename_collection(&self, path: &str, display_name: &str) {
        let mut data = lock(&self.inner.data);
        let seq = data.next();
        if let Some(coll) = data.collections.get_mut(path) {
            display_name.clone_into(&mut coll.display_name);
            coll.changed = seq;
        }
    }

    /// Makes a collection read-only (or writable).
    pub fn set_read_only(&self, path: &str, read_only: bool) {
        if let Some(coll) = lock(&self.inner.data).collections.get_mut(path) {
            coll.read_only = read_only;
        }
    }

    /// A copy of a collection.
    pub fn collection(&self, path: &str) -> Option<MockCollection> {
        lock(&self.inner.data).collections.get(path).cloned()
    }

    /// Paths of every calendar and address book (schedule inboxes and
    /// outboxes are left out).
    pub fn collection_paths(&self) -> Vec<String> {
        lock(&self.inner.data)
            .collections
            .iter()
            .filter(|(_, c)| !c.is_schedule())
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// Adds a principal (with its `personal` calendar, and inbox / outbox
    /// when scheduling is on).
    pub fn add_user(&self, user: MockDavUser) {
        lock(&self.inner.data).add_user(user, self.inner.config.scheduling, true);
    }

    /// The principal path of `user`.
    pub fn user_principal_path(&self, user: &str) -> String {
        principal_of(user)
    }

    /// Path of `user`'s calendar `name`.
    pub fn user_calendar_path(&self, user: &str, name: &str) -> String {
        format!("{}{name}/", calendar_home_of(user))
    }

    /// Path of `user`'s schedule inbox.
    pub fn inbox_path(&self, user: &str) -> String {
        inbox_of(user)
    }

    /// Path of `user`'s schedule outbox.
    pub fn outbox_path(&self, user: &str) -> String {
        outbox_of(user)
    }

    /// The items in `user`'s schedule inbox by name.
    pub fn inbox(&self, user: &str) -> Vec<(String, MockObject)> {
        lock(&self.inner.data)
            .collections
            .get(&inbox_of(user))
            .map(|c| {
                c.objects
                    .iter()
                    .map(|(n, o)| (n.clone(), o.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Puts an iTIP message into `user`'s schedule inbox, as if the server
    /// had delivered it (from another server, or an iMIP gateway); returns
    /// its ETag, `None` without such an inbox.
    pub fn deliver(&self, user: &str, name: &str, data: &str) -> Option<String> {
        lock(&self.inner.data).put(&inbox_of(user), name, data)
    }

    /// Switches auto-scheduling on or off (see the module docs).
    pub fn set_auto_schedule(&self, on: bool) {
        lock(&self.inner.data).auto_schedule = on && self.inner.config.scheduling;
    }

    /// Whether the server schedules itself now.
    pub fn auto_schedule(&self) -> bool {
        lock(&self.inner.data).auto_schedule
    }

    /// Whether others may see `user`'s free/busy (default yes; no =
    /// request status `3.8`).
    pub fn set_free_busy_visible(&self, user: &str, visible: bool) {
        let mut data = lock(&self.inner.data);
        data.hidden_free_busy.retain(|u| u != user);
        if !visible {
            data.hidden_free_busy.push(user.to_owned());
        }
    }

    /// Every iTIP message the server scheduled, in order.
    pub fn scheduled_messages(&self) -> Vec<MockScheduleMessage> {
        lock(&self.inner.data).scheduled.clone()
    }

    /// Stores an object as another client would; returns its ETag, `None`
    /// when the collection does not exist.
    pub fn put_object(&self, collection: &str, name: &str, data: &str) -> Option<String> {
        lock(&self.inner.data).put(collection, name, data)
    }

    /// Deletes an object as another client would.
    pub fn delete_object(&self, collection: &str, name: &str) -> bool {
        lock(&self.inner.data).delete(collection, name)
    }

    /// An object's data and ETag.
    pub fn object(&self, collection: &str, name: &str) -> Option<MockObject> {
        lock(&self.inner.data)
            .collections
            .get(collection)?
            .objects
            .get(name)
            .cloned()
    }

    /// Makes every sync token issued so far invalid (the server lost its
    /// history): `sync-collection` with an old token answers `403` with
    /// `valid-sync-token`.
    pub fn forget_sync_history(&self) {
        let mut data = lock(&self.inner.data);
        let seq = data.next();
        for coll in data.collections.values_mut() {
            coll.floor = seq;
            coll.tombstones.clear();
        }
    }

    /// The next request with `method` (any method for `"*"`) answers
    /// `status` with an empty body (`503` adds `Retry-After: 1`).
    pub fn fail_next(&self, method: &str, status: u16) {
        lock(&self.inner.data)
            .failures
            .push_back((method.to_ascii_uppercase(), status));
    }

    /// Every request received, in order.
    pub fn requests(&self) -> Vec<DavRequest> {
        lock(&self.inner.requests).clone()
    }

    /// Forgets the recorded requests.
    pub fn clear_requests(&self) {
        lock(&self.inner.requests).clear();
    }
}

impl Drop for MockDavServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

// ---------------------------------------------------------------------------
// Request handling

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn respond(status: u16, content_type: Option<&str>, body: String) -> Response {
    let mut out = Response::new(Body::from(body));
    *out.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if let Some(ct) = content_type
        && let Ok(value) = HeaderValue::from_str(ct)
    {
        out.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    out
}

fn with_header(mut response: Response, name: &'static str, value: &str) -> Response {
    if let Ok(value) = HeaderValue::from_str(value) {
        response.headers_mut().insert(name, value);
    }
    response
}

fn empty(status: u16) -> Response {
    respond(status, None, String::new())
}

fn multistatus(body: &str) -> Response {
    respond(
        207,
        Some("application/xml; charset=utf-8"),
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:multistatus xmlns:d=\"DAV:\" \
             xmlns:c=\"{NS_CALDAV}\" xmlns:card=\"{NS_CARDDAV}\" xmlns:cs=\"{NS_CS}\" \
             xmlns:ic=\"{NS_APPLE}\">{body}</d:multistatus>"
        ),
    )
}

fn dav_error(status: u16, ns: &str, condition: &str) -> Response {
    respond(
        status,
        Some("application/xml; charset=utf-8"),
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:error xmlns:d=\"DAV:\"><x:{condition} \
             xmlns:x=\"{ns}\"/></d:error>"
        ),
    )
}

/// The user the request signs in as, if the login is valid (a bearer
/// token belongs to the configured user).
fn authorized(
    config: &MockDavConfig,
    users: &BTreeMap<String, MockDavUser>,
    authorization: Option<&str>,
) -> Option<String> {
    let value = authorization?;
    if let Some(token) = value.strip_prefix("Bearer ") {
        return (config.bearer_token.as_deref() == Some(token.trim()))
            .then(|| config.username.clone());
    }
    let encoded = value.strip_prefix("Basic ")?;
    users
        .values()
        .find(|u| {
            base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", u.username, u.password))
                == encoded.trim()
        })
        .map(|u| u.username.clone())
}

async fn handle(State(inner): State<Arc<Inner>>, request: Request) -> Response {
    let method = request.method().as_str().to_ascii_uppercase();
    let path = request.uri().path().to_owned();
    let headers = request.headers().clone();
    let body = to_bytes(request.into_body(), MAX_BODY)
        .await
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let recorded = DavRequest {
        method: method.clone(),
        path: path.clone(),
        depth: header_text(&headers, "depth"),
        if_match: header_text(&headers, "if-match"),
        if_none_match: header_text(&headers, "if-none-match"),
        authorization: header_text(&headers, "authorization"),
        content_type: header_text(&headers, "content-type"),
        schedule_reply: header_text(&headers, "schedule-reply"),
        body,
    };
    lock(&inner.requests).push(recorded.clone());

    {
        let mut data = lock(&inner.data);
        let failure = data
            .failures
            .iter()
            .position(|(m, _)| m == "*" || *m == method);
        if let Some(index) = failure
            && let Some((_, status)) = data.failures.remove(index)
        {
            let response = empty(status);
            return if status == 503 {
                with_header(response, "retry-after", "1")
            } else {
                response
            };
        }
    }

    if path.starts_with("/.well-known/") {
        let known = matches!(
            path.as_str(),
            "/.well-known/caldav" | "/.well-known/carddav"
        );
        if inner.config.well_known && known {
            return with_header(empty(301), "location", CONTEXT_PATH);
        }
        return empty(404);
    }
    if method == "OPTIONS" {
        let mut classes =
            "1, 3, access-control, calendar-access, addressbook, extended-mkcol".to_owned();
        if inner.config.scheduling {
            classes.push_str(", calendar-schedule");
        }
        if lock(&inner.data).auto_schedule {
            classes.push_str(", calendar-auto-schedule");
        }
        let response = with_header(empty(200), "dav", &classes);
        return with_header(
            response,
            "allow",
            "OPTIONS, GET, PUT, DELETE, PROPFIND, PROPPATCH, REPORT, MKCALENDAR, MKCOL, POST",
        );
    }
    let me = authorized(
        &inner.config,
        &lock(&inner.data).users,
        recorded.authorization.as_deref(),
    );
    let Some(me) = me else {
        return with_header(
            empty(401),
            "www-authenticate",
            "Basic realm=\"nail-mock-dav\", charset=\"UTF-8\"",
        );
    };
    match method.as_str() {
        "PROPFIND" => propfind(&inner, &recorded, &me),
        "REPORT" => report(&inner, &recorded, &me),
        "GET" | "HEAD" => get(&inner, &recorded),
        "PUT" => put(&inner, &recorded, &me),
        "DELETE" => delete(&inner, &recorded, &me),
        "MKCALENDAR" => mkcol(&inner, &recorded, true, &me),
        "MKCOL" => mkcol(&inner, &recorded, false, &me),
        "PROPPATCH" => proppatch(&inner, &recorded),
        "POST" => schedule::post(&inner, &recorded, &me),
        _ => empty(405),
    }
}

/// What a path names (homes and principals name their user).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Resource {
    Context,
    Principal(String),
    CalendarHome(String),
    AddressBookHome(String),
    Collection(String),
    Object(String, String),
}

fn resolve(_inner: &Inner, data: &Data, path: &str) -> Option<Resource> {
    let with_slash = if path.ends_with('/') {
        path.to_owned()
    } else {
        format!("{path}/")
    };
    if with_slash == CONTEXT_PATH {
        return Some(Resource::Context);
    }
    for user in data.users.keys() {
        if with_slash == principal_of(user) {
            return Some(Resource::Principal(user.clone()));
        }
        if with_slash == calendar_home_of(user) {
            return Some(Resource::CalendarHome(user.clone()));
        }
        if with_slash == addressbook_home_of(user) {
            return Some(Resource::AddressBookHome(user.clone()));
        }
    }
    if data.collections.contains_key(&with_slash) {
        return Some(Resource::Collection(with_slash));
    }
    let (parent, name) = split_member(path)?;
    let coll = data.collections.get(&parent)?;
    coll.objects
        .contains_key(&name)
        .then_some(Resource::Object(parent, name))
}

/// `/a/b/c.ics` → (`/a/b/`, `c.ics`).
fn split_member(path: &str) -> Option<(String, String)> {
    if path.ends_with('/') {
        return None;
    }
    let index = path.rfind('/')?;
    Some((path[..=index].to_owned(), path[index + 1..].to_owned()))
}

/// The requested properties of a PROPFIND / REPORT (`None` = allprop).
fn requested_props(root: &Element) -> Option<Vec<(String, String)>> {
    let prop = root.child(NS_DAV, "prop")?;
    Some(
        prop.children
            .iter()
            .map(|p| (p.ns.clone(), p.name.clone()))
            .collect(),
    )
}

const ALLPROP: &[(&str, &str)] = &[
    (NS_DAV, "resourcetype"),
    (NS_DAV, "displayname"),
    (NS_DAV, "getetag"),
    (NS_DAV, "getcontenttype"),
    (NS_CS, "getctag"),
    (NS_DAV, "sync-token"),
];

fn prop_tag(ns: &str) -> &'static str {
    match ns {
        NS_DAV => "d",
        NS_CALDAV => "c",
        NS_CARDDAV => "card",
        NS_CS => "cs",
        NS_APPLE => "ic",
        _ => "",
    }
}

/// `<p:name>value</p:name>` (or with an inline namespace for unknown ones).
fn element(ns: &str, name: &str, value: &str) -> String {
    let prefix = prop_tag(ns);
    if prefix.is_empty() {
        let ns = escape(ns);
        if value.is_empty() {
            format!("<x:{name} xmlns:x=\"{ns}\"/>")
        } else {
            format!("<x:{name} xmlns:x=\"{ns}\">{value}</x:{name}>")
        }
    } else if value.is_empty() {
        format!("<{prefix}:{name}/>")
    } else {
        format!("<{prefix}:{name}>{value}</{prefix}:{name}>")
    }
}

fn href(path: &str) -> String {
    format!("<d:href>{}</d:href>", escape(path))
}

/// The value of property `{ns}name` on `resource`, `None` = not defined.
fn prop_value(
    inner: &Inner,
    data: &Data,
    resource: &Resource,
    me: &str,
    (ns, name): (&str, &str),
) -> Option<String> {
    let config = &inner.config;
    let collection = match resource {
        Resource::Collection(path) | Resource::Object(path, _) => data.collections.get(path),
        _ => None,
    };
    let principal = match resource {
        Resource::Principal(user) => data.users.get(user),
        _ => None,
    };
    match (ns, name) {
        (NS_DAV, "current-user-principal") => Some(href(&principal_of(me))),
        (NS_DAV, "resourcetype") => Some(match resource {
            Resource::Context | Resource::CalendarHome(_) | Resource::AddressBookHome(_) => {
                "<d:collection/>".to_owned()
            }
            Resource::Principal(_) => "<d:collection/><d:principal/>".to_owned(),
            Resource::Collection(_) => match collection?.kind {
                MockCollectionKind::Calendar(_) => "<d:collection/><c:calendar/>".to_owned(),
                MockCollectionKind::AddressBook => "<d:collection/><card:addressbook/>".to_owned(),
                MockCollectionKind::ScheduleInbox => {
                    "<d:collection/><c:schedule-inbox/>".to_owned()
                }
                MockCollectionKind::ScheduleOutbox => {
                    "<d:collection/><c:schedule-outbox/>".to_owned()
                }
            },
            Resource::Object(..) => String::new(),
        }),
        (NS_DAV, "displayname") => match resource {
            Resource::Principal(_) => Some(escape(&principal?.display_name).into_owned()),
            Resource::Collection(_) => Some(escape(&collection?.display_name).into_owned()),
            _ => None,
        },
        (NS_DAV, "principal-URL") => principal.map(|p| href(&principal_of(&p.username))),
        (NS_CALDAV, "calendar-home-set") => principal.map(|p| href(&calendar_home_of(&p.username))),
        (NS_CARDDAV, "addressbook-home-set") => {
            principal.map(|p| href(&addressbook_home_of(&p.username)))
        }
        (NS_CALDAV, "calendar-user-address-set") => principal.map(|p| {
            format!(
                "{}{}",
                href(&format!("mailto:{}", p.email)),
                href(&principal_of(&p.username))
            )
        }),
        (NS_CALDAV, "calendar-user-type") => principal.map(|p| escape(&p.user_type).into_owned()),
        (NS_CALDAV, "schedule-inbox-URL") if config.scheduling => {
            principal.map(|p| href(&inbox_of(&p.username)))
        }
        (NS_CALDAV, "schedule-outbox-URL") if config.scheduling => {
            principal.map(|p| href(&outbox_of(&p.username)))
        }
        (NS_CALDAV, "schedule-default-calendar-URL") => match (resource, collection) {
            (Resource::Collection(path), Some(c))
                if c.kind == MockCollectionKind::ScheduleInbox =>
            {
                let owner = data.owner_of(path)?;
                data.default_calendar(&owner).map(|p| href(&p))
            }
            _ => None,
        },
        (NS_CS, "getctag") if config.ctag => match resource {
            Resource::Collection(_) => Some(format!("\"ctag-{}\"", collection?.changed)),
            _ => None,
        },
        (NS_DAV, "sync-token") if config.sync_collection => match resource {
            Resource::Collection(_) => {
                collection?;
                Some(escape(&data.token()).into_owned())
            }
            _ => None,
        },
        (NS_DAV, "supported-report-set") => match resource {
            Resource::Collection(_) => {
                let coll = collection.filter(|c| c.kind != MockCollectionKind::ScheduleOutbox)?;
                let mut reports = Vec::new();
                if coll.is_calendar() {
                    reports.extend(["<c:calendar-query/>", "<c:calendar-multiget/>"]);
                } else {
                    reports.extend(["<card:addressbook-query/>", "<card:addressbook-multiget/>"]);
                }
                if config.sync_collection {
                    reports.push("<d:sync-collection/>");
                }
                Some(reports.iter().fold(String::new(), |mut out, r| {
                    let _ = write!(
                        out,
                        "<d:supported-report><d:report>{r}</d:report></d:supported-report>"
                    );
                    out
                }))
            }
            _ => None,
        },
        (NS_CALDAV, "supported-calendar-component-set") => match &collection?.kind {
            MockCollectionKind::Calendar(components)
                if matches!(resource, Resource::Collection(_)) =>
            {
                Some(components.iter().fold(String::new(), |mut out, c| {
                    let _ = write!(out, "<c:comp name=\"{}\"/>", escape(c));
                    out
                }))
            }
            _ => None,
        },
        (NS_APPLE, "calendar-color") => match resource {
            Resource::Collection(_) => collection?.color.as_deref().map(|c| escape(c).into_owned()),
            _ => None,
        },
        (NS_DAV, "current-user-privilege-set") => {
            let read_only = collection.is_some_and(|c| c.read_only);
            Some(if read_only {
                "<d:privilege><d:read/></d:privilege>".to_owned()
            } else {
                "<d:privilege><d:read/></d:privilege><d:privilege><d:write/></d:privilege>\
                 <d:privilege><d:write-content/></d:privilege><d:privilege><d:bind/></d:privilege>\
                 <d:privilege><d:unbind/></d:privilege>"
                    .to_owned()
            })
        }
        (NS_DAV, "getetag") => match resource {
            Resource::Object(path, name) => {
                Some(escape(&data.collections.get(path)?.objects.get(name)?.etag).into_owned())
            }
            _ => None,
        },
        (NS_DAV, "getcontenttype") => match resource {
            Resource::Object(..) => Some(if collection?.is_calendar() {
                "text/calendar; charset=utf-8".to_owned()
            } else {
                "text/vcard; charset=utf-8".to_owned()
            }),
            _ => None,
        },
        (NS_CALDAV, "calendar-data") | (NS_CARDDAV, "address-data") => match resource {
            Resource::Object(path, name) => {
                Some(escape(&data.collections.get(path)?.objects.get(name)?.data).into_owned())
            }
            _ => None,
        },
        _ => None,
    }
}

fn resource_path(resource: &Resource) -> String {
    match resource {
        Resource::Context => CONTEXT_PATH.to_owned(),
        Resource::Principal(user) => principal_of(user),
        Resource::CalendarHome(user) => calendar_home_of(user),
        Resource::AddressBookHome(user) => addressbook_home_of(user),
        Resource::Collection(path) => path.clone(),
        Resource::Object(path, name) => format!("{path}{name}"),
    }
}

/// One `<d:response>` with the requested properties.
fn response_for(
    inner: &Inner,
    data: &Data,
    resource: &Resource,
    me: &str,
    props: Option<&[(String, String)]>,
) -> String {
    let mut found = String::new();
    let mut missing = String::new();
    let wanted: Vec<(String, String)> = match props {
        Some(props) => props.to_vec(),
        None => ALLPROP
            .iter()
            .map(|(ns, name)| ((*ns).to_owned(), (*name).to_owned()))
            .collect(),
    };
    for (ns, name) in &wanted {
        match prop_value(inner, data, resource, me, (ns, name)) {
            Some(value) => found.push_str(&element(ns, name, &value)),
            None if props.is_some() => missing.push_str(&element(ns, name, "")),
            None => {}
        }
    }
    let mut out = format!("<d:response>{}", href(&resource_path(resource)));
    if !found.is_empty() {
        let _ = write!(
            out,
            "<d:propstat><d:prop>{found}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>"
        );
    }
    if !missing.is_empty() {
        let _ = write!(
            out,
            "<d:propstat><d:prop>{missing}</d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>"
        );
    }
    out.push_str("</d:response>");
    out
}

fn parse_body(body: &str) -> Result<Option<Element>, Box<Response>> {
    if body.trim().is_empty() {
        return Ok(None);
    }
    xml::parse(body).map(Some).map_err(|_| Box::new(empty(400)))
}

fn propfind(inner: &Inner, request: &DavRequest, me: &str) -> Response {
    let root = match parse_body(&request.body) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let props = root.as_ref().and_then(requested_props);
    let depth = request.depth.as_deref().unwrap_or("infinity");
    if depth == "infinity" {
        return dav_error(403, NS_DAV, "propfind-finite-depth");
    }
    let data = lock(&inner.data);
    let Some(resource) = resolve(inner, &data, &request.path) else {
        return empty(404);
    };
    let mut resources = vec![resource.clone()];
    if depth == "1" {
        match &resource {
            Resource::CalendarHome(user) => resources.extend(
                data.collections
                    .iter()
                    .filter(|(p, c)| {
                        (c.is_calendar() || c.is_schedule())
                            && p.starts_with(&calendar_home_of(user))
                    })
                    .map(|(p, _)| Resource::Collection(p.clone())),
            ),
            Resource::AddressBookHome(user) => resources.extend(
                data.collections
                    .iter()
                    .filter(|(p, c)| {
                        c.kind == MockCollectionKind::AddressBook
                            && p.starts_with(&addressbook_home_of(user))
                    })
                    .map(|(p, _)| Resource::Collection(p.clone())),
            ),
            Resource::Collection(path) => {
                if let Some(coll) = data.collections.get(path) {
                    resources.extend(
                        coll.objects
                            .keys()
                            .map(|name| Resource::Object(path.clone(), name.clone())),
                    );
                }
            }
            _ => {}
        }
    }
    let body: String = resources
        .iter()
        .map(|r| response_for(inner, &data, r, me, props.as_deref()))
        .collect();
    multistatus(&body)
}

fn report(inner: &Inner, request: &DavRequest, me: &str) -> Response {
    let root = match parse_body(&request.body) {
        Ok(Some(root)) => root,
        Ok(None) => return empty(400),
        Err(response) => return *response,
    };
    let props = requested_props(&root);
    let data = lock(&inner.data);
    let Some(Resource::Collection(path)) = resolve(inner, &data, &request.path) else {
        return empty(404);
    };
    let Some(coll) = data.collections.get(&path) else {
        return empty(404);
    };
    let objects = |names: Vec<String>| -> String {
        names
            .iter()
            .map(|name| {
                response_for(
                    inner,
                    &data,
                    &Resource::Object(path.clone(), name.clone()),
                    me,
                    props.as_deref(),
                )
            })
            .collect()
    };
    match (root.ns.as_str(), root.name.as_str()) {
        (NS_CALDAV, "calendar-query") if coll.is_calendar() => {
            let names = coll
                .objects
                .iter()
                .filter(|(_, o)| calendar_filter_matches(root.child(NS_CALDAV, "filter"), &o.data))
                .map(|(n, _)| n.clone())
                .collect();
            multistatus(&objects(names))
        }
        (NS_CARDDAV, "addressbook-query") if !coll.is_calendar() => {
            let names = coll
                .objects
                .iter()
                .filter(|(_, o)| card_filter_matches(root.child(NS_CARDDAV, "filter"), &o.data))
                .map(|(n, _)| n.clone())
                .collect();
            multistatus(&objects(names))
        }
        (NS_CALDAV, "calendar-multiget") | (NS_CARDDAV, "addressbook-multiget") => {
            if coll.is_calendar() != (root.ns == NS_CALDAV) {
                return dav_error(403, NS_DAV, "supported-report");
            }
            let mut body = String::new();
            for target in root.children_named(NS_DAV, "href") {
                let target = target.text.trim();
                match split_member(target) {
                    Some((parent, name)) if parent == path && coll.objects.contains_key(&name) => {
                        body.push_str(&objects(vec![name]));
                    }
                    _ => {
                        let _ = write!(
                            body,
                            "<d:response>{}<d:status>HTTP/1.1 404 Not Found</d:status></d:response>",
                            href(target)
                        );
                    }
                }
            }
            multistatus(&body)
        }
        (NS_DAV, "sync-collection") if inner.config.sync_collection => {
            sync_collection(inner, &data, (&path, coll), &root, me, props.as_deref())
        }
        _ => dav_error(403, NS_DAV, "supported-report"),
    }
}

fn sync_collection(
    inner: &Inner,
    data: &Data,
    (path, coll): (&str, &MockCollection),
    root: &Element,
    me: &str,
    props: Option<&[(String, String)]>,
) -> Response {
    let token = root.child_text(NS_DAV, "sync-token").unwrap_or_default();
    let since = if token.is_empty() {
        0
    } else {
        match token
            .strip_prefix(SYNC_TOKEN_PREFIX)
            .and_then(|n| n.parse::<u64>().ok())
        {
            Some(n) if n >= coll.floor && n <= data.seq => n,
            _ => return dav_error(403, NS_DAV, "valid-sync-token"),
        }
    };
    // (seq, name, deleted), oldest first.
    let mut changes: Vec<(u64, String, bool)> = coll
        .objects
        .iter()
        .filter(|(_, o)| o.seq > since)
        .map(|(n, o)| (o.seq, n.clone(), false))
        .collect();
    if since > 0 {
        changes.extend(
            coll.tombstones
                .iter()
                .filter(|(_, s)| **s > since)
                .map(|(n, s)| (*s, n.clone(), true)),
        );
    }
    changes.sort();
    let client_limit = root
        .path(&[(NS_DAV, "limit"), (NS_DAV, "nresults")])
        .and_then(|n| n.text.trim().parse::<usize>().ok());
    let limit = match (client_limit, inner.config.sync_page_size) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let mut new_token = data.seq;
    let mut truncated = false;
    if let Some(limit) = limit.map(|l| l.max(1))
        && changes.len() > limit
    {
        changes.truncate(limit);
        new_token = changes.last().map_or(since, |c| c.0);
        truncated = true;
    }
    let mut body = String::new();
    for (_, name, deleted) in &changes {
        if *deleted {
            let _ = write!(
                body,
                "<d:response>{}<d:status>HTTP/1.1 404 Not Found</d:status></d:response>",
                href(&format!("{path}{name}"))
            );
        } else {
            body.push_str(&response_for(
                inner,
                data,
                &Resource::Object(path.to_owned(), name.clone()),
                me,
                props,
            ));
        }
    }
    if truncated {
        let _ = write!(
            body,
            "<d:response>{}<d:status>HTTP/1.1 507 Insufficient Storage</d:status>\
             <d:error><d:number-of-matches-within-limits/></d:error></d:response>",
            href(path)
        );
    }
    let _ = write!(
        body,
        "<d:sync-token>{}</d:sync-token>",
        escape(&format!("{SYNC_TOKEN_PREFIX}{new_token}"))
    );
    multistatus(&body)
}

/// The value of the first `NAME[;params]:value` line of `data` inside the
/// first `component` block.
fn ical_value(data: &str, component: &str, name: &str) -> Option<String> {
    let begin = format!("BEGIN:{component}");
    let mut inside = false;
    for line in data.lines() {
        let line = line.trim_end_matches('\r');
        if line.eq_ignore_ascii_case(&begin) {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.to_ascii_uppercase().starts_with("END:") {
            if line[4..].eq_ignore_ascii_case(component) {
                return None;
            }
            continue;
        }
        let (key, value) = line.split_once(':')?;
        let key = key.split(';').next().unwrap_or_default();
        if key.eq_ignore_ascii_case(name) {
            return Some(value.trim().to_owned());
        }
    }
    None
}

/// `20261001` / `20261001T100000` / `20261001T100000Z` → `20261001T100000Z`
/// (floating and zoned times are compared as if UTC: good enough for a mock).
fn normalize_time(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('Z');
    match value.len() {
        8 => Some(format!("{value}T000000Z")),
        15 => Some(format!("{value}Z")),
        _ => None,
    }
}

fn calendar_filter_matches(filter: Option<&Element>, data: &str) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let Some(vcalendar) = filter.child(NS_CALDAV, "comp-filter") else {
        return true;
    };
    let Some(component) = vcalendar.child(NS_CALDAV, "comp-filter") else {
        return true;
    };
    let name = component
        .attr("name")
        .unwrap_or_default()
        .to_ascii_uppercase();
    if !name.is_empty() && !data.contains(&format!("BEGIN:{name}")) {
        return false;
    }
    let Some(range) = component.child(NS_CALDAV, "time-range") else {
        return true;
    };
    let start = range.attr("start").map(str::to_owned);
    let end = range.attr("end").map(str::to_owned);
    let comp = if name.is_empty() {
        "VEVENT".to_owned()
    } else {
        name
    };
    if ical_value(data, &comp, "RRULE").is_some() || ical_value(data, &comp, "RDATE").is_some() {
        return true;
    }
    let Some(dtstart) = ical_value(data, &comp, "DTSTART")
        .or_else(|| ical_value(data, &comp, "DUE"))
        .and_then(|v| normalize_time(&v))
    else {
        return true;
    };
    let dtend = ical_value(data, &comp, "DTEND")
        .or_else(|| ical_value(data, &comp, "DUE"))
        .and_then(|v| normalize_time(&v))
        .unwrap_or_else(|| dtstart.clone());
    let after_start = start.is_none_or(|s| dtend > s || (dtend == dtstart && dtstart >= s));
    let before_end = end.is_none_or(|e| dtstart < e);
    after_start && before_end
}

fn card_filter_matches(filter: Option<&Element>, data: &str) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let all = filter.attr("test") == Some("allof");
    let mut results = filter.children_named(NS_CARDDAV, "prop-filter").map(|pf| {
        let name = pf.attr("name").unwrap_or_default().to_owned();
        let needle = pf
            .child(NS_CARDDAV, "text-match")
            .map(|t| t.text.trim().to_lowercase())
            .unwrap_or_default();
        data.lines().any(|line| {
            let Some((key, value)) = line.split_once(':') else {
                return false;
            };
            let key = key.split(';').next().unwrap_or_default();
            let key = key.rsplit('.').next().unwrap_or_default();
            key.eq_ignore_ascii_case(&name) && value.to_lowercase().contains(&needle)
        })
    });
    if all {
        results.all(|r| r)
    } else {
        let mut any = false;
        let mut seen = false;
        for r in results {
            seen = true;
            any |= r;
        }
        any || !seen
    }
}

fn get(inner: &Inner, request: &DavRequest) -> Response {
    let data = lock(&inner.data);
    match resolve(inner, &data, &request.path) {
        Some(Resource::Object(path, name)) => {
            let coll = &data.collections[&path];
            let object = &coll.objects[&name];
            let content_type = if coll.is_calendar() {
                "text/calendar; charset=utf-8"
            } else {
                "text/vcard; charset=utf-8"
            };
            let body = if request.method == "HEAD" {
                String::new()
            } else {
                object.data.clone()
            };
            with_header(respond(200, Some(content_type), body), "etag", &object.etag)
        }
        Some(_) => empty(405),
        None => empty(404),
    }
}

fn precondition_fails(request: &DavRequest, current: Option<&MockObject>) -> bool {
    if let Some(none_match) = request.if_none_match.as_deref()
        && none_match.trim() == "*"
        && current.is_some()
    {
        return true;
    }
    if let Some(if_match) = request.if_match.as_deref() {
        let if_match = if_match.trim();
        return match current {
            None => true,
            Some(object) => if_match != "*" && if_match != object.etag,
        };
    }
    false
}

fn put(inner: &Inner, request: &DavRequest, me: &str) -> Response {
    let Some((parent, name)) = split_member(&request.path) else {
        return empty(405);
    };
    let mut data = lock(&inner.data);
    let Some(coll) = data.collections.get(&parent) else {
        return empty(409);
    };
    if coll.read_only || coll.is_schedule() {
        return dav_error(403, NS_DAV, "need-privileges");
    }
    let expected = if coll.is_calendar() {
        (
            "text/calendar",
            "BEGIN:VCALENDAR",
            NS_CALDAV,
            "supported-calendar-data",
        )
    } else {
        (
            "text/vcard",
            "BEGIN:VCARD",
            NS_CARDDAV,
            "supported-address-data",
        )
    };
    let content_type_ok = request
        .content_type
        .as_deref()
        .is_some_and(|ct| ct.to_ascii_lowercase().starts_with(expected.0));
    if !content_type_ok || !request.body.trim_start().starts_with(expected.1) {
        return dav_error(403, expected.2, expected.3);
    }
    let current = coll.objects.get(&name);
    if precondition_fails(request, current) {
        return empty(412);
    }
    let created = current.is_none();
    let previous = current.map(|o| o.data.clone());
    let Some(etag) = data.put(&parent, &name, &request.body) else {
        return empty(409);
    };
    let rewritten = data.auto_schedule
        && schedule::after_put(
            &mut data,
            (&parent, &name),
            me,
            previous.as_deref(),
            request,
        );
    let response = empty(if created { 201 } else { 204 });
    if inner.config.etag_on_put && !rewritten {
        with_header(response, "etag", &etag)
    } else {
        response
    }
}

fn delete(inner: &Inner, request: &DavRequest, me: &str) -> Response {
    let mut data = lock(&inner.data);
    match resolve(inner, &data, &request.path) {
        Some(Resource::Object(path, name)) => {
            let coll = &data.collections[&path];
            if coll.read_only {
                return dav_error(403, NS_DAV, "need-privileges");
            }
            if precondition_fails(request, coll.objects.get(&name)) {
                return empty(412);
            }
            let previous = coll
                .is_calendar()
                .then(|| coll.objects.get(&name).map(|o| o.data.clone()))
                .flatten()
                .filter(|_| coll.kind != MockCollectionKind::ScheduleInbox);
            data.delete(&path, &name);
            if data.auto_schedule
                && let Some(previous) = previous
            {
                schedule::after_delete(&mut data, me, &previous, request);
            }
            empty(204)
        }
        Some(Resource::Collection(path)) if data.collections[&path].is_schedule() => {
            dav_error(403, NS_DAV, "need-privileges")
        }
        Some(Resource::Collection(path)) => {
            data.collections.remove(&path);
            empty(204)
        }
        Some(_) => empty(403),
        None => {
            if request.if_match.is_some() {
                empty(412)
            } else {
                empty(404)
            }
        }
    }
}

/// Properties set by MKCALENDAR / MKCOL / PROPPATCH (`set` blocks).
fn set_props(root: Option<&Element>) -> Vec<&Element> {
    let Some(root) = root else {
        return Vec::new();
    };
    root.children_named(NS_DAV, "set")
        .filter_map(|s| s.child(NS_DAV, "prop"))
        .flat_map(|p| p.children.iter())
        .collect()
}

fn mkcol(inner: &Inner, request: &DavRequest, calendar: bool, me: &str) -> Response {
    let root = match parse_body(&request.body) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = if request.path.ends_with('/') {
        request.path.clone()
    } else {
        format!("{}/", request.path)
    };
    let home = if calendar {
        calendar_home_of(me)
    } else {
        addressbook_home_of(me)
    };
    let parent_ok = path.starts_with(&home)
        && path.len() > home.len()
        && !path[home.len()..path.len() - 1].contains('/');
    let mut data = lock(&inner.data);
    if resolve(inner, &data, &path).is_some() {
        return empty(405);
    }
    if !parent_ok {
        return empty(409);
    }
    let props = set_props(root.as_ref());
    let is_addressbook = props
        .iter()
        .any(|p| p.is(NS_DAV, "resourcetype") && p.child(NS_CARDDAV, "addressbook").is_some());
    if !calendar && !is_addressbook {
        // A plain MKCOL: the mock only creates address books.
        return dav_error(403, NS_DAV, "valid-resourcetype");
    }
    let mut components = Vec::new();
    let mut name = String::new();
    let mut color = None;
    for prop in &props {
        if prop.is(NS_DAV, "displayname") {
            prop.text.trim().clone_into(&mut name);
        } else if prop.is(NS_APPLE, "calendar-color") {
            color = Some(prop.text.trim().to_owned());
        } else if prop.is(NS_CALDAV, "supported-calendar-component-set") {
            components.extend(
                prop.children_named(NS_CALDAV, "comp")
                    .filter_map(|c| c.attr("name"))
                    .map(str::to_owned),
            );
        }
    }
    if components.is_empty() {
        components = vec!["VEVENT".to_owned(), "VTODO".to_owned()];
    }
    let kind = if calendar {
        MockCollectionKind::Calendar(components)
    } else {
        MockCollectionKind::AddressBook
    };
    let seq = data.next();
    let mut coll = MockCollection::new(kind, &name, seq);
    coll.color = color;
    data.collections.insert(path, coll);
    empty(201)
}

fn proppatch(inner: &Inner, request: &DavRequest) -> Response {
    let root = match parse_body(&request.body) {
        Ok(Some(root)) => root,
        Ok(None) => return empty(400),
        Err(response) => return *response,
    };
    let mut data = lock(&inner.data);
    let Some(Resource::Collection(path)) = resolve(inner, &data, &request.path) else {
        return empty(404);
    };
    let seq = data.next();
    let Some(coll) = data.collections.get_mut(&path) else {
        return empty(404);
    };
    if coll.read_only {
        return dav_error(403, NS_DAV, "need-privileges");
    }
    let mut ok = String::new();
    let mut forbidden = String::new();
    for prop in set_props(Some(&root)) {
        if prop.is(NS_DAV, "displayname") {
            prop.text.trim().clone_into(&mut coll.display_name);
            ok.push_str(&element(&prop.ns, &prop.name, ""));
        } else if prop.is(NS_APPLE, "calendar-color") {
            coll.color = Some(prop.text.trim().to_owned());
            ok.push_str(&element(&prop.ns, &prop.name, ""));
        } else {
            forbidden.push_str(&element(&prop.ns, &prop.name, ""));
        }
    }
    coll.changed = seq;
    let mut body = format!("<d:response>{}", href(&path));
    if !ok.is_empty() {
        let _ = write!(
            body,
            "<d:propstat><d:prop>{ok}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>"
        );
    }
    if !forbidden.is_empty() {
        let _ = write!(
            body,
            "<d:propstat><d:prop>{forbidden}</d:prop><d:status>HTTP/1.1 403 Forbidden</d:status></d:propstat>"
        );
    }
    body.push_str("</d:response>");
    multistatus(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const AUTH: &str = "Authorization: Basic YWRhOnNlY3JldA==\r\n";

    async fn raw(addr: SocketAddr, head: &str, body: &str) -> String {
        let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let request = format!(
            "{head}Host: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        tcp.write_all(request.as_bytes()).await.unwrap();
        let mut out = String::new();
        tcp.read_to_string(&mut out).await.unwrap();
        out
    }

    fn status(response: &str) -> u16 {
        response[9..12].parse().unwrap()
    }

    #[tokio::test]
    async fn well_known_options_and_auth() {
        let server = MockDavServer::start(MockDavConfig::default())
            .await
            .unwrap();
        assert!(server.addr().ip().is_loopback());
        let redirect = raw(server.addr(), "GET /.well-known/caldav HTTP/1.1\r\n", "").await;
        assert_eq!(status(&redirect), 301);
        assert!(redirect.to_ascii_lowercase().contains("location: /dav/"));
        let options = raw(server.addr(), "OPTIONS /dav/ HTTP/1.1\r\n", "").await;
        assert!(options.to_ascii_lowercase().contains("calendar-access"));
        let anonymous = raw(server.addr(), "PROPFIND /dav/ HTTP/1.1\r\nDepth: 0\r\n", "").await;
        assert_eq!(status(&anonymous), 401);
        assert!(
            anonymous
                .to_ascii_lowercase()
                .contains("www-authenticate: basic")
        );
        let infinite = raw(
            server.addr(),
            &format!("PROPFIND /dav/ HTTP/1.1\r\n{AUTH}"),
            "",
        )
        .await;
        assert_eq!(status(&infinite), 403);
        let ok = raw(
            server.addr(),
            &format!("PROPFIND /dav/ HTTP/1.1\r\nDepth: 0\r\n{AUTH}"),
            "<d:propfind xmlns:d=\"DAV:\"><d:prop><d:current-user-principal/><d:x-unknown/></d:prop></d:propfind>",
        )
        .await;
        assert_eq!(status(&ok), 207);
        assert!(ok.contains("<d:href>/dav/principals/ada/</d:href>"));
        assert!(ok.contains("404 Not Found"), "unknown properties: {ok}");
        let bad = raw(
            server.addr(),
            &format!("PROPFIND /dav/ HTTP/1.1\r\nDepth: 0\r\n{AUTH}"),
            "<not-closed>",
        )
        .await;
        assert_eq!(status(&bad), 400);
        assert_eq!(server.requests().len(), 6);
        assert_eq!(server.requests()[2].depth.as_deref(), Some("0"));
    }

    #[tokio::test]
    async fn conditional_put_and_delete() {
        let server = MockDavServer::start(MockDavConfig::default())
            .await
            .unwrap();
        let personal = server.calendar_path("personal");
        let path = format!("{personal}a.ics");
        let ics = "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n";
        let put = async |extra: &str| {
            raw(
                server.addr(),
                &format!("PUT {path} HTTP/1.1\r\nContent-Type: text/calendar\r\n{extra}{AUTH}"),
                ics,
            )
            .await
        };
        let created = put("If-None-Match: *\r\n").await;
        assert_eq!(status(&created), 201);
        let etag = server.object(&personal, "a.ics").unwrap().etag;
        assert!(created.contains(&etag));
        assert_eq!(status(&put("If-None-Match: *\r\n").await), 412);
        assert_eq!(status(&put("If-Match: \"nope\"\r\n").await), 412);
        let wrong_type = raw(
            server.addr(),
            &format!("PUT {path} HTTP/1.1\r\nContent-Type: text/vcard\r\n{AUTH}"),
            "BEGIN:VCARD\r\nEND:VCARD\r\n",
        )
        .await;
        assert_eq!(status(&wrong_type), 403);
        let stale = raw(
            server.addr(),
            &format!("DELETE {path} HTTP/1.1\r\nIf-Match: \"old\"\r\n{AUTH}"),
            "",
        )
        .await;
        assert_eq!(status(&stale), 412);
        let deleted = raw(
            server.addr(),
            &format!("DELETE {path} HTTP/1.1\r\nIf-Match: {etag}\r\n{AUTH}"),
            "",
        )
        .await;
        assert_eq!(status(&deleted), 204);
        let gone = raw(server.addr(), &format!("GET {path} HTTP/1.1\r\n{AUTH}"), "").await;
        assert_eq!(status(&gone), 404);
        let readonly = format!("{}x.ics", server.calendar_path("holidays"));
        let refused = raw(
            server.addr(),
            &format!("PUT {readonly} HTTP/1.1\r\nContent-Type: text/calendar\r\n{AUTH}"),
            ics,
        )
        .await;
        assert_eq!(status(&refused), 403);
    }

    #[tokio::test]
    async fn sync_tokens_and_fault_injection() {
        let server = MockDavServer::start(MockDavConfig {
            seed: false,
            ..MockDavConfig::default()
        })
        .await
        .unwrap();
        assert!(server.collection_paths().is_empty());
        let book = server.address_book_path("b");
        server.add_address_book(&book, "B");
        server.put_object(&book, "1.vcf", "BEGIN:VCARD\r\nEND:VCARD\r\n");
        let sync = async |token: &str| {
            raw(
                server.addr(),
                &format!("REPORT {book} HTTP/1.1\r\nDepth: 0\r\n{AUTH}"),
                &format!(
                    "<d:sync-collection xmlns:d=\"DAV:\"><d:sync-token>{token}</d:sync-token>\
                     <d:sync-level>1</d:sync-level><d:prop><d:getetag/></d:prop></d:sync-collection>"
                ),
            )
            .await
        };
        let first = sync("").await;
        assert_eq!(status(&first), 207);
        assert!(first.contains("1.vcf"));
        let token = first
            .split("<d:sync-token>")
            .nth(1)
            .unwrap()
            .split('<')
            .next()
            .unwrap()
            .to_owned();
        server.delete_object(&book, "1.vcf");
        let second = sync(&token).await;
        assert!(
            second.contains("1.vcf</d:href><d:status>HTTP/1.1 404"),
            "{second}"
        );
        server.forget_sync_history();
        let refused = sync(&token).await;
        assert_eq!(status(&refused), 403);
        assert!(refused.contains("valid-sync-token"));
        assert_eq!(status(&sync("garbage").await), 403);

        server.fail_next("report", 503);
        let failed = sync("").await;
        assert_eq!(status(&failed), 503);
        assert!(failed.to_ascii_lowercase().contains("retry-after: 1"));
        assert_eq!(status(&sync("").await), 207);
        server.clear_requests();
        assert!(server.requests().is_empty());
    }

    #[test]
    fn filters() {
        let event = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Europe/Berlin:20261005T100000\r\n\
                     DTEND:20261005T110000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert_eq!(
            ical_value(event, "VEVENT", "dtstart").as_deref(),
            Some("20261005T100000")
        );
        assert_eq!(ical_value(event, "VTODO", "DTSTART"), None);
        assert_eq!(
            normalize_time("20261005").as_deref(),
            Some("20261005T000000Z")
        );
        assert_eq!(normalize_time("2026"), None);
        let filter = |start: &str, end: &str, comp: &str| {
            xml::parse(&format!(
                "<c:filter xmlns:c=\"{NS_CALDAV}\"><c:comp-filter name=\"VCALENDAR\">\
                 <c:comp-filter name=\"{comp}\"><c:time-range start=\"{start}\" end=\"{end}\"/>\
                 </c:comp-filter></c:comp-filter></c:filter>"
            ))
            .unwrap()
        };
        let inside = filter("20261001T000000Z", "20261101T000000Z", "VEVENT");
        assert!(calendar_filter_matches(Some(&inside), event));
        let after = filter("20261006T000000Z", "20261101T000000Z", "VEVENT");
        assert!(!calendar_filter_matches(Some(&after), event));
        let todo = filter("20261001T000000Z", "20261101T000000Z", "VTODO");
        assert!(!calendar_filter_matches(Some(&todo), event));
        assert!(calendar_filter_matches(None, event));

        let card = "BEGIN:VCARD\r\nFN:Ada Lovelace\r\nitem1.EMAIL;TYPE=work:ada@example.org\r\nEND:VCARD\r\n";
        let prop = |test: &str, filters: &str| {
            xml::parse(&format!(
                "<c:filter xmlns:c=\"{NS_CARDDAV}\" test=\"{test}\">{filters}</c:filter>"
            ))
            .unwrap()
        };
        let fn_ada = "<c:prop-filter name=\"FN\"><c:text-match>ADA</c:text-match></c:prop-filter>";
        let email_bob =
            "<c:prop-filter name=\"EMAIL\"><c:text-match>bob</c:text-match></c:prop-filter>";
        let email_ada =
            "<c:prop-filter name=\"EMAIL\"><c:text-match>ada@</c:text-match></c:prop-filter>";
        assert!(card_filter_matches(Some(&prop("anyof", fn_ada)), card));
        let either = format!("{fn_ada}{email_bob}");
        assert!(card_filter_matches(Some(&prop("anyof", &either)), card));
        assert!(!card_filter_matches(Some(&prop("allof", &either)), card));
        let both = format!("{fn_ada}{email_ada}");
        assert!(card_filter_matches(Some(&prop("allof", &both)), card));
        assert!(card_filter_matches(Some(&prop("anyof", "")), card));
        assert_eq!(
            split_member("/a/b/c.ics"),
            Some(("/a/b/".to_owned(), "c.ics".to_owned()))
        );
        assert_eq!(split_member("/a/b/"), None);
    }

    // -----------------------------------------------------------------------
    // Scheduling (WP83)

    const BOB: &str = "Authorization: Basic Ym9iOnNlY3JldA==\r\n";

    fn freebusy_request(organizer: &str, attendees: &[&str]) -> String {
        let mut out = format!(
            "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VFREEBUSY\r\nUID:q\r\n\
             DTSTART:20261005T000000Z\r\nDTEND:20261006T000000Z\r\nORGANIZER:mailto:{organizer}\r\n"
        );
        for a in attendees {
            write!(out, "ATTENDEE:mailto:{a}\r\n").unwrap();
        }
        out + "END:VFREEBUSY\r\nEND:VCALENDAR\r\n"
    }

    async fn scheduling_server(auto: bool) -> MockDavServer {
        MockDavServer::start(MockDavConfig {
            auto_schedule: auto,
            users: vec![MockDavUser::new("bob")],
            ..MockDavConfig::default()
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn principals_inbox_and_outbox() {
        let server = scheduling_server(false).await;
        server.add_user(MockDavUser::room("room-1", "Room 1"));
        assert_eq!(
            server.collection_paths(),
            [
                "/dav/addressbooks/ada/contacts/",
                "/dav/calendars/ada/holidays/",
                "/dav/calendars/ada/personal/",
                "/dav/calendars/ada/tasks/",
                "/dav/calendars/bob/personal/",
                "/dav/calendars/room-1/personal/",
            ],
            "inboxes and outboxes are not listed"
        );
        let options = raw(server.addr(), "OPTIONS /dav/ HTTP/1.1\r\n", "").await;
        let lower = options.to_ascii_lowercase();
        assert!(lower.contains("calendar-schedule"));
        assert!(!lower.contains("calendar-auto-schedule"));
        server.set_auto_schedule(true);
        let options = raw(server.addr(), "OPTIONS /dav/ HTTP/1.1\r\n", "").await;
        assert!(
            options
                .to_ascii_lowercase()
                .contains("calendar-auto-schedule")
        );

        // Bob's login sees Bob as the current principal.
        let props = "<d:propfind xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\"><d:prop>\
                     <d:current-user-principal/><c:schedule-inbox-URL/><c:schedule-outbox-URL/>\
                     <c:calendar-user-address-set/><c:calendar-user-type/></d:prop></d:propfind>";
        let bob = raw(
            server.addr(),
            &format!("PROPFIND /dav/principals/bob/ HTTP/1.1\r\nDepth: 0\r\n{BOB}"),
            props,
        )
        .await;
        assert_eq!(status(&bob), 207);
        for part in [
            "<d:current-user-principal><d:href>/dav/principals/bob/</d:href>",
            "<c:schedule-inbox-URL><d:href>/dav/calendars/bob/inbox/</d:href>",
            "<c:schedule-outbox-URL><d:href>/dav/calendars/bob/outbox/</d:href>",
            "<d:href>mailto:bob@example.org</d:href>",
            "<c:calendar-user-type>INDIVIDUAL</c:calendar-user-type>",
        ] {
            assert!(bob.contains(part), "{part} in {bob}");
        }
        let room = raw(
            server.addr(),
            &format!("PROPFIND /dav/principals/room-1/ HTTP/1.1\r\nDepth: 0\r\n{AUTH}"),
            props,
        )
        .await;
        assert!(room.contains("<c:calendar-user-type>ROOM</c:calendar-user-type>"));
        assert!(room.contains("<d:current-user-principal><d:href>/dav/principals/ada/</d:href>"));

        // The home lists the inbox and outbox with their resource types;
        // the inbox names the default calendar.
        let home = raw(
            server.addr(),
            &format!("PROPFIND /dav/calendars/bob/ HTTP/1.1\r\nDepth: 1\r\n{BOB}"),
            "<d:propfind xmlns:d=\"DAV:\"><d:prop><d:resourcetype/></d:prop></d:propfind>",
        )
        .await;
        assert!(home.contains("<c:schedule-inbox/>") && home.contains("<c:schedule-outbox/>"));
        let inbox = raw(
            server.addr(),
            &format!("PROPFIND /dav/calendars/bob/inbox/ HTTP/1.1\r\nDepth: 0\r\n{BOB}"),
            "<d:propfind xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\"><d:prop>\
             <c:schedule-default-calendar-URL/></d:prop></d:propfind>",
        )
        .await;
        assert!(inbox.contains("<d:href>/dav/calendars/bob/personal/</d:href>"));

        // Inboxes are read-only, the outbox is not a collection to delete.
        let put = raw(
            server.addr(),
            &format!(
                "PUT /dav/calendars/bob/inbox/x.ics HTTP/1.1\r\nContent-Type: text/calendar\r\n{BOB}"
            ),
            "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n",
        )
        .await;
        assert_eq!(status(&put), 403);
        let delete = raw(
            server.addr(),
            &format!("DELETE /dav/calendars/bob/outbox/ HTTP/1.1\r\n{BOB}"),
            "",
        )
        .await;
        assert_eq!(status(&delete), 403);
        assert!(
            server
                .deliver("bob", "i.ics", "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n")
                .is_some()
        );
        assert_eq!(server.inbox("bob").len(), 1);
        assert!(server.deliver("nobody", "i.ics", "x").is_none());
    }

    #[tokio::test]
    async fn free_busy_posts() {
        let server = scheduling_server(false).await;
        let bob_personal = server.user_calendar_path("bob", "personal");
        server.put_object(
            &bob_personal,
            "a.ics",
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261005T090000Z\r\nDURATION:PT1H\r\n\
             END:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        let post = async |head: &str, body: &str| {
            raw(
                server.addr(),
                &format!("POST {head} HTTP/1.1\r\nContent-Type: text/calendar\r\n{AUTH}"),
                body,
            )
            .await
        };
        let ok = post(
            "/dav/calendars/ada/outbox/",
            &freebusy_request("ada@example.org", &["bob@example.org", "zed@example.org"]),
        )
        .await;
        assert_eq!(status(&ok), 200);
        assert!(ok.contains("<c:schedule-response"));
        assert!(ok.contains("FREEBUSY;FBTYPE=BUSY:20261005T090000Z/20261005T100000Z"));
        assert!(ok.contains("3.7;Invalid calendar user"));
        server.set_free_busy_visible("bob", false);
        let hidden = post(
            "/dav/calendars/ada/outbox/",
            &freebusy_request("ada@example.org", &["bob@example.org"]),
        )
        .await;
        assert!(hidden.contains("3.8;No authority"));
        assert!(!hidden.contains("FREEBUSY"));
        server.set_free_busy_visible("bob", true);

        let wrong_organizer = post(
            "/dav/calendars/ada/outbox/",
            &freebusy_request("bob@example.org", &["bob@example.org"]),
        )
        .await;
        assert_eq!(status(&wrong_organizer), 403);
        assert!(wrong_organizer.contains("organizer-allowed"));
        let not_mine = post(
            "/dav/calendars/bob/outbox/",
            &freebusy_request("ada@example.org", &["bob@example.org"]),
        )
        .await;
        assert_eq!(status(&not_mine), 403);
        let not_freebusy = post(
            "/dav/calendars/ada/outbox/",
            "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n",
        )
        .await;
        assert_eq!(status(&not_freebusy), 400);
        assert_eq!(
            status(&post("/dav/calendars/ada/personal/", "x").await),
            405
        );
        assert_eq!(status(&post("/dav/nowhere/", "x").await), 404);
        let xml = raw(
            server.addr(),
            &format!("POST /dav/calendars/ada/outbox/ HTTP/1.1\r\nContent-Type: application/xml\r\n{AUTH}"),
            "<x/>",
        )
        .await;
        assert_eq!(status(&xml), 415);
    }

    #[tokio::test]
    async fn auto_scheduling_honours_schedule_reply() {
        let server = scheduling_server(true).await;
        let invite = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:m\r\nDTSTART:20261005T090000Z\r\nDTEND:20261005T100000Z\r\n\
                      ORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let created = raw(
            server.addr(),
            &format!(
                "PUT /dav/calendars/ada/personal/m.ics HTTP/1.1\r\nContent-Type: text/calendar\r\n{AUTH}"
            ),
            invite,
        )
        .await;
        assert_eq!(status(&created), 201);
        assert!(
            !created.to_ascii_lowercase().contains("etag:"),
            "the stored object was changed: {created}"
        );
        assert_eq!(server.inbox("bob").len(), 1);
        let copy = server
            .object(&server.user_calendar_path("bob", "personal"), "m.ics")
            .unwrap();
        // Bob accepts without a reply (Schedule-Reply: F).
        let accepted = copy.data.replace(
            "ATTENDEE:mailto:bob",
            "ATTENDEE;PARTSTAT=ACCEPTED:mailto:bob",
        );
        let quiet = raw(
            server.addr(),
            &format!(
                "PUT /dav/calendars/bob/personal/m.ics HTTP/1.1\r\nContent-Type: text/calendar\r\n\
                 Schedule-Reply: F\r\n{BOB}"
            ),
            &accepted,
        )
        .await;
        assert_eq!(status(&quiet), 204);
        assert!(quiet.to_ascii_lowercase().contains("etag:"));
        assert!(server.inbox("ada").is_empty());
        assert_eq!(server.scheduled_messages().len(), 1);
        assert_eq!(
            server.requests().last().unwrap().schedule_reply.as_deref(),
            Some("F")
        );
        // Unchanged PARTSTAT: no reply either.
        let same = raw(
            server.addr(),
            &format!(
                "PUT /dav/calendars/bob/personal/m.ics HTTP/1.1\r\nContent-Type: text/calendar\r\n{BOB}"
            ),
            &accepted,
        )
        .await;
        assert_eq!(status(&same), 204);
        assert!(server.inbox("ada").is_empty());
        // Taking Bob off the meeting cancels it for him.
        let without_bob = invite.replace(
            "ATTENDEE:mailto:bob@example.org\r\n",
            "ATTENDEE:mailto:ext@x.example\r\n",
        );
        raw(
            server.addr(),
            &format!(
                "PUT /dav/calendars/ada/personal/m.ics HTTP/1.1\r\nContent-Type: text/calendar\r\n{AUTH}"
            ),
            &without_bob,
        )
        .await;
        let methods: Vec<(String, String)> = server
            .scheduled_messages()
            .into_iter()
            .map(|m| (m.method, m.to))
            .collect();
        assert_eq!(
            methods,
            [
                ("REQUEST".into(), "bob@example.org".into()),
                ("REQUEST".into(), "ext@x.example".into()),
                ("CANCEL".into(), "bob@example.org".into()),
            ]
        );
        let cancelled = server
            .object(&server.user_calendar_path("bob", "personal"), "m.ics")
            .unwrap();
        assert!(cancelled.data.contains("STATUS:CANCELLED"));
        assert!(
            cancelled.data.contains("PARTSTAT=ACCEPTED"),
            "Bob's answer is kept"
        );
    }

    #[tokio::test]
    async fn without_scheduling_there_is_no_inbox_or_outbox() {
        let server = MockDavServer::start(MockDavConfig {
            scheduling: false,
            auto_schedule: true,
            ..MockDavConfig::default()
        })
        .await
        .unwrap();
        assert!(!server.auto_schedule());
        server.set_auto_schedule(true);
        assert!(!server.auto_schedule(), "auto-scheduling needs scheduling");
        let options = raw(server.addr(), "OPTIONS /dav/ HTTP/1.1\r\n", "").await;
        assert!(!options.to_ascii_lowercase().contains("calendar-schedule"));
        let post = raw(
            server.addr(),
            &format!(
                "POST /dav/calendars/ada/outbox/ HTTP/1.1\r\nContent-Type: text/calendar\r\n{AUTH}"
            ),
            &freebusy_request("ada@example.org", &["ada@example.org"]),
        )
        .await;
        assert_eq!(status(&post), 404);
        assert!(server.inbox("ada").is_empty());
    }
}
