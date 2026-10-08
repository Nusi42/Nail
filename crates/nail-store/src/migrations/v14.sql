
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
