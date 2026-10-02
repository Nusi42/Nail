//! Threading: incremental thread membership in the store, plus the pure
//! JWZ algorithm ([`jwz`]) for laying out one thread.
//!
//! # Thread membership (incremental)
//!
//! Every Message-ID the store has seen — a message's own id and every id in
//! its `References` / `In-Reply-To`, even of messages that are not stored —
//! is mapped to a thread in `thread_refs`. When a message is written:
//!
//! 1. Its ids are looked up. None known → a new thread (or the subject
//!    fallback below); one thread → it joins; several threads → they are
//!    merged into the one with the oldest message (the message links them).
//! 2. All its ids are mapped to that thread.
//!
//! This is the connected-components view of JWZ: two messages share a
//! thread iff a chain of Message-ID / References links joins them, so
//! missing parents (their ids are still mapped), loops (links have no
//! direction here) and duplicate Message-IDs (both copies map to the same
//! thread) need no special cases. Threads are global, so a thread spans
//! folders and accounts; a virtual account filters it by its members.
//!
//! **Subject fallback** (JWZ step 5) for messages without any parent
//! reference and whose own id is not referenced yet: a reply (`Re: x`)
//! joins the newest thread with the same normalized subject within
//! [`SUBJECT_WINDOW_SECS`]; an original (`x`) joins a thread that so far
//! only holds orphan replies of that subject (the original arrived late).
//! Forwards and empty subjects never join by subject.
//!
//! **Server threads** (M4c, schema v15): a message with a server thread /
//! conversation id (`messages.remote_thread_id`: Gmail `threadId`, Graph
//! `conversationId`, EWS `ConversationId`) also maps the key
//! `remote-thread:<account>:<id>`, so everything the server puts in one
//! thread shares a Nail thread (and still merges with Message-ID links).
//! Such messages never join by subject.
//!
//! Deleting messages does not split threads; [`Store::rethread_all`]
//! rebuilds everything from the stored headers.

pub mod jwz;
pub mod subject;

use std::collections::{BTreeSet, HashMap};

use nail_api::{AccountId, Address, FolderId, MessageId, MessageSummary, ThreadId, ThreadSummary};
use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::mail::{MESSAGE_COLS, summary_from_row};
use crate::store::Store;
use subject::{SubjectKind, normalize_subject};

/// How far apart (in seconds) two messages may be to join by subject.
pub const SUBJECT_WINDOW_SECS: i64 = 60 * 24 * 60 * 60;

/// Derives a thread id from a seed (a root Message-ID or a row id).
fn thread_id_for(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    format!("t{}", &hex::encode(digest)[..20])
}

/// The threading-relevant columns of one message row.
struct ThreadRow {
    id: String,
    message_id: Option<String>,
    parents: Vec<String>,
    thread_key: Option<String>,
    subject_key: String,
    kind: i64,
    date_sort: i64,
    current: Option<String>,
    /// `remote-thread:<account>:<id>` when the server threads the message.
    remote_thread: Option<String>,
}

/// The thread key of a server thread id ([`remote_thread_key`] in
/// `thread_refs`).
pub(crate) fn remote_thread_key(account: &str, remote_thread_id: &str) -> String {
    format!("remote-thread:{account}:{remote_thread_id}")
}

