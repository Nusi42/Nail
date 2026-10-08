//! Mail actions on conversations (P3, ADR 0164; WP-actions-core).
//!
//! `ApplyMailAction` / `ResolveMailTargets` / `UndoMailAction`:
//!
//! * **Targets** resolve per [`MailAction::reach`] to **every stored copy**
//!   (`nail_store::thread::targets`): a conversation's messages in the
//!   listed folder (a virtual folder: its member folders; none: everywhere
//!   but Drafts, Trash, Junk), plus the user's replies from Sent for the
//!   `whole` actions, or the row's own message for Flag. A remote session's
//!   grant narrows the folders before anything else: it acts on exactly
//!   the conversation it sees and is never refused for hidden messages.
//! * **One batch**: the planned per-message operations are applied
//!   locally, queued and recorded with their inverses in one store
//!   transaction (`Store::apply_mail_batch`); the queue groups them again
//!   for the engines (one `UID MOVE` per folder, a Gmail `batchModify`, an
//!   EWS multi-item request).
//! * **Undo** queues the inverses of the actor's own batch (newest first)
//!   and marks it undone; a snooze also cancels its wake timer, a junk
//!   verdict its training.
//! * **Actors**: `user` (the UI), `remote:<session>`, `mcp:<client>`,
//!   `plugin:<id>` — only the actor of a batch can undo it.

use std::collections::BTreeSet;

use nail_api::agents::ActionActor;
use nail_api::mail_actions::{
    MAIL_UNDO_SECS, MAX_MAIL_ACTION_MESSAGES, MAX_MAIL_ACTION_TARGETS, MailActionIssue,
    MailActionKind, MailActionResult, MailTargetResolution,
};
use nail_api::{
    AccountId, ApiError, ErrorCode, Event, FolderRole, LocalizedMessage, MailAction, MailBatchId,
    MessageId, MessageTarget, MoveTarget, OpId,
};
use nail_store::{
    MailBatchPlan, MailBatchUndo, Operation, TargetMessage, mail_batch_id, parse_mail_batch_id,
};

use crate::conversations::{FolderFilter, as_keep};
use crate::error::Result;
use crate::service::Service;

/// How many of a Sent folder's newest messages tell which addresses the
/// user writes from (restoring from Trash).
const SENT_SAMPLE: u32 = 200;

/// The UI's own actor.
pub(crate) const USER_ACTOR: &str = "user";

/// The batch actor of a remote session.
pub(crate) fn remote_actor(session: &impl std::fmt::Display) -> String {
    format!("remote:{session}")
}

/// The batch actor of an MCP client or a plug-in.
pub(crate) fn agent_actor(actor: &ActionActor) -> String {
    match actor {
        ActionActor::McpClient { client_id } => format!("mcp:{client_id}"),
        ActionActor::Plugin { plugin_id } => format!("plugin:{plugin_id}"),
    }
}

/// The messages a set of targets resolves to.
struct Resolved {
    messages: Vec<TargetMessage>,
    conversations: u32,
}

impl Resolved {
    fn ids(&self) -> Vec<MessageId> {
        self.messages.iter().map(|m| m.id.clone()).collect()
    }
}

fn issue(issue: MailActionIssue, detail: &str) -> ApiError {
    ApiError::invalid_localized(detail.to_owned(), issue.message_id())
}

impl Service {
    /// One message target as a [`TargetMessage`].
    async fn message_target(&self, id: &MessageId) -> Result<TargetMessage> {
        let id = id.clone();
        Ok(self
            .inner
            .store
            .call(move |s| {
                let summary = s.get_message(&id)?.summary;
                let role = s.get_folder(&summary.folder_id)?.role;
                Ok(TargetMessage {
                    id: summary.id,
                    account_id: summary.account_id,
                    folder_id: summary.folder_id,
                    role: role.as_str().to_owned(),
                })
            })
            .await?)
    }

