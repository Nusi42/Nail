//! Junk mail (ADR 0048, logic in `nail-triage`, ADR 0045): the store as
//! the classifier's [`JunkStore`], training from "Mark as junk / Not junk"
//! and from the server's Junk flags, classification of arriving mail, and
//! the [`TriageOp`]s both plan, carried out through the operation queue.
//!
//! Nothing here deletes mail: the planners only set keywords and move to
//! the Junk folder or back to the Inbox.

use std::collections::HashMap;

use nail_api::triage::{JunkSource, JunkVerdict};
use nail_api::{AccountId, ApiError, Event, FolderId, FolderRole, MessageId, OpId};
use nail_store::{BlobStore, Operation, Store};
use nail_triage::classifier::{
    self, ClassifierParams, JunkStore, MessageFacts, TokenCounts, TrainOutcome, Trained,
};
use nail_triage::{FolderTarget, TokenSource, Tokenizer, TriageOp};

use crate::error::Result;
use crate::service::Service;

/// The store as the classifier's persistence.
pub(crate) struct StoreJunk<'a>(pub &'a Store);

impl JunkStore for StoreJunk<'_> {
    type Error = nail_store::Error;

    fn token_counts(
        &self,
        account: &AccountId,
        tokens: &[String],
    ) -> nail_store::Result<TokenCounts> {
        self.0.junk_token_counts(account, tokens)
    }

    fn trained(&self, account: &AccountId) -> nail_store::Result<Trained> {
        let stats = self.0.junk_stats(account)?;
        Ok(Trained {
            junk: stats.trained_junk,
            ham: stats.trained_ham,
        })
    }

    fn train(
        &self,
        account: &AccountId,
        message_key: &str,
        junk: bool,
        tokens: &[String],
        now: i64,
    ) -> nail_store::Result<TrainOutcome> {
        Ok(
            match self.0.train_junk(account, message_key, junk, tokens, now)? {
                nail_store::TrainOutcome::Trained => TrainOutcome::Trained,
                nail_store::TrainOutcome::Retrained => TrainOutcome::Retrained,
                nail_store::TrainOutcome::Unchanged => TrainOutcome::Unchanged,
            },
        )
    }

    fn untrain(&self, account: &AccountId, message_key: &str) -> nail_store::Result<bool> {
        self.0.untrain_junk(account, message_key)
    }
}

/// A stored message as the classifier sees it.
pub(crate) struct JunkInput {
    pub account_id: AccountId,
    pub folder_role: FolderRole,
    pub keywords: Vec<String>,
    pub key: String,
    pub tokens: Vec<String>,
}

/// Loads a message's tokens and facts (blocking); `None` when it is gone.
pub(crate) fn junk_input(store: &Store, blobs: &BlobStore, id: &MessageId) -> Option<JunkInput> {
    let stored = store.get_message(id).ok()?;
    let folder = store.get_folder(&stored.summary.folder_id).ok()?;
    let raw = stored
        .blob_hash
        .as_ref()
        .and_then(|h| blobs.get(h).ok())
        .unwrap_or_default();
    let tokens = tokens_of(&raw);
    let rfc = store.message_meta(id).ok().and_then(|m| m.message_id);
    Some(JunkInput {
        account_id: stored.summary.account_id.clone(),
        folder_role: folder.role,
        keywords: stored.summary.tags.clone(),
        key: classifier::message_key(rfc.as_deref(), id, &tokens),
        tokens,
    })
}

/// The classifier's tokens of a raw message: decoded headers and text.
pub(crate) fn tokens_of(raw: &[u8]) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    let parsed = nail_mime::parse_message(raw);
    let headers = nail_mime::header_fields(raw);
    Tokenizer::default().tokens(TokenSource {
        headers: &headers,
        text: &parsed.text,
    })
}

impl Service {
    /// `MarkJunk`: trains each message's account, records the user's
    /// verdict and queues the keywords and moves.
    pub(crate) async fn mark_junk(&self, ids: Vec<MessageId>, junk: bool) -> Result<Vec<OpId>> {
        if ids.is_empty() {
            return Err(ApiError::invalid_request("no messages to mark").into());
        }
        let (ops, accounts) = self.plan_mark_junk(ids, junk).await?;
        let op_ids = self.run_triage_ops(ops).await?;
        for account_id in accounts {
            self.broadcast(Event::JunkStatusChanged { account_id });
        }
        Ok(op_ids)
    }

