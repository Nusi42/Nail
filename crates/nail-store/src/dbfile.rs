//! Full-file database encryption (M4c WP122, ADR 0095).
//!
//! The bundled SQLite is SQLite3 Multiple Ciphers (Nail's
//! `libsqlite3-sys` patch), so a database file can be encrypted page by
//! page with ChaCha20-Poly1305 (`cipher = 'chacha20'`, the sqleet scheme:
//! authenticated pages, a per-file KDF salt in the first 16 bytes). The
//! write-ahead log is encrypted too. Plain files keep working unchanged.
//!
//! * [`file_encryption`] tells from the file header whether a database is
//!   plain (`SQLite format 3\0`), encrypted (anything else) or missing.
//! * [`Store::open_with_key`] opens either kind; a new file is created
//!   encrypted when a key is given.
//! * [`Store::rewrite_file`] converts the open database between plain and
//!   encrypted (or to another key) **crash-safely**: `VACUUM INTO` a
//!   temporary file in the target form, sync and check it, close the
//!   connection (the WAL was checkpointed and is empty), atomically rename
//!   the copy over the database and reopen. A crash before the rename
//!   leaves the old file and a stray temporary (removed next time); after
//!   it, the new file is complete. Which form the file is in is read from
//!   its header, so nothing else must be recorded first.

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::store::Store;

/// The SQLite3 Multiple Ciphers cipher scheme of encrypted databases.
pub const DATABASE_CIPHER: &str = "chacha20";

/// Suffix of the temporary copy [`Store::rewrite_file`] writes next to the
/// database.
pub const REWRITE_SUFFIX: &str = ".rewrite";

/// The first 16 bytes of every plain SQLite database.
const PLAIN_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// How often a rename over the database is retried (a virus scanner or a
/// sync client may hold the file for a moment).
const RENAME_ATTEMPTS: u32 = 40;
const RENAME_PAUSE: Duration = Duration::from_millis(50);

/// The 256-bit key of an encrypted database file. `nail-vault` derives it
/// from the data key; it never leaves memory (zeroed on drop, redacted in
/// `Debug`).
#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseKey(Zeroizing<[u8; 32]>);

impl DatabaseKey {
    /// Wraps raw key bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    fn hex(&self) -> Zeroizing<String> {
        Zeroizing::new(hex::encode(*self.0))
    }
}

impl std::fmt::Debug for DatabaseKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DatabaseKey(..)")
    }
}

/// What a database file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEncryption {
    /// No file (or an empty one): it will be created.
    Missing,
    /// A plain SQLite database.
    Plain,
    /// Not a plain SQLite database: encrypted (or not a database at all,
    /// which opening with the key then reports).
    Encrypted,
}

/// Reads the header of the database file at `path`.
pub fn file_encryption(path: &Path) -> Result<FileEncryption> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileEncryption::Missing);
        }
        Err(err) => return Err(err.into()),
    };
    let mut header = [0u8; 16];
    let mut read = 0;
    while read < header.len() {
        match file.read(&mut header[read..])? {
            0 => break,
            n => read += n,
        }
    }
    Ok(match read {
        0 => FileEncryption::Missing,
        16 if &header == PLAIN_HEADER => FileEncryption::Plain,
        _ => FileEncryption::Encrypted,
    })
}

/// The temporary copy of a rewrite.
pub fn rewrite_temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(REWRITE_SUFFIX);
    PathBuf::from(name)
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Removes a stray temporary copy of an interrupted rewrite (and its
/// journal). Called before every open.
pub fn remove_rewrite_leftovers(path: &Path) -> Result<()> {
    let tmp = rewrite_temp_path(path);
    for file in [
        sidecar(&tmp, "-journal"),
        sidecar(&tmp, "-wal"),
        sidecar(&tmp, "-shm"),
        tmp,
    ] {
        remove_if_exists(&file)?;
    }
    Ok(())
}

/// An SQLite URI for `path` with a query string (`file:C:/a%20b/x?q`).
fn sqlite_uri(path: &Path, query: &str) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:");
    if text.starts_with("//") {
        // A UNC path keeps its authority-less form: file:////server/share.
        out.push_str("//");
    }
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"/:-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out.push('?');
    out.push_str(query);
    out
}

fn is_not_a_database(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::NotADatabase
    )
}

/// Applies the key to a fresh connection and checks that it opens the
/// file (a wrong key fails with [`Error::Crypto`]).
fn apply_key(conn: &Connection, key: &DatabaseKey) -> Result<()> {
    let sql = Zeroizing::new(format!(
        "PRAGMA cipher = '{DATABASE_CIPHER}'; PRAGMA hexkey = '{}';",
        key.hex().as_str()
    ));
    conn.execute_batch(&sql)?;
    match conn.query_row("SELECT count(*) FROM sqlite_schema", [], |r| {
        r.get::<_, i64>(0)
    }) {
        Ok(_) => Ok(()),
        Err(err) if is_not_a_database(&err) => Err(Error::Crypto(
            "the database key does not open this database".into(),
        )),
        Err(err) => Err(err.into()),
    }
}

/// Opens one connection to the file at `path` with the key (if any). An
/// encrypted file without a key is [`Error::Locked`].
fn open_raw(path: &Path, key: Option<&DatabaseKey>) -> Result<Connection> {
    let state = file_encryption(path)?;
    if state == FileEncryption::Encrypted && key.is_none() {
        return Err(Error::Locked(
            "the database is encrypted and its key is not available".into(),
        ));
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )?;
    // A plain file opens plain even when a key is at hand (the vault then
    // converts it); a new file is created encrypted.
    if let Some(key) = key.filter(|_| state != FileEncryption::Plain) {
        apply_key(&conn, key)?;
    } else if state == FileEncryption::Plain {
        // Catch files that look plain but are damaged early.
        conn.query_row("SELECT count(*) FROM sqlite_schema", [], |r| {
            r.get::<_, i64>(0)
        })?;
    }
    Ok(conn)
}

