//! Building the outgoing message with [`mail_builder`].
//!
//! Structure by mode (design, "Composer: HTML and Markdown"):
//!
//! ```text
//! plain:    text/plain; format=flowed
//! html:     multipart/alternative [ text/plain (from the HTML),
//!                                   multipart/related [ text/html, cid images ] ]
//! markdown: multipart/alternative [ text/plain (the source),
//!                                   text/markdown (optional, RFC 7763),
//!                                   multipart/related [ text/html, cid images ] ]
//! ```
//!
//! With attachments the body is wrapped in `multipart/mixed`. Inline images
//! that the HTML does not reference (and every inline image of a plain
//! message) are sent as regular attachments. `Bcc` never appears in the
//! headers; it is only part of the envelope.

use std::borrow::Cow;

use mail_builder::MessageBuilder;
use mail_builder::headers::HeaderType;
use mail_builder::headers::address::Address as MbAddress;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::message_id::MessageId as MbMessageId;
use mail_builder::headers::raw::Raw;
use mail_builder::mime::{BodyPart, MimePart};
use nail_api::accounts::ComposerMode;
use nail_api::engine::Envelope;
use nail_api::{Address, AutomaticMail, Draft, DraftAttachment, Identity, LOOP_HEADER};
use sha2::{Digest, Sha256};

use crate::convert::html_to_text;
use crate::error::ComposeError;
use crate::flowed::encode_flowed;
use crate::markdown::render_markdown;
use crate::sanitize::sanitize_outgoing;
use crate::style::{MailStyle, mail_document};

/// Reads attachment bytes from the core's blob store.
pub trait BlobSource {
    /// The bytes of blob `hash`, or `None` when it is missing.
    fn read_blob(&self, hash: &str) -> Option<Vec<u8>>;
}

impl<F: Fn(&str) -> Option<Vec<u8>>> BlobSource for F {
    fn read_blob(&self, hash: &str) -> Option<Vec<u8>> {
        self(hash)
    }
}

/// Threading headers of the message being replied to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplyHeaders {
    /// Its `Message-ID` (without angle brackets).
    pub message_id: Option<String>,
    /// Its `References` (without angle brackets), oldest first.
    pub references: Vec<String>,
}

/// Values the caller decides: the send time and the message id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageMeta<'a> {
    /// `Date` header value (RFC 5322 date-time, e.g.
    /// `Tue, 29 Sep 2026 10:00:00 +0200`).
    pub date: &'a str,
    /// `Message-ID` without angle brackets (see [`new_message_id`]).
    pub message_id: &'a str,
}

/// A built message ready for submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMessage {
    /// SMTP envelope (sender and every recipient, Bcc included).
    pub envelope: Envelope,
    /// The RFC 5322 bytes (no `Bcc` header), CRLF line ends.
    pub raw: Vec<u8>,
    /// Its `Message-ID` (without angle brackets).
    pub message_id: String,
}

fn bare_id(id: &str) -> String {
    id.trim().trim_matches(['<', '>']).to_owned()
}

/// A new `Message-ID` (without brackets) in the sender's domain.
pub fn new_message_id(sender: &str, seed: &[u8]) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let domain = sender
        .rsplit_once('@')
        .map(|(_, d)| d.trim())
        .filter(|d| !d.is_empty())
        .unwrap_or("nail.invalid");
    let mut hasher = Sha256::new();
    hasher.update(seed);
    hasher.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(
        COUNTER
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_le_bytes(),
    );
    let mut hex = hex::encode(hasher.finalize());
    hex.truncate(32);
    format!("{hex}@{domain}")
}

/// The SMTP envelope: the identity's address, and every recipient (To, Cc,
/// Bcc, the identity's automatic Bcc) once.
pub fn envelope_of(draft: &Draft, identity: &Identity) -> Envelope {
    let mut to: Vec<String> = Vec::new();
    for address in draft
        .to
        .iter()
        .chain(&draft.cc)
        .chain(&draft.bcc)
        .map(|a| a.email.trim().to_owned())
        .chain(identity.bcc.iter().map(|b| b.trim().to_owned()))
    {
        if !address.is_empty() && !to.iter().any(|t| t.eq_ignore_ascii_case(&address)) {
            to.push(address);
        }
    }
    Envelope {
        // The bounce address is the user's own for "on behalf of".
        from: on_behalf_sender(identity)
            .unwrap_or_else(|| identity.email.trim())
            .to_owned(),
        to,
    }
}

