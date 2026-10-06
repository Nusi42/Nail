//! Incremental threading in the store: JWZ links, the classic edge cases,
//! the subject fallback, cross-account threads and thread listings.
#![cfg(test)]

mod common;

use common::{account, folder, message, seeded_store};
use nail_api::{AccountId, Address, Flags, FolderId, FolderRole, MessageId, ThreadId};
use nail_store::{MessageMeta, Store, SyncedMessage};
use pretty_assertions::assert_eq;

/// A message with headers, dated `day` days into 2026.
fn mail(
    id: &str,
    account: &str,
    folder: &str,
    mid: Option<&str>,
    refs: &[&str],
    subject: &str,
    day: u32,
) -> SyncedMessage {
    let mut m = message(
        id,
        account,
        folder,
        &format!("2026-01-{:02}T10:00:00Z", day.clamp(1, 28)),
    );
    m.summary.subject = subject.into();
    SyncedMessage {
        message: m,
        meta: Some(MessageMeta {
            message_id: mid.map(Into::into),
            in_reply_to: refs
                .last()
                .map(|r| vec![(*r).to_owned()])
                .unwrap_or_default(),
            references: refs.iter().map(|r| (*r).to_owned()).collect(),
            size: Some(1000),
            remote_thread_id: None,
        }),
    }
}

fn thread(store: &Store, id: &str) -> ThreadId {
    store.thread_of(&id.into()).unwrap()
}

fn same_thread(store: &Store, ids: &[&str]) -> bool {
    let first = thread(store, ids[0]);
    ids.iter().all(|id| thread(store, id) == first)
}

#[test]
fn replies_join_their_parents_in_any_order() {
    let store = seeded_store();
    // Children first: c references a and b before either exists.
    store
        .merge_messages(&[mail(
            "c",
            "a1",
            "inbox",
            Some("c@x"),
            &["a@x", "b@x"],
            "Re: Plan",
            3,
        )])
        .unwrap();
    store
        .merge_messages(&[mail("a", "a1", "inbox", Some("a@x"), &[], "Plan", 1)])
        .unwrap();
    store
        .merge_messages(&[mail(
            "b",
            "a1",
            "inbox",
            Some("b@x"),
            &["a@x"],
            "Re: Plan",
            2,
        )])
        .unwrap();
    store
        .merge_messages(&[mail("z", "a1", "inbox", Some("z@x"), &[], "Other", 4)])
        .unwrap();
    assert!(same_thread(&store, &["a", "b", "c"]));
    assert_ne!(thread(&store, "a"), thread(&store, "z"));
}

#[test]
fn missing_parent_still_joins_siblings() {
    // Two replies to a message we never received.
    let store = seeded_store();
    store
        .merge_messages(&[
            mail("b", "a1", "inbox", Some("b@x"), &["gone@x"], "Re: Lost", 2),
            mail("c", "a1", "inbox", Some("c@x"), &["gone@x"], "Re: Lost", 3),
        ])
        .unwrap();
    assert!(same_thread(&store, &["b", "c"]));
    // The tree has a dummy root: both at depth 0.
    let tree = store.thread_tree(&thread(&store, "b"), None).unwrap();
    let layout: Vec<(String, u32)> = tree
        .iter()
        .map(|e| (e.message.id.0.clone(), e.depth))
        .collect();
    assert_eq!(layout, [("b".into(), 0), ("c".into(), 0)]);
    // When the parent arrives later it joins too.
    store
        .merge_messages(&[mail("a", "a1", "inbox", Some("gone@x"), &[], "Lost", 1)])
        .unwrap();
    assert!(same_thread(&store, &["a", "b", "c"]));
    let tree = store.thread_tree(&thread(&store, "a"), None).unwrap();
    let layout: Vec<(String, u32)> = tree
        .iter()
        .map(|e| (e.message.id.0.clone(), e.depth))
        .collect();
    assert_eq!(layout, [("a".into(), 0), ("b".into(), 1), ("c".into(), 1)]);
}

