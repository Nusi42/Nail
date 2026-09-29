//! Mail domain types as seen by clients.
//!
//! These are read models: the core builds them from the store and sends them
//! to the UI. Writes never go through these types directly; they are
//! expressed as [`crate::Request`]s and go through the core's operation queue.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{AccountId, FolderId, MessageId, ThreadId};
use crate::remote_content::RemoteContentInfo;
use crate::security::{AuthResults, MessageWarning, SecurityStatus};

/// The protocol family behind an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AccountKind {
    /// IMAP for incoming mail (plus SMTP for sending, from M1).
    Imap,
    /// POP3 delivering into local folders (M1).
    Pop3,
    /// Gmail via the Gmail API (M3; in M1 Gmail is an IMAP account).
    Gmail,
    /// Microsoft Exchange via Graph or EWS (M3).
    Exchange,
    /// Local folders only, no server.
    Local,
    /// A virtual account merging several real accounts (M1).
    Virtual,
}

/// A mail account as listed by [`crate::Request::ListAccounts`]: every
/// real account, every user-defined virtual account and the built-in "All
/// accounts" virtual account ([`AccountId::ALL`]). The configuration behind
/// it is [`crate::accounts::AccountConfig`] /
/// [`crate::accounts::VirtualAccountConfig`].
///
/// The built-in "All accounts" account has an empty `name` and `email`; the
/// UI shows the Fluent message `account-all` for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Account {
    /// Opaque account id.
    pub id: AccountId,
    /// Display name chosen by the user, e.g. "Work".
    pub name: String,
    /// Primary email address of the account.
    pub email: String,
    /// Protocol family.
    pub kind: AccountKind,
}

/// The special-use role of a folder (RFC 6154 SPECIAL-USE or name heuristics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum FolderRole {
    /// The inbox.
    Inbox,
    /// Sent mail.
    Sent,
    /// Drafts.
    Drafts,
    /// Archive.
    Archive,
    /// Junk / spam.
    Junk,
    /// Trash / deleted items.
    Trash,
    /// Any folder without a special role.
    #[default]
    Other,
}

/// A folder (mailbox) of an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Folder {
    /// Opaque folder id.
    pub id: FolderId,
    /// Owning account.
    pub account_id: AccountId,
    /// Parent folder in the tree, `null` for top-level folders.
    #[serde(default)]
    pub parent_id: Option<FolderId>,
    /// Display name (last path segment, decoded), e.g. "Receipts".
    pub name: String,
    /// Full server path, e.g. "INBOX/Receipts". Unique within the account.
    pub path: String,
    /// Special-use role.
    #[serde(default)]
    pub role: FolderRole,
    /// Number of unread messages.
    pub unread: u32,
    /// Total number of messages.
    pub total: u32,
}

/// A mail address with an optional display name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct Address {
    /// Display name, e.g. "Ada Lovelace". `null` when the header has none.
    #[serde(default)]
    pub name: Option<String>,
    /// The address itself, e.g. "ada@example.org".
    pub email: String,
}

/// Standard IMAP system flags of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Flags {
    /// `\Seen`: the message has been read.
    pub seen: bool,
    /// `\Flagged`: starred / flagged for follow-up.
    pub flagged: bool,
    /// `\Answered`: a reply was sent.
    pub answered: bool,
    /// `\Draft`: the message is a draft.
    pub draft: bool,
}

/// One row of a message list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageSummary {
    /// Opaque message id.
    pub id: MessageId,
    /// Folder the message lives in.
    pub folder_id: FolderId,
    /// Account the message belongs to (its source account).
    pub account_id: AccountId,
    /// Decoded subject; empty string when missing.
    pub subject: String,
    /// First `From` address; `null` for malformed mail without one.
    pub from: Option<Address>,
    /// `To` recipients.
    pub to: Vec<Address>,
    /// Message date as RFC 3339 (e.g. "2026-09-29T08:15:00+02:00"). Taken from
    /// the `Date` header, falling back to the server's internal date.
    pub date: String,
    /// System flags.
    pub flags: Flags,
    /// Whether the message has at least one non-inline attachment.
    pub has_attachments: bool,
    /// A short plain-text preview of the body (at most ~200 characters,
    /// whitespace collapsed); empty when not yet available.
    pub preview: String,
    /// The thread the message belongs to; `null` until threading ran.
    #[serde(default)]
    pub thread_id: Option<ThreadId>,
    /// Tags (IMAP keywords such as `$Label1` or `work`), sorted.
    #[serde(default)]
    pub tags: Vec<String>,
    /// In a virtual account: the same message (same `Message-ID`) in other
    /// member accounts, collapsed into this row. Empty elsewhere.
    #[serde(default)]
    pub duplicates: Vec<MessageLocation>,
}

/// Where a copy of a message lives.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct MessageLocation {
    /// The copy's message id.
    pub message_id: MessageId,
    /// Its account.
    pub account_id: AccountId,
    /// Its folder.
    pub folder_id: FolderId,
}

/// One row of a threaded message list ([`crate::Request::ListThreads`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadSummary {
    /// The thread.
    pub id: ThreadId,
    /// Subject of the thread's first message (without `Re:` / `Fwd:`
    /// prefixes); empty when missing.
    pub subject: String,
    /// Distinct senders, oldest first.
    pub participants: Vec<Address>,
    /// Messages of the thread in the listed folder (or virtual folder).
    pub message_count: u32,
    /// Unread messages among them.
    pub unread_count: u32,
    /// Whether any of them is flagged.
    pub flagged: bool,
    /// Whether any of them has attachments.
    pub has_attachments: bool,
    /// Date of the newest of them (RFC 3339).
    pub date: String,
    /// Preview of the newest of them.
    pub preview: String,
    /// The newest of them.
    pub latest_message_id: MessageId,
    /// Accounts the messages come from (more than one only in virtual
    /// accounts).
    pub account_ids: Vec<AccountId>,
}

