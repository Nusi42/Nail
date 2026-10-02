//! `manifest.json` of a plug-in (design.md, "Packaging and development";
//! plan M2d WP60).
//!
//! ```json
//! {
//!   "id": "follow-up",
//!   "name": "Follow-up reminders",
//!   "version": "1.0.0",
//!   "apiVersion": "1",
//!   "main": "dist/index.js",
//!   "permissions": ["mail.read", "ui.dialogs", "storage", "notifications"],
//!   "contributes": {
//!     "commands": [{ "id": "follow-up", "title": "%follow-up-title%", "contexts": ["list", "reader"] }],
//!     "placements": [{ "toolbar": "reader", "after": "mail.forward", "command": "follow-up" }],
//!     "keys": [{ "key": "shift+f", "command": "follow-up", "context": "list" }],
//!     "panels": [{ "id": "due", "title": "Follow-ups" }],
//!     "settings": [{ "type": "number", "key": "days", "title": "Default delay", "default": 1, "min": 0 }],
//!     "locales": { "en": "locales/en.ftl", "de": "locales/de.ftl" }
//!   }
//! }
//! ```
//!
//! **Texts.** `name`, `description` and every `title` / `description` of a
//! contribution are shown to users. A text of the form `%<fluent-id>%`
//! ([`text_key`]) is looked up in the plug-in's own catalogs
//! (`contributes.locales`, language → `.ftl` file in the package) for the
//! user's language, then English, then the id itself; any other text is
//! shown as written. The host resolves them before [`super::PluginInfo`]
//! reaches the UI.
//!
//! **M4a** (ADR 0079) adds `hooks`, `composerActions`, `markdownBlocks`,
//! `attachmentPreviews`, `attachmentActions`, `linkActions`, `jobs` and
//! `mcpTools` ([`super::hooks`], [`super::extensions`], [`super::jobs`]);
//! each needs its permission in the same manifest
//! ([`PluginManifestIssueKind::ContributionNeedsPermission`]).
//!
//! **Placements** are suggestions: on install (after the user confirms in
//! the merge dialog) they become ordinary lines in `layout.kdl`
//! (`item "plugin.follow-up.follow-up"` in `toolbar "reader"`,
//! `plugin-panel "follow-up.due"` after the named widget) that the user can
//! move or delete.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::extensions::{
    MAX_MATCHERS, MAX_MCP_TOOL_DESCRIPTION, MAX_MCP_TOOL_NAME, MAX_PREVIEW_INPUT,
    PluginAttachmentActionDef, PluginAttachmentPreviewDef, PluginComposerActionDef,
    PluginLinkActionDef, PluginMarkdownBlockDef, PluginMcpToolDef, is_extension, is_mime_pattern,
    plugin_mcp_tool_name,
};
use super::hooks::PluginHookDef;
use super::jobs::{MAX_JOBS, PluginJobDef};
use super::permission::{PluginPermission, PluginPermissionError, is_domain_pattern};
use super::runtime::PluginCommandSurface;
use super::ui::PluginSelectOption;
use super::{is_contribution_name, is_plugin_id, is_plugin_version, is_supported_api_version};
use crate::ids::PluginId;
use crate::localized::LocalizedMessage;

/// Largest `manifest.json` (bytes).
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
/// Longest name or title (characters).
pub const MAX_TITLE_LEN: usize = 100;
/// Longest description (characters).
pub const MAX_DESCRIPTION_LEN: usize = 1000;
/// Most commands, placements or keys a manifest may contribute (each).
pub const MAX_CONTRIBUTIONS: usize = 100;
/// Most panels, settings or locales a manifest may contribute (each).
pub const MAX_SMALL_CONTRIBUTIONS: usize = 50;
/// Key-binding contexts of `layout.kdl` a plug-in key may use.
pub const KEY_CONTEXTS: &[&str] = &["global", "list", "reader", "composer", "search", "window"];

/// A plug-in's `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    /// The plug-in id ([`super::is_plugin_id`]), unique per installation.
    pub id: PluginId,
    /// Display name (or `%fluent-id%`).
    pub name: String,
    /// Semantic version ([`super::is_plugin_version`]).
    pub version: String,
    /// The plug-in API major version it is written for (`"1"`).
    pub api_version: String,
    /// The bundled JavaScript entry point, relative to the package root
    /// (`dist/index.js`).
    pub main: String,
    /// One-paragraph description (or `%fluent-id%`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    /// The author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub author: Option<String>,
    /// A web page about the plug-in (shown as text, `https:` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub homepage: Option<String>,
    /// The permissions it needs.
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginPermission>>")]
    pub permissions: Vec<PluginPermission>,
    /// What it adds to Nail.
    #[serde(default)]
    #[ts(optional, as = "Option<PluginContributions>")]
    pub contributes: PluginContributions,
}

/// The contributions of a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginContributions {
    /// Named actions (`plugin.<id>.<command>` in `layout.kdl`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginCommandDef>>")]
    pub commands: Vec<PluginCommandDef>,
    /// Suggested toolbar / menu items and panel positions.
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginPlacement>>")]
    pub placements: Vec<PluginPlacement>,
    /// Suggested key bindings.
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginKeyDef>>")]
    pub keys: Vec<PluginKeyDef>,
    /// Panels placeable in the layout (`plugin-panel "<id>.<panel>"`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginPanelDef>>")]
    pub panels: Vec<PluginPanelDef>,
    /// The settings schema; becomes a generated settings page.
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginSettingDef>>")]
    pub settings: Vec<PluginSettingDef>,
    /// The plug-in's Fluent catalogs: language tag (`en`, `de`, `pt-BR`)
    /// → `.ftl` file in the package (also `nail.l10n` at run time).
    #[serde(default)]
    #[ts(optional, as = "Option<BTreeMap<String, String>>")]
    pub locales: BTreeMap<String, String>,
    /// Mail hooks, at most one per kind (M4a; `hooks.*`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginHookDef>>")]
    pub hooks: Vec<PluginHookDef>,
    /// Composer actions (M4a; `composer`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginComposerActionDef>>")]
    pub composer_actions: Vec<PluginComposerActionDef>,
    /// Markdown block renderers (M4a; `composer`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginMarkdownBlockDef>>")]
    pub markdown_blocks: Vec<PluginMarkdownBlockDef>,
    /// Attachment preview renderers (M4a; `attachments`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginAttachmentPreviewDef>>")]
    pub attachment_previews: Vec<PluginAttachmentPreviewDef>,
    /// Attachment menu actions (M4a; `attachments`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginAttachmentActionDef>>")]
    pub attachment_actions: Vec<PluginAttachmentActionDef>,
    /// Link actions (M4a; no permission: they get only the link the user
    /// picked). Not offered in the reader yet, so users cannot pick them
    /// in this version.
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginLinkActionDef>>")]
    pub link_actions: Vec<PluginLinkActionDef>,
    /// Background jobs (M4a; `background`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginJobDef>>")]
    pub jobs: Vec<PluginJobDef>,
    /// Tools offered to MCP clients (M4a; `mcp.expose`).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginMcpToolDef>>")]
    pub mcp_tools: Vec<PluginMcpToolDef>,
}

