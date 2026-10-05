//! The MCP tool table and the arguments of the write tools (M4a, ADR 0079;
//! design.md, "MCP server" → Tools).
//!
//! [`MCP_TOOLS`] lists every tool Nail's MCP server offers with the scopes
//! a client needs and, for writes, the [`ActionKind`] whose policy decides
//! whether the user confirms first. The argument structs below are what
//! agents send (`snake_case` JSON, ids as strings); with the `json-schema`
//! feature they also derive `schemars::JsonSchema`, so `nail-mcp` publishes
//! exactly these shapes as the tools' input schemas.
//!
//! Every write tool takes an optional `idempotency_key`: a repeated call
//! with the same key (per client, for 24 hours) returns the first call's
//! result instead of acting twice.
//!
//! Plug-in commands exposed under `mcp.expose` are additional tools named
//! [`crate::plugins::plugin_mcp_tool_name`] (`<plugin>__<tool>`); they are
//! not in this table.

use serde::{Deserialize, Serialize};

use crate::agents::ActionKind;
use crate::mcp::McpScope;

/// Longest idempotency key (characters).
pub const MAX_IDEMPOTENCY_KEY: usize = 128;
/// How long an idempotency key is remembered (seconds).
pub const IDEMPOTENCY_TTL_SECS: u64 = 24 * 3600;
/// Most messages one `update_messages` / `delete_messages` call may name.
pub const MAX_BATCH: usize = 500;
/// Largest attachment an agent may add to a draft (bytes, decoded).
pub const MAX_AGENT_ATTACHMENT: usize = 25 * 1024 * 1024;

/// One tool of the MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpToolSpec {
    /// The tool name.
    pub name: &'static str,
    /// Every scope the client needs (on top of the account / folder
    /// checks of the items it names).
    pub scopes: &'static [McpScope],
    /// For writes: the kind whose policy decides about a confirmation.
    /// Some tools pick a different kind from their arguments (see
    /// [`Self::kind_note`]).
    pub kind: Option<ActionKind>,
    /// When the kind depends on the arguments, how (English, for docs).
    pub kind_note: Option<&'static str>,
}

impl McpToolSpec {
    /// Whether the tool changes data.
    pub fn is_write(&self) -> bool {
        self.kind.is_some()
    }
}

const fn read(name: &'static str, scopes: &'static [McpScope]) -> McpToolSpec {
    McpToolSpec {
        name,
        scopes,
        kind: None,
        kind_note: None,
    }
}

const fn write(
    name: &'static str,
    scopes: &'static [McpScope],
    kind: ActionKind,
    kind_note: Option<&'static str>,
) -> McpToolSpec {
    McpToolSpec {
        name,
        scopes,
        kind: Some(kind),
        kind_note,
    }
}

/// Every built-in tool (design.md's table), reads first.
pub const MCP_TOOLS: &[McpToolSpec] = &[
    read("search_messages", &[McpScope::Read]),
    read("get_message", &[McpScope::Read]),
    read("get_thread", &[McpScope::Read]),
    read("list_folders", &[McpScope::Read]),
    read("get_ui_context", &[McpScope::Context]),
    read("list_events", &[McpScope::Read]),
    read("find_free_time", &[McpScope::Read]),
    read("search_contacts", &[McpScope::Read]),
    read("list_tasks", &[McpScope::Read]),
    // `test_rule` changes nothing: a dry run over existing mail.
    read("test_rule", &[McpScope::Read, McpScope::Rules]),
    write(
        "update_messages",
        &[McpScope::Organize],
        ActionKind::Organize,
        None,
    ),
    write(
        "delete_messages",
        &[McpScope::Organize],
        ActionKind::Trash,
        Some("`permanent: true` is `permanent-delete` and needs the `permanent-delete` scope"),
    ),
    write("create_draft", &[McpScope::Draft], ActionKind::Draft, None),
    write("update_draft", &[McpScope::Draft], ActionKind::Draft, None),
    write("send_message", &[McpScope::Send], ActionKind::Send, None),
    write(
        "create_event",
        &[McpScope::Calendar],
        ActionKind::Calendar,
        Some("with attendees and `send_invitations` it is `invite` and needs `send`"),
    ),
    write(
        "update_event",
        &[McpScope::Calendar],
        ActionKind::Calendar,
        Some("with `send_updates` to attendees it is `invite` and needs `send`"),
    ),
    write(
        "respond_to_invite",
        &[McpScope::Calendar],
        ActionKind::Invite,
        Some("with `send: false` it is `calendar` and needs no `send`"),
    ),
    write(
        "upsert_contact",
        &[McpScope::Pim],
        ActionKind::Contact,
        None,
    ),
    write("upsert_task", &[McpScope::Pim], ActionKind::Task, None),
    write("create_rule", &[McpScope::Rules], ActionKind::Rule, None),
    write(
        "edit_open_draft",
        &[McpScope::Context, McpScope::Draft],
        ActionKind::Draft,
        None,
    ),
];

