//! Mail hooks (M4a, ADR 0079; design.md, "Plug-ins" → Mail hooks): what a
//! plug-in declares, receives and answers.
//!
//! * **arrival** (`hooks.arrival`): called for each new message that
//!   matches the hook's `filter` (Nail's search language, like a rule's
//!   condition) in the accounts the grant reaches, after the user's own
//!   arrival rules. It receives the [`super::PluginMessage`] (plus the body
//!   with `mail.read.body`) and answers [`PluginArrivalResult`] actions —
//!   applied like a rule's actions through the operation queue (they need
//!   `mail.organize`; undoable; never a delete).
//! * **beforeSend** (`hooks.beforeSend`): called with every outgoing
//!   message ([`PluginOutgoingMessage`]) before it is queued; answers
//!   [`PluginBeforeSendResult`]: extra headers, content added at the top
//!   or bottom, or a veto with a reason. A veto names the plug-in and the
//!   user can "Send anyway" ([`crate::Response::SendVetoed`],
//!   [`crate::Request::OverrideSendVeto`]).
//! * **open** (`hooks.open`): called when the user opens a message;
//!   answers [`PluginOpenResult`] banners and badges shown with the message
//!   (the message itself is never rewritten).
//!
//! **Budgets.** Every hook call has a wall-clock budget
//! ([`PluginHookKind::default_timeout_ms`], at most
//! [`MAX_HOOK_TIMEOUT_MS`]; a manifest may ask for less). A hook that
//! throws, times out or answers something invalid is skipped and counted
//! ([`PluginHookStats`]): it can never block delivery or sending — the
//! mail arrives, the message is sent, the reader opens.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::runtime::{PluginAttachment, PluginNotificationLevel};
use crate::domain::Address;
use crate::ids::{AccountId, DraftId, FolderId, IdentityId, MessageId, PluginId};

/// Longest hook budget a manifest may ask for (ms).
pub const MAX_HOOK_TIMEOUT_MS: u32 = 10_000;
/// Shortest hook budget a manifest may ask for (ms).
pub const MIN_HOOK_TIMEOUT_MS: u32 = 50;
/// Longest `filter` of a hook (characters).
pub const MAX_HOOK_FILTER: usize = 500;
/// Most actions one arrival hook call may answer.
pub const MAX_ARRIVAL_ACTIONS: usize = 20;
/// Most headers one before-send call may add.
pub const MAX_ADDED_HEADERS: usize = 10;
/// Longest added header value (bytes; RFC 5322 line limit).
pub const MAX_HEADER_VALUE: usize = 998;
/// Longest header name (bytes).
pub const MAX_HEADER_NAME: usize = 76;
/// Largest content one before-send call may add (bytes of text + HTML).
pub const MAX_ADDED_CONTENT: usize = 64 * 1024;
/// Longest veto reason, banner or badge tooltip text (characters).
pub const MAX_HOOK_TEXT: usize = 500;
/// Longest badge text (characters).
pub const MAX_BADGE_TEXT: usize = 32;
/// Most banners and badges one open hook call may answer (each).
pub const MAX_OPEN_ITEMS: usize = 3;
/// Header names plug-ins may never add (Nail's own loop marker and
/// similar; compared case-insensitively as a prefix).
pub const RESERVED_HEADER_PREFIX: &str = "X-Nail-";

/// The kind of a mail hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PluginHookKind {
    /// New mail.
    Arrival,
    /// Outgoing mail, before it is queued.
    BeforeSend,
    /// A message opened in the reader.
    Open,
}

impl PluginHookKind {
    /// All hooks.
    pub const ALL: [PluginHookKind; 3] = [Self::Arrival, Self::BeforeSend, Self::Open];

    /// The wire / store name (`arrival`, `beforeSend`, `open`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Arrival => "arrival",
            Self::BeforeSend => "beforeSend",
            Self::Open => "open",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.as_str() == text)
    }

    /// The permission a hook of this kind needs.
    pub fn permission(self) -> super::PluginPermission {
        match self {
            Self::Arrival => super::PluginPermission::HooksArrival,
            Self::BeforeSend => super::PluginPermission::HooksBeforeSend,
            Self::Open => super::PluginPermission::HooksOpen,
        }
    }

    /// The budget of one call unless the manifest asks for less (ms).
    pub fn default_timeout_ms(self) -> u32 {
        match self {
            Self::Arrival => 2_000,
            Self::BeforeSend => 3_000,
            Self::Open => 1_000,
        }
    }

    /// Fluent id of the hook's name (`plugin-hook-<kebab name>`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Arrival => "plugin-hook-arrival",
            Self::BeforeSend => "plugin-hook-before-send",
            Self::Open => "plugin-hook-open",
        }
    }
}

