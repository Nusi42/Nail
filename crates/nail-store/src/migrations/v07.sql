
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
