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
//! * R5 (ADR 0235): a [`MessageTarget::List`] names a whole message list
//!   (a folder's conversations or messages, or a search with the filter
//!   chips) except some rows, resolved in the core, so "select all" never
//!   sends the ids of thousands of rows. There is no limit on the number of
//!   conversations or messages an action touches: the core applies a large
//!   action in parts of at most [`MAIL_ACTION_PART_MESSAGES`] messages as
//!   one batch with one undo.
//!
//! The older per-message requests (`UpdateFlags`, `MoveMessages`,
//! `DeleteMessages`, `MarkJunk`, `SnoozeMessages`) stay as they are, without
//! undo.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::agents::ActionKind;
use crate::ids::{AccountId, FolderId, MailBatchId, MessageId, OpId, ThreadId};
use crate::ops::MoveTarget;
use crate::search::SearchQuery;

/// Most rows one [`crate::Request::ApplyMailAction`] /
/// [`crate::Request::ResolveMailTargets`] may name one by one
/// ([`explicit_target_count`]: message and conversation targets, the
/// exceptions of list targets included). Not a limit on what an action
/// touches — a whole list is one [`MessageTarget::List`] — but a sanity cap
/// on the request's size: [`crate::MAX_REQUEST_BYTES`] (the largest request
/// the web socket accepts) shared out at [`MAIL_TARGET_BYTES`] per target
/// (ADR 0235; 4 096 rows). More fail with `invalid_request`
/// ([`MailActionIssue::TooManyTargets`]).
pub const MAX_MAIL_ACTION_TARGETS: usize = crate::MAX_REQUEST_BYTES / MAIL_TARGET_BYTES;
/// The request bytes [`MAX_MAIL_ACTION_TARGETS`] grants one target: a
/// conversation target with a thread and a folder id of 64 bytes each and
/// its JSON keys fits with room to spare (a test pins it).
pub const MAIL_TARGET_BYTES: usize = 256;
/// Most messages the core plans, applies locally and queues in one store
/// transaction; a larger action runs in parts of this size, recorded as one
/// batch with one undo (ADR 0235). Not a limit on the action.
pub const MAIL_ACTION_PART_MESSAGES: usize = 10_000;
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
/// JSON: `{"kind": "message", "message_id": "…"}`,
/// `{"kind": "conversation", "thread_id": "…", "folder_id": "…" | null}` or
/// `{"kind": "list", "list": {…}, "except": […], "as_of": "…" | null}`.
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
    /// Every row of a message list except some (R5, ADR 0235: Ctrl+A in
    /// the message list, rows not loaded yet included).
    List(ListTarget),
}

/// A whole message list except some rows ([`MessageTarget::List`]).
///
/// The core lists the rows exactly as the message list pages them and
/// treats each row as the target it stands for in the list: a conversation
/// row as [`MessageTarget::Conversation`] (with the listed folder; without
/// a folder in a search), a message row as [`MessageTarget::Message`]. The
/// action reaches the rows' messages minus the messages the `except`
/// targets resolve to; a conversation counts when one of its messages is
/// left.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct ListTarget {
    /// Which list.
    pub list: MailList,
    /// Rows deselected after "select all" (Ctrl / Shift + click), as the
    /// targets those rows stand for. Never a list target
    /// ([`MailActionIssue::NestedList`]); they count toward
    /// [`MAX_MAIL_ACTION_TARGETS`].
    #[serde(default)]
    pub except: Vec<MessageTarget>,
    /// [`MailTargetResolution::as_of`] of the confirmation that counted
    /// this list, passed back unchanged: only messages the core held then
    /// are acted on, so nothing that arrived meanwhile is touched unasked.
    /// `null` = the list as it is when the request runs.
    #[serde(default)]
    pub as_of: Option<String>,
}

/// A message list as the message list widget pages it: the same query, so
/// "select all" means exactly the rows the user would see by scrolling.
///
/// JSON: `{"source": "conversations", "folder_id": "…"}`,
/// `{"source": "messages", "folder_id": "…"}` or
/// `{"source": "search", "query": {…}, "conversations": true}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum MailList {
    /// A folder's conversation rows ([`crate::Request::ListThreads`]; real
    /// or virtual, e.g. the unified inbox).
    Conversations {
        /// The folder.
        folder_id: FolderId,
    },
    /// A folder's message rows ([`crate::Request::ListMessages`]: a list
    /// without conversations).
    Messages {
        /// The folder.
        folder_id: FolderId,
    },
    /// The hits of a search ([`crate::Request::Search`]): the typed search
    /// and the filter chips' terms, as the list runs it. Every local hit:
    /// `offset`, `limit` and `sort` are ignored, and hits found only by the
    /// server search (`from_server`) are not part of the list.
    Search {
        /// The query the list runs.
        query: SearchQuery,
        /// The rows stand for their conversations without a folder (a list
        /// with conversations) instead of the hit messages.
        #[serde(default)]
        conversations: bool,
    },
}

impl MailList {
    /// The folder the list is in (a search's folder scope; `None` for a
    /// search without one).
    pub fn folder_id(&self) -> Option<&FolderId> {
        match self {
            Self::Conversations { folder_id } | Self::Messages { folder_id } => Some(folder_id),
            Self::Search { query, .. } => query.folder_id.as_ref(),
        }
    }
}

