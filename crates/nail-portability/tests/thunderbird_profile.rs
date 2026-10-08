//! The committed synthetic Thunderbird profile (`tests/fixtures/thunderbird`,
//! see `tests/fixtures/README.md`): found from `profiles.ini`, previewed
//! and imported like the core does it (WP125's Playwright scenario uses the
//! same fixture).
#![cfg(test)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nail_api::accounts::{AuthConfig, IncomingServer, OutgoingServer, TlsMode};
use nail_api::domain::FolderRole;
use nail_api::portability::{
    ImportCollectionKind, ImportFormat, ImportOptions, ImportSource, IssueSeverity, JobKind,
    JobReport, LOCAL_FOLDERS_ACCOUNT_ID, PortabilityIssueCode,
};
use nail_api::rules::{Condition, FolderRef, RuleAction, RuleTrigger};
use nail_portability::job::finish_report;
use nail_portability::testing::MemorySink;
use nail_portability::{JobContext, import, thunderbird};
use pretty_assertions::assert_eq;
use rusqlite::Connection;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/thunderbird")
}

fn profile_dir() -> PathBuf {
    data_dir().join("Profiles").join("x7k2m9qa.default-release")
}

// ---------------------------------------------------------------------------
// The SQLite address books of the fixture (binary files, generated)

type Cards = &'static [(&'static str, &'static [(&'static str, &'static str)])];
type Lists = &'static [(&'static str, &'static str, &'static [&'static str])];

const ABOOK_CARDS: Cards = &[
    (
        "4f0c1f6e-0001-4000-8000-000000000001",
        &[
            ("DisplayName", "Charles Babbage"),
            ("FirstName", "Charles"),
            ("LastName", "Babbage"),
            ("PrimaryEmail", "charles@example.org"),
            ("WorkPhone", "+44 20 0000 0001"),
            ("Company", "Analytical Engines Ltd."),
        ],
    ),
    (
        "4f0c1f6e-0001-4000-8000-000000000002",
        &[(
            "_vCard",
            "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Mary Somerville\r\nN:Somerville;Mary;;;\r\nEMAIL;PREF=1:mary@example.org\r\nNOTE:Stored as a vCard (Thunderbird 102+)\r\nEND:VCARD\r\n",
        )],
    ),
];
const ABOOK_LISTS: Lists = &[(
    "4f0c1f6e-0002-4000-8000-000000000001",
    "Engine friends",
    &[
        "4f0c1f6e-0001-4000-8000-000000000001",
        "4f0c1f6e-0001-4000-8000-000000000002",
    ],
)];
const HISTORY_CARDS: Cards = &[(
    "4f0c1f6e-0003-4000-8000-000000000001",
    &[("DisplayName", "Bob"), ("PrimaryEmail", "bob@example.net")],
)];

/// What a book holds: card properties and lists (uid, name, members).
type BookContent = (
    BTreeMap<String, BTreeMap<String, String>>,
    Vec<(String, String, Vec<String>)>,
);

fn write_book(path: &Path, cards: Cards, lists: Lists) {
    drop(std::fs::remove_file(path));
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "PRAGMA journal_mode = DELETE;
             CREATE TABLE properties (card TEXT, name TEXT, value TEXT);
             CREATE TABLE lists (uid TEXT PRIMARY KEY, name TEXT, nickName TEXT, description TEXT);
             CREATE TABLE list_cards (list TEXT, card TEXT, PRIMARY KEY(list, card));
             CREATE INDEX properties_card ON properties(card);
             CREATE INDEX properties_name ON properties(name);",
        )
        .unwrap();
    for (card, properties) in cards {
        for (name, value) in *properties {
            connection
                .execute(
                    "INSERT INTO properties VALUES (?1, ?2, ?3)",
                    [card, name, value],
                )
                .unwrap();
        }
    }
    for (uid, name, members) in lists {
        connection
            .execute("INSERT INTO lists VALUES (?1, ?2, '', '')", [uid, name])
            .unwrap();
        for member in *members {
            connection
                .execute("INSERT INTO list_cards VALUES (?1, ?2)", [uid, member])
                .unwrap();
        }
    }
}

fn read_book(path: &Path) -> BookContent {
    let (cards, lists) = thunderbird::abook::read_tables(path).unwrap();
    (
        cards,
        lists
            .into_iter()
            .map(|l| (l.uid, l.name, l.members))
            .collect(),
    )
}

fn expected(cards: Cards, lists: Lists) -> BookContent {
    (
        cards
            .iter()
            .map(|(card, properties)| {
                (
                    (*card).to_owned(),
                    properties
                        .iter()
                        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                        .collect(),
                )
            })
            .collect(),
        lists
            .iter()
            .map(|(uid, name, members)| {
                (
                    (*uid).to_owned(),
                    (*name).to_owned(),
                    members.iter().map(|m| (*m).to_owned()).collect(),
                )
            })
            .collect(),
    )
}