#[test]
fn reference_loops_do_not_break_threading() {
    let store = seeded_store();
    store
        .merge_messages(&[
            mail("a", "a1", "inbox", Some("a@x"), &["b@x"], "Loop", 1),
            mail("b", "a1", "inbox", Some("b@x"), &["a@x"], "Re: Loop", 2),
            mail("s", "a1", "inbox", Some("s@x"), &["s@x"], "Self", 3),
        ])
        .unwrap();
    assert!(same_thread(&store, &["a", "b"]));
    let tree = store.thread_tree(&thread(&store, "a"), None).unwrap();
    assert_eq!(tree.len(), 2);
    assert_eq!(
        store.thread_tree(&thread(&store, "s"), None).unwrap().len(),
        1
    );
}

#[test]
fn duplicate_message_ids_share_a_thread_and_collapse() {
    let store = seeded_store();
    store.upsert_account(&account("a2")).unwrap();
    store
        .upsert_folder(&folder("inbox2", "a2", "INBOX", None, FolderRole::Inbox))
        .unwrap();
    // The same list mail in two accounts, plus a reply.
    store
        .merge_messages(&[
            mail("l1", "a1", "inbox", Some("list@x"), &[], "Release", 1),
            mail("l2", "a2", "inbox2", Some("list@x"), &[], "Release", 1),
            mail(
                "r",
                "a2",
                "inbox2",
                Some("r@x"),
                &["list@x"],
                "Re: Release",
                2,
            ),
        ])
        .unwrap();
    assert!(same_thread(&store, &["l1", "l2", "r"]));

    // A virtual account over both: the copies collapse into one row.
    let (summary, messages) = store.get_thread(&thread(&store, "r"), None).unwrap();
    assert_eq!(summary.message_count, 2);
    assert_eq!(
        messages.iter().map(|m| m.id.0.as_str()).collect::<Vec<_>>(),
        ["l1", "r"]
    );
    assert_eq!(messages[0].duplicates.len(), 1);
    assert_eq!(messages[0].duplicates[0].message_id, MessageId::from("l2"));
    assert_eq!(
        summary.account_ids,
        [AccountId::from("a1"), AccountId::from("a2")]
    );

    // Scoped to one member only its copies are listed.
    let (summary, messages) = store
        .get_thread(&thread(&store, "r"), Some(&["a1".into()]))
        .unwrap();
    assert_eq!(summary.message_count, 1);
    assert!(
        messages[0].duplicates.is_empty(),
        "{:?}",
        messages[0].duplicates
    );
    assert!(
        store
            .get_thread(&thread(&store, "r"), Some(&["nobody".into()]))
            .unwrap_err()
            .is_not_found()
    );

    // Virtual folder listing (both inboxes): one row with a duplicate.
    let page = store
        .list_messages_in(&["inbox".into(), "inbox2".into()], 0, 10)
        .unwrap();
    assert_eq!(page.total, 2);
    let ids: Vec<&str> = page.messages.iter().map(|m| m.id.0.as_str()).collect();
    assert_eq!(ids, ["r", "l2"]);
    assert_eq!(
        page.messages[1].duplicates[0].message_id,
        MessageId::from("l1")
    );
}

#[test]
fn a_linking_message_merges_two_threads() {
    let store = seeded_store();
    store
        .merge_messages(&[
            mail("a", "a1", "inbox", Some("a@x"), &[], "One", 1),
            mail("b", "a1", "inbox", Some("b@x"), &[], "Two", 2),
        ])
        .unwrap();
    let older = thread(&store, "a");
    assert_ne!(older, thread(&store, "b"));
    // c references both roots: the threads merge into the older one.
    store
        .merge_messages(&[mail(
            "c",
            "a1",
            "archive",
            Some("c@x"),
            &["a@x", "b@x"],
            "Re: Two",
            3,
        )])
        .unwrap();
    assert!(same_thread(&store, &["a", "b", "c"]));
    assert_eq!(thread(&store, "b"), older);
    // Later replies to b land there as well.
    store
        .merge_messages(&[mail(
            "d",
            "a1",
            "inbox",
            Some("d@x"),
            &["b@x"],
            "Re: Two",
            4,
        )])
        .unwrap();
    assert_eq!(thread(&store, "d"), older);
}

