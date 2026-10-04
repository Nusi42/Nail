//! Composer, attachment, link and MCP extension points (M4a, ADR 0079;
//! design.md, "Plug-ins" → Extension points).
//!
//! * **Composer actions** (`composer`): buttons / menu items in the
//!   composer ("Insert signature block", "Translate selection"). The
//!   handler gets the open draft and the selected text
//!   ([`PluginComposerContext`]) and edits the draft with
//!   `nail.composer.insert` / `replaceSelection` / `replaceAll`, which the
//!   core forwards to the composer as [`crate::Event::ComposerEditRequested`]
//!   ([`ComposerEdit`], the same edit MCP's `edit_open_draft` makes).
//! * **Markdown blocks** (`composer`): fenced blocks whose info string is
//!   `<plugin>.<block>` (```` ```acme-crm.ticket ````) are rendered by the
//!   plug-in to HTML + text ([`PluginMarkdownBlockResult`]) in the preview
//!   and at send time; the HTML is sanitized like a message body. A plug-in
//!   that fails leaves the block as a code block.
//! * **Attachment previews** (`attachments`): a renderer for MIME types /
//!   extensions gets the attachment's bytes and answers a component tree or
//!   an image ([`PluginPreview`]) — never HTML.
//! * **Attachment and link actions**: "Save to …", "Open in …" entries in
//!   the attachment and link menus; the handler gets the attachment
//!   (`attachments`) or the link ([`PluginActionContext`]).
//! * **MCP tools** (`mcp.expose`): handlers offered to MCP clients as
//!   tools named [`plugin_mcp_tool_name`], under the clients' grants.
//!
//! The UI runs composer, attachment and link actions with
//! [`crate::Request::RunPluginAction`] and previews with
//! [`crate::Request::RenderAttachmentPreview`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::ui::PluginUiNode;
use crate::ids::{DraftId, MessageId, PluginId};
use crate::mcp::McpScope;

/// Longest preview input a renderer gets unless it declares less (bytes).
pub const MAX_PREVIEW_INPUT: u64 = 10 * 1024 * 1024;
/// Largest image a preview may answer (bytes, decoded).
pub const MAX_PREVIEW_IMAGE: usize = 5 * 1024 * 1024;
/// Image types a preview may answer (no SVG: it can carry script).
pub const PREVIEW_IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
/// Largest HTML a Markdown block may render to (bytes).
pub const MAX_BLOCK_HTML: usize = 256 * 1024;
/// Longest MCP tool name (what agents' APIs accept).
pub const MAX_MCP_TOOL_NAME: usize = 64;
/// Largest `inputSchema` of an MCP tool (bytes of JSON).
pub const MAX_MCP_TOOL_SCHEMA: usize = 16 * 1024;
/// Longest MCP tool description (characters).
pub const MAX_MCP_TOOL_DESCRIPTION: usize = 1000;
/// Most MIME types / extensions / hosts in one contribution.
pub const MAX_MATCHERS: usize = 20;

/// A composer action (`contributes.composerActions`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginComposerActionDef {
    /// The name ([`super::is_contribution_name`]); the UI's action id is
    /// [`plugin_extension_id`].
    pub id: String,
    /// The label (or `%fluent-id%`).
    pub title: String,
    /// An icon of the theme's icon set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<String>,
    /// A tooltip (or `%fluent-id%`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
}

/// A Markdown block renderer (`contributes.markdownBlocks`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarkdownBlockDef {
    /// The block name; fences use [`markdown_block_info`]
    /// (`<plugin>.<block>`).
    pub id: String,
    /// What the composer's block menu shows (or `%fluent-id%`).
    pub title: String,
    /// Text inserted as the block's body when picked from the menu (or
    /// `%fluent-id%`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub template: Option<String>,
}

/// An attachment preview renderer (`contributes.attachmentPreviews`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginAttachmentPreviewDef {
    /// The renderer's name.
    pub id: String,
    /// What the preview pane names it (or `%fluent-id%`).
    pub title: String,
    /// MIME types it renders (`text/csv`, `application/pdf`; `type/*`
    /// allowed).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub mime_types: Vec<String>,
    /// File extensions it renders, lowercase without the dot (`csv`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub extensions: Vec<String>,
    /// Largest attachment it accepts (bytes; at most
    /// [`MAX_PREVIEW_INPUT`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_bytes: Option<u64>,
}

