//! The remote UI's users, credentials, devices, sessions, pairing codes,
//! login throttling and audit log, and the remote-content allow-lists
//! (schema v6, ADR 0036).
//!
//! **Only hashed or referenced secrets** reach this module: password
//! hashes must be Argon2id PHC strings, TOTP secrets are `keychain:`
//! references, device secrets / session tokens / pairing codes are passed
//! as [`hash_remote_secret`] values. Hashing, TOTP and WebAuthn happen in
//! `nail-remote-auth`; this module only stores and enforces invariants:
//!
//! * grants never widen: a device's grant is narrowed to its user's, a
//!   session's to its device's and user's
//!   ([`nail_api::remote::RemoteGrant::intersect`]); narrowing a user or
//!   device narrows what hangs below it at once;
//! * a TOTP step is accepted once ([`Store::accept_totp_step`]);
//! * a pairing code is redeemed once, before it expires
//!   ([`Store::redeem_pairing_code`]);
//! * a session is found by its current token, or by the previous one only
//!   within the rotation grace period ([`Store::remote_session_by_token`]).
//!
//! Times are Unix seconds (UTC) in and RFC 3339 out, as in the rest of the
//! store. Ids: `ruser-<n>`, `rdev-<n>`, `rsess-<n>`, `passkey-<n>`,
//! `pair-<n>`.

use nail_api::remote::{
    PairingCode, PairingState, Passkey, RemoteAuditEntry, RemoteAuditKind, RemoteAuditOutcome,
    RemoteAuthMethod, RemoteDevice, RemoteGrant, RemoteSession, RemoteSessionFilter, RemoteUser,
    RemoteUserUpdate, SessionEndReason,
};
use nail_api::remote_content::{RemoteContentAllow, RemoteContentScope, normalize_allow_value};
use nail_api::{
    ErrorCode, MessageId, PairingId, PasskeyId, RemoteDeviceId, RemoteSessionId, RemoteUserId,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::store::Store;

/// Key of the remote-UI settings ([`nail_api::remote::RemoteUiSettings`])
/// in [`Store::setting`].
pub const REMOTE_UI_SETTINGS_KEY: &str = "remote-ui";

/// Key of the remote UI's certificate record (owned by `nail-server`: the
/// certificate, its fingerprint and the `keychain:` reference of its
/// private key — never the key itself) in [`Store::setting`].
pub const REMOTE_UI_CERTIFICATE_KEY: &str = "remote-ui-certificate";

/// Ended sessions returned at most by [`Store::list_remote_sessions`].
const ENDED_SESSIONS_LIMIT: u32 = 200;

/// Pairing codes listed at most (pending ones are always listed).
const PAIRING_LIST_LIMIT: u32 = 50;

/// The stored form of a device secret, session token or pairing code:
/// lowercase hex SHA-256. These are random and long (pairing codes are
/// short but single-use, short-lived and rate limited), so a plain hash is
/// enough.
pub fn hash_remote_secret(secret: &str) -> String {
    hex::encode(Sha256::digest(secret.as_bytes()))
}

// ---------------------------------------------------------------------------
// Ids, times, enums
// ---------------------------------------------------------------------------

macro_rules! row_id {
    ($make:ident, $parse:ident, $ty:ty, $prefix:literal, $kind:literal) => {
        fn $make(n: i64) -> $ty {
            <$ty>::new(format!(concat!($prefix, "{}"), n))
        }

        fn $parse(id: &$ty) -> Result<i64> {
            id.as_str()
                .strip_prefix($prefix)
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| Error::not_found($kind, id))
        }
    };
}

row_id!(user_id, parse_user, RemoteUserId, "ruser-", "remote user");
row_id!(
    device_id,
    parse_device,
    RemoteDeviceId,
    "rdev-",
    "remote device"
);
row_id!(
    session_id,
    parse_session,
    RemoteSessionId,
    "rsess-",
    "remote session"
);
row_id!(passkey_id, parse_passkey, PasskeyId, "passkey-", "passkey");
row_id!(
    pairing_id,
    parse_pairing,
    PairingId,
    "pair-",
    "pairing code"
);

/// RFC 3339 (UTC, seconds) of Unix seconds.
pub(crate) fn rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

fn opt_rfc3339(secs: Option<i64>) -> Option<String> {
    secs.map(rfc3339)
}

fn enum_str<T: serde::Serialize>(value: T) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(Error::InvalidData(format!("not a string enum: {other}"))),
    }
}

fn enum_from<T: serde::de::DeserializeOwned>(idx: usize, s: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn grant_from(idx: usize, text: String) -> rusqlite::Result<RemoteGrant> {
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn grant_json(grant: &RemoteGrant) -> Result<String> {
    Ok(serde_json::to_string(grant)?)
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// A user's stored credentials (for `nail-remote-auth`; never sent to a
/// client).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteCredentials {
    /// Argon2id PHC string, `None` = no password.
    pub password_hash: Option<String>,
    /// `keychain:` reference of the TOTP secret, `None` = no TOTP.
    pub totp_ref: Option<String>,
    /// The last accepted TOTP time step.
    pub totp_last_step: Option<i64>,
}

/// A passkey to store ([`Store::add_passkey`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPasskey {
    /// The owner.
    pub user_id: RemoteUserId,
    /// WebAuthn credential id.
    pub credential_id: Vec<u8>,
    /// Display name.
    pub name: String,
    /// The WebAuthn library's serialized credential (public data only).
    pub credential: String,
    /// The device it was registered from.
    pub device_id: Option<RemoteDeviceId>,
}

/// A stored passkey with its credential (for verification).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPasskey {
    /// The passkey.
    pub passkey: Passkey,
    /// Its owner.
    pub user_id: RemoteUserId,
    /// WebAuthn credential id.
    pub credential_id: Vec<u8>,
    /// The serialized credential.
    pub credential: String,
}

/// A device to pair ([`Store::create_remote_device`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRemoteDevice {
    /// The owner.
    pub user_id: RemoteUserId,
    /// Name.
    pub name: String,
    /// [`hash_remote_secret`] of the device cookie's secret.
    pub secret_hash: String,
    /// Requested grant (narrowed to the user's).
    pub grant: RemoteGrant,
    /// Browser `User-Agent`.
    pub user_agent: Option<String>,
    /// Client address.
    pub ip: Option<String>,
}

/// A session to start ([`Store::create_remote_session`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRemoteSession {
    /// [`hash_remote_secret`] of the session token.
    pub token_hash: String,
    /// The user.
    pub user_id: RemoteUserId,
    /// The device.
    pub device_id: RemoteDeviceId,
    /// How it logged in.
    pub auth_method: RemoteAuthMethod,
    /// Requested grant (narrowed to the device's and user's).
    pub grant: RemoteGrant,
    /// Absolute end (Unix seconds).
    pub expires_at: i64,
    /// Client address.
    pub ip: Option<String>,
    /// Browser `User-Agent`.
    pub user_agent: Option<String>,
}

