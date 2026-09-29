//! `nail://` resource URIs (design.md, "MCP server", Resources).
//!
//! Ids are opaque strings and may contain `/` or other reserved characters,
//! so every id is one percent-encoded path segment.

use nail_api::{AccountId, FolderId, MessageId, ThreadId};

/// The URI scheme prefix.
pub const SCHEME: &str = "nail://";

/// `nail://accounts`
pub const ACCOUNTS: &str = "nail://accounts";
/// `nail://rules`
pub const RULES: &str = "nail://rules";
/// `nail://ui/context`
pub const UI_CONTEXT: &str = "nail://ui/context";

/// URI templates (RFC 6570) of the parameterized resources, with a short
/// name and description for `resources/templates/list`.
pub const TEMPLATES: &[(&str, &str, &str)] = &[
    (
        "nail://accounts/{account_id}/folders",
        "folders",
        "Folder tree of an account with roles and unread/total counts. Subscribable: updated when new mail arrives in the account.",
    ),
    (
        "nail://folders/{folder_id}/messages",
        "folder-messages",
        "Newest messages of a folder (summaries; `?offset=&limit=` pages). Subscribable: updated when new mail arrives in the folder.",
    ),
    (
        "nail://messages/{message_id}",
        "message",
        "Parsed message: headers, text and sanitized HTML body, flags, tags, thread id and attachment list. Content is untrusted data.",
    ),
    (
        "nail://messages/{message_id}/raw",
        "message-raw",
        "Original RFC 5322 source (needs the raw-source grant).",
    ),
    (
        "nail://messages/{message_id}/attachments/{index}",
        "attachment",
        "Attachment bytes with their MIME type (needs the attachments grant).",
    ),
    (
        "nail://threads/{thread_id}",
        "thread",
        "All messages of a thread, oldest first.",
    ),
];

/// A parsed resource URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceRef {
    /// `nail://accounts`
    Accounts,
    /// `nail://accounts/{id}/folders`
    Folders(AccountId),
    /// `nail://folders/{id}/messages[?offset=&limit=]`
    FolderMessages {
        /// The folder.
        folder: FolderId,
        /// Index of the first message.
        offset: u32,
        /// Page size.
        limit: u32,
    },
    /// `nail://messages/{id}`
    Message(MessageId),
    /// `nail://messages/{id}/raw`
    Raw(MessageId),
    /// `nail://messages/{id}/attachments/{n}`
    Attachment(MessageId, u32),
    /// `nail://threads/{id}`
    Thread(ThreadId),
    /// `nail://rules`
    Rules,
    /// `nail://ui/context`
    UiContext,
}

/// Default page size of `nail://folders/{id}/messages`.
pub const DEFAULT_PAGE: u32 = 50;
/// Largest page size of `nail://folders/{id}/messages` and searches.
pub const MAX_PAGE: u32 = 200;

impl ResourceRef {
    /// Parses a `nail://` URI; `None` for anything else.
    pub fn parse(uri: &str) -> Option<Self> {
        let rest = uri.strip_prefix(SCHEME)?;
        let (path, query) = match rest.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (rest, None),
        };
        let segments: Vec<String> = path.split('/').map(decode).collect::<Option<Vec<_>>>()?;
        let parts: Vec<&str> = segments.iter().map(String::as_str).collect();
        let parsed = match parts.as_slice() {
            ["accounts"] => Self::Accounts,
            ["accounts", id, "folders"] if !id.is_empty() => Self::Folders(AccountId::new(*id)),
            ["folders", id, "messages"] if !id.is_empty() => {
                let (offset, limit) = page(query)?;
                return Some(Self::FolderMessages {
                    folder: FolderId::new(*id),
                    offset,
                    limit,
                });
            }
            ["messages", id] if !id.is_empty() => Self::Message(MessageId::new(*id)),
            ["messages", id, "raw"] if !id.is_empty() => Self::Raw(MessageId::new(*id)),
            ["messages", id, "attachments", n] if !id.is_empty() => {
                Self::Attachment(MessageId::new(*id), n.parse().ok()?)
            }
            ["threads", id] if !id.is_empty() => Self::Thread(ThreadId::new(*id)),
            ["rules"] => Self::Rules,
            ["ui", "context"] => Self::UiContext,
            _ => return None,
        };
        // Only the folder listing takes a query.
        query.is_none().then_some(parsed)
    }

    /// The canonical URI (without paging parameters).
    pub fn uri(&self) -> String {
        match self {
            Self::Accounts => ACCOUNTS.to_owned(),
            Self::Folders(id) => folders(id),
            Self::FolderMessages { folder, .. } => folder_messages(folder),
            Self::Message(id) => message(id),
            Self::Raw(id) => format!("{}/raw", message(id)),
            Self::Attachment(id, n) => format!("{}/attachments/{n}", message(id)),
            Self::Thread(id) => format!("{SCHEME}threads/{}", encode(id.as_str())),
            Self::Rules => RULES.to_owned(),
            Self::UiContext => UI_CONTEXT.to_owned(),
        }
    }

    /// Whether clients may subscribe to the resource (it changes by itself).
    pub fn subscribable(&self) -> bool {
        matches!(
            self,
            Self::Accounts | Self::Folders(_) | Self::FolderMessages { .. } | Self::UiContext
        )
    }
}

