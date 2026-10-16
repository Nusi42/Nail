//! Queued folder changes and folder paths (schema v20, ADR 0196).
//!
//! `folder_ops` queues create / update (rename, move, into Trash) / delete
//! per account; the core replays them before the message queue and changes
//! the `folders` rows once the server confirmed. `folders.delimiter` keeps
//! the IMAP hierarchy separator `LIST` reported, so child paths can be
//! built. [`Store::move_folder_tree`] renames a folder and its subfolders in
//! place: ids, messages and sync state stay.

use nail_api::{AccountId, FolderId, MessageId, OpId};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::Store;
use crate::util::collect_ids;

/// Schema v20.
pub(crate) const V20: &str = r"
ALTER TABLE folders ADD COLUMN delimiter TEXT;

CREATE TABLE folder_ops (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- nail_store::FolderOp as JSON.
    payload    TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'pending'
               CHECK (state IN ('pending', 'done', 'failed')),
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX folder_ops_state ON folder_ops(account_id, state, id);
";

/// A queued folder change. Stored as JSON, so variants and fields may only
/// be added once released.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FolderOp {
    /// Create a folder named `name` below `parent_id` (`None` = top level).
    Create {
        /// The parent folder.
        parent_id: Option<FolderId>,
        /// The display name.
        name: String,
    },
    /// Give a folder this name and parent (rename, move, move into Trash).
    Update {
        /// The folder.
        folder_id: FolderId,
        /// The new display name.
        name: String,
        /// The new parent (`None` = top level).
        parent_id: Option<FolderId>,
    },
    /// Delete a folder, its subfolders and their messages for good.
    Delete {
        /// The folder.
        folder_id: FolderId,
    },
}

/// A queued folder change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedFolderOp {
    /// Row id.
    pub id: i64,
    /// The account.
    pub account_id: AccountId,
    /// What to do.
    pub op: FolderOp,
    /// Attempts so far.
    pub attempts: u32,
}

impl QueuedFolderOp {
    /// The operation's id as the protocol shows it (`folder-<n>`).
    pub fn op_id(&self) -> OpId {
        folder_op_id(self.id)
    }
}

/// The protocol id of a folder operation row.
pub fn folder_op_id(id: i64) -> OpId {
    OpId::new(format!("folder-{id}"))
}

/// How replaying a folder change ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderOpOutcome {
    /// Done.
    Done,
    /// Try again later (offline, timeout).
    Retry(String),
    /// Failed for good.
    Failed(String),
}

