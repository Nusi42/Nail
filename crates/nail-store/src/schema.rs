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
const V1: &str = r"
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
    -- Unix seconds (UTC) parsed from `date`; 0 when unparsable. Sort key.
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
";

/// Schema v2 (M1a): threading headers and thread ids, tags, message size,
/// per-message operation index, drafts and the outbox. Identities and
/// virtual accounts are **not** stored: `accounts.kdl` is their only source
/// of truth (ADR 0012). `folders.highestmodseq` exists since v1.
const V2: &str = r"
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
";

/// Schema v3 (M1a WP14): POP3 bookkeeping. POP3 has no folders or numeric
/// UIDs on the server; the core remembers which `UIDL`s it downloaded (and
/// when), which local message each became, and whether the user deleted
/// that local copy, for the leave-on-server policies.
const V3: &str = r"
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
";

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
const V4: &str = r"
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
";

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
const V5: &str = r"
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
";

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
const V6: &str = r"
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
";

/// Schema v7 (M2c WP50, ADR 0043): sync policies, snooze, the junk
/// classifier, notification overrides and the Sieve state.
///
/// * `messages.body_state` — where the body is (`synced`, `headers-only`,
///   `on-demand`; `nail_api::sync::BodyState`). Existing rows are `synced`.
///   The policy itself stays in `accounts.kdl`.
/// * `snoozed_messages` — wake times of snoozed messages (the message sits
///   in the hidden Snoozed folder with the `$Snoozed` keyword). No foreign
///   key to `messages`: a resync may give the message a new row; the core
///   rebinds it by `rfc_message_id` or drops the entry.
/// * `junk_settings`, `junk_tokens`, `junk_training`, `junk_verdicts` — the
///   per-account classifier. Tokens are stored **hashed** (64-bit prefix of
///   SHA-256, [`crate::hash_junk_token`]) so the table does not spell out
///   words of the mail; training is recorded per message (by `Message-ID`)
///   so re-marking a message moves it between classes instead of counting
///   it twice.
/// * `notify_overrides` — per-account / per-folder notification overrides
///   (removed with their account or folder by triggers); the global
///   notification settings live in `app_settings`.
/// * `sieve_state` — per account: hash and time of the uploaded script,
///   the server's extensions and other scripts, the vacation settings and
///   the last error.
const V7: &str = r"
ALTER TABLE messages ADD COLUMN body_state TEXT NOT NULL DEFAULT 'synced'
    CHECK (body_state IN ('synced', 'headers-only', 'on-demand'));
CREATE INDEX messages_headers_only ON messages(folder_id) WHERE body_state = 'headers-only';

CREATE TABLE snoozed_messages (
    message_id     TEXT PRIMARY KEY NOT NULL,
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Unix seconds (UTC).
    until          INTEGER NOT NULL,
    wake_folder_id TEXT NOT NULL,
    snoozed_at     INTEGER NOT NULL,
    -- Message-ID header (without angle brackets), to find the message again.
    rfc_message_id TEXT
) WITHOUT ROWID;
CREATE INDEX snoozed_until ON snoozed_messages(until);

CREATE TABLE junk_settings (
    account_id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- JSON (nail_api::triage::JunkSettings).
    settings   TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE junk_tokens (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- 16 hex characters: the first 64 bits of SHA-256 of the token.
    token      TEXT NOT NULL CHECK (length(token) = 16),
    junk       INTEGER NOT NULL DEFAULT 0 CHECK (junk >= 0),
    ham        INTEGER NOT NULL DEFAULT 0 CHECK (ham >= 0),
    PRIMARY KEY (account_id, token)
) WITHOUT ROWID;

CREATE TABLE junk_training (
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Message-ID header, or `local:<message id>` when there is none.
    message_key TEXT NOT NULL,
    junk        INTEGER NOT NULL CHECK (junk IN (0, 1)),
    -- The hashed tokens it was trained with (JSON array), to untrain it.
    tokens      TEXT NOT NULL DEFAULT '[]',
    trained_at  INTEGER NOT NULL,
    PRIMARY KEY (account_id, message_key)
) WITHOUT ROWID;

CREATE TABLE junk_verdicts (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    junk       INTEGER NOT NULL CHECK (junk IN (0, 1)),
    score      INTEGER CHECK (score IS NULL OR (score >= 0 AND score <= 100)),
    source     TEXT NOT NULL CHECK (source IN ('server', 'classifier', 'user')),
    decided_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE notify_overrides (
    scope     TEXT NOT NULL CHECK (scope IN ('account', 'folder')),
    target_id TEXT NOT NULL,
    notify    INTEGER NOT NULL CHECK (notify IN (0, 1)),
    position  INTEGER NOT NULL,
    PRIMARY KEY (scope, target_id)
) WITHOUT ROWID;
CREATE TRIGGER notify_overrides_account AFTER DELETE ON accounts BEGIN
    DELETE FROM notify_overrides WHERE scope = 'account' AND target_id = OLD.id;
END;
CREATE TRIGGER notify_overrides_folder AFTER DELETE ON folders BEGIN
    DELETE FROM notify_overrides WHERE scope = 'folder' AND target_id = OLD.id;
END;

CREATE TABLE sieve_state (
    account_id    TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- SHA-256 (hex) of the uploaded script; NULL = never uploaded.
    script_hash   TEXT CHECK (script_hash IS NULL OR length(script_hash) = 64),
    -- Unix seconds (UTC).
    uploaded_at   INTEGER,
    -- JSON arrays of the server's Sieve extensions and other scripts.
    extensions    TEXT NOT NULL DEFAULT '[]',
    other_scripts TEXT NOT NULL DEFAULT '[]',
    -- JSON (nail_api::sieve::VacationSettings); NULL = the defaults.
    vacation      TEXT,
    -- JSON (nail_api::ApiError) of the last failure; NULL = none.
    last_error    TEXT
) WITHOUT ROWID;
";

/// Schema v8 (M2c review fixes, ADR 0049):
///
/// * `sieve_state.location` — where the uploaded script lives (JSON:
///   host, port, TLS mode, script name), so switching ManageSieve off or
///   changing the host, port or script name removes Nail's script from the
///   place it was uploaded to, not from the current settings' place.
/// * `sieve_state.server_rules` — the rules the uploaded (and still active)
///   script runs (JSON array of rule ids). Arrival rules skip them for the
///   account's mail; it survives restarts and failed uploads (the script
///   stays active on the server) and is cleared only when the script is
///   removed.
/// * `folder_marks` — folders Nail created for itself (`snoozed`: the
///   hidden Snoozed folder). A user folder that happens to be called
///   `Snoozed` is not marked and stays visible. Removed with the folder.
///   Existing top-level `Snoozed` folders holding snoozed mail are marked.
const V8: &str = r"
ALTER TABLE sieve_state ADD COLUMN location TEXT;
ALTER TABLE sieve_state ADD COLUMN server_rules TEXT NOT NULL DEFAULT '[]';

CREATE TABLE folder_marks (
    folder_id  TEXT PRIMARY KEY NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('snoozed')),
    UNIQUE (account_id, kind)
) WITHOUT ROWID;

INSERT OR IGNORE INTO folder_marks (folder_id, account_id, kind)
    SELECT f.id, f.account_id, 'snoozed' FROM folders f
    WHERE f.parent_id IS NULL AND f.path = 'Snoozed'
      AND EXISTS (SELECT 1 FROM messages m JOIN snoozed_messages s ON s.message_id = m.id
                  WHERE m.folder_id = f.id);
";

/// Schema v9 (M2d, ADR 0050): the plug-in preview.
///
/// * `plugins` — installed plug-ins: the manifest as installed (JSON,
///   texts unresolved), where it came from (`file`, `url`, `dev`), the
///   package hash, whether it is enabled, its settings (JSON object of the
///   values the user changed), its storage quota and the last error
///   (JSON `ApiError`) that stopped it.
/// * `plugin_grants` — granted permissions per plug-in (`accounts`: JSON
///   array for mail permissions limited to accounts; `NULL` = all).
/// * `plugin_storage` — the per-plug-in key-value storage (`nail.storage`).
///   Values are JSON, **sealed at rest** when encryption at rest is on
///   (`SEALED_COLUMNS`); `size` = key + plain JSON bytes, summed against the
///   plug-in's quota. Rows are addressed by `id` for sealing.
/// * `plugin_logs` — log lines per plug-in (bounded, the oldest go first);
///   the message is sealed at rest (it may quote mail).
///
/// Everything of a plug-in goes with it (`ON DELETE CASCADE`).
const V9: &str = r"
CREATE TABLE plugins (
    id             TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 64),
    version        TEXT NOT NULL,
    -- JSON (nail_api::plugins::PluginManifest), as in the package.
    manifest       TEXT NOT NULL,
    source_kind    TEXT NOT NULL CHECK (source_kind IN ('file', 'url', 'dev')),
    -- Path, URL or folder.
    source         TEXT NOT NULL,
    package_sha256 TEXT CHECK (package_sha256 IS NULL OR length(package_sha256) = 64),
    enabled        INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    -- JSON object: setting key -> value (only values the user changed).
    settings       TEXT NOT NULL DEFAULT '{}',
    storage_quota  INTEGER NOT NULL CHECK (storage_quota >= 0),
    -- Unix seconds (UTC).
    installed_at   INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    -- JSON (nail_api::ApiError) of what stopped it last; NULL = nothing.
    last_error     TEXT
) WITHOUT ROWID;