/// The `Sender` address of an identity sending on behalf of a mailbox
/// (its `sender`, when set; the core fills in the account's address).
pub(crate) fn on_behalf_sender(identity: &Identity) -> Option<&str> {
    identity
        .send_as
        .as_ref()
        .filter(|s| s.mode == nail_api::accounts::SendAsMode::OnBehalf)
        .and_then(|s| s.sender.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn clean_header(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

fn address_list(addresses: &[Address]) -> MbAddress<'static> {
    MbAddress::new_list(
        addresses
            .iter()
            .map(|a| {
                MbAddress::new_address(
                    a.name
                        .as_deref()
                        .map(str::trim)
                        .filter(|n| !n.is_empty())
                        .map(clean_header),
                    a.email.trim().to_owned(),
                )
            })
            .collect(),
    )
}

fn text_part(content_type: ContentType<'static>, body: &str) -> MimePart<'static> {
    MimePart::new(
        content_type,
        BodyPart::Text(Cow::Owned(normalize_newlines(body))),
    )
}

/// Normalizes line ends to LF; mail-builder writes CRLF for text parts.
fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn utf8(kind: &'static str) -> ContentType<'static> {
    ContentType::new(kind).attribute("charset", "utf-8")
}

fn load(att: &DraftAttachment, blobs: &dyn BlobSource) -> Result<Vec<u8>, ComposeError> {
    blobs
        .read_blob(&att.blob)
        .ok_or_else(|| ComposeError::MissingBlob {
            filename: att.filename.clone(),
            blob: att.blob.clone(),
        })
}

fn mime_type(att: &DraftAttachment) -> String {
    let mime = att.mime.trim();
    let valid = mime.split_once('/').is_some_and(|(a, b)| {
        !a.is_empty()
            && !b.is_empty()
            && mime
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/+-.".contains(c))
    });
    if valid {
        mime.to_ascii_lowercase()
    } else {
        "application/octet-stream".to_owned()
    }
}

/// The part carrying an attachment's bytes with its content type.
fn data_part(
    att: &DraftAttachment,
    blobs: &dyn BlobSource,
) -> Result<MimePart<'static>, ComposeError> {
    let data = load(att, blobs)?;
    Ok(MimePart::new(
        ContentType::new(mime_type(att)),
        BodyPart::Binary(data.into()),
    ))
}

fn attachment_part(
    att: &DraftAttachment,
    blobs: &dyn BlobSource,
) -> Result<MimePart<'static>, ComposeError> {
    Ok(data_part(att, blobs)?.attachment(clean_header(&att.filename)))
}

fn inline_part(
    att: &DraftAttachment,
    cid: &str,
    blobs: &dyn BlobSource,
) -> Result<MimePart<'static>, ComposeError> {
    Ok(data_part(att, blobs)?
        .header(
            "Content-Disposition",
            HeaderType::ContentType(
                ContentType::new("inline").attribute("filename", clean_header(&att.filename)),
            ),
        )
        .cid(cid.to_owned()))
}

/// Whether the HTML references `cid:<id>`.
fn references_cid(html: &str, cid: &str) -> bool {
    let needle = format!("cid:{cid}").to_ascii_lowercase();
    html.to_ascii_lowercase().contains(&needle)
}

/// The text part, the optional Markdown part and the HTML fragment of a
/// draft.
struct Bodies {
    text: MimePart<'static>,
    markdown: Option<MimePart<'static>>,
    html: Option<String>,
}

fn bodies(draft: &Draft, style: &MailStyle) -> Bodies {
    match draft.mode {
        ComposerMode::Plain => Bodies {
            text: text_part(
                utf8("text/plain").attribute("format", "flowed"),
                &encode_flowed(&draft.text),
            ),
            markdown: None,
            html: None,
        },
        ComposerMode::Html => {
            let html = sanitize_outgoing(draft.html.as_deref().unwrap_or_default());
            Bodies {
                text: text_part(utf8("text/plain"), &html_to_text(&html)),
                markdown: None,
                html: Some(html),
            }
        }
        ComposerMode::Markdown => {
            let source = draft.markdown.as_deref().unwrap_or_default();
            Bodies {
                text: text_part(utf8("text/plain"), source),
                markdown: draft
                    .markdown_part
                    .then(|| text_part(utf8("text/markdown").attribute("variant", "GFM"), source)),
                html: Some(render_markdown(source, style)),
            }
        }
    }
}

/// The `Auto-Submitted` value (RFC 3834) of mail a rule sends.
fn auto_submitted(draft: &Draft) -> Option<&'static str> {
    match draft.automatic.as_ref()? {
        AutomaticMail::Reply => Some("auto-replied"),
        AutomaticMail::Forward | AutomaticMail::Redirect { .. } => Some("auto-generated"),
        AutomaticMail::AutocryptSetup | AutomaticMail::Itip => None,
    }
}

