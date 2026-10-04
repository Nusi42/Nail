//! The mock server's data: mailboxes with folders and items (kept as EWS
//! XML elements), sync-state snapshots, subscriptions, settings, the
//! directory, logins and the request log.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;

use base64::Engine as _;
use chrono::{Duration, NaiveDateTime};
use tokio::sync::Notify;

use super::zones;
use super::{InjectedFault, LoggedRequest, MockAuth, SentMessage};
use crate::recurrence::parse_recurrence;
use crate::types::{format_utc, parse_time};
use crate::xml::{self, Element};
use nail_api::exchange::pim::{ExchangeRecurrenceRange, PatternKind};

/// Who may open a mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Everything.
    Full,
    /// Only the calendar, contacts and tasks folders (a delegate).
    PimOnly,
    /// Nothing (`ErrorAccessDenied`).
    Denied,
}

/// A folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockFolder {
    /// Id.
    pub id: String,
    /// Change key.
    pub change_key: String,
    /// Parent id (`None` = the root itself).
    pub parent: Option<String>,
    /// Display name.
    pub name: String,
    /// `FolderClass`.
    pub class: String,
    /// The distinguished id, if any.
    pub distinguished: Option<String>,
}

impl MockFolder {
    /// The element name of the folder (`Folder`, `CalendarFolder`, …).
    pub fn tag(&self) -> &'static str {
        if self.class.starts_with("IPF.Appointment") {
            "CalendarFolder"
        } else if self.class.starts_with("IPF.Contact") {
            "ContactsFolder"
        } else if self.class.starts_with("IPF.Task") {
            "TasksFolder"
        } else {
            "Folder"
        }
    }
}

/// An item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockItem {
    /// Id.
    pub id: String,
    /// Change key.
    pub change_key: String,
    /// Folder id.
    pub folder: String,
    /// `Message`, `CalendarItem`, `Task`, `Contact`, `DistributionList`.
    pub tag: String,
    /// Its properties (children of the item element, without ids).
    pub props: Element,
    /// The MIME source of messages.
    pub mime: Option<Vec<u8>>,
    /// Changed occurrences of a series (index → occurrence element).
    pub exceptions: BTreeMap<u32, Element>,
    /// Deleted occurrences of a series.
    pub deleted: BTreeSet<u32>,
}

impl MockItem {
    /// A property's text.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.props.child_text(name)
    }

    /// Sets a simple property.
    pub fn set(&mut self, name: &str, value: &str) {
        if let Some(child) = self.props.child_mut(name) {
            value.clone_into(&mut child.text);
            child.children.clear();
        } else {
            let mut child = Element::new(name);
            value.clone_into(&mut child.text);
            self.props.children.push(child);
        }
    }

    /// Whether it is read.
    pub fn is_read(&self) -> bool {
        self.props.child_bool("IsRead").unwrap_or(false)
    }
}

/// A mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockMailbox {
    /// Primary address (lowercase).
    pub address: String,
    /// Display name.
    pub name: String,
    /// Who may open it.
    pub access: Access,
    /// Folders (the root first).
    pub folders: Vec<MockFolder>,
    /// Items.
    pub items: Vec<MockItem>,
}

/// What the client has seen of a folder (a sync state).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    /// The folder.
    pub folder: String,
    /// Item id → (change key, read).
    pub items: HashMap<String, (String, bool)>,
}

/// A streaming subscription.
#[derive(Debug, Clone)]
pub struct Subscription {
    /// The mailbox.
    pub mailbox: String,
    /// Watched folder ids.
    pub folders: HashSet<String>,
    /// Events not yet delivered (`<t:…Event>` XML).
    pub pending: Vec<String>,
    /// Wakes open streams.
    pub notify: Arc<Notify>,
}

/// Automatic reply settings as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oof {
    /// `Disabled`, `Enabled`, `Scheduled`.
    pub state: String,
    /// `None`, `Known`, `All`.
    pub audience: String,
    /// Start / end (UTC).
    pub duration: Option<(String, String)>,
    /// Internal reply HTML.
    pub internal: String,
    /// External reply HTML.
    pub external: String,
}

impl Default for Oof {
    fn default() -> Self {
        Self {
            state: "Disabled".into(),
            audience: "All".into(),
            duration: None,
            internal: String::new(),
            external: String::new(),
        }
    }
}

