//! Composer, attachment, link and MCP extensions on the host side (M4a,
//! ADR 0079; design.md, "Extension points").
//!
//! * [`PluginHost::run_action`] (`RunPluginAction`): composer actions (with
//!   the draft and selection; the plug-in may then edit that draft with
//!   `nail.composer.*` for a while), attachment actions (with the
//!   attachment's bytes) and link actions (with the link). It answers once
//!   the plug-in took the call; failures are logged.
//! * [`PluginHost::render_attachment_preview`] (`RenderAttachmentPreview`):
//!   a renderer gets the attachment's bytes and answers a component tree
//!   (validated like a panel's) or a PNG / JPEG / GIF / WebP image —
//!   never HTML, never SVG.
//! * [`PluginHost::render_markdown_block`]: a fenced block
//!   ```` ```<plugin>.<block> ```` rendered to HTML (sanitized like a message
//!   body) and text, for the composer's preview and at send time.
//! * [`PluginHost::mcp_tools`] / [`PluginHost::call_mcp_tool`]: tools of
//!   plug-ins with `mcp.expose`, offered to MCP clients as
//!   `<plugin>__<tool>`; writes the plug-in makes while serving a call are
//!   attributed to it *on behalf of* the client.
//!
//! Attachments of end-to-end encrypted messages are never handed to
//! plug-ins (`mail.read.encrypted` is not available).

use std::sync::Arc;

use base64::Engine as _;
use nail_api::agents::McpClientRef;
use nail_api::mcp::McpScope;
use nail_api::plugins::extensions::{
    MAX_BLOCK_HTML, MAX_PREVIEW_IMAGE, PREVIEW_IMAGE_TYPES, parse_plugin_extension_id,
    parse_plugin_mcp_tool_name, plugin_mcp_tool_name,
};
use nail_api::plugins::{
    PluginActionContext, PluginActionTarget, PluginAttachmentRef, PluginComposerContext,
    PluginLink, PluginLogLevel, PluginMarkdownBlockInput, PluginMarkdownBlockResult,
    PluginPermission, PluginPreview, PluginRenderPurpose,
};
use nail_api::{ApiError, ErrorCode, LocalizedMessage, MessageId, PluginId};
use serde_json::{Value, json};

use crate::gate::Denied;
use crate::host::{Inner, PluginHost, Running};
use crate::l10n;
use crate::runtime::{Invoke, Origin, Registry};
use crate::services::AttachmentData;
use crate::tree;

/// A tool a plug-in offers to MCP clients (`mcp.expose`).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginMcpTool {
    /// The MCP name (`<plugin>__<tool>`).
    pub name: String,
    /// The plug-in.
    pub plugin_id: PluginId,
    /// Its name (in the core's language).
    pub plugin_name: String,
    /// What the tool does (the plug-in's words, for the agent).
    pub description: String,
    /// The JSON Schema of its arguments (an object schema).
    pub input_schema: Value,
    /// The scopes a client needs to see and call it.
    pub scopes: Vec<McpScope>,
}

/// Longest text of a rendered Markdown block (bytes).
pub const MAX_BLOCK_TEXT: usize = 256 * 1024;

fn denied(call: &str, permission: impl ToString) -> ApiError {
    Denied::new(call, permission).to_api_error()
}

impl Inner {
    /// Logs a failed handler call.
    fn handler_failed(&self, plugin: &PluginId, what: &str, error: &ApiError) {
        self.host_log(
            plugin,
            PluginLogLevel::Warn,
            format!("{what} failed: {}", error.message),
            LocalizedMessage::new(l10n::HANDLER_FAILED)
                .arg("name", what)
                .arg("detail", error.message.as_str()),
        );
    }

    /// An attachment for a plug-in with `attachments`: the message must be
    /// in the grant's accounts and not end-to-end encrypted; the bytes are
    /// returned when at most `max_bytes`.
    async fn plugin_attachment(
        &self,
        running: &Running,
        call: &str,
        message_id: &MessageId,
        index: u32,
        max_bytes: u64,
    ) -> Result<(PluginAttachmentRef, Vec<u8>), ApiError> {
        let scope = running
            .gate
            .scope(&PluginPermission::Attachments, call, &self.expand())
            .map_err(|d| d.to_api_error())?;
        let not_found = || ApiError::not_found("attachment not found");
        let message = self
            .services
            .mail_get(message_id.clone())
            .await?
            .filter(|m| scope.contains(&m.account_id))
            .ok_or_else(not_found)?;
        if message.encrypted {
            return Err(denied(call, "mail.read.encrypted"));
        }
        let AttachmentData {
            name,
            mime_type,
            size,
            bytes,
        } = self
            .services
            .attachment(message_id.clone(), index, max_bytes)
            .await?
            .ok_or_else(not_found)?;
        if size > max_bytes || bytes.len() as u64 > max_bytes {
            return Err(ApiError::new(
                ErrorCode::TooLarge,
                format!("the attachment is larger than {max_bytes} bytes"),
            ));
        }
        Ok((
            PluginAttachmentRef {
                message_id: message_id.clone(),
                index,
                name,
                mime_type,
                size,
            },
            bytes,
        ))
    }
}

