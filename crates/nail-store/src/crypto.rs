//! Keyring, trust decisions, Autocrypt peers, encryption-at-rest state,
//! cached message security and settings (schema v5, ADR 0029).
//!
//! The store keeps bytes and metadata only; parsing keys, checking
//! signatures and wrapping secret keys is the job of `nail-pgp`,
//! `nail-smime` and `nail-vault`. Secret key material handed to
//! [`Store::upsert_key`] must already be encrypted ([`SecretKeyData`]).
//!
//! Key ids are `key-<n>`. Times are Unix seconds (UTC).

use nail_api::security::{
    AuthResults, AutocryptPreference, CryptoTechnology, KeySource, SecretStorage, SecurityStatus,
    TrustDecision, VaultPhase, VaultState,
};
use nail_api::{AccountId, KeyId, MessageId};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::store::Store;

/// Settings key of [`nail_api::security::SecuritySettings`] in
/// [`Store::setting`].
pub const SECURITY_SETTINGS_KEY: &str = "security";

fn key_id(n: i64) -> KeyId {
    KeyId(format!("key-{n}"))
}

fn parse_key_id(id: &KeyId) -> Result<i64> {
    id.as_str()
        .strip_prefix("key-")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| Error::not_found("key", id))
}

fn parse_technology(text: &str) -> rusqlite::Result<CryptoTechnology> {
    match text {
        "openpgp" => Ok(CryptoTechnology::OpenPgp),
        "smime" => Ok(CryptoTechnology::Smime),
        other => Err(bad_value("technology", other)),
    }
}

fn parse_source(text: &str) -> rusqlite::Result<KeySource> {
    Ok(match text {
        "generated" => KeySource::Generated,
        "imported" => KeySource::Imported,
        "pkcs12" => KeySource::Pkcs12,
        "windows-store" => KeySource::WindowsStore,
        "autocrypt" => KeySource::Autocrypt,
        "autocrypt-gossip" => KeySource::AutocryptGossip,
        "setup-message" => KeySource::SetupMessage,
        "wkd" => KeySource::Wkd,
        "keyserver" => KeySource::Keyserver,
        "message" => KeySource::Message,
        other => return Err(bad_value("source", other)),
    })
}

fn storage_str(storage: SecretStorage) -> &'static str {
    match storage {
        SecretStorage::Keychain => "keychain",
        SecretStorage::Passphrase => "passphrase",
        SecretStorage::WindowsStore => "windows-store",
    }
}

fn parse_storage(text: &str) -> rusqlite::Result<SecretStorage> {
    Ok(match text {
        "keychain" => SecretStorage::Keychain,
        "passphrase" => SecretStorage::Passphrase,
        "windows-store" => SecretStorage::WindowsStore,
        other => return Err(bad_value("secret_storage", other)),
    })
}

fn parse_decision(text: &str) -> rusqlite::Result<TrustDecision> {
    match text {
        "verified" => Ok(TrustDecision::Verified),
        "rejected" => Ok(TrustDecision::Rejected),
        other => Err(bad_value("decision", other)),
    }
}

fn preference_str(preference: AutocryptPreference) -> &'static str {
    match preference {
        AutocryptPreference::Mutual => "mutual",
        AutocryptPreference::NoPreference => "nopreference",
    }
}

fn parse_vault_state(text: &str) -> rusqlite::Result<VaultState> {
    Ok(match text {
        "plain" => VaultState::Plain,
        "encrypting" => VaultState::Encrypting,
        "encrypted" => VaultState::Encrypted,
        "decrypting" => VaultState::Decrypting,
        other => return Err(bad_value("vault state", other)),
    })
}

fn parse_phase(text: &str) -> rusqlite::Result<VaultPhase> {
    Ok(match text {
        "database" => VaultPhase::Database,
        "blobs" => VaultPhase::Blobs,
        "index" => VaultPhase::Index,
        other => return Err(bad_value("vault phase", other)),
    })
}

fn bad_value(what: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        format!("unknown {what} {value:?}").into(),
    )
}

fn normalize_fingerprint(fingerprint: &str) -> Result<String> {
    let hex: String = fingerprint.chars().filter(|c| *c != ' ').collect();
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::InvalidData(format!(
            "not a hexadecimal fingerprint: {fingerprint:?}"
        )));
    }
    Ok(hex.to_ascii_uppercase())
}

