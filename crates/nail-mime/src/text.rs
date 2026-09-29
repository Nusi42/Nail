//! Plain-text helpers: previews.

/// Default maximum preview length in characters.
pub const PREVIEW_CHARS: usize = 200;

/// Builds a list preview from a plain-text body: quoted lines (`>`) and
/// signature blocks (after a `-- ` line) are skipped, whitespace is collapsed,
/// and the result is cut at a word boundary to at most `max_chars` characters.
pub fn preview_text(text: &str, max_chars: usize) -> String {
    let mut words = Vec::new();
    for line in text.lines() {
        if line == "-- " || line == "--" {
            break;
        }
        if line.trim_start().starts_with('>') {
            continue;
        }
        words.extend(line.split_whitespace());
    }

    let mut out = String::new();
    let mut len = 0;
    for word in words {
        let word_len = word.chars().count();
        let sep = usize::from(!out.is_empty());
        if len + sep + word_len > max_chars {
            if out.is_empty() {
                // A single overlong word: hard cut.
                out.extend(word.chars().take(max_chars));
            }
            break;
        }
        if sep == 1 {
            out.push(' ');
        }
        out.push_str(word);
        len += sep + word_len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_whitespace_and_skips_quotes() {
        let text = "Hi  Sam,\r\n\r\n> quoted\n  numbers\tare in.\n-- \nSig";
        assert_eq!(preview_text(text, 200), "Hi Sam, numbers are in.");
    }

    #[test]
    fn cuts_at_word_boundary() {
        assert_eq!(preview_text("alpha beta gamma", 10), "alpha beta");
        assert_eq!(preview_text("alpha beta gamma", 11), "alpha beta");
        assert_eq!(preview_text("supercalifragilistic", 5), "super");
        assert_eq!(preview_text("", 10), "");
        assert_eq!(preview_text("äöü äöü", 3), "äöü");
    }
}