/// A hook in the manifest (`contributes.hooks`; at most one per kind).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookDef {
    /// Which hook.
    pub hook: PluginHookKind,
    /// Only messages matching this query of Nail's search language
    /// (`from:@acme.example has:attachment`); arrival and open hooks only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub filter: Option<String>,
    /// A smaller budget than the default (ms, at least
    /// [`MIN_HOOK_TIMEOUT_MS`], at most the kind's default … up to
    /// [`MAX_HOOK_TIMEOUT_MS`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timeout_ms: Option<u32>,
}

impl PluginHookDef {
    /// The budget of one call (ms).
    pub fn timeout_ms(&self) -> u32 {
        self.timeout_ms
            .unwrap_or_else(|| self.hook.default_timeout_ms())
            .clamp(MIN_HOOK_TIMEOUT_MS, MAX_HOOK_TIMEOUT_MS)
    }

    /// Whether the definition is valid (filter only where it applies and
    /// not too long, budget in range).
    pub fn is_valid(&self) -> bool {
        let filter_ok = match &self.filter {
            None => true,
            Some(f) => {
                self.hook != PluginHookKind::BeforeSend
                    && !f.trim().is_empty()
                    && f.chars().count() <= MAX_HOOK_FILTER
            }
        };
        let timeout_ok = self
            .timeout_ms
            .is_none_or(|t| (MIN_HOOK_TIMEOUT_MS..=MAX_HOOK_TIMEOUT_MS).contains(&t));
        filter_ok && timeout_ok
    }
}

/// An action an arrival hook asks for (camelCase, tagged by `type`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginMailAction {
    /// Add a tag.
    AddTag {
        /// The tag.
        tag: String,
    },
    /// Remove a tag.
    RemoveTag {
        /// The tag.
        tag: String,
    },
    /// Flag or unflag.
    SetFlagged {
        /// Flagged.
        flagged: bool,
    },
    /// Mark read or unread.
    SetRead {
        /// Read.
        read: bool,
    },
    /// Move to a folder of the same account.
    Move {
        /// The folder.
        folder_id: FolderId,
    },
    /// Move to the account's Archive folder.
    Archive,
}

/// What an arrival hook answers (`undefined` / `null` = nothing).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginArrivalResult {
    /// Actions, applied in order (at most [`MAX_ARRIVAL_ACTIONS`]).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginMailAction>>")]
    pub actions: Vec<PluginMailAction>,
}

/// A message on its way out, as a before-send hook sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginOutgoingMessage {
    /// The draft being sent.
    pub draft_id: DraftId,
    /// The sending account.
    pub account_id: AccountId,
    /// The identity.
    pub identity_id: IdentityId,
    /// The sender.
    pub from: Address,
    /// Recipients.
    pub to: Vec<Address>,
    /// Cc.
    #[serde(default)]
    pub cc: Vec<Address>,
    /// Bcc.
    #[serde(default)]
    pub bcc: Vec<Address>,
    /// Subject.
    pub subject: String,
    /// The body as plain text (the text rendering of HTML / Markdown);
    /// empty when the message will be end-to-end encrypted (hooks never see
    /// encrypted content).
    pub text: String,
    /// The HTML body as it will be sent, if any (never for a message that
    /// will be end-to-end encrypted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub html: Option<String>,
    /// Attachments (names, types, sizes).
    #[serde(default)]
    pub attachments: Vec<PluginAttachment>,
    /// The message being replied to or forwarded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub in_reply_to: Option<MessageId>,
    /// Whether it will be signed / encrypted (hooks cannot change that;
    /// content added to an encrypted message is encrypted with it).
    #[serde(default)]
    pub encrypted: bool,
}

/// A header a before-send hook adds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginHeader {
    /// The name ([`is_plugin_header_name`]: `X-…`).
    pub name: String,
    /// The value (one line).
    pub value: String,
}

/// Where added content goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PluginContentPosition {
    /// Before the body.
    Top,
    /// After the body (before the quoted original, after the signature).
    #[default]
    Bottom,
}

