
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
