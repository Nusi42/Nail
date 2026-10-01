//! Hooks for encryption at rest (M2a WP33; design in
//! `docs/adr/0032-m2a-data-protection.md`).
//!
//! The store does not know keys or ciphers. `nail-vault` implements
//! [`DataCipher`] and installs it with [`Store::set_cipher`] and
//! [`crate::BlobStore::set_cipher`]. With a cipher installed:
//!
//! * **Sensitive columns** ([`SEALED_COLUMNS`]: message previews and draft
//!   payloads) are written sealed when [`SealMode::Seal`] is active and
//!   read back transparently. SQLite itself stays a plain file (no
//!   SQLCipher without OpenSSL on Windows MSVC, see the ADR), so this is
//!   column-level encryption: SQL predicates, sort keys and thread keys
//!   (subjects, addresses, dates, flags) stay readable on disk.
//! * **Blob files** are sealed after compression (see [`crate::blob`]).
//!
//! Sealed column values are TEXT: [`SEALED_FIELD_PREFIX`] followed by the
//! standard base64 of the cipher's output, so every existing read path that
//! expects TEXT keeps working. Sealing is done in SQL through two
//! application functions registered on the connection:
//! `nail_seal(context, value)` and `nail_open(context, value)`. Both pass
//! `NULL` through; `nail_open` passes plain values through and
//! `nail_seal` leaves already-sealed values alone, which makes the
//! plain ↔ sealed conversion ([`Store::convert_fields`]) idempotent and
//! resumable after a crash.
//!
//! The cipher binds each value to its column (`context`, the associated
//! data), so a sealed preview cannot be moved into a draft payload.

use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rusqlite::Connection;
use rusqlite::functions::{Context, FunctionFlags};
use rusqlite::types::ValueRef;

use crate::error::{Error, Result};
use crate::store::Store;

/// Prefix of a sealed TEXT column value. Starts with a control character
/// (U+0001) that the parser never puts into a preview or a JSON payload,
/// so plain values cannot be mistaken for sealed ones.
pub const SEALED_FIELD_PREFIX: &str = "\u{1}nlv1:";

/// The first bytes of every sealed file and of the cipher output
/// ([`DataCipher::seal`]). A zstd frame starts with `28 B5 2F FD`, so plain
/// and sealed blobs can be told apart by their first four bytes.
pub const SEALED_MAGIC: [u8; 4] = *b"NLV\x01";

/// Marker the SQL functions put in front of their error messages, so the
/// store can map them back to [`Error::Locked`] / [`Error::Crypto`] (the
/// error crosses SQLite's C API as text).
const LOCKED_MARKER: &str = "nail-seal locked: ";
const CRYPTO_MARKER: &str = "nail-seal crypto: ";

/// Why sealing or opening failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CipherError {
    /// No key is available (the vault is not unlocked).
    #[error("{0}")]
    Locked(String),
    /// The data is damaged, was sealed with another key or for another
    /// context, or has an unknown format.
    #[error("{0}")]
    Failed(String),
}

impl From<CipherError> for Error {
    fn from(err: CipherError) -> Self {
        match err {
            CipherError::Locked(message) => Error::Locked(message),
            CipherError::Failed(message) => Error::Crypto(message),
        }
    }
}

/// Authenticated encryption with the data key, provided by `nail-vault`.
///
/// `seal` output must start with [`SEALED_MAGIC`]; `open` must reject data
/// sealed under another `context` (use it as associated data).
pub trait DataCipher: Send + Sync + fmt::Debug {
    /// Encrypts `plain`, bound to `context`.
    fn seal(&self, context: &[u8], plain: &[u8]) -> std::result::Result<Vec<u8>, CipherError>;
    /// Decrypts and authenticates what [`DataCipher::seal`] produced.
    fn open(&self, context: &[u8], sealed: &[u8]) -> std::result::Result<Vec<u8>, CipherError>;
}

/// Whether new writes are sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealMode {
    /// Write plain (plain data dir, or decrypting). Sealed data is still
    /// read when a cipher is installed.
    Plain,
    /// Seal every new write (encrypting or encrypted).
    Seal,
}

