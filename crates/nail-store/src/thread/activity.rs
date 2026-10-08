//! Per-folder thread activity (P3, ADR 0164; WP-perf).
//!
//! `ListThreads` grouped every message of the listed folders by thread for
//! each page (`GROUPING_PAGE_SQL`: 33 ms p95 on the standard corpus, P2
//! review). Schema v17 adds `thread_activity`: one row per (folder,
//! thread) with the row's order key `last_date` (the thread's newest
//! message in the folder, or a newer reply of the user in the Sent folders
//! of the folder's account — P2's Sent join), so a page is an index range:
//!
//! * one real folder: [`PAGE_SQL`] over `thread_activity_order`, the total
//!   is [`COUNT_SQL`];
//! * a virtual folder (several member folders): [`PAGE_SQL`] per member with
//!   `LIMIT offset + limit`, merged in Rust by `(last_date DESC, thread_id
//!   DESC)`; a thread in two members counts once with its newer key. Taking
//!   each member's first `offset + limit` rows is enough: a thread whose
//!   newest member row is not among them has at least that many threads
//!   ahead of it;
//! * a Drafts folder has no rows: its messages stay rows of their own
//!   (`single-<id>`, [`SINGLES_PAGE_SQL`] over `messages_folder_date`).
//!
//! The rows of a page are then read with `conversation::listed_sql` as
//! before (a page is at most `MAX_PAGE_SIZE` conversations).
//!
//! # Maintenance (schema v18, `MAINTENANCE_SQL`)
//!
//! **Invariant:** the table equals [`REBUILD_SELECT`] (the v17 backfill)
//! after every committed write. SQLite triggers keep it, so no write path
//! of the store changes and none can forget it:
//!
//! * `messages` insert / delete, and updates of `folder_id`, `thread_id` or
//!   `date_sort` (ingest, moves, deletes, expunges, re-threading, JWZ
//!   merges, cascades from a deleted folder or account) recompute the
//!   (folder, thread) row of the old and the new values from the covering
//!   index `messages_folder_thread`; when the folder is a Sent folder, the
//!   order key of the thread's rows in the folder's account is recomputed
//!   as well (the user's reply moves the Inbox row up);
//! * a folder's role or account change rebuilds its rows and recomputes the
//!   order keys of its account(s); a deleted Sent folder recomputes its
//!   account's order keys (its own rows go with the `ON DELETE CASCADE`).
//!
//! Flag, tag and read changes do not touch the table (it holds no flags).
//! A migration that rebuilds `messages` or `folders` (the "12-step" table
//! change) drops these triggers and must create them again.
//!
//! **Grants:** the order key always counts every Sent folder of the
//! folder's account. A list whose Sent folders were narrowed (a remote
//! grant without Sent) falls back to `GROUPING_PAGE_SQL`, so the order
//! reveals nothing about hidden replies (`covers`).
//!
//! **Deviation from the grouping query:** the grouping query joined the
//! Sent folders of *every* listed account to every listed thread; the table
//! joins the folder's own account's Sent folders only (ADR 0164's
//! definition). The two differ only in a unified list over several accounts
//! for a thread that one account received and another account answered,
//! and then only in the order key (the row's content is unchanged).

use std::collections::{HashMap, HashSet};

use rusqlite::{Transaction, params};

use super::conversation::{ConversationFolders, ScopeFolder};
use crate::error::Result;

/// A page of one folder's conversations, newest activity first (`?1`
/// folder, `?2` limit, `?3` offset). Reads `thread_activity_order` only.
pub const PAGE_SQL: &str = "SELECT thread_id, last_date FROM thread_activity
     WHERE folder_id = ?1 ORDER BY last_date DESC, thread_id DESC LIMIT ?2 OFFSET ?3";

/// The number of conversations listed in one folder (`?1`).
pub const COUNT_SQL: &str = "SELECT COUNT(*) FROM thread_activity WHERE folder_id = ?1";

/// The number of distinct conversations in several folders (`?1`, JSON
/// array of folder ids): a thread in two members counts once.
pub const COUNT_MANY_SQL: &str = "SELECT COUNT(DISTINCT thread_id) FROM thread_activity
     WHERE folder_id IN (SELECT value FROM json_each(?1))";

/// The newest drafts of one Drafts folder as conversations of their own
/// (`?1` folder, `?2` limit), in the order of [`PAGE_SQL`].
pub const SINGLES_PAGE_SQL: &str = "SELECT 'single-' || id, date_sort FROM messages
     WHERE folder_id = ?1 ORDER BY date_sort DESC, id DESC LIMIT ?2";