/// The header subset shown in the message view.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct MessageHeaders {
    /// Decoded subject.
    pub subject: String,
    /// `From` addresses (usually one).
    pub from: Vec<Address>,
    /// `To` recipients.
    pub to: Vec<Address>,
    /// `Cc` recipients.
    pub cc: Vec<Address>,
    /// `Reply-To` addresses.
    pub reply_to: Vec<Address>,
    /// Date as RFC 3339, `null` when unknown.
    pub date: Option<String>,
    /// `Message-ID` without angle brackets, `null` when missing.
    pub message_id: Option<String>,
}

/// Metadata of one attachment. The bytes are fetched separately (blob
/// endpoint / later request), addressed by message id and [`Self::index`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AttachmentInfo {
    /// Zero-based index among the message's attachments.
    pub index: u32,
    /// Decoded file name, `null` when the part has none.
    pub filename: Option<String>,
    /// MIME type, e.g. "application/pdf".
    pub mime: String,
    /// Decoded size in bytes.
    pub size: u64,
    /// Whether the part is inline (e.g. a `cid:` image) rather than a download.
    #[serde(default)]
    pub inline: bool,
}

/// A fully parsed message for the reader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageBody {
    /// Opaque message id.
    pub id: MessageId,
    /// Header subset for display.
    pub headers: MessageHeaders,
    /// Plain-text body. Derived from the HTML part when the message has no
    /// text part; empty when the message has no body at all.
    pub text: String,
    /// Sanitized HTML body (scripts, forms, event handlers and remote
    /// resources removed), `null` for text-only mail. Must only ever be
    /// rendered inside the sandboxed reader iframe.
    pub sanitized_html: Option<String>,
    /// Attachments in MIME order.
    pub attachments: Vec<AttachmentInfo>,
    /// `true` when the sanitizer removed or neutralized remote content
    /// (images, fonts, CSS) that the user could choose to load.
    pub remote_content_blocked: bool,
    /// End-to-end security (M2a): signatures, encryption, header
    /// protection. Absent for mail that is neither signed nor encrypted. For encrypted mail that was decrypted, `text`,
    /// `sanitized_html` and `attachments` are the decrypted content, which
    /// the core never stores unless decrypted indexing is enabled
    /// ([`crate::security::SecuritySettings::index_decrypted`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub security: Option<SecurityStatus>,
    /// Sender authentication from a trusted `Authentication-Results`
    /// header (M2a); absent when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auth_results: Option<AuthResults>,
    /// Link and attachment warnings for the reader (M2a); absent = none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<MessageWarning>>", optional)]
    pub warnings: Vec<MessageWarning>,
    /// Remote content (M2b): how many remote resources the message has and
    /// whether they are loaded through the proxy (then `sanitized_html`
    /// references them as `nail-proxy:<token>`); absent when it has none.
    /// `remote_content_blocked` stays `true` while they are blocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub remote_content: Option<RemoteContentInfo>,
    /// Where the browser UI loads the HTML body from (M2b, remote UI
    /// only): `<sandbox origin>/render/<token>`, a separate origin that
    /// serves nothing but mail documents and the proxy
    /// ([`crate::remote_content::RENDER_PATH`]). The reader frames it
    /// instead of `sanitized_html`; absent = render `sanitized_html`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub render_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn folder_role_and_kind_use_lowercase_names() {
        assert_eq!(
            serde_json::to_value(FolderRole::Inbox).unwrap(),
            json!("inbox")
        );
        assert_eq!(
            serde_json::to_value(AccountKind::Pop3).unwrap(),
            json!("pop3")
        );
    }

    #[test]
    fn folder_defaults_missing_role_and_parent() {
        let folder: Folder = serde_json::from_value(json!({
            "id": "f1", "account_id": "a1", "name": "Receipts",
            "path": "INBOX/Receipts", "unread": 2, "total": 10
        }))
        .unwrap();
        assert_eq!(folder.role, FolderRole::Other);
        assert_eq!(folder.parent_id, None);
    }

    #[test]
    fn summary_round_trips() {
        let summary = MessageSummary {
            id: "m1".into(),
            folder_id: "f1".into(),
            account_id: "a1".into(),
            subject: "Hello".into(),
            from: Some(Address {
                name: Some("Ada".into()),
                email: "ada@example.org".into(),
            }),
            to: vec![Address {
                name: None,
                email: "bob@example.org".into(),
            }],
            date: "2026-09-29T08:15:00+02:00".into(),
            flags: Flags {
                seen: true,
                ..Flags::default()
            },
            has_attachments: false,
            preview: "Hi Bob".into(),
            thread_id: Some("t1".into()),
            tags: vec!["work".into()],
            duplicates: vec![],
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(
            json["flags"],
            json!({"seen": true, "flagged": false, "answered": false, "draft": false})
        );
        assert_eq!(json["thread_id"], json!("t1"));
        let back: MessageSummary = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back, summary);

        // Rows written before M1a (no thread, tags or duplicates) still parse.
        let mut old = json;
        let obj = old.as_object_mut().unwrap();
        obj.remove("thread_id");
        obj.remove("tags");
        obj.remove("duplicates");
        let back: MessageSummary = serde_json::from_value(old).unwrap();
        assert_eq!(back.thread_id, None);
        assert!(back.tags.is_empty() && back.duplicates.is_empty());
    }
}
