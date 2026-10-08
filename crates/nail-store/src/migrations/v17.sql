
CREATE TABLE thread_activity (
    folder_id     TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    thread_id     TEXT NOT NULL,
    -- Unix seconds (UTC): the newest date_sort of the thread in this folder.
    folder_last   INTEGER NOT NULL,
    -- Unix seconds (UTC): MAX(folder_last, the newest date_sort of the
    -- thread in the Sent folders of the folder's account). Order key.
    last_date     INTEGER NOT NULL,
    -- The thread's messages in this folder (every stored copy).
    message_count INTEGER NOT NULL CHECK (message_count > 0),
    PRIMARY KEY (folder_id, thread_id)
) WITHOUT ROWID;
CREATE INDEX thread_activity_order ON thread_activity(folder_id, last_date DESC, thread_id DESC);
CREATE INDEX thread_activity_thread ON thread_activity(thread_id);

INSERT INTO thread_activity (folder_id, thread_id, folder_last, last_date, message_count)
SELECT g.folder_id, g.thread_id, g.folder_last,
       MAX(g.folder_last, COALESCE((
           SELECT MAX(s.date_sort) FROM messages s
            WHERE s.thread_id = g.thread_id
              AND s.folder_id IN (SELECT sf.id FROM folders sf
                                   WHERE sf.account_id = g.account_id AND sf.role = 'sent')
       ), 0)),
       g.message_count
  FROM (SELECT m.folder_id, m.thread_id, f.account_id,
               MAX(m.date_sort) AS folder_last, COUNT(*) AS message_count
          FROM messages m JOIN folders f ON f.id = m.folder_id
         WHERE m.thread_id IS NOT NULL AND f.role <> 'drafts'
         GROUP BY m.folder_id, m.thread_id) g;

CREATE TABLE mail_batches (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Who acted: 'user', 'remote:<session id>', 'mcp:<client id>',
    -- 'plugin:<plug-in id>' (undo is the actor's).
    actor         TEXT NOT NULL,
    -- nail_api::mail_actions::MailActionKind wire name.
    kind          TEXT NOT NULL,
    state         TEXT NOT NULL DEFAULT 'done' CHECK (state IN ('done', 'undone')),
    -- Unix seconds (UTC).
    created_at    INTEGER NOT NULL,
    message_count INTEGER NOT NULL,
    conversations INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX mail_batches_created ON mail_batches(created_at);

-- Every queued operation of a batch with its inverse.
CREATE TABLE mail_batch_ops (
    batch_id   INTEGER NOT NULL REFERENCES mail_batches(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    message_id TEXT NOT NULL,
    -- op_queue id.
    op_id      INTEGER,
    -- nail_store::Operation JSON that undoes it; NULL = cannot be undone.
    undo       TEXT,
    PRIMARY KEY (batch_id, seq)
) WITHOUT ROWID;