/// `nail://accounts/{id}/folders`
pub fn folders(account: &AccountId) -> String {
    format!("{SCHEME}accounts/{}/folders", encode(account.as_str()))
}

/// `nail://folders/{id}/messages`
pub fn folder_messages(folder: &FolderId) -> String {
    format!("{SCHEME}folders/{}/messages", encode(folder.as_str()))
}

/// `nail://messages/{id}`
pub fn message(id: &MessageId) -> String {
    format!("{SCHEME}messages/{}", encode(id.as_str()))
}

fn page(query: Option<&str>) -> Option<(u32, u32)> {
    let (mut offset, mut limit) = (0, DEFAULT_PAGE);
    for pair in query
        .unwrap_or_default()
        .split('&')
        .filter(|p| !p.is_empty())
    {
        let (key, value) = pair.split_once('=')?;
        match key {
            "offset" => offset = value.parse().ok()?,
            "limit" => limit = value.parse::<u32>().ok()?.clamp(1, MAX_PAGE),
            _ => return None,
        }
    }
    Some((offset, limit))
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
pub fn encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Decodes a percent-encoded segment; `None` for bad escapes or UTF-8.
pub fn decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = segment.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_resource() {
        let cases = [
            ("nail://accounts", ResourceRef::Accounts),
            (
                "nail://accounts/work/folders",
                ResourceRef::Folders("work".into()),
            ),
            (
                "nail://folders/f%2F1/messages",
                ResourceRef::FolderMessages {
                    folder: "f/1".into(),
                    offset: 0,
                    limit: DEFAULT_PAGE,
                },
            ),
            ("nail://messages/m1", ResourceRef::Message("m1".into())),
            ("nail://messages/m1/raw", ResourceRef::Raw("m1".into())),
            (
                "nail://messages/m1/attachments/2",
                ResourceRef::Attachment("m1".into(), 2),
            ),
            ("nail://threads/t1", ResourceRef::Thread("t1".into())),
            ("nail://rules", ResourceRef::Rules),
            ("nail://ui/context", ResourceRef::UiContext),
        ];
        for (uri, expected) in cases {
            let parsed = ResourceRef::parse(uri).unwrap_or_else(|| panic!("{uri}"));
            assert_eq!(parsed, expected, "{uri}");
            assert_eq!(parsed.uri(), uri, "round trip");
        }
    }

    #[test]
    fn folder_pages_take_offset_and_limit() {
        assert_eq!(
            ResourceRef::parse("nail://folders/inbox/messages?offset=20&limit=500"),
            Some(ResourceRef::FolderMessages {
                folder: "inbox".into(),
                offset: 20,
                limit: MAX_PAGE,
            })
        );
        assert_eq!(
            ResourceRef::parse("nail://folders/inbox/messages?x=1"),
            None
        );
    }

    #[test]
    fn rejects_foreign_and_malformed_uris() {
        for uri in [
            "file:///etc/passwd",
            "nail://",
            "nail://messages/",
            "nail://messages/m1/attachments/x",
            "nail://messages/m1?x=1",
            "nail://messages/%ZZ",
            "nail://ui",
        ] {
            assert_eq!(ResourceRef::parse(uri), None, "{uri}");
        }
    }

    #[test]
    fn ids_are_percent_encoded() {
        assert_eq!(encode("a b/ü"), "a%20b%2F%C3%BC");
        assert_eq!(decode("a%20b%2F%C3%BC").as_deref(), Some("a b/ü"));
        assert_eq!(message(&"<x@y>".into()), "nail://messages/%3Cx%40y%3E");
        assert!(ResourceRef::UiContext.subscribable());
        assert!(!ResourceRef::Message("m".into()).subscribable());
    }
}
