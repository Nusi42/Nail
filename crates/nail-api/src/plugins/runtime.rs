//! What plug-ins exchange with the UI while they run (plan M2d WP60):
//! command contexts, dialogs and their results, notifications, UI actions
//! and the read-only mail view.
//!
//! Plug-in-facing types (what `@nail/plugin-api` hands to plug-in code)
//! are camelCase; the wrappers that travel in [`crate::Event`]s are
//! `snake_case` like the rest of the protocol.
//!
//! **Which UI.** Several UIs can be connected (desktop window, browser
//! tabs of the remote UI). A UI sends a random `client_id` with
//! [`crate::Request::RunPluginCommand`] and friends; dialogs and
//! notifications raised while the plug-in handles that call carry it, and
//! a UI shows only those with its own id or none (`null`: background work
//! of the plug-in, shown by every UI).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::ui::PluginUiNode;
use crate::context::UiContext;
use crate::domain::{Address, Flags};
use crate::ids::{
    AccountId, CalendarId, DraftId, EventId, FolderId, MessageId, PluginDialogId, PluginId,
    ThreadId,
};

/// Longest `client_id` a UI may send.
pub const MAX_CLIENT_ID_LEN: usize = 64;

/// Where a command can be invoked from (the manifest's `contexts`) and
/// where it was invoked.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS,
)]
#[serde(rename_all = "kebab-case")]
pub enum PluginCommandSurface {
    /// The message list (toolbar, context menu, `list` keys).
    List,
    /// The reader (toolbar, context menu, `reader` keys).
    Reader,
    /// The composer.
    Composer,
    /// Anywhere (command palette, `global` keys, the app menu).
    #[default]
    Global,
}

impl PluginCommandSurface {
    /// All surfaces.
    pub const ALL: [PluginCommandSurface; 4] =
        [Self::List, Self::Reader, Self::Composer, Self::Global];

    /// The `layout.kdl` key context of the surface.
    pub fn key_context(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Reader => "reader",
            Self::Composer => "composer",
            Self::Global => "global",
        }
    }
}

/// What a command handler receives (`ctx` in the design's example).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandContext {
    /// Where it was invoked.
    #[ts(optional, as = "Option<PluginCommandSurface>")]
    #[serde(default)]
    pub surface: PluginCommandSurface,
    /// The selection.
    pub selection: PluginSelection,
    /// The active account (real or virtual).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account_id: Option<AccountId>,
    /// The active folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder_id: Option<FolderId>,
    /// The draft open in the composer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub draft_id: Option<DraftId>,
}

/// The messages a command acts on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginSelection {
    /// Selected messages (the open one first when it is selected).
    #[serde(default)]
    pub message_ids: Vec<MessageId>,
    /// The message open in the reader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub open_message_id: Option<MessageId>,
    /// Its thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thread_id: Option<ThreadId>,
}

impl PluginCommandContext {
    /// The context of a command invoked on `surface` with the UI's
    /// `context`. Selected text and the draft cursor are not passed on.
    /// The host still drops ids the plug-in may not read.
    pub fn from_ui(surface: PluginCommandSurface, context: &UiContext) -> Self {
        let mut message_ids = context.selected_message_ids.clone();
        if let Some(open) = &context.message_id
            && message_ids.is_empty()
        {
            message_ids.push(open.clone());
        }
        Self {
            surface,
            selection: PluginSelection {
                message_ids,
                open_message_id: context.message_id.clone(),
                thread_id: context.thread_id.clone(),
            },
            account_id: context.account_id.clone(),
            folder_id: context.folder_id.clone(),
            draft_id: context.draft_id.clone(),
        }
    }
}

/// A dialog as a plug-in asks for it (`nail.ui.showDialog(spec)`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginDialogSpec {
    /// The title.
    pub title: String,
    /// The content.
    pub body: Vec<PluginUiNode>,
    /// The buttons that close the dialog (none: only the close button).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginDialogButton>>")]
    pub buttons: Vec<PluginDialogButton>,
    /// Whether the rest of the UI is blocked while it is open (default
    /// `true`); a non-modal dialog is a floating window.
    #[serde(default = "yes")]
    #[ts(optional, as = "Option<bool>")]
    pub modal: bool,
    /// Its size.
    #[serde(default)]
    #[ts(optional, as = "Option<PluginDialogSize>")]
    pub size: PluginDialogSize,
}

fn yes() -> bool {
    true
}

/// A dialog button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginDialogButton {
    /// Reported as [`PluginDialogResult::button`].
    pub id: String,
    /// The label.
    pub label: String,
    /// The default button (Enter); it needs every `required` value.
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub primary: bool,
}

/// The size of a dialog.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginDialogSize {
    /// Narrow (confirmations).
    Small,
    /// The default.
    #[default]
    Medium,
    /// Wide (tables, lists).
    Large,
}

