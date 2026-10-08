
CREATE TABLE pim_sources (
    id           TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 64),
    kind         TEXT NOT NULL CHECK (kind IN ('local', 'ics', 'caldav', 'carddav')),
    -- JSON: discovered endpoints (principal, home sets, capabilities).
    discovery    TEXT,
    state        TEXT NOT NULL DEFAULT 'idle'
                 CHECK (state IN ('idle', 'syncing', 'offline', 'auth-error', 'error', 'disabled')),
    -- Unix seconds (UTC).
    last_sync_at INTEGER,
    -- JSON (nail_api::ApiError) of the last failure; NULL = none.
    last_error   TEXT,
    created_at   INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO pim_sources (id, kind, created_at)
    VALUES ('local', 'local', CAST(strftime('%s', 'now') AS INTEGER));

CREATE TABLE pim_collections (
    id            TEXT PRIMARY KEY NOT NULL,
    source_id     TEXT NOT NULL REFERENCES pim_sources(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('calendar', 'address-book')),
    -- Collection URL (DAV) or feed URL (ICS); NULL for local collections.
    href          TEXT,
    -- JSON (nail_api CalendarSettings / AddressBookSettings).
    settings      TEXT NOT NULL,
    -- JSON array of nail_api CalendarComponent; '[]' for address books.
    components    TEXT NOT NULL DEFAULT '[]',
    read_only     INTEGER NOT NULL DEFAULT 0 CHECK (read_only IN (0, 1)),
    timezone      TEXT,
    role          TEXT CHECK (role IS NULL OR role IN ('collected')),
    ctag          TEXT,
    sync_token    TEXT,
    etag          TEXT,
    last_modified TEXT,
    last_sync_at  INTEGER,
    UNIQUE (source_id, href)
) WITHOUT ROWID;
CREATE INDEX pim_collections_source ON pim_collections(source_id, kind);
CREATE UNIQUE INDEX pim_collections_role ON pim_collections(role) WHERE role IS NOT NULL;