/// Whether `messages.remote_thread_id` exists (schema v15); the v2
/// backfill threads rows while the schema is still older.
fn has_remote_thread_column(tx: &Transaction<'_>) -> Result<bool> {
    let n: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name = 'remote_thread_id'",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

const LOAD_ROW: &str = "SELECT id, message_id, in_reply_to_json, references_json, thread_key,
        subject_key, subject_is_reply, date_sort, thread_id, account_id, remote_thread_id
     FROM messages WHERE id = ?1";
const LOAD_ROW_BEFORE_V15: &str = "SELECT id, message_id, in_reply_to_json, references_json,
        thread_key, subject_key, subject_is_reply, date_sort, thread_id, account_id, NULL
     FROM messages WHERE id = ?1";

fn load_row(tx: &Transaction<'_>, id: &str, remote_column: bool) -> Result<ThreadRow> {
    tx.prepare_cached(if remote_column {
        LOAD_ROW
    } else {
        LOAD_ROW_BEFORE_V15
    })?
    .query_row([id], |row| {
        let in_reply_to: String = row.get(2)?;
        let references: String = row.get(3)?;
        let account: String = row.get(9)?;
        let remote_thread = row
            .get::<_, Option<String>>(10)?
            .filter(|t| !t.is_empty())
            .map(|t| remote_thread_key(&account, &t));
        Ok((
            (
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                in_reply_to,
                references,
                row.get::<_, Option<String>>(4)?,
            ),
            (
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?,
                remote_thread,
            ),
        ))
    })
    .optional()?
    .ok_or_else(|| Error::not_found("message", id))
    .and_then(
        |(
            (id, message_id, irt, refs, thread_key),
            (subject_key, kind, date_sort, current, remote_thread),
        )| {
            let irt: Vec<String> = serde_json::from_str(&irt)?;
            let refs: Vec<String> = serde_json::from_str(&refs)?;
            Ok(ThreadRow {
                id,
                message_id,
                parents: jwz::parent_chain(&refs, &irt),
                thread_key,
                subject_key,
                kind,
                date_sort,
                current,
                remote_thread,
            })
        },
    )
}

/// (Re)assigns the thread of one message row; see the module docs.
/// Returns the thread id.
pub(crate) fn assign_thread(tx: &Transaction<'_>, id: &str) -> Result<String> {
    assign_thread_with(tx, id, true)
}

/// [`assign_thread`] on a schema with or without `remote_thread_id`.
fn assign_thread_with(tx: &Transaction<'_>, id: &str, remote_column: bool) -> Result<String> {
    let row = load_row(tx, id, remote_column)?;
    let mut keys: Vec<&str> = Vec::new();
    if let Some(own) = row.message_id.as_deref() {
        keys.push(own);
    }
    keys.extend(row.parents.iter().map(String::as_str));
    if let Some(remote) = row.remote_thread.as_deref() {
        keys.push(remote);
    }
    // Legacy rows (written without headers) carry the M0 thread key, the
    // root of their reference chain.
    if row.message_id.is_none()
        && row.parents.is_empty()
        && let Some(key) = row.thread_key.as_deref()
    {
        keys.push(key);
    }
    keys.sort_unstable();
    keys.dedup();

    let mut threads: BTreeSet<String> = BTreeSet::new();
    {
        let mut stmt = tx.prepare_cached("SELECT thread_id FROM thread_refs WHERE msgid = ?1")?;
        for key in &keys {
            if let Some(t) = stmt
                .query_row([key], |r| r.get::<_, String>(0))
                .optional()?
            {
                threads.insert(t);
            }
        }
    }
    if let Some(current) = &row.current {
        threads.insert(current.clone());
    }

    let winner = if threads.is_empty() {
        let fallback = if row.parents.is_empty() && row.remote_thread.is_none() {
            subject_fallback(tx, &row)?
        } else {
            None
        };
        fallback.unwrap_or_else(|| {
            let seed = row
                .parents
                .first()
                .or(row.message_id.as_ref())
                .or(row.remote_thread.as_ref())
                .cloned()
                .or_else(|| row.thread_key.clone())
                .unwrap_or_else(|| format!("row:{}", row.id));
            thread_id_for(&seed)
        })
    } else if threads.len() == 1 {
        threads.into_iter().next().expect("one thread")
    } else {
        merge_threads(tx, threads)?
    };

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO thread_refs (msgid, thread_id) VALUES (?1, ?2)
             ON CONFLICT (msgid) DO UPDATE SET thread_id = excluded.thread_id",
        )?;
        for key in &keys {
            stmt.execute(params![key, winner])?;
        }
    }
    tx.prepare_cached("UPDATE messages SET thread_id = ?2 WHERE id = ?1")?
        .execute(params![row.id, winner])?;
    Ok(winner)
}

