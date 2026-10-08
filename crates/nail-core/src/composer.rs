//! Requests of M1b WP21 (composer, ADR 0023): `NewDraft`, `RenderPreview`,
//! `ConvertDraft`, `UploadAttachment`, `SuggestAddresses` — plus what the rest of
//! the core needs from the composer: the identity's style and settings,
//! building a draft's message ([`Service::build_draft_message`]) and the
//! autosave copy in the server's Drafts folder.
//!
//! The work is done by `nail-compose`, a pure library; this module feeds
//! it from the store (original messages, attachment blobs) and the Fluent
//! catalogs (quote header, forward texts).

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use nail_api::accounts::{ComposerMode, QuotePosting};
use nail_api::{
    AccountId, Address, ApiError, Draft, DraftAttachment, DraftContext, DraftContextMode, DraftId,
    ErrorCode, Flags, FolderRole, Identity, IdentityId, IncomingServer, MessageId, Request,
    Response,
};
use nail_compose::{
    ComposeError, ComposeSettings, MailStyle, MessageMeta, OriginalMessage, OutgoingMessage,
    Posting, QuoteStrings, ReplyHeaders,
};
use nail_store::{DraftServerCopy, Operation};
use tokio::task::JoinHandle;

use crate::error::Result;
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};
use crate::service::{Service, lock};
use crate::strings::{message, message_with};

/// Largest attachment `UploadAttachment` accepts (decoded bytes, 25 MiB).
pub(crate) const MAX_ATTACHMENT_BYTES: u64 = nail_api::compose::MAX_ATTACHMENT_BYTES;

/// Longest address suggestion list.
const MAX_SUGGESTIONS: u32 = 50;

/// Uploads of drafts to the server's Drafts folder (autosave), by draft.
///
/// An upload first waits for the autosave delay; a newer save replaces a
/// waiting upload and sending or deleting the draft cancels it. Once an
/// upload has *started* it is never aborted: its store writes and the
/// operations it queues belong together (aborting between them left stale
/// or duplicate copies on the server). It runs under the draft's lock, and
/// whoever reads the draft's server copy afterwards (the next upload,
/// `SendDraft`, `DeleteDraft`) takes the same lock first
/// ([`DraftUploads::cancel`]), so it sees the finished result.
#[derive(Default, Clone)]
pub(crate) struct DraftUploads(Arc<UploadsInner>);

#[derive(Default)]
struct UploadsInner {
    waiting: Mutex<HashMap<DraftId, Waiting>>,
    locks: Mutex<HashMap<DraftId, Arc<tokio::sync::Mutex<()>>>>,
    generation: AtomicU64,
}

/// An upload waiting for its delay.
struct Waiting {
    generation: u64,
    task: JoinHandle<()>,
}

impl DraftUploads {
    fn draft_lock(&self, id: &DraftId) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(lock(&self.0.locks).entry(id.clone()).or_default())
    }

    /// Runs `upload` for draft `id` after `delay`, replacing an upload of
    /// the same draft that is still waiting.
    fn schedule<F>(&self, id: &DraftId, delay: Duration, upload: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let generation = self.0.generation.fetch_add(1, Ordering::Relaxed);
        let this = self.clone();
        let draft = id.clone();
        // Held while spawning, so the task cannot claim itself before it
        // is registered.
        let mut waiting = lock(&self.0.waiting);
        let task = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            // Claim the upload: from here on nothing aborts it.
            {
                let mut waiting = lock(&this.0.waiting);
                if waiting.get(&draft).map(|w| w.generation) != Some(generation) {
                    return;
                }
                waiting.remove(&draft);
            }
            let draft_lock = this.draft_lock(&draft);
            let _guard = draft_lock.lock().await;
            upload.await;
        });
        if let Some(old) = waiting.insert(id.clone(), Waiting { generation, task }) {
            old.task.abort();
        }
    }

    /// Cancels a waiting upload and waits for a running one to finish. The
    /// returned guard keeps later uploads of the draft out until the caller
    /// is done with the draft.
    pub(crate) async fn cancel(&self, id: &DraftId) -> tokio::sync::OwnedMutexGuard<()> {
        let waiting = lock(&self.0.waiting).remove(id);
        if let Some(old) = waiting {
            old.task.abort();
        }
        self.draft_lock(id).lock_owned().await
    }
}

