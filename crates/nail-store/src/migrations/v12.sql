
CREATE TABLE pim_sources_v12 (
    id           TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 128),
    kind         TEXT NOT NULL
                 CHECK (kind IN ('local', 'ics', 'caldav', 'carddav', 'exchange')),
    -- The Exchange account of an `exchange` source.
    account_id   TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    discovery    TEXT,
    state        TEXT NOT NULL DEFAULT 'idle'
                 CHECK (state IN ('idle', 'syncing', 'offline', 'auth-error', 'error', 'disabled')),
    last_sync_at INTEGER,
    last_error   TEXT,
    created_at   INTEGER NOT NULL,
    CHECK ((kind = 'exchange') = (account_id IS NOT NULL))
) WITHOUT ROWID;
INSERT INTO pim_sources_v12 (id, kind, discovery, state, last_sync_at, last_error, created_at)
    SELECT id, kind, discovery, state, last_sync_at, last_error, created_at FROM pim_sources;
DROP TABLE pim_sources;
ALTER TABLE pim_sources_v12 RENAME TO pim_sources;
CREATE UNIQUE INDEX pim_sources_account ON pim_sources(account_id) WHERE account_id IS NOT NULL;

CREATE TABLE exchange_sync (
    account_id      TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    engine          TEXT NOT NULL CHECK (engine IN ('graph', 'ews')),
    server_version  TEXT,
    -- EWS streaming subscription (resumable with its watermark).
    subscription_id TEXT,
    watermark       TEXT,
    -- Unix seconds.
    throttled_until INTEGER,
    rules_state     TEXT NOT NULL DEFAULT 'pending'
                    CHECK (rules_state IN ('disabled', 'pending', 'syncing', 'in-sync', 'failed')),
    rules_synced_at INTEGER,
    -- JSON (nail_api::ApiError) of the last rules failure.
    rules_error     TEXT,
    last_sync_at    INTEGER
) WITHOUT ROWID;

CREATE TABLE exchange_mailbox_sync (
    account_id       TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Lowercase address; '' = the account's own mailbox.
    mailbox          TEXT NOT NULL DEFAULT '',
    hierarchy_cursor TEXT,
    last_sync_at     INTEGER,
    PRIMARY KEY (account_id, mailbox)
) WITHOUT ROWID;

CREATE TABLE exchange_folders (
    folder_id        TEXT PRIMARY KEY NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    account_id       TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    mailbox          TEXT NOT NULL DEFAULT '',
    remote_id        TEXT NOT NULL,
    change_key       TEXT,
    parent_remote_id TEXT,
    -- nail_api ExchangeWellKnownFolder (kebab-case).
    well_known       TEXT,
    -- Items cursor; NULL = a full sync is needed.
    cursor           TEXT,
    -- An interrupted first sync (Graph next link / EWS state mid-pass).
    full_sync_page   TEXT,
    last_sync_at     INTEGER,
    UNIQUE (account_id, mailbox, remote_id)
) WITHOUT ROWID;

CREATE TABLE exchange_items (
    message_id      TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    mailbox         TEXT NOT NULL DEFAULT '',
    change_key      TEXT,
    conversation_id TEXT,
    inference       TEXT CHECK (inference IS NULL OR inference IN ('focused', 'other'))
) WITHOUT ROWID;
CREATE INDEX exchange_items_inference ON exchange_items(account_id, inference)
    WHERE inference IS NOT NULL;
CREATE INDEX exchange_items_conversation ON exchange_items(account_id, conversation_id)
    WHERE conversation_id IS NOT NULL;

CREATE TABLE exchange_pim_collections (
    collection_id TEXT PRIMARY KEY NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    mailbox       TEXT NOT NULL DEFAULT '',
    remote_id     TEXT NOT NULL,
    change_key    TEXT,
    role          TEXT CHECK (role IS NULL OR role IN
                  ('default-calendar', 'default-contacts', 'default-tasks', 'flagged-emails')),
    todo          INTEGER NOT NULL DEFAULT 0 CHECK (todo IN (0, 1)),
    cursor        TEXT,
    last_sync_at  INTEGER,
    UNIQUE (account_id, mailbox, remote_id)
) WITHOUT ROWID;

