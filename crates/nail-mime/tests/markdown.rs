//! `text/markdown` parts as body alternatives (M2c WP53).

use nail_mime::{attachment_content, parse_message};
use pretty_assertions::assert_eq;

const SOURCE: &str = "# Plan\r\n\r\nShip **M2c** on Friday.\r\n";

fn alternative(parts: &[(&str, &str)]) -> String {
    let mut raw = String::from(
        "From: Ada <ada@example.org>\r\nTo: sam@example.org\r\nSubject: Plan\r\n\
         MIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"alt\"\r\n\r\n",
    );
    for (content_type, body) in parts {
        raw.push_str(&format!(
            "--alt\r\nContent-Type: {content_type}; charset=utf-8\r\n\r\n{body}\r\n"
        ));
    }
    raw.push_str("--alt--\r\n");
    raw
}

/// What Nail's composer sends for Markdown mail.
#[test]
fn the_composers_three_alternatives_show_text_and_html() {
    let raw = alternative(&[
        ("text/plain", "Plan\r\n\r\nShip M2c on Friday."),
        ("text/markdown; variant=GFM", SOURCE),
        (
            "text/html",
            "<h1>Plan</h1><p>Ship <strong>M2c</strong> on Friday.</p>",
        ),
    ]);
    let parsed = parse_message(raw.as_bytes());
    assert_eq!(parsed.text, "Plan\r\n\r\nShip M2c on Friday.");
    assert!(
        parsed
            .sanitized_html
            .as_deref()
            .unwrap()
            .contains("<strong>M2c</strong>")
    );
    assert!(parsed.attachments.is_empty(), "{:?}", parsed.attachments);
    assert!(!parsed.has_attachments());
}

#[test]
fn markdown_and_html_show_the_markdown_source_as_text() {
    let raw = alternative(&[
        ("text/markdown", SOURCE),
        ("text/html", "<p>Ship <b>M2c</b> on Friday.</p>"),
    ]);
    let parsed = parse_message(raw.as_bytes());
    assert_eq!(parsed.text, "# Plan\r\n\r\nShip **M2c** on Friday.");
    assert_eq!(parsed.preview, "# Plan Ship **M2c** on Friday.");
    assert!(
        parsed
            .sanitized_html
            .as_deref()
            .unwrap()
            .contains("<b>M2c</b>")
    );
    assert!(parsed.attachments.is_empty());
}

#[test]
fn plain_and_markdown_render_the_markdown() {
    let raw = alternative(&[
        ("text/plain", "Ship M2c on Friday."),
        ("text/x-markdown", SOURCE),
    ]);
    let parsed = parse_message(raw.as_bytes());
    assert_eq!(parsed.text, "Ship M2c on Friday.");
    let html = parsed.sanitized_html.unwrap();
    assert!(html.contains("<h1>Plan</h1>"), "{html}");
    assert!(html.contains("<strong>M2c</strong>"), "{html}");
    assert!(parsed.attachments.is_empty());
}

#[test]
fn a_lone_markdown_body_is_text_and_rendered_safely() {
    let raw = "From: ada@example.org\r\nSubject: md\r\nMIME-Version: 1.0\r\n\
               Content-Type: text/markdown; charset=utf-8\r\n\r\n\
               Hello *world*.\r\n\r\n<script>alert(1)</script>\r\n\r\n\
               [site](https://example.org/)\r\n";
    let parsed = parse_message(raw.as_bytes());
    assert!(parsed.text.starts_with("Hello *world*."), "{}", parsed.text);
    let html = parsed.sanitized_html.unwrap();
    assert!(html.contains("<em>world</em>"), "{html}");
    assert!(!html.contains("<script"), "{html}");
    assert!(html.contains("https://example.org/"), "{html}");
    assert!(parsed.attachments.is_empty());
}

#[test]
fn attached_markdown_files_stay_attachments() {
    let raw = "From: ada@example.org\r\nSubject: notes\r\nMIME-Version: 1.0\r\n\
               Content-Type: multipart/mixed; boundary=\"mix\"\r\n\r\n\
               --mix\r\nContent-Type: multipart/alternative; boundary=\"alt\"\r\n\r\n\
               --alt\r\nContent-Type: text/plain\r\n\r\nSee the notes.\r\n\
               --alt\r\nContent-Type: text/markdown\r\n\r\nSee the *notes*.\r\n\
               --alt--\r\n\
               --mix\r\nContent-Type: text/markdown; name=\"notes.md\"\r\n\
               Content-Disposition: attachment; filename=\"notes.md\"\r\n\r\n\
               # Notes\r\n\
               --mix--\r\n";
    let parsed = parse_message(raw.as_bytes());
    assert_eq!(parsed.text, "See the notes.");
    let names: Vec<_> = parsed
        .attachments
        .iter()
        .map(|a| (a.filename.as_deref(), a.mime.as_str()))
        .collect();
    assert_eq!(names, [(Some("notes.md"), "text/markdown")]);
    // The numbering stays the parser's, so the attachment can be opened.
    let index = parsed.attachments[0].index;
    let content = attachment_content(raw.as_bytes(), index).unwrap();
    assert_eq!(content.info.filename.as_deref(), Some("notes.md"));
    assert!(String::from_utf8_lossy(&content.data).contains("# Notes"));
}
