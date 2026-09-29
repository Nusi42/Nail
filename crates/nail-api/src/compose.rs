//! Drafts, the outbox and the composer's helper requests.
//!
//! A [`Draft`] is plain text, HTML or Markdown ([`ComposerMode`]); the
//! source the user edits is `text`, `html` or `markdown` by mode, and the
//! core derives the other parts when sending (M1b, ADR 0022):
//!
//! * `plain`: `text/plain; format=flowed`.
//! * `html`: `multipart/alternative` of a generated `text/plain` and the
//!   HTML with the identity's mail stylesheet inlined; inline images
//!   ([`DraftAttachment::content_id`]) as `multipart/related`.
//! * `markdown`: the Markdown source as `text/plain` plus the HTML the core
//!   renders from it (the same renderer as [`crate::Request::RenderPreview`]);
//!   optionally a `text/markdown` part ([`Draft::markdown_part`]).
//!
//! Attachments are blobs in the core's blob store, uploaded with
//! [`crate::Request::UploadAttachment`]. Replies and forwards carry a
//! [`DraftContext`]; [`crate::Request::NewDraft`] builds a draft with the
//! quote, recipients, subject, signature and forwarded attachments already
//! filled in.
//!
//! Sending: [`crate::Request::SendDraft`] moves a saved draft into the
//! outbox with a delay (undo send) or a time (send later). Until the entry
//! starts sending, [`crate::Request::CancelSend`] turns it back into a
//! draft. Progress is pushed as [`crate::Event::SendStatus`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use crate::accounts::ComposerMode;
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
    /// Which body is the source the user edits.
    #[serde(default)]
    pub mode: ComposerMode,
    /// Plain-text body: the source in `plain` mode. In the other modes the
    /// core derives the text part when sending and ignores this value.
    #[serde(default)]
    pub text: String,
    /// HTML body: the source in `html` mode (sanitized by the core on save
    /// and on send). `null` in the other modes.
    #[serde(default)]
    pub html: Option<String>,
    /// Markdown source: the source in `markdown` mode. `null` in the other
    /// modes.
    #[serde(default)]
    pub markdown: Option<String>,
    /// Attachments and inline images, in display order.
    #[serde(default)]
    pub attachments: Vec<DraftAttachment>,
    /// The message this draft replies to or forwards. A reply sets
    /// `In-Reply-To` / `References` and marks the original `\Answered` once
    /// sent; a forward marks it `$Forwarded`. `null` for a new message.
    #[serde(default)]
    pub context: Option<DraftContext>,
    /// Which signature the draft carries. The composer inserts it into the
    /// body and swaps it when the identity changes.
    #[serde(default)]
    pub signature: SignatureChoice,
    /// Markdown mode: also send the source as a `text/markdown` part
    /// (RFC 7763).
    #[serde(default)]
    pub markdown_part: bool,
    /// Set by the core for mail a filter rule sends (forward, template
    /// reply, redirect; ADR 0028). Such mail carries `Auto-Submitted` and
    /// the [`LOOP_HEADER`] marker, and on-send rules skip it, so rules
    /// cannot start mail loops. Clients leave it as they got it; absent
    /// for mail the user writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub automatic: Option<AutomaticMail>,
}

/// The header that marks mail sent by a Nail filter rule. Its value is the
/// sending address. Rules never forward, redirect or answer a message that
/// carries it.
pub const LOOP_HEADER: &str = "X-Nail-Loop";

/// What kind of automatic mail a rule sends ([`Draft::automatic`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AutomaticMail {
    /// A forward (`Auto-Submitted: auto-generated`).
    Forward,
    /// A template reply (`Auto-Submitted: auto-replied`).
    Reply,
    /// A redirect: the stored message is sent unchanged to the draft's
    /// recipients, with `Resent-*` headers (RFC 5322 §3.6.6); the draft's
    /// body is not used.
    Redirect {
        /// The redirected message.
        message_id: MessageId,
    },
}

