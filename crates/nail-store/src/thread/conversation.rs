//! Conversations (P2, ADR 0148): what one row of a threaded message list
//! stands for, and the messages the reader shows for it.
//!
//! A **conversation listed in a folder** is the thread's messages in the
//! listed folders plus the user's own messages of the thread from the Sent
//! folders of the listed folders' accounts (they carry `from_me`). Drafts
//! never join: a listed Drafts folder shows each draft as a conversation of
//! its own, with the id [`SINGLE_PREFIX`]`<message id>`. **Without a
//! folder** a conversation is the thread's messages in every folder but
//! Drafts, Trash and Junk. Copies with the same Message-ID count once (the
//! oldest copy, a listed one before a joined Sent copy).
//!
//! The store classifies the folders ([`Store::conversation_folders`]); the
//! core may narrow them (remote grants) before listing
//! ([`Store::list_conversations`]) or reading one
//! ([`Store::conversation`]). "Me" comes from the identities the core
//! passes as [`OwnAddresses`].

use std::collections::{HashMap, HashSet};

use nail_api::{
    AccountId, ConversationMessage, FolderId, MessageSummary, ThreadId, ThreadParticipant,
    ThreadSummary,
};
use rusqlite::{OptionalExtension, Transaction, params};

use super::subject::normalize_subject;
use super::{ThreadPage, json_ids};
use crate::error::{Error, Result};
use crate::mail::{MESSAGE_COL_COUNT, MESSAGE_COLS, attach_tags, summary_from_row};
use crate::store::Store;

/// The id prefix of a conversation that is one message on its own (a draft
/// listed in Drafts): `single-<message id>`.
pub const SINGLE_PREFIX: &str = "single-";

/// The user's addresses per account (lower case): the account address and
/// its identities' addresses, which make a sender "me".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnAddresses {
    by_account: HashMap<AccountId, HashSet<String>>,
}

impl OwnAddresses {
    /// No addresses: nobody is "me" (only Sent folders mark the user's
    /// messages).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an address of `account` (compared case-insensitively; empty
    /// addresses are ignored).
    pub fn add(&mut self, account: &AccountId, email: &str) {
        let email = email.trim();
        if email.is_empty() {
            return;
        }
        self.by_account
            .entry(account.clone())
            .or_default()
            .insert(email.to_lowercase());
    }

    /// Whether `email` is one of `account`'s addresses.
    #[must_use]
    pub fn is_mine(&self, account: &AccountId, email: &str) -> bool {
        self.by_account
            .get(account)
            .is_some_and(|set| set.contains(&email.trim().to_lowercase()))
    }

    /// Whether `email` is an address of any of `accounts`.
    #[must_use]
    pub fn is_mine_in(&self, accounts: &[AccountId], email: &str) -> bool {
        accounts.iter().any(|a| self.is_mine(a, email))
    }

    /// Sets `me` on the participants (and the newest sender) of `summary`
    /// whose address belongs to one of its accounts; for summaries built
    /// without identities ([`Store::get_thread`]).
    pub fn mark(&self, summary: &mut ThreadSummary) {
        let accounts = summary.account_ids.clone();
        for p in summary
            .participants
            .iter_mut()
            .chain(summary.latest_from.as_mut())
        {
            p.me |= self.is_mine_in(&accounts, &p.address.email);
        }
    }
}

/// A folder of a conversation scope with its account (so a grant can
/// narrow it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFolder {
    /// The folder.
    pub id: FolderId,
    /// Its account.
    pub account_id: AccountId,
}

/// The folders behind a conversation list ([`Store::conversation_folders`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversationFolders {
    /// Listed folders whose messages group by thread.
    pub threaded: Vec<ScopeFolder>,
    /// Listed Drafts folders: every message a conversation of its own.
    pub singles: Vec<ScopeFolder>,
    /// Sent folders of the listed folders' accounts that are not listed
    /// themselves: the user's messages from them join the conversations.
    pub sent: Vec<ScopeFolder>,
}

