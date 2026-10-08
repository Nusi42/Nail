//! Mail action batches with one undo (P3, ADR 0164; WP-actions-core).
//!
//! `ApplyMailAction` records its queued operations and their inverses in
//! `mail_batches` / `mail_batch_ops` (schema v17) in the same transaction
//! as the local change and the queue entries, like a rule run's actions
//! (`rules.rs`): [`Store::apply_mail_batch`]. `UndoMailAction` queues the
//! inverses newest first in one transaction and marks the batch `undone`
//! ([`Store::undo_mail_batch`]); an inverse that no longer applies (the
//! message moved or was deleted since) is skipped. Batches older than
//! [`MAIL_UNDO_SECS`], and all but each actor's newest [`MAIL_UNDO_KEEP`], are purged
//! on every write.
//!
//! # Inverses
//!
//! Computed from the message's state right before its operation applies:
//!
//! * a flag / tag change → the previous value of every flag it changes, the
//!   tags it added removed and the tags it removed added again; a change
//!   that changes nothing locally is **not queued at all** (marking a read
//!   message read costs no server call);
//! * a move, or a delete to Trash → a move back to the folder it came from.
//!   The inverse's `source` names where the batch put the message: undo
//!   skips it when the message is no longer there (moved again since);
//! * a permanent delete (also Trash on a message already in Trash) → none:
//!   the batch cannot be undone ([`MailBatchApplied::undoable`] is false and
//!   no batch is recorded).

use nail_api::mail_actions::{MAIL_UNDO_KEEP, MAIL_UNDO_SECS};
use nail_api::{FolderId, MessageId};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::error::{Error, Result};
use crate::mail::message_in_tx;
use crate::queue::{
    Enqueued, FlagChange, MessageSource, Operation, enqueue_if_applies, enqueue_in_tx,
    role_folder_in_tx,
};
use crate::store::Store;

/// A batch's id as the API names it (`batch-<n>`).
#[must_use]
pub fn mail_batch_id(n: i64) -> String {
    format!("batch-{n}")
}

/// The row id of a `batch-<n>` id; `None` for anything else.
#[must_use]
pub fn parse_mail_batch_id(id: &str) -> Option<i64> {
    let n = id.strip_prefix("batch-")?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok().filter(|n| *n > 0)
}

/// What [`Store::apply_mail_batch`] should record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailBatchPlan<'a> {
    /// Who acts (`user`, `remote:<session>`, `mcp:<client>`,
    /// `plugin:<id>`); only this actor may undo.
    pub actor: &'a str,
    /// `nail_api::mail_actions::MailActionKind` wire name.
    pub kind: &'a str,
    /// Conversation targets that reached at least one message.
    pub conversations: u32,
    /// Whether the action can be undone at all (`false`: delete
    /// permanently; no batch is recorded).
    pub undoable: bool,
    /// Now (Unix seconds).
    pub now: i64,
}

/// What [`Store::apply_mail_batch`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailBatchApplied {
    /// The recorded batch (`None`: nothing queued, or not undoable).
    pub batch: Option<i64>,
    /// The queued operations, in order.
    pub queued: Vec<Enqueued>,
    /// Messages whose operation was left out because it changes nothing.
    pub unchanged: Vec<MessageId>,
    /// Whether every queued operation has an inverse.
    pub undoable: bool,
}

/// A recorded batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailBatchRecord {
    /// Row id (`batch-<n>`).
    pub id: i64,
    /// Who acted.
    pub actor: String,
    /// `MailActionKind` wire name.
    pub kind: String,
    /// Whether it was undone.
    pub undone: bool,
    /// When (Unix seconds).
    pub created_at: i64,
    /// The messages it touched, in order, each once.
    pub message_ids: Vec<MessageId>,
    /// Conversations it reached.
    pub conversations: u32,
}

/// What [`Store::undo_mail_batch`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailBatchUndo {
    /// The inverses are queued; the batch is `undone`.
    Undone {
        /// The batch (as it was before the undo).
        record: MailBatchRecord,
        /// The queued inverses, newest first.
        queued: Vec<Enqueued>,
        /// Inverses skipped because they no longer apply.
        skipped: usize,
    },
    /// Unknown, purged, older than [`MAIL_UNDO_SECS`], or another actor's
    /// (never told apart, so an actor learns nothing of others' batches).
    Expired,
    /// Undone before.
    UndoneAlready,
}