CREATE TABLE pim_objects (
    id            INTEGER PRIMARY KEY,
    collection_id TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    uid           TEXT NOT NULL,
    -- Resource URL on the server; NULL until uploaded (and for local / ICS).
    href          TEXT,
    etag          TEXT,
    kind          TEXT NOT NULL CHECK (kind IN ('event', 'task', 'contact', 'group')),
    -- The iCalendar / vCard text; sealed at rest.
    raw           TEXT NOT NULL,
    sync          TEXT NOT NULL DEFAULT 'synced' CHECK (sync IN ('synced', 'pending', 'conflict')),
    -- The local version kept after a conflict (the server's won); sealed.
    conflict_raw  TEXT,
    -- Deleted locally, the server delete is queued.
    deleted       INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    -- Unix seconds (UTC).
    updated_at    INTEGER NOT NULL,
    UNIQUE (collection_id, uid)
);
CREATE INDEX pim_objects_href ON pim_objects(collection_id, href) WHERE href IS NOT NULL;
CREATE INDEX pim_objects_unsynced ON pim_objects(collection_id) WHERE sync <> 'synced';

CREATE TABLE pim_events (
    id          TEXT PRIMARY KEY NOT NULL,
    object_id   INTEGER NOT NULL UNIQUE REFERENCES pim_objects(id) ON DELETE CASCADE,
    calendar_id TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    uid         TEXT NOT NULL,
    title       TEXT NOT NULL DEFAULT '',
    -- Unix seconds of the first start and the last end (NULL = recurs
    -- forever); dates and floating times as UTC wall time.
    first_start INTEGER NOT NULL,
    last_end    INTEGER,
    recurring   INTEGER NOT NULL DEFAULT 0 CHECK (recurring IN (0, 1)),
    -- JSON (nail_api CalendarEvent); sealed at rest.
    data        TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX pim_events_calendar ON pim_events(calendar_id, first_start);

CREATE TABLE pim_event_instances (
    event_id      TEXT NOT NULL REFERENCES pim_events(id) ON DELETE CASCADE,
    calendar_id   TEXT NOT NULL,
    -- The occurrence's recurrence id (nail_api PimTime as JSON); '' for a
    -- single event.
    recurrence_id TEXT NOT NULL,
    -- Unix seconds (UTC); dates and floating times as UTC wall time.
    start_at      INTEGER NOT NULL,
    end_at        INTEGER NOT NULL,
    floating      INTEGER NOT NULL DEFAULT 0 CHECK (floating IN (0, 1)),
    all_day       INTEGER NOT NULL DEFAULT 0 CHECK (all_day IN (0, 1)),
    overridden    INTEGER NOT NULL DEFAULT 0 CHECK (overridden IN (0, 1)),
    cancelled     INTEGER NOT NULL DEFAULT 0 CHECK (cancelled IN (0, 1)),
    busy          INTEGER NOT NULL DEFAULT 1 CHECK (busy IN (0, 1)),
    PRIMARY KEY (event_id, recurrence_id)
) WITHOUT ROWID;
CREATE INDEX pim_event_instances_range ON pim_event_instances(start_at, end_at);
CREATE INDEX pim_event_instances_calendar ON pim_event_instances(calendar_id, start_at);

CREATE TABLE pim_instance_windows (
    event_id       TEXT PRIMARY KEY NOT NULL REFERENCES pim_events(id) ON DELETE CASCADE,
    -- Unix seconds: the expanded range [from, until).
    expanded_from  INTEGER NOT NULL,
    expanded_until INTEGER NOT NULL,
    CHECK (expanded_from <= expanded_until)
) WITHOUT ROWID;

CREATE TABLE pim_contacts (
    id              TEXT PRIMARY KEY NOT NULL,
    object_id       INTEGER NOT NULL UNIQUE REFERENCES pim_objects(id) ON DELETE CASCADE,
    address_book_id TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    uid             TEXT NOT NULL,
    kind            TEXT NOT NULL DEFAULT 'individual'
                    CHECK (kind IN ('individual', 'organization', 'location', 'group')),
    display_name    TEXT NOT NULL DEFAULT '',
    -- Lowercase, family name first.
    sort_name       TEXT NOT NULL DEFAULT '',
    -- SHA-256 (hex) of the photo in the blob store.
    photo_sha256    TEXT CHECK (photo_sha256 IS NULL OR length(photo_sha256) = 64),
    -- JSON (nail_api ContactFields, or the group name and members); sealed.
    data            TEXT NOT NULL,
    updated_at      INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX pim_contacts_book ON pim_contacts(address_book_id, kind, sort_name);

CREATE TABLE pim_contact_emails (
    contact_id TEXT NOT NULL REFERENCES pim_contacts(id) ON DELETE CASCADE,
    -- Lowercase.
    email      TEXT NOT NULL,
    preferred  INTEGER NOT NULL DEFAULT 0 CHECK (preferred IN (0, 1)),
    PRIMARY KEY (contact_id, email)
) WITHOUT ROWID;
CREATE INDEX pim_contact_emails_email ON pim_contact_emails(email);

CREATE TABLE pim_contact_links (
    contact_id TEXT PRIMARY KEY NOT NULL REFERENCES pim_contacts(id) ON DELETE CASCADE,
    -- The merged person's primary contact.
    primary_id TEXT NOT NULL REFERENCES pim_contacts(id) ON DELETE CASCADE,
    CHECK (contact_id <> primary_id)
) WITHOUT ROWID;
CREATE INDEX pim_contact_links_primary ON pim_contact_links(primary_id);

CREATE TABLE pim_group_members (
    group_id TEXT NOT NULL REFERENCES pim_contacts(id) ON DELETE CASCADE,
    -- The member's vCard UID (as in MEMBER, e.g. urn:uuid:…) or a
    -- mailto: address.
    member   TEXT NOT NULL,
    PRIMARY KEY (group_id, member)
) WITHOUT ROWID;

CREATE TABLE pim_tasks (
    id           TEXT PRIMARY KEY NOT NULL,
    object_id    INTEGER NOT NULL UNIQUE REFERENCES pim_objects(id) ON DELETE CASCADE,
    calendar_id  TEXT NOT NULL REFERENCES pim_collections(id) ON DELETE CASCADE,
    uid          TEXT NOT NULL,
    -- UID of the parent task (RELATED-TO;RELTYPE=PARENT).
    parent_uid   TEXT,
    title        TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL DEFAULT 'needs-action'
                 CHECK (status IN ('needs-action', 'in-process', 'completed', 'cancelled')),
    priority     INTEGER CHECK (priority IS NULL OR priority BETWEEN 0 AND 9),
    -- Unix seconds; dates and floating times as UTC wall time.
    due_at       INTEGER,
    start_at     INTEGER,
    completed_at INTEGER,
    sort_order   INTEGER,
    -- JSON (nail_api Task); sealed at rest.
    data         TEXT NOT NULL,
    updated_at   INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX pim_tasks_list ON pim_tasks(calendar_id, status, due_at);
CREATE INDEX pim_tasks_parent ON pim_tasks(calendar_id, parent_uid) WHERE parent_uid IS NOT NULL;

CREATE TABLE pim_reminders (
    id            TEXT PRIMARY KEY NOT NULL,
    item_kind     TEXT NOT NULL CHECK (item_kind IN ('event', 'task')),
    item_id       TEXT NOT NULL,
    -- The occurrence (as in pim_event_instances); '' for single items.
    recurrence_id TEXT NOT NULL DEFAULT '',
    -- The alarm's UID, else its position in the item.
    alarm_key     TEXT NOT NULL,
    -- Unix seconds (UTC).
    fire_at       INTEGER NOT NULL,
    state         TEXT NOT NULL DEFAULT 'pending'
                  CHECK (state IN ('pending', 'fired', 'snoozed', 'dismissed')),
    snoozed_until INTEGER,
    updated_at    INTEGER NOT NULL,
    UNIQUE (item_kind, item_id, recurrence_id, alarm_key)
) WITHOUT ROWID;
CREATE INDEX pim_reminders_due ON pim_reminders(state, fire_at);

CREATE TRIGGER pim_events_drop_reminders AFTER DELETE ON pim_events BEGIN
    DELETE FROM pim_reminders WHERE item_kind = 'event' AND item_id = OLD.id;
END;
CREATE TRIGGER pim_tasks_drop_reminders AFTER DELETE ON pim_tasks BEGIN
    DELETE FROM pim_reminders WHERE item_kind = 'task' AND item_id = OLD.id;
END;

CREATE TABLE pim_ops (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id  TEXT NOT NULL REFERENCES pim_sources(id) ON DELETE CASCADE,
    object_id  INTEGER REFERENCES pim_objects(id) ON DELETE SET NULL,
    kind       TEXT NOT NULL,
    -- JSON: what to replay (target URL, If-Match ETag, collection, …);
    -- item content is read from pim_objects when replaying.
    payload    TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'pending'
               CHECK (state IN ('pending', 'in_flight', 'done', 'failed')),
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX pim_ops_state ON pim_ops(source_id, state, id);
