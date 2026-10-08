
CREATE TABLE remote_users (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name   TEXT,
    -- Argon2id PHC string; NULL = no password.
    password_hash  TEXT CHECK (password_hash IS NULL OR substr(password_hash, 1, 10) = '$argon2id$'),
    -- Keychain reference of the TOTP secret; NULL = no TOTP.
    totp_ref       TEXT CHECK (totp_ref IS NULL OR substr(totp_ref, 1, 9) = 'keychain:'),
    -- Last accepted TOTP time step (a code is accepted once).
    totp_last_step INTEGER,
    -- nail_api::remote::RemoteGrant JSON: the most devices and sessions get.
    grant_json     TEXT NOT NULL DEFAULT '{}',
    disabled       INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds (UTC).
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    last_login_at  INTEGER,
    locked_until   INTEGER
);

CREATE TABLE remote_devices (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES remote_users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- SHA-256 (hex) of the device cookie's secret.
    secret_hash  TEXT NOT NULL UNIQUE,
    grant_json   TEXT NOT NULL DEFAULT '{}',
    -- Unix seconds (UTC).
    paired_at    INTEGER NOT NULL,
    last_seen_at INTEGER,
    user_agent   TEXT,
    last_ip      TEXT,
    revoked_at   INTEGER
);
CREATE INDEX remote_devices_user ON remote_devices(user_id);

CREATE TABLE remote_passkeys (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id       INTEGER NOT NULL REFERENCES remote_users(id) ON DELETE CASCADE,
    -- WebAuthn credential id (public).
    credential_id BLOB NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    -- The WebAuthn library's serialized credential (public key, algorithm,
    -- signature counter, flags); no secret.
    credential    TEXT NOT NULL,
    device_id     INTEGER REFERENCES remote_devices(id) ON DELETE SET NULL,
    -- Unix seconds (UTC).
    created_at    INTEGER NOT NULL,
    last_used_at  INTEGER
);
CREATE INDEX remote_passkeys_user ON remote_passkeys(user_id);

CREATE TABLE remote_sessions (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id              INTEGER NOT NULL REFERENCES remote_users(id) ON DELETE CASCADE,
    device_id            INTEGER NOT NULL REFERENCES remote_devices(id) ON DELETE CASCADE,
    -- SHA-256 (hex) of the session token.
    token_hash           TEXT NOT NULL UNIQUE,
    -- The token before the last rotation, accepted until previous_valid_until.
    previous_token_hash  TEXT UNIQUE,
    previous_valid_until INTEGER,
    auth_method          TEXT NOT NULL
                         CHECK (auth_method IN ('passkey', 'password', 'password-totp', 'pairing')),
    grant_json           TEXT NOT NULL DEFAULT '{}',
    -- Unix seconds (UTC).
    created_at           INTEGER NOT NULL,
    last_seen_at         INTEGER NOT NULL,
    expires_at           INTEGER NOT NULL,
    rotated_at           INTEGER NOT NULL,
    reauth_at            INTEGER,
    ip                   TEXT,
    user_agent           TEXT,
    ended_at             INTEGER,
    end_reason           TEXT CHECK (end_reason IN ('logout', 'revoked', 'expired',
                             'device-revoked', 'user-changed'))
);
CREATE INDEX remote_sessions_user ON remote_sessions(user_id, id DESC);
CREATE INDEX remote_sessions_device ON remote_sessions(device_id);

CREATE TABLE remote_pairing_codes (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES remote_users(id) ON DELETE CASCADE,
    -- SHA-256 (hex) of the normalized code.
    code_hash    TEXT NOT NULL UNIQUE,
    device_name  TEXT,
    grant_json   TEXT NOT NULL DEFAULT '{}',
    -- Unix seconds (UTC).
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    used_at      INTEGER,
    cancelled_at INTEGER,
    device_id    INTEGER REFERENCES remote_devices(id) ON DELETE SET NULL
);

-- Failed-login counters: key 'ip:<address>' or 'user:<lowercase name>'.
CREATE TABLE remote_throttle (
    key              TEXT PRIMARY KEY NOT NULL,
    failures         INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds (UTC).
    first_failure_at INTEGER NOT NULL,
    last_failure_at  INTEGER NOT NULL,
    locked_until     INTEGER
) WITHOUT ROWID;

CREATE TABLE remote_audit (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Unix seconds (UTC).
    at         INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    outcome    TEXT NOT NULL,
    user_id    INTEGER REFERENCES remote_users(id) ON DELETE SET NULL,
    user_name  TEXT,
    device_id  INTEGER REFERENCES remote_devices(id) ON DELETE SET NULL,
    session_id INTEGER REFERENCES remote_sessions(id) ON DELETE SET NULL,
    ip         TEXT,
    user_agent TEXT,
    method     TEXT,
    name       TEXT,
    error      TEXT,
    detail     TEXT
);
CREATE INDEX remote_audit_user ON remote_audit(user_id, id DESC);

CREATE TABLE remote_content_allow (
    scope      TEXT NOT NULL CHECK (scope IN ('message', 'sender', 'domain')),
    -- Message id, lowercase address or lowercase domain.
    value      TEXT NOT NULL,
    label      TEXT,
    -- Unix seconds (UTC).
    created_at INTEGER NOT NULL,
    PRIMARY KEY (scope, value)
) WITHOUT ROWID;
