//! Tests of the thread-activity table: the v17 backfill, the v18 triggers
//! against the backfill query after random writes (raw SQL and the store's
//! own write paths), the table's pages against the grouping query, and the
//! query plans of the hot statements.

use nail_api::{
    Account, AccountKind, Address, Flags, Folder, FolderId, FolderRole, MessageSummary,
};
use rusqlite::{Connection, params};

use super::{
    COUNT_MANY_SQL, COUNT_SQL, GROUPING_PAGE_SQL, PAGE_SQL, REBUILD_SELECT, SINGLES_PAGE_SQL,
    covers, grouping_page, page,
};
use crate::schema::{migrate, migrate_to_version};
use crate::{MessageMeta, Operation, Store, StoredMessage, SyncedMessage};

/// A small deterministic generator (xorshift64): random operation
/// sequences without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap()).unwrap()
    }

    /// A `date_sort` value (few distinct ones, so ties happen).
    fn date(&mut self) -> i64 {
        i64::try_from(self.next() % 1000).unwrap()
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

type Row = (String, String, i64, i64, i64);

fn rows_of(conn: &Connection, sql: &str) -> Vec<Row> {
    conn.prepare(&format!("SELECT * FROM ({sql}) ORDER BY 1, 2"))
        .unwrap()
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// The invariant: the table equals the backfill query.
fn assert_consistent(conn: &Connection, context: &str) {
    let table = rows_of(
        conn,
        "SELECT folder_id, thread_id, folder_last, last_date, message_count FROM thread_activity",
    );
    let expected = rows_of(conn, REBUILD_SELECT);
    assert_eq!(table, expected, "after {context}");
}

fn plan(conn: &Connection, sql: &str, args: &[&dyn rusqlite::ToSql]) -> String {
    let rows: Vec<String> = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap()
        .query_map(args, |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    rows.join("\n")
}

fn assert_no_scan(plan: &str) {
    for line in plan.lines().map(str::trim) {
        assert!(
            !(line.starts_with("SCAN messages")
                || line.starts_with("SCAN m ")
                || line == "SCAN m"
                || line.starts_with("SCAN s")
                || line.starts_with("SCAN thread_activity")),
            "the query scans a table:\n{plan}"
        );
    }
}

/// A v16 database with an Inbox thread answered from Sent, an older
/// Inbox thread, and a draft.
fn v16() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    assert_eq!(migrate_to_version(&mut conn, 16).unwrap(), 16);
    conn.execute_batch(
        r"
        INSERT INTO accounts VALUES ('a1', 'A', 'a@example.org', 'imap');
        INSERT INTO folders (id, account_id, name, path, role) VALUES
            ('in', 'a1', 'Inbox', 'INBOX', 'inbox'),
            ('sent', 'a1', 'Sent', 'Sent', 'sent'),
            ('drafts', 'a1', 'Drafts', 'Drafts', 'drafts');
        INSERT INTO messages (id, account_id, folder_id, date, date_sort, thread_id) VALUES
            ('m1', 'a1', 'in', '', 100, 't1'),
            ('m2', 'a1', 'in', '', 200, 't1'),
            ('m3', 'a1', 'sent', '', 300, 't1'),
            ('m4', 'a1', 'in', '', 250, 't2'),
            ('m5', 'a1', 'drafts', '', 400, 't1');
        ",
    )
    .unwrap();
    conn
}

fn page_keys(conn: &Connection, folder: &str) -> Vec<String> {
    conn.prepare(PAGE_SQL)
        .unwrap()
        .query_map(params![folder, 50, 0], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn v17_backfills_the_order_key_with_the_users_replies() {
    let mut conn = v16();
    migrate(&mut conn).unwrap();
    assert_eq!(
        rows_of(&conn, "SELECT * FROM thread_activity"),
        vec![
            ("in".into(), "t1".into(), 200, 300, 2),
            ("in".into(), "t2".into(), 250, 250, 1),
            ("sent".into(), "t1".into(), 300, 300, 1),
        ]
    );
    // The reply from Sent puts t1 first in the Inbox.
    assert_eq!(page_keys(&conn, "in"), ["t1", "t2"]);
    let total: i64 = conn.query_row(COUNT_SQL, ["in"], |r| r.get(0)).unwrap();
    assert_eq!(total, 2);
}

#[test]
fn v18_rebuilds_a_stale_table_and_keeps_it_current() {
    let mut conn = v16();
    assert_eq!(migrate_to_version(&mut conn, 17).unwrap(), 17);
    // A v17 build without triggers wrote a message: the table went stale.
    conn.execute(
        "INSERT INTO messages (id, account_id, folder_id, date, date_sort, thread_id)
         VALUES ('m6', 'a1', 'in', '', 500, 't2')",
        [],
    )
    .unwrap();
    assert_ne!(
        rows_of(&conn, "SELECT * FROM thread_activity"),
        rows_of(&conn, REBUILD_SELECT)
    );
    migrate(&mut conn).unwrap();
    assert_consistent(&conn, "the v18 rebuild");
    assert_eq!(page_keys(&conn, "in"), ["t2", "t1"]);
    // From now on the triggers keep it: a newer reply from Sent.
    conn.execute(
        "INSERT INTO messages (id, account_id, folder_id, date, date_sort, thread_id)
         VALUES ('m7', 'a1', 'sent', '', 600, 't1')",
        [],
    )
    .unwrap();
    assert_consistent(&conn, "a reply in Sent");
    assert_eq!(page_keys(&conn, "in"), ["t1", "t2"]);
    // The draft never got a row; a draft in a thread changes nothing.
    let drafts: i64 = conn.query_row(COUNT_SQL, ["drafts"], |r| r.get(0)).unwrap();
    assert_eq!(drafts, 0);
}

const ROLES: [&str; 7] = [
    "inbox", "archive", "sent", "drafts", "trash", "junk", "other",
];

fn random_role(rng: &mut Rng) -> &'static str {
    ROLES[rng.below(ROLES.len())]
}

/// Random writes in raw SQL (every shape a write path can take, including
/// ones the store never makes, like a folder changing account) keep the
/// table equal to the backfill query.
#[test]
fn triggers_match_the_backfill_after_random_writes() {
    for seed in [0x9e37_79b9_7f4a_7c15_u64, 42, 7_777_777, 0xdead_beef] {
        let mut rng = Rng(seed);
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate(&mut conn).unwrap();
        let mut next_folder = 0;
        let mut next_message = 0;
        let add_account = |conn: &Connection, account: &str, next_folder: &mut u32| {
            conn.execute(
                "INSERT INTO accounts (id, name, email, kind) VALUES (?1, ?1, ?1, 'imap')",
                [account],
            )
            .unwrap();
            for role in ["inbox", "sent", "drafts", "archive", "sent"] {
                *next_folder += 1;
                conn.execute(
                    "INSERT INTO folders (id, account_id, name, path, role)
                     VALUES (?1, ?2, ?1, ?1, ?3)",
                    params![format!("f{next_folder}"), account, role],
                )
                .unwrap();
            }
        };
        add_account(&conn, "a1", &mut next_folder);
        add_account(&conn, "a2", &mut next_folder);
        let folders = |conn: &Connection| -> Vec<(String, String)> {
            conn.prepare("SELECT id, account_id FROM folders ORDER BY id")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let messages = |conn: &Connection| -> Vec<String> {
            conn.prepare("SELECT id FROM messages ORDER BY id")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let thread = |rng: &mut Rng| -> Option<String> {
            (!rng.chance(10)).then(|| format!("t{}", rng.below(6)))
        };
        for step in 0..400 {
            let all_folders = folders(&conn);
            let all_messages = messages(&conn);
            let op = rng.below(13);
            let context = format!("seed {seed:#x}, step {step}, op {op}");
            match op {
                // Inserts (several in one transaction now and then).
                0..=3 => {
                    let tx = conn.transaction().unwrap();
                    for _ in 0..=rng.below(3) {
                        if all_folders.is_empty() {
                            break;
                        }
                        let (folder, account) = &all_folders[rng.below(all_folders.len())];
                        next_message += 1;
                        tx.execute(
                            "INSERT INTO messages (id, account_id, folder_id, date, date_sort,
                                                   thread_id)
                             VALUES (?1, ?2, ?3, '', ?4, ?5)",
                            params![
                                format!("m{next_message}"),
                                account,
                                folder,
                                rng.date(),
                                thread(&mut rng)
                            ],
                        )
                        .unwrap();
                    }
                    tx.commit().unwrap();
                }
                4 if !all_messages.is_empty() => {
                    let m = &all_messages[rng.below(all_messages.len())];
                    conn.execute("DELETE FROM messages WHERE id = ?1", [m])
                        .unwrap();
                }
                5 if !all_messages.is_empty() && !all_folders.is_empty() => {
                    // A move (sometimes into another account).
                    let m = &all_messages[rng.below(all_messages.len())];
                    let (folder, account) = &all_folders[rng.below(all_folders.len())];
                    conn.execute(
                        "UPDATE messages SET folder_id = ?2, account_id = ?3 WHERE id = ?1",
                        params![m, folder, account],
                    )
                    .unwrap();
                }
                6 if !all_messages.is_empty() => {
                    let m = &all_messages[rng.below(all_messages.len())];
                    conn.execute(
                        "UPDATE messages SET date_sort = ?2 WHERE id = ?1",
                        params![m, rng.date()],
                    )
                    .unwrap();
                }
                7 if !all_messages.is_empty() => {
                    let m = &all_messages[rng.below(all_messages.len())];
                    conn.execute(
                        "UPDATE messages SET thread_id = ?2 WHERE id = ?1",
                        params![m, thread(&mut rng)],
                    )
                    .unwrap();
                }
                8 => {
                    // A JWZ merge: one thread joins another.
                    conn.execute(
                        "UPDATE messages SET thread_id = ?2 WHERE thread_id = ?1",
                        params![format!("t{}", rng.below(6)), format!("t{}", rng.below(6))],
                    )
                    .unwrap();
                }
                9 if !all_folders.is_empty() => {
                    let (folder, _) = &all_folders[rng.below(all_folders.len())];
                    conn.execute(
                        "UPDATE folders SET role = ?2 WHERE id = ?1",
                        params![folder, random_role(&mut rng)],
                    )
                    .unwrap();
                }
                10 if !all_folders.is_empty() && rng.chance(40) => {
                    // A folder goes (its messages with it) and another comes.
                    let (folder, account) = all_folders[rng.below(all_folders.len())].clone();
                    conn.execute("DELETE FROM folders WHERE id = ?1", [&folder])
                        .unwrap();
                    next_folder += 1;
                    conn.execute(
                        "INSERT INTO folders (id, account_id, name, path, role)
                         VALUES (?1, ?2, ?1, ?1, ?3)",
                        params![format!("f{next_folder}"), account, random_role(&mut rng)],
                    )
                    .unwrap();
                }
                11 if rng.chance(15) => {
                    // An account goes and comes back empty.
                    conn.execute("DELETE FROM accounts WHERE id = 'a2'", [])
                        .unwrap();
                    assert_consistent(&conn, &format!("{context} (account removed)"));
                    add_account(&conn, "a2", &mut next_folder);
                }
                12 if !all_folders.is_empty() && rng.chance(20) => {
                    let (folder, account) = &all_folders[rng.below(all_folders.len())];
                    let other = if account == "a1" { "a2" } else { "a1" };
                    conn.execute(
                        "UPDATE folders SET account_id = ?2 WHERE id = ?1",
                        params![folder, other],
                    )
                    .unwrap();
                }
                _ => {}
            }
            assert_consistent(&conn, &context);
        }
    }
}

// --- The store's own write paths ----------------------------------------------

fn summary(id: &str, account: &str, folder: &str, day: usize, seen: bool) -> MessageSummary {
    MessageSummary {
        subject: format!("{account} subject {id}"),
        from: Some(Address {
            name: None,
            email: format!("{id}@example.org"),
        }),
        date: format!("2026-01-{:02}T{:02}:00:00Z", 1 + day / 24, day % 24),
        flags: Flags {
            seen,
            ..Flags::default()
        },
        ..nail_api::testing::summary(id, account, folder)
    }
}

fn folder(id: &str, account: &str, role: FolderRole) -> Folder {
    Folder {
        id: id.into(),
        account_id: account.into(),
        parent_id: None,
        name: id.to_owned(),
        path: id.to_owned(),
        role,
        unread: 0,
        total: 0,
    }
}

const STORE_ROLES: [FolderRole; 6] = [
    FolderRole::Inbox,
    FolderRole::Archive,
    FolderRole::Sent,
    FolderRole::Drafts,
    FolderRole::Trash,
    FolderRole::Other,
];

/// The folder sets the store-level test lists: every folder of `a1` on its
/// own, and unified lists of `a1`'s folders.
fn scopes(store: &Store) -> Vec<Vec<FolderId>> {
    let ids: Vec<FolderId> = store
        .list_folders(&"a1".into())
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    let mut out: Vec<Vec<FolderId>> = ids.iter().map(|id| vec![id.clone()]).collect();
    out.push(ids.iter().take(3).cloned().collect());
    out.push(ids.clone());
    out
}

/// Pages from the table equal pages from the grouping query for every
/// scope, offset and a small and a large page size.
fn assert_pages_match(store: &Store, context: &str) {
    for ids in scopes(store) {
        let scope = store.conversation_folders(&ids).unwrap();
        store
            .with_tx(|tx| {
                assert!(covers(tx, &scope).unwrap(), "{context}: {ids:?}");
                let (total, _) = grouping_page(tx, &scope, 0, 1)?;
                for limit in [3_u32, 50] {
                    for offset in (0..=total + 1).step_by(2) {
                        let table = page(tx, &scope, offset, limit)?;
                        let grouped = grouping_page(tx, &scope, offset, limit)?;
                        assert_eq!(table, grouped, "{context}: {ids:?} at {offset}/{limit}");
                    }
                }
                Ok(())
            })
            .unwrap();
    }
}

fn assert_store_consistent(store: &Store, context: &str) {
    store
        .with_conn(|conn| {
            assert_consistent(conn, context);
            Ok(())
        })
        .unwrap();
}

/// Ingest (with Message-ID links that merge threads), local moves and
/// deletes through the queue, expunges, flag changes, re-threading, a
/// folder's role change and removal: after each, the table matches the
/// backfill and its pages match the grouping query (the P2 list).
#[test]
fn store_writes_keep_the_table_and_the_pages_right() {
    for seed in [3_u64, 1_234_567, 0x5eed_5eed] {
        let mut rng = Rng(seed);
        let store = Store::open_in_memory().unwrap();
        for account in ["a1", "a2"] {
            store
                .upsert_account(&Account {
                    id: account.into(),
                    name: account.into(),
                    email: format!("me@{account}"),
                    kind: AccountKind::Imap,
                })
                .unwrap();
            for (name, role) in [
                ("inbox", FolderRole::Inbox),
                ("archive", FolderRole::Archive),
                ("sent", FolderRole::Sent),
                ("drafts", FolderRole::Drafts),
                ("trash", FolderRole::Trash),
            ] {
                store
                    .upsert_folder(&folder(&format!("{account}-{name}"), account, role))
                    .unwrap();
            }
        }
        let mut next = 0;
        let mut ids: Vec<(String, String)> = Vec::new();
        for step in 0..150 {
            let op = rng.below(10);
            let context = format!("seed {seed}, step {step}, op {op}");
            let pick_folder = |rng: &mut Rng, account: &str| -> String {
                let folders = store.list_folders(&account.into()).unwrap();
                folders[rng.below(folders.len())].id.as_str().to_owned()
            };
            match op {
                0..=3 => {
                    // New mail, replying to an earlier message of the
                    // account now and then (threads grow and merge).
                    let mut batch = Vec::new();
                    for _ in 0..=rng.below(4) {
                        let account = if rng.chance(70) { "a1" } else { "a2" };
                        let folder = pick_folder(&mut rng, account);
                        next += 1;
                        let id = format!("{account}-m{next}");
                        let own: Vec<&(String, String)> =
                            ids.iter().filter(|(_, a)| a == account).collect();
                        let refs: Vec<String> = (0..rng.below(3))
                            .filter(|_| !own.is_empty())
                            .map(|_| format!("<{}@x>", own[rng.below(own.len())].0))
                            .collect();
                        batch.push(SyncedMessage {
                            message: StoredMessage::new(summary(
                                &id,
                                account,
                                &folder,
                                rng.below(500),
                                rng.chance(50),
                            )),
                            meta: Some(MessageMeta {
                                message_id: Some(format!("<{id}@x>")),
                                in_reply_to: refs.last().cloned().into_iter().collect(),
                                references: refs,
                                size: Some(100),
                                remote_thread_id: None,
                            }),
                        });
                        ids.push((id, account.to_owned()));
                    }
                    store.merge_messages(&batch).unwrap();
                }
                4 if !ids.is_empty() => {
                    let (id, account) = ids[rng.below(ids.len())].clone();
                    let target = pick_folder(&mut rng, &account);
                    if store
                        .get_message(&id.as_str().into())
                        .is_ok_and(|m| m.summary.folder_id.as_str() != target)
                    {
                        store
                            .enqueue(&Operation::MoveMessage {
                                message_id: id.as_str().into(),
                                target_folder_id: target.into(),
                                source: None,
                            })
                            .unwrap();
                    }
                }
                5 if !ids.is_empty() => {
                    let (id, _) = ids[rng.below(ids.len())].clone();
                    if store.message_exists(&id.as_str().into()).unwrap() {
                        store
                            .enqueue(&Operation::DeleteMessage {
                                message_id: id.as_str().into(),
                                permanent: rng.chance(30),
                                source: None,
                                trash_folder_id: None,
                            })
                            .map_or_else(
                                // A role change may have left the account
                                // without Trash.
                                |e| assert!(matches!(e, crate::Error::NotFound { .. }), "{e}"),
                                drop,
                            );
                    }
                }
                6 if !ids.is_empty() => {
                    // A server-side expunge, or a newer copy of the message
                    // with another date.
                    let (id, account) = ids[rng.below(ids.len())].clone();
                    if rng.chance(50) {
                        store.delete_message(&id.as_str().into()).unwrap();
                    } else if let Ok(stored) = store.get_message(&id.as_str().into()) {
                        let mut changed = summary(
                            &id,
                            &account,
                            stored.summary.folder_id.as_str(),
                            rng.below(500),
                            true,
                        );
                        changed.subject = stored.summary.subject.clone();
                        store
                            .merge_messages(&[SyncedMessage {
                                message: StoredMessage::new(changed),
                                meta: None,
                            }])
                            .unwrap();
                    }
                }
                7 if !ids.is_empty() => {
                    // Flags change nothing in the table.
                    let (id, _) = ids[rng.below(ids.len())].clone();
                    if store.message_exists(&id.as_str().into()).unwrap() {
                        store
                            .update_flags(
                                &id.as_str().into(),
                                Flags {
                                    seen: rng.chance(50),
                                    flagged: rng.chance(50),
                                    ..Flags::default()
                                },
                            )
                            .unwrap();
                    }
                }
                8 => {
                    let account = if rng.chance(70) { "a1" } else { "a2" };
                    let folders = store.list_folders(&account.into()).unwrap();
                    let mut changed = folders[rng.below(folders.len())].clone();
                    changed.role = STORE_ROLES[rng.below(STORE_ROLES.len())];
                    store.upsert_folder(&changed).unwrap();
                }
                9 if rng.chance(30) => {
                    store.rethread_all().unwrap();
                }
                _ => {}
            }
            assert_store_consistent(&store, &context);
            assert_pages_match(&store, &context);
        }
        // A folder and an account go.
        let folders = store.list_folders(&"a1".into()).unwrap();
        store.delete_folder(&folders[0].id).unwrap();
        assert_store_consistent(&store, &format!("seed {seed}: folder removed"));
        assert_pages_match(&store, &format!("seed {seed}: folder removed"));
        store.delete_account(&"a2".into()).unwrap();
        assert_store_consistent(&store, &format!("seed {seed}: account removed"));
        let left: i64 = store
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM thread_activity WHERE folder_id LIKE 'a2-%'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(left, 0);
    }
}

/// A list whose Sent folders a grant narrowed orders by the visible
/// messages only (the grouping query): hidden replies must not move a row.
#[test]
fn a_narrowed_sent_falls_back_to_the_grouping_query() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_account(&Account {
            id: "a1".into(),
            name: "a1".into(),
            email: "me@a1".into(),
            kind: AccountKind::Imap,
        })
        .unwrap();
    for (id, role) in [("in", FolderRole::Inbox), ("sent", FolderRole::Sent)] {
        store.upsert_folder(&folder(id, "a1", role)).unwrap();
    }
    let mail = |id: &str, folder: &str, day: usize, refs: &[&str]| SyncedMessage {
        message: StoredMessage::new(summary(id, "a1", folder, day, true)),
        meta: Some(MessageMeta {
            message_id: Some(format!("<{id}@x>")),
            in_reply_to: refs.iter().map(|r| format!("<{r}@x>")).collect(),
            references: refs.iter().map(|r| format!("<{r}@x>")).collect(),
            size: None,
            remote_thread_id: None,
        }),
    };
    store
        .merge_messages(&[
            mail("old", "in", 1, &[]),
            mail("new", "in", 5, &[]),
            mail("reply", "sent", 9, &["old"]),
        ])
        .unwrap();
    let ids = [FolderId::from("in")];
    let full = store.conversation_folders(&ids).unwrap();
    let subjects = |scope| -> Vec<String> {
        store
            .list_conversations(scope, &crate::OwnAddresses::new(), 0, 50)
            .unwrap()
            .threads
            .into_iter()
            .map(|t| t.latest_message_id.as_str().to_owned())
            .collect()
    };
    assert_eq!(subjects(&full), ["old", "new"], "the reply moves `old` up");
    let mut narrowed = full.clone();
    narrowed.retain(|f| f.id.as_str() != "sent");
    store
        .with_tx(|tx| {
            assert!(covers(tx, &full)?);
            assert!(!covers(tx, &narrowed)?);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        subjects(&narrowed),
        ["new", "old"],
        "no hidden reply counts"
    );
}

/// The documented deviation: a unified list over two accounts orders a
/// thread by the Sent folders of the account it is listed in.
#[test]
fn a_reply_from_another_account_does_not_reorder_a_unified_list() {
    let mut conn = v16();
    migrate(&mut conn).unwrap();
    conn.execute_batch(
        r"
        INSERT INTO accounts VALUES ('a2', 'B', 'b@example.org', 'imap');
        INSERT INTO folders (id, account_id, name, path, role) VALUES
            ('in2', 'a2', 'Inbox', 'INBOX', 'inbox'),
            ('sent2', 'a2', 'Sent', 'Sent', 'sent');
        INSERT INTO messages (id, account_id, folder_id, date, date_sort, thread_id) VALUES
            ('b1', 'a2', 'sent2', '', 900, 't2');
        ",
    )
    .unwrap();
    assert_consistent(&conn, "a reply from a2");
    // In a1's Inbox, t2 keeps its own key (250), below t1 (300).
    assert_eq!(page_keys(&conn, "in"), ["t1", "t2"]);
}

#[test]
fn hot_queries_are_index_ranges() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|conn| {
            let p = plan(conn, PAGE_SQL, &[&"in", &50_i64, &0_i64]);
            assert!(p.contains("thread_activity_order"), "{p}");
            assert!(!p.contains("TEMP B-TREE"), "{p}");
            assert_no_scan(&p);

            let p = plan(conn, COUNT_SQL, &[&"in"]);
            assert!(p.contains("SEARCH thread_activity"), "{p}");
            assert_no_scan(&p);

            let p = plan(conn, COUNT_MANY_SQL, &[&"[\"in\",\"ar\"]"]);
            assert!(p.contains("SEARCH thread_activity"), "{p}");
            assert_no_scan(&p);

            let p = plan(conn, SINGLES_PAGE_SQL, &[&"drafts", &50_i64]);
            assert!(p.contains("messages_folder_date"), "{p}");
            assert!(!p.contains("TEMP B-TREE"), "{p}");
            assert_no_scan(&p);

            // The fallback reads the covering folder-thread index (v16).
            let p = plan(
                conn,
                GROUPING_PAGE_SQL,
                &[&"[\"in\"]", &"[\"sent\"]", &"[]", &50_i64, &0_i64],
            );
            assert!(
                p.contains("USING COVERING INDEX messages_folder_thread"),
                "{p}"
            );
            assert!(!p.contains("messages_thread_id"), "{p}");
            assert_no_scan(&p);
            Ok(())
        })
        .unwrap();
}

/// The trigger statements (with the row's values as parameters) read the
/// folder-thread index and the table's keys, never a whole table: every
/// message write pays a few index lookups.
#[test]
fn trigger_statements_are_index_lookups() {
    let store = Store::open_in_memory().unwrap();
    let bind = |sql: &str| {
        sql.replace("NEW.folder_id", "?1")
            .replace("NEW.thread_id", "?2")
            .replace("NEW.account_id", "?3")
    };
    let statements: Vec<String> = [
        refresh_row!("NEW"),
        refresh_sent!("NEW"),
        refresh_account!("NEW.account_id"),
    ]
    .iter()
    .flat_map(|sql| sql.split(';'))
    .map(bind)
    .filter(|s| !s.trim().is_empty())
    .collect();
    assert_eq!(statements.len(), 4);
    store
        .with_conn(|conn| {
            for sql in &statements {
                let count = (1..=3)
                    .filter(|n| sql.contains(&format!("?{n}")))
                    .max()
                    .unwrap_or(0);
                let args: Vec<&dyn rusqlite::ToSql> = (0..count)
                    .map(|_| -> &dyn rusqlite::ToSql { &"x" })
                    .collect();
                let p = plan(conn, sql, &args);
                assert_no_scan(&p);
                if sql.contains("FROM messages m JOIN folders") {
                    assert!(p.contains("messages_folder_thread"), "{sql}\n{p}");
                }
            }
            Ok(())
        })
        .unwrap();
}
