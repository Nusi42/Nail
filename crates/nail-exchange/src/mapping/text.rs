//! Bodies of PIM items: Exchange keeps HTML (Graph `itemBody`, EWS
//! `Body`), Nail's events, tasks and contacts keep plain text. HTML is
//! turned into text (block elements and `<br>` become line breaks, tags
//! dropped, entities decoded, scripts and styles skipped); text going back
//! is written as plain text (the engines send it as such).

use nail_api::exchange::pim::ExchangeBody;

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let number = entity.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

const BLOCKS: &[&str] = &[
    "p",
    "div",
    "br",
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
    "ul",
    "ol",
];

/// Plain text of an HTML fragment.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut skip_until: Option<&str> = None;
    while !rest.is_empty() {
        if let Some(end) = skip_until {
            match rest.to_ascii_lowercase().find(end) {
                Some(at) => {
                    rest = &rest[at..];
                    skip_until = None;
                }
                None => break,
            }
        }
        match rest.find(['<', '&']) {
            None => {
                out.push_str(rest);
                break;
            }
            Some(at) => {
                out.push_str(&rest[..at]);
                rest = &rest[at..];
            }
        }
        if rest.starts_with('&') {
            let entity_end = rest[1..].find(';').filter(|e| *e <= 10);
            match entity_end.and_then(|e| decode_entity(&rest[1..=e]).map(|c| (c, e))) {
                Some((c, e)) => {
                    out.push(c);
                    rest = &rest[e + 2..];
                }
                None => {
                    out.push('&');
                    rest = &rest[1..];
                }
            }
            continue;
        }
        // A tag (or a comment).
        if rest.starts_with("<!--") {
            rest = rest.find("-->").map_or("", |e| &rest[e + 3..]);
            continue;
        }
        let Some(close) = rest.find('>') else {
            out.push_str(rest);
            break;
        };
        let tag = rest[1..close].trim().to_ascii_lowercase();
        rest = &rest[close + 1..];
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if !closing && (name == "script" || name == "style" || name == "head") {
            skip_until = Some(match name.as_str() {
                "script" => "</script",
                "style" => "</style",
                _ => "</head",
            });
            continue;
        }
        if BLOCKS.contains(&name.as_str()) && !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
    }
    let lines: Vec<&str> = out.lines().map(|l| l.trim_end()).collect();
    let mut text = lines.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.replace('\u{a0}', " ").trim().to_owned()
}

/// The text of an Exchange body (`None` for an empty one).
pub fn body_text(body: Option<&ExchangeBody>) -> Option<String> {
    let body = body?;
    let text = if body.html {
        html_to_text(&body.content)
    } else {
        body.content.replace("\r\n", "\n").trim().to_owned()
    };
    (!text.is_empty()).then_some(text)
}

/// A plain-text body for Exchange (`None` for no text).
pub fn exchange_body(text: Option<&str>) -> Option<ExchangeBody> {
    let text = text?.trim();
    (!text.is_empty()).then(|| ExchangeBody {
        html: false,
        content: text.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_text() {
        let html = "<html><head><style>p{color:red}</style></head><body>\
            <p>Hello&nbsp;<b>Ada</b> &amp; Bob</p><div>Line&#32;two<br>Line three</div>\
            <!-- comment --><script>alert(1)</script><p>&#x263A; &unknown; 5 &lt; 6</p></body></html>";
        assert_eq!(
            html_to_text(html),
            "Hello Ada & Bob\nLine two\nLine three\n\u{263a} &unknown; 5 < 6"
        );
        assert_eq!(html_to_text("plain"), "plain");
        assert_eq!(html_to_text("<p></p><p></p>"), "");
        assert_eq!(html_to_text("a < b"), "a < b");
    }

    #[test]
    fn bodies() {
        assert_eq!(
            body_text(Some(&ExchangeBody {
                html: true,
                content: "<p>Agenda</p><ul><li>One</li><li>Two</li></ul>".into()
            }))
            .as_deref(),
            Some("Agenda\nOne\nTwo")
        );
        assert_eq!(
            body_text(Some(&ExchangeBody {
                html: false,
                content: " a\r\nb ".into()
            }))
            .as_deref(),
            Some("a\nb")
        );
        assert_eq!(body_text(Some(&ExchangeBody::default())), None);
        assert_eq!(body_text(None), None);
        assert_eq!(exchange_body(Some("  ")), None);
        assert_eq!(
            exchange_body(Some("x")),
            Some(ExchangeBody {
                html: false,
                content: "x".into()
            })
        );
    }
}