CREATE TABLE plugin_grants (
    plugin_id  TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    -- Permission name (nail_api::plugins::PluginPermission).
    permission TEXT NOT NULL,
    -- JSON array of account ids; NULL = every account.
    accounts   TEXT,
    granted_at INTEGER NOT NULL,
    PRIMARY KEY (plugin_id, permission)
) WITHOUT ROWID;

CREATE TABLE plugin_storage (
    id         INTEGER PRIMARY KEY,
    plugin_id  TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    key        TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 256),
    -- JSON value; sealed at rest.
    value      TEXT NOT NULL,
    size       INTEGER NOT NULL CHECK (size >= 0),
    updated_at INTEGER NOT NULL,
    UNIQUE (plugin_id, key)
);

CREATE TABLE plugin_logs (
    seq       INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_id TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    -- Unix milliseconds (UTC).
    at        INTEGER NOT NULL,
    level     TEXT NOT NULL CHECK (level IN ('debug', 'info', 'warn', 'error')),
    source    TEXT NOT NULL CHECK (source IN ('plugin', 'host', 'api')),
    -- Sealed at rest.
    message   TEXT NOT NULL,
    -- JSON (nail_api::LocalizedMessage) for host lines; NULL otherwise.
    localized TEXT
);
CREATE INDEX plugin_logs_by_plugin ON plugin_logs(plugin_id, seq);
";

/// Schema v10 (M3a, ADR 0058): calendars, contacts and tasks.
///
/// * `pim_sources` — one row per source: the built-in `local` one
///   (inserted here) and every configured source the core has seen.
///   Settings stay in `accounts.kdl`; the row holds discovery results and
///   sync state.
/// * `pim_collections` — calendars / task lists (`calendar`) and address
///   books (`address-book`) of a source: the user's settings (JSON), the
///   server URL and the sync tokens (CalDAV / CardDAV `getctag`,
///   RFC 6578 sync-token, ICS `ETag` / `Last-Modified`). `role` marks the
///   local "Collected" book.
/// * `pim_objects` — every iCalendar / vCard resource as received or last
///   written (lossless; **sealed at rest**), its server URL and ETag, and
///   its sync state; on a conflict the server's version wins and the local
///   one is kept in `conflict_raw` (sealed).
/// * `pim_events`, `pim_tasks`, `pim_contacts` — the parsed items (JSON of
///   the `nail_api` types, sealed) with the plain columns lists, sorting
///   and range queries need (titles and names stay readable, like
///   subjects). Groups are `pim_contacts` rows of kind `group` with
///   `pim_group_members`; `pim_contact_emails` serves autocomplete and
///   duplicate detection; `pim_contact_links` holds merged persons.
/// * `pim_event_instances` — the instance cache: expanded occurrences in
///   Unix seconds; dates and floating times are stored as UTC wall time
///   (`floating` = 1) and placed in the viewer's zone when read
///   (range queries widen by 14 hours for them). `pim_instance_windows`
///   says how far each event has been expanded.
/// * `pim_reminders` — alarm occurrences with their state (pending, fired,
///   snoozed, dismissed); removed with their event or task by triggers.
/// * `pim_ops` — the PIM operation queue (like `op_queue`, per source).
///
/// Everything of a source goes with it (`ON DELETE CASCADE`).
const V10: &str = r"
CREATE TABLE pim_sources (
    id           TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 64),
    kind         TEXT NOT NULL CHECK (kind IN ('local', 'ics', 'caldav', 'carddav')),
    -- JSON: discovered endpoints (principal, home sets, capabilities).
    discovery    TEXT,
    state        TEXT NOT NULL DEFAULT 'idle'
                 CHECK (state IN ('idle', 'syncing', 'offline', 'auth-error', 'error', 'disabled')),
    -- Unix seconds (UTC).
    last_sync_at INTEGER,
    -- JSON (nail_api::ApiError) of the last failure; NULL = none.
    last_error   TEXT,
    created_at   INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO pim_sources (id, kind, created_at)
    VALUES ('local', 'local', CAST(strftime('%s', 'now') AS INTEGER));

CREATE TABLE pim_collections (
    id            TEXT PRIMARY KEY NOT NULL,
    source_id     TEXT NOT NULL REFERENCES pim_sources(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('calendar', 'address-book')),
    -- Collection URL (DAV) or feed URL (ICS); NULL for local collections.
    href          TEXT,
    -- JSON (nail_api CalendarSettings / AddressBookSettings).
    settings      TEXT NOT NULL,
    -- JSON array of nail_api CalendarComponent; '[]' for address books.
    components    TEXT NOT NULL DEFAULT '[]',
    read_only     INTEGER NOT NULL DEFAULT 0 CHECK (read_only IN (0, 1)),
    timezone      TEXT,
    role          TEXT CHECK (role IS NULL OR role IN ('collected')),
    ctag          TEXT,
    sync_token    TEXT,
    etag          TEXT,
    last_modified TEXT,
    last_sync_at  INTEGER,
    UNIQUE (source_id, href)
) WITHOUT ROWID;
CREATE INDEX pim_collections_source ON pim_collections(source_id, kind);
CREATE UNIQUE INDEX pim_collections_role ON pim_collections(role) WHERE role IS NOT NULL;

