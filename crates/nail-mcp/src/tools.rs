//! The tools (design.md, "MCP server", Tools): the read tools
//! `search_messages`, `get_message`, `get_thread`, `list_folders`,
//! `get_ui_context`, the PIM reads (M3a) and `test_rule`, and the write
//! tools of M4a (`crate::writes`), in the order of
//! [`nail_api::mcp::tools::MCP_TOOLS`].
//!
//! Tools wrap the resources for clients without resource support. Results
//! are structured JSON; anything from the mailbox is wrapped in the
//! untrusted-data envelope ([`crate::content`]). A refused call is a tool
//! error naming the missing grant (not a protocol error), so the agent can
//! tell the user what to grant.

use std::sync::Arc;

use nail_api::mcp::tools::{
    CreateDraftArgs, CreateEventArgs, CreateRuleArgs, DeleteMessagesArgs, EditOpenDraftArgs,
    RespondToInviteArgs, SendMessageArgs, TestRuleArgs, UpdateDraftArgs, UpdateEventArgs,
    UpdateMessagesArgs, UpsertContactArgs, UpsertTaskArgs,
};
use nail_api::mcp::{McpCallKind, McpClient, McpOutcome, McpScope};
use nail_api::search::RESTRICTED_SCAN;
use nail_api::search::SearchSort;
use nail_api::{AccountId, FolderId, MessageId, MessageSummary, SearchQuery, ThreadId};
use rmcp::ErrorData;
use rmcp::handler::server::common::schema_for_type;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, JsonObject, Tool, ToolAnnotations,
};
use rmcp::schemars::JsonSchema;
use rmcp::service::{RequestContext, RoleServer};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::content::untrusted;
use crate::data::{McpData, SearchOutcome};
use crate::server::{Failure, Shared, summary_json};
use crate::uri::MAX_PAGE;

/// `search_messages` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchArgs {
    /// Query in Nail's search language, e.g. `from:ada is:unread after:2026-01-01 "exact phrase"`
    /// (fields: from: to: subject: has:attachment is:unread is:flagged tag: account: folder:
    /// before: after: size>; AND / OR / NOT and parentheses).
    pub query: String,
    /// Only this account (real or virtual).
    #[serde(default)]
    pub account_id: Option<String>,
    /// Only this folder.
    #[serde(default)]
    pub folder_id: Option<String>,
    /// Index of the first hit (paging).
    #[serde(default)]
    pub offset: u32,
    /// Maximum number of hits (default 20, at most 200).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// `get_message` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct MessageArgs {
    /// Message id (from search results, folders or threads).
    pub message_id: String,
}

/// `get_thread` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ThreadArgs {
    /// Thread id (a message's `thread_id`).
    pub thread_id: String,
}

/// `list_folders` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct FoldersArgs {
    /// Account id (from `nail://accounts`).
    pub account_id: String,
}

/// `get_ui_context` takes no arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct NoArgs {}

/// Names of the tools, in [`nail_api::mcp::tools::MCP_TOOLS`] order (M3a
/// adds the read-only PIM tools, M4a `test_rule` and the writes).
pub const TOOL_NAMES: [&str; 22] = [
    "search_messages",
    "get_message",
    "get_thread",
    "list_folders",
    "get_ui_context",
    "list_events",
    "find_free_time",
    "search_contacts",
    "list_tasks",
    "test_rule",
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
    "edit_open_draft",
];

fn read_only(name: &'static str, description: &'static str, schema: Arc<JsonObject>) -> Tool {
    let mut tool = Tool::new(name, description, schema);
    tool.annotations = Some(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    );
    tool
}

/// A write tool: not read-only; `destructive` / `sends` set the hints.
fn write(
    name: &'static str,
    description: &'static str,
    schema: Arc<JsonObject>,
    destructive: bool,
    sends: bool,
) -> Tool {
    let mut tool = Tool::new(name, description, schema);
    tool.annotations = Some(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(destructive)
            .idempotent(false)
            .open_world(sends),
    );
    tool
}

/// The tool list.
pub fn definitions() -> Vec<Tool> {
    let mut tools = read_definitions();
    tools.extend(write_definitions());
    tools
}

