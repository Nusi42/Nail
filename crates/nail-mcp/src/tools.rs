//! Read-only tools (design.md, "MCP server", Tools): `search_messages`,
//! `get_message`, `get_thread`, `list_folders`, `get_ui_context`.
//!
//! Tools wrap the resources for clients without resource support. Results
//! are structured JSON; anything from the mailbox is wrapped in the
//! untrusted-data envelope ([`crate::content`]). A refused call is a tool
//! error naming the missing grant (not a protocol error), so the agent can
//! tell the user what to grant.

use std::sync::Arc;

use nail_api::mcp::{McpCallKind, McpClient, McpScope};
use nail_api::search::SearchSort;
use nail_api::{AccountId, FolderId, MessageId, SearchQuery, ThreadId};
use rmcp::ErrorData;
use rmcp::handler::server::common::schema_for_type;
use rmcp::model::{CallToolRequestParams, CallToolResult, JsonObject, Tool, ToolAnnotations};
use rmcp::schemars::JsonSchema;
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

/// Names of the tools.
pub const TOOL_NAMES: [&str; 5] = [
    "search_messages",
    "get_message",
    "get_thread",
    "list_folders",
    "get_ui_context",
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

/// The tool list.
pub fn definitions() -> Vec<Tool> {
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
    ]
}

fn args<T: for<'de> Deserialize<'de>>(request: &CallToolRequestParams) -> Result<T, ErrorData> {
    let value = Value::Object(request.arguments.clone().unwrap_or_default());
    serde_json::from_value(value)
        .map_err(|e| ErrorData::invalid_params(format!("invalid arguments: {e}"), None))
}

/// A successful tool result carrying mailbox content.
fn untrusted_result(data: Value) -> CallToolResult {
    CallToolResult::structured(untrusted(data))
}

fn failure_result(failure: &Failure) -> CallToolResult {
    match failure {
        Failure::Denied(denial) => CallToolResult::structured_error(json!({
            "error": "denied",
            "detail": denial.to_string(),
            "hint": "The user can grant access in Nail: Settings → MCP clients.",
        })),
        Failure::Api(err) => CallToolResult::structured_error(json!({
            "error": err.code.as_str(),
            "detail": err.message,
        })),
    }
}

/// Runs a tool call for `client`, audited.
pub(crate) async fn call<D: McpData>(
    shared: &Shared<D>,
    client: &McpClient,
    request: CallToolRequestParams,
) -> Result<CallToolResult, ErrorData> {
    let name = request.name.to_string();
    let access = shared.access(client).await;
    let result: Result<CallToolResult, Failure> = match name.as_str() {
        "search_messages" => {
            let a: SearchArgs = args(&request)?;
            search(shared, &access, a).await
        }
        "get_message" => {
            let a: MessageArgs = args(&request)?;
            shared
                .message_json(&access, &MessageId::new(a.message_id))
                .await
                .map(untrusted_result)
        }
        "get_thread" => {
            let a: ThreadArgs = args(&request)?;
            shared
                .thread_json(&access, &ThreadId::new(a.thread_id))
                .await
                .map(untrusted_result)
        }
        "list_folders" => {
            let a: FoldersArgs = args(&request)?;
            shared
                .folders_json(&access, &AccountId::new(a.account_id))
                .await
                .map(CallToolResult::structured)
        }
        "get_ui_context" => shared
            .ui_context_json(&access, client)
            .await
            .map(untrusted_result),
        _ => {
            return Err(ErrorData::invalid_params(
                format!("unknown tool {name}"),
                None,
            ));
        }
    };
    let result = shared
        .audited(client, McpCallKind::Tool, &name, result)
        .await;
    Ok(result.unwrap_or_else(|failure| failure_result(&failure)))
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
    match shared.data.search(query).await? {
        SearchOutcome::Invalid(error) => Ok(CallToolResult::structured_error(json!({
            "error": "invalid_query",
            "detail": error.message,
            "kind": error.kind,
            "start": error.start,
            "end": error.end,
        }))),
        SearchOutcome::Hits {
            offset,
            total,
            hits,
        } => {
            let returned = u32::try_from(hits.len()).unwrap_or(u32::MAX);
            let messages: Vec<Value> = hits
                .iter()
                .filter(|h| access.allows_message(&h.message))
                .map(|h| summary_json(&h.message))
                .collect();
            let hidden = hits.len() - messages.len();
            let exact = access.all_accounts() && !access.excludes_folders();
            let next = offset.saturating_add(returned);
            Ok(untrusted_result(json!({
                "offset": offset,
                // Hits outside the grant are dropped; the total is exact only
                // for an unrestricted grant.
                "total": exact.then_some(total),
                "hidden": hidden,
                "next_offset": (next < total).then_some(next),
                "messages": messages,
            })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_read_only_with_object_schemas() {
        let tools = definitions();
        assert_eq!(
            tools.iter().map(|t| t.name.as_ref()).collect::<Vec<_>>(),
            TOOL_NAMES
        );
        for tool in &tools {
            assert_eq!(
                tool.input_schema.get("type"),
                Some(&json!("object")),
                "{}",
                tool.name
            );
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.read_only_hint, Some(true));
        }
        let search = &tools[0];
        let props = search.input_schema["properties"].as_object().unwrap();
        assert!(props.contains_key("query") && props.contains_key("limit"));
    }
}
