
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
