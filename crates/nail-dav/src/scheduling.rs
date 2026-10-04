//! CalDAV scheduling (RFC 6638) and free/busy over the schedule outbox
//! (M3b WP83; contract ADR 0065).
//!
//! * **Discovery** ([`discover_scheduling`]): the principal's
//!   `schedule-outbox-URL`, `schedule-inbox-URL`,
//!   `calendar-user-address-set` and `calendar-user-type` (rooms and
//!   resources are recognised here; the room finder is filled for Exchange
//!   in M3c), the inbox's `schedule-default-calendar-URL`, and whether the
//!   server schedules itself (`calendar-auto-schedule` in the `DAV`
//!   header of `OPTIONS`).
//! * **Free/busy** ([`query_free_busy`], [`CalDavFreeBusy`] as the
//!   contract's [`FreeBusyProvider`]): a `VFREEBUSY` request `POST`ed to
//!   the outbox; the `schedule-response` gives one answer per recipient
//!   (`request-status` + a `VFREEBUSY` reply).
//! * **Schedule inbox** ([`list_inbox`], [`delete_inbox_item`],
//!   [`process_inbox`]): iTIP messages the server delivered (invitations,
//!   replies, cancellations). The caller (the core with `nail-itip`)
//!   applies each one and deletes it.
//! * **`SCHEDULE-AGENT` / `SCHEDULE-STATUS`** (RFC 6638 §7.1, §7.3):
//!   whether the server or the client delivers an event's iTIP messages
//!   ([`server_schedules`]), marking an event so the server leaves the
//!   delivery to Nail ([`set_schedule_agent`]), and reading the delivery
//!   state the server wrote per attendee ([`schedule_statuses`]).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nail_api::ErrorCode;
use nail_api::engine::{BoxFuture, EngineError, FreeBusyProvider};
use nail_api::ids::PimSourceId;
use nail_api::localized::LocalizedMessage;
use nail_api::pim::calendar::AttendeeKind;
use nail_api::scheduling::{
    AttendeeFreeBusy, BusyPeriod, FreeBusyQuery, FreeBusySource, ItipMethod,
};
use reqwest::Method;
use url::Url;

use crate::client::{DavClient, Depth};
use crate::error::DavError;
use crate::freebusy;
use crate::icalendar::{ContentLine, address_of, located_lines, patch_lines, same_address};
use crate::ops::{CollectionKind, href_key};
use crate::xml;
use crate::{NS_CALDAV, NS_DAV};

/// Most recipients per outbox `POST` (larger queries are split).
pub const MAX_RECIPIENTS_PER_POST: usize = 25;

/// Most inbox items read in one pass.
pub const MAX_INBOX_ITEMS: usize = 500;

/// The scheduling properties read from a principal.
pub const SCHEDULING_PROPS: &[(&str, &str)] = &[
    (NS_CALDAV, "schedule-inbox-URL"),
    (NS_CALDAV, "schedule-outbox-URL"),
    (NS_CALDAV, "calendar-user-address-set"),
    (NS_CALDAV, "calendar-user-type"),
    (NS_DAV, "displayname"),
];

/// Every Fluent id this module puts on per-attendee free/busy failures
/// (in `locales/en/core-dav.ftl`).
pub const MESSAGE_IDS: [&str; 2] = ["dav-freebusy-no-access", "dav-freebusy-failed"];

// ---------------------------------------------------------------------------
// Discovery

/// What a principal offers for scheduling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulingInfo {
    /// The principal.
    pub principal: Url,
    /// `schedule-inbox-URL` (iTIP messages the server delivered).
    pub inbox: Option<Url>,
    /// `schedule-outbox-URL` (free/busy requests are `POST`ed here).
    pub outbox: Option<Url>,
    /// The calendar user's addresses (`mailto:` entries of
    /// `calendar-user-address-set`, without the scheme, in server order).
    pub addresses: Vec<String>,
    /// `calendar-user-type` (RFC 6638 §2.4.2): a person, group, room or
    /// resource.
    pub user_type: AttendeeKind,
    /// Display name of the principal.
    pub display_name: Option<String>,
    /// The server delivers iTIP messages itself (`calendar-auto-schedule`
    /// in the `DAV` header): Nail must not send iMIP mail for events on
    /// this server unless they carry `SCHEDULE-AGENT=CLIENT`.
    pub auto_schedule: bool,
    /// `schedule-default-calendar-URL` of the inbox: where the server
    /// puts new invitations.
    pub default_calendar: Option<Url>,
}

impl SchedulingInfo {
    /// The server can answer free/busy questions.
    pub fn supports_free_busy(&self) -> bool {
        self.outbox.is_some()
    }

    /// Whether `address` is one of the calendar user's.
    pub fn is_own_address(&self, address: &str) -> bool {
        self.addresses.iter().any(|a| same_address(a, address))
    }
}