/// The tool spec by name.
pub fn tool(name: &str) -> Option<&'static McpToolSpec> {
    MCP_TOOLS.iter().find(|t| t.name == name)
}

/// Names of the write tools.
pub fn write_tool_names() -> Vec<&'static str> {
    MCP_TOOLS
        .iter()
        .filter(|t| t.is_write())
        .map(|t| t.name)
        .collect()
}

/// Whether `key` is an acceptable idempotency key: 1–128 printable ASCII
/// characters without spaces.
pub fn is_idempotency_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_IDEMPOTENCY_KEY && key.bytes().all(|b| b.is_ascii_graphic())
}

/// The format of a body or inserted text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum McpTextFormat {
    /// Plain text.
    Text,
    /// Markdown (rendered to HTML when sent).
    #[default]
    Markdown,
    /// HTML (sanitized).
    Html,
}

/// `update_messages`: flag, tag, mark read, move, archive or snooze
/// messages. Fields left out stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct UpdateMessagesArgs {
    /// The messages (at most 500).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_ids: Vec<String>,
    /// Conversations as listed (P3, ADR 0164; at most 500): read state and
    /// tags reach every message of each, moves / archive / snooze its
    /// messages in the listed folder, `flagged: true` its newest message,
    /// `flagged: false` every flagged one. `message_ids` and
    /// `conversations` may be combined; at least one is needed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversations: Vec<McpConversationArg>,
    /// Mark read (`true`) or unread (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<bool>,
    /// Flag (`true`) or unflag (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flagged: Option<bool>,
    /// Tags to add.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_tags: Vec<String>,
    /// Tags to remove.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_tags: Vec<String>,
    /// Move to this folder (id from `list_folders`; same account).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_to_folder_id: Option<String>,
    /// Move to the account's Archive folder.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archive: bool,
    /// Snooze until this time (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snooze_until: Option<String>,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// `delete_messages`: move messages to Trash, or delete them for good.
/// Always confirmed by the user in Nail.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct DeleteMessagesArgs {
    /// The messages (at most 500).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_ids: Vec<String>,
    /// Conversations as listed (P3, ADR 0164; at most 500): to Trash with
    /// the user's replies from Sent; `permanent` only their messages in
    /// Trash or Junk.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversations: Vec<McpConversationArg>,
    /// Delete for good instead of moving to Trash (needs the
    /// `permanent-delete` grant; never confirmed automatically).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub permanent: bool,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// A conversation as a list row shows it (P3, ADR 0164): the thread id of
/// `get_thread` / a search hit, and the folder it is listed in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct McpConversationArg {
    /// The thread.
    pub thread_id: String,
    /// The folder it is listed in (id from `list_folders`); left out = the
    /// thread in every folder but Drafts, Trash and Junk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
}

/// An attachment for a draft: either the bytes or an attachment of an
/// existing message.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct DraftAttachmentArg {
    /// File name.
    pub filename: String,
    /// MIME type (default: from the file name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// The content, base64 (at most 25 MB decoded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    /// Or: copy an attachment of this message …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_message_id: Option<String>,
    /// … with this index (from `get_message`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_attachment_index: Option<u32>,
}

