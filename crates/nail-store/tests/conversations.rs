//! Conversations (P2, ADR 0148): the listed folder's messages plus the
//! user's from Sent, drafts on their own, "me", counts, order and scopes.
#![cfg(test)]

mod common;

use common::{account, folder, message, seeded_store};
use nail_api::{Address, Flags, FolderId, FolderRole, MessageId, ThreadId};
use nail_store::{
    ConversationFolders, MessageMeta, OwnAddresses, SINGLE_PREFIX, Store, SyncedMessage,
};
use pretty_assertions::assert_eq;

/// A message from `from`, dated `day` days into 2026, with Message-ID
/// `<id>@x` and the given parents.
fn mail(
    id: &str,
    account: &str,
    folder: &str,
    from: &str,
    refs: &[&str],
    day: u32,
) -> SyncedMessage {
    let mut m = message(id, account, folder, &format!("2026-01-{day:02}T10:00:00Z"));
    m.summary.subject = if refs.is_empty() {
        "Plan".into()
    } else {
        "Re: Plan".into()
    };
    m.summary.from = Some(Address {
        name: None,
        email: from.into(),
    });
    m.summary.flags = Flags {
        seen: true,
        ..Flags::default()
    };
    let refs: Vec<String> = refs.iter().map(|r| format!("{r}@x")).collect();
    SyncedMessage {
        message: m,
        meta: Some(MessageMeta {
            message_id: Some(format!("{id}@x")),
            in_reply_to: refs.last().cloned().into_iter().collect(),
            references: refs,
            size: Some(1000),
            remote_thread_id: None,
        }),
    }
}

fn other(id: &str, subject: &str, day: u32) -> SyncedMessage {
    let mut m = mail(id, "a1", "inbox", "zed@x", &[], day);
    m.message.summary.subject = subject.into();
    m.meta.as_mut().unwrap().message_id = Some(format!("{id}@other"));
    m
}

/// Account `a1` (me@a1) with Inbox, Archive, Sent, Drafts, Trash, Junk and
/// account `a2` (me@a2) with Inbox and Sent. The "Plan" thread: `a` (Ann,
/// unread) and `b` (Bob) in the Inbox, the user's `s1` and `s2` in Sent, a
/// draft `d`, a reply in Trash, a reply in Junk, and `x` in a2's Sent.
/// "Other" (`z`, day 7) is a second conversation in the Inbox; "Solo"
/// lives only in Sent.
fn store() -> Store {
    let store = seeded_store();
    store.upsert_account(&account("a2")).unwrap();
    for (id, acc, path, role) in [
        ("sent", "a1", "Sent", FolderRole::Sent),
        ("drafts", "a1", "Drafts", FolderRole::Drafts),
        ("trash", "a1", "Trash", FolderRole::Trash),
        ("junk", "a1", "Junk", FolderRole::Junk),
        ("inbox2", "a2", "INBOX", FolderRole::Inbox),
        ("sent2", "a2", "Sent", FolderRole::Sent),
    ] {
        store
            .upsert_folder(&folder(id, acc, path, None, role))
            .unwrap();
    }
    let mut a = mail("a", "a1", "inbox", "ann@x", &[], 1);
    a.message.summary.flags.seen = false;
    let mut solo = mail("solo", "a1", "sent", "me@a1", &[], 9);
    solo.message.summary.subject = "Solo".into();
    solo.meta.as_mut().unwrap().message_id = Some("solo@other".into());
    store
        .merge_messages(&[
            a,
            mail("s1", "a1", "sent", "me@a1", &["a"], 2),
            mail("b", "a1", "inbox", "bob@x", &["a", "s1"], 3),
            mail("d", "a1", "drafts", "me@a1", &["a", "b"], 4),
            mail("t", "a1", "trash", "carl@x", &["a"], 5),
            mail("j", "a1", "junk", "spam@x", &["a"], 5),
            mail("x", "a2", "sent2", "me@a2", &["a"], 6),
            other("z", "Other", 7),
            mail("s2", "a1", "sent", "me@a1", &["a", "b"], 8),
            solo,
        ])
        .unwrap();
    store
}

fn own() -> OwnAddresses {
    let mut own = OwnAddresses::new();
    own.add(&"a1".into(), "me@a1");
    own.add(&"a2".into(), "ME@a2");
    own
}

fn folders(store: &Store, ids: &[&str]) -> ConversationFolders {
    let ids: Vec<FolderId> = ids.iter().map(|id| FolderId::from(*id)).collect();
    store.conversation_folders(&ids).unwrap()
}

fn ids(messages: &[nail_api::ConversationMessage]) -> Vec<(&str, bool)> {
    messages
        .iter()
        .map(|m| (m.message.id.as_str(), m.from_me))
        .collect()
}

