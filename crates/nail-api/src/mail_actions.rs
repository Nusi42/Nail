//! Mail actions on conversations (P3, ADR 0164).
//!
//! A list row is a conversation (P2, ADR 0148): the thread's messages in the
//! listed folder plus the user's own replies from the accounts' Sent
//! folders. P3 makes the actions on such a row behave like Gmail instead of
//! acting on the row's newest message only:
//!
//! * [`crate::Request::ApplyMailAction`] takes [`MessageTarget`]s — single
//!   messages (a card's own actions) or conversations as a list row shows
//!   them ([`ConversationRef`]: thread + listed folder, real or virtual) —
//!   and one [`MailAction`]. The core resolves every target to messages
//!   ([`MailAction::reach`] says which messages of a conversation), applies
//!   the change locally at once, queues it per account through the
//!   operation queue and records the inverse operations as **one batch**
//!   with one undo ([`MailActionResult`]).
//! * [`crate::Request::ResolveMailTargets`] answers which messages an action
//!   would touch without changing anything (confirmations: "Delete 7
//!   messages permanently?").
//! * [`crate::Request::UndoMailAction`] queues a batch's inverse operations
//!   (moves back to where each message was, flags and tags as they were).
//!
//! The older per-message requests (`UpdateFlags`, `MoveMessages`,
//! `DeleteMessages`, `MarkJunk`, `SnoozeMessages`) stay as they are, without
//! undo.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::agents::ActionKind;
use crate::ids::{AccountId, FolderId, MailBatchId, MessageId, OpId, ThreadId};
use crate::ops::MoveTarget;

/// Most targets one [`crate::Request::ApplyMailAction`] /
/// [`crate::Request::ResolveMailTargets`] may name.
pub const MAX_MAIL_ACTION_TARGETS: usize = 500;
/// Most messages the targets of one action may resolve to; more fail with
/// `invalid_request` ([`MailActionIssue::TooManyMessages`]).
pub const MAX_MAIL_ACTION_MESSAGES: usize = 10_000;
/// How long a batch can be undone (seconds).
pub const MAIL_UNDO_SECS: u64 = 3600;
/// How many batches the core keeps for undo per actor (the oldest go
/// first; another actor's batches never evict the user's).
pub const MAIL_UNDO_KEEP: usize = 50;

/// A conversation as a list row shows it: the thread and the folder it is
/// listed in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct ConversationRef {
    /// The thread ([`crate::ThreadSummary::id`]); a `single-<message id>`
    /// row (a draft) is that one message.
    pub thread_id: ThreadId,
    /// The folder (real or virtual, e.g. the unified inbox) the row is
    /// listed in; `null` = shown without a folder (search hits): the
    /// thread's messages in every folder but Drafts, Trash and Junk, as
    /// [`crate::Request::GetConversation`] without a folder.
    #[serde(default)]
    pub folder_id: Option<FolderId>,
}

/// What a mail action acts on.
///
/// JSON: `{"kind": "message", "message_id": "…"}` or
/// `{"kind": "conversation", "thread_id": "…", "folder_id": "…" | null}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MessageTarget {
    /// One message (a card's own actions, a child row of an expandable
    /// list, a list without conversations).
    Message {
        /// The message.
        message_id: MessageId,
    },
    /// A conversation as listed ([`MailAction::reach`] picks its messages).
    Conversation(ConversationRef),
}

impl MessageTarget {
    /// A single-message target.
    pub fn message(id: impl Into<MessageId>) -> Self {
        Self::Message {
            message_id: id.into(),
        }
    }

    /// A conversation target.
    pub fn conversation(thread: impl Into<ThreadId>, folder: Option<FolderId>) -> Self {
        Self::Conversation(ConversationRef {
            thread_id: thread.into(),
            folder_id: folder,
        })
    }
}

