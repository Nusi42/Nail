//! End-to-end search: messages in a store, their text in an index, queries
//! in the query language.
#![cfg(test)]

use nail_api::search::{SearchErrorKind, SearchQuery, SearchSort};
use nail_api::{Account, AccountKind, Address, Flags, Folder, FolderRole, MessageSummary};
use nail_search::{
    CommitPolicy, IndexDocument, PageRequest, SearchContext, SearchIndex, SearchScope,
    VirtualAccountScope, matching_ids, parse_query, run_request, search,
};
use nail_store::{MessageMeta, Store, StoredMessage, SyncedMessage};
use pretty_assertions::assert_eq;

/// 2026-09-29T12:00:00Z
const NOW: i64 = 1_790_683_200;

struct Fixture {
    store: Store,
    index: SearchIndex,
    ctx: SearchContext,
}

struct Mail {
    id: &'static str,
    account: &'static str,
    folder: &'static str,
    from: (&'static str, &'static str),
    to: &'static str,
    subject: &'static str,
    body: &'static str,
    date: &'static str,
    flags: Flags,
    tags: &'static [&'static str],
    attachments: &'static [&'static str],
    size: u64,
}

const DEFAULT: Mail = Mail {
    id: "",
    account: "work",
    folder: "work-inbox",
    from: ("Ada Lovelace", "ada@example.org"),
    to: "bob@example.org",
    subject: "",
    body: "",
    date: "2026-09-20T10:00:00Z",
    flags: Flags {
        seen: false,
        flagged: false,
        answered: false,
        draft: false,
    },
    tags: &[],
    attachments: &[],
    size: 2_000,
};

fn fixture() -> Fixture {
    let store = Store::open_in_memory().unwrap();
    for (id, name) in [("work", "Work"), ("home", "Home")] {
        store
            .upsert_account(&Account {
                id: id.into(),
                name: name.into(),
                email: format!("me@{id}.example"),
                kind: AccountKind::Imap,
            })
            .unwrap();
    }
    for (id, account, path, role) in [
        ("work-inbox", "work", "INBOX", FolderRole::Inbox),
        ("work-archive", "work", "Archive", FolderRole::Archive),
        ("home-inbox", "home", "INBOX", FolderRole::Inbox),
    ] {
        store
            .upsert_folder(&Folder {
                id: id.into(),
                account_id: account.into(),
                parent_id: None,
                name: path.into(),
                path: path.into(),
                role,
                unread: 0,
                total: 0,
            })
            .unwrap();
    }
    let mails = [
        Mail {
            id: "report",
            subject: "Quarterly report",
            body: "The numbers for Q3 are attached.",
            attachments: &["q3-report.pdf"],
            date: "2026-09-25T09:00:00Z",
            tags: &["work"],
            size: 3 << 20,
            ..DEFAULT
        },
        Mail {
            id: "lunch",
            from: ("Bob Builder", "bob@example.org"),
            to: "ada@example.org",
            subject: "Lunch on Friday?",
            body: "Shall we try the new place? The report can wait.",
            date: "2026-09-28T11:00:00Z",
            flags: Flags {
                seen: true,
                flagged: true,
                ..Flags::default()
            },
            ..DEFAULT
        },
        Mail {
            id: "old",
            folder: "work-archive",
            subject: "Budget 2025",
            body: "Old budget numbers.",
            date: "2025-12-01T08:00:00Z",
            flags: Flags {
                seen: true,
                answered: true,
                ..Flags::default()
            },
            size: 500,
            ..DEFAULT
        },
        Mail {
            id: "grandma",
            account: "home",
            folder: "home-inbox",
            from: ("Grandma Müller", "oma@family.example"),
            to: "me@home.example",
            subject: "Grüße aus dem Urlaub",
            body: "Das Wetter bleibt schön. Photos attached!",
            attachments: &["strand.jpg"],
            date: "2026-09-29T07:00:00Z",
            tags: &["family"],
            size: 8 << 20,
            ..DEFAULT
        },
    ];
    let index = SearchIndex::open_in_ram(CommitPolicy::default()).unwrap();
    let mut synced = Vec::new();
    let mut docs = Vec::new();
    for m in &mails {
        let summary = MessageSummary {
            id: m.id.into(),
            folder_id: m.folder.into(),
            account_id: m.account.into(),
            subject: m.subject.into(),
            from: Some(Address {
                name: Some(m.from.0.into()),
                email: m.from.1.into(),
            }),
            to: vec![Address {
                name: None,
                email: m.to.into(),
            }],
            date: m.date.into(),
            flags: m.flags,
            has_attachments: !m.attachments.is_empty(),
            preview: String::new(),
            thread_id: None,
            tags: m.tags.iter().map(|t| (*t).to_owned()).collect(),
            duplicates: vec![],
            headers_only: false,
            gmail: None,
            exchange: None,
        };
        synced.push(SyncedMessage {
            message: StoredMessage::new(summary),
            meta: Some(MessageMeta {
                message_id: Some(format!("{}@x", m.id)),
                size: Some(m.size),
                ..MessageMeta::default()
            }),
        });
        docs.push(IndexDocument {
            message_id: m.id.into(),
            account_id: m.account.into(),
            from: format!("{} <{}>", m.from.0, m.from.1),
            to: m.to.into(),
            cc: String::new(),
            subject: m.subject.into(),
            body: m.body.into(),
            attachment_names: m.attachments.iter().map(|a| (*a).to_owned()).collect(),
            decrypted: false,
        });
    }
    store.merge_messages(&synced).unwrap();
    index.add(&docs).unwrap();
    Fixture {
        store,
        index,
        ctx: SearchContext {
            now: NOW,
            utc_offset_secs: 0,
            virtual_accounts: vec![VirtualAccountScope {
                id: "everything".into(),
                name: "Everything".into(),
                members: vec!["work".into(), "home".into()],
            }],
        },
    }
}