CREATE TABLE exchange_pim_items (
    object_id     INTEGER PRIMARY KEY REFERENCES pim_objects(id) ON DELETE CASCADE,
    collection_id TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    remote_id     TEXT NOT NULL,
    change_key    TEXT,
    UNIQUE (collection_id, remote_id)
);

CREATE TABLE exchange_mailboxes (
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Lowercase.
    address        TEXT NOT NULL,
    name           TEXT,
    kind           TEXT NOT NULL CHECK (kind IN ('shared', 'delegate')),
    origin         TEXT NOT NULL DEFAULT 'user' CHECK (origin IN ('user', 'automapped')),
    mail           INTEGER NOT NULL DEFAULT 1 CHECK (mail IN (0, 1)),
    calendar       INTEGER NOT NULL DEFAULT 1 CHECK (calendar IN (0, 1)),
    contacts       INTEGER NOT NULL DEFAULT 0 CHECK (contacts IN (0, 1)),
    tasks          INTEGER NOT NULL DEFAULT 0 CHECK (tasks IN (0, 1)),
    -- An automapped mailbox the user removed.
    hidden         INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    state          TEXT NOT NULL DEFAULT 'pending'
                   CHECK (state IN ('pending', 'synced', 'access-denied', 'not-found', 'error')),
    root_folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL,
    -- JSON (nail_api::ApiError) of the last failure.
    last_error     TEXT,
    created_at     INTEGER NOT NULL,
    PRIMARY KEY (account_id, address)
) WITHOUT ROWID;

CREATE TABLE exchange_categories (
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    category_id TEXT NOT NULL,
    -- Also the Nail tag.
    name        TEXT NOT NULL,
    -- nail_api CategoryColor (`none`, `preset0` … `preset24`).
    color       TEXT NOT NULL DEFAULT 'none',
    sync        TEXT NOT NULL DEFAULT 'synced' CHECK (sync IN ('synced', 'pending')),
    PRIMARY KEY (account_id, category_id)
) WITHOUT ROWID;
CREATE UNIQUE INDEX exchange_categories_name
    ON exchange_categories(account_id, name COLLATE NOCASE);

CREATE TABLE exchange_automatic_replies (
    account_id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- JSON (nail_api AutomaticRepliesSettings); sealed at rest.
    settings   TEXT NOT NULL,
    sync       TEXT NOT NULL DEFAULT 'synced' CHECK (sync IN ('synced', 'pending')),
    -- Unix seconds of the last read from the server.
    fetched_at INTEGER
) WITHOUT ROWID;

CREATE TABLE exchange_rules (
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    server_rule_id TEXT NOT NULL,
    -- The Nail rule it was placed from (rules.kdl id); NULL = the user's.
    rule_id        TEXT,
    -- Digest of the placed Nail rule (placed again when it changes).
    digest         TEXT,
    sequence       INTEGER NOT NULL DEFAULT 0,
    -- JSON (nail_api ExchangeInboxRule).
    data           TEXT NOT NULL,
    PRIMARY KEY (account_id, server_rule_id)
) WITHOUT ROWID;
CREATE UNIQUE INDEX exchange_rules_rule ON exchange_rules(account_id, rule_id)
    WHERE rule_id IS NOT NULL;

CREATE TABLE exchange_rooms (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('room-list', 'room')),
    -- The room list a room was listed from; '' = all rooms / the lists.
    room_list  TEXT NOT NULL DEFAULT '',
    address    TEXT NOT NULL,
    -- JSON (nail_api RoomList / Room).
    data       TEXT NOT NULL,
    fetched_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, kind, room_list, address)
) WITHOUT ROWID;

CREATE TABLE exchange_ops (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('create-category', 'update-category',
               'delete-category', 'set-automatic-replies', 'set-rule-enabled', 'delete-rule',
               'classify')),
    -- The category / server rule id, or NULL.
    target     TEXT,
    -- JSON (nail_store::exchange::ExchangeOp); sealed at rest.
    payload    TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'pending'
               CHECK (state IN ('pending', 'done', 'failed')),
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX exchange_ops_state ON exchange_ops(account_id, state, id);
