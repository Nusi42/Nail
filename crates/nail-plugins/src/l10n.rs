//! Texts: the Fluent ids of the host's own messages
//! (`locales/*/core-plugins.ftl`) and the plug-in's own catalogs, which
//! resolve `%fluent-id%` texts of its manifest
//! ([`nail_api::plugins::manifest::text_key`]).

use std::collections::BTreeMap;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use nail_api::plugins::extensions::MAX_MCP_TOOL_DESCRIPTION;
use nail_api::plugins::manifest::text_key;
use nail_api::plugins::{PluginManifest, PluginSettingDef};
use nail_api::{LocalizedMessage, MessageArg};
use unic_langid::LanguageIdentifier;

use crate::package::PackageFiles;

/// The plug-in started (`$plugin`, `$version`).
pub const STARTED: &str = "plugin-host-started";
/// It could not start (`$plugin`, `$detail`).
pub const START_FAILED: &str = "plugin-host-start-failed";
/// A dev plug-in's files changed and it is reloaded (`$plugin`).
pub const RELOADING: &str = "plugin-host-reloading";
/// A dev reload found an invalid manifest (`$plugin`); the issues follow
/// as their own lines.
pub const RELOAD_INVALID: &str = "plugin-host-reload-invalid";
/// An API call without its permission (`$call`, `$permission`).
pub const API_DENIED: &str = "plugin-host-api-denied";
/// A rejected promise nobody handled (`$detail`).
pub const UNHANDLED_REJECTION: &str = "plugin-host-unhandled-rejection";
/// Log lines dropped because a call wrote too many (`$count`).
pub const LOG_DROPPED: &str = "plugin-host-log-dropped";
/// A hook threw or answered something invalid and was skipped (`$plugin`,
/// `$hook` = the hook's name, `$detail`).
pub const HOOK_FAILED: &str = "plugin-host-hook-failed";
/// A hook ran out of its time budget and was skipped (`$plugin`, `$hook`,
/// `$ms`).
pub const HOOK_TIMED_OUT: &str = "plugin-host-hook-timed-out";
/// Arrival hook actions were dropped: the plug-in may not organize mail in
/// that account (`$plugin`).
pub const HOOK_ACTIONS_DENIED: &str = "plugin-host-hook-actions-denied";
/// A job was skipped because its previous run had not finished (`$plugin`,
/// `$job`).
pub const JOB_OVERLAP: &str = "plugin-host-job-overlap";
/// A job was skipped because the plug-in used its CPU time for this hour
/// (`$plugin`, `$job`).
pub const JOB_CPU_QUOTA: &str = "plugin-host-job-cpu-quota";
/// A job failed (`$job`, `$detail`).
pub const JOB_FAILED: &str = "plugin-host-job-failed";
/// A job ran out of wall-clock time (`$job`, `$detail`).
pub const JOB_TIMED_OUT: &str = "plugin-host-job-timed-out";
/// An extension handler (composer / attachment / link action, preview,
/// Markdown block, MCP tool, banner button) failed (`$name`, `$detail`).
pub const HANDLER_FAILED: &str = "plugin-host-handler-failed";
/// The archive is larger than allowed.
pub const PACKAGE_TOO_LARGE: &str = "plugin-package-too-large";
/// Not a zip archive (`$detail`).
pub const PACKAGE_NOT_A_ZIP: &str = "plugin-package-not-a-zip";
/// Too many files.
pub const PACKAGE_TOO_MANY_FILES: &str = "plugin-package-too-many-files";
/// A path outside the package or with unsafe characters (`$path`).
pub const PACKAGE_UNSAFE_PATH: &str = "plugin-package-unsafe-path";
/// Two entries with the same path (ignoring case) (`$path`).
pub const PACKAGE_DUPLICATE_PATH: &str = "plugin-package-duplicate-path";
/// A symbolic link (`$path`).
pub const PACKAGE_LINK: &str = "plugin-package-link";
/// An encrypted entry (`$path`).
pub const PACKAGE_ENCRYPTED: &str = "plugin-package-encrypted";
/// A compression method other than stored / deflate (`$path`).
pub const PACKAGE_COMPRESSION: &str = "plugin-package-compression";
/// No `manifest.json` at the top.
pub const PACKAGE_NO_MANIFEST: &str = "plugin-package-no-manifest";
/// The archive or a folder cannot be read (`$detail`).
pub const PACKAGE_UNREADABLE: &str = "plugin-package-unreadable";
/// The installed package differs from the one that was approved.
pub const PACKAGE_HASH_MISMATCH: &str = "plugin-package-hash-mismatch";
/// A file the manifest names is missing (`$path`).
pub const PACKAGE_MISSING_FILE: &str = "plugin-package-missing-file";