fn write_definitions() -> Vec<Tool> {
    vec![
        write(
            "update_messages",
            "Flag, tag, mark read, move, archive or snooze up to 500 messages (organize grant; the \
             messages' accounts and folders must be granted). Runs through Nail's operation queue \
             (undoable); asks the user first only if they set this client to ask.",
            schema_for_type::<UpdateMessagesArgs>(),
            false,
            false,
        ),
        write(
            "delete_messages",
            "Move messages to Trash (organize grant) or, with permanent: true, delete them for good \
             (also needs the permanent-delete grant). Always waits until the user confirms in Nail; \
             a denial or timeout changes nothing.",
            schema_for_type::<DeleteMessagesArgs>(),
            true,
            false,
        ),
        write(
            "create_draft",
            "Save a new draft (draft grant): recipients, subject, a Markdown, HTML or text body, \
             attachments, or a reply to / forward of a message (which needs the read grant for it). \
             Never sends; use send_message. Returns the draft id in `created`.",
            schema_for_type::<CreateDraftArgs>(),
            false,
            false,
        ),
        write(
            "update_draft",
            "Change a saved draft (draft grant): recipients, subject, body, attachments.",
            schema_for_type::<UpdateDraftArgs>(),
            false,
            false,
        ),
        write(
            "send_message",
            "Send a saved draft (send grant). Waits until the user confirms in Nail unless they \
             allowed this client to send without asking; counts against the client's sends per hour.",
            schema_for_type::<SendMessageArgs>(),
            false,
            true,
        ),
        write(
            "create_event",
            "Create a calendar event (calendar grant). With attendees and send_invitations it mails \
             invitations, which needs the send grant and the user's confirmation.",
            schema_for_type::<CreateEventArgs>(),
            false,
            true,
        ),
        write(
            "update_event",
            "Change an event, one occurrence or an occurrence and the following ones (calendar \
             grant). send_updates mails the attendees (send grant, confirmed by the user).",
            schema_for_type::<UpdateEventArgs>(),
            false,
            true,
        ),
        write(
            "respond_to_invite",
            "Accept, tentatively accept or decline an invitation, from its message or its event \
             (calendar grant; telling the organizer needs the send grant and the user's confirmation).",
            schema_for_type::<RespondToInviteArgs>(),
            false,
            true,
        ),
        write(
            "upsert_contact",
            "Create a contact or change one (pim grant).",
            schema_for_type::<UpsertContactArgs>(),
            false,
            false,
        ),
        write(
            "upsert_task",
            "Create a task or change one, optionally linked to a message (pim grant).",
            schema_for_type::<UpsertTaskArgs>(),
            false,
            false,
        ),
        write(
            "create_rule",
            "Add a filter rule from its rules.kdl text form (rules grant for all accounts). Waits \
             until the user confirms in Nail unless they allowed rule changes for this client. Try \
             it first with test_rule.",
            schema_for_type::<CreateRuleArgs>(),
            false,
            false,
        ),
        write(
            "edit_open_draft",
            "Insert or replace text in the draft open in Nail's composer; the user sees it live \
             (context and draft grants). Sending still needs send_message.",
            schema_for_type::<EditOpenDraftArgs>(),
            false,
            false,
        ),
    ]
}

fn read_definitions() -> Vec<Tool> {
    vec![
        read_only(
            "search_messages",
            "Search the user's mail with Nail's query language. Returns message ids and summaries \
             (needs the read grant; only granted accounts are searched). Page with offset / next_offset.",
            schema_for_type::<SearchArgs>(),
        ),
        read_only(
            "get_message",
            "One message: headers, text and sanitized HTML body, flags, tags, thread id and \
             attachment list (read grant; encrypted messages also need the encrypted grant).",
            schema_for_type::<MessageArgs>(),
        ),
        read_only(
            "get_thread",
            "All messages of a thread, oldest first (read grant).",
            schema_for_type::<ThreadArgs>(),
        ),
        read_only(
            "list_folders",
            "Folders of an account with roles and unread/total counts (read grant).",
            schema_for_type::<FoldersArgs>(),
        ),
        read_only(
            "get_ui_context",
            "What the user has on screen now: the open message (with selected text), the selected \
             messages and the open draft (needs the context grant; Nail shows an indicator).",
            schema_for_type::<NoArgs>(),
        ),
        read_only(
            "list_events",
            "Calendar events in a range (default: the next 30 days): occurrences of recurring              events expanded, plus the event series and their iCalendar text (read grant).",
            schema_for_type::<crate::pim::ListEventsArgs>(),
        ),
        read_only(
            "find_free_time",
            "Free stretches of at least duration_minutes between busy events, optionally within              working hours (day_start / day_end) in a time zone (read grant).",
            schema_for_type::<crate::pim::FreeTimeArgs>(),
        ),
        read_only(
            "search_contacts",
            "Contacts whose name, email address, phone number or organization matches (read grant).",
            schema_for_type::<crate::pim::SearchContactsArgs>(),
        ),
        read_only(
            "list_tasks",
            "Tasks of the task lists, open ones by default, optionally only those due before a              time (read grant).",
            schema_for_type::<crate::pim::ListTasksArgs>(),
        ),
        read_only(
            "test_rule",
            "Dry run of a filter rule in rules.kdl text form: which existing messages it would match \
             and what it would do. Changes nothing (read and rules grants for all accounts).",
            schema_for_type::<TestRuleArgs>(),
        ),
    ]
}

