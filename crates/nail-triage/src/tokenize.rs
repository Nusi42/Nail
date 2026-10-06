//! The junk classifier's tokenizer.
//!
//! Input: selected, already decoded headers (RFC 2047 words decoded by the
//! caller, `nail-mime`) and the decoded text of the message (the plain
//! text part, or the text of the HTML part). Output: the message's
//! distinct tokens, in first-seen order.
//!
//! Tokens are namespaced so the same word means different things in
//! different places:
//!
//! * `<header>:<word>` for words of a selected header (`subject:cheap`),
//!   plus `<header>:addr:<address>` and `<header>:domain:<domain>` for the
//!   addresses of address headers;
//! * plain `<word>` for words of the body text (lowercased, UAX #29 word
//!   boundaries; words longer than [`Tokenizer::max_word_len`] become
//!   `skip:<first char>:<length bucket>`, long numbers `num:<digits>`);
//! * `url:host:<host>`, `url:domain:<registrable part>`, `url:path:<word>`,
//!   `url:ip` and `url:insecure` for links found in the text.
//!
//! The store hashes every token before writing it ([ADR 0043]); nothing
//! here keeps text of the mail around.

use std::collections::HashSet;

use unicode_segmentation::UnicodeSegmentation;

/// Headers the tokenizer looks at (lowercase), and whether they carry
/// addresses.
pub const TOKEN_HEADERS: &[(&str, bool)] = &[
    ("from", true),
    ("sender", true),
    ("reply-to", true),
    ("return-path", true),
    ("to", true),
    ("cc", true),
    ("subject", false),
    ("list-id", false),
    ("x-mailer", false),
    ("user-agent", false),
    ("content-type", false),
    ("x-spam-flag", false),
    ("x-spam-status", false),
];

/// Tokenizer limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokenizer {
    /// Words shorter than this (in characters) are ignored, except words of
    /// scripts written without spaces (CJK), which count from one
    /// character.
    pub min_word_len: usize,
    /// Words longer than this (in characters) become a `skip:` token.
    pub max_word_len: usize,
    /// At most this many distinct tokens per message.
    pub max_tokens: usize,
    /// At most this many bytes of body text are read.
    pub max_text_bytes: usize,
}

impl Default for Tokenizer {
    fn default() -> Self {
        Self {
            min_word_len: 3,
            max_word_len: 24,
            max_tokens: 3000,
            max_text_bytes: 256 * 1024,
        }
    }
}

/// The parts of a message the tokenizer reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenSource<'a> {
    /// Decoded headers `(name, value)`, any case; only
    /// [`TOKEN_HEADERS`] are used.
    pub headers: &'a [(String, String)],
    /// Decoded body text.
    pub text: &'a str,
}

struct Collector {
    seen: HashSet<String>,
    out: Vec<String>,
    max: usize,
}

impl Collector {
    fn push(&mut self, token: String) {
        if self.out.len() < self.max && !token.is_empty() && self.seen.insert(token.clone()) {
            self.out.push(token);
        }
    }

    fn full(&self) -> bool {
        self.out.len() >= self.max
    }
}

impl Tokenizer {
    /// The distinct tokens of a message, in first-seen order (headers
    /// first), at most [`Self::max_tokens`].
    pub fn tokens(&self, source: TokenSource<'_>) -> Vec<String> {
        let mut c = Collector {
            seen: HashSet::new(),
            out: Vec::new(),
            max: self.max_tokens,
        };
        for (name, value) in source.headers {
            let name = name.trim().to_ascii_lowercase();
            let Some(&(name, addresses)) = TOKEN_HEADERS.iter().find(|(n, _)| *n == name) else {
                continue;
            };
            if addresses {
                for addr in extract_addresses(value) {
                    if let Some((_, domain)) = addr.rsplit_once('@') {
                        c.push(format!("{name}:domain:{domain}"));
                    }
                    c.push(format!("{name}:addr:{addr}"));
                }
            }
            for word in self.words(value) {
                c.push(format!("{name}:{word}"));
            }
            if c.full() {
                return c.out;
            }
        }
        let text = truncate_at_char(source.text, self.max_text_bytes);
        for chunk in text.split_whitespace() {
            for token in url_tokens(chunk) {
                c.push(token);
            }
        }
        for word in self.words(text) {
            c.push(word);
            if c.full() {
                break;
            }
        }
        c.out
    }

    /// The word tokens of a text (not deduplicated).
    fn words<'t>(&'t self, text: &'t str) -> impl Iterator<Item = String> + 't {
        text.unicode_words()
            .filter_map(|word| self.word_token(word))
    }

