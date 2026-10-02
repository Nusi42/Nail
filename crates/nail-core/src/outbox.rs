//! Drafts and the outbox: undo-send delay, send later, submission through
//! the account's SMTP server, the Sent copy and `SendStatus` events.
//!
//! `SendDraft` schedules the draft (`now + delay`, or `send_at`); until the
//! sender claims it, `CancelSend` turns it back into a draft (undo send).
//! The sender task wakes for the next due entry, builds the message with
//! `nail-compose` (plain, HTML or Markdown, attachments; ADR 0023), submits
//! it and records the outcome:
//!
//! * accepted: `sent`; the draft is deleted; a copy goes to the local Sent
//!   folder at once and, for IMAP accounts, to the server's Sent folder
//!   through the operation queue (`AppendMessage`, replayed like every
//!   other write, so it survives being offline) — unless
//!   `save-sent-copy=#false` or the provider files sent mail itself, like
//!   Gmail and Outlook.com; a reply marks the original answered through
//!   the operation queue; the draft's autosave copy in the server's Drafts
//!   folder is deleted, the recipients are recorded for autocomplete and
//!   the on-send rules run on the Sent copy (not for mail a rule sent);
//! * a temporary failure: back to `scheduled` with a growing delay (at
//!   most five attempts);
//! * a permanent failure: `failed` until the user retries or edits it.

use std::sync::Arc;
use std::time::Duration;

use nail_api::engine::{EngineError, OutgoingTransport};
use nail_api::{
    AccountConfig, ApiError, AutomaticMail, Draft, DraftId, Event, Flags, FolderRole, Identity,
    IncomingServer, OutboxEntry, OutboxId, OutboxState, OutgoingServer, SendCrypto,
};
use nail_proto_smtp::{SendOptions, SmtpAuth, SmtpConfig, SmtpTransport};
use nail_store::{Operation, SendOutcome};

use crate::engine::unix_now;
use crate::error::Result;
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};
use crate::service::Service;

/// Attempts before a temporarily failing send is given up.
pub(crate) const MAX_SEND_ATTEMPTS: u32 = 5;

/// Providers that file submitted mail in Sent themselves (and API
/// accounts that send through their API, like Gmail; M3b).
fn saves_sent_automatically(account: &AccountConfig) -> bool {
    if account.incoming.sends_itself() {
        return true;
    }
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
        localized: None,
    });
}