CREATE TABLE pim_objects (
    id            INTEGER PRIMARY KEY,
    collection_id TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    uid           TEXT NOT NULL,
    -- Resource URL on the server; NULL until uploaded (and for local / ICS).
    href          TEXT,
    etag          TEXT,
    kind          TEXT NOT NULL CHECK (kind IN ('event', 'task', 'contact', 'group')),
    -- The iCalendar / vCard text; sealed at rest.
    raw           TEXT NOT NULL,
    sync          TEXT NOT NULL DEFAULT 'synced' CHECK (sync IN ('synced', 'pending', 'conflict')),
    -- The local version kept after a conflict (the server's won); sealed.
    conflict_raw  TEXT,
    -- Deleted locally, the server delete is queued.
    deleted       INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    -- Unix seconds (UTC).
    updated_at    INTEGER NOT NULL,
    UNIQUE (collection_id, uid)
);
CREATE INDEX pim_objects_href ON pim_objects(collection_id, href) WHERE href IS NOT NULL;
CREATE INDEX pim_objects_unsynced ON pim_objects(collection_id) WHERE sync <> 'synced';

CREATE TABLE pim_events (
    id          TEXT PRIMARY KEY NOT NULL,
    object_id   INTEGER NOT NULL UNIQUE REFERENCES pim_objects(id) ON DELETE CASCADE,
    calendar_id TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    uid         TEXT NOT NULL,
    title       TEXT NOT NULL DEFAULT '',
    -- Unix seconds of the first start and the last end (NULL = recurs
    -- forever); dates and floating times as UTC wall time.
    first_start INTEGER NOT NULL,
    last_end    INTEGER,
    recurring   INTEGER NOT NULL DEFAULT 0 CHECK (recurring IN (0, 1)),
    -- JSON (nail_api CalendarEvent); sealed at rest.
    data        TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX pim_events_calendar ON pim_events(calendar_id, first_start);

CREATE TABLE pim_event_instances (
    event_id      TEXT NOT NULL REFERENCES pim_events(id) ON DELETE CASCADE,
    calendar_id   TEXT NOT NULL,
    -- The occurrence's recurrence id (nail_api PimTime as JSON); '' for a
    -- single event.
    recurrence_id TEXT NOT NULL,
    -- Unix seconds (UTC); dates and floating times as UTC wall time.
    start_at      INTEGER NOT NULL,
    end_at        INTEGER NOT NULL,
    floating      INTEGER NOT NULL DEFAULT 0 CHECK (floating IN (0, 1)),
    all_day       INTEGER NOT NULL DEFAULT 0 CHECK (all_day IN (0, 1)),
    overridden    INTEGER NOT NULL DEFAULT 0 CHECK (overridden IN (0, 1)),
    cancelled     INTEGER NOT NULL DEFAULT 0 CHECK (cancelled IN (0, 1)),
    busy          INTEGER NOT NULL DEFAULT 1 CHECK (busy IN (0, 1)),
    PRIMARY KEY (event_id, recurrence_id)
) WITHOUT ROWID;
CREATE INDEX pim_event_instances_range ON pim_event_instances(start_at, end_at);
CREATE INDEX pim_event_instances_calendar ON pim_event_instances(calendar_id, start_at);

CREATE TABLE pim_instance_windows (
    event_id       TEXT PRIMARY KEY NOT NULL REFERENCES pim_events(id) ON DELETE CASCADE,
    -- Unix seconds: the expanded range [from, until).
    expanded_from  INTEGER NOT NULL,
    expanded_until INTEGER NOT NULL,
    CHECK (expanded_from <= expanded_until)
) WITHOUT ROWID;

CREATE TABLE pim_contacts (
    id              TEXT PRIMARY KEY NOT NULL,
    object_id       INTEGER NOT NULL UNIQUE REFERENCES pim_objects(id) ON DELETE CASCADE,
    address_book_id TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    uid             TEXT NOT NULL,
    kind            TEXT NOT NULL DEFAULT 'individual'
                    CHECK (kind IN ('individual', 'organization', 'location', 'group')),
    display_name    TEXT NOT NULL DEFAULT '',
    -- Lowercase, family name first.
    sort_name       TEXT NOT NULL DEFAULT '',
    -- SHA-256 (hex) of the photo in the blob store.
    photo_sha256    TEXT CHECK (photo_sha256 IS NULL OR length(photo_sha256) = 64),
    -- JSON (nail_api ContactFields, or the group name and members); sealed.
    data            TEXT NOT NULL,
    updated_at      INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX pim_contacts_book ON pim_contacts(address_book_id, kind, sort_name);

CREATE TABLE pim_contact_emails (
    contact_id TEXT NOT NULL REFERENCES pim_contacts(id) ON DELETE CASCADE,
    -- Lowercase.
    email      TEXT NOT NULL,
    preferred  INTEGER NOT NULL DEFAULT 0 CHECK (preferred IN (0, 1)),
    PRIMARY KEY (contact_id, email)
) WITHOUT ROWID;
CREATE INDEX pim_contact_emails_email ON pim_contact_emails(email);

CREATE TABLE pim_contact_links (
    contact_id TEXT PRIMARY KEY NOT NULL REFERENCES pim_contacts(id) ON DELETE CASCADE,
    -- The merged person's primary contact.
    primary_id TEXT NOT NULL REFERENCES pim_contacts(id) ON DELETE CASCADE,
    CHECK (contact_id <> primary_id)
) WITHOUT ROWID;
CREATE INDEX pim_contact_links_primary ON pim_contact_links(primary_id);

CREATE TABLE pim_group_members (
    group_id TEXT NOT NULL REFERENCES pim_contacts(id) ON DELETE CASCADE,
    -- The member's vCard UID (as in MEMBER, e.g. urn:uuid:…) or a
    -- mailto: address.
    member   TEXT NOT NULL,
    PRIMARY KEY (group_id, member)
) WITHOUT ROWID;

CREATE TABLE pim_tasks (
    id           TEXT PRIMARY KEY NOT NULL,
    object_id    INTEGER NOT NULL UNIQUE REFERENCES pim_objects(id) ON DELETE CASCADE,
    calendar_id  TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    uid          TEXT NOT NULL,
    -- UID of the parent task (RELATED-TO;RELTYPE=PARENT).
    parent_uid   TEXT,
    title        TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL DEFAULT 'needs-action'
                 CHECK (status IN ('needs-action', 'in-process', 'completed', 'cancelled')),
    priority     INTEGER CHECK (priority IS NULL OR priority BETWEEN 0 AND 9),
    -- Unix seconds; dates and floating times as UTC wall time.
    due_at       INTEGER,
    start_at     INTEGER,
    completed_at INTEGER,
    sort_order   INTEGER,
    -- JSON (nail_api Task); sealed at rest.
    data         TEXT NOT NULL,
    updated_at   INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX pim_tasks_list ON pim_tasks(calendar_id, status, due_at);
CREATE INDEX pim_tasks_parent ON pim_tasks(calendar_id, parent_uid) WHERE parent_uid IS NOT NULL;

CREATE TABLE pim_reminders (
    id            TEXT PRIMARY KEY NOT NULL,
    item_kind     TEXT NOT NULL CHECK (item_kind IN ('event', 'task')),
    item_id       TEXT NOT NULL,
    -- The occurrence (as in pim_event_instances); '' for single items.
    recurrence_id TEXT NOT NULL DEFAULT '',
    -- The alarm's UID, else its position in the item.
    alarm_key     TEXT NOT NULL,
    -- Unix seconds (UTC).
    fire_at       INTEGER NOT NULL,
    state         TEXT NOT NULL DEFAULT 'pending'
                  CHECK (state IN ('pending', 'fired', 'snoozed', 'dismissed')),
    snoozed_until INTEGER,
    updated_at    INTEGER NOT NULL,
    UNIQUE (item_kind, item_id, recurrence_id, alarm_key)
) WITHOUT ROWID;
CREATE INDEX pim_reminders_due ON pim_reminders(state, fire_at);

CREATE TRIGGER pim_events_drop_reminders AFTER DELETE ON pim_events BEGIN
    DELETE FROM pim_reminders WHERE item_kind = 'event' AND item_id = OLD.id;
END;
CREATE TRIGGER pim_tasks_drop_reminders AFTER DELETE ON pim_tasks BEGIN
    DELETE FROM pim_reminders WHERE item_kind = 'task' AND item_id = OLD.id;
END;

CREATE TABLE pim_ops (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id  TEXT NOT NULL REFERENCES pim_sources(id) ON DELETE CASCADE,
    object_id  INTEGER REFERENCES pim_objects(id) ON DELETE SET NULL,
    kind       TEXT NOT NULL,
    -- JSON: what to replay (target URL, If-Match ETag, collection, …);
    -- item content is read from pim_objects when replaying.
    payload    TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'pending'
               CHECK (state IN ('pending', 'in_flight', 'done', 'failed')),
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX pim_ops_state ON pim_ops(source_id, state, id);
";

/// Schema v11 (M3b, ADR 0065): scheduling state and Gmail accounts.
///
/// * `messages.remote_id` — the message's id on an API server (Gmail; M3c
///   Graph / EWS), unique per account and kept across moves (API messages
///   are stored once, their folder follows their labels).
/// * `itip_state` — per event (`UID` + recurrence id, `''` = the whole
///   event) and address (organizer or attendee, lowercase): the newest
///   `SEQUENCE` / `DTSTAMP` applied and the answer (`PARTSTAT`), so older
///   iTIP messages are recognised as outdated.
/// * `itip_messages` — every received iTIP message and what was done
///   with it (`new`, `applied`, `outdated`, `ignored`, `failed`).
/// * `itip_sent` — every iTIP message Nail sent (or had the CalDAV server
///   send): replies, requests, cancels, counters, refreshes.
/// * `gmail_sync` — per Gmail account: the last applied `historyId` and an
///   interrupted full sync's page token.
/// * `gmail_labels` — the account's labels (id ↔ name ↔ Nail tag / folder,
///   colours, visibility, local sync state).
/// * `gmail_messages` — Gmail's thread id, label ids and category of each
///   stored Gmail message.
/// * `gmail_label_ops` — the label operation queue (create / update /
///   delete), per account.
const V11: &str = r"
ALTER TABLE messages ADD COLUMN remote_id TEXT;
CREATE UNIQUE INDEX messages_remote_id ON messages(account_id, remote_id)
    WHERE remote_id IS NOT NULL;

CREATE TABLE itip_state (
    uid           TEXT NOT NULL,
    -- The occurrence (nail_api PimTime as JSON); '' = the whole event.
    recurrence_id TEXT NOT NULL DEFAULT '',
    -- Lowercase email address.
    address       TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('organizer', 'attendee')),
    sequence      INTEGER NOT NULL DEFAULT 0 CHECK (sequence >= 0),
    -- RFC 3339 UTC.
    dtstamp       TEXT,
    -- nail_api ParticipationStatus (kebab-case); NULL for the organizer.
    partstat      TEXT,
    -- The message last applied for this address (no FK: kept after the
    -- message is deleted).
    message_id    TEXT,
    -- Unix seconds.
    updated_at    INTEGER NOT NULL,
    PRIMARY KEY (uid, recurrence_id, address)
) WITHOUT ROWID;

CREATE TABLE itip_messages (
    message_id    TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- nail_api ItipMethod (kebab-case).
    method        TEXT NOT NULL,
    uid           TEXT NOT NULL,
    recurrence_id TEXT NOT NULL DEFAULT '',
    sequence      INTEGER NOT NULL DEFAULT 0 CHECK (sequence >= 0),
    dtstamp       TEXT,
    -- Lowercase address of the organizer / attendee it came from.
    sender        TEXT,
    state         TEXT NOT NULL DEFAULT 'new'
                  CHECK (state IN ('new', 'applied', 'outdated', 'ignored', 'failed')),
    -- The user's event it was applied to (no FK: events come and go).
    event_id      TEXT,
    processed_at  INTEGER
) WITHOUT ROWID;
CREATE INDEX itip_messages_uid ON itip_messages(uid, recurrence_id);

CREATE TABLE itip_sent (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    identity_id   TEXT NOT NULL,
    method        TEXT NOT NULL,
    uid           TEXT NOT NULL,
    recurrence_id TEXT NOT NULL DEFAULT '',
    sequence      INTEGER NOT NULL DEFAULT 0 CHECK (sequence >= 0),
    dtstamp       TEXT NOT NULL,
    partstat      TEXT,
    -- JSON array of addresses.
    recipients    TEXT NOT NULL DEFAULT '[]',
    transport     TEXT NOT NULL CHECK (transport IN ('imip', 'server')),
    -- The outbox entry of an iMIP mail (nail_api OutboxId).
    outbox_id     TEXT,
    created_at    INTEGER NOT NULL
);
CREATE INDEX itip_sent_uid ON itip_sent(uid, recurrence_id);

CREATE TABLE gmail_sync (
    account_id           TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- The last historyId whose changes are all stored; NULL = full sync.
    history_id           INTEGER,
    -- An interrupted full sync: the next page token and the historyId
    -- taken when it started.
    full_sync_page       TEXT,
    full_sync_history_id INTEGER,
    -- Unix seconds.
    last_sync_at         INTEGER
) WITHOUT ROWID;

CREATE TABLE gmail_labels (
    account_id         TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    label_id           TEXT NOT NULL,
    name               TEXT NOT NULL,
    kind               TEXT NOT NULL CHECK (kind IN ('system', 'user')),
    color_background   TEXT,
    color_text         TEXT,
    list_visibility    TEXT NOT NULL DEFAULT 'show'
                       CHECK (list_visibility IN ('show', 'show-if-unread', 'hide')),
    message_visibility TEXT NOT NULL DEFAULT 'show'
                       CHECK (message_visibility IN ('show', 'hide')),
    -- The Nail tag of a user label (its name).
    tag                TEXT,
    -- The folder of a system label shown as a folder.
    folder_id          TEXT REFERENCES folders(id) ON DELETE SET NULL,
    sync               TEXT NOT NULL DEFAULT 'synced' CHECK (sync IN ('synced', 'pending')),
    PRIMARY KEY (account_id, label_id)
) WITHOUT ROWID;
CREATE UNIQUE INDEX gmail_labels_tag ON gmail_labels(account_id, tag) WHERE tag IS NOT NULL;

CREATE TABLE gmail_messages (
    message_id      TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    gmail_thread_id TEXT NOT NULL,
    -- JSON array of label ids.
    label_ids       TEXT NOT NULL DEFAULT '[]',
    category        TEXT CHECK (category IS NULL
                    OR category IN ('primary', 'social', 'promotions', 'updates', 'forums'))
) WITHOUT ROWID;
CREATE INDEX gmail_messages_thread ON gmail_messages(account_id, gmail_thread_id);
CREATE INDEX gmail_messages_category ON gmail_messages(account_id, category)
    WHERE category IS NOT NULL;

CREATE TABLE gmail_label_ops (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- The label (a local-… id until a create is replayed).
    label_id   TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('create', 'update', 'delete')),
    -- JSON (nail_api GmailLabelSettings) for create / update.
    payload    TEXT,
    state      TEXT NOT NULL DEFAULT 'pending'
               CHECK (state IN ('pending', 'done', 'failed')),
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX gmail_label_ops_state ON gmail_label_ops(account_id, state, id);
";

/// Schema v12 (M3c, ADR 0072): Exchange accounts.
///
/// * `pim_sources` is rebuilt (SQLite cannot change a `CHECK`): `kind`
///   also takes `exchange`, and a new `account_id` ties an Exchange
///   account's PIM source to the account (deleting the account deletes
///   the source and its collections). Runs with foreign keys off
///   ([`Migration::foreign_keys_off`]), checked before commit.
/// * `exchange_sync` — per account: engine, folder-hierarchy cursor per
///   mailbox is in `exchange_mailbox_sync`; server version, EWS streaming
///   subscription, throttling, the rules state, last sync.
/// * `exchange_mailbox_sync` — the folder-hierarchy cursor (Graph
///   `mailFolders/delta` link, EWS `SyncFolderHierarchy` state) per
///   mailbox (`''` = the account's own).
/// * `exchange_folders` — per mail folder: mailbox, remote id, change key,
///   parent, well-known name and the items cursor (Graph
///   `messages/delta` link, EWS `SyncFolderItems` state) plus an
///   interrupted first sync.
/// * `exchange_items` — per message: mailbox, change key, conversation id,
///   Focused Inbox classification (the remote id is `messages.remote_id`).
/// * `exchange_pim_collections` / `exchange_pim_items` — the same for
///   calendars, contact folders and task lists (cursor per collection;
///   remote id + change key per item).
/// * `exchange_mailboxes` — shared / delegate mailboxes (what is shown,
///   automapped / hidden, reachability, root folder).
/// * `exchange_categories` — the master category list (id ↔ name = Nail
///   tag, colour, local sync state).
/// * `exchange_automatic_replies` — the out-of-office settings (sealed).
/// * `exchange_rules` — the server's inbox rules and which Nail rule each
///   was placed from.
/// * `exchange_rooms` — cached room lists and rooms.
/// * `exchange_ops` — the Exchange operation queue (categories, automatic
///   replies, inbox rules, Focused Inbox); payloads sealed.
const V12: &str = r"
CREATE TABLE pim_sources_v12 (
    id           TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 128),
    kind         TEXT NOT NULL
                 CHECK (kind IN ('local', 'ics', 'caldav', 'carddav', 'exchange')),
    -- The Exchange account of an `exchange` source.
    account_id   TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    discovery    TEXT,
    state        TEXT NOT NULL DEFAULT 'idle'
                 CHECK (state IN ('idle', 'syncing', 'offline', 'auth-error', 'error', 'disabled')),
    last_sync_at INTEGER,
    last_error   TEXT,
    created_at   INTEGER NOT NULL,
    CHECK ((kind = 'exchange') = (account_id IS NOT NULL))
) WITHOUT ROWID;
INSERT INTO pim_sources_v12 (id, kind, discovery, state, last_sync_at, last_error, created_at)
    SELECT id, kind, discovery, state, last_sync_at, last_error, created_at FROM pim_sources;
DROP TABLE pim_sources;
ALTER TABLE pim_sources_v12 RENAME TO pim_sources;
CREATE UNIQUE INDEX pim_sources_account ON pim_sources(account_id) WHERE account_id IS NOT NULL;

CREATE TABLE exchange_sync (
    account_id      TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    engine          TEXT NOT NULL CHECK (engine IN ('graph', 'ews')),
    server_version  TEXT,
    -- EWS streaming subscription (resumable with its watermark).
    subscription_id TEXT,
    watermark       TEXT,
    -- Unix seconds.
    throttled_until INTEGER,
    rules_state     TEXT NOT NULL DEFAULT 'pending'
                    CHECK (rules_state IN ('disabled', 'pending', 'syncing', 'in-sync', 'failed')),
    rules_synced_at INTEGER,
    -- JSON (nail_api::ApiError) of the last rules failure.
    rules_error     TEXT,
    last_sync_at    INTEGER
) WITHOUT ROWID;

CREATE TABLE exchange_mailbox_sync (
    account_id       TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Lowercase address; '' = the account's own mailbox.
    mailbox          TEXT NOT NULL DEFAULT '',
    hierarchy_cursor TEXT,
    last_sync_at     INTEGER,
    PRIMARY KEY (account_id, mailbox)
) WITHOUT ROWID;

CREATE TABLE exchange_folders (
    folder_id        TEXT PRIMARY KEY NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    account_id       TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    mailbox          TEXT NOT NULL DEFAULT '',
    remote_id        TEXT NOT NULL,
    change_key       TEXT,
    parent_remote_id TEXT,
    -- nail_api ExchangeWellKnownFolder (kebab-case).
    well_known       TEXT,
    -- Items cursor; NULL = a full sync is needed.
    cursor           TEXT,
    -- An interrupted first sync (Graph next link / EWS state mid-pass).
    full_sync_page   TEXT,
    last_sync_at     INTEGER,
    UNIQUE (account_id, mailbox, remote_id)
) WITHOUT ROWID;

CREATE TABLE exchange_items (
    message_id      TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    mailbox         TEXT NOT NULL DEFAULT '',
    change_key      TEXT,
    conversation_id TEXT,
    inference       TEXT CHECK (inference IS NULL OR inference IN ('focused', 'other'))
) WITHOUT ROWID;
CREATE INDEX exchange_items_inference ON exchange_items(account_id, inference)
    WHERE inference IS NOT NULL;
CREATE INDEX exchange_items_conversation ON exchange_items(account_id, conversation_id)
    WHERE conversation_id IS NOT NULL;

CREATE TABLE exchange_pim_collections (
    collection_id TEXT PRIMARY KEY NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    mailbox       TEXT NOT NULL DEFAULT '',
    remote_id     TEXT NOT NULL,
    change_key    TEXT,
    role          TEXT CHECK (role IS NULL OR role IN
                  ('default-calendar', 'default-contacts', 'default-tasks', 'flagged-emails')),
    todo          INTEGER NOT NULL DEFAULT 0 CHECK (todo IN (0, 1)),
    cursor        TEXT,
    last_sync_at  INTEGER,
    UNIQUE (account_id, mailbox, remote_id)
) WITHOUT ROWID;

CREATE TABLE exchange_pim_items (
    object_id     INTEGER PRIMARY KEY REFERENCES pim_objects(id) ON DELETE CASCADE,
    collection_id TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    remote_id     TEXT NOT NULL,
    change_key    TEXT,
    UNIQUE (collection_id, remote_id)
);

CREATE TABLE exchange_mailboxes (
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Lowercase.
    address        TEXT NOT NULL,
    name           TEXT,
    kind           TEXT NOT NULL CHECK (kind IN ('shared', 'delegate')),
    origin         TEXT NOT NULL DEFAULT 'user' CHECK (origin IN ('user', 'automapped')),
    mail           INTEGER NOT NULL DEFAULT 1 CHECK (mail IN (0, 1)),
    calendar       INTEGER NOT NULL DEFAULT 1 CHECK (calendar IN (0, 1)),
    contacts       INTEGER NOT NULL DEFAULT 0 CHECK (contacts IN (0, 1)),
    tasks          INTEGER NOT NULL DEFAULT 0 CHECK (tasks IN (0, 1)),
    -- An automapped mailbox the user removed.
    hidden         INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    state          TEXT NOT NULL DEFAULT 'pending'
                   CHECK (state IN ('pending', 'synced', 'access-denied', 'not-found', 'error')),
    root_folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL,
    -- JSON (nail_api::ApiError) of the last failure.
    last_error     TEXT,
    created_at     INTEGER NOT NULL,
    PRIMARY KEY (account_id, address)
) WITHOUT ROWID;

CREATE TABLE exchange_categories (
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    category_id TEXT NOT NULL,
    -- Also the Nail tag.
    name        TEXT NOT NULL,
    -- nail_api CategoryColor (`none`, `preset0` … `preset24`).
    color       TEXT NOT NULL DEFAULT 'none',
    sync        TEXT NOT NULL DEFAULT 'synced' CHECK (sync IN ('synced', 'pending')),
    PRIMARY KEY (account_id, category_id)
) WITHOUT ROWID;
CREATE UNIQUE INDEX exchange_categories_name
    ON exchange_categories(account_id, name COLLATE NOCASE);

CREATE TABLE exchange_automatic_replies (
    account_id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- JSON (nail_api AutomaticRepliesSettings); sealed at rest.
    settings   TEXT NOT NULL,
    sync       TEXT NOT NULL DEFAULT 'synced' CHECK (sync IN ('synced', 'pending')),
    -- Unix seconds of the last read from the server.
    fetched_at INTEGER
) WITHOUT ROWID;

CREATE TABLE exchange_rules (
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    server_rule_id TEXT NOT NULL,
    -- The Nail rule it was placed from (rules.kdl id); NULL = the user's.
    rule_id        TEXT,
    -- Digest of the placed Nail rule (placed again when it changes).
    digest         TEXT,
    sequence       INTEGER NOT NULL DEFAULT 0,
    -- JSON (nail_api ExchangeInboxRule).
    data           TEXT NOT NULL,
    PRIMARY KEY (account_id, server_rule_id)
) WITHOUT ROWID;
CREATE UNIQUE INDEX exchange_rules_rule ON exchange_rules(account_id, rule_id)
    WHERE rule_id IS NOT NULL;

CREATE TABLE exchange_rooms (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('room-list', 'room')),
    -- The room list a room was listed from; '' = all rooms / the lists.
    room_list  TEXT NOT NULL DEFAULT '',
    address    TEXT NOT NULL,
    -- JSON (nail_api RoomList / Room).
    data       TEXT NOT NULL,
    fetched_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, kind, room_list, address)
) WITHOUT ROWID;

CREATE TABLE exchange_ops (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('create-category', 'update-category',
               'delete-category', 'set-automatic-replies', 'set-rule-enabled', 'delete-rule',
               'classify')),
    -- The category / server rule id, or NULL.
    target     TEXT,
    -- JSON (nail_store::exchange::ExchangeOp); sealed at rest.
    payload    TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'pending'
               CHECK (state IN ('pending', 'done', 'failed')),
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX exchange_ops_state ON exchange_ops(account_id, state, id);
";

/// Schema v13 (M3c review fixes, ADR 0078): resumable Exchange listings,
/// moves seen from other clients, meeting answers.
///
/// * `exchange_pim_collections.full_sync_page` — a full listing of a
///   calendar / contact folder / task list in progress (the page to go on
///   with), like `exchange_folders.full_sync_page` for mail folders.
/// * `exchange_listed` — the server ids a full listing of a mail folder or
///   PIM collection (`scope`) has seen so far; the merge after the last
///   page removes what the server no longer lists, also when the listing
///   took several passes.
/// * `exchange_removed` — messages a folder's changes reported as removed,
///   kept until every folder of the mailbox was synced: a message another
///   client moved shows up in its new folder and is moved locally instead
///   of being downloaded again.
/// * `exchange_meeting_answers` — per meeting the user was invited to: the
///   own answer as last synced, and an answer queued from Nail with its
///   comment (sealed) and whether the organizer is told.
const V13: &str = r"
ALTER TABLE exchange_pim_collections ADD COLUMN full_sync_page TEXT;

CREATE TABLE exchange_listed (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- A mail folder id or a PIM collection id.
    scope      TEXT NOT NULL,
    remote_id  TEXT NOT NULL,
    PRIMARY KEY (scope, remote_id)
) WITHOUT ROWID;

CREATE TABLE exchange_removed (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Lowercase address; '' = the account's own mailbox.
    mailbox    TEXT NOT NULL DEFAULT '',
    removed_at INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX exchange_removed_mailbox ON exchange_removed(account_id, mailbox);

CREATE TABLE exchange_meeting_answers (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    collection_id TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    uid           TEXT NOT NULL,
    -- nail_api ParticipationStatus (kebab-case) as the server last reported it.
    synced_status TEXT,
    -- An answer queued from Nail (RespondToInvitation).
    queued        INTEGER NOT NULL DEFAULT 0 CHECK (queued IN (0, 1)),
    send          INTEGER NOT NULL DEFAULT 1 CHECK (send IN (0, 1)),
    -- The answer's comment; sealed at rest.
    comment       TEXT,
    UNIQUE (collection_id, uid)
);
";

/// Schema v14 (M4a, ADR 0079): writes by MCP clients and plug-ins.
///
/// * `confirmations` — writes waiting for (or decided by) the user: the
///   actor (MCP client or plug-in, with the MCP client a plug-in serves),
///   the action kind and operation, what the confirmation center shows
///   (`view`: summary, targets, before / after, accounts — **sealed**:
///   subjects and addresses) and what to run once approved (`payload`,
///   the requester's own JSON — **sealed**), the caller's idempotency key,
///   state and decision. Decided rows are kept as history (pruned to
///   `nail_api::agents::CONFIRMATION_HISTORY`).
/// * `action_policies` — "always allow" / "ask" per actor and kind, only
///   where the user changed the default; `allow` is refused for
///   destructive kinds (a `CHECK`, besides the API's own check). Rows of a
///   removed client or plug-in are deleted with it by the core (actors are
///   not foreign keys: MCP clients are rows, plug-ins ids).
/// * `mcp_clients.rate_limits` — the client's `McpRateLimits` (JSON; NULL =
///   the defaults); `mcp_idempotency` — results of write tool calls by
///   idempotency key (**sealed**), kept for a day.
/// * `plugin_jobs` — per background job: last / next run, outcome, counts;
///   `plugin_hook_stats` — per hook: calls, failures, time-outs, vetoes,
///   time; `plugin_cpu` — CPU time per plug-in and minute for the hourly
///   quota. All go with their plug-in.
const V14: &str = r"
CREATE TABLE confirmations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_kind      TEXT NOT NULL CHECK (actor_kind IN ('mcp-client', 'plugin')),
    actor_id        TEXT NOT NULL,
    actor_name      TEXT NOT NULL,
    -- A plug-in serving an MCP tool call: the client.
    via_client_id   TEXT,
    via_client_name TEXT,
    -- nail_api::agents::ActionKind.
    kind            TEXT NOT NULL CHECK (kind IN ('organize', 'trash', 'permanent-delete',
                        'draft', 'send', 'calendar', 'invite', 'contact', 'task',
                        'pim-delete', 'rule')),
    operation       TEXT NOT NULL,
    -- JSON {summary, targets, more_targets, changes, account_ids}; sealed.
    view            TEXT NOT NULL,
    -- JSON of what to run once approved; sealed. NULL once decided.
    payload         TEXT,
    idempotency_key TEXT,
    state           TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'approved',
                        'denied', 'expired', 'cancelled', 'failed')),
    -- Unix seconds (UTC).
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL,
    decided_at      INTEGER,
    decided_via     TEXT CHECK (decided_via IN ('nail', 'elicitation', 'system')),
    -- JSON (nail_api::LocalizedMessage) for state 'failed'.
    error           TEXT
);
CREATE INDEX confirmations_pending ON confirmations(expires_at) WHERE state = 'pending';
CREATE INDEX confirmations_actor ON confirmations(actor_kind, actor_id, id DESC);
CREATE UNIQUE INDEX confirmations_idempotency
    ON confirmations(actor_kind, actor_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE action_policies (
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('mcp-client', 'plugin')),
    actor_id   TEXT NOT NULL,
    kind       TEXT NOT NULL,
    mode       TEXT NOT NULL CHECK (mode IN ('ask', 'allow')),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (actor_kind, actor_id, kind),
    CHECK (NOT (mode = 'allow' AND kind IN ('trash', 'permanent-delete', 'pim-delete')))
) WITHOUT ROWID;

