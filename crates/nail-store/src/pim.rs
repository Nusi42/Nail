//! Calendars, contacts and tasks in the store (M3a; contract ADR 0058).
//!
//! The tables are created by schema v10 (`schema.rs`): sources,
//! collections, the raw iCalendar / vCard resources (`pim_objects`, sealed
//! at rest), the parsed events / tasks / contacts, groups, links of merged
//! contacts, the instance cache, reminders and the PIM operation queue.
//!
//! Slot of WP73: the store APIs for every PIM table (items, instance cache
//! range queries, reminders, the queue with sync-side merge, conflict
//! copies, duplicate detection) live here. The contract stage only names
//! the built-in local source.

use crate::error::Result;
use crate::store::Store;

/// Id of the built-in local source row (inserted by the migration).
pub const LOCAL_SOURCE_ID: &str = nail_api::PimSourceId::LOCAL;

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
}