impl Fixture {
    fn ids(&self, text: &str) -> Vec<String> {
        self.scoped(text, &SearchScope::default())
    }

    fn scoped(&self, text: &str, scope: &SearchScope) -> Vec<String> {
        let query = parse_query(text).unwrap_or_else(|e| panic!("{text:?}: {e:?}"));
        matching_ids(&self.index, &self.store, &query, scope, &self.ctx)
            .unwrap()
            .into_iter()
            .map(|id| id.0)
            .collect()
    }
}

#[test]
fn free_text_and_fields() {
    let f = fixture();
    assert_eq!(
        f.ids(""),
        ["grandma", "lunch", "report", "old"],
        "newest first"
    );
    assert_eq!(f.ids("report"), ["lunch", "report"]);
    assert_eq!(f.ids("subject:report"), ["report"]);
    assert_eq!(f.ids("body:report"), ["lunch"]);
    assert_eq!(f.ids("from:bob"), ["lunch"]);
    assert_eq!(f.ids("from:ada@example.org"), ["report", "old"]);
    assert_eq!(f.ids("to:ada"), ["lunch"]);
    assert_eq!(f.ids("filename:pdf"), ["report"]);
    assert_eq!(
        f.ids("q3"),
        ["report"],
        "attachment names and body are searched"
    );
    assert_eq!(f.ids("\"new place\""), ["lunch"]);
    assert_eq!(f.ids("\"place new\"").len(), 0);
    assert_eq!(f.ids("grusse"), ["grandma"], "ASCII folding");
    assert_eq!(f.ids("from:muller"), ["grandma"]);
    assert_eq!(f.ids("!!!"), f.ids(""), "text without words is ignored");
}

#[test]
fn flags_tags_accounts_folders() {
    let f = fixture();
    assert_eq!(f.ids("is:unread"), ["grandma", "report"]);
    assert_eq!(f.ids("is:read"), ["lunch", "old"]);
    assert_eq!(f.ids("is:flagged"), ["lunch"]);
    assert_eq!(f.ids("is:answered"), ["old"]);
    assert_eq!(f.ids("has:attachment"), ["grandma", "report"]);
    assert_eq!(f.ids("tag:WORK"), ["report"]);
    assert_eq!(f.ids("account:home"), ["grandma"]);
    assert_eq!(f.ids("account:Work"), ["lunch", "report", "old"]);
    assert_eq!(f.ids("account:me@home.example"), ["grandma"]);
    assert_eq!(
        f.ids("account:everything").len(),
        4,
        "virtual account by id"
    );
    assert_eq!(f.ids("account:all").len(), 4);
    assert_eq!(f.ids("account:nobody").len(), 0);
    assert_eq!(f.ids("folder:archive"), ["old"]);
    assert_eq!(f.ids("folder:inbox"), ["grandma", "lunch", "report"]);
}

#[test]
fn dates_and_sizes() {
    let f = fixture();
    assert_eq!(f.ids("before:2026-01-01"), ["old"]);
    assert_eq!(
        f.ids("after:2026-09-28"),
        ["grandma", "lunch"],
        "inclusive day"
    );
    assert_eq!(f.ids("before:2026-09-28"), ["report", "old"]);
    assert_eq!(f.ids("after:2d"), ["grandma", "lunch"]);
    assert_eq!(f.ids("after:1w before:2d"), ["report"]);
    assert_eq!(f.ids("after:1y"), ["grandma", "lunch", "report", "old"]);
    assert_eq!(f.ids("size>1m"), ["grandma", "report"]);
    assert_eq!(f.ids("size>5m"), ["grandma"]);
    assert_eq!(f.ids("size<=2000"), ["lunch", "old"]);
    assert_eq!(f.ids("size<1k"), ["old"]);
}

