//! The plug-in preview (design.md, "Plug-ins"; plan M2d; ADR 0050).
//!
//! Plug-ins are JavaScript bundles that run inside the core, one QuickJS
//! runtime per plug-in (`nail-plugins`). They never run in the webview:
//! their UI is declared as data ([`PluginUiNode`] trees) and drawn by the
//! React renderer, so it works the same on the desktop and in the remote
//! UI and is styled by `theme.kdl` like everything else.
//!
//! This module is the contract between the plug-in host, the core, the UI
//! and the SDK (`@nail/plugin-api`):
//!
//! * [`manifest`]: `manifest.json` ([`PluginManifest`]: id, name, version,
//!   `apiVersion`, `main`, permissions, contributions — commands,
//!   placements, keys, panels, a settings schema, locales) and its
//!   validation ([`PluginManifest::parse`], [`PluginManifestIssue`]).
//! * [`permission`]: the preview's permission names ([`PluginPermission`]:
//!   `mail.read`, `mail.read.body`, `calendar.read` (M3a),
//!   `ui.dialogs`, `notifications`, `storage`, `network:<domain>`) and
//!   grants ([`PluginGrant`]).
//! * [`ui`]: the declarative component set ([`PluginUiNode`]) with its
//!   stable JSON shape and limits ([`validate_tree`]).
//! * [`runtime`]: what plug-ins exchange with the UI at run time —
//!   command contexts, dialogs and their results, notifications, the
//!   read-only mail view ([`PluginMessage`]).
//! * [`admin`]: installed plug-ins ([`PluginInfo`]), install sources and
//!   previews, logs, settings, storage limits.
//!
//! **Naming.** Everything a plug-in author writes or receives (manifest,
//! component trees, dialog specs and results, command contexts, the mail
//! view) is **camelCase** JSON, like the design's example and JavaScript
//! code. The protocol around it (requests, responses, events, admin types)
//! stays snake_case like the rest of the API. The SDK's types are generated
//! from these Rust types ([`crate::export_plugin_typescript`]).
//!
//! **Ids.** A plug-in id ([`is_plugin_id`]) and the names of its commands,
//! panels and settings ([`is_contribution_name`]) are kebab-case. In
//! `layout.kdl` a command is the action `plugin.<plugin>.<command>`
//! ([`plugin_command_id`]) and a panel the widget
//! `plugin-panel "<plugin>.<panel>"` ([`plugin_panel_id`]).

pub mod admin;
pub mod manifest;
pub mod permission;
pub mod runtime;
pub mod ui;

pub use admin::{
    PluginInfo, PluginInstallPreview, PluginLogEntry, PluginLogLevel, PluginLogSource,
    PluginPackageSource, PluginRunState, PluginStopReason, PluginStorageUsage,
};
pub use manifest::{
    PluginCommandDef, PluginContributions, PluginKeyDef, PluginManifest, PluginManifestIssue,
    PluginManifestIssueKind, PluginPanelDef, PluginPlacement, PluginSettingDef,
};
pub use permission::{PluginGrant, PluginPermission, PluginPermissionError};
pub use runtime::{
    MAX_PLUGIN_CALENDAR_DAYS, MAX_PLUGIN_CALENDAR_EVENTS, PluginAttachment, PluginCalendarEvent,
    PluginCommandContext, PluginCommandSurface, PluginDialog, PluginDialogButton,
    PluginDialogResult, PluginDialogSize, PluginDialogSpec, PluginMessage, PluginMessageBody,
    PluginNotification, PluginNotificationLevel, PluginNotificationSpec, PluginSelection,
    PluginUiTarget,
};
pub use ui::{
    PluginSelectOption, PluginTableColumn, PluginUiIssue, PluginUiIssueKind, PluginUiNode,
    validate_tree,
};

use crate::ids::PluginId;

/// The plug-in API version this core implements (the manifest's
/// `apiVersion`, a major version as a string).
pub const PLUGIN_API_VERSION: &str = "1";

/// API versions this core runs: the current major and the previous one
/// (design.md: "Nail supports the current and previous major API
/// versions"; there is no previous one yet).
pub const SUPPORTED_API_VERSIONS: &[&str] = &["1"];

/// Prefix of plug-in action ids in `layout.kdl`.
pub const PLUGIN_ACTION_PREFIX: &str = "plugin.";

/// Layout widget kind of plug-in panels (`plugin-panel "<plugin>.<panel>"`).
pub const PLUGIN_PANEL_WIDGET: &str = "plugin-panel";

/// Longest plug-in id and contribution name.
pub const MAX_NAME_LEN: usize = 64;

/// Whether `id` is a valid plug-in id: 1–64 characters, lowercase ASCII
/// letters, digits and single `-` between them, starting with a letter
/// (`follow-up`, `acme-crm`). Dots are not allowed: they separate the
/// parts of `plugin.<id>.<name>`.
pub fn is_plugin_id(id: &str) -> bool {
    is_kebab_name(id)
}

/// Whether `name` is a valid name of a command, panel or setting (same
/// rule as [`is_plugin_id`]).
pub fn is_contribution_name(name: &str) -> bool {
    is_kebab_name(name)
}