/// `calendar-user-type` / `CUTYPE` text → [`AttendeeKind`].
pub fn user_type(text: &str) -> AttendeeKind {
    match text.trim().to_ascii_uppercase().as_str() {
        "INDIVIDUAL" | "" => AttendeeKind::Individual,
        "GROUP" => AttendeeKind::Group,
        "RESOURCE" => AttendeeKind::Resource,
        "ROOM" => AttendeeKind::Room,
        _ => AttendeeKind::Unknown,
    }
}

impl DavClient {
    /// `OPTIONS`: the compliance classes of the `DAV` header (lower-case,
    /// e.g. `calendar-access`, `calendar-auto-schedule`).
    pub async fn dav_classes(&self, url: &Url) -> Result<Vec<String>, DavError> {
        let reply = self.send(Method::OPTIONS, url, None, &[], None).await?;
        if !reply.is_success() {
            return Err(reply.error("OPTIONS"));
        }
        Ok(reply
            .headers
            .get_all("dav")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .map(|c| c.trim().to_ascii_lowercase())
            .filter(|c| !c.is_empty())
            .collect())
    }
}

/// Reads the scheduling properties of `principal` and asks `OPTIONS` on
/// `options_url` (a calendar home or calendar; the principal when `None`)
/// whether the server schedules itself. A principal without inbox and
/// outbox is [`DavError::NoScheduling`].
pub async fn discover_scheduling(
    client: &DavClient,
    principal: &Url,
    options_url: Option<&Url>,
) -> Result<SchedulingInfo, DavError> {
    let (base, ms) = client
        .propfind(principal, Depth::Zero, SCHEDULING_PROPS)
        .await?;
    let Some(resp) = ms.responses.first() else {
        return Err(DavError::NoScheduling);
    };
    let first = |name: &str| resp.prop_hrefs(&base, NS_CALDAV, name).into_iter().next();
    let inbox = first("schedule-inbox-URL");
    let outbox = first("schedule-outbox-URL");
    if inbox.is_none() && outbox.is_none() {
        return Err(DavError::NoScheduling);
    }
    let addresses = resp
        .prop(NS_CALDAV, "calendar-user-address-set")
        .map(|set| {
            set.children_named(NS_DAV, "href")
                .map(|h| h.text.trim())
                .filter(|h| {
                    h.get(..7)
                        .is_some_and(|p| p.eq_ignore_ascii_case("mailto:"))
                })
                .map(|h| address_of(h).to_owned())
                .filter(|a| !a.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let user_type = resp
        .prop_text(NS_CALDAV, "calendar-user-type")
        .map_or(AttendeeKind::Individual, |t| user_type(&t));
    let options_target = options_url.unwrap_or(principal);
    let auto_schedule = match client.dav_classes(options_target).await {
        Ok(classes) => classes.iter().any(|c| c == "calendar-auto-schedule"),
        // Login problems, and transient failures: those leave the answer
        // unknown (asked again later); assuming no server scheduling would
        // make Nail mail what the server also delivers.
        Err(
            e @ (DavError::AuthFailed
            | DavError::Token(_)
            | DavError::Network(_)
            | DavError::Timeout
            | DavError::Tls(_)
            | DavError::RateLimited(_)),
        ) => return Err(e),
        Err(e @ DavError::Status { status, .. }) if status >= 500 => return Err(e),
        Err(e) => {
            tracing::debug!(error = %e, "OPTIONS failed; assuming no server scheduling");
            false
        }
    };
    let default_calendar = match &inbox {
        Some(inbox) => match client
            .propfind(
                inbox,
                Depth::Zero,
                &[(NS_CALDAV, "schedule-default-calendar-URL")],
            )
            .await
        {
            Ok((ibase, ims)) => ims.responses.first().and_then(|r| {
                r.prop_hrefs(&ibase, NS_CALDAV, "schedule-default-calendar-URL")
                    .into_iter()
                    .next()
            }),
            Err(e @ (DavError::AuthFailed | DavError::Token(_))) => return Err(e),
            Err(_) => None,
        },
        None => None,
    };
    Ok(SchedulingInfo {
        principal: principal.clone(),
        inbox,
        outbox,
        addresses,
        user_type,
        display_name: resp.prop_text(NS_DAV, "displayname"),
        auto_schedule,
        default_calendar,
    })
}

// ---------------------------------------------------------------------------
// Free/busy through the outbox

/// A recipient's `REQUEST-STATUS` (RFC 5545 §3.8.8.3), e.g. `2.0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestStatus {
    /// The code (`2.0`, `3.7`, `5.1`, …).
    pub code: String,
    /// The description, if any.
    pub description: String,
}

/// What a request status means for a free/busy answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientOutcome {
    /// `2.x`: answered.
    Success,
    /// `3.7` (invalid calendar user) or `5.3` (no scheduling support for
    /// the user): the server does not know the recipient.
    UnknownUser,
    /// `3.8`: the user may not see the recipient's free/busy.
    NoAccess,
    /// Anything else (delivery failed, server error).
    Failed,
}

impl RequestStatus {
    /// Parses `2.0;Success` (or a bare code).
    pub fn parse(text: &str) -> Self {
        let (code, description) = text.trim().split_once(';').unwrap_or((text.trim(), ""));
        Self {
            code: code.trim().to_owned(),
            description: description.trim().to_owned(),
        }
    }

    /// The meaning.
    pub fn outcome(&self) -> RecipientOutcome {
        let mut parts = self.code.split('.');
        match (parts.next(), parts.next()) {
            (Some("2"), _) => RecipientOutcome::Success,
            (Some("3"), Some("7")) | (Some("5"), Some("3")) => RecipientOutcome::UnknownUser,
            (Some("3"), Some("8")) => RecipientOutcome::NoAccess,
            _ => RecipientOutcome::Failed,
        }
    }
}

/// One recipient of a `schedule-response`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientResponse {
    /// The recipient's address (without `mailto:`).
    pub recipient: String,
    /// Its request status.
    pub status: RequestStatus,
    /// The `VFREEBUSY` reply, if the server sent one.
    pub calendar_data: Option<String>,
}