/// The inverse of a flag / tag change on a message with `flags` and
/// `tags`; `None` when the change changes nothing.
fn flag_inverse(
    change: &FlagChange,
    flags: nail_api::Flags,
    tags: &[String],
) -> Option<FlagChange> {
    let undo = |new: Option<bool>, old: bool| new.filter(|n| *n != old).map(|_| old);
    let inverse = FlagChange {
        seen: undo(change.seen, flags.seen),
        flagged: undo(change.flagged, flags.flagged),
        answered: undo(change.answered, flags.answered),
        draft: undo(change.draft, flags.draft),
        add_tags: change
            .remove_tags
            .iter()
            .filter(|t| tags.contains(t))
            .cloned()
            .collect(),
        remove_tags: change
            .add_tags
            .iter()
            .filter(|t| !t.is_empty() && !tags.contains(t) && !change.remove_tags.contains(t))
            .cloned()
            .collect(),
    };
    (!inverse.is_empty()).then_some(inverse)
}

/// A move back to `before`, expecting the message in `after`.
fn move_back(message: &MessageId, before: &MessageSource, after: &FolderId) -> Operation {
    Operation::MoveMessage {
        message_id: message.clone(),
        target_folder_id: before.folder_id.clone(),
        source: Some(MessageSource {
            account_id: before.account_id.clone(),
            folder_id: after.clone(),
            uid: None,
            uid_validity: None,
            header_message_id: None,
            remote_id: None,
        }),
    }
}

enum Planned {
    /// Queue it; `Some` = its inverse.
    Queue(Option<Operation>),
    /// It changes nothing.
    Skip,
}

/// Plans one operation against the message's current state.
fn plan_one(tx: &Transaction<'_>, op: &Operation) -> Result<Planned> {
    let message = op.message_id();
    let stored = message_in_tx(tx, message)?;
    let here = MessageSource {
        account_id: stored.summary.account_id.clone(),
        folder_id: stored.summary.folder_id.clone(),
        uid: None,
        uid_validity: None,
        header_message_id: None,
        remote_id: None,
    };
    Ok(match op {
        Operation::UpdateFlags { change, .. } => {
            match flag_inverse(change, stored.summary.flags, &stored.summary.tags) {
                Some(inverse) => Planned::Queue(Some(Operation::UpdateFlags {
                    message_id: message.clone(),
                    change: inverse,
                })),
                None => Planned::Skip,
            }
        }
        Operation::MoveMessage {
            target_folder_id, ..
        } => {
            if *target_folder_id == here.folder_id {
                Planned::Skip
            } else {
                Planned::Queue(Some(move_back(message, &here, target_folder_id)))
            }
        }
        Operation::DeleteMessage { permanent, .. } => {
            let trash = if *permanent {
                None
            } else {
                role_folder_in_tx(tx, &here.account_id, nail_api::FolderRole::Trash)?
                    .filter(|t| *t != here.folder_id)
            };
            Planned::Queue(trash.map(|trash| move_back(message, &here, &trash)))
        }
        _ => Planned::Queue(None),
    })
}

/// Drops batches older than [`MAIL_UNDO_SECS`] and all but `actor`'s newest
/// [`MAIL_UNDO_KEEP`] (the limit is per actor, so an agent's or another
/// session's writes never evict the user's undo).
fn purge(tx: &Transaction<'_>, actor: &str, now: i64) -> Result<()> {
    let oldest = now.saturating_sub(i64::try_from(MAIL_UNDO_SECS).unwrap_or(i64::MAX));
    tx.execute("DELETE FROM mail_batches WHERE created_at < ?1", [oldest])?;
    tx.execute(
        "DELETE FROM mail_batches WHERE actor = ?1 AND id NOT IN
           (SELECT id FROM mail_batches WHERE actor = ?1 ORDER BY id DESC LIMIT ?2)",
        params![actor, i64::try_from(MAIL_UNDO_KEEP).unwrap_or(i64::MAX)],
    )?;
    Ok(())
}