/// The mail style of an identity (defaults when it sets none).
pub(crate) fn style_of(identity: &Identity) -> MailStyle {
    identity
        .mail_style
        .as_ref()
        .map(|s| MailStyle {
            css: s.css.clone(),
            code_theme: s.code_theme.clone(),
        })
        .unwrap_or_default()
}

/// Composer settings of an identity.
pub(crate) fn settings_of(identity: &Identity) -> ComposeSettings {
    ComposeSettings {
        style: style_of(identity),
        posting: match identity.posting {
            Some(QuotePosting::Top) => Posting::Top,
            Some(QuotePosting::Bottom) | None => Posting::Bottom,
        },
    }
}

fn compose_error(err: &ComposeError) -> ApiError {
    match err {
        ComposeError::MissingBlob { .. } => ApiError::not_found(err.to_string()),
        ComposeError::NoSender | ComposeError::Stylesheet(_) => {
            ApiError::invalid_request(err.to_string())
        }
        ComposeError::Io(_) => ApiError::internal(err.to_string()),
    }
}

/// A date for the quote header: the original's date in local time.
fn quote_date(rfc3339: Option<&str>) -> String {
    rfc3339
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// Localized quote header and forward texts for `original`.
pub(crate) fn quote_strings(
    locale: Option<&str>,
    original: &OriginalMessage,
    date: Option<&str>,
) -> QuoteStrings {
    let sender = original
        .from
        .first()
        .map(|a| {
            a.name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| a.email.clone())
        })
        .unwrap_or_default();
    let date = quote_date(date);
    let mut fields = vec![
        (
            message(locale, "message-header-from"),
            Address::list_text(&original.from),
        ),
        (message(locale, "message-header-date"), date.clone()),
        (
            message(locale, "composer-field-subject"),
            original.subject.clone(),
        ),
        (
            message(locale, "message-header-to"),
            Address::list_text(&original.to),
        ),
    ];
    if !original.cc.is_empty() {
        fields.push((
            message(locale, "message-header-cc"),
            Address::list_text(&original.cc),
        ));
    }
    QuoteStrings {
        reply_header: message_with(
            locale,
            "composer-quote-header",
            &[("date", date.as_str()), ("name", sender.as_str())],
        ),
        forward_intro: message(locale, "composer-forward-header"),
        forward_fields: fields,
    }
}

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::NewDraft {
            identity_id,
            account_id,
            mode,
            context,
        } => Response::Draft {
            draft: Box::new(
                service
                    .new_draft(identity_id, account_id.as_ref(), mode, context)
                    .await?,
            ),
        },
        Request::RenderPreview {
            markdown,
            identity_id,
        } => {
            let style = service.style_for(identity_id.as_ref());
            let html = tokio::task::spawn_blocking(move || {
                nail_compose::render_markdown_document(&markdown, &style)
            })
            .await?
            .map_err(|e| compose_error(&e))?;
            Response::PreviewRendered { html }
        }
        Request::ConvertDraft { draft, mode } => {
            let style = service.style_for(Some(&draft.identity_id));
            let converted = tokio::task::spawn_blocking(move || {
                nail_compose::convert_draft(&draft, mode, &style)
            })
            .await?;
            Response::DraftConverted {
                lossy: converted.lossy(),
                draft: Box::new(converted.draft),
            }
        }
        Request::UploadAttachment {
            filename,
            mime,
            data,
            content_id,
        } => Response::AttachmentUploaded {
            attachment: service
                .upload_attachment(filename, mime, &data, content_id)
                .await?,
        },
        Request::SuggestAddresses { prefix, limit } => {
            // Contacts (books with autocomplete on), the Collected book and
            // addresses used recently, ranked (M3a, ADR 0061).
            let limit = limit.clamp(1, MAX_SUGGESTIONS);
            let now = service.pim_now();
            Response::AddressSuggestions {
                addresses: service
                    .inner
                    .store
                    .call(move |s| s.suggest_recipients(&prefix, limit, now))
                    .await?
                    .into_iter()
                    .map(|r| r.address)
                    .collect(),
            }
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} routed to the composer module",
                other.wire_type()
            ))
            .into());
        }
    })
}

