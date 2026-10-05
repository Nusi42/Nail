//! `AttachmentInfo::shown_in_body` (ADR 0162): parts the HTML body shows
//! as inline images are flagged; every other part stays a listed attachment,
//! whatever its `Content-Disposition`.
#![cfg(test)]

use nail_mime::{attachment_content, inline_content_by_cid, parse_message};

/// A valid 1×1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/// A message of `content_type` (`multipart/<sub>; boundary="b"`) with an
/// HTML body `html` and the parts `parts` (`(headers, base64 body)`).
fn message(content_type: &str, html: &str, parts: &[(&str, &str)]) -> Vec<u8> {
    let mut out = format!(
        "From: a@example.com\r\nTo: b@example.com\r\nSubject: s\r\nMIME-Version: 1.0\r\nContent-Type: {content_type}\r\n\r\n--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n{html}\r\n"
    );
    out.extend(parts.iter().map(|(headers, body)| {
        format!("--b\r\n{headers}\r\nContent-Transfer-Encoding: base64\r\n\r\n{body}\r\n")
    }));
    out.push_str("--b--\r\n");
    out.into_bytes()
}

#[test]
fn a_referenced_cid_image_is_shown_in_the_body() {
    // Content-ID and reference differ in case and angle brackets.
    let raw = message(
        r#"multipart/related; boundary="b""#,
        r#"<p>Hi</p><img src="cid:Logo@Shop.Example" alt="Logo">"#,
        &[(
            "Content-Type: image/png\r\nContent-ID: <logo@shop.example>\r\nContent-Disposition: inline",
            PNG,
        )],
    );
    let parsed = parse_message(&raw);
    assert_eq!(parsed.attachments.len(), 1);
    let logo = &parsed.attachments[0];
    assert!(logo.inline);
    assert!(logo.shown_in_body);
    assert_eq!(logo.filename, None);
    assert!(!parsed.has_attachments(), "nothing left to list");
    // The download paths report the same.
    assert!(attachment_content(&raw, 0).unwrap().info.shown_in_body);
    assert!(
        inline_content_by_cid(&raw, "cid:logo@shop.example")
            .unwrap()
            .info
            .shown_in_body
    );
}

#[test]
fn an_inline_attachment_the_body_does_not_show_stays_listed() {
    let raw = message(
        r#"multipart/mixed; boundary="b""#,
        "<p>Photos from the trip</p>",
        &[(
            "Content-Type: image/png; name=\"photo.png\"\r\nContent-ID: <photo@x>\r\nContent-Disposition: inline; filename=\"photo.png\"",
            PNG,
        )],
    );
    let parsed = parse_message(&raw);
    assert_eq!(parsed.attachments.len(), 1);
    let photo = &parsed.attachments[0];
    assert!(photo.inline, "the header says inline");
    assert!(!photo.shown_in_body);
    assert!(parsed.has_attachments());
    assert!(!attachment_content(&raw, 0).unwrap().info.shown_in_body);
}

#[test]
fn an_attachment_disposition_image_the_body_refers_to_is_shown() {
    // Percent-encoded `@` in the reference (RFC 2392), next to a real attachment.
    let raw = message(
        r#"multipart/mixed; boundary="b""#,
        r#"<div style="background: url(cid:chart%40x)">x</div>"#,
        &[
            (
                "Content-Type: image/png\r\nContent-ID: <chart@x>\r\nContent-Disposition: attachment; filename=\"chart.png\"",
                PNG,
            ),
            (
                "Content-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"report.pdf\"",
                "JVBERi0xLjQK",
            ),
        ],
    );
    let parsed = parse_message(&raw);
    assert_eq!(parsed.attachments.len(), 2);
    let chart = &parsed.attachments[0];
    assert!(!chart.inline);
    assert!(chart.shown_in_body);
    assert!(!parsed.attachments[1].shown_in_body);
    assert!(parsed.has_attachments());
}

#[test]
fn references_the_reader_cannot_show_do_not_hide_a_part() {
    // A program referenced as an image, an image referenced only from a
    // script, and a link to an image: none of them shows in the body.
    let raw = message(
        r#"multipart/related; boundary="b""#,
        r#"<img src="cid:tool@x"><script>i.src="cid:pic@x"</script><a href="cid:pic@x">pic</a>"#,
        &[
            (
                "Content-Type: application/x-msdownload\r\nContent-ID: <tool@x>\r\nContent-Disposition: inline; filename=\"tool.exe\"",
                "TVqQAAMAAAAEAAAA",
            ),
            (
                "Content-Type: image/png\r\nContent-ID: <pic@x>\r\nContent-Disposition: inline",
                PNG,
            ),
        ],
    );
    let parsed = parse_message(&raw);
    assert_eq!(parsed.attachments.len(), 2);
    assert!(parsed.attachments.iter().all(|a| !a.shown_in_body));
    assert!(parsed.has_attachments());
}
