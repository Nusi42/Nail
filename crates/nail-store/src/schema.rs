//! Numbered schema migrations.
//!
//! [`MIGRATIONS`]`[n]` upgrades a database from version `n` to `n + 1`. Each
//! migration runs in its own `IMMEDIATE` transaction together with the row
//! that records it in `schema_version`, so a crash leaves the database at
//! a whole version. Migrations are append-only: never edit a released one,
//! add a new one instead.

use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{Error, Result};

/// Schema v1: accounts, folders, messages and the operation queue.
const V1: &str = r#"
CREATE TABLE accounts (
    id    TEXT PRIMARY KEY NOT NULL,
    name  TEXT NOT NULL,
    email TEXT NOT NULL,
    kind  TEXT NOT NULL
);

CREATE TABLE folders (
    id            TEXT PRIMARY KEY NOT NULL,
    account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    parent_id     TEXT REFERENCES folders(id) ON DELETE SET NULL,
    name          TEXT NOT NULL,
    path          TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'other',
    uidvalidity   INTEGER,
    uidnext       INTEGER,
    highestmodseq INTEGER,
    UNIQUE (account_id, path)
);
CREATE INDEX folders_parent ON folders(parent_id);

CREATE TABLE messages (
    id              TEXT PRIMARY KEY NOT NULL,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid             INTEGER,
    subject         TEXT NOT NULL DEFAULT '',
    from_name       TEXT,
    from_email      TEXT,
    to_json         TEXT NOT NULL DEFAULT '[]',
    date            TEXT NOT NULL,
    -- Unix seconds (UTC) parsed from `date`; 0 when unparseable. Sort key.
    date_sort       INTEGER NOT NULL DEFAULT 0,
    seen            INTEGER NOT NULL DEFAULT 0,
    flagged         INTEGER NOT NULL DEFAULT 0,
    answered        INTEGER NOT NULL DEFAULT 0,
    draft           INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    preview         TEXT NOT NULL DEFAULT '',
    blob_hash       TEXT,
    thread_key      TEXT
);
CREATE UNIQUE INDEX messages_folder_uid ON messages(folder_id, uid) WHERE uid IS NOT NULL;
CREATE INDEX messages_folder_date ON messages(folder_id, date_sort DESC, id DESC);
CREATE INDEX messages_folder_unread ON messages(folder_id) WHERE seen = 0;
CREATE INDEX messages_account ON messages(account_id);
CREATE INDEX messages_thread ON messages(thread_key) WHERE thread_key IS NOT NULL;
CREATE INDEX messages_blob ON messages(blob_hash) WHERE blob_hash IS NOT NULL;

