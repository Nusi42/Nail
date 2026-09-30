//! Mail requests: accounts and folders as listed (virtual accounts
//! included), message lists, threads, and the writes that go through the
//! operation queue.

use std::collections::{HashMap, HashSet};

use nail_api::{
    Account, AccountId, AccountKind, ApiError, Event, FlagChange, Folder, FolderId, MessageId,
    MessageSummary, MoveTarget, OpId, OpState, OpStatus, ThreadId, ThreadSummary,
};
use nail_store::{Enqueued, MessagePage, Operation, ThreadPage};

use crate::error::Result;
use crate::ids;
use crate::service::Service;
use crate::virtuals::{self, VirtualFolder};

/// What a folder id stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedFolder {
    /// The real folders it shows.
    pub sources: Vec<FolderId>,
    /// Whether it is a virtual folder.
    pub is_virtual: bool,
}

impl Service {
    /// `ListAccounts`: "All accounts" first, then the configured accounts
    /// in file order (plus local accounts), then the user's virtual
    /// accounts.
    pub(crate) async fn list_accounts(&self) -> Result<Vec<Account>> {
        let stored = self.stored_accounts().await?;
        let config = self.inner.accounts.config();
        let mut out = vec![Account {
            id: AccountId::all(),
            name: String::new(),
            email: String::new(),
            kind: AccountKind::Virtual,
        }];
        for account in &config.accounts {
            out.push(Account {
                id: account.id.clone(),
                name: account.name.clone(),
                email: account.email.clone(),
                kind: account.kind(),
            });
        }
        for account in stored {
            if virtuals::extra_account(&config, &account)
                || (config.account(&account.id).is_none()
                    && self.inner.manual.is_registered(&account.id))
            {
                out.push(account);
            }
        }
        for va in &config.virtual_accounts {
            out.push(Account {
                id: va.id.clone(),
                name: va.name.clone(),
                email: String::new(),
                kind: AccountKind::Virtual,
            });
        }
        Ok(out)
    }

    /// The members of "All accounts" (every enabled configured account,
    /// local and registered accounts).
    pub(crate) async fn all_members(&self) -> Result<Vec<AccountId>> {
        let stored = self.stored_accounts().await?;
        let config = self.inner.accounts.config();
        let mut members = virtuals::all_members(&config, &stored);
        for account in stored {
            if config.account(&account.id).is_none()
                && self.inner.manual.is_registered(&account.id)
                && !members.contains(&account.id)
            {
                members.push(account.id);
            }
        }
        Ok(members)
    }

    /// The folders of a virtual account (`None` for other ids).
    pub(crate) async fn virtual_folders_of(
        &self,
        account: &AccountId,
    ) -> Result<Option<Vec<VirtualFolder>>> {
        let stored = self.stored_accounts().await?;
        let config = self.inner.accounts.config();
        let Some(mut va) = virtuals::virtual_config(&config, &stored, account) else {
            return Ok(None);
        };
        if account.as_str() == AccountId::ALL {
            va.members = self.all_members().await?;
        }
        let members = va.members.clone();
        let folders = self
            .inner
            .store
            .call(move |s| {
                let mut all = Vec::new();
                for member in &members {
                    all.extend(
                        s.list_folders(member)?
                            .into_iter()
                            .filter(|f| !crate::junk::is_snoozed_folder(s, f)),
                    );
                }
                Ok(all)
            })
            .await?;
        let mut names: HashMap<AccountId, String> =
            stored.into_iter().map(|a| (a.id, a.name)).collect();
        for account in &config.accounts {
            names.insert(account.id.clone(), account.name.clone());
        }
        Ok(Some(virtuals::virtual_folders(&va, &folders, &names)))
    }

    /// `ListFolders` for real and virtual accounts.
    pub(crate) async fn list_folders(&self, account: &AccountId) -> Result<Vec<Folder>> {
        if let Some(folders) = self.virtual_folders_of(account).await? {
            return Ok(folders.into_iter().map(|v| v.folder).collect());
        }
        let id = account.clone();
        // The hidden folder snoozed mail waits in (M2c).
        let folders = self
            .inner
            .store
            .call(move |s| {
                let mut folders = s.list_folders(&id)?;
                folders.retain(|f| !crate::junk::is_snoozed_folder(s, f));
                Ok(folders)
            })
            .await?;
        Ok(folders)
    }

    /// The real folders behind a folder id. Fails with `not_found` for an
    /// unknown virtual folder.
    pub(crate) async fn resolve_folder(&self, folder: &FolderId) -> Result<ResolvedFolder> {
        if !virtuals::is_virtual_folder(folder) {
            return Ok(ResolvedFolder {
                sources: vec![folder.clone()],
                is_virtual: false,
            });
        }
        let account = virtuals::virtual_account_of(folder)
            .ok_or_else(|| ApiError::not_found(format!("folder {folder}")))?;
        let found = self
            .virtual_folders_of(&account)
            .await?
            .and_then(|folders| folders.into_iter().find(|v| &v.folder.id == folder))
            .ok_or_else(|| ApiError::not_found(format!("folder {folder}")))?;
        Ok(ResolvedFolder {
            sources: found.sources,
            is_virtual: true,
        })
    }

