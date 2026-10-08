
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
