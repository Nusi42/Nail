//! Plug-in writes (M4a, ADR 0079): the `nail.mail` / `nail.calendar` /
//! `nail.contacts` / `nail.tasks` write calls, turned from the plug-in's
//! camelCase arguments into [`PluginWrite`] — the MCP write tools' argument
//! types (`nail_api::mcp::tools`), so the core runs plug-in and agent writes
//! through one path (policy, confirmation, operation queue, audit) — plus
//! the deletes of calendar, contact and task items that MCP has no tool for.
//!
//! This module only parses and classifies: [`parse`] checks shapes and
//! limits, [`PluginWrite::kind`] says which [`ActionKind`] (and so which
//! policy) applies, [`PluginWrite::permissions`] which plug-in permissions
//! it needs. The runtime adds the gate, the account checks and the call
//! into the core.

use std::collections::HashMap;

use base64::Engine as _;
use nail_api::agents::ActionKind;
use nail_api::mcp::tools::{
    CreateDraftArgs, CreateEventArgs, DeleteMessagesArgs, DraftAttachmentArg, MAX_AGENT_ATTACHMENT,
    MAX_BATCH, McpConversationArg, McpEditScope, McpInviteResponse, McpTextFormat, McpWriteResult,
    RespondToInviteArgs, SendMessageArgs, UpdateDraftArgs, UpdateEventArgs, UpdateMessagesArgs,
    UpsertContactArgs, UpsertTaskArgs,
};
use nail_api::plugins::PluginPermission;
use nail_api::{ApiError, MessageId};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// Longest text field of a write (subject, title, notes, body; bytes).
pub const MAX_WRITE_TEXT: usize = 1024 * 1024;
/// Most recipients / attendees / addresses of one write.
pub const MAX_WRITE_ADDRESSES: usize = 500;
/// Most attachments one draft write adds.
pub const MAX_WRITE_ATTACHMENTS: usize = 20;

/// A write a plug-in asks for, in the MCP tools' argument shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginWrite {
    /// `nail.mail.update`: flags, tags, read state, move, archive, snooze.
    UpdateMessages(UpdateMessagesArgs),
    /// `nail.mail.delete`: Trash or permanent delete.
    DeleteMessages(DeleteMessagesArgs),
    /// `nail.mail.createDraft`.
    CreateDraft(CreateDraftArgs),
    /// `nail.mail.updateDraft`.
    UpdateDraft(UpdateDraftArgs),
    /// `nail.mail.send`.
    SendMessage(SendMessageArgs),
    /// `nail.calendar.createEvent`.
    CreateEvent(CreateEventArgs),
    /// `nail.calendar.updateEvent`.
    UpdateEvent(UpdateEventArgs),
    /// `nail.calendar.deleteEvent`.
    DeleteEvent {
        /// The event (series).
        event_id: String,
        /// For recurring events: the occurrence's original start.
        occurrence: Option<String>,
        /// Which occurrences.
        scope: McpEditScope,
    },
    /// `nail.calendar.respond`: answer an invitation.
    RespondToInvite(RespondToInviteArgs),
    /// `nail.contacts.upsert`.
    UpsertContact(UpsertContactArgs),
    /// `nail.contacts.delete`.
    DeleteContact {
        /// The contact.
        contact_id: String,
    },
    /// `nail.tasks.create` / `nail.tasks.update`.
    UpsertTask(UpsertTaskArgs),
    /// `nail.tasks.delete`.
    DeleteTask {
        /// The task.
        task_id: String,
    },
}

impl PluginWrite {
    /// The action kind whose policy applies.
    pub fn kind(&self) -> ActionKind {
        match self {
            Self::UpdateMessages(_) => ActionKind::Organize,
            Self::DeleteMessages(a) if a.permanent => ActionKind::PermanentDelete,
            Self::DeleteMessages(_) => ActionKind::Trash,
            Self::CreateDraft(_) | Self::UpdateDraft(_) => ActionKind::Draft,
            Self::SendMessage(_) => ActionKind::Send,
            Self::CreateEvent(a) if a.send_invitations => ActionKind::Invite,
            Self::UpdateEvent(a) if a.send_updates => ActionKind::Invite,
            Self::RespondToInvite(a) if a.send => ActionKind::Invite,
            Self::CreateEvent(_) | Self::UpdateEvent(_) | Self::RespondToInvite(_) => {
                ActionKind::Calendar
            }
            Self::UpsertContact(_) => ActionKind::Contact,
            Self::UpsertTask(_) => ActionKind::Task,
            Self::DeleteEvent { .. } | Self::DeleteContact { .. } | Self::DeleteTask { .. } => {
                ActionKind::PimDelete
            }
        }
    }

    /// Every plug-in permission the write needs.
    pub fn permissions(&self) -> Vec<PluginPermission> {
        match self {
            Self::DeleteEvent { .. } => vec![PluginPermission::CalendarWrite],
            Self::DeleteContact { .. } => vec![PluginPermission::ContactsWrite],
            Self::DeleteTask { .. } => vec![PluginPermission::TasksWrite],
            // Mail-dependent PIM writes: reading the message they name needs
            // `mail.read` too (checked by the runtime when one is named).
            other => other.kind().plugin_permissions(),
        }
    }