    fn word_token(&self, word: &str) -> Option<String> {
        let lower = word.to_lowercase();
        let chars = lower.chars().count();
        if lower.chars().all(|ch| ch.is_ascii_digit()) {
            // Short numbers (years, prices) are words; long ones (order
            // numbers, phone numbers) only count by length.
            return Some(if chars <= 4 {
                lower
            } else {
                format!("num:{}", chars.min(20))
            });
        }
        let spaceless = lower.chars().any(is_spaceless_script);
        if chars < self.min_word_len && !spaceless {
            return None;
        }
        if chars > self.max_word_len {
            let first = lower.chars().next()?;
            return Some(format!("skip:{first}:{}", chars / 10 * 10));
        }
        Some(lower)
    }
}

/// Characters of scripts written without spaces between words (CJK, Thai
/// and friends); UAX #29 yields them one by one.
fn is_spaceless_script(ch: char) -> bool {
    matches!(ch as u32,
        0x0E00..=0x0EFF      // Thai, Lao
        | 0x1000..=0x109F    // Myanmar
        | 0x1780..=0x17FF    // Khmer
        | 0x2E80..=0x9FFF    // CJK radicals … unified ideographs, kana
        | 0xAC00..=0xD7AF    // Hangul syllables
        | 0xF900..=0xFAFF    // CJK compatibility ideographs
        | 0x20000..=0x3FFFF) // CJK extensions
}

fn truncate_at_char(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Addresses (`local@domain`, lowercased) in a header value.
fn extract_addresses(value: &str) -> Vec<String> {
    value
        .split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | ',' | ';' | '"' | '(' | ')'))
        .filter_map(|part| {
            let part = part.trim_matches(|c: char| matches!(c, '.' | ':' | '\''));
            let (local, domain) = part.split_once('@')?;
            (!local.is_empty() && domain.contains('.') && !domain.contains('@'))
                .then(|| part.to_lowercase())
        })
        .collect()
}

