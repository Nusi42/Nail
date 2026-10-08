//! Rendering [`LocalizedMessage`]s in Rust (M2c WP53; contract ADR 0043).
//!
//! The UI renders localized core messages itself. Rust renders them only
//! where Rust shows text: user-facing CLI output (`nail mcp`, `nail
//! serve`), OS notifications of the desktop shell, and the English
//! diagnostic that accompanies every message (logs, `message` fields).
//!
//! [`Catalog`] is a `fluent-bundle` over the same `locales/<lang>/*.ftl`
//! catalogs the UI loads, embedded at build time ([`CATALOGS`], listed by
//! `build.rs` from the `locales/` folders). Lookups fall back per key: the requested language first,
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

include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

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
        if let Err(errors) = bundle.add_resource(resource) {
            tracing::debug!(?errors, "duplicate Fluent ids; the first definition wins");
        }
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
            .finish_non_exhaustive()
    }
}

/// The bundle of each language of [`LANGUAGES`], in that order.
fn bundles() -> &'static [Bundle] {
    static PER_LANGUAGE: OnceLock<Vec<Bundle>> = OnceLock::new();
    PER_LANGUAGE.get_or_init(|| {
        CATALOGS
            .iter()
            .map(|(language, files)| bundle(language, files))
            .collect()
    })
}

/// The bundle of `language` (one of [`LANGUAGES`]).
fn language_bundle(language: &str) -> Option<&'static Bundle> {
    let index = LANGUAGES.iter().position(|l| *l == language)?;
    bundles().get(index)
}

fn english_bundle() -> &'static Bundle {
    // English is the first language (build.rs).
    &bundles()[0]
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
        match language_of(tag).and_then(|l| Some((l, language_bundle(l)?))) {
            Some((language, bundle)) if language != "en" => Self {
                bundles: vec![bundle, english_bundle()],
                language,
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
            args: indexmap::IndexMap::new(),
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

/// Test support: fails when the `.rs` files under `<manifest_dir>/src`
/// spell out a message id with one of `prefixes` that the English
/// catalogs lack (see [`missing_message_ids`]). Pass
/// `env!("CARGO_MANIFEST_DIR")`.
///
/// # Panics
///
/// When an id is missing, naming each one and its file.
#[doc(hidden)]
#[track_caller]
pub fn assert_message_ids_in_catalog(manifest_dir: &str, prefixes: &[&str]) {
    let missing = missing_message_ids(&std::path::Path::new(manifest_dir).join("src"), prefixes);
    assert!(missing.is_empty(), "{missing:#?}");
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
    use std::path::PathBuf;

    /// `build.rs` embeds every catalog folder and file, English first.
    #[test]
    fn every_catalog_file_is_embedded() {
        let locales = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        assert_eq!(LANGUAGES, ["en", "de"]);
        for (lang, files) in CATALOGS {
            let on_disk = std::fs::read_dir(locales.join(lang))
                .unwrap()
                .filter(|e| {
                    e.as_ref()
                        .unwrap()
                        .path()
                        .extension()
                        .is_some_and(|x| x == "ftl")
                })
                .count();
            assert_eq!(files.len(), on_disk, "{lang}");
            assert!(files.iter().any(|(n, _)| *n == "core.ftl"), "{lang}");
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
        for (name, text) in CATALOGS[0].1 {
            if let Err((_, errors)) = FluentResource::try_new((*text).to_owned()) {
                panic!("{name}: {errors:?}");
            }
        }
    }

    #[test]
    fn renders_with_arguments_and_plurals() {
        let en = Catalog::english();
        assert!(en.has("error-not-found"));
        assert_ne!(en.render(&msg("error-not-found")).len(), 0);
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
    /// One text per meaning (wave Q2): a message that only references
    /// another renders its text, with the caller's arguments.
    #[test]
    fn message_references_render_the_shared_text() {
        assert_eq!(
            english(&msg("config-pim-account-unknown").arg("name", "x")),
            english(&msg("config-member-unknown").arg("name", "x"))
        );
        assert_eq!(
            english(&msg("config-pim-account-unknown").arg("name", "x")),
            "unknown account \"x\""
        );
        assert_eq!(english(&msg("export-format-mbox")), "mbox file");
        assert_eq!(
            english(&msg("exchange-rule-reason-disabled")),
            "The rule is switched off."
        );
    }

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
