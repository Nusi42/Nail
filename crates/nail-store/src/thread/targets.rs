//! Conversation targets of mail actions (P3, ADR 0164; WP-actions-core).
//!
//! A conversation target is a thread as a list row shows it: its messages
//! in the listed folders (`ConversationFolders::threaded`) plus the user's
//! messages from the Sent folders that join (`sent`), or without a folder
//! its messages in every folder but Drafts, Trash and Junk. Which of them
//! an action reaches is [`ConversationReach`]:
//!
//! * `listed` — the messages in the listed folders (without a folder: all
//!   but those in Sent);
//! * `whole` — every message the row counts (listed + joined Sent; never
//!   drafts);
//! * `newest` — the row's own message ([`ThreadSummary::latest_message_id`]:
//!   the newest one the user did not write) and its copies among the listed
//!   messages;
//! * `thread` — Gmail's Delete: `whole` plus the thread's messages in every
//!   other folder of the listed folders' accounts (without a folder: of
//!   every account) but Drafts, Trash and Junk (its archived messages, the
//!   Inbox copies when Delete runs in Sent, …).
//!
//! Unlike the conversation view, copies with the same Message-ID are **not**
//! collapsed here: an action reaches every stored copy. `keep` (a remote
//! session's grant) drops copies outside it before anything else, so the
//! answer covers exactly what the caller can see.
//!
//! [`ThreadSummary::latest_message_id`]: nail_api::ThreadSummary::latest_message_id

use nail_api::mail_actions::ConversationReach;
use nail_api::{AccountId, FolderId, MessageId, ThreadId};
use rusqlite::{OptionalExtension, params};

use super::conversation::{ConversationFolders, OwnAddresses, SINGLE_PREFIX, ScopeFolder};
use crate::error::{Error, Result};
use crate::store::Store;

/// One stored message an action reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetMessage {
    /// The message (one stored copy).
    pub id: MessageId,
    /// Its account.
    pub account_id: AccountId,
    /// Its folder.
    pub folder_id: FolderId,
    /// Its folder's role (wire name: `inbox`, `sent`, `trash`, …).
    pub role: String,
    /// When it entered its folder, as an order: its `listed_seq`, which
    /// grows with every message stored and every move into a folder
    /// ([`Store::message_mark`], schema v24).
    pub listed_seq: i64,
}

/// Schema v24 (R5 review, ADR 0236): when each message entered the folder
/// it is in. `messages.listed_seq` takes the next number of the counter
/// `message_listed_seq` when the message is stored and whenever its folder
/// changes (triggers, so every path is covered: the sync, the queue, a
/// snooze waking up, an undo, a rule). Messages stored before v24 keep 0.
/// The `as_of` mark of a whole list is the counter's value: a message that
/// entered a folder after the mark — new, or moved in — is left out.
pub(crate) const V24: &str = "
ALTER TABLE messages ADD COLUMN listed_seq INTEGER NOT NULL DEFAULT 0;
CREATE TABLE message_listed_seq (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    n  INTEGER NOT NULL
);
INSERT INTO message_listed_seq (id, n) VALUES (1, 0);
CREATE TRIGGER messages_listed_insert AFTER INSERT ON messages BEGIN
    UPDATE message_listed_seq SET n = n + 1 WHERE id = 1;
    UPDATE messages SET listed_seq = (SELECT n FROM message_listed_seq WHERE id = 1)
     WHERE rowid = NEW.rowid;
END;
CREATE TRIGGER messages_listed_move AFTER UPDATE OF folder_id ON messages
WHEN OLD.folder_id IS NOT NEW.folder_id BEGIN
    UPDATE message_listed_seq SET n = n + 1 WHERE id = 1;
    UPDATE messages SET listed_seq = (SELECT n FROM message_listed_seq WHERE id = 1)
     WHERE rowid = NEW.rowid;
END;
";

/// Every stored copy of a thread's messages, oldest first.
const THREAD_ROWS_SQL: &str = "SELECT m.id, m.account_id, m.folder_id, f.role, m.listed_seq
       FROM messages m JOIN folders f ON f.id = m.folder_id
      WHERE m.thread_id = ?1
      ORDER BY m.date_sort, m.id";