/// Link tokens of one whitespace-free chunk of text.
fn url_tokens(chunk: &str) -> Vec<String> {
    let lower = chunk.to_lowercase();
    let (rest, insecure) = if let Some(i) = lower.find("https://") {
        (&lower[i + 8..], false)
    } else if let Some(i) = lower.find("http://") {
        (&lower[i + 7..], true)
    } else if let Some(i) = lower.find("www.") {
        (&lower[i..], false)
    } else {
        return Vec::new();
    };
    let end = rest
        .find(['/', '?', '#', '"', '\'', '>', '<', ')', ']'])
        .unwrap_or(rest.len());
    let authority = &rest[..end];
    // Drop user info and port.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host).trim_end_matches('.');
    if host.is_empty() || !host.contains('.') {
        return Vec::new();
    }
    let mut out = Vec::new();
    if insecure {
        out.push("url:insecure".to_owned());
    }
    if host
        .split('.')
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        out.push("url:ip".to_owned());
    } else {
        out.push(format!("url:host:{host}"));
        let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
        if labels.len() > 2 {
            out.push(format!(
                "url:domain:{}.{}",
                labels[labels.len() - 2],
                labels[labels.len() - 1]
            ));
        }
    }
    for word in rest[end..].unicode_words() {
        let chars = word.chars().count();
        if (3..=24).contains(&chars) && !word.chars().all(|c| c.is_ascii_digit()) {
            out.push(format!("url:path:{word}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::fmt::Write as _;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn tokens_of(h: &[(&str, &str)], text: &str) -> Vec<String> {
        let h = headers(h);
        Tokenizer::default().tokens(TokenSource { headers: &h, text })
    }

    #[test]
    fn headers_are_namespaced_and_filtered() {
        let t = tokens_of(
            &[
                ("From", "\"Cheap Pills\" <Sales@Pharma.Example>"),
                ("Subject", "CHEAP pills now"),
                ("X-Unrelated", "ignored words here"),
                ("List-Id", "<news.list.example>"),
            ],
            "",
        );
        assert_eq!(
            t,
            [
                "from:domain:pharma.example",
                "from:addr:sales@pharma.example",
                "from:cheap",
                "from:pills",
                "from:sales",
                "from:pharma.example",
                "subject:cheap",
                "subject:pills",
                "subject:now",
                "list-id:news.list.example",
            ]
        );
    }

    #[test]
    fn body_words_numbers_and_long_words() {
        let t = tokens_of(
            &[],
            "Buy NOW, buy now: 2026 offers! Order 1234567 at 50% off. \
             Supercalifragilisticexpialidocious ok",
        );
        assert_eq!(
            t,
            [
                "buy",
                "now",
                "2026",
                "offers",
                "order",
                "num:7",
                "50",
                "off",
                "skip:s:30",
            ]
        );
    }

    #[test]
    fn urls_become_host_domain_and_path_tokens() {
        let t = tokens_of(
            &[],
            "Click http://login.bank.example.com/verify?id=1 or https://192.168.0.1/x \
             and www.shop.example",
        );
        for want in [
            "url:insecure",
            "url:host:login.bank.example.com",
            "url:domain:example.com",
            "url:path:verify",
            "url:ip",
            "url:host:www.shop.example",
            "url:domain:shop.example",
            "click",
        ] {
            assert!(t.contains(&want.to_owned()), "{want} missing in {t:?}");
        }
        // Not a link: no host with a dot.
        assert_eq!(url_tokens("http://localhost/x").len(), 0);
        assert_eq!(url_tokens("plain").len(), 0);
        assert_eq!(
            url_tokens("<https://user:pw@Mail.Example.org:8443/a>"),
            ["url:host:mail.example.org", "url:domain:example.org"]
        );
    }

    #[test]
    fn cjk_words_count_from_one_character() {
        let t = tokens_of(&[], "免费 发票 ok");
        assert!(
            t.contains(&"免".to_owned()) || t.contains(&"免费".to_owned()),
            "{t:?}"
        );
        assert!(!t.contains(&"ok".to_owned()));
    }

    #[test]
    fn limits_hold() {
        let text: String = (0..10_000).fold(String::new(), |mut s, i| {
            write!(s, "word{i} ").unwrap();
            s
        });
        let tok = Tokenizer {
            max_tokens: 100,
            ..Tokenizer::default()
        };
        assert_eq!(
            tok.tokens(TokenSource {
                headers: &[],
                text: &text
            })
            .len(),
            100
        );
        let tok = Tokenizer {
            max_text_bytes: 11,
            ..Tokenizer::default()
        };
        assert_eq!(
            tok.tokens(TokenSource {
                headers: &[],
                text: "alpha beta gamma delta"
            }),
            ["alpha", "beta"]
        );
        assert_eq!(truncate_at_char("äöü", 3), "ä");
    }

    fn arb_text() -> impl Strategy<Value = String> {
        prop_oneof![
            ".{0,200}",
            "[a-zA-Z0-9 .,:/@<>?=_-]{0,200}",
            "(https?://[a-z0-9.]{1,20}/[a-z/]{0,20} |[a-zA-Z]{1,30} |[0-9]{1,12} |\\n)*",
        ]
    }

    proptest! {
        #[test]
        fn tokens_are_well_formed(text in arb_text(), subject in arb_text()) {
            let tok = Tokenizer::default();
            let h = headers(&[("Subject", &subject), ("From", &subject)]);
            let t = tok.tokens(TokenSource { headers: &h, text: &text });
            let unique: HashSet<&String> = t.iter().collect();
            prop_assert_eq!(unique.len(), t.len());
            prop_assert!(t.len() <= tok.max_tokens);
            for token in &t {
                prop_assert!(!token.is_empty());
                prop_assert!(!token.chars().any(char::is_whitespace), "{token:?}");
                prop_assert_eq!(&token.to_lowercase(), token);
            }
            // Deterministic.
            prop_assert_eq!(&t, &tok.tokens(TokenSource { headers: &h, text: &text }));
        }

        #[test]
        fn body_tokens_are_bounded(text in arb_text()) {
            let tok = Tokenizer::default();
            for token in tok.tokens(TokenSource { headers: &[], text: &text }) {
                if token.starts_with("url:") {
                    continue;
                }
                prop_assert!(token.chars().count() <= tok.max_word_len.max(10), "{token:?}");
            }
        }

        #[test]
        fn concatenation_is_the_union(a in arb_text(), b in arb_text()) {
            let tok = Tokenizer::default();
            let set = |text: &str| -> HashSet<String> {
                tok.tokens(TokenSource { headers: &[], text }).into_iter().collect()
            };
            let joined = format!("{a}\n{b}");
            let union: HashSet<String> = set(&a).union(&set(&b)).cloned().collect();
            prop_assert_eq!(set(&joined), union);
        }

        #[test]
        fn ascii_case_does_not_matter(text in "[a-zA-Z0-9 ./:]{0,200}") {
            let tok = Tokenizer::default();
            let t = |s: &str| tok.tokens(TokenSource { headers: &[], text: s });
            prop_assert_eq!(t(&text.to_ascii_uppercase()), t(&text.to_ascii_lowercase()));
        }
    }
}
