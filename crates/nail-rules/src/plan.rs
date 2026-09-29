//! Turning a rule's actions into planned effects: queue operations with
//! the operations that undo them, copies, messages to send, notification
//! choices. The engine only plans; the core executes the plan through the
//! operation queue and records it as a rule run (`nail_store`'s
//! `rule_run_actions`), so a run can be undone as one unit by queueing the
//! `undo` operations in reverse order.

use nail_api::rules::{FolderRef, RuleAction};
use nail_api::{AccountId, Flags, FolderId, FolderRole, IdentityId, MessageId};
use nail_store::{FlagChange, Operation};
use serde::{Deserialize, Serialize};

use crate::message::{Env, MessageView};

/// The state of a message while the actions of its rules are planned:
/// later actions and later rules see what earlier ones did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageState {
    /// The message.
    pub message_id: MessageId,
    /// Its account (changes with a move into another account).
    pub account_id: AccountId,
    /// Its folder.
    pub folder_id: FolderId,
    /// Its flags.
    pub flags: Flags,
    /// Its tags.
    pub tags: Vec<String>,
    /// Deleted by an earlier action; nothing more happens to it.
    pub deleted: bool,
}

impl MessageState {
    /// The state of a message before any rule ran.
    pub fn of(message: &dyn MessageView) -> Self {
        Self {
            message_id: message.id().clone(),
            account_id: message.account_id().clone(),
            folder_id: message.folder_id().clone(),
            flags: message.flags(),
            tags: message.tags(),
            deleted: false,
        }
    }

    fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
    }
}

/// Why a planned action does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// The message already is as the action wants (tagged, in that
    /// folder, …).
    Unchanged,
    /// The target folder does not exist in the message's account.
    FolderNotFound,
    /// An earlier action deleted the message.
    Deleted,
    /// No automatic reply to automatic mail, mailing lists or bounces
    /// (RFC 3834), so rules cannot start mail loops.
    AutomaticMail,
}

/// What an action does to one message.
// Plans are short-lived and small in number; boxing the operations would
// only make matching on them clumsier.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Queue `op`; queueing `undo` reverts it (`None`: it cannot be undone,
    /// e.g. a permanent delete).
    Queue {
        /// The operation.
        op: Operation,
        /// Its inverse.
        undo: Option<Operation>,
    },
    /// Copy the message into a folder (the core copies; a copy is not
    /// undone).
    Copy {
        /// The folder.
        target: FolderId,
    },
    /// Forward the message as a new message.
    Forward {
        /// Recipients.
        to: Vec<String>,
        /// Sending identity; `None` = the account's default.
        identity_id: Option<IdentityId>,
    },
    /// Redirect the message unchanged.
    Redirect {
        /// Recipients.
        to: Vec<String>,
    },
    /// Reply with a template (rendered by the composer).
    Reply {
        /// Markdown template.
        template: String,
        /// Sending identity; `None` = the one the message was addressed to.
        identity_id: Option<IdentityId>,
    },
    /// Show (`true`) or suppress (`false`) the new-mail notification.
    Notify(bool),
    /// Stop processing: no further actions or rules for this message.
    Stop,
    /// Nothing happens.
    Skipped(SkipReason),
}

/// One action of a rule, planned for one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedAction {
    /// The action as written in the rule.
    pub action: RuleAction,
    /// What it does.
    pub effect: Effect,
}

impl PlannedAction {
    /// Whether the effect changes something that cannot be undone (a sent
    /// message, a permanent delete, a copy).
    pub fn is_irreversible(&self) -> bool {
        match &self.effect {
            Effect::Queue { undo, .. } => undo.is_none(),
            Effect::Copy { .. }
            | Effect::Forward { .. }
            | Effect::Redirect { .. }
            | Effect::Reply { .. } => true,
            Effect::Notify(_) | Effect::Stop | Effect::Skipped(_) => false,
        }
    }
}

/// How planning a rule's actions ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flow {
    /// A `stop` action ran (or the message was deleted).
    pub stopped: bool,
    /// The last notify choice, if any.
    pub notify: Option<bool>,
}

