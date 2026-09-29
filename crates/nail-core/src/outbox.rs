//! Drafts and the outbox: undo-send delay, send later, submission through
//! the account's SMTP server, the Sent copy and `SendStatus` events.
//!
//! `SendDraft` schedules the draft (`now + delay`, or `send_at`); until the
//! sender claims it, `CancelSend` turns it back into a draft (undo send).
//! The sender task wakes for the next due entry, builds the message
//! (plain text in M1a, `mime.rs`), submits it and records the outcome:
//!
//! * accepted: `sent`; the draft is deleted; a copy goes to the local Sent
//!   folder at once and, for IMAP accounts, to the server's Sent folder
//!   through the operation queue (`AppendMessage`, replayed like every
//!   other write, so it survives being offline) — unless
//!   `save-sent-copy=#false` or the provider files sent mail itself, like
//!   Gmail and Outlook.com; a reply marks the original answered through
//!   the operation queue;
//! * a temporary failure: back to `scheduled` with a growing delay (at
//!   most five attempts);
//! * a permanent failure: `failed` until the user retries or edits it.

use std::sync::Arc;
use std::time::Duration;

use nail_api::engine::{EngineError, OutgoingTransport};
use nail_api::{
    AccountConfig, ApiError, Draft, DraftId, Event, Flags, FolderRole, Identity, IncomingServer,
    OutboxEntry, OutboxId, OutboxState, OutgoingServer,
};
use nail_proto_smtp::{SendOptions, SmtpAuth, SmtpConfig, SmtpTransport};
use nail_store::{Operation, SendOutcome};

use crate::engine::unix_now;
use crate::error::Result;
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};
use crate::mime::{ReplyContext, build_plain_text, new_message_id};
use crate::service::Service;

/// Attempts before a temporarily failing send is given up.
pub(crate) const MAX_SEND_ATTEMPTS: u32 = 5;

/// Providers that file submitted mail in Sent themselves.
fn saves_sent_automatically(account: &AccountConfig) -> bool {
    let hosts = [
        Some(account.incoming.server().host.as_str()),
        account.outgoing.as_ref().map(|o| o.server().host.as_str()),
    ];
    hosts.into_iter().flatten().any(|host| {
        let host = host.to_ascii_lowercase();
        [
            "gmail.com",
            "googlemail.com",
            "outlook.com",
            "office365.com",
            "hotmail.com",
        ]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    })
}

/// Whether the Sent copy is uploaded by Nail.
pub(crate) fn wants_sent_copy(account: &AccountConfig) -> bool {
    account
        .save_sent_copy
        .unwrap_or_else(|| !saves_sent_automatically(account))
}

fn send_status(service: &Service, entry: &OutboxEntry) {
    service.broadcast(Event::SendStatus {
        outbox_id: entry.id.clone(),
        state: entry.state,
        error: entry.error,
        message: entry.message.clone(),
    });
}

impl Service {
    fn identity(&self, id: &nail_api::IdentityId) -> Result<(AccountConfig, Identity)> {
        let config = self.inner.accounts.config();
        let identity = config
            .identity(id)
            .cloned()
            .ok_or_else(|| ApiError::invalid_request(format!("unknown identity {id}")))?;
        let account = config
            .account(&identity.account_id)
            .cloned()
            .ok_or_else(|| {
                ApiError::invalid_request(format!("unknown account {}", identity.account_id))
            })?;
        Ok((account, identity))
    }

    /// `SaveDraft`.
    pub(crate) async fn save_draft(&self, draft: Draft) -> Result<DraftId> {
        if draft.uses_rich_content() {
            return Err(ApiError::unsupported(
                "HTML, Markdown and attachments arrive with the M1b composer",
            )
            .into());
        }
        self.identity(&draft.identity_id)?;
        Ok(self.inner.store.call(move |s| s.save_draft(&draft)).await?)
    }

    /// `SendDraft`.
    pub(crate) async fn send_draft(
        &self,
        draft_id: DraftId,
        delay_secs: Option<u32>,
        send_at: Option<String>,
    ) -> Result<OutboxEntry> {
        let draft = {
            let id = draft_id.clone();
            self.inner.store.call(move |s| s.get_draft(&id)).await?
        };
        let (account, _) = self.identity(&draft.identity_id)?;
        if account.outgoing.is_none() {
            return Err(ApiError::invalid_request(format!(
                "account {} has no outgoing server",
                account.id
            ))
            .into());
        }
        if draft.to.is_empty() && draft.cc.is_empty() && draft.bcc.is_empty() {
            return Err(ApiError::invalid_request("the message has no recipients").into());
        }
        let now = unix_now();
        let at = match send_at {
            Some(text) => chrono::DateTime::parse_from_rfc3339(&text)
                .map_err(|e| ApiError::invalid_request(format!("send_at {text:?}: {e}")))?
                .timestamp(),
            None => {
                let delay = delay_secs
                    .unwrap_or_else(|| self.inner.accounts.config().effective_undo_send_secs());
                now + i64::from(delay)
            }
        };
        let account_id = account.id.clone();
        let entry = self
            .inner
            .store
            .call(move |s| s.schedule_send(&draft_id, &account_id, at))
            .await?;
        send_status(self, &entry);
        self.inner.outbox_wake.notify_one();
        Ok(entry)
    }