/// `create_draft`: a new draft (never sent by this tool).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct CreateDraftArgs {
    /// Sending identity (from `nail://accounts`); default: the account's
    /// default identity (of the replied-to message's account when
    /// replying).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_id: Option<String>,
    /// Recipients (`Name <address>` or `address`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,
    /// Cc recipients.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<String>,
    /// Bcc recipients.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bcc: Vec<String>,
    /// Subject (default when replying: `Re: <subject>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// The body.
    #[serde(default)]
    pub body: String,
    /// Format of `body` (default Markdown).
    #[serde(default)]
    pub format: McpTextFormat,
    /// Reply to this message (threading headers, quoted original).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<String>,
    /// With `reply_to_message_id`: reply to all.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reply_all: bool,
    /// Forward this message (as an attachment-free inline forward).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_message_id: Option<String>,
    /// Attachments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<DraftAttachmentArg>,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// `update_draft`: change a saved draft. Fields left out stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct UpdateDraftArgs {
    /// The draft.
    pub draft_id: String,
    /// New recipients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Vec<String>>,
    /// New Cc recipients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cc: Option<Vec<String>>,
    /// New Bcc recipients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bcc: Option<Vec<String>>,
    /// New subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// New body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Format of `body` (default: the draft's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<McpTextFormat>,
    /// Attachments to add.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_attachments: Vec<DraftAttachmentArg>,
    /// Positions of attachments to remove.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_attachments: Vec<u32>,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// `send_message`: send a saved draft. Confirmed by the user unless the
/// user allows this client to send without asking.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct SendMessageArgs {
    /// The draft.
    pub draft_id: String,
    /// Send later (RFC 3339); default: now (after the user's undo-send
    /// delay).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_at: Option<String>,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// `create_event`: a new calendar event.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct CreateEventArgs {
    /// Calendar (from `nail://calendars`); default: the default calendar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar_id: Option<String>,
    /// Title.
    pub title: String,
    /// Start: RFC 3339, or `YYYY-MM-DD` for all-day events.
    pub start: String,
    /// End (exclusive): RFC 3339, or `YYYY-MM-DD` for all-day events.
    pub end: String,
    /// IANA zone of the times (default: the system's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
    /// Location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Description (plain text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Attendee addresses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attendees: Vec<String>,
    /// An iCalendar `RRULE` value (`FREQ=WEEKLY;BYDAY=MO`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<String>,
    /// Mail invitations to the attendees (needs the `send` grant).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub send_invitations: bool,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// Which occurrences an event change applies to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum McpEditScope {
    /// Only the occurrence named by `occurrence`.
    This,
    /// That occurrence and every later one.
    ThisAndFollowing,
    /// The whole series.
    #[default]
    All,
}

/// `update_event`: change an event. Fields left out stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct UpdateEventArgs {
    /// The event (series).
    pub event_id: String,
    /// For recurring events: the occurrence's original start (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<String>,
    /// Which occurrences (default: all).
    #[serde(default)]
    pub scope: McpEditScope,
    /// New title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// New end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// New zone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
    /// New location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// New description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Attendees to add.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_attendees: Vec<String>,
    /// Attendees to remove.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_attendees: Vec<String>,
    /// Mail the change to the attendees (needs the `send` grant).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub send_updates: bool,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// An answer to an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum McpInviteResponse {
    /// Accept.
    Accept,
    /// Tentatively accept.
    Tentative,
    /// Decline.
    Decline,
}

/// `respond_to_invite`: answer an invitation (from its message or its
/// event).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct RespondToInviteArgs {
    /// The invitation message …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// … or the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// For recurring events: the occurrence (RFC 3339 original start);
    /// default: the whole series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<String>,
    /// The answer.
    pub response: McpInviteResponse,
    /// A note to the organizer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Tell the organizer (default `true`; needs the `send` grant).
    #[serde(default = "yes")]
    pub send: bool,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

fn yes() -> bool {
    true
}