-- JSON (nail_api::mcp::McpRateLimits); NULL = the defaults.
ALTER TABLE mcp_clients ADD COLUMN rate_limits TEXT;

CREATE TABLE mcp_idempotency (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id  INTEGER NOT NULL REFERENCES mcp_clients(id) ON DELETE CASCADE,
    key        TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 128),
    tool       TEXT NOT NULL,
    -- JSON (the tool's structured result); sealed.
    result     TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (client_id, key)
);
CREATE INDEX mcp_idempotency_age ON mcp_idempotency(created_at);

CREATE TABLE plugin_jobs (
    plugin_id     TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    job           TEXT NOT NULL,
    -- Unix seconds (UTC).
    last_run_at   INTEGER,
    next_run_at   INTEGER,
    last_outcome  TEXT CHECK (last_outcome IN ('ok', 'error', 'timeout', 'cpu-quota', 'overlap')),
    last_error    TEXT,
    run_count     INTEGER NOT NULL DEFAULT 0,
    failure_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (plugin_id, job)
) WITHOUT ROWID;

CREATE TABLE plugin_hook_stats (
    plugin_id  TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    hook       TEXT NOT NULL CHECK (hook IN ('arrival', 'beforeSend', 'open')),
    calls      INTEGER NOT NULL DEFAULT 0,
    failures   INTEGER NOT NULL DEFAULT 0,
    timeouts   INTEGER NOT NULL DEFAULT 0,
    vetoes     INTEGER NOT NULL DEFAULT 0,
    total_ms   INTEGER NOT NULL DEFAULT 0,
    max_ms     INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    -- Unix seconds (UTC).
    last_at    INTEGER,
    PRIMARY KEY (plugin_id, hook)
) WITHOUT ROWID;