    /// Resolves `targets` for `action` within `filter` (a remote session's
    /// grant; `None` = everything).
    async fn resolve_targets(
        &self,
        targets: &[MessageTarget],
        action: &MailAction,
        filter: Option<&FolderFilter>,
    ) -> Result<Resolved> {
        if targets.is_empty() {
            return Err(issue(MailActionIssue::NoTargets, "no targets").into());
        }
        if targets.len() > MAX_MAIL_ACTION_TARGETS {
            return Err(issue(
                MailActionIssue::TooManyTargets,
                &format!("at most {MAX_MAIL_ACTION_TARGETS} targets"),
            )
            .into());
        }
        if action.is_empty() {
            return Err(issue(MailActionIssue::NothingToChange, "nothing to change").into());
        }
        let reach = action.reach();
        let own = self.own_addresses_by_account().await?;
        let mut out = Resolved {
            messages: Vec::new(),
            conversations: 0,
        };
        let mut seen = BTreeSet::new();
        for target in targets {
            let picked = match target {
                MessageTarget::Message { message_id } => {
                    vec![self.message_target(message_id).await?]
                }
                MessageTarget::Conversation(conversation) => {
                    let scope = match &conversation.folder_id {
                        Some(folder) => Some(self.conversation_scope(folder, filter).await?),
                        None => None,
                    };
                    let (thread, own, keep) =
                        (conversation.thread_id.clone(), own.clone(), filter.cloned());
                    let picked = self
                        .inner
                        .store
                        .call(move |s| {
                            s.conversation_targets(
                                &thread,
                                scope.as_ref(),
                                &own,
                                as_keep(keep.as_ref()),
                                reach,
                            )
                        })
                        .await?;
                    let reached = picked
                        .iter()
                        .any(|m| !(matches!(action, MailAction::Trash) && m.role == "trash"));
                    if reached {
                        out.conversations += 1;
                    }
                    picked
                }
            };
            for message in picked {
                // Trash never deletes for good: messages already in Trash
                // stay (Delete permanently has its own scope and question).
                if matches!(action, MailAction::Trash) && message.role == "trash" {
                    continue;
                }
                if seen.insert(message.id.clone()) {
                    out.messages.push(message);
                }
            }
            if out.messages.len() > MAX_MAIL_ACTION_MESSAGES {
                return Err(issue(
                    MailActionIssue::TooManyMessages,
                    &format!("at most {MAX_MAIL_ACTION_MESSAGES} messages"),
                )
                .into());
            }
        }
        // Grouped by account and folder (stable), so the queue hands each
        // folder's operations to the engine together (one `UID MOVE` per
        // folder, not one per alternating Inbox / Sent message).
        out.messages
            .sort_by(|a, b| (&a.account_id, &a.folder_id).cmp(&(&b.account_id, &b.folder_id)));
        if matches!(action, MailAction::DeletePermanently)
            && let Some(m) = out
                .messages
                .iter()
                .find(|m| !matches!(m.role.as_str(), "trash" | "junk"))
        {
            return Err(issue(
                MailActionIssue::NotInTrash,
                &format!("message {} is not in Trash or Junk", m.id),
            )
            .into());
        }
        Ok(out)
    }

    /// `ResolveMailTargets`.
    pub(crate) async fn resolve_mail_targets(
        &self,
        targets: &[MessageTarget],
        action: &MailAction,
        filter: Option<&FolderFilter>,
    ) -> Result<MailTargetResolution> {
        let resolved = self.resolve_targets(targets, action, filter).await?;
        let accounts: BTreeSet<AccountId> = resolved
            .messages
            .iter()
            .map(|m| m.account_id.clone())
            .collect();
        Ok(MailTargetResolution {
            message_ids: resolved.ids(),
            account_ids: accounts.into_iter().collect(),
            conversations: resolved.conversations,
        })
    }

