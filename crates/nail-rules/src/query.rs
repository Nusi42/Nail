//! The `query` condition: a query of the search language (parsed by
//! `nail-search`) evaluated against one message, without the index.
//!
//! Text terms match case-insensitively as substrings (several words as a
//! phrase, whitespace-insensitive); a value with `*` is a wildcard pattern
//! over a whole address, name, file name or word (`from:*@substack.com`).

use nail_api::FolderRole;
use nail_api::rules::AddressField;
use nail_search::query::{DateValue, FlagTerm, Query, SizeOp, TermKind, TextField};

use crate::message::{Env, MessageView};
use crate::text::glob_match;
use crate::time::days_from_civil;

/// Whether `message` matches `query`.
pub fn matches(query: &Query, message: &dyn MessageView, env: &Env<'_>) -> bool {
    match query {
        Query::All => true,
        Query::Term(term) => term_matches(&term.kind, message, env),
        Query::Not(inner) => !matches(inner, message, env),
        Query::And(items) => items.iter().all(|q| matches(q, message, env)),
        Query::Or(items) => items.iter().any(|q| matches(q, message, env)),
    }
}

fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether a term value hits one candidate text.
fn hit(value: &str, candidate: &str) -> bool {
    if value.contains('*') {
        glob_match(value, candidate.trim())
            || candidate
                .split_whitespace()
                .any(|word| glob_match(value, word.trim_matches(|c: char| "<>,;\"'()".contains(c))))
    } else {
        normalize(candidate).contains(&normalize(value))
    }
}

fn address_candidates(message: &dyn MessageView, field: AddressField) -> Vec<String> {
    message
        .addresses(field)
        .into_iter()
        .flat_map(|a| {
            let mut v = vec![a.email];
            v.extend(a.name);
            v
        })
        .collect()
}

fn text_candidates(message: &dyn MessageView, field: TextField) -> Vec<String> {
    let names = || {
        message
            .attachments()
            .into_iter()
            .filter_map(|a| a.filename)
            .collect::<Vec<_>>()
    };
    match field {
        TextField::From => address_candidates(message, AddressField::From),
        TextField::To => address_candidates(message, AddressField::To),
        TextField::Cc => address_candidates(message, AddressField::Cc),
        TextField::Subject => vec![message.subject()],
        TextField::Body => message.body_text().into_iter().collect(),
        TextField::Filename => names(),
        TextField::Any => {
            let mut all = vec![message.subject()];
            all.extend(address_candidates(message, AddressField::From));
            all.extend(address_candidates(message, AddressField::To));
            all.extend(address_candidates(message, AddressField::Cc));
            all.extend(message.body_text());
            all.extend(names());
            all
        }
    }
}

fn role_name(role: FolderRole) -> &'static str {
    match role {
        FolderRole::Inbox => "inbox",
        FolderRole::Sent => "sent",
        FolderRole::Drafts => "drafts",
        FolderRole::Archive => "archive",
        FolderRole::Junk => "junk",
        FolderRole::Trash => "trash",
        FolderRole::Other => "",
    }
}

/// When a date value starts, Unix seconds.
pub(crate) fn resolve_date(date: DateValue, env: &Env<'_>) -> i64 {
    match date {
        DateValue::Day { year, month, day } => {
            days_from_civil(year, month, day) * 86_400 - i64::from(env.utc_offset_secs)
        }
        DateValue::Ago { amount, unit } => env.now - i64::from(amount) * unit.seconds(),
    }
}

