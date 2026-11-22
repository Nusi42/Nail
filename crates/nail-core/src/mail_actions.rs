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
//!   transaction (`Store::apply_mail_batch_part`); the queue groups them
//!   again for the engines (one `UID MOVE` per folder, a Gmail
//!   `batchModify`, an EWS multi-item request). An action on more than
//!   `MAIL_ACTION_PART_MESSAGES` messages runs in parts of that size, one
//!   transaction and one `MailChanged` / `OpsChanged` each, all in the
//!   same batch (R5, ADR 0235).
//! * **Lists** (R5): a list target stands for the rows of its list
//!   (`mail_lists.rs`), resolved [`ROWS_PER_STORE_CALL`] rows per store
//!   call. `ResolveMailTargets` answers a list with the count and an
//!   `as_of` mark (the store's message mark); an apply with the mark
//!   leaves out messages stored after it.
//! * **Undo** queues the inverses of the actor's own batch (newest first)
//!   and marks it undone; a snooze also cancels its wake timer, a junk
//!   verdict its training.
//! * **Actors**: `user` (the UI), `remote:<session>`, `mcp:<client>`,
//!   `plugin:<id>` — only the actor of a batch can undo it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use nail_api::agents::ActionActor;
use nail_api::mail_actions::{
    MAIL_ACTION_PART_MESSAGES, MAIL_UNDO_SECS, MAX_MAIL_ACTION_TARGETS, MailActionIssue,
    MailActionKind, MailActionResult, MailTargetResolution, explicit_target_count, has_list,
    has_nested_list,
};
use nail_api::{
    AccountId, ApiError, ErrorCode, Event, FolderId, FolderRole, LocalizedMessage, MailAction,
    MailBatchId, MessageId, MessageTarget, MoveTarget, OpId,
};
use nail_store::{
    ConversationFolders, MailBatchApplied, MailBatchPlan, MailBatchUndo, Operation, OwnAddresses,
    TargetMessage, mail_batch_id, parse_mail_batch_id,
};

use crate::conversations::{FolderFilter, as_keep};
use crate::error::Result;
use crate::mail_lists::within;
use crate::service::Service;

/// How many of a Sent folder's newest messages tell which addresses the
/// user writes from (restoring from Trash).
const SENT_SAMPLE: u32 = 200;

/// How many rows one store call resolves (a list of tens of thousands of
/// rows takes a few dozen calls, and other requests get the store between
/// them).
const ROWS_PER_STORE_CALL: usize = 500;

/// The prefix of an `as_of` mark (ADR 0235; opaque to clients): the
/// store's message mark ([`nail_store::Store::message_mark`]) as text.
const MARK_PREFIX: &str = "mark-";

/// An `as_of` mark as the API carries it.
fn mark_text(mark: i64) -> String {
    format!("{MARK_PREFIX}{mark}")
}