    /// The messages the write names that the plug-in must be able to read
    /// (`mail.read`): changed / deleted messages, replied-to and forwarded
    /// messages, invitation messages, messages a task links to.
    pub fn read_messages(&self) -> Vec<MessageId> {
        let ids: Vec<&String> = match self {
            Self::UpdateMessages(a) => a.message_ids.iter().collect(),
            Self::DeleteMessages(a) => a.message_ids.iter().collect(),
            Self::CreateDraft(a) => a
                .reply_to_message_id
                .iter()
                .chain(a.forward_message_id.iter())
                .collect(),
            Self::RespondToInvite(a) => a.message_id.iter().collect(),
            Self::UpsertTask(a) => a.related_message_id.iter().collect(),
            _ => Vec::new(),
        };
        ids.into_iter()
            .map(|id| MessageId::from(id.as_str()))
            .collect()
    }

    /// The messages whose attachments the write copies (their content:
    /// `mail.read.body`).
    pub fn content_messages(&self) -> Vec<MessageId> {
        let attachments = match self {
            Self::CreateDraft(a) => &a.attachments,
            Self::UpdateDraft(a) => &a.add_attachments,
            _ => return Vec::new(),
        };
        attachments
            .iter()
            .filter_map(|a| a.from_message_id.as_deref())
            .map(MessageId::from)
            .collect()
    }

    /// Whether the messages it changes must also be within `mail.organize`
    /// (message changes and deletes).
    pub fn organizes_messages(&self) -> bool {
        matches!(self, Self::UpdateMessages(_) | Self::DeleteMessages(_))
    }

    /// Whether the account-limited `mail.send` grant applies (drafts,
    /// sending, invitations).
    pub fn uses_send_scope(&self) -> bool {
        matches!(
            self.kind(),
            ActionKind::Draft | ActionKind::Send | ActionKind::Invite
        )
    }
}

/// Every write call the runtime accepts (`host_call` op names) with the
/// API name the plug-in used.
pub const WRITE_OPS: &[(&str, &str)] = &[
    ("mail.update", "nail.mail.update"),
    ("mail.delete", "nail.mail.delete"),
    ("mail.createDraft", "nail.mail.createDraft"),
    ("mail.updateDraft", "nail.mail.updateDraft"),
    ("mail.send", "nail.mail.send"),
    ("calendar.createEvent", "nail.calendar.createEvent"),
    ("calendar.updateEvent", "nail.calendar.updateEvent"),
    ("calendar.deleteEvent", "nail.calendar.deleteEvent"),
    ("calendar.respond", "nail.calendar.respond"),
    ("contacts.upsert", "nail.contacts.upsert"),
    ("contacts.delete", "nail.contacts.delete"),
    ("tasks.create", "nail.tasks.create"),
    ("tasks.update", "nail.tasks.update"),
    ("tasks.delete", "nail.tasks.delete"),
];

/// The API name of a write op, `None` for other ops.
pub fn api_name(op: &str) -> Option<&'static str> {
    WRITE_OPS.iter().find(|(o, _)| *o == op).map(|(_, n)| *n)
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::invalid_request(message)
}

fn from<T: DeserializeOwned>(value: &Value, what: &str) -> Result<T, ApiError> {
    serde_json::from_value(if value.is_null() {
        json!({})
    } else {
        value.clone()
    })
    .map_err(|e| invalid(format!("{what}: {e}")))
}

fn id(payload: &Value, field: &str) -> Result<String, ApiError> {
    let id = payload
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("`{field}` must be a string")))?;
    if id.is_empty() || id.len() > 512 {
        return Err(invalid(format!("`{field}` is not a valid id")));
    }
    Ok(id.to_owned())
}

fn ids(payload: &Value) -> Result<Vec<String>, ApiError> {
    let list = payload
        .get("ids")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("the message ids must be an array of strings"))?;
    if list.is_empty() || list.len() > MAX_BATCH {
        return Err(invalid(format!("1 to {MAX_BATCH} message ids")));
    }
    let mut out = Vec::with_capacity(list.len());
    for item in list {
        let id = item
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .ok_or_else(|| invalid("the message ids must be an array of strings"))?;
        if !out.iter().any(|x: &String| x == id) {
            out.push(id.to_owned());
        }
    }
    Ok(out)
}

/// A conversation as a plug-in names it (P3, ADR 0164).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationIn {
    thread_id: String,
    #[serde(default)]
    folder_id: Option<String>,
}

/// The targets of `nail.mail.update` / `delete`: message ids and (P3)
/// conversations (`{ threadId, folderId? }`, at most [`MAX_BATCH`]); at
/// least one of the two.
fn targets(payload: &Value) -> Result<(Vec<String>, Vec<McpConversationArg>), ApiError> {
    let conversations: Vec<ConversationIn> = match payload.get("conversations") {
        None | Some(Value::Null) => Vec::new(),
        Some(list) => serde_json::from_value(list.clone())
            .map_err(|_| invalid("conversations must be a list of { threadId, folderId? }"))?,
    };
    if conversations.is_empty() {
        return Ok((ids(payload)?, Vec::new()));
    }
    if conversations.len() > MAX_BATCH {
        return Err(invalid(format!("1 to {MAX_BATCH} conversations")));
    }
    let mut out: Vec<McpConversationArg> = Vec::with_capacity(conversations.len());
    for c in conversations {
        let valid = |s: &str| !s.is_empty() && s.len() <= 512;
        if !valid(&c.thread_id) || c.folder_id.as_deref().is_some_and(|f| !valid(f)) {
            return Err(invalid(
                "a conversation needs its threadId (and a valid folderId)",
            ));
        }
        let arg = McpConversationArg {
            thread_id: c.thread_id,
            folder_id: c.folder_id,
        };
        if !out.contains(&arg) {
            out.push(arg);
        }
    }
    let named = match payload.get("ids").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => ids(payload)?,
        _ => Vec::new(),
    };
    Ok((named, out))
}

