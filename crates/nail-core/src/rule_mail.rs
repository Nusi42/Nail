//! What rules do besides queue operations (ADR 0024): copy a message into
//! another folder, forward it, redirect it unchanged and reply with a
//! template. None of these can be undone; they are recorded in the run
//! without an inverse.
//!
//! * A copy is stored locally at once and, for an IMAP account, filed on
//!   the server through the operation queue (`AppendMessage`), like a Sent
//!   copy.
//! * Forwards and template replies are drafts sent through the outbox
//!   (no undo-send delay), so they get a Sent copy and retries.
//! * A redirect submits the original bytes with `Resent-*` headers
//!   (RFC 5322 §3.6.6) through the account's SMTP server.

use std::collections::BTreeMap;

use nail_api::accounts::ComposerMode;
use nail_api::engine::{Envelope, OutgoingTransport};
use nail_api::{
    Address, ApiError, DraftContext, DraftContextMode, Event, FolderId, IdentityId, IncomingServer,
    MessageId,
};
use nail_store::Operation;

use crate::error::Result;
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};
use crate::service::Service;

fn addresses(to: &[String]) -> Vec<Address> {
    to.iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|email| Address {
            name: None,
            email: email.to_owned(),
        })
        .collect()
}

impl Service {
    /// The raw bytes of a stored message.
    async fn raw_message(&self, id: &MessageId) -> Result<(nail_store::StoredMessage, Vec<u8>)> {
        let store = self.inner.store.clone();
        let blobs = self.inner.blobs.clone();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<_> {
            let stored = store.get_message(&id)?;
            let hash = stored.blob_hash.clone().ok_or_else(|| {
                ApiError::new(
                    nail_api::ErrorCode::Unavailable,
                    format!("the body of message {id} is not downloaded yet"),
                )
            })?;
            let raw = blobs.get(&hash)?;
            Ok((stored, raw))
        })
        .await?
    }

    /// Copies a message into a folder (rule action `copy`).
    pub(crate) async fn copy_message(&self, message: &MessageId, target: &FolderId) -> Result<()> {
        let (stored, raw) = self.raw_message(message).await?;
        let folder = {
            let id = target.clone();
            self.inner.store.call(move |s| s.get_folder(&id)).await?
        };
        let id = ids::local_message_id(&folder.id, &format!("copy:{message}"));
        let incoming = IncomingMessage {
            id: Some(id.clone()),
            flags: stored.summary.flags,
            keywords: stored.summary.tags.clone(),
            raw,
            ..IncomingMessage::default()
        };
        let blobs = self.inner.blobs.clone();
        let index = std::sync::Arc::clone(&self.inner.search);
        let target_folder = folder.clone();
        self.inner
            .store
            .call(move |s| ingest(s, &blobs, Some(&index), &target_folder, 0, &[incoming]))
            .await?;
        let imap = self
            .inner
            .accounts
            .config()
            .account(&folder.account_id)
            .is_some_and(|a| matches!(a.incoming, IncomingServer::Imap { .. }));
        if imap {
            self.enqueue(vec![Operation::AppendMessage {
                message_id: id,
                folder_id: folder.id.clone(),
            }])
            .await?;
        } else {
            self.broadcast(Event::MailChanged {
                folder_ids: vec![folder.id.clone()],
            });
        }
        Ok(())
    }

    /// The identity a rule sends from: the named one, or the one the
    /// message was addressed to (else the account's first).
    async fn rule_identity(
        &self,
        message: &MessageId,
        account: &nail_api::AccountId,
        identity: Option<IdentityId>,
    ) -> Result<IdentityId> {
        match identity {
            Some(id) => Ok(id),
            None => self
                .choose_identity(Some(account), &[], Some(message.clone()))
                .await?
                .ok_or_else(|| {
                    ApiError::invalid_request(format!("account {account} has no identity")).into()
                }),
        }
    }

    /// Sends a prepared draft at once through the outbox.
    async fn send_now(&self, draft: nail_api::Draft) -> Result<()> {
        let id = self.save_draft(draft).await?;
        self.send_draft(id, Some(0), None).await?;
        Ok(())
    }

    /// Rule action `forward`.
    pub(crate) async fn rule_forward(
        &self,
        message: &MessageId,
        to: &[String],
        identity: Option<IdentityId>,
    ) -> Result<()> {
        let (stored, _) = self.raw_message(message).await?;
        let identity = self
            .rule_identity(message, &stored.summary.account_id, identity)
            .await?;
        let mut draft = self
            .new_draft(
                Some(identity),
                None,
                None,
                Some(DraftContext {
                    mode: DraftContextMode::Forward,
                    message_id: message.clone(),
                }),
            )
            .await?;
        draft.to = addresses(to);
        self.send_now(draft).await
    }

    /// Rule action `reply`: the template (Markdown, `{variables}`) above the
    /// quoted original.
    pub(crate) async fn rule_reply(
        &self,
        message: &MessageId,
        template: &str,
        identity: Option<IdentityId>,
    ) -> Result<()> {
        let (stored, _) = self.raw_message(message).await?;
        let identity_id = self
            .rule_identity(message, &stored.summary.account_id, identity)
            .await?;
        let (_, identity) = self.identity(&identity_id)?;
        let mut draft = self
            .new_draft(
                Some(identity_id),
                None,
                Some(ComposerMode::Markdown),
                Some(DraftContext {
                    mode: DraftContextMode::Reply,
                    message_id: message.clone(),
                }),
            )
            .await?;
        let from = stored.summary.from.clone();
        let vars = BTreeMap::from([
            (
                "sender.name".to_owned(),
                from.as_ref()
                    .and_then(|a| a.name.clone())
                    .or_else(|| from.as_ref().map(|a| a.email.clone()))
                    .unwrap_or_default(),
            ),
            (
                "sender.email".to_owned(),
                from.as_ref().map(|a| a.email.clone()).unwrap_or_default(),
            ),
            ("subject".to_owned(), stored.summary.subject.clone()),
            ("date".to_owned(), stored.summary.date.clone()),
            (
                "identity.name".to_owned(),
                identity.name.clone().unwrap_or_default(),
            ),
            ("identity.email".to_owned(), identity.email.clone()),
        ]);
        let text = nail_compose::expand_template(template, &vars)
            .map_err(|e| ApiError::invalid_request(format!("reply template: {e}")))?;
        let quoted = draft.markdown.take().unwrap_or_default();
        draft.markdown = Some(format!("{}\n\n{}", text.trim_end(), quoted.trim_start()));
        self.send_now(draft).await
    }

    /// Rule action `redirect`: the message unchanged, with `Resent-*`
    /// headers, to new recipients.
    pub(crate) async fn rule_redirect(&self, message: &MessageId, to: &[String]) -> Result<()> {
        let (stored, raw) = self.raw_message(message).await?;
        let recipients: Vec<String> = addresses(to).into_iter().map(|a| a.email).collect();
        if recipients.is_empty() {
            return Err(ApiError::invalid_request("redirect has no recipients").into());
        }
        let identity_id = self
            .rule_identity(message, &stored.summary.account_id, None)
            .await?;
        let (account, identity) = self.identity(&identity_id)?;
        let transport = self.transport(&account)?;
        let resent_id = nail_compose::new_message_id(&identity.email, message.as_str().as_bytes());
        let mut bytes = format!(
            "Resent-From: {}\r\nResent-To: {}\r\nResent-Date: {}\r\nResent-Message-ID: <{}>\r\n",
            identity.email,
            recipients.join(", "),
            chrono::Local::now().to_rfc2822(),
            resent_id
        )
        .into_bytes();
        bytes.extend_from_slice(&raw);
        let envelope = Envelope {
            from: identity.email.clone(),
            to: recipients,
        };
        transport
            .send(&envelope, &bytes)
            .await
            .map_err(ApiError::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_recipients_become_addresses() {
        assert_eq!(
            addresses(&[" a@example.org ".into(), String::new()]),
            vec![Address {
                name: None,
                email: "a@example.org".into()
            }]
        );
    }
}
