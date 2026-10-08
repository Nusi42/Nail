//! The WebDAV / CalDAV / CardDAV operations: PROPFIND, REPORT
//! (`calendar-query`, `calendar-multiget`, `addressbook-query`,
//! `addressbook-multiget`, `sync-collection`), GET, conditional PUT /
//! DELETE, MKCALENDAR, extended MKCOL, PROPPATCH.

use std::fmt::Write as _;

use nail_api::pim::calendar::CalendarComponent;
use nail_api::pim::source::DiscoveredCollection;
use nail_net::status as http_status;
use nail_util::percent::decode as percent_decode;
use reqwest::Method;
use reqwest::header::{HeaderName, IF_MATCH, IF_NONE_MATCH};
use url::Url;

use crate::client::{DavClient, Depth, Reply};
use crate::error::DavError;
use crate::multistatus::{self, MsResponse, Multistatus};
use crate::xml::escape;
use crate::{NS_APPLE_ICAL, NS_CALDAV, NS_CALENDARSERVER, NS_CARDDAV, NS_DAV};

const XML: &str = "application/xml; charset=utf-8";
/// Most hrefs per multiget request.
pub const MULTIGET_BATCH: usize = 50;

/// What a collection holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionKind {
    /// A CalDAV calendar (events and / or tasks).
    Calendar,
    /// A CardDAV address book.
    AddressBook,
}

impl CollectionKind {
    /// The media type of members.
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Calendar => "text/calendar; charset=utf-8",
            Self::AddressBook => "text/vcard; charset=utf-8",
        }
    }

    /// The file extension of members.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Calendar => "ics",
            Self::AddressBook => "vcf",
        }
    }

    fn data_prop(self) -> (&'static str, &'static str) {
        match self {
            Self::Calendar => (NS_CALDAV, "calendar-data"),
            Self::AddressBook => (NS_CARDDAV, "address-data"),
        }
    }

    fn data_element(self) -> &'static str {
        match self {
            Self::Calendar => "<c:calendar-data/>",
            Self::AddressBook => "<card:address-data/>",
        }
    }
}

/// A calendar or address book on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DavCollection {
    /// Its URL.
    pub url: Url,
    /// Calendar or address book.
    pub kind: CollectionKind,
    /// `displayname`.
    pub display_name: Option<String>,
    /// `calendar-description` / `addressbook-description`.
    pub description: Option<String>,
    /// Apple `calendar-color` (`#rrggbb` or `#rrggbbaa`).
    pub color: Option<String>,
    /// Components of a calendar (all when the server does not say).
    pub components: Vec<CalendarComponent>,
    /// The login lacks write privileges.
    pub read_only: bool,
    /// CalendarServer `getctag`.
    pub ctag: Option<String>,
    /// RFC 6578 `sync-token`.
    pub sync_token: Option<String>,
    /// `sync-collection` is in the `supported-report-set` (or the server
    /// returned a sync token).
    pub supports_sync: bool,
}

impl DavCollection {
    /// The display name, else the last path segment.
    pub fn name(&self) -> String {
        if let Some(name) = self
            .display_name
            .as_deref()
            .filter(|n| !n.trim().is_empty())
        {
            return name.trim().to_owned();
        }
        self.url
            .path_segments()
            .and_then(|mut s| s.rfind(|s| !s.is_empty()))
            .map(percent_decode)
            .unwrap_or_default()
    }

    /// The contract shape ([`nail_api::pim::source::PimSourceTestResult`]).
    pub fn to_discovered(&self) -> DiscoveredCollection {
        DiscoveredCollection {
            url: self.url.to_string(),
            name: self.name(),
            components: match self.kind {
                CollectionKind::Calendar => self.components.clone(),
                CollectionKind::AddressBook => Vec::new(),
            },
            address_book: self.kind == CollectionKind::AddressBook,
            read_only: self.read_only,
            color: self.color.clone().filter(|c| nail_api::pim::is_color(c)),
        }
    }
}

/// A calendar object resource or vCard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DavObject {
    /// Its key: the URL's path (percent-encoded, as the server names it).
    pub href: String,
    /// Its URL.
    pub url: Url,
    /// Its ETag (quoted, as sent).
    pub etag: Option<String>,
    /// Its iCalendar / vCard text, when fetched.
    pub data: Option<String>,
}