/// Whether a message must not get an automatic reply: it is automatic
/// itself (`Auto-Submitted` other than `no`), bulk or list mail
/// (`Precedence`, `List-Id`), or from a bounce / no-reply address.
pub fn refuses_automatic_reply(message: &dyn MessageView) -> bool {
    let header = |name: &str| message.header_values(name);
    if header("Auto-Submitted")
        .iter()
        .any(|v| !v.trim().eq_ignore_ascii_case("no"))
    {
        return true;
    }
    if header("Precedence").iter().any(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "bulk" | "list" | "junk"
        )
    }) || !header("List-Id").is_empty()
        || !header("List-Unsubscribe").is_empty()
    {
        return true;
    }
    let from = message.addresses(nail_api::rules::AddressField::From);
    let Some(sender) = from.first() else {
        return true;
    };
    let local = sender
        .email
        .split('@')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    local.is_empty()
        || local.starts_with("mailer-daemon")
        || local.starts_with("postmaster")
        || local.starts_with("noreply")
        || local.starts_with("no-reply")
        || local.starts_with("donotreply")
        || local.starts_with("do-not-reply")
        || local.starts_with("bounce")
}

fn flag_change(
    state: &mut MessageState,
    change: FlagChange,
    undo: FlagChange,
    action: &RuleAction,
) -> PlannedAction {
    state.flags = change.apply_to(state.flags);
    for tag in &change.add_tags {
        if !state.has_tag(tag) {
            state.tags.push(tag.clone());
        }
    }
    state
        .tags
        .retain(|t| !change.remove_tags.iter().any(|r| r.eq_ignore_ascii_case(t)));
    PlannedAction {
        action: action.clone(),
        effect: Effect::Queue {
            op: Operation::UpdateFlags {
                message_id: state.message_id.clone(),
                change,
            },
            undo: Some(Operation::UpdateFlags {
                message_id: state.message_id.clone(),
                change: undo,
            }),
        },
    }
}

fn move_to(state: &mut MessageState, target: FolderId, target_account: AccountId) -> Effect {
    let op = Operation::MoveMessage {
        message_id: state.message_id.clone(),
        target_folder_id: target.clone(),
        source: None,
    };
    let undo = Operation::MoveMessage {
        message_id: state.message_id.clone(),
        target_folder_id: state.folder_id.clone(),
        source: None,
    };
    state.folder_id = target;
    state.account_id = target_account;
    Effect::Queue {
        op,
        undo: Some(undo),
    }
}