/// The number of messages in some folders (`?1`, JSON array): the drafts
/// listed as conversations of their own.
pub const SINGLES_COUNT_SQL: &str = "SELECT COUNT(*) FROM messages
     WHERE folder_id IN (SELECT value FROM json_each(?1))";

/// The listed conversations' keys straight from `messages`, newest activity
/// first (`?1` threaded folders, `?2` Sent folders that join, `?3` Drafts
/// folders, `?4` limit, `?5` offset): the P2 query, kept for lists whose
/// Sent folders a grant narrowed (`covers`) and as the test oracle. A
/// Sent message only adds activity to a thread that has a message in the
/// listed folders.
pub(crate) const GROUPING_PAGE_SQL: &str = "SELECT k, MAX(d) AS last FROM (
        SELECT thread_id AS k, date_sort AS d FROM messages
         WHERE folder_id IN (SELECT value FROM json_each(?1)) AND thread_id IS NOT NULL
        UNION ALL
        SELECT thread_id, date_sort FROM messages
         WHERE folder_id IN (SELECT value FROM json_each(?2))
           AND +thread_id IN (SELECT thread_id FROM messages
                               WHERE folder_id IN (SELECT value FROM json_each(?1)))
        UNION ALL
        SELECT 'single-' || id, date_sort FROM messages
         WHERE folder_id IN (SELECT value FROM json_each(?3))
     ) GROUP BY k ORDER BY last DESC, k DESC LIMIT ?4 OFFSET ?5";

/// The number of listed conversations from `messages` (`?1` threaded, `?2`
/// Drafts folders); the count of `GROUPING_PAGE_SQL`.
pub(crate) const GROUPING_COUNT_SQL: &str = "SELECT
        (SELECT COUNT(DISTINCT thread_id) FROM messages
          WHERE folder_id IN (SELECT value FROM json_each(?1)) AND thread_id IS NOT NULL)
      + (SELECT COUNT(*) FROM messages WHERE folder_id IN (SELECT value FROM json_each(?2)))";

/// The rows of `thread_activity` computed from `messages` for the folders
/// that `$where` (a condition on `m`) selects: the v17 backfill.
macro_rules! rows_select {
    ($where:expr) => {
        concat!(
            "SELECT g.folder_id, g.thread_id, g.folder_last,
                    MAX(g.folder_last, COALESCE((
                        SELECT MAX(s.date_sort) FROM messages s
                         WHERE s.thread_id = g.thread_id
                           AND s.folder_id IN (SELECT sf.id FROM folders sf
                                                WHERE sf.account_id = g.account_id
                                                  AND sf.role = 'sent')
                    ), 0)),
                    g.message_count
               FROM (SELECT m.folder_id, m.thread_id, f.account_id,
                            MAX(m.date_sort) AS folder_last, COUNT(*) AS message_count
                       FROM messages m JOIN folders f ON f.id = m.folder_id
                      WHERE ",
            $where,
            " AND f.role <> 'drafts'
                      GROUP BY m.folder_id, m.thread_id) g"
        )
    };
}

/// Recomputes the row of one (folder, thread) — `$r` is `OLD` or `NEW` of a
/// `messages` trigger — from `messages_folder_thread`. A NULL thread
/// matches nothing; a Drafts folder (or a deleted one) gets no row.
macro_rules! refresh_row {
    ($r:literal) => {
        concat!(
            "DELETE FROM thread_activity WHERE folder_id = ",
            $r,
            ".folder_id AND thread_id = ",
            $r,
            ".thread_id;
             INSERT INTO thread_activity (folder_id, thread_id, folder_last, last_date, message_count) ",
            rows_select!(concat!(
                "m.folder_id = ",
                $r,
                ".folder_id AND m.thread_id = ",
                $r,
                ".thread_id"
            )),
            ";"
        )
    };
}

/// The order-key update of `thread_activity` rows up to the Sent folders'
/// account (`sent_order_update!()`, the account expression, then
/// `sent_order_where!()` and the rows' condition).
macro_rules! sent_order_update {
    () => {
        "UPDATE thread_activity SET last_date = MAX(folder_last, COALESCE((
                 SELECT MAX(s.date_sort) FROM messages s
                  WHERE s.thread_id = thread_activity.thread_id
                    AND s.folder_id IN (SELECT sf.id FROM folders sf
                                         WHERE sf.role = 'sent' AND sf.account_id ="
    };
}

/// See `sent_order_update!`.
macro_rules! sent_order_where {
    () => {
        "
             ), 0))
             WHERE "
    };
}