/// The host of an `http(s)` link (`""` for `mailto:`); `None` for other
/// schemes.
fn link_host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    match parsed.scheme() {
        "http" | "https" => parsed.host_str().map(|h| h.to_ascii_lowercase()),
        "mailto" => Some(String::new()),
        _ => None,
    }
}

/// Checks an image preview: an allowed type and valid base64 of at most
/// [`MAX_PREVIEW_IMAGE`] bytes.
fn image_ok(mime_type: &str, data: &str) -> bool {
    PREVIEW_IMAGE_TYPES.contains(&mime_type)
        && data.len() / 4 * 3 <= MAX_PREVIEW_IMAGE + 3
        && base64::engine::general_purpose::STANDARD
            .decode(data.as_bytes())
            .is_ok_and(|b| !b.is_empty() && b.len() <= MAX_PREVIEW_IMAGE)
}

/// Checks MCP tool arguments against the tool's schema: an object, every
/// `required` property present, and properties of a declared JSON type
/// having that type. (Full JSON Schema validation is the client's job; this
/// keeps the plug-in from getting what its schema rules out.)
pub fn check_arguments(schema: &Value, args: &Value) -> Result<(), String> {
    let Some(map) = args.as_object() else {
        return Err("the arguments must be an object".to_owned());
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !map.contains_key(name) {
                return Err(format!("`{name}` is required"));
            }
        }
    }
    let properties = schema.get("properties").and_then(Value::as_object);
    if schema.get("additionalProperties") == Some(&Value::Bool(false))
        && let Some(unknown) = map
            .keys()
            .find(|k| properties.is_none_or(|p| !p.contains_key(*k)))
    {
        return Err(format!("`{unknown}` is not an argument of this tool"));
    }
    for (name, value) in map {
        let Some(ty) = properties
            .and_then(|p| p.get(name))
            .and_then(|p| p.get("type"))
        else {
            continue;
        };
        let types: Vec<&str> = match ty {
            Value::String(t) => vec![t.as_str()],
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            _ => continue,
        };
        let fits = types.iter().any(|t| match *t {
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.is_i64() || value.is_u64(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            "null" => value.is_null(),
            _ => true,
        });
        if !fits {
            return Err(format!("`{name}` must be of type {}", types.join(" or ")));
        }
    }
    Ok(())
}

