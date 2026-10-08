//! The few user-visible strings the core renders itself: the pages a
//! browser shows after an OAuth2 sign-in, the quote header / forward
//! texts the composer puts into replies and forwards, and the text of an
//! Autocrypt setup message (M2a). They come from the
//! same Fluent catalogs as the UI's (`locales/<lang>/*.ftl`), embedded at
//! build time by `nail_config::l10n` (fluent-bundle), with the per-key
//! English fallback.

/// The text of `key` in `locale` (a BCP 47 tag such as `de-AT`), falling
/// back to English, and to the key itself when even English lacks it.
pub(crate) fn message(locale: Option<&str>, key: &str) -> String {
    message_with(locale, key, &[])
}

/// Like [`message`], with `{ $name }` placeables filled from `args`.
pub(crate) fn message_with(locale: Option<&str>, key: &str, args: &[(&str, &str)]) -> String {
    nail_config::l10n::Catalog::for_locale(locale).text(key, args)
}

/// A [`LocalizedMessage`](nail_api::LocalizedMessage) rendered in `locale`.
pub(crate) fn render(locale: Option<&str>, message: &nail_api::LocalizedMessage) -> String {
    nail_config::l10n::Catalog::for_locale(locale).render(message)
}

/// Escapes text for HTML element content.
pub(crate) use nail_util::markup::escape_html as html_escape;

/// The browser pages shown after an OAuth2 redirect, localized.
pub(crate) fn oauth_callback_pages(locale: Option<&str>) -> nail_auth::CallbackPages {
    let page = |key: &str| {
        let title = html_escape(&message(locale, "oauth-callback-title"));
        let text = html_escape(&message(locale, key));
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head>\
             <body><p>{text}</p></body></html>"
        )
    };
    nail_auth::CallbackPages {
        success_html: page("oauth-callback-success"),
        failure_html: page("oauth-callback-failure"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_with_english_fallback() {
        assert_eq!(
            message(None, "oauth-callback-success"),
            "Signed in. You can close this window and return to Nail."
        );
        // The German catalog is empty: per-key English fallback.
        assert_eq!(
            message(Some("de-AT"), "oauth-callback-success"),
            message(None, "oauth-callback-success")
        );
        assert_eq!(message(None, "no-such-key"), "no-such-key");
    }

    #[test]
    fn placeables_are_filled_in() {
        assert_eq!(
            message_with(
                None,
                "composer-quote-header",
                &[("date", "1 May 2026"), ("name", "Bob")]
            ),
            "On 1 May 2026, Bob wrote:"
        );
        assert_ne!(message(None, "composer-forward-header").len(), 0);
        assert_eq!(message(None, "message-header-from"), "From");
    }

    #[test]
    fn autocrypt_setup_texts_exist() {
        for key in [
            "autocrypt-setup-message-body",
            "autocrypt-setup-message-html-intro",
        ] {
            let text = message(Some("de"), key);
            assert_ne!(text, key);
            assert!(text.contains("setup code"), "{text}");
        }
    }

    /// Every Fluent id the core builds itself exists in English (M2c).
    #[test]
    fn core_message_ids_exist() {
        let catalog = nail_config::l10n::Catalog::english();
        for id in [
            "sync-error-folders",
            "cli-data-dir-in-use",
            "cli-data-dir-in-use-by",
            "snooze-error-not-in-future",
            "snooze-error-too-far",
            "snooze-error-store",
        ]
        .into_iter()
        .chain(nail_triage::FLUENT_IDS.iter().copied())
        .chain(nail_triage::reminders::FLUENT_IDS.iter().copied())
        .chain(nail_sieve::reasons::ALL.iter().copied())
        // M3a: the core's own PIM texts.
        .chain([
            nail_store::pim::COLLECTED_BOOK_NAME_ID,
            "pim-calendar-default-name",
            "pim-task-list-default-name",
            "pim-address-book-default-name",
        ]) {
            assert!(catalog.has(id), "{id} is missing in locales/en");
        }
        let rendered = render(
            None,
            &nail_api::LocalizedMessage::new("sync-error-folders")
                .arg("count", 1u64)
                .arg("folders", "Lists"),
        );
        assert_eq!(rendered, "The folder Lists could not be synchronized.");
    }

    #[test]
    fn pages_are_escaped_html() {
        let pages = oauth_callback_pages(None);
        assert!(pages.success_html.contains("Signed in."));
        assert!(pages.failure_html.starts_with("<!doctype html>"));
        assert_eq!(html_escape("<a & 'b'>"), "&lt;a &amp; &#39;b&#39;&gt;");
    }
}