/// A command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandDef {
    /// The command name ([`super::is_contribution_name`]).
    pub id: String,
    /// The label (or `%fluent-id%`).
    pub title: String,
    /// Icon name from the theme's icon set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<String>,
    /// A tooltip / palette description (or `%fluent-id%`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    /// Where it can be invoked; empty = everywhere.
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginCommandSurface>>")]
    pub contexts: Vec<PluginCommandSurface>,
}

/// A suggested placement. Exactly one of `toolbar`, `menu` and `panel` is
/// set; at most one of `after` and `before`.
///
/// * `{"toolbar": "reader", "command": "x", "after": "mail.forward"}` —
///   an item in a toolbar of `layout.kdl`.
/// * `{"menu": "context:message-list.row", "command": "x"}` — an item in a
///   menu (`app` or `context:<part>`).
/// * `{"panel": "due", "after": "message-view"}` — the panel after (or
///   before) the first widget of that kind; at the end of the main window
///   when there is none.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginPlacement {
    /// A toolbar name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub toolbar: Option<String>,
    /// A menu name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub menu: Option<String>,
    /// A panel of this plug-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub panel: Option<String>,
    /// The command (toolbar and menu placements).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command: Option<String>,
    /// Place after this action (toolbar / menu) or widget kind (panel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after: Option<String>,
    /// Place before this action or widget kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub before: Option<String>,
}

/// Where a placement goes ([`PluginPlacement::target`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementTarget<'a> {
    /// `toolbar "<name>"`, item = the command.
    Toolbar {
        /// Toolbar name.
        toolbar: &'a str,
        /// Command name.
        command: &'a str,
    },
    /// `menu "<name>"`, item = the command.
    Menu {
        /// Menu name.
        menu: &'a str,
        /// Command name.
        command: &'a str,
    },
    /// A `plugin-panel` widget.
    Panel {
        /// Panel name.
        panel: &'a str,
    },
}

impl PluginPlacement {
    /// Where it goes; `None` when it is malformed (see the type's docs).
    pub fn target(&self) -> Option<PlacementTarget<'_>> {
        match (
            self.toolbar.as_deref(),
            self.menu.as_deref(),
            self.panel.as_deref(),
            self.command.as_deref(),
        ) {
            (Some(toolbar), None, None, Some(command)) => {
                Some(PlacementTarget::Toolbar { toolbar, command })
            }
            (None, Some(menu), None, Some(command)) => {
                Some(PlacementTarget::Menu { menu, command })
            }
            (None, None, Some(panel), None) => Some(PlacementTarget::Panel { panel }),
            _ => None,
        }
    }
}

/// A suggested key binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginKeyDef {
    /// Key sequence as in `layout.kdl` (`shift+f`, `ctrl+alt+r`, `g f`).
    pub key: String,
    /// The command.
    pub command: String,
    /// The key context ([`KEY_CONTEXTS`]); default `global`.
    #[serde(default = "global_context")]
    #[ts(optional, as = "Option<String>")]
    pub context: String,
}

fn global_context() -> String {
    "global".to_owned()
}

/// A panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginPanelDef {
    /// The panel name ([`super::is_contribution_name`]).
    pub id: String,
    /// Its title (or `%fluent-id%`).
    pub title: String,
    /// Icon name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<String>,
}

/// One setting of the generated settings page. Values are JSON: string,
/// integer, boolean, or one of the options' values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum PluginSettingDef {
    /// Text.
    String {
        /// The setting's key ([`super::is_contribution_name`]).
        key: String,
        /// Its label (or `%fluent-id%`).
        title: String,
        /// Help text (or `%fluent-id%`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        description: Option<String>,
        /// The default (empty when absent).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        default: Option<String>,
        /// A multi-line field.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        multiline: bool,
        /// Longest value (characters; at most 10 000).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        max_length: Option<u32>,
    },
    /// An integer.
    Number {
        /// The setting's key.
        key: String,
        /// Its label.
        title: String,
        /// Help text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        description: Option<String>,
        /// The default (0 when absent, or `min`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        default: Option<i64>,
        /// Smallest value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        min: Option<i64>,
        /// Largest value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        max: Option<i64>,
    },
    /// On / off.
    Boolean {
        /// The setting's key.
        key: String,
        /// Its label.
        title: String,
        /// Help text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        description: Option<String>,
        /// The default.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        default: bool,
    },
    /// One of a list.
    Select {
        /// The setting's key.
        key: String,
        /// Its label.
        title: String,
        /// Help text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        description: Option<String>,
        /// The choices (labels may be `%fluent-id%`).
        options: Vec<PluginSelectOption>,
        /// The default (the first option when absent).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        default: Option<String>,
    },
}

/// Longest string setting value (characters).
pub const MAX_SETTING_STRING: u32 = 10_000;

impl PluginSettingDef {
    /// The setting's key.
    pub fn key(&self) -> &str {
        match self {
            Self::String { key, .. }
            | Self::Number { key, .. }
            | Self::Boolean { key, .. }
            | Self::Select { key, .. } => key,
        }
    }

    /// The setting's title.
    pub fn title(&self) -> &str {
        match self {
            Self::String { title, .. }
            | Self::Number { title, .. }
            | Self::Boolean { title, .. }
            | Self::Select { title, .. } => title,
        }
    }

    /// The value used until the user changes it.
    pub fn default_value(&self) -> Value {
        match self {
            Self::String { default, .. } => Value::from(default.clone().unwrap_or_default()),
            Self::Number { default, min, .. } => {
                Value::from(default.or(*min).unwrap_or(0).max(min.unwrap_or(i64::MIN)))
            }
            Self::Boolean { default, .. } => Value::from(*default),
            Self::Select {
                default, options, ..
            } => Value::from(
                default
                    .clone()
                    .or_else(|| options.first().map(|o| o.value.clone()))
                    .unwrap_or_default(),
            ),
        }
    }

