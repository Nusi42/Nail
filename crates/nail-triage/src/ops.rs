//! Operations the triage logic plans and the core executes.
//!
//! Every write of junk handling and snoozing (keywords, moves, the hidden
//! Snoozed folder) is a server-visible change, so it goes through the
//! core's operation queue (design.md: "every write goes through the
//! operation queue"). This crate never performs them: it returns
//! [`TriageOp`]s and the core maps each one onto its queue operations.
//!
//! There is deliberately **no delete operation**: junk handling never
//! deletes mail (design.md, "Spam"), and snoozing only moves.

use nail_api::{AccountId, FolderId, FolderRole, MessageId};

/// Where a [`TriageOp::Move`] goes; the core resolves it to a folder id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FolderTarget {
    /// A known folder.
    Folder(FolderId),
    /// The folder with a role in an account (Inbox, Junk).
    Role {
        /// The account.
        account_id: AccountId,
        /// The role.
        role: FolderRole,
    },
    /// The hidden [`nail_api::triage::SNOOZED_FOLDER_NAME`] folder of an
    /// account (created by a preceding [`TriageOp::EnsureSnoozedFolder`]).
    Snoozed {
        /// The account.
        account_id: AccountId,
    },
}

/// One planned write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriageOp {
    /// Make sure the hidden Snoozed folder exists on the server (for POP3
    /// accounts: locally). Idempotent; hidden from the folder tree.
    EnsureSnoozedFolder {
        /// The account.
        account_id: AccountId,
    },
    /// Set an IMAP keyword.
    AddKeyword {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// The keyword, e.g. `$Junk`.
        keyword: String,
    },
    /// Clear an IMAP keyword.
    RemoveKeyword {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// The keyword.
        keyword: String,
    },
    /// Move messages.
    Move {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// The destination.
        to: FolderTarget,
    },
}

impl TriageOp {
    /// The messages the operation touches (none for
    /// [`TriageOp::EnsureSnoozedFolder`]).
    pub fn message_ids(&self) -> &[MessageId] {
        match self {
            Self::EnsureSnoozedFolder { .. } => &[],
            Self::AddKeyword { message_ids, .. }
            | Self::RemoveKeyword { message_ids, .. }
            | Self::Move { message_ids, .. } => message_ids,
        }
    }

    /// Whether the operation could lose mail: never, by construction; a
    /// move to the Trash role would be the only way and no planner emits
    /// one (checked by the tests of every planner).
    pub fn targets_trash(&self) -> bool {
        matches!(
            self,
            Self::Move {
                to: FolderTarget::Role {
                    role: FolderRole::Trash,
                    ..
                },
                ..
            }
        )
    }
}

/// Merges operations of the same kind and target that follow each other
/// (keeps the order otherwise), so a batch of messages becomes one queue
/// operation per step.
pub fn coalesce(ops: Vec<TriageOp>) -> Vec<TriageOp> {
    let mut out: Vec<TriageOp> = Vec::with_capacity(ops.len());
    for op in ops {
        let merged = match (out.last_mut(), &op) {
            (
                Some(TriageOp::AddKeyword {
                    message_ids,
                    keyword,
                }),
                TriageOp::AddKeyword {
                    message_ids: more,
                    keyword: k,
                },
            )
            | (
                Some(TriageOp::RemoveKeyword {
                    message_ids,
                    keyword,
                }),
                TriageOp::RemoveKeyword {
                    message_ids: more,
                    keyword: k,
                },
            ) if keyword == k => {
                extend_unique(message_ids, more);
                true
            }
            (
                Some(TriageOp::Move { message_ids, to }),
                TriageOp::Move {
                    message_ids: more,
                    to: t,
                },
            ) if to == t => {
                extend_unique(message_ids, more);
                true
            }
            (
                Some(TriageOp::EnsureSnoozedFolder { account_id }),
                TriageOp::EnsureSnoozedFolder { account_id: a },
            ) => account_id == a,
            _ => false,
        };
        if !merged {
            out.push(op);
        }
    }
    out
}

fn extend_unique(ids: &mut Vec<MessageId>, more: &[MessageId]) {
    for id in more {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn add(ids: &[&str], k: &str) -> TriageOp {
        TriageOp::AddKeyword {
            message_ids: ids.iter().map(|i| MessageId::from(*i)).collect(),
            keyword: k.to_owned(),
        }
    }

    #[test]
    fn coalesce_merges_neighbours_only() {
        let junk = FolderTarget::Role {
            account_id: "a".into(),
            role: FolderRole::Junk,
        };
        let mv = |ids: &[&str]| TriageOp::Move {
            message_ids: ids.iter().map(|i| MessageId::from(*i)).collect(),
            to: junk.clone(),
        };
        let ops = vec![
            add(&["m1"], "$Junk"),
            add(&["m2", "m1"], "$Junk"),
            add(&["m3"], "$NotJunk"),
            mv(&["m1"]),
            mv(&["m2"]),
            add(&["m4"], "$Junk"),
        ];
        assert_eq!(
            coalesce(ops),
            vec![
                add(&["m1", "m2"], "$Junk"),
                add(&["m3"], "$NotJunk"),
                mv(&["m1", "m2"]),
                add(&["m4"], "$Junk"),
            ]
        );
        let ensure = |a: &str| TriageOp::EnsureSnoozedFolder {
            account_id: a.into(),
        };
        assert_eq!(
            coalesce(vec![ensure("a"), ensure("a"), ensure("b")]),
            vec![ensure("a"), ensure("b")]
        );
    }

    #[test]
    fn trash_detection() {
        let op = TriageOp::Move {
            message_ids: vec!["m".into()],
            to: FolderTarget::Role {
                account_id: "a".into(),
                role: FolderRole::Trash,
            },
        };
        assert!(op.targets_trash());
        assert!(!add(&["m"], "$Junk").targets_trash());
        assert_eq!(op.message_ids(), [MessageId::from("m")]);
    }
}
