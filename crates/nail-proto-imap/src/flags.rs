//! Message flags: system flags, `\Deleted` and keywords.

use std::borrow::Cow;
use std::cmp::Ordering;

use nail_api::Flags;

use crate::error::{ImapError, Result};

/// One flag to set or clear with [`crate::ImapClient::store_flags`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Flag {
    /// `\Seen`.
    Seen,
    /// `\Answered`.
    Answered,
    /// `\Flagged`.
    Flagged,
    /// `\Deleted` (removed by the next expunge).
    Deleted,
    /// `\Draft`.
    Draft,
    /// A keyword such as `$Forwarded`, `$Junk` or a user tag. Build it with
    /// [`Flag::keyword`], which checks that it is a valid IMAP atom.
    Keyword(String),
}

impl Flag {
    /// A keyword flag; fails for names that are not valid IMAP atoms
    /// (spaces, parentheses, quotes, `\`, `%`, `*`, `]`, `{`, control or
    /// non-ASCII characters) or that start with `\`.
    pub fn keyword(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        if is_valid_keyword(&name) {
            Ok(Self::Keyword(name))
        } else {
            Err(ImapError::InvalidArgument(format!(
                "{name:?} is not a valid IMAP keyword"
            )))
        }
    }

    /// The flag as sent to the server (`\Seen`, `$Label1`).
    pub fn as_imap(&self) -> Cow<'_, str> {
        match self {
            Self::Seen => Cow::Borrowed("\\Seen"),
            Self::Answered => Cow::Borrowed("\\Answered"),
            Self::Flagged => Cow::Borrowed("\\Flagged"),
            Self::Deleted => Cow::Borrowed("\\Deleted"),
            Self::Draft => Cow::Borrowed("\\Draft"),
            Self::Keyword(name) => Cow::Borrowed(name),
        }
    }

    /// Checks a flag built without [`Flag::keyword`].
    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Keyword(name) if !is_valid_keyword(name) => Err(ImapError::InvalidArgument(
                format!("{name:?} is not a valid IMAP keyword"),
            )),
            _ => Ok(()),
        }
    }
}

fn is_valid_keyword(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('\\')
        && name.bytes().all(|b| {
            b.is_ascii_graphic()
                && !matches!(b, b'(' | b')' | b'{' | b'%' | b'*' | b'"' | b'\\' | b']')
        })
}

/// The flags of one message on the server: system flags, `\Deleted` and
/// keywords (sorted, without duplicates; keywords compare
/// case-insensitively as in IMAP).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageFlags {
    /// `\Seen`, `\Flagged`, `\Answered`, `\Draft`.
    pub flags: Flags,
    /// `\Deleted`: marked for removal by the next expunge.
    pub deleted: bool,
    /// Keywords (non-system flags such as `$Forwarded`).
    pub keywords: Vec<String>,
}

impl MessageFlags {
    /// Builds flags from IMAP flag atoms. `\Recent` and unknown system flags
    /// are ignored.
    pub fn from_imap<I, S>(atoms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut result = Self::default();
        for atom in atoms {
            let atom = atom.as_ref();
            match atom.to_ascii_lowercase().as_str() {
                "\\seen" => result.flags.seen = true,
                "\\flagged" => result.flags.flagged = true,
                "\\answered" => result.flags.answered = true,
                "\\draft" => result.flags.draft = true,
                "\\deleted" => result.deleted = true,
                other if other.starts_with('\\') => {}
                _ => result.keywords.push(atom.to_owned()),
            }
        }
        result.keywords.sort_by(|a, b| compare_keywords(a, b));
        result.keywords.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        result
    }

    /// The flags as IMAP atoms (for `FLAGS` replacement or `APPEND`).
    pub fn to_imap(&self) -> Vec<Flag> {
        let mut out = Vec::new();
        let system = [
            (self.flags.seen, Flag::Seen),
            (self.flags.answered, Flag::Answered),
            (self.flags.flagged, Flag::Flagged),
            (self.deleted, Flag::Deleted),
            (self.flags.draft, Flag::Draft),
        ];
        out.extend(system.into_iter().filter(|(on, _)| *on).map(|(_, f)| f));
        out.extend(self.keywords.iter().cloned().map(Flag::Keyword));
        out
    }

    /// Whether both describe the same server state (keywords compared
    /// case-insensitively, order ignored).
    pub fn same_as(&self, other: &Self) -> bool {
        self.flags == other.flags
            && self.deleted == other.deleted
            && self.keywords.len() == other.keywords.len()
            && self
                .keywords
                .iter()
                .zip(&other.keywords)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
    }

    /// Normalizes keyword order after manual construction.
    pub(crate) fn normalized(mut self) -> Self {
        self.keywords.sort_by(|a, b| compare_keywords(a, b));
        self.keywords.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        self
    }
}

fn compare_keywords(a: &str, b: &str) -> Ordering {
    a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
}

/// Formats a parenthesized flag list after validating every flag.
pub(crate) fn flag_list(flags: &[Flag]) -> Result<String> {
    let mut out = String::from("(");
    for (i, flag) in flags.iter().enumerate() {
        flag.validate()?;
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&flag.as_imap());
    }
    out.push(')');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes() {
        let flags = MessageFlags::from_imap([
            "\\Seen",
            "\\Recent",
            "\\Deleted",
            "work",
            "$Forwarded",
            "Work",
        ]);
        assert!(flags.flags.seen && flags.deleted && !flags.flags.flagged);
        assert_eq!(flags.keywords, ["$Forwarded", "work"]);
        let other = MessageFlags::from_imap(["\\Deleted", "\\SEEN", "$forwarded", "WORK"]);
        assert!(flags.same_as(&other));
        assert!(!flags.same_as(&MessageFlags::default()));
        let round_trip: Vec<String> = flags
            .to_imap()
            .iter()
            .map(|f| f.as_imap().into_owned())
            .collect();
        assert_eq!(round_trip, ["\\Seen", "\\Deleted", "$Forwarded", "work"]);
    }

    #[test]
    fn keywords_must_be_atoms() {
        assert!(Flag::keyword("$Label1").is_ok());
        assert!(Flag::keyword("project-x").is_ok());
        for bad in [
            "",
            "two words",
            "(x",
            "a\"b",
            "\\Seen",
            "ümlaut",
            "x]",
            "a*",
        ] {
            assert!(
                matches!(Flag::keyword(bad), Err(ImapError::InvalidArgument(_))),
                "{bad}"
            );
        }
        assert!(flag_list(&[Flag::Keyword("bad word".into())]).is_err());
        assert_eq!(
            flag_list(&[Flag::Seen, Flag::Keyword("$Junk".into())]).unwrap(),
            "(\\Seen $Junk)"
        );
        assert_eq!(flag_list(&[]).unwrap(), "()");
    }
}