    /// The training, verdicts and planned writes of [`Self::mark_junk`]:
    /// trains each message's account and records the user's verdict at
    /// once; answers the keyword / move writes and the accounts trained.
    pub(crate) async fn plan_mark_junk(
        &self,
        ids: Vec<MessageId>,
        junk: bool,
    ) -> Result<(Vec<TriageOp>, Vec<AccountId>)> {
        let now = self.triage_now();
        self.blocking(move |service| {
            let store = service.store();
            let mut ops = Vec::new();
            let mut accounts: Vec<AccountId> = Vec::new();
            for id in &ids {
                let input = junk_input(store, service.blobs(), id)
                    .ok_or_else(|| ApiError::not_found(format!("message {id}")))?;
                let settings = store.junk_settings(&input.account_id)?;
                classifier::train(
                    &StoreJunk(store),
                    &input.account_id,
                    &input.key,
                    junk,
                    &input.tokens,
                    now,
                )?;
                store.set_junk_verdict(
                    id,
                    &JunkVerdict {
                        junk,
                        score: None,
                        source: JunkSource::User,
                    },
                    now,
                )?;
                ops.extend(classifier::plan_mark(
                    &settings,
                    &MessageFacts {
                        message_id: Some(id),
                        folder_role: input.folder_role,
                        keywords: &input.keywords,
                    },
                    &input.account_id,
                    junk,
                ));
                if !accounts.contains(&input.account_id) {
                    accounts.push(input.account_id);
                }
            }
            Ok((ops, accounts))
        })
        .await
    }

    /// Forgets the user's training of `ids` (an undone "Mark as junk / not
    /// junk", P3): the classifier no longer counts them. Answers the
    /// accounts whose training changed.
    pub(crate) async fn untrain_junk(&self, ids: Vec<MessageId>) -> Result<Vec<AccountId>> {
        self.blocking(move |service| {
            let store = service.store();
            let mut accounts: Vec<AccountId> = Vec::new();
            for id in &ids {
                let Some(input) = junk_input(store, service.blobs(), id) else {
                    continue;
                };
                if StoreJunk(store).untrain(&input.account_id, &input.key)?
                    && !accounts.contains(&input.account_id)
                {
                    accounts.push(input.account_id);
                }
            }
            Ok(accounts)
        })
        .await
    }

    /// Junk handling of newly arrived messages: the server's verdict
    /// (trusted when the account says so, and trained), else the
    /// classifier when it is enabled and trained enough. Returns the
    /// messages filed as junk (they do not notify).
    pub(crate) async fn classify_arrivals(&self, ids: Vec<MessageId>) -> Vec<MessageId> {
        let now = self.triage_now();
        let planned = self
            .blocking(move |service| {
                let store = service.store();
                let mut ops = Vec::new();
                let mut junk = Vec::new();
                let mut trained: Vec<AccountId> = Vec::new();
                let mut settings_of = HashMap::new();
                for id in &ids {
                    let Some(input) = junk_input(store, service.blobs(), id) else {
                        continue;
                    };
                    if !settings_of.contains_key(&input.account_id) {
                        settings_of.insert(
                            input.account_id.clone(),
                            store.junk_settings(&input.account_id)?,
                        );
                    }
                    let settings = &settings_of[&input.account_id];
                    let classification = if settings.enabled {
                        Some(classifier::classify(
                            &StoreJunk(store),
                            &ClassifierParams::default(),
                            &input.account_id,
                            settings,
                            &input.tokens,
                        )?)
                    } else {
                        None
                    };
                    let decision = classifier::plan_arrival(
                        settings,
                        &MessageFacts {
                            message_id: Some(id),
                            folder_role: input.folder_role,
                            keywords: &input.keywords,
                        },
                        &input.account_id,
                        classification.as_ref(),
                    );
                    if let Some(verdict) = &decision.verdict {
                        store.set_junk_verdict(id, verdict, now)?;
                        if verdict.junk {
                            junk.push(id.clone());
                        }
                    }
                    if let Some(as_junk) = decision.train_as {
                        classifier::train(
                            &StoreJunk(store),
                            &input.account_id,
                            &input.key,
                            as_junk,
                            &input.tokens,
                            now,
                        )?;
                        if !trained.contains(&input.account_id) {
                            trained.push(input.account_id.clone());
                        }
                    }
                    ops.extend(decision.ops);
                }
                Ok((ops, junk, trained))
            })
            .await;
        match planned {
            Ok((ops, junk, trained)) => {
                if let Err(err) = self.run_triage_ops(ops).await {
                    tracing::warn!(%err, "cannot queue the junk handling of new mail");
                }
                for account_id in trained {
                    self.broadcast(Event::JunkStatusChanged { account_id });
                }
                junk
            }
            Err(err) => {
                tracing::warn!(%err, "cannot classify new mail");
                Vec::new()
            }
        }
    }

    /// Carries out planned triage writes: keywords and moves become queue
    /// operations (in order); the hidden Snoozed folder is created locally
    /// (the first move into it creates it on the server).
    pub(crate) async fn run_triage_ops(&self, ops: Vec<TriageOp>) -> Result<Vec<OpId>> {
        if ops.is_empty() {
            return Ok(Vec::new());
        }
        let operations = self.triage_operations(ops).await?;
        self.enqueue(operations).await
    }

