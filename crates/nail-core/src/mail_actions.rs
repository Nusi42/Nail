//! Mail actions on conversations (P3, ADR 0164).
//!
//! `ApplyMailAction` / `ResolveMailTargets` / `UndoMailAction`. The contract
//! stage ships a working **stub**: targets resolve per
//! [`MailAction::reach`] from the conversation request's messages (one copy
//! per Message-ID; duplicates are WP-actions-core's), and the action is
//! forwarded to the per-message writes (`update_flags`, `move_messages`,
//! `delete_messages`, `snooze_messages`, `mark_junk`) — several queue
//! operations, **no batch and no undo** (`batch_id` is `null`,
//! `UndoMailAction` answers `unsupported`). WP-actions-core replaces it:
//! every stored copy, one batch in the store (`mail_batches`, schema v17)
//! with the inverse operations, undo, engine-side batching.

use std::collections::{BTreeSet, HashMap};

use nail_api::mail_actions::{
    ConversationReach, MAX_MAIL_ACTION_MESSAGES, MAX_MAIL_ACTION_TARGETS, MailActionIssue,
    MailActionResult, MailTargetResolution,
};
use nail_api::{
    AccountId, ApiError, FlagChange, FolderId, FolderRole, LocalizedMessage, MailAction,
    MailBatchId, MessageId, MessageTarget, MoveTarget, OpId,
};

use crate::conversations::FolderFilter;
use crate::error::Result;
use crate::service::Service;

/// The messages a set of targets resolves to.
struct Resolved {
    ids: Vec<MessageId>,
    accounts: BTreeSet<AccountId>,
    /// Folder of each message.
    folders: HashMap<MessageId, FolderId>,
    conversations: u32,
}

fn issue(issue: MailActionIssue, detail: &str) -> ApiError {
    ApiError::invalid_request(detail.to_owned())
        .with_localized(LocalizedMessage::new(issue.message_id()))
}

