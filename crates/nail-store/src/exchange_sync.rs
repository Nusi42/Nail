//! Exchange sync bookkeeping added by schema v13 (M3c review fixes,
//! ADR 0078).
//!
//! * **Resumable full listings**: a full listing of a mail folder or PIM
//!   collection records the server ids it has seen
//!   ([`Store::add_exchange_listed`]) so the merge after the last page
//!   ([`Store::exchange_unlisted_messages`], [`Store::exchange_listed`])
//!   also works when the listing took several passes. A PIM collection's
//!   listing in progress lives in `exchange_pim_collections.full_sync_page`
//!   ([`Store::set_exchange_pim_listing`]).
//! * **Removals kept until the mailbox was synced**
//!   ([`Store::mark_exchange_removed`]): a message another client moved is
//!   found in its new folder and moved locally
//!   ([`Store::exchange_removed_by_message_id`],
//!   [`Store::clear_exchange_removed`]); what is left is deleted
//!   ([`Store::exchange_removed`]).
//! * **Meeting answers** ([`ExchangeMeetingAnswer`]): the own answer as
//!   last synced and an answer queued from Nail with its comment (sealed).

use std::collections::HashSet;

use nail_api::{AccountId, FolderId, MessageId};
use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::mail::normalize_msgid;
use crate::store::Store;

/// The store's mailbox key (`''` = the account's own mailbox).
fn mailbox_key(mailbox: Option<&str>) -> String {
    mailbox.map(str::to_ascii_lowercase).unwrap_or_default()
}

/// An answer to a meeting invitation of an Exchange calendar.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExchangeMeetingAnswer {
    /// The own participation status as the server last reported it
    /// (kebab-case `ParticipationStatus`), if known.
    pub synced_status: Option<String>,
    /// An answer queued from Nail: its comment and whether the organizer
    /// is told.
    pub queued: Option<QueuedMeetingAnswer>,
}

/// An answer queued from Nail (`RespondToInvitation`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMeetingAnswer {
    /// The comment for the organizer.
    pub comment: Option<String>,
    /// Whether the organizer gets the answer.
    pub send: bool,
}

impl Store {
    // --- Full listings ----------------------------------------------------------