/// One mail action. Absolute values (never toggles), so a repeated request
/// changes nothing more.
///
/// JSON: `{"type": "archive"}`, `{"type": "move", "target": {"role":
/// "archive"}}`, `{"type": "mark-read", "read": true}`, …
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MailAction {
    /// Move to each account's Archive folder (messages already there stay).
    Archive,
    /// Move to each account's Trash ("Delete"), like Gmail the whole
    /// conversation ([`ConversationReach::Thread`]). Messages already in
    /// Trash stay there: Trash never deletes for good (that is
    /// [`MailAction::DeletePermanently`], with its own scope and
    /// confirmation).
    Trash,
    /// Delete for good. Only messages in Trash or Junk; any other message
    /// among the targets fails the whole request
    /// ([`MailActionIssue::NotInTrash`]). Never undoable; the UI confirms
    /// first.
    DeletePermanently,
    /// Move to a folder or a role's folder. Moving a conversation from
    /// Trash to the Inbox restores the user's own messages (sent from one
    /// of the account's addresses) to the account's Sent folder instead.
    Move {
        /// Where to.
        target: MoveTarget,
    },
    /// Mark read (`true`) or unread (`false`).
    MarkRead {
        /// The new state.
        read: bool,
    },
    /// Flag (`true`: the conversation's newest message, like Gmail's star)
    /// or unflag (`false`: every flagged message of the conversation).
    Flag {
        /// The new state.
        flagged: bool,
    },
    /// Add and remove tags (IMAP keywords; Gmail user labels and Exchange
    /// categories are tags).
    Tags {
        /// Tags to add.
        #[serde(default)]
        add: Vec<String>,
        /// Tags to remove.
        #[serde(default)]
        remove: Vec<String>,
    },
    /// Snooze until a time (RFC 3339); messages come back to the folder each
    /// was in, or to `wake_folder_id`.
    Snooze {
        /// When they come back.
        until: String,
        /// Where they come back to; `null` = where each was.
        #[serde(default)]
        wake_folder_id: Option<FolderId>,
    },
    /// Mark as junk (`true`: train, `$Junk`, move to Junk) or not junk
    /// (`false`: train, `$NotJunk`, move from Junk to the Inbox).
    Junk {
        /// The new state.
        junk: bool,
    },
}

/// Which messages of a conversation an action reaches (ADR 0164).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationReach {
    /// The conversation's messages in the listed folder (a virtual folder:
    /// in each of its member folders; without a folder: in every folder
    /// but Sent, Drafts, Trash and Junk). The user's replies joined from
    /// Sent stay where they are.
    Listed,
    /// The listed messages plus the user's own replies joined from the
    /// accounts' Sent folders — every message the row counts. Never drafts.
    Whole,
    /// One message: the conversation's newest message the user did not
    /// write, else its newest ([`crate::ThreadSummary::latest_message_id`]).
    Newest,
    /// The whole thread like Gmail's Delete: [`ConversationReach::Whole`]
    /// plus the thread's messages in every other folder of the listed
    /// folders' accounts (without a folder: of every account) but Drafts,
    /// Trash and Junk — e.g. its archived messages.
    Thread,
}

impl MailAction {
    /// The label / policy kind of this action.
    pub fn kind(&self) -> MailActionKind {
        match self {
            Self::Archive => MailActionKind::Archive,
            Self::Trash => MailActionKind::Trash,
            Self::DeletePermanently => MailActionKind::DeletePermanently,
            Self::Move { .. } => MailActionKind::Move,
            Self::MarkRead { read: true } => MailActionKind::MarkRead,
            Self::MarkRead { read: false } => MailActionKind::MarkUnread,
            Self::Flag { flagged: true } => MailActionKind::Flag,
            Self::Flag { flagged: false } => MailActionKind::Unflag,
            Self::Tags { .. } => MailActionKind::Tags,
            Self::Snooze { .. } => MailActionKind::Snooze,
            Self::Junk { junk: true } => MailActionKind::Junk,
            Self::Junk { junk: false } => MailActionKind::NotJunk,
        }
    }