/// Subject fallback for a reply without references. `subject_key <> ''`
/// repeats the condition of the partial index `messages_subject_key`, and
/// the date window is a range, so SQLite searches that index instead of
/// scanning every message (WP123: the scan made initial syncs quadratic).
pub(crate) const SUBJECT_REPLY_SQL: &str = "SELECT thread_id FROM messages
         WHERE subject_key = ?1 AND subject_key <> '' AND id <> ?2 AND thread_id IS NOT NULL
           AND date_sort BETWEEN ?3 - ?4 AND ?3 + ?4
         ORDER BY date_sort DESC, id DESC LIMIT 1";

/// Subject fallback for an original that a reply-subject thread may
/// adopt (see [`SUBJECT_REPLY_SQL`] for the index conditions).
pub(crate) const SUBJECT_ORIGINAL_SQL: &str = "SELECT m.thread_id FROM messages m
         WHERE m.subject_key = ?1 AND m.subject_key <> '' AND m.id <> ?2 AND m.thread_id IS NOT NULL
           AND m.date_sort BETWEEN ?3 - ?4 AND ?3 + ?4
           AND m.subject_is_reply = 1
           AND m.in_reply_to_json = '[]' AND m.references_json = '[]'
           AND (m.message_id IS NOT NULL OR m.thread_key IS NULL)
           AND NOT EXISTS (
               SELECT 1 FROM messages o
               WHERE o.thread_id = m.thread_id AND o.id <> ?2 AND o.subject_is_reply <> 1)
         ORDER BY m.date_sort DESC, m.id DESC LIMIT 1";

