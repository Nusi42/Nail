//! `users.messages`: list, get (`raw` / `minimal` / `metadata` / `full`),
//! modify, batchModify, trash, untrash, delete, insert, send (raw MIME,
//! media upload for large messages), drafts.create, attachments.get.

use nail_api::gmail::LabelChange;
use reqwest::Method;
use serde::de::IgnoredAny;
use serde_json::json;

use nail_net::http::Body;

use crate::client::{Call, GmailClient, parse_json};
use crate::error::GmailError;
use crate::wire::{
    BatchModifyRequest, Draft, ListMessagesResponse, Message, MessagePartBody, MessageRef,
    ModifyRequest, decode_base64url, encode_base64url,
};

/// Largest message sent as JSON `raw`; larger ones go through the media
/// upload endpoint (`/upload/gmail/v1/…`, `uploadType=multipart`).
pub const MAX_SIMPLE_UPLOAD: usize = 4 * 1024 * 1024;

/// Most ids per `batchModify`.
pub const MAX_BATCH_MODIFY: usize = 1000;

/// The `format` of `messages.get`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Ids, labels, dates, size.
    Minimal,
    /// Plus the headers.
    Metadata,
    /// Plus the parsed body structure.
    Full,
    /// Plus the whole message as base64url.
    Raw,
}

impl Format {
    /// The parameter value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Metadata => "metadata",
            Self::Full => "full",
            Self::Raw => "raw",
        }
    }
}

/// Parameters of `messages.list` / `threads.list`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListQuery {
    /// Gmail search (`after:1700000000`, `rfc822msgid:…`).
    pub q: Option<String>,
    /// Only messages with all of these labels.
    pub label_ids: Vec<String>,
    /// Include Spam and Trash.
    pub include_spam_trash: bool,
    /// Page size (Gmail: at most 500).
    pub max_results: Option<u32>,
    /// The page.
    pub page_token: Option<String>,
}

impl ListQuery {
    pub(crate) fn params(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(q) = &self.q {
            out.push(("q", q.clone()));
        }
        for label in &self.label_ids {
            out.push(("labelIds", label.clone()));
        }
        if self.include_spam_trash {
            out.push(("includeSpamTrash", "true".to_owned()));
        }
        if let Some(n) = self.max_results {
            out.push(("maxResults", n.min(500).to_string()));
        }
        if let Some(token) = &self.page_token {
            out.push(("pageToken", token.clone()));
        }
        out
    }
}

/// The Gmail search term for messages received after `unix_secs`.
pub fn after_query(unix_secs: i64) -> String {
    format!("after:{}", unix_secs.max(0))
}

/// The Gmail search term for a `Message-ID` (angle brackets optional).
pub fn message_id_query(message_id: &str) -> String {
    let id = message_id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>');
    format!("rfc822msgid:{id}")
}

/// A boundary that does not occur in `content`.
pub(crate) fn boundary_for(prefix: &str, content: &[u8]) -> String {
    let mut n: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_nanos()));
    loop {
        let candidate = format!("{prefix}_{n:016x}");
        let needle = candidate.as_bytes();
        if !content.windows(needle.len()).any(|w| w == needle) {
            return candidate;
        }
        n = n.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    }
}

impl GmailClient {
    /// `messages.list`: one page of message ids, newest first.
    pub async fn list_messages(
        &self,
        query: &ListQuery,
    ) -> Result<ListMessagesResponse, GmailError> {
        self.json(
            Method::GET,
            &["messages"],
            &query.params(),
            None::<&()>,
            true,
            "users.messages.list",
        )
        .await
    }

    /// `messages.get` in `format`; `metadata_headers` limits the headers of
    /// `metadata`.
    pub async fn get_message(
        &self,
        id: &str,
        format: Format,
        metadata_headers: &[&str],
    ) -> Result<Message, GmailError> {
        let mut query = vec![("format", format.as_str().to_owned())];
        if format == Format::Metadata {
            query.extend(
                metadata_headers
                    .iter()
                    .map(|h| ("metadataHeaders", (*h).to_owned())),
            );
        }
        self.json(
            Method::GET,
            &["messages", id],
            &query,
            None::<&()>,
            true,
            "users.messages.get",
        )
        .await
    }