impl PluginAttachmentPreviewDef {
    /// Whether it renders an attachment of this type and name.
    pub fn matches(&self, mime_type: &str, filename: Option<&str>) -> bool {
        matches_file(&self.mime_types, &self.extensions, mime_type, filename)
    }

    /// The largest input it gets (bytes).
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
            .unwrap_or(MAX_PREVIEW_INPUT)
            .min(MAX_PREVIEW_INPUT)
    }
}

/// An action in the attachment menu (`contributes.attachmentActions`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginAttachmentActionDef {
    /// The action's name.
    pub id: String,
    /// The label (or `%fluent-id%`).
    pub title: String,
    /// An icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<String>,
    /// MIME types it applies to (empty with no extensions = every
    /// attachment).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub mime_types: Vec<String>,
    /// Extensions it applies to.
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub extensions: Vec<String>,
}

impl PluginAttachmentActionDef {
    /// Whether it applies to an attachment of this type and name.
    pub fn matches(&self, mime_type: &str, filename: Option<&str>) -> bool {
        (self.mime_types.is_empty() && self.extensions.is_empty())
            || matches_file(&self.mime_types, &self.extensions, mime_type, filename)
    }
}

/// An action for links (`contributes.linkActions`). The reader does not
/// offer link actions yet (links sit in its sandboxed frame): they are
/// declared and testable, but users cannot pick them in this version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginLinkActionDef {
    /// The action's name.
    pub id: String,
    /// The label (or `%fluent-id%`).
    pub title: String,
    /// An icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<String>,
    /// Host patterns it applies to (`jira.example.com`, `*.example.com`;
    /// empty = every link).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub hosts: Vec<String>,
}

impl PluginLinkActionDef {
    /// Whether it applies to a link to `host` (any `http(s)` / `mailto`
    /// link; the UI passes the host, empty for `mailto:`).
    pub fn matches_host(&self, host: &str) -> bool {
        self.hosts.is_empty()
            || self
                .hosts
                .iter()
                .any(|pattern| super::PluginPermission::Network(pattern.clone()).allows_host(host))
    }
}

/// A tool offered to MCP clients (`contributes.mcpTools`, needs
/// `mcp.expose`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginMcpToolDef {
    /// The tool's name within the plug-in (the MCP name is
    /// [`plugin_mcp_tool_name`], at most [`MAX_MCP_TOOL_NAME`]
    /// characters).
    pub id: String,
    /// What it does, for the agent (English is fine: agents read it; or
    /// `%fluent-id%`, given in Nail's language; at most
    /// [`MAX_MCP_TOOL_DESCRIPTION`] characters).
    pub description: String,
    /// The JSON Schema of its arguments (an object schema; default: no
    /// arguments).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub input_schema: Option<Value>,
    /// The scopes an MCP client needs to see and call it (default
    /// `read`). Writes the plug-in makes while serving the call need the
    /// plug-in's own permissions and follow its action policies.
    #[serde(default = "read_scope")]
    #[ts(optional, as = "Option<Vec<McpScope>>")]
    pub scopes: Vec<McpScope>,
}

fn read_scope() -> Vec<McpScope> {
    vec![McpScope::Read]
}

impl PluginMcpToolDef {
    /// Whether the input schema is acceptable: absent, or a JSON object
    /// with `"type": "object"` of at most [`MAX_MCP_TOOL_SCHEMA`] bytes.
    pub fn schema_ok(&self) -> bool {
        self.input_schema.as_ref().is_none_or(|schema| {
            schema.get("type").and_then(Value::as_str) == Some("object")
                && serde_json::to_vec(schema).is_ok_and(|b| b.len() <= MAX_MCP_TOOL_SCHEMA)
        })
    }
}

/// The MCP tool name of a plug-in tool: `<plugin>__<tool>` (plug-in ids
/// and names never contain `_`, built-in tools never `__`).
pub fn plugin_mcp_tool_name(plugin: &PluginId, tool: &str) -> String {
    format!("{plugin}__{tool}")
}