fn subject_fallback(tx: &Transaction<'_>, row: &ThreadRow) -> Result<Option<String>> {
    if row.subject_key.is_empty() {
        return Ok(None);
    }
    let sql = if row.kind == SubjectKind::Reply.to_sql() {
        SUBJECT_REPLY_SQL
    } else if row.kind == SubjectKind::Original.to_sql() {
        SUBJECT_ORIGINAL_SQL
    } else {
        return Ok(None);
    };
    Ok(tx
        .prepare_cached(sql)?
        .query_row(
            params![row.subject_key, row.id, row.date_sort, SUBJECT_WINDOW_SECS],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// Merges threads into the one with the oldest message (ties: smallest
/// id); returns the survivor.
fn merge_threads(tx: &Transaction<'_>, threads: BTreeSet<String>) -> Result<String> {
    let mut best: Option<(i64, String)> = None;
    {
        let mut stmt = tx.prepare_cached(
            "SELECT COALESCE(MIN(date_sort), 9223372036854775807) FROM messages WHERE thread_id = ?1",
        )?;
        for t in &threads {
            let oldest: i64 = stmt.query_row([t], |r| r.get(0))?;
            let candidate = (oldest, t.clone());
            if best.as_ref().is_none_or(|b| candidate < *b) {
                best = Some(candidate);
            }
        }
    }
    let (_, winner) = best.expect("at least two threads");
    for loser in threads.iter().filter(|t| **t != winner) {
        tx.execute(
            "UPDATE messages SET thread_id = ?2 WHERE thread_id = ?1",
            params![loser, winner],
        )?;
        tx.execute(
            "UPDATE thread_refs SET thread_id = ?2 WHERE thread_id = ?1",
            params![loser, winner],
        )?;
    }
    Ok(winner)
}

/// Recomputes `subject_key` / `subject_is_reply` of one row from its subject.
pub(crate) fn set_subject_key(tx: &Transaction<'_>, id: &str, subject: &str) -> Result<()> {
    let n = normalize_subject(subject);
    tx.prepare_cached("UPDATE messages SET subject_key = ?2, subject_is_reply = ?3 WHERE id = ?1")?
        .execute(params![id, n.key, n.kind.to_sql()])?;
    Ok(())
}

/// Rebuilds all threads from the stored headers, oldest message first.
pub(crate) fn rethread_in_tx(tx: &Transaction<'_>) -> Result<usize> {
    tx.execute("DELETE FROM thread_refs", [])?;
    tx.execute("UPDATE messages SET thread_id = NULL", [])?;
    let ids: Vec<String> = {
        let mut stmt = tx.prepare("SELECT id FROM messages ORDER BY date_sort, id")?;
        stmt.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let remote_column = has_remote_thread_column(tx)?;
    for id in &ids {
        assign_thread_with(tx, id, remote_column)?;
    }
    Ok(ids.len())
}

/// Schema v2 backfill: subject keys and threads for the v1 rows (grouped
/// by their M0 `thread_key`, the root of the reference chain).
pub(crate) fn backfill_v2(tx: &Transaction<'_>) -> Result<()> {
    let rows: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT id, subject FROM messages")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (id, subject) in &rows {
        set_subject_key(tx, id, subject)?;
    }
    let threaded = rethread_in_tx(tx)?;
    tracing::info!(messages = threaded, "threaded messages from schema v1");
    Ok(())
}

/// A page of threads ([`Store::list_threads`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadPage {
    /// Number of threads in the listed folders.
    pub total: u32,
    /// The requested page, newest activity first.
    pub threads: Vec<ThreadSummary>,
}

/// One message of a thread laid out as a tree ([`Store::thread_tree`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadTreeEntry {
    /// The message.
    pub message: MessageSummary,
    /// Nesting depth (0 = a root of the thread).
    pub depth: u32,
}

/// Builds a [`ThreadSummary`] from a thread's messages (oldest first).
fn summarize(thread_id: &str, messages: &[MessageSummary]) -> Option<ThreadSummary> {
    let first = messages.first()?;
    let latest = messages.last()?;
    let mut participants: Vec<Address> = Vec::new();
    let mut accounts: Vec<AccountId> = Vec::new();
    for m in messages {
        if let Some(from) = &m.from
            && !participants
                .iter()
                .any(|p| p.email.eq_ignore_ascii_case(&from.email))
        {
            participants.push(from.clone());
        }
        if !accounts.contains(&m.account_id) {
            accounts.push(m.account_id.clone());
        }
    }
    Some(ThreadSummary {
        id: ThreadId(thread_id.to_owned()),
        subject: normalize_subject(&first.subject).display,
        participants,
        message_count: u32::try_from(messages.len()).unwrap_or(u32::MAX),
        unread_count: u32::try_from(messages.iter().filter(|m| !m.flags.seen).count())
            .unwrap_or(u32::MAX),
        flagged: messages.iter().any(|m| m.flags.flagged),
        has_attachments: messages.iter().any(|m| m.has_attachments),
        date: latest.date.clone(),
        preview: latest.preview.clone(),
        latest_message_id: latest.id.clone(),
        account_ids: accounts,
    })
}

fn json_ids<T: std::fmt::Display>(ids: &[T]) -> Result<String> {
    Ok(serde_json::to_string(
        &ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
    )?)
}

impl Store {
    /// A page of the threads that have messages in `folders` (a real
    /// folder, or the member folders of a virtual folder), newest activity
    /// first. Counts and previews consider only the messages in `folders`;
    /// copies with the same Message-ID count once.
    pub fn list_threads(
        &self,
        folders: &[FolderId],
        offset: u32,
        limit: u32,
    ) -> Result<ThreadPage> {
        let folder_json = json_ids(folders)?;
        self.with_tx(|tx| {
            let total: u32 = tx.query_row(
                "SELECT COUNT(DISTINCT thread_id) FROM messages
                 WHERE folder_id IN (SELECT value FROM json_each(?1)) AND thread_id IS NOT NULL",
                [&folder_json],
                |r| r.get(0),
            )?;
            let page: Vec<String> = {
                let mut stmt = tx.prepare_cached(
                    "SELECT thread_id FROM messages
                     WHERE folder_id IN (SELECT value FROM json_each(?1)) AND thread_id IS NOT NULL
                     GROUP BY thread_id
                     ORDER BY MAX(date_sort) DESC, thread_id DESC
                     LIMIT ?2 OFFSET ?3",
                )?;
                stmt.query_map(params![folder_json, limit, offset], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            let mut threads = Vec::with_capacity(page.len());
            let mut stmt = tx.prepare_cached(&format!(
                "SELECT {MESSAGE_COLS}, m.message_id FROM messages m
                 WHERE m.thread_id = ?1 AND m.folder_id IN (SELECT value FROM json_each(?2))
                 ORDER BY m.date_sort, m.id"
            ))?;
            for thread_id in page {
                let messages = stmt
                    .query_map(params![thread_id, folder_json], |r| {
                        Ok((
                            summary_from_row(r)?,
                            r.get::<_, Option<String>>(MESSAGE_ID_COL)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let messages =
                    crate::mail::attach_tags(tx, crate::mail::collapse_duplicates(messages))?;
                if let Some(summary) = summarize(&thread_id, &messages) {
                    threads.push(summary);
                }
            }
            Ok(ThreadPage { total, threads })
        })
    }

    /// All messages of a thread, oldest first, across folders; limited to
    /// `accounts` when given (the members of a virtual account). Copies of
    /// one mail (same Message-ID) are collapsed into the oldest copy, the
    /// others listed in its `duplicates`. Fails with [`Error::NotFound`]
    /// when no message is in scope.
    pub fn get_thread(
        &self,
        thread: &ThreadId,
        accounts: Option<&[AccountId]>,
    ) -> Result<(ThreadSummary, Vec<MessageSummary>)> {
        let messages = self.thread_messages(thread, accounts)?;
        let summary = summarize(thread.as_str(), &messages)
            .ok_or_else(|| Error::not_found("thread", thread))?;
        Ok((summary, messages))
    }

    /// The messages of a thread (as [`Store::get_thread`]) laid out as a
    /// JWZ reply tree, in depth-first order.
    pub fn thread_tree(
        &self,
        thread: &ThreadId,
        accounts: Option<&[AccountId]>,
    ) -> Result<Vec<ThreadTreeEntry>> {
        let rows = self.with_tx(|tx| {
            let messages = thread_rows(tx, thread, accounts)?;
            let mut out = Vec::with_capacity(messages.len());
            let mut stmt = tx.prepare_cached(
                "SELECT message_id, in_reply_to_json, references_json, date_sort FROM messages WHERE id = ?1",
            )?;
            let mut seen = BTreeSet::new();
            for (summary, copy_of) in messages {
                // Copies of one mail are shown once (the oldest copy).
                if let Some(mid) = copy_of
                    && !seen.insert(mid)
                {
                    continue;
                }
                let (mid, irt, refs, date): (Option<String>, String, String, i64) = stmt
                    .query_row([summary.id.as_str()], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                    })?;
                let irt: Vec<String> = serde_json::from_str(&irt)?;
                let refs: Vec<String> = serde_json::from_str(&refs)?;
                out.push((summary, mid, jwz::parent_chain(&refs, &irt), date));
            }
            Ok(out)
        })?;
        if rows.is_empty() {
            return Err(Error::not_found("thread", thread));
        }
        let input: Vec<jwz::ThreadMessage<usize>> = rows
            .iter()
            .enumerate()
            .map(|(i, (summary, mid, parents, date))| jwz::ThreadMessage {
                key: i,
                message_id: mid.clone(),
                references: parents.clone(),
                subject: summary.subject.clone(),
                date: *date,
            })
            .collect();
        let mut flat = Vec::with_capacity(rows.len());
        for root in jwz::build_threads(&input) {
            root.flatten(0, &mut flat);
        }
        let mut summaries: Vec<Option<MessageSummary>> =
            rows.into_iter().map(|(s, ..)| Some(s)).collect();
        Ok(flat
            .into_iter()
            .filter_map(|(i, depth)| {
                summaries[i]
                    .take()
                    .map(|message| ThreadTreeEntry { message, depth })
            })
            .collect())
    }

    fn thread_messages(
        &self,
        thread: &ThreadId,
        accounts: Option<&[AccountId]>,
    ) -> Result<Vec<MessageSummary>> {
        self.with_tx(|tx| {
            let rows = thread_rows(tx, thread, accounts)?;
            crate::mail::attach_tags(tx, crate::mail::collapse_duplicates(rows))
        })
    }

    /// The thread a message belongs to.
    pub fn thread_of(&self, message: &MessageId) -> Result<ThreadId> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT thread_id FROM messages WHERE id = ?1",
                [message.as_str()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or_else(|| Error::not_found("message", message))?
            .map(ThreadId)
            .ok_or_else(|| Error::not_found("thread of message", message))
        })
    }

    /// Rebuilds every thread from the stored headers (e.g. after headers
    /// were backfilled from blobs). Returns the number of messages.
    pub fn rethread_all(&self) -> Result<usize> {
        self.with_tx(rethread_in_tx)
    }

    /// Thread ids by message, for a batch of messages (missing ones are
    /// left out).
    pub fn thread_ids(&self, messages: &[MessageId]) -> Result<HashMap<MessageId, ThreadId>> {
        let json = json_ids(messages)?;
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, thread_id FROM messages
                 WHERE id IN (SELECT value FROM json_each(?1)) AND thread_id IS NOT NULL",
            )?;
            let rows =
                stmt.query_map([json], |r| Ok((MessageId(r.get(0)?), ThreadId(r.get(1)?))))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }
}

/// Column index of the extra `m.message_id` selected after [`MESSAGE_COLS`].
const MESSAGE_ID_COL: usize = crate::mail::MESSAGE_COL_COUNT;

fn thread_rows(
    tx: &Transaction<'_>,
    thread: &ThreadId,
    accounts: Option<&[AccountId]>,
) -> Result<Vec<(MessageSummary, Option<String>)>> {
    let accounts_json = accounts.map(json_ids).transpose()?;
    let mut stmt = tx.prepare_cached(&format!(
        "SELECT {MESSAGE_COLS}, m.message_id FROM messages m
         WHERE m.thread_id = ?1
           AND (?2 IS NULL OR m.account_id IN (SELECT value FROM json_each(?2)))
         ORDER BY m.date_sort, m.id"
    ))?;
    let rows = stmt
        .query_map(params![thread.as_str(), accounts_json], |r| {
            Ok((
                summary_from_row(r)?,
                r.get::<_, Option<String>>(MESSAGE_ID_COL)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn plan(conn: &Connection, sql: &str) -> String {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let rows = stmt
            .query_map(params!["subject", "id", 1_000_000_i64, 3600_i64], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows.join("\n")
    }

    /// WP123: without the index the subject fallback scanned every
    /// message once per reply without references (quadratic syncs).
    #[test]
    fn subject_fallback_searches_the_subject_index() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        for sql in [SUBJECT_REPLY_SQL, SUBJECT_ORIGINAL_SQL] {
            let plan = plan(&conn, sql);
            assert!(
                plan.contains("USING INDEX messages_subject_key"),
                "subject fallback does not use the index:\n{plan}"
            );
            assert!(
                !plan
                    .lines()
                    .any(|l| l.trim() == "SCAN messages" || l.trim() == "SCAN m"),
                "subject fallback scans the messages table:\n{plan}"
            );
        }
    }
}
