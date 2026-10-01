//! Rendering [`LocalizedMessage`]s in Rust (M2c WP53; contract ADR 0043).
//!
//! The UI renders localized core messages itself. Rust renders them only
//! where Rust shows text: user-facing CLI output (`nail mcp`, `nail
//! serve`), OS notifications of the desktop shell, and the English
//! diagnostic that accompanies every message (logs, `message` fields).
//!
//! [`Catalog`] is a `fluent-bundle` over the same `locales/<lang>/*.ftl`
//! catalogs the UI loads, embedded at build time ([`EN_FILES`],
//! [`DE_FILES`]). Lookups fall back per key: the requested language first,
//! then English, then the message id itself. [`english`] renders with the
//! English catalog (the diagnostics), [`Catalog::for_locale`] with a
//! requested language.
//!
//! Configuration errors ([`crate::ConfigError`]), layout edit errors
//! ([`crate::EditError`]), merge conflicts and backup errors are built as
//! [`LocalizedMessage`]s here (ids `config-*` in `locales/en/core.ftl`);
//! their English `message` is the English rendering.

use std::sync::OnceLock;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use nail_api::localized::{LocalizedMessage, MessageArg};
use unic_langid::LanguageIdentifier;

/// The English catalogs (complete), by file name.
pub const EN_FILES: &[(&str, &str)] = &[
    (
        "accounts.ftl",
        include_str!("../../../locales/en/accounts.ftl"),
    ),
    ("app.ftl", include_str!("../../../locales/en/app.ftl")),
    (
        "compose.ftl",
        include_str!("../../../locales/en/compose.ftl"),
    ),
    (
        "composer.ftl",
        include_str!("../../../locales/en/composer.ftl"),
    ),
    (
        "core-crypto.ftl",
        include_str!("../../../locales/en/core-crypto.ftl"),
    ),
    (
        "core-dav.ftl",
        include_str!("../../../locales/en/core-dav.ftl"),
    ),
    (
        "core-gmail.ftl",
        include_str!("../../../locales/en/core-gmail.ftl"),
    ),
    (
        "core-ical.ftl",
        include_str!("../../../locales/en/core-ical.ftl"),
    ),
    (
        "core-pim.ftl",
        include_str!("../../../locales/en/core-pim.ftl"),
    ),
    (
        "core-plugins.ftl",
        include_str!("../../../locales/en/core-plugins.ftl"),
    ),
    (
        "core-scheduling.ftl",
        include_str!("../../../locales/en/core-scheduling.ftl"),
    ),
    (
        "core-sieve.ftl",
        include_str!("../../../locales/en/core-sieve.ftl"),
    ),
    (
        "core-triage.ftl",
        include_str!("../../../locales/en/core-triage.ftl"),
    ),
    ("core.ftl", include_str!("../../../locales/en/core.ftl")),
    (
        "customization.ftl",
        include_str!("../../../locales/en/customization.ftl"),
    ),
    ("gmail.ftl", include_str!("../../../locales/en/gmail.ftl")),
    ("mcp.ftl", include_str!("../../../locales/en/mcp.ftl")),
    ("menus.ftl", include_str!("../../../locales/en/menus.ftl")),
    ("pim.ftl", include_str!("../../../locales/en/pim.ftl")),
    (
        "plugins.ftl",
        include_str!("../../../locales/en/plugins.ftl"),
    ),
    ("remote.ftl", include_str!("../../../locales/en/remote.ftl")),
    ("rules.ftl", include_str!("../../../locales/en/rules.ftl")),
    (
        "scheduling.ftl",
        include_str!("../../../locales/en/scheduling.ftl"),
    ),
    (
        "security.ftl",
        include_str!("../../../locales/en/security.ftl"),
    ),
    ("sieve.ftl", include_str!("../../../locales/en/sieve.ftl")),
    ("sync.ftl", include_str!("../../../locales/en/sync.ftl")),
    ("triage.ftl", include_str!("../../../locales/en/triage.ftl")),
];

