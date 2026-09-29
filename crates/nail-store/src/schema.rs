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

/// Schema v4 (M1b WP20, ADR 0022): rich drafts, collected addresses,
/// rule runs, MCP clients and saved searches.
///
/// * Drafts keep their full `nail-api` JSON in `payload`; the new columns
///   index what the core queries (mode, reply / forward context, the copy
///   in the server's Drafts folder), and `draft_attachments` lists the
///   blobs a draft references so blob garbage collection keeps them.
/// * Rules themselves live in `rules.kdl` (the source of truth, like
///   `accounts.kdl`); the store keeps their runtime state, the run history
///   with every queued action and its inverse (undo as one unit), and the
///   messages waiting for arrival rules.
/// * MCP clients are stored with a SHA-256 hash of their token only.
const V4: &str = r#"
ALTER TABLE drafts ADD COLUMN mode TEXT NOT NULL DEFAULT 'plain'
    CHECK (mode IN ('plain', 'html', 'markdown'));
ALTER TABLE drafts ADD COLUMN context_mode TEXT
    CHECK (context_mode IN ('reply', 'reply-all', 'forward'));
ALTER TABLE drafts ADD COLUMN context_message_id TEXT;
-- The copy in the server's Drafts folder (autosave); NULL until uploaded.
ALTER TABLE drafts ADD COLUMN server_folder_id TEXT;
ALTER TABLE drafts ADD COLUMN server_uid INTEGER;
-- Message-ID header of the uploaded copy (to find it again after UIDVALIDITY changes).
ALTER TABLE drafts ADD COLUMN server_message_id TEXT;
CREATE INDEX drafts_context ON drafts(context_message_id) WHERE context_message_id IS NOT NULL;