fn plan_one(
    action: &RuleAction,
    state: &mut MessageState,
    message: &dyn MessageView,
    env: &Env<'_>,
) -> PlannedAction {
    let planned = |effect| PlannedAction {
        action: action.clone(),
        effect,
    };
    let skipped = |reason| planned(Effect::Skipped(reason));
    if state.deleted && !matches!(action, RuleAction::Notify { .. } | RuleAction::Stop) {
        return skipped(SkipReason::Deleted);
    }
    let resolve =
        |folder: &FolderRef, state: &MessageState| env.resolve_folder(folder, &state.account_id);
    match action {
        RuleAction::Move { folder } => match resolve(folder, state) {
            None => skipped(SkipReason::FolderNotFound),
            Some(f) if f.id == state.folder_id => skipped(SkipReason::Unchanged),
            Some(f) => planned(move_to(state, f.id, f.account_id)),
        },
        RuleAction::Archive => {
            let archive = FolderRef::Role {
                role: FolderRole::Archive,
                account_id: None,
            };
            match resolve(&archive, state) {
                None => skipped(SkipReason::FolderNotFound),
                Some(f) if f.id == state.folder_id => skipped(SkipReason::Unchanged),
                Some(f) => planned(move_to(state, f.id, f.account_id)),
            }
        }
        RuleAction::Copy { folder } => match resolve(folder, state) {
            None => skipped(SkipReason::FolderNotFound),
            Some(f) if f.id == state.folder_id => skipped(SkipReason::Unchanged),
            Some(f) => planned(Effect::Copy { target: f.id }),
        },
        RuleAction::Delete { permanent } => {
            let in_trash = env
                .mailbox
                .folder(&state.folder_id)
                .is_some_and(|f| f.role == FolderRole::Trash);
            let has_trash = env
                .mailbox
                .folders(&state.account_id)
                .iter()
                .any(|f| f.role == FolderRole::Trash);
            let expunge = *permanent || in_trash || !has_trash;
            let op = Operation::DeleteMessage {
                message_id: state.message_id.clone(),
                permanent: *permanent,
                source: None,
                trash_folder_id: None,
            };
            let undo = (!expunge).then(|| Operation::MoveMessage {
                message_id: state.message_id.clone(),
                target_folder_id: state.folder_id.clone(),
                source: None,
            });
            state.deleted = true;
            planned(Effect::Queue { op, undo })
        }
        RuleAction::Tag { tag } => {
            if state.has_tag(tag) {
                return skipped(SkipReason::Unchanged);
            }
            flag_change(
                state,
                FlagChange {
                    add_tags: vec![tag.clone()],
                    ..FlagChange::default()
                },
                FlagChange {
                    remove_tags: vec![tag.clone()],
                    ..FlagChange::default()
                },
                action,
            )
        }
        RuleAction::Untag { tag } => {
            let Some(existing) = state
                .tags
                .iter()
                .find(|t| t.eq_ignore_ascii_case(tag))
                .cloned()
            else {
                return skipped(SkipReason::Unchanged);
            };
            flag_change(
                state,
                FlagChange {
                    remove_tags: vec![existing.clone()],
                    ..FlagChange::default()
                },
                FlagChange {
                    add_tags: vec![existing],
                    ..FlagChange::default()
                },
                action,
            )
        }
        RuleAction::Flag { flagged } => {
            if state.flags.flagged == *flagged {
                return skipped(SkipReason::Unchanged);
            }
            let before = state.flags.flagged;
            flag_change(
                state,
                FlagChange {
                    flagged: Some(*flagged),
                    ..FlagChange::default()
                },
                FlagChange {
                    flagged: Some(before),
                    ..FlagChange::default()
                },
                action,
            )
        }
        RuleAction::MarkRead { read } => {
            if state.flags.seen == *read {
                return skipped(SkipReason::Unchanged);
            }
            let before = state.flags.seen;
            flag_change(
                state,
                FlagChange {
                    seen: Some(*read),
                    ..FlagChange::default()
                },
                FlagChange {
                    seen: Some(before),
                    ..FlagChange::default()
                },
                action,
            )
        }
        RuleAction::Forward { to, identity_id } => planned(Effect::Forward {
            to: to.clone(),
            identity_id: identity_id.clone(),
        }),
        RuleAction::Redirect { to } => planned(Effect::Redirect { to: to.clone() }),
        RuleAction::Reply {
            template,
            identity_id,
        } => {
            if refuses_automatic_reply(message) {
                skipped(SkipReason::AutomaticMail)
            } else {
                planned(Effect::Reply {
                    template: template.clone(),
                    identity_id: identity_id.clone(),
                })
            }
        }
        RuleAction::Notify { notify } => planned(Effect::Notify(*notify)),
        RuleAction::Stop => planned(Effect::Stop),
    }
}

/// Plans `actions` in order for one message, updating `state`. Planning
/// ends after a `stop` (which is included) and after a delete (the actions
/// after it are skipped, `notify` and `stop` still apply).
pub fn plan_actions(
    actions: &[RuleAction],
    state: &mut MessageState,
    message: &dyn MessageView,
    env: &Env<'_>,
) -> (Vec<PlannedAction>, Flow) {
    let mut flow = Flow::default();
    let mut planned = Vec::with_capacity(actions.len());
    for action in actions {
        let p = plan_one(action, state, message, env);
        match p.effect {
            Effect::Notify(n) => flow.notify = Some(n),
            Effect::Stop => flow.stopped = true,
            _ => {}
        }
        planned.push(p);
        if flow.stopped {
            break;
        }
    }
    if state.deleted {
        flow.stopped = true;
    }
    (planned, flow)
}