/// `upsert_contact`: create a contact, or change one (`contact_id`).
/// Fields left out stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct UpsertContactArgs {
    /// The contact to change; `null` = create one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_id: Option<String>,
    /// For new contacts: the address book (default: the default one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address_book_id: Option<String>,
    /// Full name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    /// Email addresses (replace the existing ones).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emails: Option<Vec<String>>,
    /// Phone numbers (replace the existing ones).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phones: Option<Vec<String>>,
    /// Organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    /// Job title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// `upsert_task`: create a task, or change one (`task_id`). Fields left
/// out stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct UpsertTaskArgs {
    /// The task to change; `null` = create one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// For new tasks: the task list (default: the default one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_id: Option<String>,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Due: RFC 3339 or `YYYY-MM-DD`; `""` removes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    /// Notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Done or open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<bool>,
    /// Priority 1 (highest) … 9 (lowest); 0 = none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    /// Link the task to this message (`nail://messages/<id>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_message_id: Option<String>,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// `create_rule`: add a filter rule from its text form.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct CreateRuleArgs {
    /// One `rule "<name>" { … }` node in `rules.kdl` syntax (see
    /// `nail://rules`).
    pub text: String,
    /// Switched on (default `true`).
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Repeating the key repeats the first call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// `test_rule`: which existing messages a rule would match and what it
/// would do (changes nothing).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct TestRuleArgs {
    /// One `rule "<name>" { … }` node.
    pub text: String,
    /// Only messages of this folder (default: the rule's folders, else the
    /// inboxes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    /// Most messages to report (default 50, at most 500).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// How `edit_open_draft` changes the open composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum McpEditOp {
    /// Insert at the cursor.
    Insert,
    /// Replace the selected text (insert at the cursor without one).
    ReplaceSelection,
    /// Replace the whole body.
    ReplaceAll,
}

/// `edit_open_draft`: insert or replace text in the draft open in Nail's
/// composer; the user sees it live. Sending still needs `send_message`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct EditOpenDraftArgs {
    /// What to do.
    pub op: McpEditOp,
    /// The text.
    pub text: String,
    /// Its format (default Markdown; converted to the composer's mode).
    #[serde(default)]
    pub format: McpTextFormat,
    /// Only if this draft is the open one (from `get_ui_context`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
}

