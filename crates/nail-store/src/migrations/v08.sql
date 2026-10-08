
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
