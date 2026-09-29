//! Rendering [`nail_api::LocalizedMessage`]s in Rust (M2c; contract
//! ADR 0043).
//!
//! **Slot (WP50) for WP53.** The UI renders localized core messages
//! itself; Rust renders them only where Rust shows text: CLI output that
//! is user-facing (`nail mcp`, `nail serve` hints), OS notifications of the
//! desktop shell. WP53 builds that here on `fluent-bundle` over the same
//! `locales/<lang>/*.ftl` catalogs, with the per-key English fallback, and
//! turns the English messages `nail-config` builds (configuration errors,
//! [`crate::ConfigError`]) into Fluent ids + arguments
//! (`ConfigIssue::localized`). The core's own small renderer
//! (`nail-core/src/strings.rs`) moves over during WP55.

#[cfg(test)]
mod tests {
    use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
    use unic_langid::LanguageIdentifier;

    /// `fluent-bundle` parses the shipped English catalog and renders a
    /// message with an argument (library smoke test for WP53).
    #[test]
    fn fluent_bundle_renders_the_shipped_catalog() {
        let text = include_str!("../../../locales/en/app.ftl");
        let resource = FluentResource::try_new(text.to_owned())
            .unwrap_or_else(|(_, errors)| panic!("app.ftl: {errors:?}"));
        let en: LanguageIdentifier = "en".parse().unwrap();
        let mut bundle = FluentBundle::new(vec![en]);
        bundle.set_use_isolating(false);
        bundle.add_resource(resource).unwrap();
        let message = bundle.get_message("error-not-found").unwrap();
        let mut errors = Vec::new();
        let rendered = bundle.format_pattern(message.value().unwrap(), None, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(!rendered.is_empty());

        let extra = FluentResource::try_new("count-items = { $count } items".to_owned()).unwrap();
        bundle.add_resource(extra).unwrap();
        let mut args = FluentArgs::new();
        args.set("count", 3);
        let message = bundle.get_message("count-items").unwrap();
        let text = bundle.format_pattern(message.value().unwrap(), Some(&args), &mut errors);
        assert_eq!(text, "3 items");
    }
}