/// What a write tool answers (the structured result, `snake_case`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum McpWriteResult {
    /// Done (queued in the operation queue; undoable).
    Done {
        /// The queued operations.
        #[serde(default)]
        op_ids: Vec<String>,
        /// Ids of what was created (draft, event, contact, task, rule).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        created: Vec<String>,
        /// The confirmation the user approved, if one was needed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confirmation_id: Option<String>,
    },
    /// The user denied it; nothing changed.
    Denied {
        /// The confirmation.
        confirmation_id: String,
    },
    /// Nobody decided in time; nothing changed.
    Expired {
        /// The confirmation.
        confirmation_id: String,
    },
    /// A plug-in's before-send check stopped a `send_message`; the draft
    /// is unchanged and the user can send it from Nail.
    Vetoed {
        /// The plug-ins' reasons (`<plug-in name>: <reason>`).
        reasons: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_table_covers_the_design() {
        let names: Vec<&str> = MCP_TOOLS.iter().map(|t| t.name).collect();
        for name in [
            "update_messages",
            "delete_messages",
            "create_draft",
            "update_draft",
            "send_message",
            "create_event",
            "update_event",
            "respond_to_invite",
            "upsert_contact",
            "upsert_task",
            "create_rule",
            "test_rule",
            "edit_open_draft",
        ] {
            assert!(names.contains(&name), "{name}");
        }
        let unique: std::collections::BTreeSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
        // Every read tool of M1b / M3a stays.
        for name in crate::pim::mcp::MCP_PIM_TOOLS {
            assert!(!tool(name).unwrap().is_write());
        }
        assert!(!tool("test_rule").unwrap().is_write());
        assert_eq!(tool("send_message").unwrap().kind, Some(ActionKind::Send));
        assert_eq!(
            tool("delete_messages").unwrap().kind,
            Some(ActionKind::Trash)
        );
        assert_eq!(write_tool_names().len(), 12);
        for t in MCP_TOOLS {
            assert!(!t.scopes.is_empty(), "{}", t.name);
            if let Some(kind) = t.kind {
                // The tool asks for at least the kind's own scopes (or the
                // kind's scopes are decided per item).
                assert!(
                    kind.mcp_scopes().iter().all(|s| t.scopes.contains(s)) || t.kind_note.is_some(),
                    "{}",
                    t.name
                );
            }
        }
        assert_eq!(tool("nope"), None);
    }

    #[test]
    fn args_parse_minimal_and_full_calls() {
        let update: UpdateMessagesArgs =
            serde_json::from_value(json!({"message_ids": ["m1"], "flagged": true})).unwrap();
        assert_eq!(update.flagged, Some(true));
        assert!(!update.archive && update.add_tags.is_empty());
        assert_eq!(
            serde_json::to_value(&update).unwrap(),
            json!({"message_ids": ["m1"], "flagged": true})
        );
        let delete: DeleteMessagesArgs =
            serde_json::from_value(json!({"message_ids": ["m1"], "permanent": true})).unwrap();
        assert!(delete.permanent);
        let draft: CreateDraftArgs = serde_json::from_value(json!({
            "to": ["Ada <ada@example.com>"], "subject": "Hi", "body": "**Hello**",
            "attachments": [{"filename": "a.txt", "data_base64": "aGk="}],
            "idempotency_key": "k-1"
        }))
        .unwrap();
        assert_eq!(draft.format, McpTextFormat::Markdown);
        assert_eq!(draft.attachments[0].filename, "a.txt");
        let respond: RespondToInviteArgs =
            serde_json::from_value(json!({"message_id": "m1", "response": "tentative"})).unwrap();
        assert!(respond.send);
        assert_eq!(respond.response, McpInviteResponse::Tentative);
        let event: UpdateEventArgs =
            serde_json::from_value(json!({"event_id": "e1", "scope": "this-and-following"}))
                .unwrap();
        assert_eq!(event.scope, McpEditScope::ThisAndFollowing);
        let rule: CreateRuleArgs =
            serde_json::from_value(json!({"text": "rule \"x\" {}"})).unwrap();
        assert!(rule.enabled);
        let edit: EditOpenDraftArgs =
            serde_json::from_value(json!({"op": "replace-selection", "text": "x"})).unwrap();
        assert_eq!(edit.op, McpEditOp::ReplaceSelection);
        assert!(
            serde_json::from_value::<RespondToInviteArgs>(json!({"response": "maybe"})).is_err()
        );
    }

    #[test]
    fn results_and_keys() {
        assert_eq!(
            serde_json::to_value(McpWriteResult::Denied {
                confirmation_id: "7".into()
            })
            .unwrap(),
            json!({"status": "denied", "confirmation_id": "7"})
        );
        assert_eq!(
            serde_json::to_value(McpWriteResult::Done {
                op_ids: vec!["op-1".into()],
                created: vec![],
                confirmation_id: None
            })
            .unwrap(),
            json!({"status": "done", "op_ids": ["op-1"]})
        );
        assert!(is_idempotency_key("abc-123_X"));
        assert!(!is_idempotency_key(""));
        assert!(!is_idempotency_key("with space"));
        assert!(!is_idempotency_key(&"k".repeat(129)));
    }

    #[cfg(feature = "json-schema")]
    #[test]
    fn schemas_are_objects_with_descriptions() {
        let schema = schemars::schema_for!(CreateDraftArgs);
        let value = serde_json::to_value(&schema).unwrap();
        assert_eq!(value["type"], "object");
        assert!(value["properties"]["reply_to_message_id"].is_object());
        let required = value["required"].as_array().cloned().unwrap_or_default();
        assert!(required.is_empty(), "{required:?}");
        let send = serde_json::to_value(schemars::schema_for!(SendMessageArgs)).unwrap();
        assert_eq!(send["required"], json!(["draft_id"]));
    }
}
