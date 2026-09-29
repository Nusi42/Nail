//! The few user-visible strings the core renders itself (the pages a
//! browser shows after an OAuth2 sign-in). They come from the same Fluent
//! catalogs as the UI's (`locales/<lang>/app.ftl`), embedded at build time,
//! with the per-key English fallback.
//!
//! Only simple messages (`key = text` on one line, no placeables) are
//! supported, which is all the core needs.

/// The English catalog (complete).
const EN: &str = include_str!("../../../locales/en/app.ftl");
/// The German catalog (may be empty; falls back to English per key).
const DE: &str = include_str!("../../../locales/de/app.ftl");

fn lookup(catalog: &str, key: &str) -> Option<String> {
    catalog.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (!line.starts_with([' ', '\t', '#', '-']) && k.trim() == key)
            .then(|| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    })
}

/// The text of `key` in `locale` (a BCP 47 tag such as `de-AT`), falling
/// back to English, and to the key itself when even English lacks it.
pub fn message(locale: Option<&str>, key: &str) -> String {
    let language = locale
        .and_then(|l| l.split(['-', '_']).next())
        .map(str::to_ascii_lowercase);
    let localized = match language.as_deref() {
        Some("de") => lookup(DE, key),
        _ => None,
    };
    localized
        .or_else(|| lookup(EN, key))
        .unwrap_or_else(|| key.to_owned())
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
    fn pages_are_escaped_html() {
        let pages = oauth_callback_pages(None);
        assert!(pages.success_html.contains("Signed in."));
        assert!(pages.failure_html.starts_with("<!doctype html>"));
        assert_eq!(html_escape("<a & 'b'>"), "&lt;a &amp; &#39;b&#39;&gt;");
    }
}