/// When `$r`'s folder is a Sent folder: recomputes the order key of the
/// thread's rows in every folder of that account.
macro_rules! refresh_sent {
    ($r:literal) => {
        concat!(
            sent_order_update!(),
            "
                                               (SELECT account_id FROM folders WHERE id = ",
            $r,
            ".folder_id))",
            sent_order_where!(),
            "thread_id = ",
            $r,
            ".thread_id
               AND folder_id IN (SELECT id FROM folders WHERE account_id =
                     (SELECT account_id FROM folders WHERE id = ",
            $r,
            ".folder_id AND role = 'sent'));"
        )
    };
}

/// Recomputes the order key of every row of the account `$a` (an
/// expression); for Sent folders that come, go or change role.
macro_rules! refresh_account {
    ($a:literal) => {
        concat!(
            sent_order_update!(),
            " ",
            $a,
            ")",
            sent_order_where!(),
            "folder_id IN (SELECT id FROM folders WHERE account_id = ",
            $a,
            ");"
        )
    };
}

/// What `thread_activity` must hold: the v17 backfill as a query (columns
/// `folder_id, thread_id, folder_last, last_date, message_count`).
pub const REBUILD_SELECT: &str = rows_select!("m.thread_id IS NOT NULL");

/// Schema v18 (P3 WP-perf): the triggers that keep `thread_activity` equal
/// to [`REBUILD_SELECT`], then a rebuild of the table (rows written between
/// v17 and v18 by a build without the triggers are recomputed).
pub(crate) const MAINTENANCE_SQL: &str = concat!(
    "CREATE TRIGGER thread_activity_insert AFTER INSERT ON messages
     WHEN NEW.thread_id IS NOT NULL BEGIN ",
    refresh_row!("NEW"),
    refresh_sent!("NEW"),
    " END;
     CREATE TRIGGER thread_activity_delete AFTER DELETE ON messages
     WHEN OLD.thread_id IS NOT NULL BEGIN ",
    refresh_row!("OLD"),
    refresh_sent!("OLD"),
    " END;
     CREATE TRIGGER thread_activity_update
     AFTER UPDATE OF folder_id, thread_id, date_sort ON messages
     WHEN OLD.folder_id IS NOT NEW.folder_id OR OLD.thread_id IS NOT NEW.thread_id
       OR OLD.date_sort IS NOT NEW.date_sort BEGIN ",
    refresh_row!("OLD"),
    refresh_sent!("OLD"),
    refresh_row!("NEW"),
    refresh_sent!("NEW"),
    " END;
     CREATE TRIGGER thread_activity_folder_change
     AFTER UPDATE OF role, account_id ON folders
     WHEN OLD.role IS NOT NEW.role OR OLD.account_id IS NOT NEW.account_id BEGIN
       DELETE FROM thread_activity WHERE folder_id = NEW.id;
       INSERT INTO thread_activity (folder_id, thread_id, folder_last, last_date, message_count) ",
    rows_select!("m.folder_id = NEW.id AND m.thread_id IS NOT NULL"),
    ";",
    refresh_account!("OLD.account_id"),
    refresh_account!("NEW.account_id"),
    " END;
     CREATE TRIGGER thread_activity_folder_delete AFTER DELETE ON folders
     WHEN OLD.role = 'sent' BEGIN ",
    refresh_account!("OLD.account_id"),
    " END;
     DELETE FROM thread_activity;
     INSERT INTO thread_activity (folder_id, thread_id, folder_last, last_date, message_count) ",
    rows_select!("m.thread_id IS NOT NULL"),
    ";"
);

fn folder_ids(list: &[ScopeFolder]) -> Vec<&str> {
    list.iter().map(|f| f.id.as_str()).collect()
}

/// Whether the table can order `folders`: the joining Sent folders are
/// exactly the Sent folders of the listed accounts that are not listed
/// themselves (what [`Store::conversation_folders`] returns). A grant that
/// dropped one of them makes the list fall back to `GROUPING_PAGE_SQL`:
/// the table's order key counts every Sent folder.
///
/// [`Store::conversation_folders`]: crate::Store::conversation_folders
pub(crate) fn covers(tx: &Transaction<'_>, folders: &ConversationFolders) -> Result<bool> {
    let mut accounts: Vec<&str> = folders
        .threaded
        .iter()
        .map(|f| f.account_id.as_str())
        .collect();
    accounts.sort_unstable();
    accounts.dedup();
    if accounts.is_empty() {
        return Ok(folders.sent.is_empty());
    }
    let mut listed = folder_ids(&folders.threaded);
    listed.extend(folder_ids(&folders.singles));
    let mut stmt = tx.prepare_cached(
        "SELECT id FROM folders
         WHERE role = 'sent' AND account_id IN (SELECT value FROM json_each(?1))
           AND id NOT IN (SELECT value FROM json_each(?2))",
    )?;
    let expected: HashSet<String> = stmt
        .query_map(
            params![super::json_ids(&accounts)?, super::json_ids(&listed)?],
            |r| r.get(0),
        )?
        .collect::<rusqlite::Result<_>>()?;
    let joined: HashSet<String> = folders
        .sent
        .iter()
        .map(|f| f.id.as_str().to_owned())
        .collect();
    Ok(expected == joined)
}

/// One page of conversation keys with the total, from the table (see the
/// module docs). Keys of Drafts folders are `single-<id>`.
pub(crate) fn page(
    tx: &Transaction<'_>,
    folders: &ConversationFolders,
    offset: u32,
    limit: u32,
) -> Result<(u32, Vec<String>)> {
    if let ([only], []) = (folders.threaded.as_slice(), folders.singles.as_slice()) {
        let total: u32 = tx
            .prepare_cached(COUNT_SQL)?
            .query_row([only.id.as_str()], |r| r.get(0))?;
        let keys = tx
            .prepare_cached(PAGE_SQL)?
            .query_map(params![only.id.as_str(), limit, offset], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        return Ok((total, keys));
    }
    let threaded = folder_ids(&folders.threaded);
    let singles = folder_ids(&folders.singles);
    let mut total: u32 = 0;
    if !threaded.is_empty() {
        total += tx
            .prepare_cached(COUNT_MANY_SQL)?
            .query_row([super::json_ids(&threaded)?], |r| r.get::<_, u32>(0))?;
    }
    if !singles.is_empty() {
        total += tx
            .prepare_cached(SINGLES_COUNT_SQL)?
            .query_row([super::json_ids(&singles)?], |r| r.get::<_, u32>(0))?;
    }
    let wanted = offset.saturating_add(limit);
    let mut newest: HashMap<String, i64> = HashMap::new();
    let mut add = |key: String, date: i64| {
        let slot = newest.entry(key).or_insert(date);
        *slot = (*slot).max(date);
    };
    for (sql, list) in [(PAGE_SQL, &threaded), (SINGLES_PAGE_SQL, &singles)] {
        let mut stmt = tx.prepare_cached(sql)?;
        for folder in list {
            let rows = if sql == PAGE_SQL {
                stmt.query(params![folder, wanted, 0])?
            } else {
                stmt.query(params![folder, wanted])?
            };
            let rows = rows.mapped(|r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)));
            for row in rows {
                let (key, date) = row?;
                add(key, date);
            }
        }
    }
    let mut merged: Vec<(String, i64)> = newest.into_iter().collect();
    merged.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    let skip = usize::try_from(offset).unwrap_or(usize::MAX);
    let take = usize::try_from(limit).unwrap_or(usize::MAX);
    let keys = merged
        .into_iter()
        .skip(skip)
        .take(take)
        .map(|(key, _)| key)
        .collect();
    Ok((total, keys))
}

/// One page of conversation keys with the total from `messages`
/// (`GROUPING_PAGE_SQL`).
pub(crate) fn grouping_page(
    tx: &Transaction<'_>,
    folders: &ConversationFolders,
    offset: u32,
    limit: u32,
) -> Result<(u32, Vec<String>)> {
    let threaded = super::json_ids(&folder_ids(&folders.threaded))?;
    let singles = super::json_ids(&folder_ids(&folders.singles))?;
    let sent = super::json_ids(&folder_ids(&folders.sent))?;
    let total: u32 = tx
        .prepare_cached(GROUPING_COUNT_SQL)?
        .query_row(params![threaded, singles], |r| r.get(0))?;
    let keys = tx
        .prepare_cached(GROUPING_PAGE_SQL)?
        .query_map(params![threaded, sent, singles, limit, offset], |r| {
            r.get(0)
        })?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok((total, keys))
}

#[cfg(test)]
#[path = "activity_tests.rs"]
mod tests;
