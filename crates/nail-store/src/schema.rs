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

/// Schema v5 (M2a WP30, ADR 0029): the keyring, Autocrypt peer state,
/// trust decisions, encryption-at-rest state, cached message security and
/// a key/value settings table.
///
/// * `crypto_keys` holds OpenPGP keys and S/MIME certificates: metadata
///   the UI lists, the public part (binary OpenPGP packets / DER) and, for
///   own keys, the **encrypted** secret part (OpenPGP S2K-protected or
///   wrapped with a keychain key; PKCS#8 for S/MIME) or a reference to
///   where it lives (keychain entry, Windows store thumbprint). Nothing in
///   this database is ever a plain secret key.
/// * `crypto_key_emails` indexes keys by address for recipient lookup.
/// * `crypto_trust` keeps the user's decisions apart from the key rows, so
///   deleting and re-importing a key keeps them.
/// * `autocrypt_peers` is Autocrypt Level 1 peer state, per own account.
/// * `vault_state` is a single row describing encryption at rest and a
///   running migration (for crash-safe resume).
/// * `message_security` caches what reading a message found (security
///   status and trusted `Authentication-Results`, never decrypted
///   content), for list badges and rule conditions.
/// * `app_settings` holds small JSON settings such as
///   [`nail_api::security::SecuritySettings`].
const V5: &str = r#"
CREATE TABLE crypto_keys (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    technology        TEXT NOT NULL CHECK (technology IN ('openpgp', 'smime')),
    -- OpenPGP: primary key fingerprint; S/MIME: SHA-256 of the DER
    -- certificate. Uppercase hex.
    fingerprint       TEXT NOT NULL,
    -- JSON array of user ids (OpenPGP) or the subject (S/MIME), primary first.
    user_ids          TEXT NOT NULL DEFAULT '[]',
    algorithm         TEXT NOT NULL,
    -- Unix seconds (UTC).
    created_at        INTEGER NOT NULL,
    expires_at        INTEGER,
    revoked           INTEGER NOT NULL DEFAULT 0,
    can_sign          INTEGER NOT NULL DEFAULT 0,
    can_encrypt       INTEGER NOT NULL DEFAULT 0,
    source            TEXT NOT NULL CHECK (source IN ('generated', 'imported', 'pkcs12',
                          'windows-store', 'autocrypt', 'autocrypt-gossip', 'setup-message',
                          'wkd', 'keyserver', 'message')),
    -- Transferable public key (binary packets) or DER certificate.
    public_data       BLOB NOT NULL,
    -- NULL = no secret part (a correspondent's key).
    secret_storage    TEXT CHECK (secret_storage IN ('keychain', 'passphrase', 'windows-store')),
    -- Encrypted secret material; NULL for 'windows-store'.
    secret_data       BLOB,
    -- Keychain reference (`keychain:…`) of the wrapping key, or the
    -- Windows store thumbprint.
    secret_ref        TEXT,
    -- S/MIME only.
    issuer            TEXT,
    serial            TEXT,
    imported_at       INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    UNIQUE (technology, fingerprint),
    CHECK (secret_storage IS NOT NULL OR (secret_data IS NULL AND secret_ref IS NULL))
);

CREATE TABLE crypto_key_emails (
    key_id INTEGER NOT NULL REFERENCES crypto_keys(id) ON DELETE CASCADE,
    email  TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (key_id, email)
) WITHOUT ROWID;
CREATE INDEX crypto_key_emails_email ON crypto_key_emails(email);

CREATE TABLE crypto_trust (
    technology  TEXT NOT NULL CHECK (technology IN ('openpgp', 'smime')),
    fingerprint TEXT NOT NULL,
    decision    TEXT NOT NULL CHECK (decision IN ('verified', 'rejected')),
    -- Unix seconds (UTC).
    decided_at  INTEGER NOT NULL,
    PRIMARY KEY (technology, fingerprint)
) WITHOUT ROWID;

-- Autocrypt Level 1 peer state. No foreign key on account_id: peers of a
-- removed account are removed by the core with the account.
CREATE TABLE autocrypt_peers (
    account_id          TEXT NOT NULL,
    email               TEXT NOT NULL COLLATE NOCASE,
    -- Unix seconds (UTC) of the newest message seen from the peer.
    last_seen           INTEGER NOT NULL,
    -- Date of the newest message with an Autocrypt header.
    autocrypt_timestamp INTEGER,
    -- Fingerprint of the key from that header (a crypto_keys row).
    key_fingerprint     TEXT,
    prefer_encrypt      TEXT NOT NULL DEFAULT 'nopreference'
                        CHECK (prefer_encrypt IN ('mutual', 'nopreference')),
    gossip_timestamp    INTEGER,
    gossip_fingerprint  TEXT,
    PRIMARY KEY (account_id, email)
) WITHOUT ROWID;

CREATE TABLE vault_state (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    state       TEXT NOT NULL DEFAULT 'plain'
                CHECK (state IN ('plain', 'encrypting', 'encrypted', 'decrypting')),
    phase       TEXT CHECK (phase IN ('database', 'blobs', 'index')),
    done        INTEGER NOT NULL DEFAULT 0,
    total       INTEGER NOT NULL DEFAULT 0,
    -- Keychain reference of the data key.
    key_ref     TEXT,
    -- A value encrypted with the data key, to detect a wrong key.
    key_check   BLOB,
    -- Unix seconds (UTC).
    started_at  INTEGER,
    finished_at INTEGER,
    -- Last migration error (English diagnostic), NULL when none.
    error       TEXT
);
INSERT INTO vault_state (id) VALUES (1);

CREATE TABLE message_security (
    message_id   TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    signed       INTEGER NOT NULL DEFAULT 0,
    encrypted    INTEGER NOT NULL DEFAULT 0,
    -- nail_api::security::SecurityStatus JSON; NULL = neither.
    status       TEXT,
    -- nail_api::security::AuthResults JSON; NULL = none trusted.
    auth_results TEXT,
    -- Unix seconds (UTC).
    checked_at   INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY NOT NULL,
    -- JSON.
    value TEXT NOT NULL
) WITHOUT ROWID;
"#;

/// Schema v6 (M2b WP40, ADR 0036): the remote UI and remote content.
///
/// **Only hashed or referenced secrets.** Passwords are Argon2id PHC
/// strings (a `CHECK` refuses anything else); TOTP secrets live in the OS
/// keychain and only their `keychain:` reference is stored; device
/// secrets, session tokens and pairing codes are stored as SHA-256 (hex).
/// Passkey rows hold the public credential (public key, counter).
///
/// * `remote_users` — login name, credentials (as above), grant ceiling
///   (`nail_api::remote::RemoteGrant` JSON), disabled / lockout state and
///   the last accepted TOTP step (replay protection).
/// * `remote_devices` — paired browsers with the hash of their device
///   cookie and their grant.
/// * `remote_passkeys` — WebAuthn credentials per user.
/// * `remote_sessions` — sessions with the hash of the current token and,
///   for a short grace period after a rotation, of the previous one.
/// * `remote_pairing_codes` — one-time codes (hash), single use.
/// * `remote_throttle` — failed-login counters per address and per login
///   name, so backoff and lockouts survive a restart.
/// * `remote_audit` — the remote-UI audit log.
/// * `remote_content_allow` — remote-content allow-lists (per message,
///   sender, domain). Message entries have no foreign key (ids are
///   strings, also of virtual messages); the core removes stale ones.
const V6: &str = r#"
CREATE TABLE remote_users (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name   TEXT,
    -- Argon2id PHC string; NULL = no password.
    password_hash  TEXT CHECK (password_hash IS NULL OR substr(password_hash, 1, 10) = '$argon2id$'),
    -- Keychain reference of the TOTP secret; NULL = no TOTP.
    totp_ref       TEXT CHECK (totp_ref IS NULL OR substr(totp_ref, 1, 9) = 'keychain:'),
    -- Last accepted TOTP time step (a code is accepted once).
    totp_last_step INTEGER,
    -- nail_api::remote::RemoteGrant JSON: the most devices and sessions get.
    grant_json     TEXT NOT NULL DEFAULT '{}',
    disabled       INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds (UTC).
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    last_login_at  INTEGER,
    locked_until   INTEGER
);

CREATE TABLE remote_devices (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES remote_users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- SHA-256 (hex) of the device cookie's secret.
    secret_hash  TEXT NOT NULL UNIQUE,
    grant_json   TEXT NOT NULL DEFAULT '{}',
    -- Unix seconds (UTC).
    paired_at    INTEGER NOT NULL,
    last_seen_at INTEGER,
    user_agent   TEXT,
    last_ip      TEXT,
    revoked_at   INTEGER
);
CREATE INDEX remote_devices_user ON remote_devices(user_id);

CREATE TABLE remote_passkeys (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id       INTEGER NOT NULL REFERENCES remote_users(id) ON DELETE CASCADE,
    -- WebAuthn credential id (public).
    credential_id BLOB NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    -- The WebAuthn library's serialized credential (public key, algorithm,
    -- signature counter, flags); no secret.
    credential    TEXT NOT NULL,
    device_id     INTEGER REFERENCES remote_devices(id) ON DELETE SET NULL,
    -- Unix seconds (UTC).
    created_at    INTEGER NOT NULL,
    last_used_at  INTEGER
);
CREATE INDEX remote_passkeys_user ON remote_passkeys(user_id);