/// A redirect (RFC 5322 §3.6.6): `raw` unchanged, preceded by `Resent-*`
/// headers and the [`LOOP_HEADER`] marker, so the redirect is not
/// redirected again when it comes back.
pub fn resent_message(
    raw: &[u8],
    sender: &str,
    recipients: &[String],
    meta: MessageMeta<'_>,
) -> Vec<u8> {
    let sender = clean_header(sender.trim());
    let mut bytes = format!(
        "Resent-From: {sender}\r\nResent-To: {}\r\nResent-Date: {}\r\nResent-Message-ID: <{}>\r\n{LOOP_HEADER}: {sender}\r\n",
        clean_header(&recipients.join(", ")),
        clean_header(meta.date),
        bare_id(meta.message_id),
    )
    .into_bytes();
    bytes.extend_from_slice(raw);
    bytes
}

/// Builds the message for `draft`, sent as `identity`. `reply` carries the
/// threading headers of a replied message; `blobs` supplies attachment
/// bytes.
pub fn build_message(
    draft: &Draft,
    identity: &Identity,
    reply: Option<&ReplyHeaders>,
    meta: MessageMeta<'_>,
    style: &MailStyle,
    blobs: &dyn BlobSource,
) -> Result<OutgoingMessage, ComposeError> {
    if identity.email.trim().is_empty() {
        return Err(ComposeError::NoSender);
    }
    let message_id = bare_id(meta.message_id);
    let Bodies {
        text,
        markdown,
        html,
    } = bodies(draft, style);

    // Inline images the HTML uses go into multipart/related; the rest are
    // attachments.
    let mut related = Vec::new();
    let mut attachments = Vec::new();
    for att in &draft.attachments {
        match (&html, att.content_id.as_deref().map(str::trim)) {
            (Some(html), Some(cid)) if !cid.is_empty() && references_cid(html, cid) => {
                related.push(inline_part(att, cid.trim_matches(['<', '>']), blobs)?);
            }
            _ => attachments.push(attachment_part(att, blobs)?),
        }
    }

    let body = match html {
        None => text,
        Some(html) => {
            let document = mail_document(&html, style)?;
            let mut html_part = text_part(utf8("text/html"), &document);
            if !related.is_empty() {
                let mut parts = vec![html_part];
                parts.append(&mut related);
                html_part = MimePart::new("multipart/related", parts);
            }
            let mut alternatives = vec![text];
            alternatives.extend(markdown);
            alternatives.push(html_part);
            MimePart::new("multipart/alternative", alternatives)
        }
    };
    let body = if attachments.is_empty() {
        body
    } else {
        let mut parts = vec![body];
        parts.append(&mut attachments);
        MimePart::new("multipart/mixed", parts)
    };

    let from_name = identity
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(clean_header);
    let mut builder = MessageBuilder::new()
        .header("Date", Raw::new(clean_header(meta.date)))
        .from(MbAddress::new_address(
            from_name,
            identity.email.trim().to_owned(),
        ));
    // M4c (ADR 0094): "on behalf of" a shared mailbox puts the user into
    // `Sender`; "send as" sends from the mailbox alone.
    if let Some(sender) = on_behalf_sender(identity) {
        builder = builder.sender(MbAddress::new_address(None::<String>, sender.to_owned()));
    }
    if !draft.to.is_empty() {
        builder = builder.to(address_list(&draft.to));
    }
    if !draft.cc.is_empty() {
        builder = builder.cc(address_list(&draft.cc));
    }
    if let Some(reply_to) = identity
        .reply_to
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        builder = builder.reply_to(MbAddress::new_address(None::<String>, reply_to.to_owned()));
    }
    builder = builder
        .subject(clean_header(&draft.subject))
        .message_id(MbMessageId::new(message_id.clone()));
    if let Some(reply) = reply
        && let Some(parent) = reply.message_id.as_deref().map(bare_id)
        && !parent.is_empty()
    {
        let mut refs: Vec<String> = reply
            .references
            .iter()
            .map(|r| bare_id(r))
            .filter(|r| !r.is_empty() && *r != parent)
            .collect();
        refs.push(parent.clone());
        builder = builder
            .in_reply_to(MbMessageId::new(parent))
            .references(MbMessageId::new_list(refs.into_iter()));
    }
    if let Some(kind) = auto_submitted(draft) {
        builder = builder
            .header("Auto-Submitted", Raw::new(kind))
            .header(LOOP_HEADER, Raw::new(clean_header(identity.email.trim())));
    }
    let raw = builder.body(body).write_to_vec()?;
    Ok(OutgoingMessage {
        envelope: envelope_of(draft, identity),
        raw,
        message_id,
    })
}
