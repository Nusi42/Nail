//! Shared fixtures: two real accounts, a virtual account over both, their
//! folders, and a builder for messages.

use nail_api::rules::Rule;
use nail_api::{Address, Flags, FolderRole};
use nail_rules::{AccountInfo, Attachment, Env, FolderInfo, MessageData, StaticMailbox};

/// 2026-09-29T12:00:00Z.
pub(crate) const NOW: i64 = 1_790_683_200;
pub(crate) const DAY: i64 = 86_400;

pub(crate) fn mailbox() -> StaticMailbox {
    let folder = |id: &str, account: &str, path: &str, role| FolderInfo {
        id: id.into(),
        account_id: account.into(),
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        role,
    };
    StaticMailbox {
        accounts: vec![
            AccountInfo {
                id: "work".into(),
                name: "Work".into(),
                email: "me@work.example".into(),
                members: vec![],
            },
            AccountInfo {
                id: "home".into(),
                name: "Home".into(),
                email: "me@home.example".into(),
                members: vec![],
            },
            AccountInfo {
                id: "everything".into(),
                name: "Everything".into(),
                email: String::new(),
                members: vec!["work".into(), "home".into()],
            },
        ],
        folders: vec![
            folder("w-inbox", "work", "INBOX", FolderRole::Inbox),
            folder("w-archive", "work", "Archive", FolderRole::Archive),
            folder("w-trash", "work", "Trash", FolderRole::Trash),
            folder("w-reading", "work", "Reading", FolderRole::Other),
            folder("w-rust", "work", "Lists/Rust", FolderRole::Other),
            folder("h-inbox", "home", "INBOX", FolderRole::Inbox),
            folder("h-reading", "home", "Reading", FolderRole::Other),
        ],
    }
}

pub(crate) fn env(mailbox: &StaticMailbox) -> Env<'_> {
    Env {
        now: NOW,
        utc_offset_secs: 0,
        mailbox,
    }
}

pub(crate) fn addr(name: Option<&str>, email: &str) -> Address {
    Address {
        name: name.map(str::to_owned),
        email: email.into(),
    }
}

/// A newsletter from Substack in the work inbox, a day old.
pub(crate) fn newsletter() -> MessageData {
    MessageData {
        headers: vec![
            (
                "Authentication-Results".into(),
                "mx.work.example; dkim=pass header.d=substack.com; spf=softfail (sender IP) smtp.mailfrom=substack.com; dmarc=pass".into(),
            ),
            (
                "Authentication-Results".into(),
                "forged.example; dkim=fail".into(),
            ),
            (
                "List-Id".into(),
                "Rust Weekly <rust-weekly.substack.com>".into(),
            ),
            ("Precedence".into(), "bulk".into()),
            ("X-Mailer".into(), "Substack".into()),
        ],
        from: vec![addr(Some("Ada Writer"), "ada@substack.com")],
        to: vec![addr(None, "me@work.example")],
        cc: vec![addr(Some("Team"), "team@work.example")],
        reply_to: vec![addr(None, "noreply@substack.com")],
        sender: vec![addr(None, "bounce@substack.com")],
        subject: "[Rust Weekly] Issue #42".into(),
        body: Some("Hello!\nThis week in Rust: async closures.\nUnsubscribe here.".into()),
        attachments: vec![Attachment {
            filename: Some("Issue-42.PDF".into()),
            mime: "application/pdf".into(),
        }],
        size: 150_000,
        date: NOW - DAY,
        flags: Flags::default(),
        tags: vec!["Newsletter-Old".into()],
        ..MessageData::new("m-news", "work", "w-inbox")
    }
}

/// A personal mail from the boss in the work inbox, 40 days old, read.
pub(crate) fn from_boss() -> MessageData {
    MessageData {
        headers: vec![(
            "Authentication-Results".into(),
            "mx.work.example; dkim=pass; spf=pass; dmarc=pass".into(),
        )],
        from: vec![addr(Some("The Boss"), "boss@work.example")],
        to: vec![addr(Some("Me"), "me@work.example")],
        subject: "Quarterly plan".into(),
        body: Some("Please review.".into()),
        size: 2_000,
        date: NOW - 40 * DAY,
        flags: Flags {
            seen: true,
            ..Flags::default()
        },
        ..MessageData::new("m-boss", "work", "w-inbox")
    }
}

/// A mail in the home account's inbox, without body or headers.
pub(crate) fn at_home() -> MessageData {
    MessageData {
        from: vec![addr(None, "friend@mail.example")],
        to: vec![addr(None, "me@home.example")],
        subject: "Dinner?".into(),
        size: 800,
        date: NOW - 3_600,
        ..MessageData::new("m-home", "home", "h-inbox")
    }
}

/// Parses one rule written in rules.kdl syntax.
pub(crate) fn rule(text: &str) -> Rule {
    nail_config::rules::parse_rule_text(text).unwrap_or_else(|errors| {
        panic!(
            "{}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

/// A rule with an arrival trigger and these `when` / `then` bodies.
pub(crate) fn arrival(when: &str, then: &str) -> Rule {
    rule(&format!(
        "rule \"test\" trigger=\"arrival\" {{\n  when {{\n{when}\n  }}\n  then {{\n{then}\n  }}\n}}\n"
    ))
}