impl Service {
    /// The style of an identity, or the default for an unknown / absent one.
    pub(crate) fn style_for(&self, identity: Option<&IdentityId>) -> MailStyle {
        identity
            .and_then(|id| self.inner.accounts.config().identity(id).map(style_of))
            .unwrap_or_default()
    }

    /// Every address of the user's identities (left out of reply-all).
    pub(crate) fn own_addresses(&self) -> Vec<String> {
        self.inner
            .accounts
            .config()
            .accounts
            .iter()
            .flat_map(|a| {
                std::iter::once(a.email.clone()).chain(a.identities.iter().map(|i| i.email.clone()))
            })
            .collect()
    }

    /// `NewDraft`: an unsaved draft with identity, signature and, for a
    /// reply or forward, recipients, subject, quote and attachments.
    pub(crate) async fn new_draft(
        &self,
        identity_id: Option<IdentityId>,
        account_id: Option<&AccountId>,
        mode: Option<ComposerMode>,
        context: Option<DraftContext>,
    ) -> Result<Draft> {
        let identity_id = match identity_id {
            Some(id) => id,
            None => self
                .choose_identity(
                    account_id,
                    &[],
                    context.as_ref().map(|c| c.message_id.clone()),
                )
                .await?
                .ok_or_else(|| ApiError::invalid_request("there is no identity to send from"))?,
        };
        let (_, identity) = self.identity(&identity_id)?;
        let settings = settings_of(&identity);
        let own = self.own_addresses();
        let original = match &context {
            Some(context) => Some(
                self.original_message(&context.message_id, context.mode)
                    .await?,
            ),
            None => None,
        };
        // Answering or forwarding encrypted mail starts encrypted (M2a).
        let reply_crypto = original
            .as_ref()
            .and_then(|(_, _, security)| security.as_ref())
            .filter(|s| s.is_encrypted())
            .map(|security| nail_api::security::SendCrypto {
                sign: identity.crypto.as_ref().is_some_and(|c| c.sign),
                encrypt: true,
                technology: Some(security.technology),
                protect_headers: None,
            });
        let original = original.map(|(message, date, _)| (message, date));
        let locale = self.inner.locale.clone();
        let draft = tokio::task::spawn_blocking(move || {
            let with_context = match (&context, &original) {
                (Some(context), Some((original, date))) => Some((
                    context.mode,
                    original,
                    quote_strings(locale.as_deref(), original, date.as_deref()),
                )),
                _ => None,
            };
            nail_compose::new_draft(
                &identity,
                mode,
                &settings,
                with_context.as_ref().map(|(m, o, s)| (*m, *o, s)),
                &own,
            )
        })
        .await?;
        let mut draft = draft;
        if reply_crypto.is_some() {
            draft.crypto = reply_crypto;
        }
        Ok(draft)
    }

