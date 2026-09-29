//! Signatures: the identity's plain, HTML and Markdown variants, and the
//! `-- ` separator line (RFC 3676 4.3).

use nail_api::accounts::{ComposerMode, Signature};

use crate::convert::{html_to_markdown, html_to_text};
use crate::markdown::render_markdown;
use crate::sanitize::sanitize_outgoing;
use crate::style::MailStyle;
use crate::text_to_html;

/// The signature separator line (dash, dash, space).
pub const SIGNATURE_SEPARATOR: &str = "-- ";

/// Splits plain text or Markdown at its last signature separator line:
/// `(body, Some(signature))`, or `(text, None)` without one.
pub(crate) fn split_signature(text: &str) -> (&str, Option<&str>) {
    if let Some(rest) = text.strip_prefix("-- \n")
        && !rest.contains("\n-- \n")
    {
        return ("", Some(rest));
    }
    match text.rfind("\n-- \n") {
        Some(at) => (&text[..at], Some(&text[at + 5..])),
        None => (text, None),
    }
}

fn nonempty(value: Option<&String>) -> Option<&str> {
    value.map(String::as_str).filter(|v| !v.trim().is_empty())
}

/// The signature block for a body in `mode`, separator included, or `None`
/// when the identity has no signature. A missing variant is derived from
/// another one (HTML → text / Markdown, Markdown → HTML, text → HTML).
pub fn signature_block(
    signature: Option<&Signature>,
    mode: ComposerMode,
    style: &MailStyle,
) -> Option<String> {
    let sig = signature?;
    let text = nonempty(sig.text.as_ref());
    let html = nonempty(sig.html.as_ref());
    let markdown = nonempty(sig.markdown.as_ref());
    match mode {
        ComposerMode::Plain => {
            let body = text
                .map(str::to_owned)
                .or_else(|| html.map(html_to_text))
                .or_else(|| markdown.map(str::to_owned))?;
            Some(format!("{SIGNATURE_SEPARATOR}\n{}", body.trim_end()))
        }
        ComposerMode::Markdown => {
            let body = markdown
                .map(str::to_owned)
                .or_else(|| text.map(str::to_owned))
                .or_else(|| html.map(|h| html_to_markdown(h).markdown))?;
            Some(format!("{SIGNATURE_SEPARATOR}\n{}", body.trim_end()))
        }
        ComposerMode::Html => {
            let body = html
                .map(sanitize_outgoing)
                .or_else(|| markdown.map(|m| render_markdown(m, style)))
                .or_else(|| text.map(|t| text_to_html(&t.replace("\n\n", "\n"))))?;
            Some(format!(
                "<div class=\"signature\"><p>{SIGNATURE_SEPARATOR}</p>{}</div>",
                body.trim()
            ))
        }
    }
}

/// Removes a trailing signature from plain text or Markdown.
pub fn strip_signature(text: &str) -> &str {
    split_signature(text).0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(text: Option<&str>, html: Option<&str>, markdown: Option<&str>) -> Signature {
        Signature {
            text: text.map(Into::into),
            html: html.map(Into::into),
            markdown: markdown.map(Into::into),
        }
    }

    #[test]
    fn picks_the_variant_of_the_mode() {
        let s = sig(
            Some("Ada\nAnalyst"),
            Some("<p><b>Ada</b></p>"),
            Some("**Ada**"),
        );
        let style = MailStyle::default();
        assert_eq!(
            signature_block(Some(&s), ComposerMode::Plain, &style).unwrap(),
            "-- \nAda\nAnalyst"
        );
        assert_eq!(
            signature_block(Some(&s), ComposerMode::Markdown, &style).unwrap(),
            "-- \n**Ada**"
        );
        assert_eq!(
            signature_block(Some(&s), ComposerMode::Html, &style).unwrap(),
            "<div class=\"signature\"><p>-- </p><p><b>Ada</b></p></div>"
        );
        assert_eq!(signature_block(None, ComposerMode::Plain, &style), None);
        assert_eq!(
            signature_block(
                Some(&sig(Some(" "), None, None)),
                ComposerMode::Html,
                &style
            ),
            None
        );
    }

    #[test]
    fn derives_missing_variants() {
        let style = MailStyle::default();
        let text_only = sig(Some("Ada <ada@x.org>\nAnalyst"), None, None);
        assert_eq!(
            signature_block(Some(&text_only), ComposerMode::Html, &style).unwrap(),
            "<div class=\"signature\"><p>-- </p><p>Ada &lt;ada@x.org&gt;<br>Analyst</p></div>"
        );
        let html_only = sig(None, Some("<p><b>Ada</b><br>Analyst</p>"), None);
        assert_eq!(
            signature_block(Some(&html_only), ComposerMode::Plain, &style).unwrap(),
            "-- \nAda\nAnalyst"
        );
        assert_eq!(
            signature_block(Some(&html_only), ComposerMode::Markdown, &style).unwrap(),
            "-- \n**Ada**\\\nAnalyst"
        );
        let md_only = sig(None, None, Some("*Ada*"));
        assert!(
            signature_block(Some(&md_only), ComposerMode::Html, &style)
                .unwrap()
                .contains("<em>Ada</em>")
        );
    }

    #[test]
    fn splits_at_the_last_separator() {
        assert_eq!(
            split_signature("a\n-- \nb\n-- \nc"),
            ("a\n-- \nb", Some("c"))
        );
        assert_eq!(split_signature("-- \nsig"), ("", Some("sig")));
        assert_eq!(split_signature("a --\nb"), ("a --\nb", None));
        assert_eq!(strip_signature("body\n-- \nsig"), "body");
    }
}