#[test]
fn boolean_logic_mixes_text_and_state() {
    let f = fixture();
    assert_eq!(f.ids("report is:unread"), ["report"]);
    assert_eq!(f.ids("report OR is:flagged"), ["lunch", "report"]);
    assert_eq!(f.ids("from:bob OR from:oma"), ["grandma", "lunch"]);
    assert_eq!(f.ids("numbers -budget"), ["report"]);
    assert_eq!(f.ids("NOT report"), ["grandma", "old"]);
    assert_eq!(f.ids("NOT (report OR is:flagged)"), ["grandma", "old"]);
    assert_eq!(f.ids("(from:ada OR from:bob) AND NOT is:read"), ["report"]);
    assert_eq!(f.ids("-has:attachment -is:read"), Vec::<String>::new());
    assert_eq!(
        f.ids("numbers (tag:work OR folder:archive)"),
        ["report", "old"]
    );
}

#[test]
fn scope_restricts_accounts_and_folders() {
    let f = fixture();
    let scope = SearchScope {
        accounts: Some(vec!["home".into()]),
        folders: None,
    };
    assert_eq!(f.scoped("", &scope), ["grandma"]);
    let scope = SearchScope {
        accounts: None,
        folders: Some(vec!["work-inbox".into(), "home-inbox".into()]),
    };
    assert_eq!(f.scoped("attached", &scope), ["grandma", "report"]);
}

#[test]
fn paging_sorting_and_requests() {
    let f = fixture();
    let q = parse_query("numbers OR report OR photos").unwrap();
    let all = search(
        &f.index,
        &f.store,
        &q,
        &SearchScope::default(),
        PageRequest {
            sort: SearchSort::Date,
            offset: 0,
            limit: 10,
        },
        &f.ctx,
    )
    .unwrap();
    assert_eq!(all.total, 4);
    let ids: Vec<&str> = all.hits.iter().map(|h| h.message.id.0.as_str()).collect();
    assert_eq!(ids, ["grandma", "lunch", "report", "old"]);
    assert!(all.hits.iter().all(|h| h.score > 0.0));
    assert_eq!(all.hits[2].message.tags, ["work"]);

    // `report` matches "report" (subject and file name) and "numbers":
    // it ranks first by relevance.
    let ranked = search(
        &f.index,
        &f.store,
        &q,
        &SearchScope::default(),
        PageRequest {
            sort: SearchSort::Relevance,
            offset: 0,
            limit: 10,
        },
        &f.ctx,
    )
    .unwrap();
    assert_eq!(ranked.hits[0].message.id.0, "report");
    let scores: Vec<f32> = ranked.hits.iter().map(|h| h.score).collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");

    let page = search(
        &f.index,
        &f.store,
        &q,
        &SearchScope::default(),
        PageRequest {
            sort: SearchSort::Date,
            offset: 1,
            limit: 2,
        },
        &f.ctx,
    )
    .unwrap();
    assert_eq!(page.total, 4);
    let ids: Vec<&str> = page.hits.iter().map(|h| h.message.id.0.as_str()).collect();
    assert_eq!(ids, ["lunch", "report"]);
    let beyond = search(
        &f.index,
        &f.store,
        &q,
        &SearchScope::default(),
        PageRequest {
            sort: SearchSort::Date,
            offset: 10,
            limit: 2,
        },
        &f.ctx,
    )
    .unwrap();
    assert_eq!((beyond.total, beyond.hits.len()), (4, 0));

    let request = SearchQuery {
        text: "is:flagged".into(),
        account_id: None,
        folder_id: None,
        offset: 0,
        limit: 5,
        sort: SearchSort::Date,
    };
    let ok = run_request(
        &f.index,
        &f.store,
        &request,
        &SearchScope::default(),
        &f.ctx,
    )
    .unwrap()
    .unwrap();
    assert_eq!(ok.total, 1);
    let bad = SearchQuery {
        text: "is:purple".into(),
        ..request
    };
    let err = run_request(&f.index, &f.store, &bad, &SearchScope::default(), &f.ctx)
        .unwrap()
        .unwrap_err();
    assert_eq!(
        (err.kind, err.start, err.end),
        (SearchErrorKind::InvalidValue, 3, 9)
    );
}

#[test]
fn index_updates_are_visible_to_searches() {
    let f = fixture();
    f.index.remove(&["lunch".into()]).unwrap();
    assert_eq!(f.ids("from:bob"), Vec::<String>::new());
    f.index
        .add(&[IndexDocument {
            message_id: "old".into(),
            account_id: "work".into(),
            from: String::new(),
            to: String::new(),
            cc: String::new(),
            subject: "Budget 2025 (revised)".into(),
            body: "revised".into(),
            attachment_names: vec![],
            decrypted: false,
        }])
        .unwrap();
    assert_eq!(f.ids("revised"), ["old"]);
    assert!(
        f.ids("from:ada budget").is_empty(),
        "the old document was replaced"
    );
    // Messages the store no longer has are dropped from results.
    f.store.delete_message(&"old".into()).unwrap();
    assert_eq!(f.ids("revised").len(), 0);
}