/// What `nail.ui.showDialog` resolves to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginDialogResult {
    /// The button that closed the dialog; `null` when it was dismissed
    /// (Escape, the close button, the plug-in stopped).
    pub button: Option<String>,
    /// The values of the input nodes by id (see [`super::ui`]).
    pub values: BTreeMap<String, Value>,
}

/// An open plug-in dialog ([`crate::Event::PluginDialogOpened`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginDialog {
    /// The dialog (answer with [`crate::Request::RespondPluginDialog`]).
    pub dialog_id: PluginDialogId,
    /// Its plug-in.
    pub plugin_id: PluginId,
    /// The UI that caused it; `null` = any UI.
    pub client_id: Option<String>,
    /// What to show (validated, images inlined, message lists filled).
    pub spec: PluginDialogSpec,
}

/// Where a UI action happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PluginUiTarget {
    /// In an open dialog.
    Dialog {
        /// The dialog.
        dialog_id: PluginDialogId,
    },
    /// In a panel (`<plugin>.<panel>`).
    Panel {
        /// The panel.
        panel: String,
    },
    /// A button of a banner an open hook showed with a message (M4a,
    /// [`super::hooks::PluginBannerSpec`]).
    Banner {
        /// The message.
        message_id: MessageId,
        /// `<plugin>.<banner id>`.
        banner: String,
    },
}

/// The level of a notification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginNotificationLevel {
    /// Information.
    #[default]
    Info,
    /// Something worked.
    Success,
    /// Something needs attention.
    Warning,
    /// Something failed.
    Error,
}

/// A notification as a plug-in asks for it (`nail.ui.notify`). A plain
/// string is accepted by the SDK as `{title}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginNotificationSpec {
    /// The text.
    pub title: String,
    /// More text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
    /// The level.
    #[serde(default)]
    #[ts(optional, as = "Option<PluginNotificationLevel>")]
    pub level: PluginNotificationLevel,
    /// Also as an OS notification (desktop) / Web Notification (remote
    /// UI), subject to the notification settings' quiet hours; otherwise
    /// an in-app toast only.
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub system: bool,
}

/// A plug-in notification ([`crate::Event::PluginNotification`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginNotification {
    /// Unique id of this notification.
    pub id: String,
    /// Its plug-in (the UI names it next to the text).
    pub plugin_id: PluginId,
    /// The UI that caused it; `null` = any UI.
    pub client_id: Option<String>,
    /// What to show.
    pub spec: PluginNotificationSpec,
}

/// A message as a plug-in with `mail.read` sees it (`nail.mail.get`,
/// `nail.mail.list`, `message-list` rows): headers, no body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginMessage {
    /// The message.
    pub id: MessageId,
    /// `nail://messages/<id>` ([`super::message_uri`]).
    pub uri: String,
    /// Its account.
    pub account_id: AccountId,
    /// Its folder.
    pub folder_id: FolderId,
    /// Its thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thread_id: Option<ThreadId>,
    /// The subject (empty when missing).
    pub subject: String,
    /// The sender.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub from: Option<Address>,
    /// `To` recipients.
    pub to: Vec<Address>,
    /// `Cc` recipients.
    #[serde(default)]
    pub cc: Vec<Address>,
    /// RFC 3339 date.
    pub date: String,
    /// Flags.
    pub flags: Flags,
    /// Tags (keywords).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether it has attachments.
    pub has_attachments: bool,
    /// Whether it is end-to-end encrypted (its body is then never given to
    /// a preview plug-in).
    #[serde(default)]
    pub encrypted: bool,
}

/// An event occurrence as a plug-in with `calendar.read` sees it
/// (`nail.calendar.events`; M3a).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginCalendarEvent {
    /// The event (series).
    pub event_id: EventId,
    /// Its calendar.
    pub calendar_id: CalendarId,
    /// Title.
    pub title: String,
    /// Start (RFC 3339 UTC; all-day events at local midnight).
    pub start: String,
    /// End (RFC 3339 UTC).
    pub end: String,
    /// An all-day event.
    pub all_day: bool,
    /// Location, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub location: Option<String>,
    /// Part of a recurring series.
    #[serde(default)]
    pub recurring: bool,
}

/// Most events one `nail.calendar.events` call returns.
pub const MAX_PLUGIN_CALENDAR_EVENTS: u32 = 500;

/// Longest range of one `nail.calendar.events` call, in days.
pub const MAX_PLUGIN_CALENDAR_DAYS: i64 = 92;

