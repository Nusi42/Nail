//! The mail stylesheet: how outgoing HTML looks to recipients.
//!
//! Every identity may set its own stylesheet and code-block theme (design,
//! "Rendering and safety"). Many mail clients strip `<style>` blocks, so the
//! rules are inlined into `style` attributes with [`css_inline`]. The
//! stylesheet is text the caller passes in; nothing is ever fetched.

use crate::error::ComposeError;

/// The syntect theme used for code blocks unless an identity picks another.
pub const DEFAULT_CODE_THEME: &str = "InspiredGitHub";

/// The built-in mail stylesheet: readable defaults that survive most
/// clients.
pub const DEFAULT_MAIL_CSS: &str = "\
body { font-family: -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif; font-size: 14px; line-height: 1.5; color: #1f2328; }
p { margin: 0 0 10px 0; }
h1, h2, h3, h4, h5, h6 { margin: 16px 0 8px 0; line-height: 1.25; }
blockquote { margin: 0 0 10px 0; padding: 0 0 0 10px; border-left: 3px solid #d0d7de; color: #57606a; }
pre { margin: 0 0 10px 0; padding: 10px; border-radius: 6px; font-family: Consolas, 'Courier New', monospace; font-size: 13px; overflow: auto; }
code { font-family: Consolas, 'Courier New', monospace; font-size: 13px; }
table { border-collapse: collapse; margin: 0 0 10px 0; }
th, td { border: 1px solid #d0d7de; padding: 4px 8px; }
th { background-color: #f6f8fa; }
img { max-width: 100%; }
hr { border: 0; border-top: 1px solid #d0d7de; }
.footnotes { font-size: 12px; color: #57606a; }
.signature { color: #57606a; }
";

/// How outgoing HTML is styled (per identity).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStyle {
    /// The mail stylesheet; `None` = [`DEFAULT_MAIL_CSS`].
    pub css: Option<String>,
    /// The code-block theme (one of [`code_themes`]); `None` or an unknown
    /// name = [`DEFAULT_CODE_THEME`].
    pub code_theme: Option<String>,
}

impl MailStyle {
    /// The stylesheet to inline.
    pub fn css(&self) -> &str {
        self.css.as_deref().unwrap_or(DEFAULT_MAIL_CSS)
    }

    /// The code-block theme, falling back to the default for unknown names.
    pub fn code_theme(&self) -> &str {
        match self.code_theme.as_deref() {
            Some(name) if code_themes().contains(&name) => name,
            _ => DEFAULT_CODE_THEME,
        }
    }
}

/// The code-block themes that can be chosen (syntect's bundled themes).
pub fn code_themes() -> &'static [&'static str] {
    &[
        "InspiredGitHub",
        "Solarized (light)",
        "Solarized (dark)",
        "base16-ocean.light",
        "base16-ocean.dark",
        "base16-eighties.dark",
        "base16-mocha.dark",
    ]
}

/// A complete HTML document for `fragment` with the stylesheet inlined:
/// what is sent as `text/html` and what the preview shows.
pub fn mail_document(fragment: &str, style: &MailStyle) -> Result<String, ComposeError> {
    let document = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>{fragment}</body></html>"
    );
    let inliner = css_inline::CSSInliner::options()
        .extra_css(Some(style.css().into()))
        .keep_style_tags(false)
        .keep_link_tags(false)
        .load_remote_stylesheets(false)
        .build();
    inliner
        .inline(&document)
        .map_err(|e| ComposeError::Stylesheet(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inlines_the_stylesheet_and_keeps_own_styles() {
        let html = mail_document(
            "<p>a</p><p style=\"color: red\">b</p>",
            &MailStyle {
                css: Some("p { color: blue; margin: 0 }".into()),
                code_theme: None,
            },
        )
        .unwrap();
        assert!(
            html.contains("<p style=\"color: blue;margin: 0\">a</p>")
                || html.contains("<p style=\"color: blue;margin: 0;\">a</p>")
                || html.contains("color: blue"),
            "{html}"
        );
        // An element's own style wins over the stylesheet.
        let b = html.split("b</p>").next().unwrap();
        let b = &b[b.rfind("<p").unwrap()..];
        assert!(
            b.contains("color: red") && !b.contains("color: blue"),
            "{b}"
        );
        assert!(
            html.starts_with("<!DOCTYPE html>") || html.starts_with("<html"),
            "{html}"
        );
        assert!(html.contains("charset=\"utf-8\""), "{html}");
    }

    #[test]
    fn default_style_and_theme_fallbacks() {
        let style = MailStyle {
            css: None,
            code_theme: Some("nope".into()),
        };
        assert_eq!(style.code_theme(), DEFAULT_CODE_THEME);
        assert_eq!(style.css(), DEFAULT_MAIL_CSS);
        let dark = MailStyle {
            css: None,
            code_theme: Some("Solarized (dark)".into()),
        };
        assert_eq!(dark.code_theme(), "Solarized (dark)");
        let html = mail_document("<blockquote>q</blockquote>", &style).unwrap();
        assert!(html.contains("border-left: 3px solid #d0d7de"), "{html}");
        assert!(!html.contains("<style"), "{html}");
    }
}