    /// The raw RFC 5322 bytes of a message (attachments included).
    pub async fn get_raw(&self, id: &str) -> Result<Vec<u8>, GmailError> {
        let message = self.get_message(id, Format::Raw, &[]).await?;
        message
            .raw_bytes()?
            .ok_or_else(|| GmailError::Protocol("messages.get?format=raw without raw".into()))
    }

    /// `messages.modify`: adds and removes labels.
    pub async fn modify_message(
        &self,
        id: &str,
        change: &LabelChange,
    ) -> Result<Message, GmailError> {
        let body = ModifyRequest {
            add_label_ids: change.add.clone(),
            remove_label_ids: change.remove.clone(),
        };
        self.json(
            Method::POST,
            &["messages", id, "modify"],
            &[],
            Some(&body),
            true,
            "users.messages.modify",
        )
        .await
    }

    /// `messages.batchModify` (in chunks of [`MAX_BATCH_MODIFY`]).
    pub async fn batch_modify(
        &self,
        ids: &[String],
        change: &LabelChange,
    ) -> Result<(), GmailError> {
        for chunk in ids.chunks(MAX_BATCH_MODIFY) {
            let body = BatchModifyRequest {
                ids: chunk.to_vec(),
                add_label_ids: change.add.clone(),
                remove_label_ids: change.remove.clone(),
            };
            self.json::<IgnoredAny>(
                Method::POST,
                &["messages", "batchModify"],
                &[],
                Some(&body),
                true,
                "users.messages.batchModify",
            )
            .await?;
        }
        Ok(())
    }

    /// `messages.trash`.
    pub async fn trash_message(&self, id: &str) -> Result<Message, GmailError> {
        self.json(
            Method::POST,
            &["messages", id, "trash"],
            &[],
            None::<&()>,
            true,
            "users.messages.trash",
        )
        .await
    }

    /// `messages.untrash`.
    pub async fn untrash_message(&self, id: &str) -> Result<Message, GmailError> {
        self.json(
            Method::POST,
            &["messages", id, "untrash"],
            &[],
            None::<&()>,
            true,
            "users.messages.untrash",
        )
        .await
    }

    /// `messages.delete` (permanent).
    pub async fn delete_message(&self, id: &str) -> Result<(), GmailError> {
        self.json::<IgnoredAny>(
            Method::DELETE,
            &["messages", id],
            &[],
            None::<&()>,
            true,
            "users.messages.delete",
        )
        .await
        .map(drop)
    }

    /// `messages.insert`: stores a message with `label_ids` (the date
    /// comes from its `Date` header), like IMAP `APPEND`.
    pub async fn insert_message(
        &self,
        raw: &[u8],
        label_ids: &[String],
        thread_id: Option<&str>,
    ) -> Result<Message, GmailError> {
        let mut metadata = json!({ "labelIds": label_ids });
        if let Some(thread) = thread_id {
            metadata["threadId"] = json!(thread);
        }
        self.upload(
            &["messages"],
            &[("internalDateSource", "dateHeader".to_owned())],
            metadata,
            raw,
            "users.messages.insert",
        )
        .await
    }

    /// `messages.send`: Gmail delivers `raw` to the recipients of its
    /// `To` / `Cc` / `Bcc` headers (and removes `Bcc`); `thread_id` files a
    /// reply into its thread.
    pub async fn send_message(
        &self,
        raw: &[u8],
        thread_id: Option<&str>,
    ) -> Result<Message, GmailError> {
        let mut metadata = json!({});
        if let Some(thread) = thread_id {
            metadata["threadId"] = json!(thread);
        }
        self.upload(
            &["messages", "send"],
            &[],
            metadata,
            raw,
            "users.messages.send",
        )
        .await
    }