/// Splits [`plugin_mcp_tool_name`]; `None` for other names.
pub fn parse_plugin_mcp_tool_name(name: &str) -> Option<(PluginId, String)> {
    let (plugin, tool) = name.split_once("__")?;
    (super::is_plugin_id(plugin) && super::is_contribution_name(tool))
        .then(|| (PluginId::new(plugin), tool.to_owned()))
}

/// The id of a composer / attachment / link action, Markdown block or
/// preview renderer across plug-ins: `<plugin>.<name>` (also the Markdown
/// fence info string).
pub fn plugin_extension_id(plugin: &PluginId, name: &str) -> String {
    format!("{plugin}.{name}")
}

/// Splits [`plugin_extension_id`].
pub fn parse_plugin_extension_id(id: &str) -> Option<(PluginId, String)> {
    super::parse_plugin_panel_id(id)
}

/// The info string of a Markdown block's fence (`acme-crm.ticket`).
pub fn markdown_block_info(plugin: &PluginId, block: &str) -> String {
    plugin_extension_id(plugin, block)
}

/// Whether `mime` is a MIME type pattern of a contribution: `type/sub` or
/// `type/*` in lowercase.
pub fn is_mime_pattern(mime: &str) -> bool {
    let Some((ty, sub)) = mime.split_once('/') else {
        return false;
    };
    let token = |s: &str| {
        !s.is_empty()
            && s.len() <= 64
            && s.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.')
            })
    };
    token(ty) && (sub == "*" || token(sub))
}

/// Whether `ext` is a file extension of a contribution (lowercase letters
/// and digits, 1–16, no dot).
pub fn is_extension(ext: &str) -> bool {
    !ext.is_empty()
        && ext.len() <= 16
        && ext
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn matches_file(
    mime_types: &[String],
    extensions: &[String],
    mime_type: &str,
    filename: Option<&str>,
) -> bool {
    let mime = mime_type.to_ascii_lowercase();
    let by_type = mime_types
        .iter()
        .any(|pattern| match pattern.strip_suffix("/*") {
            Some(ty) => mime.split_once('/').is_some_and(|(t, _)| t == ty),
            None => *pattern == mime,
        });
    let ext = filename
        .and_then(|f| f.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase());
    by_type || ext.is_some_and(|e| extensions.contains(&e))
}

/// The selection in the composer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ComposerSelection {
    /// The selected text (empty: only a cursor).
    pub text: String,
    /// Start offset in the draft's source (UTF-16).
    pub start: u32,
    /// End offset (UTF-16; equals `start` without a selection).
    pub end: u32,
}

/// How a composer edit changes the draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ComposerEditOp {
    /// Insert at the cursor.
    Insert,
    /// Replace the selection (insert at the cursor without one).
    ReplaceSelection,
    /// Replace the whole body.
    ReplaceAll,
}

/// The format of inserted text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ComposerTextFormat {
    /// Plain text.
    #[default]
    Text,
    /// Markdown.
    Markdown,
    /// HTML (sanitized; converted for plain-text drafts).
    Html,
}

/// An edit of the open draft, asked for by a plug-in's composer action or
/// MCP's `edit_open_draft` ([`crate::Event::ComposerEditRequested`]). The
/// composer applies it live (and the user can undo it like typing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ComposerEdit {
    /// What to do.
    pub op: ComposerEditOp,
    /// The text.
    pub text: String,
    /// Its format.
    #[serde(default)]
    #[ts(optional, as = "Option<ComposerTextFormat>")]
    pub format: ComposerTextFormat,
}

/// Where a plug-in action was invoked ([`crate::Request::RunPluginAction`];
/// protocol, `snake_case`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PluginActionTarget {
    /// A composer action on the open draft.
    Composer {
        /// The draft (saved at least once).
        draft_id: DraftId,
        /// The selection.
        selection: ComposerSelection,
    },
    /// An attachment action.
    Attachment {
        /// The message.
        message_id: MessageId,
        /// The attachment's index ([`crate::AttachmentInfo::index`]).
        index: u32,
    },
    /// A link action.
    Link {
        /// The link's URL.
        url: String,
        /// The link's text.
        #[serde(default)]
        text: Option<String>,
        /// The message it is in.
        #[serde(default)]
        message_id: Option<MessageId>,
    },
}