    /// The message a reply or forward refers to, with its date and its
    /// end-to-end status; a forward gets the original's attachments
    /// (decrypted ones for encrypted mail) copied into the blob store.
    async fn original_message(
        &self,
        id: &MessageId,
        mode: DraftContextMode,
    ) -> Result<(
        OriginalMessage,
        Option<String>,
        Option<nail_api::security::SecurityStatus>,
    )> {
        let body = self.message_body(id.clone()).await?;
        let attachments = if mode == DraftContextMode::Forward && !body.attachments.is_empty() {
            let service = self.clone();
            let id = id.clone();
            tokio::task::spawn_blocking(move || -> Result<Vec<DraftAttachment>> {
                let blobs = service.blobs().clone();
                let stored = service.store().get_message(&id)?;
                let Some(hash) = stored.blob_hash else {
                    return Ok(Vec::new());
                };
                let (raw, _) = service.readable_raw(blobs.get(&hash)?);
                let mut out = Vec::new();
                for index in 0.. {
                    let Some(content) = nail_mime::attachment_content(&raw, index) else {
                        break;
                    };
                    let blob = blobs.put(&content.data)?;
                    out.push(DraftAttachment {
                        filename: content
                            .info
                            .filename
                            .clone()
                            .unwrap_or_else(|| format!("attachment-{}", index + 1)),
                        mime: content.info.mime.clone(),
                        size: content.data.len() as u64,
                        blob: blob.as_str().to_owned(),
                        content_id: None,
                    });
                }
                Ok(out)
            })
            .await??
        } else {
            Vec::new()
        };
        let headers = body.headers;
        let security = body.security;
        Ok((
            OriginalMessage {
                id: id.clone(),
                from: headers.from,
                to: headers.to,
                cc: headers.cc,
                reply_to: headers.reply_to,
                subject: headers.subject,
                text: body.text,
                html: body.sanitized_html,
                attachments,
            },
            headers.date,
            security,
        ))
    }

    /// `UploadAttachment`: stores the bytes as a blob.
    async fn upload_attachment(
        &self,
        filename: String,
        mime: String,
        data: &str,
        content_id: Option<String>,
    ) -> Result<DraftAttachment> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .map_err(|e| {
                ApiError::invalid_request(format!("attachment data is not base64: {e}"))
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_ATTACHMENT_BYTES {
            return Err(ApiError::new(
                ErrorCode::TooLarge,
                format!(
                    "{filename} is {} bytes; attachments may be at most {MAX_ATTACHMENT_BYTES} bytes",
                    bytes.len()
                ),
            )
            .into());
        }
        let filename = filename.trim().to_owned();
        if filename.is_empty() {
            return Err(ApiError::invalid_request("the attachment has no file name").into());
        }
        let size = bytes.len() as u64;
        let blobs = self.inner.blobs.clone();
        let hash = tokio::task::spawn_blocking(move || blobs.put(&bytes)).await??;
        let mime = if mime.trim().is_empty() {
            "application/octet-stream".to_owned()
        } else {
            mime.trim().to_ascii_lowercase()
        };
        Ok(DraftAttachment {
            filename,
            mime,
            size,
            blob: hash.as_str().to_owned(),
            content_id: content_id.filter(|c| !c.trim().is_empty()),
        })
    }

    /// Builds the message of a draft sent (or saved) as `identity`:
    /// threading headers from the replied message, the identity's style,
    /// attachment bytes from the blob store.
    pub(crate) async fn build_draft_message(
        &self,
        draft: &Draft,
        identity: &Identity,
        seed: &[u8],
    ) -> Result<OutgoingMessage> {
        let reply = match draft.replied_message() {
            Some(parent) => {
                let parent = parent.clone();
                self.inner
                    .store
                    .call(move |s| s.message_meta(&parent))
                    .await
                    .ok()
                    .map(|meta| ReplyHeaders {
                        message_id: meta.message_id,
                        references: meta.references,
                    })
            }
            None => None,
        };
        let date = chrono::Local::now().to_rfc2822();
        let message_id = nail_compose::new_message_id(&identity.email, seed);
        let style = style_of(identity);
        let mut draft = draft.clone();
        // Plug-in Markdown blocks are rendered by their plug-ins at send
        // time (M4a).
        if draft.mode == ComposerMode::Markdown
            && let Some(markdown) = &draft.markdown
        {
            draft.markdown = Some(self.render_plugin_blocks(markdown).await);
        }
        let identity = identity.clone();
        let blobs = self.inner.blobs.clone();
        let built = tokio::task::spawn_blocking(move || {
            // Attachment bytes, read before building (the builder does no
            // I/O).
            let mut bytes: HashMap<String, Vec<u8>> = HashMap::new();
            for attachment in &draft.attachments {
                if let Ok(hash) = nail_store::BlobHash::parse(&attachment.blob)
                    && let Ok(data) = blobs.get(&hash)
                {
                    bytes.insert(attachment.blob.clone(), data);
                }
            }
            let source = |hash: &str| bytes.get(hash).cloned();
            nail_compose::build_message(
                &draft,
                &identity,
                reply.as_ref(),
                MessageMeta {
                    date: &date,
                    message_id: &message_id,
                },
                &style,
                &source,
            )
        })
        .await?
        .map_err(|e| compose_error(&e))?;
        Ok(built)
    }

    /// Schedules the upload of a saved draft to the account's Drafts folder
    /// on the server (autosave), replacing an earlier upload of the same
    /// draft that has not started yet ([`DraftUploads`]).
    pub(crate) fn schedule_draft_upload(&self, id: &DraftId) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let service = self.clone();
        let draft_id = id.clone();
        self.inner
            .draft_uploads
            .schedule(id, self.inner.timing.draft_upload, async move {
                if let Err(err) = service.upload_draft_copy(&draft_id).await {
                    tracing::warn!(draft = %draft_id, %err, "cannot upload the draft copy");
                }
            });
    }