    /// Whether `value` is a valid value of this setting.
    pub fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::String { max_length, .. } => value.as_str().is_some_and(|s| {
                let limit = max_length
                    .unwrap_or(MAX_SETTING_STRING)
                    .min(MAX_SETTING_STRING);
                s.chars().count() <= limit as usize
            }),
            Self::Number { min, max, .. } => value
                .as_i64()
                .is_some_and(|n| min.is_none_or(|min| n >= min) && max.is_none_or(|max| n <= max)),
            Self::Boolean { .. } => value.is_boolean(),
            Self::Select { options, .. } => value
                .as_str()
                .is_some_and(|s| options.iter().any(|o| o.value == s)),
        }
    }

    fn is_consistent(&self) -> bool {
        match self {
            Self::String { max_length, .. } => {
                max_length.is_none_or(|m| (1..=MAX_SETTING_STRING).contains(&m))
                    && self.accepts(&self.default_value())
            }
            Self::Number { min, max, .. } => {
                min.zip(*max).is_none_or(|(min, max)| min <= max)
                    && self.accepts(&self.default_value())
            }
            Self::Boolean { .. } => true,
            Self::Select { options, .. } => {
                let mut seen = HashSet::new();
                !options.is_empty()
                    && options.iter().all(|o| seen.insert(o.value.as_str()))
                    && self.accepts(&self.default_value())
            }
        }
    }
}

/// If `text` is a reference into the plug-in's catalogs (`%fluent-id%`),
/// the id.
pub fn text_key(text: &str) -> Option<&str> {
    let key = text.strip_prefix('%')?.strip_suffix('%')?;
    crate::localized::is_message_id(key).then_some(key)
}

/// Whether `path` is a relative path inside a package: `/`-separated
/// parts of letters, digits, `.`, `-`, `_` (no empty, `.` or `..` part, no
/// leading `/`, no `\`), at most 200 characters.
pub fn is_package_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 200
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        })
}

/// Whether `tag` looks like a BCP 47 language tag (`en`, `de`, `pt-BR`,
/// `zh-Hant`): a 2–3 letter language and optional alphanumeric subtags.
pub fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    parts
        .next()
        .is_some_and(|l| (2..=3).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_lowercase()))
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn is_key_sequence(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key.trim() == key
        && key.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
        && !key.contains("  ")
}

fn is_layout_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

fn is_title(text: &str) -> bool {
    !text.trim().is_empty() && text.chars().count() <= MAX_TITLE_LEN
}

/// What is wrong with a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginManifestIssueKind {
    /// Larger than [`MAX_MANIFEST_BYTES`].
    TooLarge,
    /// Not JSON, or a field has the wrong type (the detail says which).
    InvalidJson,
    /// `id` is not a valid plug-in id.
    InvalidId,
    /// `name` is empty or too long, or a description is too long.
    InvalidName,
    /// `version` is not a semantic version.
    InvalidVersion,
    /// `apiVersion` is not one this Nail runs.
    UnsupportedApiVersion,
    /// `main` is not a `.js` / `.mjs` path inside the package.
    InvalidMain,
    /// `homepage` is not an `https:` URL.
    InvalidHomepage,
    /// A permission name Nail does not know.
    UnknownPermission,
    /// A permission of the full API, not available in the preview.
    PermissionNotInPreview,
    /// `network:` with an invalid host pattern.
    InvalidNetworkDomain,
    /// A permission listed twice.
    DuplicatePermission,
    /// `mail.read.body` without `mail.read`.
    BodyWithoutRead,
    /// A command with an invalid id or title, or unknown contexts.
    InvalidCommand,
    /// Two commands with the same id.
    DuplicateCommand,
    /// A placement that is not exactly one of toolbar / menu / panel, has
    /// both `after` and `before`, or names an invalid toolbar, menu or
    /// neighbour.
    InvalidPlacement,
    /// A placement or key names a command the manifest does not declare.
    UnknownCommand,
    /// A placement names a panel the manifest does not declare.
    UnknownPanel,
    /// A key binding with an invalid key or context.
    InvalidKey,
    /// A panel with an invalid id or title.
    InvalidPanel,
    /// Two panels with the same id.
    DuplicatePanel,
    /// A setting with an invalid key, title, bounds, options or default.
    InvalidSetting,
    /// Two settings with the same key.
    DuplicateSetting,
    /// A locale with an invalid language tag or path.
    InvalidLocale,
    /// More contributions of one kind than allowed.
    TooManyContributions,
    /// A permission without one it needs (`mail.organize`, `hooks.arrival`
    /// and `hooks.open` need `mail.read`; M4a). `mail.read.body` keeps
    /// [`Self::BodyWithoutRead`].
    MissingRequiredPermission,
    /// A contribution whose permission is not declared (hooks, composer,
    /// attachment, job and MCP contributions; the detail names it).
    ContributionNeedsPermission,
    /// A hook with an invalid filter or budget.
    InvalidHook,
    /// Two hooks of the same kind.
    DuplicateHook,
    /// A composer action, Markdown block, attachment preview / action or
    /// link action with an invalid id, title or matcher.
    InvalidExtension,
    /// Two extensions of one kind with the same id.
    DuplicateExtension,
    /// A job with an invalid id or title, nothing to run on, or an
    /// invalid / too frequent schedule.
    InvalidJob,
    /// Two jobs with the same id.
    DuplicateJob,
    /// An MCP tool with an invalid id, description, schema or scopes, or
    /// a name longer than MCP clients accept.
    InvalidMcpTool,
    /// Two MCP tools with the same id.
    DuplicateMcpTool,
}

impl PluginManifestIssueKind {
    /// All issues.
    pub const ALL: [PluginManifestIssueKind; 35] = [
        Self::TooLarge,
        Self::InvalidJson,
        Self::InvalidId,
        Self::InvalidName,
        Self::InvalidVersion,
        Self::UnsupportedApiVersion,
        Self::InvalidMain,
        Self::InvalidHomepage,
        Self::UnknownPermission,
        Self::PermissionNotInPreview,
        Self::InvalidNetworkDomain,
        Self::DuplicatePermission,
        Self::BodyWithoutRead,
        Self::InvalidCommand,
        Self::DuplicateCommand,
        Self::InvalidPlacement,
        Self::UnknownCommand,
        Self::UnknownPanel,
        Self::InvalidKey,
        Self::InvalidPanel,
        Self::DuplicatePanel,
        Self::InvalidSetting,
        Self::DuplicateSetting,
        Self::InvalidLocale,
        Self::TooManyContributions,
        Self::MissingRequiredPermission,
        Self::ContributionNeedsPermission,
        Self::InvalidHook,
        Self::DuplicateHook,
        Self::InvalidExtension,
        Self::DuplicateExtension,
        Self::InvalidJob,
        Self::DuplicateJob,
        Self::InvalidMcpTool,
        Self::DuplicateMcpTool,
    ];