fn batch_in_tx(tx: &Transaction<'_>, id: i64) -> Result<Option<MailBatchRecord>> {
    let row = tx
        .query_row(
            "SELECT actor, kind, state, created_at, conversations FROM mail_batches WHERE id = ?1",
            [id],
            |r| <(String, String, String, i64, u32)>::try_from(r),
        )
        .optional()?;
    let Some((actor, kind, state, created_at, conversations)) = row else {
        return Ok(None);
    };
    let message_ids = {
        let mut stmt = tx.prepare_cached(
            "SELECT message_id FROM mail_batch_ops WHERE batch_id = ?1 ORDER BY seq",
        )?;
        let mut ids: Vec<MessageId> = Vec::new();
        for id in stmt.query_map([id], |r| r.get::<_, String>(0))? {
            let id = MessageId(id?);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    };
    Ok(Some(MailBatchRecord {
        id,
        actor,
        kind,
        undone: state == "undone",
        created_at,
        message_ids,
        conversations,
    }))
}

/// The folder a message is in now (`None` when it is gone).
fn current_folder(tx: &Transaction<'_>, id: &MessageId) -> Result<Option<FolderId>> {
    Ok(tx
        .query_row(
            "SELECT folder_id FROM messages WHERE id = ?1",
            [id.as_str()],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .map(FolderId))
}

impl Store {
    /// Applies a mail action's operations locally, queues them and records
    /// them with their inverses as one batch — all in one transaction (all
    /// or nothing). Operations that change nothing are left out
    /// ([`MailBatchApplied::unchanged`]). A batch is recorded only when
    /// something was queued, `plan.undoable` holds and every queued
    /// operation has an inverse.
    pub fn apply_mail_batch(
        &self,
        plan: &MailBatchPlan<'_>,
        operations: &[Operation],
    ) -> Result<MailBatchApplied> {
        self.with_tx(|tx| {
            let mut queued = Vec::new();
            let mut unchanged = Vec::new();
            let mut inverses: Vec<(MessageId, i64, Option<Operation>)> = Vec::new();
            for op in operations {
                match plan_one(tx, op)? {
                    Planned::Skip => unchanged.push(op.message_id().clone()),
                    Planned::Queue(inverse) => {
                        let enqueued = enqueue_in_tx(tx, op)?;
                        inverses.push((op.message_id().clone(), enqueued.op.id.0, inverse));
                        queued.push(enqueued);
                    }
                }
            }
            let undoable = inverses.iter().all(|(_, _, inverse)| inverse.is_some());
            let batch = if plan.undoable && undoable && !queued.is_empty() {
                let mut distinct: Vec<&MessageId> = Vec::new();
                for (id, _, _) in &inverses {
                    if !distinct.contains(&id) {
                        distinct.push(id);
                    }
                }
                tx.execute(
                    "INSERT INTO mail_batches (actor, kind, created_at, message_count, conversations)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        plan.actor,
                        plan.kind,
                        plan.now,
                        i64::try_from(distinct.len()).unwrap_or(i64::MAX),
                        plan.conversations
                    ],
                )?;
                let id = tx.last_insert_rowid();
                let mut stmt = tx.prepare_cached(
                    "INSERT INTO mail_batch_ops (batch_id, seq, message_id, op_id, undo)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                for (seq, (message, op, inverse)) in inverses.iter().enumerate() {
                    let undo = inverse.as_ref().map(serde_json::to_string).transpose()?;
                    stmt.execute(params![
                        id,
                        i64::try_from(seq).unwrap_or(i64::MAX),
                        message.as_str(),
                        op,
                        undo
                    ])?;
                }
                Some(id)
            } else {
                None
            };
            purge(tx, plan.actor, plan.now)?;
            Ok(MailBatchApplied {
                batch,
                queued,
                unchanged,
                undoable,
            })
        })
    }

    /// A recorded batch (`None` when unknown or purged).
    pub fn mail_batch(&self, id: i64) -> Result<Option<MailBatchRecord>> {
        self.with_tx(|tx| batch_in_tx(tx, id))
    }

    /// Undoes a batch of `actor` in one transaction: queues its inverses,
    /// newest first, and marks it `undone`. An inverse whose message is no
    /// longer where the batch put it, or is gone, is skipped. Answers
    /// [`MailBatchUndo::Expired`] for unknown, old or other actors'
    /// batches.
    pub fn undo_mail_batch(&self, id: i64, actor: &str, now: i64) -> Result<MailBatchUndo> {
        self.with_tx(|tx| {
            purge(tx, actor, now)?;
            let Some(record) = batch_in_tx(tx, id)? else {
                return Ok(MailBatchUndo::Expired);
            };
            if record.actor != actor {
                return Ok(MailBatchUndo::Expired);
            }
            if record.undone {
                return Ok(MailBatchUndo::UndoneAlready);
            }
            let inverses: Vec<Operation> = {
                let mut stmt = tx.prepare_cached(
                    "SELECT undo FROM mail_batch_ops
                     WHERE batch_id = ?1 AND undo IS NOT NULL ORDER BY seq DESC",
                )?;
                let rows = stmt
                    .query_map([id], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.iter()
                    .map(|s| serde_json::from_str(s).map_err(Error::from))
                    .collect::<Result<_>>()?
            };
            let mut queued = Vec::with_capacity(inverses.len());
            let mut skipped = 0;
            for inverse in inverses {
                let inverse = match inverse {
                    Operation::MoveMessage {
                        message_id,
                        target_folder_id,
                        source,
                    } => {
                        // Only while the message is where the batch put it.
                        let current = current_folder(tx, &message_id)?;
                        let expected = source.map(|s| s.folder_id);
                        if current.is_none() || expected.is_some_and(|e| Some(e) != current) {
                            skipped += 1;
                            continue;
                        }
                        Operation::MoveMessage {
                            message_id,
                            target_folder_id,
                            source: None,
                        }
                    }
                    other => other,
                };
                match enqueue_if_applies(tx, &inverse)? {
                    Some(enqueued) => queued.push(enqueued),
                    None => skipped += 1,
                }
            }
            tx.execute(
                "UPDATE mail_batches SET state = 'undone' WHERE id = ?1",
                [id],
            )?;
            Ok(MailBatchUndo::Undone {
                record,
                queued,
                skipped,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::OpState;
    use crate::test_store::{add, store_with};
    use nail_api::testing;
    use nail_api::{AccountKind, Flags, Folder, FolderRole, MessageSummary};

    use nail_util::time::FIXTURE_LATER as NOW;

    fn store() -> Store {
        let folders: Vec<Folder> = [
            ("INBOX", FolderRole::Inbox),
            ("Archive", FolderRole::Archive),
            ("Trash", FolderRole::Trash),
        ]
        .into_iter()
        .map(|(name, role)| testing::folder(&format!("a/{name}"), "a", name, role))
        .collect();
        let store = store_with(&testing::account("a", AccountKind::Local), &folders);
        for n in 1..=3 {
            add(
                &store,
                MessageSummary {
                    subject: format!("s{n}"),
                    date: "2026-09-29T10:00:00Z".into(),
                    flags: Flags {
                        seen: n == 1,
                        ..Flags::default()
                    },
                    ..testing::summary(&format!("m{n}"), "a", "a/INBOX")
                },
            );
        }
        store
    }

    fn plan(actor: &str) -> MailBatchPlan<'_> {
        MailBatchPlan {
            actor,
            kind: "archive",
            conversations: 1,
            undoable: true,
            now: NOW,
        }
    }

    fn moves(ids: &[&str], to: &str) -> Vec<Operation> {
        ids.iter()
            .map(|id| Operation::MoveMessage {
                message_id: MessageId((*id).into()),
                target_folder_id: FolderId(to.into()),
                source: None,
            })
            .collect()
    }

    fn folder_of(store: &Store, id: &str) -> String {
        store
            .get_message(&MessageId(id.into()))
            .unwrap()
            .summary
            .folder_id
            .0
    }

    #[test]
    fn batch_ids_round_trip() {
        assert_eq!(mail_batch_id(7), "batch-7");
        assert_eq!(parse_mail_batch_id("batch-7"), Some(7));
        for bad in [
            "7", "batch-", "batch-x", "batch-0", "batch--1", "batch-+1", "run-7",
        ] {
            assert_eq!(parse_mail_batch_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_batch_moves_and_undo_brings_every_message_back() {
        let store = store();
        let applied = store
            .apply_mail_batch(&plan("user"), &moves(&["m1", "m2", "m3"], "a/Archive"))
            .unwrap();
        assert_eq!(applied.queued.len(), 3);
        assert!(applied.undoable);
        let batch = applied.batch.unwrap();
        for id in ["m1", "m2", "m3"] {
            assert_eq!(folder_of(&store, id), "a/Archive");
        }
        let record = store.mail_batch(batch).unwrap().unwrap();
        assert_eq!(record.message_ids.len(), 3);
        assert_eq!(record.kind, "archive");
        assert_eq!(record.conversations, 1);
        // Moved on by hand since: its inverse is skipped.
        store.enqueue(&moves(&["m2"], "a/Trash")[0]).unwrap();
        let MailBatchUndo::Undone {
            queued, skipped, ..
        } = store.undo_mail_batch(batch, "user", NOW + 10).unwrap()
        else {
            panic!("not undone");
        };
        assert_eq!((queued.len(), skipped), (2, 1));
        assert_eq!(folder_of(&store, "m1"), "a/INBOX");
        assert_eq!(folder_of(&store, "m2"), "a/Trash");
        assert_eq!(folder_of(&store, "m3"), "a/INBOX");
        // Newest first.
        assert_eq!(queued[0].op.operation.message_id().as_str(), "m3");
        assert_eq!(
            store.undo_mail_batch(batch, "user", NOW + 11).unwrap(),
            MailBatchUndo::UndoneAlready
        );
        assert_eq!(store.list_ops(Some(OpState::Pending)).unwrap().len(), 6);
    }

    #[test]
    fn undo_is_the_actors_and_expires() {
        let store = store();
        let batch = store
            .apply_mail_batch(&plan("remote:s1"), &moves(&["m1"], "a/Archive"))
            .unwrap()
            .batch
            .unwrap();
        assert_eq!(
            store.undo_mail_batch(batch, "user", NOW).unwrap(),
            MailBatchUndo::Expired
        );
        assert_eq!(
            store.undo_mail_batch(batch + 99, "remote:s1", NOW).unwrap(),
            MailBatchUndo::Expired
        );
        let late = NOW + i64::try_from(MAIL_UNDO_SECS).unwrap() + 1;
        assert_eq!(
            store.undo_mail_batch(batch, "remote:s1", late).unwrap(),
            MailBatchUndo::Expired
        );
        assert!(store.mail_batch(batch).unwrap().is_none(), "purged");
        assert_eq!(folder_of(&store, "m1"), "a/Archive");
    }

    #[test]
    fn only_the_newest_batches_are_kept() {
        let store = store();
        let mut ids = Vec::new();
        for n in 0..(MAIL_UNDO_KEEP + 2) {
            let to = if n % 2 == 0 { "a/Archive" } else { "a/INBOX" };
            ids.push(
                store
                    .apply_mail_batch(&plan("user"), &moves(&["m1"], to))
                    .unwrap()
                    .batch
                    .unwrap(),
            );
        }
        // The next write purges all but the newest MAIL_UNDO_KEEP.
        store
            .apply_mail_batch(&plan("user"), &moves(&["m2"], "a/Archive"))
            .unwrap();
        for old in &ids[..3] {
            assert!(store.mail_batch(*old).unwrap().is_none());
        }
        assert!(store.mail_batch(ids[3]).unwrap().is_some());
    }

    #[test]
    fn other_actors_never_evict_the_users_undo() {
        let store = store();
        let mine = store
            .apply_mail_batch(&plan("user"), &moves(&["m1"], "a/Archive"))
            .unwrap()
            .batch
            .unwrap();
        // An agent toggling a message back and forth records many batches.
        for n in 0..(MAIL_UNDO_KEEP + 5) {
            let to = if n % 2 == 0 { "a/Archive" } else { "a/INBOX" };
            store
                .apply_mail_batch(&plan("mcp:c1"), &moves(&["m2"], to))
                .unwrap();
        }
        assert!(store.mail_batch(mine).unwrap().is_some());
        assert!(matches!(
            store.undo_mail_batch(mine, "user", NOW).unwrap(),
            MailBatchUndo::Undone { .. }
        ));
        assert_eq!(folder_of(&store, "m1"), "a/INBOX");
    }

    #[test]
    fn flag_changes_record_what_was_before_and_skip_no_ops() {
        let store = store();
        let read = |id: &str| Operation::UpdateFlags {
            message_id: MessageId(id.into()),
            change: FlagChange {
                seen: Some(true),
                add_tags: vec!["work".into()],
                ..FlagChange::default()
            },
        };
        // m1 is read already; it still gets the tag.
        let applied = store
            .apply_mail_batch(&plan("user"), &[read("m1"), read("m2")])
            .unwrap();
        assert_eq!(applied.queued.len(), 2);
        let again = store
            .apply_mail_batch(&plan("user"), &[read("m1"), read("m2")])
            .unwrap();
        assert!(again.queued.is_empty(), "{:?}", again.queued);
        assert_eq!(again.batch, None);
        assert_eq!(again.unchanged.len(), 2);
        let MailBatchUndo::Undone { .. } = store
            .undo_mail_batch(applied.batch.unwrap(), "user", NOW)
            .unwrap()
        else {
            panic!("not undone");
        };
        let m1 = store.get_message(&MessageId("m1".into())).unwrap().summary;
        let m2 = store.get_message(&MessageId("m2".into())).unwrap().summary;
        assert!(m1.flags.seen, "m1 was read before");
        assert!(!m2.flags.seen);
        assert!(m1.tags.is_empty() && m2.tags.is_empty());
    }

    #[test]
    fn trash_is_undoable_but_a_delete_from_trash_is_not() {
        let store = store();
        let delete = |id: &str| Operation::DeleteMessage {
            message_id: MessageId(id.into()),
            permanent: false,
            source: None,
            trash_folder_id: None,
        };
        let applied = store
            .apply_mail_batch(&plan("user"), &[delete("m1")])
            .unwrap();
        assert!(applied.batch.is_some());
        assert_eq!(folder_of(&store, "m1"), "a/Trash");
        // Already in Trash: deleted for good, no batch.
        let gone = store
            .apply_mail_batch(&plan("user"), &[delete("m1")])
            .unwrap();
        assert!(!gone.undoable);
        assert_eq!(gone.batch, None);
        assert!(store.get_message(&MessageId("m1".into())).is_err());
        let MailBatchUndo::Undone { skipped, .. } = store
            .undo_mail_batch(applied.batch.unwrap(), "user", NOW)
            .unwrap()
        else {
            panic!("not undone");
        };
        assert_eq!(skipped, 1, "the message is gone");
    }

    #[test]
    fn a_failing_operation_rolls_the_whole_batch_back() {
        let store = store();
        let mut ops = moves(&["m1"], "a/Archive");
        ops.extend(moves(&["missing"], "a/Archive"));
        assert!(store.apply_mail_batch(&plan("user"), &ops).is_err());
        assert_eq!(folder_of(&store, "m1"), "a/INBOX");
        assert_eq!(store.list_ops(None).unwrap().len(), 0);
    }

    #[test]
    fn flag_inverse_restores_only_what_changed() {
        let change = FlagChange {
            seen: Some(true),
            flagged: Some(false),
            add_tags: vec!["a".into(), "b".into()],
            remove_tags: vec!["c".into(), "d".into()],
            ..FlagChange::default()
        };
        let inverse = flag_inverse(
            &change,
            Flags {
                seen: false,
                flagged: false,
                ..Flags::default()
            },
            &["b".into(), "c".into()],
        )
        .unwrap();
        assert_eq!(inverse.seen, Some(false));
        assert_eq!(inverse.flagged, None);
        assert_eq!(inverse.add_tags, ["c"]);
        assert_eq!(inverse.remove_tags, ["a"]);
        assert!(flag_inverse(&FlagChange::default(), Flags::default(), &[]).is_none());
    }
}