/// Parses a `schedule-response` body (RFC 6638 §10.1). The recipient is
/// read from `recipient/href` or, from older servers, the element's text.
pub fn parse_schedule_response(body: &str) -> Result<Vec<RecipientResponse>, DavError> {
    let root = xml::parse(body).map_err(|e| DavError::Protocol(e.to_string()))?;
    if !root.is(NS_CALDAV, "schedule-response") {
        return Err(DavError::Protocol(format!(
            "expected a schedule-response, got {{{}}}{}",
            root.ns, root.name
        )));
    }
    Ok(root
        .children_named(NS_CALDAV, "response")
        .filter_map(|r| {
            let recipient = r.child(NS_CALDAV, "recipient")?;
            let address = recipient
                .child_text(NS_DAV, "href")
                .unwrap_or_else(|| recipient.text.trim());
            let address = address_of(address).to_owned();
            if address.is_empty() {
                return None;
            }
            Some(RecipientResponse {
                recipient: address,
                status: RequestStatus::parse(r.child_text(NS_CALDAV, "request-status")?),
                calendar_data: r
                    .child(NS_CALDAV, "calendar-data")
                    .map(|d| d.text.clone())
                    .filter(|d| !d.trim().is_empty()),
            })
        })
        .collect())
}

static UID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// A unique `UID` for a free/busy request.
pub fn request_uid() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = UID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("nail-freebusy-{nanos:x}-{n}")
}

/// `POST`s one `VFREEBUSY` request to `outbox` (all of `query`'s
/// attendees in one request; see [`query_free_busy`] for batching) and
/// returns the recipients' answers.
pub async fn post_free_busy(
    client: &DavClient,
    outbox: &Url,
    query: &FreeBusyQuery,
) -> Result<Vec<RecipientResponse>, DavError> {
    let body = freebusy::build_request(query, &request_uid(), now_unix())
        .ok_or_else(|| DavError::Protocol("free/busy range is not RFC 3339 UTC".to_owned()))?;
    let reply = client
        .send(
            Method::POST,
            outbox,
            None,
            &[],
            Some(("text/calendar; charset=utf-8", body.into_bytes())),
        )
        .await?;
    if !reply.is_success() {
        return Err(reply.error("free/busy POST"));
    }
    parse_schedule_response(&reply.text())
}

/// Asks the schedule outbox for the free/busy of `query`'s attendees, in
/// batches of [`MAX_RECIPIENTS_PER_POST`], and maps the answers to the
/// contract: one entry per attendee the server knows, in query order,
/// periods clipped to the range and merged, failures with an
/// [`ErrorCode`] and a `dav-freebusy-*` text. Unknown users are left out
/// (the core reports them as [`FreeBusySource::Unknown`]).
pub async fn query_free_busy(
    client: &DavClient,
    outbox: &Url,
    query: &FreeBusyQuery,
    source_id: &PimSourceId,
) -> Result<Vec<AttendeeFreeBusy>, DavError> {
    let mut responses = Vec::new();
    for batch in query.attendees.chunks(MAX_RECIPIENTS_PER_POST) {
        let part = FreeBusyQuery {
            attendees: batch.to_vec(),
            ..query.clone()
        };
        responses.extend(post_free_busy(client, outbox, &part).await?);
    }
    Ok(to_attendee_free_busy(&responses, query, source_id))
}