    /// The per-message operations of `action` on `messages`.
    async fn plan_mail_action(
        &self,
        messages: &[TargetMessage],
        action: &MailAction,
    ) -> Result<Vec<Operation>> {
        let ids: Vec<MessageId> = messages.iter().map(|m| m.id.clone()).collect();
        let flags = |change: nail_store::FlagChange| -> Vec<Operation> {
            ids.iter()
                .map(|id| Operation::UpdateFlags {
                    message_id: id.clone(),
                    change: change.clone(),
                })
                .collect()
        };
        Ok(match action {
            MailAction::Archive => {
                // Messages already in their account's Archive stay.
                let movable: Vec<MessageId> = messages
                    .iter()
                    .filter(|m| m.role != "archive")
                    .map(|m| m.id.clone())
                    .collect();
                self.plan_moves(&movable, &MoveTarget::Role(FolderRole::Archive))
                    .await?
            }
            MailAction::Move { target } => {
                let moves = self.plan_moves(&ids, target).await?;
                self.restore_own_to_sent(messages, moves).await?
            }
            MailAction::Trash | MailAction::DeletePermanently => ids
                .iter()
                .map(|id| Operation::DeleteMessage {
                    message_id: id.clone(),
                    permanent: matches!(action, MailAction::DeletePermanently),
                    source: None,
                    trash_folder_id: None,
                })
                .collect(),
            MailAction::MarkRead { read } => flags(nail_store::FlagChange {
                seen: Some(*read),
                ..nail_store::FlagChange::default()
            }),
            MailAction::Flag { flagged } => flags(nail_store::FlagChange {
                flagged: Some(*flagged),
                ..nail_store::FlagChange::default()
            }),
            MailAction::Tags { add, remove } => {
                for tag in add.iter().chain(remove) {
                    if !crate::mail::is_valid_tag(tag) {
                        return Err(
                            ApiError::invalid_request(format!("invalid tag {tag:?}")).into()
                        );
                    }
                }
                flags(nail_store::FlagChange {
                    add_tags: add.clone(),
                    remove_tags: remove.clone(),
                    ..nail_store::FlagChange::default()
                })
            }
            MailAction::Snooze {
                until,
                wake_folder_id,
            } => {
                let ops = self
                    .plan_snooze(ids.clone(), until, wake_folder_id.clone())
                    .await?;
                match self.triage_operations(ops).await {
                    Ok(operations) => operations,
                    Err(err) => {
                        drop(self.forget_snoozes(ids).await);
                        return Err(err);
                    }
                }
            }
            MailAction::Junk { junk } => {
                let (ops, _) = self.plan_mark_junk(ids, *junk).await?;
                self.triage_operations(ops).await?
            }
        })
    }

    /// Moves from Trash to an Inbox put the user's own messages (the
    /// replies a Delete took from Sent) back into the account's Sent folder
    /// instead, like Gmail's restore. "Own" = sent from one of the account's
    /// addresses, or from an address the account's Sent folder sends from
    /// (its newest [`SENT_SAMPLE`] messages: the addresses the user really
    /// writes from, also when the account's configured address differs).
    async fn restore_own_to_sent(
        &self,
        messages: &[TargetMessage],
        moves: Vec<Operation>,
    ) -> Result<Vec<Operation>> {
        let from_trash: Vec<MessageId> = messages
            .iter()
            .filter(|m| m.role == "trash")
            .map(|m| m.id.clone())
            .collect();
        if from_trash.is_empty() {
            return Ok(moves);
        }
        let mut own = self.own_addresses_by_account().await?;
        self.inner
            .store
            .call(move |s| {
                let summaries = s.message_summaries(&from_trash)?;
                let accounts: BTreeSet<AccountId> =
                    summaries.iter().map(|m| m.account_id.clone()).collect();
                for account in accounts {
                    if let Some(sent) = s.folder_with_role(&account, FolderRole::Sent)? {
                        for m in s.list_messages(&sent, 0, SENT_SAMPLE)?.messages {
                            if let Some(from) = &m.from {
                                own.add(&account, &from.email);
                            }
                        }
                    }
                }
                let mut out = Vec::with_capacity(moves.len());
                for op in moves {
                    let Operation::MoveMessage {
                        message_id,
                        target_folder_id,
                        source,
                    } = op
                    else {
                        out.push(op);
                        continue;
                    };
                    let mine = summaries.iter().find(|m| m.id == message_id).filter(|m| {
                        m.from
                            .as_ref()
                            .is_some_and(|a| own.is_mine(&m.account_id, &a.email))
                    });
                    let sent = match mine {
                        Some(m) if s.get_folder(&target_folder_id)?.role == FolderRole::Inbox => {
                            s.folder_with_role(&m.account_id, FolderRole::Sent)?
                        }
                        _ => None,
                    };
                    out.push(Operation::MoveMessage {
                        message_id,
                        target_folder_id: sent.unwrap_or(target_folder_id),
                        source,
                    });
                }
                Ok(out)
            })
            .await
            .map_err(Into::into)
    }