/// A recorded answer for an operation.
#[derive(Debug, Clone)]
pub struct Recorded {
    /// The operation.
    pub operation: String,
    /// The SOAP document.
    pub document: String,
    /// How many requests it answers.
    pub times: u32,
}

/// A meeting message the server sent (invitations, updates,
/// cancellations, responses).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingMessage {
    /// `Request`, `Update`, `Cancellation`, `Accept`, `Tentative`,
    /// `Decline`.
    pub kind: String,
    /// The item's subject.
    pub subject: String,
    /// Recipients.
    pub to: Vec<String>,
    /// The comment of a response.
    pub comment: Option<String>,
}

/// A room list: address, name and rooms (address, name).
pub(super) type RoomListEntry = (String, String, Vec<(String, String)>);

/// The whole server state.
#[derive(Debug)]
pub struct State {
    /// The account's own mailbox address.
    pub own: String,
    /// Mailboxes (the own one first).
    pub mailboxes: Vec<MockMailbox>,
    pub(crate) next_id: u64,
    /// Sync states.
    pub sync_states: HashMap<String, Snapshot>,
    /// Hierarchy sync states (folder id → change key).
    pub hierarchy_states: HashMap<String, (String, HashMap<String, String>)>,
    /// Subscriptions.
    pub subscriptions: HashMap<String, Subscription>,
    /// The request log.
    pub log: Vec<LoggedRequest>,
    /// Faults to inject.
    pub faults: Vec<InjectedFault>,
    /// Recorded answers to replay.
    pub recorded: Vec<Recorded>,
    /// The login the server wants.
    pub auth: MockAuth,
    /// Connections signed in with Negotiate.
    pub authenticated: HashSet<SocketAddr>,
    /// Connections in the middle of a Negotiate handshake.
    pub challenged: HashSet<SocketAddr>,
    /// Messages sent.
    pub sent: Vec<SentMessage>,
    /// Meeting messages sent.
    pub meeting_messages: Vec<MeetingMessage>,
    /// Automatic replies.
    pub oof: Oof,
    /// Inbox rules (as `t:Rule` elements).
    pub rules: Vec<Element>,
    /// Outlook keeps client-only rules (`ErrorOutlookRuleBlobExists`).
    pub outlook_rule_blob: bool,
    /// The category list document.
    pub categories: Option<String>,
    /// Room lists with their rooms (address, name).
    pub room_lists: Vec<RoomListEntry>,
    /// Room lists answer `ErrorAccessDenied`.
    pub rooms_denied: bool,
    /// Other people of the directory (address, name).
    pub directory: Vec<(String, String)>,
    /// Busy periods of people and rooms without a mailbox here
    /// (address → (start, end, busy type)).
    pub free_busy: HashMap<String, Vec<(String, String, String)>>,
    /// Delegates of the own mailbox (address, name, calendar permission).
    pub delegates: Vec<(String, String, String)>,
    /// `RequestServerVersion` values the server refuses.
    pub refused_versions: Vec<String>,
    /// `ServerVersionInfo` (major, minor, build, revision).
    pub version: (u32, u32, u32, u32),
    /// Streams close after this long (the server's `ConnectionTimeout`
    /// would be minutes).
    pub stream_close_after: std::time::Duration,
    /// Streams to break (abort the connection).
    pub break_streams: u32,
    /// `MoveItem` / `CopyItem` between mailboxes allowed (else
    /// `ErrorCrossMailboxMoveCopy`; M4c).
    pub cross_mailbox_moves: bool,
}