/// Maps recipient answers to [`AttendeeFreeBusy`] entries (see
/// [`query_free_busy`]).
pub fn to_attendee_free_busy(
    responses: &[RecipientResponse],
    query: &FreeBusyQuery,
    source_id: &PimSourceId,
) -> Vec<AttendeeFreeBusy> {
    let mut out = Vec::new();
    for attendee in &query.attendees {
        let Some(response) = responses
            .iter()
            .find(|r| same_address(&r.recipient, attendee))
        else {
            continue;
        };
        let mut entry = AttendeeFreeBusy {
            email: attendee.clone(),
            source: FreeBusySource::Caldav {
                source_id: source_id.clone(),
            },
            periods: Vec::new(),
            error: None,
            localized: None,
        };
        match response.status.outcome() {
            RecipientOutcome::UnknownUser => continue,
            RecipientOutcome::Success => {
                let periods: Vec<BusyPeriod> = response
                    .calendar_data
                    .as_deref()
                    .map(freebusy::parse)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|fb| {
                        fb.attendees.is_empty()
                            || fb.attendees.iter().any(|a| same_address(a, attendee))
                    })
                    .flat_map(|fb| fb.periods)
                    .collect();
                entry.periods = freebusy::clip(&periods, &query.from, &query.to);
            }
            RecipientOutcome::NoAccess => {
                entry.error = Some(ErrorCode::Forbidden);
                entry.localized = Some(Box::new(
                    LocalizedMessage::new("dav-freebusy-no-access")
                        .arg("attendee", attendee.as_str()),
                ));
            }
            RecipientOutcome::Failed => {
                entry.error = Some(ErrorCode::Unavailable);
                entry.localized = Some(Box::new(
                    LocalizedMessage::new("dav-freebusy-failed")
                        .arg("attendee", attendee.as_str())
                        .arg("status", response.status.code.as_str()),
                ));
            }
        }
        out.push(entry);
    }
    out
}

/// Free/busy from a CalDAV source's schedule outbox, as the contract's
/// [`FreeBusyProvider`].
#[derive(Debug, Clone)]
pub struct CalDavFreeBusy {
    client: DavClient,
    source_id: PimSourceId,
    info: SchedulingInfo,
}

impl CalDavFreeBusy {
    /// A provider for the source `source_id`; `None` when the principal
    /// has no outbox.
    pub fn new(client: DavClient, source_id: PimSourceId, info: SchedulingInfo) -> Option<Self> {
        info.outbox.is_some().then_some(Self {
            client,
            source_id,
            info,
        })
    }

    /// The scheduling information it works with.
    pub fn info(&self) -> &SchedulingInfo {
        &self.info
    }

    /// The query as sent: the organizer must be one of the principal's
    /// addresses (RFC 6638 §5.1 lets servers refuse others), so a foreign
    /// organizer is replaced by the principal's first address.
    pub fn effective_query(&self, query: &FreeBusyQuery) -> FreeBusyQuery {
        let mut out = query.clone();
        if !self.info.addresses.is_empty() && !self.info.is_own_address(&query.organizer) {
            out.organizer.clone_from(&self.info.addresses[0]);
        }
        out
    }
}

impl FreeBusyProvider for CalDavFreeBusy {
    fn free_busy<'a>(
        &'a self,
        query: &'a FreeBusyQuery,
    ) -> BoxFuture<'a, Result<Vec<AttendeeFreeBusy>, EngineError>> {
        Box::pin(async move {
            if let Some(issue) = query.issues().first() {
                return Err(EngineError::permanent(
                    ErrorCode::InvalidRequest,
                    format!("invalid free/busy query: {issue:?}"),
                )
                .with_localized(LocalizedMessage::new(issue.message_id())));
            }
            let Some(outbox) = &self.info.outbox else {
                return Err(DavError::NoScheduling.into());
            };
            let query = self.effective_query(query);
            query_free_busy(&self.client, outbox, &query, &self.source_id)
                .await
                .map_err(EngineError::from)
        })
    }
}

// ---------------------------------------------------------------------------
// The schedule inbox

/// An iTIP message in the schedule inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItem {
    /// Its path.
    pub href: String,
    /// Its URL.
    pub url: Url,
    /// Its ETag.
    pub etag: Option<String>,
    /// The iCalendar text.
    pub data: String,
    /// `METHOD` (`None` when missing or unknown).
    pub method: Option<ItipMethod>,
    /// `UID` of the first component.
    pub uid: Option<String>,
    /// Who sent it: the `ORGANIZER` for organizer methods, else the
    /// (first) `ATTENDEE`; without `mailto:`.
    pub sender: Option<String>,
    /// `DTSTAMP` of the first component (`YYYYMMDDTHHMMSSZ`), for
    /// ordering.
    pub dtstamp: Option<String>,
}

impl InboxItem {
    /// Reads method, UID, sender and stamp from the text.
    pub fn from_data(href: String, url: Url, etag: Option<String>, data: String) -> Self {
        let lines = located_lines(&data);
        let calendar_value = |name: &str| {
            lines
                .iter()
                .find(|l| l.component == "VCALENDAR" && l.line.name == name)
                .map(|l| l.line.value.trim().to_owned())
        };
        let component_value = |name: &str| {
            lines
                .iter()
                .find(|l| {
                    l.component.starts_with('V')
                        && l.component != "VCALENDAR"
                        && l.component != "VTIMEZONE"
                        && l.line.name == name
                })
                .map(|l| l.line.value.trim().to_owned())
        };
        let method = calendar_value("METHOD").and_then(|m| ItipMethod::from_ical(&m));
        let sender = match method {
            Some(m) if !m.from_organizer() => component_value("ATTENDEE"),
            _ => component_value("ORGANIZER"),
        }
        .map(|a| address_of(&a).to_owned());
        Self {
            uid: component_value("UID"),
            dtstamp: component_value("DTSTAMP"),
            href,
            url,
            etag,
            data,
            method,
            sender,
        }
    }
}