fn is_kebab_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME_LEN
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.ends_with('-')
        && !s.contains("--")
}

/// The layout action id of a plug-in command: `plugin.<plugin>.<command>`.
pub fn plugin_command_id(plugin: &PluginId, command: &str) -> String {
    format!("{PLUGIN_ACTION_PREFIX}{plugin}.{command}")
}

/// Splits `plugin.<plugin>.<command>` into its parts; `None` for anything
/// else (including invalid names).
pub fn parse_plugin_command_id(action: &str) -> Option<(PluginId, String)> {
    let rest = action.strip_prefix(PLUGIN_ACTION_PREFIX)?;
    let (plugin, command) = rest.split_once('.')?;
    (is_plugin_id(plugin) && is_contribution_name(command))
        .then(|| (PluginId::new(plugin), command.to_owned()))
}

/// The id of a plug-in panel, as named by `plugin-panel "<plugin>.<panel>"`.
pub fn plugin_panel_id(plugin: &PluginId, panel: &str) -> String {
    format!("{plugin}.{panel}")
}

/// Splits `<plugin>.<panel>` into its parts; `None` for anything else.
pub fn parse_plugin_panel_id(id: &str) -> Option<(PluginId, String)> {
    let (plugin, panel) = id.split_once('.')?;
    (is_plugin_id(plugin) && is_contribution_name(panel))
        .then(|| (PluginId::new(plugin), panel.to_owned()))
}

/// Whether `version` is a semantic version `MAJOR.MINOR.PATCH` with an
/// optional `-prerelease` and `+build` (digits without leading zeros;
/// identifiers of ASCII letters, digits and `-`).
pub fn is_plugin_version(version: &str) -> bool {
    if version.is_empty() || version.len() > 64 {
        return false;
    }
    let (rest, build) = match version.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (version, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let number_ok = |n: &&str| {
        !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit())
            && !(n.len() > 1 && n.starts_with('0'))
    };
    let idents_ok = |s: &str| {
        s.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    };
    numbers.len() == 3
        && numbers.iter().all(number_ok)
        && pre.is_none_or(idents_ok)
        && build.is_none_or(idents_ok)
}

/// Whether the core runs plug-ins written for `api_version`.
pub fn is_supported_api_version(api_version: &str) -> bool {
    SUPPORTED_API_VERSIONS.contains(&api_version)
}

/// The URI a plug-in sees for a message (`nail://messages/<id>`, the same
/// form as the MCP resources).
pub fn message_uri(id: &crate::ids::MessageId) -> String {
    format!("nail://messages/{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_ids_are_kebab_case() {
        for ok in ["follow-up", "a", "acme-crm2", "x1-y2"] {
            assert!(is_plugin_id(ok), "{ok}");
        }
        for bad in [
            "",
            "Follow",
            "1abc",
            "a--b",
            "a-",
            "-a",
            "a.b",
            "a_b",
            "ä",
            &"a".repeat(65),
        ] {
            assert!(!is_plugin_id(bad), "{bad}");
        }
        assert!(is_contribution_name("contact-sidebar"));
        assert!(!is_contribution_name("contact sidebar"));
    }

    #[test]
    fn command_and_panel_ids_round_trip() {
        let plugin = PluginId::new("follow-up");
        let action = plugin_command_id(&plugin, "remind");
        assert_eq!(action, "plugin.follow-up.remind");
        assert_eq!(
            parse_plugin_command_id(&action),
            Some((plugin.clone(), "remind".to_owned()))
        );
        assert_eq!(parse_plugin_command_id("mail.reply"), None);
        assert_eq!(parse_plugin_command_id("plugin.follow-up"), None);
        assert_eq!(parse_plugin_command_id("plugin.follow-up.a.b"), None);
        assert_eq!(parse_plugin_command_id("plugin.Bad.x"), None);

        let panel = plugin_panel_id(&plugin, "sidebar");
        assert_eq!(panel, "follow-up.sidebar");
        assert_eq!(
            parse_plugin_panel_id(&panel),
            Some((plugin, "sidebar".to_owned()))
        );
        assert_eq!(parse_plugin_panel_id("follow-up"), None);
        assert_eq!(parse_plugin_panel_id("follow-up.a.b"), None);
    }

    #[test]
    fn versions_are_semver() {
        for ok in [
            "1.0.0",
            "0.1.12",
            "1.0.0-beta.1",
            "2.3.4+build-5",
            "1.0.0-rc-1+x",
        ] {
            assert!(is_plugin_version(ok), "{ok}");
        }
        for bad in [
            "",
            "1",
            "1.0",
            "1.0.0.0",
            "01.0.0",
            "1.0.x",
            "1.0.0-",
            "1.0.0-a..b",
            "v1.0.0",
        ] {
            assert!(!is_plugin_version(bad), "{bad}");
        }
    }

    #[test]
    fn api_versions() {
        assert!(is_supported_api_version(PLUGIN_API_VERSION));
        assert!(!is_supported_api_version("2"));
        assert!(!is_supported_api_version("1.0"));
    }

    #[test]
    fn message_uris_match_mcp() {
        assert_eq!(
            message_uri(&crate::ids::MessageId::from("m1")),
            "nail://messages/m1"
        );
    }
}