fn term_matches(kind: &TermKind, message: &dyn MessageView, env: &Env<'_>) -> bool {
    match kind {
        TermKind::Text { field, value, .. } => text_candidates(message, *field)
            .iter()
            .any(|c| hit(value, c)),
        TermKind::Is(flag) => {
            let f = message.flags();
            match flag {
                FlagTerm::Unread => !f.seen,
                FlagTerm::Read => f.seen,
                FlagTerm::Flagged => f.flagged,
                FlagTerm::Unflagged => !f.flagged,
                FlagTerm::Answered => f.answered,
                FlagTerm::Unanswered => !f.answered,
                FlagTerm::Draft => f.draft,
            }
        }
        TermKind::HasAttachment => !message.attachments().is_empty(),
        TermKind::Tag(v) => message
            .tags()
            .iter()
            .any(|t| t.eq_ignore_ascii_case(&v.text) || t.to_lowercase() == v.text.to_lowercase()),
        TermKind::Category(v) => message
            .gmail_category()
            .is_some_and(|c| c.eq_ignore_ascii_case(&v.text)),
        TermKind::Focus(v) => message
            .exchange_focus()
            .is_some_and(|c| c.eq_ignore_ascii_case(&v.text)),
        TermKind::Account(v) => {
            let wanted = v.text.to_lowercase();
            let own = message.account_id();
            env.mailbox.accounts().iter().any(|a| {
                let named = a.id.as_str().to_lowercase() == wanted
                    || a.name.to_lowercase() == wanted
                    || (!a.email.is_empty() && a.email.to_lowercase() == wanted);
                named && (&a.id == own || a.members.contains(own))
            }) || own.as_str().eq_ignore_ascii_case(&wanted)
        }
        TermKind::Folder(v) => env.mailbox.folder(message.folder_id()).is_some_and(|f| {
            let wanted = v.text.trim_matches('/');
            f.name.eq_ignore_ascii_case(wanted)
                || f.path.eq_ignore_ascii_case(wanted)
                || (f.role != FolderRole::Other && role_name(f.role).eq_ignore_ascii_case(wanted))
        }),
        TermKind::Before(date) => message.date() < resolve_date(*date, env),
        TermKind::After(date) => message.date() >= resolve_date(*date, env),
        TermKind::Size(op, n) => {
            let size = message.size();
            match op {
                SizeOp::Greater => size > *n,
                SizeOp::GreaterOrEqual => size >= *n,
                SizeOp::Less => size < *n,
                SizeOp::LessOrEqual => size <= *n,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{AccountInfo, Attachment, FolderInfo, MessageData, StaticMailbox};
    use nail_api::{Address, Flags};
    use nail_search::parse_query;

    fn mailbox() -> StaticMailbox {
        StaticMailbox {
            accounts: vec![
                AccountInfo {
                    id: "work".into(),
                    name: "Work".into(),
                    email: "me@work.example".into(),
                    members: vec![],
                },
                AccountInfo {
                    id: "all-mine".into(),
                    name: "Everything".into(),
                    email: String::new(),
                    members: vec!["work".into()],
                },
            ],
            folders: vec![FolderInfo {
                id: "f1".into(),
                account_id: "work".into(),
                path: "INBOX".into(),
                name: "INBOX".into(),
                role: FolderRole::Inbox,
            }],
        }
    }

    fn message() -> MessageData {
        MessageData {
            from: vec![Address {
                name: Some("Ada Writer".into()),
                email: "ada@substack.com".into(),
            }],
            to: vec![Address {
                name: None,
                email: "me@work.example".into(),
            }],
            subject: "Weekly   Rust news".into(),
            body: Some("Hello,\nthis week in Rust...".into()),
            attachments: vec![Attachment {
                filename: Some("report.pdf".into()),
                mime: "application/pdf".into(),
            }],
            size: 20_000,
            date: 1_788_998_400, // 2026-09-10T00:00Z
            flags: Flags {
                seen: false,
                flagged: true,
                answered: false,
                draft: false,
            },
            tags: vec!["Work".into()],
            gmail_category: Some("social".into()),
            ..MessageData::new("m1", "work", "f1")
        }
    }

    fn check(query: &str) -> bool {
        let m = mailbox();
        let env = Env {
            now: 1_789_000_000,
            utc_offset_secs: 0,
            mailbox: &m,
        };
        let q = parse_query(query).unwrap_or_else(|e| panic!("{query}: {e:?}"));
        matches(&q, &message(), &env)
    }

    #[test]
    fn text_terms() {
        assert!(check("from:*@substack.com"));
        assert!(check("from:ada"));
        assert!(check("from:\"ada writer\""));
        assert!(!check("from:*@example.com"));
        assert!(check("to:me@work.example"));
        assert!(!check("cc:anyone"));
        assert!(check("subject:\"weekly rust\""));
        assert!(check("body:\"this week\""));
        assert!(check("filename:*.pdf"));
        assert!(check("rust"));
        assert!(!check("python"));
    }

    #[test]
    fn structured_terms() {
        assert!(check("is:unread is:flagged is:unanswered"));
        assert!(!check("is:read"));
        assert!(!check("is:draft"));
        assert!(check("has:attachment"));
        assert!(check("tag:work"));
        assert!(!check("tag:home"));
        // M3b: Gmail categories.
        assert!(check("category:Social"));
        assert!(!check("category:promotions"));
        assert!(check(
            "account:work account:Everything account:me@work.example"
        ));
        assert!(!check("account:home"));
        assert!(check("folder:inbox"));
        assert!(!check("folder:Archive"));
        assert!(check("after:2026-09-10 before:2026-09-11"));
        assert!(!check("before:2026-09-10"));
        assert!(check("after:1d"));
        assert!(check("size>10k size<=20000"));
        assert!(!check("size>1m"));
    }

    #[test]
    fn operators() {
        assert!(check("from:*@substack.com OR from:*@example.com"));
        assert!(check("rust -python"));
        assert!(!check("NOT has:attachment"));
        assert!(check("(tag:home OR tag:work) is:flagged"));
        assert!(check(""));
    }
}
