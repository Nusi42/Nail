//! The composer's engine (M1b WP21, ADR 0022 / ADR 0023).
//!
//! Everything the core needs to turn a [`nail_api::Draft`] into a message and
//! to help the composer UI, without any I/O of its own:
//!
//! * [`build_message`]: RFC 5322 bytes with [`mail_builder`]:
//!   `multipart/alternative` (text + HTML), `multipart/related` for `cid:`
//!   images, attachments from blob bytes supplied through [`BlobSource`], an
//!   optional `text/markdown` part, `format=flowed` for plain text, threading
//!   headers for replies.
//! * [`render_markdown`] / [`render_markdown_document`]: Markdown with
//!   [`comrak`] (CommonMark + GFM + footnotes), code blocks highlighted by
//!   syntect with inline colours; [`mail_document`] inlines the per-identity
//!   mail stylesheet ([`MailStyle`]) with [`css_inline`].
//! * [`sanitize_outgoing`]: allowlist sanitizing with [`ammonia`].
//! * [`html_to_text`] and [`html_to_markdown`] (best effort, reports what it
//!   dropped), [`convert_draft`] for mode switches.
//! * Quoting ([`quote`]): reply / forward bodies in every mode, top or
//!   bottom posting, with a quote header the caller localizes.
//! * [`signature_block`] (plain / HTML / Markdown variants) and
//!   [`expand_template`] (`{variable}` templates).
//! * [`draft`]: new reply / forward drafts, subjects and recipients.

#![warn(missing_docs)]

mod build;
mod convert;
pub mod draft;
mod error;
mod flowed;
mod markdown;
pub mod quote;
mod sanitize;
mod signature;
mod style;
mod template;

pub use build::{
    BlobSource, MessageMeta, OutgoingMessage, ReplyHeaders, build_message, envelope_of,
    new_message_id, resent_message,
};
pub use convert::{Conversion, has_formatting, html_to_markdown, html_to_text};
pub use draft::{
    ComposeSettings, Converted, OriginalMessage, QuoteStrings, body_of, convert_draft,
    forward_subject, new_draft, reply_recipients, reply_subject,
};
pub use error::ComposeError;
pub use flowed::encode_flowed;
pub use markdown::{render_markdown, render_markdown_document};
pub use quote::Posting;
pub use sanitize::sanitize_outgoing;
pub use signature::{SIGNATURE_SEPARATOR, signature_block, strip_signature};
pub use style::{DEFAULT_CODE_THEME, DEFAULT_MAIL_CSS, MailStyle, code_themes, mail_document};
pub use template::{
    TemplateError, expand_template, expand_template_html, render_markdown_template,
};

/// Escapes text for HTML element content and attribute values.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Plain text as HTML: escaped, paragraphs for blank-line separated blocks,
/// `<br>` for single line breaks.
pub fn text_to_html(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let mut out = String::new();
    for block in text.split("\n\n") {
        let block = block.trim_matches('\n');
        if block.trim().is_empty() {
            continue;
        }
        let lines: Vec<String> = block.split('\n').map(escape_html).collect();
        out.push_str("<p>");
        out.push_str(&lines.join("<br>"));
        out.push_str("</p>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_wraps_text() {
        assert_eq!(
            escape_html("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
        assert_eq!(
            text_to_html("Hi <Bob>,\nline two\n\n\nnext"),
            "<p>Hi &lt;Bob&gt;,<br>line two</p><p>next</p>"
        );
        assert_eq!(text_to_html("  \n"), "");
    }

    #[test]
    fn libraries_render_inline_sanitize_and_build_mime() {
        // Smoke test kept from WP20: the chosen libraries work together here.
        let html = render_markdown("| a |\n|---|\n| ~~b~~ |\n", &MailStyle::default());
        assert!(
            html.contains("<table>") && html.contains("<del>b</del>"),
            "{html}"
        );
        let clean = sanitize_outgoing("<p onclick=\"x()\">hi<script>bad()</script></p>");
        assert_eq!(clean, "<p>hi</p>");
    }
}
