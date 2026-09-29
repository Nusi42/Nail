//! `text/plain; format=flowed` (RFC 3676) encoding.
//!
//! Long paragraphs are wrapped at 72 characters with soft line breaks (a
//! trailing space), so receiving clients can reflow them; quoted lines keep
//! their `>` depth on every wrapped line; lines that start with a space,
//! `>` (outside a quote) or `From ` are space-stuffed. The signature
//! separator `-- ` stays as it is.

const WIDTH: usize = 72;

/// Encodes text (LF or CRLF line ends) as format=flowed with LF line ends.
pub fn encode_flowed(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(text.len() + text.len() / 16);
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if line == "-- " {
            out.push_str(line);
            continue;
        }
        let depth = line.chars().take_while(|&c| c == '>').count();
        let rest = &line[depth..];
        let content = if depth > 0 {
            rest.strip_prefix(' ').unwrap_or(rest)
        } else {
            rest
        };
        // Trailing spaces would mark a soft break: a hard line has none.
        let content = content.trim_end_matches(' ');
        let prefix = if depth > 0 {
            format!("{} ", ">".repeat(depth))
        } else if content.starts_with(' ')
            || content.starts_with('>')
            || content.starts_with("From ")
        {
            " ".to_owned()
        } else {
            String::new()
        };
        let wrapped = wrap(
            content,
            WIDTH.saturating_sub(prefix.chars().count()).max(20),
        );
        let last = wrapped.len() - 1;
        for (i, piece) in wrapped.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            // Continuation lines of an unquoted paragraph need stuffing too.
            let stuff = i > 0
                && depth == 0
                && (piece.starts_with(' ') || piece.starts_with('>') || piece.starts_with("From "));
            if i > 0 && stuff && prefix.is_empty() {
                out.push(' ');
            }
            out.push_str(&prefix);
            out.push_str(piece);
            if i < last {
                out.push(' ');
            }
        }
    }
    out
}

/// Splits `line` into pieces of at most `width` characters at spaces (the
/// space that ends a piece is dropped; the caller adds it back as the soft
/// break). Words longer than `width` stay whole.
fn wrap(line: &str, width: usize) -> Vec<String> {
    if line.chars().count() <= width {
        return vec![line.to_owned()];
    }
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for word in line.split(' ') {
        let len = word.chars().count();
        if current_len > 0 && current_len + 1 + len > width {
            pieces.push(std::mem::take(&mut current));
            current_len = 0;
        }
        if current_len > 0 {
            current.push(' ');
            current_len += 1;
        }
        current.push_str(word);
        current_len += len;
    }
    pieces.push(current);
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// RFC 3676 decoding, for the round-trip checks.
    fn decode(text: &str) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut joining = false;
        for line in text.split('\n') {
            let depth = line.chars().take_while(|&c| c == '>').count();
            let mut content = &line[depth..];
            content = content.strip_prefix(' ').unwrap_or(content);
            let soft = line != "-- " && content.ends_with(' ');
            let content = if soft {
                &content[..content.len() - 1]
            } else {
                content
            };
            let quoted = format!("{}{}", ">".repeat(depth), if depth > 0 { " " } else { "" });
            if joining {
                let last = out.last_mut().unwrap();
                last.push(' ');
                last.push_str(content);
            } else {
                out.push(format!("{quoted}{content}"));
            }
            joining = soft;
        }
        out.join("\n")
    }

    #[test]
    fn wraps_long_paragraphs_with_soft_breaks() {
        let long = "word ".repeat(40).trim_end().to_owned();
        let encoded = encode_flowed(&format!("{long}\nshort"));
        for line in encoded.lines() {
            assert!(line.chars().count() <= 73, "{line:?}");
        }
        let lines: Vec<&str> = encoded.lines().collect();
        assert!(lines[0].ends_with(' '), "soft break: {:?}", lines[0]);
        assert_eq!(*lines.last().unwrap(), "short");
        assert_eq!(decode(&encoded), format!("{long}\nshort"));
    }

    #[test]
    fn quotes_keep_their_depth_and_stuffing_is_applied() {
        let long_quote = format!(">> {}", "quoted ".repeat(20).trim_end());
        let text = format!("From the start\n >indented\n{long_quote}\n-- \nAda  ");
        let encoded = encode_flowed(&text);
        let lines: Vec<&str> = encoded.lines().collect();
        assert_eq!(lines[0], " From the start");
        assert_eq!(lines[1], "  >indented");
        assert!(lines[2].starts_with(">> quoted") && lines[2].ends_with(' '));
        assert!(lines[3].starts_with(">> quoted"));
        assert!(encoded.contains("\n-- \nAda"), "{encoded:?}");
        assert!(encoded.ends_with("Ada"), "hard line has no trailing space");
        assert_eq!(
            decode(&encoded),
            format!("From the start\n >indented\n{long_quote}\n-- \nAda")
        );
    }

    #[test]
    fn long_words_stay_whole() {
        let url = format!("https://example.org/{}", "x".repeat(100));
        assert_eq!(encode_flowed(&url), url);
    }
}
