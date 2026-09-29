//! The composer's engine (M1b WP21; stub after WP20).
//!
//! Expected shape (ADR 0022): MIME building with [`mail_builder`]
//! (`multipart/alternative` text + HTML, `multipart/related` for `cid:`
//! images, attachments, an optional `text/markdown` part), Markdown
//! rendering with [`comrak`] (CommonMark + GFM + footnotes, code highlighted
//! through syntect), the per-identity mail stylesheet inlined with
//! [`css_inline`], outgoing HTML sanitized with [`ammonia`], HTML → text,
//! reply / forward quoting, signatures and templates. It replaces
//! `nail-core`'s plain-text `mime.rs`.

#![warn(missing_docs)]

#[cfg(test)]
mod tests {
    //! Smoke tests: the chosen libraries build and work together on this
    //! platform (WP20 verifies them before WP21 relies on them).

    #[test]
    fn libraries_render_inline_sanitize_and_build_mime() {
        let mut options = comrak::Options::default();
        options.extension.table = true;
        options.extension.strikethrough = true;
        options.extension.footnotes = true;
        let adapter = comrak::plugins::syntect::SyntectAdapter::new(None);
        let mut plugins = comrak::options::Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(&adapter);
        let html = comrak::markdown_to_html_with_plugins(
            "| a |\n|---|\n| ~~b~~ |\n\n```rust\nfn main() {}\n```\n",
            &options,
            &plugins,
        );
        assert!(
            html.contains("<table>") && html.contains("<del>b</del>"),
            "{html}"
        );
        assert!(html.contains("<span class="), "code is highlighted: {html}");

        let inliner = css_inline::CSSInliner::options()
            .extra_css(Some("p { color: red }".into()))
            .build();
        let inlined = inliner
            .inline("<html><body><p>x</p></body></html>")
            .unwrap();
        assert!(inlined.contains("color: red"), "{inlined}");

        let clean = ammonia::clean("<p onclick=\"x()\">hi<script>bad()</script></p>");
        assert_eq!(clean, "<p>hi</p>");

        let raw = mail_builder::MessageBuilder::new()
            .from(("Ada", "ada@example.org"))
            .to("bob@example.org")
            .subject("Hi")
            .text_body("hi")
            .html_body(clean)
            .write_to_vec()
            .unwrap();
        let raw = String::from_utf8(raw).unwrap();
        assert!(raw.contains("multipart/alternative"), "{raw}");
    }
}