/// The German catalogs (may be empty; English fills the gaps per key).
pub const DE_FILES: &[(&str, &str)] = &[
    (
        "accounts.ftl",
        include_str!("../../../locales/de/accounts.ftl"),
    ),
    ("app.ftl", include_str!("../../../locales/de/app.ftl")),
    (
        "compose.ftl",
        include_str!("../../../locales/de/compose.ftl"),
    ),
    (
        "composer.ftl",
        include_str!("../../../locales/de/composer.ftl"),
    ),
    (
        "core-crypto.ftl",
        include_str!("../../../locales/de/core-crypto.ftl"),
    ),
    (
        "core-dav.ftl",
        include_str!("../../../locales/de/core-dav.ftl"),
    ),
    (
        "core-gmail.ftl",
        include_str!("../../../locales/de/core-gmail.ftl"),
    ),
    (
        "core-ical.ftl",
        include_str!("../../../locales/de/core-ical.ftl"),
    ),
    (
        "core-pim.ftl",
        include_str!("../../../locales/de/core-pim.ftl"),
    ),
    (
        "core-plugins.ftl",
        include_str!("../../../locales/de/core-plugins.ftl"),
    ),
    (
        "core-scheduling.ftl",
        include_str!("../../../locales/de/core-scheduling.ftl"),
    ),
    (
        "core-sieve.ftl",
        include_str!("../../../locales/de/core-sieve.ftl"),
    ),
    (
        "core-triage.ftl",
        include_str!("../../../locales/de/core-triage.ftl"),
    ),
    ("core.ftl", include_str!("../../../locales/de/core.ftl")),
    (
        "customization.ftl",
        include_str!("../../../locales/de/customization.ftl"),
    ),
    ("gmail.ftl", include_str!("../../../locales/de/gmail.ftl")),
    ("mcp.ftl", include_str!("../../../locales/de/mcp.ftl")),
    ("menus.ftl", include_str!("../../../locales/de/menus.ftl")),
    ("pim.ftl", include_str!("../../../locales/de/pim.ftl")),
    (
        "plugins.ftl",
        include_str!("../../../locales/de/plugins.ftl"),
    ),
    ("remote.ftl", include_str!("../../../locales/de/remote.ftl")),
    ("rules.ftl", include_str!("../../../locales/de/rules.ftl")),
    (
        "scheduling.ftl",
        include_str!("../../../locales/de/scheduling.ftl"),
    ),
    (
        "security.ftl",
        include_str!("../../../locales/de/security.ftl"),
    ),
    ("sieve.ftl", include_str!("../../../locales/de/sieve.ftl")),
    ("sync.ftl", include_str!("../../../locales/de/sync.ftl")),
    ("triage.ftl", include_str!("../../../locales/de/triage.ftl")),
];

/// The languages with embedded catalogs, English first.
pub const LANGUAGES: &[&str] = &["en", "de"];

/// A message without arguments (shorthand for [`LocalizedMessage::new`]).
pub fn msg(id: &str) -> LocalizedMessage {
    LocalizedMessage::new(id)
}

type Bundle = FluentBundle<FluentResource>;

fn bundle(language: &str, files: &[(&str, &str)]) -> Bundle {
    let langid: LanguageIdentifier = language.parse().unwrap_or_default();
    let mut bundle = Bundle::new_concurrent(vec![langid]);
    bundle.set_use_isolating(false);
    for (name, text) in files {
        let resource = match FluentResource::try_new((*text).to_owned()) {
            Ok(resource) => resource,
            Err((resource, errors)) => {
                // The UI and the catalog tests report syntax errors; keep
                // what parsed.
                tracing::warn!(file = name, language, ?errors, "Fluent syntax errors");
                resource
            }
        };
        // A duplicate id across files keeps the first definition.
        let _ = bundle.add_resource(resource);
    }
    bundle
}

/// Localized texts: a language's catalogs with the English fallback.
pub struct Catalog {
    /// Requested language first (absent for English), then English.
    bundles: Vec<&'static Bundle>,
    language: &'static str,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("language", &self.language)
            .finish()
    }
}

fn english_bundle() -> &'static Bundle {
    static EN: OnceLock<Bundle> = OnceLock::new();
    EN.get_or_init(|| bundle("en", EN_FILES))
}

fn german_bundle() -> &'static Bundle {
    static DE: OnceLock<Bundle> = OnceLock::new();
    DE.get_or_init(|| bundle("de", DE_FILES))
}

impl Catalog {
    /// The English catalog.
    pub fn english() -> Self {
        Self {
            bundles: vec![english_bundle()],
            language: "en",
        }
    }

