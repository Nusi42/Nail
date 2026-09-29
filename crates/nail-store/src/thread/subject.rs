//! Subject normalization for threading (JWZ step 5, "group by subject").
//!
//! [`normalize_subject`] strips reply / forward prefixes in the common
//! languages (`Re:`, `AW:`, `SV:`, `Antw:`, `Fwd:`, `WG:`, …, also with
//! counters like `Re[2]:` or `Re(3):`) and mailing-list tags (`[nail-dev]`),
//! repeatedly, so `"[list] Re: AW: Fwd: Plans"` becomes `"Plans"`.

/// What the stripped prefixes said about a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SubjectKind {
    /// No reply or forward prefix.
    #[default]
    Original,
    /// At least one reply prefix (`Re:`, `AW:`, …).
    Reply,
    /// A forward prefix (`Fwd:`, `WG:`, …) and no reply prefix before it.
    Forward,
}

impl SubjectKind {
    /// The value stored in `messages.subject_is_reply`.
    pub(crate) fn to_sql(self) -> i64 {
        match self {
            Self::Original => 0,
            Self::Reply => 1,
            Self::Forward => 2,
        }
    }
}

/// A subject split into its display form and grouping key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedSubject {
    /// The subject without prefixes, original case, whitespace collapsed.
    pub display: String,
    /// Lowercased [`Self::display`]; the subject-fallback grouping key.
    /// Empty for an empty subject (never grouped).
    pub key: String,
    /// Reply / forward / original.
    pub kind: SubjectKind,
}

const REPLY_PREFIXES: &[&str] = &["re", "aw", "sv", "vs", "antw", "ynt", "odp", "rif", "res"];
const FORWARD_PREFIXES: &[&str] = &["fwd", "fw", "wg", "tr", "enc", "rv", "vl", "doorst"];

/// Strips one prefix like `Re:`, `Re[2]:`, `Re (2):` or `AW :` from the
/// start of `s`. Returns the rest and whether it was a reply prefix.
fn strip_prefix(s: &str) -> Option<(&str, SubjectKind)> {
    let word_len = s
        .char_indices()
        .find(|(_, c)| !c.is_alphabetic())
        .map_or(s.len(), |(i, _)| i);
    if word_len == 0 {
        return None;
    }
    let word = s[..word_len].to_lowercase();
    let kind = if REPLY_PREFIXES.contains(&word.as_str()) {
        SubjectKind::Reply
    } else if FORWARD_PREFIXES.contains(&word.as_str()) {
        SubjectKind::Forward
    } else {
        return None;
    };
    let mut rest = s[word_len..].trim_start_matches(' ');
    // Optional counter: [2] or (2).
    if let Some(close) = match rest.as_bytes().first() {
        Some(b'[') => Some(']'),
        Some(b'(') => Some(')'),
        _ => None,
    } {
        let end = rest.find(close)?;
        if !rest[1..end].chars().all(|c| c.is_ascii_digit()) || end == 1 {
            return None;
        }
        rest = rest[end + 1..].trim_start_matches(' ');
    }
    // Full-width colon is common in CJK mail clients.
    let rest = rest
        .strip_prefix(':')
        .or_else(|| rest.strip_prefix('\u{ff1a}'))?;
    Some((rest, kind))
}

/// Strips a leading `[list-tag]` when text follows it.
fn strip_list_tag(s: &str) -> Option<&str> {
    let rest = s.strip_prefix('[')?;
    let end = rest.find(']')?;
    let after = rest[end + 1..].trim_start();
    (end > 0 && !after.is_empty()).then_some(after)
}

/// Normalizes a subject; see the [module docs](self).
pub fn normalize_subject(subject: &str) -> NormalizedSubject {
    let mut rest = subject.trim();
    let mut kind = SubjectKind::Original;
    loop {
        if let Some((after, k)) = strip_prefix(rest) {
            // The outermost prefix decides ("Re: Fwd: x" is a reply).
            if kind == SubjectKind::Original {
                kind = k;
            }
            rest = after.trim_start();
        } else if let Some(after) = strip_list_tag(rest) {
            rest = after;
        } else {
            break;
        }
    }
    let display = rest.split_whitespace().collect::<Vec<_>>().join(" ");
    let key = display.to_lowercase();
    NormalizedSubject { display, key, kind }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(s: &str) -> (String, SubjectKind) {
        let n = normalize_subject(s);
        (n.display, n.kind)
    }

    #[test]
    fn strips_reply_and_forward_prefixes() {
        assert_eq!(norm("Plans"), ("Plans".into(), SubjectKind::Original));
        assert_eq!(norm("Re: Plans"), ("Plans".into(), SubjectKind::Reply));
        assert_eq!(norm("RE: re: Plans"), ("Plans".into(), SubjectKind::Reply));
        assert_eq!(norm("AW: WG: Plans"), ("Plans".into(), SubjectKind::Reply));
        assert_eq!(
            norm("Fwd: Re: Plans"),
            ("Plans".into(), SubjectKind::Forward)
        );
        assert_eq!(norm("Re[2]: Plans"), ("Plans".into(), SubjectKind::Reply));
        assert_eq!(norm("Re (3): Plans"), ("Plans".into(), SubjectKind::Reply));
        assert_eq!(norm("SV : Plans"), ("Plans".into(), SubjectKind::Reply));
        assert_eq!(norm("Re：Plans"), ("Plans".into(), SubjectKind::Reply));
    }

    #[test]
    fn strips_list_tags_and_collapses_whitespace() {
        assert_eq!(
            norm("[nail-dev] Re:  Release   plan "),
            ("Release plan".into(), SubjectKind::Reply)
        );
        assert_eq!(
            norm("Re: [nail-dev] Plans"),
            ("Plans".into(), SubjectKind::Reply)
        );
        // A tag alone is the subject.
        assert_eq!(norm("[draft]"), ("[draft]".into(), SubjectKind::Original));
        assert_eq!(normalize_subject("[x] Re: HeLLo").key, "hello");
    }

    #[test]
    fn keeps_words_that_only_look_like_prefixes() {
        assert_eq!(
            norm("Reply needed"),
            ("Reply needed".into(), SubjectKind::Original)
        );
        assert_eq!(norm("Re2: x"), ("Re2: x".into(), SubjectKind::Original));
        assert_eq!(norm("Re[a]: x"), ("Re[a]: x".into(), SubjectKind::Original));
        assert_eq!(
            norm("Regarding: x"),
            ("Regarding: x".into(), SubjectKind::Original)
        );
        assert_eq!(norm(""), (String::new(), SubjectKind::Original));
        assert_eq!(norm("Re:"), (String::new(), SubjectKind::Reply));
        assert_eq!(normalize_subject("  ").key, "");
    }
}