CREATE TABLE plugin_cpu (
    plugin_id TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    -- Unix minute (seconds / 60).
    minute    INTEGER NOT NULL,
    cpu_ms    INTEGER NOT NULL CHECK (cpu_ms >= 0),
    PRIMARY KEY (plugin_id, minute)
) WITHOUT ROWID;
";

/// Schema v15 (M4c, ADR 0094): smartcard keys and server threads.
///
/// * `crypto_keys` is rebuilt so `source` and `secret_storage` may be
///   `'smartcard'` (a key on a PKCS#11 token: `secret_ref` holds the JSON of
///   its `nail_api::smartcard::SmartcardKeyRef`, `secret_data` is NULL).
///   `crypto_key_emails` references it, so the step runs with foreign keys
///   off ([`Migration::rebuild`]).
/// * `messages.remote_thread_id` — the server's thread / conversation id
///   (Gmail `threadId`, Graph / EWS conversation id); messages sharing one
///   share a Nail thread (`thread_refs` key `remote-thread:<account>:<id>`,
///   `Store::set_message_remote_thread_id`).
const V15: &str = r"
CREATE TABLE crypto_keys_v15 (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    technology        TEXT NOT NULL CHECK (technology IN ('openpgp', 'smime')),
    fingerprint       TEXT NOT NULL,
    user_ids          TEXT NOT NULL DEFAULT '[]',
    algorithm         TEXT NOT NULL,
    created_at        INTEGER NOT NULL,
    expires_at        INTEGER,
    revoked           INTEGER NOT NULL DEFAULT 0,
    can_sign          INTEGER NOT NULL DEFAULT 0,
    can_encrypt       INTEGER NOT NULL DEFAULT 0,
    source            TEXT NOT NULL CHECK (source IN ('generated', 'imported', 'pkcs12',
                          'windows-store', 'autocrypt', 'autocrypt-gossip', 'setup-message',
                          'wkd', 'keyserver', 'message', 'smartcard')),
    public_data       BLOB NOT NULL,
    secret_storage    TEXT CHECK (secret_storage IN ('keychain', 'passphrase', 'windows-store',
                          'smartcard')),
    -- Encrypted secret material; NULL for 'windows-store' and 'smartcard'.
    secret_data       BLOB,
    -- Keychain reference, Windows store thumbprint, or (smartcard) the
    -- JSON of the SmartcardKeyRef.
    secret_ref        TEXT,
    issuer            TEXT,
    serial            TEXT,
    imported_at       INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    UNIQUE (technology, fingerprint),
    CHECK (secret_storage IS NOT NULL OR (secret_data IS NULL AND secret_ref IS NULL))
);
INSERT INTO crypto_keys_v15 (id, technology, fingerprint, user_ids, algorithm, created_at,
    expires_at, revoked, can_sign, can_encrypt, source, public_data, secret_storage,
    secret_data, secret_ref, issuer, serial, imported_at, updated_at)