/// The committed `abook.sqlite` / `history.sqlite` hold exactly the cards
/// above. `NAIL_REGENERATE_FIXTURES=1 cargo test -p nail-portability
/// --test thunderbird_profile` writes them again.
#[test]
fn the_fixture_address_books_are_current() {
    let books = [
        ("abook.sqlite", ABOOK_CARDS, ABOOK_LISTS),
        ("history.sqlite", HISTORY_CARDS, &[]),
    ];
    let regenerate = std::env::var_os("NAIL_REGENERATE_FIXTURES").is_some();
    for (file, cards, lists) in books {
        let path = profile_dir().join(file);
        if regenerate {
            write_book(&path, cards, lists);
        }
        assert!(
            path.is_file(),
            "{file} is missing: run with NAIL_REGENERATE_FIXTURES=1"
        );
        assert_eq!(
            read_book(&path),
            expected(cards, lists),
            "{file} is out of date: run with NAIL_REGENERATE_FIXTURES=1"
        );
    }
}

// ---------------------------------------------------------------------------
// The core's side, in memory

fn source() -> ImportSource {
    ImportSource {
        format: ImportFormat::ThunderbirdProfile,
        path: profile_dir().display().to_string(),
    }
}

#[test]
fn the_profile_is_found() {
    let found = thunderbird::find_profiles(&data_dir());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label, "default-release");
    assert!(found[0].is_default);
    assert_eq!(found[0].source.format, ImportFormat::ThunderbirdProfile);
    assert_eq!(PathBuf::from(&found[0].source.path), profile_dir());
}

#[test]
fn the_preview_lists_folders_books_accounts_and_filters() {
    let preview = import::preview(&source()).unwrap();
    let folders: Vec<(&str, u64)> = preview
        .folders
        .iter()
        .map(|f| (f.path.as_str(), f.messages))
        .collect();
    assert_eq!(
        folders,
        vec![
            ("ada@example.net", 0),
            ("ada@example.net/Inbox", 1),
            ("Local Folders", 0),
            ("Local Folders/Archives", 0),
            ("Local Folders/Archives/2024", 1),
            ("Local Folders/Inbox", 3),
            ("Local Folders/Trash", 0),
            ("Local Folders/Unsent Messages", 0),
        ]
    );
    let books: Vec<(&str, &str, u64)> = preview
        .collections
        .iter()
        .map(|c| (c.key.as_str(), c.name.as_str(), c.items))
        .collect();
    assert_eq!(
        books,
        vec![
            ("history.sqlite", "history", 1),
            ("old.mab", "Old address book", 2),
            ("abook.sqlite", "abook", 3),
        ]
    );
    assert!(
        preview
            .collections
            .iter()
            .all(|c| c.kind == ImportCollectionKind::AddressBook)
    );

    // Accounts: IMAP and POP3 proposed, the feed account not.
    assert_eq!(preview.accounts.len(), 2);
    let work = &preview.accounts[0];
    assert_eq!(work.key, "account1");
    assert_eq!(work.config.name, "Work");
    assert_eq!(work.config.email, "ada@example.org");
    assert_eq!(
        work.config.identities[0].reply_to.as_deref(),
        Some("replies@example.org")
    );
    let IncomingServer::Imap { server, .. } = &work.config.incoming else {
        panic!("not IMAP");
    };
    assert_eq!(
        (server.host.as_str(), server.port, server.tls),
        ("imap.example.org", 993, TlsMode::Implicit)
    );
    let Some(OutgoingServer::Smtp { server: smtp, .. }) = &work.config.outgoing else {
        panic!("no SMTP");
    };
    assert_eq!(
        (smtp.host.as_str(), smtp.port, smtp.tls),
        ("smtp.example.org", 587, TlsMode::Starttls)
    );
    assert!(matches!(
        &server.auth,
        AuthConfig::Password { secret: None, .. }
    ));
    assert!(
        work.issues
            .iter()
            .any(|i| i.code == PortabilityIssueCode::SecretNotImported)
    );
    let pop = &preview.accounts[1];
    assert!(matches!(pop.config.incoming, IncomingServer::Pop3 { .. }));
    assert!(
        preview
            .issues
            .iter()
            .any(|i| i.code == PortabilityIssueCode::AccountNotExpressible
                && i.severity == IssueSeverity::Skipped
                && i.item.label.as_deref() == Some("Blogs & News Feeds"))
    );
    assert_eq!(preview.counts.accounts, 2);

    // Filters.
    let names: Vec<(&str, bool, bool)> = preview
        .rules
        .iter()
        .map(|r| (r.rule.name.as_str(), r.complete, r.rule.enabled))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Rust lists", true, true),
            ("Priorities", false, false),
            ("Elsewhere", true, false),
            ("Engines to archive", true, true),
        ]
    );
    let rust = &preview.rules[0];
    assert_eq!(rust.account_key.as_deref(), Some("account1"));
    assert!(
        matches!(&rust.rule.conditions, Condition::Any { conditions } if conditions.len() == 2)
    );
    assert_eq!(
        rust.rule.actions[0],
        RuleAction::Move {
            folder: FolderRef::Path {
                path: "Lists/Rust".into(),
                account_id: None
            }
        }
    );
    let priorities = &preview.rules[1];
    assert_eq!(priorities.issues.len(), 2);
    assert!(
        priorities
            .issues
            .iter()
            .all(|i| i.code == PortabilityIssueCode::FilterNotExpressible)
    );
    let archive = &preview.rules[3];
    assert_eq!(archive.rule.triggers, vec![RuleTrigger::Arrival]);
    assert_eq!(
        archive.rule.actions[0],
        RuleAction::Move {
            folder: FolderRef::Path {
                path: "Local Folders/Archives/2024".into(),
                account_id: Some(LOCAL_FOLDERS_ACCOUNT_ID.into())
            }
        }
    );
}