impl State {
    /// A new id with a prefix.
    pub fn new_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{:06}", self.next_id)
    }

    /// A new change key.
    pub fn new_key(&mut self) -> String {
        self.next_id += 1;
        format!("CQAAABY{:06}", self.next_id)
    }

    /// The mailbox of an address (`None` = own).
    pub fn mailbox_index(&self, address: Option<&str>) -> Option<usize> {
        let address = address.unwrap_or(&self.own).to_ascii_lowercase();
        self.mailboxes.iter().position(|m| m.address == address)
    }

    /// The mailbox and folder index of a folder id.
    pub fn folder_location(&self, id: &str) -> Option<(usize, usize)> {
        self.mailboxes.iter().enumerate().find_map(|(m, mailbox)| {
            mailbox
                .folders
                .iter()
                .position(|f| f.id == id)
                .map(|f| (m, f))
        })
    }

    /// The mailbox and item index of an item id.
    pub fn item_location(&self, id: &str) -> Option<(usize, usize)> {
        self.mailboxes.iter().enumerate().find_map(|(m, mailbox)| {
            mailbox
                .items
                .iter()
                .position(|i| i.id == id)
                .map(|i| (m, i))
        })
    }

    /// The folder of a distinguished id in a mailbox.
    pub fn distinguished(&self, mailbox: usize, name: &str) -> Option<usize> {
        self.mailboxes[mailbox]
            .folders
            .iter()
            .position(|f| f.distinguished.as_deref() == Some(name))
    }

    /// Queues an event for subscriptions watching `folder` and wakes
    /// their streams.
    pub fn notify(&mut self, folders: &[&str], event: &str) {
        for subscription in self.subscriptions.values_mut() {
            if folders.iter().any(|f| subscription.folders.contains(*f)) {
                subscription.pending.push(event.to_owned());
                subscription.notify.notify_waiters();
                subscription.notify.notify_one();
            }
        }
    }

    /// Queues the item event of a change in `folder` (and `old`).
    pub fn item_event(&mut self, kind: &str, item: &str, folder: &str, old: Option<&str>) {
        let old_part = old
            .map(|o| format!(r#"<t:OldItemId Id="{item}"/><t:OldParentFolderId Id="{o}"/>"#))
            .unwrap_or_default();
        let event = format!(
            r#"<t:{kind}><t:TimeStamp>{}</t:TimeStamp><t:ItemId Id="{item}"/><t:ParentFolderId Id="{folder}"/>{old_part}</t:{kind}>"#,
            format_utc(chrono::Utc::now())
        );
        let mut folders = vec![folder];
        folders.extend(old);
        self.notify(&folders, &event);
        if kind == "CreatedEvent" {
            let new_mail = event.replace("CreatedEvent", "NewMailEvent");
            self.notify(&[folder], &new_mail);
        }
    }

    /// Adds an item; answers its id.
    pub fn add_item(
        &mut self,
        mailbox: usize,
        folder: &str,
        tag: &str,
        props: Element,
        mime: Option<Vec<u8>>,
    ) -> String {
        let id = self.new_id("AAMkItem");
        let change_key = self.new_key();
        self.mailboxes[mailbox].items.push(MockItem {
            id: id.clone(),
            change_key,
            folder: folder.to_owned(),
            tag: tag.to_owned(),
            props,
            mime,
            exceptions: BTreeMap::new(),
            deleted: BTreeSet::new(),
        });
        self.item_event("CreatedEvent", &id, folder, None);
        id
    }

    /// Renders an item element (with `MimeContent` when asked).
    pub fn render_item(item: &MockItem, mime: bool) -> String {
        let mut out = format!("<t:{}>", item.tag);
        if mime && let Some(raw) = &item.mime {
            let _ = write!(
                out,
                r#"<t:MimeContent CharacterSet="UTF-8">{}</t:MimeContent>"#,
                base64::engine::general_purpose::STANDARD.encode(raw)
            );
        }
        let _ = write!(
            out,
            r#"<t:ItemId Id="{}" ChangeKey="{}"/><t:ParentFolderId Id="{}"/>"#,
            item.id, item.change_key, item.folder
        );
        for child in &item.props.children {
            if child.name == "StartTimeZone" {
                if let Some(zone) = child.attr("Id").and_then(zones::zone) {
                    out.push_str(&zone.element("StartTimeZone"));
                }
                continue;
            }
            child.write_xml(Some("t"), &mut out);
        }
        if item.tag == "CalendarItem" && item.props.child("Recurrence").is_some() {
            if !item.exceptions.is_empty() {
                out.push_str("<t:ModifiedOccurrences>");
                for (index, exception) in &item.exceptions {
                    let _ = write!(
                        out,
                        r#"<t:Occurrence><t:ItemId Id="{}" ChangeKey="{}"/>"#,
                        occurrence_id(&item.id, *index),
                        item.change_key
                    );
                    for name in ["Start", "End", "OriginalStart"] {
                        if let Some(value) = exception.child_text(name) {
                            out.push_str(&xml::el(&format!("t:{name}"), value));
                        }
                    }
                    out.push_str("</t:Occurrence>");
                }
                out.push_str("</t:ModifiedOccurrences>");
            }
            if !item.deleted.is_empty() {
                out.push_str("<t:DeletedOccurrences>");
                for (index, start, _) in occurrences(item, 1000) {
                    if item.deleted.contains(&index) {
                        let _ = write!(
                            out,
                            "<t:DeletedOccurrence>{}</t:DeletedOccurrence>",
                            xml::el("t:Start", &start)
                        );
                    }
                }
                out.push_str("</t:DeletedOccurrences>");
            }
        }
        let _ = write!(out, "</t:{}>", item.tag);
        out
    }
}

/// The id of a series' occurrence.
pub(super) fn occurrence_id(master: &str, index: u32) -> String {
    format!("{master}_occ_{index}")
}

/// The master id and index of an occurrence id.
pub(super) fn split_occurrence(id: &str) -> Option<(&str, u32)> {
    let (master, index) = id.rsplit_once("_occ_")?;
    Some((master, index.parse().ok()?))
}

fn naive(text: &str) -> Option<NaiveDateTime> {
    parse_time(text).map(|t| t.naive_utc())
}

/// The occurrences of a series (index from 1, start, end; UTC), at most
/// `limit`. Daily and weekly patterns are expanded; others yield the first
/// occurrence only.
pub(super) fn occurrences(item: &MockItem, limit: usize) -> Vec<(u32, String, String)> {
    let (Some(start), Some(end)) = (
        item.text("Start").and_then(naive),
        item.text("End").and_then(naive),
    ) else {
        return Vec::new();
    };
    let length = end - start;
    let Some(recurrence) = item
        .props
        .child("Recurrence")
        .and_then(|r| parse_recurrence(r, None))
    else {
        return vec![(1, format_utc(start.and_utc()), format_utc(end.and_utc()))];
    };
    let (max_count, until) = match &recurrence.range {
        ExchangeRecurrenceRange::Numbered { occurrences, .. } => (*occurrences as usize, None),
        ExchangeRecurrenceRange::EndDate { end_date, .. } => (
            limit,
            chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d").ok(),
        ),
        ExchangeRecurrenceRange::NoEnd { .. } => (limit, None),
    };
    let interval = i64::from(recurrence.pattern.interval.max(1));
    let mut out = Vec::new();
    let mut day = start;
    let mut index = 0u32;
    let steps = usize::try_from(interval)
        .map_or(usize::MAX, |i| limit.saturating_mul(7).saturating_mul(i))
        .min(20_000);
    for _ in 0..steps {
        if out.len() >= max_count.min(limit) {
            break;
        }
        if until.is_some_and(|u| day.date() > u) {
            break;
        }
        let include = match recurrence.pattern.kind {
            PatternKind::Daily => (day - start).num_days() % interval == 0,
            PatternKind::Weekly => {
                let weeks = (day.date() - start.date()).num_days() / 7;
                let weekday = day.format("%A").to_string();
                weeks % interval == 0
                    && recurrence
                        .pattern
                        .days_of_week
                        .iter()
                        .any(|d| crate::recurrence::day_name(*d) == weekday)
            }
            _ => day == start,
        };
        if include {
            index += 1;
            out.push((
                index,
                format_utc(day.and_utc()),
                format_utc((day + length).and_utc()),
            ));
        }
        day += Duration::days(1);
        if !matches!(
            recurrence.pattern.kind,
            PatternKind::Daily | PatternKind::Weekly
        ) && day > start
        {
            break;
        }
    }
    out
}

/// The busy periods of a calendar item between two UTC times (start,
/// end, busy type), occurrences expanded.
pub(super) fn busy_periods(item: &MockItem, from: &str, to: &str) -> Vec<(String, String, String)> {
    let kind = item
        .text("LegacyFreeBusyStatus")
        .unwrap_or("Busy")
        .to_owned();
    if kind == "Free" || item.props.child_bool("IsCancelled") == Some(true) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (index, start, end) in occurrences(item, 1000) {
        if item.deleted.contains(&index) {
            continue;
        }
        let (start, end) = match item.exceptions.get(&index) {
            Some(e) => (
                e.child_text("Start").unwrap_or(&start).to_owned(),
                e.child_text("End").unwrap_or(&end).to_owned(),
            ),
            None => (start, end),
        };
        if start.as_str() < to && end.as_str() > from {
            out.push((start, end, kind.clone()));
        }
    }
    out
}