impl Service {
    async fn folder_role(&self, id: &FolderId) -> Result<FolderRole> {
        let id = id.clone();
        Ok(self
            .inner
            .store
            .call(move |s| s.get_folder(&id))
            .await?
            .role)
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
        let mut out = Resolved {
            ids: Vec::new(),
            accounts: BTreeSet::new(),
            folders: HashMap::new(),
            conversations: 0,
        };
        let mut seen = BTreeSet::new();
        for target in targets {
            let picked: Vec<(MessageId, AccountId, FolderId)> = match target {
                MessageTarget::Message { message_id } => {
                    let id = message_id.clone();
                    let stored = self.inner.store.call(move |s| s.get_message(&id)).await?;
                    let summary = stored.summary;
                    vec![(summary.id, summary.account_id, summary.folder_id)]
                }
                MessageTarget::Conversation(conversation) => {
                    let (thread, messages) = self
                        .get_conversation(
                            &conversation.thread_id,
                            conversation.folder_id.as_ref(),
                            filter,
                        )
                        .await?;
                    let listed: Option<Vec<FolderId>> = match &conversation.folder_id {
                        Some(folder) => Some(self.resolve_folder(folder).await?.sources),
                        None => None,
                    };
                    let mut picked = Vec::new();
                    for m in messages {
                        let m = m.message;
                        let keep = match action.reach() {
                            ConversationReach::Whole => true,
                            ConversationReach::Newest => m.id == thread.latest_message_id,
                            ConversationReach::Listed => match &listed {
                                Some(sources) => sources.contains(&m.folder_id),
                                None => self.folder_role(&m.folder_id).await? != FolderRole::Sent,
                            },
                        };
                        if keep {
                            picked.push((m.id, m.account_id, m.folder_id));
                        }
                    }
                    if !picked.is_empty() {
                        out.conversations += 1;
                    }
                    picked
                }
            };
            for (id, account, folder) in picked {
                if seen.insert(id.clone()) {
                    out.accounts.insert(account);
                    out.folders.insert(id.clone(), folder);
                    out.ids.push(id);
                }
            }
        }
        if out.ids.len() > MAX_MAIL_ACTION_MESSAGES {
            return Err(issue(
                MailActionIssue::TooManyMessages,
                &format!("at most {MAX_MAIL_ACTION_MESSAGES} messages"),
            )
            .into());
        }
        if matches!(action, MailAction::DeletePermanently) {
            for id in &out.ids {
                let role = self.folder_role(&out.folders[id]).await?;
                if !matches!(role, FolderRole::Trash | FolderRole::Junk) {
                    return Err(issue(
                        MailActionIssue::NotInTrash,
                        &format!("message {id} is not in Trash or Junk"),
                    )
                    .into());
                }
            }
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
        Ok(MailTargetResolution {
            message_ids: resolved.ids,
            account_ids: resolved.accounts.into_iter().collect(),
            conversations: resolved.conversations,
        })
    }

    /// `ApplyMailAction` (contract stub: forwarded per message, no undo).
    pub(crate) async fn apply_mail_action(
        &self,
        targets: &[MessageTarget],
        action: MailAction,
        filter: Option<&FolderFilter>,
    ) -> Result<MailActionResult> {
        let resolved = self.resolve_targets(targets, &action, filter).await?;
        let mut ids = resolved.ids.clone();
        if matches!(action, MailAction::Archive) {
            // Messages already in their Archive stay.
            let mut kept = Vec::with_capacity(ids.len());
            for id in ids {
                if self.folder_role(&resolved.folders[&id]).await? != FolderRole::Archive {
                    kept.push(id);
                }
            }
            ids = kept;
        }
        if ids.is_empty() {
            return Err(issue(MailActionIssue::NoMessages, "no messages to act on").into());
        }
        let op_ids: Vec<OpId> = match &action {
            MailAction::Archive => {
                self.move_messages(&ids, &MoveTarget::Role(FolderRole::Archive))
                    .await?
            }
            MailAction::Trash => self.delete_messages(&ids, false).await?,
            MailAction::DeletePermanently => self.delete_messages(&ids, true).await?,
            MailAction::Move { target } => self.move_messages(&ids, target).await?,
            MailAction::MarkRead { read } => {
                self.update_flags(
                    &ids,
                    FlagChange {
                        seen: Some(*read),
                        ..FlagChange::default()
                    },
                )
                .await?
            }
            MailAction::Flag { flagged } => {
                self.update_flags(
                    &ids,
                    FlagChange {
                        flagged: Some(*flagged),
                        ..FlagChange::default()
                    },
                )
                .await?
            }
            MailAction::Tags { add, remove } => {
                self.update_flags(
                    &ids,
                    FlagChange {
                        add_tags: add.clone(),
                        remove_tags: remove.clone(),
                        ..FlagChange::default()
                    },
                )
                .await?
            }
            MailAction::Snooze {
                until,
                wake_folder_id,
            } => {
                self.snooze_messages(ids.clone(), until, wake_folder_id.clone())
                    .await?
            }
            MailAction::Junk { junk } => self.mark_junk(ids.clone(), *junk).await?,
        };
        Ok(MailActionResult {
            batch_id: None,
            op_ids,
            kind: action.kind(),
            message_ids: ids,
            conversations: resolved.conversations,
            undo_until: None,
        })
    }

    /// `UndoMailAction` (contract stub: the stub keeps no batches).
    #[expect(
        clippy::unused_async,
        reason = "WP-actions-core reads the batch from the store here"
    )]
    pub(crate) async fn undo_mail_action(&self, batch: &MailBatchId) -> Result<Vec<OpId>> {
        Err(ApiError::unsupported(format!(
            "batch {batch}: undo arrives with P3 WP-actions-core"
        ))
        .with_localized(LocalizedMessage::new(
            MailActionIssue::UndoExpired.message_id(),
        ))
        .into())
    }
}
