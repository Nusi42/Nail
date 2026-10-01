//! Calendars, contacts and tasks in the store (M3a; contract ADR 0058,
//! store APIs ADR 0061).
//!
//! The tables are created by schema v10 (`schema.rs`): sources,
//! collections, the raw iCalendar / vCard resources (`pim_objects`, sealed
//! at rest), the parsed events / tasks / contacts, groups, links of merged
//! contacts, the instance cache, reminders and the PIM operation queue.
//!
//! The store does not parse iCalendar or vCard (that is `nail-ical`): the
//! core hands it each item twice — parsed ([`PimItem`], the `nail_api`
//! types) and as the raw text it was parsed from or written to — and the
//! store keeps both in step.
//!
//! * [`collections`] — sources ([`Store::ensure_pim_source`]), calendars
//!   and address books, sync tokens, the local "Collected" book.
//! * [`items`] — user writes ([`Store::save_pim_item`],
//!   [`Store::delete_pim_item`], [`Store::move_pim_item`]) that change the
//!   tables **and** queue the server write in one transaction; the sync
//!   side ([`Store::apply_server_item`], [`Store::remove_server_item`])
//!   with "server wins, the local version is kept as a conflict copy";
//!   reads and lists.
//! * [`ops`] — the PIM operation queue and its replay
//!   ([`Store::replay_pim_source`]).
//! * [`instances`] — the instance cache: occurrences for a range in a
//!   viewer's zone ([`Store::list_event_instances`]), expanded on demand
//!   by an [`InstanceExpander`] (`nail-ical`) and invalidated on edits and
//!   zone changes.
//! * [`contacts`] — duplicate detection, merging with links to the
//!   sources' cards, the Collected book and address autocomplete ranking
//!   ([`Store::suggest_recipients`]).
//! * [`reminders`] — alarm occurrences and their state for the reminder
//!   scheduler in `nail-triage`.
//! * [`time`] — calendar times as cache seconds.

pub mod collections;
pub mod contacts;
pub mod instances;
pub mod items;
pub mod ops;
pub mod reminders;
pub mod time;

use nail_api::pim::{CalendarEvent, Contact, ContactGroup, PimItemKind, PimItemRef, Task};

use crate::error::Result;
use crate::store::Store;

pub use collections::{
    COLLECTED_BOOK_ID, COLLECTED_BOOK_NAME_ID, CollectionKind, CollectionSync, PimSourceRecord,
};
pub use contacts::{
    MergeOutcome, RankedAddress, SuggestionSource, contact_summary, merge_contact_fields,
    normalize_phone,
};
pub use instances::{
    ExpandedInstance, INSTANCE_CHUNK, InstanceExpander, InstanceScope, free_slots,
};
pub use items::{PimObject, PimWrite, ServerApply, ServerObjectInfo, ServerRemove, member_key};
pub use ops::{PimOpId, PimOperation, PimReplayOutcome, PimReplayReport, PimReplayer, QueuedPimOp};
pub use reminders::{PlannedReminder, ReminderItemKind, ReminderRecord, reminder_id};
pub use time::{CacheTime, cache_time};

/// Id of the built-in local source row (inserted by the migration).
pub const LOCAL_SOURCE_ID: &str = nail_api::PimSourceId::LOCAL;

/// One parsed PIM item as the core hands it to the store.
#[derive(Debug, Clone, PartialEq)]
pub enum PimItem {
    /// An event series.
    Event(CalendarEvent),
    /// A task.
    Task {
        /// The task. `fields.parent_id` and `subtask_ids` are recomputed
        /// from `parent_uid` when read.
        task: Task,
        /// The parent's iCalendar `UID` (`RELATED-TO;RELTYPE=PARENT`);
        /// `None` = taken from `task.fields.parent_id`.
        parent_uid: Option<String>,
    },
    /// A contact. `group_ids` and `linked_ids` are recomputed when read.
    Contact(Contact),
    /// A contact group.
    Group {
        /// The group. `member_ids` / `member_emails` are recomputed from
        /// `members` when read.
        group: ContactGroup,
        /// The vCard `MEMBER` values (`urn:uuid:…`, `mailto:…`); empty =
        /// taken from `group.member_ids` / `member_emails`.
        members: Vec<String>,
    },
}

