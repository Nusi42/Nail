//! The Gmail API's JSON resources (`users.messages`, `users.history`,
//! `users.labels`, `users.threads`, errors), as the server sends them.
//!
//! Gmail writes 64-bit numbers (`historyId`, `internalDate`) as strings;
//! the fields here accept a number or a string and keep Gmail's form when
//! written. Unknown fields are ignored, missing ones take defaults, so
//! answers with `fields=` masks parse too.

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::GmailError;

/// `users.getProfile`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    /// The mailbox's address.
    pub email_address: String,
    /// Messages in the mailbox.
    pub messages_total: u64,
    /// Threads in the mailbox.
    pub threads_total: u64,
    /// The mailbox's current history id.
    #[serde(deserialize_with = "string_or_number")]
    pub history_id: String,
}

/// A message (`users.messages` resource).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Message {
    /// Message id.
    pub id: String,
    /// Thread id.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub thread_id: String,
    /// Label ids.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub label_ids: Vec<String>,
    /// A short part of the text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// The last history record that changed the message.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_string_or_number"
    )]
    pub history_id: Option<String>,
    /// Receive time, milliseconds since the epoch.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_string_or_number"
    )]
    pub internal_date: Option<String>,
    /// Estimated size in bytes.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "opt_u64")]
    pub size_estimate: Option<u64>,
    /// The whole message, base64url (`format=raw`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// The parsed structure (`format=full` / `metadata`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<MessagePart>,
}

impl Message {
    /// The receive time in Unix seconds.
    pub fn internal_date_secs(&self) -> Option<i64> {
        self.internal_date
            .as_deref()
            .and_then(|d| d.parse::<i64>().ok())
            .map(|ms| ms.div_euclid(1000))
    }

    /// The decoded raw message (`format=raw`).
    pub fn raw_bytes(&self) -> Result<Option<Vec<u8>>, GmailError> {
        self.raw.as_deref().map(decode_base64url).transpose()
    }

    /// The value of the first header `name` of the payload (`format=metadata`).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.payload.as_ref()?.headers.iter().find_map(|h| {
            h.name
                .eq_ignore_ascii_case(name)
                .then_some(h.value.as_str())
        })
    }
}

/// A MIME part of a message (`format=full` / `metadata`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MessagePart {
    /// Part id (`0`, `0.1`, …).
    pub part_id: String,
    /// MIME type.
    pub mime_type: String,
    /// File name of an attachment.
    pub filename: String,
    /// Headers.
    pub headers: Vec<Header>,
    /// Body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<MessagePartBody>,
    /// Child parts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<MessagePart>,
}

/// A header.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Header {
    /// Name.
    pub name: String,
    /// Value.
    pub value: String,
}

/// The body of a part, or an attachment (`users.messages.attachments`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MessagePartBody {
    /// The attachment's id, when the data is fetched separately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<String>,
    /// Size in bytes.
    pub size: u64,
    /// Data, base64url.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

/// An id pair in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MessageRef {
    /// Message id.
    pub id: String,
    /// Thread id.
    pub thread_id: String,
}

/// `users.messages.list`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListMessagesResponse {
    /// The page's messages (ids only), newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<MessageRef>,
    /// The next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Estimated total.
    #[serde(deserialize_with = "opt_u64")]
    pub result_size_estimate: Option<u64>,
}

/// A message inside a history record.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryMessage {
    /// The message (id, thread id, its labels after the change).
    pub message: Message,
}

/// A label change inside a history record.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryLabels {
    /// The message (id, thread id, its labels after the change).
    pub message: Message,
    /// The labels added or removed.
    pub label_ids: Vec<String>,
}

/// One history record (`users.history`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct History {
    /// The record's id (increasing).
    #[serde(deserialize_with = "string_or_number")]
    pub id: String,
    /// Messages the record touched.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
    /// Messages added to the mailbox.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages_added: Vec<HistoryMessage>,
    /// Messages deleted permanently.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages_deleted: Vec<HistoryMessage>,
    /// Labels added to messages.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels_added: Vec<HistoryLabels>,
    /// Labels removed from messages.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels_removed: Vec<HistoryLabels>,
}