/// A pairing code to store ([`Store::create_pairing_code`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPairingCode {
    /// The user the device will belong to.
    pub user_id: RemoteUserId,
    /// [`hash_remote_secret`] of the normalized code.
    pub code_hash: String,
    /// Name for the device.
    pub device_name: Option<String>,
    /// Grant of the device (narrowed to the user's).
    pub grant: RemoteGrant,
    /// Valid until (Unix seconds).
    pub expires_at: i64,
}

/// Failed-login counters of one key ([`Store::record_auth_failure`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThrottleRecord {
    /// `ip:<address>` or `user:<lowercase name>`.
    pub key: String,
    /// Failures in the current window.
    pub failures: u32,
    /// First failure of the window (Unix seconds).
    pub first_failure_at: i64,
    /// Last failure (Unix seconds).
    pub last_failure_at: i64,
    /// Locked out until (Unix seconds).
    pub locked_until: Option<i64>,
}

/// An audit entry to write ([`Store::append_remote_audit`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAuditRecord {
    /// Time (Unix seconds).
    pub at: i64,
    /// What happened.
    pub kind: RemoteAuditKind,
    /// How it ended.
    pub outcome: RemoteAuditOutcome,
    /// The user, when known.
    pub user_id: Option<RemoteUserId>,
    /// The login name.
    pub user_name: Option<String>,
    /// The device, when known.
    pub device_id: Option<RemoteDeviceId>,
    /// The session, when known.
    pub session_id: Option<RemoteSessionId>,
    /// Client address.
    pub ip: Option<String>,
    /// Browser `User-Agent`.
    pub user_agent: Option<String>,
    /// Login method.
    pub method: Option<RemoteAuthMethod>,
    /// Request type or other short name.
    pub name: Option<String>,
    /// Error class.
    pub error: Option<ErrorCode>,
    /// Diagnostic detail.
    pub detail: Option<String>,
}

