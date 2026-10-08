
CREATE TABLE crypto_keys (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    technology        TEXT NOT NULL CHECK (technology IN ('openpgp', 'smime')),
    -- OpenPGP: primary key fingerprint; S/MIME: SHA-256 of the DER
    -- certificate. Uppercase hex.
    fingerprint       TEXT NOT NULL,
    -- JSON array of user ids (OpenPGP) or the subject (S/MIME), primary first.
    user_ids          TEXT NOT NULL DEFAULT '[]',
    algorithm         TEXT NOT NULL,
    -- Unix seconds (UTC).
    created_at        INTEGER NOT NULL,
    expires_at        INTEGER,
    revoked           INTEGER NOT NULL DEFAULT 0,
    can_sign          INTEGER NOT NULL DEFAULT 0,
    can_encrypt       INTEGER NOT NULL DEFAULT 0,
    source            TEXT NOT NULL CHECK (source IN ('generated', 'imported', 'pkcs12',
                          'windows-store', 'autocrypt', 'autocrypt-gossip', 'setup-message',
                          'wkd', 'keyserver', 'message')),
    -- Transferable public key (binary packets) or DER certificate.
    public_data       BLOB NOT NULL,
    -- NULL = no secret part (a correspondent's key).
    secret_storage    TEXT CHECK (secret_storage IN ('keychain', 'passphrase', 'windows-store')),
    -- Encrypted secret material; NULL for 'windows-store'.
    secret_data       BLOB,
    -- Keychain reference (`keychain:…`) of the wrapping key, or the
    -- Windows store thumbprint.
    secret_ref        TEXT,
    -- S/MIME only.
    issuer            TEXT,
    serial            TEXT,
    imported_at       INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    UNIQUE (technology, fingerprint),
    CHECK (secret_storage IS NOT NULL OR (secret_data IS NULL AND secret_ref IS NULL))
);

CREATE TABLE crypto_key_emails (
    key_id INTEGER NOT NULL REFERENCES crypto_keys(id) ON DELETE CASCADE,
    email  TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (key_id, email)
) WITHOUT ROWID;
CREATE INDEX crypto_key_emails_email ON crypto_key_emails(email);

CREATE TABLE crypto_trust (
    technology  TEXT NOT NULL CHECK (technology IN ('openpgp', 'smime')),
    fingerprint TEXT NOT NULL,
    decision    TEXT NOT NULL CHECK (decision IN ('verified', 'rejected')),
    -- Unix seconds (UTC).
    decided_at  INTEGER NOT NULL,
    PRIMARY KEY (technology, fingerprint)
) WITHOUT ROWID;

-- Autocrypt Level 1 peer state. No foreign key on account_id: peers of a
-- removed account are removed by the core with the account.
CREATE TABLE autocrypt_peers (
    account_id          TEXT NOT NULL,
    email               TEXT NOT NULL COLLATE NOCASE,
    -- Unix seconds (UTC) of the newest message seen from the peer.
    last_seen           INTEGER NOT NULL,
    -- Date of the newest message with an Autocrypt header.
    autocrypt_timestamp INTEGER,
    -- Fingerprint of the key from that header (a crypto_keys row).
    key_fingerprint     TEXT,
    prefer_encrypt      TEXT NOT NULL DEFAULT 'nopreference'
                        CHECK (prefer_encrypt IN ('mutual', 'nopreference')),
    gossip_timestamp    INTEGER,
    gossip_fingerprint  TEXT,
    PRIMARY KEY (account_id, email)
) WITHOUT ROWID;

CREATE TABLE vault_state (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    state       TEXT NOT NULL DEFAULT 'plain'
                CHECK (state IN ('plain', 'encrypting', 'encrypted', 'decrypting')),
    phase       TEXT CHECK (phase IN ('database', 'blobs', 'index')),
    done        INTEGER NOT NULL DEFAULT 0,
    total       INTEGER NOT NULL DEFAULT 0,
    -- Keychain reference of the data key.
    key_ref     TEXT,
    -- A value encrypted with the data key, to detect a wrong key.
    key_check   BLOB,
    -- Unix seconds (UTC).
    started_at  INTEGER,
    finished_at INTEGER,
    -- Last migration error (English diagnostic), NULL when none.
    error       TEXT
);
INSERT INTO vault_state (id) VALUES (1);

CREATE TABLE message_security (
    message_id   TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    signed       INTEGER NOT NULL DEFAULT 0,
    encrypted    INTEGER NOT NULL DEFAULT 0,
    -- nail_api::security::SecurityStatus JSON; NULL = neither.
    status       TEXT,
    -- nail_api::security::AuthResults JSON; NULL = none trusted.
    auth_results TEXT,
    -- Unix seconds (UTC).
    checked_at   INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY NOT NULL,
    -- JSON.
    value TEXT NOT NULL
) WITHOUT ROWID;
