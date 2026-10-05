//! Mail domain types as seen by clients.
//!
//! These are read models: the core builds them from the store and sends them
//! to the UI. Writes never go through these types directly; they are
//! expressed as [`crate::Request`]s and go through the core's operation queue.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::exchange::ExchangeMessageInfo;
use crate::gmail::GmailMessageInfo;
use crate::ids::{AccountId, FolderId, MessageId, ThreadId};
use crate::remote_content::RemoteContentInfo;
use crate::scheduling::Invitation;
use crate::security::{AuthResults, MessageWarning, SecurityStatus};
use crate::triage::JunkVerdict;

/// The protocol family behind an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AccountKind {
    /// IMAP for incoming mail (plus SMTP for sending, from M1).
    Imap,
    /// POP3 delivering into local folders (M1).
    Pop3,
    /// Gmail via the Gmail API (M3b; a Gmail address configured with
    /// `imap` stays an IMAP account).
    Gmail,
    /// Microsoft Exchange via Graph or EWS (M3c).
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "the IMAP system flags are independent booleans by definition"
)]
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
    /// The body is not stored locally (sync policy headers-only or recent
    /// N days, M2c); opening the message fetches it. Absent = `false`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub headers_only: bool,
    /// Gmail labels, thread id and category (M3b, Gmail accounts only);
    /// absent elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub gmail: Option<GmailMessageInfo>,
    /// Exchange item id, conversation, Focused Inbox and mailbox (M3c,
    /// Exchange accounts only); absent elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exchange: Option<ExchangeMessageInfo>,
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

/// One row of a threaded message list ([`crate::Request::ListThreads`]):
/// a conversation (P2, ADR 0148).
///
/// A conversation listed in a folder is the thread's messages in that folder
/// (or the member folders of a virtual folder) plus the user's own messages
/// of the thread from the same accounts' Sent folders, which carry
/// `from_me` ([`ConversationMessage`]); drafts never join one. In Drafts
/// every message is a conversation of its own (the Outbox is no folder).
/// Copies with the same Message-ID count once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadSummary {
    /// The thread.
    pub id: ThreadId,
    /// Subject of the thread's first message (without `Re:` / `Fwd:`
    /// prefixes); empty when missing.
    pub subject: String,
    /// Distinct senders of the conversation, oldest first, with "me"
    /// resolved from the identities of the messages' accounts and the
    /// senders of unread messages marked (P2: the row's first line,
    /// template field `{participants}`).
    pub participants: Vec<ThreadParticipant>,
    /// Messages of the conversation (the count pill; 1 = no pill).
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
    /// The message choosing the row selects and the row's actions act on:
    /// the newest one the user did not write, else the newest (P2: never the
    /// user's own reply joined from Sent).
    pub latest_message_id: MessageId,
    /// That message's summary (P2): what choosing the row selects, so the
    /// UI's actions see its flags, tags and account at once, without a
    /// second request.
    pub latest_message: Box<MessageSummary>,
    /// The sender of the newest message (P2: "Sender: preview" on the row's
    /// third line); `null` when it has no `From`. Absent from older cores.
    #[serde(default)]
    pub latest_from: Option<ThreadParticipant>,
    /// Accounts the messages come from (more than one only in virtual
    /// accounts).
    pub account_ids: Vec<AccountId>,
}

/// A sender in a conversation ([`ThreadSummary::participants`]): the
/// address plus what the row needs to show it (P2, ADR 0148). On the wire
/// the address fields sit beside `me` / `unread` (`{name, email, me,
/// unread}`), so older readers that expect an [`Address`] still work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadParticipant {
    /// The sender.
    #[serde(flatten)]
    pub address: Address,
    /// The address is one of the user's identities of the conversation's
    /// accounts (compared case-insensitively): the UI writes "me".
    #[serde(default)]
    pub me: bool,
    /// The participant sent at least one unread message of the
    /// conversation (shown bold).
    #[serde(default)]
    pub unread: bool,
}

impl ThreadParticipant {
    /// A participant that is neither the user nor unread.
    #[must_use]
    pub const fn new(address: Address) -> Self {
        Self {
            address,
            me: false,
            unread: false,
        }
    }
}

/// One message of a conversation ([`crate::Request::GetConversation`]):
/// the summary plus where it stands in the conversation (P2, ADR 0148). On
/// the wire the summary's fields sit beside `from_me`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConversationMessage {
    /// The message.
    #[serde(flatten)]
    pub message: MessageSummary,
    /// The user wrote it: it is in a Sent folder, or its sender is one of
    /// the identities of its account (the reader marks the card `from-me`).
    #[serde(default)]
    pub from_me: bool,
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
    /// The HTML body as the reader renders it, when it differs from
    /// `sanitized_html` (P1): inline (`cid:`) images embedded as `data:` URLs,
    /// because the reader frame can load nothing else. Only for the reader:
    /// quoting, MCP and plug-ins use `sanitized_html`, where inline images
    /// stay small `cid:` references. Absent = render `sanitized_html`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reader_html: Option<String>,
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
    /// Junk verdict (M2c): absent when the message was neither marked nor
    /// classified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub junk: Option<JunkVerdict>,
    /// The invitation card (M3b): the message carries an iTIP method
    /// (`text/calendar; method=…`); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub invitation: Option<Box<Invitation>>,
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

    /// P2 (ADR 0148): participants and conversation messages keep the
    /// shape older readers know, with the new flags beside it.
    #[test]
    fn conversation_types_flatten_their_address_and_summary() {
        let participant = ThreadParticipant {
            me: true,
            ..ThreadParticipant::new(Address {
                name: Some("Ada".into()),
                email: "ada@x".into(),
            })
        };
        let value = serde_json::to_value(&participant).unwrap();
        assert_eq!(
            value,
            json!({"name": "Ada", "email": "ada@x", "me": true, "unread": false})
        );
        // An old core's plain address still reads.
        let old: ThreadParticipant =
            serde_json::from_value(json!({"name": null, "email": "b@x"})).unwrap();
        assert_eq!(
            old,
            ThreadParticipant::new(Address {
                name: None,
                email: "b@x".into()
            })
        );
        let address: Address = serde_json::from_value(value).unwrap();
        assert_eq!(address.email, "ada@x");

        let message: ConversationMessage = serde_json::from_value(json!({
            "id": "m1", "folder_id": "f1", "account_id": "a1", "subject": "Hi",
            "from": null, "to": [], "date": "2026-10-05T08:00:00Z",
            "flags": {}, "has_attachments": false, "preview": "",
            "thread_id": "t1", "tags": [], "duplicates": [], "from_me": true
        }))
        .unwrap();
        assert!(message.from_me);
        assert_eq!(message.message.subject, "Hi");
        let back = serde_json::to_value(&message).unwrap();
        assert_eq!(back["from_me"], json!(true));
        assert_eq!(back["id"], json!("m1"));
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
            headers_only: false,
            gmail: None,
            exchange: None,
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