/// One message (a `single-<id>` row).
const SINGLE_ROW_SQL: &str = "SELECT m.id, m.account_id, m.folder_id, f.role, m.listed_seq
       FROM messages m JOIN folders f ON f.id = m.folder_id
      WHERE m.id = ?1";

/// A row of [`THREAD_ROWS_SQL`] / [`SINGLE_ROW_SQL`].
fn target_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TargetMessage> {
    Ok(TargetMessage {
        id: MessageId(r.get(0)?),
        account_id: AccountId(r.get(1)?),
        folder_id: FolderId(r.get(2)?),
        role: r.get(3)?,
        listed_seq: r.get(4)?,
    })
}

impl Store {
    /// The mark of the messages in their folders so far (R5, ADR 0235):
    /// every message stored or moved into a folder later has a greater
    /// [`TargetMessage::listed_seq`].
    pub fn message_mark(&self) -> Result<i64> {
        self.with_conn(|conn| {
            Ok(
                conn.query_row("SELECT n FROM message_listed_seq WHERE id = 1", [], |r| {
                    r.get(0)
                })?,
            )
        })
    }

    /// The messages of `ids` still stored, in order, each with its thread
    /// (the rows of a whole search list, R5): an id no longer stored is
    /// left out.
    pub fn message_threads(&self, ids: &[MessageId]) -> Result<Vec<(MessageId, Option<ThreadId>)>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached("SELECT thread_id FROM messages WHERE id = ?1")?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(thread) = stmt
                    .query_row(params![id.as_str()], |r| r.get::<_, Option<String>>(0))
                    .optional()?
                {
                    out.push((id.clone(), thread.map(ThreadId)));
                }
            }
            Ok(out)
        })
    }

    /// The message targets of `ids` still stored, in order: an unknown
    /// message is left out (a whole list's rows and exceptions, R5: a
    /// message removed meanwhile is gone, not an error).
    pub fn stored_target_messages(&self, ids: &[MessageId]) -> Result<Vec<TargetMessage>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(SINGLE_ROW_SQL)?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(m) = stmt
                    .query_row(params![id.as_str()], target_row)
                    .optional()?
                {
                    out.push(m);
                }
            }
            Ok(out)
        })
    }

    /// Message targets named by id, in order; fails with `not_found` for
    /// an unknown message.
    pub fn target_messages(&self, ids: &[MessageId]) -> Result<Vec<TargetMessage>> {
        let found = self.stored_target_messages(ids)?;
        let stored: std::collections::HashSet<&MessageId> = found.iter().map(|m| &m.id).collect();
        if let Some(missing) = ids.iter().find(|id| !stored.contains(id)) {
            return Err(Error::NotFound {
                kind: "message",
                id: missing.as_str().to_owned(),
            });
        }
        Ok(found)
    }

    /// The stored messages a conversation target reaches (see the module
    /// docs), oldest first. `folders` = the listed folders as
    /// [`Store::conversation_folders`] answers them (already narrowed to a
    /// grant), `None` = shown without a folder; `keep` = a remote session's
    /// grant over single copies. Empty when nothing is reached.
    pub fn conversation_targets(
        &self,
        thread: &ThreadId,
        folders: Option<&ConversationFolders>,
        own: &OwnAddresses,
        keep: Option<&dyn Fn(&ScopeFolder) -> bool>,
        reach: ConversationReach,
    ) -> Result<Vec<TargetMessage>> {
        let (sql, key) = match thread.as_str().strip_prefix(SINGLE_PREFIX) {
            Some(message) => (SINGLE_ROW_SQL, message),
            None => (THREAD_ROWS_SQL, thread.as_str()),
        };
        let mut rows: Vec<TargetMessage> = self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(sql)?;
            let rows = stmt
                .query_map(params![key], target_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        if let Some(keep) = keep {
            rows.retain(|m| {
                keep(&ScopeFolder {
                    id: m.folder_id.clone(),
                    account_id: m.account_id.clone(),
                })
            });
        }
        let single = key != thread.as_str();
        if single {
            return Ok(rows);
        }
        let in_list =
            |list: &[ScopeFolder], m: &TargetMessage| list.iter().any(|f| f.id == m.folder_id);
        let listed = |m: &TargetMessage| match folders {
            Some(f) => in_list(&f.threaded, m),
            None => !matches!(m.role.as_str(), "drafts" | "trash" | "junk" | "sent"),
        };
        let whole = |m: &TargetMessage| {
            m.role != "drafts"
                && match folders {
                    Some(f) => in_list(&f.threaded, m) || in_list(&f.sent, m),
                    None => !matches!(m.role.as_str(), "trash" | "junk"),
                }
        };
        let accounts: Option<Vec<&AccountId>> = folders.map(|f| {
            f.threaded
                .iter()
                .chain(&f.sent)
                .map(|s| &s.account_id)
                .collect()
        });
        let in_thread = |m: &TargetMessage| {
            whole(m)
                || (!matches!(m.role.as_str(), "drafts" | "trash" | "junk")
                    && accounts.as_ref().is_none_or(|a| a.contains(&&m.account_id)))
        };
        Ok(match reach {
            ConversationReach::Listed => rows.into_iter().filter(|m| listed(m)).collect(),
            ConversationReach::Whole => rows.into_iter().filter(|m| whole(m)).collect(),
            ConversationReach::Thread => rows.into_iter().filter(|m| in_thread(m)).collect(),
            ConversationReach::Newest => {
                let (summary, messages) = match self.conversation_within(thread, folders, own, keep)
                {
                    Ok(found) => found,
                    Err(err) if err.is_not_found() => return Ok(Vec::new()),
                    Err(err) => return Err(err),
                };
                let mut copies: Vec<MessageId> = vec![summary.latest_message_id.clone()];
                if let Some(newest) = messages
                    .iter()
                    .find(|m| m.message.id == summary.latest_message_id)
                {
                    copies.extend(
                        newest
                            .message
                            .duplicates
                            .iter()
                            .map(|d| d.message_id.clone()),
                    );
                }
                rows.into_iter()
                    .filter(|m| {
                        m.id == summary.latest_message_id || (copies.contains(&m.id) && listed(m))
                    })
                    .collect()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::{AccountKind, Folder, FolderRole, MessageSummary, testing};

    fn store() -> Store {
        let folders: Vec<Folder> = [
            ("INBOX", FolderRole::Inbox),
            ("Sent", FolderRole::Sent),
            ("Drafts", FolderRole::Drafts),
            ("Trash", FolderRole::Trash),
            ("Other", FolderRole::Other),
        ]
        .into_iter()
        .map(|(name, role)| testing::folder(&format!("a/{name}"), "a", name, role))
        .collect();
        crate::test_store::store_with(&testing::account("a", AccountKind::Local), &folders)
    }

    fn put(store: &Store, id: &str, folder: &str, minute: u32, mid: &str, from: &str) {
        let summary = MessageSummary {
            subject: "Topic".into(),
            from: Some(nail_api::Address {
                name: None,
                email: from.into(),
            }),
            date: format!("2026-09-29T10:{minute:02}:00Z"),
            ..testing::summary(id, "a", &format!("a/{folder}"))
        };
        let refs = if mid == "1@x" {
            Vec::new()
        } else {
            vec!["1@x".to_owned()]
        };
        store
            .merge_messages(&[crate::SyncedMessage {
                message: crate::StoredMessage::new(summary),
                meta: Some(crate::MessageMeta {
                    message_id: Some(mid.into()),
                    in_reply_to: refs.clone(),
                    references: refs,
                    size: Some(100),
                    remote_thread_id: None,
                }),
            }])
            .unwrap();
    }

    /// Thread `t`: `i1` (Ann) and `i3` (Bob) in the Inbox, the user's `s2`
    /// and `s4` in Sent, copies of `i3` in Other and Trash, a draft `d5`.
    fn fixture() -> (Store, ThreadId) {
        let store = store();
        put(&store, "i1", "INBOX", 1, "1@x", "ann@example.org");
        put(&store, "s2", "Sent", 2, "2@x", "me@example.org");
        put(&store, "i3", "INBOX", 3, "3@x", "bob@example.org");
        put(&store, "o3", "Other", 4, "3@x", "bob@example.org");
        put(&store, "t3", "Trash", 5, "3@x", "bob@example.org");
        put(&store, "s4", "Sent", 6, "4@x", "me@example.org");
        put(&store, "d5", "Drafts", 7, "5@x", "me@example.org");
        let thread = store.thread_of(&MessageId("i1".into())).unwrap();
        (store, thread)
    }

    fn own() -> OwnAddresses {
        let mut own = OwnAddresses::new();
        own.add(&"a".into(), "me@example.org");
        own
    }

    fn ids(found: &[TargetMessage]) -> Vec<&str> {
        let mut ids: Vec<&str> = found.iter().map(|m| m.id.as_str()).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn reach_in_a_listed_folder() {
        let (store, t) = fixture();
        let inbox = store
            .conversation_folders(&[FolderId("a/INBOX".into())])
            .unwrap();
        let get = |reach| {
            store
                .conversation_targets(&t, Some(&inbox), &own(), None, reach)
                .unwrap()
        };
        assert_eq!(ids(&get(ConversationReach::Listed)), ["i1", "i3"]);
        assert_eq!(
            ids(&get(ConversationReach::Whole)),
            ["i1", "i3", "s2", "s4"],
            "the replies in Sent, never the draft or other folders' copies"
        );
        assert_eq!(
            ids(&get(ConversationReach::Newest)),
            ["i3"],
            "the newest message the user did not write"
        );
        assert_eq!(
            ids(&get(ConversationReach::Thread)),
            ["i1", "i3", "o3", "s2", "s4"],
            "Gmail's Delete: every folder but Drafts, Trash and Junk"
        );
    }

    #[test]
    fn thread_reach_from_sent_and_trash() {
        let (store, t) = fixture();
        let get = |folder: &str| {
            let listed = store
                .conversation_folders(&[FolderId(format!("a/{folder}"))])
                .unwrap();
            store
                .conversation_targets(&t, Some(&listed), &own(), None, ConversationReach::Thread)
                .unwrap()
        };
        // Delete in Sent takes the Inbox copies too, like Gmail.
        assert_eq!(ids(&get("Sent")), ["i1", "i3", "o3", "s2", "s4"]);
        // Listed in Trash: its Trash copy plus the rest of the thread.
        assert_eq!(ids(&get("Trash")), ["i1", "i3", "o3", "s2", "s4", "t3"]);
    }

    #[test]
    fn thread_reach_stays_in_the_listed_account() {
        let (store, t) = fixture();
        store
            .upsert_account(&testing::account("b", AccountKind::Local))
            .unwrap();
        store
            .upsert_folder(&testing::folder("b/INBOX", "b", "INBOX", FolderRole::Inbox))
            .unwrap();
        let mut copy = store.get_message(&MessageId("i3".into())).unwrap().summary;
        copy.id = MessageId("b3".into());
        copy.account_id = "b".into();
        copy.folder_id = FolderId("b/INBOX".into());
        store
            .merge_messages(&[crate::SyncedMessage {
                message: crate::StoredMessage::new(copy),
                meta: Some(crate::MessageMeta {
                    message_id: Some("3@x".into()),
                    in_reply_to: vec!["1@x".into()],
                    references: vec!["1@x".into()],
                    size: Some(100),
                    remote_thread_id: None,
                }),
            }])
            .unwrap();
        // Threads span accounts (the same mail received twice).
        assert_eq!(store.thread_of(&MessageId("b3".into())).unwrap(), t);
        let inbox = store
            .conversation_folders(&[FolderId("a/INBOX".into())])
            .unwrap();
        let found = store
            .conversation_targets(&t, Some(&inbox), &own(), None, ConversationReach::Thread)
            .unwrap();
        assert!(!ids(&found).contains(&"b3"), "{found:?}");
        // Without a folder (search hits) every account's copies.
        let everywhere = store
            .conversation_targets(&t, None, &own(), None, ConversationReach::Thread)
            .unwrap();
        assert!(ids(&everywhere).contains(&"b3"));
    }

    #[test]
    fn reach_without_a_folder_takes_every_copy() {
        let (store, t) = fixture();
        let get = |reach| {
            store
                .conversation_targets(&t, None, &own(), None, reach)
                .unwrap()
        };
        assert_eq!(ids(&get(ConversationReach::Listed)), ["i1", "i3", "o3"]);
        assert_eq!(
            ids(&get(ConversationReach::Whole)),
            ["i1", "i3", "o3", "s2", "s4"]
        );
        let newest = get(ConversationReach::Newest);
        assert_eq!(ids(&newest), ["i3", "o3"], "the newest and its copies");
    }

    #[test]
    fn a_grant_narrows_before_anything_else() {
        let (store, t) = fixture();
        let mut inbox = store
            .conversation_folders(&[FolderId("a/INBOX".into())])
            .unwrap();
        let no_sent = |f: &ScopeFolder| f.id.as_str() != "a/Sent";
        inbox.retain(no_sent);
        let whole = store
            .conversation_targets(
                &t,
                Some(&inbox),
                &own(),
                Some(&no_sent),
                ConversationReach::Whole,
            )
            .unwrap();
        assert_eq!(ids(&whole), ["i1", "i3"]);
        let nothing = |_: &ScopeFolder| false;
        assert_eq!(
            store
                .conversation_targets(&t, None, &own(), Some(&nothing), ConversationReach::Newest)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn a_single_row_is_its_message() {
        let (store, _) = fixture();
        let found = store
            .conversation_targets(
                &ThreadId(format!("{SINGLE_PREFIX}d5")),
                None,
                &own(),
                None,
                ConversationReach::Whole,
            )
            .unwrap();
        assert_eq!(ids(&found), ["d5"]);
    }

    #[test]
    fn the_mark_orders_messages_by_when_they_were_stored() {
        let (store, _) = fixture();
        let mark = store.message_mark().unwrap();
        let named = [MessageId("i1".into()), MessageId("s4".into())];
        let found = store.target_messages(&named).unwrap();
        assert_eq!(ids(&found), ["i1", "s4"]);
        assert_eq!(found[1].role, "sent");
        assert!(found.iter().all(|m| m.listed_seq <= mark));
        assert!(
            found[0].listed_seq < found[1].listed_seq,
            "stored in that order"
        );
        put(&store, "i9", "INBOX", 9, "9@x", "ann@example.org");
        let later = store.target_messages(&[MessageId("i9".into())]).unwrap();
        assert!(later[0].listed_seq > mark, "stored after the mark");
        assert_eq!(store.message_mark().unwrap(), later[0].listed_seq);
        let unknown = store.target_messages(&[MessageId("nope".into())]);
        assert!(unknown.is_err_and(|e| e.is_not_found()));
        // Unknown ids are left out where a list asks.
        let stored = store
            .stored_target_messages(&[MessageId("nope".into()), MessageId("i9".into())])
            .unwrap();
        assert_eq!(ids(&stored), ["i9"]);
    }

    #[test]
    fn a_message_moved_into_a_folder_counts_as_after_the_mark() {
        let (store, _) = fixture();
        let mark = store.message_mark().unwrap();
        let before = store.target_messages(&[MessageId("o3".into())]).unwrap();
        assert!(before[0].listed_seq <= mark);
        // Any change of folder (the queue, the sync, a snooze waking up).
        store
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE messages SET folder_id = 'a/INBOX' WHERE id = 'o3'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let after = store.target_messages(&[MessageId("o3".into())]).unwrap();
        assert!(after[0].listed_seq > mark, "moved in after the mark");
        assert_eq!(store.message_mark().unwrap(), after[0].listed_seq);
        // Another change (flags) keeps its place.
        store
            .with_conn(|conn| {
                conn.execute("UPDATE messages SET date_sort = 1 WHERE id = 'o3'", [])?;
                Ok(())
            })
            .unwrap();
        let same = store.target_messages(&[MessageId("o3".into())]).unwrap();
        assert_eq!(same[0].listed_seq, after[0].listed_seq);
    }

    #[test]
    fn a_list_reads_each_row_with_its_thread_and_skips_gone_ones() {
        let (store, t) = fixture();
        let rows = store
            .message_threads(&[MessageId("i1".into()), MessageId("nope".into())])
            .unwrap();
        assert_eq!(rows, vec![(MessageId("i1".into()), Some(t))]);
    }
}
