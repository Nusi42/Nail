//! Conversations (P2, ADR 0148; WP201).
//!
//! A list row in a folder is a conversation: the thread's messages in the
//! listed folder (or the member folders of a virtual folder) plus the
//! user's own messages from the Sent folders of the same accounts
//! (`from_me`), never drafts — a listed Drafts folder shows every draft on
//! its own (`single-<message id>`). Without a folder a conversation is the
//! thread's messages in every folder but Drafts, Trash and Junk. The store
//! does the work (`nail_store::thread::conversation`); the core resolves
//! virtual folders, "me" from the identities (`accounts.kdl`, compared
//! case-insensitively), and narrows the folders to a remote session's grant
//! ([`FolderFilter`]).

use std::sync::Arc;

use nail_api::{ConversationMessage, FolderId, MessageSummary, ThreadId, ThreadSummary};
use nail_store::{ConversationFolders, OwnAddresses, ScopeFolder, ThreadPage};

use crate::error::Result;
use crate::service::Service;

/// Which folders a caller may see (a remote session's grant); callers pass
/// `None` for all.
pub(crate) type FolderFilter = Arc<dyn Fn(&ScopeFolder) -> bool + Send + Sync>;

/// A filter as the store's conversation queries take it.
pub(crate) fn as_keep(filter: Option<&FolderFilter>) -> Option<&dyn Fn(&ScopeFolder) -> bool> {
    match filter {
        Some(keep) => Some(keep.as_ref()),
        None => None,
    }
}

impl Service {
    /// The user's addresses per account: the configured accounts' addresses
    /// and identities, and the stored accounts' addresses (local and
    /// registered accounts). An identity that sends as or on behalf of
    /// another mailbox (`send-as=`, ADR 0094) names a shared or delegate
    /// mailbox, not the user: colleagues write from it too, so only its
    /// `sender=` address counts (P2 review).
    pub(crate) async fn own_addresses_by_account(&self) -> Result<OwnAddresses> {
        let mut own = OwnAddresses::new();
        for account in &self.inner.accounts.config().accounts {
            own.add(&account.id, &account.email);
            for identity in &account.identities {
                match &identity.send_as {
                    None => own.add(&account.id, &identity.email),
                    Some(send_as) => {
                        if let Some(sender) = &send_as.sender {
                            own.add(&account.id, sender);
                        }
                    }
                }
            }
        }
        for account in self.stored_accounts().await? {
            own.add(&account.id, &account.email);
        }
        Ok(own)
    }

    /// The conversation folders behind a listed folder id, narrowed by
    /// `filter`. Unknown folders fail with `not_found`, as for
    /// `ListMessages`.
    pub(crate) async fn conversation_scope(
        &self,
        folder: &FolderId,
        filter: Option<&FolderFilter>,
    ) -> Result<ConversationFolders> {
        let resolved = self.resolve_folder(folder).await?;
        if !resolved.is_virtual {
            let id = folder.clone();
            self.inner.store.call(move |s| s.get_folder(&id)).await?;
        }
        let mut scope = self
            .inner
            .store
            .call(move |s| s.conversation_folders(&resolved.sources))
            .await?;
        if let Some(keep) = filter {
            scope.retain(|f| keep(f));
        }
        Ok(scope)
    }

    /// `ListThreads`: a page of the folder's conversations, newest activity
    /// first, within `filter`.
    pub(crate) async fn list_conversations(
        &self,
        folder: &FolderId,
        offset: u32,
        limit: u32,
        filter: Option<&FolderFilter>,
    ) -> Result<ThreadPage> {
        // One store hop for the folder check, the scope and the page (as
        // `conversation_scope` + the listing): the list is the hot path
        // (P3, `thread-page-p95-ms`).
        let resolved = self.resolve_folder(folder).await?;
        let own = self.own_addresses_by_account().await?;
        let id = folder.clone();
        let filter = filter.cloned();
        Ok(self
            .inner
            .store
            .call(move |s| {
                if !resolved.is_virtual {
                    s.get_folder(&id)?;
                }
                let mut scope = s.conversation_folders(&resolved.sources)?;
                if let Some(keep) = &filter {
                    scope.retain(|f| keep(f));
                }
                s.list_conversations(&scope, &own, offset, limit)
            })
            .await?)
    }

    /// `GetConversation`: the conversation's row and its messages, oldest
    /// first, as listed in `folder` (within `filter`), or without a folder
    /// across every folder but Drafts, Trash and Junk.
    pub(crate) async fn get_conversation(
        &self,
        thread: &ThreadId,
        folder: Option<&FolderId>,
        filter: Option<&FolderFilter>,
    ) -> Result<(ThreadSummary, Vec<ConversationMessage>)> {
        let scope = match folder {
            Some(folder) => Some(self.conversation_scope(folder, filter).await?),
            None => None,
        };
        let own = self.own_addresses_by_account().await?;
        let thread = thread.clone();
        // Without a folder the copies are read everywhere: the grant drops
        // the copies it does not cover before copies collapse.
        let keep = if scope.is_none() {
            filter.cloned()
        } else {
            None
        };
        Ok(self
            .inner
            .store
            .call(move |s| {
                s.conversation_within(&thread, scope.as_ref(), &own, as_keep(keep.as_ref()))
            })
            .await?)
    }

    /// The row of a conversation rebuilt from the messages a caller may
    /// see (a remote session's narrowed answer): nothing about the dropped
    /// messages remains in the counts, participants or newest message.
    pub(crate) async fn conversation_row(
        &self,
        thread: &ThreadId,
        messages: &[ConversationMessage],
    ) -> Result<Option<ThreadSummary>> {
        let own = self.own_addresses_by_account().await?;
        Ok(nail_store::summarize_conversation(
            thread.as_str(),
            messages,
            &own,
        ))
    }

    /// [`Self::conversation_row`] for a thread's plain messages
    /// (`GetThread`).
    pub(crate) async fn thread_row(
        &self,
        thread: &ThreadId,
        messages: &[MessageSummary],
    ) -> Result<Option<ThreadSummary>> {
        let messages: Vec<ConversationMessage> = messages
            .iter()
            .map(|m| ConversationMessage {
                message: m.clone(),
                from_me: false,
            })
            .collect();
        self.conversation_row(thread, &messages).await
    }
}