CREATE TABLE draft_attachments (
    draft_id   INTEGER NOT NULL REFERENCES drafts(id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    filename   TEXT NOT NULL,
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    blob_hash  TEXT NOT NULL,
    content_id TEXT,
    PRIMARY KEY (draft_id, position)
) WITHOUT ROWID;
CREATE INDEX draft_attachments_blob ON draft_attachments(blob_hash);

-- Addresses seen in sent and received mail, for autocomplete.
CREATE TABLE collected_addresses (
    email     TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
    name      TEXT,
    use_count INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds (UTC).
    last_used INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX collected_addresses_rank ON collected_addresses(use_count DESC, last_used DESC);

CREATE TABLE rule_state (
    rule_id     TEXT PRIMARY KEY NOT NULL,
    -- Unix seconds (UTC).
    last_run_at INTEGER,
    next_run_at INTEGER,
    run_count   INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE TABLE rule_runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id     TEXT NOT NULL,
    rule_name   TEXT NOT NULL,
    cause       TEXT NOT NULL CHECK (cause IN ('arrival', 'on-send', 'schedule', 'manual')),
    state       TEXT NOT NULL DEFAULT 'running'
                CHECK (state IN ('running', 'done', 'failed', 'undone')),
    -- Unix seconds (UTC).
    started_at  INTEGER NOT NULL,
    finished_at INTEGER,
    matched     INTEGER NOT NULL DEFAULT 0,
    message     TEXT
);
CREATE INDEX rule_runs_rule ON rule_runs(rule_id, id DESC);

-- Every action a run took, with the queued operation and its inverse.
CREATE TABLE rule_run_actions (
    run_id     INTEGER NOT NULL REFERENCES rule_runs(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    message_id TEXT NOT NULL,
    -- op_queue id; NULL for actions without a queued operation (notify, stop).
    op_id      INTEGER,
    -- nail_api::rules::RuleAction JSON.
    action     TEXT NOT NULL,
    -- nail_store::Operation JSON that undoes it; NULL = cannot be undone.
    undo       TEXT,
    PRIMARY KEY (run_id, seq)
) WITHOUT ROWID;

-- New messages waiting for the arrival rules (survives a restart).
CREATE TABLE rule_inbox (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    -- Unix seconds (UTC).
    queued_at  INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE mcp_clients (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    name                    TEXT NOT NULL,
    -- Lowercase hex SHA-256 of the bearer token; the token itself is never stored.
    token_hash              TEXT NOT NULL UNIQUE,
    token_hint              TEXT NOT NULL,
    -- Unix seconds (UTC).
    created_at              INTEGER NOT NULL,
    last_used_at            INTEGER,
    revoked_at              INTEGER,
    all_accounts            INTEGER NOT NULL DEFAULT 0,
    skip_draft_confirmation INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX mcp_clients_active_name ON mcp_clients(name) WHERE revoked_at IS NULL;

-- Grant rows: scopes, accounts and excluded folders.
CREATE TABLE mcp_grants (
    client_id INTEGER NOT NULL REFERENCES mcp_clients(id) ON DELETE CASCADE,
    kind      TEXT NOT NULL CHECK (kind IN ('scope', 'account', 'exclude-folder')),
    value     TEXT NOT NULL,
    PRIMARY KEY (client_id, kind, value)
) WITHOUT ROWID;

CREATE TABLE mcp_audit (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id   INTEGER REFERENCES mcp_clients(id) ON DELETE SET NULL,
    client_name TEXT NOT NULL,
    -- Unix seconds (UTC).
    at          INTEGER NOT NULL,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    outcome     TEXT NOT NULL,
    detail      TEXT
);
CREATE INDEX mcp_audit_client ON mcp_audit(client_id, id DESC);

CREATE TABLE saved_searches (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    query      TEXT NOT NULL,
    -- No foreign keys: may name virtual accounts / folders.
    account_id TEXT,
    folder_id  TEXT,
    sort       TEXT NOT NULL DEFAULT 'date' CHECK (sort IN ('date', 'relevance')),
    position   INTEGER NOT NULL
);
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
    Migration {
        sql: V4,
        backfill: Some(crate::compose::backfill_v4),
    },
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
                "collected_addresses",
                "draft_attachments",
                "drafts",
                "folders",
                "mcp_audit",
                "mcp_clients",
                "mcp_grants",
                "message_tags",
                "messages",
                "op_queue",
                "outbox",
                "pop3_uids",
                "rule_inbox",
                "rule_run_actions",
                "rule_runs",
                "rule_state",
                "saved_searches",
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
    fn v4_upgrades_m1a_drafts() {
        // A database as M1a (schema v2 / v3) left it: a plain reply draft
        // with `in_reply_to` and a new one, one of them in the outbox.
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        assert_eq!(migrate_to_version(&mut conn, 3).unwrap(), 3);
        conn.execute_batch(
            r#"
            INSERT INTO accounts VALUES ('a1', 'A', 'a@example.org', 'imap');
            INSERT INTO drafts (identity_id, payload) VALUES ('ada@example.org',
                '{"identity_id":"ada@example.org","to":[],"cc":[],"bcc":[],"subject":"Re: Hi",
                  "mode":"plain","text":"> hi","html":null,"markdown":null,"attachments":[],
                  "in_reply_to":"m7"}');
            INSERT INTO drafts (identity_id, payload) VALUES ('ada@example.org',
                '{"identity_id":"ada@example.org","subject":"New","text":"x","in_reply_to":null}');
            INSERT INTO outbox (draft_id, account_id, identity_id, send_at) VALUES (2, 'a1', 'ada@example.org', 0);
            "#,
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);

        let rows: Vec<(String, String, Option<String>, Option<String>)> = {
            let mut stmt = conn
                .prepare("SELECT payload, mode, context_mode, context_message_id FROM drafts ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        let reply: nail_api::Draft = serde_json::from_str(&rows[0].0).unwrap();
        assert_eq!(
            reply.context,
            Some(nail_api::DraftContext {
                mode: nail_api::DraftContextMode::Reply,
                message_id: "m7".into(),
            })
        );
        assert!(!rows[0].0.contains("in_reply_to"));
        assert_eq!(
            (
                rows[0].1.as_str(),
                rows[0].2.as_deref(),
                rows[0].3.as_deref()
            ),
            ("plain", Some("reply"), Some("m7"))
        );
        let new: nail_api::Draft = serde_json::from_str(&rows[1].0).unwrap();
        assert_eq!((new.subject.as_str(), new.context), ("New", None));
        assert_eq!(rows[1].2, None);
        let outbox: i64 = conn
            .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(outbox, 1);
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