/// A message body as a plug-in with `mail.read.body` sees it
/// (`nail.mail.getBody`). Never of end-to-end encrypted mail in the
/// preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginMessageBody {
    /// The message.
    pub id: MessageId,
    /// The plain-text body (or the text rendering of the HTML body).
    pub text: String,
    /// The sanitized HTML body, if the message has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub html: Option<String>,
    /// The attachments (names and sizes, no content).
    #[serde(default)]
    pub attachments: Vec<PluginAttachment>,
}

/// An attachment as a plug-in sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginAttachment {
    /// The file name.
    pub name: String,
    /// The MIME type.
    pub mime_type: String,
    /// The size in bytes.
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dialog_specs_parse_the_design_example() {
        let spec: PluginDialogSpec = serde_json::from_value(json!({
            "title": "Follow up",
            "body": [{ "type": "date", "id": "due", "label": "Due", "default": "tomorrow" }],
            "buttons": [{ "id": "ok", "label": "Create task", "primary": true },
                        { "id": "cancel", "label": "Cancel" }],
        }))
        .unwrap();
        assert!(spec.modal);
        assert_eq!(spec.size, PluginDialogSize::Medium);
        assert!(spec.buttons[0].primary && !spec.buttons[1].primary);

        let result = PluginDialogResult {
            button: Some("ok".into()),
            values: BTreeMap::from([("due".to_owned(), json!("2026-10-01"))]),
        };
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({"button": "ok", "values": {"due": "2026-10-01"}})
        );
        let dismissed: PluginDialogResult =
            serde_json::from_value(json!({"button": null, "values": {}})).unwrap();
        assert_eq!(dismissed, PluginDialogResult::default());
    }

    #[test]
    fn command_context_from_the_ui_context() {
        let ui = UiContext {
            account_id: Some("work".into()),
            folder_id: Some("f1".into()),
            message_id: Some("m1".into()),
            thread_id: Some("t1".into()),
            selected_message_ids: vec![],
            selected_text: Some("secret".into()),
            draft_id: None,
            draft_cursor: Some(3),
        };
        let ctx = PluginCommandContext::from_ui(PluginCommandSurface::Reader, &ui);
        assert_eq!(ctx.selection.message_ids, [MessageId::from("m1")]);
        let value = serde_json::to_value(&ctx).unwrap();
        assert_eq!(
            value,
            json!({
                "surface": "reader",
                "selection": {"messageIds": ["m1"], "openMessageId": "m1", "threadId": "t1"},
                "accountId": "work",
                "folderId": "f1",
            })
        );
        let selected = UiContext {
            selected_message_ids: vec!["m2".into(), "m3".into()],
            message_id: Some("m1".into()),
            ..UiContext::default()
        };
        let ctx = PluginCommandContext::from_ui(PluginCommandSurface::List, &selected);
        assert_eq!(ctx.selection.message_ids.len(), 2);
        assert_eq!(
            serde_json::from_value::<PluginCommandContext>(json!({"selection": {}}))
                .unwrap()
                .surface,
            PluginCommandSurface::Global
        );
    }

    #[test]
    fn surfaces_and_targets() {
        assert_eq!(PluginCommandSurface::List.key_context(), "list");
        assert_eq!(PluginCommandSurface::ALL.len(), 4);
        assert_eq!(
            serde_json::to_value(PluginUiTarget::Panel {
                panel: "crm.sidebar".into()
            })
            .unwrap(),
            json!({"kind": "panel", "panel": "crm.sidebar"})
        );
        assert_eq!(
            serde_json::to_value(PluginUiTarget::Dialog {
                dialog_id: "d1".into()
            })
            .unwrap(),
            json!({"kind": "dialog", "dialog_id": "d1"})
        );
    }

    #[test]
    fn notifications_default_to_info_toasts() {
        let spec: PluginNotificationSpec =
            serde_json::from_value(json!({"title": "Follow-up saved"})).unwrap();
        assert_eq!(spec.level, PluginNotificationLevel::Info);
        assert!(!spec.system);
        assert_eq!(
            serde_json::to_value(&spec).unwrap(),
            json!({"title": "Follow-up saved", "level": "info", "system": false})
        );
    }

    #[test]
    fn message_views_are_camel_case() {
        let msg = PluginMessage {
            id: "m1".into(),
            uri: super::super::message_uri(&"m1".into()),
            account_id: "work".into(),
            folder_id: "f1".into(),
            thread_id: None,
            subject: "Hi".into(),
            from: None,
            to: vec![],
            cc: vec![],
            date: "2026-09-30T08:00:00Z".into(),
            flags: Flags::default(),
            tags: vec![],
            has_attachments: false,
            encrypted: false,
        };
        let value = serde_json::to_value(&msg).unwrap();
        assert_eq!(value["accountId"], "work");
        assert_eq!(value["hasAttachments"], false);
        assert_eq!(serde_json::from_value::<PluginMessage>(value).unwrap(), msg);
    }
}