impl Draft {
    /// An empty plain-text draft from `identity_id`.
    pub fn new(identity_id: impl Into<IdentityId>) -> Self {
        Self {
            id: None,
            identity_id: identity_id.into(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: String::new(),
            mode: ComposerMode::Plain,
            text: String::new(),
            html: None,
            markdown: None,
            attachments: Vec::new(),
            context: None,
            signature: SignatureChoice::default(),
            markdown_part: false,
            automatic: None,
        }
    }

    /// Whether the draft uses anything beyond plain text (rich bodies or
    /// attachments).
    pub fn uses_rich_content(&self) -> bool {
        self.mode != ComposerMode::Plain
            || self.html.is_some()
            || self.markdown.is_some()
            || !self.attachments.is_empty()
    }

    /// The message a reply answers (`In-Reply-To`); `None` for new
    /// messages and forwards.
    pub fn replied_message(&self) -> Option<&MessageId> {
        self.context
            .as_ref()
            .filter(|c| c.mode.is_reply())
            .map(|c| &c.message_id)
    }
}

/// What a draft answers or forwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DraftContext {
    /// Reply, reply to all or forward.
    pub mode: DraftContextMode,
    /// The original message.
    pub message_id: MessageId,
}

/// How a draft relates to its original message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DraftContextMode {
    /// Reply to the sender (or `Reply-To`).
    Reply,
    /// Reply to the sender and all other recipients.
    ReplyAll,
    /// Forward inline (quoted body, original attachments attached).
    Forward,
}

impl DraftContextMode {
    /// Whether this is a reply (sets threading headers and `\Answered`).
    pub fn is_reply(self) -> bool {
        matches!(self, Self::Reply | Self::ReplyAll)
    }
}

/// Which signature a draft carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureChoice {
    /// The sending identity's signature, in the variant of the draft's mode.
    #[default]
    Identity,
    /// No signature.
    None,
}

/// An attachment or inline image of a draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DraftAttachment {
    /// File name.
    pub filename: String,
    /// MIME type.
    pub mime: String,
    /// Size in bytes.
    pub size: u64,
    /// Hash of the content in the core's blob store (from
    /// [`crate::Request::UploadAttachment`], or copied from a forwarded
    /// message by [`crate::Request::NewDraft`]).
    pub blob: String,
    /// `Content-ID` (without angle brackets) of an inline image the HTML
    /// references as `cid:<id>`; `null` for a regular attachment.
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
        assert_eq!(draft.signature, SignatureChoice::Identity);
        assert!(!draft.uses_rich_content());
        assert_eq!(draft.replied_message(), None);
        let rich = Draft {
            html: Some("<p>x</p>".into()),
            ..draft.clone()
        };
        assert!(rich.uses_rich_content());
        let empty = Draft {
            to: Vec::new(),
            subject: String::new(),
            text: String::new(),
            ..draft
        };
        assert_eq!(Draft::new("ada@example.org"), empty);
        assert_eq!(
            serde_json::to_value(OutboxState::Scheduled).unwrap(),
            json!("scheduled")
        );
    }

    #[test]
    fn rich_draft_round_trips() {
        let draft = Draft {
            mode: ComposerMode::Markdown,
            markdown: Some("# Hi".into()),
            attachments: vec![DraftAttachment {
                filename: "logo.png".into(),
                mime: "image/png".into(),
                size: 3,
                blob: "ab".into(),
                content_id: Some("logo@nail".into()),
            }],
            context: Some(DraftContext {
                mode: DraftContextMode::ReplyAll,
                message_id: "m1".into(),
            }),
            signature: SignatureChoice::None,
            markdown_part: true,
            ..Draft::new("ada@example.org")
        };
        let value = serde_json::to_value(&draft).unwrap();
        assert_eq!(
            value["context"],
            json!({"mode": "reply-all", "message_id": "m1"})
        );
        assert_eq!(value["signature"], json!("none"));
        assert_eq!(value["mode"], json!("markdown"));
        assert_eq!(serde_json::from_value::<Draft>(value).unwrap(), draft);
        assert_eq!(draft.replied_message(), Some(&MessageId::from("m1")));
        let forward = Draft {
            context: Some(DraftContext {
                mode: DraftContextMode::Forward,
                message_id: "m1".into(),
            }),
            ..draft
        };
        assert_eq!(forward.replied_message(), None);
    }
}
