//! PIM items: the raw resources (`pim_objects`) and the parsed events,
//! tasks, contacts and groups, written by the user (through the PIM queue)
//! or by sync (server wins, conflict copies).
//!
//! # User writes
//!
//! [`Store::save_pim_item`], [`Store::delete_pim_item`] and
//! [`Store::move_pim_item`] change the tables and queue the server write
//! ([`super::PimOperation`]) in **one transaction**. Items of the local
//! source are never queued (there is no server); subscriptions and
//! read-only collections refuse writes ([`Error::Conflict`]). A queued
//! item is `pending` until its last operation is replayed.
//!
//! # Sync
//!
//! [`Store::apply_server_item`] merges the server's version of an item:
//!
//! * unknown → inserted; known and unchanged locally → replaced;
//! * changed locally (`pending`) and the server's ETag is still the one
//!   the local change was based on → the local change stays (it uploads
//!   next);
//! * changed locally and changed on the server → **the server wins**: its
//!   version is shown, the local one is kept in `conflict_raw` (state
//!   `conflict`) and the item's queued writes are dropped. A local delete
//!   that meets a server change is undone (the server's version returns).
//!
//! [`Store::remove_server_item`] does the same for server deletes: a
//! locally changed item is kept as a conflict copy without a server URL.
//! Resolving: [`Store::resolve_conflict_keep_server`] drops the copy;
//! keeping the local copy is a normal [`Store::save_pim_item`] of the
//! parsed copy (a user edit of a conflicted item resolves it the same way).

use std::collections::HashSet;

use nail_api::pim::{
    CalendarEvent, Contact, ContactGroup, PimItemKind, PimItemRef, PimItemSync, PimSearchHit,
    PimSourceKind, Task,
};
use nail_api::{AddressBookId, CalendarId, ContactGroupId, ContactId, EventId, TaskId};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::collections::{CollectionInfo, CollectionKind, collection_info};
use super::ops::{PimOpId, PimOperation, cancel_object_ops, enqueue_in_tx};
use super::time::{cache_time, parse_instant};
use super::{PimItem, item_ref, ref_id};
use crate::error::{Error, Result};
use crate::store::Store;

/// A raw resource as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PimObject {
    /// Row id (stable; queued operations name it).
    pub id: i64,
    /// Its calendar or address book.
    pub collection_id: String,
    /// iCalendar / vCard `UID`.
    pub uid: String,
    /// Server URL; `None` until uploaded (and for local / ICS items).
    pub href: Option<String>,
    /// Server ETag.
    pub etag: Option<String>,
    /// What it holds.
    pub kind: PimItemKind,
    /// The iCalendar / vCard text.
    pub raw: String,
    /// Sync state.
    pub sync: PimItemSync,
    /// The local version kept after a conflict.
    pub conflict_raw: Option<String>,
    /// Deleted locally, the server delete is queued.
    pub deleted: bool,
    /// Last change (Unix seconds).
    pub updated_at: i64,
}

/// Result of a user write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PimWrite {
    /// The item.
    pub item: PimItemRef,
    /// Its raw resource (gone after a delete of an item never uploaded).
    pub object_id: i64,
    /// The queued operations (none for the local source).
    pub op_ids: Vec<PimOpId>,
}

/// What [`Store::apply_server_item`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerApply {
    /// A new item.
    Inserted(PimItemRef),
    /// The local item now shows the server's version.
    Updated(PimItemRef),
    /// Nothing changed.
    Unchanged(PimItemRef),
    /// The local change stays; it was made on top of this server version.
    KeptLocal(PimItemRef),
    /// Both changed: the server's version is shown, the local one kept as
    /// a conflict copy.
    Conflict(PimItemRef),
}

impl ServerApply {
    /// The item.
    pub fn item(&self) -> &PimItemRef {
        match self {
            Self::Inserted(i)
            | Self::Updated(i)
            | Self::Unchanged(i)
            | Self::KeptLocal(i)
            | Self::Conflict(i) => i,
        }
    }

    /// Whether views must refresh.
    pub fn changed(&self) -> bool {
        !matches!(self, Self::Unchanged(_) | Self::KeptLocal(_))
    }
}

/// What [`Store::remove_server_item`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerRemove {
    /// The item is gone.
    Removed(PimItemRef),
    /// It had local changes: kept as a conflict copy (no server URL).
    KeptAsConflict(PimItemRef),
    /// No such item.
    Missing,
}

/// A stored resource as sync sees it (to diff against the server).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerObjectInfo {
    /// Row id.
    pub object_id: i64,
    /// `UID`.
    pub uid: String,
    /// Server URL.
    pub href: Option<String>,
    /// Server ETag.
    pub etag: Option<String>,
    /// Sync state.
    pub sync: PimItemSync,
    /// Deleted locally (server delete queued).
    pub deleted: bool,
}

pub(crate) fn kind_str(kind: PimItemKind) -> &'static str {
    match kind {
        PimItemKind::Event => "event",
        PimItemKind::Task => "task",
        PimItemKind::Contact => "contact",
        PimItemKind::ContactGroup => "group",
    }
}

pub(crate) fn parse_kind(text: &str) -> PimItemKind {
    match text {
        "event" => PimItemKind::Event,
        "task" => PimItemKind::Task,
        "group" => PimItemKind::ContactGroup,
        _ => PimItemKind::Contact,
    }
}

fn sync_str(sync: PimItemSync) -> &'static str {
    match sync {
        PimItemSync::Synced => "synced",
        PimItemSync::Pending => "pending",
        PimItemSync::Conflict => "conflict",
    }
}

pub(crate) fn parse_sync(text: &str) -> PimItemSync {
    match text {
        "pending" => PimItemSync::Pending,
        "conflict" => PimItemSync::Conflict,
        _ => PimItemSync::Synced,
    }
}

/// The table and id column of a kind's parsed rows.
fn item_table(kind: PimItemKind) -> &'static str {
    match kind {
        PimItemKind::Event => "pim_events",
        PimItemKind::Task => "pim_tasks",
        PimItemKind::Contact | PimItemKind::ContactGroup => "pim_contacts",
    }
}

/// A group member or contact `UID` as `pim_group_members` keys it:
/// `mailto:` addresses lower-cased, `UID`s lower-cased without
/// `urn:uuid:`.
pub fn member_key(member: &str) -> String {
    let trimmed = member.trim();
    let lower = trimmed.to_lowercase();
    if let Some(rest) = lower.strip_prefix("mailto:") {
        return format!("mailto:{}", rest.trim());
    }
    lower
        .strip_prefix("urn:uuid:")
        .map(str::to_owned)
        .unwrap_or(lower)
}

fn json_err(idx: usize, e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
}

const OBJECT_COLS: &str = "id, collection_id, uid, href, etag, kind,
    nail_open('pim_objects.raw', raw), sync,
    nail_open('pim_objects.conflict_raw', conflict_raw), deleted, updated_at";

fn object_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PimObject> {
    Ok(PimObject {
        id: row.get(0)?,
        collection_id: row.get(1)?,
        uid: row.get(2)?,
        href: row.get(3)?,
        etag: row.get(4)?,
        kind: parse_kind(&row.get::<_, String>(5)?),
        raw: row.get(6)?,
        sync: parse_sync(&row.get::<_, String>(7)?),
        conflict_raw: row.get(8)?,
        deleted: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

pub(crate) fn object_by_id(conn: &Connection, id: i64) -> Result<Option<PimObject>> {
    Ok(conn
        .query_row(
            &format!("SELECT {OBJECT_COLS} FROM pim_objects WHERE id = ?1"),
            [id],
            object_from_row,
        )
        .optional()?)
}

fn object_by_uid(conn: &Connection, collection: &str, uid: &str) -> Result<Option<PimObject>> {
    Ok(conn
        .query_row(
            &format!("SELECT {OBJECT_COLS} FROM pim_objects WHERE collection_id = ?1 AND uid = ?2"),
            params![collection, uid],
            object_from_row,
        )
        .optional()?)
}

fn object_by_href(conn: &Connection, collection: &str, href: &str) -> Result<Option<PimObject>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {OBJECT_COLS} FROM pim_objects WHERE collection_id = ?1 AND href = ?2"
            ),
            params![collection, href],
            object_from_row,
        )
        .optional()?)
}

/// The object id of an item.
pub(crate) fn object_of_item(conn: &Connection, item: &PimItemRef) -> Result<Option<i64>> {
    let kind = item.kind();
    let group_filter = match kind {
        PimItemKind::Contact => " AND kind <> 'group'",
        PimItemKind::ContactGroup => " AND kind = 'group'",
        _ => "",
    };
    Ok(conn
        .query_row(
            &format!(
                "SELECT object_id FROM {} WHERE id = ?1{group_filter}",
                item_table(kind)
            ),
            [ref_id(item)],
            |r| r.get(0),
        )
        .optional()?)
}

/// The id of the parsed item of an object.
fn item_id_of_object(
    conn: &Connection,
    object_id: i64,
    kind: PimItemKind,
) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            &format!("SELECT id FROM {} WHERE object_id = ?1", item_table(kind)),
            [object_id],
            |r| r.get(0),
        )
        .optional()?)
}

fn check_kind(info: &CollectionInfo, kind: PimItemKind) -> Result<()> {
    let wanted = match kind {
        PimItemKind::Event | PimItemKind::Task => CollectionKind::Calendar,
        PimItemKind::Contact | PimItemKind::ContactGroup => CollectionKind::AddressBook,
    };
    if info.kind != wanted {
        return Err(Error::InvalidData(format!(
            "a {} cannot live in collection {}",
            kind_str(kind),
            info.id
        )));
    }
    Ok(())
}