/// An attachment as an action or preview handler gets it (the bytes come
/// separately, as a `Uint8Array`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginAttachmentRef {
    /// The message.
    pub message_id: MessageId,
    /// The attachment's index.
    pub index: u32,
    /// File name.
    pub name: String,
    /// MIME type.
    pub mime_type: String,
    /// Size (bytes).
    pub size: u64,
}

/// A link as a link action gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginLink {
    /// The URL.
    pub url: String,
    /// The link's text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
    /// The message it is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message_id: Option<MessageId>,
}

/// The open draft as a composer action gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginComposerContext {
    /// The draft.
    pub draft_id: DraftId,
    /// The selection.
    pub selection: ComposerSelection,
    /// The draft's mode (`plain`, `html`, `markdown`).
    pub mode: String,
}

/// What a composer / attachment / link action handler receives
/// (plug-in-facing, camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginActionContext {
    /// The action's name.
    pub action: String,
    /// For composer actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub composer: Option<PluginComposerContext>,
    /// For attachment actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub attachment: Option<PluginAttachmentRef>,
    /// For link actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub link: Option<PluginLink>,
}

/// What a Markdown block renderer gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarkdownBlockInput {
    /// The block's name.
    pub block: String,
    /// The text inside the fence.
    pub source: String,
    /// Whether this is for the preview or the message being sent.
    pub purpose: PluginRenderPurpose,
}

/// Why something is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PluginRenderPurpose {
    /// The composer's preview.
    Preview,
    /// The message being sent.
    Send,
}

/// What a Markdown block renderer answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarkdownBlockResult {
    /// HTML for the HTML part (sanitized; at most [`MAX_BLOCK_HTML`]
    /// bytes).
    pub html: String,
    /// Text for the plain-text part.
    pub text: String,
}

/// What an attachment preview renderer answers (tagged by `kind`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginPreview {
    /// A component tree (validated like a panel's).
    Tree {
        /// The tree.
        tree: Vec<PluginUiNode>,
    },
    /// An image.
    Image {
        /// One of [`PREVIEW_IMAGE_TYPES`].
        mime_type: String,
        /// The image, base64 (at most [`MAX_PREVIEW_IMAGE`] bytes
        /// decoded).
        data: String,
        /// Alternative text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        alt: Option<String>,
    },
}

