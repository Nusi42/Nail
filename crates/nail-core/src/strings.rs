//! The few user-visible strings the core renders itself: the pages a
//! browser shows after an OAuth2 sign-in, the quote header / forward
//! texts the composer puts into replies and forwards, and the text of an
//! Autocrypt setup message (M2a). They come from the
//! same Fluent catalogs as the UI's (`locales/<lang>/*.ftl`), embedded at
//! build time, with the per-key English fallback.
//!
//! Only single-line messages are supported, with variable placeables
//! (`{ $name }`), which is all the core needs.

/// The catalogs the core reads, English (complete) and German (may be
/// empty; falls back to English per key).
const EN: &[&str] = &[
    include_str!("../../../locales/en/app.ftl"),
    include_str!("../../../locales/en/compose.ftl"),
    include_str!("../../../locales/en/composer.ftl"),
    include_str!("../../../locales/en/security.ftl"),
];
const DE: &[&str] = &[
    include_str!("../../../locales/de/app.ftl"),
    include_str!("../../../locales/de/compose.ftl"),
    include_str!("../../../locales/de/composer.ftl"),
    include_str!("../../../locales/de/security.ftl"),
];

fn lookup(catalogs: &[&str], key: &str) -> Option<String> {
    catalogs.iter().find_map(|catalog| {
        catalog.lines().find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (!line.starts_with([' ', '\t', '#', '-']) && k.trim() == key)
                .then(|| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        })
    })
}

/// The text of `key` in `locale` (a BCP 47 tag such as `de-AT`), falling
/// back to English, and to the key itself when even English lacks it.
pub fn message(locale: Option<&str>, key: &str) -> String {
    message_with(locale, key, &[])
}

/// Like [`message`], with `{ $name }` placeables replaced by `args`
/// (unknown variables stay as written).
pub fn message_with(locale: Option<&str>, key: &str, args: &[(&str, &str)]) -> String {
    let language = locale
        .and_then(|l| l.split(['-', '_']).next())
        .map(str::to_ascii_lowercase);
    let localized = match language.as_deref() {
        Some("de") => lookup(DE, key),
        _ => None,
    };
    let text = localized
        .or_else(|| lookup(EN, key))
        .unwrap_or_else(|| key.to_owned());
    substitute(&text, args)
}

fn substitute(text: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = after[..end].trim();
        let value = inner
            .strip_prefix('$')
            .and_then(|name| args.iter().find(|(n, _)| *n == name.trim()))
            .map(|(_, v)| *v);
        match value {
            Some(v) => out.push_str(v),
            None => out.push_str(&rest[start..start + end + 2]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Escapes text for HTML element content.
pub fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The browser pages shown after an OAuth2 redirect, localized.
pub fn oauth_callback_pages(locale: Option<&str>) -> nail_auth::CallbackPages {
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
        assert_eq!(
            substitute("a { $x } { $y } {", &[("x", "1")]),
            "a 1 { $y } {"
        );
        assert!(!message(None, "composer-forward-header").is_empty());
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

    #[test]
    fn pages_are_escaped_html() {
        let pages = oauth_callback_pages(None);
        assert!(pages.success_html.contains("Signed in."));
        assert!(pages.failure_html.starts_with("<!doctype html>"));
        assert_eq!(html_escape("<a & 'b'>"), "&lt;a &amp; &#39;b&#39;&gt;");
    }
}