/// A UTC time range for `calendar-query` (`YYYYMMDDTHHMMSSZ`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeRange {
    start: String,
    end: String,
}

fn is_utc_stamp(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 16
        && b[8] == b'T'
        && b[15] == b'Z'
        && b[..8].iter().chain(&b[9..15]).all(u8::is_ascii_digit)
}

impl TimeRange {
    /// A range from iCalendar UTC stamps; `None` when malformed or empty.
    pub fn new(start: &str, end: &str) -> Option<Self> {
        (is_utc_stamp(start) && is_utc_stamp(end) && start < end).then(|| Self {
            start: start.to_owned(),
            end: end.to_owned(),
        })
    }

    /// A range from Unix seconds; `None` when empty or before 1970.
    pub fn from_unix(start: i64, end: i64) -> Option<Self> {
        if start < 0 || end <= start {
            return None;
        }
        Self::new(&utc_stamp(start), &utc_stamp(end))
    }

    /// The start stamp.
    pub fn start(&self) -> &str {
        &self.start
    }

    /// The end stamp.
    pub fn end(&self) -> &str {
        &self.end
    }
}

/// Unix seconds → `YYYYMMDDTHHMMSSZ` (proleptic Gregorian).
pub use nail_util::ical_text::utc_stamp;
/// An `addressbook-query` filter: the property contains the text
/// (case-insensitive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMatch {
    /// vCard property (`FN`, `EMAIL`, …).
    pub property: String,
    /// The text.
    pub text: String,
}

/// The result of a multiget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Multiget {
    /// Objects found, with data.
    pub objects: Vec<DavObject>,
    /// Requested hrefs the server reported missing.
    pub missing: Vec<String>,
}

/// The result of one `sync-collection` request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// The new token.
    pub token: Option<String>,
    /// Added or changed members (href, ETag).
    pub changed: Vec<(String, Option<String>)>,
    /// Removed members.
    pub removed: Vec<String>,
    /// The server stopped early (`507`); ask again with `token`.
    pub truncated: bool,
}

/// The condition of a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Precondition {
    /// Unconditional.
    None,
    /// `If-None-Match: *` — create only.
    Create,
    /// `If-Match: <etag>` — only if unchanged.
    IfMatch(String),
}

/// A new calendar or address book.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewCollection {
    /// `displayname`.
    pub display_name: String,
    /// Description.
    pub description: Option<String>,
    /// Calendar colour.
    pub color: Option<String>,
    /// Calendar components (empty = server default).
    pub components: Vec<CalendarComponent>,
}

/// The key of a URL: its path.
pub fn href_key(url: &Url) -> String {
    url.path().to_owned()
}

/// The member URL for an item `uid` in `collection`: `<uid>.ics` /
/// `<uid>.vcf`, with every byte outside `A-Z a-z 0-9 - _ . ~ @`
/// percent-encoded (so a `/` in a UID cannot leave the collection).
pub fn member_url(collection: &Url, uid: &str, kind: CollectionKind) -> Url {
    let mut name = String::new();
    for byte in uid.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'@') {
            name.push(char::from(byte));
        } else {
            let _ = write!(name, "%{byte:02X}");
        }
    }
    if name.is_empty() || name.chars().all(|c| c == '.') {
        name = format!("item{}", name.len());
    }
    let mut base = collection.clone();
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    let mut url = base.clone();
    url.set_path(&format!("{}{name}.{}", base.path(), kind.extension()));
    url
}

fn prop_list(props: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (ns, name) in props {
        let prefix = match *ns {
            NS_DAV => "d",
            NS_CALDAV => "c",
            NS_CARDDAV => "card",
            NS_CALENDARSERVER => "cs",
            NS_APPLE_ICAL => "ic",
            _ => {
                let _ = write!(out, "<x:{name} xmlns:x=\"{}\"/>", escape(ns));
                continue;
            }
        };
        let _ = write!(out, "<{prefix}:{name}/>");
    }
    out
}

fn namespaces() -> String {
    format!(
        "xmlns:d=\"DAV:\" xmlns:c=\"{NS_CALDAV}\" xmlns:card=\"{NS_CARDDAV}\" \
         xmlns:cs=\"{NS_CALENDARSERVER}\" xmlns:ic=\"{NS_APPLE_ICAL}\""
    )
}