#[derive(Debug, Clone)]
struct Installed {
    cipher: Arc<dyn DataCipher>,
    mode: SealMode,
}

/// The cipher shared by every clone of a [`Store`] or
/// [`crate::BlobStore`] (and the SQL functions).
#[derive(Debug, Clone, Default)]
pub(crate) struct CipherSlot {
    inner: Arc<RwLock<Option<Installed>>>,
}

impl CipherSlot {
    pub(crate) fn set(&self, cipher: Option<Arc<dyn DataCipher>>, mode: SealMode) {
        let mut slot = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        *slot = cipher.map(|cipher| Installed { cipher, mode });
    }

    fn get(&self) -> Option<Installed> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether new writes are sealed.
    pub(crate) fn seals(&self) -> bool {
        self.get().is_some_and(|i| i.mode == SealMode::Seal)
    }

    /// Seals `plain` if the mode says so; returns `None` when it stays plain.
    pub(crate) fn seal_if_active(
        &self,
        context: &[u8],
        plain: &[u8],
    ) -> std::result::Result<Option<Vec<u8>>, CipherError> {
        match self.get() {
            Some(installed) if installed.mode == SealMode::Seal => {
                installed.cipher.seal(context, plain).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// Seals `plain` (whatever the mode).
    pub(crate) fn seal(
        &self,
        context: &[u8],
        plain: &[u8],
    ) -> std::result::Result<Vec<u8>, CipherError> {
        let installed = self.get().ok_or_else(no_key)?;
        let sealed = installed.cipher.seal(context, plain)?;
        if !sealed.starts_with(&SEALED_MAGIC) {
            return Err(CipherError::Failed(
                "cipher output lacks the sealed-data magic".into(),
            ));
        }
        Ok(sealed)
    }

    /// Opens sealed data.
    pub(crate) fn open(
        &self,
        context: &[u8],
        sealed: &[u8],
    ) -> std::result::Result<Vec<u8>, CipherError> {
        let installed = self.get().ok_or_else(no_key)?;
        installed.cipher.open(context, sealed)
    }

    fn seal_field(&self, column: &str, value: &str) -> std::result::Result<String, CipherError> {
        let sealed = self.seal(field_context(column).as_bytes(), value.as_bytes())?;
        Ok(format!("{SEALED_FIELD_PREFIX}{}", BASE64.encode(sealed)))
    }

    fn open_field(&self, column: &str, value: &str) -> std::result::Result<String, CipherError> {
        let Some(encoded) = value.strip_prefix(SEALED_FIELD_PREFIX) else {
            return Ok(value.to_owned());
        };
        let sealed = BASE64
            .decode(encoded)
            .map_err(|_| CipherError::Failed(format!("sealed {column} value is not base64")))?;
        let plain = self.open(field_context(column).as_bytes(), &sealed)?;
        String::from_utf8(plain)
            .map_err(|_| CipherError::Failed(format!("sealed {column} value is not UTF-8")))
    }
}

fn no_key() -> CipherError {
    CipherError::Locked("the data key is not available (vault locked)".into())
}

fn field_context(column: &str) -> String {
    format!("nail/field/{column}")
}

/// Whether a column value is sealed.
pub fn is_sealed_field(value: &str) -> bool {
    value.starts_with(SEALED_FIELD_PREFIX)
}

/// A column holding sensitive content that is sealed at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SealedColumn {
    /// Table.
    pub table: &'static str,
    /// Column (TEXT).
    pub column: &'static str,
    /// Primary-key column used to address rows while converting.
    pub key: &'static str,
}

impl SealedColumn {
    /// `table.column`, the sealing context.
    pub fn name(&self) -> String {
        format!("{}.{}", self.table, self.column)
    }
}

/// The columns sealed at rest. Everything else in the database stays
/// plain; see the ADR for the reasoning (they are SQL predicates, sort or
/// thread keys) and the residual risk.
pub const SEALED_COLUMNS: &[SealedColumn] = &[
    // The list preview: the first ~200 characters of the body.
    SealedColumn {
        table: "messages",
        column: "preview",
        key: "id",
    },
    // The full draft (body, recipients, attachments list) as JSON.
    SealedColumn {
        table: "drafts",
        column: "payload",
        key: "id",
    },
    // Plug-in storage values (JSON) and log messages (M2d, ADR 0050).
    SealedColumn {
        table: "plugin_storage",
        column: "value",
        key: "id",
    },
    SealedColumn {
        table: "plugin_logs",
        column: "message",
        key: "seq",
    },
    // Calendar, contact and task data (M3a, ADR 0058): the iCalendar /
    // vCard resources, kept conflict copies and the parsed items.
    SealedColumn {
        table: "pim_objects",
        column: "raw",
        key: "id",
    },
    SealedColumn {
        table: "pim_objects",
        column: "conflict_raw",
        key: "id",
    },
    SealedColumn {
        table: "pim_events",
        column: "data",
        key: "id",
    },
    SealedColumn {
        table: "pim_contacts",
        column: "data",
        key: "id",
    },
    SealedColumn {
        table: "pim_tasks",
        column: "data",
        key: "id",
    },
    // Exchange automatic replies and the Exchange operation queue (M3c,
    // ADR 0072): out-of-office texts.
    SealedColumn {
        table: "exchange_automatic_replies",
        column: "settings",
        key: "account_id",
    },
    SealedColumn {
        table: "exchange_ops",
        column: "payload",
        key: "id",
    },
];

/// Progress of the sealed columns: how many values are sealed and how many
/// are plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldCounts {
    /// Sealed values.
    pub sealed: u64,
    /// Plain (non-NULL) values.
    pub plain: u64,
}

impl FieldCounts {
    /// All values.
    pub fn total(&self) -> u64 {
        self.sealed + self.plain
    }
}

fn user_error(err: CipherError) -> rusqlite::Error {
    let message = match err {
        CipherError::Locked(m) => format!("{LOCKED_MARKER}{m}"),
        CipherError::Failed(m) => format!("{CRYPTO_MARKER}{m}"),
    };
    rusqlite::Error::UserFunctionError(message.into())
}

/// Maps an SQLite error raised by `nail_seal` / `nail_open` back to
/// [`Error::Locked`] / [`Error::Crypto`]; other errors stay SQLite errors.
pub(crate) fn map_sqlite_error(err: rusqlite::Error) -> Error {
    let text = match &err {
        rusqlite::Error::SqliteFailure(_, Some(message)) => Some(message.as_str()),
        rusqlite::Error::UserFunctionError(inner) => {
            // Not reached through SQLite, but keep the mapping total.
            return map_message(&inner.to_string()).unwrap_or(Error::Sqlite(err));
        }
        _ => None,
    };
    text.and_then(map_message).unwrap_or(Error::Sqlite(err))
}

fn map_message(text: &str) -> Option<Error> {
    if let Some(rest) = text.strip_prefix(LOCKED_MARKER) {
        Some(Error::Locked(rest.to_owned()))
    } else {
        text.strip_prefix(CRYPTO_MARKER)
            .map(|rest| Error::Crypto(rest.to_owned()))
    }
}

fn text_arg(ctx: &Context<'_>, idx: usize) -> rusqlite::Result<Option<String>> {
    match ctx.get_raw(idx) {
        ValueRef::Null => Ok(None),
        ValueRef::Text(bytes) => Ok(Some(String::from_utf8_lossy(bytes).into_owned())),
        ValueRef::Integer(i) => Ok(Some(i.to_string())),
        ValueRef::Real(r) => Ok(Some(r.to_string())),
        ValueRef::Blob(_) => Err(rusqlite::Error::UserFunctionError(
            "nail_seal / nail_open expect TEXT".into(),
        )),
    }
}

/// Registers `nail_seal(column, value)` and `nail_open(column, value)` on
/// a connection. `DIRECTONLY`: they cannot be called from triggers or views
/// stored in the database file.
pub(crate) fn register_functions(conn: &Connection, slot: &CipherSlot) -> rusqlite::Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DIRECTONLY;
    let seal_slot = slot.clone();
    conn.create_scalar_function("nail_seal", 2, flags, move |ctx| {
        let column = text_arg(ctx, 0)?.unwrap_or_default();
        let Some(value) = text_arg(ctx, 1)? else {
            return Ok(None);
        };
        if is_sealed_field(&value) || !seal_slot.seals() {
            return Ok(Some(value));
        }
        seal_slot
            .seal_field(&column, &value)
            .map(Some)
            .map_err(user_error)
    })?;
    let open_slot = slot.clone();
    conn.create_scalar_function("nail_open", 2, flags, move |ctx| {
        let column = text_arg(ctx, 0)?.unwrap_or_default();
        let Some(value) = text_arg(ctx, 1)? else {
            return Ok(None);
        };
        open_slot
            .open_field(&column, &value)
            .map(Some)
            .map_err(user_error)
    })?;
    Ok(())
}