/// Every item in the schedule inbox (at most [`MAX_INBOX_ITEMS`]), oldest
/// `DTSTAMP` first, with data (PROPFIND for the members, then
/// `calendar-multiget`).
pub async fn list_inbox(client: &DavClient, inbox: &Url) -> Result<Vec<InboxItem>, DavError> {
    let mut members = client.list_etags(inbox).await?;
    members.truncate(MAX_INBOX_ITEMS);
    if members.is_empty() {
        return Ok(Vec::new());
    }
    let hrefs: Vec<String> = members.iter().map(|(h, _)| h.clone()).collect();
    let found = client
        .multiget(inbox, CollectionKind::Calendar, &hrefs)
        .await?;
    let mut items: Vec<InboxItem> = found
        .objects
        .into_iter()
        .filter_map(|o| {
            let data = o.data?;
            Some(InboxItem::from_data(o.href, o.url, o.etag, data))
        })
        .collect();
    items.sort_by(|a, b| (&a.dtstamp, &a.href).cmp(&(&b.dtstamp, &b.href)));
    Ok(items)
}

/// Deletes a processed inbox item (with `If-Match` when its ETag is
/// known). An item already gone counts as deleted.
pub async fn delete_inbox_item(client: &DavClient, item: &InboxItem) -> Result<(), DavError> {
    let precondition = match &item.etag {
        Some(etag) => crate::ops::Precondition::IfMatch(etag.clone()),
        None => crate::ops::Precondition::None,
    };
    match client.delete(&item.url, &precondition).await {
        // Gone already, or changed since it was read (the server merged a
        // newer message into it): keep the latter for the next pass.
        Ok(()) | Err(DavError::NotFound(_) | DavError::PreconditionFailed) => Ok(()),
        Err(e) => Err(e),
    }
}

/// What to do with an inbox item after handling it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxDecision {
    /// Applied (or not wanted): delete it from the inbox.
    Delete,
    /// Keep it (e.g. waiting for the user, or applying failed for now).
    Keep,
}

/// The result of [`process_inbox`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxReport {
    /// Items handled and deleted (hrefs).
    pub deleted: Vec<String>,
    /// Items kept (hrefs).
    pub kept: Vec<String>,
}

/// Reads the inbox and hands every item to `handle`, oldest first;
/// deletes those it answers [`InboxDecision::Delete`] for.
pub async fn process_inbox(
    client: &DavClient,
    inbox: &Url,
    mut handle: impl AsyncFnMut(&InboxItem) -> InboxDecision,
) -> Result<InboxReport, DavError> {
    let mut report = InboxReport::default();
    for item in list_inbox(client, inbox).await? {
        match handle(&item).await {
            InboxDecision::Delete => {
                delete_inbox_item(client, &item).await?;
                report.deleted.push(item.href);
            }
            InboxDecision::Keep => report.kept.push(item.href),
        }
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// SCHEDULE-AGENT / SCHEDULE-STATUS

/// `SCHEDULE-AGENT` (RFC 6638 §7.1): who delivers iTIP messages for a
/// calendar user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScheduleAgent {
    /// The server (the default).
    #[default]
    Server,
    /// The client (Nail sends iMIP itself).
    Client,
    /// Nobody (handled out of band).
    None,
}

impl ScheduleAgent {
    /// Parses the parameter value; unknown values are `SERVER` (RFC 6638
    /// §7.1: unknown values are treated as the default).
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_uppercase()).as_deref() {
            Some("CLIENT") => Self::Client,
            Some("NONE") => Self::None,
            _ => Self::Server,
        }
    }

    /// The parameter value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "SERVER",
            Self::Client => "CLIENT",
            Self::None => "NONE",
        }
    }
}

/// Delivery state of one `SCHEDULE-STATUS` code (RFC 6638 §3.2.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleDelivery {
    /// `1.0` / `1.1`: queued, not yet delivered.
    Pending,
    /// `1.2`: delivered.
    Delivered,
    /// `3.x` / `5.x`: not delivered (invalid user, no authority, failure).
    Failed,
    /// Something else.
    Unknown,
}

impl ScheduleDelivery {
    /// The state of a code (`1.2`).
    pub fn of(code: &str) -> Self {
        match code.trim() {
            "1.0" | "1.1" => Self::Pending,
            "1.2" => Self::Delivered,
            c if c.starts_with("3.") || c.starts_with("5.") => Self::Failed,
            _ => Self::Unknown,
        }
    }
}

/// The scheduling parameters the server wrote on one attendee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttendeeScheduleStatus {
    /// Attendee address (without `mailto:`).
    pub email: String,
    /// `SCHEDULE-AGENT`.
    pub agent: ScheduleAgent,
    /// `SCHEDULE-STATUS` codes (usually one).
    pub codes: Vec<String>,
}