/// Every id of this crate in `core-plugins.ftl` (the catalog test checks
/// each has an English text).
pub const ALL: &[&str] = &[
    STARTED,
    START_FAILED,
    RELOADING,
    RELOAD_INVALID,
    API_DENIED,
    UNHANDLED_REJECTION,
    LOG_DROPPED,
    HOOK_FAILED,
    HOOK_TIMED_OUT,
    HOOK_ACTIONS_DENIED,
    JOB_OVERLAP,
    JOB_CPU_QUOTA,
    JOB_FAILED,
    JOB_TIMED_OUT,
    HANDLER_FAILED,
    PACKAGE_TOO_LARGE,
    PACKAGE_NOT_A_ZIP,
    PACKAGE_TOO_MANY_FILES,
    PACKAGE_UNSAFE_PATH,
    PACKAGE_DUPLICATE_PATH,
    PACKAGE_LINK,
    PACKAGE_ENCRYPTED,
    PACKAGE_COMPRESSION,
    PACKAGE_NO_MANIFEST,
    PACKAGE_UNREADABLE,
    PACKAGE_HASH_MISMATCH,
    PACKAGE_MISSING_FILE,
];

/// Largest catalog file of a plug-in (bytes).
pub const MAX_CATALOG_BYTES: usize = 256 * 1024;

type Bundle = FluentBundle<FluentResource>;

/// A plug-in's own catalogs (`contributes.locales`), by language tag.
pub struct PluginCatalogs {
    bundles: BTreeMap<String, Bundle>,
}