/// The call's arguments as `T` (`invalid_params` when they do not fit).
pub(crate) fn args<T: for<'de> Deserialize<'de>>(
    request: &CallToolRequestParams,
) -> Result<T, ErrorData> {
    let value = Value::Object(request.arguments.clone().unwrap_or_default());
    serde_json::from_value(value)
        .map_err(|e| ErrorData::invalid_params(format!("invalid arguments: {e}"), None))
}

/// A successful tool result carrying mailbox content.
fn untrusted_result(data: Value) -> CallToolResult {
    CallToolResult::structured(untrusted(data))
}

use crate::writes::failure_result;

/// Runs a tool call for `client`, audited (writes audit when they end).
pub(crate) async fn call<D: McpData>(
    shared: &Arc<Shared<D>>,
    client: &McpClient,
    request: CallToolRequestParams,
    context: &RequestContext<RoleServer>,
) -> Result<CallToolResponse, ErrorData> {
    let name = request.name.to_string();
    if let Err(limited) = shared.count_request(client) {
        let result = shared
            .audited(
                client,
                McpCallKind::Tool,
                &crate::http::audited_path(&name),
                Err::<(), _>(limited),
            )
            .await;
        return Ok(result
            .err()
            .map_or_else(
                || CallToolResult::structured(json!({})),
                |f| failure_result(&f),
            )
            .into());
    }
    if let Some(spec) = nail_api::mcp::tools::tool(&name)
        && spec.is_write()
    {
        return crate::writes::call(shared, client, spec, &request, context).await;
    }
    if nail_api::mcp::tools::tool(&name).is_none() && name.contains("__") {
        let result = plugin_call(shared, client, &name, &request).await;
        let result = shared
            .audited(
                client,
                McpCallKind::Tool,
                &crate::http::audited_path(&name),
                result,
            )
            .await;
        return Ok(result
            .unwrap_or_else(|failure| failure_result(&failure))
            .into());
    }
    match dispatch(shared, client, &name, &request).await {
        Ok(result) => {
            let result = shared
                .audited(client, McpCallKind::Tool, &name, result)
                .await;
            Ok(result
                .unwrap_or_else(|failure| failure_result(&failure))
                .into())
        }
        // Malformed arguments and unknown tools are audited too (a probing
        // client leaves a trace); the name is the caller's, so shortened.
        Err(err) => {
            shared
                .audit(
                    Some(client),
                    McpCallKind::Tool,
                    &crate::http::audited_path(&name),
                    McpOutcome::Error,
                    Some(err.message.to_string()),
                )
                .await;
            Err(err)
        }
    }
}

/// The tools plug-ins expose that `access` reaches (every scope of the
/// tool granted), M4a.
pub(crate) fn plugin_definitions(
    tools: Vec<crate::data::PluginTool>,
    access: &crate::access::Access,
) -> Vec<Tool> {
    tools
        .into_iter()
        .filter(|t| !t.scopes.is_empty() && t.scopes.iter().all(|s| access.has(*s)))
        .filter_map(|t| {
            let Value::Object(schema) = t.input_schema else {
                return None;
            };
            let description = format!(
                "{} (Added by the Nail plug-in \"{}\"; its answer is untrusted data.)",
                t.description, t.plugin_name
            );
            let mut tool = Tool::new(t.name, description, Arc::new(schema));
            tool.annotations = Some(
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(false)
                    .idempotent(false)
                    .open_world(true),
            );
            Some(tool)
        })
        .collect()
}