/// The store's mark in an `as_of` mark.
fn parse_mark(text: &str) -> Result<i64> {
    text.strip_prefix(MARK_PREFIX)
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| ApiError::invalid_request(format!("invalid as_of mark {text:?}")).into())
}

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
    /// Resolves `targets` for `action` within `filter` (a remote session's
    /// grant; `None` = everything). A list target (R5, ADR 0235) stands for
    /// its rows, each resolved like the target it stands for, minus the
    /// messages its exceptions resolve to, and — with an `as_of` mark, or
    /// `fresh` for a list without one — minus the messages stored after the
    /// mark. No limit on the messages: large actions run in parts
    /// ([`MAIL_ACTION_PART_MESSAGES`]).
    async fn resolve_targets(
        &self,
        targets: &[MessageTarget],
        action: &MailAction,
        filter: Option<&FolderFilter>,
        fresh: Option<i64>,
    ) -> Result<Resolved> {
        if targets.is_empty() {
            return Err(issue(MailActionIssue::NoTargets, "no targets").into());
        }
        if explicit_target_count(targets) > MAX_MAIL_ACTION_TARGETS {
            return Err(issue(
                MailActionIssue::TooManyTargets,
                &format!("at most {MAX_MAIL_ACTION_TARGETS} rows named one by one"),
            )
            .into());
        }
        if has_nested_list(targets) {
            return Err(issue(
                MailActionIssue::NestedList,
                "a list among a list's exceptions",
            )
            .into());
        }
        if action.is_empty() {
            return Err(issue(MailActionIssue::NothingToChange, "nothing to change").into());
        }
        let own = self.own_addresses_by_account().await?;
        let mut out = Resolved {
            messages: Vec::new(),
            conversations: 0,
        };
        let mut seen = HashSet::new();
        let mut push = |out: &mut Resolved, picked: Vec<TargetMessage>| {
            for message in picked {
                if seen.insert(message.id.clone()) {
                    out.messages.push(message);
                }
            }
        };
        let rows: Vec<MessageTarget> = targets
            .iter()
            .filter(|t| !matches!(t, MessageTarget::List(_)))
            .cloned()
            .collect();
        for (picked, reached) in self.resolve_rows(&rows, action, filter, &own).await? {
            if reached {
                out.conversations += 1;
            }
            push(&mut out, picked);
        }
        for target in targets {
            let MessageTarget::List(list) = target else {
                continue;
            };
            let mark = match &list.as_of {
                Some(text) => Some(parse_mark(text)?),
                None => fresh,
            };
            let except: HashSet<MessageId> = self
                .resolve_rows(&list.except, action, filter, &own)
                .await?
                .into_iter()
                .flat_map(|(picked, _)| picked.into_iter().map(|m| m.id))
                .collect();
            let rows = self.mail_list_rows(&list.list, filter).await?;
            let resolved = self.resolve_rows(&rows, action, filter, &own).await?;
            for (row, (mut picked, _)) in rows.iter().zip(resolved) {
                picked.retain(|m| {
                    !except.contains(&m.id)
                        && mark.is_none_or(|mark| m.row <= mark)
                        && within(filter, &m.folder_id, &m.account_id)
                });
                if matches!(row, MessageTarget::Conversation(_)) && !picked.is_empty() {
                    out.conversations += 1;
                }
                push(&mut out, picked);
            }
        }
        // Grouped by account and folder (stable), so the queue hands each
        // folder's operations to the engine together (one `UID MOVE` per
        // folder, not one per alternating Inbox / Sent message), and each
        // part of a large action holds whole folders where it can.
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

    /// The messages each row target (a message or a conversation) reaches,
    /// in order, and whether it is a conversation that reached one. Trash
    /// never deletes for good: for Trash, messages already in Trash are
    /// left out (Delete permanently has its own scope and question). The
    /// rows are read [`ROWS_PER_STORE_CALL`] at a time, so a list of tens
    /// of thousands of rows costs a few dozen store calls, and other
    /// requests get the store in between.
    async fn resolve_rows(
        &self,
        rows: &[MessageTarget],
        action: &MailAction,
        filter: Option<&FolderFilter>,
        own: &OwnAddresses,
    ) -> Result<Vec<(Vec<TargetMessage>, bool)>> {
        let mut scopes: HashMap<FolderId, ConversationFolders> = HashMap::new();
        for row in rows {
            match row {
                MessageTarget::Conversation(c) => {
                    if let Some(folder) = &c.folder_id
                        && !scopes.contains_key(folder)
                    {
                        let scope = self.conversation_scope(folder, filter).await?;
                        scopes.insert(folder.clone(), scope);
                    }
                }
                MessageTarget::Message { .. } => {}
                MessageTarget::List(_) => {
                    return Err(issue(MailActionIssue::NestedList, "a list is not a row").into());
                }
            }
        }
        let scopes = Arc::new(scopes);
        let reach = action.reach();
        let trash = matches!(action, MailAction::Trash);
        let mut out = Vec::with_capacity(rows.len());
        for chunk in rows.chunks(ROWS_PER_STORE_CALL) {
            let (chunk, scopes, own, keep) =
                (chunk.to_vec(), scopes.clone(), own.clone(), filter.cloned());
            let part = self
                .inner
                .store
                .call(move |s| {
                    let mut out = Vec::with_capacity(chunk.len());
                    for row in &chunk {
                        let (mut picked, conversation) = match row {
                            MessageTarget::Conversation(c) => {
                                let scope = c.folder_id.as_ref().and_then(|f| scopes.get(f));
                                let picked = s.conversation_targets(
                                    &c.thread_id,
                                    scope,
                                    &own,
                                    as_keep(keep.as_ref()),
                                    reach,
                                )?;
                                (picked, true)
                            }
                            MessageTarget::Message { message_id } => {
                                (s.target_messages(std::slice::from_ref(message_id))?, false)
                            }
                            MessageTarget::List(_) => (Vec::new(), false),
                        };
                        if trash {
                            picked.retain(|m| m.role != "trash");
                        }
                        let reached = conversation && !picked.is_empty();
                        out.push((picked, reached));
                    }
                    Ok(out)
                })
                .await?;
            out.extend(part);
        }
        Ok(out)
    }

    /// `ResolveMailTargets`. With a list among the targets the answer
    /// carries the count only and an `as_of` mark: the list's own mark, or
    /// the store's mark now (the count leaves out what came later, so the
    /// apply with that mark acts on exactly what was counted).
    pub(crate) async fn resolve_mail_targets(
        &self,
        targets: &[MessageTarget],
        action: &MailAction,
        filter: Option<&FolderFilter>,
    ) -> Result<MailTargetResolution> {
        let list = has_list(targets);
        let fresh = if list {
            Some(
                self.inner
                    .store
                    .call(nail_store::Store::message_mark)
                    .await?,
            )
        } else {
            None
        };
        let resolved = self.resolve_targets(targets, action, filter, fresh).await?;
        let accounts: BTreeSet<AccountId> = resolved
            .messages
            .iter()
            .map(|m| m.account_id.clone())
            .collect();
        let given = targets.iter().find_map(|t| match t {
            MessageTarget::List(l) => l.as_of.clone(),
            _ => None,
        });
        Ok(MailTargetResolution {
            // A list's ids can be many: the count only (ADR 0235).
            message_ids: if list { Vec::new() } else { resolved.ids() },
            messages: u32::try_from(resolved.messages.len()).unwrap_or(u32::MAX),
            account_ids: accounts.into_iter().collect(),
            conversations: resolved.conversations,
            as_of: given.or_else(|| fresh.map(mark_text)),
        })
    }
}