    /// The catalog for a BCP 47 tag (`de-AT`, `de_DE.UTF-8`, …): its
    /// language when Nail ships it, else English.
    pub fn for_locale(tag: Option<&str>) -> Self {
        match language_of(tag) {
            Some("de") => Self {
                bundles: vec![german_bundle(), english_bundle()],
                language: "de",
            },
            _ => Self::english(),
        }
    }

    /// The language this catalog renders (`"en"`, `"de"`).
    pub fn language(&self) -> &'static str {
        self.language
    }

    /// Whether the English catalog (or the requested language) defines `id`.
    pub fn has(&self, id: &str) -> bool {
        self.bundles.iter().any(|b| b.has_message(id))
    }

    /// Renders a message: the first catalog that defines its id wins; an
    /// id nobody defines renders as the id itself (like the UI).
    pub fn render(&self, message: &LocalizedMessage) -> String {
        let args = fluent_args(message);
        for bundle in &self.bundles {
            let Some(pattern) = bundle.get_message(&message.id).and_then(|m| m.value()) else {
                continue;
            };
            let mut errors = Vec::new();
            let text = bundle.format_pattern(pattern, Some(&args), &mut errors);
            if !errors.is_empty() {
                tracing::debug!(id = %message.id, ?errors, "Fluent formatting errors");
            }
            return text.into_owned();
        }
        message.id.clone()
    }

    /// Renders an id with `(name, value)` arguments (the core's
    /// `strings::message_with` shape).
    pub fn text(&self, id: &str, args: &[(&str, &str)]) -> String {
        let mut message = LocalizedMessage {
            id: id.to_owned(),
            args: Default::default(),
        };
        for (name, value) in args {
            message
                .args
                .insert((*name).to_owned(), MessageArg::Text((*value).to_owned()));
        }
        self.render(&message)
    }
}

fn fluent_args(message: &LocalizedMessage) -> FluentArgs<'_> {
    let mut args = FluentArgs::new();
    for (name, value) in &message.args {
        let value = match value {
            MessageArg::Number(n) => FluentValue::from(*n),
            MessageArg::Text(t) => FluentValue::from(t.as_str()),
        };
        args.set(name.as_str(), value);
    }
    args
}

/// The English text of a message (diagnostics, logs).
pub fn english(message: &LocalizedMessage) -> String {
    Catalog::english().render(message)
}

/// The language part of a locale tag, lowercased, when Nail ships a
/// catalog for it (`"de-AT"` → `"de"`, `"C"` → `None`).
pub fn language_of(tag: Option<&str>) -> Option<&'static str> {
    let language = tag?
        .split(['-', '_', '.', '@'])
        .next()?
        .to_ascii_lowercase();
    LANGUAGES.iter().copied().find(|l| *l == language)
}

/// The user's language for command-line output: `NAIL_LOCALE`, then the
/// POSIX variables (`LC_ALL`, `LC_MESSAGES`, `LANG`). `None` = English.
pub fn locale_from_env() -> Option<String> {
    locale_from(|name| std::env::var(name).ok())
}