    /// `CancelSend`.
    pub(crate) async fn cancel_send(&self, outbox_id: OutboxId) -> Result<DraftId> {
        let id = outbox_id.clone();
        let draft = self.inner.store.call(move |s| s.cancel_send(&id)).await?;
        self.broadcast(Event::SendStatus {
            outbox_id,
            state: OutboxState::Cancelled,
            error: None,
            message: None,
        });
        Ok(draft)
    }

    /// The transport of an account's outgoing server.
    fn transport(&self, account: &AccountConfig) -> Result<SmtpTransport> {
        let Some(OutgoingServer::Smtp { server, options }) = &account.outgoing else {
            return Err(ApiError::invalid_request(format!(
                "account {} has no outgoing server",
                account.id
            ))
            .into());
        };
        let auth = SmtpAuth::from_config(
            &account.id,
            &server.auth,
            Arc::clone(&self.inner.secrets),
            Arc::clone(&self.inner.tokens) as Arc<dyn nail_api::engine::AccessTokenProvider>,
        )?;
        let config = SmtpConfig::from_endpoint(server).with_timeout(crate::creds::NETWORK_TIMEOUT);
        Ok(SmtpTransport::new(config, auth, SendOptions::from(options)))
    }

    /// Submits one claimed entry and records the outcome.
    async fn send_entry(&self, entry: OutboxEntry) {
        let result = self.submit(&entry).await;
        let outcome = match &result {
            Ok(_) => SendOutcome::Sent,
            Err(err) if err.retryable && entry.attempts < MAX_SEND_ATTEMPTS => {
                let base = self.inner.timing.send_retry.as_secs().max(1);
                let factor = 1u64 << entry.attempts.saturating_sub(1).min(6);
                SendOutcome::Retry {
                    error: err.code,
                    message: err.message.clone(),
                    retry_at: unix_now() + i64::try_from((base * factor).min(3600)).unwrap_or(3600),
                }
            }
            Err(err) => SendOutcome::Failed {
                error: err.code,
                message: err.message.clone(),
            },
        };
        let id = entry.id.clone();
        let finished = self
            .inner
            .store
            .call(move |s| s.finish_send(&id, &outcome))
            .await;
        match finished {
            Ok(updated) => {
                tracing::info!(outbox = %updated.id, state = ?updated.state, "send finished");
                send_status(self, &updated);
                if updated.state == OutboxState::Scheduled {
                    // A retry: the sender must wake for its new time.
                    self.inner.outbox_wake.notify_one();
                }
            }
            Err(err) => tracing::error!(%err, "cannot record a send result"),
        }
        if let Ok(sent) = result {
            self.after_send(&entry, sent).await;
        }
    }

    /// Builds and submits a message; returns what the Sent copy needs.
    async fn submit(&self, entry: &OutboxEntry) -> Result<Sent, EngineError> {
        let draft = {
            let id = entry.draft_id.clone();
            self.inner
                .store
                .call(move |s| s.get_draft(&id))
                .await
                .map_err(|e| EngineError::from(crate::CoreError::from(e)))?
        };
        let (account, identity) = self
            .identity(&draft.identity_id)
            .map_err(EngineError::from)?;
        let reply = match draft.replied_message() {
            Some(parent) => {
                let parent = parent.clone();
                self.inner
                    .store
                    .call(move |s| s.message_meta(&parent))
                    .await
                    .ok()
                    .map(|meta| ReplyContext {
                        message_id: meta.message_id,
                        references: meta.references,
                    })
            }
            None => None,
        };
        let date = chrono::Local::now().to_rfc2822();
        let message_id = new_message_id(&identity.email, entry.id.as_str().as_bytes());
        let message = build_plain_text(&draft, &identity, reply.as_ref(), &date, &message_id);
        let transport = self.transport(&account).map_err(EngineError::from)?;
        transport.send(&message.envelope, &message.raw).await?;
        Ok(Sent {
            account,
            raw: message.raw,
            in_reply_to: draft.replied_message().cloned(),
        })
    }