impl PluginPreview {
    /// Whether an image answer has an allowed type and size (trees are
    /// checked with [`super::validate_tree`]).
    pub fn image_ok(&self) -> bool {
        match self {
            Self::Tree { .. } => true,
            Self::Image {
                mime_type, data, ..
            } => {
                PREVIEW_IMAGE_TYPES.contains(&mime_type.as_str())
                    && data.len() / 4 * 3 <= MAX_PREVIEW_IMAGE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn previews_match_types_and_extensions() {
        let csv: PluginAttachmentPreviewDef = serde_json::from_value(json!({
            "id": "csv", "title": "Table", "mimeTypes": ["text/csv"], "extensions": ["csv", "tsv"]
        }))
        .unwrap();
        assert!(csv.matches("text/csv", None));
        assert!(csv.matches("TEXT/CSV", Some("x.bin")));
        assert!(csv.matches("application/octet-stream", Some("data.TSV")));
        assert!(!csv.matches("text/plain", Some("notes.txt")));
        assert!(!csv.matches("text/plain", None));
        assert_eq!(csv.max_bytes(), MAX_PREVIEW_INPUT);
        let images: PluginAttachmentPreviewDef = serde_json::from_value(json!({
            "id": "img", "title": "Image", "mimeTypes": ["image/*"], "maxBytes": 999_999_999u64
        }))
        .unwrap();
        assert!(images.matches("image/heic", None));
        assert!(!images.matches("imagex/heic", None));
        assert_eq!(images.max_bytes(), MAX_PREVIEW_INPUT);
        let any: PluginAttachmentActionDef =
            serde_json::from_value(json!({"id": "save", "title": "Save to CRM"})).unwrap();
        assert!(any.matches("application/pdf", Some("a.pdf")));
        assert!(is_mime_pattern("text/csv"));
        assert!(is_mime_pattern("image/*"));
        assert!(is_mime_pattern("application/vnd.ms-excel"));
        assert!(!is_mime_pattern("*/*"));
        assert!(!is_mime_pattern("Text/CSV"));
        assert!(!is_mime_pattern("text"));
        assert!(is_extension("csv"));
        assert!(!is_extension(".csv"));
        assert!(!is_extension("CSV"));
    }

    #[test]
    fn link_actions_match_hosts() {
        let jira: PluginLinkActionDef = serde_json::from_value(json!({
            "id": "open-ticket", "title": "Open ticket", "hosts": ["*.atlassian.net", "jira.example.com"]
        }))
        .unwrap();
        assert!(jira.matches_host("acme.atlassian.net"));
        assert!(jira.matches_host("jira.example.com"));
        assert!(!jira.matches_host("example.com"));
        let all: PluginLinkActionDef =
            serde_json::from_value(json!({"id": "copy", "title": "Copy"})).unwrap();
        assert!(all.matches_host("anything.example"));
    }

    #[test]
    fn mcp_tools() {
        let plugin = PluginId::new("acme-crm");
        let name = plugin_mcp_tool_name(&plugin, "find-customer");
        assert_eq!(name, "acme-crm__find-customer");
        assert_eq!(
            parse_plugin_mcp_tool_name(&name),
            Some((plugin.clone(), "find-customer".to_owned()))
        );
        assert_eq!(parse_plugin_mcp_tool_name("search_messages"), None);
        assert_eq!(parse_plugin_mcp_tool_name("a__b__c"), None);
        let tool: PluginMcpToolDef = serde_json::from_value(json!({
            "id": "find-customer", "description": "Find a customer by address",
            "inputSchema": {"type": "object", "properties": {"email": {"type": "string"}}}
        }))
        .unwrap();
        assert_eq!(tool.scopes, [McpScope::Read]);
        assert!(tool.schema_ok());
        let bad = PluginMcpToolDef {
            input_schema: Some(json!({"type": "string"})),
            ..tool
        };
        assert!(!bad.schema_ok());
        assert_eq!(markdown_block_info(&plugin, "ticket"), "acme-crm.ticket");
        assert_eq!(
            parse_plugin_extension_id("acme-crm.ticket"),
            Some((plugin, "ticket".to_owned()))
        );
    }

    #[test]
    fn edits_targets_and_previews() {
        let edit: ComposerEdit =
            serde_json::from_value(json!({"op": "replace-selection", "text": "Hi"})).unwrap();
        assert_eq!(edit.format, ComposerTextFormat::Text);
        let target = PluginActionTarget::Attachment {
            message_id: "m1".into(),
            index: 2,
        };
        assert_eq!(
            serde_json::to_value(&target).unwrap(),
            json!({"kind": "attachment", "message_id": "m1", "index": 2})
        );
        let link: PluginActionTarget =
            serde_json::from_value(json!({"kind": "link", "url": "https://x.example/"})).unwrap();
        assert!(matches!(link, PluginActionTarget::Link { text: None, .. }));
        let ctx = PluginActionContext {
            action: "save".into(),
            composer: None,
            attachment: Some(PluginAttachmentRef {
                message_id: "m1".into(),
                index: 0,
                name: "a.csv".into(),
                mime_type: "text/csv".into(),
                size: 3,
            }),
            link: None,
        };
        assert_eq!(
            serde_json::to_value(&ctx).unwrap()["attachment"]["mimeType"],
            "text/csv"
        );
        let image: PluginPreview = serde_json::from_value(
            json!({"kind": "image", "mimeType": "image/png", "data": "iVBORw0KGgo="}),
        )
        .unwrap();
        assert!(image.image_ok());
        let svg = PluginPreview::Image {
            mime_type: "image/svg+xml".into(),
            data: String::new(),
            alt: None,
        };
        assert!(!svg.image_ok());
        let tree: PluginPreview = serde_json::from_value(
            json!({"kind": "tree", "tree": [{"type": "text", "value": "x"}]}),
        )
        .unwrap();
        assert!(tree.image_ok());
    }
}