/// [`locale_from_env`] over any variable lookup (tests).
pub fn locale_from(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    ["NAIL_LOCALE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(var)
        .map(|v| v.trim().to_owned())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
}

/// Test support: message ids with one of `prefixes` that the `.rs` files
/// under `dir` spell out as string literals (`"config-…"`) but the English
/// catalogs lack, as `(file, id)`. Producers of localized messages call it
/// from a test so that every id they build has an English text.
#[doc(hidden)]
pub fn missing_message_ids(dir: &std::path::Path, prefixes: &[&str]) -> Vec<(String, String)> {
    let english = Catalog::english();
    let mut missing = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for literal in text.split('"').skip(1).step_by(2) {
                let wanted = prefixes.iter().any(|p| literal.starts_with(p))
                    && nail_api::localized::is_message_id(literal);
                if wanted && !english.has(literal) {
                    missing.push((path.display().to_string(), literal.to_owned()));
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn locales() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales")
    }

    fn dir_files(lang: &str) -> BTreeSet<String> {
        std::fs::read_dir(locales().join(lang))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".ftl"))
            .collect()
    }

    /// The embedded lists name every catalog file (a new file needs a line
    /// in [`EN_FILES`] / [`DE_FILES`]).
    #[test]
    fn every_catalog_file_is_embedded() {
        for (lang, files) in [("en", EN_FILES), ("de", DE_FILES)] {
            let embedded: BTreeSet<String> = files.iter().map(|(n, _)| (*n).to_owned()).collect();
            assert_eq!(embedded, dir_files(lang), "{lang}");
        }
    }

    /// Every `config-*` id built in this crate has an English text (widget
    /// kinds such as `config-errors` are not messages).
    #[test]
    fn every_config_message_id_is_in_the_catalog() {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let missing: Vec<_> = missing_message_ids(&src, &["config-"])
            .into_iter()
            .filter(|(file, _)| !file.contains("widgets"))
            .collect();
        assert!(missing.is_empty(), "{missing:#?}");
    }

    /// Every embedded English catalog parses without Fluent syntax errors.
    #[test]
    fn english_catalogs_parse() {
        for (name, text) in EN_FILES {
            if let Err((_, errors)) = FluentResource::try_new((*text).to_owned()) {
                panic!("{name}: {errors:?}");
            }
        }
    }

    #[test]
    fn renders_with_arguments_and_plurals() {
        let en = Catalog::english();
        assert!(en.has("error-not-found"));
        assert!(!en.render(&msg("error-not-found")).is_empty());
        let text = en.render(
            &msg("config-unknown-property-hint")
                .arg("key", "colr")
                .arg("node", "part")
                .arg("allowed", "color, size"),
        );
        assert_eq!(
            text,
            "unknown property `colr` on `part` (allowed: color, size)"
        );
        let one = en.render(&msg("mcp-core-refused-more").arg("count", 1));
        let many = en.render(&msg("mcp-core-refused-more").arg("count", 3));
        assert_ne!(one, many);
        assert!(many.contains('3'), "{many}");
        // Unknown ids render as themselves.
        assert_eq!(en.render(&msg("no-such-message")), "no-such-message");
        assert_eq!(english(&msg("no-such-message")), "no-such-message");
    }

    /// German falls back to English per key (the German catalogs are
    /// empty for now).
    #[test]
    fn languages_fall_back_to_english() {
        let de = Catalog::for_locale(Some("de-AT"));
        assert_eq!(de.language(), "de");
        assert_eq!(
            de.render(&msg("error-not-found")),
            Catalog::english().render(&msg("error-not-found"))
        );
        assert_eq!(Catalog::for_locale(Some("fr")).language(), "en");
        assert_eq!(Catalog::for_locale(None).language(), "en");
        assert_eq!(
            de.text("config-duplicate-property", &[("key", "id")]),
            "property `id` is set twice"
        );
    }

    #[test]
    fn locale_tags() {
        assert_eq!(language_of(Some("de_DE.UTF-8")), Some("de"));
        assert_eq!(language_of(Some("EN-us")), Some("en"));
        assert_eq!(language_of(Some("fr")), None);
        assert_eq!(language_of(Some("")), None);
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            locale_from(env(&[("LANG", "de_DE.UTF-8")])).as_deref(),
            Some("de_DE.UTF-8")
        );
        assert_eq!(
            locale_from(env(&[("LANG", "de_DE"), ("NAIL_LOCALE", "en")])).as_deref(),
            Some("en")
        );
        assert_eq!(locale_from(env(&[("LC_ALL", "C")])), None);
        assert_eq!(locale_from(env(&[])), None);
    }

    /// `fluent-bundle` renders a message with an argument (library smoke
    /// test kept from the WP50 slot).
    #[test]
    fn fluent_bundle_renders_a_resource() {
        let en: LanguageIdentifier = "en".parse().unwrap();
        let mut bundle = fluent_bundle::FluentBundle::new(vec![en]);
        bundle.set_use_isolating(false);
        let extra = FluentResource::try_new("count-items = { $count } items".to_owned()).unwrap();
        bundle.add_resource(extra).unwrap();
        let mut args = FluentArgs::new();
        args.set("count", 3);
        let message = bundle.get_message("count-items").unwrap();
        let mut errors = Vec::new();
        let text = bundle.format_pattern(message.value().unwrap(), Some(&args), &mut errors);
        assert_eq!(text, "3 items");
    }
}
