//! Reply and forward bodies (design, "Switching and quoting").
//!
//! * Plain text quotes the original's text with `> ` (nested quotes become
//!   `>>`).
//! * HTML keeps the original HTML (sanitized) inside a
//!   `<blockquote type="cite">`.
//! * Markdown converts the original HTML to Markdown and quotes it with `>`;
//!   a text-only original is quoted as is.
//!
//! The quote header ("On 29 Sep 2026, Ada wrote:") and the forward header
//! are localized by the caller; top or bottom posting is per identity.

use nail_api::accounts::ComposerMode;

use crate::convert::html_to_markdown;
use crate::sanitize::sanitize_outgoing;
use crate::{escape_html, text_to_html};

/// Where the reply goes relative to the quote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Posting {
    /// Quote first, reply below it (interleaved style).
    #[default]
    Bottom,
    /// Reply above the quote.
    Top,
}

/// Prefixes every line of `text` with `> ` (`>` for empty lines, `>` only
/// for lines that are quotes already).
pub fn quote_text(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    text.trim_end_matches('\n')
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".to_owned()
            } else if line.starts_with('>') {
                format!(">{line}")
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The original as an HTML blockquote: its HTML (sanitized) or its text.
pub fn quote_html(text: &str, html: Option<&str>) -> String {
    let inner = match html.filter(|h| !h.trim().is_empty()) {
        Some(html) => sanitize_outgoing(html),
        None => text_to_html(text),
    };
    format!("<blockquote type=\"cite\">{}</blockquote>", inner.trim())
}

/// The original as a Markdown quote, and whether converting its HTML lost
/// formatting.
pub fn quote_markdown(text: &str, html: Option<&str>) -> (String, bool) {
    let (source, lossy) = match html.filter(|h| !h.trim().is_empty()) {
        Some(html) => {
            let conversion = html_to_markdown(html);
            let lossy = conversion.lossy();
            (conversion.markdown, lossy)
        }
        None => (text.to_owned(), false),
    };
    (quote_text(&source), lossy)
}

/// The body of a reply in `mode`: the quote header and quote, the
/// signature block (already in the mode's format), placed for `posting`.
/// The user writes at the start (top posting) or at the end (bottom
/// posting), before the signature.
pub fn reply_body(
    mode: ComposerMode,
    posting: Posting,
    header: &str,
    text: &str,
    html: Option<&str>,
    signature: Option<&str>,
) -> String {
    match mode {
        ComposerMode::Html => {
            let quote = format!("<p>{}</p>{}", escape_html(header), quote_html(text, html));
            let signature = signature.unwrap_or_default();
            match posting {
                Posting::Top => format!("<p></p>{signature}{quote}"),
                Posting::Bottom => format!("{quote}<p></p>{signature}"),
            }
        }
        ComposerMode::Plain | ComposerMode::Markdown => {
            let quoted = if mode == ComposerMode::Markdown {
                quote_markdown(text, html).0
            } else {
                quote_text(text)
            };
            let quote = format!("{header}\n{quoted}");
            match (posting, signature) {
                (Posting::Top, Some(sig)) => format!("\n\n{sig}\n\n{quote}"),
                (Posting::Top, None) => format!("\n\n{quote}"),
                (Posting::Bottom, Some(sig)) => format!("{quote}\n\n\n\n{sig}"),
                (Posting::Bottom, None) => format!("{quote}\n\n"),
            }
        }
    }
}

/// The body of a forward in `mode`: room to write, the signature, then the
/// localized intro line, the header fields (label, value) and the original
/// (not quoted).
pub fn forward_body(
    mode: ComposerMode,
    intro: &str,
    fields: &[(String, String)],
    text: &str,
    html: Option<&str>,
    signature: Option<&str>,
) -> String {
    match mode {
        ComposerMode::Html => {
            let mut head = escape_html(intro);
            for (label, value) in fields {
                head.push_str(&format!(
                    "<br>{}: {}",
                    escape_html(label),
                    escape_html(value)
                ));
            }
            let original = match html.filter(|h| !h.trim().is_empty()) {
                Some(html) => sanitize_outgoing(html),
                None => text_to_html(text),
            };
            format!(
                "<p></p>{}<div class=\"forwarded\"><p>{head}</p>{}</div>",
                signature.unwrap_or_default(),
                original.trim()
            )
        }
        ComposerMode::Plain | ComposerMode::Markdown => {
            let markdown = mode == ComposerMode::Markdown;
            let break_ = if markdown { "\\\n" } else { "\n" };
            let mut head = intro.to_owned();
            for (label, value) in fields {
                head.push_str(break_);
                head.push_str(&format!("{label}: {value}"));
            }
            let original = match (markdown, html.filter(|h| !h.trim().is_empty())) {
                (true, Some(html)) => html_to_markdown(html).markdown,
                _ => text.trim_end().to_owned(),
            };
            let sig = signature.map(|s| format!("{s}\n\n")).unwrap_or_default();
            format!("\n\n{sig}{head}\n\n{original}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const HEADER: &str = "On 1 May, Bob wrote:";

    #[test]
    fn quotes_text_lines() {
        assert_eq!(quote_text("a\n\n> b\nc\n"), "> a\n>\n>> b\n> c");
    }

    #[test]
    fn plain_reply_top_and_bottom() {
        let top = reply_body(
            ComposerMode::Plain,
            Posting::Top,
            HEADER,
            "Hi\nthere",
            None,
            Some("-- \nAda"),
        );
        assert_eq!(top, "\n\n-- \nAda\n\nOn 1 May, Bob wrote:\n> Hi\n> there");
        let bottom = reply_body(
            ComposerMode::Plain,
            Posting::Bottom,
            HEADER,
            "Hi",
            None,
            None,
        );
        assert_eq!(bottom, "On 1 May, Bob wrote:\n> Hi\n\n");
        let bottom_sig = reply_body(
            ComposerMode::Plain,
            Posting::Bottom,
            HEADER,
            "Hi",
            None,
            Some("-- \nAda"),
        );
        assert_eq!(bottom_sig, "On 1 May, Bob wrote:\n> Hi\n\n\n\n-- \nAda");
    }

    #[test]
    fn html_reply_keeps_the_original_html() {
        let body = reply_body(
            ComposerMode::Html,
            Posting::Top,
            "On <date>, Bob wrote:",
            "ignored",
            Some("<p onclick=\"x()\">Hi <b>Ada</b></p><script>bad()</script>"),
            Some("<div class=\"signature\"><p>-- </p><p>Ada</p></div>"),
        );
        assert_eq!(
            body,
            "<p></p><div class=\"signature\"><p>-- </p><p>Ada</p></div>\
             <p>On &lt;date&gt;, Bob wrote:</p>\
             <blockquote type=\"cite\"><p>Hi <b>Ada</b></p></blockquote>"
        );
        let text_only = reply_body(
            ComposerMode::Html,
            Posting::Bottom,
            HEADER,
            "a\nb",
            None,
            None,
        );
        assert_eq!(
            text_only,
            "<p>On 1 May, Bob wrote:</p><blockquote type=\"cite\"><p>a<br>b</p></blockquote><p></p>"
        );
    }

    #[test]
    fn markdown_reply_converts_html_to_quoted_markdown() {
        let body = reply_body(
            ComposerMode::Markdown,
            Posting::Bottom,
            HEADER,
            "ignored",
            Some("<p>Hi <b>Ada</b></p><ul><li>one</li></ul>"),
            None,
        );
        assert_eq!(body, "On 1 May, Bob wrote:\n> Hi **Ada**\n>\n> - one\n\n");
        let (_, lossy) = quote_markdown("", Some("<p><u>under</u></p>"));
        assert!(lossy);
    }

    #[test]
    fn forwards_in_every_mode() {
        let fields = vec![
            ("From".to_owned(), "Bob <bob@x.org>".to_owned()),
            ("Subject".to_owned(), "Plans".to_owned()),
        ];
        assert_eq!(
            forward_body(
                ComposerMode::Plain,
                "-- Forwarded --",
                &fields,
                "Body\n",
                None,
                None
            ),
            "\n\n-- Forwarded --\nFrom: Bob <bob@x.org>\nSubject: Plans\n\nBody"
        );
        assert_eq!(
            forward_body(
                ComposerMode::Markdown,
                "Fwd",
                &fields,
                "",
                Some("<p><i>x</i></p>"),
                Some("-- \nAda")
            ),
            "\n\n-- \nAda\n\nFwd\\\nFrom: Bob <bob@x.org>\\\nSubject: Plans\n\n*x*"
        );
        assert_eq!(
            forward_body(ComposerMode::Html, "Fwd", &fields[..1], "t", None, None),
            "<p></p><div class=\"forwarded\"><p>Fwd<br>From: Bob &lt;bob@x.org&gt;</p><p>t</p></div>"
        );
    }
}