/// How many rows `targets` name one by one (checked against
/// [`MAX_MAIL_ACTION_TARGETS`]): every message and conversation target,
/// the exceptions of list targets included; a list target itself counts
/// nothing.
pub fn explicit_target_count(targets: &[MessageTarget]) -> usize {
    targets
        .iter()
        .map(|target| match target {
            MessageTarget::List(list) => explicit_target_count(&list.except),
            MessageTarget::Message { .. } | MessageTarget::Conversation(_) => 1,
        })
        .sum()
}

/// Whether a list target names another list among its exceptions
/// ([`MailActionIssue::NestedList`]).
pub fn has_nested_list(targets: &[MessageTarget]) -> bool {
    targets.iter().any(|target| match target {
        MessageTarget::List(list) => list
            .except
            .iter()
            .any(|e| matches!(e, MessageTarget::List(_))),
        MessageTarget::Message { .. } | MessageTarget::Conversation(_) => false,
    })
}

/// Whether one of `targets` is a whole list.
pub fn has_list(targets: &[MessageTarget]) -> bool {
    targets.iter().any(|t| matches!(t, MessageTarget::List(_)))
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

    /// A whole list except some rows, without an `as_of` mark.
    pub fn list(list: MailList, except: Vec<MessageTarget>) -> Self {
        Self::List(ListTarget {
            list,
            except,
            as_of: None,
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
        /// More than [`MAX_MAIL_ACTION_TARGETS`] rows named one by one
        /// ([`explicit_target_count`]).
        TooManyTargets = "too-many-targets",
        /// A list target among a list target's exceptions.
        NestedList = "nested-list",
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
    /// Every message the action would touch; empty when a target is a
    /// [`MessageTarget::List`] (the count is in `messages`; the UI then
    /// applies the same list targets with `as_of`).
    pub message_ids: Vec<MessageId>,
    /// How many messages the action would touch (with or without
    /// `message_ids`): the confirmation's true count.
    pub messages: u32,
    /// Their accounts.
    pub account_ids: Vec<AccountId>,
    /// How many conversation targets (a list's conversation rows included)
    /// resolve to at least one message.
    pub conversations: u32,
    /// An opaque mark of the core's mail at the time of this answer, for
    /// [`ListTarget::as_of`]; `null` when no target is a list (or from a
    /// core without the mark: the list then means the list at apply time).
    #[serde(default)]
    pub as_of: Option<String>,
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
    fn list_targets_name_the_list_and_its_exceptions() {
        let list = MessageTarget::list(
            MailList::Conversations {
                folder_id: "f-in".into(),
            },
            vec![MessageTarget::conversation("t1", Some("f-in".into()))],
        );
        assert_eq!(
            serde_json::to_value(&list).unwrap(),
            json!({
                "kind": "list",
                "list": {"source": "conversations", "folder_id": "f-in"},
                "except": [{"kind": "conversation", "thread_id": "t1", "folder_id": "f-in"}],
                "as_of": null
            })
        );
        // `except`, `as_of` and `conversations` may be left out.
        let parsed: MessageTarget = serde_json::from_value(json!({
            "kind": "list",
            "list": {"source": "search", "query": {"text": "is:unread", "limit": 50}}
        }))
        .unwrap();
        let MessageTarget::List(parsed) = parsed else {
            panic!("a list target");
        };
        assert_eq!(parsed.except, vec![]);
        assert_eq!(parsed.as_of, None);
        assert!(matches!(
            parsed.list,
            MailList::Search { ref query, conversations: false } if query.text == "is:unread"
        ));
        assert_eq!(
            serde_json::to_value(MailList::Messages {
                folder_id: "f".into()
            })
            .unwrap(),
            json!({"source": "messages", "folder_id": "f"})
        );
    }

    #[test]
    fn rows_named_one_by_one_are_counted_and_lists_never_nest() {
        let row = MessageTarget::message("m1");
        let inbox = MailList::Messages {
            folder_id: "f-in".into(),
        };
        let list = MessageTarget::list(inbox.clone(), vec![row.clone(), row.clone()]);
        assert_eq!(explicit_target_count(&[]), 0);
        assert_eq!(explicit_target_count(&[row.clone(), list.clone()]), 3);
        assert_eq!(
            explicit_target_count(&[MessageTarget::list(inbox.clone(), vec![])]),
            0
        );
        assert!(has_list(&[row.clone(), list.clone()]));
        assert!(!has_list(std::slice::from_ref(&row)));
        assert!(!has_nested_list(&[row.clone(), list.clone()]));
        assert_eq!(inbox.folder_id().map(FolderId::as_str), Some("f-in"));
        assert!(has_nested_list(&[MessageTarget::list(inbox, vec![list])]));
    }

    #[test]
    fn the_largest_request_fits_the_transport() {
        // The longest ids the request may reasonably carry.
        let id = "x".repeat(64);
        let request = crate::Request::ApplyMailAction {
            targets: (0..MAX_MAIL_ACTION_TARGETS)
                .map(|_| MessageTarget::conversation(id.as_str(), Some(id.as_str().into())))
                .collect(),
            action: MailAction::Archive,
        };
        let bytes = serde_json::to_vec(&crate::RpcRequest { id: 1, request }).unwrap();
        assert!(
            bytes.len() < crate::MAX_REQUEST_BYTES,
            "{} bytes for {MAX_MAIL_ACTION_TARGETS} targets",
            bytes.len()
        );
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
        assert_wire_labels!(MailActionKind, MailActionIssue);
    }
}