    /// Cancels a waiting upload and waits for a running one (the draft is
    /// being sent or deleted); hold the guard while reading or changing the
    /// draft's server copy.
    pub(crate) async fn cancel_draft_upload(
        &self,
        id: &DraftId,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.inner.draft_uploads.cancel(id).await
    }

    /// The local id of a draft's server copy in the Drafts folder.
    fn draft_copy_id(folder: &nail_api::FolderId, message_id: &str) -> MessageId {
        ids::local_message_id(folder, &format!("draft-copy:{message_id}"))
    }

    /// Uploads the current text of a draft to the Drafts folder of its
    /// (IMAP) account: stored locally at once, filed on the server through
    /// the operation queue (`AppendMessage`); the previous copy is deleted.
    async fn upload_draft_copy(&self, id: &DraftId) -> Result<()> {
        let draft = {
            let id = id.clone();
            match self.inner.store.call(move |s| s.get_draft(&id)).await {
                Ok(draft) => draft,
                // Deleted or sent meanwhile.
                Err(_) => return Ok(()),
            }
        };
        let (account, identity) = self.identity(&draft.identity_id)?;
        if !matches!(account.incoming, IncomingServer::Imap { .. }) {
            return Ok(());
        }
        // An encrypted message's draft never goes to the server in plain
        // text (M2a): decided like sending decides, and an earlier plain
        // copy (from before encryption was turned on) is deleted.
        if self
            .draft_goes_encrypted(&draft, &account.id, &identity)
            .await
        {
            let draft_id = id.clone();
            let queued = self
                .inner
                .store
                .call(move |s| {
                    let Some(previous) = s.draft_server_copy(&draft_id)? else {
                        return Ok(Vec::new());
                    };
                    s.set_draft_server_copy(&draft_id, None)?;
                    match draft_copy_removal_in(s, &previous) {
                        Some(op) => s.enqueue_all(&[op]),
                        None => Ok(Vec::new()),
                    }
                })
                .await?;
            self.announce_enqueued(&queued);
            return Ok(());
        }
        let folder = {
            let account_id = account.id.clone();
            self.inner
                .store
                .call(move |s| {
                    Ok(match s.folder_with_role(&account_id, FolderRole::Drafts)? {
                        Some(f) => Some(s.get_folder(&f)?),
                        None => None,
                    })
                })
                .await?
        };
        let Some(folder) = folder else {
            tracing::debug!(account = %account.id, "no Drafts folder; draft stays local");
            return Ok(());
        };
        let built = self
            .build_draft_message(&draft, &identity, id.as_str().as_bytes())
            .await?;
        let copy_id = Self::draft_copy_id(&folder.id, &built.message_id);
        let incoming = IncomingMessage {
            id: Some(copy_id.clone()),
            flags: Flags {
                seen: true,
                draft: true,
                ..Flags::default()
            },
            raw: built.raw.clone(),
            ..IncomingMessage::default()
        };
        let blobs = self.inner.blobs.clone();
        let index = self.inner.search.get();
        let target = folder.clone();
        let draft_id = id.clone();
        let copy = DraftServerCopy {
            folder_id: folder.id.clone(),
            uid: None,
            message_id: built.message_id.clone(),
        };
        // One blocking call without an await in between: the new copy, the
        // draft's pointer to it and the queued append / delete of the old
        // copy are written together.
        let queued = self
            .inner
            .store
            .call(move |s| {
                // Sent, being sent or deleted meanwhile: no copy.
                if s.is_draft_queued(&draft_id)? || s.get_draft(&draft_id).is_err() {
                    return Ok(Vec::new());
                }
                let previous = s.draft_server_copy(&draft_id)?;
                ingest(s, &blobs, Some(&index), &target, 0, &[incoming])?;
                s.set_draft_server_copy(&draft_id, Some(&copy))?;
                let mut ops = vec![Operation::AppendMessage {
                    message_id: copy_id,
                    folder_id: target.id.clone(),
                }];
                ops.extend(previous.and_then(|p| draft_copy_removal_in(s, &p)));
                s.enqueue_all(&ops)
            })
            .await?;
        self.announce_enqueued(&queued);
        Ok(())
    }