impl ConversationFolders {
    /// Keeps only the folders `keep` accepts (a remote session's grant).
    pub fn retain(&mut self, keep: impl Fn(&ScopeFolder) -> bool) {
        self.threaded.retain(&keep);
        self.singles.retain(&keep);
        self.sent.retain(&keep);
    }

    fn json(list: &[ScopeFolder]) -> Result<String> {
        json_ids(&list.iter().map(|f| f.id.as_str()).collect::<Vec<_>>())
    }
}

/// One message of a conversation as read from the store.
struct Row {
    summary: MessageSummary,
    message_id: Option<String>,
    in_sent: bool,
}

/// Columns after [`MESSAGE_COLS`]: the Message-ID, whether the copy is a
/// joined Sent copy (`?3`), whether it sits in any Sent folder.
const CONVERSATION_EXTRA: &str = "m.message_id,
    m.folder_id IN (SELECT value FROM json_each(?3)) AS joined,
    EXISTS (SELECT 1 FROM folders f WHERE f.id = m.folder_id AND f.role = 'sent') AS in_sent";

/// The listed conversations' keys, newest activity first (`?1` threaded
/// folders, `?2` Sent folders that join, `?3` Drafts folders, `?4` limit,
/// `?5` offset). A Sent message only adds activity to a thread that has a
/// message in the listed folders. Every part reads the covering index
/// `messages_folder_thread` (schema v16) only; `+thread_id` keeps SQLite on
/// it for the Sent folders (one pass over Sent against the listed threads)
/// instead of one thread lookup per listed message.
pub(crate) const PAGE_SQL: &str = "SELECT k, MAX(d) AS last FROM (
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

/// The number of listed conversations (`?1` threaded, `?2` Drafts folders).
pub(crate) const COUNT_SQL: &str = "SELECT
        (SELECT COUNT(DISTINCT thread_id) FROM messages
          WHERE folder_id IN (SELECT value FROM json_each(?1)) AND thread_id IS NOT NULL)
      + (SELECT COUNT(*) FROM messages WHERE folder_id IN (SELECT value FROM json_each(?2)))";

/// A thread's messages in the listed folders (`?2`) and the joining Sent
/// folders (`?3`), oldest first; a listed copy before a joined one.
pub(crate) fn listed_sql() -> String {
    format!(
        "SELECT {MESSAGE_COLS}, {CONVERSATION_EXTRA} FROM messages m
         WHERE m.thread_id = ?1
           AND (m.folder_id IN (SELECT value FROM json_each(?2))
                OR m.folder_id IN (SELECT value FROM json_each(?3)))
         ORDER BY m.date_sort, joined, m.id"
    )
}

/// A thread's messages in every folder but Drafts, Trash and Junk (`?2` and
/// `?3` unused, `'[]'`), oldest first.
pub(crate) fn everywhere_sql() -> String {
    format!(
        "SELECT {MESSAGE_COLS}, {CONVERSATION_EXTRA} FROM messages m
         WHERE m.thread_id = ?1 AND ?2 = ?3
           AND NOT EXISTS (SELECT 1 FROM folders f WHERE f.id = m.folder_id
                           AND f.role IN ('drafts', 'trash', 'junk'))
         ORDER BY m.date_sort, m.id"
    )
}

/// One message as a conversation of its own (`?1` the message id; `?2`,
/// `?3` unused, `'[]'`).
fn single_sql() -> String {
    format!(
        "SELECT {MESSAGE_COLS}, {CONVERSATION_EXTRA} FROM messages m
         WHERE m.id = ?1 AND ?2 = ?3"
    )
}