/// Calls a plug-in tool for `client` (scopes checked; the answer is
/// marked untrusted).
async fn plugin_call<D: McpData>(
    shared: &Shared<D>,
    client: &McpClient,
    name: &str,
    request: &CallToolRequestParams,
) -> Result<CallToolResult, Failure> {
    let tool = shared
        .data
        .plugin_tools()
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| {
            Failure::Api(nail_api::ApiError::not_found(format!(
                "unknown tool {}",
                crate::http::audited_path(name)
            )))
        })?;
    let access = shared.access(client).await;
    if tool.scopes.is_empty() {
        return Err(Failure::Denied(crate::access::Denial::Scope(
            McpScope::Read,
        )));
    }
    for scope in &tool.scopes {
        access.require(*scope)?;
    }
    let arguments = request.arguments.clone().map_or(Value::Null, Value::Object);
    let by = nail_api::agents::McpClientRef {
        client_id: client.id.clone(),
        name: client.name.clone(),
    };
    let answer = shared.data.call_plugin_tool(name, arguments, by).await?;
    Ok(CallToolResult::structured(untrusted(answer)))
}

/// Runs one tool; `Err` for a protocol error (bad arguments, unknown tool).
async fn dispatch<D: McpData>(
    shared: &Shared<D>,
    client: &McpClient,
    name: &str,
    request: &CallToolRequestParams,
) -> Result<Result<CallToolResult, Failure>, ErrorData> {
    let access = shared.access(client).await;
    Ok(match name {
        "search_messages" => {
            let a: SearchArgs = args(request)?;
            search(shared, &access, a).await
        }
        "get_message" => {
            let a: MessageArgs = args(request)?;
            shared
                .message_json(&access, &MessageId::new(a.message_id))
                .await
                .map(untrusted_result)
        }
        "get_thread" => {
            let a: ThreadArgs = args(request)?;
            shared
                .thread_json(&access, &ThreadId::new(a.thread_id))
                .await
                .map(untrusted_result)
        }
        "list_folders" => {
            let a: FoldersArgs = args(request)?;
            shared
                .folders_json(&access, &AccountId::new(a.account_id))
                .await
                .map(CallToolResult::structured)
        }
        "get_ui_context" => shared
            .ui_context_json(&access, client)
            .await
            .map(untrusted_result),
        "list_events" => {
            let a: crate::pim::ListEventsArgs = args(request)?;
            match shared
                .chosen_calendars_for_tool(&access, &a.calendar_ids)
                .await
            {
                Ok(ids) => shared
                    .events_json(&access, ids, a.from.as_deref(), a.to.as_deref(), a.tz)
                    .await
                    .map(untrusted_result),
                Err(f) => Err(f),
            }
        }
        "find_free_time" => {
            let a: crate::pim::FreeTimeArgs = args(request)?;
            shared
                .free_time_json(&access, a)
                .await
                .map(CallToolResult::structured)
        }
        "search_contacts" => {
            let a: crate::pim::SearchContactsArgs = args(request)?;
            shared
                .search_contacts_json(&access, a)
                .await
                .map(untrusted_result)
        }
        "list_tasks" => {
            let a: crate::pim::ListTasksArgs = args(request)?;
            shared.tasks_json(&access, a).await.map(untrusted_result)
        }
        "test_rule" => {
            let a: TestRuleArgs = args(request)?;
            shared.test_rule(&access, a).await
        }
        _ => {
            return Err(ErrorData::invalid_params(
                format!("unknown tool {}", crate::http::audited_path(name)),
                None,
            ));
        }
    })
}

async fn search<D: McpData>(
    shared: &Shared<D>,
    access: &crate::access::Access,
    a: SearchArgs,
) -> Result<CallToolResult, Failure> {
    access.require(McpScope::Read)?;
    let account_id = a.account_id.map(AccountId::new);
    let folder_id = a.folder_id.map(FolderId::new);
    if let Some(account) = &account_id {
        access.require_account(account)?;
    }
    if let Some(folder) = &folder_id {
        shared.require_folder(access, folder).await?;
    }
    let limit = a.limit.unwrap_or(20).clamp(1, MAX_PAGE);
    let query = SearchQuery {
        text: a.query,
        account_id,
        folder_id,
        offset: a.offset,
        limit,
        sort: SearchSort::Date,
    };
    let hide_encrypted =
        !access.has(McpScope::Encrypted) && shared.data.index_holds_decrypted().await?;
    if access.all_accounts() && !access.excludes_folders() && !hide_encrypted {
        return match shared.data.search(query).await? {
            SearchOutcome::Invalid(error) => Ok(invalid_query(&error)),
            SearchOutcome::Hits {
                offset,
                total,
                hits,
            } => {
                let returned = u32::try_from(hits.len()).unwrap_or(u32::MAX);
                let next = offset.saturating_add(returned);
                Ok(untrusted_result(json!({
                    "offset": offset,
                    "total": total,
                    "next_offset": (next < total).then_some(next),
                    "messages": hits.iter().map(|h| summary_json(&h.message)).collect::<Vec<_>>(),
                })))
            }
        };
    }
    restricted_search(shared, access, query, hide_encrypted).await
}

