//! Large messages, send as / on behalf, headers-only fetches and moves
//! between mailboxes (plan M4c WP123; ADR 0094 "Exchange / Gmail
//! leftovers").
//!
//! **Upload sessions.** Graph takes at most 4 MB per request, so a MIME
//! upload (`sendMail`, MIME creates) of a larger message is impossible.
//! Such a message is split into its parts ([`DraftParts::from_mime`]): a
//! draft is created from the headers and the body as JSON (`Message-ID`,
//! `In-Reply-To` and `References` travel as MAPI properties so threading
//! survives), attachments up to 3 MB are posted as `fileAttachment`s,
//! larger ones go through `attachments/createUploadSession` in byte ranges
//! (multiples of 320 KiB, [`EngineOptions::upload_chunk`]) to the
//! pre-authenticated upload URL (never with the bearer token), and the
//! draft is sent (`POST messages/{id}/send`). Progress is reported per
//! range ([`nail_api::engine::OutgoingTransport::send_with_progress`]).
//! Signed or encrypted messages (S/MIME, PGP/MIME) cannot be rebuilt from
//! parts without breaking the signature: over 4 MB they stay
//! [`GraphError::TooLarge`]. Appends over 4 MB use the same parts, created
//! in the target folder and marked sent (`PR_MESSAGE_FLAGS`).
//!
//! **Send as / on behalf** ([`SendingMailbox::of`]): `From` other than the
//! user's address sends from that mailbox (`/users/{mailbox}/sendMail`,
//! the copy lands in its Sent Items: Send As); with a `Sender` header that
//! is the user, the message goes out from the user's own mailbox with
//! `from` = the shared mailbox and `sender` = the user (Send on Behalf).
//! Which one Exchange allows is the mailbox's permission.
//!
//! **Headers only** ([`GraphEngine::fetch_headers`]): the transport headers
//! (`internetMessageHeaders`) without the body, for folders whose sync
//! policy keeps headers only; messages without transport headers (sent
//! mail, drafts) fall back to the MIME cut at the header end.
//!
//! **Moves between mailboxes** ([`GraphEngine::move_to_mailbox`]): Graph's
//! `move` works only inside one mailbox; across mailboxes the message is
//! copied as MIME (or parts) into the target and deleted at the source.
//!
//! [`EngineOptions::upload_chunk`]: crate::EngineOptions::upload_chunk

use base64::Engine as _;
use mail_parser::{MessageParser, MimeHeaders};
use nail_api::accounts::SendingMailbox;
use nail_api::domain::Flags;
use nail_api::engine::{Envelope, SendProgress, SendReceipt};
use reqwest::Method;
use serde_json::{Value, json};

use crate::engine::GraphEngine;
use crate::error::GraphError;
use crate::mail::{MAX_MIME_UPLOAD, encode_mime};
use crate::wire::{Message, UploadSession};
use crate::{mailbox_path, mime, path_segment};

/// Graph's unit for upload-session ranges (320 KiB = 327,680 bytes).
pub const UPLOAD_UNIT: usize = 327_680;

/// The default bytes per upload range (10 units, just over 3 MiB).
pub const DEFAULT_UPLOAD_CHUNK: usize = 10 * UPLOAD_UNIT;

/// Attachments up to this size are posted in one request (base64 keeps
/// the request under 4 MB); larger ones use an upload session.
pub const SMALL_ATTACHMENT: usize = 3 * 1024 * 1024;

/// The largest attachment Graph accepts through an upload session.
pub const MAX_ATTACHMENT: usize = 150 * 1024 * 1024;

/// `PidTagInternetReferences` (`References`).
const REFERENCES_PROP: &str = "String 0x1039";
/// `PidTagInReplyToId` (`In-Reply-To`).
const IN_REPLY_TO_PROP: &str = "String 0x1042";
/// `PidTagMessageFlags`: 1 = read, without 8 (unsent) = not a draft.
pub(crate) const MESSAGE_FLAGS_PROP: &str = "Integer 0x0E07";

/// Whether a raw message is too large for one MIME upload.
pub fn needs_upload_session(raw: &[u8]) -> bool {
    raw.len().div_ceil(3) * 4 > MAX_MIME_UPLOAD
}