impl AttendeeScheduleStatus {
    /// The weakest state of the codes (`Failed` beats `Pending` beats
    /// `Delivered`); `None` without codes.
    pub fn delivery(&self) -> Option<ScheduleDelivery> {
        let states: Vec<ScheduleDelivery> =
            self.codes.iter().map(|c| ScheduleDelivery::of(c)).collect();
        [
            ScheduleDelivery::Failed,
            ScheduleDelivery::Unknown,
            ScheduleDelivery::Pending,
            ScheduleDelivery::Delivered,
        ]
        .into_iter()
        .find(|s| states.contains(s))
    }
}

/// `SCHEDULE-AGENT` and `SCHEDULE-STATUS` of every attendee of the
/// event's components (deduplicated by address, first wins), e.g. from
/// the server's copy after a `PUT`.
pub fn schedule_statuses(ical: &str) -> Vec<AttendeeScheduleStatus> {
    let mut out: Vec<AttendeeScheduleStatus> = Vec::new();
    for located in located_lines(ical) {
        if located.line.name != "ATTENDEE" || located.component == "VFREEBUSY" {
            continue;
        }
        let email = address_of(&located.line.value).to_owned();
        if out.iter().any(|s| s.email.eq_ignore_ascii_case(&email)) {
            continue;
        }
        out.push(AttendeeScheduleStatus {
            agent: ScheduleAgent::parse(located.line.param("SCHEDULE-AGENT")),
            codes: located
                .line
                .param("SCHEDULE-STATUS")
                .map(|v| {
                    v.split(',')
                        .map(|c| c.trim().to_owned())
                        .filter(|c| !c.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            email,
        });
    }
    out
}

/// Whether the server delivers the iTIP messages for this event when the
/// user (one of `own_addresses`) writes it to a calendar on a server with
/// `auto_schedule`:
///
/// * as organizer: some attendee other than the user does not carry
///   `SCHEDULE-AGENT=CLIENT` / `NONE`;
/// * as attendee: the `ORGANIZER` does not carry `SCHEDULE-AGENT=CLIENT` /
///   `NONE` (the reply goes to the organizer).
///
/// `false` without `auto_schedule`, or when the user is neither.
pub fn server_schedules(ical: &str, own_addresses: &[String], auto_schedule: bool) -> bool {
    if !auto_schedule {
        return false;
    }
    let own = |a: &str| own_addresses.iter().any(|o| same_address(o, a));
    let lines: Vec<_> = located_lines(ical)
        .into_iter()
        .filter(|l| matches!(l.component.as_str(), "VEVENT" | "VTODO"))
        .collect();
    let organizer = lines.iter().find(|l| l.line.name == "ORGANIZER");
    match organizer {
        Some(org) if own(&org.line.value) => lines.iter().any(|l| {
            l.line.name == "ATTENDEE"
                && !own(&l.line.value)
                && ScheduleAgent::parse(l.line.param("SCHEDULE-AGENT")) == ScheduleAgent::Server
        }),
        Some(org) => {
            lines
                .iter()
                .any(|l| l.line.name == "ATTENDEE" && own(&l.line.value))
                && ScheduleAgent::parse(org.line.param("SCHEDULE-AGENT")) == ScheduleAgent::Server
        }
        None => false,
    }
}

/// Sets `SCHEDULE-AGENT` on every `ORGANIZER` and `ATTENDEE` of the
/// event's components (`SERVER` removes the parameter, the default), so a
/// scheduling server leaves the delivery to Nail's iMIP mail (`CLIENT`).
/// Other lines stay as they are.
pub fn set_schedule_agent(ical: &str, agent: ScheduleAgent) -> String {
    patch_lines(ical, |component, line| {
        if component == "VFREEBUSY" || !matches!(line.name.as_str(), "ORGANIZER" | "ATTENDEE") {
            return None;
        }
        let mut new = line.clone();
        match agent {
            ScheduleAgent::Server => {
                new.remove_param("SCHEDULE-AGENT");
            }
            other => new.set_param("SCHEDULE-AGENT", other.as_str()),
        }
        Some(new)
    })
    .0
}

/// Removes `SCHEDULE-STATUS` (and `SCHEDULE-FORCE-SEND`) from every
/// `ORGANIZER` / `ATTENDEE`: they are the server's to write, and an
/// iMIP copy for another user must not carry them.
pub fn strip_schedule_params(ical: &str) -> String {
    patch_lines(ical, |_, line| {
        if !matches!(line.name.as_str(), "ORGANIZER" | "ATTENDEE") {
            return None;
        }
        let mut new: ContentLine = line.clone();
        new.remove_param("SCHEDULE-STATUS");
        new.remove_param("SCHEDULE-FORCE-SEND");
        Some(new)
    })
    .0
}

/// The path key of an inbox item URL (as [`href_key`]).
pub fn inbox_key(url: &Url) -> String {
    href_key(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn user_types() {
        assert_eq!(user_type("ROOM"), AttendeeKind::Room);
        assert_eq!(user_type(" resource "), AttendeeKind::Resource);
        assert_eq!(user_type("GROUP"), AttendeeKind::Group);
        assert_eq!(user_type("individual"), AttendeeKind::Individual);
        assert_eq!(user_type(""), AttendeeKind::Individual);
        assert_eq!(user_type("X-CAR"), AttendeeKind::Unknown);
    }

    #[test]
    fn request_statuses() {
        let ok = RequestStatus::parse("2.0;Success");
        assert_eq!(ok.code, "2.0");
        assert_eq!(ok.description, "Success");
        assert_eq!(ok.outcome(), RecipientOutcome::Success);
        assert_eq!(
            RequestStatus::parse("3.7;Invalid calendar user").outcome(),
            RecipientOutcome::UnknownUser
        );
        assert_eq!(
            RequestStatus::parse("5.3").outcome(),
            RecipientOutcome::UnknownUser
        );
        assert_eq!(
            RequestStatus::parse("3.8;No authority").outcome(),
            RecipientOutcome::NoAccess
        );
        assert_eq!(
            RequestStatus::parse("5.1;Could not deliver").outcome(),
            RecipientOutcome::Failed
        );
        assert_eq!(RequestStatus::parse("").outcome(), RecipientOutcome::Failed);
    }

    const RESPONSE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<C:schedule-response xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <C:response>
    <C:recipient><D:href>mailto:bob@example.org</D:href></C:recipient>
    <C:request-status>2.0;Success</C:request-status>
    <C:calendar-data>BEGIN:VCALENDAR
METHOD:REPLY
BEGIN:VFREEBUSY
ATTENDEE:mailto:bob@example.org
FREEBUSY;FBTYPE=BUSY:20261001T090000Z/20261001T100000Z,20261001T093000Z/PT1H
FREEBUSY;FBTYPE=BUSY-TENTATIVE:20261001T120000Z/PT30M
END:VFREEBUSY
END:VCALENDAR
</C:calendar-data>
  </C:response>
  <C:response>
    <C:recipient>MAILTO:Carol@example.org</C:recipient>
    <C:request-status>3.8;No authority</C:request-status>
  </C:response>
  <C:response>
    <C:recipient><D:href>mailto:nobody@example.org</D:href></C:recipient>
    <C:request-status>3.7;Invalid calendar user</C:request-status>
  </C:response>
  <C:response>
    <C:recipient><D:href>mailto:dan@example.org</D:href></C:recipient>
    <C:request-status>5.1;Service unavailable</C:request-status>
  </C:response>
  <C:response>
    <C:recipient><D:href>mailto:erin@example.org</D:href></C:recipient>
    <C:request-status>2.0;Success</C:request-status>
  </C:response>
  <C:response><C:request-status>2.0</C:request-status></C:response>
</C:schedule-response>"#;

    #[test]
    fn schedule_responses_map_to_the_contract() {
        let responses = parse_schedule_response(RESPONSE).unwrap();
        assert_eq!(
            responses.len(),
            5,
            "a response without recipient is skipped"
        );
        assert_eq!(responses[1].recipient, "Carol@example.org");
        let query = FreeBusyQuery {
            organizer: "ada@example.org".into(),
            attendees: vec![
                "erin@example.org".into(),
                "BOB@example.org".into(),
                "carol@example.org".into(),
                "nobody@example.org".into(),
                "dan@example.org".into(),
                "frank@example.org".into(),
            ],
            from: "2026-10-01T00:00:00Z".into(),
            to: "2026-10-02T00:00:00Z".into(),
        };
        let source = PimSourceId::new("work");
        let out = to_attendee_free_busy(&responses, &query, &source);
        let emails: Vec<&str> = out.iter().map(|a| a.email.as_str()).collect();
        assert_eq!(
            emails,
            [
                "erin@example.org",
                "BOB@example.org",
                "carol@example.org",
                "dan@example.org"
            ],
            "query order; unknown and unanswered recipients left out"
        );
        assert!(out[0].periods.is_empty() && out[0].error.is_none());
        assert_eq!(
            out[1].source,
            FreeBusySource::Caldav {
                source_id: source.clone()
            }
        );
        assert_eq!(
            out[1].periods,
            vec![
                BusyPeriod {
                    start: "2026-10-01T09:00:00Z".into(),
                    end: "2026-10-01T10:30:00Z".into(),
                    kind: nail_api::scheduling::BusyKind::Busy,
                },
                BusyPeriod {
                    start: "2026-10-01T12:00:00Z".into(),
                    end: "2026-10-01T12:30:00Z".into(),
                    kind: nail_api::scheduling::BusyKind::Tentative,
                },
            ]
        );
        assert_eq!(out[2].error, Some(ErrorCode::Forbidden));
        assert_eq!(
            out[2].localized.as_ref().unwrap().id,
            "dav-freebusy-no-access"
        );
        assert_eq!(out[3].error, Some(ErrorCode::Unavailable));
        let failed = out[3].localized.as_ref().unwrap();
        assert_eq!(failed.id, "dav-freebusy-failed");
        assert!(failed.args.contains_key("status"));
        for id in out
            .iter()
            .filter_map(|a| a.localized.as_ref())
            .map(|l| l.id.as_str())
        {
            assert!(MESSAGE_IDS.contains(&id));
        }
        assert!(matches!(
            parse_schedule_response("<D:multistatus xmlns:D=\"DAV:\"/>"),
            Err(DavError::Protocol(_))
        ));
        assert!(parse_schedule_response("<<").is_err());
    }

    const MEETING: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VTIMEZONE\r\nTZID:X\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:m-1\r\n\
        DTSTAMP:20261001T080000Z\r\nORGANIZER;CN=Ada:mailto:ada@example.org\r\n\
        ATTENDEE;PARTSTAT=ACCEPTED:mailto:ada@example.org\r\n\
        ATTENDEE;PARTSTAT=NEEDS-ACTION;SCHEDULE-STATUS=1.2:mailto:bob@example.org\r\n\
        ATTENDEE;SCHEDULE-AGENT=CLIENT;SCHEDULE-STATUS=\"3.7,1.1\":mailto:carol@other.org\r\n\
        END:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn schedule_status_and_agent() {
        let statuses = schedule_statuses(MEETING);
        assert_eq!(statuses.len(), 3);
        assert_eq!(statuses[0].delivery(), None);
        assert_eq!(statuses[1].email, "bob@example.org");
        assert_eq!(statuses[1].delivery(), Some(ScheduleDelivery::Delivered));
        assert_eq!(statuses[2].agent, ScheduleAgent::Client);
        assert_eq!(statuses[2].codes, ["3.7", "1.1"]);
        assert_eq!(statuses[2].delivery(), Some(ScheduleDelivery::Failed));
        assert_eq!(ScheduleDelivery::of("1.0"), ScheduleDelivery::Pending);
        assert_eq!(ScheduleDelivery::of("2.0"), ScheduleDelivery::Unknown);
        assert_eq!(ScheduleAgent::parse(Some("x-other")), ScheduleAgent::Server);
        assert_eq!(ScheduleAgent::parse(Some("none")), ScheduleAgent::None);

        let ada = vec!["ada@example.org".to_owned()];
        let bob = vec!["mailto:BOB@example.org".to_owned()];
        assert!(
            server_schedules(MEETING, &ada, true),
            "bob is server-scheduled"
        );
        assert!(!server_schedules(MEETING, &ada, false));
        assert!(
            server_schedules(MEETING, &bob, true),
            "bob replies via the server"
        );
        let stranger = vec!["zed@example.org".to_owned()];
        assert!(!server_schedules(MEETING, &stranger, true));

        let client = set_schedule_agent(MEETING, ScheduleAgent::Client);
        assert!(!server_schedules(&client, &ada, true));
        assert!(!server_schedules(&client, &bob, true));
        assert!(
            client.contains("ORGANIZER;CN=Ada;SCHEDULE-AGENT=CLIENT:mailto:ada@example.org\r\n")
        );
        assert!(client.contains("UID:m-1\r\n"));
        let server = set_schedule_agent(&client, ScheduleAgent::Server);
        assert!(!server.contains("SCHEDULE-AGENT"));
        assert!(server_schedules(&server, &ada, true));

        let stripped = strip_schedule_params(MEETING);
        assert!(!stripped.contains("SCHEDULE-STATUS"));
        assert!(stripped.contains("SCHEDULE-AGENT=CLIENT"));
        let no_organizer = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nATTENDEE:mailto:ada@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert!(!server_schedules(no_organizer, &ada, true));
    }

    #[test]
    fn inbox_items_read_their_headers() {
        let url = Url::parse("https://dav.example.org/inbox/1.ics").unwrap();
        let item = InboxItem::from_data("/inbox/1.ics".into(), url.clone(), None, MEETING.into());
        assert_eq!(item.method, Some(ItipMethod::Request));
        assert_eq!(item.uid.as_deref(), Some("m-1"));
        assert_eq!(item.sender.as_deref(), Some("ada@example.org"));
        assert_eq!(item.dtstamp.as_deref(), Some("20261001T080000Z"));
        let reply = MEETING
            .replace("METHOD:REQUEST", "METHOD:REPLY")
            .replace("ATTENDEE;PARTSTAT=ACCEPTED:mailto:ada@example.org\r\n", "");
        let item = InboxItem::from_data("/inbox/2.ics".into(), url.clone(), None, reply);
        assert_eq!(item.method, Some(ItipMethod::Reply));
        assert_eq!(item.sender.as_deref(), Some("bob@example.org"));
        let unknown = InboxItem::from_data(
            "/x".into(),
            url,
            None,
            "BEGIN:VCALENDAR\r\nMETHOD:POLL\r\nEND:VCALENDAR\r\n".into(),
        );
        assert_eq!(unknown.method, None);
        assert_eq!(unknown.uid, None);
        assert_eq!(
            inbox_key(&Url::parse("https://h/a%20b/c.ics").unwrap()),
            "/a%20b/c.ics"
        );
    }

    #[test]
    fn request_uids_are_unique() {
        let a = request_uid();
        let b = request_uid();
        assert_ne!(a, b);
        assert!(a.starts_with("nail-freebusy-"));
    }
}
