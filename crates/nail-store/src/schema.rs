//! Numbered schema migrations.
//!
//! [`MIGRATIONS`]`[n]` upgrades a database from version `n` to `n + 1`. Each
//! migration runs in its own `IMMEDIATE` transaction together with the row
//! that records it in `schema_version`, so a crash leaves the database at
//! a whole version. Migrations are append-only: never edit a released one,
//! add a new one instead.

use rusqlite::{Connection, TransactionBehavior};

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

/// All migrations; index `n` upgrades version `n` to `n + 1`.
pub(crate) const MIGRATIONS: &[&str] = &[V1];

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

pub(crate) fn migrate_with(conn: &mut Connection, migrations: &[&str]) -> Result<u32> {
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
        tx.execute_batch(migrations[version as usize])?;
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
                "folders",
                "messages",
                "op_queue",
                "schema_version"
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
    fn applies_only_missing_migrations_in_order() {
        let mut conn = Connection::open_in_memory().unwrap();
        let first = ["CREATE TABLE a (x INTEGER)"];
        assert_eq!(migrate_with(&mut conn, &first).unwrap(), 1);
        let both = [
            "CREATE TABLE a (x INTEGER)", // would fail if re-applied
            "ALTER TABLE a ADD COLUMN y INTEGER; INSERT INTO a VALUES (1, 2)",
        ];
        assert_eq!(migrate_with(&mut conn, &both).unwrap(), 2);
        let y: i64 = conn.query_row("SELECT y FROM a", [], |r| r.get(0)).unwrap();
        assert_eq!(y, 2);
    }

    #[test]
    fn failed_migration_rolls_back_whole_step() {
        let mut conn = Connection::open_in_memory().unwrap();
        let broken = ["CREATE TABLE ok (x INTEGER); CREATE TABLE broken (;"];
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