/// A PROPFIND body asking for `props`.
pub fn propfind_body(props: &[(&str, &str)]) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:propfind {}><d:prop>{}</d:prop></d:propfind>",
        namespaces(),
        prop_list(props)
    )
}

/// The properties read for collections.
pub const COLLECTION_PROPS: &[(&str, &str)] = &[
    (NS_DAV, "resourcetype"),
    (NS_DAV, "displayname"),
    (NS_DAV, "current-user-privilege-set"),
    (NS_DAV, "supported-report-set"),
    (NS_DAV, "sync-token"),
    (NS_CALENDARSERVER, "getctag"),
    (NS_CALDAV, "supported-calendar-component-set"),
    (NS_CALDAV, "calendar-description"),
    (NS_CARDDAV, "addressbook-description"),
    (NS_APPLE_ICAL, "calendar-color"),
];

fn is_collection_of(resp: &MsResponse) -> Option<CollectionKind> {
    let types = resp.prop(NS_DAV, "resourcetype")?;
    if types.child(NS_CALDAV, "calendar").is_some() {
        Some(CollectionKind::Calendar)
    } else if types.child(NS_CARDDAV, "addressbook").is_some() {
        Some(CollectionKind::AddressBook)
    } else {
        None
    }
}

/// Reads a collection from a PROPFIND response (`None` when the resource
/// is no calendar / address book, or a calendar without events or tasks).
pub fn collection_from(base: &Url, resp: &MsResponse) -> Option<DavCollection> {
    let kind = is_collection_of(resp)?;
    let url = resp.url(base)?;
    let components = match resp.prop(NS_CALDAV, "supported-calendar-component-set") {
        Some(set) => {
            let mut out: Vec<CalendarComponent> = set
                .children_named(NS_CALDAV, "comp")
                .filter_map(|c| match c.attr("name")?.to_ascii_uppercase().as_str() {
                    "VEVENT" => Some(CalendarComponent::Event),
                    "VTODO" => Some(CalendarComponent::Task),
                    _ => None,
                })
                .collect();
            out.sort();
            out.dedup();
            out
        }
        None => vec![CalendarComponent::Event, CalendarComponent::Task],
    };
    if kind == CollectionKind::Calendar && components.is_empty() {
        return None;
    }
    let read_only = resp
        .prop(NS_DAV, "current-user-privilege-set")
        .is_some_and(|set| {
            !set.children_named(NS_DAV, "privilege").any(|p| {
                ["write", "write-content", "all", "bind"]
                    .iter()
                    .any(|w| p.child(NS_DAV, w).is_some())
            })
        });
    let sync_token = resp.prop_text(NS_DAV, "sync-token");
    let supports_sync = sync_token.is_some()
        || resp
            .prop(NS_DAV, "supported-report-set")
            .is_some_and(|set| {
                set.children_named(NS_DAV, "supported-report").any(|r| {
                    r.child(NS_DAV, "report")
                        .is_some_and(|r| r.child(NS_DAV, "sync-collection").is_some())
                })
            });
    let description = match kind {
        CollectionKind::Calendar => resp.prop_text(NS_CALDAV, "calendar-description"),
        CollectionKind::AddressBook => resp.prop_text(NS_CARDDAV, "addressbook-description"),
    };
    Some(DavCollection {
        url,
        kind,
        display_name: resp.prop_text(NS_DAV, "displayname"),
        description,
        color: resp.prop_text(NS_APPLE_ICAL, "calendar-color"),
        components: match kind {
            CollectionKind::Calendar => components,
            CollectionKind::AddressBook => Vec::new(),
        },
        read_only,
        ctag: resp.prop_text(NS_CALENDARSERVER, "getctag"),
        sync_token,
        supports_sync,
    })
}

fn object_from(base: &Url, resp: &MsResponse, kind: CollectionKind) -> Option<DavObject> {
    let url = resp.url(base)?;
    let (ns, name) = kind.data_prop();
    Some(DavObject {
        href: href_key(&url),
        etag: resp.prop_text(NS_DAV, "getetag"),
        data: resp.prop(ns, name).map(|d| d.text.clone()),
        url,
    })
}

fn expect_multistatus(reply: &Reply, what: &str) -> Result<Multistatus, DavError> {
    if reply.status != 207 {
        return Err(reply.error(what));
    }
    multistatus::parse(&reply.text())
}