CREATE TABLE remote_sessions (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id              INTEGER NOT NULL REFERENCES remote_users(id) ON DELETE CASCADE,
    device_id            INTEGER NOT NULL REFERENCES remote_devices(id) ON DELETE CASCADE,
    -- SHA-256 (hex) of the session token.
    token_hash           TEXT NOT NULL UNIQUE,
    -- The token before the last rotation, accepted until previous_valid_until.
    previous_token_hash  TEXT UNIQUE,
    previous_valid_until INTEGER,
    auth_method          TEXT NOT NULL
                         CHECK (auth_method IN ('passkey', 'password', 'password-totp', 'pairing')),
    grant_json           TEXT NOT NULL DEFAULT '{}',
    -- Unix seconds (UTC).
    created_at           INTEGER NOT NULL,
    last_seen_at         INTEGER NOT NULL,
    expires_at           INTEGER NOT NULL,
    rotated_at           INTEGER NOT NULL,
    reauth_at            INTEGER,
    ip                   TEXT,
    user_agent           TEXT,
    ended_at             INTEGER,
    end_reason           TEXT CHECK (end_reason IN ('logout', 'revoked', 'expired',
                             'device-revoked', 'user-changed'))
);
CREATE INDEX remote_sessions_user ON remote_sessions(user_id, id DESC);
CREATE INDEX remote_sessions_device ON remote_sessions(device_id);