/// The bytes per range for a configured chunk size: a multiple of 320 KiB,
/// at least one unit.
pub fn range_size(configured: usize) -> usize {
    (configured / UPLOAD_UNIT).max(1) * UPLOAD_UNIT
}

/// One attachment of a [`DraftParts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentPart {
    /// File name.
    pub name: String,
    /// Media type (`application/pdf`).
    pub content_type: String,
    /// `Content-ID` without angle brackets (inline images).
    pub content_id: Option<String>,
    /// Shown inline in the body.
    pub inline: bool,
    /// The decoded content.
    pub bytes: Vec<u8>,
}

/// A message split into a Graph draft (JSON) and its attachments.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftParts {
    /// The `message` resource to create (no attachments).
    pub message: Value,
    /// The attachments, in order.
    pub attachments: Vec<AttachmentPart>,
}

fn recipients(address: Option<&mail_parser::Address<'_>>) -> Vec<Value> {
    address
        .map(|a| {
            a.iter()
                .filter_map(|addr| {
                    let address = addr.address()?.trim();
                    (!address.is_empty()).then(|| {
                        let mut email = json!({ "address": address });
                        if let Some(name) = addr.name().filter(|n| !n.trim().is_empty()) {
                            email["name"] = json!(name);
                        }
                        json!({ "emailAddress": email })
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn header_text(raw: &[u8], name: &str) -> Option<String> {
    mime::header(raw, name).filter(|v| !v.trim().is_empty())
}

impl DraftParts {
    /// Splits a raw message. Signed or encrypted messages are refused
    /// ([`GraphError::TooLarge`]: they cannot be rebuilt from parts and are
    /// over the MIME limit); so are attachments over 150 MB.
    pub fn from_mime(raw: &[u8]) -> Result<Self, GraphError> {
        let content_type = mime::header(raw, "content-type")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if content_type.starts_with("multipart/signed")
            || content_type.starts_with("multipart/encrypted")
            || content_type.starts_with("application/pkcs7-mime")
            || content_type.starts_with("application/x-pkcs7-mime")
        {
            return Err(GraphError::TooLarge(format!(
                "a signed or encrypted message of {} bytes (Graph takes 4 MB of MIME)",
                raw.len()
            )));
        }
        let parsed = MessageParser::default()
            .parse(raw)
            .ok_or_else(|| GraphError::Rejected("the message does not parse".into()))?;
        let mut message = json!({
            "subject": parsed.subject().unwrap_or_default(),
        });
        // The body: the first HTML part, else the text.
        let html = parsed
            .html_bodies()
            .find(|p| p.is_text_html())
            .and_then(|p| p.text_contents().map(str::to_owned));
        message["body"] = match html {
            Some(content) => json!({ "contentType": "html", "content": content }),
            None => json!({
                "contentType": "text",
                "content": parsed.body_text(0).map(std::borrow::Cow::into_owned).unwrap_or_default(),
            }),
        };
        let from = recipients(parsed.from());
        if let Some(first) = from.first() {
            message["from"] = first.clone();
        }
        let sender = recipients(parsed.sender());
        if let Some(first) = sender.first() {
            message["sender"] = first.clone();
        }
        for (field, list) in [
            ("toRecipients", recipients(parsed.to())),
            ("ccRecipients", recipients(parsed.cc())),
            ("bccRecipients", recipients(parsed.bcc())),
            ("replyTo", recipients(parsed.reply_to())),
        ] {
            if !list.is_empty() {
                message[field] = json!(list);
            }
        }
        if let Some(id) = mime::message_id(raw) {
            message["internetMessageId"] = json!(id);
        }
        let importance = header_text(raw, "importance")
            .or_else(|| header_text(raw, "x-priority"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if importance.starts_with("high") || importance.starts_with('1') {
            message["importance"] = json!("high");
        } else if importance.starts_with("low") || importance.starts_with('5') {
            message["importance"] = json!("low");
        }
        if header_text(raw, "disposition-notification-to").is_some() {
            message["isReadReceiptRequested"] = json!(true);
        }
        let mut props = Vec::new();
        if let Some(v) = header_text(raw, "in-reply-to") {
            props.push(json!({ "id": IN_REPLY_TO_PROP, "value": v }));
        }
        if let Some(v) = header_text(raw, "references") {
            props.push(json!({ "id": REFERENCES_PROP, "value": v }));
        }
        if !props.is_empty() {
            message["singleValueExtendedProperties"] = json!(props);
        }
        // Graph keeps custom headers only when they start with `x-` (at
        // most five).
        let custom: Vec<Value> = mime::headers(raw)
            .into_iter()
            .filter(|(n, _)| n.starts_with("x-") && n != "x-priority")
            .take(5)
            .map(|(n, v)| json!({ "name": header_case(&n), "value": v }))
            .collect();
        if !custom.is_empty() {
            message["internetMessageHeaders"] = json!(custom);
        }
        let mut attachments = Vec::new();
        for (i, part) in parsed.attachments().enumerate() {
            let bytes = part.contents().to_vec();
            if bytes.len() > MAX_ATTACHMENT {
                return Err(GraphError::TooLarge(format!(
                    "an attachment of {} bytes (Graph takes 150 MB)",
                    bytes.len()
                )));
            }
            let content_type = part.content_type().map_or_else(
                || {
                    if part.is_message() {
                        "message/rfc822".to_owned()
                    } else {
                        "application/octet-stream".to_owned()
                    }
                },
                |ct| match ct.subtype() {
                    Some(sub) => format!("{}/{sub}", ct.ctype()),
                    None => ct.ctype().to_owned(),
                },
            );
            let name = part.attachment_name().map_or_else(
                || {
                    if part.is_message() {
                        format!("message-{}.eml", i + 1)
                    } else {
                        format!("attachment-{}", i + 1)
                    }
                },
                str::to_owned,
            );
            let content_id = part
                .content_id()
                .map(|c| c.trim().trim_matches(['<', '>']).to_owned())
                .filter(|c| !c.is_empty());
            let inline = part
                .content_disposition()
                .is_some_and(mail_parser::ContentType::is_inline)
                && content_id.is_some();
            attachments.push(AttachmentPart {
                name,
                content_type,
                content_id,
                inline,
                bytes,
            });
        }
        Ok(Self {
            message,
            attachments,
        })
    }

    /// The bytes an upload of these parts sends (body plus attachments),
    /// the total of the progress reports.
    pub fn upload_bytes(&self) -> u64 {
        let body = self.message["body"]["content"].as_str().map_or(0, str::len);
        body as u64
            + self
                .attachments
                .iter()
                .map(|a| a.bytes.len() as u64)
                .sum::<u64>()
    }
}

/// `x-my-header` → `X-My-Header`.
fn header_case(name: &str) -> String {
    name.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| {
                f.to_uppercase().collect::<String>() + c.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// The first `Content-Range` start a session still wants (`"12-"`,
/// `"12-99"`), if any.
fn next_start(session: &UploadSession) -> Option<u64> {
    session
        .next_expected_ranges
        .first()
        .and_then(|r| r.split('-').next())
        .and_then(|s| s.trim().parse().ok())
}

/// Reports progress and remembers how far it got.
struct Progress<'a> {
    report: Option<SendProgress<'a>>,
    sent: u64,
    total: u64,
}

impl Progress<'_> {
    fn advance(&mut self, bytes: u64) {
        self.sent = (self.sent + bytes).min(self.total);
        if let Some(report) = self.report {
            report(self.sent, self.total);
        }
    }
}

impl GraphEngine {
    /// Sends a raw message from the right mailbox (send as / on behalf,
    /// see the module docs), through an upload session when it is over
    /// 4 MB (reporting progress). Exchange files the Sent Items copy.
    pub async fn send_message(
        &self,
        envelope: &Envelope,
        raw: &[u8],
        progress: Option<SendProgress<'_>>,
    ) -> Result<SendReceipt, GraphError> {
        let mailbox = self.sending_mailbox(raw).await?;
        if needs_upload_session(&mime::with_envelope_bcc(raw, &envelope.to)?) {
            self.send_large(mailbox.as_deref(), envelope, raw, progress)
                .await
        } else {
            self.send_mime(mailbox.as_deref(), envelope, raw).await
        }
    }

    /// The mailbox a message is sent from: `None` = the user's own (own
    /// mail and on-behalf mail), `Some(address)` = send as that mailbox.
    pub async fn sending_mailbox(&self, raw: &[u8]) -> Result<Option<String>, GraphError> {
        let Some(from) = header_text(raw, "from").and_then(|f| mime::addresses(&f).pop()) else {
            return Ok(None);
        };
        let sender = header_text(raw, "sender").and_then(|s| mime::addresses(&s).pop());
        let own = self.own_address().await?;
        Ok(match SendingMailbox::of(&from, sender.as_deref(), &own) {
            SendingMailbox::Own | SendingMailbox::OnBehalf { .. } => None,
            SendingMailbox::SendAs { mailbox } => Some(mailbox),
        })
    }

    /// Sends a message over 4 MB: a draft from its parts, attachments
    /// (upload sessions over 3 MB), then `send`. A failed upload deletes
    /// the draft again.
    pub async fn send_large(
        &self,
        mailbox: Option<&str>,
        envelope: &Envelope,
        raw: &[u8],
        progress: Option<SendProgress<'_>>,
    ) -> Result<SendReceipt, GraphError> {
        let raw = mime::for_sending(raw, envelope)?;
        let parts = DraftParts::from_mime(&raw)?;
        let base = mailbox_path(mailbox);
        let id = self
            .create_draft_message(&format!("{base}/messages"), &parts.message)
            .await?;
        let mut progress = Progress {
            report: progress,
            sent: 0,
            total: parts.upload_bytes(),
        };
        let result = async {
            progress.advance(
                parts.message["body"]["content"]
                    .as_str()
                    .map_or(0, str::len) as u64,
            );
            for attachment in &parts.attachments {
                self.add_attachment(mailbox, &id, attachment, &mut progress)
                    .await?;
            }
            self.client
                .send_json(
                    Method::POST,
                    &format!("{base}/messages/{}/send", path_segment(&id)),
                    None,
                    &[],
                    false,
                    "POST messages/send",
                )
                .await
        }
        .await;
        if let Err(error) = result {
            self.discard_draft(mailbox, &id).await;
            return Err(error);
        }
        Ok(SendReceipt {
            response: format!(
                "202 accepted by Microsoft Graph ({} attachments uploaded)",
                parts.attachments.len()
            ),
        })
    }

    /// Stores a message over 4 MB in a folder from its parts (not shown as
    /// unsent: `PR_MESSAGE_FLAGS` without the unsent bit); answers its id.
    pub async fn append_large(
        &self,
        mailbox: Option<&str>,
        folder: &str,
        raw: &[u8],
        flags: Flags,
    ) -> Result<String, GraphError> {
        let mut parts = DraftParts::from_mime(raw)?;
        if !flags.draft {
            let props = parts.message["singleValueExtendedProperties"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mut props: Vec<Value> = props;
            props.push(json!({
                "id": MESSAGE_FLAGS_PROP,
                "value": if flags.seen { "1" } else { "0" },
            }));
            parts.message["singleValueExtendedProperties"] = json!(props);
        }
        let base = mailbox_path(mailbox);
        let id = self
            .create_draft_message(
                &format!("{base}/mailFolders/{}/messages", path_segment(folder)),
                &parts.message,
            )
            .await?;
        let mut progress = Progress {
            report: None,
            sent: 0,
            total: parts.upload_bytes(),
        };
        for attachment in &parts.attachments {
            if let Err(error) = self
                .add_attachment(mailbox, &id, attachment, &mut progress)
                .await
            {
                self.discard_draft(mailbox, &id).await;
                return Err(error);
            }
        }
        self.set_flags(mailbox, &id, &crate::mail::stored_flags(flags))
            .await?;
        Ok(id)
    }

    async fn create_draft_message(
        &self,
        path: &str,
        message: &Value,
    ) -> Result<String, GraphError> {
        let created = self
            .client
            .send_json(
                Method::POST,
                path,
                Some(message),
                &[],
                false,
                "POST messages (draft)",
            )
            .await?;
        let created: Message = serde_json::from_value(created)
            .map_err(|e| GraphError::Protocol(format!("draft: {e}")))?;
        if created.id.is_empty() {
            return Err(GraphError::Protocol("the draft has no id".into()));
        }
        Ok(created.id)
    }

    async fn discard_draft(&self, mailbox: Option<&str>, id: &str) {
        if let Err(error) = self
            .client
            .send_json(
                Method::DELETE,
                &format!("{}/messages/{}", mailbox_path(mailbox), path_segment(id)),
                None,
                &[],
                true,
                "DELETE draft",
            )
            .await
        {
            tracing::warn!(%error, "could not delete the unsent draft");
        }
    }

    /// Adds one attachment to a draft: posted at once up to 3 MB, else
    /// through an upload session.
    async fn add_attachment(
        &self,
        mailbox: Option<&str>,
        message_id: &str,
        attachment: &AttachmentPart,
        progress: &mut Progress<'_>,
    ) -> Result<(), GraphError> {
        let path = format!(
            "{}/messages/{}/attachments",
            mailbox_path(mailbox),
            path_segment(message_id)
        );
        if attachment.bytes.len() <= SMALL_ATTACHMENT {
            let mut body = json!({
                "@odata.type": "#microsoft.graph.fileAttachment",
                "name": attachment.name,
                "contentType": attachment.content_type,
                "contentBytes": base64::engine::general_purpose::STANDARD.encode(&attachment.bytes),
                "isInline": attachment.inline,
            });
            if let Some(cid) = &attachment.content_id {
                body["contentId"] = json!(cid);
            }
            self.client
                .send_json(
                    Method::POST,
                    &path,
                    Some(&body),
                    &[],
                    false,
                    "POST messages/attachments",
                )
                .await?;
            progress.advance(attachment.bytes.len() as u64);
            return Ok(());
        }
        let mut item = json!({
            "attachmentType": "file",
            "name": attachment.name,
            "size": attachment.bytes.len(),
            "contentType": attachment.content_type,
            "isInline": attachment.inline,
        });
        if let Some(cid) = &attachment.content_id {
            item["contentId"] = json!(cid);
        }
        let session = self
            .client
            .send_json(
                Method::POST,
                &format!("{path}/createUploadSession"),
                Some(&json!({ "AttachmentItem": item })),
                &[],
                false,
                "POST attachments/createUploadSession",
            )
            .await?;
        let session: UploadSession = serde_json::from_value(session)
            .map_err(|e| GraphError::Protocol(format!("createUploadSession: {e}")))?;
        if session.upload_url.is_empty() {
            return Err(GraphError::Protocol(
                "createUploadSession answered no upload URL".into(),
            ));
        }
        let total = attachment.bytes.len() as u64;
        let chunk = range_size(self.options.upload_chunk) as u64;
        let mut start = next_start(&session).unwrap_or(0);
        let mut reported = 0u64;
        // At most one request per range plus a few repeats the server asks
        // for (it may want a range again after a lost answer).
        let mut budget = total.div_ceil(chunk) + 8;
        loop {
            if budget == 0 {
                return Err(GraphError::Protocol(
                    "the upload session never completed".into(),
                ));
            }
            budget -= 1;
            let end = (start + chunk).min(total);
            let range = &attachment.bytes[usize::try_from(start).unwrap_or(usize::MAX)
                ..usize::try_from(end).unwrap_or(usize::MAX)];
            let (status, answer) = self
                .client
                .put_upload_range(&session.upload_url, range, start, total, "PUT upload range")
                .await?;
            if end > reported {
                progress.advance(end - reported);
                reported = end;
            }
            if status == 201 || end >= total && status != 200 {
                return Ok(());
            }
            let next: UploadSession = serde_json::from_value(answer).unwrap_or_default();
            match next_start(&next) {
                Some(n) if n < total => start = n,
                // Nothing more expected: done.
                _ if end >= total => return Ok(()),
                _ => start = end,
            }
        }
    }

    /// The transport headers of a message as an RFC 5322 header section
    /// (ending with the empty line), for headers-only sync policies.
    /// Messages without transport headers (sent mail, drafts) fall back to
    /// the MIME cut at the end of its headers.
    pub async fn fetch_headers(
        &self,
        mailbox: Option<&str>,
        remote_id: &str,
    ) -> Result<Vec<u8>, GraphError> {
        let message: Message = self
            .client
            .get_json(
                &format!(
                    "{}/messages/{}",
                    mailbox_path(mailbox),
                    path_segment(remote_id)
                ),
                &[("$select", "internetMessageHeaders".to_owned())],
                &[],
                "GET message headers",
            )
            .await?;
        let headers = message.internet_message_headers.unwrap_or_default();
        if headers.is_empty() {
            let raw = self.mime(mailbox, remote_id).await?;
            return Ok(header_section(&raw));
        }
        let mut out = String::new();
        for h in headers {
            // Header injection: names are tokens, values one line.
            if h.name.is_empty()
                || h.name
                    .contains(|c: char| c.is_whitespace() || c == ':' || c.is_control())
            {
                continue;
            }
            let value = h.value.replace(['\r', '\n'], " ");
            out.push_str(&h.name);
            out.push_str(": ");
            out.push_str(value.trim());
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        Ok(out.into_bytes())
    }

    /// Moves a message to a folder of another mailbox of the account
    /// (`None` = the user's own): `move` within one mailbox, else a copy
    /// in the target (MIME, or parts over 4 MB, with the flags) and a
    /// permanent delete at the source. Answers the new id.
    pub async fn move_to_mailbox(
        &self,
        from_mailbox: Option<&str>,
        remote_id: &str,
        to_mailbox: Option<&str>,
        to_folder: &str,
        flags: Flags,
    ) -> Result<String, GraphError> {
        let same = match (from_mailbox, to_mailbox) {
            (None, None) => true,
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (Some(a), None) | (None, Some(a)) => {
                let own = self.own_address().await?;
                a.eq_ignore_ascii_case(&own)
            }
        };
        if same {
            return self.move_message(from_mailbox, remote_id, to_folder).await;
        }
        let raw = self.mime(from_mailbox, remote_id).await?;
        let new_id = if needs_upload_session(&raw) {
            self.append_large(to_mailbox, to_folder, &raw, flags)
                .await?
        } else {
            self.append(to_mailbox, to_folder, &raw, flags).await?
        };
        match self
            .client
            .send_json(
                Method::POST,
                &format!(
                    "{}/messages/{}/permanentDelete",
                    mailbox_path(from_mailbox),
                    path_segment(remote_id)
                ),
                None,
                &[],
                true,
                "POST messages/permanentDelete",
            )
            .await
        {
            Ok(_) | Err(GraphError::NotFound(_)) => Ok(new_id),
            Err(e) => Err(e),
        }
    }
}

/// The header section of a raw message, ending with the empty line.
pub fn header_section(raw: &[u8]) -> Vec<u8> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|i| i + 2));
    if let Some(end) = end {
        raw[..end].to_vec()
    } else {
        let mut out = raw.to_vec();
        if !out.ends_with(b"\r\n") {
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out
    }
}

/// A raw message as the base64 text of a MIME upload, or `None` when it is
/// over 4 MB (then [`DraftParts`] apply).
pub fn mime_upload(raw: &[u8]) -> Option<Vec<u8>> {
    encode_mime(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const LARGE: &[u8] = b"From: Team <team@contoso.example>\r\nSender: Ada <ada@contoso.example>\r\nTo: \"Bob B\" <bob@fabrikam.example>, carol@fabrikam.example\r\nCc: dave@contoso.example\r\nReply-To: help@contoso.example\r\nSubject: Plans\r\nMessage-ID: <plan-1@contoso.example>\r\nIn-Reply-To: <plan-0@contoso.example>\r\nReferences: <root@contoso.example> <plan-0@contoso.example>\r\nImportance: high\r\nX-Nail-Draft: 7\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: multipart/related; boundary=\"r\"\r\n\r\n--r\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Hello <img src=\"cid:logo@x\"></p>\r\n--r\r\nContent-Type: image/png\r\nContent-ID: <logo@x>\r\nContent-Disposition: inline; filename=\"logo.png\"\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n--r--\r\n--b\r\nContent-Type: application/pdf; name=\"plan.pdf\"\r\nContent-Disposition: attachment; filename=\"plan.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQ=\r\n--b--\r\n";

    #[test]
    fn messages_split_into_a_draft_and_attachments() {
        let parts = DraftParts::from_mime(LARGE).unwrap();
        let m = &parts.message;
        assert_eq!(m["subject"], "Plans");
        assert_eq!(m["body"]["contentType"], "html");
        assert!(
            m["body"]["content"]
                .as_str()
                .unwrap()
                .contains("cid:logo@x")
        );
        assert_eq!(m["from"]["emailAddress"]["address"], "team@contoso.example");
        assert_eq!(
            m["sender"]["emailAddress"]["address"],
            "ada@contoso.example"
        );
        assert_eq!(m["toRecipients"].as_array().unwrap().len(), 2);
        assert_eq!(m["toRecipients"][0]["emailAddress"]["name"], "Bob B");
        assert_eq!(
            m["ccRecipients"][0]["emailAddress"]["address"],
            "dave@contoso.example"
        );
        assert_eq!(
            m["replyTo"][0]["emailAddress"]["address"],
            "help@contoso.example"
        );
        assert_eq!(m["internetMessageId"], "<plan-1@contoso.example>");
        assert_eq!(m["importance"], "high");
        assert_eq!(
            m["singleValueExtendedProperties"],
            json!([
                {"id": IN_REPLY_TO_PROP, "value": "<plan-0@contoso.example>"},
                {"id": REFERENCES_PROP, "value": "<root@contoso.example> <plan-0@contoso.example>"},
            ])
        );
        assert_eq!(
            m["internetMessageHeaders"],
            json!([{"name": "X-Nail-Draft", "value": "7"}])
        );
        assert_eq!(parts.attachments.len(), 2);
        let logo = &parts.attachments[0];
        assert_eq!(logo.name, "logo.png");
        assert!(logo.inline);
        assert_eq!(logo.content_id.as_deref(), Some("logo@x"));
        assert_eq!(logo.content_type, "image/png");
        let pdf = &parts.attachments[1];
        assert_eq!(
            (pdf.name.as_str(), pdf.content_type.as_str(), pdf.inline),
            ("plan.pdf", "application/pdf", false)
        );
        assert_eq!(pdf.bytes, b"%PDF-1.4");
        let body = parts.message["body"]["content"].as_str().unwrap().len() as u64;
        assert_eq!(parts.upload_bytes(), body + 8 + 8);
    }

    #[test]
    fn plain_text_messages_keep_their_text() {
        let parts = DraftParts::from_mime(
            b"From: a@x.example\r\nTo: b@x.example\r\nSubject: s\r\n\r\nJust text\r\n",
        )
        .unwrap();
        assert_eq!(parts.message["body"]["contentType"], "text");
        assert!(
            parts.message["body"]["content"]
                .as_str()
                .unwrap()
                .contains("Just text")
        );
        assert!(parts.attachments.is_empty(), "{:?}", parts.attachments);
        assert!(parts.message.get("sender").is_none());
        assert!(parts.message.get("singleValueExtendedProperties").is_none());
    }

    #[test]
    fn signed_and_encrypted_messages_are_refused() {
        for ct in [
            "multipart/signed; protocol=\"application/pkcs7-signature\"; boundary=x",
            "multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=x",
            "application/pkcs7-mime; smime-type=enveloped-data",
        ] {
            let raw = format!("From: a@x.example\r\nContent-Type: {ct}\r\n\r\nx\r\n");
            assert!(
                matches!(
                    DraftParts::from_mime(raw.as_bytes()),
                    Err(GraphError::TooLarge(_))
                ),
                "{ct}"
            );
        }
    }

    #[test]
    fn sizes_and_ranges() {
        assert!(!needs_upload_session(&vec![b'a'; 3 * 1024 * 1024]));
        assert!(needs_upload_session(&vec![b'a'; 3 * 1024 * 1024 + 4]));
        assert_eq!(range_size(0), UPLOAD_UNIT);
        assert_eq!(range_size(UPLOAD_UNIT * 3 + 5), UPLOAD_UNIT * 3);
        assert_eq!(DEFAULT_UPLOAD_CHUNK % UPLOAD_UNIT, 0);
        let session = UploadSession {
            next_expected_ranges: vec!["655360-".into()],
            ..UploadSession::default()
        };
        assert_eq!(next_start(&session), Some(655_360));
        assert_eq!(next_start(&UploadSession::default()), None);
        assert_eq!(header_case("x-nail-draft"), "X-Nail-Draft");
        assert!(mime_upload(b"x").is_some());
    }

    #[test]
    fn header_sections() {
        assert_eq!(
            header_section(b"A: 1\r\nB: 2\r\n\r\nbody"),
            b"A: 1\r\nB: 2\r\n\r\n"
        );
        assert_eq!(header_section(b"A: 1\n\nbody"), b"A: 1\n\n");
        assert_eq!(header_section(b"A: 1"), b"A: 1\r\n\r\n");
    }
}
