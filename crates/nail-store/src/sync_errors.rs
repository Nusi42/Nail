//! Sync errors and skipped messages (schema v19, ADRs 0190 / 0191).
//!
//! `sync_errors` keeps the last error of an account (`folder_id = ''`) and
//! of each of its folders until a later sync of it succeeds, so "Show
//! details" survives a restart. `sync_skipped` lists the messages a folder
//! sync could not download (refused, withheld, unreadable, too large): the
//! folder's checkpoint moves past them and later syncs retry them.
//! `sync_refetch` (schema v23) lists server messages a sync left out on
//! purpose or a dismissed write gave back: they are fetched again once
//! nothing holds them back.

use nail_api::connlog::{ErrorDetails, SkippedMessage, SyncErrorRecord};
use nail_api::{AccountId, FolderId};
use rusqlite::params;

use crate::error::Result;
use crate::store::Store;
use crate::util::collect_rows;

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

/// Schema v23: server messages to fetch again. A sync leaves a message out
/// while a move or delete of its `Message-ID` out of the folder waits or
/// failed (the write takes it away; stored, it would show twice); a
/// dismissed failed write whose local row is gone lists the message's
/// source UID. `message_id` (normalized, may be NULL) names the write that
/// holds the message back.
pub(crate) const V23: &str = r"
CREATE TABLE sync_refetch (
    folder_id  TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid        INTEGER NOT NULL,
    message_id TEXT,
    PRIMARY KEY (folder_id, uid)
);
";

/// Lists message `uid` of `folder` to fetch again (see [`V23`]), inside a
/// transaction or on the store's connection.
pub(crate) fn record_refetch_in(
    conn: &rusqlite::Connection,
    folder: &FolderId,
    uid: u32,
    message_id: Option<&str>,
) -> Result<()> {
    let mid = message_id.and_then(crate::mail::normalize_msgid);
    conn.prepare_cached(
        "INSERT INTO sync_refetch (folder_id, uid, message_id) VALUES (?1, ?2, ?3)
         ON CONFLICT (folder_id, uid) DO UPDATE SET message_id = excluded.message_id",
    )?
    .execute(params![folder.as_str(), uid, mid])?;
    Ok(())
}

/// Deletes the rows of `uids` in `folder` from a `(folder_id, uid)` table;
/// returns how many there were.
fn delete_uids(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    folder: &FolderId,
    uids: &[u32],
) -> Result<usize> {
    let mut stmt = tx.prepare_cached(&format!(
        "DELETE FROM {table} WHERE folder_id = ?1 AND uid = ?2"
    ))?;
    let mut removed = 0;
    for uid in uids {
        removed += stmt.execute(params![folder.as_str(), uid])?;
    }
    Ok(removed)
}

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
            let rows: Vec<(String, String, String)> =
                collect_rows(&mut stmt, [account.map(AccountId::as_str)])?;
            rows.into_iter()
                .map(|(account_id, folder_id, details)| {
                    Ok(SyncErrorRecord {
                        account_id: AccountId(account_id),
                        folder_id: (!folder_id.is_empty()).then_some(FolderId(folder_id)),
                        details: serde_json::from_str(&details)?,
                    })
                })
                .collect()
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
        self.with_tx(|tx| delete_uids(tx, "sync_skipped", folder, uids))
    }

    /// Forgets every skipped message and every message to fetch again of a
    /// folder (its UIDs became invalid).
    pub fn clear_skipped(&self, folder: &FolderId) -> Result<()> {
        self.with_tx(|tx| {
            for table in ["sync_skipped", "sync_refetch"] {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE folder_id = ?1"),
                    [folder.as_str()],
                )?;
            }
            Ok(())
        })
    }

    /// Lists message `uid` of `folder` to fetch again once no move or
    /// delete of `message_id` out of `folder` holds it back (schema v23).
    pub fn record_refetch(
        &self,
        folder: &FolderId,
        uid: u32,
        message_id: Option<&str>,
    ) -> Result<()> {
        self.with_conn(|conn| record_refetch_in(conn, folder, uid, message_id))
    }

    /// Up to `limit` UIDs of `folder` to fetch again now: listed by
    /// [`Store::record_refetch`] and no longer held back by a write
    /// ([`Store::held_move_outs`]), lowest first.
    pub fn refetch_uids(&self, folder: &FolderId, limit: usize) -> Result<Vec<u32>> {
        let held = self.held_move_outs(folder)?;
        let rows: Vec<(u32, Option<String>)> = self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT uid, message_id FROM sync_refetch WHERE folder_id = ?1 ORDER BY uid",
            )?;
            collect_rows(&mut stmt, [folder.as_str()])
        })?;
        Ok(rows
            .into_iter()
            .filter(|(_, mid)| mid.as_ref().is_none_or(|m| !held.contains(m)))
            .map(|(uid, _)| uid)
            .take(limit)
            .collect())
    }

    /// Forgets messages to fetch again (fetched, or gone); returns how many
    /// were listed.
    pub fn remove_refetch(&self, folder: &FolderId, uids: &[u32]) -> Result<usize> {
        self.with_tx(|tx| delete_uids(tx, "sync_refetch", folder, uids))
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
            let rows: Vec<(String, String, u32, u32, String)> =
                collect_rows(&mut stmt, [account.map(AccountId::as_str)])?;
            rows.into_iter()
                .map(|(account_id, folder_id, uid, attempts, details)| {
                    Ok(SkippedMessage {
                        account_id: AccountId(account_id),
                        folder_id: FolderId(folder_id),
                        uid,
                        attempts,
                        details: serde_json::from_str(&details)?,
                    })
                })
                .collect()
        })
    }
}