impl PimItem {
    /// A task whose parent is `task.fields.parent_id`.
    pub fn task(task: Task) -> Self {
        Self::Task {
            task,
            parent_uid: None,
        }
    }

    /// A group whose members are `group.member_ids` / `member_emails`.
    pub fn group(group: ContactGroup) -> Self {
        Self::Group {
            group,
            members: Vec::new(),
        }
    }

    /// The kind of item.
    pub fn kind(&self) -> PimItemKind {
        match self {
            Self::Event(_) => PimItemKind::Event,
            Self::Task { .. } => PimItemKind::Task,
            Self::Contact(_) => PimItemKind::Contact,
            Self::Group { .. } => PimItemKind::ContactGroup,
        }
    }

    /// The calendar or address book.
    pub fn collection_id(&self) -> &str {
        match self {
            Self::Event(e) => e.calendar_id.as_str(),
            Self::Task { task, .. } => task.calendar_id.as_str(),
            Self::Contact(c) => c.address_book_id.as_str(),
            Self::Group { group, .. } => group.address_book_id.as_str(),
        }
    }

    /// The iCalendar / vCard `UID`.
    pub fn uid(&self) -> &str {
        match self {
            Self::Event(e) => &e.uid,
            Self::Task { task, .. } => &task.uid,
            Self::Contact(c) => &c.uid,
            Self::Group { group, .. } => &group.uid,
        }
    }

    /// The item's id.
    pub fn id(&self) -> &str {
        match self {
            Self::Event(e) => e.id.as_str(),
            Self::Task { task, .. } => task.id.as_str(),
            Self::Contact(c) => c.id.as_str(),
            Self::Group { group, .. } => group.id.as_str(),
        }
    }

    /// A reference to the item.
    pub fn item_ref(&self) -> PimItemRef {
        item_ref(self.kind(), self.id())
    }

    pub(crate) fn set_id(&mut self, id: &str) {
        match self {
            Self::Event(e) => e.id = id.into(),
            Self::Task { task, .. } => task.id = id.into(),
            Self::Contact(c) => c.id = id.into(),
            Self::Group { group, .. } => group.id = id.into(),
        }
    }

    pub(crate) fn set_collection(&mut self, collection: &str) {
        match self {
            Self::Event(e) => e.calendar_id = collection.into(),
            Self::Task { task, .. } => task.calendar_id = collection.into(),
            Self::Contact(c) => c.address_book_id = collection.into(),
            Self::Group { group, .. } => group.address_book_id = collection.into(),
        }
    }
}

/// A reference of a kind and an id.
pub(crate) fn item_ref(kind: PimItemKind, id: &str) -> PimItemRef {
    match kind {
        PimItemKind::Event => PimItemRef::Event {
            event_id: id.into(),
        },
        PimItemKind::Task => PimItemRef::Task { task_id: id.into() },
        PimItemKind::Contact => PimItemRef::Contact {
            contact_id: id.into(),
        },
        PimItemKind::ContactGroup => PimItemRef::ContactGroup {
            group_id: id.into(),
        },
    }
}

/// The id of a reference.
pub(crate) fn ref_id(item: &PimItemRef) -> &str {
    match item {
        PimItemRef::Event { event_id } => event_id.as_str(),
        PimItemRef::Task { task_id } => task_id.as_str(),
        PimItemRef::Contact { contact_id } => contact_id.as_str(),
        PimItemRef::ContactGroup { group_id } => group_id.as_str(),
    }
}

