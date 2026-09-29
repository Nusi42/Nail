//! Markdown → HTML, rendered in the core so that the preview, the sent
//! HTML and the MCP draft tools produce identical output (design,
//! "Rendering and safety").
//!
//! CommonMark + GFM (tables, task lists, strikethrough, autolinks) and
//! footnotes; fenced code blocks are highlighted by syntect with inline
//! colours (the identity's code theme), so they survive clients that strip
//! `<style>`. Raw HTML in the source is allowed and then sanitized like any
//! outgoing HTML. A signature after the standard `-- ` line keeps its line
//! breaks.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};

use crate::error::ComposeError;
use crate::sanitize::sanitize_outgoing;
use crate::signature::split_signature;
use crate::style::{MailStyle, mail_document};

fn adapter(theme: &str) -> Arc<SyntectAdapter> {
    static ADAPTERS: OnceLock<Mutex<HashMap<String, Arc<SyntectAdapter>>>> = OnceLock::new();
    let mut adapters = ADAPTERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    adapters
        .entry(theme.to_owned())
        .or_insert_with(|| Arc::new(SyntectAdapterBuilder::new().theme(theme).build()))
        .clone()
}

fn options(hardbreaks: bool) -> comrak::Options<'static> {
    let mut options = comrak::Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.render.r#unsafe = true;
    options.render.hardbreaks = hardbreaks;
    options
}

fn to_html(markdown: &str, theme: &str, hardbreaks: bool) -> String {
    let adapter = adapter(theme);
    let mut plugins = comrak::options::Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(adapter.as_ref());
    comrak::markdown_to_html_with_plugins(markdown, &options(hardbreaks), &plugins)
}

/// Renders Markdown to a sanitized HTML fragment (no stylesheet applied).
pub fn render_markdown(markdown: &str, style: &MailStyle) -> String {
    let markdown = markdown.replace("\r\n", "\n");
    let (body, signature) = split_signature(&markdown);
    let mut html = to_html(body, style.code_theme(), false);
    if let Some(signature) = signature {
        html.push_str("<div class=\"signature\"><p>-- </p>");
        html.push_str(&to_html(signature, style.code_theme(), true));
        html.push_str("</div>");
    }
    sanitize_outgoing(&html)
}

/// Renders Markdown to the complete HTML document that is sent: sanitized,
/// with the mail stylesheet inlined. `RenderPreview` answers with this.
pub fn render_markdown_document(markdown: &str, style: &MailStyle) -> Result<String, ComposeError> {
    mail_document(&render_markdown(markdown, style), style)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_gfm_and_footnotes() {
        let md = "# Title\n\n~~old~~ **new** https://example.org\n\n\
            - [x] done\n- [ ] open\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n\
            Note[^1].\n\n[^1]: The footnote.\n";
        let html = render_markdown(md, &MailStyle::default());
        for part in [
            "<h1>Title</h1>",
            "<del>old</del>",
            "<strong>new</strong>",
            "<a href=\"https://example.org\">https://example.org</a>",
            "type=\"checkbox\"",
            "<table>",
            "<td>2</td>",
            "footnotes",
            "The footnote.",
        ] {
            assert!(html.contains(part), "{part} missing in {html}");
        }
    }

    #[test]
    fn highlights_code_with_inline_colours() {
        let md = "```rust\nfn main() { let x = 1; }\n```\n";
        let html = render_markdown(md, &MailStyle::default());
        assert!(html.contains("<pre"), "{html}");
        assert!(
            html.contains("style=\"color:#"),
            "code is coloured inline: {html}"
        );
        assert!(html.contains("background-color:#"), "{html}");
        let dark = render_markdown(
            md,
            &MailStyle {
                css: None,
                code_theme: Some("base16-ocean.dark".into()),
            },
        );
        assert_ne!(html, dark);
    }

    #[test]
    fn raw_html_is_sanitized() {
        let html = render_markdown(
            "Hi <b>there</b><script>x()</script> <img src=x onerror=alert(1)>",
            &MailStyle::default(),
        );
        assert!(html.contains("<b>there</b>"), "{html}");
        assert!(
            !html.contains("script") && !html.contains("onerror"),
            "{html}"
        );
    }

    #[test]
    fn signature_keeps_its_line_breaks() {
        let html = render_markdown("Hello\n\n-- \nAda\nAnalyst", &MailStyle::default());
        assert!(html.contains("<p>Hello</p>"), "{html}");
        assert!(html.contains("class=\"signature\""), "{html}");
        assert!(html.contains("Ada<br"), "{html}");
    }

    #[test]
    fn document_has_the_stylesheet_inlined() {
        let doc = render_markdown_document("> quoted", &MailStyle::default()).unwrap();
        assert!(doc.contains("<blockquote style="), "{doc}");
        assert!(!doc.contains("<style"), "{doc}");
    }
}