/// [`open_raw`] plus WAL mode and `synchronous = NORMAL`.
pub(crate) fn open_connection(path: &Path, key: Option<&DatabaseKey>) -> Result<Connection> {
    let conn = open_raw(path, key)?;
    let journal: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !journal.eq_ignore_ascii_case("wal") {
        tracing::warn!(%journal, "could not enable WAL mode");
    }
    // NORMAL is durable in WAL mode except for the last transactions on
    // power loss, which sync repairs; it avoids an fsync per commit.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

fn rename_retrying(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(err) if attempt + 1 < RENAME_ATTEMPTS => {
                attempt += 1;
                tracing::debug!(%err, attempt, "database rename failed; retrying");
                std::thread::sleep(RENAME_PAUSE);
            }
            Err(err) => return Err(err),
        }
    }
}

impl Store {
    /// Opens (creating if needed) the database at `path` like
    /// [`Store::open`], with the key of a full-file encrypted database. A
    /// plain file opens plain even with a key; a missing file is created
    /// encrypted with it. An encrypted file needs the key
    /// ([`Error::Locked`] without, [`Error::Crypto`] with a wrong one).
    pub fn open_with_key(path: impl AsRef<Path>, key: Option<&DatabaseKey>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        remove_rewrite_leftovers(path)?;
        let conn = open_connection(path, key)?;
        let encrypted = file_encryption(path)? == FileEncryption::Encrypted;
        Self::init(
            conn,
            Some(path.to_owned()),
            key.filter(|_| encrypted).cloned(),
        )
    }

    /// Whether the open database file is encrypted (`false` for in-memory
    /// stores).
    pub fn is_file_encrypted(&self) -> bool {
        self.file_key().is_some()
    }

    /// Rewrites the database file in the target form — encrypted with
    /// `key`, or plain with `None` — crash-safely (see the [module
    /// docs](self)). Also compacts it: no free page or old WAL frame of
    /// the previous form survives in the new file. Blocks every other use
    /// of the store while it runs. In-memory stores are refused.
    pub fn rewrite_file(&self, key: Option<&DatabaseKey>) -> Result<()> {
        let path = self
            .path()
            .ok_or_else(|| Error::InvalidData("an in-memory database has no file".into()))?
            .to_owned();
        let tmp = rewrite_temp_path(&path);
        let old_key = self.file_key();
        self.with_conn(|conn| {
            remove_rewrite_leftovers(&path)?;
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            let query = match key {
                Some(key) => Zeroizing::new(format!(
                    "cipher={DATABASE_CIPHER}&hexkey={}",
                    key.hex().as_str()
                )),
                // An empty key: the copy is plain even when the main
                // database is encrypted (attached files inherit its key
                // otherwise).
                None => Zeroizing::new("key=".to_owned()),
            };
            let uri = Zeroizing::new(sqlite_uri(&tmp, &query));
            conn.execute("VACUUM INTO ?1", [uri.as_str()])?;
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&tmp)?
                .sync_all()?;
            // The copy must open with the target key and be sound before
            // it replaces anything.
            {
                let check = open_raw(&tmp, key)?;
                let verdict: String = check.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
                if verdict != "ok" {
                    return Err(Error::InvalidData(format!(
                        "the rewritten database failed its check: {verdict}"
                    )));
                }
            }
            // Close the old connection: the last close checkpoints and
            // removes the (already empty) WAL.
            let old = std::mem::replace(conn, Connection::open_in_memory()?);
            if let Err((old, err)) = old.close() {
                *conn = old;
                return Err(err.into());
            }
            let swapped = remove_if_exists(&sidecar(&path, "-wal"))
                .and_then(|()| remove_if_exists(&sidecar(&path, "-shm")))
                .and_then(|()| rename_retrying(&tmp, &path).map_err(Error::from));
            let reopen_key = if swapped.is_ok() {
                key
            } else {
                old_key.as_ref()
            };
            let reopened =
                open_connection(&path, reopen_key).and_then(|c| self.prepare_connection(c));
            match (swapped, reopened) {
                (Ok(()), Ok(new)) => {
                    *conn = new;
                    self.set_file_key(key.cloned());
                    Ok(())
                }
                (Err(err), Ok(old)) => {
                    *conn = old;
                    Err(err)
                }
                (_, Err(err)) => Err(err),
            }
        })?;
        tracing::info!(encrypted = key.is_some(), "database file rewritten");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_escape_paths() {
        assert_eq!(
            sqlite_uri(Path::new(r"C:\Users\A B\nail#1.db"), "key="),
            "file:C:/Users/A%20B/nail%231.db?key="
        );
        assert_eq!(
            sqlite_uri(Path::new("/tmp/x?y.db"), "q=1"),
            "file:/tmp/x%3Fy.db?q=1"
        );
    }

    #[test]
    fn header_detection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.db");
        assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Missing);
        std::fs::write(&path, b"").unwrap();
        assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Missing);
        std::fs::write(&path, b"SQLite format 3\0rest").unwrap();
        assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Plain);
        std::fs::write(&path, [7u8; 64]).unwrap();
        assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Encrypted);
        std::fs::write(&path, b"short").unwrap();
        assert_eq!(file_encryption(&path).unwrap(), FileEncryption::Encrypted);
    }

    #[test]
    fn keys_are_redacted() {
        let key = DatabaseKey::from_bytes([9; 32]);
        assert_eq!(format!("{key:?}"), "DatabaseKey(..)");
        assert_eq!(key.hex().len(), 64);
    }
}