#[test]
fn folders_are_classified_and_sent_joins_per_account() {
    let store = store();
    let scope = folders(&store, &["inbox", "drafts", "nope"]);
    let names = |list: &[nail_store::ScopeFolder]| {
        list.iter()
            .map(|f| f.id.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&scope.threaded), ["inbox"]);
    assert_eq!(names(&scope.singles), ["drafts"]);
    assert_eq!(names(&scope.sent), ["sent"], "only a1's Sent folder joins");
    // Listing Sent itself: nothing joins it a second time.
    let scope = folders(&store, &["sent"]);
    assert_eq!(names(&scope.threaded), ["sent"]);
    assert!(scope.sent.is_empty());
    // A unified inbox joins both accounts' Sent folders.
    let scope = folders(&store, &["inbox", "inbox2"]);
    assert_eq!(names(&scope.sent), ["sent", "sent2"]);
}

#[test]
fn an_inbox_row_counts_the_users_replies_from_sent() {
    let store = store();
    let page = store
        .list_conversations(&folders(&store, &["inbox"]), &own(), 0, 50)
        .unwrap();
    assert_eq!(page.total, 2, "Solo lives only in Sent");
    let subjects: Vec<&str> = page.threads.iter().map(|t| t.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["Plan", "Other"],
        "the reply from Sent (day 8) is the newest activity"
    );
    let plan = &page.threads[0];
    assert_eq!(
        plan.message_count, 4,
        "a, s1, b, s2 — no draft, Trash, Junk or a2's Sent"
    );
    assert_eq!(plan.unread_count, 1);
    // The row shows the newest activity (the user's reply s2), but choosing
    // it selects, and its actions act on, the newest message the user did
    // not write (b in the Inbox), never the Sent copy.
    assert_eq!(plan.latest_message_id, MessageId::from("b"));
    assert_eq!(plan.latest_message.id, MessageId::from("b"));
    assert!(plan.date.starts_with("2026-01-08"));
    let latest = plan.latest_from.as_ref().unwrap();
    assert!(latest.me && !latest.unread);
    assert_eq!(
        plan.participants
            .iter()
            .map(|p| (p.address.email.as_str(), p.me, p.unread))
            .collect::<Vec<_>>(),
        [
            ("ann@x", false, true),
            ("me@a1", true, false),
            ("bob@x", false, false)
        ]
    );
    // The conversation as the reader shows it: oldest first, own marked.
    let (summary, messages) = store
        .conversation(&plan.id, Some(&folders(&store, &["inbox"])), &own())
        .unwrap();
    assert_eq!(&summary, plan, "the row and the reader agree");
    assert_eq!(
        ids(&messages),
        [("a", false), ("s1", true), ("b", false), ("s2", true)]
    );
}

#[test]
fn without_identities_sent_still_marks_the_user() {
    let store = store();
    let page = store.list_threads(&["inbox".into()], 0, 50).unwrap();
    let plan = &page.threads[0];
    assert_eq!(plan.message_count, 4);
    let me = plan
        .participants
        .iter()
        .find(|p| p.address.email == "me@a1")
        .unwrap();
    assert!(me.me, "the sender of the user's Sent messages is \"me\"");
}

#[test]
fn identities_mark_the_users_messages_outside_sent() {
    let store = store();
    // The user wrote to their own Inbox from an identity.
    store
        .merge_messages(&[mail("own", "a1", "inbox", "Me@A1", &["a"], 10)])
        .unwrap();
    let thread = store.thread_of(&"own".into()).unwrap();
    let (_, messages) = store
        .conversation(&thread, Some(&folders(&store, &["inbox"])), &own())
        .unwrap();
    assert_eq!(
        messages.last().map(|m| (m.message.id.as_str(), m.from_me)),
        Some(("own", true))
    );
    let (_, messages) = store
        .conversation(
            &thread,
            Some(&folders(&store, &["inbox"])),
            &OwnAddresses::new(),
        )
        .unwrap();
    assert_eq!(
        messages.last().map(|m| m.from_me),
        Some(false),
        "no identities, not in Sent"
    );
}

#[test]
fn without_a_folder_every_folder_but_drafts_trash_and_junk() {
    let store = store();
    let thread = store.thread_of(&"a".into()).unwrap();
    let (summary, messages) = store.conversation(&thread, None, &own()).unwrap();
    assert_eq!(
        ids(&messages),
        [
            ("a", false),
            ("s1", true),
            ("b", false),
            ("x", true),
            ("s2", true)
        ]
    );
    assert_eq!(summary.message_count, 5);
    assert_eq!(summary.account_ids, ["a1".into(), "a2".into()]);
}

#[test]
fn drafts_list_one_conversation_per_draft() {
    let store = store();
    store
        .merge_messages(&[mail("d2", "a1", "drafts", "me@a1", &["a"], 11)])
        .unwrap();
    let scope = folders(&store, &["drafts"]);
    let page = store.list_conversations(&scope, &own(), 0, 50).unwrap();
    assert_eq!(page.total, 2, "two drafts of one thread are two rows");
    let ids: Vec<&str> = page.threads.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        [format!("{SINGLE_PREFIX}d2"), format!("{SINGLE_PREFIX}d")]
    );
    assert!(page.threads.iter().all(|t| t.message_count == 1));
    let (summary, messages) = store
        .conversation(&page.threads[1].id, Some(&scope), &own())
        .unwrap();
    assert_eq!(summary.latest_message_id, MessageId::from("d"));
    assert_eq!(ids_of(&messages), ["d"]);
    // Drafts never join a conversation listed elsewhere.
    let thread = store.thread_of(&"d".into()).unwrap();
    let (_, messages) = store
        .conversation(&thread, Some(&folders(&store, &["inbox"])), &own())
        .unwrap();
    assert!(!ids_of(&messages).contains(&"d"));
    // An unknown single message is not found.
    assert!(
        store
            .conversation(&ThreadId::new(format!("{SINGLE_PREFIX}gone")), None, &own())
            .is_err()
    );
}