    /// Which messages of a conversation target this action reaches.
    pub fn reach(&self) -> ConversationReach {
        match self {
            Self::Archive
            | Self::Move { .. }
            | Self::Snooze { .. }
            | Self::Junk { .. }
            | Self::DeletePermanently => ConversationReach::Listed,
            Self::Flag { flagged: true } => ConversationReach::Newest,
            Self::Trash => ConversationReach::Thread,
            Self::MarkRead { .. } | Self::Tags { .. } | Self::Flag { flagged: false } => {
                ConversationReach::Whole
            }
        }
    }

    /// Whether the touched messages leave the listed folder (the UI drops
    /// the row at once and moves the selection on).
    pub fn removes_from_list(&self) -> bool {
        matches!(
            self,
            Self::Archive
                | Self::Trash
                | Self::DeletePermanently
                | Self::Move { .. }
                | Self::Snooze { .. }
                | Self::Junk { .. }
        )
    }

    /// Whether a batch of this action can be undone.
    pub fn undoable(&self) -> bool {
        !matches!(self, Self::DeletePermanently)
    }

    /// The agent policy kind (MCP / plug-in writes, ADR 0079): destructive
    /// kinds always ask.
    pub fn action_kind(&self) -> ActionKind {
        match self {
            Self::Trash => ActionKind::Trash,
            Self::DeletePermanently => ActionKind::PermanentDelete,
            _ => ActionKind::Organize,
        }
    }

    /// Whether the action changes nothing (a tag change without tags).
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Tags { add, remove } if add.is_empty() && remove.is_empty())
    }
}

labeled_enum! {
    /// What an action did, for labels (`mail-action-kind-*` in
    /// `core-mail-actions.ftl`), undo toasts, confirmations and audit lines.
    pub enum MailActionKind : "mail-action-kind" {
        /// Archived.
        Archive = "archive",
        /// Moved to Trash.
        Trash = "trash",
        /// Deleted for good.
        DeletePermanently = "delete-permanently",
        /// Moved.
        Move = "move",
        /// Marked read.
        MarkRead = "mark-read",
        /// Marked unread.
        MarkUnread = "mark-unread",
        /// Flagged.
        Flag = "flag",
        /// Unflagged.
        Unflag = "unflag",
        /// Tags changed.
        Tags = "tags",
        /// Snoozed.
        Snooze = "snooze",
        /// Marked as junk.
        Junk = "junk",
        /// Marked as not junk.
        NotJunk = "not-junk",
    }
}

labeled_enum! {
    /// Why an action or an undo was refused (`mail-action-issue-*`, the
    /// `localized` id of the error).
    pub enum MailActionIssue : "mail-action-issue" {
        /// No targets.
        NoTargets = "no-targets",
        /// More than [`MAX_MAIL_ACTION_TARGETS`] targets.
        TooManyTargets = "too-many-targets",
        /// The targets resolve to more than [`MAX_MAIL_ACTION_MESSAGES`].
        TooManyMessages = "too-many-messages",
        /// The targets resolve to no message.
        NoMessages = "no-messages",
        /// A tag change without tags.
        NothingToChange = "nothing-to-change",
        /// Delete permanently on a message outside Trash and Junk.
        NotInTrash = "not-in-trash",
        /// The batch is unknown or older than [`MAIL_UNDO_SECS`].
        UndoExpired = "undo-expired",
        /// The batch was undone already.
        UndoneAlready = "undone-already",
        /// The batch cannot be undone (delete permanently).
        NotUndoable = "not-undoable",
    }
}

/// What [`crate::Request::ApplyMailAction`] did
/// ([`crate::Response::MailActionQueued`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MailActionResult {
    /// The batch to undo ([`crate::Request::UndoMailAction`]); `null` when
    /// it cannot be undone (delete permanently, messages deleted from
    /// Trash, or a core without undo).
    pub batch_id: Option<MailBatchId>,
    /// The queued operations, for matching [`crate::Event::OpStatus`].
    pub op_ids: Vec<OpId>,
    /// What was done.
    pub kind: MailActionKind,
    /// Every message the action touched (the UI patches its caches).
    pub message_ids: Vec<MessageId>,
    /// How many conversation targets touched at least one message.
    pub conversations: u32,
    /// Until when the batch can be undone (RFC 3339); `null` with
    /// `batch_id`.
    pub undo_until: Option<String>,
}