impl RemoteAuditRecord {
    /// An entry with only kind, outcome and time set.
    pub fn new(at: i64, kind: RemoteAuditKind, outcome: RemoteAuditOutcome) -> Self {
        Self {
            at,
            kind,
            outcome,
            user_id: None,
            user_name: None,
            device_id: None,
            session_id: None,
            ip: None,
            user_agent: None,
            method: None,
            name: None,
            error: None,
            detail: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

const USER_COLS: &str = "id, name, display_name, created_at, last_login_at,
    password_hash IS NOT NULL, totp_ref IS NOT NULL, grant_json, disabled, locked_until";

fn user_from_row(row: &Row<'_>) -> rusqlite::Result<RemoteUser> {
    Ok(RemoteUser {
        id: user_id(row.get(0)?),
        name: row.get(1)?,
        display_name: row.get(2)?,
        created_at: rfc3339(row.get(3)?),
        last_login_at: opt_rfc3339(row.get(4)?),
        has_password: row.get(5)?,
        totp_enabled: row.get(6)?,
        passkeys: Vec::new(),
        grant: grant_from(7, row.get(7)?)?,
        disabled: row.get(8)?,
        locked_until: opt_rfc3339(row.get(9)?),
    })
}

const PASSKEY_COLS: &str =
    "id, name, created_at, last_used_at, device_id, user_id, credential_id, credential";

fn passkey_from_row(row: &Row<'_>) -> rusqlite::Result<StoredPasskey> {
    Ok(StoredPasskey {
        passkey: Passkey {
            id: passkey_id(row.get(0)?),
            name: row.get(1)?,
            created_at: rfc3339(row.get(2)?),
            last_used_at: opt_rfc3339(row.get(3)?),
            device_id: row.get::<_, Option<i64>>(4)?.map(device_id),
        },
        user_id: user_id(row.get(5)?),
        credential_id: row.get(6)?,
        credential: row.get(7)?,
    })
}

const DEVICE_COLS: &str = "id, user_id, name, paired_at, last_seen_at, user_agent, last_ip,
    grant_json, revoked_at";

fn device_from_row(row: &Row<'_>) -> rusqlite::Result<RemoteDevice> {
    Ok(RemoteDevice {
        id: device_id(row.get(0)?),
        user_id: user_id(row.get(1)?),
        name: row.get(2)?,
        paired_at: rfc3339(row.get(3)?),
        last_seen_at: opt_rfc3339(row.get(4)?),
        user_agent: row.get(5)?,
        last_ip: row.get(6)?,
        grant: grant_from(7, row.get(7)?)?,
        revoked_at: opt_rfc3339(row.get(8)?),
    })
}

const SESSION_SELECT: &str = "SELECT s.id, s.user_id, u.name, s.device_id, d.name, s.auth_method,
    s.created_at, s.last_seen_at, s.expires_at, s.ip, s.user_agent, s.reauth_at, s.grant_json,
    s.ended_at, s.end_reason
    FROM remote_sessions s
    JOIN remote_users u ON u.id = s.user_id
    JOIN remote_devices d ON d.id = s.device_id";

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<RemoteSession> {
    Ok(RemoteSession {
        id: session_id(row.get(0)?),
        user_id: user_id(row.get(1)?),
        user_name: row.get(2)?,
        device_id: device_id(row.get(3)?),
        device_name: row.get(4)?,
        auth_method: enum_from(5, row.get(5)?)?,
        created_at: rfc3339(row.get(6)?),
        last_seen_at: rfc3339(row.get(7)?),
        expires_at: rfc3339(row.get(8)?),
        ip: row.get(9)?,
        user_agent: row.get(10)?,
        reauth_at: opt_rfc3339(row.get(11)?),
        grant: grant_from(12, row.get(12)?)?,
        ended_at: opt_rfc3339(row.get(13)?),
        end_reason: row
            .get::<_, Option<String>>(14)?
            .map(|s| enum_from(14, s))
            .transpose()?,
    })
}

const PAIRING_COLS: &str = "id, user_id, device_name, grant_json, created_at, expires_at,
    used_at, cancelled_at, device_id";

fn pairing_from_row(row: &Row<'_>, now: i64) -> rusqlite::Result<PairingCode> {
    let expires_at: i64 = row.get(5)?;
    let used_at: Option<i64> = row.get(6)?;
    let cancelled_at: Option<i64> = row.get(7)?;
    let state = if used_at.is_some() {
        PairingState::Used
    } else if cancelled_at.is_some() {
        PairingState::Cancelled
    } else if expires_at <= now {
        PairingState::Expired
    } else {
        PairingState::Pending
    };
    Ok(PairingCode {
        id: pairing_id(row.get(0)?),
        user_id: user_id(row.get(1)?),
        device_name: row.get(2)?,
        grant: grant_from(3, row.get(3)?)?,
        created_at: rfc3339(row.get(4)?),
        expires_at: rfc3339(expires_at),
        state,
        device_id: row.get::<_, Option<i64>>(8)?.map(device_id),
    })
}

fn throttle_from_row(row: &Row<'_>) -> rusqlite::Result<ThrottleRecord> {
    Ok(ThrottleRecord {
        key: row.get(0)?,
        failures: row.get(1)?,
        first_failure_at: row.get(2)?,
        last_failure_at: row.get(3)?,
        locked_until: row.get(4)?,
    })
}

// ---------------------------------------------------------------------------
// Helpers inside transactions
// ---------------------------------------------------------------------------

fn user_in(tx: &Transaction<'_>, n: i64) -> Result<RemoteUser> {
    let mut user = tx
        .query_row(
            &format!("SELECT {USER_COLS} FROM remote_users WHERE id = ?1"),
            [n],
            user_from_row,
        )
        .optional()?
        .ok_or_else(|| Error::not_found("remote user", user_id(n)))?;
    user.passkeys = passkeys_in(tx, n)?.into_iter().map(|p| p.passkey).collect();
    Ok(user)
}

fn passkeys_in(tx: &Transaction<'_>, user: i64) -> Result<Vec<StoredPasskey>> {
    let mut stmt = tx.prepare_cached(&format!(
        "SELECT {PASSKEY_COLS} FROM remote_passkeys WHERE user_id = ?1 ORDER BY id"
    ))?;
    let rows = stmt.query_map([user], passkey_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn device_in(tx: &Transaction<'_>, n: i64) -> Result<RemoteDevice> {
    tx.query_row(
        &format!("SELECT {DEVICE_COLS} FROM remote_devices WHERE id = ?1"),
        [n],
        device_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("remote device", device_id(n)))
}

fn session_in(tx: &Transaction<'_>, n: i64) -> Result<RemoteSession> {
    tx.query_row(
        &format!("{SESSION_SELECT} WHERE s.id = ?1"),
        [n],
        session_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("remote session", session_id(n)))
}

/// Narrows every device of `user` and every valid session below them to
/// the grants above them.
fn narrow_below_user(tx: &Transaction<'_>, user: i64) -> Result<()> {
    let ceiling = user_in(tx, user)?.grant;
    let devices: Vec<(i64, String)> = {
        let mut stmt =
            tx.prepare_cached("SELECT id, grant_json FROM remote_devices WHERE user_id = ?1")?;
        stmt.query_map([user], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (device, grant) in devices {
        let grant: RemoteGrant = serde_json::from_str(&grant)?;
        tx.execute(
            "UPDATE remote_devices SET grant_json = ?2 WHERE id = ?1",
            params![device, grant_json(&grant.intersect(&ceiling))?],
        )?;
        narrow_below_device(tx, device)?;
    }
    Ok(())
}

/// Narrows the valid sessions of a device to the device's grant.
fn narrow_below_device(tx: &Transaction<'_>, device: i64) -> Result<()> {
    let ceiling = device_in(tx, device)?.grant;
    let sessions: Vec<(i64, String)> = {
        let mut stmt = tx.prepare_cached(
            "SELECT id, grant_json FROM remote_sessions WHERE device_id = ?1 AND ended_at IS NULL",
        )?;
        stmt.query_map([device], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (session, grant) in sessions {
        let grant: RemoteGrant = serde_json::from_str(&grant)?;
        tx.execute(
            "UPDATE remote_sessions SET grant_json = ?2 WHERE id = ?1",
            params![session, grant_json(&grant.intersect(&ceiling))?],
        )?;
    }
    Ok(())
}

/// Ends the valid sessions matching `condition` (SQL over `s`), returning
/// their ids.
fn end_sessions_where(
    tx: &Transaction<'_>,
    condition: &str,
    args: &[&dyn rusqlite::ToSql],
    reason: SessionEndReason,
    at: i64,
) -> Result<Vec<RemoteSessionId>> {
    let ids: Vec<i64> = {
        let mut stmt = tx.prepare(&format!(
            "SELECT s.id FROM remote_sessions s WHERE s.ended_at IS NULL AND ({condition})"
        ))?;
        stmt.query_map(args, |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let reason = enum_str(reason)?;
    for n in &ids {
        tx.execute(
            "UPDATE remote_sessions SET ended_at = ?2, end_reason = ?3 WHERE id = ?1",
            params![n, at, reason],
        )?;
    }
    Ok(ids.into_iter().map(session_id).collect())
}

fn check_password_hash(hash: &str) -> Result<()> {
    if hash.starts_with("$argon2id$") {
        Ok(())
    } else {
        Err(Error::InvalidData(
            "a remote password must be stored as an Argon2id PHC string".into(),
        ))
    }
}

fn check_totp_ref(reference: &str) -> Result<()> {
    if reference.starts_with("keychain:") {
        Ok(())
    } else {
        Err(Error::InvalidData(
            "a TOTP secret must be stored as a keychain: reference".into(),
        ))
    }
}

fn check_hash(hash: &str) -> Result<()> {
    if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Error::InvalidData(
            "expected a SHA-256 hash (hash_remote_secret)".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Users and credentials
// ---------------------------------------------------------------------------

impl Store {
    /// Adds a user without credentials at `at` (Unix seconds). Fails with
    /// [`Error::Conflict`] when the name is taken (case-insensitive) and
    /// [`Error::InvalidData`] for an empty name.
    pub fn create_remote_user(
        &self,
        name: &str,
        display_name: Option<&str>,
        grant: &RemoteGrant,
        at: i64,
    ) -> Result<RemoteUser> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(Error::InvalidData("a remote user needs a name".into()));
        }
        let grant = grant_json(grant)?;
        self.with_tx(|tx| {
            let taken = tx
                .query_row("SELECT 1 FROM remote_users WHERE name = ?1", [name], |_| {
                    Ok(())
                })
                .optional()?
                .is_some();
            if taken {
                return Err(Error::Conflict(format!(
                    "a remote user named {name:?} exists"
                )));
            }
            tx.execute(
                "INSERT INTO remote_users (name, display_name, grant_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![name, display_name, grant, at],
            )?;
            user_in(tx, tx.last_insert_rowid())
        })
    }

    /// All users by name.
    pub fn list_remote_users(&self) -> Result<Vec<RemoteUser>> {
        self.with_tx(|tx| {
            let ids: Vec<i64> = {
                let mut stmt = tx.prepare_cached("SELECT id FROM remote_users ORDER BY name")?;
                stmt.query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            ids.into_iter().map(|n| user_in(tx, n)).collect()
        })
    }

    /// One user.
    pub fn get_remote_user(&self, id: &RemoteUserId) -> Result<RemoteUser> {
        let n = parse_user(id)?;
        self.with_tx(|tx| user_in(tx, n))
    }

    /// The user with this login name (case-insensitive).
    pub fn remote_user_by_name(&self, name: &str) -> Result<Option<RemoteUser>> {
        self.with_tx(|tx| {
            let n: Option<i64> = tx
                .query_row(
                    "SELECT id FROM remote_users WHERE name = ?1",
                    [name.trim()],
                    |r| r.get(0),
                )
                .optional()?;
            n.map(|n| user_in(tx, n)).transpose()
        })
    }

    /// Changes a user at `at`. A narrower grant narrows the user's devices
    /// and sessions; disabling ends the user's sessions (use
    /// [`Store::end_remote_sessions`] for the event ids first when needed).
    pub fn update_remote_user(
        &self,
        id: &RemoteUserId,
        update: &RemoteUserUpdate,
        at: i64,
    ) -> Result<RemoteUser> {
        let n = parse_user(id)?;
        self.with_tx(|tx| {
            user_in(tx, n)?;
            if let Some(display_name) = &update.display_name {
                let display_name = Some(display_name.trim()).filter(|d| !d.is_empty());
                tx.execute(
                    "UPDATE remote_users SET display_name = ?2 WHERE id = ?1",
                    params![n, display_name],
                )?;
            }
            if let Some(grant) = &update.grant {
                tx.execute(
                    "UPDATE remote_users SET grant_json = ?2 WHERE id = ?1",
                    params![n, grant_json(grant)?],
                )?;
                narrow_below_user(tx, n)?;
            }
            if let Some(disabled) = update.disabled {
                tx.execute(
                    "UPDATE remote_users SET disabled = ?2 WHERE id = ?1",
                    params![n, disabled],
                )?;
                if disabled {
                    end_sessions_where(
                        tx,
                        "s.user_id = ?1",
                        &[&n],
                        SessionEndReason::UserChanged,
                        at,
                    )?;
                }
            }
            tx.execute(
                "UPDATE remote_users SET updated_at = ?2 WHERE id = ?1",
                params![n, at],
            )?;
            user_in(tx, n)
        })
    }

    /// Removes a user with passkeys, devices, sessions and pairing codes
    /// (audit entries stay, with the user id cleared). Returns whether the
    /// user existed.
    pub fn delete_remote_user(&self, id: &RemoteUserId) -> Result<bool> {
        let n = parse_user(id)?;
        self.with_conn(|conn| Ok(conn.execute("DELETE FROM remote_users WHERE id = ?1", [n])? > 0))
    }

    /// A user's stored credentials.
    pub fn remote_credentials(&self, id: &RemoteUserId) -> Result<RemoteCredentials> {
        let n = parse_user(id)?;
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT password_hash, totp_ref, totp_last_step FROM remote_users WHERE id = ?1",
                [n],
                |r| {
                    Ok(RemoteCredentials {
                        password_hash: r.get(0)?,
                        totp_ref: r.get(1)?,
                        totp_last_step: r.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| Error::not_found("remote user", id))
        })
    }

    /// Sets or removes a user's password hash (an Argon2id PHC string;
    /// anything else is [`Error::InvalidData`]).
    pub fn set_remote_password_hash(
        &self,
        id: &RemoteUserId,
        hash: Option<&str>,
        at: i64,
    ) -> Result<RemoteUser> {
        if let Some(hash) = hash {
            check_password_hash(hash)?;
        }
        let n = parse_user(id)?;
        self.with_tx(|tx| {
            user_in(tx, n)?;
            tx.execute(
                "UPDATE remote_users SET password_hash = ?2, updated_at = ?3 WHERE id = ?1",
                params![n, hash, at],
            )?;
            user_in(tx, n)
        })
    }

    /// Sets or removes the keychain reference of a user's TOTP secret
    /// (anything but `keychain:…` is [`Error::InvalidData`]); resets the
    /// replay guard.
    pub fn set_remote_totp(
        &self,
        id: &RemoteUserId,
        totp_ref: Option<&str>,
        at: i64,
    ) -> Result<RemoteUser> {
        if let Some(reference) = totp_ref {
            check_totp_ref(reference)?;
        }
        let n = parse_user(id)?;
        self.with_tx(|tx| {
            user_in(tx, n)?;
            tx.execute(
                "UPDATE remote_users SET totp_ref = ?2, totp_last_step = NULL, updated_at = ?3
                 WHERE id = ?1",
                params![n, totp_ref, at],
            )?;
            user_in(tx, n)
        })
    }

    /// Accepts TOTP time step `step` for a user once: `true` when it is
    /// newer than every step accepted before (and records it), `false` for
    /// a replayed or older code.
    pub fn accept_totp_step(&self, id: &RemoteUserId, step: i64) -> Result<bool> {
        let n = parse_user(id)?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE remote_users SET totp_last_step = ?2
                 WHERE id = ?1 AND (totp_last_step IS NULL OR totp_last_step < ?2)",
                params![n, step],
            )?;
            Ok(changed == 1)
        })
    }

    /// Records a successful login at `at` and lifts a lockout.
    pub fn record_remote_login(&self, id: &RemoteUserId, at: i64) -> Result<()> {
        let n = parse_user(id)?;
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE remote_users SET last_login_at = ?2, locked_until = NULL WHERE id = ?1",
                params![n, at],
            )?;
            Ok(())
        })
    }

    /// Locks a user out until `until` (Unix seconds), or lifts the lockout.
    pub fn set_remote_user_lock(
        &self,
        id: &RemoteUserId,
        until: Option<i64>,
    ) -> Result<RemoteUser> {
        let n = parse_user(id)?;
        self.with_tx(|tx| {
            user_in(tx, n)?;
            tx.execute(
                "UPDATE remote_users SET locked_until = ?2 WHERE id = ?1",
                params![n, until],
            )?;
            user_in(tx, n)
        })
    }

    // --- Passkeys ---

    /// Stores a registered passkey at `at`. [`Error::Conflict`] when the
    /// credential id is registered already.
    pub fn add_passkey(&self, passkey: &NewPasskey, at: i64) -> Result<StoredPasskey> {
        let user = parse_user(&passkey.user_id)?;
        let device = passkey.device_id.as_ref().map(parse_device).transpose()?;
        self.with_tx(|tx| {
            user_in(tx, user)?;
            let taken = tx
                .query_row(
                    "SELECT 1 FROM remote_passkeys WHERE credential_id = ?1",
                    [&passkey.credential_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if taken {
                return Err(Error::Conflict("the passkey is registered already".into()));
            }
            tx.execute(
                "INSERT INTO remote_passkeys (user_id, credential_id, name, credential, device_id,
                     created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    user,
                    passkey.credential_id,
                    passkey.name.trim(),
                    passkey.credential,
                    device,
                    at
                ],
            )?;
            let n = tx.last_insert_rowid();
            Ok(tx.query_row(
                &format!("SELECT {PASSKEY_COLS} FROM remote_passkeys WHERE id = ?1"),
                [n],
                passkey_from_row,
            )?)
        })
    }

    /// The passkey with this credential id.
    pub fn passkey_by_credential_id(&self, credential_id: &[u8]) -> Result<Option<StoredPasskey>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {PASSKEY_COLS} FROM remote_passkeys WHERE credential_id = ?1"),
                    [credential_id],
                    passkey_from_row,
                )
                .optional()?)
        })
    }

    /// A user's passkeys with their credentials, oldest first.
    pub fn passkeys_of_user(&self, id: &RemoteUserId) -> Result<Vec<StoredPasskey>> {
        let n = parse_user(id)?;
        self.with_tx(|tx| passkeys_in(tx, n))
    }

    /// Stores a passkey's updated credential (signature counter) after a
    /// login at `at`.
    pub fn update_passkey_credential(
        &self,
        id: &PasskeyId,
        credential: &str,
        at: i64,
    ) -> Result<()> {
        let n = parse_passkey(id)?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE remote_passkeys SET credential = ?2, last_used_at = ?3 WHERE id = ?1",
                params![n, credential, at],
            )?;
            if changed == 0 {
                return Err(Error::not_found("passkey", id));
            }
            Ok(())
        })
    }

    /// Renames a passkey; returns its owner.
    pub fn rename_passkey(&self, id: &PasskeyId, name: &str) -> Result<RemoteUser> {
        let n = parse_passkey(id)?;
        self.with_tx(|tx| {
            let user: i64 = tx
                .query_row(
                    "SELECT user_id FROM remote_passkeys WHERE id = ?1",
                    [n],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| Error::not_found("passkey", id))?;
            tx.execute(
                "UPDATE remote_passkeys SET name = ?2 WHERE id = ?1",
                params![n, name.trim()],
            )?;
            user_in(tx, user)
        })
    }

    /// Removes a passkey; returns its owner.
    pub fn delete_passkey(&self, id: &PasskeyId) -> Result<RemoteUser> {
        let n = parse_passkey(id)?;
        self.with_tx(|tx| {
            let user: i64 = tx
                .query_row(
                    "SELECT user_id FROM remote_passkeys WHERE id = ?1",
                    [n],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| Error::not_found("passkey", id))?;
            tx.execute("DELETE FROM remote_passkeys WHERE id = ?1", [n])?;
            user_in(tx, user)
        })
    }

    // --- Devices ---

    /// Pairs a device at `at`; its grant is narrowed to the user's.
    pub fn create_remote_device(&self, device: &NewRemoteDevice, at: i64) -> Result<RemoteDevice> {
        check_hash(&device.secret_hash)?;
        let user = parse_user(&device.user_id)?;
        self.with_tx(|tx| {
            let ceiling = user_in(tx, user)?.grant;
            tx.execute(
                "INSERT INTO remote_devices (user_id, name, secret_hash, grant_json, paired_at,
                     last_seen_at, user_agent, last_ip)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7)",
                params![
                    user,
                    device.name.trim(),
                    device.secret_hash,
                    grant_json(&device.grant.intersect(&ceiling))?,
                    at,
                    device.user_agent,
                    device.ip
                ],
            )?;
            device_in(tx, tx.last_insert_rowid())
        })
    }

    /// Devices (revoked ones included), most recently seen first.
    pub fn list_remote_devices(&self, user: Option<&RemoteUserId>) -> Result<Vec<RemoteDevice>> {
        let user = user.map(parse_user).transpose()?;
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {DEVICE_COLS} FROM remote_devices WHERE ?1 IS NULL OR user_id = ?1
                 ORDER BY COALESCE(last_seen_at, paired_at) DESC, id DESC"
            ))?;
            let rows = stmt.query_map([user], device_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// One device.
    pub fn get_remote_device(&self, id: &RemoteDeviceId) -> Result<RemoteDevice> {
        let n = parse_device(id)?;
        self.with_tx(|tx| device_in(tx, n))
    }

    /// The active (not revoked) device with this secret hash.
    pub fn remote_device_by_secret(&self, secret_hash: &str) -> Result<Option<RemoteDevice>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {DEVICE_COLS} FROM remote_devices
                         WHERE secret_hash = ?1 AND revoked_at IS NULL"
                    ),
                    [secret_hash],
                    device_from_row,
                )
                .optional()?)
        })
    }

    /// Renames a device and / or changes its grant (narrowed to its
    /// user's; its sessions are narrowed too).
    pub fn update_remote_device(
        &self,
        id: &RemoteDeviceId,
        name: Option<&str>,
        grant: Option<&RemoteGrant>,
    ) -> Result<RemoteDevice> {
        let n = parse_device(id)?;
        self.with_tx(|tx| {
            let device = device_in(tx, n)?;
            if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) {
                tx.execute(
                    "UPDATE remote_devices SET name = ?2 WHERE id = ?1",
                    params![n, name],
                )?;
            }
            if let Some(grant) = grant {
                let ceiling = user_in(tx, parse_user(&device.user_id)?)?.grant;
                tx.execute(
                    "UPDATE remote_devices SET grant_json = ?2 WHERE id = ?1",
                    params![n, grant_json(&grant.intersect(&ceiling))?],
                )?;
                narrow_below_device(tx, n)?;
            }
            device_in(tx, n)
        })
    }

    /// Records a request from a device at `at`.
    pub fn touch_remote_device(
        &self,
        id: &RemoteDeviceId,
        ip: Option<&str>,
        user_agent: Option<&str>,
        at: i64,
    ) -> Result<()> {
        let n = parse_device(id)?;
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE remote_devices SET last_seen_at = ?2,
                     last_ip = COALESCE(?3, last_ip), user_agent = COALESCE(?4, user_agent)
                 WHERE id = ?1",
                params![n, at, ip, user_agent],
            )?;
            Ok(())
        })
    }

    /// Revokes a device at `at` and ends its sessions; returns the ended
    /// sessions. Revoking twice ends nothing more.
    pub fn revoke_remote_device(
        &self,
        id: &RemoteDeviceId,
        at: i64,
    ) -> Result<Vec<RemoteSessionId>> {
        let n = parse_device(id)?;
        self.with_tx(|tx| {
            device_in(tx, n)?;
            tx.execute(
                "UPDATE remote_devices SET revoked_at = COALESCE(revoked_at, ?2) WHERE id = ?1",
                params![n, at],
            )?;
            end_sessions_where(
                tx,
                "s.device_id = ?1",
                &[&n],
                SessionEndReason::DeviceRevoked,
                at,
            )
        })
    }

    // --- Sessions ---

    /// Starts a session at `at`; its grant is narrowed to the device's and
    /// user's. [`Error::Conflict`] for a revoked device, a disabled user or
    /// a device of another user.
    pub fn create_remote_session(
        &self,
        session: &NewRemoteSession,
        at: i64,
    ) -> Result<RemoteSession> {
        check_hash(&session.token_hash)?;
        let user = parse_user(&session.user_id)?;
        let device = parse_device(&session.device_id)?;
        let method = enum_str(session.auth_method)?;
        self.with_tx(|tx| {
            let owner = user_in(tx, user)?;
            let paired = device_in(tx, device)?;
            if owner.disabled {
                return Err(Error::Conflict(format!(
                    "remote user {} is disabled",
                    owner.id
                )));
            }
            if paired.revoked_at.is_some() || paired.user_id != owner.id {
                return Err(Error::Conflict(format!(
                    "remote device {} cannot start a session for {}",
                    paired.id, owner.id
                )));
            }
            let grant = session
                .grant
                .intersect(&paired.grant)
                .intersect(&owner.grant);
            tx.execute(
                "INSERT INTO remote_sessions (user_id, device_id, token_hash, auth_method,
                     grant_json, created_at, last_seen_at, expires_at, rotated_at, ip, user_agent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?6, ?8, ?9)",
                params![
                    user,
                    device,
                    session.token_hash,
                    method,
                    grant_json(&grant)?,
                    at,
                    session.expires_at,
                    session.ip,
                    session.user_agent
                ],
            )?;
            session_in(tx, tx.last_insert_rowid())
        })
    }

    /// The valid session holding this token hash at `now`: by its current
    /// token, or by the token before the last rotation while its grace
    /// period lasts. Ended or expired sessions, revoked devices and
    /// disabled users yield `None`.
    pub fn remote_session_by_token(
        &self,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<RemoteSession>> {
        self.with_tx(|tx| {
            let n: Option<i64> = tx
                .query_row(
                    "SELECT s.id FROM remote_sessions s
                     JOIN remote_devices d ON d.id = s.device_id
                     JOIN remote_users u ON u.id = s.user_id
                     WHERE (s.token_hash = ?1
                            OR (s.previous_token_hash = ?1 AND s.previous_valid_until > ?2))
                       AND s.ended_at IS NULL AND s.expires_at > ?2
                       AND d.revoked_at IS NULL AND u.disabled = 0",
                    params![token_hash, now],
                    |r| r.get(0),
                )
                .optional()?;
            n.map(|n| session_in(tx, n)).transpose()
        })
    }

    /// Rotates a session's token at `at`: `new_hash` becomes current, the
    /// old one stays valid until `grace_until` (for requests in flight).
    pub fn rotate_remote_session(
        &self,
        id: &RemoteSessionId,
        new_hash: &str,
        grace_until: i64,
        at: i64,
    ) -> Result<()> {
        check_hash(new_hash)?;
        let n = parse_session(id)?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE remote_sessions SET previous_token_hash = token_hash,
                     previous_valid_until = ?3, token_hash = ?2, rotated_at = ?4
                 WHERE id = ?1 AND ended_at IS NULL",
                params![n, new_hash, grace_until, at],
            )?;
            if changed == 0 {
                return Err(Error::not_found("remote session", id));
            }
            Ok(())
        })
    }

    /// Records a request of a session at `at`.
    pub fn touch_remote_session(
        &self,
        id: &RemoteSessionId,
        ip: Option<&str>,
        at: i64,
    ) -> Result<()> {
        let n = parse_session(id)?;
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE remote_sessions SET last_seen_at = ?2, ip = COALESCE(?3, ip) WHERE id = ?1",
                params![n, at, ip],
            )?;
            Ok(())
        })
    }

    /// Records a successful re-authentication of a session at `at`.
    pub fn set_remote_reauth(&self, id: &RemoteSessionId, at: i64) -> Result<RemoteSession> {
        let n = parse_session(id)?;
        self.with_tx(|tx| {
            tx.execute(
                "UPDATE remote_sessions SET reauth_at = ?2 WHERE id = ?1 AND ended_at IS NULL",
                params![n, at],
            )?;
            session_in(tx, n)
        })
    }

    /// One session.
    pub fn get_remote_session(&self, id: &RemoteSessionId) -> Result<RemoteSession> {
        let n = parse_session(id)?;
        self.with_tx(|tx| session_in(tx, n))
    }

    /// Sessions, newest first: valid ones at `now` (not ended, not past
    /// `expires_at`), plus up to 200 ended or expired ones with
    /// `include_ended`.
    pub fn list_remote_sessions(
        &self,
        filter: &RemoteSessionFilter,
        now: i64,
    ) -> Result<Vec<RemoteSession>> {
        let user = filter.user_id.as_ref().map(parse_user).transpose()?;
        let device = filter.device_id.as_ref().map(parse_device).transpose()?;
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "{SESSION_SELECT}
                 WHERE (?1 IS NULL OR s.user_id = ?1) AND (?2 IS NULL OR s.device_id = ?2)
                   AND (?3 OR (s.ended_at IS NULL AND s.expires_at > ?4))
                 ORDER BY s.id DESC LIMIT ?5"
            ))?;
            let limit = if filter.include_ended {
                ENDED_SESSIONS_LIMIT
            } else {
                u32::MAX
            };
            let rows = stmt.query_map(
                params![user, device, filter.include_ended, now, limit],
                session_from_row,
            )?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Number of valid sessions at `now`.
    pub fn count_active_remote_sessions(&self, now: i64) -> Result<u32> {
        self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM remote_sessions WHERE ended_at IS NULL AND expires_at > ?1",
                [now],
                |r| r.get(0),
            )?)
        })
    }

    /// Ends one session at `at`; `false` when it had ended already.
    pub fn end_remote_session(
        &self,
        id: &RemoteSessionId,
        reason: SessionEndReason,
        at: i64,
    ) -> Result<bool> {
        let n = parse_session(id)?;
        self.with_tx(|tx| {
            session_in(tx, n)?;
            Ok(!end_sessions_where(tx, "s.id = ?1", &[&n], reason, at)?.is_empty())
        })
    }

    /// Ends every valid session of a user (or of everyone) at `at`;
    /// returns them.
    pub fn end_remote_sessions(
        &self,
        user: Option<&RemoteUserId>,
        reason: SessionEndReason,
        at: i64,
    ) -> Result<Vec<RemoteSessionId>> {
        let user = user.map(parse_user).transpose()?;
        self.with_tx(|tx| {
            end_sessions_where(tx, "?1 IS NULL OR s.user_id = ?1", &[&user], reason, at)
        })
    }

    /// Ends (as `expired`) every session past its maximum age or idle for
    /// `idle_secs` at `now`; returns them.
    pub fn expire_remote_sessions(&self, now: i64, idle_secs: i64) -> Result<Vec<RemoteSessionId>> {
        self.with_tx(|tx| {
            end_sessions_where(
                tx,
                "s.expires_at <= ?1 OR s.last_seen_at <= ?1 - ?2",
                &[&now, &idle_secs],
                SessionEndReason::Expired,
                now,
            )
        })
    }

    // --- Pairing codes ---

    /// Stores a pairing code at `at`; its grant is narrowed to the user's.
    pub fn create_pairing_code(&self, code: &NewPairingCode, at: i64) -> Result<PairingCode> {
        check_hash(&code.code_hash)?;
        let user = parse_user(&code.user_id)?;
        self.with_tx(|tx| {
            let ceiling = user_in(tx, user)?.grant;
            tx.execute(
                "INSERT INTO remote_pairing_codes (user_id, code_hash, device_name, grant_json,
                     created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    user,
                    code.code_hash,
                    code.device_name,
                    grant_json(&code.grant.intersect(&ceiling))?,
                    at,
                    code.expires_at
                ],
            )?;
            let n = tx.last_insert_rowid();
            Ok(tx.query_row(
                &format!("SELECT {PAIRING_COLS} FROM remote_pairing_codes WHERE id = ?1"),
                [n],
                |r| pairing_from_row(r, at),
            )?)
        })
    }

    /// Pending codes, then the newest others (up to 50), as of `now`.
    pub fn list_pairing_codes(&self, now: i64) -> Result<Vec<PairingCode>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {PAIRING_COLS} FROM remote_pairing_codes
                 ORDER BY (used_at IS NULL AND cancelled_at IS NULL AND expires_at > ?1) DESC,
                          id DESC
                 LIMIT ?2"
            ))?;
            let rows = stmt.query_map(params![now, PAIRING_LIST_LIMIT], |r| {
                pairing_from_row(r, now)
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Redeems a code by its hash at `now`: marks it used and returns it
    /// (state `used`). An unknown, used, cancelled or expired code — or a
    /// code of a disabled user — is [`Error::NotFound`] (deliberately one
    /// answer for all).
    pub fn redeem_pairing_code(&self, code_hash: &str, now: i64) -> Result<PairingCode> {
        self.with_tx(|tx| {
            let n: Option<i64> = tx
                .query_row(
                    "SELECT p.id FROM remote_pairing_codes p
                     JOIN remote_users u ON u.id = p.user_id
                     WHERE p.code_hash = ?1 AND p.used_at IS NULL AND p.cancelled_at IS NULL
                       AND p.expires_at > ?2 AND u.disabled = 0",
                    params![code_hash, now],
                    |r| r.get(0),
                )
                .optional()?;
            let n = n.ok_or_else(|| Error::not_found("pairing code", "(code)"))?;
            tx.execute(
                "UPDATE remote_pairing_codes SET used_at = ?2 WHERE id = ?1",
                params![n, now],
            )?;
            Ok(tx.query_row(
                &format!("SELECT {PAIRING_COLS} FROM remote_pairing_codes WHERE id = ?1"),
                [n],
                |r| pairing_from_row(r, now),
            )?)
        })
    }

    /// Records the device a redeemed code paired.
    pub fn set_pairing_device(&self, id: &PairingId, device: &RemoteDeviceId) -> Result<()> {
        let n = parse_pairing(id)?;
        let device = parse_device(device)?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE remote_pairing_codes SET device_id = ?2 WHERE id = ?1",
                params![n, device],
            )?;
            if changed == 0 {
                return Err(Error::not_found("pairing code", id));
            }
            Ok(())
        })
    }

    /// Cancels a pending code at `at`; `false` when it was not pending.
    pub fn cancel_pairing_code(&self, id: &PairingId, at: i64) -> Result<bool> {
        let n = parse_pairing(id)?;
        self.with_tx(|tx| {
            let exists = tx
                .query_row(
                    "SELECT 1 FROM remote_pairing_codes WHERE id = ?1",
                    [n],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                return Err(Error::not_found("pairing code", id));
            }
            Ok(tx.execute(
                "UPDATE remote_pairing_codes SET cancelled_at = ?2
                 WHERE id = ?1 AND used_at IS NULL AND cancelled_at IS NULL AND expires_at > ?2",
                params![n, at],
            )? == 1)
        })
    }

    // --- Throttling ---

    /// The failed-login counters of a key (`ip:<address>`,
    /// `user:<lowercase name>`).
    pub fn auth_throttle(&self, key: &str) -> Result<Option<ThrottleRecord>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT key, failures, first_failure_at, last_failure_at, locked_until
                     FROM remote_throttle WHERE key = ?1",
                    [key],
                    throttle_from_row,
                )
                .optional()?)
        })
    }

    /// Counts a failed login for `key` at `at`. Failures older than
    /// `window_secs` start a new window. Returns the new counters (the
    /// caller decides on backoff and lockout).
    pub fn record_auth_failure(
        &self,
        key: &str,
        at: i64,
        window_secs: i64,
    ) -> Result<ThrottleRecord> {
        self.with_tx(|tx| {
            tx.execute(
                "INSERT INTO remote_throttle (key, failures, first_failure_at, last_failure_at)
                 VALUES (?1, 1, ?2, ?2)
                 ON CONFLICT (key) DO UPDATE SET
                     failures = CASE WHEN first_failure_at <= ?2 - ?3 THEN 1 ELSE failures + 1 END,
                     first_failure_at = CASE WHEN first_failure_at <= ?2 - ?3 THEN ?2
                                             ELSE first_failure_at END,
                     last_failure_at = ?2",
                params![key, at, window_secs],
            )?;
            Ok(tx.query_row(
                "SELECT key, failures, first_failure_at, last_failure_at, locked_until
                 FROM remote_throttle WHERE key = ?1",
                [key],
                throttle_from_row,
            )?)
        })
    }

    /// Locks `key` out until `until` (Unix seconds).
    pub fn lock_auth_throttle(&self, key: &str, until: i64) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE remote_throttle SET locked_until = ?2 WHERE key = ?1",
                params![key, until],
            )?;
            Ok(())
        })
    }

    /// Forgets the counters of `key` (after a successful login).
    pub fn clear_auth_throttle(&self, key: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM remote_throttle WHERE key = ?1", [key])?;
            Ok(())
        })
    }

    // --- Audit ---

    /// Appends an audit entry; returns its id.
    pub fn append_remote_audit(&self, record: &RemoteAuditRecord) -> Result<u64> {
        let user = record.user_id.as_ref().map(parse_user).transpose()?;
        let device = record.device_id.as_ref().map(parse_device).transpose()?;
        let session = record.session_id.as_ref().map(parse_session).transpose()?;
        let kind = enum_str(record.kind)?;
        let outcome = enum_str(record.outcome)?;
        let method = record.method.map(enum_str).transpose()?;
        let error = record.error.map(|e| e.as_str());
        self.with_conn(|conn| {
            // Rows that no longer exist are recorded without the reference.
            let exists = |table: &str, n: Option<i64>| -> rusqlite::Result<Option<i64>> {
                match n {
                    None => Ok(None),
                    Some(n) => Ok(conn
                        .query_row(&format!("SELECT id FROM {table} WHERE id = ?1"), [n], |r| {
                            r.get(0)
                        })
                        .optional()?),
                }
            };
            let user = exists("remote_users", user)?;
            let device = exists("remote_devices", device)?;
            let session = exists("remote_sessions", session)?;
            conn.execute(
                "INSERT INTO remote_audit (at, kind, outcome, user_id, user_name, device_id,
                     session_id, ip, user_agent, method, name, error, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    record.at,
                    kind,
                    outcome,
                    user,
                    record.user_name,
                    device,
                    session,
                    record.ip,
                    record.user_agent,
                    method,
                    record.name,
                    error,
                    record.detail
                ],
            )?;
            Ok(u64::try_from(conn.last_insert_rowid()).unwrap_or_default())
        })
    }

    /// A page of the audit log, newest first: entries of one user (or all)
    /// with an id below `before` (or the newest).
    pub fn list_remote_audit(
        &self,
        user: Option<&RemoteUserId>,
        before: Option<u64>,
        limit: u32,
    ) -> Result<Vec<RemoteAuditEntry>> {
        let user = user.map(parse_user).transpose()?;
        let before = before.map(|b| i64::try_from(b).unwrap_or(i64::MAX));
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, at, kind, outcome, user_id, user_name, device_id, session_id, ip,
                        user_agent, method, name, error, detail
                 FROM remote_audit
                 WHERE (?1 IS NULL OR user_id = ?1) AND (?2 IS NULL OR id < ?2)
                 ORDER BY id DESC LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![user, before, limit], |r| {
                let error: Option<String> = r.get(12)?;
                Ok(RemoteAuditEntry {
                    id: r.get::<_, i64>(0)?.try_into().unwrap_or_default(),
                    at: rfc3339(r.get(1)?),
                    kind: enum_from(2, r.get(2)?)?,
                    outcome: enum_from(3, r.get(3)?)?,
                    user_id: r.get::<_, Option<i64>>(4)?.map(user_id),
                    user_name: r.get(5)?,
                    device_id: r.get::<_, Option<i64>>(6)?.map(device_id),
                    session_id: r.get::<_, Option<i64>>(7)?.map(session_id),
                    ip: r.get(8)?,
                    user_agent: r.get(9)?,
                    method: r
                        .get::<_, Option<String>>(10)?
                        .map(|m| enum_from(10, m))
                        .transpose()?,
                    name: r.get(11)?,
                    error: error.map(|e| enum_from(12, e)).transpose()?,
                    detail: r.get(13)?,
                    localized: None,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Deletes audit entries older than `before` (Unix seconds); returns
    /// how many.
    pub fn purge_remote_audit(&self, before: i64) -> Result<usize> {
        self.with_conn(|conn| Ok(conn.execute("DELETE FROM remote_audit WHERE at < ?1", [before])?))
    }

    // --- Remote content ---

    /// Adds (or refreshes) an allow-list entry at `at`. The value is
    /// normalized ([`normalize_allow_value`]); an unusable one is
    /// [`Error::InvalidData`].
    pub fn allow_remote_content(
        &self,
        scope: RemoteContentScope,
        value: &str,
        label: Option<&str>,
        at: i64,
    ) -> Result<RemoteContentAllow> {
        if scope == RemoteContentScope::Once {
            return Err(Error::InvalidData(
                "a load-once allowance is not stored".to_owned(),
            ));
        }
        let value = normalize_allow_value(scope, value).ok_or_else(|| {
            Error::InvalidData(format!("not a usable remote-content {scope:?} entry"))
        })?;
        let scope_str = enum_str(scope)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO remote_content_allow (scope, value, label, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (scope, value) DO UPDATE SET label = excluded.label,
                     created_at = excluded.created_at",
                params![scope_str, value, label, at],
            )?;
            Ok(RemoteContentAllow {
                scope,
                value,
                label: label.map(str::to_owned),
                created_at: rfc3339(at),
            })
        })
    }

    /// Every allow-list entry: messages, senders, domains; newest first
    /// within each.
    pub fn list_remote_content_allows(&self) -> Result<Vec<RemoteContentAllow>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT scope, value, label, created_at FROM remote_content_allow
                 ORDER BY CASE scope WHEN 'message' THEN 0 WHEN 'sender' THEN 1 ELSE 2 END,
                          created_at DESC, value",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(RemoteContentAllow {
                    scope: enum_from(0, r.get(0)?)?,
                    value: r.get(1)?,
                    label: r.get(2)?,
                    created_at: rfc3339(r.get(3)?),
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Removes an entry (the value is normalized first); `false` when
    /// there was none.
    pub fn remove_remote_content_allow(
        &self,
        scope: RemoteContentScope,
        value: &str,
    ) -> Result<bool> {
        if scope == RemoteContentScope::Once {
            return Ok(false);
        }
        let Some(value) = normalize_allow_value(scope, value) else {
            return Ok(false);
        };
        let scope = enum_str(scope)?;
        self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM remote_content_allow WHERE scope = ?1 AND value = ?2",
                params![scope, value],
            )? > 0)
        })
    }

    /// Removes per-message entries whose message no longer exists
    /// (deleted or expunged); returns how many.
    pub fn purge_stale_remote_content_allows(&self) -> Result<usize> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM remote_content_allow WHERE scope = 'message'
                 AND value NOT IN (SELECT id FROM messages)",
                [],
            )?)
        })
    }

    /// Why a message's remote content is allowed: the most specific
    /// matching entry (message, then sender address, then the sender's
    /// domain), `None` = blocked.
    pub fn remote_content_allowed_by(
        &self,
        message: &MessageId,
        sender: Option<&str>,
    ) -> Result<Option<RemoteContentScope>> {
        let sender_value =
            sender.and_then(|s| normalize_allow_value(RemoteContentScope::Sender, s));
        let domain = sender.and_then(nail_api::remote_content::sender_domain);
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT 1 FROM remote_content_allow WHERE scope = ?1 AND value = ?2",
            )?;
            let candidates = [
                (
                    RemoteContentScope::Message,
                    Some(message.as_str().to_owned()),
                ),
                (RemoteContentScope::Sender, sender_value),
                (RemoteContentScope::Domain, domain),
            ];
            for (scope, value) in candidates {
                let Some(value) = value else { continue };
                if stmt.exists(params![enum_str(scope)?, value])? {
                    return Ok(Some(scope));
                }
            }
            Ok(None)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_formats_unix_seconds() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_790_000_000), "2026-09-21T14:13:20Z");
        assert_eq!(rfc3339(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn schema_refuses_plain_secrets_on_raw_writes() {
        let store = Store::open_in_memory().unwrap();
        store
            .create_remote_user("ada", None, &RemoteGrant::default(), 1)
            .unwrap();
        let raw = |sql: &str| store.with_conn(|conn| Ok(conn.execute(sql, [])?));
        assert!(raw("UPDATE remote_users SET password_hash = 'hunter2'").is_err());
        assert!(raw("UPDATE remote_users SET password_hash = '$argon2i$v=19$x'").is_err());
        assert!(raw("UPDATE remote_users SET totp_ref = 'JBSWY3DPEHPK3PXP'").is_err());
        assert_eq!(
            raw("UPDATE remote_users SET password_hash = '$argon2id$v=19$x'").unwrap(),
            1
        );
        assert!(check_hash(&hash_remote_secret("x")).is_ok());
        assert!(check_hash("abc").is_err());
    }
}