/// Content a before-send hook adds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginContentAddition {
    /// Where.
    #[serde(default)]
    #[ts(optional, as = "Option<PluginContentPosition>")]
    pub position: PluginContentPosition,
    /// The text (for the plain-text part, and the HTML part when `html`
    /// is absent).
    pub text: String,
    /// HTML for the HTML part (sanitized like a message body).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub html: Option<String>,
}

/// A before-send veto as the plug-in answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginVetoSpec {
    /// Why (shown to the user with the plug-in's name; at most
    /// [`MAX_HOOK_TEXT`] characters).
    pub reason: String,
}

/// What a before-send hook answers (`undefined` / `null` = nothing).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginBeforeSendResult {
    /// Stop sending (the user can override it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub veto: Option<PluginVetoSpec>,
    /// Headers to add (at most [`MAX_ADDED_HEADERS`]).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginHeader>>")]
    pub headers: Vec<PluginHeader>,
    /// Content to add (at most [`MAX_ADDED_CONTENT`] bytes in all).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginContentAddition>>")]
    pub add_content: Vec<PluginContentAddition>,
}

impl PluginBeforeSendResult {
    /// Problems with the answer (the host skips an invalid answer and
    /// logs why; empty = valid).
    pub fn problems(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if let Some(veto) = &self.veto
            && (veto.reason.trim().is_empty() || veto.reason.chars().count() > MAX_HOOK_TEXT)
        {
            out.push("veto reason empty or too long");
        }
        if self.headers.len() > MAX_ADDED_HEADERS {
            out.push("too many headers");
        }
        if self
            .headers
            .iter()
            .any(|h| !is_plugin_header_name(&h.name) || !is_header_value(&h.value))
        {
            out.push("invalid header");
        }
        let size: usize = self
            .add_content
            .iter()
            .map(|c| c.text.len() + c.html.as_ref().map_or(0, String::len))
            .sum();
        if size > MAX_ADDED_CONTENT {
            out.push("added content too large");
        }
        out
    }
}

/// Whether a plug-in may add a header of this name: an `X-` header
/// (RFC 5322 field-name characters, at most [`MAX_HEADER_NAME`] bytes), not
/// one of Nail's own ([`RESERVED_HEADER_PREFIX`]).
pub fn is_plugin_header_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.len() > 2
        && name.len() <= MAX_HEADER_NAME
        && lower.starts_with("x-")
        && !lower.starts_with(&RESERVED_HEADER_PREFIX.to_ascii_lowercase())
        && name.bytes().all(|b| b.is_ascii_graphic() && b != b':')
}

/// Whether `value` is a valid single-line header value (no CR, LF or NUL;
/// at most [`MAX_HEADER_VALUE`] bytes).
pub fn is_header_value(value: &str) -> bool {
    value.len() <= MAX_HEADER_VALUE && !value.contains(['\r', '\n', '\0'])
}

/// A banner an open hook shows above a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginBannerSpec {
    /// Its id (reported with button presses; a contribution name).
    pub id: String,
    /// Its level (styling).
    #[serde(default)]
    #[ts(optional, as = "Option<PluginNotificationLevel>")]
    pub level: PluginNotificationLevel,
    /// The text (at most [`MAX_HOOK_TEXT`] characters).
    pub text: String,
    /// Buttons; pressing one calls the plug-in's `onBannerAction`
    /// ([`crate::plugins::PluginUiTarget::Banner`]).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginBannerButton>>")]
    pub buttons: Vec<PluginBannerButton>,
}

/// A banner button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginBannerButton {
    /// Reported as the action's `component_id`.
    pub id: String,
    /// The label.
    pub label: String,
}

/// A badge an open hook shows in the message header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginBadgeSpec {
    /// The text (at most [`MAX_BADGE_TEXT`] characters).
    pub text: String,
    /// Its level (styling).
    #[serde(default)]
    #[ts(optional, as = "Option<PluginNotificationLevel>")]
    pub level: PluginNotificationLevel,
    /// An icon of the theme's icon set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<String>,
    /// A tooltip (at most [`MAX_HOOK_TEXT`] characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tooltip: Option<String>,
}

/// What an open hook answers (`undefined` / `null` = nothing).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginOpenResult {
    /// Banners (at most [`MAX_OPEN_ITEMS`]).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginBannerSpec>>")]
    pub banners: Vec<PluginBannerSpec>,
    /// Badges (at most [`MAX_OPEN_ITEMS`]).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginBadgeSpec>>")]
    pub badges: Vec<PluginBadgeSpec>,
}

