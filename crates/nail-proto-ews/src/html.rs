//! Plain text ↔ the simple HTML EWS stores for automatic replies and item
//! bodies (ADR 0072: replies are plain text in the contract; Outlook
//! formatting is lost once edited in Nail).

use crate::xml::escape;

/// Plain text as a small HTML document (lines become `<br>`).
pub fn text_to_html(text: &str) -> String {
    let lines: Vec<String> = text
        .replace("\r\n", "\n")
        .split('\n')
        .map(|l| escape(l).into_owned())
        .collect();
    format!(
        "<html><body><div>{}</div></body></html>",
        lines.join("<br>")
    )
}

/// The text of HTML: tags dropped, line breaks for `<br>` and block ends,
/// entities decoded, `<head>` / `<style>` / `<script>` skipped, blank
/// lines collapsed.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        push_text(&mut out, &rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            rest = "";
            break;
        };
        let tag = after[..end].trim();
        rest = &after[end + 1..];
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        let closing = tag.starts_with('/');
        if !closing && matches!(name.as_str(), "head" | "style" | "script" | "title") {
            let close = format!("</{name}");
            match rest.to_ascii_lowercase().find(&close) {
                Some(pos) => {
                    rest = &rest[pos..];
                    if let Some(gt) = rest.find('>') {
                        rest = &rest[gt + 1..];
                    }
                }
                None => rest = "",
            }
            continue;
        }
        match (name.as_str(), closing) {
            ("br", _) => out.push('\n'),
            (
                "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote",
                true,
            ) => {
                out.push('\n');
            }
            ("p", false) if !out.is_empty() && !out.ends_with('\n') => out.push('\n'),
            _ => {}
        }
    }
    push_text(&mut out, rest);
    let mut collapsed = String::new();
    let mut blank = 0;
    for line in out.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        collapsed.push_str(line);
        collapsed.push('\n');
    }
    collapsed.trim().to_owned()
}

fn push_text(out: &mut String, text: &str) {
    let text = text.replace(['\r', '\n'], " ");
    let mut rest = text.as_str();
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        match after.find(';').filter(|i| *i <= 10) {
            Some(semi) => {
                match decode_entity(&after[..semi]) {
                    Some(c) => out.push(c),
                    None => {
                        out.push('&');
                        out.push_str(&after[..=semi]);
                    }
                }
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
}

fn decode_entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips() {
        let text = "Out until Monday.\nUrgent: call <Bob> & Co.";
        let html = text_to_html(text);
        assert!(html.contains("&lt;Bob&gt; &amp; Co."));
        assert!(html.contains("<br>"));
        assert_eq!(html_to_text(&html), text);
    }

    #[test]
    fn outlook_html_becomes_text() {
        let html = concat!(
            "<html>\r\n<head><meta charset=\"utf-8\"><style>p {color:red}</style></head>",
            "<body><p>Hello&nbsp;there,</p><p>I am <b>away</b> &#8211; back on 5&#x2F;10.</p>",
            "<div><br></div><div>Ada</div><script>x()</script></body></html>"
        );
        assert_eq!(
            html_to_text(html),
            "Hello there,\nI am away \u{2013} back on 5/10.\n\nAda"
        );
        assert_eq!(html_to_text("a &unknown; b & c"), "a &unknown; b & c");
        assert_eq!(html_to_text("plain"), "plain");
        assert_eq!(html_to_text("<p>unclosed"), "unclosed");
    }
}
