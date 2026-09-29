//! Drafts and the outbox.
//!
//! M1a composes plain text only. The HTML / Markdown bodies and attachments
//! are part of the contract already so the M1b composer can fill them in
//! without breaking clients; in M1a the core rejects a draft that sets them
//! with `unsupported`.
//!
//! Sending: [`crate::Request::SendDraft`] moves a saved draft into the
//! outbox with a delay (undo send) or a time (send later). Until the entry
//! starts sending, [`crate::Request::CancelSend`] turns it back into a
//! draft. Progress is pushed as [`crate::Event::SendStatus`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::accounts::ComposerMode;
use crate::domain::Address;
use crate::error::ErrorCode;
use crate::ids::{AccountId, DraftId, IdentityId, MessageId, OutboxId};

/// A message being written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Draft {
    /// `null` for a new draft; [`crate::Request::SaveDraft`] answers with
    /// the id to use from then on.
    #[serde(default)]
    pub id: Option<DraftId>,
    /// The identity to send from (it decides the account and `From`).
    pub identity_id: IdentityId,
    /// `To` recipients.
    #[serde(default)]
    pub to: Vec<Address>,
    /// `Cc` recipients.
    #[serde(default)]
    pub cc: Vec<Address>,
    /// `Bcc` recipients.
    #[serde(default)]
    pub bcc: Vec<Address>,
    /// Subject.
    #[serde(default)]
    pub subject: String,
    /// Which body is the source the user edits. M1a: always `plain`.
    #[serde(default)]
    pub mode: ComposerMode,
    /// Plain-text body (the source in `plain` mode; in the other modes the
    /// core derives it when sending).
    #[serde(default)]
    pub text: String,
    /// HTML body (M1b; must be `null` in M1a).
    #[serde(default)]
    pub html: Option<String>,
    /// Markdown source (M1b; must be `null` in M1a).
    #[serde(default)]
    pub markdown: Option<String>,
    /// Attachments (M1b; must be empty in M1a).
    #[serde(default)]
    pub attachments: Vec<DraftAttachment>,
    /// The message this one replies to: sets `In-Reply-To` / `References`
    /// and marks the original `\Answered` once sent. `null` for a new
    /// message.
    #[serde(default)]
    pub in_reply_to: Option<MessageId>,
}

impl Draft {
    /// Whether the draft uses anything beyond plain text (not supported
    /// before M1b).
    pub fn uses_rich_content(&self) -> bool {
        self.mode != ComposerMode::Plain
            || self.html.is_some()
            || self.markdown.is_some()
            || !self.attachments.is_empty()
    }
}

/// An attachment of a draft (reserved for M1b).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DraftAttachment {
    /// File name.
    pub filename: String,
    /// MIME type.
    pub mime: String,
    /// Size in bytes.
    pub size: u64,
    /// Hash of the content in the core's blob store.
    pub blob: String,
    /// `Content-ID` for inline images referenced as `cid:`; `null` for a
    /// regular attachment.
    #[serde(default)]
    pub content_id: Option<String>,
}

/// State of an outbox entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum OutboxState {
    /// Waiting for its send time (undo-send delay or send later); can be
    /// cancelled.
    Scheduled,
    /// Being submitted; can no longer be cancelled.
    Sending,
    /// Accepted by the server (and copied to Sent where needed).
    Sent,
    /// Submission failed; see the error. Retried automatically when the
    /// error is temporary (offline).
    Failed,
    /// Cancelled; the message is a draft again.
    Cancelled,
}

/// A message in the outbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OutboxEntry {
    /// The entry.
    pub id: OutboxId,
    /// The draft it was created from (restored on cancel).
    pub draft_id: DraftId,
    /// The account it is sent through.
    pub account_id: AccountId,
    /// The sending identity.
    pub identity_id: IdentityId,
    /// Subject, for display.
    pub subject: String,
    /// All recipients (`To`, `Cc`, `Bcc`), for display.
    pub recipients: Vec<Address>,
    /// When it is (or was) due to be sent (RFC 3339).
    pub send_at: String,
    /// Current state.
    pub state: OutboxState,
    /// Submission attempts so far.
    pub attempts: u32,
    /// Error class of the last failed attempt, else `null`.
    pub error: Option<ErrorCode>,
    /// Diagnostic detail of the last failed attempt, else `null`.
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_draft_parses_as_plain_text() {
        let draft: Draft = serde_json::from_value(json!({
            "identity_id": "ada@example.org",
            "to": [{"name": null, "email": "bob@example.org"}],
            "subject": "Hi",
            "text": "Hello Bob"
        }))
        .unwrap();
        assert_eq!(draft.id, None);
        assert_eq!(draft.mode, ComposerMode::Plain);
        assert!(!draft.uses_rich_content());
        let rich = Draft {
            html: Some("<p>x</p>".into()),
            ..draft
        };
        assert!(rich.uses_rich_content());
        assert_eq!(
            serde_json::to_value(OutboxState::Scheduled).unwrap(),
            json!("scheduled")
        );
    }
}