    /// Whether a draft will be sent encrypted, as the composer shows it:
    /// the draft's explicit choice, else the identity's defaults
    /// ([`crate::crypto::effective_crypto`], as `SendDraft` decides), else
    /// an Autocrypt recommendation to encrypt (the composer's suggestion).
    /// When the recommendation cannot be computed the answer is `true`:
    /// a draft is rather kept local than uploaded in plain text.
    async fn draft_goes_encrypted(
        &self,
        draft: &Draft,
        account: &AccountId,
        identity: &Identity,
    ) -> bool {
        let crypto = crate::crypto::effective_crypto(None, draft.crypto.as_ref(), identity);
        if crypto.encrypt {
            return true;
        }
        let settings = identity.crypto.clone().unwrap_or_default();
        if draft.crypto.is_some() || !settings.autocrypt {
            return false;
        }
        let mut recipients: Vec<String> = Vec::new();
        for address in draft.to.iter().chain(&draft.cc).chain(&draft.bcc) {
            let email = address.email.trim().to_ascii_lowercase();
            if !email.is_empty() && !recipients.contains(&email) {
                recipients.push(email);
            }
        }
        if recipients.is_empty() {
            return false;
        }
        let account = account.clone();
        let identity = identity.clone();
        let recommendation = self
            .blocking(move |s| {
                let own = crate::crypto::own_keys(s, &identity)?;
                Ok(nail_pgp::autocrypt::recommend(
                    &s.pgp(),
                    &account,
                    &identity.email,
                    settings.prefer_encrypt,
                    own.openpgp,
                    &recipients,
                    false,
                )?)
            })
            .await;
        match recommendation {
            Ok(r) => r == nail_api::security::AutocryptRecommendation::Encrypt,
            Err(err) => {
                tracing::debug!(%err, "cannot compute the Autocrypt recommendation");
                true
            }
        }
    }

    /// The operation that deletes a draft's server copy, if it still
    /// exists locally.
    async fn draft_copy_removal(&self, copy: &DraftServerCopy) -> Option<Operation> {
        let copy = copy.clone();
        self.inner
            .store
            .call(move |s| Ok(draft_copy_removal_in(s, &copy)))
            .await
            .ok()
            .flatten()
    }