/// A WebDAV method `reqwest` has no constant for (`PROPFIND`, `REPORT`, ...).
#[expect(
    clippy::expect_used,
    reason = "only called with the constant method names of this module, all valid tokens (test `dav_methods_are_valid`)"
)]
fn dav_method(name: &'static [u8]) -> Method {
    Method::from_bytes(name).expect("a valid HTTP method token")
}

impl DavClient {
    /// `PROPFIND` with `props` at `depth`.
    pub async fn propfind(
        &self,
        url: &Url,
        depth: Depth,
        props: &[(&str, &str)],
    ) -> Result<(Url, Multistatus), DavError> {
        let reply = self
            .send(
                dav_method(b"PROPFIND"),
                url,
                Some(depth),
                &[],
                Some((XML, propfind_body(props).into_bytes())),
            )
            .await?;
        let ms = expect_multistatus(&reply, "PROPFIND")?;
        Ok((reply.url, ms))
    }

    /// `REPORT` with `body` at `depth`.
    pub async fn report(&self, url: &Url, depth: Depth, body: String) -> Result<Reply, DavError> {
        self.send(
            dav_method(b"REPORT"),
            url,
            Some(depth),
            &[],
            Some((XML, body.into_bytes())),
        )
        .await
    }

    /// The properties of one collection.
    pub async fn collection(&self, url: &Url) -> Result<DavCollection, DavError> {
        let (base, ms) = self.propfind(url, Depth::Zero, COLLECTION_PROPS).await?;
        ms.responses
            .iter()
            .find_map(|r| collection_from(&base, r))
            .ok_or_else(|| DavError::NotFound(format!("no calendar or address book at {url}")))
    }

    /// The calendars or address books in a home set (depth 1).
    pub async fn collections(
        &self,
        home: &Url,
        kind: CollectionKind,
    ) -> Result<Vec<DavCollection>, DavError> {
        let (base, ms) = self.propfind(home, Depth::One, COLLECTION_PROPS).await?;
        Ok(ms
            .responses
            .iter()
            .filter_map(|r| collection_from(&base, r))
            .filter(|c| c.kind == kind)
            .collect())
    }

    /// `getctag` and `sync-token` of a collection (depth 0).
    pub async fn ctag(&self, url: &Url) -> Result<(Option<String>, Option<String>), DavError> {
        let (_, ms) = self
            .propfind(
                url,
                Depth::Zero,
                &[(NS_CALENDARSERVER, "getctag"), (NS_DAV, "sync-token")],
            )
            .await?;
        let first = ms.responses.first();
        Ok((
            first.and_then(|r| r.prop_text(NS_CALENDARSERVER, "getctag")),
            first.and_then(|r| r.prop_text(NS_DAV, "sync-token")),
        ))
    }

    /// Every member's ETag (PROPFIND depth 1), for the ctag / ETag sync
    /// fallback.
    pub async fn list_etags(&self, collection: &Url) -> Result<Vec<(String, String)>, DavError> {
        let (base, ms) = self
            .propfind(
                collection,
                Depth::One,
                &[(NS_DAV, "getetag"), (NS_DAV, "resourcetype")],
            )
            .await?;
        let own = href_key(collection);
        Ok(ms
            .responses
            .iter()
            .filter_map(|r| {
                let url = r.url(&base)?;
                let key = href_key(&url);
                let is_collection = r
                    .prop(NS_DAV, "resourcetype")
                    .is_some_and(|t| t.child(NS_DAV, "collection").is_some());
                if key == own
                    || key.trim_end_matches('/') == own.trim_end_matches('/')
                    || is_collection
                {
                    return None;
                }
                Some((key, r.prop_text(NS_DAV, "getetag")?))
            })
            .collect())
    }