impl Store {
    /// Installs (or removes, with `None`) the data cipher for sealed
    /// columns. Every clone of this store sees the change. With
    /// [`SealMode::Seal`], new writes of [`SEALED_COLUMNS`] are sealed and
    /// `secure_delete` is switched on, so SQLite zeroes the space of
    /// overwritten plain values.
    pub fn set_cipher(&self, cipher: Option<Arc<dyn DataCipher>>, mode: SealMode) -> Result<()> {
        let secure = cipher.is_some();
        self.cipher_slot().set(cipher, mode);
        self.with_conn(|conn| {
            conn.pragma_update(None, "secure_delete", secure)?;
            Ok(())
        })
    }

    /// Whether new writes of sealed columns are sealed now.
    pub fn seals_writes(&self) -> bool {
        self.cipher_slot().seals()
    }

    /// How many values of [`SEALED_COLUMNS`] are sealed and plain.
    pub fn sealed_field_counts(&self) -> Result<FieldCounts> {
        self.with_conn(|conn| {
            let mut counts = FieldCounts::default();
            for col in SEALED_COLUMNS {
                let (sealed, plain): (i64, i64) = conn.query_row(
                    &format!(
                        "SELECT
                           COALESCE(SUM(substr({c}, 1, ?1) = ?2), 0),
                           COALESCE(SUM(substr({c}, 1, ?1) <> ?2), 0)
                         FROM {t} WHERE {c} IS NOT NULL",
                        c = col.column,
                        t = col.table
                    ),
                    rusqlite::params![prefix_len(), SEALED_FIELD_PREFIX],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                counts.sealed += u64::try_from(sealed).unwrap_or(0);
                counts.plain += u64::try_from(plain).unwrap_or(0);
            }
            Ok(counts)
        })
    }

    /// Converts up to `limit` values of [`SEALED_COLUMNS`] to sealed
    /// (`seal = true`) or plain form, in one transaction. Returns how many
    /// were converted; `0` means nothing is left. Needs an installed
    /// cipher either way. Idempotent: values already in the target form
    /// are skipped, so a crash between batches loses nothing.
    pub fn convert_fields(&self, seal: bool, limit: usize) -> Result<usize> {
        let slot = self.cipher_slot().clone();
        if slot.get().is_none() {
            return Err(no_key().into());
        }
        let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
        self.with_tx(|tx| {
            let mut converted = 0usize;
            for col in SEALED_COLUMNS {
                let remaining = limit - i64::try_from(converted).unwrap_or(i64::MAX);
                if remaining <= 0 {
                    break;
                }
                let op = if seal { "<>" } else { "=" };
                let rows: Vec<(rusqlite::types::Value, String)> = {
                    let mut stmt = tx.prepare(&format!(
                        "SELECT {k}, {c} FROM {t}
                         WHERE {c} IS NOT NULL AND substr({c}, 1, ?1) {op} ?2
                         LIMIT ?3",
                        k = col.key,
                        c = col.column,
                        t = col.table
                    ))?;
                    stmt.query_map(
                        rusqlite::params![prefix_len(), SEALED_FIELD_PREFIX, remaining],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )?
                    .collect::<rusqlite::Result<_>>()?
                };
                let name = col.name();
                let mut update = tx.prepare(&format!(
                    "UPDATE {t} SET {c} = ?2 WHERE {k} = ?1",
                    k = col.key,
                    c = col.column,
                    t = col.table
                ))?;
                for (key, value) in rows {
                    let new = if seal {
                        slot.seal_field(&name, &value)?
                    } else {
                        slot.open_field(&name, &value)?
                    };
                    update.execute(rusqlite::params![key, new])?;
                    converted += 1;
                }
            }
            Ok(converted)
        })
    }

    /// Rewrites the database file so no old page (in free lists or the
    /// write-ahead log) keeps a value from before a conversion: checkpoint
    /// and truncate the WAL, `VACUUM` with temporary data in memory (not in
    /// the system temp directory), checkpoint again.
    pub fn compact(&self) -> Result<()> {
        self.with_conn(|conn| {
            if self.path().is_some() {
                conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            }
            conn.pragma_update(None, "temp_store", "MEMORY")?;
            conn.execute_batch("VACUUM")?;
            conn.pragma_update(None, "temp_store", "DEFAULT")?;
            if self.path().is_some() {
                conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            }
            Ok(())
        })
    }
}

fn prefix_len() -> i64 {
    // substr() counts characters; the prefix is ASCII apart from U+0001,
    // which is one character too.
    i64::try_from(SEALED_FIELD_PREFIX.chars().count()).unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A reversible test cipher: magic + context length + context + bytes
    /// XOR 0x5A. Not encryption; enough to test the plumbing.
    #[derive(Debug)]
    pub(crate) struct XorCipher;

    impl DataCipher for XorCipher {
        fn seal(&self, context: &[u8], plain: &[u8]) -> std::result::Result<Vec<u8>, CipherError> {
            let mut out = SEALED_MAGIC.to_vec();
            out.push(u8::try_from(context.len()).unwrap());
            out.extend_from_slice(context);
            out.extend(plain.iter().map(|b| b ^ 0x5A));
            Ok(out)
        }

        fn open(&self, context: &[u8], sealed: &[u8]) -> std::result::Result<Vec<u8>, CipherError> {
            let rest = sealed
                .strip_prefix(&SEALED_MAGIC)
                .ok_or_else(|| CipherError::Failed("magic".into()))?;
            let (&len, rest) = rest
                .split_first()
                .ok_or_else(|| CipherError::Failed("short".into()))?;
            let (ctx, body) = rest.split_at(usize::from(len));
            if ctx != context {
                return Err(CipherError::Failed("context".into()));
            }
            Ok(body.iter().map(|b| b ^ 0x5A).collect())
        }
    }

    #[test]
    fn field_round_trip_and_context_binding() {
        let slot = CipherSlot::default();
        assert!(matches!(
            slot.seal_field("messages.preview", "x"),
            Err(CipherError::Locked(_))
        ));
        slot.set(Some(Arc::new(XorCipher)), SealMode::Seal);
        let sealed = slot.seal_field("messages.preview", "Hello Sam").unwrap();
        assert!(is_sealed_field(&sealed));
        assert!(!sealed.contains("Hello"));
        assert_eq!(
            slot.open_field("messages.preview", &sealed).unwrap(),
            "Hello Sam"
        );
        assert!(matches!(
            slot.open_field("drafts.payload", &sealed),
            Err(CipherError::Failed(_))
        ));
        // Plain values pass through.
        assert_eq!(
            slot.open_field("messages.preview", "plain").unwrap(),
            "plain"
        );
        // Broken base64 is a crypto error, not a panic.
        assert!(matches!(
            slot.open_field("messages.preview", &format!("{SEALED_FIELD_PREFIX}@@")),
            Err(CipherError::Failed(_))
        ));
    }

    #[test]
    fn sql_errors_map_back_to_store_errors() {
        let err = map_sqlite_error(user_error(CipherError::Locked("no key".into())));
        assert!(matches!(err, Error::Locked(m) if m == "no key"));
        let err = map_sqlite_error(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(1),
            Some(format!("{CRYPTO_MARKER}bad tag")),
        ));
        assert!(matches!(err, Error::Crypto(m) if m == "bad tag"));
        let err = map_sqlite_error(rusqlite::Error::QueryReturnedNoRows);
        assert!(matches!(err, Error::Sqlite(_)));
    }
}