    /// Deletes a draft's server copy (after sending, or when the draft is
    /// discarded). `copy` was read before the draft row went away.
    pub(crate) async fn remove_draft_copy(&self, copy: Option<DraftServerCopy>) {
        let Some(copy) = copy else { return };
        if let Some(op) = self.draft_copy_removal(&copy).await
            && let Err(err) = self.enqueue(vec![op]).await
        {
            tracing::warn!(%err, "cannot delete the draft's server copy");
        }
    }

    /// Records the recipients of a sent message for address autocomplete.
    pub(crate) async fn record_recipients(&self, draft: &Draft) {
        let addresses: Vec<Address> = draft
            .to
            .iter()
            .chain(&draft.cc)
            .chain(&draft.bcc)
            .cloned()
            .collect();
        if addresses.is_empty() {
            return;
        }
        let at = nail_util::time::unix_now();
        // Counted for ranking; unknown ones go to the Collected book (M3a).
        let book = message(
            self.inner.locale.as_deref(),
            nail_store::pim::COLLECTED_BOOK_NAME_ID,
        );
        match self
            .inner
            .store
            .call(move |s| s.collect_addresses(&addresses, at, &book))
            .await
        {
            // New or renamed Collected contacts: searchable at once, and
            // open contact views refresh.
            Ok(written) if !written.is_empty() => {
                let items: Vec<nail_api::pim::PimItemRef> = written
                    .into_iter()
                    .map(|contact_id| nail_api::pim::PimItemRef::Contact { contact_id })
                    .collect();
                self.pim_index_apply(&items, &[]).await;
                self.broadcast(nail_api::Event::ContactsChanged {
                    address_book_ids: vec![nail_api::AddressBookId::new(
                        nail_store::pim::COLLECTED_BOOK_ID,
                    )],
                });
            }
            Ok(_) => {}
            Err(err) => tracing::debug!(%err, "cannot record addresses"),
        }
    }
}