fn text_ok(field: &str, value: Option<&String>) -> Result<(), ApiError> {
    if value.is_some_and(|v| v.len() > MAX_WRITE_TEXT) {
        return Err(ApiError::new(
            nail_api::ErrorCode::TooLarge,
            format!("`{field}` is too long"),
        ));
    }
    Ok(())
}

fn list_ok(field: &str, value: &[String]) -> Result<(), ApiError> {
    if value.len() > MAX_WRITE_ADDRESSES || value.iter().any(|v| v.len() > 998) {
        return Err(invalid(format!(
            "`{field}` has too many or too long entries"
        )));
    }
    Ok(())
}

fn format(value: Option<&str>) -> Result<Option<McpTextFormat>, ApiError> {
    Ok(match value {
        None => None,
        Some("text" | "plain") => Some(McpTextFormat::Text),
        Some("markdown") => Some(McpTextFormat::Markdown),
        Some("html") => Some(McpTextFormat::Html),
        Some(other) => return Err(invalid(format!("unknown format `{other}`"))),
    })
}

/// Bytes a plug-in handed over as `Uint8Array` / `ArrayBuffer`: the
/// prelude replaces them with `{"$nailBytes": <id>}` markers; the runtime
/// keeps the bytes by id.
pub type Blobs = HashMap<u64, Vec<u8>>;