impl std::fmt::Debug for PluginCatalogs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginCatalogs")
            .field("languages", &self.bundles.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl PluginCatalogs {
    /// Loads the catalogs the manifest names. Missing, oversized or
    /// unparsable files are skipped (texts then fall back to English or
    /// the id); what parsed of a file with syntax errors is kept.
    pub fn load(manifest: &PluginManifest, files: &PackageFiles) -> Self {
        let mut bundles = BTreeMap::new();
        for (tag, path) in &manifest.contributes.locales {
            let Ok(bytes) = files.read(path) else {
                continue;
            };
            if bytes.len() > MAX_CATALOG_BYTES {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            if let Some(bundle) = make_bundle(tag, text) {
                bundles.insert(tag.clone(), bundle);
            }
        }
        Self { bundles }
    }

    /// Catalogs from `(tag, source)` pairs (tests, the SDK's checks).
    pub fn from_sources<'a>(sources: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let bundles = sources
            .into_iter()
            .filter_map(|(tag, text)| Some((tag.to_owned(), make_bundle(tag, text.to_owned())?)))
            .collect();
        Self { bundles }
    }

    /// The languages to try for `language`: the tag, its primary language,
    /// then English.
    fn chain(&self, language: Option<&str>) -> Vec<&Bundle> {
        let mut tags: Vec<String> = Vec::new();
        if let Some(tag) = language {
            let tag = tag.replace('_', "-");
            let tag = tag.split('.').next().unwrap_or_default().to_owned();
            if let Some((primary, _)) = tag.split_once('-') {
                tags.push(tag.clone());
                tags.push(primary.to_ascii_lowercase());
            } else {
                tags.push(tag.to_ascii_lowercase());
            }
        }
        tags.push("en".to_owned());
        let mut out: Vec<&Bundle> = Vec::new();
        for tag in tags {
            if let Some(found) = self
                .bundles
                .iter()
                .find(|(t, _)| t.eq_ignore_ascii_case(&tag))
                .map(|(_, b)| b)
                && !out.iter().any(|b| std::ptr::eq(*b, found))
            {
                out.push(found);
            }
        }
        out
    }

    /// A message of the plug-in's catalogs for `language` (then English,
    /// then the id itself).
    pub fn message(&self, language: Option<&str>, message: &LocalizedMessage) -> String {
        let mut args = FluentArgs::new();
        for (name, value) in &message.args {
            match value {
                MessageArg::Number(n) => args.set(name.as_str(), FluentValue::from(*n)),
                other => args.set(name.as_str(), FluentValue::from(other.to_string())),
            }
        }
        for bundle in self.chain(language) {
            if let Some(pattern) = bundle.get_message(&message.id).and_then(|m| m.value()) {
                let mut errors = Vec::new();
                return bundle
                    .format_pattern(pattern, Some(&args), &mut errors)
                    .into_owned();
            }
        }
        message.id.clone()
    }

    /// Whether a message exists for `language` or in English
    /// (`nail.l10n.has`).
    pub fn has(&self, language: Option<&str>, id: &str) -> bool {
        self.chain(language)
            .iter()
            .any(|bundle| bundle.get_message(id).and_then(|m| m.value()).is_some())
    }

    /// Resolves one manifest text: `%id%` is looked up, anything else is
    /// returned as written.
    pub fn text(&self, language: Option<&str>, text: &str) -> String {
        match text_key(text) {
            Some(key) => self.message(language, &LocalizedMessage::new(key)),
            None => text.to_owned(),
        }
    }

    /// An MCP tool's description (`%id%` resolved), cut to
    /// [`nail_api::plugins::extensions::MAX_MCP_TOOL_DESCRIPTION`]
    /// characters like the manifest's own.
    pub fn mcp_description(&self, language: Option<&str>, description: &str) -> String {
        let text = self.text(language, description);
        if text.chars().count() <= MAX_MCP_TOOL_DESCRIPTION {
            return text;
        }
        text.chars().take(MAX_MCP_TOOL_DESCRIPTION).collect()
    }

    /// The manifest with every user-visible text resolved for `language`:
    /// name, description, command / panel titles and descriptions,
    /// setting titles, descriptions and option labels, and (M4a) the
    /// titles of composer actions, Markdown blocks (and their templates),
    /// attachment previews, attachment and link actions and jobs,
    /// composer action tooltips and MCP tool descriptions.
    pub fn localize_manifest(
        &self,
        manifest: &PluginManifest,
        language: Option<&str>,
    ) -> PluginManifest {
        let t = |s: &str| self.text(language, s);
        let mut m = manifest.clone();
        m.name = t(&m.name);
        m.description = m.description.as_deref().map(t);
        for c in &mut m.contributes.commands {
            c.title = t(&c.title);
            c.description = c.description.as_deref().map(t);
        }
        for p in &mut m.contributes.panels {
            p.title = t(&p.title);
        }
        let c = &mut m.contributes;
        for a in &mut c.composer_actions {
            a.title = t(&a.title);
            a.description = a.description.as_deref().map(t);
        }
        for b in &mut c.markdown_blocks {
            b.title = t(&b.title);
            b.template = b.template.as_deref().map(t);
        }
        for p in &mut c.attachment_previews {
            p.title = t(&p.title);
        }
        for a in &mut c.attachment_actions {
            a.title = t(&a.title);
        }
        for a in &mut c.link_actions {
            a.title = t(&a.title);
        }
        for j in &mut c.jobs {
            j.title = j.title.as_deref().map(t);
        }
        for tool in &mut c.mcp_tools {
            tool.description = self.mcp_description(language, &tool.description);
        }
        for s in &mut m.contributes.settings {
            match s {
                PluginSettingDef::String {
                    title, description, ..
                }
                | PluginSettingDef::Number {
                    title, description, ..
                }
                | PluginSettingDef::Boolean {
                    title, description, ..
                } => {
                    *title = t(title);
                    *description = description.as_deref().map(t);
                }
                PluginSettingDef::Select {
                    title,
                    description,
                    options,
                    ..
                } => {
                    *title = t(title);
                    *description = description.as_deref().map(t);
                    for o in options {
                        o.label = t(&o.label);
                    }
                }
            }
        }
        m
    }
}

fn make_bundle(tag: &str, text: String) -> Option<Bundle> {
    let langid: LanguageIdentifier = tag.parse().ok()?;
    let resource = match FluentResource::try_new(text) {
        Ok(r) => r,
        Err((r, _errors)) => r,
    };
    let mut bundle = Bundle::new_concurrent(vec![langid]);
    bundle.set_use_isolating(false);
    // Duplicate ids keep the first definition.
    let _ = bundle.add_resource(resource);
    Some(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::localized::is_message_id;

    #[test]
    fn manifest_texts_resolve_with_fallback() {
        let manifest = PluginManifest::parse(
            br#"{"id":"x","name":"%plugin-name%","version":"1.0.0","apiVersion":"1","main":"index.js",
                "description":"Plain text",
                "contributes":{
                  "commands":[{"id":"go","title":"%go-title%","description":"%only-en%","contexts":["list"]}],
                  "panels":[{"id":"side","title":"%missing-everywhere%"}],
                  "settings":[{"type":"select","key":"mode","title":"%mode%","options":[{"value":"a","label":"%opt-a%"}]}],
                  "locales":{"en":"locales/en.ftl","de":"locales/de.ftl"}}}"#,
        )
        .unwrap();
        let catalogs = PluginCatalogs::from_sources([
            (
                "en",
                "plugin-name = Follow up\ngo-title = Go\nonly-en = English only\nmode = Mode\nopt-a = Option A\n",
            ),
            ("de", "plugin-name = Nachfassen\ngo-title = Los\n"),
        ]);
        let de = catalogs.localize_manifest(&manifest, Some("de-AT"));
        assert_eq!(de.name, "Nachfassen");
        assert_eq!(de.description.as_deref(), Some("Plain text"));
        assert_eq!(de.contributes.commands[0].title, "Los");
        assert_eq!(
            de.contributes.commands[0].description.as_deref(),
            Some("English only")
        );
        assert_eq!(de.contributes.panels[0].title, "missing-everywhere");
        let PluginSettingDef::Select { title, options, .. } = &de.contributes.settings[0] else {
            panic!()
        };
        assert_eq!(title, "Mode");
        assert_eq!(options[0].label, "Option A");

        let en = catalogs.localize_manifest(&manifest, None);
        assert_eq!(en.name, "Follow up");
        let fr = catalogs.localize_manifest(&manifest, Some("fr_FR.UTF-8"));
        assert_eq!(fr.contributes.commands[0].title, "Go");
    }

    /// A manifest with only `contributes` (not validated: these tests are
    /// about texts).
    fn with_contributes(contributes: serde_json::Value) -> PluginManifest {
        serde_json::from_value(serde_json::json!({
            "id": "x", "name": "X", "version": "1.0.0", "apiVersion": "1", "main": "index.js",
            "contributes": contributes,
        }))
        .unwrap()
    }

    fn m4a_catalogs() -> PluginCatalogs {
        PluginCatalogs::from_sources([
            (
                "en",
                "t-title = Title\nt-tip = Tooltip\nt-template = Template body\n\
                 t-tool = Looks things up\n",
            ),
            ("de", "t-title = Titel\nt-tip = Hinweis\n"),
        ])
    }

    #[test]
    fn composer_action_texts_resolve() {
        let m = with_contributes(serde_json::json!({"composerActions": [
            {"id": "a", "title": "%t-title%", "description": "%t-tip%"},
            {"id": "b", "title": "Plain"}]}));
        let de = m4a_catalogs().localize_manifest(&m, Some("de"));
        let actions = &de.contributes.composer_actions;
        assert_eq!(actions[0].title, "Titel");
        assert_eq!(actions[0].description.as_deref(), Some("Hinweis"));
        assert_eq!(actions[1].title, "Plain");
        assert_eq!(actions[1].description, None);
    }

    #[test]
    fn markdown_block_texts_resolve() {
        let m = with_contributes(serde_json::json!({"markdownBlocks": [
            {"id": "chart", "title": "%t-title%", "template": "%t-template%"},
            {"id": "raw", "title": "Raw", "template": "a,b\n1,2"}]}));
        let de = m4a_catalogs().localize_manifest(&m, Some("de-CH"));
        let blocks = &de.contributes.markdown_blocks;
        assert_eq!(blocks[0].title, "Titel");
        // Not in German: English.
        assert_eq!(blocks[0].template.as_deref(), Some("Template body"));
        assert_eq!(blocks[1].template.as_deref(), Some("a,b\n1,2"));
    }

    #[test]
    fn attachment_preview_title_resolves() {
        let m = with_contributes(serde_json::json!({"attachmentPreviews": [
            {"id": "table", "title": "%t-title%", "extensions": ["csv"]}]}));
        let en = m4a_catalogs().localize_manifest(&m, None);
        assert_eq!(en.contributes.attachment_previews[0].title, "Title");
        // Matching data is untouched.
        assert_eq!(en.contributes.attachment_previews[0].extensions, ["csv"]);
    }

    #[test]
    fn attachment_action_title_resolves() {
        let m = with_contributes(serde_json::json!({"attachmentActions": [
            {"id": "save", "title": "%t-title%"}]}));
        let de = m4a_catalogs().localize_manifest(&m, Some("de"));
        assert_eq!(de.contributes.attachment_actions[0].title, "Titel");
    }

    #[test]
    fn link_action_title_resolves() {
        let m = with_contributes(serde_json::json!({"linkActions": [
            {"id": "check", "title": "%t-title%", "hosts": ["*.example.com"]}]}));
        let de = m4a_catalogs().localize_manifest(&m, Some("de"));
        assert_eq!(de.contributes.link_actions[0].title, "Titel");
        assert_eq!(de.contributes.link_actions[0].hosts, ["*.example.com"]);
    }

    #[test]
    fn job_title_resolves() {
        let m = with_contributes(serde_json::json!({"jobs": [
            {"id": "tick", "title": "%t-title%", "schedule": "@hourly"},
            {"id": "quiet", "schedule": "@daily"}]}));
        let de = m4a_catalogs().localize_manifest(&m, Some("de"));
        assert_eq!(de.contributes.jobs[0].title.as_deref(), Some("Titel"));
        assert_eq!(de.contributes.jobs[0].schedule.as_deref(), Some("@hourly"));
        assert_eq!(de.contributes.jobs[1].title, None);
    }

    #[test]
    fn mcp_tool_description_resolves_and_is_bounded() {
        let m = with_contributes(serde_json::json!({"mcpTools": [
            {"id": "lookup", "description": "%t-tool%"},
            {"id": "long", "description": "%t-long%"}]}));
        let long = "x".repeat(MAX_MCP_TOOL_DESCRIPTION + 50);
        let source = format!("t-tool = Looks things up\nt-long = {long}\n");
        let catalogs = PluginCatalogs::from_sources([("en", source.as_str())]);
        let en = catalogs.localize_manifest(&m, None);
        assert_eq!(en.contributes.mcp_tools[0].description, "Looks things up");
        assert_eq!(
            en.contributes.mcp_tools[1].description.chars().count(),
            MAX_MCP_TOOL_DESCRIPTION
        );
        assert_eq!(catalogs.mcp_description(None, "Plain words"), "Plain words");
    }

    /// The examples' M4a texts resolve (the CSV preview's renderer title,
    /// the follow-up reminder job).
    #[test]
    fn example_contribution_texts_resolve() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let load = |name: &str| {
            let dir = root.join(name);
            let manifest =
                PluginManifest::parse(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
            let en = std::fs::read_to_string(dir.join("locales/en.ftl")).unwrap();
            let catalogs = PluginCatalogs::from_sources([("en", en.as_str())]);
            catalogs.localize_manifest(&manifest, Some("de"))
        };
        let csv = load("csv-preview");
        assert_eq!(csv.contributes.attachment_previews[0].title, "Table");
        let follow_up = load("follow-up");
        assert_eq!(
            follow_up.contributes.jobs[0].title.as_deref(),
            Some("Due follow-up reminders")
        );
        for manifest in [&csv, &follow_up] {
            let text = serde_json::to_string(manifest).unwrap();
            assert!(
                !text.contains("\"%"),
                "unresolved manifest text in {}: {text}",
                manifest.id
            );
        }
    }

    #[test]
    fn messages_take_arguments() {
        let catalogs = PluginCatalogs::from_sources([("en", "due = Due in { $days } days\n")]);
        let text = catalogs.message(None, &LocalizedMessage::new("due").arg("days", 3));
        assert_eq!(text, "Due in 3 days");
        // Broken files keep what parsed.
        let broken = PluginCatalogs::from_sources([("en", "ok = fine\n= broken\n")]);
        assert_eq!(broken.text(None, "%ok%"), "fine");
    }

    /// Every id has an English text, and every host id in the catalog is
    /// used (the contract's own ids are checked by nail-api).
    #[test]
    fn catalog_has_every_id() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        let en = std::fs::read_to_string(root.join("en/core-plugins.ftl")).unwrap();
        let defined: Vec<&str> = en
            .lines()
            .filter_map(|l| l.split_once(" =").map(|(id, _)| id))
            .filter(|id| is_message_id(id))
            .collect();
        for id in ALL {
            assert!(
                defined.contains(id),
                "{id} missing in locales/en/core-plugins.ftl"
            );
        }
        for id in &defined {
            if id.starts_with("plugin-host-") || id.starts_with("plugin-package-") {
                assert!(ALL.contains(id), "{id} in core-plugins.ftl is not used");
            }
        }
        assert!(root.join("de/core-plugins.ftl").exists());
    }
}