    /// `ListMessages`: a real folder's page, or a virtual folder's page with
    /// copies of one mail collapsed.
    pub(crate) async fn list_messages(
        &self,
        folder: &FolderId,
        offset: u32,
        limit: u32,
    ) -> Result<MessagePage> {
        let resolved = self.resolve_folder(folder).await?;
        let store = self.inner.store.clone();
        if !resolved.is_virtual {
            let id = folder.clone();
            return Ok(store
                .call(move |s| s.list_messages(&id, offset, limit))
                .await?);
        }
        Ok(store
            .call(move |s| s.list_messages_in(&resolved.sources, offset, limit))
            .await?)
    }

    /// `ListThreads` over a real or virtual folder.
    pub(crate) async fn list_threads(
        &self,
        folder: &FolderId,
        offset: u32,
        limit: u32,
    ) -> Result<ThreadPage> {
        let resolved = self.resolve_folder(folder).await?;
        if !resolved.is_virtual {
            // Unknown real folders are an error, as for ListMessages.
            let id = folder.clone();
            self.inner.store.call(move |s| s.get_folder(&id)).await?;
        }
        Ok(self
            .inner
            .store
            .call(move |s| s.list_threads(&resolved.sources, offset, limit))
            .await?)
    }

    /// `GetThread`: every stored message of the thread, across folders and
    /// accounts, oldest first.
    pub(crate) async fn get_thread(
        &self,
        thread: &ThreadId,
    ) -> Result<(ThreadSummary, Vec<MessageSummary>)> {
        let thread = thread.clone();
        Ok(self
            .inner
            .store
            .call(move |s| s.get_thread(&thread, None))
            .await?)
    }

    /// Applies operations locally, queues them and tells the engines.
    pub(crate) async fn enqueue(&self, operations: Vec<Operation>) -> Result<Vec<OpId>> {
        if operations.is_empty() {
            return Ok(Vec::new());
        }
        let queued: Vec<Enqueued> = self
            .inner
            .store
            .call(move |s| s.enqueue_all(&operations))
            .await?;
        Ok(self.announce_enqueued(&queued))
    }

    /// Tells the UIs and the engines about operations queued in the store
    /// (by [`Self::enqueue`], or by a store call that queued them together
    /// with other writes).
    pub(crate) fn announce_enqueued(&self, queued: &[Enqueued]) -> Vec<OpId> {
        if queued.is_empty() {
            return Vec::new();
        }
        let mut folders: Vec<FolderId> = Vec::new();
        let mut accounts: Vec<AccountId> = Vec::new();
        let mut op_ids = Vec::with_capacity(queued.len());
        for item in queued {
            for folder in &item.changed_folders {
                if !folders.contains(folder) {
                    folders.push(folder.clone());
                }
            }
            if !accounts.contains(&item.op.account_id) {
                accounts.push(item.op.account_id.clone());
            }
            let op_id = ids::op_id(item.op.id);
            self.broadcast(Event::OpStatus {
                status: OpStatus {
                    op_id: op_id.clone(),
                    account_id: item.op.account_id.clone(),
                    state: OpState::Pending,
                    error: None,
                    message: None,
                    localized: None,
                },
            });
            op_ids.push(op_id);
        }
        // Deleted messages leave the search index at once.
        let gone: Vec<MessageId> = queued
            .iter()
            .filter_map(|q| match &q.op.operation {
                Operation::DeleteMessage {
                    message_id,
                    trash_folder_id: None,
                    ..
                } => Some(message_id.clone()),
                _ => None,
            })
            .collect();
        if !gone.is_empty() {
            let index = self.inner.search.get();
            tokio::task::spawn_blocking(move || {
                if let Err(err) = index.remove(&gone) {
                    tracing::warn!(%err, "cannot remove deleted messages from the index");
                }
            });
        }
        self.broadcast(Event::MailChanged {
            folder_ids: folders,
        });
        for account in accounts {
            self.request_replay(&account);
        }
        op_ids
    }

    /// `RetryOp`: puts a failed operation back into its account's queue
    /// and replays it.
    pub(crate) async fn retry_op(&self, id: &OpId) -> Result<Vec<OpId>> {
        let op =
            ids::parse_op_id(id).ok_or_else(|| ApiError::not_found(format!("operation {id}")))?;
        let (queued, retried) = self
            .inner
            .store
            .call(move |s| {
                let queued = s.get_op(op)?;
                let retried = s.retry_op(op)?;
                Ok((queued, retried))
            })
            .await?;
        if !retried {
            return Err(ApiError::new(
                nail_api::ErrorCode::Conflict,
                format!("operation {id} has not failed"),
            )
            .into());
        }
        self.broadcast(Event::OpStatus {
            status: OpStatus {
                op_id: id.clone(),
                account_id: queued.account_id.clone(),
                state: OpState::Pending,
                error: None,
                message: None,
                localized: None,
            },
        });
        self.request_replay(&queued.account_id);
        Ok(vec![id.clone()])
    }