/// `users.history.list`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListHistoryResponse {
    /// Records after the start id, oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<History>,
    /// The next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// The mailbox's current history id.
    #[serde(deserialize_with = "string_or_number")]
    pub history_id: String,
}

/// A label colour as Gmail writes it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LabelColor {
    /// Text colour `#rrggbb`.
    pub text_color: String,
    /// Background colour `#rrggbb`.
    pub background_color: String,
}

/// A label (`users.labels` resource).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Label {
    /// Label id (`INBOX`, `Label_12`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// Name.
    pub name: String,
    /// `system` or `user`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// `show` / `hide`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_list_visibility: Option<String>,
    /// `labelShow` / `labelShowIfUnread` / `labelHide`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_list_visibility: Option<String>,
    /// Colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<LabelColor>,
    /// Messages with the label (`labels.get` only).
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "opt_u64")]
    pub messages_total: Option<u64>,
    /// Unread messages with the label (`labels.get` only).
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "opt_u64")]
    pub messages_unread: Option<u64>,
    /// Threads with the label (`labels.get` only).
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "opt_u64")]
    pub threads_total: Option<u64>,
    /// Unread threads with the label (`labels.get` only).
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "opt_u64")]
    pub threads_unread: Option<u64>,
}

impl Label {
    /// Whether this is a system label.
    pub fn is_system(&self) -> bool {
        self.kind.as_deref() == Some("system")
    }
}

/// `users.labels.list`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ListLabelsResponse {
    /// Every label.
    pub labels: Vec<Label>,
}

/// A thread (`users.threads` resource).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Thread {
    /// Thread id.
    pub id: String,
    /// A short part of the newest message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// The thread's history id.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_string_or_number"
    )]
    pub history_id: Option<String>,
    /// Its messages, oldest first (`threads.get`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
}

/// `users.threads.list`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListThreadsResponse {
    /// The page's threads.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub threads: Vec<Thread>,
    /// The next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Estimated total.
    #[serde(deserialize_with = "opt_u64")]
    pub result_size_estimate: Option<u64>,
}

/// `users.messages.modify` / `threads.modify`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModifyRequest {
    /// Label ids to add.
    pub add_label_ids: Vec<String>,
    /// Label ids to remove.
    pub remove_label_ids: Vec<String>,
}

/// `users.messages.batchModify`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BatchModifyRequest {
    /// Message ids (at most 1000).
    pub ids: Vec<String>,
    /// Label ids to add.
    pub add_label_ids: Vec<String>,
    /// Label ids to remove.
    pub remove_label_ids: Vec<String>,
}

/// A draft (`users.drafts` resource).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    /// Draft id.
    pub id: String,
    /// The draft's message.
    pub message: Message,
}

/// Gmail's error answer (`{"error": {...}}`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorBody {
    /// The error.
    pub error: ErrorDetail,
}

impl ErrorBody {
    /// The first error's reason (`notFound`, `rateLimitExceeded`, …), or
    /// the status (`NOT_FOUND`, `PERMISSION_DENIED`) when there is none.
    pub fn reason(&self) -> Option<&str> {
        self.error
            .errors
            .iter()
            .find_map(|e| (!e.reason.is_empty()).then_some(e.reason.as_str()))
            .or(self.error.details.iter().find_map(|d| d.reason.as_deref()))
            .or(self.error.status.as_deref())
    }

    /// An error body like Gmail's.
    pub fn new(code: u16, status: &str, reason: &str, message: &str) -> Self {
        Self {
            error: ErrorDetail {
                code,
                message: message.to_owned(),
                status: Some(status.to_owned()),
                errors: vec![ErrorItem {
                    domain: "global".to_owned(),
                    reason: reason.to_owned(),
                    message: message.to_owned(),
                }],
                details: Vec::new(),
            },
        }
    }
}

/// The inside of [`ErrorBody`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorDetail {
    /// HTTP status.
    pub code: u16,
    /// English text.
    pub message: String,
    /// Google status name (`NOT_FOUND`, `RESOURCE_EXHAUSTED`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Legacy error items with the reason.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ErrorItem>,
    /// Newer error details (`google.rpc.ErrorInfo` with a reason).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ErrorInfo>,
}