impl PluginHost {
    /// Runs a composer, attachment or link action (`<plugin>.<action>`;
    /// `RunPluginAction`). Returns once the plug-in got the call; what it
    /// does arrives as events, failures are logged. `not_found` for an
    /// unknown action or item, `forbidden` without the permission,
    /// `unavailable` when the plug-in is not running, `invalid_request`
    /// when the action does not apply to the attachment or link.
    pub async fn run_action(
        &self,
        action: &str,
        target: &PluginActionTarget,
        client_id: Option<String>,
    ) -> Result<(), ApiError> {
        let inner = Arc::clone(self.inner());
        let (plugin, name) = parse_plugin_extension_id(action)
            .ok_or_else(|| ApiError::not_found(format!("`{action}` is not a plug-in action")))?;
        let running = inner.running_info(&plugin)?;
        let contributes = &running.manifest.contributes;
        let unknown = || ApiError::not_found(format!("`{action}` is not declared by the plug-in"));
        let mut context = PluginActionContext {
            action: name.clone(),
            composer: None,
            attachment: None,
            link: None,
        };
        let mut call = match target {
            PluginActionTarget::Composer {
                draft_id,
                selection,
            } => {
                if !contributes.composer_actions.iter().any(|a| a.id == name) {
                    return Err(unknown());
                }
                if !running.gate.has(&PluginPermission::Composer) {
                    return Err(denied("composer actions", PluginPermission::Composer));
                }
                let mode = inner
                    .services
                    .draft_mode(draft_id.clone())
                    .await?
                    .ok_or_else(|| ApiError::not_found("draft not found"))?;
                context.composer = Some(PluginComposerContext {
                    draft_id: draft_id.clone(),
                    selection: selection.clone(),
                    mode,
                });
                let mut call = Invoke::new(Registry::ComposerAction, &name, Value::Null);
                call.composer_draft = Some(draft_id.clone());
                call
            }
            PluginActionTarget::Attachment { message_id, index } => {
                let def = contributes
                    .attachment_actions
                    .iter()
                    .find(|a| a.id == name)
                    .ok_or_else(unknown)?;
                let max = nail_api::plugins::extensions::MAX_PREVIEW_INPUT;
                let (attachment, bytes) = inner
                    .plugin_attachment(&running, "attachment actions", message_id, *index, max)
                    .await?;
                if !def.matches(&attachment.mime_type, Some(&attachment.name)) {
                    return Err(ApiError::invalid_request(
                        "the action does not apply to this attachment",
                    ));
                }
                context.attachment = Some(attachment);
                let mut call = Invoke::new(Registry::AttachmentAction, &name, Value::Null);
                call.bytes = Some(bytes);
                call
            }
            PluginActionTarget::Link {
                url,
                text,
                message_id,
            } => {
                let def = contributes
                    .link_actions
                    .iter()
                    .find(|a| a.id == name)
                    .ok_or_else(unknown)?;
                let host = link_host(url)
                    .ok_or_else(|| ApiError::invalid_request("not an http(s) or mailto link"))?;
                if url.len() > 8192 || !def.matches_host(&host) {
                    return Err(ApiError::invalid_request(
                        "the action does not apply to this link",
                    ));
                }
                // The message id only for plug-ins that may read it.
                let readable = match message_id {
                    Some(id) => {
                        let scope = running
                            .gate
                            .mail_scope("link actions", &inner.expand())
                            .ok();
                        let found = tree::readable_messages(
                            inner.services.as_ref(),
                            scope.as_ref(),
                            vec![id.clone()],
                        )
                        .await;
                        found.contains_key(id).then(|| id.clone())
                    }
                    None => None,
                };
                context.link = Some(PluginLink {
                    url: url.clone(),
                    text: text.as_ref().map(|t| t.chars().take(1000).collect()),
                    message_id: readable,
                });
                Invoke::new(Registry::LinkAction, &name, Value::Null)
            }
        };
        call.input =
            serde_json::to_value(&context).map_err(|e| ApiError::internal(e.to_string()))?;
        call.origin = Origin::client(client_id);
        let what = format!("{} {name}", call.registry.as_str());
        let rx = inner.invoke(&plugin, Some(running.generation), call)?;
        let host = self.clone();
        inner.handle.spawn(async move {
            // Composer actions may wait for a dialog: only failures matter.
            let limit = host.limits().job_wall_clock;
            if let Err(e) = host.wait_for(limit, rx).await
                && e.code != ErrorCode::Unavailable
            {
                host.inner().handler_failed(&plugin, &what, &e);
            }
        });
        Ok(())
    }