fn writable(info: &CollectionInfo) -> Result<()> {
    if info.read_only || info.source_kind == PimSourceKind::Ics {
        return Err(Error::Conflict(format!(
            "collection {} is read-only",
            info.id
        )));
    }
    Ok(())
}

fn info_of(conn: &Connection, collection: &str) -> Result<CollectionInfo> {
    collection_info(conn, collection)?.ok_or_else(|| Error::not_found("PIM collection", collection))
}

/// Removes an object's parsed rows (all kinds).
fn delete_parsed(tx: &Transaction<'_>, object_id: i64) -> Result<()> {
    for table in ["pim_events", "pim_tasks", "pim_contacts"] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE object_id = ?1"),
            [object_id],
        )?;
    }
    Ok(())
}

fn insert_object(
    tx: &Transaction<'_>,
    item: &PimItem,
    raw: &str,
    href: Option<&str>,
    etag: Option<&str>,
    sync: PimItemSync,
    now: i64,
) -> Result<i64> {
    tx.execute(
        "INSERT INTO pim_objects (collection_id, uid, href, etag, kind, raw, sync, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, nail_seal('pim_objects.raw', ?6), ?7, ?8)",
        params![
            item.collection_id(),
            item.uid(),
            href,
            etag,
            kind_str(item.kind()),
            raw,
            sync_str(sync),
            now
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Event range columns: first start and last end (`None` = open-ended).
pub(crate) fn event_span(event: &CalendarEvent) -> (i64, Option<i64>) {
    let start = cache_time(&event.fields.start).map(|t| t.secs);
    let end = cache_time(&event.fields.end).map(|t| t.secs);
    let first = start.unwrap_or(0);
    let duration = match (start, end) {
        (Some(s), Some(e)) => (e - s).max(0),
        _ => 0,
    };
    let mut last = end.unwrap_or(first).max(first);
    for o in &event.overrides {
        if let Some(e) = cache_time(&o.fields.end) {
            last = last.max(e.secs);
        }
    }
    let Some(recurrence) = event.recurrence.as_ref().filter(|_| event.is_recurring()) else {
        return (first, Some(last));
    };
    for rule in &recurrence.rules {
        match &rule.end {
            nail_api::pim::RecurrenceEnd::Until { until } => match cache_time(until) {
                Some(u) => last = last.max(u.secs + duration),
                None => return (first, None),
            },
            _ => return (first, None),
        }
    }
    for date in &recurrence.dates {
        if let Some(d) = cache_time(date) {
            last = last.max(d.secs + duration);
        }
    }
    (first, Some(last))
}

fn write_event(
    tx: &Transaction<'_>,
    object_id: i64,
    event: &CalendarEvent,
    now: i64,
) -> Result<()> {
    let (first, last) = event_span(event);
    tx.execute(
        "INSERT INTO pim_events
             (id, object_id, calendar_id, uid, title, first_start, last_end, recurring, data, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, nail_seal('pim_events.data', ?9), ?10)
         ON CONFLICT (id) DO UPDATE SET
             object_id = excluded.object_id, calendar_id = excluded.calendar_id,
             uid = excluded.uid, title = excluded.title, first_start = excluded.first_start,
             last_end = excluded.last_end, recurring = excluded.recurring,
             data = excluded.data, updated_at = excluded.updated_at",
        params![
            event.id.as_str(),
            object_id,
            event.calendar_id.as_str(),
            event.uid,
            event.fields.title,
            first,
            last,
            event.is_recurring(),
            serde_json::to_string(event)?,
            now
        ],
    )?;
    super::instances::invalidate_event_in_tx(tx, event.id.as_str())?;
    Ok(())
}

fn task_status_str(task: &Task) -> &'static str {
    use nail_api::pim::TaskStatus;
    match task.fields.status {
        TaskStatus::NeedsAction => "needs-action",
        TaskStatus::InProcess => "in-process",
        TaskStatus::Completed => "completed",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn write_task(
    tx: &Transaction<'_>,
    object_id: i64,
    task: &Task,
    parent_uid: Option<&str>,
    now: i64,
) -> Result<()> {
    let parent_uid: Option<String> = match parent_uid {
        Some(uid) => Some(uid.to_owned()),
        None => match &task.fields.parent_id {
            Some(parent) => tx
                .query_row(
                    "SELECT uid FROM pim_tasks WHERE id = ?1",
                    [parent.as_str()],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        },
    };
    if parent_uid.as_deref() == Some(task.uid.as_str()) {
        return Err(Error::InvalidData("a task cannot be its own parent".into()));
    }
    let secs = |t: &Option<nail_api::pim::PimTime>| t.as_ref().and_then(cache_time).map(|c| c.secs);
    tx.execute(
        "INSERT INTO pim_tasks
             (id, object_id, calendar_id, uid, parent_uid, title, status, priority, due_at,
              start_at, completed_at, sort_order, data, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 nail_seal('pim_tasks.data', ?13), ?14)
         ON CONFLICT (id) DO UPDATE SET
             object_id = excluded.object_id, calendar_id = excluded.calendar_id,
             uid = excluded.uid, parent_uid = excluded.parent_uid, title = excluded.title,
             status = excluded.status, priority = excluded.priority, due_at = excluded.due_at,
             start_at = excluded.start_at, completed_at = excluded.completed_at,
             sort_order = excluded.sort_order, data = excluded.data,
             updated_at = excluded.updated_at",
        params![
            task.id.as_str(),
            object_id,
            task.calendar_id.as_str(),
            task.uid,
            parent_uid,
            task.fields.title,
            task_status_str(task),
            task.fields.priority.filter(|p| *p <= 9),
            secs(&task.fields.due),
            secs(&task.fields.start),
            task.fields.completed_at.as_deref().and_then(parse_instant),
            task.fields.order,
            serde_json::to_string(task)?,
            now
        ],
    )?;
    Ok(())
}

/// Lower-case sort key, family name first.
pub(crate) fn sort_name(contact: &Contact) -> String {
    let name = contact.fields.name.as_ref();
    let family = name.and_then(|n| n.family.as_deref()).map_or("", str::trim);
    let given = name.and_then(|n| n.given.as_deref()).map_or("", str::trim);
    let key = if family.is_empty() {
        contact.fields.display_name.trim().to_owned()
    } else {
        format!("{family} {given}").trim().to_owned()
    };
    key.to_lowercase()
}

fn contact_kind_str(contact: &Contact) -> &'static str {
    use nail_api::pim::contact::ContactKind;
    match contact.fields.kind {
        ContactKind::Individual => "individual",
        ContactKind::Organization => "organization",
        ContactKind::Location => "location",
    }
}

fn write_contact(tx: &Transaction<'_>, object_id: i64, contact: &Contact, now: i64) -> Result<()> {
    let photo = contact
        .fields
        .photo
        .as_ref()
        .and_then(|p| p.sha256.clone())
        .filter(|h| h.len() == 64);
    tx.execute(
        "INSERT INTO pim_contacts
             (id, object_id, address_book_id, uid, kind, display_name, sort_name,
              photo_sha256, data, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, nail_seal('pim_contacts.data', ?9), ?10)
         ON CONFLICT (id) DO UPDATE SET
             object_id = excluded.object_id, address_book_id = excluded.address_book_id,
             uid = excluded.uid, kind = excluded.kind, display_name = excluded.display_name,
             sort_name = excluded.sort_name, photo_sha256 = excluded.photo_sha256,
             data = excluded.data, updated_at = excluded.updated_at",
        params![
            contact.id.as_str(),
            object_id,
            contact.address_book_id.as_str(),
            contact.uid,
            contact_kind_str(contact),
            contact.fields.display_name,
            sort_name(contact),
            photo,
            serde_json::to_string(contact)?,
            now
        ],
    )?;
    tx.execute(
        "DELETE FROM pim_contact_emails WHERE contact_id = ?1",
        [contact.id.as_str()],
    )?;
    let mut seen = HashSet::new();
    for email in &contact.fields.emails {
        let address = email.value.trim().to_lowercase();
        if address.is_empty() || !seen.insert(address.clone()) {
            continue;
        }
        tx.execute(
            "INSERT INTO pim_contact_emails (contact_id, email, preferred) VALUES (?1, ?2, ?3)",
            params![contact.id.as_str(), address, email.preferred],
        )?;
    }
    Ok(())
}

fn write_group(
    tx: &Transaction<'_>,
    object_id: i64,
    group: &ContactGroup,
    members: &[String],
    now: i64,
) -> Result<()> {
    let mut keys: Vec<String> = members.iter().map(|m| member_key(m)).collect();
    if members.is_empty() {
        for id in &group.member_ids {
            let uid: Option<String> = tx
                .query_row(
                    "SELECT uid FROM pim_contacts WHERE id = ?1 AND kind <> 'group'",
                    [id.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            match uid {
                Some(uid) => keys.push(member_key(&uid)),
                None => return Err(Error::not_found("contact", id)),
            }
        }
        keys.extend(
            group
                .member_emails
                .iter()
                .map(|e| member_key(&format!("mailto:{e}"))),
        );
    }
    keys.retain(|k| !k.is_empty() && k != "mailto:");
    keys.sort();
    keys.dedup();
    tx.execute(
        "INSERT INTO pim_contacts
             (id, object_id, address_book_id, uid, kind, display_name, sort_name, data, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'group', ?5, ?6, nail_seal('pim_contacts.data', ?7), ?8)
         ON CONFLICT (id) DO UPDATE SET
             object_id = excluded.object_id, address_book_id = excluded.address_book_id,
             uid = excluded.uid, kind = 'group', display_name = excluded.display_name,
             sort_name = excluded.sort_name, data = excluded.data,
             updated_at = excluded.updated_at",
        params![
            group.id.as_str(),
            object_id,
            group.address_book_id.as_str(),
            group.uid,
            group.name,
            group.name.trim().to_lowercase(),
            serde_json::to_string(group)?,
            now
        ],
    )?;
    tx.execute(
        "DELETE FROM pim_group_members WHERE group_id = ?1",
        [group.id.as_str()],
    )?;
    for key in keys {
        tx.execute(
            "INSERT INTO pim_group_members (group_id, member) VALUES (?1, ?2)",
            params![group.id.as_str(), key],
        )?;
    }
    Ok(())
}

/// Writes the parsed rows of an item onto its object. Rows of another kind
/// on the same object (a `UID` reused for another kind) are removed.
fn write_parsed(tx: &Transaction<'_>, object_id: i64, item: &PimItem, now: i64) -> Result<()> {
    let kind = item.kind();
    for table in ["pim_events", "pim_tasks", "pim_contacts"] {
        if table != item_table(kind) {
            tx.execute(
                &format!("DELETE FROM {table} WHERE object_id = ?1"),
                [object_id],
            )?;
        }
    }
    // A contact that became a group (or back) keeps one row of the same id.
    if matches!(kind, PimItemKind::Contact | PimItemKind::ContactGroup) {
        tx.execute(
            "DELETE FROM pim_contacts WHERE object_id = ?1 AND id <> ?2",
            params![object_id, item.id()],
        )?;
    }
    tx.execute(
        "UPDATE pim_objects SET kind = ?2 WHERE id = ?1",
        params![object_id, kind_str(kind)],
    )?;
    match item {
        PimItem::Event(e) => write_event(tx, object_id, e, now),
        PimItem::Task { task, parent_uid } => {
            write_task(tx, object_id, task, parent_uid.as_deref(), now)
        }
        PimItem::Contact(c) => write_contact(tx, object_id, c, now),
        PimItem::Group { group, members } => write_group(tx, object_id, group, members, now),
    }
}

/// Reads the stored item of an object as written (for moves).
fn load_item(tx: &Transaction<'_>, kind: PimItemKind, id: &str) -> Result<PimItem> {
    let data = |table: &str, column: &'static str| -> Result<String> {
        tx.query_row(
            &format!("SELECT nail_open('{table}.data', data) FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::not_found(column, id))
    };
    Ok(match kind {
        PimItemKind::Event => PimItem::Event(serde_json::from_str(&data("pim_events", "event")?)?),
        PimItemKind::Task => {
            let parent_uid: Option<String> = tx.query_row(
                "SELECT parent_uid FROM pim_tasks WHERE id = ?1",
                [id],
                |r| r.get(0),
            )?;
            PimItem::Task {
                task: serde_json::from_str(&data("pim_tasks", "task")?)?,
                parent_uid,
            }
        }
        PimItemKind::Contact => {
            PimItem::Contact(serde_json::from_str(&data("pim_contacts", "contact")?)?)
        }
        PimItemKind::ContactGroup => {
            let mut stmt =
                tx.prepare("SELECT member FROM pim_group_members WHERE group_id = ?1")?;
            let members = stmt
                .query_map([id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(|key| {
                    if key.starts_with("mailto:") {
                        key
                    } else {
                        format!("urn:uuid:{key}")
                    }
                })
                .collect();
            PimItem::Group {
                group: serde_json::from_str(&data("pim_contacts", "group")?)?,
                members,
            }
        }
    })
}

/// The transaction part of [`Store::save_pim_item`].
pub(crate) fn save_item_tx(
    tx: &Transaction<'_>,
    item: &PimItem,
    raw: &str,
    now: i64,
) -> Result<PimWrite> {
    let info = info_of(tx, item.collection_id())?;
    check_kind(&info, item.kind())?;
    writable(&info)?;
    let existing = object_by_uid(tx, &info.id, item.uid())?;
    if let Some(obj) = &existing
        && let Some(current) = item_id_of_object(tx, obj.id, item.kind())?
        && current != item.id()
    {
        return Err(Error::InvalidData(format!(
            "UID {} belongs to another item",
            item.uid()
        )));
    }
    if let Some(other) = object_of_item(tx, &item.item_ref())?
        && existing.as_ref().map(|o| o.id) != Some(other)
    {
        return Err(Error::InvalidData(format!(
            "item {} lives in another collection or under another UID",
            item.id()
        )));
    }
    let remote = info.source_kind != PimSourceKind::Local;
    let sync = if remote {
        PimItemSync::Pending
    } else {
        PimItemSync::Synced
    };
    let object_id = match &existing {
        Some(obj) => {
            tx.execute(
                "UPDATE pim_objects SET raw = nail_seal('pim_objects.raw', ?2), sync = ?3,
                         conflict_raw = NULL, deleted = 0, updated_at = ?4
                     WHERE id = ?1",
                params![obj.id, raw, sync_str(sync), now],
            )?;
            obj.id
        }
        None => insert_object(tx, item, raw, None, None, sync, now)?,
    };
    write_parsed(tx, object_id, item, now)?;
    let op_ids = if remote {
        vec![enqueue_in_tx(
            tx,
            &info.source_id,
            &PimOperation::Put { object_id },
        )?]
    } else {
        Vec::new()
    };
    Ok(PimWrite {
        item: item.item_ref(),
        object_id,
        op_ids,
    })
}

/// The transaction part of [`Store::delete_pim_item`].
pub(crate) fn delete_item_tx(
    tx: &Transaction<'_>,
    item: &PimItemRef,
    now: i64,
) -> Result<PimWrite> {
    let object_id =
        object_of_item(tx, item)?.ok_or_else(|| Error::not_found("PIM item", ref_id(item)))?;
    let obj =
        object_by_id(tx, object_id)?.ok_or_else(|| Error::not_found("PIM object", object_id))?;
    let info = info_of(tx, &obj.collection_id)?;
    writable(&info)?;
    let mut op_ids = Vec::new();
    if info.source_kind == PimSourceKind::Local {
        tx.execute("DELETE FROM pim_objects WHERE id = ?1", [object_id])?;
    } else {
        let in_flight: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM pim_ops WHERE object_id = ?1
                                AND state = 'in_flight')",
            [object_id],
            |r| r.get(0),
        )?;
        // Uploads not started yet are pointless now.
        tx.execute(
            "DELETE FROM pim_ops WHERE object_id = ?1 AND state = 'pending'",
            [object_id],
        )?;
        if obj.href.is_none() && !in_flight {
            tx.execute("DELETE FROM pim_objects WHERE id = ?1", [object_id])?;
        } else {
            delete_parsed(tx, object_id)?;
            tx.execute(
                "UPDATE pim_objects SET deleted = 1, sync = 'pending', updated_at = ?2
                     WHERE id = ?1",
                params![object_id, now],
            )?;
            op_ids.push(enqueue_in_tx(
                tx,
                &info.source_id,
                &PimOperation::Delete {
                    object_id: Some(object_id),
                    collection_id: info.id.clone(),
                    href: obj.href.clone(),
                    etag: obj.etag.clone(),
                },
            )?);
        }
    }
    Ok(PimWrite {
        item: item.clone(),
        object_id,
        op_ids,
    })
}

impl Store {
    // ------------------------------------------------------------- user writes

    /// Saves a user's new or changed item with its raw text (written by
    /// `nail-ical`) and queues the upload — one transaction. Items of the
    /// local source are saved `synced` without a queued write. Editing a
    /// conflicted item drops its conflict copy (the edit wins).
    pub fn save_pim_item(&self, item: &PimItem, raw: &str, now: i64) -> Result<PimWrite> {
        self.with_tx(|tx| save_item_tx(tx, item, raw, now))
    }

    /// Deletes a user's item. Server items are hidden at once and their
    /// delete is queued (the raw resource stays, marked deleted, until the
    /// server confirmed); local items and items never uploaded are removed
    /// right away. Reminders and cached instances go with the item.
    pub fn delete_pim_item(&self, item: &PimItemRef, now: i64) -> Result<PimWrite> {
        self.with_tx(|tx| delete_item_tx(tx, item, now))
    }

    /// Moves an item to another calendar / address book (of any source):
    /// queued as an upload to the new place and a delete at the old one.
    pub fn move_pim_item(&self, item: &PimItemRef, to: &str, now: i64) -> Result<PimWrite> {
        self.with_tx(|tx| {
            let object_id = object_of_item(tx, item)?
                .ok_or_else(|| Error::not_found("PIM item", ref_id(item)))?;
            let obj = object_by_id(tx, object_id)?
                .ok_or_else(|| Error::not_found("PIM object", object_id))?;
            let from = info_of(tx, &obj.collection_id)?;
            let target = info_of(tx, to)?;
            check_kind(&target, item.kind())?;
            writable(&from)?;
            writable(&target)?;
            if from.id == target.id {
                return Ok(PimWrite {
                    item: item.clone(),
                    object_id,
                    op_ids: Vec::new(),
                });
            }
            if object_by_uid(tx, &target.id, &obj.uid)?.is_some() {
                return Err(Error::Conflict(format!(
                    "collection {to} already holds UID {}",
                    obj.uid
                )));
            }
            let mut moved = load_item(tx, item.kind(), ref_id(item))?;
            moved.set_collection(&target.id);
            let to_remote = target.source_kind != PimSourceKind::Local;
            // Uploads queued for the old place must not run any more.
            tx.execute(
                "DELETE FROM pim_ops WHERE object_id = ?1 AND state = 'pending'",
                [object_id],
            )?;
            tx.execute(
                "UPDATE pim_objects SET collection_id = ?2, href = NULL, etag = NULL,
                     sync = ?3, conflict_raw = NULL, updated_at = ?4
                 WHERE id = ?1",
                params![
                    object_id,
                    target.id,
                    sync_str(if to_remote {
                        PimItemSync::Pending
                    } else {
                        PimItemSync::Synced
                    }),
                    now
                ],
            )?;
            write_parsed(tx, object_id, &moved, now)?;
            let mut op_ids = Vec::new();
            if from.source_kind != PimSourceKind::Local
                && let Some(href) = &obj.href
            {
                op_ids.push(enqueue_in_tx(
                    tx,
                    &from.source_id,
                    &PimOperation::Delete {
                        object_id: None,
                        collection_id: from.id.clone(),
                        href: Some(href.clone()),
                        etag: obj.etag.clone(),
                    },
                )?);
            }
            if to_remote {
                op_ids.push(enqueue_in_tx(
                    tx,
                    &target.source_id,
                    &PimOperation::Put { object_id },
                )?);
            }
            Ok(PimWrite {
                item: item.clone(),
                object_id,
                op_ids,
            })
        })
    }

    // -------------------------------------------------------------------- sync

    /// Merges the server's version of an item (see the module docs). The
    /// item's id is kept when the store knows the resource already (by URL,
    /// else by `UID`); `item`'s own id is used for new ones.
    pub fn apply_server_item(
        &self,
        item: &PimItem,
        raw: &str,
        href: Option<&str>,
        etag: Option<&str>,
        now: i64,
    ) -> Result<ServerApply> {
        self.with_tx(|tx| {
            let info = info_of(tx, item.collection_id())?;
            check_kind(&info, item.kind())?;
            let existing = match href {
                Some(h) => object_by_href(tx, &info.id, h)?,
                None => None,
            };
            let existing = match existing {
                Some(obj) => Some(obj),
                None => object_by_uid(tx, &info.id, item.uid())?,
            };
            let Some(obj) = existing else {
                if object_of_item(tx, &item.item_ref())?.is_some() {
                    return Err(Error::InvalidData(format!(
                        "item id {} is in use",
                        item.id()
                    )));
                }
                let object_id = insert_object(tx, item, raw, href, etag, PimItemSync::Synced, now)?;
                write_parsed(tx, object_id, item, now)?;
                return Ok(ServerApply::Inserted(item.item_ref()));
            };
            let mut item = item.clone();
            if let Some(id) = item_id_of_object(tx, obj.id, item.kind())? {
                item.set_id(&id);
            }
            let reference = item.item_ref();
            let replace = |sync: PimItemSync, conflict: Option<&str>| -> Result<()> {
                tx.execute(
                    "UPDATE pim_objects SET raw = nail_seal('pim_objects.raw', ?2), href = ?3,
                         etag = ?4, sync = ?5,
                         conflict_raw = nail_seal('pim_objects.conflict_raw', ?6),
                         deleted = 0, updated_at = ?7
                     WHERE id = ?1",
                    params![
                        obj.id,
                        raw,
                        href.or(obj.href.as_deref()),
                        etag,
                        sync_str(sync),
                        conflict,
                        now
                    ],
                )?;
                write_parsed(tx, obj.id, &item, now)
            };
            match obj.sync {
                PimItemSync::Synced => {
                    if obj.raw == raw && (etag.is_none() || obj.etag.as_deref() == etag) {
                        if href.is_some() && obj.href.as_deref() != href {
                            tx.execute(
                                "UPDATE pim_objects SET href = ?2 WHERE id = ?1",
                                params![obj.id, href],
                            )?;
                        }
                        return Ok(ServerApply::Unchanged(reference));
                    }
                    replace(PimItemSync::Synced, None)?;
                    Ok(ServerApply::Updated(reference))
                }
                PimItemSync::Pending => {
                    if etag.is_some() && obj.etag.as_deref() == etag {
                        return Ok(ServerApply::KeptLocal(reference));
                    }
                    cancel_object_ops(tx, obj.id, "conflict")?;
                    if obj.deleted {
                        // The server changed what was deleted here: it returns.
                        replace(PimItemSync::Synced, None)?;
                        return Ok(ServerApply::Updated(reference));
                    }
                    replace(PimItemSync::Conflict, Some(&obj.raw))?;
                    Ok(ServerApply::Conflict(reference))
                }
                PimItemSync::Conflict => {
                    replace(PimItemSync::Conflict, obj.conflict_raw.as_deref())?;
                    Ok(ServerApply::Updated(reference))
                }
            }
        })
    }

    /// The server no longer has a resource (by URL, else by `UID`). A
    /// locally changed item is kept as a conflict copy without a server
    /// URL; anything else is removed.
    pub fn remove_server_item(
        &self,
        collection: &str,
        href: Option<&str>,
        uid: Option<&str>,
    ) -> Result<ServerRemove> {
        self.with_tx(|tx| {
            let mut obj = match href {
                Some(h) => object_by_href(tx, collection, h)?,
                None => None,
            };
            if obj.is_none()
                && let Some(uid) = uid
            {
                obj = object_by_uid(tx, collection, uid)?;
            }
            let Some(obj) = obj else {
                return Ok(ServerRemove::Missing);
            };
            let id = item_id_of_object(tx, obj.id, obj.kind)?;
            let reference = item_ref(obj.kind, id.as_deref().unwrap_or_default());
            if obj.sync == PimItemSync::Pending && !obj.deleted {
                cancel_object_ops(tx, obj.id, "conflict")?;
                tx.execute(
                    "UPDATE pim_objects SET href = NULL, etag = NULL, sync = 'conflict',
                         conflict_raw = raw
                     WHERE id = ?1",
                    [obj.id],
                )?;
                return Ok(ServerRemove::KeptAsConflict(reference));
            }
            cancel_object_ops(tx, obj.id, "deleted on the server")?;
            tx.execute("DELETE FROM pim_objects WHERE id = ?1", [obj.id])?;
            Ok(ServerRemove::Removed(reference))
        })
    }

    /// Removes the resources of a collection whose `UID` is not in `uids`
    /// (an ICS feed was fetched in full), like [`Store::remove_server_item`].
    pub fn retain_server_uids(
        &self,
        collection: &str,
        uids: &[String],
    ) -> Result<Vec<ServerRemove>> {
        let keep: HashSet<&str> = uids.iter().map(String::as_str).collect();
        let gone: Vec<String> = self
            .server_objects(collection)?
            .into_iter()
            .filter(|o| !keep.contains(o.uid.as_str()))
            .map(|o| o.uid)
            .collect();
        gone.iter()
            .map(|uid| self.remove_server_item(collection, None, Some(uid)))
            .collect()
    }

    /// Makes the next [`Store::apply_server_item`] of every synced resource
    /// of a collection rewrite it even when the server's text and ETag did
    /// not change (the stored text and ETag are cleared): after the way
    /// server items are read changed (a subscription's `alarms`). Returns
    /// how many were marked.
    pub fn forget_server_versions(&self, collection: &str) -> Result<usize> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE pim_objects SET raw = nail_seal('pim_objects.raw', ''), etag = NULL
                 WHERE collection_id = ?1 AND sync = 'synced' AND deleted = 0",
                [collection],
            )?)
        })
    }

    /// Every resource of a collection with its URL, ETag and state.
    pub fn server_objects(&self, collection: &str) -> Result<Vec<ServerObjectInfo>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, uid, href, etag, sync, deleted FROM pim_objects
                 WHERE collection_id = ?1 ORDER BY id",
            )?;
            let rows = stmt.query_map([collection], |r| {
                Ok(ServerObjectInfo {
                    object_id: r.get(0)?,
                    uid: r.get(1)?,
                    href: r.get(2)?,
                    etag: r.get(3)?,
                    sync: parse_sync(&r.get::<_, String>(4)?),
                    deleted: r.get(5)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Resolves a conflict for the server: drops the local copy (an item
    /// the server deleted goes away).
    pub fn resolve_conflict_keep_server(&self, item: &PimItemRef) -> Result<bool> {
        self.with_tx(|tx| {
            let Some(object_id) = object_of_item(tx, item)? else {
                return Ok(false);
            };
            let Some(obj) = object_by_id(tx, object_id)? else {
                return Ok(false);
            };
            if obj.sync != PimItemSync::Conflict {
                return Ok(false);
            }
            if obj.href.is_none() {
                let info = info_of(tx, &obj.collection_id)?;
                if info.source_kind != PimSourceKind::Local {
                    tx.execute("DELETE FROM pim_objects WHERE id = ?1", [object_id])?;
                    return Ok(true);
                }
            }
            tx.execute(
                "UPDATE pim_objects SET conflict_raw = NULL, sync = 'synced' WHERE id = ?1",
                [object_id],
            )?;
            Ok(true)
        })
    }

    // ------------------------------------------------------------------- reads

    /// The raw resource of an item.
    pub fn pim_object(&self, item: &PimItemRef) -> Result<Option<PimObject>> {
        self.with_conn(|conn| match object_of_item(conn, item)? {
            Some(id) => object_by_id(conn, id),
            None => Ok(None),
        })
    }

    /// A raw resource by row id (replay).
    pub fn pim_object_by_id(&self, object_id: i64) -> Result<Option<PimObject>> {
        self.with_conn(|conn| object_by_id(conn, object_id))
    }

    /// One event series.
    pub fn event(&self, id: &EventId) -> Result<Option<CalendarEvent>> {
        self.with_conn(|conn| load_events(conn, "e.id = ?1", &[id.as_str()]).map(|mut v| v.pop()))
    }

    /// The events of calendars (all calendars when `None`), by first start.
    pub fn events(&self, calendars: Option<&[CalendarId]>) -> Result<Vec<CalendarEvent>> {
        self.with_conn(|conn| match calendars {
            None => load_events(conn, "1", &[]),
            Some(ids) => {
                let ids: Vec<&str> = ids.iter().map(CalendarId::as_str).collect();
                load_events(conn, &in_list("e.calendar_id", ids.len(), 1), &ids)
            }
        })
    }

    /// One task (with its parent and subtasks resolved).
    pub fn task(&self, id: &TaskId) -> Result<Option<Task>> {
        self.with_conn(|conn| load_tasks(conn, "t.id = ?1", &[id.as_str()]).map(|mut v| v.pop()))
    }

    /// Tasks of task lists (every visible list holding tasks when `None`):
    /// open ones first, then by manual order, due date, priority, title.
    pub fn list_tasks(
        &self,
        calendars: Option<&[CalendarId]>,
        include_completed: bool,
    ) -> Result<Vec<Task>> {
        let ids: Vec<String> = match calendars {
            Some(ids) => ids.iter().map(|c| c.0.clone()).collect(),
            None => self
                .calendars(None)?
                .into_iter()
                .filter(|c| c.settings.visible && c.has_tasks())
                .map(|c| c.id.0)
                .collect(),
        };
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut filter = in_list("t.calendar_id", refs.len(), 1);
        if !include_completed {
            filter.push_str(" AND t.status NOT IN ('completed', 'cancelled')");
        }
        self.with_conn(|conn| load_tasks(conn, &filter, &refs))
    }

    /// One contact (groups and links resolved).
    pub fn contact(&self, id: &ContactId) -> Result<Option<Contact>> {
        self.with_conn(|conn| {
            load_contacts(conn, "p.id = ?1 AND p.kind <> 'group'", &[id.as_str()])
                .map(|mut v| v.pop())
        })
    }

    /// Contacts by id (missing ones skipped), in the given order.
    pub fn contacts_by_ids(&self, ids: &[ContactId]) -> Result<Vec<Contact>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let refs: Vec<&str> = ids.iter().map(ContactId::as_str).collect();
        let found = self.with_conn(|conn| {
            load_contacts(
                conn,
                &format!("{} AND p.kind <> 'group'", in_list("p.id", refs.len(), 1)),
                &refs,
            )
        })?;
        Ok(ids
            .iter()
            .filter_map(|id| found.iter().find(|c| &c.id == id).cloned())
            .collect())
    }

    /// Every contact of address books (all when `None`).
    pub fn contacts(&self, books: Option<&[AddressBookId]>) -> Result<Vec<Contact>> {
        self.with_conn(|conn| match books {
            None => load_contacts(conn, "p.kind <> 'group'", &[]),
            Some(ids) => {
                let ids: Vec<&str> = ids.iter().map(AddressBookId::as_str).collect();
                load_contacts(
                    conn,
                    &format!(
                        "{} AND p.kind <> 'group'",
                        in_list("p.address_book_id", ids.len(), 1)
                    ),
                    &ids,
                )
            }
        })
    }

    /// One group (members resolved).
    pub fn contact_group(&self, id: &ContactGroupId) -> Result<Option<ContactGroup>> {
        self.with_conn(|conn| load_groups(conn, "p.id = ?1", &[id.as_str()]).map(|mut v| v.pop()))
    }

    /// The groups of address books (all when `None`), by name.
    pub fn contact_groups(&self, books: Option<&[AddressBookId]>) -> Result<Vec<ContactGroup>> {
        self.with_conn(|conn| match books {
            None => load_groups(conn, "1", &[]),
            Some(ids) => {
                let ids: Vec<&str> = ids.iter().map(AddressBookId::as_str).collect();
                load_groups(conn, &in_list("p.address_book_id", ids.len(), 1), &ids)
            }
        })
    }

    /// Search hits for items (missing ones skipped), in the given order.
    pub fn pim_search_hits(&self, items: &[PimItemRef]) -> Result<Vec<PimSearchHit>> {
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let hit = match item {
                PimItemRef::Event { event_id } => self.event(event_id)?.map(|e| PimSearchHit {
                    item: item.clone(),
                    title: e.fields.title.clone(),
                    detail: e.fields.location.clone(),
                    calendar_id: Some(e.calendar_id.clone()),
                    address_book_id: None,
                    start: Some(e.fields.start.clone()),
                }),
                PimItemRef::Task { task_id } => self.task(task_id)?.map(|t| PimSearchHit {
                    item: item.clone(),
                    title: t.fields.title.clone(),
                    detail: t.fields.location.clone(),
                    calendar_id: Some(t.calendar_id.clone()),
                    address_book_id: None,
                    start: t.fields.due.clone(),
                }),
                PimItemRef::Contact { contact_id } => {
                    self.contact(contact_id)?.map(|c| PimSearchHit {
                        item: item.clone(),
                        title: c.fields.display_name.clone(),
                        detail: c.fields.primary_email().map(str::to_owned),
                        calendar_id: None,
                        address_book_id: Some(c.address_book_id.clone()),
                        start: None,
                    })
                }
                PimItemRef::ContactGroup { group_id } => {
                    self.contact_group(group_id)?.map(|g| PimSearchHit {
                        item: item.clone(),
                        title: g.name.clone(),
                        detail: None,
                        calendar_id: None,
                        address_book_id: Some(g.address_book_id.clone()),
                        start: None,
                    })
                }
            };
            out.extend(hit);
        }
        Ok(out)
    }
}

/// `column IN (?n, ?n+1, …)` for `count` parameters starting at `first`.
pub(crate) fn in_list(column: &str, count: usize, first: usize) -> String {
    if count == 0 {
        return "0".to_owned();
    }
    let params: Vec<String> = (first..first + count).map(|i| format!("?{i}")).collect();
    format!("{column} IN ({})", params.join(", "))
}

pub(crate) fn load_events(
    conn: &Connection,
    filter: &str,
    args: &[&str],
) -> Result<Vec<CalendarEvent>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT nail_open('pim_events.data', e.data), o.sync, c.read_only
         FROM pim_events e
         JOIN pim_objects o ON o.id = e.object_id
         JOIN pim_collections c ON c.id = e.calendar_id
         WHERE {filter} ORDER BY e.first_start, e.id"
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
        let mut event: CalendarEvent =
            serde_json::from_str(&r.get::<_, String>(0)?).map_err(|e| json_err(0, e))?;
        event.sync = parse_sync(&r.get::<_, String>(1)?);
        event.read_only = r.get(2)?;
        Ok(event)
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn load_tasks(conn: &Connection, filter: &str, args: &[&str]) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT nail_open('pim_tasks.data', t.data), o.sync, c.read_only, t.parent_uid, t.uid,
                t.calendar_id
         FROM pim_tasks t
         JOIN pim_objects o ON o.id = t.object_id
         JOIN pim_collections c ON c.id = t.calendar_id
         WHERE {filter}
         ORDER BY t.status IN ('completed', 'cancelled'), t.sort_order IS NULL, t.sort_order,
                  t.due_at IS NULL, t.due_at, t.priority IS NULL OR t.priority = 0, t.priority,
                  t.title COLLATE NOCASE, t.id"
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
        let mut task: Task =
            serde_json::from_str(&r.get::<_, String>(0)?).map_err(|e| json_err(0, e))?;
        task.sync = parse_sync(&r.get::<_, String>(1)?);
        task.read_only = r.get(2)?;
        let parent_uid: Option<String> = r.get(3)?;
        let uid: String = r.get(4)?;
        let calendar: String = r.get(5)?;
        Ok((task, parent_uid, uid, calendar))
    })?;
    let rows: Vec<_> = rows.collect::<rusqlite::Result<_>>()?;
    let mut parent_stmt =
        conn.prepare_cached("SELECT id FROM pim_tasks WHERE calendar_id = ?1 AND uid = ?2")?;
    let mut child_stmt = conn.prepare_cached(
        "SELECT id FROM pim_tasks WHERE calendar_id = ?1 AND parent_uid = ?2
         ORDER BY sort_order IS NULL, sort_order, due_at IS NULL, due_at, title COLLATE NOCASE, id",
    )?;
    let mut out = Vec::with_capacity(rows.len());
    for (mut task, parent_uid, uid, calendar) in rows {
        task.fields.parent_id = match parent_uid {
            Some(p) => parent_stmt
                .query_row(params![calendar, p], |r| r.get::<_, String>(0))
                .optional()?
                .map(TaskId),
            None => None,
        };
        task.subtask_ids = child_stmt
            .query_map(params![calendar, uid], |r| r.get::<_, String>(0))?
            .map(|r| r.map(TaskId))
            .collect::<rusqlite::Result<_>>()?;
        out.push(task);
    }
    Ok(out)
}

/// Group ids of a contact: groups of its book listing its `UID` or one of
/// its addresses.
fn group_ids_of(conn: &Connection, contact: &Contact) -> Result<Vec<ContactGroupId>> {
    let mut keys = vec![member_key(&contact.uid)];
    keys.extend(
        contact
            .fields
            .emails
            .iter()
            .map(|e| member_key(&format!("mailto:{}", e.value))),
    );
    let mut args: Vec<&str> = vec![contact.address_book_id.as_str()];
    args.extend(keys.iter().map(String::as_str));
    let mut stmt = conn.prepare(&format!(
        "SELECT DISTINCT g.id, g.sort_name FROM pim_group_members m
         JOIN pim_contacts g ON g.id = m.group_id
         WHERE g.address_book_id = ?1 AND {}
         ORDER BY g.sort_name, g.id",
        in_list("m.member", keys.len(), 2)
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
        r.get::<_, String>(0).map(ContactGroupId)
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Linked contacts: a primary's linked contacts, or a linked contact's
/// primary.
pub(crate) fn linked_ids_of(conn: &Connection, id: &str) -> Result<Vec<ContactId>> {
    let mut stmt = conn.prepare_cached(
        "SELECT contact_id FROM pim_contact_links WHERE primary_id = ?1
         UNION SELECT primary_id FROM pim_contact_links WHERE contact_id = ?1
         ORDER BY 1",
    )?;
    let rows = stmt.query_map([id], |r| r.get::<_, String>(0).map(ContactId))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn load_contacts(
    conn: &Connection,
    filter: &str,
    args: &[&str],
) -> Result<Vec<Contact>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT nail_open('pim_contacts.data', p.data), o.sync, c.read_only
         FROM pim_contacts p
         JOIN pim_objects o ON o.id = p.object_id
         JOIN pim_collections c ON c.id = p.address_book_id
         WHERE {filter} ORDER BY p.sort_name, p.id"
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
        let mut contact: Contact =
            serde_json::from_str(&r.get::<_, String>(0)?).map_err(|e| json_err(0, e))?;
        contact.sync = parse_sync(&r.get::<_, String>(1)?);
        contact.read_only = r.get(2)?;
        Ok(contact)
    })?;
    let mut out: Vec<Contact> = rows.collect::<rusqlite::Result<_>>()?;
    for contact in &mut out {
        contact.group_ids = group_ids_of(conn, contact)?;
        contact.linked_ids = linked_ids_of(conn, contact.id.as_str())?;
    }
    Ok(out)
}

fn load_groups(conn: &Connection, filter: &str, args: &[&str]) -> Result<Vec<ContactGroup>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT nail_open('pim_contacts.data', p.data), o.sync, c.read_only
         FROM pim_contacts p
         JOIN pim_objects o ON o.id = p.object_id
         JOIN pim_collections c ON c.id = p.address_book_id
         WHERE p.kind = 'group' AND {filter} ORDER BY p.sort_name, p.id"
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
        let mut group: ContactGroup =
            serde_json::from_str(&r.get::<_, String>(0)?).map_err(|e| json_err(0, e))?;
        group.sync = parse_sync(&r.get::<_, String>(1)?);
        group.read_only = r.get(2)?;
        Ok(group)
    })?;
    let mut out: Vec<ContactGroup> = rows.collect::<rusqlite::Result<_>>()?;
    let mut members_stmt = conn.prepare_cached(
        "SELECT member FROM pim_group_members WHERE group_id = ?1 ORDER BY member",
    )?;
    let mut contact_stmt = conn.prepare_cached(
        "SELECT id FROM pim_contacts WHERE address_book_id = ?1 AND kind <> 'group'
             AND (lower(uid) = ?2 OR lower(uid) = 'urn:uuid:' || ?2)
         ORDER BY id LIMIT 1",
    )?;
    for group in &mut out {
        let members: Vec<String> = members_stmt
            .query_map([group.id.as_str()], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        group.member_ids.clear();
        group.member_emails.clear();
        for member in members {
            if let Some(email) = member.strip_prefix("mailto:") {
                group.member_emails.push(email.to_owned());
            } else if let Some(id) = contact_stmt
                .query_row(params![group.address_book_id.as_str(), member], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
            {
                group.member_ids.push(ContactId(id));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pim::testing::{self, BOOK, CAL, DAV_BOOK, DAV_CAL, ICS_CAL, raw};
    use crate::pim::{PimOperation, item_ref};
    use nail_api::pim::{PimTime, TaskStatus};
    use pretty_assertions::assert_eq;

    fn dav_event(id: &str, uid: &str, title: &str) -> CalendarEvent {
        let mut e = testing::event(id, uid, title);
        e.calendar_id = DAV_CAL.into();
        e
    }

    #[test]
    fn local_items_are_saved_without_queueing() {
        let store = testing::store();
        let event = testing::event("ev1", "u1", "Standup");
        let write = store
            .save_pim_item(&PimItem::Event(event.clone()), &raw("e1"), 10)
            .unwrap();
        assert!(write.op_ids.is_empty());
        let stored = store.event(&"ev1".into()).unwrap().unwrap();
        assert_eq!(stored, event);
        let object = store.pim_object(&write.item).unwrap().unwrap();
        assert_eq!(object.raw, raw("e1"));
        assert_eq!(object.sync, PimItemSync::Synced);
        // Edit, then delete.
        let mut edited = event.clone();
        edited.fields.title = "Daily".into();
        store
            .save_pim_item(&PimItem::Event(edited.clone()), &raw("e2"), 11)
            .unwrap();
        assert_eq!(store.event(&"ev1".into()).unwrap().unwrap(), edited);
        assert_eq!(store.events(Some(&[CAL.into()])).unwrap().len(), 1);
        let deleted = store.delete_pim_item(&write.item, 12).unwrap();
        assert!(deleted.op_ids.is_empty());
        assert!(store.event(&"ev1".into()).unwrap().is_none());
        assert!(store.pim_object(&write.item).unwrap().is_none());
        assert!(store.delete_pim_item(&write.item, 13).is_err());
    }

    #[test]
    fn writes_are_checked() {
        let store = testing::store();
        let mut event = testing::event("ev1", "u1", "X");
        event.calendar_id = ICS_CAL.into();
        assert!(matches!(
            store.save_pim_item(&PimItem::Event(event.clone()), &raw("x"), 1),
            Err(Error::Conflict(_))
        ));
        event.calendar_id = BOOK.into();
        assert!(matches!(
            store.save_pim_item(&PimItem::Event(event.clone()), &raw("x"), 1),
            Err(Error::InvalidData(_))
        ));
        event.calendar_id = "nowhere".into();
        assert!(matches!(
            store.save_pim_item(&PimItem::Event(event.clone()), &raw("x"), 1),
            Err(Error::NotFound { .. })
        ));
        // A UID belongs to one item; an id to one resource.
        event.calendar_id = CAL.into();
        store
            .save_pim_item(&PimItem::Event(event.clone()), &raw("x"), 1)
            .unwrap();
        let mut other = event.clone();
        other.id = "ev2".into();
        assert!(
            store
                .save_pim_item(&PimItem::Event(other), &raw("x"), 1)
                .is_err()
        );
        let mut same_id = event.clone();
        same_id.uid = "u2".into();
        assert!(
            store
                .save_pim_item(&PimItem::Event(same_id), &raw("x"), 1)
                .is_err()
        );
    }

    #[test]
    fn server_items_queue_their_writes() {
        let store = testing::store();
        let event = dav_event("ev1", "u1", "Review");
        let write = store
            .save_pim_item(&PimItem::Event(event.clone()), &raw("v1"), 10)
            .unwrap();
        assert_eq!(write.op_ids.len(), 1);
        let op = store.pim_op(write.op_ids[0]).unwrap();
        assert_eq!(
            op.operation,
            PimOperation::Put {
                object_id: write.object_id
            }
        );
        assert_eq!(op.source_id.as_str(), testing::DAV);
        assert_eq!(
            store.event(&"ev1".into()).unwrap().unwrap().sync,
            PimItemSync::Pending
        );
        // A second edit before the upload reuses the queued upload.
        let again = store
            .save_pim_item(&PimItem::Event(event.clone()), &raw("v2"), 11)
            .unwrap();
        assert_eq!(again.op_ids, write.op_ids);
        // Deleting an item that never reached the server drops everything.
        let deleted = store.delete_pim_item(&write.item, 12).unwrap();
        assert!(deleted.op_ids.is_empty());
        assert!(store.pim_ops(None).unwrap().is_empty());
        assert!(store.pim_object(&write.item).unwrap().is_none());
    }

    #[test]
    fn deleting_an_uploaded_item_queues_a_delete() {
        let store = testing::store();
        let event = dav_event("ev1", "u1", "Review");
        store
            .apply_server_item(
                &PimItem::Event(event.clone()),
                &raw("s1"),
                Some("/cal/u1.ics"),
                Some("e1"),
                5,
            )
            .unwrap();
        let reference = item_ref(PimItemKind::Event, "ev1");
        let write = store.delete_pim_item(&reference, 6).unwrap();
        assert_eq!(write.op_ids.len(), 1);
        assert_eq!(
            store.pim_op(write.op_ids[0]).unwrap().operation,
            PimOperation::Delete {
                object_id: Some(write.object_id),
                collection_id: DAV_CAL.into(),
                href: Some("/cal/u1.ics".into()),
                etag: Some("e1".into())
            }
        );
        // Hidden at once, the resource stays until the server confirmed.
        assert!(store.event(&"ev1".into()).unwrap().is_none());
        let object = store.pim_object_by_id(write.object_id).unwrap().unwrap();
        assert!(object.deleted);
        assert_eq!(object.sync, PimItemSync::Pending);
    }

    #[test]
    fn server_versions_merge() {
        let store = testing::store();
        let event = dav_event("srv-1", "u1", "From server");
        let item = PimItem::Event(event.clone());
        let href = Some("/cal/u1.ics");
        assert_eq!(
            store
                .apply_server_item(&item, &raw("s1"), href, Some("e1"), 1)
                .unwrap(),
            ServerApply::Inserted(item.item_ref())
        );
        // The same version again changes nothing.
        let unchanged = store
            .apply_server_item(&item, &raw("s1"), href, Some("e1"), 2)
            .unwrap();
        assert!(!unchanged.changed());
        // A new server version under a fresh id keeps the stored id.
        let mut changed = event.clone();
        changed.id = "srv-2".into();
        changed.fields.title = "Renamed".into();
        let applied = store
            .apply_server_item(&PimItem::Event(changed), &raw("s2"), href, Some("e2"), 3)
            .unwrap();
        assert_eq!(applied, ServerApply::Updated(item.item_ref()));
        assert_eq!(applied.item(), &item.item_ref());
        let stored = store.event(&"srv-1".into()).unwrap().unwrap();
        assert_eq!(stored.fields.title, "Renamed");
        assert!(store.event(&"srv-2".into()).unwrap().is_none());
        // A local edit on top of e2 stays while the server still says e2.
        let mut local = stored.clone();
        local.fields.title = "Local".into();
        store
            .save_pim_item(&PimItem::Event(local.clone()), &raw("l1"), 4)
            .unwrap();
        assert_eq!(
            store
                .apply_server_item(&item, &raw("s2"), href, Some("e2"), 5)
                .unwrap(),
            ServerApply::KeptLocal(item.item_ref())
        );
        assert_eq!(
            store.event(&"srv-1".into()).unwrap().unwrap().fields.title,
            "Local"
        );
        // The server changed too: it wins, the local version is kept.
        let mut server = event.clone();
        server.fields.title = "Server".into();
        assert_eq!(
            store
                .apply_server_item(&PimItem::Event(server), &raw("s3"), href, Some("e3"), 6)
                .unwrap(),
            ServerApply::Conflict(item.item_ref())
        );
        let stored = store.event(&"srv-1".into()).unwrap().unwrap();
        assert_eq!(stored.fields.title, "Server");
        assert_eq!(stored.sync, PimItemSync::Conflict);
        let object = store.pim_object(&item.item_ref()).unwrap().unwrap();
        assert_eq!(object.raw, raw("s3"));
        assert_eq!(object.conflict_raw.as_deref(), Some(raw("l1").as_str()));
        // The queued upload was dropped.
        let ops = store.pim_ops(None).unwrap();
        assert!(ops.iter().all(|o| o.state != crate::OpState::Pending));
        // Resolving for the server drops the copy.
        assert!(
            store
                .resolve_conflict_keep_server(&item.item_ref())
                .unwrap()
        );
        let object = store.pim_object(&item.item_ref()).unwrap().unwrap();
        assert_eq!(object.conflict_raw, None);
        assert_eq!(object.sync, PimItemSync::Synced);
        assert!(
            !store
                .resolve_conflict_keep_server(&item.item_ref())
                .unwrap()
        );
    }

    #[test]
    fn keeping_the_local_copy_is_a_save() {
        let store = testing::store();
        let event = dav_event("ev", "u", "Base");
        let item = PimItem::Event(event.clone());
        store
            .apply_server_item(&item, &raw("s1"), Some("/cal/u.ics"), Some("e1"), 1)
            .unwrap();
        let mut local = event.clone();
        local.fields.title = "Mine".into();
        store
            .save_pim_item(&PimItem::Event(local.clone()), &raw("mine"), 2)
            .unwrap();
        store
            .apply_server_item(&item, &raw("s2"), Some("/cal/u.ics"), Some("e2"), 3)
            .unwrap();
        // The UI chose "keep mine": the core re-parses the copy and saves it.
        let copy = store
            .pim_object(&item.item_ref())
            .unwrap()
            .unwrap()
            .conflict_raw
            .unwrap();
        let write = store
            .save_pim_item(&PimItem::Event(local), &copy, 4)
            .unwrap();
        assert_eq!(write.op_ids.len(), 1);
        let object = store.pim_object(&item.item_ref()).unwrap().unwrap();
        assert_eq!(object.raw, raw("mine"));
        assert_eq!(object.conflict_raw, None);
        assert_eq!(object.sync, PimItemSync::Pending);
        assert_eq!(object.etag.as_deref(), Some("e2"));
    }

    #[test]
    fn server_deletes() {
        let store = testing::store();
        let a = PimItem::Event(dav_event("a", "ua", "A"));
        let b = PimItem::Event(dav_event("b", "ub", "B"));
        store
            .apply_server_item(&a, &raw("a"), Some("/cal/a.ics"), Some("1"), 1)
            .unwrap();
        store
            .apply_server_item(&b, &raw("b"), Some("/cal/b.ics"), Some("1"), 1)
            .unwrap();
        assert_eq!(store.server_objects(DAV_CAL).unwrap().len(), 2);
        assert_eq!(
            store
                .remove_server_item(DAV_CAL, Some("/cal/a.ics"), None)
                .unwrap(),
            ServerRemove::Removed(a.item_ref())
        );
        assert_eq!(
            store
                .remove_server_item(DAV_CAL, Some("/cal/a.ics"), None)
                .unwrap(),
            ServerRemove::Missing
        );
        // A locally edited item survives as a conflict copy.
        store.save_pim_item(&b, &raw("b-local"), 2).unwrap();
        assert_eq!(
            store.remove_server_item(DAV_CAL, None, Some("ub")).unwrap(),
            ServerRemove::KeptAsConflict(b.item_ref())
        );
        let object = store.pim_object(&b.item_ref()).unwrap().unwrap();
        assert_eq!(object.href, None);
        assert_eq!(object.sync, PimItemSync::Conflict);
        // "Keep the server's" = it is gone there.
        assert!(store.resolve_conflict_keep_server(&b.item_ref()).unwrap());
        assert!(store.pim_object(&b.item_ref()).unwrap().is_none());
    }

    #[test]
    fn a_server_change_undoes_a_local_delete() {
        let store = testing::store();
        let a = PimItem::Event(dav_event("a", "ua", "A"));
        store
            .apply_server_item(&a, &raw("a1"), Some("/cal/a.ics"), Some("1"), 1)
            .unwrap();
        store.delete_pim_item(&a.item_ref(), 2).unwrap();
        assert_eq!(
            store
                .apply_server_item(&a, &raw("a2"), Some("/cal/a.ics"), Some("2"), 3)
                .unwrap(),
            ServerApply::Updated(a.item_ref())
        );
        assert!(store.event(&"a".into()).unwrap().is_some());
        assert!(
            store
                .pim_ops(Some(crate::OpState::Pending))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn ics_feeds_are_retained_by_uid() {
        let store = testing::store();
        for (id, uid) in [("h1", "x1"), ("h2", "x2"), ("h3", "x3")] {
            let mut e = testing::event(id, uid, id);
            e.calendar_id = ICS_CAL.into();
            store
                .apply_server_item(&PimItem::Event(e), &raw(id), None, None, 1)
                .unwrap();
        }
        let removed = store
            .retain_server_uids(ICS_CAL, &["x2".to_owned()])
            .unwrap();
        assert_eq!(removed.len(), 2);
        let left: Vec<String> = store
            .events(Some(&[ICS_CAL.into()]))
            .unwrap()
            .into_iter()
            .map(|e| e.id.0)
            .collect();
        assert_eq!(left, ["h2"]);
        // Feed items are read-only for the user.
        let e = store.event(&"h2".into()).unwrap().unwrap();
        assert!(e.read_only);
        assert!(
            store
                .delete_pim_item(&item_ref(PimItemKind::Event, "h2"), 2)
                .is_err()
        );
    }

    #[test]
    fn forgotten_server_versions_are_rewritten() {
        let store = testing::store();
        let mut e = testing::event("h1", "x1", "Plain");
        e.calendar_id = ICS_CAL.into();
        let item = PimItem::Event(e.clone());
        store
            .apply_server_item(&item, &raw("h1"), None, None, 1)
            .unwrap();
        // The same text again: nothing to do.
        let mut with_alarm = e.clone();
        with_alarm
            .fields
            .alarms
            .push(nail_api::pim::Alarm::minutes_before(10));
        let changed = PimItem::Event(with_alarm);
        assert!(matches!(
            store
                .apply_server_item(&changed, &raw("h1"), None, None, 2)
                .unwrap(),
            ServerApply::Unchanged(_)
        ));
        assert_eq!(store.forget_server_versions(ICS_CAL).unwrap(), 1);
        assert!(
            store
                .apply_server_item(&changed, &raw("h1"), None, None, 3)
                .unwrap()
                .changed()
        );
        let stored = store.event(&"h1".into()).unwrap().unwrap();
        assert_eq!(stored.fields.alarms.len(), 1);
        let object = store.pim_object(&changed.item_ref()).unwrap().unwrap();
        assert_eq!(object.raw, raw("h1"));
    }

    #[test]
    fn moving_between_sources() {
        let store = testing::store();
        let c = testing::contact("c1", "uc1", "Ada", &["ada@example.org"]);
        store
            .save_pim_item(&PimItem::Contact(c.clone()), &raw("c"), 1)
            .unwrap();
        let reference = item_ref(PimItemKind::Contact, "c1");
        let moved = store.move_pim_item(&reference, DAV_BOOK, 2).unwrap();
        assert_eq!(moved.op_ids.len(), 1);
        let stored = store.contact(&"c1".into()).unwrap().unwrap();
        assert_eq!(stored.address_book_id.as_str(), DAV_BOOK);
        assert_eq!(stored.sync, PimItemSync::Pending);
        // Uploaded there, then moved back: a delete at the server.
        store
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE pim_objects SET href = '/card/c1.vcf', etag = 'x'",
                    [],
                )?;
                conn.execute("UPDATE pim_ops SET state = 'done'", [])?;
                Ok(())
            })
            .unwrap();
        let back = store.move_pim_item(&reference, BOOK, 3).unwrap();
        assert_eq!(back.op_ids.len(), 1);
        assert!(matches!(
            store.pim_op(back.op_ids[0]).unwrap().operation,
            PimOperation::Delete {
                object_id: None,
                ..
            }
        ));
        let object = store.pim_object(&reference).unwrap().unwrap();
        assert_eq!(object.collection_id, BOOK);
        assert_eq!(object.href, None);
        assert_eq!(object.sync, PimItemSync::Synced);
        // Same place: nothing to do; wrong kind of collection: refused.
        assert!(
            store
                .move_pim_item(&reference, BOOK, 4)
                .unwrap()
                .op_ids
                .is_empty()
        );
        assert!(store.move_pim_item(&reference, CAL, 4).is_err());
    }

    #[test]
    fn tasks_with_subtasks() {
        let store = testing::store();
        let parent = testing::task("t1", "ut1", "Plan trip");
        let mut child_b = testing::task("t3", "ut3", "Book hotel");
        child_b.fields.parent_id = Some("t1".into());
        child_b.fields.order = Some(2);
        let mut child_a = testing::task("t2", "ut2", "Book train");
        child_a.fields.order = Some(1);
        let mut done = testing::task("t4", "ut4", "Old");
        done.fields.status = TaskStatus::Completed;
        store
            .save_pim_item(&PimItem::task(parent), &raw("p"), 1)
            .unwrap();
        store
            .save_pim_item(&PimItem::task(child_b), &raw("b"), 1)
            .unwrap();
        // Parent given by UID (as parsed from RELATED-TO).
        store
            .save_pim_item(
                &PimItem::Task {
                    task: child_a,
                    parent_uid: Some("ut1".into()),
                },
                &raw("a"),
                1,
            )
            .unwrap();
        store
            .save_pim_item(&PimItem::task(done), &raw("d"), 1)
            .unwrap();
        let parent = store.task(&"t1".into()).unwrap().unwrap();
        assert_eq!(parent.subtask_ids, [TaskId::new("t2"), TaskId::new("t3")]);
        assert_eq!(
            store.task(&"t2".into()).unwrap().unwrap().fields.parent_id,
            Some("t1".into())
        );
        let open: Vec<String> = store
            .list_tasks(None, false)
            .unwrap()
            .into_iter()
            .map(|t| t.id.0)
            .collect();
        assert_eq!(open, ["t2", "t3", "t1"]);
        assert_eq!(
            store.list_tasks(Some(&[CAL.into()]), true).unwrap().len(),
            4
        );
        // A task cannot be its own parent.
        let mut own = testing::task("t5", "ut5", "Loop");
        own.fields.parent_id = None;
        assert!(
            store
                .save_pim_item(
                    &PimItem::Task {
                        task: own,
                        parent_uid: Some("ut5".into())
                    },
                    &raw("l"),
                    1
                )
                .is_err()
        );
    }

    #[test]
    fn task_due_columns() {
        let store = testing::store();
        let mut t = testing::task("t1", "u1", "Due");
        t.fields.due = Some(PimTime::date("2026-10-05"));
        t.fields.completed_at = Some("2026-10-04T10:00:00Z".into());
        store
            .save_pim_item(&PimItem::task(t), &raw("t"), 1)
            .unwrap();
        let (due, completed): (Option<i64>, Option<i64>) = store
            .with_conn(|c| {
                Ok(c.query_row(
                    "SELECT due_at, completed_at FROM pim_tasks WHERE id = 't1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(due, Some(testing::at("2026-10-05T00:00:00Z")));
        assert_eq!(completed, Some(testing::at("2026-10-04T10:00:00Z")));
    }

    #[test]
    fn groups_resolve_members() {
        let store = testing::store();
        let ada = testing::contact("c1", "urn:uuid:AAA", "Ada", &["ada@example.org"]);
        let bob = testing::contact("c2", "bbb", "Bob", &["bob@example.org"]);
        store
            .save_pim_item(&PimItem::Contact(ada), &raw("a"), 1)
            .unwrap();
        store
            .save_pim_item(&PimItem::Contact(bob), &raw("b"), 1)
            .unwrap();
        let mut team = testing::group("g1", "ug", "Team", &["c1"]);
        team.member_emails = vec!["Bob@Example.org".into(), "eve@example.org".into()];
        store
            .save_pim_item(&PimItem::group(team), &raw("g"), 1)
            .unwrap();
        let group = store.contact_group(&"g1".into()).unwrap().unwrap();
        assert_eq!(group.member_ids, [ContactId::new("c1")]);
        assert_eq!(group.member_emails, ["bob@example.org", "eve@example.org"]);
        // Contacts know their groups (by UID or by address).
        assert_eq!(
            store.contact(&"c1".into()).unwrap().unwrap().group_ids,
            [ContactGroupId::new("g1")]
        );
        assert_eq!(
            store.contact(&"c2".into()).unwrap().unwrap().group_ids,
            [ContactGroupId::new("g1")]
        );
        // MEMBER values from a server card.
        let server = testing::group("g2", "ug2", "Server", &[]);
        store
            .save_pim_item(
                &PimItem::Group {
                    group: server,
                    members: vec!["urn:uuid:bbb".into(), "urn:uuid:unknown".into()],
                },
                &raw("g2"),
                1,
            )
            .unwrap();
        let group = store.contact_group(&"g2".into()).unwrap().unwrap();
        assert_eq!(group.member_ids, [ContactId::new("c2")]);
        assert_eq!(store.contact_groups(Some(&[BOOK.into()])).unwrap().len(), 2);
        // Unknown member ids are refused.
        let bad = testing::group("g3", "ug3", "Bad", &["nobody"]);
        assert!(
            store
                .save_pim_item(&PimItem::group(bad), &raw("g3"), 1)
                .is_err()
        );
        // Groups are not contacts.
        assert!(store.contact(&"g1".into()).unwrap().is_none());
        assert_eq!(store.contacts(None).unwrap().len(), 2);
    }

    #[test]
    fn search_hits_and_spans() {
        let store = testing::store();
        let mut e = testing::event("e1", "u1", "Lunch");
        e.fields.location = Some("Canteen".into());
        store
            .save_pim_item(&PimItem::Event(e), &raw("e"), 1)
            .unwrap();
        let c = testing::contact("c1", "uc", "Ada", &["ada@example.org"]);
        store
            .save_pim_item(&PimItem::Contact(c), &raw("c"), 1)
            .unwrap();
        let hits = store
            .pim_search_hits(&[
                item_ref(PimItemKind::Contact, "c1"),
                item_ref(PimItemKind::Event, "missing"),
                item_ref(PimItemKind::Event, "e1"),
            ])
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].detail.as_deref(), Some("ada@example.org"));
        assert_eq!(hits[1].detail.as_deref(), Some("Canteen"));
        // Range columns of a recurring series.
        let mut series = testing::event("s", "us", "Daily");
        series.recurrence = Some(testing::daily(nail_api::pim::RecurrenceEnd::Count {
            count: 3,
        }));
        assert_eq!(event_span(&series).1, None);
        series.recurrence = Some(testing::daily(nail_api::pim::RecurrenceEnd::Until {
            until: PimTime::utc("2026-10-05T07:00:00"),
        }));
        assert_eq!(
            event_span(&series).1,
            Some(testing::at("2026-10-05T08:00:00Z"))
        );
    }

    #[test]
    fn sealed_at_rest() {
        let store = testing::store();
        store
            .set_cipher(
                Some(std::sync::Arc::new(crate::seal::tests::XorCipher)),
                crate::SealMode::Seal,
            )
            .unwrap();
        let c = testing::contact("c1", "uc", "Ada", &["ada@example.org"]);
        store
            .save_pim_item(&PimItem::Contact(c.clone()), &raw("secret"), 1)
            .unwrap();
        let (data, raw_text): (String, String) = store
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT p.data, o.raw FROM pim_contacts p JOIN pim_objects o ON o.id = p.object_id",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert!(data.starts_with(crate::SEALED_FIELD_PREFIX));
        assert!(raw_text.starts_with(crate::SEALED_FIELD_PREFIX));
        assert_eq!(
            store.contact(&"c1".into()).unwrap().unwrap().fields,
            c.fields
        );
        assert_eq!(
            store
                .pim_object(&item_ref(PimItemKind::Contact, "c1"))
                .unwrap()
                .unwrap()
                .raw,
            raw("secret")
        );
    }

    #[test]
    fn member_keys() {
        assert_eq!(member_key("urn:uuid:ABC"), "abc");
        assert_eq!(
            member_key(" MAILTO:Ada@Example.org "),
            "mailto:ada@example.org"
        );
        assert_eq!(member_key("plain-UID"), "plain-uid");
    }
}
