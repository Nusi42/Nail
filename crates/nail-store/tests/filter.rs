//! Store-side search filters.

mod common;

use common::{account, folder, message, seeded_store};
use nail_api::{Flags, FolderRole, MessageId};
use nail_store::{MessageFilter as F, MessageMeta, SizeCmp, Store, SyncedMessage};
use pretty_assertions::assert_eq;

/// m1 (inbox, unread, 1 KiB, tag work), m2 (inbox, seen+flagged,
/// attachment, 5 MiB), m3 (archive, answered, draft, size unknown),
/// n1 (account a2 "Private", inbox2).
fn store() -> Store {
    let store = seeded_store();
    let mut a2 = account("a2");
    a2.name = "Private".into();
    store.upsert_account(&a2).unwrap();
    store
        .upsert_folder(&folder("inbox2", "a2", "INBOX", None, FolderRole::Inbox))
        .unwrap();
    let mut m1 = message("m1", "a1", "inbox", "2026-09-01T10:00:00Z");
    m1.summary.tags = vec!["Work".into()];
    let mut m2 = message("m2", "a1", "inbox", "2026-09-10T10:00:00Z");
    m2.summary.flags = Flags {
        seen: true,
        flagged: true,
        ..Flags::default()
    };
    m2.summary.has_attachments = true;
    let mut m3 = message("m3", "a1", "archive", "2026-08-01T10:00:00Z");
    m3.summary.flags = Flags {
        answered: true,
        draft: true,
        ..Flags::default()
    };
    let n1 = message("n1", "a2", "inbox2", "2026-09-20T10:00:00Z");
    let with_size = |m: nail_store::StoredMessage, size: Option<u64>| SyncedMessage {
        message: m,
        meta: Some(MessageMeta {
            size,
            ..MessageMeta::default()
        }),
    };
    store
        .merge_messages(&[
            with_size(m1, Some(1024)),
            with_size(m2, Some(5 << 20)),
            with_size(m3, None),
            with_size(n1, Some(10)),
        ])
        .unwrap();
    store
}

fn ids(store: &Store, filter: &F) -> Vec<String> {
    store
        .filter_message_ids(filter)
        .unwrap()
        .into_iter()
        .map(|m| m.0)
        .collect()
}

#[test]
fn leaves() {
    let s = store();
    assert_eq!(ids(&s, &F::All), ["n1", "m2", "m1", "m3"], "newest first");
    assert!(ids(&s, &F::None).is_empty());
    assert_eq!(ids(&s, &F::Seen(false)), ["n1", "m1", "m3"]);
    assert_eq!(ids(&s, &F::Flagged(true)), ["m2"]);
    assert_eq!(ids(&s, &F::Answered(true)), ["m3"]);
    assert_eq!(ids(&s, &F::Draft(true)), ["m3"]);
    assert_eq!(ids(&s, &F::HasAttachments(true)), ["m2"]);
    assert_eq!(ids(&s, &F::Tag("work".into())), ["m1"], "case-insensitive");
    assert_eq!(ids(&s, &F::AccountIn(vec!["a2".into()])), ["n1"]);
    assert_eq!(ids(&s, &F::AccountNamed("private".into())), ["n1"]);
    assert_eq!(ids(&s, &F::AccountNamed("A1@EXAMPLE.ORG".into())).len(), 3);
    assert_eq!(ids(&s, &F::FolderIn(vec!["archive".into()])), ["m3"]);
    assert_eq!(ids(&s, &F::FolderNamed("inbox".into())), ["n1", "m2", "m1"]);
    assert_eq!(ids(&s, &F::FolderNamed("Archive".into())), ["m3"]);
    let sep10 = 1_788_998_400; // 2026-09-10T00:00:00Z
    assert_eq!(ids(&s, &F::DateBefore(sep10)), ["m1", "m3"]);
    assert_eq!(ids(&s, &F::DateOnOrAfter(sep10)), ["n1", "m2"]);
    assert_eq!(ids(&s, &F::Size(SizeCmp::Greater, 1024)), ["m2"]);
    assert_eq!(
        ids(&s, &F::Size(SizeCmp::GreaterOrEqual, 1024)),
        ["m2", "m1"]
    );
    assert_eq!(ids(&s, &F::Size(SizeCmp::Less, 1024)), ["n1"]);
    assert_eq!(ids(&s, &F::Size(SizeCmp::LessOrEqual, 1024)), ["n1", "m1"]);
    assert_eq!(
        ids(&s, &F::Ids(vec!["m3".into(), "m1".into(), "zz".into()])),
        ["m1", "m3"]
    );
}

#[test]
fn boolean_combinations() {
    let s = store();
    let f = F::And(vec![
        F::FolderNamed("inbox".into()),
        F::Or(vec![F::Flagged(true), F::Tag("work".into())]),
        F::Not(Box::new(F::AccountIn(vec!["a2".into()]))),
    ]);
    assert_eq!(ids(&s, &f), ["m2", "m1"]);
    assert_eq!(s.count_messages(&f).unwrap(), 2);
    assert_eq!(ids(&s, &F::Not(Box::new(F::All))), Vec::<String>::new());
    assert_eq!(ids(&s, &F::And(vec![])).len(), 4);
    assert!(ids(&s, &F::Or(vec![])).is_empty());
}

#[test]
fn summaries_keep_the_requested_order() {
    let s = store();
    let got = s
        .message_summaries(&["m3".into(), "nope".into(), "m1".into()])
        .unwrap();
    let got: Vec<&MessageId> = got.iter().map(|m| &m.id).collect();
    assert_eq!(got, [&MessageId::from("m3"), &MessageId::from("m1")]);
    let m1 = s.message_summaries(&["m1".into()]).unwrap();
    assert_eq!(m1[0].tags, ["Work"]);
}