/// The secret part of an own key, already encrypted by the crypto crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretKeyData {
    /// How it is protected.
    pub storage: SecretStorage,
    /// Encrypted secret material; `None` for [`SecretStorage::WindowsStore`].
    pub data: Option<Vec<u8>>,
    /// Keychain reference of the wrapping key, or the Windows store
    /// thumbprint; `None` for passphrase-protected keys.
    pub reference: Option<String>,
}

/// A key or certificate to store (see [`Store::upsert_key`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyData {
    /// OpenPGP or S/MIME.
    pub technology: CryptoTechnology,
    /// Primary key fingerprint / SHA-256 of the certificate (hex; spaces
    /// and case are normalized).
    pub fingerprint: String,
    /// User ids or the subject, primary first.
    pub user_ids: Vec<String>,
    /// Addresses the key is for (stored lowercase, deduplicated).
    pub emails: Vec<String>,
    /// Display name of the algorithm, e.g. `Ed25519`.
    pub algorithm: String,
    /// Creation / validity start.
    pub created_at: i64,
    /// Expiry, `None` = never.
    pub expires_at: Option<i64>,
    /// Revoked.
    pub revoked: bool,
    /// Usable for signing.
    pub can_sign: bool,
    /// Usable for encryption.
    pub can_encrypt: bool,
    /// Where it came from.
    pub source: KeySource,
    /// Binary transferable public key or DER certificate.
    pub public_data: Vec<u8>,
    /// The encrypted secret part of an own key.
    pub secret: Option<SecretKeyData>,
    /// S/MIME issuer DN.
    pub issuer: Option<String>,
    /// S/MIME serial (hex).
    pub serial: Option<String>,
}

/// A stored key with its id, the user's trust decision and timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredKey {
    /// `key-<n>`.
    pub id: KeyId,
    /// The key.
    pub data: KeyData,
    /// The user's decision, `None` when there is none.
    pub decision: Option<TrustDecision>,
    /// First stored.
    pub imported_at: i64,
    /// Last changed.
    pub updated_at: i64,
}

impl StoredKey {
    /// Whether this is an own key (it has a secret part).
    pub fn is_own(&self) -> bool {
        self.data.secret.is_some()
    }
}

/// Autocrypt Level 1 state of one peer, per own account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocryptPeerRecord {
    /// The own account.
    pub account_id: AccountId,
    /// The peer's address (stored lowercase).
    pub email: String,
    /// Newest message seen from the peer.
    pub last_seen: i64,
    /// Date of the newest message with an `Autocrypt` header.
    pub autocrypt_timestamp: Option<i64>,
    /// Fingerprint of the key from that header.
    pub key_fingerprint: Option<String>,
    /// The peer's `prefer-encrypt`.
    pub prefer_encrypt: AutocryptPreference,
    /// Date of the newest gossip about the peer.
    pub gossip_timestamp: Option<i64>,
    /// Fingerprint of the gossiped key.
    pub gossip_fingerprint: Option<String>,
}

/// Encryption-at-rest state (one row).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VaultRecord {
    /// Current state.
    pub state: VaultState,
    /// Phase of a running migration.
    pub phase: Option<VaultPhase>,
    /// Items done in the phase.
    pub done: u64,
    /// Items in the phase.
    pub total: u64,
    /// Keychain reference of the data key.
    pub key_ref: Option<String>,
    /// A value encrypted with the data key (wrong-key detection).
    pub key_check: Option<Vec<u8>>,
    /// Migration start.
    pub started_at: Option<i64>,
    /// Migration end.
    pub finished_at: Option<i64>,
    /// Last migration error.
    pub error: Option<String>,
}

/// Cached security findings of one message (never decrypted content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSecurityRecord {
    /// The message.
    pub message_id: MessageId,
    /// End-to-end status, `None` = neither signed nor encrypted.
    pub status: Option<SecurityStatus>,
    /// Trusted sender authentication, if any.
    pub auth_results: Option<AuthResults>,
    /// When it was checked.
    pub checked_at: i64,
}

const KEY_COLS: &str = "k.id, k.technology, k.fingerprint, k.user_ids, k.algorithm, k.created_at,
    k.expires_at, k.revoked, k.can_sign, k.can_encrypt, k.source, k.public_data,
    k.secret_storage, k.secret_data, k.secret_ref, k.issuer, k.serial, k.imported_at,
    k.updated_at, t.decision";

const KEY_FROM: &str = "crypto_keys k LEFT JOIN crypto_trust t
    ON t.technology = k.technology AND t.fingerprint = k.fingerprint";

