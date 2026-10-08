
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
