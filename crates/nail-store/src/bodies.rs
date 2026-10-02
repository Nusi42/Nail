//! Body-less messages and on-demand bodies (M2c WP53; sync policies of
//! ADR 0043).
//!
//! A folder synced "headers only" (or "recent N days" for older mail)
//! stores its messages with only the header block as their blob and
//! `body_state = 'headers-only'`; the summaries say so
//! (`MessageSummary::headers_only`). Opening one fetches the body from the
//! server and attaches it ([`Store::attach_body`], state `on-demand`); a
//! policy that keeps fewer bodies later detaches them again
//! ([`Store::detach_body`]).
//!
//! These are sync-side writes, like the rest of the message CRUD: the
//! server already has the mail, nothing is replayed.

use nail_api::sync::BodyState;
use nail_api::{AccountId, FolderId, MessageId};
use rusqlite::{OptionalExtension, params};

use crate::blob::BlobHash;
use crate::error::{Error, Result};
use crate::mail::{MergeOutcome, MergeReport, SyncedMessage, upsert_message_in_tx};
use crate::store::Store;

/// What the core needs to fetch one message's body from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyFetchTarget {
    /// The message.
    pub message_id: MessageId,
    /// Its account.
    pub account_id: AccountId,
    /// Its folder.
    pub folder_id: FolderId,
    /// The folder's server path.
    pub folder_path: String,
    /// Its UID in the folder; `None` for a message without one (local
    /// only, or moved and not resynced yet), which cannot be fetched.
    pub uid: Option<u32>,
    /// Where the body is now.
    pub state: BodyState,
}

/// A message whose body is stored, for trimming bodies outside a policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBody {
    /// The message.
    pub message_id: MessageId,
    /// Its blob (the full raw message).
    pub blob_hash: BlobHash,
    /// Where the body is (`synced` or `on-demand`).
    pub state: BodyState,
}

fn parse_state(text: &str) -> Result<BodyState> {
    BodyState::parse(text).ok_or_else(|| Error::InvalidData(format!("unknown body state {text:?}")))
}