fn key_from_row(row: &Row<'_>) -> rusqlite::Result<(i64, StoredKey)> {
    let n: i64 = row.get(0)?;
    let user_ids: String = row.get(3)?;
    let user_ids: Vec<String> = serde_json::from_str(&user_ids).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, e.into())
    })?;
    let secret = match row.get::<_, Option<String>>(12)? {
        None => None,
        Some(storage) => Some(SecretKeyData {
            storage: parse_storage(&storage)?,
            data: row.get(13)?,
            reference: row.get(14)?,
        }),
    };
    let decision = match row.get::<_, Option<String>>(19)? {
        None => None,
        Some(text) => Some(parse_decision(&text)?),
    };
    Ok((
        n,
        StoredKey {
            id: key_id(n),
            data: KeyData {
                technology: parse_technology(&row.get::<_, String>(1)?)?,
                fingerprint: row.get(2)?,
                user_ids,
                emails: Vec::new(),
                algorithm: row.get(4)?,
                created_at: row.get(5)?,
                expires_at: row.get(6)?,
                revoked: row.get(7)?,
                can_sign: row.get(8)?,
                can_encrypt: row.get(9)?,
                source: parse_source(&row.get::<_, String>(10)?)?,
                public_data: row.get(11)?,
                secret,
                issuer: row.get(15)?,
                serial: row.get(16)?,
            },
            decision,
            imported_at: row.get(17)?,
            updated_at: row.get(18)?,
        },
    ))
}