impl PluginOpenResult {
    /// Problems with the answer (empty = valid).
    pub fn problems(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.banners.len() > MAX_OPEN_ITEMS || self.badges.len() > MAX_OPEN_ITEMS {
            out.push("too many banners or badges");
        }
        if self.banners.iter().any(|b| {
            !super::is_contribution_name(&b.id)
                || b.text.trim().is_empty()
                || b.text.chars().count() > MAX_HOOK_TEXT
                || b.buttons.len() > 3
                || b.buttons
                    .iter()
                    .any(|x| !super::is_contribution_name(&x.id) || x.label.trim().is_empty())
        }) {
            out.push("invalid banner");
        }
        if self.badges.iter().any(|b| {
            b.text.trim().is_empty()
                || b.text.chars().count() > MAX_BADGE_TEXT
                || b.tooltip
                    .as_ref()
                    .is_some_and(|t| t.chars().count() > MAX_HOOK_TEXT)
        }) {
            out.push("invalid badge");
        }
        out
    }
}

/// A banner as the UI gets it
/// ([`crate::Response::MessagePluginAnnotations`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginBanner {
    /// The plug-in.
    pub plugin_id: PluginId,
    /// Its name (resolved).
    pub plugin_name: String,
    /// The banner.
    pub banner: PluginBannerSpec,
}

/// A badge as the UI gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginBadge {
    /// The plug-in.
    pub plugin_id: PluginId,
    /// Its name (resolved).
    pub plugin_name: String,
    /// The badge.
    pub badge: PluginBadgeSpec,
}

/// A before-send veto as the UI gets it ([`crate::Response::SendVetoed`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginSendVeto {
    /// The plug-in.
    pub plugin_id: PluginId,
    /// Its name (resolved; the dialog always names it).
    pub plugin_name: String,
    /// Its reason (the plug-in's words).
    pub reason: String,
}

/// How one hook of a plug-in has been doing (the plug-in manager).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginHookStats {
    /// The hook.
    pub hook: PluginHookKind,
    /// Calls.
    pub calls: u64,
    /// Calls that threw or answered something invalid.
    pub failures: u64,
    /// Calls stopped by the budget.
    pub timeouts: u64,
    /// Before-send vetoes.
    pub vetoes: u64,
    /// Total time spent (ms).
    pub total_ms: u64,
    /// Longest call (ms).
    pub max_ms: u64,
    /// The last failure (English, for logs).
    pub last_error: Option<String>,
    /// The last call (RFC 3339).
    pub last_at: Option<String>,
}