impl Store {
    /// The sync-side merge ([`Store::merge_messages`]) for messages fetched
    /// with `state`:
    ///
    /// * [`BodyState::Synced`]: full messages; every row becomes `synced`
    ///   (also one that had only its headers before: the policy now keeps
    ///   its body).
    /// * [`BodyState::HeadersOnly`]: the blob is the header block. New rows
    ///   and rows without a body become `headers-only`; a row that already
    ///   has its body (synced before, or fetched on demand) **keeps its
    ///   blob and state** — a resync never throws a downloaded body away.
    /// * [`BodyState::OnDemand`] is refused: bodies fetched on demand are
    ///   attached with [`Store::attach_body`].
    pub fn merge_with_body_state(
        &self,
        messages: &[SyncedMessage],
        state: BodyState,
    ) -> Result<MergeReport> {
        if state == BodyState::OnDemand {
            return Err(Error::InvalidData(
                "bodies fetched on demand are attached with attach_body".to_owned(),
            ));
        }
        self.with_tx(|tx| {
            let mut report = MergeReport::default();
            for m in messages {
                let mut message = m.message.clone();
                let existing: Option<(String, Option<String>, Option<i64>)> = tx
                    .prepare_cached(
                        "SELECT body_state, blob_hash, size FROM messages WHERE id = ?1",
                    )?
                    .query_row([message.summary.id.as_str()], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })
                    .optional()?;
                let keep_body = state == BodyState::HeadersOnly
                    && existing
                        .as_ref()
                        .is_some_and(|(s, _, _)| s != BodyState::HeadersOnly.as_str());
                let mut meta = m.meta.clone();
                if keep_body && let Some((_, hash, size)) = &existing {
                    if let Some(hash) = hash {
                        message.blob_hash = Some(BlobHash::parse(hash)?);
                    }
                    // The size stays the full message's, not the headers'.
                    if let Some(meta) = &mut meta {
                        meta.size = size.map(i64::cast_unsigned);
                    }
                }
                match upsert_message_in_tx(tx, &message, meta.as_ref())? {
                    MergeOutcome::Written => report.written += 1,
                    MergeOutcome::Adjusted => {
                        report.written += 1;
                        report.kept_local.push(message.summary.id.clone());
                    }
                    MergeOutcome::Skipped => {
                        report.skipped.push(message.summary.id.clone());
                        continue;
                    }
                }
                if !keep_body {
                    tx.prepare_cached("UPDATE messages SET body_state = ?2 WHERE id = ?1")?
                        .execute(params![message.summary.id.as_str(), state.as_str()])?;
                }
            }
            Ok(report)
        })
    }

    /// Attaches a body fetched on demand: the message's blob becomes the
    /// full raw message `blob` (already written to the blob store) of
    /// `size` bytes. A `headers-only` message becomes `on-demand`; a
    /// message that had its body keeps its state. Returns the blob it had
    /// before (the header block), for [`Store::remove_unreferenced_blobs`];
    /// `None` when there was none or it is unchanged.
    pub fn attach_body(
        &self,
        id: &MessageId,
        blob: &BlobHash,
        size: u64,
    ) -> Result<Option<BlobHash>> {
        let size = i64::try_from(size)
            .map_err(|_| Error::InvalidData(format!("size {size} too large")))?;
        self.with_tx(|tx| {
            let (state, old): (String, Option<String>) = tx
                .query_row(
                    "SELECT body_state, blob_hash FROM messages WHERE id = ?1",
                    [id.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| Error::not_found("message", id))?;
            let state = match parse_state(&state)? {
                BodyState::HeadersOnly => BodyState::OnDemand,
                other => other,
            };
            tx.execute(
                "UPDATE messages SET blob_hash = ?2, size = ?3, body_state = ?4 WHERE id = ?1",
                params![id.as_str(), blob.as_str(), size, state.as_str()],
            )?;
            old.map(|h| BlobHash::parse(&h))
                .transpose()
                .map(|old| old.filter(|h| h != blob))
        })
    }

    /// Drops a stored body again (a policy that keeps fewer bodies): the
    /// blob becomes `headers` (the header block, already in the blob store)
    /// and the state `headers-only`. Returns the full message's blob for
    /// [`Store::remove_unreferenced_blobs`]; `None` when the message had no
    /// body to drop.
    pub fn detach_body(&self, id: &MessageId, headers: &BlobHash) -> Result<Option<BlobHash>> {
        self.with_tx(|tx| {
            let (state, old): (String, Option<String>) = tx
                .query_row(
                    "SELECT body_state, blob_hash FROM messages WHERE id = ?1",
                    [id.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| Error::not_found("message", id))?;
            if parse_state(&state)? == BodyState::HeadersOnly {
                return Ok(None);
            }
            tx.execute(
                "UPDATE messages SET blob_hash = ?2, body_state = 'headers-only' WHERE id = ?1",
                params![id.as_str(), headers.as_str()],
            )?;
            old.map(|h| BlobHash::parse(&h))
                .transpose()
                .map(|old| old.filter(|h| h != headers))
        })
    }

    /// Where to fetch the bodies of `ids` from: the messages among them
    /// whose body is not stored (`headers-only`), with folder path and UID,
    /// in the order given. Unknown ids and messages with a body are left
    /// out.
    pub fn body_fetch_targets(&self, ids: &[MessageId]) -> Result<Vec<BodyFetchTarget>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT m.account_id, m.folder_id, f.path, m.uid, m.body_state
                 FROM messages m JOIN folders f ON f.id = m.folder_id
                 WHERE m.id = ?1 AND m.body_state = 'headers-only'",
            )?;
            let mut out = Vec::new();
            for id in ids {
                let row = stmt
                    .query_row([id.as_str()], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, Option<u32>>(3)?,
                            r.get::<_, String>(4)?,
                        ))
                    })
                    .optional()?;
                if let Some((account, folder, path, uid, state)) = row {
                    out.push(BodyFetchTarget {
                        message_id: id.clone(),
                        account_id: AccountId(account),
                        folder_id: FolderId(folder),
                        folder_path: path,
                        uid,
                        state: parse_state(&state)?,
                    });
                }
            }
            Ok(out)
        })
    }

    /// Messages of a folder dated before `cutoff` (Unix seconds) whose body
    /// is stored (`synced` or `on-demand`), oldest first: what a "recent N
    /// days" policy no longer keeps once they age out.
    pub fn bodies_before(
        &self,
        folder: &FolderId,
        cutoff: i64,
        limit: u32,
    ) -> Result<Vec<StoredBody>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, blob_hash, body_state FROM messages
                 WHERE folder_id = ?1 AND body_state != 'headers-only'
                   AND blob_hash IS NOT NULL AND date_sort < ?2
                 ORDER BY date_sort, id LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![folder.as_str(), cutoff, limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, hash, state) = row?;
                out.push(StoredBody {
                    message_id: MessageId(id),
                    blob_hash: BlobHash::parse(&hash)?,
                    state: parse_state(&state)?,
                });
            }
            Ok(out)
        })
    }

    /// How many messages of a folder have no stored body.
    pub fn headers_only_count(&self, folder: &FolderId) -> Result<u32> {
        self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE folder_id = ?1 AND body_state = 'headers-only'",
                [folder.as_str()],
                |r| r.get(0),
            )?)
        })
    }
}
