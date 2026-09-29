//! Read-only tools (design.md, "MCP server", Tools): `search_messages`,
//! `get_message`, `get_thread`, `list_folders`, `get_ui_context`.
//!
//! Tools wrap the resources for clients without resource support. Results
//! are structured JSON; anything from the mailbox is wrapped in the
//! untrusted-data envelope ([`crate::content`]). A refused call is a tool
//! error naming the missing grant (not a protocol error), so the agent can
//! tell the user what to grant.

use std::sync::Arc;

use nail_api::mcp::{McpCallKind, McpClient, McpOutcome, McpScope};
use nail_api::search::SearchSort;
use nail_api::{AccountId, FolderId, MessageId, MessageSummary, SearchQuery, ThreadId};
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
    match dispatch(shared, client, &name, &request).await {
        Ok(result) => {
            let result = shared
                .audited(client, McpCallKind::Tool, &name, result)
                .await;
            Ok(result.unwrap_or_else(|failure| failure_result(&failure)))
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

/// Hits a restricted search looks at, at most. Past this the result is
/// cut short (no `next_offset`).
const RESTRICTED_SCAN: u32 = 5000;

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