    /// Fluent message id (`plugin-manifest-<kind>`; arguments `$path` and
    /// `$detail`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::TooLarge => "plugin-manifest-too-large",
            Self::InvalidJson => "plugin-manifest-invalid-json",
            Self::InvalidId => "plugin-manifest-invalid-id",
            Self::InvalidName => "plugin-manifest-invalid-name",
            Self::InvalidVersion => "plugin-manifest-invalid-version",
            Self::UnsupportedApiVersion => "plugin-manifest-unsupported-api-version",
            Self::InvalidMain => "plugin-manifest-invalid-main",
            Self::InvalidHomepage => "plugin-manifest-invalid-homepage",
            Self::UnknownPermission => "plugin-manifest-unknown-permission",
            Self::PermissionNotInPreview => "plugin-manifest-permission-not-in-preview",
            Self::InvalidNetworkDomain => "plugin-manifest-invalid-network-domain",
            Self::DuplicatePermission => "plugin-manifest-duplicate-permission",
            Self::BodyWithoutRead => "plugin-manifest-body-without-read",
            Self::InvalidCommand => "plugin-manifest-invalid-command",
            Self::DuplicateCommand => "plugin-manifest-duplicate-command",
            Self::InvalidPlacement => "plugin-manifest-invalid-placement",
            Self::UnknownCommand => "plugin-manifest-unknown-command",
            Self::UnknownPanel => "plugin-manifest-unknown-panel",
            Self::InvalidKey => "plugin-manifest-invalid-key",
            Self::InvalidPanel => "plugin-manifest-invalid-panel",
            Self::DuplicatePanel => "plugin-manifest-duplicate-panel",
            Self::InvalidSetting => "plugin-manifest-invalid-setting",
            Self::DuplicateSetting => "plugin-manifest-duplicate-setting",
            Self::InvalidLocale => "plugin-manifest-invalid-locale",
            Self::TooManyContributions => "plugin-manifest-too-many-contributions",
            Self::MissingRequiredPermission => "plugin-manifest-missing-required-permission",
            Self::ContributionNeedsPermission => "plugin-manifest-contribution-needs-permission",
            Self::InvalidHook => "plugin-manifest-invalid-hook",
            Self::DuplicateHook => "plugin-manifest-duplicate-hook",
            Self::InvalidExtension => "plugin-manifest-invalid-extension",
            Self::DuplicateExtension => "plugin-manifest-duplicate-extension",
            Self::InvalidJob => "plugin-manifest-invalid-job",
            Self::DuplicateJob => "plugin-manifest-duplicate-job",
            Self::InvalidMcpTool => "plugin-manifest-invalid-mcp-tool",
            Self::DuplicateMcpTool => "plugin-manifest-duplicate-mcp-tool",
        }
    }
}

/// A problem with a manifest, shown by the install dialog and logged by
/// `nail plugin dev`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginManifestIssue {
    /// What.
    pub kind: PluginManifestIssueKind,
    /// Where, as a JSON path (`contributes.commands[0].id`); empty for the
    /// whole file.
    pub path: String,
    /// The offending value or the parser's message (English, not
    /// translated; shown as a detail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<String>,
}

impl PluginManifestIssue {
    fn new(kind: PluginManifestIssueKind, path: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
            detail: None,
        }
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The user-facing text.
    pub fn localized(&self) -> LocalizedMessage {
        LocalizedMessage::new(self.kind.message_id())
            .arg("path", self.path.as_str())
            .arg("detail", self.detail.clone().unwrap_or_default())
    }
}