SELECT id, technology, fingerprint, user_ids, algorithm, created_at,
    expires_at, revoked, can_sign, can_encrypt, source, public_data, secret_storage,
    secret_data, secret_ref, issuer, serial, imported_at, updated_at
FROM crypto_keys;
DROP TABLE crypto_keys;
ALTER TABLE crypto_keys_v15 RENAME TO crypto_keys;

ALTER TABLE messages ADD COLUMN remote_thread_id TEXT;
CREATE INDEX messages_remote_thread ON messages(account_id, remote_thread_id)
    WHERE remote_thread_id IS NOT NULL;
";

/// One schema step: SQL plus an optional Rust backfill that runs in the same
/// transaction (for data SQLite cannot compute, e.g. normalized subjects).
#[derive(Clone, Copy)]
pub(crate) struct Migration {
    pub sql: &'static str,
    pub backfill: Option<fn(&Transaction<'_>) -> Result<()>>,
    /// The step rebuilds a table other tables reference (SQLite's
    /// "12-step" table change): foreign keys are switched off around its
    /// transaction (a `DROP TABLE` would otherwise cascade) and
    /// `PRAGMA foreign_key_check` must come back empty before the commit.
    pub foreign_keys_off: bool,
}

/// Schema v16 (P2 WP201, ADR 0149): conversations.
///
/// * `messages_folder_thread` — a folder's messages by thread, newest last:
///   a conversation list groups a folder by thread, counts its threads and
///   joins the Sent folders' messages of the listed threads from this index
///   alone (no row reads; `thread::activity::GROUPING_PAGE_SQL`).
const V16: &str = r"
CREATE INDEX messages_folder_thread ON messages(folder_id, thread_id, date_sort);
";

/// Schema v17 (P3 contract, ADR 0164): conversation speed and conversation
/// actions.
///
/// * `thread_activity` — one row per (folder, thread) with the thread's
///   newest message in the folder (`folder_last`), the row's order key
///   (`last_date`: also the user's own messages of the thread in the Sent
///   folders of the folder's account, P2's Sent join) and the thread's
///   messages in the folder (`message_count`, copies not collapsed). Drafts
///   folders have no rows (their messages are rows of their own). A page of
///   a folder's conversations is an index range of
///   `thread_activity_order` (`thread::activity`); WP-perf keeps the table
///   current (schema v18) and switches `ListThreads` to it.
/// * `mail_batches` / `mail_batch_ops` — a mail action's queued operations
///   with their inverses: undo as one unit (`UndoMailAction`), like a rule
///   run's actions; WP-actions-core writes and reads them.
const V17: &str = r"
CREATE TABLE thread_activity (
    folder_id     TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    thread_id     TEXT NOT NULL,
    -- Unix seconds (UTC): the newest date_sort of the thread in this folder.
    folder_last   INTEGER NOT NULL,
    -- Unix seconds (UTC): MAX(folder_last, the newest date_sort of the
    -- thread in the Sent folders of the folder's account). Order key.
    last_date     INTEGER NOT NULL,
    -- The thread's messages in this folder (every stored copy).
    message_count INTEGER NOT NULL CHECK (message_count > 0),
    PRIMARY KEY (folder_id, thread_id)
) WITHOUT ROWID;
CREATE INDEX thread_activity_order ON thread_activity(folder_id, last_date DESC, thread_id DESC);
CREATE INDEX thread_activity_thread ON thread_activity(thread_id);

INSERT INTO thread_activity (folder_id, thread_id, folder_last, last_date, message_count)
SELECT g.folder_id, g.thread_id, g.folder_last,
       MAX(g.folder_last, COALESCE((
           SELECT MAX(s.date_sort) FROM messages s
            WHERE s.thread_id = g.thread_id
              AND s.folder_id IN (SELECT sf.id FROM folders sf
                                   WHERE sf.account_id = g.account_id AND sf.role = 'sent')
       ), 0)),
       g.message_count
  FROM (SELECT m.folder_id, m.thread_id, f.account_id,
               MAX(m.date_sort) AS folder_last, COUNT(*) AS message_count
          FROM messages m JOIN folders f ON f.id = m.folder_id
         WHERE m.thread_id IS NOT NULL AND f.role <> 'drafts'
         GROUP BY m.folder_id, m.thread_id) g;

CREATE TABLE mail_batches (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Who acted: 'user', 'remote:<session id>', 'mcp:<client id>',
    -- 'plugin:<plug-in id>' (undo is the actor's).
    actor         TEXT NOT NULL,
    -- nail_api::mail_actions::MailActionKind wire name.
    kind          TEXT NOT NULL,
    state         TEXT NOT NULL DEFAULT 'done' CHECK (state IN ('done', 'undone')),
    -- Unix seconds (UTC).
    created_at    INTEGER NOT NULL,
    message_count INTEGER NOT NULL,
    conversations INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX mail_batches_created ON mail_batches(created_at);

-- Every queued operation of a batch with its inverse.
CREATE TABLE mail_batch_ops (
    batch_id   INTEGER NOT NULL REFERENCES mail_batches(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    message_id TEXT NOT NULL,
    -- op_queue id.
    op_id      INTEGER,
    -- nail_store::Operation JSON that undoes it; NULL = cannot be undone.
    undo       TEXT,
    PRIMARY KEY (batch_id, seq)
) WITHOUT ROWID;
";

impl Migration {
    pub(crate) const fn sql(sql: &'static str) -> Self {
        Self {
            sql,
            backfill: None,
            foreign_keys_off: false,
        }
    }

    /// A step that rebuilds a referenced table (see
    /// [`Self::foreign_keys_off`]).
    pub(crate) const fn rebuild(sql: &'static str) -> Self {
        Self {
            sql,
            backfill: None,
            foreign_keys_off: true,
        }
    }
}

/// All migrations; index `n` upgrades version `n` to `n + 1`.
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration::sql(V1),
    Migration {
        sql: V2,
        backfill: Some(crate::thread::backfill_v2),
        foreign_keys_off: false,
    },
    Migration::sql(V3),
    Migration {
        sql: V4,
        backfill: Some(crate::compose::backfill_v4),
        foreign_keys_off: false,
    },
    Migration::sql(V5),
    Migration::sql(V6),
    Migration::sql(V7),
    Migration::sql(V8),
    Migration::sql(V9),
    Migration::sql(V10),
    Migration::sql(V11),
    Migration::rebuild(V12),
    Migration::sql(V13),
    Migration::sql(V14),
    Migration::rebuild(V15),
    Migration::sql(V16),
    Migration::sql(V17),
    // Schema v18 (P3 WP-perf): triggers keep `thread_activity` current, and
    // the table is rebuilt once (`thread::activity::MAINTENANCE_SQL`).
    Migration::sql(crate::thread::activity::MAINTENANCE_SQL),
    // Schema v19 (ADRs 0190 / 0191): stored sync errors and skipped messages.
    Migration::sql(crate::sync_errors::V19),
];

/// The schema version this build creates and understands.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a few dozen migrations; the assertion below checks the bound at compile time"
)]
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;
const _: () = assert!(MIGRATIONS.len() < 1 << 16);

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
        // A step that rebuilds a referenced table runs with foreign keys
        // off; the pragma is a no-op inside a transaction, so it is set
        // before (and restored after) the step's transaction.
        let peek = current_version(conn)?;
        let rebuild = migrations
            .get(peek as usize)
            .is_some_and(|m| m.foreign_keys_off);
        let restore_fks = if rebuild {
            let on: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
            conn.pragma_update(None, "foreign_keys", false)?;
            on
        } else {
            false
        };
        let step_result = migrate_step(conn, migrations, supported);
        if restore_fks {
            conn.pragma_update(None, "foreign_keys", true)?;
        }
        if let Some(version) = step_result? {
            return Ok(version);
        }
    }
}