#[test]
fn subject_fallback() {
    let store = seeded_store();
    store
        .merge_messages(&[
            mail("orig", "a1", "inbox", Some("o@x"), &[], "Lunch?", 1),
            // A reply from a client that drops References.
            mail("re", "a1", "inbox", Some("re@x"), &[], "RE: Lunch?", 2),
            // Same subject, but a new original: its own thread.
            mail("again", "a1", "inbox", Some("again@x"), &[], "Lunch?", 3),
            // Forwards never join by subject.
            mail("fwd", "a1", "inbox", Some("fwd@x"), &[], "Fwd: Lunch?", 4),
            // Empty subjects never join.
            mail("e1", "a1", "inbox", Some("e1@x"), &[], "", 5),
            mail("e2", "a1", "inbox", Some("e2@x"), &[], "Re:", 6),
        ])
        .unwrap();
    assert!(same_thread(&store, &["orig", "re"]));
    assert_ne!(thread(&store, "again"), thread(&store, "orig"));
    assert_ne!(thread(&store, "fwd"), thread(&store, "orig"));
    assert_ne!(thread(&store, "fwd"), thread(&store, "again"));
    assert_ne!(thread(&store, "e1"), thread(&store, "e2"));

    // An original arriving after its orphan replies joins them.
    store
        .merge_messages(&[
            mail("r1", "a1", "inbox", Some("r1@x"), &[], "Re: Late", 10),
            mail("r2", "a1", "inbox", Some("r2@x"), &[], "Re: Late", 11),
            mail("late", "a1", "inbox", Some("late@x"), &[], "Late", 9),
        ])
        .unwrap();
    assert!(same_thread(&store, &["r1", "r2", "late"]));

    // Replies far outside the window do not join.
    let mut far = mail("far", "a1", "inbox", Some("far@x"), &[], "Re: Lunch?", 1);
    far.message.summary.date = "2027-06-01T00:00:00Z".into();
    store.merge_messages(&[far]).unwrap();
    assert_ne!(thread(&store, "far"), thread(&store, "orig"));
}

#[test]
fn legacy_upserts_thread_by_the_m0_key_and_rethread_is_stable() {
    let store = seeded_store();
    let mut a = message("a", "a1", "inbox", "2026-01-01T00:00:00Z");
    a.thread_key = Some("root@x".into());
    let mut b = message("b", "a1", "inbox", "2026-01-02T00:00:00Z");
    b.thread_key = Some("root@x".into());
    let c = message("c", "a1", "inbox", "2026-01-03T00:00:00Z");
    store.upsert_messages(&[a, b, c]).unwrap();
    assert!(same_thread(&store, &["a", "b"]));
    assert_ne!(thread(&store, "a"), thread(&store, "c"));
    // A header-aware message referencing the root joins.
    store
        .merge_messages(&[mail(
            "d",
            "a1",
            "inbox",
            Some("d@x"),
            &["root@x"],
            "Re: x",
            4,
        )])
        .unwrap();
    assert!(same_thread(&store, &["a", "b", "d"]));

    let before: Vec<ThreadId> = ["a", "b", "c", "d"]
        .iter()
        .map(|m| thread(&store, m))
        .collect();
    assert_eq!(store.rethread_all().unwrap(), 4);
    let after: Vec<ThreadId> = ["a", "b", "c", "d"]
        .iter()
        .map(|m| thread(&store, m))
        .collect();
    assert_eq!(before, after, "thread ids are derived, not random");
    let ids = store
        .thread_ids(&["a".into(), "c".into(), "missing".into()])
        .unwrap();
    assert_eq!(ids.len(), 2);
}