/// One legacy error item.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorItem {
    /// `global`, `usageLimits`.
    pub domain: String,
    /// `notFound`, `rateLimitExceeded`, `insufficientPermissions`, …
    pub reason: String,
    /// English text.
    pub message: String,
}

/// A `google.rpc.ErrorInfo` detail.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorInfo {
    /// `ACCESS_TOKEN_SCOPE_INSUFFICIENT`, `RATE_LIMIT_EXCEEDED`, …
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Decodes Gmail's base64url (padding optional; tolerant of the standard
/// alphabet and line breaks, which some proxies add).
pub fn decode_base64url(text: &str) -> Result<Vec<u8>, GmailError> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            c => c,
        })
        .collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD_INDIFFERENT
        .decode(cleaned.as_bytes())
        .map_err(|e| GmailError::Protocol(format!("invalid base64url: {e}")))
}

/// Encodes bytes as base64url without padding (what Gmail accepts for
/// `raw`).
pub fn encode_base64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrNumber {
    String(String),
    Number(serde_json::Number),
}

impl StringOrNumber {
    fn into_string(self) -> String {
        match self {
            Self::String(s) => s,
            Self::Number(n) => n.to_string(),
        }
    }
}

fn string_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<StringOrNumber>::deserialize(d)?
        .map(StringOrNumber::into_string)
        .unwrap_or_default())
}

fn opt_string_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<StringOrNumber>::deserialize(d)?.map(StringOrNumber::into_string))
}

fn opt_u64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    match Option::<StringOrNumber>::deserialize(d)? {
        None => Ok(None),
        Some(v) => v
            .into_string()
            .parse()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_as_strings_or_numbers() {
        let m: Message = serde_json::from_value(json!({
            "id": "a", "threadId": "t", "historyId": 42, "internalDate": "1790856000123",
            "sizeEstimate": "2048"
        }))
        .unwrap();
        assert_eq!(m.history_id.as_deref(), Some("42"));
        assert_eq!(m.internal_date_secs(), Some(1_790_856_000));
        assert_eq!(m.size_estimate, Some(2048));
        let out = serde_json::to_value(&m).unwrap();
        assert_eq!(out["internalDate"], "1790856000123");
        assert!(out.get("raw").is_none());
    }

    #[test]
    fn base64url_round_trip_and_tolerance() {
        let raw = b"Subject: ?>?\r\n\r\n\xff\xfe";
        let encoded = encode_base64url(raw);
        assert_eq!(decode_base64url(&encoded).unwrap(), raw);
        // Padded, standard alphabet, line breaks.
        let std = base64::engine::general_purpose::STANDARD.encode(raw);
        let wrapped = format!("{}\r\n{}", &std[..8], &std[8..]);
        assert_eq!(decode_base64url(&wrapped).unwrap(), raw);
        assert!(matches!(
            decode_base64url("!!!"),
            Err(GmailError::Protocol(_))
        ));
    }

    #[test]
    fn error_reasons() {
        let body: ErrorBody = serde_json::from_value(json!({
            "error": {"code": 403, "message": "Request had insufficient authentication scopes.",
                      "status": "PERMISSION_DENIED",
                      "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo",
                                   "reason": "ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}
        }))
        .unwrap();
        assert_eq!(body.reason(), Some("ACCESS_TOKEN_SCOPE_INSUFFICIENT"));
        let body = ErrorBody::new(
            404,
            "NOT_FOUND",
            "notFound",
            "Requested entity was not found.",
        );
        assert_eq!(body.reason(), Some("notFound"));
        let bare: ErrorBody =
            serde_json::from_value(json!({"error": {"code": 500, "status": "INTERNAL"}})).unwrap();
        assert_eq!(bare.reason(), Some("INTERNAL"));
    }

    #[test]
    fn metadata_headers() {
        let m: Message = serde_json::from_value(json!({
            "id": "a", "payload": {"headers": [{"name": "Message-ID", "value": "<x@y>"}]}
        }))
        .unwrap();
        assert_eq!(m.header("message-id"), Some("<x@y>"));
        assert_eq!(m.header("subject"), None);
    }
}