    /// Renders an attachment with a plug-in's preview renderer
    /// (`<plugin>.<renderer>`; `RenderAttachmentPreview`): a validated
    /// component tree (package images inlined, message lists filled with
    /// what the plug-in may read) or an image.
    pub async fn render_attachment_preview(
        &self,
        message_id: &MessageId,
        index: u32,
        renderer: &str,
        client_id: Option<String>,
    ) -> Result<PluginPreview, ApiError> {
        let inner = Arc::clone(self.inner());
        let (plugin, name) = parse_plugin_extension_id(renderer)
            .ok_or_else(|| ApiError::not_found(format!("`{renderer}` is not a renderer")))?;
        let running = inner.running_info(&plugin)?;
        let def = running
            .manifest
            .contributes
            .attachment_previews
            .iter()
            .find(|p| p.id == name)
            .cloned()
            .ok_or_else(|| ApiError::not_found(format!("`{renderer}` is not declared")))?;
        let (attachment, bytes) = inner
            .plugin_attachment(
                &running,
                "attachment previews",
                message_id,
                index,
                def.max_bytes(),
            )
            .await?;
        if !def.matches(&attachment.mime_type, Some(&attachment.name)) {
            return Err(ApiError::invalid_request(
                "the renderer does not render this attachment",
            ));
        }
        let mut call = Invoke::new(
            Registry::AttachmentPreview,
            &name,
            serde_json::to_value(&attachment).unwrap_or_default(),
        );
        call.bytes = Some(bytes);
        call.origin = Origin::client(client_id);
        let rx = inner.invoke(&plugin, Some(running.generation), call)?;
        let what = format!("attachmentPreview {name}");
        let value = self.wait(rx).await.inspect_err(|e| {
            if e.code == ErrorCode::Internal {
                inner.handler_failed(&plugin, &what, e);
            }
        })?;
        let preview: PluginPreview = serde_json::from_value(value).map_err(|e| {
            let error = ApiError::invalid_request(format!("the preview is not valid: {e}"));
            inner.handler_failed(&plugin, &what, &error);
            error
        })?;
        match preview {
            PluginPreview::Tree { tree: nodes } => {
                let nodes = tree::prepare_tree(&running.files, nodes).map_err(|issues| {
                    let error = ApiError::invalid_request(format!(
                        "the preview is not valid: {}",
                        issues
                            .iter()
                            .map(|i| format!("{:?} at {}", i.kind, i.path))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                    inner.handler_failed(&plugin, &what, &error);
                    error
                })?;
                let scope = running.gate.mail_scope("preview", &inner.expand()).ok();
                let nodes = tree::fill_tree(inner.services.as_ref(), scope.as_ref(), nodes).await;
                Ok(PluginPreview::Tree { tree: nodes })
            }
            PluginPreview::Image {
                mime_type,
                data,
                alt,
            } => {
                if !image_ok(&mime_type, &data) {
                    let error = ApiError::invalid_request(
                        "a preview image must be PNG, JPEG, GIF or WebP data of at most 5 MiB",
                    );
                    inner.handler_failed(&plugin, &what, &error);
                    return Err(error);
                }
                Ok(PluginPreview::Image {
                    mime_type,
                    data,
                    alt: alt.map(|a| a.chars().take(500).collect()),
                })
            }
        }
    }

    /// Renders a Markdown block (`info` = the fence's info string
    /// `<plugin>.<block>`) with its plug-in. The HTML is sanitized like a
    /// message body; `not_found` when no running plug-in with `composer`
    /// renders it (the composer then keeps it as a code block).
    pub async fn render_markdown_block(
        &self,
        info: &str,
        source: &str,
        purpose: PluginRenderPurpose,
    ) -> Result<PluginMarkdownBlockResult, ApiError> {
        let inner = Arc::clone(self.inner());
        let (plugin, name) = parse_plugin_extension_id(info)
            .ok_or_else(|| ApiError::not_found(format!("`{info}` is not a plug-in block")))?;
        let running = inner.running_info(&plugin)?;
        if !running
            .manifest
            .contributes
            .markdown_blocks
            .iter()
            .any(|b| b.id == name)
        {
            return Err(ApiError::not_found(format!("`{info}` is not declared")));
        }
        if !running.gate.has(&PluginPermission::Composer) {
            return Err(denied("markdown blocks", PluginPermission::Composer));
        }
        if source.len() > MAX_BLOCK_TEXT {
            return Err(ApiError::new(ErrorCode::TooLarge, "the block is too long"));
        }
        let input = PluginMarkdownBlockInput {
            block: name.clone(),
            source: source.to_owned(),
            purpose,
        };
        let call = Invoke::new(
            Registry::MarkdownBlock,
            &name,
            serde_json::to_value(&input).unwrap_or_default(),
        );
        let rx = inner.invoke(&plugin, Some(running.generation), call)?;
        let what = format!("markdownBlock {name}");
        let value = self.wait(rx).await.inspect_err(|e| {
            if e.code == ErrorCode::Internal {
                inner.handler_failed(&plugin, &what, e);
            }
        })?;
        let result: PluginMarkdownBlockResult = serde_json::from_value(value).map_err(|e| {
            let error = ApiError::invalid_request(format!("the block is not valid: {e}"));
            inner.handler_failed(&plugin, &what, &error);
            error
        })?;
        if result.html.len() > MAX_BLOCK_HTML || result.text.len() > MAX_BLOCK_TEXT {
            let error = ApiError::new(ErrorCode::TooLarge, "the rendered block is too large");
            inner.handler_failed(&plugin, &what, &error);
            return Err(error);
        }
        Ok(PluginMarkdownBlockResult {
            html: nail_mime::sanitize_html(&result.html).html,
            text: result.text,
        })
    }

    /// The tools running plug-ins with `mcp.expose` offer, by name.
    pub fn mcp_tools(&self) -> Vec<PluginMcpTool> {
        let mut tools = Vec::new();
        for (plugin, running) in self.inner().running_all() {
            if !running.gate.has(&PluginPermission::McpExpose) {
                continue;
            }
            for def in &running.manifest.contributes.mcp_tools {
                tools.push(PluginMcpTool {
                    name: plugin_mcp_tool_name(&plugin, &def.id),
                    plugin_id: plugin.clone(),
                    plugin_name: running.name.clone(),
                    description: def.description.clone(),
                    input_schema: def
                        .input_schema
                        .clone()
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                    scopes: def.scopes.clone(),
                });
            }
        }
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    /// Calls an exposed tool for an MCP client (the MCP server checked the
    /// client's scopes against [`PluginMcpTool::scopes`]). Arguments are
    /// checked against the tool's schema ([`check_arguments`]); the result
    /// is the handler's JSON value (at most
    /// [`crate::HostLimits::max_tool_result_bytes`]), within
    /// [`crate::HostLimits::mcp_tool_timeout`].
    pub async fn call_mcp_tool(
        &self,
        name: &str,
        arguments: Value,
        client: McpClientRef,
    ) -> Result<Value, ApiError> {
        let inner = Arc::clone(self.inner());
        let (plugin, tool) = parse_plugin_mcp_tool_name(name)
            .ok_or_else(|| ApiError::not_found(format!("`{name}` is not a plug-in tool")))?;
        let running = inner.running_info(&plugin)?;
        let def = running
            .manifest
            .contributes
            .mcp_tools
            .iter()
            .find(|t| t.id == tool)
            .cloned()
            .ok_or_else(|| ApiError::not_found(format!("`{name}` is not declared")))?;
        if !running.gate.has(&PluginPermission::McpExpose) {
            return Err(ApiError::not_found(format!("`{name}` is not offered")));
        }
        let arguments = if arguments.is_null() {
            json!({})
        } else {
            arguments
        };
        if let Some(schema) = &def.input_schema {
            check_arguments(schema, &arguments).map_err(ApiError::invalid_request)?;
        } else if !arguments.is_object() {
            return Err(ApiError::invalid_request("the arguments must be an object"));
        }
        // The handler gets the agent's arguments; who called is not its
        // business (its writes are attributed to the client anyway).
        let mut call = Invoke::new(Registry::McpTool, &tool, arguments);
        call.origin = Origin {
            client: None,
            on_behalf: Some(client),
        };
        let rx = inner.invoke(&plugin, Some(running.generation), call)?;
        let what = format!("mcpTool {tool}");
        let value = self
            .wait_for(inner.limits.mcp_tool_timeout, rx)
            .await
            .inspect_err(|e| {
                if e.code == ErrorCode::Internal {
                    inner.handler_failed(&plugin, &what, e);
                }
            })?;
        let size = serde_json::to_vec(&value).map_or(usize::MAX, |b| b.len());
        if size > inner.limits.max_tool_result_bytes {
            return Err(ApiError::new(
                ErrorCode::TooLarge,
                "the tool's result is too large",
            ));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_hosts() {
        assert_eq!(
            link_host("https://Jira.Example.com/browse/X-1").as_deref(),
            Some("jira.example.com")
        );
        assert_eq!(link_host("mailto:ada@example.com").as_deref(), Some(""));
        assert_eq!(link_host("javascript:alert(1)"), None);
        assert_eq!(link_host("file:///c:/x"), None);
        assert_eq!(link_host("not a url"), None);
    }

    #[test]
    fn preview_images_are_checked() {
        assert!(image_ok("image/png", "iVBORw0KGgo="));
        assert!(!image_ok("image/svg+xml", "PHN2Zy8+"));
        assert!(!image_ok("text/html", "PGI+"));
        assert!(!image_ok("image/png", "not base64!"));
        assert!(!image_ok("image/png", ""));
        let big =
            base64::engine::general_purpose::STANDARD.encode(vec![0u8; MAX_PREVIEW_IMAGE + 1]);
        assert!(!image_ok("image/png", &big));
    }

    #[test]
    fn tool_arguments_follow_the_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "email": {"type": "string"},
                "limit": {"type": "integer"},
                "tags": {"type": ["array", "null"]}
            },
            "required": ["email"],
            "additionalProperties": false
        });
        assert!(check_arguments(&schema, &json!({"email": "a@b.c", "limit": 3})).is_ok());
        assert!(check_arguments(&schema, &json!({"email": "a@b.c", "tags": null})).is_ok());
        assert!(check_arguments(&schema, &json!({"limit": 3})).is_err());
        assert!(check_arguments(&schema, &json!({"email": 1})).is_err());
        assert!(check_arguments(&schema, &json!({"email": "x", "limit": 1.5})).is_err());
        assert!(check_arguments(&schema, &json!({"email": "x", "other": 1})).is_err());
        assert!(check_arguments(&schema, &json!(["x"])).is_err());
        let open = json!({"type": "object"});
        assert!(check_arguments(&open, &json!({"anything": [1]})).is_ok());
    }
}
