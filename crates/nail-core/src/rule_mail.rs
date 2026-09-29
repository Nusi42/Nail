//! What rules do besides queue operations (ADR 0024): copy a message into
//! another folder, forward it, redirect it unchanged and reply with a
//! template. None of these can be undone; they are recorded in the run
//! without an inverse.
//!
//! * A copy is stored locally at once and, for an IMAP account, filed on
//!   the server through the operation queue (`AppendMessage`), like a Sent
//!   copy.
//! * Forwards, template replies and redirects are drafts sent through the
//!   outbox (no undo-send delay), so they get retries, survive being
//!   offline and get a Sent copy. A redirect's draft only names the
//!   message; the outbox submits its original bytes with `Resent-*`
//!   headers (RFC 5322 §3.6.6).
//! * Rule mail is marked ([`Draft::automatic`]: `Auto-Submitted` and the
//!   `X-Nail-Loop` header), on-send rules skip it, and rules never act on
//!   marked mail again (ADR 0028), so rules cannot loop.
//! * A template reply renders the user's Markdown template and inserts the
//!   message's values (subject, sender) as escaped text, so a sender cannot
//!   put links or images into the user's automatic reply.

use std::collections::BTreeMap;

use nail_api::accounts::ComposerMode;
use nail_api::engine::Envelope;
use nail_api::{
    Address, ApiError, AutomaticMail, Draft, DraftAttachment, DraftContext, DraftContextMode,
    Event, FolderId, Identity, IdentityId, IncomingServer, MessageId,
};
use nail_compose::{MessageMeta, OutgoingMessage};
use nail_store::Operation;

use crate::error::Result;
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};
use crate::service::Service;