fn emails_of(tx: &rusqlite::Connection, n: i64) -> Result<Vec<String>> {
    let mut stmt =
        tx.prepare_cached("SELECT email FROM crypto_key_emails WHERE key_id = ?1 ORDER BY email")?;
    let rows = stmt.query_map([n], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn query_keys(
    conn: &rusqlite::Connection,
    where_clause: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<StoredKey>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {KEY_COLS} FROM {KEY_FROM} {where_clause}
         ORDER BY (k.secret_storage IS NULL), k.id"
    ))?;
    let rows = stmt
        .query_map(params, key_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut keys = Vec::with_capacity(rows.len());
    for (n, mut key) in rows {
        key.data.emails = emails_of(conn, n)?;
        keys.push(key);
    }
    Ok(keys)
}

fn get_key_in(conn: &rusqlite::Connection, n: i64) -> Result<StoredKey> {
    query_keys(conn, "WHERE k.id = ?1", &[&n])?
        .pop()
        .ok_or_else(|| Error::not_found("key", key_id(n)))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn write_emails(tx: &Transaction<'_>, n: i64, emails: &[String]) -> Result<()> {
    tx.execute("DELETE FROM crypto_key_emails WHERE key_id = ?1", [n])?;
    let mut stmt = tx.prepare_cached(
        "INSERT OR IGNORE INTO crypto_key_emails (key_id, email) VALUES (?1, ?2)",
    )?;
    for email in emails {
        let email = email.trim().to_lowercase();
        if !email.is_empty() {
            stmt.execute(params![n, email])?;
        }
    }
    Ok(())
}

impl Store {
    /// Inserts a key, or updates the key with the same technology and
    /// fingerprint. An update keeps an existing secret part when `key`
    /// has none (importing someone's public key never drops an own
    /// secret key) and keeps the user's trust decision. Returns the key as
    /// stored and whether it is new.
    pub fn upsert_key(&self, key: &KeyData) -> Result<(StoredKey, bool)> {
        let fingerprint = normalize_fingerprint(&key.fingerprint)?;
        if key.public_data.is_empty() {
            return Err(Error::InvalidData("a key needs its public part".into()));
        }
        if let Some(secret) = &key.secret {
            let consistent = match secret.storage {
                SecretStorage::WindowsStore => secret.reference.is_some() && secret.data.is_none(),
                SecretStorage::Keychain => secret.reference.is_some() && secret.data.is_some(),
                SecretStorage::Passphrase => secret.data.is_some(),
            };
            if !consistent {
                return Err(Error::InvalidData(
                    "secret key storage does not match its data".into(),
                ));
            }
        }
        let user_ids = serde_json::to_string(&key.user_ids)?;
        self.with_tx(|tx| {
            let now = now();
            let existing: Option<i64> = tx
                .query_row(
                    "SELECT id FROM crypto_keys WHERE technology = ?1 AND fingerprint = ?2",
                    params![key.technology.as_str(), fingerprint],
                    |r| r.get(0),
                )
                .optional()?;
            let (storage, data, reference) = match &key.secret {
                Some(s) => (
                    Some(storage_str(s.storage)),
                    s.data.clone(),
                    s.reference.clone(),
                ),
                None => (None, None, None),
            };
            let n = match existing {
                None => {
                    tx.execute(
                        "INSERT INTO crypto_keys (technology, fingerprint, user_ids, algorithm,
                             created_at, expires_at, revoked, can_sign, can_encrypt, source,
                             public_data, secret_storage, secret_data, secret_ref, issuer, serial,
                             imported_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                                 ?15, ?16, ?17, ?17)",
                        params![
                            key.technology.as_str(),
                            fingerprint,
                            user_ids,
                            key.algorithm,
                            key.created_at,
                            key.expires_at,
                            key.revoked,
                            key.can_sign,
                            key.can_encrypt,
                            key.source.as_str(),
                            key.public_data,
                            storage,
                            data,
                            reference,
                            key.issuer,
                            key.serial,
                            now
                        ],
                    )?;
                    tx.last_insert_rowid()
                }
                Some(n) => {
                    tx.execute(
                        "UPDATE crypto_keys SET user_ids = ?2, algorithm = ?3, created_at = ?4,
                             expires_at = ?5, revoked = ?6, can_sign = ?7, can_encrypt = ?8,
                             source = ?9, public_data = ?10, issuer = ?11, serial = ?12,
                             updated_at = ?13
                         WHERE id = ?1",
                        params![
                            n,
                            user_ids,
                            key.algorithm,
                            key.created_at,
                            key.expires_at,
                            key.revoked,
                            key.can_sign,
                            key.can_encrypt,
                            key.source.as_str(),
                            key.public_data,
                            key.issuer,
                            key.serial,
                            now
                        ],
                    )?;
                    if key.secret.is_some() {
                        tx.execute(
                            "UPDATE crypto_keys SET secret_storage = ?2, secret_data = ?3,
                                 secret_ref = ?4 WHERE id = ?1",
                            params![n, storage, data, reference],
                        )?;
                    }
                    n
                }
            };
            write_emails(tx, n, &key.emails)?;
            Ok((get_key_in(tx, n)?, existing.is_none()))
        })
    }

    /// One key.
    pub fn get_key(&self, id: &KeyId) -> Result<StoredKey> {
        let n = parse_key_id(id)?;
        self.with_conn(|conn| get_key_in(conn, n))
    }

    /// The key with this technology and fingerprint, if stored.
    pub fn find_key(
        &self,
        technology: CryptoTechnology,
        fingerprint: &str,
    ) -> Result<Option<StoredKey>> {
        let fingerprint = normalize_fingerprint(fingerprint)?;
        self.with_conn(|conn| {
            Ok(query_keys(
                conn,
                "WHERE k.technology = ?1 AND k.fingerprint = ?2",
                &[&technology.as_str(), &fingerprint],
            )?
            .pop())
        })
    }

    /// All keys (own keys first), optionally of one technology and only
    /// own keys.
    pub fn list_keys(
        &self,
        technology: Option<CryptoTechnology>,
        own_only: bool,
    ) -> Result<Vec<StoredKey>> {
        let tech = technology.map(CryptoTechnology::as_str);
        self.with_conn(|conn| {
            query_keys(
                conn,
                "WHERE (?1 IS NULL OR k.technology = ?1)
                   AND (?2 = 0 OR k.secret_storage IS NOT NULL)",
                &[&tech, &own_only],
            )
        })
    }

    /// Keys for an address (case-insensitive), own keys first.
    pub fn keys_for_email(
        &self,
        email: &str,
        technology: Option<CryptoTechnology>,
    ) -> Result<Vec<StoredKey>> {
        let email = email.trim().to_lowercase();
        let tech = technology.map(CryptoTechnology::as_str);
        self.with_conn(|conn| {
            query_keys(
                conn,
                "WHERE k.id IN (SELECT key_id FROM crypto_key_emails WHERE email = ?1)
                   AND (?2 IS NULL OR k.technology = ?2)",
                &[&email, &tech],
            )
        })
    }

    /// Deletes a key. A key with a secret part is deleted only with
    /// `delete_secret` (else [`Error::Conflict`]). The trust decision
    /// stays (it is keyed by fingerprint). Returns whether it existed.
    pub fn delete_key(&self, id: &KeyId, delete_secret: bool) -> Result<bool> {
        let n = parse_key_id(id)?;
        self.with_tx(|tx| {
            let secret: Option<Option<String>> = tx
                .query_row(
                    "SELECT secret_storage FROM crypto_keys WHERE id = ?1",
                    [n],
                    |r| r.get(0),
                )
                .optional()?;
            match secret {
                None => Ok(false),
                Some(Some(_)) if !delete_secret => Err(Error::Conflict(format!(
                    "key {id} has a secret part; delete it explicitly"
                ))),
                Some(_) => {
                    tx.execute("DELETE FROM crypto_keys WHERE id = ?1", [n])?;
                    Ok(true)
                }
            }
        })
    }

    /// Stores or clears the user's trust decision for a fingerprint.
    pub fn set_trust_decision(
        &self,
        technology: CryptoTechnology,
        fingerprint: &str,
        decision: TrustDecision,
    ) -> Result<()> {
        let fingerprint = normalize_fingerprint(fingerprint)?;
        self.with_conn(|conn| {
            match decision {
                TrustDecision::Unset => {
                    conn.execute(
                        "DELETE FROM crypto_trust WHERE technology = ?1 AND fingerprint = ?2",
                        params![technology.as_str(), fingerprint],
                    )?;
                }
                decision => {
                    conn.execute(
                        "INSERT INTO crypto_trust (technology, fingerprint, decision, decided_at)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT (technology, fingerprint)
                         DO UPDATE SET decision = excluded.decision, decided_at = excluded.decided_at",
                        params![technology.as_str(), fingerprint, decision.as_str(), now()],
                    )?;
                }
            }
            Ok(())
        })
    }

    /// The user's trust decision for a fingerprint, if any.
    pub fn trust_decision(
        &self,
        technology: CryptoTechnology,
        fingerprint: &str,
    ) -> Result<Option<TrustDecision>> {
        let fingerprint = normalize_fingerprint(fingerprint)?;
        self.with_conn(|conn| {
            let text: Option<String> = conn
                .query_row(
                    "SELECT decision FROM crypto_trust WHERE technology = ?1 AND fingerprint = ?2",
                    params![technology.as_str(), fingerprint],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(text.map(|t| parse_decision(&t)).transpose()?)
        })
    }

    /// Autocrypt state of a peer for an own account.
    pub fn autocrypt_peer(
        &self,
        account_id: &AccountId,
        email: &str,
    ) -> Result<Option<AutocryptPeerRecord>> {
        let email = email.trim().to_lowercase();
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT account_id, email, last_seen, autocrypt_timestamp, key_fingerprint,
                            prefer_encrypt, gossip_timestamp, gossip_fingerprint
                     FROM autocrypt_peers WHERE account_id = ?1 AND email = ?2",
                    params![account_id.as_str(), email],
                    peer_from_row,
                )
                .optional()?)
        })
    }

    /// Autocrypt state of a peer address in every own account.
    pub fn autocrypt_peers_for_email(&self, email: &str) -> Result<Vec<AutocryptPeerRecord>> {
        let email = email.trim().to_lowercase();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT account_id, email, last_seen, autocrypt_timestamp, key_fingerprint,
                        prefer_encrypt, gossip_timestamp, gossip_fingerprint
                 FROM autocrypt_peers WHERE email = ?1 ORDER BY account_id",
            )?;
            let rows = stmt.query_map([email], peer_from_row)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Inserts or replaces a peer's Autocrypt state. The caller applies
    /// the Autocrypt update rules (only newer headers win).
    pub fn put_autocrypt_peer(&self, peer: &AutocryptPeerRecord) -> Result<()> {
        let email = peer.email.trim().to_lowercase();
        if email.is_empty() {
            return Err(Error::InvalidData(
                "an Autocrypt peer needs an address".into(),
            ));
        }
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO autocrypt_peers (account_id, email, last_seen,
                     autocrypt_timestamp, key_fingerprint, prefer_encrypt, gossip_timestamp,
                     gossip_fingerprint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    peer.account_id.as_str(),
                    email,
                    peer.last_seen,
                    peer.autocrypt_timestamp,
                    peer.key_fingerprint,
                    preference_str(peer.prefer_encrypt),
                    peer.gossip_timestamp,
                    peer.gossip_fingerprint
                ],
            )?;
            Ok(())
        })
    }

    /// Removes every Autocrypt peer of an account (when it is removed).
    /// Returns how many there were.
    pub fn delete_autocrypt_peers(&self, account_id: &AccountId) -> Result<usize> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM autocrypt_peers WHERE account_id = ?1",
                [account_id.as_str()],
            )?)
        })
    }

    /// The encryption-at-rest state.
    pub fn vault_state(&self) -> Result<VaultRecord> {
        self.with_conn(|conn| {
            let row = conn.query_row(
                "SELECT state, phase, done, total, key_ref, key_check, started_at, finished_at,
                        error FROM vault_state WHERE id = 1",
                [],
                |r| {
                    let phase: Option<String> = r.get(1)?;
                    Ok(VaultRecord {
                        state: parse_vault_state(&r.get::<_, String>(0)?)?,
                        phase: phase.map(|p| parse_phase(&p)).transpose()?,
                        done: r.get::<_, i64>(2)?.try_into().unwrap_or(0),
                        total: r.get::<_, i64>(3)?.try_into().unwrap_or(0),
                        key_ref: r.get(4)?,
                        key_check: r.get(5)?,
                        started_at: r.get(6)?,
                        finished_at: r.get(7)?,
                        error: r.get(8)?,
                    })
                },
            )?;
            Ok(row)
        })
    }

    /// Replaces the encryption-at-rest state.
    pub fn set_vault_state(&self, record: &VaultRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE vault_state SET state = ?1, phase = ?2, done = ?3, total = ?4,
                     key_ref = ?5, key_check = ?6, started_at = ?7, finished_at = ?8, error = ?9
                 WHERE id = 1",
                params![
                    record.state.as_str(),
                    record.phase.map(VaultPhase::as_str),
                    i64::try_from(record.done).unwrap_or(i64::MAX),
                    i64::try_from(record.total).unwrap_or(i64::MAX),
                    record.key_ref,
                    record.key_check,
                    record.started_at,
                    record.finished_at,
                    record.error
                ],
            )?;
            Ok(())
        })
    }

    /// Cached security findings of a message, if checked.
    pub fn message_security(&self, id: &MessageId) -> Result<Option<MessageSecurityRecord>> {
        self.with_conn(|conn| {
            let row: Option<(Option<String>, Option<String>, i64)> = conn
                .query_row(
                    "SELECT status, auth_results, checked_at FROM message_security
                     WHERE message_id = ?1",
                    [id.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((status, auth, checked_at)) = row else {
                return Ok(None);
            };
            Ok(Some(MessageSecurityRecord {
                message_id: id.clone(),
                status: status.map(|s| serde_json::from_str(&s)).transpose()?,
                auth_results: auth.map(|s| serde_json::from_str(&s)).transpose()?,
                checked_at,
            }))
        })
    }

    /// Stores the security findings of a message (replacing earlier ones).
    pub fn put_message_security(&self, record: &MessageSecurityRecord) -> Result<()> {
        let signed = record
            .status
            .as_ref()
            .is_some_and(SecurityStatus::is_signed);
        let encrypted = record
            .status
            .as_ref()
            .is_some_and(SecurityStatus::is_encrypted);
        let status = record
            .status
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let auth = record
            .auth_results
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO message_security (message_id, signed, encrypted, status,
                     auth_results, checked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    record.message_id.as_str(),
                    signed,
                    encrypted,
                    status,
                    auth,
                    record.checked_at
                ],
            )?;
            Ok(())
        })
    }

    /// Forgets all cached security findings (after the keyring changed in a
    /// way that changes verification results). Returns how many there were.
    pub fn clear_message_security(&self) -> Result<usize> {
        self.with_conn(|conn| Ok(conn.execute("DELETE FROM message_security", [])?))
    }

    /// A JSON setting, `None` when never stored.
    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.with_conn(|conn| {
            let text: Option<String> = conn
                .query_row(
                    "SELECT value FROM app_settings WHERE key = ?1",
                    [key],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(text.map(|t| serde_json::from_str(&t)).transpose()?)
        })
    }

    /// Stores a JSON setting.
    pub fn set_setting<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<()> {
        let text = serde_json::to_string(value)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, text],
            )?;
            Ok(())
        })
    }
}

fn peer_from_row(row: &Row<'_>) -> rusqlite::Result<AutocryptPeerRecord> {
    let preference: String = row.get(5)?;
    Ok(AutocryptPeerRecord {
        account_id: AccountId(row.get(0)?),
        email: row.get(1)?,
        last_seen: row.get(2)?,
        autocrypt_timestamp: row.get(3)?,
        key_fingerprint: row.get(4)?,
        prefer_encrypt: if preference == "mutual" {
            AutocryptPreference::Mutual
        } else {
            AutocryptPreference::NoPreference
        },
        gossip_timestamp: row.get(6)?,
        gossip_fingerprint: row.get(7)?,
    })
}