    /// The queue operations for planned triage writes (creates the hidden
    /// Snoozed folder locally when an op needs it).
    pub(crate) async fn triage_operations(&self, ops: Vec<TriageOp>) -> Result<Vec<Operation>> {
        debug_assert!(!ops.iter().any(TriageOp::targets_trash));
        self.blocking(move |service| {
                let store = service.store();
                let mut out = Vec::new();
                for op in ops {
                    match op {
                        TriageOp::EnsureSnoozedFolder { account_id } => {
                            snoozed_folder(store, &account_id)?;
                        }
                        TriageOp::AddKeyword {
                            message_ids,
                            keyword,
                        } => out.extend(message_ids.into_iter().map(|message_id| {
                            Operation::UpdateFlags {
                                message_id,
                                change: nail_store::FlagChange {
                                    add_tags: vec![keyword.clone()],
                                    ..nail_store::FlagChange::default()
                                },
                            }
                        })),
                        TriageOp::RemoveKeyword {
                            message_ids,
                            keyword,
                        } => out.extend(message_ids.into_iter().map(|message_id| {
                            Operation::UpdateFlags {
                                message_id,
                                change: nail_store::FlagChange {
                                    remove_tags: vec![keyword.clone()],
                                    ..nail_store::FlagChange::default()
                                },
                            }
                        })),
                        TriageOp::Move { message_ids, to } => {
                            for message_id in message_ids {
                                let Ok(message) = store.get_message(&message_id) else {
                                    continue;
                                };
                                let Some(target) =
                                    resolve_target(store, &to, &message.summary.account_id)?
                                else {
                                    tracing::info!(message = %message_id, ?to, "no target folder; the message stays");
                                    continue;
                                };
                                if target != message.summary.folder_id {
                                    out.push(Operation::MoveMessage {
                                        message_id,
                                        target_folder_id: target,
                                        source: None,
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(out)
            })
            .await
    }
}

/// The folder a [`FolderTarget`] names for a message of `account`.
fn resolve_target(
    store: &Store,
    target: &FolderTarget,
    account: &AccountId,
) -> nail_store::Result<Option<FolderId>> {
    Ok(match target {
        FolderTarget::Folder(id) => store.get_folder(id).ok().map(|f| f.id),
        FolderTarget::Role { account_id, role } => store.folder_with_role(account_id, *role)?,
        FolderTarget::Snoozed { account_id } => {
            let account = if account_id == account {
                account_id
            } else {
                account
            };
            Some(snoozed_folder(store, account)?)
        }
    })
}

/// Names tried for the hidden Snoozed folder, in order: the first that no
/// folder of the account has yet. A folder of the user that is already
/// called `Snoozed` stays the user's (visible, never swept).
fn snoozed_folder_names() -> impl Iterator<Item = String> {
    let base = nail_api::triage::SNOOZED_FOLDER_NAME;
    std::iter::once(base.to_owned())
        .chain(std::iter::once(format!("Nail {base}")))
        .chain((2..=99).map(move |n| format!("Nail {base} {n}")))
}

/// The hidden Snoozed folder of an account: the folder Nail marked as its
/// own (store `folder_marks`), else a new top-level folder with the first
/// free name (created in the store and marked; the first move into it
/// creates it on the server). Hidden from `ListFolders`.
pub(crate) fn snoozed_folder(store: &Store, account: &AccountId) -> nail_store::Result<FolderId> {
    if let Some(id) = store.snoozed_folder_of(account)?
        && store.get_folder(&id).is_ok()
    {
        return Ok(id);
    }
    for name in snoozed_folder_names() {
        if store.find_folder_by_path(account, &name)?.is_some() {
            continue;
        }
        let folder = nail_api::Folder {
            id: crate::ids::folder_id(account, &name),
            account_id: account.clone(),
            parent_id: None,
            name: name.clone(),
            path: name,
            role: FolderRole::Other,
            unread: 0,
            total: 0,
        };
        store.upsert_folder(&folder)?;
        store.mark_snoozed_folder(&folder.id)?;
        return Ok(folder.id);
    }
    Err(nail_store::Error::InvalidData(
        "no free name for the Snoozed folder".to_owned(),
    ))
}

/// Whether a folder is Nail's hidden Snoozed folder (marked by
/// [`snoozed_folder`]; a user folder called `Snoozed` is not).
pub(crate) fn is_snoozed_folder(store: &Store, folder: &nail_api::Folder) -> bool {
    folder.parent_id.is_none()
        && store
            .is_snoozed_folder(&folder.id)
            .inspect_err(|err| tracing::warn!(%err, "cannot read folder marks"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_come_from_headers_and_text() {
        let raw = b"From: Spammer <win@lottery.example>\r\nSubject: You won a prize\r\n\r\nClaim your prize money today at http://lottery.example/claim\r\n";
        let tokens = tokens_of(raw);
        assert!(tokens.iter().any(|t| t.contains("prize")), "{tokens:?}");
        assert!(
            tokens.iter().any(|t| t.contains("lottery.example")),
            "{tokens:?}"
        );
        assert_eq!(tokens_of(b"").len(), 0);
    }
}