/// File name of the original a redirect draft carries.
const REDIRECT_FILENAME: &str = "redirect.eml";

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
    pub(crate) async fn raw_message(
        &self,
        id: &MessageId,
    ) -> Result<(nail_store::StoredMessage, Vec<u8>)> {
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
        let index = self.inner.search.get();
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

    /// Sends a prepared rule draft at once through the outbox (no server
    /// Drafts copy: it is sent right away).
    async fn send_now(&self, mut draft: Draft, automatic: AutomaticMail) -> Result<()> {
        draft.automatic = Some(automatic);
        let id = self.store_draft(draft, false).await?;
        self.send_draft(id, Some(0), None, None).await?;
        Ok(())
    }

    /// Rule action `forward`.
    pub(crate) async fn rule_forward(
        &self,
        message: &MessageId,
        to: &[String],
        identity: Option<IdentityId>,
    ) -> Result<()> {
        let (stored, raw) = self.raw_message(message).await?;
        self.refuse_encrypted(&raw, "forward").await?;
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
        self.send_now(draft, AutomaticMail::Forward).await
    }

    /// Rule action `reply`: the template (Markdown, `{variables}`) above the
    /// quoted original.
    pub(crate) async fn rule_reply(
        &self,
        message: &MessageId,
        template: &str,
        identity: Option<IdentityId>,
    ) -> Result<()> {
        let (stored, raw) = self.raw_message(message).await?;
        self.refuse_encrypted(&raw, "reply to").await?;
        let identity_id = self
            .rule_identity(message, &stored.summary.account_id, identity)
            .await?;
        let (_, identity) = self.identity(&identity_id)?;
        let mut draft = self
            .new_draft(
                Some(identity_id.clone()),
                None,
                Some(ComposerMode::Html),
                Some(DraftContext {
                    mode: DraftContextMode::Reply,
                    message_id: message.clone(),
                }),
            )
            .await?;
        let vars = reply_variables(&stored.summary, &identity);
        let style = crate::composer::style_of(&identity);
        let template = template.to_owned();
        let text = tokio::task::spawn_blocking(move || {
            nail_compose::render_markdown_template(&template, &vars, &style)
        })
        .await?
        .map_err(|e| ApiError::invalid_request(format!("reply template: {e}")))?;
        let quoted = draft.html.take().unwrap_or_default();
        draft.html = Some(format!("{text}{quoted}"));
        self.send_now(draft, AutomaticMail::Reply).await
    }

    /// A message prepared by the core (an Autocrypt setup message) in the
    /// draft's first attachment, sent unchanged to the draft's recipients.
    pub(crate) async fn prepared_message(
        &self,
        draft: &Draft,
        identity: &Identity,
    ) -> Result<OutgoingMessage> {
        let prepared = draft.attachments.first().ok_or_else(|| {
            ApiError::internal("the prepared message of an automatic mail is missing")
        })?;
        let hash = nail_store::BlobHash::parse(&prepared.blob)?;
        let blobs = self.inner.blobs.clone();
        let raw = tokio::task::spawn_blocking(move || blobs.get(&hash)).await??;
        let message_id = nail_mime::parse_message(&raw)
            .headers
            .message_id
            .unwrap_or_default();
        Ok(OutgoingMessage {
            envelope: Envelope {
                from: identity.email.trim().to_owned(),
                to: draft.to.iter().map(|a| a.email.trim().to_owned()).collect(),
            },
            raw,
            message_id,
        })
    }

    /// Refuses automatic forwards and replies of encrypted mail: they would
    /// send its decrypted content without encryption (ADR 0034).
    async fn refuse_encrypted(&self, raw: &[u8], action: &str) -> Result<()> {
        if crate::crypto::is_encrypted_raw(raw) {
            return Err(ApiError::unsupported(format!(
                "a rule cannot {action} an encrypted message"
            ))
            .into());
        }
        Ok(())
    }

    /// Rule action `redirect`: the message unchanged, with `Resent-*`
    /// headers, to new recipients — queued in the outbox like every other
    /// message ([`Self::redirect_message`] builds it when it is sent).
    pub(crate) async fn rule_redirect(&self, message: &MessageId, to: &[String]) -> Result<()> {
        let (stored, raw) = self.raw_message(message).await?;
        let recipients = addresses(to);
        if recipients.is_empty() {
            return Err(ApiError::invalid_request("redirect has no recipients").into());
        }
        let identity_id = self
            .rule_identity(message, &stored.summary.account_id, None)
            .await?;
        let blob = stored
            .blob_hash
            .ok_or_else(|| ApiError::internal("a downloaded message without a blob"))?;
        let mut draft = Draft::new(identity_id);
        draft.to = recipients;
        draft.subject = stored.summary.subject.clone();
        // The original bytes ride along as the draft's attachment: that
        // keeps them while the redirect waits in the outbox, even when a
        // later action deletes the message.
        draft.attachments = vec![DraftAttachment {
            filename: REDIRECT_FILENAME.to_owned(),
            mime: "message/rfc822".to_owned(),
            size: raw.len() as u64,
            blob: blob.as_str().to_owned(),
            content_id: None,
        }];
        self.send_now(
            draft,
            AutomaticMail::Redirect {
                message_id: message.clone(),
            },
        )
        .await
    }

    /// The bytes of a queued redirect: the original with `Resent-*` headers
    /// and the loop marker; the envelope goes to the draft's recipients only.
    pub(crate) async fn redirect_message(
        &self,
        draft: &Draft,
        identity: &Identity,
        message: &MessageId,
        seed: &[u8],
    ) -> Result<OutgoingMessage> {
        let original = draft.attachments.first().ok_or_else(|| {
            ApiError::internal(format!("the redirect of {message} lost its original"))
        })?;
        let hash = nail_store::BlobHash::parse(&original.blob)?;
        let blobs = self.inner.blobs.clone();
        let raw = tokio::task::spawn_blocking(move || blobs.get(&hash)).await??;
        let recipients: Vec<String> = draft.to.iter().map(|a| a.email.trim().to_owned()).collect();
        let message_id = nail_compose::new_message_id(&identity.email, seed);
        let date = chrono::Local::now().to_rfc2822();
        let raw = nail_compose::resent_message(
            &raw,
            &identity.email,
            &recipients,
            MessageMeta {
                date: &date,
                message_id: &message_id,
            },
        );
        Ok(OutgoingMessage {
            envelope: Envelope {
                from: identity.email.trim().to_owned(),
                to: recipients,
            },
            raw,
            message_id,
        })
    }
}

/// The variables of a reply template.
fn reply_variables(
    summary: &nail_api::MessageSummary,
    identity: &Identity,
) -> BTreeMap<String, String> {
    let from = summary.from.clone();
    BTreeMap::from([
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
        ("subject".to_owned(), summary.subject.clone()),
        ("date".to_owned(), summary.date.clone()),
        (
            "identity.name".to_owned(),
            identity.name.clone().unwrap_or_default(),
        ),
        ("identity.email".to_owned(), identity.email.clone()),
    ])
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