/// Applies the next migration in its own transaction; `Some(version)` when
/// the database is already at `supported`.
fn migrate_step(
    conn: &mut Connection,
    migrations: &[Migration],
    supported: u32,
) -> Result<Option<u32>> {
    {
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
            return Ok(Some(version));
        }
        let next = version + 1;
        tracing::info!(from = version, to = next, "migrating store schema");
        let step = migrations[version as usize];
        tx.execute_batch(step.sql)?;
        if let Some(backfill) = step.backfill {
            backfill(&tx)?;
        }
        if step.foreign_keys_off {
            let broken: i64 =
                tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                    r.get(0)
                })?;
            if broken > 0 {
                return Err(Error::InvalidData(format!(
                    "schema step {next} left {broken} broken references"
                )));
            }
        }
        tx.execute("INSERT INTO schema_version (version) VALUES (?1)", [next])?;
        tx.commit()?;
        Ok(None)
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
                "action_policies",
                "app_settings",
                "autocrypt_peers",
                "collected_addresses",
                "confirmations",
                "crypto_key_emails",
                "crypto_keys",
                "crypto_trust",
                "draft_attachments",
                "drafts",
                "exchange_automatic_replies",
                "exchange_categories",
                "exchange_folders",
                "exchange_items",
                "exchange_listed",
                "exchange_mailbox_sync",
                "exchange_mailboxes",
                "exchange_meeting_answers",
                "exchange_ops",
                "exchange_pim_collections",
                "exchange_pim_items",
                "exchange_removed",
                "exchange_rooms",
                "exchange_rules",
                "exchange_sync",
                "folder_marks",
                "folders",
                "gmail_label_ops",
                "gmail_labels",
                "gmail_messages",
                "gmail_sync",
                "itip_messages",
                "itip_sent",
                "itip_state",
                "junk_settings",
                "junk_tokens",
                "junk_training",
                "junk_verdicts",
                "mail_batch_ops",
                "mail_batches",
                "mcp_audit",
                "mcp_clients",
                "mcp_grants",
                "mcp_idempotency",
                "message_security",
                "message_tags",
                "messages",
                "notify_overrides",
                "op_queue",
                "outbox",
                "pim_collections",
                "pim_contact_emails",
                "pim_contact_links",
                "pim_contacts",
                "pim_event_instances",
                "pim_events",
                "pim_group_members",
                "pim_instance_windows",
                "pim_objects",
                "pim_ops",
                "pim_reminders",
                "pim_sources",
                "pim_tasks",
                "plugin_cpu",
                "plugin_grants",
                "plugin_hook_stats",
                "plugin_jobs",
                "plugin_logs",
                "plugin_storage",
                "plugins",
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
                "sieve_state",
                "snoozed_messages",
                "sync_errors",
                "sync_skipped",
                "thread_activity",
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
            foreign_keys_off: false,
        }];
        migrate_with(&mut conn, &steps).unwrap();
        let x: i64 = conn.query_row("SELECT x FROM a", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 7);

        let mut conn = Connection::open_in_memory().unwrap();
        let steps = [Migration {
            sql: "CREATE TABLE b (x INTEGER)",
            backfill: Some(broken),
            foreign_keys_off: false,
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
    fn v7_keeps_existing_messages_synced() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_to_version(&mut conn, 6).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, name, email, kind) VALUES ('a', 'A', 'a@x', 'imap');
             INSERT INTO folders (id, account_id, name, path) VALUES ('f', 'a', 'INBOX', 'INBOX');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('m', 'a', 'f', 'x');",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let state: String = conn
            .query_row("SELECT body_state FROM messages WHERE id = 'm'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(state, "synced");
        assert!(
            conn.execute("UPDATE messages SET body_state = 'bogus'", [])
                .is_err()
        );
        assert!(
            conn.execute(
                "INSERT INTO junk_tokens (account_id, token) VALUES ('a', 'plain-word')",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn v8_marks_snoozed_folders_holding_snoozed_mail_only() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_to_version(&mut conn, 7).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, name, email, kind) VALUES ('a', 'A', 'a@x', 'imap');
             INSERT INTO accounts (id, name, email, kind) VALUES ('b', 'B', 'b@x', 'imap');
             INSERT INTO folders (id, account_id, name, path) VALUES ('fa', 'a', 'Snoozed', 'Snoozed');
             INSERT INTO folders (id, account_id, name, path) VALUES ('fb', 'b', 'Snoozed', 'Snoozed');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('m', 'a', 'fa', 'x');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('n', 'b', 'fb', 'x');
             INSERT INTO snoozed_messages (message_id, account_id, until, wake_folder_id, snoozed_at)
                 VALUES ('m', 'a', 10, 'fa', 1);
             INSERT INTO sieve_state (account_id, script_hash) VALUES ('a', NULL);",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let marked: Vec<String> = conn
            .prepare("SELECT folder_id FROM folder_marks")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(marked, ["fa"], "the other Snoozed folder is the user's");
        let rules: String = conn
            .query_row("SELECT server_rules FROM sieve_state", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rules, "[]");
    }

    #[test]
    fn v11_adds_remote_ids_scheduling_and_gmail_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate_to_version(&mut conn, 10).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, name, email, kind) VALUES ('g', 'G', 'g@x', 'gmail');
             INSERT INTO folders (id, account_id, name, path) VALUES ('fi', 'g', 'Inbox', 'INBOX');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('m', 'g', 'fi', 'x');",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        conn.execute_batch(
            "UPDATE messages SET remote_id = '18c0' WHERE id = 'm';
             INSERT INTO gmail_messages (message_id, account_id, gmail_thread_id, label_ids, category)
                 VALUES ('m', 'g', 't1', '[\"INBOX\"]', 'social');
             INSERT INTO gmail_labels (account_id, label_id, name, kind, tag)
                 VALUES ('g', 'Label_1', 'Work', 'user', 'Work');
             INSERT INTO itip_messages (message_id, account_id, method, uid)
                 VALUES ('m', 'g', 'request', 'uid-1');",
        )
        .unwrap();
        // One remote id per account; categories and states are checked.
        assert!(
            conn.execute(
                "INSERT INTO messages (id, account_id, folder_id, date, remote_id)
                 VALUES ('n', 'g', 'fi', 'x', '18c0')",
                []
            )
            .is_err()
        );
        assert!(
            conn.execute("UPDATE gmail_messages SET category = 'spam'", [])
                .is_err()
        );
        assert!(
            conn.execute("UPDATE itip_messages SET state = 'bogus'", [])
                .is_err()
        );
        // Deleting the message removes its Gmail and iTIP rows; deleting
        // the account removes the labels.
        conn.execute("DELETE FROM messages WHERE id = 'm'", [])
            .unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM gmail_messages) + (SELECT COUNT(*) FROM itip_messages)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        conn.execute("DELETE FROM accounts WHERE id = 'g'", [])
            .unwrap();
        let labels: i64 = conn
            .query_row("SELECT COUNT(*) FROM gmail_labels", [], |r| r.get(0))
            .unwrap();
        assert_eq!(labels, 0);
    }

    #[test]
    fn v14_keeps_mcp_clients_and_adds_agent_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate_to_version(&mut conn, 13).unwrap();
        conn.execute_batch(
            "INSERT INTO mcp_clients (name, token_hash, token_hint, created_at)
                 VALUES ('agent', 'h', 'hint', 1);",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let limits: Option<String> = conn
            .query_row("SELECT rate_limits FROM mcp_clients", [], |r| r.get(0))
            .unwrap();
        assert_eq!(limits, None);
        // Idempotency entries go with their client; confirmations accept
        // only known kinds and states.
        conn.execute(
            "INSERT INTO mcp_idempotency (client_id, key, tool, result, created_at)
             VALUES (1, 'k', 'send_message', '{}', 1)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM mcp_clients", []).unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM mcp_idempotency", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
        assert!(
            conn.execute(
                "INSERT INTO confirmations (actor_kind, actor_id, actor_name, kind, operation,
                     view, created_at, expires_at)
                 VALUES ('plugin', 'x', 'X', 'teleport', 'op', '{}', 1, 2)",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn v12_rebuilds_pim_sources_and_adds_exchange_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate_to_version(&mut conn, 11).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, name, email, kind) VALUES ('x', 'X', 'x@x', 'exchange');
             INSERT INTO folders (id, account_id, name, path) VALUES ('fi', 'x', 'Inbox', 'Inbox');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('m', 'x', 'fi', 'x');
             INSERT INTO pim_sources (id, kind, created_at) VALUES ('dav', 'caldav', 1);
             INSERT INTO pim_collections (id, source_id, kind, settings)
                 VALUES ('c1', 'dav', 'calendar', '{}');",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        // The rebuild kept every source and collection, and foreign keys
        // are on again.
        let fk: bool = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert!(fk);
        let collections: i64 = conn
            .query_row("SELECT COUNT(*) FROM pim_collections", [], |r| r.get(0))
            .unwrap();
        assert_eq!(collections, 1);
        let sources: Vec<String> = conn
            .prepare("SELECT id FROM pim_sources ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(sources, ["dav", "local"]);
        // Exchange sources need their account (and only they have one).
        assert!(
            conn.execute(
                "INSERT INTO pim_sources (id, kind, created_at) VALUES ('exchange:x', 'exchange', 1)",
                []
            )
            .is_err()
        );
        conn.execute_batch(
            "INSERT INTO pim_sources (id, kind, account_id, created_at)
                 VALUES ('exchange:x', 'exchange', 'x', 1);
             INSERT INTO pim_collections (id, source_id, kind, settings)
                 VALUES ('xc', 'exchange:x', 'calendar', '{}');
             INSERT INTO exchange_pim_collections (collection_id, account_id, remote_id)
                 VALUES ('xc', 'x', 'AQMk');
             INSERT INTO exchange_folders (folder_id, account_id, remote_id)
                 VALUES ('fi', 'x', 'AAMk');
             INSERT INTO exchange_items (message_id, account_id, inference)
                 VALUES ('m', 'x', 'focused');",
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO pim_sources (id, kind, account_id, created_at)
                 VALUES ('dav2', 'caldav', 'x', 1)",
                []
            )
            .is_err()
        );
        assert!(
            conn.execute("UPDATE exchange_items SET inference = 'primary'", [])
                .is_err()
        );
        // Deleting the account removes its source, collections and rows.
        conn.execute("DELETE FROM accounts WHERE id = 'x'", [])
            .unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM pim_sources WHERE kind = 'exchange')
                     + (SELECT COUNT(*) FROM pim_collections WHERE id = 'xc')
                     + (SELECT COUNT(*) FROM exchange_pim_collections)
                     + (SELECT COUNT(*) FROM exchange_folders)
                     + (SELECT COUNT(*) FROM exchange_items)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn rebuild_steps_refuse_broken_references() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        let steps = [
            Migration::sql(
                "CREATE TABLE p (id TEXT PRIMARY KEY);
                 CREATE TABLE c (p TEXT REFERENCES p(id) ON DELETE CASCADE);
                 INSERT INTO p VALUES ('a'); INSERT INTO c VALUES ('a');",
            ),
            // Drops the referenced row without the cascade (foreign keys
            // are off): the check refuses the step.
            Migration::rebuild(
                "CREATE TABLE p2 (id TEXT PRIMARY KEY); DROP TABLE p; ALTER TABLE p2 RENAME TO p;",
            ),
        ];
        assert!(migrate_with(&mut conn, &steps).is_err());
        assert_eq!(current_version(&conn).unwrap(), 1);
        let kept: i64 = conn
            .query_row("SELECT COUNT(*) FROM c", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 1, "nothing cascaded");
        let fk: bool = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert!(fk, "foreign keys are restored after a failed step");
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

    #[test]
    fn v15_keeps_keys_and_allows_smartcards_and_server_threads() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate_to_version(&mut conn, 14).unwrap();
        conn.execute_batch(
            "INSERT INTO crypto_keys (id, technology, fingerprint, algorithm, created_at, source,
                 public_data, secret_storage, secret_data, secret_ref, imported_at, updated_at)
                 VALUES (7, 'openpgp', 'AB', 'Ed25519', 1, 'generated', x'01', 'keychain',
                         x'02', 'keychain:nail/openpgp/AB', 1, 1);
             INSERT INTO crypto_key_emails (key_id, email) VALUES (7, 'ada@x');
             INSERT INTO accounts (id, name, email, kind) VALUES ('g', 'G', 'g@x', 'gmail');
             INSERT INTO folders (id, account_id, name, path) VALUES ('fi', 'g', 'Inbox', 'INBOX');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('m', 'g', 'fi', 'x');",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let (fpr, email): (String, String) = conn
            .query_row(
                "SELECT k.fingerprint, e.email FROM crypto_keys k
                 JOIN crypto_key_emails e ON e.key_id = k.id WHERE k.id = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((fpr.as_str(), email.as_str()), ("AB", "ada@x"));
        // Smartcard keys: no secret data, a reference.
        conn.execute(
            "INSERT INTO crypto_keys (technology, fingerprint, algorithm, created_at, source,
                 public_data, secret_storage, secret_ref, imported_at, updated_at)
             VALUES ('smime', 'CD', 'RSA 2048', 1, 'smartcard', x'01', 'smartcard',
                     '{}', 1, 1)",
            [],
        )
        .unwrap();
        assert!(
            conn.execute("UPDATE crypto_keys SET source = 'bogus' WHERE id = 7", [])
                .is_err()
        );
        // The email rows still cascade with their key.
        conn.execute("DELETE FROM crypto_keys WHERE id = 7", [])
            .unwrap();
        let emails: i64 = conn
            .query_row("SELECT COUNT(*) FROM crypto_key_emails", [], |r| r.get(0))
            .unwrap();
        assert_eq!(emails, 0);
        conn.execute(
            "UPDATE messages SET remote_thread_id = 't1' WHERE id = 'm'",
            [],
        )
        .unwrap();
    }
}