/// The operation that deletes a draft's server copy, if it still exists
/// locally.
fn draft_copy_removal_in(store: &nail_store::Store, copy: &DraftServerCopy) -> Option<Operation> {
    let old = Service::draft_copy_id(&copy.folder_id, &copy.message_id);
    store
        .get_message(&old)
        .is_ok()
        .then_some(Operation::DeleteMessage {
            message_id: old,
            permanent: true,
            source: None,
            trash_folder_id: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original() -> OriginalMessage {
        OriginalMessage {
            id: "m1".into(),
            from: vec![Address {
                name: Some("Bob".into()),
                email: "bob@example.org".into(),
            }],
            to: vec![Address {
                name: None,
                email: "ada@example.org".into(),
            }],
            cc: Vec::new(),
            reply_to: Vec::new(),
            subject: "Plans".into(),
            text: "Hello".into(),
            html: None,
            attachments: Vec::new(),
        }
    }

    /// An upload body that reports its start and waits for a release.
    fn gated(
        log: &Arc<Mutex<Vec<String>>>,
        name: &'static str,
    ) -> (
        impl Future<Output = ()> + Send + 'static,
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let log = Arc::clone(log);
        let body = async move {
            lock(&log).push(format!("{name} started"));
            let _ = started_tx.send(());
            let _ = release_rx.await;
            lock(&log).push(format!("{name} finished"));
        };
        (body, started_rx, release_tx)
    }

    #[tokio::test]
    async fn a_started_draft_upload_is_never_aborted() {
        let uploads = DraftUploads::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let id: DraftId = "draft-1".into();
        let (first, first_started, release_first) = gated(&log, "first");
        uploads.schedule(&id, Duration::ZERO, first);
        first_started.await.unwrap();
        // A newer save while the first upload runs: the first one is not
        // aborted, the second one waits for it.
        let (second, second_started, release_second) = gated(&log, "second");
        uploads.schedule(&id, Duration::ZERO, second);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(*lock(&log), ["first started"]);
        release_first.send(()).unwrap();
        second_started.await.unwrap();
        release_second.send(()).unwrap();
        // Cancel waits for nothing now and returns the draft's lock.
        drop(uploads.cancel(&id).await);
        assert_eq!(
            *lock(&log),
            [
                "first started",
                "first finished",
                "second started",
                "second finished"
            ]
        );
    }

    #[tokio::test]
    async fn cancel_skips_waiting_uploads_and_waits_for_running_ones() {
        let uploads = DraftUploads::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let id: DraftId = "draft-1".into();
        // A waiting upload is dropped.
        let (waiting, _, _keep) = gated(&log, "waiting");
        uploads.schedule(&id, Duration::from_hours(1), waiting);
        drop(uploads.cancel(&id).await);
        // A replaced waiting upload never runs either.
        let (replaced, _, _keep2) = gated(&log, "replaced");
        uploads.schedule(&id, Duration::from_hours(1), replaced);
        let (running, started, release) = gated(&log, "running");
        uploads.schedule(&id, Duration::ZERO, running);
        started.await.unwrap();
        // Cancelling a running upload waits until it is done.
        let cancel = tokio::spawn({
            let uploads = uploads.clone();
            let id = id.clone();
            async move {
                let _guard = uploads.cancel(&id).await;
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!cancel.is_finished());
        release.send(()).unwrap();
        cancel.await.unwrap();
        assert_eq!(*lock(&log), ["running started", "running finished"]);
        // While the caller holds the guard, a new upload waits.
        let guard = uploads.cancel(&id).await;
        let (later, mut later_started, release_later) = gated(&log, "later");
        uploads.schedule(&id, Duration::ZERO, later);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(later_started.try_recv().is_err());
        drop(guard);
        later_started.await.unwrap();
        release_later.send(()).unwrap();
    }

    #[test]
    fn quote_strings_are_localized_and_filled_in() {
        let strings = quote_strings(None, &original(), Some("2026-05-01T10:00:00Z"));
        assert!(
            strings.reply_header.contains("Bob wrote:"),
            "{}",
            strings.reply_header
        );
        assert!(strings.reply_header.starts_with("On 2026-05-01"));
        assert!(
            !strings.forward_intro.is_empty(),
            "{:?}",
            strings.forward_intro
        );
        let labels: Vec<&str> = strings
            .forward_fields
            .iter()
            .map(|(l, _)| l.as_str())
            .collect();
        assert_eq!(labels, ["From", "Date", "Subject", "To"]);
        assert_eq!(strings.forward_fields[0].1, "Bob <bob@example.org>");
        // German falls back to English per key.
        assert_eq!(
            quote_strings(Some("de"), &original(), None).forward_intro,
            strings.forward_intro
        );
    }

    #[test]
    fn identity_settings_map_to_the_composer() {
        let mut identity: Identity = serde_json::from_value(serde_json::json!({
            "id": "a", "account_id": "w", "email": "a@w.example"
        }))
        .unwrap();
        assert_eq!(settings_of(&identity), ComposeSettings::default());
        identity.posting = Some(QuotePosting::Top);
        identity.mail_style = Some(nail_api::accounts::MailStyleConfig {
            css: Some("p { color: red }".into()),
            code_theme: None,
        });
        let settings = settings_of(&identity);
        assert_eq!(settings.posting, Posting::Top);
        assert_eq!(settings.style.css(), "p { color: red }");
    }

    #[test]
    fn compose_errors_map_to_api_codes() {
        assert_eq!(
            compose_error(&ComposeError::NoSender).code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            compose_error(&ComposeError::MissingBlob {
                filename: "a".into(),
                blob: "b".into()
            })
            .code,
            ErrorCode::NotFound
        );
    }
}