    /// The Sent copy and the answered flag of a sent message.
    async fn after_send(&self, entry: &OutboxEntry, sent: Sent) {
        if let Some(parent) = sent.in_reply_to {
            let op = Operation::UpdateFlags {
                message_id: parent,
                change: nail_store::FlagChange {
                    answered: Some(true),
                    ..nail_store::FlagChange::default()
                },
            };
            if let Err(err) = self.enqueue(vec![op]).await {
                tracing::debug!(%err, "cannot mark the original answered");
            }
        }
        if !wants_sent_copy(&sent.account) {
            return;
        }
        if let Err(err) = self
            .file_sent_copy(&sent.account, &entry.id, &sent.raw)
            .await
        {
            tracing::warn!(account = %sent.account.id, %err, "cannot file the Sent copy");
        }
    }

    async fn file_sent_copy(
        &self,
        account: &AccountConfig,
        outbox: &OutboxId,
        raw: &[u8],
    ) -> Result<()> {
        let account_id = account.id.clone();
        let sent_folder = {
            let id = account_id.clone();
            self.inner
                .store
                .call(move |s| {
                    Ok(match s.folder_with_role(&id, FolderRole::Sent)? {
                        Some(f) => Some(s.get_folder(&f)?),
                        None => None,
                    })
                })
                .await?
        };
        let Some(folder) = sent_folder else {
            tracing::info!(account = %account_id, "no Sent folder; not keeping a copy");
            return Ok(());
        };
        // The copy is stored locally at once (so it survives being
        // offline); an IMAP account files it on the server through the
        // operation queue (`AppendMessage`), POP3 accounts keep it local.
        let id = ids::local_message_id(&folder.id, outbox.as_str());
        let incoming = IncomingMessage {
            id: Some(id.clone()),
            flags: Flags {
                seen: true,
                ..Flags::default()
            },
            raw: raw.to_vec(),
            ..IncomingMessage::default()
        };
        let blobs = self.inner.blobs.clone();
        let index = Arc::clone(&self.inner.search);
        let target = folder.clone();
        self.inner
            .store
            .call(move |s| ingest(s, &blobs, Some(&index), &target, 0, &[incoming]))
            .await?;
        match &account.incoming {
            IncomingServer::Imap { .. } => {
                let op = Operation::AppendMessage {
                    message_id: id,
                    folder_id: folder.id.clone(),
                };
                // Broadcasts the change and wakes the account's replay.
                self.enqueue(vec![op]).await?;
            }
            IncomingServer::Pop3 { .. } => {
                self.broadcast(Event::MailChanged {
                    folder_ids: vec![folder.id.clone()],
                });
            }
        }
        Ok(())
    }
}

/// A submitted message.
struct Sent {
    account: AccountConfig,
    raw: Vec<u8>,
    in_reply_to: Option<nail_api::MessageId>,
}

/// The sender: sends due entries, sleeps until the next one (or a wake-up).
pub(crate) async fn run_sender(service: Service) {
    loop {
        let now = unix_now();
        let claimed = service
            .inner
            .store
            .call(move |s| s.claim_due_sends(now))
            .await;
        match claimed {
            Ok(entries) => {
                for entry in entries {
                    send_status(&service, &entry);
                    let service = service.clone();
                    tokio::spawn(async move { service.send_entry(entry).await });
                }
            }
            Err(err) => tracing::error!(%err, "cannot read the outbox"),
        }
        let next = service
            .inner
            .store
            .call(|s| s.next_send_due())
            .await
            .ok()
            .flatten();
        let wait = match next {
            Some(at) => Duration::from_secs(u64::try_from((at - unix_now()).max(0)).unwrap_or(0))
                .max(Duration::from_millis(100))
                .min(Duration::from_secs(60)),
            None => Duration::from_secs(60),
        };
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = service.inner.outbox_wake.notified() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(kdl: &str) -> AccountConfig {
        nail_config::parse_accounts(kdl, "accounts.kdl")
            .unwrap()
            .accounts
            .remove(0)
    }

    #[test]
    fn sent_copies_skip_providers_that_file_them() {
        let gmail = account(
            "account \"g\" email=\"a@gmail.com\" { imap host=\"imap.gmail.com\"; smtp host=\"smtp.gmail.com\" }",
        );
        assert!(!wants_sent_copy(&gmail));
        let own = account(
            "account \"o\" email=\"a@own.example\" { imap host=\"imap.own.example\"; smtp host=\"smtp.own.example\" }",
        );
        assert!(wants_sent_copy(&own));
        let forced = account(
            "account \"f\" email=\"a@gmail.com\" save-sent-copy=#true { imap host=\"imap.gmail.com\" }",
        );
        assert!(wants_sent_copy(&forced));
    }
}
