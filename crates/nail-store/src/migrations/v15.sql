
CREATE TABLE crypto_keys_v15 (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    technology        TEXT NOT NULL CHECK (technology IN ('openpgp', 'smime')),
    fingerprint       TEXT NOT NULL,
    user_ids          TEXT NOT NULL DEFAULT '[]',
    algorithm         TEXT NOT NULL,
    created_at        INTEGER NOT NULL,
    expires_at        INTEGER,
    revoked           INTEGER NOT NULL DEFAULT 0,
    can_sign          INTEGER NOT NULL DEFAULT 0,
    can_encrypt       INTEGER NOT NULL DEFAULT 0,
    source            TEXT NOT NULL CHECK (source IN ('generated', 'imported', 'pkcs12',
                          'windows-store', 'autocrypt', 'autocrypt-gossip', 'setup-message',
                          'wkd', 'keyserver', 'message', 'smartcard')),
    public_data       BLOB NOT NULL,
    secret_storage    TEXT CHECK (secret_storage IN ('keychain', 'passphrase', 'windows-store',
                          'smartcard')),
    -- Encrypted secret material; NULL for 'windows-store' and 'smartcard'.
    secret_data       BLOB,
    -- Keychain reference, Windows store thumbprint, or (smartcard) the
    -- JSON of the SmartcardKeyRef.
    secret_ref        TEXT,
    issuer            TEXT,
    serial            TEXT,
    imported_at       INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    UNIQUE (technology, fingerprint),
    CHECK (secret_storage IS NOT NULL OR (secret_data IS NULL AND secret_ref IS NULL))
);
INSERT INTO crypto_keys_v15 (id, technology, fingerprint, user_ids, algorithm, created_at,
    expires_at, revoked, can_sign, can_encrypt, source, public_data, secret_storage,
    secret_data, secret_ref, issuer, serial, imported_at, updated_at)
SELECT id, technology, fingerprint, user_ids, algorithm, created_at,
    expires_at, revoked, can_sign, can_encrypt, source, public_data, secret_storage,
    secret_data, secret_ref, issuer, serial, imported_at, updated_at
FROM crypto_keys;
DROP TABLE crypto_keys;
ALTER TABLE crypto_keys_v15 RENAME TO crypto_keys;

ALTER TABLE messages ADD COLUMN remote_thread_id TEXT;
CREATE INDEX messages_remote_thread ON messages(account_id, remote_thread_id)
    WHERE remote_thread_id IS NOT NULL;