CREATE TABLE op_queue (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    payload    TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'pending'
               CHECK (state IN ('pending', 'in_flight', 'done', 'failed')),
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX op_queue_state ON op_queue(state, id);
"#;

/// Schema v2 (M1a): threading headers and thread ids, tags, message size,
/// per-message operation index, drafts and the outbox. Identities and
/// virtual accounts are **not** stored: `accounts.kdl` is their only source
/// of truth (ADR 0012). `folders.highestmodseq` exists since v1.
const V2: &str = r#"
-- Message-ID without angle brackets; NULL when the message has none.
ALTER TABLE messages ADD COLUMN message_id TEXT;
ALTER TABLE messages ADD COLUMN in_reply_to_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE messages ADD COLUMN references_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE messages ADD COLUMN thread_id TEXT;
-- Normalized subject (prefixes stripped, lowercase) for the subject fallback.
ALTER TABLE messages ADD COLUMN subject_key TEXT NOT NULL DEFAULT '';
-- Whether the subject carried a reply / forward prefix.
ALTER TABLE messages ADD COLUMN subject_is_reply INTEGER NOT NULL DEFAULT 0;
-- Size of the raw message in bytes; NULL while unknown.
ALTER TABLE messages ADD COLUMN size INTEGER;
CREATE INDEX messages_message_id ON messages(message_id) WHERE message_id IS NOT NULL;
CREATE INDEX messages_thread_id ON messages(thread_id, date_sort);
CREATE INDEX messages_subject_key ON messages(subject_key, date_sort) WHERE subject_key <> '';
CREATE INDEX messages_date ON messages(date_sort DESC, id DESC);

-- Tags (IMAP keywords), one row per tag.
CREATE TABLE message_tags (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    tag        TEXT NOT NULL,
    PRIMARY KEY (message_id, tag)
) WITHOUT ROWID;
CREATE INDEX message_tags_tag ON message_tags(tag COLLATE NOCASE);

-- Every Message-ID seen (own ids and referenced ones, even when that
-- message is not stored) mapped to its thread: the incremental JWZ state.
CREATE TABLE thread_refs (
    msgid     TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL
) WITHOUT ROWID;
CREATE INDEX thread_refs_thread ON thread_refs(thread_id);

-- Operation queue: the message an operation is about (sync merge) and
-- per-account passes.
ALTER TABLE op_queue ADD COLUMN message_id TEXT;
UPDATE op_queue SET message_id = json_extract(payload, '$.message_id');
CREATE INDEX op_queue_message ON op_queue(message_id, state) WHERE message_id IS NOT NULL;
CREATE INDEX op_queue_account ON op_queue(account_id, state, id);

CREATE TABLE drafts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    identity_id TEXT NOT NULL,
    payload     TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE outbox (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    draft_id        INTEGER NOT NULL,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    identity_id     TEXT NOT NULL,
    subject         TEXT NOT NULL DEFAULT '',
    recipients_json TEXT NOT NULL DEFAULT '[]',
    -- Unix seconds (UTC).
    send_at         INTEGER NOT NULL,
    state           TEXT NOT NULL DEFAULT 'scheduled'
                    CHECK (state IN ('scheduled', 'sending', 'sent', 'failed', 'cancelled')),
    attempts        INTEGER NOT NULL DEFAULT 0,
    error           TEXT,
    message         TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX outbox_due ON outbox(state, send_at);
CREATE INDEX outbox_draft ON outbox(draft_id);
"#;

/// Schema v3 (M1a WP14): POP3 bookkeeping. POP3 has no folders or numeric
/// UIDs on the server; the core remembers which `UIDL`s it downloaded (and
/// when), which local message each became, and whether the user deleted
/// that local copy, for the leave-on-server policies.
const V3: &str = r#"
CREATE TABLE pop3_uids (
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    uidl            TEXT NOT NULL,
    -- The local message; NULL once it was deleted locally.
    message_id      TEXT,
    -- Unix seconds (UTC) of the download.
    downloaded_at   INTEGER NOT NULL,
    deleted_locally INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, uidl)
) WITHOUT ROWID;
CREATE INDEX pop3_uids_message ON pop3_uids(message_id) WHERE message_id IS NOT NULL;
"#;

/// One schema step: SQL plus an optional Rust backfill that runs in the same
/// transaction (for data SQLite cannot compute, e.g. normalized subjects).
#[derive(Clone, Copy)]
pub(crate) struct Migration {
    pub sql: &'static str,
    pub backfill: Option<fn(&Transaction<'_>) -> Result<()>>,
}

impl Migration {
    pub(crate) const fn sql(sql: &'static str) -> Self {
        Self {
            sql,
            backfill: None,
        }
    }
}

/// All migrations; index `n` upgrades version `n` to `n + 1`.
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration::sql(V1),
    Migration {
        sql: V2,
        backfill: Some(crate::thread::backfill_v2),
    },
    Migration::sql(V3),
];

/// The schema version this build creates and understands.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

const CREATE_VERSION_TABLE: &str = "CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER PRIMARY KEY NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
)";

/// The version recorded in the database (0 for an empty database).
pub(crate) fn current_version(conn: &Connection) -> Result<u32> {
    conn.execute_batch(CREATE_VERSION_TABLE)?;
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?)
}

/// Brings the database up to [`SCHEMA_VERSION`] using [`MIGRATIONS`].
/// Returns the resulting version.
pub(crate) fn migrate(conn: &mut Connection) -> Result<u32> {
    migrate_with(conn, MIGRATIONS)
}

/// Brings the database up to `target` only; creates old-version databases
/// for upgrade tests.
#[cfg(test)]
pub(crate) fn migrate_to_version(conn: &mut Connection, target: u32) -> Result<u32> {
    let target = (target as usize).min(MIGRATIONS.len());
    migrate_with(conn, &MIGRATIONS[..target])
}