    /// `calendar-query` for `component` objects, optionally overlapping
    /// `range`, with their data when `with_data`.
    pub async fn calendar_query(
        &self,
        calendar: &Url,
        component: CalendarComponent,
        range: Option<&TimeRange>,
        with_data: bool,
    ) -> Result<Vec<DavObject>, DavError> {
        let comp = match component {
            CalendarComponent::Event => "VEVENT",
            CalendarComponent::Task => "VTODO",
        };
        let range = range
            .map(|r| format!("<c:time-range start=\"{}\" end=\"{}\"/>", r.start, r.end))
            .unwrap_or_default();
        let data = if with_data { "<c:calendar-data/>" } else { "" };
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<c:calendar-query {}><d:prop><d:getetag/>{data}</d:prop>\
             <c:filter><c:comp-filter name=\"VCALENDAR\"><c:comp-filter name=\"{comp}\">{range}\
             </c:comp-filter></c:comp-filter></c:filter></c:calendar-query>",
            namespaces()
        );
        let reply = self.report(calendar, Depth::One, body).await?;
        let ms = expect_multistatus(&reply, "calendar-query")?;
        Ok(ms
            .responses
            .iter()
            .filter(|r| !r.is_not_found())
            .filter_map(|r| object_from(&reply.url, r, CollectionKind::Calendar))
            .filter(|o| href_key(&o.url) != href_key(calendar))
            .collect())
    }

    /// `addressbook-query`: every vCard, or those matching `filter`.
    pub async fn addressbook_query(
        &self,
        book: &Url,
        filter: Option<&TextMatch>,
        with_data: bool,
    ) -> Result<Vec<DavObject>, DavError> {
        let filter = filter
            .map(|f| {
                format!(
                    "<card:filter><card:prop-filter name=\"{}\"><card:text-match \
                     collation=\"i;unicode-casemap\" match-type=\"contains\">{}</card:text-match>\
                     </card:prop-filter></card:filter>",
                    escape(&f.property),
                    escape(&f.text)
                )
            })
            .unwrap_or_default();
        let data = if with_data {
            "<card:address-data/>"
        } else {
            ""
        };
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<card:addressbook-query {}><d:prop><d:getetag/>{data}\
             </d:prop>{filter}</card:addressbook-query>",
            namespaces()
        );
        let reply = self.report(book, Depth::One, body).await?;
        let ms = expect_multistatus(&reply, "addressbook-query")?;
        Ok(ms
            .responses
            .iter()
            .filter(|r| !r.is_not_found())
            .filter_map(|r| object_from(&reply.url, r, CollectionKind::AddressBook))
            .filter(|o| href_key(&o.url) != href_key(book))
            .collect())
    }

    /// `calendar-multiget` / `addressbook-multiget` of `hrefs` (paths), in
    /// batches of [`MULTIGET_BATCH`].
    pub async fn multiget(
        &self,
        collection: &Url,
        kind: CollectionKind,
        hrefs: &[String],
    ) -> Result<Multiget, DavError> {
        let mut out = Multiget::default();
        for batch in hrefs.chunks(MULTIGET_BATCH) {
            let root = match kind {
                CollectionKind::Calendar => "c:calendar-multiget",
                CollectionKind::AddressBook => "card:addressbook-multiget",
            };
            let hrefs: String = batch.iter().fold(String::new(), |mut out, h| {
                let _ = write!(out, "<d:href>{}</d:href>", escape(h));
                out
            });
            let body = format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<{root} {}><d:prop><d:getetag/>{}</d:prop>{hrefs}</{root}>",
                namespaces(),
                kind.data_element()
            );
            let reply = self.report(collection, Depth::One, body).await?;
            let ms = expect_multistatus(&reply, "multiget")?;
            let mut seen = Vec::new();
            for resp in &ms.responses {
                let Some(object) = object_from(&reply.url, resp, kind) else {
                    continue;
                };
                seen.push(object.href.clone());
                if resp.is_not_found() || object.data.is_none() {
                    out.missing.push(object.href);
                } else {
                    out.objects.push(object);
                }
            }
            out.missing.extend(
                batch
                    .iter()
                    .filter(|h| {
                        let key = collection.join(h).map(|u| href_key(&u)).unwrap_or_default();
                        !seen.contains(&key)
                    })
                    .cloned(),
            );
        }
        Ok(out)
    }

    /// One `sync-collection` request (RFC 6578) from `token` (`None` = the
    /// initial sync), with an optional result `limit`.
    pub async fn sync_collection(
        &self,
        collection: &Url,
        token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<SyncReport, DavError> {
        let limit = limit
            .map(|n| format!("<d:limit><d:nresults>{n}</d:nresults></d:limit>"))
            .unwrap_or_default();
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:sync-collection {}><d:sync-token>{}</d:sync-token>\
             <d:sync-level>1</d:sync-level>{limit}<d:prop><d:getetag/></d:prop></d:sync-collection>",
            namespaces(),
            escape(token.unwrap_or_default())
        );
        let reply = self.report(collection, Depth::Zero, body).await?;
        if matches!(reply.status, 403 | http_status::CONFLICT)
            && multistatus::error_names(&reply.text(), NS_DAV, "valid-sync-token")
        {
            return Err(DavError::InvalidSyncToken);
        }
        let ms = expect_multistatus(&reply, "sync-collection")?;
        let own = href_key(collection);
        let mut out = SyncReport {
            token: ms.sync_token.clone(),
            ..SyncReport::default()
        };
        for resp in &ms.responses {
            let Some(url) = resp.url(&reply.url) else {
                continue;
            };
            let key = href_key(&url);
            if key.trim_end_matches('/') == own.trim_end_matches('/') {
                if resp.status == Some(507) {
                    out.truncated = true;
                }
                continue;
            }
            if resp.is_not_found() {
                out.removed.push(key);
            } else if key.ends_with('/') {
                // A member collection (sync-level 1 does not descend).
            } else {
                out.changed.push((key, resp.prop_text(NS_DAV, "getetag")));
            }
        }
        Ok(out)
    }

    /// `GET` one object.
    pub async fn get(&self, url: &Url) -> Result<DavObject, DavError> {
        let reply = self.send(Method::GET, url, None, &[], None).await?;
        if !reply.is_success() {
            return Err(reply.error(&format!("GET {}", url.path())));
        }
        Ok(DavObject {
            href: href_key(&reply.url),
            etag: reply.header("etag").map(str::to_owned),
            data: Some(reply.text()),
            url: reply.url,
        })
    }

    /// `PUT` with a precondition; returns the new ETag if the server sent
    /// one. `412` is [`DavError::PreconditionFailed`].
    pub async fn put(
        &self,
        url: &Url,
        kind: CollectionKind,
        data: &str,
        precondition: &Precondition,
    ) -> Result<Option<String>, DavError> {
        let headers = condition_headers(precondition);
        let reply = self
            .send(
                Method::PUT,
                url,
                None,
                &headers,
                Some((kind.content_type(), data.as_bytes().to_vec())),
            )
            .await?;
        if !reply.is_success() {
            return Err(reply.error(&format!("PUT {}", url.path())));
        }
        Ok(reply.header("etag").map(str::to_owned))
    }

    /// `DELETE` with a precondition. `404` is [`DavError::NotFound`],
    /// `412` [`DavError::PreconditionFailed`].
    pub async fn delete(&self, url: &Url, precondition: &Precondition) -> Result<(), DavError> {
        let headers = condition_headers(precondition);
        let reply = self.send(Method::DELETE, url, None, &headers, None).await?;
        if !reply.is_success() {
            return Err(reply.error(&format!("DELETE {}", url.path())));
        }
        Ok(())
    }

    /// `MKCALENDAR` (RFC 4791 §5.3.1).
    pub async fn mkcalendar(&self, url: &Url, new: &NewCollection) -> Result<(), DavError> {
        let mut props = set_props(new);
        if !new.components.is_empty() {
            props.push_str("<c:supported-calendar-component-set>");
            for component in &new.components {
                let name = match component {
                    CalendarComponent::Event => "VEVENT",
                    CalendarComponent::Task => "VTODO",
                };
                let _ = write!(props, "<c:comp name=\"{name}\"/>");
            }
            props.push_str("</c:supported-calendar-component-set>");
        }
        if let Some(description) = &new.description {
            let _ = write!(
                props,
                "<c:calendar-description>{}</c:calendar-description>",
                escape(description)
            );
        }
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<c:mkcalendar {}><d:set><d:prop>{props}</d:prop></d:set></c:mkcalendar>",
            namespaces()
        );
        let reply = self
            .send(
                dav_method(b"MKCALENDAR"),
                url,
                None,
                &[],
                Some((XML, body.into_bytes())),
            )
            .await?;
        if reply.status != 201 && !reply.is_success() {
            return Err(reply.error(&format!("MKCALENDAR {}", url.path())));
        }
        Ok(())
    }

    /// Extended `MKCOL` (RFC 5689) creating an address book.
    pub async fn mkaddressbook(&self, url: &Url, new: &NewCollection) -> Result<(), DavError> {
        let mut props =
            String::from("<d:resourcetype><d:collection/><card:addressbook/></d:resourcetype>");
        props.push_str(&set_props(new));
        if let Some(description) = &new.description {
            let _ = write!(
                props,
                "<card:addressbook-description>{}</card:addressbook-description>",
                escape(description)
            );
        }
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:mkcol {}><d:set><d:prop>{props}</d:prop></d:set></d:mkcol>",
            namespaces()
        );
        let reply = self
            .send(
                dav_method(b"MKCOL"),
                url,
                None,
                &[],
                Some((XML, body.into_bytes())),
            )
            .await?;
        if !reply.is_success() {
            return Err(reply.error(&format!("MKCOL {}", url.path())));
        }
        Ok(())
    }

    /// `PROPPATCH` of the display name and / or colour; every property
    /// must be accepted.
    pub async fn proppatch(
        &self,
        url: &Url,
        display_name: Option<&str>,
        color: Option<&str>,
    ) -> Result<(), DavError> {
        let mut props = String::new();
        if let Some(name) = display_name {
            let _ = write!(props, "<d:displayname>{}</d:displayname>", escape(name));
        }
        if let Some(color) = color {
            let _ = write!(
                props,
                "<ic:calendar-color>{}</ic:calendar-color>",
                escape(color)
            );
        }
        if props.is_empty() {
            return Ok(());
        }
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<d:propertyupdate {}><d:set><d:prop>{props}</d:prop></d:set></d:propertyupdate>",
            namespaces()
        );
        let reply = self
            .send(
                dav_method(b"PROPPATCH"),
                url,
                None,
                &[],
                Some((XML, body.into_bytes())),
            )
            .await?;
        let ms = expect_multistatus(&reply, "PROPPATCH")?;
        let refused = ms
            .responses
            .iter()
            .flat_map(|r| r.propstats.iter())
            .any(|(status, _)| !(200..300).contains(status));
        if refused {
            return Err(DavError::Forbidden("PROPPATCH".to_owned()));
        }
        Ok(())
    }
}