    /// `ApplyMailAction` as `actor` within `filter`.
    pub(crate) async fn apply_mail_action(
        &self,
        targets: &[MessageTarget],
        action: MailAction,
        filter: Option<&FolderFilter>,
        actor: &str,
    ) -> Result<MailActionResult> {
        let resolved = self.resolve_targets(targets, &action, filter).await?;
        if resolved.messages.is_empty() {
            return Err(issue(MailActionIssue::NoMessages, "no messages to act on").into());
        }
        let operations = self.plan_mail_action(&resolved.messages, &action).await?;
        let snooze = matches!(action, MailAction::Snooze { .. });
        if operations.is_empty() {
            if snooze {
                drop(self.forget_snoozes(resolved.ids()).await);
            }
            return Err(issue(MailActionIssue::NoMessages, "no messages to act on").into());
        }
        let mut message_ids: Vec<MessageId> = Vec::new();
        for op in &operations {
            if !message_ids.contains(op.message_id()) {
                message_ids.push(op.message_id().clone());
            }
        }
        let kind = action.kind();
        let now = self.triage_now();
        let (actor_name, conversations, undoable) =
            (actor.to_owned(), resolved.conversations, action.undoable());
        let applied = self
            .inner
            .store
            .call(move |s| {
                s.apply_mail_batch(
                    &MailBatchPlan {
                        actor: &actor_name,
                        kind: kind.as_str(),
                        conversations,
                        undoable,
                        now,
                    },
                    &operations,
                )
            })
            .await;
        let applied = match applied {
            Ok(applied) => applied,
            Err(err) => {
                if snooze {
                    drop(self.forget_snoozes(resolved.ids()).await);
                }
                return Err(err.into());
            }
        };
        let op_ids = self.announce_enqueued(&applied.queued);
        match &action {
            MailAction::Snooze { .. } => {
                self.inner.snooze.wake.notify_one();
                self.broadcast(Event::SnoozeChanged);
            }
            MailAction::Junk { .. } => {
                let accounts: BTreeSet<AccountId> = resolved
                    .messages
                    .iter()
                    .map(|m| m.account_id.clone())
                    .collect();
                for account_id in accounts {
                    self.broadcast(Event::JunkStatusChanged { account_id });
                }
            }
            _ => {}
        }
        let undo_secs = i64::try_from(MAIL_UNDO_SECS).unwrap_or(i64::MAX);
        Ok(MailActionResult {
            batch_id: applied.batch.map(|n| MailBatchId(mail_batch_id(n))),
            op_ids,
            kind,
            message_ids,
            conversations: resolved.conversations,
            undo_until: applied
                .batch
                .map(|_| nail_util::time::rfc3339_utc(now.saturating_add(undo_secs))),
        })
    }

    /// `UndoMailAction` as `actor`: the actor's own batch only.
    pub(crate) async fn undo_mail_action(
        &self,
        batch: &MailBatchId,
        actor: &str,
    ) -> Result<Vec<OpId>> {
        let expired = || -> crate::error::CoreError {
            ApiError::not_found(format!("batch {batch}"))
                .with_localized(LocalizedMessage::new(
                    MailActionIssue::UndoExpired.message_id(),
                ))
                .into()
        };
        let Some(n) = parse_mail_batch_id(batch.as_str()) else {
            return Err(expired());
        };
        let (actor, now) = (actor.to_owned(), self.triage_now());
        let undone = self
            .inner
            .store
            .call(move |s| s.undo_mail_batch(n, &actor, now))
            .await?;
        let (record, queued) = match undone {
            MailBatchUndo::Undone {
                record,
                queued,
                skipped,
            } => {
                if skipped > 0 {
                    tracing::info!(%batch, skipped, "undo skipped messages that moved on since");
                }
                (record, queued)
            }
            MailBatchUndo::Expired => return Err(expired()),
            MailBatchUndo::UndoneAlready => {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    format!("batch {batch} was undone already"),
                )
                .with_localized(LocalizedMessage::new(
                    MailActionIssue::UndoneAlready.message_id(),
                ))
                .into());
            }
        };
        let op_ids = self.announce_enqueued(&queued);
        let kind = MailActionKind::ALL
            .iter()
            .copied()
            .find(|k| k.as_str() == record.kind);
        match kind {
            Some(MailActionKind::Snooze) => {
                self.forget_snoozes(record.message_ids.clone()).await?;
            }
            Some(MailActionKind::Junk | MailActionKind::NotJunk) => {
                for account_id in self.untrain_junk(record.message_ids.clone()).await? {
                    self.broadcast(Event::JunkStatusChanged { account_id });
                }
            }
            _ => {}
        }
        Ok(op_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actors_are_named_by_kind() {
        assert_eq!(USER_ACTOR, "user");
        assert_eq!(remote_actor(&"s1"), "remote:s1");
        assert_eq!(agent_actor(&ActionActor::mcp_client("c1")), "mcp:c1");
        assert_eq!(agent_actor(&ActionActor::plugin("p.x")), "plugin:p.x");
        assert_eq!(FolderRole::Trash.as_str(), "trash");
        assert_eq!(FolderRole::Archive.as_str(), "archive");
    }
}
