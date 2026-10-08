
CREATE TABLE plugins (
    id             TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 64),
    version        TEXT NOT NULL,
    -- JSON (nail_api::plugins::PluginManifest), as in the package.
    manifest       TEXT NOT NULL,
    source_kind    TEXT NOT NULL CHECK (source_kind IN ('file', 'url', 'dev')),
    -- Path, URL or folder.
    source         TEXT NOT NULL,
    package_sha256 TEXT CHECK (package_sha256 IS NULL OR length(package_sha256) = 64),
    enabled        INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    -- JSON object: setting key -> value (only values the user changed).
    settings       TEXT NOT NULL DEFAULT '{}',
    storage_quota  INTEGER NOT NULL CHECK (storage_quota >= 0),
    -- Unix seconds (UTC).
    installed_at   INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    -- JSON (nail_api::ApiError) of what stopped it last; NULL = nothing.
    last_error     TEXT
) WITHOUT ROWID;

CREATE TABLE plugin_grants (
    plugin_id  TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    -- Permission name (nail_api::plugins::PluginPermission).
    permission TEXT NOT NULL,
    -- JSON array of account ids; NULL = every account.
    accounts   TEXT,
    granted_at INTEGER NOT NULL,
    PRIMARY KEY (plugin_id, permission)
) WITHOUT ROWID;

CREATE TABLE plugin_storage (
    id         INTEGER PRIMARY KEY,
    plugin_id  TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    key        TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 256),
    -- JSON value; sealed at rest.
    value      TEXT NOT NULL,
    size       INTEGER NOT NULL CHECK (size >= 0),
    updated_at INTEGER NOT NULL,
    UNIQUE (plugin_id, key)
);

CREATE TABLE plugin_logs (
    seq       INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_id TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    -- Unix milliseconds (UTC).
    at        INTEGER NOT NULL,
    level     TEXT NOT NULL CHECK (level IN ('debug', 'info', 'warn', 'error')),
    source    TEXT NOT NULL CHECK (source IN ('plugin', 'host', 'api')),
    -- Sealed at rest.
    message   TEXT NOT NULL,
    -- JSON (nail_api::LocalizedMessage) for host lines; NULL otherwise.
    localized TEXT
);
CREATE INDEX plugin_logs_by_plugin ON plugin_logs(plugin_id, seq);