fn set_props(new: &NewCollection) -> String {
    let mut props = format!(
        "<d:displayname>{}</d:displayname>",
        escape(&new.display_name)
    );
    if let Some(color) = &new.color {
        let _ = write!(
            props,
            "<ic:calendar-color>{}</ic:calendar-color>",
            escape(color)
        );
    }
    props
}

fn condition_headers(precondition: &Precondition) -> Vec<(HeaderName, String)> {
    match precondition {
        Precondition::None => Vec::new(),
        Precondition::Create => vec![(IF_NONE_MATCH, "*".to_owned())],
        Precondition::IfMatch(etag) => vec![(IF_MATCH, etag.clone())],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    #[test]
    fn dav_methods_are_valid() {
        for name in ["PROPFIND", "REPORT", "MKCALENDAR", "MKCOL", "PROPPATCH"] {
            assert_eq!(dav_method(name.as_bytes()).as_str(), name);
        }
    }

    #[test]
    fn utc_stamps() {
        assert_eq!(utc_stamp(0), "19700101T000000Z");
        assert_eq!(utc_stamp(951_782_400), "20000229T000000Z");
        assert_eq!(utc_stamp(1_790_812_800 + 3661), "20261001T010101Z");
        assert_eq!(utc_stamp(4_102_444_799), "20991231T235959Z");
        let range = TimeRange::from_unix(0, 86_400).unwrap();
        assert_eq!(
            (range.start(), range.end()),
            ("19700101T000000Z", "19700102T000000Z")
        );
        assert!(TimeRange::from_unix(10, 10).is_none());
        assert!(TimeRange::from_unix(-1, 10).is_none());
        assert!(TimeRange::new("20260101T000000Z", "20250101T000000Z").is_none());
        assert!(TimeRange::new("2026-01-01", "20270101T000000Z").is_none());
        assert!(TimeRange::new("20260101T000000Z\"/><x", "20270101T000000Z").is_none());
    }

    #[test]
    fn member_urls_stay_in_the_collection() {
        let cal = Url::parse("https://dav.example.org/cal/work/").unwrap();
        assert_eq!(
            member_url(&cal, "abc-123@example.org", CollectionKind::Calendar).as_str(),
            "https://dav.example.org/cal/work/abc-123@example.org.ics"
        );
        assert_eq!(
            member_url(&cal, "../../x y/z", CollectionKind::AddressBook).as_str(),
            "https://dav.example.org/cal/work/..%2F..%2Fx%20y%2Fz.vcf"
        );
        let no_slash = Url::parse("https://dav.example.org/book").unwrap();
        assert_eq!(
            member_url(&no_slash, "", CollectionKind::AddressBook).as_str(),
            "https://dav.example.org/book/item0.vcf"
        );
        assert_eq!(
            member_url(&cal, "..", CollectionKind::Calendar).path(),
            "/cal/work/item2.ics"
        );
        assert_eq!(percent_decode("a%20b%2Fc%"), "a b/c%");
    }

    #[test]
    fn propfind_bodies_are_well_formed() {
        let body = propfind_body(COLLECTION_PROPS);
        let root = xml::parse(&body).unwrap();
        assert!(root.is(NS_DAV, "propfind"));
        let prop = root.child(NS_DAV, "prop").unwrap();
        assert_eq!(prop.children.len(), COLLECTION_PROPS.len());
        assert!(prop.child(NS_APPLE_ICAL, "calendar-color").is_some());
        let custom = propfind_body(&[("urn:x", "y")]);
        assert!(
            xml::parse(&custom)
                .unwrap()
                .path(&[(NS_DAV, "prop"), ("urn:x", "y")])
                .is_some()
        );
    }

    #[test]
    fn collections_from_responses() {
        let body = format!(
            r#"<d:multistatus xmlns:d="DAV:" xmlns:c="{NS_CALDAV}" xmlns:card="{NS_CARDDAV}" xmlns:ic="{NS_APPLE_ICAL}">
<d:response><d:href>/cal/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
<d:response><d:href>/cal/work/</d:href><d:propstat><d:prop>
  <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
  <d:displayname>Work</d:displayname>
  <ic:calendar-color>#FF0000FF</ic:calendar-color>
  <c:supported-calendar-component-set><c:comp name="VTODO"/><c:comp name="VEVENT"/><c:comp name="VJOURNAL"/></c:supported-calendar-component-set>
  <d:current-user-privilege-set><d:privilege><d:read/></d:privilege></d:current-user-privilege-set>
  <d:supported-report-set><d:supported-report><d:report><d:sync-collection/></d:report></d:supported-report></d:supported-report-set>
</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
<d:response><d:href>/cal/journal/</d:href><d:propstat><d:prop>
  <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
  <c:supported-calendar-component-set><c:comp name="VJOURNAL"/></c:supported-calendar-component-set>
</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
<d:response><d:href>/card/My%20Book/</d:href><d:propstat><d:prop>
  <d:resourcetype><d:collection/><card:addressbook/></d:resourcetype>
  <d:current-user-privilege-set><d:privilege><d:all/></d:privilege></d:current-user-privilege-set>
</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#
        );
        let ms = multistatus::parse(&body).unwrap();
        let base = Url::parse("https://dav.example.org/cal/").unwrap();
        let found: Vec<DavCollection> = ms
            .responses
            .iter()
            .filter_map(|r| collection_from(&base, r))
            .collect();
        assert_eq!(found.len(), 2, "plain collections and journals are skipped");
        let work = &found[0];
        assert_eq!(work.kind, CollectionKind::Calendar);
        assert_eq!(
            work.components,
            [CalendarComponent::Event, CalendarComponent::Task]
        );
        assert!(work.read_only && work.supports_sync);
        let shown = work.to_discovered();
        assert_eq!(shown.name, "Work");
        assert_eq!(shown.color.as_deref(), Some("#FF0000FF"));
        assert!(!shown.address_book);
        let book = &found[1];
        assert_eq!(book.kind, CollectionKind::AddressBook);
        assert!(!book.read_only && !book.supports_sync);
        let shown = book.to_discovered();
        assert_eq!(shown.name, "My Book", "falls back to the decoded path");
        assert!(shown.address_book && shown.components.is_empty());
    }

    #[test]
    fn condition_headers_per_precondition() {
        assert_eq!(condition_headers(&Precondition::None).len(), 0);
        assert_eq!(
            condition_headers(&Precondition::Create),
            [(IF_NONE_MATCH, "*".to_owned())]
        );
        assert_eq!(
            condition_headers(&Precondition::IfMatch("\"1\"".into())),
            [(IF_MATCH, "\"1\"".to_owned())]
        );
        assert_eq!(CollectionKind::AddressBook.extension(), "vcf");
        assert!(
            CollectionKind::Calendar
                .content_type()
                .starts_with("text/calendar")
        );
    }
}