    /// Forgets what a listing of `scope` has seen (a listing starts).
    pub fn clear_exchange_listed(&self, scope: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM exchange_listed WHERE scope = ?1", [scope])?;
            Ok(())
        })
    }

    /// Records server ids a listing of `scope` has seen.
    pub fn add_exchange_listed(
        &self,
        account: &AccountId,
        scope: &str,
        remote_ids: &[String],
    ) -> Result<()> {
        if remote_ids.is_empty() {
            return Ok(());
        }
        self.with_tx(|tx| {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO exchange_listed (account_id, scope, remote_id)
                 VALUES (?1, ?2, ?3)",
            )?;
            for remote in remote_ids {
                stmt.execute(params![account.as_str(), scope, remote])?;
            }
            Ok(())
        })
    }

    /// Every server id a listing of `scope` has seen.
    pub fn exchange_listed(&self, scope: &str) -> Result<HashSet<String>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare_cached("SELECT remote_id FROM exchange_listed WHERE scope = ?1")?;
            let rows = stmt.query_map([scope], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The messages of a folder with a server id its listing has not seen
    /// (local-only messages without a server id are not included).
    pub fn exchange_unlisted_messages(&self, folder: &FolderId) -> Result<Vec<MessageId>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT m.id FROM messages m
                 WHERE m.folder_id = ?1 AND m.remote_id IS NOT NULL
                   AND NOT EXISTS (SELECT 1 FROM exchange_listed l
                                   WHERE l.scope = ?1 AND l.remote_id = m.remote_id)
                 ORDER BY m.id",
            )?;
            let rows = stmt.query_map([folder.as_str()], |r| r.get::<_, String>(0))?;
            Ok(rows
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(MessageId)
                .collect())
        })
    }

    /// The listing in progress of a PIM collection (`full_sync_page`).
    pub fn exchange_pim_listing(&self, collection_id: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT full_sync_page FROM exchange_pim_collections WHERE collection_id = ?1",
                    [collection_id],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten())
        })
    }

    /// Stores a collection's cursor together with its listing in progress.
    pub fn set_exchange_pim_listing(
        &self,
        collection_id: &str,
        cursor: Option<&str>,
        listing: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE exchange_pim_collections
                 SET cursor = ?2, full_sync_page = ?3, last_sync_at = ?4
                 WHERE collection_id = ?1",
                params![collection_id, cursor, listing, now],
            )?;
            if changed == 0 {
                return Err(crate::error::Error::not_found(
                    "Exchange collection",
                    collection_id,
                ));
            }
            Ok(())
        })
    }

    // --- Removals ---------------------------------------------------------------

    /// Marks a message as removed from its folder on the server (deleted
    /// once its mailbox was synced, unless it shows up elsewhere).
    pub fn mark_exchange_removed(
        &self,
        account: &AccountId,
        mailbox: Option<&str>,
        message: &MessageId,
        now: i64,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_removed (message_id, account_id, mailbox, removed_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (message_id) DO NOTHING",
                params![
                    message.as_str(),
                    account.as_str(),
                    mailbox_key(mailbox),
                    now
                ],
            )?;
            Ok(())
        })
    }

    /// The messages marked as removed in a mailbox.
    pub fn exchange_removed(
        &self,
        account: &AccountId,
        mailbox: Option<&str>,
    ) -> Result<Vec<MessageId>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT message_id FROM exchange_removed
                 WHERE account_id = ?1 AND mailbox = ?2 ORDER BY message_id",
            )?;
            let rows = stmt.query_map(params![account.as_str(), mailbox_key(mailbox)], |r| {
                r.get::<_, String>(0)
            })?;
            Ok(rows
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(MessageId)
                .collect())
        })
    }

    /// The messages marked as removed in a mailbox that carry this
    /// `Message-ID`.
    pub fn exchange_removed_by_message_id(
        &self,
        account: &AccountId,
        mailbox: Option<&str>,
        message_id: &str,
    ) -> Result<Vec<MessageId>> {
        let Some(mid) = normalize_msgid(message_id) else {
            return Ok(Vec::new());
        };
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT r.message_id FROM exchange_removed r
                 JOIN messages m ON m.id = r.message_id
                 WHERE r.account_id = ?1 AND r.mailbox = ?2 AND m.message_id = ?3
                 ORDER BY r.removed_at, r.message_id",
            )?;
            let rows = stmt
                .query_map(params![account.as_str(), mailbox_key(mailbox), mid], |r| {
                    r.get::<_, String>(0)
                })?;
            Ok(rows
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(MessageId)
                .collect())
        })
    }

    /// The message is on the server after all (it moved).
    pub fn clear_exchange_removed(&self, message: &MessageId) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM exchange_removed WHERE message_id = ?1",
                [message.as_str()],
            )?;
            Ok(())
        })
    }

    // --- Meeting answers --------------------------------------------------------

    /// The answer record of a meeting (by collection and UID).
    pub fn exchange_meeting_answer(
        &self,
        collection_id: &str,
        uid: &str,
    ) -> Result<Option<ExchangeMeetingAnswer>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT synced_status, queued, send,
                            nail_open('exchange_meeting_answers.comment', comment)
                     FROM exchange_meeting_answers WHERE collection_id = ?1 AND uid = ?2",
                    params![collection_id, uid],
                    |r| {
                        let queued: bool = r.get(1)?;
                        Ok(ExchangeMeetingAnswer {
                            synced_status: r.get(0)?,
                            queued: queued
                                .then(|| -> rusqlite::Result<QueuedMeetingAnswer> {
                                    Ok(QueuedMeetingAnswer {
                                        send: r.get(2)?,
                                        comment: r.get(3)?,
                                    })
                                })
                                .transpose()?,
                        })
                    },
                )
                .optional()?)
        })
    }

    /// Records the own answer the server reported for a meeting.
    pub fn set_exchange_synced_answer(
        &self,
        collection_id: &str,
        uid: &str,
        status: Option<&str>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_meeting_answers (collection_id, uid, synced_status)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT (collection_id, uid) DO UPDATE SET synced_status = excluded.synced_status",
                params![collection_id, uid, status],
            )?;
            Ok(())
        })
    }

    /// Queues an answer from Nail: the next replay of the meeting answers
    /// it through the meeting API with this comment.
    pub fn queue_exchange_meeting_answer(
        &self,
        collection_id: &str,
        uid: &str,
        comment: Option<&str>,
        send: bool,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_meeting_answers (collection_id, uid, queued, send, comment)
                 VALUES (?1, ?2, 1, ?3, nail_seal('exchange_meeting_answers.comment', ?4))
                 ON CONFLICT (collection_id, uid) DO UPDATE SET
                     queued = 1, send = excluded.send, comment = excluded.comment",
                params![collection_id, uid, send, comment],
            )?;
            Ok(())
        })
    }

    /// The queued answer went out: the server now has `status`.
    pub fn finish_exchange_meeting_answer(
        &self,
        collection_id: &str,
        uid: &str,
        status: Option<&str>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_meeting_answers (collection_id, uid, synced_status)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT (collection_id, uid) DO UPDATE SET
                     synced_status = excluded.synced_status, queued = 0, send = 1,
                     comment = NULL",
                params![collection_id, uid, status],
            )?;
            Ok(())
        })
    }

    /// Drops a queued answer (it could not be stored).
    pub fn drop_exchange_meeting_answer(&self, collection_id: &str, uid: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE exchange_meeting_answers SET queued = 0, send = 1, comment = NULL
                 WHERE collection_id = ?1 AND uid = ?2",
                params![collection_id, uid],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredMessage;
    use crate::exchange::ExchangePimCollectionRecord;
    use nail_api::pim::{Calendar, CalendarComponent, CalendarSettings};
    use nail_api::{Account, AccountKind, CalendarId, Flags, Folder, MessageSummary};

    fn account() -> AccountId {
        AccountId::new("work")
    }

    fn store() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_account(&Account {
                id: account(),
                name: "Work".into(),
                email: "ada@contoso.example".into(),
                kind: AccountKind::Exchange,
            })
            .unwrap();
        store
            .upsert_folder(&Folder {
                id: "x-inbox".into(),
                account_id: account(),
                parent_id: None,
                name: "Inbox".into(),
                path: "Inbox".into(),
                role: nail_api::FolderRole::Inbox,
                unread: 0,
                total: 0,
            })
            .unwrap();
        for id in ["m1", "m2", "m3"] {
            store
                .upsert_message(&StoredMessage::new(MessageSummary {
                    id: id.into(),
                    folder_id: "x-inbox".into(),
                    account_id: account(),
                    subject: id.into(),
                    from: None,
                    to: vec![],
                    date: "2026-10-01T10:00:00Z".into(),
                    flags: Flags::default(),
                    has_attachments: false,
                    preview: String::new(),
                    thread_id: None,
                    tags: vec![],
                    duplicates: vec![],
                    headers_only: false,
                    gmail: None,
                    exchange: None,
                }))
                .unwrap();
        }
        store
            .set_message_remote_id(&"m1".into(), Some("R1"))
            .unwrap();
        store
            .set_message_remote_id(&"m2".into(), Some("R2"))
            .unwrap();
        // m3 is local only (no server id).
        store
    }

    #[test]
    fn a_listing_spanning_passes_finds_what_the_server_no_longer_has() {
        let store = store();
        let folder: FolderId = "x-inbox".into();
        store.clear_exchange_listed(folder.as_str()).unwrap();
        store
            .add_exchange_listed(&account(), folder.as_str(), &["R1".into()])
            .unwrap();
        // A later pass goes on with the listing: what was seen is kept.
        store
            .add_exchange_listed(&account(), folder.as_str(), &["R1".into(), "R9".into()])
            .unwrap();
        assert_eq!(
            store.exchange_unlisted_messages(&folder).unwrap(),
            vec![MessageId::from("m2")]
        );
        assert_eq!(store.exchange_listed(folder.as_str()).unwrap().len(), 2);
        store.clear_exchange_listed(folder.as_str()).unwrap();
        assert!(store.exchange_listed(folder.as_str()).unwrap().is_empty());
    }

    #[test]
    fn removals_wait_and_can_be_found_by_message_id() {
        let store = store();
        store
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE messages SET message_id = 'moved@example' WHERE id = 'm1'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        store
            .mark_exchange_removed(&account(), None, &"m1".into(), 5)
            .unwrap();
        store
            .mark_exchange_removed(&account(), Some("Boss@Contoso.example"), &"m2".into(), 5)
            .unwrap();
        assert_eq!(
            store.exchange_removed(&account(), None).unwrap(),
            vec![MessageId::from("m1")]
        );
        assert_eq!(
            store
                .exchange_removed(&account(), Some("boss@contoso.example"))
                .unwrap(),
            vec![MessageId::from("m2")]
        );
        assert_eq!(
            store
                .exchange_removed_by_message_id(&account(), None, "<moved@example>")
                .unwrap(),
            vec![MessageId::from("m1")]
        );
        assert!(
            store
                .exchange_removed_by_message_id(
                    &account(),
                    Some("boss@contoso.example"),
                    "moved@example"
                )
                .unwrap()
                .is_empty()
        );
        store.clear_exchange_removed(&"m1".into()).unwrap();
        assert!(store.exchange_removed(&account(), None).unwrap().is_empty());
        // Deleting the message drops its mark.
        store.delete_message(&"m2".into()).unwrap();
        assert!(
            store
                .exchange_removed(&account(), Some("boss@contoso.example"))
                .unwrap()
                .is_empty()
        );
    }

    fn calendar(store: &Store) -> String {
        let source = store.ensure_exchange_pim_source(&account(), 1).unwrap();
        let calendar = Calendar {
            id: CalendarId::new("exch-cal-1"),
            source_id: source,
            account_id: Some(account()),
            settings: CalendarSettings::named("Calendar"),
            components: vec![CalendarComponent::Event],
            read_only: false,
            timezone: None,
            exchange: None,
        };
        store.save_calendar(&calendar, Some("AAMkCal")).unwrap();
        store
            .upsert_exchange_pim_collection(
                &account(),
                &ExchangePimCollectionRecord {
                    collection_id: "exch-cal-1".into(),
                    mailbox: None,
                    remote_id: "AAMkCal".into(),
                    change_key: None,
                    role: None,
                    todo: false,
                    cursor: None,
                    last_sync_at: None,
                },
            )
            .unwrap();
        "exch-cal-1".into()
    }

    #[test]
    fn a_pim_listing_survives_collection_updates() {
        let store = store();
        let key = calendar(&store);
        assert_eq!(store.exchange_pim_listing(&key).unwrap(), None);
        store
            .set_exchange_pim_listing(&key, None, Some("full:page-2"), 9)
            .unwrap();
        // The collection row is written again every pass.
        store
            .upsert_exchange_pim_collection(
                &account(),
                &ExchangePimCollectionRecord {
                    collection_id: key.clone(),
                    mailbox: None,
                    remote_id: "AAMkCal".into(),
                    change_key: Some("ck2".into()),
                    role: None,
                    todo: false,
                    cursor: None,
                    last_sync_at: Some(9),
                },
            )
            .unwrap();
        assert_eq!(
            store.exchange_pim_listing(&key).unwrap().as_deref(),
            Some("full:page-2")
        );
        store
            .set_exchange_pim_listing(&key, Some("delta-9"), None, 10)
            .unwrap();
        assert_eq!(store.exchange_pim_listing(&key).unwrap(), None);
        assert!(
            store
                .set_exchange_pim_listing("nope", None, None, 1)
                .is_err()
        );
    }

    #[test]
    fn meeting_answers_queue_and_finish() {
        let store = store();
        let key = calendar(&store);
        assert_eq!(store.exchange_meeting_answer(&key, "uid-1").unwrap(), None);
        store
            .set_exchange_synced_answer(&key, "uid-1", Some("accepted"))
            .unwrap();
        assert_eq!(
            store.exchange_meeting_answer(&key, "uid-1").unwrap(),
            Some(ExchangeMeetingAnswer {
                synced_status: Some("accepted".into()),
                queued: None,
            })
        );
        store
            .queue_exchange_meeting_answer(&key, "uid-1", Some("Running late"), false)
            .unwrap();
        let answer = store
            .exchange_meeting_answer(&key, "uid-1")
            .unwrap()
            .unwrap();
        assert_eq!(answer.synced_status.as_deref(), Some("accepted"));
        assert_eq!(
            answer.queued,
            Some(QueuedMeetingAnswer {
                comment: Some("Running late".into()),
                send: false,
            })
        );
        // A sync keeps the queued answer.
        store
            .set_exchange_synced_answer(&key, "uid-1", Some("accepted"))
            .unwrap();
        assert!(
            store
                .exchange_meeting_answer(&key, "uid-1")
                .unwrap()
                .unwrap()
                .queued
                .is_some()
        );
        store
            .finish_exchange_meeting_answer(&key, "uid-1", Some("tentative"))
            .unwrap();
        assert_eq!(
            store.exchange_meeting_answer(&key, "uid-1").unwrap(),
            Some(ExchangeMeetingAnswer {
                synced_status: Some("tentative".into()),
                queued: None,
            })
        );
        store
            .queue_exchange_meeting_answer(&key, "uid-2", None, true)
            .unwrap();
        store.drop_exchange_meeting_answer(&key, "uid-2").unwrap();
        assert_eq!(
            store.exchange_meeting_answer(&key, "uid-2").unwrap(),
            Some(ExchangeMeetingAnswer::default())
        );
    }
}