impl Service {
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

    /// `ApplyMailAction` as `actor` within `filter`. An action on more than
    /// [`MAIL_ACTION_PART_MESSAGES`] messages runs in parts (R5, ADR 0235):
    /// each part is planned, applied locally, queued and recorded in its own
    /// store transaction (so the store stays free for the UI in between)
    /// and joins the first part's batch, so one undo reverses the whole
    /// action. Each part sends one `MailChanged` and one `OpsChanged`,
    /// never one event per message. When a later part fails, the parts
    /// done stay applied and undoable: the answer names what was done.
    pub(crate) async fn apply_mail_action(
        &self,
        targets: &[MessageTarget],
        action: MailAction,
        filter: Option<&FolderFilter>,
        actor: &str,
    ) -> Result<MailActionResult> {
        let resolved = self.resolve_targets(targets, &action, filter, None).await?;
        if resolved.messages.is_empty() {
            return Err(issue(MailActionIssue::NoMessages, "no messages to act on").into());
        }
        let kind = action.kind();
        let now = self.triage_now();
        let snooze = matches!(action, MailAction::Snooze { .. });
        let mut batch: Option<i64> = None;
        let mut undoable = action.undoable();
        let mut op_ids = Vec::new();
        let mut message_ids: Vec<MessageId> = Vec::new();
        let mut named: HashSet<MessageId> = HashSet::new();
        let mut done: Vec<&TargetMessage> = Vec::new();
        for (n, part) in resolved
            .messages
            .chunks(MAIL_ACTION_PART_MESSAGES)
            .enumerate()
        {
            let part_ids = || part.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
            let applied = match self
                .apply_part(
                    part,
                    &action,
                    now,
                    (actor, resolved.conversations, undoable),
                    batch,
                )
                .await
            {
                Ok(applied) => applied,
                Err(err) => {
                    if snooze {
                        drop(self.forget_snoozes(part_ids()).await);
                    }
                    if n == 0 {
                        return Err(err);
                    }
                    tracing::warn!(%err, part = n, "a later part of a mail action failed; the parts done stay");
                    break;
                }
            };
            let Some((operations, applied)) = applied else {
                if snooze {
                    drop(self.forget_snoozes(part_ids()).await);
                }
                continue;
            };
            if !applied.undoable || !undoable {
                undoable = false;
            }
            batch = applied.batch;
            for op in &operations {
                if named.insert(op.message_id().clone()) {
                    message_ids.push(op.message_id().clone());
                }
            }
            done.extend(part);
            op_ids.extend(self.announce_enqueued(&applied.queued));
        }
        if message_ids.is_empty() {
            return Err(issue(MailActionIssue::NoMessages, "no messages to act on").into());
        }
        match &action {
            MailAction::Snooze { .. } => {
                self.inner.snooze.wake.notify_one();
                self.broadcast(Event::SnoozeChanged);
            }
            MailAction::Junk { .. } => {
                let accounts: BTreeSet<AccountId> =
                    done.iter().map(|m| m.account_id.clone()).collect();
                for account_id in accounts {
                    self.broadcast(Event::JunkStatusChanged { account_id });
                }
            }
            _ => {}
        }
        let undo_secs = i64::try_from(MAIL_UNDO_SECS).unwrap_or(i64::MAX);
        Ok(MailActionResult {
            batch_id: batch.map(|n| MailBatchId(mail_batch_id(n))),
            op_ids,
            kind,
            message_ids,
            conversations: resolved.conversations,
            undo_until: batch.map(|_| nail_util::time::rfc3339_utc(now.saturating_add(undo_secs))),
        })
    }