pub(crate) fn migrate_with(conn: &mut Connection, migrations: &[Migration]) -> Result<u32> {
    let supported = u32::try_from(migrations.len()).unwrap_or(u32::MAX);
    loop {
        // Re-read the version inside the write transaction so two processes
        // opening the same fresh database cannot both apply a migration.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version = current_version(&tx)?;
        if version > supported {
            return Err(Error::SchemaTooNew {
                found: version,
                supported,
            });
        }
        if version == supported {
            tx.commit()?;
            return Ok(version);
        }
        let next = version + 1;
        tracing::info!(from = version, to = next, "migrating store schema");
        let step = migrations[version as usize];
        tx.execute_batch(step.sql)?;
        if let Some(backfill) = step.backfill {
            backfill(&tx)?;
        }
        tx.execute("INSERT INTO schema_version (version) VALUES (?1)", [next])?;
        tx.commit()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn migrates_empty_database_to_latest() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(current_version(&conn).unwrap(), 0);
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        assert_eq!(current_version(&conn).unwrap(), SCHEMA_VERSION);
        assert_eq!(
            tables(&conn),
            [
                "accounts",
                "drafts",
                "folders",
                "message_tags",
                "messages",
                "op_queue",
                "outbox",
                "pop3_uids",
                "schema_version",
                "thread_refs"
            ]
        );
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let rows: u32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, SCHEMA_VERSION);
    }

    #[test]
    fn stops_at_the_requested_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(migrate_to_version(&mut conn, 1).unwrap(), 1);
        assert!(!tables(&conn).contains(&"thread_refs".to_owned()));
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        assert!(tables(&conn).contains(&"thread_refs".to_owned()));
    }

    #[test]
    fn applies_only_missing_migrations_in_order() {
        let mut conn = Connection::open_in_memory().unwrap();
        let first = [Migration::sql("CREATE TABLE a (x INTEGER)")];
        assert_eq!(migrate_with(&mut conn, &first).unwrap(), 1);
        let both = [
            Migration::sql("CREATE TABLE a (x INTEGER)"), // would fail if re-applied
            Migration::sql("ALTER TABLE a ADD COLUMN y INTEGER; INSERT INTO a VALUES (1, 2)"),
        ];
        assert_eq!(migrate_with(&mut conn, &both).unwrap(), 2);
        let y: i64 = conn.query_row("SELECT y FROM a", [], |r| r.get(0)).unwrap();
        assert_eq!(y, 2);
    }

    #[test]
    fn backfill_runs_in_the_migration_transaction() {
        fn fill(tx: &Transaction<'_>) -> Result<()> {
            tx.execute("INSERT INTO a VALUES (7)", [])?;
            Ok(())
        }
        fn broken(_: &Transaction<'_>) -> Result<()> {
            Err(Error::InvalidData("backfill failed".into()))
        }
        let mut conn = Connection::open_in_memory().unwrap();
        let steps = [Migration {
            sql: "CREATE TABLE a (x INTEGER)",
            backfill: Some(fill),
        }];
        migrate_with(&mut conn, &steps).unwrap();
        let x: i64 = conn.query_row("SELECT x FROM a", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 7);

        let mut conn = Connection::open_in_memory().unwrap();
        let steps = [Migration {
            sql: "CREATE TABLE b (x INTEGER)",
            backfill: Some(broken),
        }];
        assert!(migrate_with(&mut conn, &steps).is_err());
        assert_eq!(current_version(&conn).unwrap(), 0);
        assert!(!tables(&conn).contains(&"b".to_owned()));
    }

    #[test]
    fn failed_migration_rolls_back_whole_step() {
        let mut conn = Connection::open_in_memory().unwrap();
        let broken = [Migration::sql(
            "CREATE TABLE ok (x INTEGER); CREATE TABLE broken (;",
        )];
        assert!(migrate_with(&mut conn, &broken).is_err());
        assert_eq!(current_version(&conn).unwrap(), 0);
        assert!(!tables(&conn).contains(&"ok".to_owned()));
    }

    #[test]
    fn refuses_newer_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION + 1],
        )
        .unwrap();
        match migrate(&mut conn) {
            Err(Error::SchemaTooNew { found, supported }) => {
                assert_eq!(found, SCHEMA_VERSION + 1);
                assert_eq!(supported, SCHEMA_VERSION);
            }
            other => panic!("expected SchemaTooNew, got {other:?}"),
        }
    }
}