fn ids_of(messages: &[nail_api::ConversationMessage]) -> Vec<&str> {
    messages.iter().map(|m| m.message.id.as_str()).collect()
}

#[test]
fn a_copy_in_the_inbox_and_in_sent_counts_once_as_the_listed_copy() {
    let store = store();
    // The user sent s3 to themselves: one copy in Sent, one in the Inbox.
    let mut sent = mail("s3-sent", "a1", "sent", "me@a1", &["a"], 12);
    sent.meta.as_mut().unwrap().message_id = Some("s3@x".into());
    let mut inbox = mail("s3-inbox", "a1", "inbox", "me@a1", &["a"], 12);
    inbox.meta.as_mut().unwrap().message_id = Some("s3@x".into());
    store.merge_messages(&[sent, inbox]).unwrap();
    let scope = folders(&store, &["inbox"]);
    let page = store
        .list_conversations(&scope, &OwnAddresses::new(), 0, 50)
        .unwrap();
    let plan = &page.threads[0];
    assert_eq!(plan.message_count, 5);
    let (_, messages) = store
        .conversation(&plan.id, Some(&scope), &OwnAddresses::new())
        .unwrap();
    let last = messages.last().unwrap();
    assert_eq!(last.message.id, MessageId::from("s3-inbox"));
    assert!(last.from_me, "a copy sits in Sent");
    assert_eq!(last.message.duplicates.len(), 1);
}

#[test]
fn a_unified_inbox_joins_every_accounts_sent_and_narrowing_drops_folders() {
    let store = store();
    store
        .merge_messages(&[mail("y", "a2", "inbox2", "yan@x", &["a"], 4)])
        .unwrap();
    let mut scope = folders(&store, &["inbox", "inbox2"]);
    let page = store.list_conversations(&scope, &own(), 0, 50).unwrap();
    let plan = &page.threads[0];
    assert_eq!(plan.message_count, 6, "a, s1, b, y, x, s2");
    // A grant without a2 and without a1's Sent: only a1's Inbox is left.
    scope.retain(|f| f.account_id.as_str() == "a1" && f.id.as_str() != "sent");
    let page = store.list_conversations(&scope, &own(), 0, 50).unwrap();
    assert_eq!(page.total, 2);
    let subjects: Vec<&str> = page.threads.iter().map(|t| t.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["Other", "Plan"],
        "without Sent, Plan's newest is day 3"
    );
    let plan = &page.threads[1];
    assert_eq!(plan.message_count, 2);
    assert_eq!(plan.account_ids, ["a1".into()]);
    assert!(plan.participants.iter().all(|p| !p.me));
}

#[test]
fn listing_sent_shows_the_users_conversations() {
    let store = store();
    let scope = folders(&store, &["sent"]);
    let page = store
        .list_conversations(&scope, &OwnAddresses::new(), 0, 50)
        .unwrap();
    assert_eq!(page.total, 2);
    let plan = page.threads.iter().find(|t| t.subject == "Plan").unwrap();
    assert_eq!(
        plan.message_count, 2,
        "s1 and s2: the Inbox does not join Sent"
    );
    let (_, messages) = store
        .conversation(&plan.id, Some(&scope), &OwnAddresses::new())
        .unwrap();
    assert_eq!(ids(&messages), [("s1", true), ("s2", true)]);
}

#[test]
fn paging_follows_the_conversation_order() {
    let store = store();
    let scope = folders(&store, &["inbox"]);
    let first = store.list_conversations(&scope, &own(), 0, 1).unwrap();
    let second = store.list_conversations(&scope, &own(), 1, 1).unwrap();
    assert_eq!((first.total, second.total), (2, 2));
    assert_eq!(first.threads[0].subject, "Plan");
    assert_eq!(second.threads[0].subject, "Other");
    assert!(
        store
            .list_conversations(&scope, &own(), 2, 1)
            .unwrap()
            .threads
            .is_empty()
    );
}

#[test]
fn a_sent_row_selects_the_users_newest_message() {
    let store = store();
    let page = store
        .list_conversations(&folders(&store, &["sent"]), &own(), 0, 50)
        .unwrap();
    let plan = page
        .threads
        .iter()
        .find(|t| t.subject == "Plan")
        .expect("the Plan conversation is listed in Sent");
    // Every message of the row is the user's: the newest one is chosen.
    assert_eq!(plan.latest_message_id, MessageId::from("s2"));
}
