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

#[allow(dead_code)]
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
#[allow(clippy::struct_excessive_bools)]
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

    fn is_calendar(&self) -> bool {
        matches!(self.kind, MockCollectionKind::Calendar(_))
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
    /// The body as text.
    pub body: String,
}

#[derive(Debug)]
struct Data {
    seq: u64,
    collections: BTreeMap<String, MockCollection>,
    failures: VecDeque<(String, u16)>,
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

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Inner {
    fn principal(&self) -> String {
        format!("{PRINCIPAL_PATH}{}/", self.config.username)
    }
    fn calendar_home(&self) -> String {
        format!("{CALENDARS_PATH}{}/", self.config.username)
    }
    fn addressbook_home(&self) -> String {
        format!("{ADDRESSBOOKS_PATH}{}/", self.config.username)
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
        };
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
                let _ = axum::serve(listener, app)
                    .with_graceful_shutdown(stopped)
                    .await;
            });
            (task, Some(material.ca_der))
        } else {
            let task = tokio::spawn(async move {
                let _ = axum::serve(listener, app)
                    .with_graceful_shutdown(stopped)
                    .await;
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
            coll.display_name = display_name.to_owned();
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

    /// Paths of every collection.
    pub fn collection_paths(&self) -> Vec<String> {
        lock(&self.inner.data).collections.keys().cloned().collect()
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

fn authorized(config: &MockDavConfig, authorization: Option<&str>) -> bool {
    let Some(value) = authorization else {
        return false;
    };
    if let Some(token) = value.strip_prefix("Bearer ") {
        return config.bearer_token.as_deref() == Some(token.trim());
    }
    if let Some(encoded) = value.strip_prefix("Basic ") {
        let expected = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", config.username, config.password));
        return encoded.trim() == expected;
    }
    false
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
        let response = empty(200);
        let response = with_header(
            response,
            "dav",
            "1, 3, access-control, calendar-access, addressbook, extended-mkcol",
        );
        return with_header(
            response,
            "allow",
            "OPTIONS, GET, PUT, DELETE, PROPFIND, PROPPATCH, REPORT, MKCALENDAR, MKCOL",
        );
    }
    if !authorized(&inner.config, recorded.authorization.as_deref()) {
        return with_header(
            empty(401),
            "www-authenticate",
            "Basic realm=\"nail-mock-dav\", charset=\"UTF-8\"",
        );
    }
    match method.as_str() {
        "PROPFIND" => propfind(&inner, &recorded),
        "REPORT" => report(&inner, &recorded),
        "GET" | "HEAD" => get(&inner, &recorded),
        "PUT" => put(&inner, &recorded),
        "DELETE" => delete(&inner, &recorded),
        "MKCALENDAR" => mkcol(&inner, &recorded, true),
        "MKCOL" => mkcol(&inner, &recorded, false),
        "PROPPATCH" => proppatch(&inner, &recorded),
        _ => empty(405),
    }
}

/// What a path names.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Resource {
    Context,
    Principal,
    CalendarHome,
    AddressBookHome,
    Collection(String),
    Object(String, String),
}

fn resolve(inner: &Inner, data: &Data, path: &str) -> Option<Resource> {
    let with_slash = if path.ends_with('/') {
        path.to_owned()
    } else {
        format!("{path}/")
    };
    if with_slash == CONTEXT_PATH {
        return Some(Resource::Context);
    }
    if with_slash == inner.principal() {
        return Some(Resource::Principal);
    }
    if with_slash == inner.calendar_home() {
        return Some(Resource::CalendarHome);
    }
    if with_slash == inner.addressbook_home() {
        return Some(Resource::AddressBookHome);
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
    ns: &str,
    name: &str,
) -> Option<String> {
    let config = &inner.config;
    let collection = match resource {
        Resource::Collection(path) | Resource::Object(path, _) => data.collections.get(path),
        _ => None,
    };
    match (ns, name) {
        (NS_DAV, "current-user-principal") => Some(href(&inner.principal())),
        (NS_DAV, "resourcetype") => Some(match resource {
            Resource::Context | Resource::CalendarHome | Resource::AddressBookHome => {
                "<d:collection/>".to_owned()
            }
            Resource::Principal => "<d:collection/><d:principal/>".to_owned(),
            Resource::Collection(_) => match collection?.kind {
                MockCollectionKind::Calendar(_) => "<d:collection/><c:calendar/>".to_owned(),
                MockCollectionKind::AddressBook => "<d:collection/><card:addressbook/>".to_owned(),
            },
            Resource::Object(..) => String::new(),
        }),
        (NS_DAV, "displayname") => match resource {
            Resource::Principal => Some(escape(&config.username).into_owned()),
            Resource::Collection(_) => Some(escape(&collection?.display_name).into_owned()),
            _ => None,
        },
        (NS_DAV, "principal-URL") if *resource == Resource::Principal => {
            Some(href(&inner.principal()))
        }
        (NS_CALDAV, "calendar-home-set") if *resource == Resource::Principal => {
            Some(href(&inner.calendar_home()))
        }
        (NS_CARDDAV, "addressbook-home-set") if *resource == Resource::Principal => {
            Some(href(&inner.addressbook_home()))
        }
        (NS_CALDAV, "calendar-user-address-set") if *resource == Resource::Principal => {
            Some(href(&format!("mailto:{}", config.username)))
        }
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
        (NS_DAV, "supported-report-set") => {
            match resource {
                Resource::Collection(_) => {
                    let coll = collection?;
                    let mut reports = Vec::new();
                    if coll.is_calendar() {
                        reports.extend(["<c:calendar-query/>", "<c:calendar-multiget/>"]);
                    } else {
                        reports
                            .extend(["<card:addressbook-query/>", "<card:addressbook-multiget/>"]);
                    }
                    if config.sync_collection {
                        reports.push("<d:sync-collection/>");
                    }
                    Some(
                    reports
                        .iter()
                        .map(|r| format!("<d:supported-report><d:report>{r}</d:report></d:supported-report>"))
                        .collect(),
                )
                }
                _ => None,
            }
        }
        (NS_CALDAV, "supported-calendar-component-set") => match &collection?.kind {
            MockCollectionKind::Calendar(components)
                if matches!(resource, Resource::Collection(_)) =>
            {
                Some(
                    components
                        .iter()
                        .map(|c| format!("<c:comp name=\"{}\"/>", escape(c)))
                        .collect(),
                )
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

fn resource_path(inner: &Inner, resource: &Resource) -> String {
    match resource {
        Resource::Context => CONTEXT_PATH.to_owned(),
        Resource::Principal => inner.principal(),
        Resource::CalendarHome => inner.calendar_home(),
        Resource::AddressBookHome => inner.addressbook_home(),
        Resource::Collection(path) => path.clone(),
        Resource::Object(path, name) => format!("{path}{name}"),
    }
}

/// One `<d:response>` with the requested properties.
fn response_for(
    inner: &Inner,
    data: &Data,
    resource: &Resource,
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
        match prop_value(inner, data, resource, ns, name) {
            Some(value) => found.push_str(&element(ns, name, &value)),
            None if props.is_some() => missing.push_str(&element(ns, name, "")),
            None => {}
        }
    }
    let mut out = format!("<d:response>{}", href(&resource_path(inner, resource)));
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

fn propfind(inner: &Inner, request: &DavRequest) -> Response {
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
            Resource::CalendarHome => resources.extend(
                data.collections
                    .iter()
                    .filter(|(p, c)| c.is_calendar() && p.starts_with(&inner.calendar_home()))
                    .map(|(p, _)| Resource::Collection(p.clone())),
            ),
            Resource::AddressBookHome => resources.extend(
                data.collections
                    .iter()
                    .filter(|(p, c)| !c.is_calendar() && p.starts_with(&inner.addressbook_home()))
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
        .map(|r| response_for(inner, &data, r, props.as_deref()))
        .collect();
    multistatus(&body)
}

fn report(inner: &Inner, request: &DavRequest) -> Response {
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
            sync_collection(inner, &data, &path, coll, &root, props.as_deref())
        }
        _ => dav_error(403, NS_DAV, "supported-report"),
    }
}

fn sync_collection(
    inner: &Inner,
    data: &Data,
    path: &str,
    coll: &MockCollection,
    root: &Element,
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

fn put(inner: &Inner, request: &DavRequest) -> Response {
    let Some((parent, name)) = split_member(&request.path) else {
        return empty(405);
    };
    let mut data = lock(&inner.data);
    let Some(coll) = data.collections.get(&parent) else {
        return empty(409);
    };
    if coll.read_only {
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
    let Some(etag) = data.put(&parent, &name, &request.body) else {
        return empty(409);
    };
    let response = empty(if created { 201 } else { 204 });
    if inner.config.etag_on_put {
        with_header(response, "etag", &etag)
    } else {
        response
    }
}

fn delete(inner: &Inner, request: &DavRequest) -> Response {
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
            data.delete(&path, &name);
            empty(204)
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

fn mkcol(inner: &Inner, request: &DavRequest, calendar: bool) -> Response {
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
        inner.calendar_home()
    } else {
        inner.addressbook_home()
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
            name = prop.text.trim().to_owned();
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
            coll.display_name = prop.text.trim().to_owned();
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
}