#[test]
fn list_threads_summarizes_per_folder() {
    let store = seeded_store();
    store.upsert_account(&account("a2")).unwrap();
    store
        .upsert_folder(&folder("inbox2", "a2", "INBOX", None, FolderRole::Inbox))
        .unwrap();
    let mut first = mail("a", "a1", "inbox", Some("a@x"), &[], "Re: Plan", 1);
    first.message.summary.from = Some(Address {
        name: Some("Ada".into()),
        email: "ada@x".into(),
    });
    let mut second = mail("b", "a2", "inbox2", Some("b@x"), &["a@x"], "Re: Plan", 3);
    second.message.summary.from = Some(Address {
        name: Some("Bob".into()),
        email: "bob@x".into(),
    });
    second.message.summary.flags = Flags {
        seen: true,
        flagged: true,
        ..Flags::default()
    };
    second.message.summary.has_attachments = true;
    let mut third = mail("c", "a1", "archive", Some("c@x"), &["a@x"], "Re: Plan", 5);
    third.message.summary.from = Some(Address {
        name: None,
        email: "ADA@x".into(),
    });
    let other = mail("z", "a1", "inbox", Some("z@x"), &[], "Other", 2);
    store
        .merge_messages(&[first, second, third, other])
        .unwrap();

    // Virtual inbox over both accounts.
    let folders: Vec<FolderId> = vec!["inbox".into(), "inbox2".into()];
    let page = store.list_threads(&folders, 0, 10).unwrap();
    assert_eq!(page.total, 2);
    let t = &page.threads[0];
    assert_eq!(t.subject, "Plan");
    assert_eq!(
        t.message_count, 2,
        "the archived reply is not in these folders"
    );
    assert_eq!(t.unread_count, 1);
    assert!(t.flagged && t.has_attachments);
    assert_eq!(t.latest_message_id, MessageId::from("b"));
    assert_eq!(
        t.participants
            .iter()
            .map(|p| p.address.email.as_str())
            .collect::<Vec<_>>(),
        ["ada@x", "bob@x"]
    );
    // P2 (ADR 0148): senders of unread messages are marked; "me" is the
    // core's to resolve; the newest sender comes with the row.
    assert_eq!(
        t.participants
            .iter()
            .map(|p| (p.unread, p.me))
            .collect::<Vec<_>>(),
        [(true, false), (false, false)]
    );
    let latest_from = t.latest_from.as_ref().unwrap();
    assert_eq!(latest_from.address.email, "bob@x");
    assert!(!latest_from.unread);
    assert_eq!(t.latest_message.id, MessageId::from("b"));
    assert!(t.latest_message.flags.flagged);
    assert_eq!(
        t.account_ids,
        [AccountId::from("a1"), AccountId::from("a2")]
    );
    assert_eq!(page.threads[1].subject, "Other");

    // Paging.
    let page = store.list_threads(&folders, 1, 1).unwrap();
    assert_eq!((page.total, page.threads.len()), (2, 1));
    assert_eq!(page.threads[0].subject, "Other");

    // GetThread spans folders.
    let (summary, messages) = store.get_thread(&t.id, None).unwrap();
    assert_eq!(summary.message_count, 3);
    assert_eq!(
        messages.iter().map(|m| m.id.0.as_str()).collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert_eq!(messages[0].thread_id.as_ref(), Some(&t.id));
    assert_eq!(
        summary.participants.len(),
        2,
        "addresses compare case-insensitively"
    );
    assert!(
        store
            .get_thread(&"t-nope".into(), None)
            .unwrap_err()
            .is_not_found()
    );
}

#[test]
fn server_thread_ids_group_messages_without_links() {
    let store = seeded_store();
    // Two unrelated subjects, no references: separate threads by JWZ...
    let mut a = mail("ga", "a1", "inbox", Some("ga@x"), &[], "Lunch", 1);
    let mut b = mail("gb", "a1", "inbox", Some("gb@x"), &[], "Different", 2);
    // ...but the server says they are one conversation.
    a.meta.as_mut().unwrap().remote_thread_id = Some("thread-1".into());
    b.meta.as_mut().unwrap().remote_thread_id = Some("thread-1".into());
    store.merge_messages(&[a]).unwrap();
    store.merge_messages(&[b]).unwrap();
    assert!(same_thread(&store, &["ga", "gb"]));
    assert_eq!(
        store
            .message_remote_thread_id(&"ga".into())
            .unwrap()
            .as_deref(),
        Some("thread-1")
    );
    // A later message joins once its thread id is known (no subject match).
    store
        .merge_messages(&[mail("gc", "a1", "inbox", Some("gc@x"), &[], "Other", 3)])
        .unwrap();
    assert!(!same_thread(&store, &["ga", "gc"]));
    let joined = store
        .set_message_remote_thread_id(&"gc".into(), Some("thread-1"))
        .unwrap();
    assert_eq!(joined, thread(&store, "ga"));
    // Re-merging without a thread id keeps the stored one.
    store
        .merge_messages(&[mail("gc", "a1", "inbox", Some("gc@x"), &[], "Other", 3)])
        .unwrap();
    assert!(same_thread(&store, &["ga", "gb", "gc"]));
    // Unknown messages are an error.
    assert!(
        store
            .set_message_remote_thread_id(&"missing".into(), Some("t"))
            .is_err()
    );
}