/// How one hook call ended (the store counts them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginHookOutcome {
    /// Answered (possibly nothing).
    Ok,
    /// Answered with a veto.
    Vetoed,
    /// Threw or answered something invalid.
    Failed,
    /// Ran out of its budget.
    TimedOut,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hook_kinds() {
        for hook in PluginHookKind::ALL {
            assert_eq!(PluginHookKind::parse(hook.as_str()), Some(hook));
            assert_eq!(serde_json::to_value(hook).unwrap(), json!(hook.as_str()));
            assert!(crate::localized::is_message_id(hook.message_id()));
            assert!(hook.default_timeout_ms() <= MAX_HOOK_TIMEOUT_MS);
        }
        assert_eq!(
            PluginHookKind::BeforeSend.permission().name(),
            "hooks.beforeSend"
        );
    }

    #[test]
    fn hook_definitions() {
        let def: PluginHookDef =
            serde_json::from_value(json!({"hook": "arrival", "filter": "from:@acme.example"}))
                .unwrap();
        assert!(def.is_valid());
        assert_eq!(def.timeout_ms(), 2_000);
        let short: PluginHookDef =
            serde_json::from_value(json!({"hook": "open", "timeoutMs": 300})).unwrap();
        assert_eq!(short.timeout_ms(), 300);
        for bad in [
            json!({"hook": "beforeSend", "filter": "x"}),
            json!({"hook": "open", "timeoutMs": 10}),
            json!({"hook": "open", "timeoutMs": 60_000}),
            json!({"hook": "arrival", "filter": " "}),
        ] {
            let def: PluginHookDef = serde_json::from_value(bad.clone()).unwrap();
            assert!(!def.is_valid(), "{bad}");
        }
    }

    #[test]
    fn arrival_actions_are_camel_case() {
        let result: PluginArrivalResult = serde_json::from_value(json!({
            "actions": [
                {"type": "addTag", "tag": "crm"},
                {"type": "setFlagged", "flagged": true},
                {"type": "move", "folderId": "f2"},
                {"type": "archive"}
            ]
        }))
        .unwrap();
        assert_eq!(result.actions.len(), 4);
        assert_eq!(
            result.actions[2],
            PluginMailAction::Move {
                folder_id: "f2".into()
            }
        );
        assert_eq!(
            serde_json::from_value::<PluginArrivalResult>(json!({})).unwrap(),
            PluginArrivalResult::default()
        );
        assert!(serde_json::from_value::<PluginMailAction>(json!({"type": "delete"})).is_err());
    }

    #[test]
    fn before_send_answers_are_checked() {
        let ok: PluginBeforeSendResult = serde_json::from_value(json!({
            "headers": [{"name": "X-Crm-Ticket", "value": "4711"}],
            "addContent": [{"text": "Ticket 4711"}]
        }))
        .unwrap();
        assert_eq!(ok.problems().len(), 0);
        assert_eq!(ok.add_content[0].position, PluginContentPosition::Bottom);
        let veto: PluginBeforeSendResult =
            serde_json::from_value(json!({"veto": {"reason": "No attachment"}})).unwrap();
        assert_eq!(veto.problems().len(), 0);
        let bad = PluginBeforeSendResult {
            veto: Some(PluginVetoSpec { reason: " ".into() }),
            headers: vec![
                PluginHeader {
                    name: "Subject".into(),
                    value: "x".into(),
                },
                PluginHeader {
                    name: "X-Ok".into(),
                    value: "a\r\nBcc: evil@example.com".into(),
                },
            ],
            add_content: vec![PluginContentAddition {
                position: PluginContentPosition::Top,
                text: "x".repeat(MAX_ADDED_CONTENT + 1),
                html: None,
            }],
        };
        assert_eq!(bad.problems().len(), 3);
        assert!(is_plugin_header_name("X-Crm-Ticket"));
        assert!(!is_plugin_header_name("X-Nail-Loop"));
        assert!(!is_plugin_header_name("x-nail-anything"));
        assert!(!is_plugin_header_name("From"));
        assert!(!is_plugin_header_name("X-"));
        assert!(!is_plugin_header_name("X-A:b"));
        assert!(!is_plugin_header_name("X-With Space"));
        assert!(is_header_value("plain"));
        assert!(!is_header_value("a\nb"));
    }

    #[test]
    fn open_answers_are_checked() {
        let ok: PluginOpenResult = serde_json::from_value(json!({
            "banners": [{"id": "customer", "text": "Key customer", "level": "warning",
                         "buttons": [{"id": "open-crm", "label": "Open in CRM"}]}],
            "badges": [{"text": "VIP", "icon": "star"}]
        }))
        .unwrap();
        assert_eq!(ok.problems().len(), 0);
        let bad = PluginOpenResult {
            banners: vec![PluginBannerSpec {
                id: "Bad Id".into(),
                level: PluginNotificationLevel::Info,
                text: "x".into(),
                buttons: vec![],
            }],
            badges: vec![PluginBadgeSpec {
                text: "x".repeat(MAX_BADGE_TEXT + 1),
                level: PluginNotificationLevel::Info,
                icon: None,
                tooltip: None,
            }],
        };
        assert_eq!(bad.problems(), ["invalid banner", "invalid badge"]);
    }

    #[test]
    fn outgoing_messages_are_camel_case() {
        let msg = PluginOutgoingMessage {
            draft_id: "d1".into(),
            account_id: "work".into(),
            identity_id: "ada@example.com".into(),
            from: Address {
                name: None,
                email: "ada@example.com".into(),
            },
            to: vec![],
            cc: vec![],
            bcc: vec![],
            subject: "See attached".into(),
            text: "Hi".into(),
            html: None,
            attachments: vec![],
            in_reply_to: None,
            encrypted: false,
        };
        let value = serde_json::to_value(&msg).unwrap();
        assert_eq!(value["draftId"], "d1");
        assert_eq!(value["identityId"], "ada@example.com");
        assert_eq!(
            serde_json::from_value::<PluginOutgoingMessage>(value).unwrap(),
            msg
        );
    }
}
