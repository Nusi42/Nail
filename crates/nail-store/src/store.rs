//! The [`Store`] handle: connection management and threading model.
//!
//! # Threading model
//!
//! A [`Store`] owns **one** SQLite connection behind a [`std::sync::Mutex`],
//! shared through an [`Arc`]; cloning a `Store` is cheap and every clone uses
//! the same connection. All methods are synchronous and short (single
//! statements or small transactions), and they are `Send + Sync`.
//!
//! From async code (tokio), use [`Store::call`], which runs a closure on
//! tokio's blocking thread pool so a slow disk never stalls the async
//! executor. Calling the synchronous methods directly from async code is
//! acceptable only in tests.
//!
//! Why one connection and not a pool: SQLite serializes writers anyway, the
//! M0 read load (one UI, one sync engine) is tiny, and a single connection
//! makes transactions trivially consistent (no `SQLITE_BUSY` between our own
//! connections). WAL mode still lets *other* processes (e.g. a future `nail`
//! CLI) read while we write. If profiling shows reader contention, a small
//! read-only connection pool can be added behind the same API
//! (see `docs/adr/0007-local-store.md`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rusqlite::Connection;

use crate::dbfile::DatabaseKey;
use crate::error::Result;
use crate::schema;

/// Conventional file name of the metadata database inside the data directory.
pub const DB_FILE_NAME: &str = "nail.sqlite3";
/// Conventional name of the blob directory inside the data directory.
pub const BLOB_DIR_NAME: &str = "blobs";

/// How long a statement waits for a lock held by another process before
/// failing with `SQLITE_BUSY`.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Handle to the metadata database. See the [module docs](self) for the
/// threading model.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    conn: Mutex<Connection>,
    /// Serializes replay passes per account so queue order holds.
    replay_locks: crate::queue::ReplayLocks,
    /// Encryption at rest (sealed columns), shared with the SQL functions.
    cipher: crate::seal::CipherSlot,
    path: Option<PathBuf>,
    /// The key of a full-file encrypted database (M4c WP122).
    file_key: Mutex<Option<DatabaseKey>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.inner.path)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Opens (creating if needed) the database at `path`, switches it to WAL
    /// mode, applies pending migrations and recovers the operation queues
    /// (mail and PIM operations left `in_flight` by a crash go back to
    /// `pending`).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_key(path, None)
    }

    /// Opens a private in-memory database (tests, demos).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, None, None)
    }

    pub(crate) fn init(
        conn: Connection,
        path: Option<PathBuf>,
        file_key: Option<DatabaseKey>,
    ) -> Result<Self> {
        let cipher = crate::seal::CipherSlot::default();
        let store = Self {
            inner: Arc::new(Inner {
                // Replaced below once the connection is prepared.
                conn: Mutex::new(Connection::open_in_memory()?),
                replay_locks: crate::queue::ReplayLocks::default(),
                cipher,
                path,
                file_key: Mutex::new(file_key),
            }),
        };
        let mut conn = store.prepare_connection(conn)?;
        let version = schema::migrate(&mut conn)?;
        *store
            .inner
            .conn
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = conn;
        let recovered = store.recover_in_flight()? + store.recover_pim_in_flight()?;
        store.recover_sending()?;
        tracing::debug!(path = ?store.inner.path, version, recovered, "store opened");
        Ok(store)
    }

    /// The database file, `None` for in-memory stores.
    pub fn path(&self) -> Option<&Path> {
        self.inner.path.as_deref()
    }

    /// The schema version of the open database.
    pub fn schema_version(&self) -> Result<u32> {
        self.with_conn(|conn| schema::current_version(conn))
    }

    /// Runs `f` with this store on tokio's blocking thread pool and returns
    /// its result. Must be called from within a tokio runtime.
    ///
    /// ```
    /// # async fn demo(store: nail_store::Store) -> nail_store::Result<()> {
    /// let accounts = store.call(|s| s.list_accounts()).await?;
    /// # Ok(()) }
    /// ```
    pub async fn call<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Store) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let store = self.clone();
        tokio::task::spawn_blocking(move || f(&store)).await?
    }

    /// Runs `f` with exclusive access to the connection.
    ///
    /// A poisoned mutex (a panic while holding it) is recovered: any open
    /// transaction was rolled back when it was dropped during unwinding, so
    /// the connection is consistent.
    pub(crate) fn with_conn<R>(&self, f: impl FnOnce(&mut Connection) -> Result<R>) -> Result<R> {
        let mut conn = self
            .inner
            .conn
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        f(&mut conn)
    }

    /// Runs `f` inside one `IMMEDIATE` transaction; commits on `Ok`, rolls
    /// back on `Err`.
    pub(crate) fn with_tx<R>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<R>,
    ) -> Result<R> {
        self.with_conn(|conn| {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let out = f(&tx)?;
            tx.commit()?;
            Ok(out)
        })
    }

    /// Per-connection settings: busy timeout, foreign keys, the sealing
    /// SQL functions and `secure_delete` while a data cipher is installed.
    pub(crate) fn prepare_connection(&self, conn: Connection) -> Result<Connection> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        crate::seal::register_functions(&conn, &self.inner.cipher)?;
        if self.inner.cipher.is_installed() {
            conn.pragma_update(None, "secure_delete", true)?;
        }
        Ok(conn)
    }

    /// The key of a full-file encrypted database (`None` = plain file).
    pub(crate) fn file_key(&self) -> Option<DatabaseKey> {
        self.inner
            .file_key
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set_file_key(&self, key: Option<DatabaseKey>) {
        *self
            .inner
            .file_key
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = key;
    }

    pub(crate) fn cipher_slot(&self) -> &crate::seal::CipherSlot {
        &self.inner.cipher
    }

    pub(crate) fn replay_locks(&self) -> &crate::queue::ReplayLocks {
        &self.inner.replay_locks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_file_in_wal_mode_with_foreign_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(DB_FILE_NAME);
        let store = Store::open(&path).unwrap();
        assert!(path.is_file());
        assert_eq!(store.path(), Some(path.as_path()));
        assert_eq!(store.schema_version().unwrap(), schema::SCHEMA_VERSION);
        store
            .with_conn(|conn| {
                let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
                assert_eq!(mode.to_ascii_lowercase(), "wal");
                let fk: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
                assert!(fk);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn reopen_keeps_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DB_FILE_NAME);
        drop(Store::open(&path).unwrap());
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), schema::SCHEMA_VERSION);
    }

    #[test]
    fn store_is_send_sync_and_clone() {
        fn assert_traits<T: Send + Sync + Clone + 'static>() {}
        assert_traits::<Store>();
    }

    #[tokio::test]
    async fn call_runs_on_blocking_pool() {
        let store = Store::open_in_memory().unwrap();
        let version = store.call(Store::schema_version).await.unwrap();
        assert_eq!(version, schema::SCHEMA_VERSION);
    }
}