/// What [`crate::Request::ResolveMailTargets`] found
/// ([`crate::Response::MailTargets`]).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct MailTargetResolution {
    /// Every message the action would touch.
    pub message_ids: Vec<MessageId>,
    /// Their accounts.
    pub account_ids: Vec<AccountId>,
    /// How many conversation targets resolve to at least one message.
    pub conversations: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FolderRole;
    use serde_json::json;

    #[test]
    fn targets_are_tagged_by_kind() {
        assert_eq!(
            serde_json::to_value(MessageTarget::message("m1")).unwrap(),
            json!({"kind": "message", "message_id": "m1"})
        );
        assert_eq!(
            serde_json::to_value(MessageTarget::conversation("t1", Some("f-in".into()))).unwrap(),
            json!({"kind": "conversation", "thread_id": "t1", "folder_id": "f-in"})
        );
        let parsed: MessageTarget =
            serde_json::from_value(json!({"kind": "conversation", "thread_id": "t1"})).unwrap();
        assert_eq!(parsed, MessageTarget::conversation("t1", None));
    }

    #[test]
    fn actions_are_tagged_by_type() {
        assert_eq!(
            serde_json::to_value(MailAction::Archive).unwrap(),
            json!({"type": "archive"})
        );
        assert_eq!(
            serde_json::to_value(MailAction::Move {
                target: MoveTarget::Role(FolderRole::Archive)
            })
            .unwrap(),
            json!({"type": "move", "target": {"role": "archive"}})
        );
        let tags: MailAction =
            serde_json::from_value(json!({"type": "tags", "add": ["work"]})).unwrap();
        assert_eq!(
            tags,
            MailAction::Tags {
                add: vec!["work".into()],
                remove: vec![]
            }
        );
        assert!(
            MailAction::Tags {
                add: vec![],
                remove: vec![]
            }
            .is_empty()
        );
        let snooze: MailAction =
            serde_json::from_value(json!({"type": "snooze", "until": "2026-10-06T08:00:00Z"}))
                .unwrap();
        assert_eq!(snooze.kind(), MailActionKind::Snooze);
    }

    #[test]
    fn reach_follows_gmail() {
        assert_eq!(MailAction::Archive.reach(), ConversationReach::Listed);
        assert_eq!(MailAction::Trash.reach(), ConversationReach::Thread);
        assert_eq!(
            MailAction::MarkRead { read: false }.reach(),
            ConversationReach::Whole
        );
        assert_eq!(
            MailAction::Flag { flagged: true }.reach(),
            ConversationReach::Newest
        );
        assert_eq!(
            MailAction::Flag { flagged: false }.reach(),
            ConversationReach::Whole
        );
        assert_eq!(
            MailAction::Junk { junk: true }.reach(),
            ConversationReach::Listed
        );
        assert_eq!(
            MailAction::DeletePermanently.reach(),
            ConversationReach::Listed
        );
    }

    #[test]
    fn kinds_policies_and_undo() {
        assert_eq!(MailAction::Trash.action_kind(), ActionKind::Trash);
        assert_eq!(
            MailAction::DeletePermanently.action_kind(),
            ActionKind::PermanentDelete
        );
        assert_eq!(
            MailAction::Junk { junk: true }.action_kind(),
            ActionKind::Organize
        );
        assert!(!MailAction::DeletePermanently.undoable());
        assert!(MailAction::Trash.undoable());
        assert!(MailAction::Archive.removes_from_list());
        assert!(!MailAction::MarkRead { read: true }.removes_from_list());
        assert_eq!(
            MailAction::MarkRead { read: false }.kind().message_id(),
            "mail-action-kind-mark-unread"
        );
        crate::labels::assert_labels(
            MailActionKind::ALL,
            MailActionKind::as_str,
            MailActionKind::message_id,
        );
        crate::labels::assert_labels(
            MailActionIssue::ALL,
            MailActionIssue::as_str,
            MailActionIssue::message_id,
        );
    }
}