/// The blob id of a `{"$nailBytes": id}` marker.
pub fn blob_id(value: &Value) -> Option<u64> {
    let map = value.as_object()?;
    if map.len() != 1 {
        return None;
    }
    map.get("$nailBytes")?.as_u64()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AttachmentIn {
    filename: String,
    #[serde(default)]
    mime_type: Option<String>,
    /// `Uint8Array` / `ArrayBuffer` (a blob marker).
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    data_base64: Option<String>,
    #[serde(default)]
    from_message_id: Option<String>,
    #[serde(default)]
    from_attachment_index: Option<u32>,
}

fn attachments(
    list: Vec<AttachmentIn>,
    blobs: &Blobs,
) -> Result<Vec<DraftAttachmentArg>, ApiError> {
    if list.len() > MAX_WRITE_ATTACHMENTS {
        return Err(invalid(format!(
            "at most {MAX_WRITE_ATTACHMENTS} attachments per call"
        )));
    }
    let b64 = base64::engine::general_purpose::STANDARD;
    list.into_iter()
        .map(|a| {
            if a.filename.trim().is_empty() || a.filename.len() > 255 {
                return Err(invalid("an attachment needs a file name"));
            }
            let data_base64 = match (a.data, a.data_base64) {
                (Some(marker), None) => {
                    let bytes = blob_id(&marker)
                        .and_then(|id| blobs.get(&id))
                        .ok_or_else(|| invalid("attachment `data` must be a Uint8Array or ArrayBuffer"))?;
                    if bytes.len() > MAX_AGENT_ATTACHMENT {
                        return Err(ApiError::new(
                            nail_api::ErrorCode::TooLarge,
                            "the attachment is too large",
                        ));
                    }
                    Some(b64.encode(bytes))
                }
                (None, Some(text)) => {
                    let decoded = b64
                        .decode(text.as_bytes())
                        .map_err(|_| invalid("`dataBase64` is not base64"))?;
                    if decoded.len() > MAX_AGENT_ATTACHMENT {
                        return Err(ApiError::new(
                            nail_api::ErrorCode::TooLarge,
                            "the attachment is too large",
                        ));
                    }
                    Some(text)
                }
                (None, None) => None,
                (Some(_), Some(_)) => {
                    return Err(invalid("pass either `data` or `dataBase64`"));
                }
            };
            let copies = a.from_message_id.is_some();
            if data_base64.is_some() == copies || copies != a.from_attachment_index.is_some() {
                return Err(invalid(
                    "an attachment needs `data` (or `dataBase64`), or `fromMessageId` with `fromAttachmentIndex`",
                ));
            }
            Ok(DraftAttachmentArg {
                filename: a.filename,
                mime_type: a.mime_type,
                data_base64,
                from_message_id: a.from_message_id,
                from_attachment_index: a.from_attachment_index,
            })
        })
        .collect()
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MailChanges {
    #[serde(default)]
    read: Option<bool>,
    #[serde(default)]
    flagged: Option<bool>,
    #[serde(default)]
    add_tags: Vec<String>,
    #[serde(default)]
    remove_tags: Vec<String>,
    #[serde(default)]
    move_to_folder_id: Option<String>,
    #[serde(default)]
    archive: bool,
    #[serde(default)]
    snooze_until: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteOptions {
    #[serde(default)]
    permanent: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftIn {
    #[serde(default)]
    identity_id: Option<String>,
    #[serde(default)]
    to: Vec<String>,
    #[serde(default)]
    cc: Vec<String>,
    #[serde(default)]
    bcc: Vec<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    reply_to_message_id: Option<String>,
    #[serde(default)]
    reply_all: bool,
    #[serde(default)]
    forward_message_id: Option<String>,
    #[serde(default)]
    attachments: Vec<AttachmentIn>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftChanges {
    #[serde(default)]
    to: Option<Vec<String>>,
    #[serde(default)]
    cc: Option<Vec<String>>,
    #[serde(default)]
    bcc: Option<Vec<String>>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    add_attachments: Vec<AttachmentIn>,
    #[serde(default)]
    remove_attachments: Vec<u32>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SendOptions {
    #[serde(default)]
    send_at: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventIn {
    #[serde(default)]
    calendar_id: Option<String>,
    title: String,
    start: String,
    end: String,
    #[serde(default)]
    tz: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    attendees: Vec<String>,
    #[serde(default)]
    recurrence: Option<String>,
    #[serde(default)]
    send_invitations: bool,
}

fn edit_scope(value: Option<&str>) -> Result<McpEditScope, ApiError> {
    Ok(match value {
        None | Some("all") => McpEditScope::All,
        Some("this") => McpEditScope::This,
        Some("thisAndFollowing" | "this-and-following") => McpEditScope::ThisAndFollowing,
        Some(other) => return Err(invalid(format!("unknown scope `{other}`"))),
    })
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventChanges {
    #[serde(default)]
    occurrence: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    tz: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    add_attendees: Vec<String>,
    #[serde(default)]
    remove_attendees: Vec<String>,
    #[serde(default)]
    send_updates: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OccurrenceOptions {
    #[serde(default)]
    occurrence: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RespondIn {
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    occurrence: Option<String>,
    response: String,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default = "yes")]
    send: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContactIn {
    #[serde(default)]
    contact_id: Option<String>,
    #[serde(default)]
    address_book_id: Option<String>,
    #[serde(default)]
    full_name: Option<String>,
    #[serde(default)]
    emails: Option<Vec<String>>,
    #[serde(default)]
    phones: Option<Vec<String>>,
    #[serde(default)]
    organization: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskIn {
    /// The design's example says `list`; `listId` works too.
    #[serde(default)]
    list: Option<String>,
    #[serde(default)]
    list_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    due: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    completed: Option<bool>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    related_message_id: Option<String>,
    /// `nail://messages/<id>` links (the design's example); at most one
    /// message link.
    #[serde(default)]
    links: Vec<String>,
}

/// The message id of a `nail://messages/<id>` URI.
pub fn message_from_uri(uri: &str) -> Option<&str> {
    uri.strip_prefix("nail://messages/")
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn task(input: TaskIn, task_id: Option<String>) -> Result<UpsertTaskArgs, ApiError> {
    for field in [&input.title, &input.notes, &input.due] {
        text_ok("task", field.as_ref())?;
    }
    if input.priority.is_some_and(|p| p > 9) {
        return Err(invalid("`priority` is 0 (none) or 1 (highest) to 9"));
    }
    if input.list.is_some() && input.list_id.is_some() {
        return Err(invalid("pass either `list` or `listId`"));
    }
    let mut related = input.related_message_id;
    for link in &input.links {
        let id = message_from_uri(link)
            .ok_or_else(|| invalid(format!("`{link}` is not a nail://messages/ link")))?;
        if related.as_deref().is_some_and(|r| r != id) {
            return Err(invalid("a task links to at most one message"));
        }
        related = Some(id.to_owned());
    }
    if task_id.is_none() && input.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
        return Err(invalid("a new task needs a title"));
    }
    if task_id.is_some() && (input.list.is_some() || input.list_id.is_some()) {
        return Err(invalid("tasks cannot be moved to another list here"));
    }
    Ok(UpsertTaskArgs {
        task_id,
        list_id: input.list.or(input.list_id),
        title: input.title,
        due: input.due,
        notes: input.notes,
        completed: input.completed,
        priority: input.priority,
        related_message_id: related,
        idempotency_key: None,
    })
}

/// Parses a write call (`op` from [`WRITE_OPS`]) with the plug-in's
/// payload; `blobs` holds the bytes the call handed over.
pub fn parse(op: &str, payload: &Value, blobs: &Blobs) -> Result<PluginWrite, ApiError> {
    let options = payload.get("options").cloned().unwrap_or(Value::Null);
    Ok(match op {
        "mail.update" => {
            let changes: MailChanges = from(&options, "changes")?;
            list_ok("addTags", &changes.add_tags)?;
            list_ok("removeTags", &changes.remove_tags)?;
            let any = changes.read.is_some()
                || changes.flagged.is_some()
                || !changes.add_tags.is_empty()
                || !changes.remove_tags.is_empty()
                || changes.move_to_folder_id.is_some()
                || changes.archive
                || changes.snooze_until.is_some();
            if !any {
                return Err(invalid("nothing to change"));
            }
            if changes.archive && changes.move_to_folder_id.is_some() {
                return Err(invalid("pass either `archive` or `moveToFolderId`"));
            }
            let (message_ids, conversations) = targets(payload)?;
            PluginWrite::UpdateMessages(UpdateMessagesArgs {
                message_ids,
                conversations,
                read: changes.read,
                flagged: changes.flagged,
                add_tags: changes.add_tags,
                remove_tags: changes.remove_tags,
                move_to_folder_id: changes.move_to_folder_id,
                archive: changes.archive,
                snooze_until: changes.snooze_until,
                idempotency_key: None,
            })
        }
        "mail.delete" => {
            let o: DeleteOptions = from(&options, "options")?;
            let (message_ids, conversations) = targets(payload)?;
            PluginWrite::DeleteMessages(DeleteMessagesArgs {
                message_ids,
                conversations,
                permanent: o.permanent,
                idempotency_key: None,
            })
        }
        "mail.createDraft" => {
            let d: DraftIn = from(&options, "draft")?;
            for (field, list) in [("to", &d.to), ("cc", &d.cc), ("bcc", &d.bcc)] {
                list_ok(field, list)?;
            }
            text_ok("subject", d.subject.as_ref())?;
            text_ok("body", Some(&d.body))?;
            if d.reply_to_message_id.is_some() && d.forward_message_id.is_some() {
                return Err(invalid("a draft replies to or forwards one message"));
            }
            PluginWrite::CreateDraft(CreateDraftArgs {
                identity_id: d.identity_id,
                to: d.to,
                cc: d.cc,
                bcc: d.bcc,
                subject: d.subject,
                body: d.body,
                format: format(d.format.as_deref())?.unwrap_or_default(),
                reply_to_message_id: d.reply_to_message_id,
                reply_all: d.reply_all,
                forward_message_id: d.forward_message_id,
                attachments: attachments(d.attachments, blobs)?,
                idempotency_key: None,
            })
        }
        "mail.updateDraft" => {
            let c: DraftChanges = from(&options, "changes")?;
            for (field, list) in [("to", &c.to), ("cc", &c.cc), ("bcc", &c.bcc)] {
                list_ok(field, list.as_deref().unwrap_or_default())?;
            }
            text_ok("subject", c.subject.as_ref())?;
            text_ok("body", c.body.as_ref())?;
            PluginWrite::UpdateDraft(UpdateDraftArgs {
                draft_id: id(payload, "draftId")?,
                to: c.to,
                cc: c.cc,
                bcc: c.bcc,
                subject: c.subject,
                body: c.body,
                format: format(c.format.as_deref())?,
                add_attachments: attachments(c.add_attachments, blobs)?,
                remove_attachments: c.remove_attachments,
                idempotency_key: None,
            })
        }
        "mail.send" => {
            let o: SendOptions = from(&options, "options")?;
            PluginWrite::SendMessage(SendMessageArgs {
                draft_id: id(payload, "draftId")?,
                send_at: o.send_at,
                idempotency_key: None,
            })
        }
        "calendar.createEvent" => {
            let e: EventIn = from(&options, "event")?;
            if e.title.trim().is_empty() {
                return Err(invalid("an event needs a title"));
            }
            for field in [&e.location, &e.description, &e.recurrence] {
                text_ok("event", field.as_ref())?;
            }
            text_ok("title", Some(&e.title))?;
            list_ok("attendees", &e.attendees)?;
            PluginWrite::CreateEvent(CreateEventArgs {
                calendar_id: e.calendar_id,
                title: e.title,
                start: e.start,
                end: e.end,
                tz: e.tz,
                location: e.location,
                description: e.description,
                attendees: e.attendees,
                recurrence: e.recurrence,
                send_invitations: e.send_invitations,
                idempotency_key: None,
            })
        }
        "calendar.updateEvent" => {
            let c: EventChanges = from(&options, "changes")?;
            for field in [&c.title, &c.location, &c.description] {
                text_ok("event", field.as_ref())?;
            }
            list_ok("addAttendees", &c.add_attendees)?;
            list_ok("removeAttendees", &c.remove_attendees)?;
            PluginWrite::UpdateEvent(UpdateEventArgs {
                event_id: id(payload, "eventId")?,
                occurrence: c.occurrence,
                scope: edit_scope(c.scope.as_deref())?,
                title: c.title,
                start: c.start,
                end: c.end,
                tz: c.tz,
                location: c.location,
                description: c.description,
                add_attendees: c.add_attendees,
                remove_attendees: c.remove_attendees,
                send_updates: c.send_updates,
                idempotency_key: None,
            })
        }
        "calendar.deleteEvent" => {
            let o: OccurrenceOptions = from(&options, "options")?;
            PluginWrite::DeleteEvent {
                event_id: id(payload, "eventId")?,
                occurrence: o.occurrence,
                scope: edit_scope(o.scope.as_deref())?,
            }
        }
        "calendar.respond" => {
            let r: RespondIn = from(&options, "answer")?;
            if r.message_id.is_some() == r.event_id.is_some() {
                return Err(invalid("pass either `messageId` or `eventId`"));
            }
            text_ok("comment", r.comment.as_ref())?;
            let response = match r.response.as_str() {
                "accept" => McpInviteResponse::Accept,
                "tentative" => McpInviteResponse::Tentative,
                "decline" => McpInviteResponse::Decline,
                other => return Err(invalid(format!("unknown response `{other}`"))),
            };
            PluginWrite::RespondToInvite(RespondToInviteArgs {
                message_id: r.message_id,
                event_id: r.event_id,
                occurrence: r.occurrence,
                response,
                comment: r.comment,
                send: r.send,
                idempotency_key: None,
            })
        }
        "contacts.upsert" => {
            let c: ContactIn = from(&options, "contact")?;
            for field in [&c.full_name, &c.organization, &c.title, &c.notes] {
                text_ok("contact", field.as_ref())?;
            }
            list_ok("emails", c.emails.as_deref().unwrap_or_default())?;
            list_ok("phones", c.phones.as_deref().unwrap_or_default())?;
            if c.contact_id.is_some() && c.address_book_id.is_some() {
                return Err(invalid(
                    "contacts cannot be moved to another address book here",
                ));
            }
            PluginWrite::UpsertContact(UpsertContactArgs {
                contact_id: c.contact_id,
                address_book_id: c.address_book_id,
                full_name: c.full_name,
                emails: c.emails,
                phones: c.phones,
                organization: c.organization,
                title: c.title,
                notes: c.notes,
                idempotency_key: None,
            })
        }
        "contacts.delete" => PluginWrite::DeleteContact {
            contact_id: id(payload, "contactId")?,
        },
        "tasks.create" => PluginWrite::UpsertTask(task(from(&options, "task")?, None)?),
        "tasks.update" => {
            let task_id = id(payload, "taskId")?;
            PluginWrite::UpsertTask(task(from(&options, "changes")?, Some(task_id))?)
        }
        "tasks.delete" => PluginWrite::DeleteTask {
            task_id: id(payload, "taskId")?,
        },
        other => return Err(invalid(format!("unknown write `{other}`"))),
    })
}

/// A write's result as the plug-in sees it (camelCase): `{status: "done",
/// opIds, created, confirmationId?}`, `{status: "denied" | "expired",
/// confirmationId}`, `{status: "vetoed", reasons}`.
pub fn result_json(result: &McpWriteResult) -> Value {
    match result {
        McpWriteResult::Done {
            op_ids,
            created,
            confirmation_id,
        } => {
            let mut v = json!({ "status": "done", "opIds": op_ids, "created": created });
            if let Some(id) = confirmation_id {
                v["confirmationId"] = json!(id);
            }
            v
        }
        McpWriteResult::Denied { confirmation_id } => {
            json!({ "status": "denied", "confirmationId": confirmation_id })
        }
        McpWriteResult::Expired { confirmation_id } => {
            json!({ "status": "expired", "confirmationId": confirmation_id })
        }
        McpWriteResult::Vetoed { reasons } => json!({ "status": "vetoed", "reasons": reasons }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn p(op: &str, payload: &Value) -> Result<PluginWrite, ApiError> {
        parse(op, payload, &Blobs::new())
    }

    #[test]
    fn every_op_has_an_api_name() {
        for (op, name) in WRITE_OPS {
            assert_eq!(api_name(op), Some(*name));
            assert_eq!(name.strip_prefix("nail.").unwrap(), *op);
        }
        assert_eq!(api_name("mail.get"), None);
    }

    #[test]
    fn mail_updates_and_deletes() {
        let w = p(
            "mail.update",
            &json!({"ids": ["m1", "m2", "m1"], "options": {"addTags": ["crm"], "flagged": true}}),
        )
        .unwrap();
        let PluginWrite::UpdateMessages(args) = &w else {
            panic!()
        };
        assert_eq!(args.message_ids, ["m1", "m2"]);
        assert_eq!(args.add_tags, ["crm"]);
        assert_eq!(w.kind(), ActionKind::Organize);
        assert_eq!(w.permissions(), [PluginPermission::MailOrganize]);
        assert!(w.organizes_messages() && !w.uses_send_scope());
        assert_eq!(w.read_messages(), [MessageId::from("m1"), "m2".into()]);

        for bad in [
            json!({"ids": ["m1"], "options": {}}),
            json!({"ids": [], "options": {"read": true}}),
            json!({"ids": "m1", "options": {"read": true}}),
            json!({"ids": ["m1"], "options": {"raed": true}}), // spellchecker:disable-line
            json!({"ids": ["m1"], "options": {"archive": true, "moveToFolderId": "f"}}),
        ] {
            assert!(p("mail.update", &bad.clone()).is_err(), "{bad}");
        }
        let many: Vec<String> = (0..=MAX_BATCH).map(|i| format!("m{i}")).collect();
        assert!(
            p(
                "mail.update",
                &json!({"ids": many, "options": {"read": true}})
            )
            .is_err()
        );

        // P3: conversations, alone or beside message ids.
        let w = p(
            "mail.update",
            &json!({"ids": [], "conversations": [{"threadId": "t1", "folderId": "f-in"}, {"threadId": "t2"}],
                    "options": {"archive": true}}),
        )
        .unwrap();
        let PluginWrite::UpdateMessages(args) = &w else {
            panic!()
        };
        assert!(args.message_ids.is_empty(), "{:?}", args.message_ids);
        assert_eq!(
            args.conversations,
            [
                McpConversationArg {
                    thread_id: "t1".into(),
                    folder_id: Some("f-in".into())
                },
                McpConversationArg {
                    thread_id: "t2".into(),
                    folder_id: None
                }
            ]
        );
        let w = p(
            "mail.delete",
            &json!({"ids": ["m9"], "conversations": [{"threadId": "t1"}], "options": {"permanent": true}}),
        )
        .unwrap();
        let PluginWrite::DeleteMessages(args) = &w else {
            panic!()
        };
        assert_eq!((args.message_ids.len(), args.conversations.len()), (1, 1));
        assert_eq!(w.kind(), ActionKind::PermanentDelete);
        for bad in [
            json!({"conversations": [{"threadId": ""}], "options": {"read": true}}),
            json!({"conversations": [{"thread": "t1"}], "options": {"read": true}}),
            json!({"conversations": "t1", "options": {"read": true}}),
            json!({"conversations": [], "options": {"read": true}}),
        ] {
            assert!(p("mail.update", &bad.clone()).is_err(), "{bad}");
        }
        let many: Vec<Value> = (0..=MAX_BATCH)
            .map(|i| json!({"threadId": format!("t{i}")}))
            .collect();
        assert!(
            p(
                "mail.update",
                &json!({"conversations": many, "options": {"read": true}})
            )
            .is_err()
        );

        let trash = p("mail.delete", &json!({"ids": ["m1"]})).unwrap();
        assert_eq!(trash.kind(), ActionKind::Trash);
        let gone = p(
            "mail.delete",
            &json!({"ids": ["m1"], "options": {"permanent": true}}),
        )
        .unwrap();
        assert_eq!(gone.kind(), ActionKind::PermanentDelete);
        assert_eq!(gone.permissions(), [PluginPermission::MailOrganize]);
    }

    #[test]
    fn drafts_and_sending() {
        let mut blobs = Blobs::new();
        blobs.insert(7, b"hello".to_vec());
        let w = parse(
            "mail.createDraft",
            &json!({"options": {
                "to": ["Ada <ada@example.com>"], "subject": "Hi", "body": "**x**",
                "replyToMessageId": "m1",
                "attachments": [
                    {"filename": "a.txt", "data": {"$nailBytes": 7}},
                    {"filename": "b.txt", "dataBase64": "aGk="},
                    {"filename": "c.pdf", "fromMessageId": "m2", "fromAttachmentIndex": 0}
                ]
            }}),
            &blobs,
        )
        .unwrap();
        let PluginWrite::CreateDraft(args) = &w else {
            panic!()
        };
        assert_eq!(args.format, McpTextFormat::Markdown);
        assert_eq!(args.attachments[0].data_base64.as_deref(), Some("aGVsbG8="));
        assert_eq!(args.attachments[1].data_base64.as_deref(), Some("aGk="));
        assert_eq!(w.kind(), ActionKind::Draft);
        assert_eq!(w.permissions(), [PluginPermission::MailSend]);
        assert!(w.uses_send_scope());
        assert_eq!(w.read_messages(), [MessageId::from("m1")]);
        assert_eq!(w.content_messages(), [MessageId::from("m2")]);

        for bad in [
            json!({"options": {"attachments": [{"filename": "a", "data": {"$nailBytes": 99}}]}}),
            json!({"options": {"attachments": [{"filename": "a"}]}}),
            json!({"options": {"attachments": [{"filename": "a", "dataBase64": "!!"}]}}),
            json!({"options": {"attachments": [{"filename": "a", "fromMessageId": "m"}]}}),
            json!({"options": {"format": "rtf"}}),
            json!({"options": {"replyToMessageId": "a", "forwardMessageId": "b"}}),
        ] {
            assert!(parse("mail.createDraft", &bad, &blobs).is_err(), "{bad}");
        }
        let update = p(
            "mail.updateDraft",
            &json!({"draftId": "d1", "options": {"subject": "New", "format": "html"}}),
        )
        .unwrap();
        assert_eq!(update.kind(), ActionKind::Draft);
        let send = p("mail.send", &json!({"draftId": "d1"})).unwrap();
        assert_eq!(send.kind(), ActionKind::Send);
        assert!(p("mail.send", &json!({})).is_err());
    }

    #[test]
    fn calendar_writes() {
        let plain = p(
            "calendar.createEvent",
            &json!({"options": {"title": "Sync", "start": "2026-10-05T09:00:00Z", "end": "2026-10-05T09:30:00Z"}}),
        )
        .unwrap();
        assert_eq!(plain.kind(), ActionKind::Calendar);
        assert_eq!(plain.permissions(), [PluginPermission::CalendarWrite]);
        let invite = p(
            "calendar.createEvent",
            &json!({"options": {"title": "Sync", "start": "a", "end": "b",
                               "attendees": ["bob@example.com"], "sendInvitations": true}}),
        )
        .unwrap();
        assert_eq!(invite.kind(), ActionKind::Invite);
        assert_eq!(
            invite.permissions(),
            [PluginPermission::CalendarWrite, PluginPermission::MailSend]
        );
        let update = p(
            "calendar.updateEvent",
            &json!({"eventId": "e1", "options": {"scope": "thisAndFollowing", "occurrence": "x", "title": "T"}}),
        )
        .unwrap();
        let PluginWrite::UpdateEvent(args) = &update else {
            panic!()
        };
        assert_eq!(args.scope, McpEditScope::ThisAndFollowing);
        let delete = p("calendar.deleteEvent", &json!({"eventId": "e1"})).unwrap();
        assert_eq!(delete.kind(), ActionKind::PimDelete);
        assert_eq!(delete.permissions(), [PluginPermission::CalendarWrite]);
        let respond = p(
            "calendar.respond",
            &json!({"options": {"messageId": "m1", "response": "accept"}}),
        )
        .unwrap();
        assert_eq!(respond.kind(), ActionKind::Invite);
        assert_eq!(respond.read_messages(), [MessageId::from("m1")]);
        let quiet = p(
            "calendar.respond",
            &json!({"options": {"eventId": "e1", "response": "decline", "send": false}}),
        )
        .unwrap();
        assert_eq!(quiet.kind(), ActionKind::Calendar);
        for (op, bad) in [
            (
                "calendar.createEvent",
                json!({"options": {"title": " ", "start": "a", "end": "b"}}),
            ),
            (
                "calendar.respond",
                json!({"options": {"response": "accept"}}),
            ),
            (
                "calendar.respond",
                json!({"options": {"eventId": "e", "response": "maybe"}}),
            ),
            (
                "calendar.deleteEvent",
                json!({"eventId": "e", "options": {"scope": "some"}}),
            ),
        ] {
            assert!(p(op, &bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn contacts_and_tasks() {
        let c = p(
            "contacts.upsert",
            &json!({"options": {"fullName": "Ada", "emails": ["ada@example.com"]}}),
        )
        .unwrap();
        assert_eq!(c.kind(), ActionKind::Contact);
        assert_eq!(c.permissions(), [PluginPermission::ContactsWrite]);
        let d = p("contacts.delete", &json!({"contactId": "c1"})).unwrap();
        assert_eq!(d.kind(), ActionKind::PimDelete);
        assert_eq!(d.permissions(), [PluginPermission::ContactsWrite]);

        // The design's Follow-up example.
        let t = p(
            "tasks.create",
            &json!({"options": {"list": "inbox-list", "title": "Follow up: Hi", "due": "2026-10-03",
                               "links": ["nail://messages/m1"]}}),
        )
        .unwrap();
        let PluginWrite::UpsertTask(args) = &t else {
            panic!()
        };
        assert_eq!(args.list_id.as_deref(), Some("inbox-list"));
        assert_eq!(args.related_message_id.as_deref(), Some("m1"));
        assert_eq!(t.kind(), ActionKind::Task);
        assert_eq!(t.permissions(), [PluginPermission::TasksWrite]);
        assert_eq!(t.read_messages(), [MessageId::from("m1")]);
        let u = p(
            "tasks.update",
            &json!({"taskId": "t1", "options": {"completed": true}}),
        )
        .unwrap();
        assert!(matches!(
            u,
            PluginWrite::UpsertTask(UpsertTaskArgs {
                task_id: Some(_),
                ..
            })
        ));
        let del = p("tasks.delete", &json!({"taskId": "t1"})).unwrap();
        assert_eq!(del.permissions(), [PluginPermission::TasksWrite]);
        for (op, bad) in [
            ("tasks.create", json!({"options": {}})),
            (
                "tasks.create",
                json!({"options": {"title": "x", "links": ["https://x"]}}),
            ),
            (
                "tasks.create",
                json!({"options": {"title": "x", "priority": 12}}),
            ),
            (
                "tasks.create",
                json!({"options": {"title": "x", "list": "a", "listId": "b"}}),
            ),
            (
                "tasks.update",
                json!({"taskId": "t", "options": {"list": "a"}}),
            ),
            (
                "contacts.upsert",
                json!({"options": {"contactId": "c", "addressBookId": "b"}}),
            ),
            ("contacts.delete", json!({})),
            ("unknown.op", json!({})),
        ] {
            assert!(p(op, &bad.clone()).is_err(), "{op} {bad}");
        }
    }

    #[test]
    fn results_are_camel_case() {
        let done = McpWriteResult::Done {
            op_ids: vec!["op1".into()],
            created: vec!["t1".into()],
            confirmation_id: Some("c1".into()),
        };
        assert_eq!(
            result_json(&done),
            json!({"status": "done", "opIds": ["op1"], "created": ["t1"], "confirmationId": "c1"})
        );
        assert_eq!(
            result_json(&McpWriteResult::Denied {
                confirmation_id: "c2".into()
            }),
            json!({"status": "denied", "confirmationId": "c2"})
        );
        assert_eq!(
            result_json(&McpWriteResult::Vetoed {
                reasons: vec!["x".into()]
            })["status"],
            "vetoed"
        );
        assert_eq!(blob_id(&json!({"$nailBytes": 3})), Some(3));
        assert_eq!(blob_id(&json!({"$nailBytes": 3, "x": 1})), None);
        assert_eq!(message_from_uri("nail://messages/m1"), Some("m1"));
        assert_eq!(message_from_uri("nail://messages/"), None);
    }
}
