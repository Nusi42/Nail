//! Sync errors and skipped messages (schema v19, ADRs 0190 / 0191).
//!
//! `sync_errors` keeps the last error of an account (`folder_id = ''`) and
//! of each of its folders until a later sync of it succeeds, so "Show
//! details" survives a restart. `sync_skipped` lists the messages a folder
//! sync could not download (refused, withheld, unreadable, too large): the
//! folder's checkpoint moves past them and later syncs retry them.

use nail_api::connlog::{ErrorDetails, SkippedMessage, SyncErrorRecord};
use nail_api::{AccountId, FolderId};
use rusqlite::params;

use crate::error::Result;
use crate::store::Store;

/// Schema v19.
pub(crate) const V19: &str = r"
CREATE TABLE sync_errors (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- '' = the account as a whole.
    folder_id  TEXT NOT NULL DEFAULT '',
    -- nail_api::connlog::ErrorDetails as JSON (redacted).
    details    TEXT NOT NULL,
    PRIMARY KEY (account_id, folder_id)
);
CREATE TRIGGER sync_errors_folder_gone AFTER DELETE ON folders
BEGIN
    DELETE FROM sync_errors WHERE folder_id = OLD.id;
END;

CREATE TABLE sync_skipped (
    folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid       INTEGER NOT NULL,
    attempts  INTEGER NOT NULL DEFAULT 1,
    -- nail_api::connlog::ErrorDetails as JSON (redacted).
    details   TEXT NOT NULL,
    PRIMARY KEY (folder_id, uid)
);
";

fn folder_key(folder: Option<&FolderId>) -> &str {
    folder.map_or("", FolderId::as_str)
}

impl Store {
    /// Records the last error of an account (`folder` `None`) or folder.
    pub fn set_sync_error(
        &self,
        account: &AccountId,
        folder: Option<&FolderId>,
        details: &ErrorDetails,
    ) -> Result<()> {
        let json = serde_json::to_string(details)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO sync_errors (account_id, folder_id, details) VALUES (?1, ?2, ?3)
                 ON CONFLICT (account_id, folder_id) DO UPDATE SET details = excluded.details",
                params![account.as_str(), folder_key(folder), json],
            )?;
            Ok(())
        })
    }

    /// Forgets the error of an account (`folder` `None`) or folder;
    /// returns whether there was one.
    pub fn clear_sync_error(&self, account: &AccountId, folder: Option<&FolderId>) -> Result<bool> {
        self.with_conn(|conn| {
            let removed = conn.execute(
                "DELETE FROM sync_errors WHERE account_id = ?1 AND folder_id = ?2",
                params![account.as_str(), folder_key(folder)],
            )?;
            Ok(removed > 0)
        })
    }

    /// The stored errors of one account or of every account, account-level
    /// first, then by folder.
    pub fn sync_errors(&self, account: Option<&AccountId>) -> Result<Vec<SyncErrorRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT account_id, folder_id, details FROM sync_errors
                 WHERE ?1 IS NULL OR account_id = ?1 ORDER BY account_id, folder_id",
            )?;
            let rows = stmt.query_map([account.map(AccountId::as_str)], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (account_id, folder_id, details) = row?;
                out.push(SyncErrorRecord {
                    account_id: AccountId(account_id),
                    folder_id: (!folder_id.is_empty()).then_some(FolderId(folder_id)),
                    details: serde_json::from_str(&details)?,
                });
            }
            Ok(out)
        })
    }

    /// Records that a folder sync skipped message `uid` (again).
    pub fn record_skipped(
        &self,
        folder: &FolderId,
        uid: u32,
        details: &ErrorDetails,
    ) -> Result<()> {
        let json = serde_json::to_string(details)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO sync_skipped (folder_id, uid, details) VALUES (?1, ?2, ?3)
                 ON CONFLICT (folder_id, uid) DO UPDATE
                 SET attempts = attempts + 1, details = excluded.details",
                params![folder.as_str(), uid, json],
            )?;
            Ok(())
        })
    }

    /// Forgets skipped messages (fetched after all, or gone); returns how
    /// many were listed.
    pub fn remove_skipped(&self, folder: &FolderId, uids: &[u32]) -> Result<usize> {
        self.with_tx(|tx| {
            let mut stmt =
                tx.prepare_cached("DELETE FROM sync_skipped WHERE folder_id = ?1 AND uid = ?2")?;
            let mut removed = 0;
            for uid in uids {
                removed += stmt.execute(params![folder.as_str(), uid])?;
            }
            Ok(removed)
        })
    }

    /// Forgets every skipped message of a folder (its UIDs became invalid).
    pub fn clear_skipped(&self, folder: &FolderId) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM sync_skipped WHERE folder_id = ?1",
                [folder.as_str()],
            )?;
            Ok(())
        })
    }

    /// Up to `limit` skipped UIDs of a folder to retry, least tried first.
    pub fn skipped_uids(&self, folder: &FolderId, limit: usize) -> Result<Vec<u32>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT uid FROM sync_skipped WHERE folder_id = ?1
                 ORDER BY attempts, uid LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![folder.as_str(), limit], |row| row.get(0))?;
            let mut uids: Vec<u32> = rows.collect::<rusqlite::Result<_>>()?;
            uids.sort_unstable();
            Ok(uids)
        })
    }

    /// The skipped messages of one account or of every account.
    pub fn skipped_messages(&self, account: Option<&AccountId>) -> Result<Vec<SkippedMessage>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT f.account_id, s.folder_id, s.uid, s.attempts, s.details
                 FROM sync_skipped s JOIN folders f ON f.id = s.folder_id
                 WHERE ?1 IS NULL OR f.account_id = ?1
                 ORDER BY f.account_id, s.folder_id, s.uid",
            )?;
            let rows = stmt.query_map([account.map(AccountId::as_str)], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (account_id, folder_id, uid, attempts, details) = row?;
                out.push(SkippedMessage {
                    account_id: AccountId(account_id),
                    folder_id: FolderId(folder_id),
                    uid,
                    attempts,
                    details: serde_json::from_str(&details)?,
                });
            }
            Ok(out)
        })
    }
}