impl Store {
    /// Ids of the PIM sources the store knows, the local one included.
    pub fn pim_source_ids(&self) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id FROM pim_sources ORDER BY id")?;
            Ok(stmt
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?)
        })
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_local_source_exists_after_migrating() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.pim_source_ids().unwrap(), [LOCAL_SOURCE_ID]);
    }

    fn count(conn: &rusqlite::Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    /// Schema v10: removing a source removes everything of it; reminders
    /// go with their event or task; the CHECKs hold.
    #[test]
    fn v10_cascades_and_checks() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_conn(|conn| {
                conn.execute_batch(
                    r#"
                    INSERT INTO pim_sources (id, kind, created_at) VALUES ('dav', 'caldav', 0);
                    INSERT INTO pim_collections (id, source_id, kind, href, settings, components)
                        VALUES ('c1', 'dav', 'calendar', '/cal/', '{"name":"Work"}', '["event","task"]');
                    INSERT INTO pim_objects (id, collection_id, uid, kind, raw, updated_at)
                        VALUES (1, 'c1', 'u1', 'event', 'BEGIN:VCALENDAR', 0),
                               (2, 'c1', 'u2', 'task', 'BEGIN:VCALENDAR', 0);
                    INSERT INTO pim_events (id, object_id, calendar_id, uid, first_start, data, updated_at)
                        VALUES ('ev1', 1, 'c1', 'u1', 100, '{}', 0);
                    INSERT INTO pim_event_instances (event_id, calendar_id, recurrence_id, start_at, end_at)
                        VALUES ('ev1', 'c1', '', 100, 200);
                    INSERT INTO pim_tasks (id, object_id, calendar_id, uid, data, updated_at)
                        VALUES ('t1', 2, 'c1', 'u2', '{}', 0);
                    INSERT INTO pim_reminders (id, item_kind, item_id, alarm_key, fire_at, updated_at)
                        VALUES ('r1', 'event', 'ev1', '0', 50, 0), ('r2', 'task', 't1', '0', 60, 0);
                    INSERT INTO pim_ops (source_id, object_id, kind, payload)
                        VALUES ('dav', 1, 'put', '{}');
                    "#,
                )?;
                // A deleted event takes its reminders and instances along.
                conn.execute("DELETE FROM pim_objects WHERE id = 1", [])?;
                assert_eq!(count(conn, "pim_events"), 0);
                assert_eq!(count(conn, "pim_event_instances"), 0);
                assert_eq!(count(conn, "pim_reminders"), 1);
                let op_object: Option<i64> =
                    conn.query_row("SELECT object_id FROM pim_ops", [], |r| r.get(0))?;
                assert_eq!(op_object, None);
                // Bad values are refused.
                assert!(
                    conn.execute(
                        "INSERT INTO pim_sources (id, kind, created_at) VALUES ('x', 'exchange', 0)",
                        []
                    )
                    .is_err()
                );
                assert!(
                    conn.execute(
                        "UPDATE pim_objects SET sync = 'dirty' WHERE id = 2",
                        []
                    )
                    .is_err()
                );
                // Removing the source removes the rest.
                conn.execute("DELETE FROM pim_sources WHERE id = 'dav'", [])?;
                for table in [
                    "pim_collections",
                    "pim_objects",
                    "pim_tasks",
                    "pim_reminders",
                    "pim_ops",
                ] {
                    assert_eq!(count(conn, table), 0, "{table}");
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn pim_content_is_sealed_at_rest() {
        let sealed: Vec<String> = crate::seal::SEALED_COLUMNS
            .iter()
            .filter(|c| c.table.starts_with("pim_"))
            .map(|c| c.name())
            .collect();
        assert_eq!(
            sealed,
            [
                "pim_objects.raw",
                "pim_objects.conflict_raw",
                "pim_events.data",
                "pim_contacts.data",
                "pim_tasks.data"
            ]
        );
    }

    #[test]
    fn items_know_their_kind_collection_and_id() {
        let fixtures = testing::Fixtures::new();
        let event = PimItem::Event(fixtures.event("ev1", "u1", "Standup"));
        assert_eq!(event.kind(), PimItemKind::Event);
        assert_eq!(event.collection_id(), testing::CAL);
        assert_eq!(event.uid(), "u1");
        assert_eq!(
            event.item_ref(),
            PimItemRef::Event {
                event_id: "ev1".into()
            }
        );
        let mut group = PimItem::group(testing::group("g1", "ug", "Team", &[]));
        group.set_id("g2");
        group.set_collection("other");
        assert_eq!(group.id(), "g2");
        assert_eq!(group.collection_id(), "other");
        assert_eq!(ref_id(&group.item_ref()), "g2");
    }
}
