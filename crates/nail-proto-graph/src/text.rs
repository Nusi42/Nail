//! Small text conversions: plain text ↔ the simple HTML Exchange stores
//! for automatic replies (ADR 0072: replies are plain text in the
//! contract), and Graph date-times ↔ RFC 3339 UTC / Unix times.

/// Plain text as simple HTML: escaped, line breaks kept as `<br>`.
pub fn text_to_html(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len() + 32);
    out.push_str("<html><body><div>");
    let text = text.replace("\r\n", "\n");
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push_str("<br>");
        }
        for c in line.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                _ => out.push(c),
            }
        }
    }
    out.push_str("</div></body></html>");
    out
}

/// Elements that end a line.
const BLOCK: [&str; 14] = [
    "br",
    "p",
    "div",
    "li",
    "tr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "pre",
    "table",
];

/// HTML as plain text: tags dropped (`<br>` and block ends become line
/// breaks, `<script>` / `<style>` / `<head>` content dropped), entities
/// decoded, runs of blank lines and trailing spaces trimmed. Text that
/// does not look like HTML is returned as it is.
pub fn html_to_text(html: &str) -> String {
    if !html.contains('<') && !html.contains('&') {
        return html.trim().to_owned();
    }
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut skip_until: Option<&'static str> = None;
    while !rest.is_empty() {
        if let Some(end_tag) = skip_until {
            match find_ci(rest, end_tag) {
                Some(at) => {
                    rest = &rest[at..];
                    skip_until = None;
                }
                None => break,
            }
            continue;
        }
        match rest.find('<') {
            None => {
                out.push_str(&decode_entities(rest));
                break;
            }
            Some(at) => {
                out.push_str(&decode_entities(&rest[..at]));
                rest = &rest[at..];
                let Some(close) = rest.find('>') else {
                    break;
                };
                let tag = &rest[1..close];
                rest = &rest[close + 1..];
                if tag.starts_with("!--") {
                    if !tag.ends_with("--")
                        && let Some(end) = rest.find("-->")
                    {
                        rest = &rest[end + 3..];
                    }
                    continue;
                }
                let closing = tag.starts_with('/');
                let name: String = tag
                    .trim_start_matches('/')
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect::<String>()
                    .to_ascii_lowercase();
                if !closing {
                    skip_until = match name.as_str() {
                        "script" => Some("</script"),
                        "style" => Some("</style"),
                        "head" => Some("</head"),
                        "title" => Some("</title"),
                        _ => None,
                    };
                }
                let ends_line = name == "br" || (closing && BLOCK.contains(&name.as_str()));
                let starts_block = !closing
                    && matches!(name.as_str(), "p" | "div" | "li" | "tr")
                    && !out.is_empty()
                    && !out.ends_with('\n');
                if ends_line || starts_block {
                    out.push('\n');
                }
            }
        }
    }
    tidy(&out)
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack.to_ascii_lowercase().find(needle)
}

fn tidy(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut blank = 0;
    for line in text.replace('\u{a0}', " ").lines() {
        let line = line.trim_end().to_owned();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
            lines.push(String::new());
        } else {
            blank = 0;
            lines.push(line);
        }
    }
    lines.join("\n").trim_matches('\n').to_owned()
}

/// Decodes the HTML entities that matter for text (named basics and
/// numeric ones).
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest.find(';').filter(|e| *e <= 10);
        let decoded = end.and_then(|e| {
            let name = &rest[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A Graph wall time (`2026-10-05T09:00:00.0000000`) without fractions
/// (`2026-10-05T09:00:00`).
pub fn trim_fraction(date_time: &str) -> String {
    let base = date_time.split('.').next().unwrap_or(date_time);
    base.trim_end_matches('Z').chars().take(19).collect()
}

/// A Graph UTC time (`2026-10-05T07:00:00Z`, with or without fractions or
/// an offset) as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn to_utc(text: &str) -> Option<String> {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(
            t.with_timezone(&chrono::Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
        );
    }
    let wall = trim_fraction(text);
    chrono::NaiveDateTime::parse_from_str(&wall, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// A Graph UTC time as Unix seconds.
pub fn to_unix(text: &str) -> Option<i64> {
    let utc = to_utc(text)?;
    chrono::DateTime::parse_from_rfc3339(&utc)
        .ok()
        .map(|t| t.timestamp())
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn unix_to_utc(secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs, 0).map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_becomes_simple_html_and_back() {
        let text = "I am away until <Monday>.\nUrgent: ask Bob & Carol.\r\n\nAda";
        let html = text_to_html(text);
        assert_eq!(
            html,
            "<html><body><div>I am away until &lt;Monday&gt;.<br>Urgent: ask Bob &amp; Carol.<br><br>Ada</div></body></html>"
        );
        assert_eq!(html_to_text(&html), text.replace("\r\n", "\n"));
        assert_eq!(text_to_html(""), "");
        assert_eq!(html_to_text(""), "");
    }

    #[test]
    fn outlook_html_reads_as_text() {
        let html = "<html><head><meta charset=\"utf-8\"><style>p{margin:0}</style></head>\
            <body><!-- note --><p>Hello&nbsp;there,</p><p>back on <b>Monday</b>&#8212;thanks&#x21;</p>\
            <ul><li>one</li><li>two</li></ul><script>alert(1)</script></body></html>";
        assert_eq!(
            html_to_text(html),
            "Hello there,\nback on Monday\u{2014}thanks!\none\ntwo"
        );
        assert_eq!(html_to_text("plain text\n"), "plain text");
        assert_eq!(html_to_text("a &unknown; b & c"), "a &unknown; b & c");
        assert_eq!(html_to_text("<div>x</div>\n\n\n<div>y</div>"), "x\n\ny");
        assert_eq!(html_to_text("unclosed <b"), "unclosed");
    }

    #[test]
    fn times() {
        assert_eq!(
            trim_fraction("2026-10-05T09:00:00.0000000"),
            "2026-10-05T09:00:00"
        );
        assert_eq!(trim_fraction("2026-10-05T09:00:00Z"), "2026-10-05T09:00:00");
        assert_eq!(
            to_utc("2026-10-05T09:00:00+02:00").as_deref(),
            Some("2026-10-05T07:00:00Z")
        );
        assert_eq!(
            to_utc("2026-10-05T07:00:00.1234567").as_deref(),
            Some("2026-10-05T07:00:00Z")
        );
        assert_eq!(to_utc("soon"), None);
        assert_eq!(to_unix("1970-01-01T00:01:00Z"), Some(60));
        assert_eq!(unix_to_utc(60).as_deref(), Some("1970-01-01T00:01:00Z"));
    }
}