impl Store {
    /// Queues a folder change; returns its row id.
    pub fn enqueue_folder_op(&self, account: &AccountId, op: &FolderOp) -> Result<i64> {
        let payload = serde_json::to_string(op)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO folder_ops (account_id, payload) VALUES (?1, ?2)",
                params![account.as_str(), payload],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// The account's pending folder changes, oldest first.
    pub fn pending_folder_ops(&self, account: &AccountId) -> Result<Vec<QueuedFolderOp>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, payload, attempts FROM folder_ops
                 WHERE account_id = ?1 AND state = 'pending' ORDER BY id",
            )?;
            let rows = stmt.query_map([account.as_str()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(|(id, payload, attempts)| {
                    Ok(QueuedFolderOp {
                        id,
                        account_id: account.clone(),
                        op: serde_json::from_str(&payload)?,
                        attempts,
                    })
                })
                .collect()
        })
    }

    /// Whether the account has pending folder changes.
    pub fn has_pending_folder_ops(&self, account: &AccountId) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM folder_ops WHERE account_id = ?1 AND state = 'pending' LIMIT 1",
                    [account.as_str()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }

    /// Whether the account has queued writes to replay: message
    /// operations or folder changes.
    pub fn has_pending_writes(&self, account: &AccountId) -> Result<bool> {
        Ok(self.accounts_with_pending_ops()?.contains(account)
            || self.has_pending_folder_ops(account)?)
    }

    /// Records how replaying a folder change ended.
    pub fn finish_folder_op(&self, id: i64, outcome: &FolderOpOutcome) -> Result<()> {
        let (state, error) = match outcome {
            FolderOpOutcome::Done => ("done", None),
            FolderOpOutcome::Retry(message) => ("pending", Some(message.as_str())),
            FolderOpOutcome::Failed(message) => ("failed", Some(message.as_str())),
        };
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE folder_ops SET state = ?2, last_error = ?3, attempts = attempts + 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![id, state, error],
            )?;
            Ok(())
        })
    }

    /// Records the IMAP hierarchy separator of a folder (`None` = flat).
    pub fn set_folder_delimiter(&self, id: &FolderId, delimiter: Option<&str>) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE folders SET delimiter = ?2 WHERE id = ?1 AND delimiter IS NOT ?2",
                params![id.as_str(), delimiter],
            )?;
            Ok(())
        })
    }

    /// The hierarchy separator of a folder, else the one most folders of
    /// its account use; `None` when none is known.
    pub fn folder_delimiter(
        &self,
        account: &AccountId,
        id: Option<&FolderId>,
    ) -> Result<Option<String>> {
        self.with_conn(|conn| {
            if let Some(id) = id {
                let own: Option<String> = conn
                    .query_row(
                        "SELECT delimiter FROM folders WHERE id = ?1",
                        [id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                if own.is_some() {
                    return Ok(own);
                }
            }
            Ok(conn
                .query_row(
                    "SELECT delimiter FROM folders WHERE account_id = ?1 AND delimiter IS NOT NULL
                     GROUP BY delimiter ORDER BY COUNT(*) DESC, delimiter LIMIT 1",
                    [account.as_str()],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    /// The ids of every message in a folder (with or without a UID).
    pub fn folder_message_ids(&self, id: &FolderId) -> Result<Vec<MessageId>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached("SELECT id FROM messages WHERE folder_id = ?1")?;
            collect_ids(&mut stmt, [id.as_str()])
        })
    }

    /// The ids of a folder's subfolders at every depth (not the folder).
    pub fn folder_descendants(&self, id: &FolderId) -> Result<Vec<FolderId>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "WITH RECURSIVE tree(id) AS (
                     SELECT id FROM folders WHERE parent_id = ?1
                     UNION SELECT f.id FROM folders f JOIN tree t ON f.parent_id = t.id
                 )
                 SELECT id FROM tree",
            )?;
            collect_ids(&mut stmt, [id.as_str()])
        })
    }

    /// Renames and / or moves a folder in place: its name, parent and path,
    /// and the path prefix of every subfolder (`old path` → `new_path`).
    /// Ids, messages and sync state stay. Fails with [`Error::InvalidData`]
    /// when `new_path` is taken in the account or the parent is the folder
    /// or one of its subfolders.
    pub fn move_folder_tree(
        &self,
        id: &FolderId,
        parent: Option<&FolderId>,
        name: &str,
        new_path: &str,
    ) -> Result<()> {
        let descendants = self.folder_descendants(id)?;
        if parent.is_some_and(|p| p == id || descendants.contains(p)) {
            return Err(Error::InvalidData(format!(
                "folder {id} cannot move into itself"
            )));
        }
        self.with_tx(|tx| {
            let (account, old_path): (String, String) = tx
                .query_row(
                    "SELECT account_id, path FROM folders WHERE id = ?1",
                    [id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| Error::not_found("folder", id))?;
            let taken = tx
                .query_row(
                    "SELECT 1 FROM folders WHERE account_id = ?1 AND path = ?2 AND id <> ?3",
                    params![account, new_path, id.as_str()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if taken {
                return Err(Error::InvalidData(format!(
                    "a folder at {new_path:?} exists already"
                )));
            }
            tx.execute(
                "UPDATE folders SET parent_id = ?2, name = ?3, path = ?4 WHERE id = ?1",
                params![id.as_str(), parent.map(FolderId::as_str), name, new_path],
            )?;
            let prefix_len = i64::try_from(old_path.len()).unwrap_or(i64::MAX);
            for child in &descendants {
                tx.execute(
                    "UPDATE folders SET path = ?2 || substr(path, ?3 + 1)
                     WHERE id = ?1 AND substr(path, 1, ?3) = ?4",
                    params![child.as_str(), new_path, prefix_len, old_path],
                )?;
            }
            Ok(())
        })
    }
}
