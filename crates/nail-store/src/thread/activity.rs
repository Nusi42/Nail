//! Per-folder thread activity (P3, ADR 0164; slot of WP-perf).
//!
//! `ListThreads` grouped every message of the listed folders by thread for
//! each page (`conversation::PAGE_SQL`: 33 ms p95 on the standard corpus,
//! P2 review). Schema v17 adds `thread_activity`: one row per (folder,
//! thread) with the row's order key `last_date` (the thread's newest
//! message in the folder, or a newer reply of the user in the Sent folders
//! of the folder's account — P2's Sent join), so a page is an index range:
//!
//! * one real folder: [`PAGE_SQL`] over `thread_activity_order`, the total
//!   is [`COUNT_SQL`];
//! * a virtual folder (several member folders): [`PAGE_SQL`] per member with
//!   `LIMIT offset + limit`, merged in Rust by `(last_date DESC, thread_id
//!   DESC)`; a thread in two members counts once with its newer key;
//! * a Drafts folder has no rows: its messages stay rows of their own
//!   (`single-<id>`, `messages_folder_date`).
//!
//! The rows of a page are then read with `conversation::listed_sql` as
//! today (a page is at most `MAX_PAGE_SIZE` conversations).
//!
//! **Invariant** (WP-perf keeps it in schema v18, e.g. with triggers on
//! `messages` insert / delete / update of `folder_id`, `thread_id`,
//! `date_sort`, and on a folder's role change): for every non-Drafts folder
//! `f` and thread `t` with messages in `f`, the row equals what the v17
//! backfill computes; no row exists for a (folder, thread) without
//! messages. A property test compares the table with the backfill query
//! after random inserts, moves, deletes and re-threading.

/// A page of one folder's conversations, newest activity first (`?1`
/// folder, `?2` limit, `?3` offset). Reads `thread_activity_order` only.
pub const PAGE_SQL: &str = "SELECT thread_id, last_date FROM thread_activity
     WHERE folder_id = ?1 ORDER BY last_date DESC, thread_id DESC LIMIT ?2 OFFSET ?3";

/// The number of conversations listed in one folder (`?1`).
pub const COUNT_SQL: &str = "SELECT COUNT(*) FROM thread_activity WHERE folder_id = ?1";

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};

    use super::{COUNT_SQL, PAGE_SQL};
    use crate::schema::{migrate, migrate_to_version};

    /// A v16 database with an Inbox thread answered from Sent, an older
    /// Inbox thread, and a draft.
    fn v16() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        assert_eq!(migrate_to_version(&mut conn, 16).unwrap(), 16);
        conn.execute_batch(
            r"
            INSERT INTO accounts VALUES ('a1', 'A', 'a@example.org', 'imap');
            INSERT INTO folders (id, account_id, name, path, role) VALUES
                ('in', 'a1', 'Inbox', 'INBOX', 'inbox'),
                ('sent', 'a1', 'Sent', 'Sent', 'sent'),
                ('drafts', 'a1', 'Drafts', 'Drafts', 'drafts');
            INSERT INTO messages (id, account_id, folder_id, date, date_sort, thread_id) VALUES
                ('m1', 'a1', 'in', '', 100, 't1'),
                ('m2', 'a1', 'in', '', 200, 't1'),
                ('m3', 'a1', 'sent', '', 300, 't1'),
                ('m4', 'a1', 'in', '', 250, 't2'),
                ('m5', 'a1', 'drafts', '', 400, 't1');
            ",
        )
        .unwrap();
        conn
    }

    #[test]
    fn v17_backfills_the_order_key_with_the_users_replies() {
        let mut conn = v16();
        migrate(&mut conn).unwrap();
        let rows: Vec<(String, String, i64, i64, i64)> = conn
            .prepare(
                "SELECT folder_id, thread_id, folder_last, last_date, message_count
                   FROM thread_activity ORDER BY folder_id, thread_id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("in".into(), "t1".into(), 200, 300, 2),
                ("in".into(), "t2".into(), 250, 250, 1),
                ("sent".into(), "t1".into(), 300, 300, 1),
            ]
        );
        // The reply from Sent puts t1 first in the Inbox.
        let page: Vec<String> = conn
            .prepare(PAGE_SQL)
            .unwrap()
            .query_map(params!["in", 50, 0], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(page, ["t1", "t2"]);
        let total: i64 = conn.query_row(COUNT_SQL, ["in"], |r| r.get(0)).unwrap();
        assert_eq!(total, 2);
    }

    #[test]
    fn a_page_is_an_index_range() {
        let mut conn = v16();
        migrate(&mut conn).unwrap();
        let plan: Vec<String> = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {PAGE_SQL}"))
            .unwrap()
            .query_map(params!["in", 50, 0], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let plan = plan.join("\n");
        assert!(plan.contains("thread_activity_order"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }
}
