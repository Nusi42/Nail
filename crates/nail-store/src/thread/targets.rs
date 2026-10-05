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
//!   messages.
//!
//! Unlike the conversation view, copies with the same Message-ID are **not**
//! collapsed here: an action reaches every stored copy. `keep` (a remote
//! session's grant) drops copies outside it before anything else, so the
//! answer covers exactly what the caller can see.
//!
//! [`ThreadSummary::latest_message_id`]: nail_api::ThreadSummary::latest_message_id

use nail_api::mail_actions::ConversationReach;
use nail_api::{AccountId, FolderId, MessageId, ThreadId};
use rusqlite::params;

use super::conversation::{ConversationFolders, OwnAddresses, SINGLE_PREFIX, ScopeFolder};
use crate::error::Result;
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
}

/// Every stored copy of a thread's messages, oldest first.
const THREAD_ROWS_SQL: &str = "SELECT m.id, m.account_id, m.folder_id, f.role
       FROM messages m JOIN folders f ON f.id = m.folder_id
      WHERE m.thread_id = ?1
      ORDER BY m.date_sort, m.id";

/// One message (a `single-<id>` row).
const SINGLE_ROW_SQL: &str = "SELECT m.id, m.account_id, m.folder_id, f.role
       FROM messages m JOIN folders f ON f.id = m.folder_id
      WHERE m.id = ?1";

impl Store {
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
                .query_map(params![key], |r| {
                    Ok(TargetMessage {
                        id: MessageId(r.get(0)?),
                        account_id: AccountId(r.get(1)?),
                        folder_id: FolderId(r.get(2)?),
                        role: r.get(3)?,
                    })
                })?
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
        Ok(match reach {
            ConversationReach::Listed => rows.into_iter().filter(|m| listed(m)).collect(),
            ConversationReach::Whole => rows.into_iter().filter(|m| whole(m)).collect(),
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
    use nail_api::{Account, AccountKind, Flags, Folder, FolderRole, MessageSummary};

    fn store() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_account(&Account {
                id: "a".into(),
                name: "A".into(),
                email: "me@example.org".into(),
                kind: AccountKind::Local,
            })
            .unwrap();
        for (name, role) in [
            ("INBOX", FolderRole::Inbox),
            ("Sent", FolderRole::Sent),
            ("Drafts", FolderRole::Drafts),
            ("Trash", FolderRole::Trash),
            ("Other", FolderRole::Other),
        ] {
            store
                .upsert_folder(&Folder {
                    id: FolderId(format!("a/{name}")),
                    account_id: "a".into(),
                    parent_id: None,
                    name: name.into(),
                    path: name.into(),
                    role,
                    unread: 0,
                    total: 0,
                })
                .unwrap();
        }
        store
    }

    fn put(store: &Store, id: &str, folder: &str, minute: u32, mid: &str, from: &str) {
        let summary = MessageSummary {
            id: MessageId(id.into()),
            folder_id: FolderId(format!("a/{folder}")),
            account_id: "a".into(),
            subject: "Topic".into(),
            from: Some(nail_api::Address {
                name: None,
                email: from.into(),
            }),
            to: Vec::new(),
            date: format!("2026-09-29T10:{minute:02}:00Z"),
            flags: Flags::default(),
            has_attachments: false,
            preview: String::new(),
            thread_id: None,
            tags: Vec::new(),
            duplicates: Vec::new(),
            headers_only: false,
            gmail: None,
            exchange: None,
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
        assert!(
            store
                .conversation_targets(&t, None, &own(), Some(&nothing), ConversationReach::Newest)
                .unwrap()
                .is_empty()
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
}
