//! Requests of M1b WP21 (composer, ADR 0023): NewDraft, RenderPreview,
//! ConvertDraft, UploadAttachment, SuggestAddresses — plus what the rest of
//! the core needs from the composer: the identity's style and settings,
//! building a draft's message ([`Service::build_draft_message`]) and the
//! autosave copy in the server's Drafts folder.
//!
//! The work is done by `nail-compose`, a pure library; this module feeds
//! it from the store (original messages, attachment blobs) and the Fluent
//! catalogs (quote header, forward texts).

use std::collections::HashMap;
use std::sync::Mutex;

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

/// Largest attachment `UploadAttachment` accepts (decoded bytes).
pub(crate) const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

/// Longest address suggestion list.
const MAX_SUGGESTIONS: u32 = 50;

/// Pending uploads of drafts to the server's Drafts folder, by draft.
#[derive(Default)]
pub(crate) struct DraftUploads(Mutex<HashMap<DraftId, JoinHandle<()>>>);

impl DraftUploads {
    fn replace(&self, id: &DraftId, task: JoinHandle<()>) {
        if let Some(old) = lock(&self.0).insert(id.clone(), task) {
            old.abort();
        }
    }

    fn cancel(&self, id: &DraftId) {
        if let Some(old) = lock(&self.0).remove(id) {
            old.abort();
        }
    }

    fn finished(&self, id: &DraftId) {
        lock(&self.0).remove(id);
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

fn compose_error(err: ComposeError) -> ApiError {
    match err {
        ComposeError::MissingBlob { .. } => ApiError::not_found(err.to_string()),
        ComposeError::NoSender => ApiError::invalid_request(err.to_string()),
        ComposeError::Stylesheet(_) => ApiError::invalid_request(err.to_string()),
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

fn display(address: &Address) -> String {
    match address.name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => format!("{name} <{}>", address.email),
        _ => address.email.clone(),
    }
}

fn join(addresses: &[Address]) -> String {
    addresses.iter().map(display).collect::<Vec<_>>().join(", ")
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
        (message(locale, "message-header-from"), join(&original.from)),
        (message(locale, "message-header-date"), date.clone()),
        (
            message(locale, "composer-field-subject"),
            original.subject.clone(),
        ),
        (message(locale, "message-header-to"), join(&original.to)),
    ];
    if !original.cc.is_empty() {
        fields.push((message(locale, "message-header-cc"), join(&original.cc)));
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
            .map_err(compose_error)?;
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
            let limit = limit.clamp(1, MAX_SUGGESTIONS);
            Response::AddressSuggestions {
                addresses: service
                    .inner
                    .store
                    .call(move |s| s.suggest_addresses(&prefix, limit))
                    .await?,
            }
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} routed to the composer module",
                crate::service::request_name(&other)
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
    fn own_addresses(&self) -> Vec<String> {
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
        Ok(draft)
    }

    /// The message a reply or forward refers to, with its date; a forward
    /// gets the original's attachments copied into the blob store.
    async fn original_message(
        &self,
        id: &MessageId,
        mode: DraftContextMode,
    ) -> Result<(OriginalMessage, Option<String>)> {
        let body = self.message_body(id.clone()).await?;
        let attachments = if mode == DraftContextMode::Forward && !body.attachments.is_empty() {
            let store = self.inner.store.clone();
            let blobs = self.inner.blobs.clone();
            let id = id.clone();
            tokio::task::spawn_blocking(move || -> Result<Vec<DraftAttachment>> {
                let stored = store.get_message(&id)?;
                let Some(hash) = stored.blob_hash else {
                    return Ok(Vec::new());
                };
                let raw = blobs.get(&hash)?;
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
        if bytes.len() > MAX_ATTACHMENT_BYTES {
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
        let draft = draft.clone();
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
        .map_err(compose_error)?;
        Ok(built)
    }

    /// Schedules the upload of a saved draft to the account's Drafts folder
    /// on the server (autosave), replacing an earlier upload of the same
    /// draft that has not started yet.
    pub(crate) fn schedule_draft_upload(&self, id: &DraftId) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let service = self.clone();
        let draft_id = id.clone();
        let delay = self.inner.timing.draft_upload;
        let task = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if let Err(err) = service.upload_draft_copy(&draft_id).await {
                tracing::warn!(draft = %draft_id, %err, "cannot upload the draft copy");
            }
            service.inner.draft_uploads.finished(&draft_id);
        });
        self.inner.draft_uploads.replace(id, task);
    }

    /// Cancels a pending upload (the draft is being sent or deleted).
    pub(crate) fn cancel_draft_upload(&self, id: &DraftId) {
        self.inner.draft_uploads.cancel(id);
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
        let previous = {
            let id = id.clone();
            self.inner
                .store
                .call(move |s| s.draft_server_copy(&id))
                .await?
        };
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
        let index = std::sync::Arc::clone(&self.inner.search);
        let target = folder.clone();
        let draft_id = id.clone();
        let copy = DraftServerCopy {
            folder_id: folder.id.clone(),
            uid: None,
            message_id: built.message_id.clone(),
        };
        self.inner
            .store
            .call(move |s| {
                ingest(s, &blobs, Some(&index), &target, 0, &[incoming])?;
                s.set_draft_server_copy(&draft_id, Some(&copy))
            })
            .await?;
        let mut ops = vec![Operation::AppendMessage {
            message_id: copy_id,
            folder_id: folder.id.clone(),
        }];
        if let Some(previous) = previous {
            ops.extend(self.draft_copy_removal(&previous).await);
        }
        self.enqueue(ops).await?;
        Ok(())
    }

    /// The operation that deletes a draft's server copy, if it still
    /// exists locally.
    async fn draft_copy_removal(&self, copy: &DraftServerCopy) -> Option<Operation> {
        let old = Self::draft_copy_id(&copy.folder_id, &copy.message_id);
        let probe = old.clone();
        let exists = self
            .inner
            .store
            .call(move |s| Ok(s.get_message(&probe).is_ok()))
            .await
            .unwrap_or(false);
        exists.then_some(Operation::DeleteMessage {
            message_id: old,
            permanent: true,
            source: None,
            trash_folder_id: None,
        })
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
        let at = crate::engine::unix_now();
        if let Err(err) = self
            .inner
            .store
            .call(move |s| s.record_addresses(&addresses, at))
            .await
        {
            tracing::debug!(%err, "cannot record addresses");
        }
    }
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

    #[test]
    fn quote_strings_are_localized_and_filled_in() {
        let strings = quote_strings(None, &original(), Some("2026-05-01T10:00:00Z"));
        assert!(
            strings.reply_header.contains("Bob wrote:"),
            "{}",
            strings.reply_header
        );
        assert!(strings.reply_header.starts_with("On 2026-05-01"));
        assert!(!strings.forward_intro.is_empty());
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
            compose_error(ComposeError::NoSender).code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            compose_error(ComposeError::MissingBlob {
                filename: "a".into(),
                blob: "b".into()
            })
            .code,
            ErrorCode::NotFound
        );
    }
}