    /// Plans and applies one part of a mail action in one store
    /// transaction, joining `into` (the batch of the parts before).
    /// `None` when the part plans no operation. `who` = the actor, the
    /// action's conversations and whether it is still undoable.
    async fn apply_part(
        &self,
        part: &[TargetMessage],
        action: &MailAction,
        now: i64,
        who: (&str, u32, bool),
        into: Option<i64>,
    ) -> Result<Option<(Vec<Operation>, MailBatchApplied)>> {
        let operations = self.plan_mail_action(part, action).await?;
        if operations.is_empty() {
            return Ok(None);
        }
        let (actor, conversations, undoable) = (who.0.to_owned(), who.1, who.2);
        let kind = action.kind();
        let planned = operations.clone();
        let applied = self
            .inner
            .store
            .call(move |s| {
                s.apply_mail_batch_part(
                    &MailBatchPlan {
                        actor: &actor,
                        kind: kind.as_str(),
                        conversations,
                        undoable,
                        now,
                    },
                    into,
                    &planned,
                )
            })
            .await?;
        Ok(Some((operations, applied)))
    }

    /// `UndoMailAction` as `actor`: the actor's own batch only. A large
    /// batch is undone in parts of [`MAIL_ACTION_PART_MESSAGES`] inverses,
    /// each in its own store transaction with one `MailChanged` and one
    /// `OpsChanged`.
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
        let (who, now) = (actor.to_owned(), self.triage_now());
        let undone = self
            .inner
            .store
            .call(move |s| s.undo_mail_batch_part(n, &who, now, MAIL_ACTION_PART_MESSAGES))
            .await?;
        let (record, queued, mut skipped, mut rest) = match undone {
            MailBatchUndo::Undone {
                record,
                queued,
                skipped,
                rest,
            } => (record, queued, skipped, rest),
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
        let mut op_ids = self.announce_enqueued(&queued);
        while let Some(below) = rest {
            let who = actor.to_owned();
            match self
                .inner
                .store
                .call(move |s| s.undo_mail_batch_rest(n, &who, below, MAIL_ACTION_PART_MESSAGES))
                .await
            {
                Ok(Some(part)) => {
                    skipped += part.skipped;
                    rest = part.rest;
                    op_ids.extend(self.announce_enqueued(&part.queued));
                }
                Ok(None) => break,
                Err(err) => {
                    tracing::warn!(%batch, %err, "a later part of an undo failed; the parts done stay");
                    break;
                }
            }
        }
        if skipped > 0 {
            tracing::info!(%batch, skipped, "undo skipped messages that moved on since");
        }
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
