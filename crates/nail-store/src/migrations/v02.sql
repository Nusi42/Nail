
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