fn read_rows(
    tx: &Transaction<'_>,
    sql: &str,
    key: &str,
    listed: &str,
    joined: &str,
) -> Result<Vec<Row>> {
    let mut stmt = tx.prepare_cached(sql)?;
    let rows = stmt
        .query_map(params![key, listed, joined], |r| {
            Ok(Row {
                summary: summary_from_row(r)?,
                message_id: r.get(MESSAGE_COL_COUNT)?,
                in_sent: r.get(MESSAGE_COL_COUNT + 2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Collapses copies with the same Message-ID into the first (the others in
/// its `duplicates`) and marks the user's messages.
fn collapse(
    tx: &Transaction<'_>,
    rows: Vec<Row>,
    own: &OwnAddresses,
) -> Result<Vec<ConversationMessage>> {
    let mut first_by_mid: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<(MessageSummary, bool)> = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(mid) = row.message_id {
            if let Some(&i) = first_by_mid.get(&mid) {
                let (kept, in_sent) = &mut out[i];
                kept.duplicates.push(nail_api::MessageLocation {
                    message_id: row.summary.id,
                    account_id: row.summary.account_id,
                    folder_id: row.summary.folder_id,
                });
                *in_sent |= row.in_sent;
                continue;
            }
            first_by_mid.insert(mid, out.len());
        }
        out.push((row.summary, row.in_sent));
    }
    let (summaries, in_sent): (Vec<MessageSummary>, Vec<bool>) = out.into_iter().unzip();
    let summaries = attach_tags(tx, summaries)?;
    Ok(summaries
        .into_iter()
        .zip(in_sent)
        .map(|(message, in_sent)| {
            let mine = message
                .from
                .as_ref()
                .is_some_and(|f| own.is_mine(&message.account_id, &f.email));
            ConversationMessage {
                from_me: in_sent || mine,
                message,
            }
        })
        .collect())
}

/// Builds the row of a conversation from its messages (oldest first):
/// distinct senders with "me" (an address of one of the conversation's
/// accounts, or the sender of one of the user's messages) and unread
/// senders, counts, and the newest message. `None` without messages.
#[must_use]
pub fn summarize_conversation(
    id: &str,
    messages: &[ConversationMessage],
    own: &OwnAddresses,
) -> Option<ThreadSummary> {
    let first = messages.first()?;
    let latest = messages.last()?;
    let mut accounts: Vec<AccountId> = Vec::new();
    for m in messages {
        if !accounts.contains(&m.message.account_id) {
            accounts.push(m.message.account_id.clone());
        }
    }
    let mut participants: Vec<ThreadParticipant> = Vec::new();
    for m in messages {
        let Some(from) = &m.message.from else {
            continue;
        };
        let unread = !m.message.flags.seen;
        if let Some(known) = participants
            .iter_mut()
            .find(|p| p.address.email.eq_ignore_ascii_case(&from.email))
        {
            known.unread |= unread;
            known.me |= m.from_me;
        } else {
            participants.push(ThreadParticipant {
                address: from.clone(),
                me: m.from_me || own.is_mine_in(&accounts, &from.email),
                unread,
            });
        }
    }
    let latest_from = latest.message.from.as_ref().map(|from| {
        let me = participants
            .iter()
            .find(|p| p.address.email.eq_ignore_ascii_case(&from.email))
            .is_some_and(|p| p.me);
        ThreadParticipant {
            address: from.clone(),
            me,
            unread: !latest.message.flags.seen,
        }
    });
    let count = |pred: &dyn Fn(&ConversationMessage) -> bool| {
        u32::try_from(messages.iter().filter(|m| pred(m)).count()).unwrap_or(u32::MAX)
    };
    Some(ThreadSummary {
        id: ThreadId(id.to_owned()),
        subject: normalize_subject(&first.message.subject).display,
        participants,
        message_count: count(&|_| true),
        unread_count: count(&|m| !m.message.flags.seen),
        flagged: messages.iter().any(|m| m.message.flags.flagged),
        has_attachments: messages.iter().any(|m| m.message.has_attachments),
        date: latest.message.date.clone(),
        preview: latest.message.preview.clone(),
        latest_message_id: latest.message.id.clone(),
        latest_message: Box::new(latest.message.clone()),
        latest_from,
        account_ids: accounts,
    })
}

/// The messages of one listed conversation key (a thread id or a single
/// message).
fn listed_messages(
    tx: &Transaction<'_>,
    key: &str,
    listed: &str,
    joined: &str,
    own: &OwnAddresses,
) -> Result<Vec<ConversationMessage>> {
    let rows = if let Some(message) = key.strip_prefix(SINGLE_PREFIX) {
        read_rows(tx, &single_sql(), message, "[]", "[]")?
    } else {
        read_rows(tx, &listed_sql(), key, listed, joined)?
    };
    collapse(tx, rows, own)
}

impl Store {
    /// Classifies the folders a list shows (a real folder, or the member
    /// folders of a virtual folder): Drafts folders list single messages,
    /// the others group by thread, and the Sent folders of their accounts
    /// join. Unknown folders are left out.
    pub fn conversation_folders(&self, listed: &[FolderId]) -> Result<ConversationFolders> {
        let listed_json = json_ids(listed)?;
        self.with_conn(|conn| {
            let mut out = ConversationFolders::default();
            {
                let mut stmt = conn.prepare_cached(
                    "SELECT id, account_id, role FROM folders
                     WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY id",
                )?;
                let rows = stmt.query_map([&listed_json], |r| {
                    Ok((
                        ScopeFolder {
                            id: FolderId(r.get(0)?),
                            account_id: AccountId(r.get(1)?),
                        },
                        r.get::<_, String>(2)?,
                    ))
                })?;
                for row in rows {
                    let (folder, role) = row?;
                    if role == "drafts" {
                        out.singles.push(folder);
                    } else {
                        out.threaded.push(folder);
                    }
                }
            }
            let mut accounts: Vec<&str> =
                out.threaded.iter().map(|f| f.account_id.as_str()).collect();
            accounts.sort_unstable();
            accounts.dedup();
            let mut stmt = conn.prepare_cached(
                "SELECT id, account_id FROM folders
                 WHERE role = 'sent' AND account_id IN (SELECT value FROM json_each(?1))
                   AND id NOT IN (SELECT value FROM json_each(?2))
                 ORDER BY id",
            )?;
            let sent = stmt
                .query_map(params![json_ids(&accounts)?, listed_json], |r| {
                    Ok(ScopeFolder {
                        id: FolderId(r.get(0)?),
                        account_id: AccountId(r.get(1)?),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.sent = sent;
            Ok(out)
        })
    }

    /// A page of the conversations in `folders`, newest activity first (the
    /// user's replies from Sent count as activity). Counts, participants
    /// and the newest message consider the conversation as defined in the
    /// module docs.
    pub fn list_conversations(
        &self,
        folders: &ConversationFolders,
        own: &OwnAddresses,
        offset: u32,
        limit: u32,
    ) -> Result<ThreadPage> {
        let threaded = ConversationFolders::json(&folders.threaded)?;
        let singles = ConversationFolders::json(&folders.singles)?;
        let sent = ConversationFolders::json(&folders.sent)?;
        self.with_tx(|tx| {
            let total: u32 = tx.query_row(COUNT_SQL, params![threaded, singles], |r| r.get(0))?;
            let page: Vec<String> = {
                let mut stmt = tx.prepare_cached(PAGE_SQL)?;
                stmt.query_map(params![threaded, sent, singles, limit, offset], |r| {
                    r.get(0)
                })?
                .collect::<rusqlite::Result<_>>()?
            };
            let mut threads = Vec::with_capacity(page.len());
            for key in page {
                let messages = listed_messages(tx, &key, &threaded, &sent, own)?;
                if let Some(summary) = summarize_conversation(&key, &messages, own) {
                    threads.push(summary);
                }
            }
            Ok(ThreadPage { total, threads })
        })
    }

    /// One conversation, oldest first: as listed in `folders`, or without
    /// folders in every folder but Drafts, Trash and Junk. A
    /// [`SINGLE_PREFIX`] id is that one message. Fails with
    /// [`Error::NotFound`] when the conversation has no message.
    pub fn conversation(
        &self,
        thread: &ThreadId,
        folders: Option<&ConversationFolders>,
        own: &OwnAddresses,
    ) -> Result<(ThreadSummary, Vec<ConversationMessage>)> {
        let listed = folders.map(|f| ConversationFolders::json(&f.threaded));
        let sent = folders.map(|f| ConversationFolders::json(&f.sent));
        let messages = self.with_tx(|tx| {
            let rows = if let Some(message) = thread.as_str().strip_prefix(SINGLE_PREFIX) {
                read_rows(tx, &single_sql(), message, "[]", "[]")?
            } else if let (Some(listed), Some(sent)) = (listed, sent) {
                read_rows(tx, &listed_sql(), thread.as_str(), &listed?, &sent?)?
            } else {
                read_rows(tx, &everywhere_sql(), thread.as_str(), "[]", "[]")?
            };
            collapse(tx, rows, own)
        })?;
        let summary = summarize_conversation(thread.as_str(), &messages, own)
            .ok_or_else(|| Error::not_found("conversation", thread))?;
        Ok((summary, messages))
    }

    /// The folder a message is in, if it exists (a single-message
    /// conversation's scope check).
    pub fn message_folder(&self, message: &nail_api::MessageId) -> Result<Option<FolderId>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT folder_id FROM messages WHERE id = ?1",
                    [message.as_str()],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .map(FolderId))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(store: &Store, sql: &str, args: &[&dyn rusqlite::ToSql]) -> String {
        store
            .with_conn(|conn| {
                let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
                let rows = stmt
                    .query_map(args, |r| r.get::<_, String>(3))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows.join(
                    "
",
                ))
            })
            .unwrap()
    }

    fn migrated() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn assert_no_scan(plan: &str) {
        assert!(
            !plan
                .lines()
                .any(|l| l.trim() == "SCAN messages" || l.trim() == "SCAN m"),
            "the query scans the messages table:\n{plan}"
        );
    }

    /// The list page reads the listed and the Sent folders from the
    /// covering folder-thread index (v16); the Sent join probes the listed
    /// threads (no lookup per listed message).
    #[test]
    fn the_page_query_uses_the_folder_index() {
        let conn = migrated();
        let p = plan(
            &conn,
            PAGE_SQL,
            &[&"[\"in\"]", &"[\"sent\"]", &"[]", &50_i64, &0_i64],
        );
        // Schema v16: every part reads only `messages_folder_thread`.
        assert!(
            p.contains("USING COVERING INDEX messages_folder_thread"),
            "{p}"
        );
        assert!(
            !p.contains("USING INDEX messages_folder_date"),
            "the page reads message rows:
{p}"
        );
        assert!(
            !p.contains("messages_thread_id"),
            "the Sent join looks up every listed thread:
{p}"
        );
        assert_no_scan(&p);
        let p = plan(&conn, COUNT_SQL, &[&"[\"in\"]", &"[]"]);
        assert!(
            p.contains("USING COVERING INDEX messages_folder_thread"),
            "{p}"
        );
        assert_no_scan(&p);
    }

    /// A conversation's messages come through the thread index.
    #[test]
    fn conversation_reads_use_the_thread_index() {
        let conn = migrated();
        for sql in [listed_sql(), everywhere_sql()] {
            let p = plan(&conn, &sql, &[&"t1", &"[]", &"[]"]);
            assert!(p.contains("messages_thread_id"), "{p}");
            assert_no_scan(&p);
        }
        let p = plan(&conn, &single_sql(), &[&"m1", &"[]", &"[]"]);
        assert_no_scan(&p);
    }

    #[test]
    fn own_addresses_compare_case_insensitively_per_account() {
        let mut own = OwnAddresses::new();
        own.add(&"a1".into(), " Me@Example.org ");
        own.add(&"a1".into(), "");
        assert!(own.is_mine(&"a1".into(), "me@example.ORG"));
        assert!(!own.is_mine(&"a2".into(), "me@example.org"));
        assert!(own.is_mine_in(&["a2".into(), "a1".into()], "ME@example.org"));
        assert!(!own.is_mine(&"a1".into(), ""));
    }
}