    /// `drafts.create`: stores a draft (Gmail keeps drafts in the drafts
    /// API, not through `insert`).
    pub async fn create_draft(
        &self,
        raw: &[u8],
        thread_id: Option<&str>,
    ) -> Result<Draft, GmailError> {
        let mut message = json!({ "raw": encode_base64url(raw) });
        if let Some(thread) = thread_id {
            message["threadId"] = json!(thread);
        }
        self.json(
            Method::POST,
            &["drafts"],
            &[],
            Some(&json!({ "message": message })),
            false,
            "users.drafts.create",
        )
        .await
    }

    /// `messages.attachments.get`: an attachment's bytes (large attachments
    /// of `format=full` answers; `format=raw` already includes them).
    pub async fn attachment(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, GmailError> {
        let body: MessagePartBody = self
            .json(
                Method::GET,
                &["messages", message_id, "attachments", attachment_id],
                &[],
                None::<&()>,
                true,
                "users.messages.attachments.get",
            )
            .await?;
        decode_base64url(body.data.as_deref().unwrap_or_default())
    }

    /// The message with this `Message-ID` anywhere in the mailbox (Spam and
    /// Trash included), if any.
    pub async fn find_by_message_id(
        &self,
        message_id: &str,
    ) -> Result<Option<MessageRef>, GmailError> {
        let page = self
            .list_messages(&ListQuery {
                q: Some(message_id_query(message_id)),
                include_spam_trash: true,
                max_results: Some(1),
                ..ListQuery::default()
            })
            .await?;
        Ok(page.messages.into_iter().next())
    }

    /// Sends a message resource: as JSON `raw` when small, else as a
    /// multipart media upload. Not retried after server errors (it may
    /// have been stored).
    async fn upload(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
        mut metadata: serde_json::Value,
        raw: &[u8],
        what: &str,
    ) -> Result<Message, GmailError> {
        if raw.len() <= MAX_SIMPLE_UPLOAD {
            metadata["raw"] = json!(encode_base64url(raw));
            return self
                .json(Method::POST, segments, query, Some(&metadata), false, what)
                .await;
        }
        let boundary = boundary_for("nail_upload", raw);
        let mut body = Vec::with_capacity(raw.len());
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(metadata.to_string().as_bytes());
        body.extend_from_slice(
            format!("\r\n--{boundary}\r\nContent-Type: message/rfc822\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(raw);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let mut url = self.url(segments, query);
        let path = format!("/upload{}", url.path());
        url.set_path(&path);
        url.query_pairs_mut().append_pair("uploadType", "multipart");
        let call = Call {
            method: Method::POST,
            url,
            body: Body::Raw {
                content_type: format!("multipart/related; boundary={boundary}"),
                bytes: body,
            },
            headers: Vec::new(),
            idempotent: false,
            what: what.to_owned(),
        };
        let reply = self.execute(&call).await?;
        parse_json(&reply.body, what)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_params() {
        let query = ListQuery {
            q: Some(after_query(1_700_000_000)),
            label_ids: vec!["INBOX".into(), "UNREAD".into()],
            include_spam_trash: true,
            max_results: Some(9999),
            page_token: Some("p2".into()),
        };
        assert_eq!(
            query.params(),
            [
                ("q", "after:1700000000".to_owned()),
                ("labelIds", "INBOX".to_owned()),
                ("labelIds", "UNREAD".to_owned()),
                ("includeSpamTrash", "true".to_owned()),
                ("maxResults", "500".to_owned()),
                ("pageToken", "p2".to_owned()),
            ]
        );
        assert_eq!(ListQuery::default().params().len(), 0);
        assert_eq!(after_query(-5), "after:0");
    }

    #[test]
    fn message_id_terms() {
        assert_eq!(message_id_query("<a.b@c>"), "rfc822msgid:a.b@c");
        assert_eq!(message_id_query(" a@c "), "rfc822msgid:a@c");
    }

    #[test]
    fn boundaries_avoid_the_content() {
        let b = boundary_for("x", b"hello");
        assert!(b.starts_with("x_"));
        let content = format!("before {b} after");
        let other = boundary_for("x", content.as_bytes());
        assert!(!content.contains(&other));
    }
}
