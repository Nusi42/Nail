-- Schema v1 exactly as released in M0 (crates/nail-store/src/schema.rs, V1).

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

-- A database as written by the M0 build (schema v1).
CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER PRIMARY KEY NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO schema_version (version) VALUES (1);

INSERT INTO accounts (id, name, email, kind) VALUES ('a1', 'Work', 'ada@example.org', 'imap');
INSERT INTO folders (id, account_id, parent_id, name, path, role, uidvalidity, uidnext, highestmodseq)
    VALUES ('inbox', 'a1', NULL, 'INBOX', 'INBOX', 'inbox', 7, 100, 5000);
INSERT INTO folders (id, account_id, parent_id, name, path, role) VALUES ('archive', 'a1', NULL, 'Archive', 'Archive', 'archive');

INSERT INTO messages (id, account_id, folder_id, uid, subject, from_name, from_email, to_json, date, date_sort,
                      seen, flagged, answered, draft, has_attachments, preview, blob_hash, thread_key)
VALUES
 ('m1', 'a1', 'inbox', 1, 'Plans', 'Ada', 'ada@example.org', '[]', '2026-01-01T10:00:00Z', 1767261600, 1, 0, 0, 0, 0, 'p1', NULL, 'root@x'),
 ('m2', 'a1', 'inbox', 2, 'Re: Plans', 'Bob', 'bob@example.org', '[{"name":null,"email":"ada@example.org"}]', '2026-01-02T10:00:00Z', 1767348000, 0, 1, 0, 0, 1, 'p2', NULL, 'root@x'),
 ('m3', 'a1', 'archive', 3, 'AW: Other', NULL, 'carol@example.org', '[]', '2026-01-03T10:00:00Z', 1767434400, 0, 0, 0, 0, 0, 'p3', NULL, 'other@x'),
 ('m4', 'a1', 'archive', 4, 'No id at all', NULL, NULL, '[]', 'garbage', 0, 0, 0, 0, 0, 0, '', NULL, NULL);

INSERT INTO op_queue (account_id, kind, payload, state)
VALUES ('a1', 'update_flags', '{"type":"update_flags","message_id":"m2","change":{"seen":true}}', 'pending'),
       ('a1', 'update_flags', '{"type":"update_flags","message_id":"m1","change":{"flagged":true}}', 'done');