    /// `UpdateFlags`: one operation per message.
    pub(crate) async fn update_flags(
        &self,
        messages: &[MessageId],
        change: FlagChange,
    ) -> Result<Vec<OpId>> {
        if change.is_empty() {
            return Err(ApiError::invalid_request("the flag change changes nothing").into());
        }
        for tag in change.add_tags.iter().chain(&change.remove_tags) {
            if !is_valid_tag(tag) {
                return Err(ApiError::invalid_request(format!("invalid tag {tag:?}")).into());
            }
        }
        let change: nail_store::FlagChange = change.into();
        self.enqueue(
            unique(messages)
                .into_iter()
                .map(|message_id| Operation::UpdateFlags {
                    message_id,
                    change: change.clone(),
                })
                .collect(),
        )
        .await
    }

    /// `MoveMessages`: the target is resolved per message (a role or a
    /// virtual folder means the matching folder of the message's own
    /// account).
    pub(crate) async fn move_messages(
        &self,
        messages: &[MessageId],
        target: &MoveTarget,
    ) -> Result<Vec<OpId>> {
        let messages = unique(messages);
        let ids = messages.clone();
        let summaries = self
            .inner
            .store
            .call(move |s| s.message_summaries(&ids))
            .await?;
        let by_id: HashMap<MessageId, MessageSummary> =
            summaries.into_iter().map(|m| (m.id.clone(), m)).collect();
        let virtual_sources = match target {
            MoveTarget::FolderId(folder) if virtuals::is_virtual_folder(folder) => {
                Some(self.resolve_folder(folder).await?.sources)
            }
            _ => None,
        };
        let mut operations = Vec::new();
        for id in messages {
            let summary = by_id
                .get(&id)
                .ok_or_else(|| ApiError::not_found(format!("message {id}")))?;
            let account = summary.account_id.clone();
            let target_folder = match (target, &virtual_sources) {
                (MoveTarget::FolderId(_), Some(sources)) => {
                    let sources = sources.clone();
                    let account = account.clone();
                    self.inner
                        .store
                        .call(move |s| {
                            for source in &sources {
                                if s.get_folder(source)?.account_id == account {
                                    return Ok(Some(source.clone()));
                                }
                            }
                            Ok(sources.first().cloned())
                        })
                        .await?
                        .ok_or_else(|| {
                            ApiError::invalid_request("the target folder has no real folder")
                        })?
                }
                (MoveTarget::FolderId(folder), None) => folder.clone(),
                (MoveTarget::Role(role), _) => {
                    let role = *role;
                    let lookup = account.clone();
                    self.inner
                        .store
                        .call(move |s| s.folder_with_role(&lookup, role))
                        .await?
                        .ok_or_else(|| {
                            ApiError::not_found(format!(
                                "account {account} has no {} folder",
                                serde_json::to_value(role)
                                    .ok()
                                    .and_then(|v| v.as_str().map(str::to_owned))
                                    .unwrap_or_default()
                            ))
                        })?
                }
            };
            if target_folder == summary.folder_id {
                continue;
            }
            operations.push(Operation::MoveMessage {
                message_id: id,
                target_folder_id: target_folder,
                source: None,
            });
        }
        self.enqueue(operations).await
    }

    /// `DeleteMessages`: to the account's Trash, or for good.
    pub(crate) async fn delete_messages(
        &self,
        messages: &[MessageId],
        permanent: bool,
    ) -> Result<Vec<OpId>> {
        self.enqueue(
            unique(messages)
                .into_iter()
                .map(|message_id| Operation::DeleteMessage {
                    message_id,
                    permanent,
                    source: None,
                    trash_folder_id: None,
                })
                .collect(),
        )
        .await
    }
}

/// The ids in order, each once.
fn unique(ids: &[MessageId]) -> Vec<MessageId> {
    let mut seen = HashSet::new();
    ids.iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect()
}

/// Whether a tag can be stored as an IMAP keyword (an atom without
/// specials, not a system flag).
pub(crate) fn is_valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && !tag.starts_with('\\')
        && tag
            .bytes()
            .all(|b| b > b' ' && b < 0x7f && !b"(){%*\"]\\".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_imap_atoms() {
        for ok in ["$label1", "work", "Junk", "x.y-z"] {
            assert!(is_valid_tag(ok), "{ok}");
        }
        for bad in ["", "a b", "\\Seen", "(x)", "ä", "a\"b"] {
            assert!(!is_valid_tag(bad), "{bad}");
        }
    }

    #[test]
    fn unique_keeps_order() {
        let ids: Vec<MessageId> = ["b", "a", "b"].into_iter().map(MessageId::new).collect();
        assert_eq!(unique(&ids), [MessageId::new("b"), MessageId::new("a")]);
    }
}