impl PluginManifest {
    /// Parses and validates `manifest.json`. Every problem found is
    /// returned (permission names are checked one by one, so an unknown
    /// permission is reported as such rather than as invalid JSON).
    pub fn parse(bytes: &[u8]) -> Result<Self, Vec<PluginManifestIssue>> {
        use PluginManifestIssueKind as K;
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(vec![PluginManifestIssue::new(K::TooLarge, "")]);
        }
        let mut value: Value = serde_json::from_slice(bytes).map_err(|e| {
            vec![PluginManifestIssue::new(K::InvalidJson, "").detail(e.to_string())]
        })?;
        let mut issues = Vec::new();
        if let Some(Value::Array(permissions)) = value.get_mut("permissions") {
            let mut kept = Vec::new();
            for (i, p) in permissions.drain(..).enumerate() {
                let path = format!("permissions[{i}]");
                match p.as_str().map(PluginPermission::parse) {
                    Some(Ok(_)) => kept.push(p),
                    Some(Err(err)) => {
                        let kind = match err {
                            PluginPermissionError::Unknown(_) => K::UnknownPermission,
                            PluginPermissionError::NotInPreview(_) => K::PermissionNotInPreview,
                            PluginPermissionError::InvalidDomain(_) => K::InvalidNetworkDomain,
                        };
                        issues.push(PluginManifestIssue::new(kind, path).detail(p.to_string()));
                    }
                    None => issues
                        .push(PluginManifestIssue::new(K::InvalidJson, path).detail(p.to_string())),
                }
            }
            *permissions = kept;
        }
        let manifest: PluginManifest = serde_json::from_value(value).map_err(|e| {
            let mut all = issues.clone();
            all.push(PluginManifestIssue::new(K::InvalidJson, "").detail(e.to_string()));
            all
        })?;
        issues.extend(manifest.validate());
        if issues.is_empty() {
            Ok(manifest)
        } else {
            Err(issues)
        }
    }

    /// Checks a parsed manifest (everything but the JSON shape).
    pub fn validate(&self) -> Vec<PluginManifestIssue> {
        use PluginManifestIssueKind as K;
        let mut out = Vec::new();
        let mut push = |kind, path: String, detail: &str| {
            out.push(PluginManifestIssue::new(kind, path).detail(detail));
        };

        if !is_plugin_id(self.id.as_str()) {
            push(K::InvalidId, "id".into(), self.id.as_str());
        }
        if !is_title(&self.name) {
            push(K::InvalidName, "name".into(), &self.name);
        }
        if let Some(d) = &self.description
            && d.chars().count() > MAX_DESCRIPTION_LEN
        {
            push(K::InvalidName, "description".into(), "");
        }
        if !is_plugin_version(&self.version) {
            push(K::InvalidVersion, "version".into(), &self.version);
        }
        if !is_supported_api_version(&self.api_version) {
            push(
                K::UnsupportedApiVersion,
                "apiVersion".into(),
                &self.api_version,
            );
        }
        if !is_package_path(&self.main)
            || !(self.main.ends_with(".js") || self.main.ends_with(".mjs"))
        {
            push(K::InvalidMain, "main".into(), &self.main);
        }
        if let Some(home) = &self.homepage
            && (!home.starts_with("https://")
                || home.len() > 500
                || home.contains(char::is_whitespace))
        {
            push(K::InvalidHomepage, "homepage".into(), home);
        }

        let mut seen = HashSet::new();
        for (i, p) in self.permissions.iter().enumerate() {
            if !seen.insert(p) {
                push(
                    K::DuplicatePermission,
                    format!("permissions[{i}]"),
                    &p.name(),
                );
            }
        }
        if seen.contains(&PluginPermission::MailReadBody)
            && !seen.contains(&PluginPermission::MailRead)
        {
            push(K::BodyWithoutRead, "permissions".into(), "mail.read.body");
        }
        for p in &self.permissions {
            if *p == PluginPermission::MailReadBody {
                continue;
            }
            for needed in p.requires() {
                if !seen.contains(needed) {
                    push(
                        K::MissingRequiredPermission,
                        "permissions".into(),
                        &format!("{p} needs {needed}"),
                    );
                }
            }
        }

        let c = &self.contributes;
        for (name, len, max) in [
            ("commands", c.commands.len(), MAX_CONTRIBUTIONS),
            ("placements", c.placements.len(), MAX_CONTRIBUTIONS),
            ("keys", c.keys.len(), MAX_CONTRIBUTIONS),
            ("panels", c.panels.len(), MAX_SMALL_CONTRIBUTIONS),
            ("settings", c.settings.len(), MAX_SMALL_CONTRIBUTIONS),
            ("locales", c.locales.len(), MAX_SMALL_CONTRIBUTIONS),
            (
                "composerActions",
                c.composer_actions.len(),
                MAX_SMALL_CONTRIBUTIONS,
            ),
            (
                "markdownBlocks",
                c.markdown_blocks.len(),
                MAX_SMALL_CONTRIBUTIONS,
            ),
            (
                "attachmentPreviews",
                c.attachment_previews.len(),
                MAX_SMALL_CONTRIBUTIONS,
            ),
            (
                "attachmentActions",
                c.attachment_actions.len(),
                MAX_SMALL_CONTRIBUTIONS,
            ),
            ("linkActions", c.link_actions.len(), MAX_SMALL_CONTRIBUTIONS),
            ("jobs", c.jobs.len(), MAX_JOBS),
            ("mcpTools", c.mcp_tools.len(), MAX_SMALL_CONTRIBUTIONS),
        ] {
            if len > max {
                push(K::TooManyContributions, format!("contributes.{name}"), "");
            }
        }

        let mut commands = HashSet::new();
        for (i, cmd) in c.commands.iter().enumerate() {
            let path = format!("contributes.commands[{i}]");
            let desc_ok = cmd
                .description
                .as_ref()
                .is_none_or(|d| d.chars().count() <= MAX_DESCRIPTION_LEN);
            if !is_contribution_name(&cmd.id) || !is_title(&cmd.title) || !desc_ok {
                push(K::InvalidCommand, path, &cmd.id);
            } else if !commands.insert(cmd.id.as_str()) {
                push(K::DuplicateCommand, path, &cmd.id);
            }
        }
        let mut panels = HashSet::new();
        for (i, panel) in c.panels.iter().enumerate() {
            let path = format!("contributes.panels[{i}]");
            if !is_contribution_name(&panel.id) || !is_title(&panel.title) {
                push(K::InvalidPanel, path, &panel.id);
            } else if !panels.insert(panel.id.as_str()) {
                push(K::DuplicatePanel, path, &panel.id);
            }
        }
        for (i, placement) in c.placements.iter().enumerate() {
            let path = format!("contributes.placements[{i}]");
            let neighbours_ok = !(placement.after.is_some() && placement.before.is_some())
                && [&placement.after, &placement.before]
                    .iter()
                    .all(|n| n.as_deref().is_none_or(is_layout_name));
            match placement.target() {
                Some(_) if !neighbours_ok => push(K::InvalidPlacement, path, ""),
                Some(PlacementTarget::Toolbar {
                    toolbar: name,
                    command,
                })
                | Some(PlacementTarget::Menu {
                    menu: name,
                    command,
                }) => {
                    let menu_ok = placement.menu.is_none()
                        || name == "app"
                        || name.strip_prefix("context:").is_some_and(is_layout_name);
                    if (placement.toolbar.is_some() && !is_layout_name(name)) || !menu_ok {
                        push(K::InvalidPlacement, path, name);
                    } else if !commands.contains(command) {
                        push(K::UnknownCommand, path, command);
                    }
                }
                Some(PlacementTarget::Panel { panel }) => {
                    if !panels.contains(panel) {
                        push(K::UnknownPanel, path, panel);
                    }
                }
                None => push(K::InvalidPlacement, path, ""),
            }
        }
        for (i, key) in c.keys.iter().enumerate() {
            let path = format!("contributes.keys[{i}]");
            if !is_key_sequence(&key.key) || !KEY_CONTEXTS.contains(&key.context.as_str()) {
                push(K::InvalidKey, path, &key.key);
            } else if !commands.contains(key.command.as_str()) {
                push(K::UnknownCommand, path, &key.command);
            }
        }
        let mut settings = HashSet::new();
        for (i, setting) in c.settings.iter().enumerate() {
            let path = format!("contributes.settings[{i}]");
            if !is_contribution_name(setting.key())
                || !is_title(setting.title())
                || !setting.is_consistent()
            {
                push(K::InvalidSetting, path, setting.key());
            } else if !settings.insert(setting.key()) {
                push(K::DuplicateSetting, path, setting.key());
            }
        }
        for (tag, file) in &c.locales {
            if !is_language_tag(tag) || !is_package_path(file) || !file.ends_with(".ftl") {
                push(K::InvalidLocale, format!("contributes.locales.{tag}"), file);
            }
        }
        self.validate_extensions(&mut out);
        out
    }

    /// The M4a contributions (hooks, composer, attachments, links, jobs,
    /// MCP tools).
    fn validate_extensions(&self, out: &mut Vec<PluginManifestIssue>) {
        use PluginManifestIssueKind as K;
        let mut push = |kind, path: String, detail: &str| {
            out.push(PluginManifestIssue::new(kind, path).detail(detail));
        };
        let c = &self.contributes;
        let declared: HashSet<&PluginPermission> = self.permissions.iter().collect();
        let needs = |permission: PluginPermission, path: &str, used: bool| {
            (used && !declared.contains(&permission)).then(|| (path.to_owned(), permission.name()))
        };
        let missing: Vec<(String, String)> = [
            needs(
                PluginPermission::HooksArrival,
                "contributes.hooks",
                c.hooks
                    .iter()
                    .any(|h| h.hook == super::hooks::PluginHookKind::Arrival),
            ),
            needs(
                PluginPermission::HooksBeforeSend,
                "contributes.hooks",
                c.hooks
                    .iter()
                    .any(|h| h.hook == super::hooks::PluginHookKind::BeforeSend),
            ),
            needs(
                PluginPermission::HooksOpen,
                "contributes.hooks",
                c.hooks
                    .iter()
                    .any(|h| h.hook == super::hooks::PluginHookKind::Open),
            ),
            needs(
                PluginPermission::Composer,
                "contributes.composerActions",
                !c.composer_actions.is_empty(),
            ),
            needs(
                PluginPermission::Composer,
                "contributes.markdownBlocks",
                !c.markdown_blocks.is_empty(),
            ),
            needs(
                PluginPermission::Attachments,
                "contributes.attachmentPreviews",
                !c.attachment_previews.is_empty(),
            ),
            needs(
                PluginPermission::Attachments,
                "contributes.attachmentActions",
                !c.attachment_actions.is_empty(),
            ),
            needs(
                PluginPermission::Background,
                "contributes.jobs",
                !c.jobs.is_empty(),
            ),
            needs(
                PluginPermission::McpExpose,
                "contributes.mcpTools",
                !c.mcp_tools.is_empty(),
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        for (path, permission) in missing {
            push(K::ContributionNeedsPermission, path, &permission);
        }

        let mut hooks = HashSet::new();
        for (i, hook) in c.hooks.iter().enumerate() {
            let path = format!("contributes.hooks[{i}]");
            if !hook.is_valid() {
                push(K::InvalidHook, path, hook.hook.as_str());
            } else if !hooks.insert(hook.hook) {
                push(K::DuplicateHook, path, hook.hook.as_str());
            }
        }

        let matchers_ok = |mimes: &[String], exts: &[String]| {
            mimes.len() <= MAX_MATCHERS
                && exts.len() <= MAX_MATCHERS
                && mimes.iter().all(|m| is_mime_pattern(m))
                && exts.iter().all(|e| is_extension(e))
        };
        let mut check = |list: &str, items: Vec<(&str, bool)>| {
            let mut seen = HashSet::new();
            for (i, (id, valid)) in items.into_iter().enumerate() {
                let path = format!("contributes.{list}[{i}]");
                if !is_contribution_name(id) || !valid {
                    push(K::InvalidExtension, path, id);
                } else if !seen.insert(id) {
                    push(K::DuplicateExtension, path, id);
                }
            }
        };
        check(
            "composerActions",
            c.composer_actions
                .iter()
                .map(|a| {
                    let desc_ok = a
                        .description
                        .as_ref()
                        .is_none_or(|d| d.chars().count() <= MAX_DESCRIPTION_LEN);
                    (a.id.as_str(), is_title(&a.title) && desc_ok)
                })
                .collect(),
        );
        check(
            "markdownBlocks",
            c.markdown_blocks
                .iter()
                .map(|b| {
                    let template_ok = b
                        .template
                        .as_ref()
                        .is_none_or(|t| t.chars().count() <= MAX_DESCRIPTION_LEN);
                    (b.id.as_str(), is_title(&b.title) && template_ok)
                })
                .collect(),
        );
        check(
            "attachmentPreviews",
            c.attachment_previews
                .iter()
                .map(|p| {
                    let some_matcher = !p.mime_types.is_empty() || !p.extensions.is_empty();
                    let size_ok = p.max_bytes.is_none_or(|b| b > 0 && b <= MAX_PREVIEW_INPUT);
                    (
                        p.id.as_str(),
                        is_title(&p.title)
                            && some_matcher
                            && size_ok
                            && matchers_ok(&p.mime_types, &p.extensions),
                    )
                })
                .collect(),
        );
        check(
            "attachmentActions",
            c.attachment_actions
                .iter()
                .map(|a| {
                    (
                        a.id.as_str(),
                        is_title(&a.title) && matchers_ok(&a.mime_types, &a.extensions),
                    )
                })
                .collect(),
        );
        check(
            "linkActions",
            c.link_actions
                .iter()
                .map(|a| {
                    let hosts_ok = a.hosts.len() <= MAX_MATCHERS
                        && a.hosts.iter().all(|h| is_domain_pattern(h));
                    (a.id.as_str(), is_title(&a.title) && hosts_ok)
                })
                .collect(),
        );

        let mut jobs = HashSet::new();
        for (i, job) in c.jobs.iter().enumerate() {
            let path = format!("contributes.jobs[{i}]");
            let title_ok = job.title.as_deref().is_none_or(is_title);
            if !is_contribution_name(&job.id) || !title_ok || !job.is_valid() {
                push(K::InvalidJob, path, &job.id);
            } else if !jobs.insert(job.id.as_str()) {
                push(K::DuplicateJob, path, &job.id);
            }
        }

        let mut tools = HashSet::new();
        for (i, tool) in c.mcp_tools.iter().enumerate() {
            let path = format!("contributes.mcpTools[{i}]");
            let name_ok = is_contribution_name(&tool.id)
                && plugin_mcp_tool_name(&self.id, &tool.id).len() <= MAX_MCP_TOOL_NAME;
            let description_ok = !tool.description.trim().is_empty()
                && tool.description.chars().count() <= MAX_MCP_TOOL_DESCRIPTION;
            if !name_ok || !description_ok || !tool.schema_ok() || tool.scopes.is_empty() {
                push(K::InvalidMcpTool, path, &tool.id);
            } else if !tools.insert(tool.id.as_str()) {
                push(K::DuplicateMcpTool, path, &tool.id);
            }
        }
    }

    /// A command by name.
    pub fn command(&self, name: &str) -> Option<&PluginCommandDef> {
        self.contributes.commands.iter().find(|c| c.id == name)
    }

    /// A panel by name.
    pub fn panel(&self, name: &str) -> Option<&PluginPanelDef> {
        self.contributes.panels.iter().find(|p| p.id == name)
    }

    /// The hook of a kind, if declared (M4a).
    pub fn hook(&self, kind: super::hooks::PluginHookKind) -> Option<&PluginHookDef> {
        self.contributes.hooks.iter().find(|h| h.hook == kind)
    }

    /// A job by name (M4a).
    pub fn job(&self, name: &str) -> Option<&PluginJobDef> {
        self.contributes.jobs.iter().find(|j| j.id == name)
    }

    /// Whether the manifest declares `permission`.
    pub fn declares(&self, permission: &PluginPermission) -> bool {
        self.permissions.contains(permission)
    }

    /// Every setting at its default.
    pub fn default_settings(&self) -> BTreeMap<String, Value> {
        self.contributes
            .settings
            .iter()
            .map(|s| (s.key().to_owned(), s.default_value()))
            .collect()
    }

    /// The keys in `values` that are unknown or invalid; empty when every
    /// value is fine. Missing keys are fine (they keep their default).
    pub fn invalid_settings(&self, values: &BTreeMap<String, Value>) -> Vec<String> {
        values
            .iter()
            .filter(|(key, value)| {
                !self
                    .contributes
                    .settings
                    .iter()
                    .any(|s| s.key() == key.as_str() && s.accepts(value))
            })
            .map(|(key, _)| key.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn example() -> Value {
        json!({
            "id": "follow-up",
            "name": "Follow-up reminders",
            "version": "1.0.0",
            "apiVersion": "1",
            "main": "dist/index.js",
            "permissions": ["mail.read", "ui.dialogs", "storage", "notifications"],
            "contributes": {
                "commands": [{ "id": "follow-up", "title": "%follow-up-title%", "icon": "flag-clock",
                               "contexts": ["list", "reader"] }],
                "placements": [
                    { "toolbar": "reader", "after": "mail.forward", "command": "follow-up" },
                    { "menu": "context:message-list.row", "command": "follow-up" },
                    { "panel": "due", "after": "message-view" }
                ],
                "keys": [{ "key": "shift+f", "command": "follow-up", "context": "list" }],
                "panels": [{ "id": "due", "title": "Follow-ups" }],
                "settings": [
                    { "type": "number", "key": "days", "title": "Default delay", "default": 1, "min": 0, "max": 30 },
                    { "type": "select", "key": "list", "title": "List",
                      "options": [{ "value": "a", "label": "A" }, { "value": "b", "label": "B" }] },
                    { "type": "boolean", "key": "notify", "title": "Notify" },
                    { "type": "string", "key": "prefix", "title": "Prefix", "maxLength": 20 }
                ],
                "locales": { "en": "locales/en.ftl", "pt-BR": "locales/pt-BR.ftl" }
            }
        })
    }

    fn parse(value: &Value) -> Result<PluginManifest, Vec<PluginManifestIssue>> {
        PluginManifest::parse(value.to_string().as_bytes())
    }

    fn kinds(value: &Value) -> Vec<PluginManifestIssueKind> {
        parse(value)
            .unwrap_err()
            .into_iter()
            .map(|i| i.kind)
            .collect()
    }

    #[test]
    fn the_example_manifest_is_valid() {
        let m = parse(&example()).unwrap();
        assert_eq!(m.id, PluginId::new("follow-up"));
        assert_eq!(m.api_version, "1");
        assert_eq!(m.contributes.keys[0].context, "list");
        assert!(m.declares(&PluginPermission::Storage));
        assert!(m.command("follow-up").is_some() && m.panel("due").is_some());
        assert_eq!(
            text_key(&m.contributes.commands[0].title),
            Some("follow-up-title")
        );
        assert_eq!(
            m.contributes.placements[0].target(),
            Some(PlacementTarget::Toolbar {
                toolbar: "reader",
                command: "follow-up"
            })
        );
        // Serializes back to the same camelCase shape.
        let back: PluginManifest =
            serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        assert_eq!(serde_json::to_value(&m).unwrap()["apiVersion"], "1");
    }

    #[test]
    fn minimal_manifest() {
        let m = parse(&json!({
            "id": "x", "name": "X", "version": "0.1.0", "apiVersion": "1", "main": "index.js"
        }))
        .unwrap();
        assert!(m.permissions.is_empty());
        assert_eq!(m.contributes, PluginContributions::default());
        assert_eq!(m.contributes.keys.len(), 0);
    }

    #[test]
    fn permission_problems_are_reported_one_by_one() {
        let mut v = example();
        v["permissions"] = json!([
            "mail.read.body",
            "ui.webview",
            "teleport",
            "network:https://x.org",
            "storage",
            "storage",
            3
        ]);
        use PluginManifestIssueKind as K;
        assert_eq!(
            kinds(&v),
            [
                K::PermissionNotInPreview,
                K::UnknownPermission,
                K::InvalidNetworkDomain,
                K::InvalidJson,
                K::DuplicatePermission,
                K::BodyWithoutRead,
            ]
        );
    }

    #[test]
    fn header_fields_are_checked() {
        use PluginManifestIssueKind as K;
        let mut v = example();
        v["id"] = json!("Follow.Up");
        v["name"] = json!("  ");
        v["version"] = json!("1.0");
        v["apiVersion"] = json!("2");
        v["main"] = json!("../evil.js");
        v["homepage"] = json!("http://example.com");
        assert_eq!(
            kinds(&v),
            [
                K::InvalidId,
                K::InvalidName,
                K::InvalidVersion,
                K::UnsupportedApiVersion,
                K::InvalidMain,
                K::InvalidHomepage,
            ]
        );
        let mut v = example();
        v["main"] = json!("dist/index.ts");
        assert_eq!(kinds(&v), [K::InvalidMain]);
        assert_eq!(
            PluginManifest::parse(b"{not json").unwrap_err()[0].kind,
            K::InvalidJson
        );
        assert_eq!(
            PluginManifest::parse(&vec![b' '; MAX_MANIFEST_BYTES + 1]).unwrap_err()[0].kind,
            K::TooLarge
        );
        let mut v = example();
        v.as_object_mut().unwrap().remove("main");
        assert_eq!(kinds(&v), [K::InvalidJson]);
    }

    #[test]
    fn contributions_are_checked() {
        use PluginManifestIssueKind as K;
        let mut v = example();
        let c = &mut v["contributes"];
        c["commands"] = json!([
            { "id": "follow-up", "title": "A" },
            { "id": "follow-up", "title": "B" },
            { "id": "Bad", "title": "C" },
        ]);
        c["placements"] = json!([
            { "toolbar": "reader", "command": "nope" },
            { "toolbar": "reader", "menu": "app", "command": "follow-up" },
            { "panel": "missing" },
            { "toolbar": "reader", "command": "follow-up", "after": "a", "before": "b" },
            { "menu": "sidebar", "command": "follow-up" },
            { "panel": "due", "command": "follow-up" },
        ]);
        c["keys"] = json!([
            { "key": "shift+f", "command": "nope" },
            { "key": "", "command": "follow-up" },
            { "key": "x", "command": "follow-up", "context": "everywhere" },
        ]);
        c["panels"] = json!([{ "id": "due", "title": "D" }, { "id": "due", "title": "E" }]);
        c["settings"] = json!([
            { "type": "number", "key": "n", "title": "N", "min": 5, "max": 1 },
            { "type": "select", "key": "s", "title": "S", "options": [] },
            { "type": "boolean", "key": "b", "title": "B" },
            { "type": "boolean", "key": "b", "title": "B again" },
            { "type": "string", "key": "t", "title": "T", "default": "long", "maxLength": 2 },
        ]);
        c["locales"] = json!({ "english": "en.ftl", "de": "de.txt" });
        assert_eq!(
            kinds(&v),
            [
                K::DuplicateCommand,
                K::InvalidCommand,
                K::DuplicatePanel,
                K::UnknownCommand,
                K::InvalidPlacement,
                K::UnknownPanel,
                K::InvalidPlacement,
                K::InvalidPlacement,
                K::InvalidPlacement,
                K::UnknownCommand,
                K::InvalidKey,
                K::InvalidKey,
                K::InvalidSetting,
                K::InvalidSetting,
                K::DuplicateSetting,
                K::InvalidSetting,
                K::InvalidLocale,
                K::InvalidLocale,
            ]
        );
        let issues = parse(&v).unwrap_err();
        assert_eq!(issues[0].path, "contributes.commands[1]");
        let text = issues[0].localized();
        assert_eq!(text.id, "plugin-manifest-duplicate-command");
        assert_eq!(text.args["path"], "contributes.commands[1]".into());
    }

    fn m4a_example() -> Value {
        json!({
            "id": "acme-crm", "name": "ACME CRM", "version": "2.0.0", "apiVersion": "1",
            "main": "dist/index.js",
            "permissions": ["mail.read", "mail.organize", "hooks.arrival", "hooks.beforeSend",
                            "hooks.open", "composer", "attachments", "background", "mcp.expose",
                            "tasks.write"],
            "contributes": {
                "hooks": [
                    { "hook": "arrival", "filter": "from:@acme.example" },
                    { "hook": "beforeSend", "timeoutMs": 1500 },
                    { "hook": "open" }
                ],
                "composerActions": [{ "id": "insert-ticket", "title": "Insert ticket" }],
                "markdownBlocks": [{ "id": "ticket", "title": "Ticket", "template": "4711" }],
                "attachmentPreviews": [{ "id": "csv", "title": "Table", "mimeTypes": ["text/csv"],
                                         "extensions": ["csv"] }],
                "attachmentActions": [{ "id": "save", "title": "Save to CRM" }],
                "linkActions": [{ "id": "open", "title": "Open in CRM", "hosts": ["*.acme.example"] }],
                "jobs": [{ "id": "sync", "schedule": "*/30 * * * *", "events": ["app.started"] }],
                "mcpTools": [{ "id": "find-customer", "description": "Find a customer",
                               "inputSchema": { "type": "object" } }]
            }
        })
    }

    #[test]
    fn m4a_contributions_are_valid() {
        let m = parse(&m4a_example()).unwrap();
        assert_eq!(m.contributes.hooks.len(), 3);
        assert!(
            m.hook(super::super::hooks::PluginHookKind::BeforeSend)
                .is_some()
        );
        assert!(m.job("sync").is_some());
        let value = serde_json::to_value(&m).unwrap();
        assert!(value["contributes"]["composerActions"].is_array());
        assert!(value["contributes"]["mcpTools"][0]["inputSchema"].is_object());
        let back: PluginManifest = serde_json::from_value(value).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn m4a_contributions_are_checked() {
        use PluginManifestIssueKind as K;
        let mut v = m4a_example();
        v["permissions"] = json!(["hooks.arrival", "mail.organize"]);
        assert_eq!(
            kinds(&v),
            [
                K::MissingRequiredPermission,
                K::MissingRequiredPermission,
                K::ContributionNeedsPermission,
                K::ContributionNeedsPermission,
                K::ContributionNeedsPermission,
                K::ContributionNeedsPermission,
                K::ContributionNeedsPermission,
                K::ContributionNeedsPermission,
                K::ContributionNeedsPermission,
                K::ContributionNeedsPermission,
            ]
        );
        let mut v = m4a_example();
        let c = &mut v["contributes"];
        c["hooks"] = json!([
            { "hook": "arrival" }, { "hook": "arrival" }, { "hook": "beforeSend", "filter": "x" }
        ]);
        c["composerActions"] = json!([{ "id": "a", "title": "A" }, { "id": "a", "title": "B" }]);
        c["attachmentPreviews"] = json!([{ "id": "p", "title": "P" },
                                         { "id": "q", "title": "Q", "mimeTypes": ["Text/CSV"] }]);
        c["linkActions"] = json!([{ "id": "l", "title": "L", "hosts": ["https://x.example"] }]);
        c["jobs"] = json!([{ "id": "j", "schedule": "* * * * *" }, { "id": "k" },
                           { "id": "m", "events": ["app.started"] },
                           { "id": "m", "events": ["app.started"] }]);
        c["mcpTools"] = json!([
            { "id": "t", "description": " " },
            { "id": "u", "description": "U", "inputSchema": { "type": "array" } },
            { "id": "a-tool-name-that-is-too-long-for-mcp-clients-with-the-plugin-id", "description": "V" },
            { "id": "w", "description": "W", "scopes": [] },
            { "id": "x", "description": "X" }, { "id": "x", "description": "X" }
        ]);
        assert_eq!(
            kinds(&v),
            [
                K::DuplicateHook,
                K::InvalidHook,
                K::DuplicateExtension,
                K::InvalidExtension,
                K::InvalidExtension,
                K::InvalidExtension,
                K::InvalidJob,
                K::InvalidJob,
                K::DuplicateJob,
                K::InvalidMcpTool,
                K::InvalidMcpTool,
                K::InvalidMcpTool,
                K::InvalidMcpTool,
                K::DuplicateMcpTool,
            ]
        );
        let issues = parse(&v).unwrap_err();
        assert_eq!(issues[0].path, "contributes.hooks[1]");
    }

    #[test]
    fn settings_defaults_and_values() {
        let m = parse(&example()).unwrap();
        let defaults = m.default_settings();
        assert_eq!(
            defaults,
            BTreeMap::from([
                ("days".to_owned(), json!(1)),
                ("list".to_owned(), json!("a")),
                ("notify".to_owned(), json!(false)),
                ("prefix".to_owned(), json!("")),
            ])
        );
        assert!(m.invalid_settings(&defaults).is_empty());
        let bad = BTreeMap::from([
            ("days".to_owned(), json!(31)),
            ("list".to_owned(), json!("c")),
            ("notify".to_owned(), json!("yes")),
            ("prefix".to_owned(), json!("x".repeat(21))),
            ("unknown".to_owned(), json!(1)),
        ]);
        assert_eq!(
            m.invalid_settings(&bad),
            ["days", "list", "notify", "prefix", "unknown"]
        );
        assert!(
            m.invalid_settings(&BTreeMap::from([("days".to_owned(), json!(30))]))
                .is_empty()
        );
    }

    #[test]
    fn helpers() {
        assert_eq!(text_key("%a-b%"), Some("a-b"));
        assert_eq!(text_key("%Not an id%"), None);
        assert_eq!(text_key("50%"), None);
        for ok in ["index.js", "dist/index.js", "locales/pt-BR.ftl", "a_b/c.d"] {
            assert!(is_package_path(ok), "{ok}");
        }
        for bad in ["", "/abs.js", "a//b", "../x", "./x", "a\\b", "a b", "a/./b"] {
            assert!(!is_package_path(bad), "{bad}");
        }
        for ok in ["en", "de", "pt-BR", "zh-Hant", "gsw"] {
            assert!(is_language_tag(ok), "{ok}");
        }
        for bad in ["", "EN", "english", "e", "de_DE", "de-"] {
            assert!(!is_language_tag(bad), "{bad}");
        }
        for kind in PluginManifestIssueKind::ALL {
            assert!(crate::localized::is_message_id(kind.message_id()));
        }
    }
}