CREATE TABLE remote_pairing_codes (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES remote_users(id) ON DELETE CASCADE,
    -- SHA-256 (hex) of the normalized code.
    code_hash    TEXT NOT NULL UNIQUE,
    device_name  TEXT,
    grant_json   TEXT NOT NULL DEFAULT '{}',
    -- Unix seconds (UTC).
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    used_at      INTEGER,
    cancelled_at INTEGER,
    device_id    INTEGER REFERENCES remote_devices(id) ON DELETE SET NULL
);

-- Failed-login counters: key 'ip:<address>' or 'user:<lowercase name>'.
CREATE TABLE remote_throttle (
    key              TEXT PRIMARY KEY NOT NULL,
    failures         INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds (UTC).
    first_failure_at INTEGER NOT NULL,
    last_failure_at  INTEGER NOT NULL,
    locked_until     INTEGER
) WITHOUT ROWID;

CREATE TABLE remote_audit (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Unix seconds (UTC).
    at         INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    outcome    TEXT NOT NULL,
    user_id    INTEGER REFERENCES remote_users(id) ON DELETE SET NULL,
    user_name  TEXT,
    device_id  INTEGER REFERENCES remote_devices(id) ON DELETE SET NULL,
    session_id INTEGER REFERENCES remote_sessions(id) ON DELETE SET NULL,
    ip         TEXT,
    user_agent TEXT,
    method     TEXT,
    name       TEXT,
    error      TEXT,
    detail     TEXT
);
CREATE INDEX remote_audit_user ON remote_audit(user_id, id DESC);

CREATE TABLE remote_content_allow (
    scope      TEXT NOT NULL CHECK (scope IN ('message', 'sender', 'domain')),
    -- Message id, lowercase address or lowercase domain.
    value      TEXT NOT NULL,
    label      TEXT,
    -- Unix seconds (UTC).
    created_at INTEGER NOT NULL,
    PRIMARY KEY (scope, value)
) WITHOUT ROWID;
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
    Migration::sql(V5),
    Migration::sql(V6),
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
                "app_settings",
                "autocrypt_peers",
                "collected_addresses",
                "crypto_key_emails",
                "crypto_keys",
                "crypto_trust",
                "draft_attachments",
                "drafts",
                "folders",
                "mcp_audit",
                "mcp_clients",
                "mcp_grants",
                "message_security",
                "message_tags",
                "messages",
                "op_queue",
                "outbox",
                "pop3_uids",
                "remote_audit",
                "remote_content_allow",
                "remote_devices",
                "remote_pairing_codes",
                "remote_passkeys",
                "remote_sessions",
                "remote_throttle",
                "remote_users",
                "rule_inbox",
                "rule_run_actions",
                "rule_runs",
                "rule_state",
                "saved_searches",
                "schema_version",
                "thread_refs",
                "vault_state"
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