impl Service {
    pub(crate) fn identity(&self, id: &nail_api::IdentityId) -> Result<(AccountConfig, Identity)> {
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

    /// `SaveDraft`: HTML is sanitized before it is stored; attachments must
    /// be uploaded blobs. Schedules the autosave copy in the server's Drafts
    /// folder.
    pub(crate) async fn save_draft(&self, draft: Draft) -> Result<DraftId> {
        self.store_draft(draft, true).await
    }

    /// Stores a draft; `upload`: schedule the copy in the server's Drafts
    /// folder (not for rule mail, which is sent at once).
    pub(crate) async fn store_draft(&self, draft: Draft, upload: bool) -> Result<DraftId> {
        let draft = self.checked_draft(draft)?;
        let id = self.inner.store.call(move |s| s.save_draft(&draft)).await?;
        if upload {
            self.schedule_draft_upload(&id);
        }
        Ok(id)
    }

    /// [`Service::store_draft`] (no upload) for a stored draft still at
    /// revision `expected` (check and write in one store transaction);
    /// returns the new revision (ADR 0086).
    pub(crate) async fn store_draft_at(&self, draft: Draft, expected: String) -> Result<String> {
        let draft = self.checked_draft(draft)?;
        Ok(self
            .inner
            .store
            .call(move |s| s.save_draft_at(&draft, &expected))
            .await?)
    }

    /// A draft ready to store: its identity exists, its HTML is sanitized
    /// and its attachments are uploaded blobs.
    fn checked_draft(&self, mut draft: Draft) -> Result<Draft> {
        self.identity(&draft.identity_id)?;
        if let Some(html) = &draft.html {
            draft.html = Some(nail_compose::sanitize_outgoing(html));
        }
        for attachment in &draft.attachments {
            let known = nail_store::BlobHash::parse(&attachment.blob)
                .is_ok_and(|hash| self.inner.blobs.contains(&hash));
            if !known {
                return Err(ApiError::not_found(format!(
                    "attachment {:?}: upload it first (unknown blob {})",
                    attachment.filename, attachment.blob
                ))
                .into());
            }
        }
        Ok(draft)
    }

    /// `GetDraftAttachment`: the content of a blob the draft lists in its
    /// attachments (never any other blob, so a draft id cannot be used to
    /// read arbitrary stored mail).
    pub(crate) async fn draft_attachment(
        &self,
        draft_id: DraftId,
        blob: String,
    ) -> Result<nail_api::Response> {
        use base64::Engine as _;
        let draft = self
            .inner
            .store
            .call(move |s| s.get_draft(&draft_id))
            .await?;
        let attachment = draft
            .attachments
            .iter()
            .find(|a| a.blob == blob)
            .ok_or_else(|| ApiError::not_found(format!("the draft has no attachment {blob}")))?;
        let hash = nail_store::BlobHash::parse(&attachment.blob)?;
        let bytes = self.inner.blobs.get(&hash)?;
        Ok(nail_api::Response::AttachmentData {
            mime: attachment.mime.clone(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }

    /// `ListDrafts`: saved drafts that are not waiting in the outbox.
    pub(crate) async fn list_drafts(&self) -> Result<Vec<Draft>> {
        Ok(self
            .inner
            .store
            .call(|s| {
                let queued: Vec<DraftId> =
                    s.list_outbox()?.into_iter().map(|e| e.draft_id).collect();
                Ok(s.list_drafts()?
                    .into_iter()
                    .filter(|d| d.id.as_ref().is_none_or(|id| !queued.contains(id)))
                    .collect())
            })
            .await?)
    }

    /// `DeleteDraft`: the draft and its server copy.
    pub(crate) async fn delete_draft(&self, draft_id: &DraftId) -> Result<()> {
        // Waits for a running upload, so its copy is the one deleted here.
        let _upload = self.cancel_draft_upload(draft_id).await;
        let id = draft_id.clone();
        let copy = self
            .inner
            .store
            .call(move |s| s.draft_server_copy(&id))
            .await
            .ok()
            .flatten();
        let id = draft_id.clone();
        if !self.inner.store.call(move |s| s.delete_draft(&id)).await? {
            return Err(ApiError::not_found(format!("draft {draft_id}")).into());
        }
        self.remove_draft_copy(copy).await;
        Ok(())
    }

    /// `SendDraft`.
    pub(crate) async fn send_draft(
        &self,
        draft_id: DraftId,
        delay_secs: Option<u32>,
        send_at: Option<String>,
        crypto: Option<SendCrypto>,
    ) -> Result<OutboxEntry> {
        self.send_draft_at(draft_id, delay_secs, send_at, crypto, None)
            .await
    }

    /// `SendDraft` of the draft at revision `expected` only (`None` = as
    /// it is): the crypto choice written into the draft and the outbox
    /// entry each check the revision in their store transaction, so a
    /// draft edited after a confirmation's approval is never queued
    /// (`DraftChanged`; ADR 0086).
    pub(crate) async fn send_draft_at(
        &self,
        draft_id: DraftId,
        delay_secs: Option<u32>,
        send_at: Option<String>,
        crypto: Option<SendCrypto>,
        expected: Option<String>,
    ) -> Result<OutboxEntry> {
        let mut expected = expected;
        let draft = {
            let id = draft_id.clone();
            self.inner.store.call(move |s| s.get_draft(&id)).await?
        };
        if expected
            .as_deref()
            .is_some_and(|rev| nail_store::draft_revision(&draft) != rev)
        {
            return Err(nail_store::Error::DraftChanged(draft_id.to_string()).into());
        }
        let (account, identity) = self.identity(&draft.identity_id)?;
        // Sign / encrypt: the request's choice, else the draft's, else the
        // identity's defaults (ADR 0029). Checked now (`no_key`, `locked`)
        // and applied when the message is sent; never sent plain instead.
        let mut crypto = crate::crypto::effective_crypto(crypto, draft.crypto.as_ref(), &identity);
        if crypto.is_active() {
            let (checked_identity, checked_draft, checked_crypto) =
                (identity.clone(), draft.clone(), crypto.clone());
            crypto.technology = Some(
                self.blocking(move |s| {
                    crate::crypto::check_send(s, &checked_identity, &checked_draft, &checked_crypto)
                })
                .await?,
            );
        }
        if draft.crypto.as_ref() != Some(&crypto) {
            // The sender applies what was checked here.
            let mut updated = draft.clone();
            updated.crypto = Some(crypto.clone());
            match expected {
                Some(revision) => {
                    expected = Some(
                        self.inner
                            .store
                            .call(move |s| s.save_draft_at(&updated, &revision))
                            .await?,
                    );
                }
                None => {
                    self.inner
                        .store
                        .call(move |s| s.save_draft(&updated))
                        .await?;
                }
            }
        }
        if !account.can_send() {
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
        // The draft is about to go; an upload now would only leave a copy
        // behind. A running upload finishes first, so the sender deletes
        // the copy it made.
        let _upload = self.cancel_draft_upload(&draft_id).await;
        let entry = self
            .inner
            .store
            .call(move |s| s.schedule_send_at(&draft_id, &account_id, at, expected.as_deref()))
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
            localized: None,
        });
        Ok(draft)
    }

    /// The transport of an account's outgoing server: SMTP, the Gmail API
    /// for Gmail accounts (M3b; `messages.send`), or the Graph / EWS
    /// engine for Exchange accounts (M3c; the server files the Sent copy).
    pub(crate) async fn transport(
        &self,
        account: &AccountConfig,
    ) -> Result<Arc<dyn OutgoingTransport>> {
        if let IncomingServer::Gmail { .. } = &account.incoming {
            return Ok(Arc::new(crate::gmail::engine_for(self, account)?));
        }
        if let IncomingServer::Exchange { .. } = &account.incoming {
            let engine = crate::exchange::engine_of(self, account).await?;
            return Ok(engine as Arc<dyn OutgoingTransport>);
        }
        Ok(Arc::new(self.smtp_transport(account)?))
    }

    /// The SMTP transport of an account's outgoing server.
    fn smtp_transport(&self, account: &AccountConfig) -> Result<SmtpTransport> {
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
            self.forget_plugin_headers(&entry.draft_id);
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
        let seed = entry.id.as_str().as_bytes();
        let message = match &draft.automatic {
            Some(AutomaticMail::Redirect { message_id }) => {
                self.redirect_message(&draft, &identity, message_id, seed)
                    .await
            }
            Some(AutomaticMail::AutocryptSetup | AutomaticMail::Itip) => {
                self.prepared_message(&draft, &identity).await
            }
            // Headers the plug-ins' before-send hooks added (M4a).
            _ => self
                .build_draft_message(&draft, &identity, seed)
                .await
                .map(|mut message| {
                    let headers = self.plugin_headers(&entry.draft_id);
                    if !headers.is_empty() {
                        message.raw = crate::agents::add_headers(&message.raw, &headers);
                    }
                    message
                }),
        }
        .map_err(EngineError::from)?;
        // The Autocrypt header, signing and encryption (M2a). Redirects,
        // setup messages and iMIP mail (M3b) go out unchanged.
        let messages = if draft.automatic.as_ref().is_some_and(|a| {
            matches!(
                a,
                AutomaticMail::Redirect { .. }
                    | AutomaticMail::AutocryptSetup
                    | AutomaticMail::Itip
            )
        }) {
            vec![message]
        } else {
            let crypto = draft.crypto.clone().unwrap_or_else(SendCrypto::none);
            let (secure_identity, secure_draft) = (identity.clone(), draft.clone());
            self.blocking(move |s| {
                crate::crypto::secure_outgoing(s, &secure_identity, &secure_draft, message, &crypto)
            })
            .await
            .map_err(EngineError::from)?
        };
        let server_copy = {
            let id = entry.draft_id.clone();
            self.inner
                .store
                .call(move |s| s.draft_server_copy(&id))
                .await
                .ok()
                .flatten()
        };
        let transport = self.transport(&account).await.map_err(EngineError::from)?;
        // M4c: upload progress (Graph upload sessions for large messages)
        // as `SendProgress`, at most a few per second.
        let events = self.inner.events.clone();
        let outbox_id = entry.id.clone();
        let last = std::sync::Mutex::new(None::<std::time::Instant>);
        let progress = move |sent_bytes: u64, total_bytes: u64| {
            let mut last = crate::service::lock(&last);
            let due = sent_bytes >= total_bytes
                || last.is_none_or(|at| at.elapsed() >= Duration::from_millis(250));
            if due {
                *last = Some(std::time::Instant::now());
                let _ = events.send(Event::SendProgress {
                    outbox_id: outbox_id.clone(),
                    sent_bytes,
                    total_bytes,
                });
            }
        };
        for message in &messages {
            transport
                .send_with_progress(&message.envelope, &message.raw, &progress)
                .await?;
        }
        // The first message is the one to the visible recipients (for an
        // encrypted message also encrypted to the sender): the Sent copy.
        let raw = messages
            .into_iter()
            .next()
            .map(|m| m.raw)
            .unwrap_or_default();
        Ok(Sent {
            account,
            raw,
            in_reply_to: draft.replied_message().cloned(),
            server_copy,
            draft,
        })
    }

    /// The Sent copy, the answered flag, the draft's server copy, collected
    /// addresses and on-send rules of a sent message. On-send rules do not
    /// run for mail a rule sent (a forward on send would forward its own
    /// forward without end).
    async fn after_send(&self, entry: &OutboxEntry, sent: Sent) {
        self.remove_draft_copy(sent.server_copy.clone()).await;
        self.record_recipients(&sent.draft).await;
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
        let mut copies = Vec::new();
        if wants_sent_copy(&sent.account) {
            match self
                .file_sent_copy(&sent.account, &entry.id, &sent.raw)
                .await
            {
                Ok(Some(copy)) if sent.draft.automatic.is_none() => {
                    self.run_on_send_rules(&copy).await;
                    copies.push(copy);
                }
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(account = %sent.account.id, %err, "cannot file the Sent copy");
                }
            }
        }
        // Plug-in jobs subscribed to `message.sent` (M4a): the Sent copy's
        // id when Nail filed one.
        if sent.draft.automatic.is_none()
            && let Some(host) = self.inner.plugins.existing_host()
        {
            host.deliver_event(
                nail_api::plugins::jobs::PluginEventName::MessageSent,
                copies,
            );
        }
    }

    async fn file_sent_copy(
        &self,
        account: &AccountConfig,
        outbox: &OutboxId,
        raw: &[u8],
    ) -> Result<Option<nail_api::MessageId>> {
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
            return Ok(None);
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
        let index = self.inner.search.get();
        let target = folder.clone();
        self.inner
            .store
            .call(move |s| ingest(s, &blobs, Some(&index), &target, 0, &[incoming]))
            .await?;
        match &account.incoming {
            IncomingServer::Imap { .. } => {
                let op = Operation::AppendMessage {
                    message_id: id.clone(),
                    folder_id: folder.id.clone(),
                };
                // Broadcasts the change and wakes the account's replay.
                self.enqueue(vec![op]).await?;
            }
            // POP3 has no server folder; Gmail files sent mail itself
            // (`messages.send` labels it SENT; M3b), so does Exchange
            // (Sent Items; M3c).
            IncomingServer::Pop3 { .. }
            | IncomingServer::Gmail { .. }
            | IncomingServer::Exchange { .. } => {
                self.broadcast(Event::MailChanged {
                    folder_ids: vec![folder.id.clone()],
                });
            }
        }
        Ok(Some(id))
    }
}

/// A submitted message.
struct Sent {
    account: AccountConfig,
    raw: Vec<u8>,
    in_reply_to: Option<nail_api::MessageId>,
    /// The draft's autosave copy on the server, to delete.
    server_copy: Option<nail_store::DraftServerCopy>,
    /// The draft as sent.
    draft: Draft,
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