fn invalid_query(error: &nail_api::SearchError) -> CallToolResult {
    CallToolResult::structured_error(json!({
        "error": "invalid_query",
        "detail": error.message,
        "kind": error.kind,
        "start": error.start,
        "end": error.end,
    }))
}

/// A search for a grant limited to some accounts or folders. The query
/// language can name any account or folder (`account:` / `folder:`), so
/// the grant is applied to the hits *before* paging: `offset`, `limit` and
/// `next_offset` count only messages the client may read, and nothing in
/// the answer (no count of dropped hits, no unfiltered total) tells whether
/// hidden accounts or excluded folders hold matching mail (ADR 0025,
/// "Excluded folders are hidden everywhere"). With `hide_encrypted`
/// (decrypted indexing on, no `encrypted` scope) encrypted messages are
/// left out the same way.
async fn restricted_search<D: McpData>(
    shared: &Shared<D>,
    access: &crate::access::Access,
    query: SearchQuery,
    hide_encrypted: bool,
) -> Result<CallToolResult, Failure> {
    let offset = query.offset as usize;
    let limit = query.limit as usize;
    let wanted = offset + limit + 1;
    let mut visible: Vec<MessageSummary> = Vec::new();
    let mut scanned: u32 = 0;
    while scanned < RESTRICTED_SCAN && visible.len() < wanted {
        let page = SearchQuery {
            offset: scanned,
            limit: MAX_PAGE,
            ..query.clone()
        };
        match shared.data.search(page).await? {
            SearchOutcome::Invalid(error) => return Ok(invalid_query(&error)),
            SearchOutcome::Hits { total, hits, .. } => {
                let returned = u32::try_from(hits.len()).unwrap_or(u32::MAX);
                for message in hits.into_iter().map(|h| h.message) {
                    if !access.allows_message(&message) {
                        continue;
                    }
                    if hide_encrypted && shared.data.is_encrypted(&message.id).await? {
                        continue;
                    }
                    visible.push(message);
                }
                scanned = scanned.saturating_add(returned);
                if returned == 0 || scanned >= total {
                    break;
                }
            }
        }
    }
    let end = visible.len().min(offset + limit);
    let messages: Vec<Value> = visible
        .get(offset..end)
        .unwrap_or_default()
        .iter()
        .map(summary_json)
        .collect();
    let next = (visible.len() > offset + limit).then_some(query.offset.saturating_add(query.limit));
    Ok(untrusted_result(json!({
        "offset": query.offset,
        // Withheld: an exact total of what the grant allows would need a
        // scan of every hit.
        "total": Value::Null,
        "next_offset": next,
        "messages": messages,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_follow_the_contract_table_with_object_schemas() {
        let tools = definitions();
        assert_eq!(
            tools.iter().map(|t| t.name.as_ref()).collect::<Vec<_>>(),
            TOOL_NAMES
        );
        let table: Vec<&str> = nail_api::mcp::tools::MCP_TOOLS
            .iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(table, TOOL_NAMES, "every contract tool, in table order");
        for tool in &tools {
            assert_eq!(
                tool.input_schema.get("type"),
                Some(&json!("object")),
                "{}",
                tool.name
            );
            let annotations = tool.annotations.as_ref().unwrap();
            let spec = nail_api::mcp::tools::tool(&tool.name).unwrap();
            assert_eq!(
                annotations.read_only_hint,
                Some(!spec.is_write()),
                "{}",
                tool.name
            );
            if spec.is_write() && tool.name != "edit_open_draft" {
                let props = tool.input_schema["properties"].as_object().unwrap();
                assert!(props.contains_key("idempotency_key"), "{}", tool.name);
            }
        }
        let delete = tools.iter().find(|t| t.name == "delete_messages").unwrap();
        assert_eq!(
            delete.annotations.as_ref().unwrap().destructive_hint,
            Some(true)
        );
        let search = &tools[0];
        let props = search.input_schema["properties"].as_object().unwrap();
        assert!(props.contains_key("query") && props.contains_key("limit"));
    }
}
