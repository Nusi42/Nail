
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
