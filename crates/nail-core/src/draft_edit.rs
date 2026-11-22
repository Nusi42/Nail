//! `EditDraftMessage`: continue writing a message of a Drafts folder.
//!
//! Nail keeps a copy of every saved draft in the account's Drafts folder
//! (`composer.rs`); such a copy answers with its draft. Any other message
//! there (a draft written in another client) is imported into a new saved
//! draft whose server copy is that message, so the next autosave replaces
//! it (and sending or discarding the draft deletes it) instead of leaving
//! a second copy behind.
//!
//! Only a message flagged `\Draft` is linked that way (ADR 0230, drafts
//! safety): any other message (received mail that landed in Drafts) is
//! copied into a new draft without a server copy, so nothing ever replaces
//! or deletes the original.

use nail_api::accounts::ComposerMode;
use nail_api::{
    AccountId, Address, ApiError, Draft, DraftContext, DraftContextMode, DraftId, FolderId,
    IdentityId, IncomingServer, MessageId, SignatureChoice,
};
use nail_store::{DraftServerCopy, Operation};

use crate::composer::encrypted_like;
use crate::error::Result;
use crate::service::Service;

/// Where a message is and what finds it again.
struct Located {
    folder_id: FolderId,
    account_id: AccountId,
    /// `Message-ID` (without angle brackets), `None` when it has none.
    header_id: Option<String>,
    /// `In-Reply-To` ids.
    in_reply_to: Vec<String>,
    /// Flagged `\Draft`: a real draft, which the new draft may replace.
    is_draft: bool,
}

impl Service {
    /// `EditDraftMessage`: the draft to continue for message `id`.
    pub(crate) async fn edit_draft_message(&self, id: MessageId) -> Result<DraftId> {
        let located = {
            let id = id.clone();
            self.inner
                .store
                .call(move |s| {
                    let stored = s.get_message(&id)?;
                    let meta = s.message_meta(&id)?;
                    Ok(Located {
                        is_draft: stored.summary.flags.draft,
                        folder_id: stored.summary.folder_id,
                        account_id: stored.summary.account_id,
                        header_id: meta.message_id,
                        in_reply_to: meta.in_reply_to,
                    })
                })
                .await?
        };
        // Nail's own copy of a saved draft (or a message imported before).
        // Only for a real draft: a received message that an older build
        // linked as a draft's server copy gets a new draft of its own.
        if located.is_draft
            && let Some(header_id) = located.header_id.clone()
        {
            let folder = located.folder_id.clone();
            let own = self
                .inner
                .store
                .call(move |s| s.draft_with_server_copy(&folder, &header_id))
                .await?;
            if let Some(draft_id) = own {
                return Ok(draft_id);
            }
        }
        let draft = self.draft_from_message(&id, &located).await?;
        let draft = self.checked_draft(draft)?;
        let (account, _) = self.identity(&draft.identity_id)?;
        // Without a Message-ID nothing finds the message again: on an IMAP
        // account (where Nail keeps the server copy) it is replaced by
        // Nail's own copy right away. A message that is not a draft is
        // never replaced, deleted or linked as the draft's server copy.
        let replace_now = located.is_draft
            && located.header_id.is_none()
            && matches!(account.incoming, IncomingServer::Imap { .. });
        let linked = located.header_id.clone().filter(|_| located.is_draft);
        let copy = linked.map(|message_id| DraftServerCopy {
            folder_id: located.folder_id.clone(),
            uid: None,
            message_id,
        });
        let message = id.clone();
        // One blocking call: a concurrent import of the same message finds
        // the draft this one wrote.
        let (draft_id, queued) = self
            .inner
            .store
            .call(move |s| {
                if let Some(copy) = &copy
                    && let Some(existing) =
                        s.draft_with_server_copy(&copy.folder_id, &copy.message_id)?
                {
                    return Ok((existing, Vec::new()));
                }
                let draft_id = s.save_draft(&draft)?;
                if let Some(copy) = &copy {
                    s.set_draft_server_copy(&draft_id, Some(copy))?;
                }
                let queued = if replace_now {
                    s.enqueue_all(&[Operation::DeleteMessage {
                        message_id: message,
                        permanent: true,
                        source: None,
                        trash_folder_id: None,
                    }])?
                } else {
                    Vec::new()
                };
                Ok((draft_id, queued))
            })
            .await?;
        if replace_now {
            self.announce_enqueued(&queued);
            self.schedule_draft_upload(&draft_id);
        }
        Ok(draft_id)
    }

    /// A draft with the content of message `id`: identity (from `From`),
    /// recipients, subject, body, attachments, the replied message and,
    /// for encrypted mail, encryption.
    async fn draft_from_message(&self, id: &MessageId, located: &Located) -> Result<Draft> {
        let body = self.message_body(id.clone()).await?;
        let identity_id = self
            .draft_identity(&located.account_id, body.headers.from.first())
            .await?;
        let (_, identity) = self.identity(&identity_id)?;
        let (bcc, attachments) = {
            let service = self.clone();
            let id = id.clone();
            tokio::task::spawn_blocking(move || -> Result<(Vec<Address>, _)> {
                Ok(match service.readable_message(&id)? {
                    Some(raw) => (
                        nail_mime::bcc_addresses(&raw),
                        service.copy_attachments(&raw, true)?,
                    ),
                    None => (Vec::new(), Vec::new()),
                })
            })
            .await??
        };
        let context = self.replied_message(id, located).await;
        let html = body.sanitized_html;
        let mut draft = Draft::new(identity_id);
        draft.to = body.headers.to;
        draft.cc = body.headers.cc;
        draft.bcc = bcc;
        draft.subject = body.headers.subject;
        draft.mode = if html.is_some() {
            ComposerMode::Html
        } else {
            ComposerMode::Plain
        };
        draft.text = body.text;
        draft.html = html;
        draft.attachments = attachments;
        draft.context = context;
        // The body already holds whatever signature it was written with.
        draft.signature = SignatureChoice::None;
        draft.crypto = encrypted_like(&identity, body.security.as_ref());
        Ok(draft)
    }

    /// The identity of the message's account whose address sent it, else
    /// the account's first identity.
    async fn draft_identity(
        &self,
        account: &AccountId,
        from: Option<&Address>,
    ) -> Result<IdentityId> {
        let identities = self.list_identities(Some(account)).await?;
        let sender = from.map(|a| a.email.to_ascii_lowercase());
        identities
            .iter()
            .find(|i| sender.as_deref() == Some(i.email.to_ascii_lowercase().as_str()))
            .or_else(|| identities.first())
            .map(|i| i.id.clone())
            .ok_or_else(|| {
                ApiError::invalid_request(format!("account {account} has no identity to send from"))
                    .into()
            })
    }

    /// The reply context of a draft from its `In-Reply-To`: the replied
    /// message of the same account, `None` when it is not stored.
    async fn replied_message(&self, id: &MessageId, located: &Located) -> Option<DraftContext> {
        let parent = located.in_reply_to.first()?.clone();
        let account = located.account_id.clone();
        let own = id.clone();
        let found = self
            .inner
            .store
            .call(move |s| s.find_messages_by_message_id(&parent))
            .await
            .ok()?;
        found
            .into_iter()
            .find(|m| m.account_id == account && m.id != own)
            .map(|m| DraftContext {
                mode: DraftContextMode::Reply,
                message_id: m.id,
            })
    }
}