#[test]
fn importing_the_profile() {
    let mut store = MemorySink::default();
    let mut ctx = JobContext::silent(JobReport::started(
        "j1".into(),
        JobKind::Import,
        "2026-10-02T10:00:00Z",
    ));
    let result = import::run(
        &source(),
        &ImportOptions::default(),
        &mut store,
        &mut MemorySink::default(),
        &mut ctx,
    );
    // The PIM items go to the second store; count them through the report.
    let mut report = ctx.into_report();
    finish_report(&mut report, &result, "2026-10-02T10:00:05Z");
    result.unwrap();
    nail_portability::testing::assert_thunderbird_fixture_imported(&report);
    assert_eq!(report.counts.accounts, 2);
    assert_eq!(report.counts.rules, 4);

    let paths: Vec<&str> = store.appended.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "ada@example.net/Inbox",
            "Local Folders/Archives/2024",
            "Local Folders/Inbox",
            "Local Folders/Inbox"
        ]
    );
    let inbox = store
        .folders
        .iter()
        .find(|f| f.path == "Local Folders/Inbox")
        .unwrap();
    assert_eq!(inbox.role, FolderRole::Inbox);
    assert_eq!(inbox.parent.as_deref(), Some("Local Folders"));
    let (_, engine) = &store.appended[2];
    assert!(engine.flags.seen && engine.flags.flagged && !engine.flags.answered);
    assert_eq!(engine.keywords, vec!["$label1"]);
    assert_eq!(
        engine.key.message_id.as_deref(),
        Some("engine-1@example.org")
    );
    assert!(String::from_utf8_lossy(&engine.raw).contains("\nFrom the drawings it should work."));
    let (_, greetings) = &store.appended[3];
    assert!(!greetings.flags.seen);
    let (_, pop) = &store.appended[0];
    assert!(pop.flags.seen && pop.flags.answered);

    // The deleted-but-not-compacted message is reported, not imported.
    let deleted: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.localized.id == import::DELETED_IN_SOURCE_ID)
        .collect();
    assert_eq!(deleted.len(), 1);
    assert_eq!(
        deleted[0].item.label.as_deref(),
        Some("Deleted but not compacted")
    );

    // Passwords never travel: not from logins.json, not anywhere.
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("NEVERIMPORTED"));
    assert!(!json.contains("SECRET"));
}

#[test]
fn the_address_books_are_imported() {
    let mut books = MemorySink::default();
    let mut ctx = JobContext::silent(JobReport::started("j1".into(), JobKind::Import, "t"));
    let options = ImportOptions {
        folders: Some(vec![]),
        ..ImportOptions::default()
    };
    import::run(
        &source(),
        &options,
        &mut MemorySink::default(),
        &mut books,
        &mut ctx,
    )
    .unwrap();
    assert_eq!(ctx.report.counts.messages, 0);
    assert_eq!(
        books.pim,
        vec![
            ("history".to_owned(), "Bob".to_owned()),
            ("Old address book".to_owned(), "Grace Hopper".to_owned()),
            ("Old address book".to_owned(), "group Pioneers".to_owned()),
            ("abook".to_owned(), "Charles Babbage".to_owned()),
            ("abook".to_owned(), "Mary Somerville".to_owned()),
            ("abook".to_owned(), "group Engine friends".to_owned()),
        ]
    );
}

#[test]
fn a_chosen_folder_and_a_chosen_book() {
    let mut store = MemorySink::default();
    let mut books = MemorySink::default();
    let mut ctx = JobContext::silent(JobReport::started("j1".into(), JobKind::Import, "t"));
    let options = ImportOptions {
        folders: Some(vec!["Local Folders/Archives".into()]),
        collections: Some(vec!["old.mab".into()]),
        keep_flags: false,
        ..ImportOptions::default()
    };
    import::run(&source(), &options, &mut store, &mut books, &mut ctx).unwrap();
    let created: Vec<&str> = store.folders.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        created,
        vec![
            "Local Folders",
            "Local Folders/Archives",
            "Local Folders/Archives/2024"
        ]
    );
    assert_eq!(store.appended.len(), 1);
    assert!(!store.appended[0].1.flags.seen, "flags dropped");
    assert_eq!(books.pim.len(), 2);
}
