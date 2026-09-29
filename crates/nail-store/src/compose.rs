//! Drafts and the outbox (runtime state of the composer, ADR 0012 and
//! ADR 0022).
//!
//! * Drafts are stored as their `nail-api` JSON ([`Draft`]) with ids
//!   `draft-<n>`. Since schema v4 the mode, the reply / forward context and
//!   the copy in the server's Drafts folder ([`DraftServerCopy`]) are also
//!   columns, and the draft's attachments are rows of `draft_attachments`
//!   so blob garbage collection keeps their blobs
//!   ([`Store::draft_blob_hashes`]).
//! * [`Store::schedule_send`] puts a draft into the outbox with a send time
//!   (undo-send delay or send later). Until the entry is claimed for sending
//!   it can be cancelled ([`Store::cancel_send`]), which turns it back into
//!   a plain draft. The core's sender claims due entries
//!   ([`Store::claim_due_sends`]) and reports the result
//!   ([`Store::finish_send`]); a sent entry deletes its draft.
//! * An entry left `sending` by a crash becomes `failed` on the next open
//!   (it may or may not have reached the server, so it is not resent
//!   automatically; the user can retry it).
//!
//! State machine: `scheduled ─claim─▶ sending ─▶ sent | failed | scheduled
//! (retry later)`; `scheduled | failed ─cancel─▶ cancelled`;
//! `failed ─retry─▶ scheduled`.

use nail_api::accounts::ComposerMode;
use nail_api::{
    AccountId, Address, Draft, DraftContext, DraftContextMode, DraftId, ErrorCode, FolderId,
    IdentityId, MessageId, OutboxEntry, OutboxId, OutboxState,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};

use crate::blob::BlobHash;
use crate::error::{Error, Result};
use crate::store::Store;

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

fn draft_id(n: i64) -> DraftId {
    DraftId(format!("draft-{n}"))
}

fn parse_draft_id(id: &DraftId) -> Result<i64> {
    id.as_str()
        .strip_prefix("draft-")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| Error::not_found("draft", id))
}

fn outbox_id(n: i64) -> OutboxId {
    OutboxId(format!("outbox-{n}"))
}

fn parse_outbox_id(id: &OutboxId) -> Result<i64> {
    id.as_str()
        .strip_prefix("outbox-")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| Error::not_found("outbox entry", id))
}

fn state_str(state: OutboxState) -> &'static str {
    match state {
        OutboxState::Scheduled => "scheduled",
        OutboxState::Sending => "sending",
        OutboxState::Sent => "sent",
        OutboxState::Failed => "failed",
        OutboxState::Cancelled => "cancelled",
    }
}

fn parse_state(s: &str) -> Option<OutboxState> {
    Some(match s {
        "scheduled" => OutboxState::Scheduled,
        "sending" => OutboxState::Sending,
        "sent" => OutboxState::Sent,
        "failed" => OutboxState::Failed,
        "cancelled" => OutboxState::Cancelled,
        _ => return None,
    })
}

fn code_to_sql(code: ErrorCode) -> String {
    code.as_str().to_owned()
}

fn code_from_sql(s: &str) -> Option<ErrorCode> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

/// How a send attempt ended ([`Store::finish_send`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// Accepted by the server; the draft is deleted.
    Sent,
    /// A temporary problem: send again at `retry_at` (Unix seconds).
    Retry {
        /// Error class.
        error: ErrorCode,
        /// Diagnostic detail.
        message: String,
        /// When to try again.
        retry_at: i64,
    },
    /// A permanent problem: stays `failed` until the user retries or
    /// cancels.
    Failed {
        /// Error class.
        error: ErrorCode,
        /// Diagnostic detail.
        message: String,
    },
}

const OUTBOX_COLS: &str = "id, draft_id, account_id, identity_id, subject, recipients_json,
    strftime('%Y-%m-%dT%H:%M:%SZ', send_at, 'unixepoch'), state, attempts, error, message";

fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<OutboxEntry> {
    let recipients: String = row.get(5)?;
    let state: String = row.get(7)?;
    let error: Option<String> = row.get(9)?;
    Ok(OutboxEntry {
        id: outbox_id(row.get(0)?),
        draft_id: draft_id(row.get(1)?),
        account_id: AccountId(row.get(2)?),
        identity_id: IdentityId(row.get(3)?),
        subject: row.get(4)?,
        recipients: serde_json::from_str(&recipients).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?,
        send_at: row.get(6)?,
        state: parse_state(&state).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                format!("unknown outbox state {state:?}").into(),
            )
        })?,
        attempts: row.get(8)?,
        error: error.as_deref().and_then(code_from_sql),
        message: row.get(10)?,
    })
}

fn entry_in_tx(tx: &Transaction<'_>, id: i64) -> Result<OutboxEntry> {
    tx.query_row(
        &format!("SELECT {OUTBOX_COLS} FROM outbox WHERE id = ?1"),
        [id],
        entry_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("outbox entry", outbox_id(id)))
}

/// Whether a draft is in the outbox and not finished (so it must not be
/// changed or deleted).
fn draft_is_queued(tx: &Transaction<'_>, draft: i64) -> Result<bool> {
    Ok(tx
        .query_row(
            "SELECT 1 FROM outbox WHERE draft_id = ?1 AND state IN ('scheduled', 'sending') LIMIT 1",
            [draft],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn mode_str(mode: ComposerMode) -> &'static str {
    match mode {
        ComposerMode::Plain => "plain",
        ComposerMode::Html => "html",
        ComposerMode::Markdown => "markdown",
    }
}

fn context_mode_str(mode: DraftContextMode) -> &'static str {
    match mode {
        DraftContextMode::Reply => "reply",
        DraftContextMode::ReplyAll => "reply-all",
        DraftContextMode::Forward => "forward",
    }
}

/// Writes the indexed columns and the attachment rows of draft `n`.
fn write_draft_details(tx: &Transaction<'_>, n: i64, draft: &Draft) -> Result<()> {
    tx.execute(
        "UPDATE drafts SET mode = ?2, context_mode = ?3, context_message_id = ?4 WHERE id = ?1",
        params![
            n,
            mode_str(draft.mode),
            draft.context.as_ref().map(|c| context_mode_str(c.mode)),
            draft.context.as_ref().map(|c| c.message_id.as_str()),
        ],
    )?;
    tx.execute("DELETE FROM draft_attachments WHERE draft_id = ?1", [n])?;
    let mut stmt = tx.prepare_cached(
        "INSERT INTO draft_attachments
         (draft_id, position, filename, mime, size, blob_hash, content_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for (position, a) in draft.attachments.iter().enumerate() {
        let hash = BlobHash::parse(&a.blob)?;
        stmt.execute(params![
            n,
            position as i64,
            a.filename,
            a.mime,
            i64::try_from(a.size).unwrap_or(i64::MAX),
            hash.as_str(),
            a.content_id,
        ])?;
    }
    Ok(())
}

/// Schema v4 backfill: M1a drafts stored the replied-to message as
/// `in_reply_to`; it becomes a reply [`DraftContext`] (payload and columns).
pub(crate) fn backfill_v4(tx: &Transaction<'_>) -> Result<()> {
    let rows: Vec<(i64, String)> = {
        let mut stmt = tx.prepare("SELECT id, payload FROM drafts")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (n, payload) in rows {
        let mut value: serde_json::Value = serde_json::from_str(&payload)?;
        let Some(object) = value.as_object_mut() else {
            continue;
        };
        let replied = object
            .remove("in_reply_to")
            .and_then(|v| v.as_str().map(str::to_owned));
        if let Some(message_id) = replied {
            let context = DraftContext {
                mode: DraftContextMode::Reply,
                message_id: MessageId(message_id),
            };
            object.insert("context".into(), serde_json::to_value(&context)?);
        }
        let draft: Draft = serde_json::from_value(value)?;
        let mut stored = draft.clone();
        stored.id = None;
        tx.execute(
            "UPDATE drafts SET payload = ?2 WHERE id = ?1",
            params![n, serde_json::to_string(&stored)?],
        )?;
        write_draft_details(tx, n, &draft)?;
    }
    Ok(())
}

/// Where a draft's copy in the server's Drafts folder is (autosave).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftServerCopy {
    /// The Drafts folder.
    pub folder_id: FolderId,
    /// UID of the copy; `None` while the upload is queued.
    pub uid: Option<u32>,
    /// `Message-ID` header of the copy (without angle brackets).
    pub message_id: String,
}

fn draft_in_tx(tx: &Transaction<'_>, n: i64) -> Result<Draft> {
    let payload: String = tx
        .query_row("SELECT payload FROM drafts WHERE id = ?1", [n], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| Error::not_found("draft", draft_id(n)))?;
    let mut draft: Draft = serde_json::from_str(&payload)?;
    draft.id = Some(draft_id(n));
    Ok(draft)
}

impl Store {
    /// Saves a draft: inserts it when `draft.id` is `None`, else replaces
    /// the stored one. Returns its id. Fails with [`Error::Conflict`] while
    /// the draft is scheduled or being sent, and with [`Error::NotFound`]
    /// for an unknown id.
    pub fn save_draft(&self, draft: &Draft) -> Result<DraftId> {
        let mut stored = draft.clone();
        stored.id = None;
        let payload = serde_json::to_string(&stored)?;
        self.with_tx(|tx| match &draft.id {
            None => {
                tx.execute(
                    "INSERT INTO drafts (identity_id, payload) VALUES (?1, ?2)",
                    params![draft.identity_id.as_str(), payload],
                )?;
                let n = tx.last_insert_rowid();
                write_draft_details(tx, n, draft)?;
                Ok(draft_id(n))
            }
            Some(id) => {
                let n = parse_draft_id(id)?;
                if draft_is_queued(tx, n)? {
                    return Err(Error::Conflict(format!("draft {id} is in the outbox")));
                }
                let changed = tx.execute(
                    &format!(
                        "UPDATE drafts SET identity_id = ?2, payload = ?3, updated_at = {NOW}
                         WHERE id = ?1"
                    ),
                    params![n, draft.identity_id.as_str(), payload],
                )?;
                if changed == 0 {
                    return Err(Error::not_found("draft", id));
                }
                write_draft_details(tx, n, draft)?;
                Ok(id.clone())
            }
        })
    }

    /// One draft (with its id set).
    pub fn get_draft(&self, id: &DraftId) -> Result<Draft> {
        let n = parse_draft_id(id)?;
        self.with_tx(|tx| draft_in_tx(tx, n))
    }

    /// All drafts, most recently changed first.
    pub fn list_drafts(&self) -> Result<Vec<Draft>> {
        self.with_tx(|tx| {
            let ids: Vec<i64> = {
                let mut stmt =
                    tx.prepare_cached("SELECT id FROM drafts ORDER BY updated_at DESC, id DESC")?;
                stmt.query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            ids.into_iter().map(|n| draft_in_tx(tx, n)).collect()
        })
    }

    /// Deletes a draft. Returns whether it existed. Fails with
    /// [`Error::Conflict`] while it is scheduled or being sent.
    pub fn delete_draft(&self, id: &DraftId) -> Result<bool> {
        let n = parse_draft_id(id)?;
        self.with_tx(|tx| {
            if draft_is_queued(tx, n)? {
                return Err(Error::Conflict(format!("draft {id} is in the outbox")));
            }
            tx.execute(
                "DELETE FROM outbox WHERE draft_id = ?1 AND state IN ('failed', 'cancelled')",
                [n],
            )?;
            Ok(tx.execute("DELETE FROM drafts WHERE id = ?1", [n])? > 0)
        })
    }

    /// Puts a draft into the outbox, to be sent through `account` at
    /// `send_at` (Unix seconds). Fails with [`Error::Conflict`] when the
    /// draft is already scheduled or being sent.
    pub fn schedule_send(
        &self,
        draft: &DraftId,
        account: &AccountId,
        send_at: i64,
    ) -> Result<OutboxEntry> {
        let n = parse_draft_id(draft)?;
        self.with_tx(|tx| {
            let d = draft_in_tx(tx, n)?;
            if draft_is_queued(tx, n)? {
                return Err(Error::Conflict(format!("draft {draft} is already in the outbox")));
            }
            // Earlier failed / cancelled attempts are superseded.
            tx.execute(
                "DELETE FROM outbox WHERE draft_id = ?1 AND state IN ('failed', 'cancelled')",
                [n],
            )?;
            let recipients: Vec<&Address> = d.to.iter().chain(&d.cc).chain(&d.bcc).collect();
            tx.execute(
                "INSERT INTO outbox (draft_id, account_id, identity_id, subject, recipients_json, send_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    n,
                    account.as_str(),
                    d.identity_id.as_str(),
                    d.subject,
                    serde_json::to_string(&recipients)?,
                    send_at
                ],
            )?;
            entry_in_tx(tx, tx.last_insert_rowid())
        })
    }

    /// Cancels a scheduled (or failed) send; the draft stays. Returns the
    /// draft's id. Fails with [`Error::Conflict`] once sending started or
    /// finished.
    pub fn cancel_send(&self, id: &OutboxId) -> Result<DraftId> {
        let n = parse_outbox_id(id)?;
        self.with_tx(|tx| {
            let entry = entry_in_tx(tx, n)?;
            if !matches!(entry.state, OutboxState::Scheduled | OutboxState::Failed) {
                return Err(Error::Conflict(format!(
                    "outbox entry {id} is {}",
                    state_str(entry.state)
                )));
            }
            tx.execute(
                &format!("UPDATE outbox SET state = 'cancelled', updated_at = {NOW} WHERE id = ?1"),
                [n],
            )?;
            Ok(entry.draft_id)
        })
    }

    /// Puts a failed entry back to `scheduled` at `send_at`. Fails with
    /// [`Error::Conflict`] unless it failed.
    pub fn retry_send(&self, id: &OutboxId, send_at: i64) -> Result<OutboxEntry> {
        let n = parse_outbox_id(id)?;
        self.with_tx(|tx| {
            let changed = tx.execute(
                &format!(
                    "UPDATE outbox SET state = 'scheduled', send_at = ?2, updated_at = {NOW}
                     WHERE id = ?1 AND state = 'failed'"
                ),
                params![n, send_at],
            )?;
            if changed == 0 {
                entry_in_tx(tx, n)?;
                return Err(Error::Conflict(format!("outbox entry {id} has not failed")));
            }
            entry_in_tx(tx, n)
        })
    }

    /// One outbox entry.
    pub fn get_outbox_entry(&self, id: &OutboxId) -> Result<OutboxEntry> {
        let n = parse_outbox_id(id)?;
        self.with_tx(|tx| entry_in_tx(tx, n))
    }

    /// The outbox: entries that are scheduled, sending or failed, by send
    /// time.
    pub fn list_outbox(&self) -> Result<Vec<OutboxEntry>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {OUTBOX_COLS} FROM outbox
                 WHERE state IN ('scheduled', 'sending', 'failed') ORDER BY send_at, id"
            ))?;
            let rows = stmt.query_map([], entry_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Marks every scheduled entry due at `now` (Unix seconds) as `sending`
    /// and returns them, oldest first. Each is counted as an attempt.
    pub fn claim_due_sends(&self, now: i64) -> Result<Vec<OutboxEntry>> {
        self.with_tx(|tx| {
            let ids: Vec<i64> = {
                let mut stmt = tx.prepare_cached(
                    "SELECT id FROM outbox WHERE state = 'scheduled' AND send_at <= ?1
                     ORDER BY send_at, id",
                )?;
                stmt.query_map([now], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                tx.execute(
                    &format!(
                        "UPDATE outbox SET state = 'sending', attempts = attempts + 1,
                         updated_at = {NOW} WHERE id = ?1"
                    ),
                    [id],
                )?;
                out.push(entry_in_tx(tx, id)?);
            }
            Ok(out)
        })
    }

    /// Records the result of sending a claimed entry. Returns the updated
    /// entry. Fails with [`Error::Conflict`] unless the entry is `sending`.
    pub fn finish_send(&self, id: &OutboxId, outcome: &SendOutcome) -> Result<OutboxEntry> {
        let n = parse_outbox_id(id)?;
        self.with_tx(|tx| {
            let entry = entry_in_tx(tx, n)?;
            if entry.state != OutboxState::Sending {
                return Err(Error::Conflict(format!(
                    "outbox entry {id} is {}",
                    state_str(entry.state)
                )));
            }
            match outcome {
                SendOutcome::Sent => {
                    tx.execute(
                        &format!(
                            "UPDATE outbox SET state = 'sent', error = NULL, message = NULL,
                             updated_at = {NOW} WHERE id = ?1"
                        ),
                        [n],
                    )?;
                    tx.execute(
                        "DELETE FROM drafts WHERE id = ?1",
                        [parse_draft_id(&entry.draft_id)?],
                    )?;
                }
                SendOutcome::Retry {
                    error,
                    message,
                    retry_at,
                } => {
                    tx.execute(
                        &format!(
                            "UPDATE outbox SET state = 'scheduled', send_at = ?2, error = ?3,
                             message = ?4, updated_at = {NOW} WHERE id = ?1"
                        ),
                        params![n, retry_at, code_to_sql(*error), message],
                    )?;
                }
                SendOutcome::Failed { error, message } => {
                    tx.execute(
                        &format!(
                            "UPDATE outbox SET state = 'failed', error = ?2, message = ?3,
                             updated_at = {NOW} WHERE id = ?1"
                        ),
                        params![n, code_to_sql(*error), message],
                    )?;
                }
            }
            entry_in_tx(tx, n)
        })
    }

    /// When the next scheduled entry is due (Unix seconds), if any.
    pub fn next_send_due(&self) -> Result<Option<i64>> {
        self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT MIN(send_at) FROM outbox WHERE state = 'scheduled'",
                [],
                |r| r.get(0),
            )?)
        })
    }

    /// Deletes sent and cancelled entries. Returns how many.
    pub fn purge_finished_sends(&self) -> Result<usize> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM outbox WHERE state IN ('sent', 'cancelled')",
                [],
            )?)
        })
    }

    /// Drafts replying to or forwarding `message`, most recent first (e.g.
    /// to reopen an unfinished reply).
    pub fn drafts_with_context(&self, message: &MessageId) -> Result<Vec<DraftId>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id FROM drafts WHERE context_message_id = ?1
                 ORDER BY updated_at DESC, id DESC",
            )?;
            let ids = stmt
                .query_map([message.as_str()], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(ids.into_iter().map(draft_id).collect())
        })
    }

    /// The distinct blobs referenced by draft attachments (garbage
    /// collection of uploads that no draft uses any more is the caller's
    /// job; [`Store::remove_unreferenced_blobs`] keeps these).
    pub fn draft_blob_hashes(&self) -> Result<Vec<BlobHash>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT DISTINCT blob_hash FROM draft_attachments ORDER BY blob_hash",
            )?;
            let hashes = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            hashes.iter().map(|h| BlobHash::parse(h)).collect()
        })
    }

    /// Whether the draft waits in the outbox or is being sent (`false` for
    /// an unknown draft).
    pub fn is_draft_queued(&self, id: &DraftId) -> Result<bool> {
        let Ok(n) = parse_draft_id(id) else {
            return Ok(false);
        };
        self.with_tx(|tx| draft_is_queued(tx, n))
    }

    /// Records (or with `None` clears) where the draft's server copy is.
    pub fn set_draft_server_copy(
        &self,
        id: &DraftId,
        copy: Option<&DraftServerCopy>,
    ) -> Result<()> {
        let n = parse_draft_id(id)?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE drafts SET server_folder_id = ?2, server_uid = ?3, server_message_id = ?4
                 WHERE id = ?1",
                params![
                    n,
                    copy.map(|c| c.folder_id.as_str()),
                    copy.and_then(|c| c.uid),
                    copy.map(|c| c.message_id.as_str()),
                ],
            )?;
            if changed == 0 {
                return Err(Error::not_found("draft", id));
            }
            Ok(())
        })
    }

    /// Where the draft's server copy is, `None` when it has none.
    pub fn draft_server_copy(&self, id: &DraftId) -> Result<Option<DraftServerCopy>> {
        let n = parse_draft_id(id)?;
        self.with_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT server_folder_id, server_uid, server_message_id FROM drafts
                     WHERE id = ?1",
                    [n],
                    |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<u32>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| Error::not_found("draft", id))?;
            Ok(match row {
                (Some(folder), uid, Some(message_id)) => Some(DraftServerCopy {
                    folder_id: FolderId(folder),
                    uid,
                    message_id,
                }),
                _ => None,
            })
        })
    }

    /// Crash recovery: entries left `sending` become `failed` (see the
    /// module docs). Runs on every [`Store::open`].
    pub(crate) fn recover_sending(&self) -> Result<usize> {
        let n = self.with_conn(|conn| {
            Ok(conn.execute(
                &format!(
                    "UPDATE outbox SET state = 'failed', error = ?1,
                     message = 'interrupted while sending; it may have been sent',
                     updated_at = {NOW} WHERE state = 'sending'"
                ),
                [code_to_sql(ErrorCode::Unavailable)],
            )?)
        })?;
        if n > 0 {
            tracing::warn!(count = n, "outbox entries interrupted while sending");
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use nail_api::accounts::ComposerMode;
    use nail_api::{Account, AccountKind};

    use super::*;

    fn store() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_account(&Account {
                id: "a1".into(),
                name: "A".into(),
                email: "a@example.org".into(),
                kind: AccountKind::Imap,
            })
            .unwrap();
        store
    }

    fn draft(subject: &str) -> Draft {
        Draft {
            id: None,
            identity_id: "ada@example.org".into(),
            to: vec![Address {
                name: None,
                email: "bob@example.org".into(),
            }],
            cc: vec![],
            bcc: vec![Address {
                name: None,
                email: "carol@example.org".into(),
            }],
            subject: subject.into(),
            mode: ComposerMode::Plain,
            text: "Hello".into(),
            html: None,
            markdown: None,
            attachments: vec![],
            context: None,
            signature: nail_api::SignatureChoice::Identity,
            markdown_part: false,
            automatic: None,
        }
    }

    #[test]
    fn drafts_crud() {
        let store = store();
        let id = store.save_draft(&draft("One")).unwrap();
        assert_eq!(id.as_str(), "draft-1");
        let mut d = store.get_draft(&id).unwrap();
        assert_eq!(d.id.as_ref(), Some(&id));
        assert_eq!(d.subject, "One");
        d.subject = "Two".into();
        assert_eq!(store.save_draft(&d).unwrap(), id);
        assert_eq!(store.get_draft(&id).unwrap().subject, "Two");
        let other = store.save_draft(&draft("Other")).unwrap();
        assert_eq!(store.list_drafts().unwrap().len(), 2);
        assert!(store.delete_draft(&other).unwrap());
        assert!(!store.delete_draft(&other).unwrap());
        assert!(store.get_draft(&other).unwrap_err().is_not_found());
        assert!(store.get_draft(&"bogus".into()).unwrap_err().is_not_found());
        let missing = Draft {
            id: Some("draft-99".into()),
            ..draft("x")
        };
        assert!(store.save_draft(&missing).unwrap_err().is_not_found());
    }

    #[test]
    fn queued_drafts_are_reported_and_automatic_mail_is_kept() {
        let store = store();
        let rule_mail = Draft {
            automatic: Some(nail_api::AutomaticMail::Redirect {
                message_id: "m1".into(),
            }),
            ..draft("Redirect")
        };
        let id = store.save_draft(&rule_mail).unwrap();
        assert_eq!(store.get_draft(&id).unwrap().automatic, rule_mail.automatic);
        assert!(!store.is_draft_queued(&id).unwrap());
        assert!(!store.is_draft_queued(&"bogus".into()).unwrap());
        let entry = store.schedule_send(&id, &"a1".into(), 100).unwrap();
        assert!(store.is_draft_queued(&id).unwrap());
        store.cancel_send(&entry.id).unwrap();
        assert!(!store.is_draft_queued(&id).unwrap());
    }

    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn attachment(blob: &str, cid: Option<&str>) -> nail_api::DraftAttachment {
        nail_api::DraftAttachment {
            filename: "f.png".into(),
            mime: "image/png".into(),
            size: 10,
            blob: blob.into(),
            content_id: cid.map(str::to_owned),
        }
    }

    #[test]
    fn rich_drafts_keep_columns_and_attachments() {
        let store = store();
        let rich = Draft {
            mode: ComposerMode::Html,
            html: Some("<p>Hi <img src=\"cid:logo\"></p>".into()),
            attachments: vec![attachment(HASH_A, Some("logo")), attachment(HASH_B, None)],
            context: Some(DraftContext {
                mode: DraftContextMode::Forward,
                message_id: "m1".into(),
            }),
            ..draft("Rich")
        };
        let id = store.save_draft(&rich).unwrap();
        let back = store.get_draft(&id).unwrap();
        assert_eq!(back.attachments, rich.attachments);
        assert_eq!(back.context, rich.context);
        assert_eq!(back.mode, ComposerMode::Html);
        assert_eq!(
            store.drafts_with_context(&"m1".into()).unwrap(),
            std::slice::from_ref(&id)
        );
        let hashes: Vec<String> = store
            .draft_blob_hashes()
            .unwrap()
            .iter()
            .map(|h| h.as_str().to_owned())
            .collect();
        assert_eq!(hashes, [HASH_A, HASH_B]);
        let (mode, context): (String, Option<String>) = store
            .with_conn(|c| {
                Ok(c.query_row(
                    "SELECT mode, context_mode FROM drafts WHERE id = 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(
            (mode.as_str(), context.as_deref()),
            ("html", Some("forward"))
        );

        // Replacing the attachments replaces the rows; deleting the draft
        // drops them.
        let fewer = Draft {
            attachments: vec![attachment(HASH_B, None)],
            ..back
        };
        store.save_draft(&fewer).unwrap();
        assert_eq!(store.draft_blob_hashes().unwrap().len(), 1);
        assert!(store.delete_draft(&id).unwrap());
        assert!(store.draft_blob_hashes().unwrap().is_empty());

        // A bad blob reference is refused and nothing is saved.
        let bad = Draft {
            attachments: vec![attachment("nope", None)],
            ..draft("Bad")
        };
        assert!(matches!(store.save_draft(&bad), Err(Error::InvalidHash(_))));
        assert!(store.list_drafts().unwrap().is_empty());
    }

    #[test]
    fn blob_gc_keeps_draft_attachments() {
        let store = store();
        let dir = tempfile::tempdir().unwrap();
        let blobs = crate::BlobStore::open(dir.path()).unwrap();
        let kept = blobs.put(b"attachment").unwrap();
        let gone = blobs.put(b"orphan").unwrap();
        let d = Draft {
            attachments: vec![attachment(kept.as_str(), None)],
            ..draft("With file")
        };
        store.save_draft(&d).unwrap();
        assert!(store.blob_is_referenced(&kept).unwrap());
        assert!(!store.blob_is_referenced(&gone).unwrap());
        let removed = store
            .remove_unreferenced_blobs(&blobs, &[kept.clone(), gone.clone()])
            .unwrap();
        assert_eq!(removed, 1);
        assert!(blobs.contains(&kept) && !blobs.contains(&gone));
    }

    #[test]
    fn server_copy_is_recorded() {
        let store = store();
        let id = store.save_draft(&draft("Hi")).unwrap();
        assert_eq!(store.draft_server_copy(&id).unwrap(), None);
        let copy = DraftServerCopy {
            folder_id: "drafts".into(),
            uid: Some(7),
            message_id: "d1@nail".into(),
        };
        store.set_draft_server_copy(&id, Some(&copy)).unwrap();
        assert_eq!(store.draft_server_copy(&id).unwrap(), Some(copy));
        // Saving the draft again keeps the copy until the core replaces it.
        store.save_draft(&store.get_draft(&id).unwrap()).unwrap();
        assert!(store.draft_server_copy(&id).unwrap().is_some());
        store.set_draft_server_copy(&id, None).unwrap();
        assert_eq!(store.draft_server_copy(&id).unwrap(), None);
        assert!(
            store
                .set_draft_server_copy(&"draft-9".into(), None)
                .unwrap_err()
                .is_not_found()
        );
    }

    #[test]
    fn schedule_cancel_and_send() {
        let store = store();
        let id = store.save_draft(&draft("Hi")).unwrap();
        let entry = store.schedule_send(&id, &"a1".into(), 1_000).unwrap();
        assert_eq!(entry.state, OutboxState::Scheduled);
        assert_eq!(entry.send_at, "1970-01-01T00:16:40Z");
        assert_eq!(entry.recipients.len(), 2);
        assert_eq!(entry.subject, "Hi");
        assert_eq!(store.next_send_due().unwrap(), Some(1_000));

        // While scheduled the draft is locked.
        assert!(matches!(
            store.schedule_send(&id, &"a1".into(), 5),
            Err(Error::Conflict(_))
        ));
        assert!(matches!(store.delete_draft(&id), Err(Error::Conflict(_))));
        assert!(matches!(
            store.save_draft(&store.get_draft(&id).unwrap()),
            Err(Error::Conflict(_))
        ));

        // Undo send.
        assert_eq!(store.cancel_send(&entry.id).unwrap(), id);
        assert!(store.list_outbox().unwrap().is_empty());
        assert!(matches!(
            store.cancel_send(&entry.id),
            Err(Error::Conflict(_))
        ));

        // Send for real.
        let entry = store.schedule_send(&id, &"a1".into(), 2_000).unwrap();
        assert!(store.claim_due_sends(1_999).unwrap().is_empty());
        let claimed = store.claim_due_sends(2_000).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(
            (claimed[0].state, claimed[0].attempts),
            (OutboxState::Sending, 1)
        );
        assert!(matches!(
            store.cancel_send(&entry.id),
            Err(Error::Conflict(_))
        ));
        let done = store.finish_send(&entry.id, &SendOutcome::Sent).unwrap();
        assert_eq!(done.state, OutboxState::Sent);
        assert!(store.get_draft(&id).unwrap_err().is_not_found());
        assert!(matches!(
            store.finish_send(&entry.id, &SendOutcome::Sent),
            Err(Error::Conflict(_))
        ));
        assert_eq!(store.purge_finished_sends().unwrap(), 1);
    }

    #[test]
    fn retry_and_failure() {
        let store = store();
        let id = store.save_draft(&draft("Hi")).unwrap();
        let entry = store.schedule_send(&id, &"a1".into(), 10).unwrap();
        store.claim_due_sends(10).unwrap();
        let e = store
            .finish_send(
                &entry.id,
                &SendOutcome::Retry {
                    error: ErrorCode::Network,
                    message: "offline".into(),
                    retry_at: 70,
                },
            )
            .unwrap();
        assert_eq!(e.state, OutboxState::Scheduled);
        assert_eq!(e.error, Some(ErrorCode::Network));
        assert_eq!(store.next_send_due().unwrap(), Some(70));
        store.claim_due_sends(70).unwrap();
        let e = store
            .finish_send(
                &entry.id,
                &SendOutcome::Failed {
                    error: ErrorCode::Rejected,
                    message: "550 no such user".into(),
                },
            )
            .unwrap();
        assert_eq!((e.state, e.attempts), (OutboxState::Failed, 2));
        assert_eq!(store.list_outbox().unwrap().len(), 1);
        let e = store.retry_send(&entry.id, 100).unwrap();
        assert_eq!(e.state, OutboxState::Scheduled);
        assert!(matches!(
            store.retry_send(&entry.id, 100),
            Err(Error::Conflict(_))
        ));
    }

    #[test]
    fn interrupted_sends_fail_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite3");
        let entry = {
            let store = Store::open(&path).unwrap();
            store
                .upsert_account(&Account {
                    id: "a1".into(),
                    name: "A".into(),
                    email: "a@example.org".into(),
                    kind: AccountKind::Imap,
                })
                .unwrap();
            let id = store.save_draft(&draft("Hi")).unwrap();
            let entry = store.schedule_send(&id, &"a1".into(), 0).unwrap();
            store.claim_due_sends(0).unwrap();
            entry
        };
        let store = Store::open(&path).unwrap();
        let e = store.get_outbox_entry(&entry.id).unwrap();
        assert_eq!(e.state, OutboxState::Failed);
        assert_eq!(e.error, Some(ErrorCode::Unavailable));
    }
}
