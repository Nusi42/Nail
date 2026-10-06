//! Gmail accounts in the store (schema v11, M3b, ADR 0065).
//!
//! * Remote ids: every message of an API account has the server's id in
//!   `messages.remote_id` ([`Store::set_message_remote_id`],
//!   [`Store::message_by_remote_id`]); it survives local moves.
//! * Sync state per account ([`GmailSyncRecord`]): the last applied
//!   `historyId`, an interrupted full sync.
//! * Labels ([`GmailLabelRecord`]): id ↔ name ↔ Nail tag (user labels) /
//!   folder (system labels shown as folders). [`Store::replace_gmail_labels`]
//!   applies the server's list; [`Store::list_gmail_labels`] adds message
//!   counts for the UI.
//! * Per-message Gmail data ([`Store::set_gmail_message`]): thread id,
//!   label ids, category.
//! * The label operation queue: [`Store::enqueue_gmail_label_op`] changes
//!   the label (and the tags of the account's messages) **and** queues the
//!   server write in one transaction; the engine replays
//!   [`Store::pending_gmail_label_ops`] and reports with
//!   [`Store::finish_gmail_label_op`].

use nail_api::gmail::{
    GmailCategory, GmailLabel, GmailLabelColor, GmailLabelKind, GmailLabelListVisibility,
    GmailLabelSettings, GmailLabelSync, GmailMessageInfo, GmailMessageListVisibility,
};
use nail_api::{AccountId, FolderId, GmailLabelId, MessageId, OpId, ThreadId};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::error::{Error, Result};
use crate::store::Store;

/// Prefix of the temporary id of a label created locally.
pub const LOCAL_LABEL_PREFIX: &str = "local-";

/// Gmail sync state of an account (`gmail_sync`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GmailSyncRecord {
    /// The last `historyId` whose changes are all stored; `None` = a full
    /// sync is needed.
    pub history_id: Option<u64>,
    /// An interrupted full sync: the next page token.
    pub full_sync_page: Option<String>,
    /// … and the `historyId` taken when it started.
    pub full_sync_history_id: Option<u64>,
    /// Last successful sync (Unix seconds).
    pub last_sync_at: Option<i64>,
}

/// A label as stored (`gmail_labels`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GmailLabelRecord {
    /// Gmail's id (or a `local-…` id while its creation is queued).
    pub id: GmailLabelId,
    /// Name.
    pub name: String,
    /// System or user label.
    pub kind: GmailLabelKind,
    /// Colour.
    pub color: Option<GmailLabelColor>,
    /// Visibility in the label list.
    pub list_visibility: GmailLabelListVisibility,
    /// Visibility on messages.
    pub message_visibility: GmailMessageListVisibility,
    /// The Nail tag (user labels).
    pub tag: Option<String>,
    /// The folder (system labels shown as folders).
    pub folder_id: Option<FolderId>,
    /// Local sync state.
    pub sync: GmailLabelSync,
}

impl GmailLabelRecord {
    /// A user label from the server: its tag is its name.
    pub fn user(id: impl Into<String>, settings: &GmailLabelSettings) -> Self {
        Self {
            id: GmailLabelId::new(id),
            name: settings.name.clone(),
            kind: GmailLabelKind::User,
            color: settings.color.clone(),
            list_visibility: settings.list_visibility,
            message_visibility: settings.message_visibility,
            tag: Some(settings.name.clone()),
            folder_id: None,
            sync: GmailLabelSync::Synced,
        }
    }

    /// A system label (`INBOX`, `CATEGORY_SOCIAL`, …), optionally shown as
    /// a folder.
    pub fn system(id: &str, folder_id: Option<FolderId>) -> Self {
        Self {
            id: GmailLabelId::new(id),
            name: id.to_owned(),
            kind: GmailLabelKind::System,
            color: None,
            list_visibility: GmailLabelListVisibility::Show,
            message_visibility: GmailMessageListVisibility::Show,
            tag: None,
            folder_id,
            sync: GmailLabelSync::Synced,
        }
    }
}

/// What a queued label operation does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GmailLabelOp {
    /// Create a user label.
    Create(GmailLabelSettings),
    /// Change a user label.
    Update(GmailLabelSettings),
    /// Delete a user label.
    Delete,
}

/// A queued label operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedGmailLabelOp {
    /// Row id.
    pub id: i64,
    /// The account.
    pub account_id: AccountId,
    /// The label (a `local-…` id for a create not replayed yet).
    pub label_id: GmailLabelId,
    /// What to do.
    pub op: GmailLabelOp,
    /// Attempts so far.
    pub attempts: u32,
}

impl QueuedGmailLabelOp {
    /// The operation's id as the protocol shows it (`gmail-label-<n>`).
    pub fn op_id(&self) -> OpId {
        gmail_label_op_id(self.id)
    }
}

/// The protocol id of a label operation row.
pub fn gmail_label_op_id(id: i64) -> OpId {
    OpId::new(format!("gmail-label-{id}"))
}

/// How replaying a label operation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GmailLabelOpOutcome {
    /// Done; a create reports the server's label id.
    Done {
        /// The id Gmail assigned (creates only).
        server_id: Option<String>,
    },
    /// Try again later (offline, rate limited).
    Retry(String),
    /// Failed for good (the local change stays; the next label sync
    /// restores the server's state).
    Failed(String),
}

fn enum_str<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn enum_parse<T: serde::de::DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

const LABEL_COLS: &str = "label_id, name, kind, color_background, color_text, list_visibility, \
                          message_visibility, tag, folder_id, sync";

fn label_from_row(row: &Row<'_>) -> rusqlite::Result<GmailLabelRecord> {
    let kind: String = row.get(2)?;
    let background: Option<String> = row.get(3)?;
    let text: Option<String> = row.get(4)?;
    let list: String = row.get(5)?;
    let message: String = row.get(6)?;
    let sync: String = row.get(9)?;
    Ok(GmailLabelRecord {
        id: GmailLabelId(row.get(0)?),
        name: row.get(1)?,
        kind: enum_parse(&kind)?,
        color: background
            .zip(text)
            .map(|(background, text)| GmailLabelColor { background, text }),
        list_visibility: enum_parse(&list)?,
        message_visibility: enum_parse(&message)?,
        tag: row.get(7)?,
        folder_id: row.get::<_, Option<String>>(8)?.map(FolderId),
        sync: enum_parse(&sync)?,
    })
}

fn upsert_label_in_tx(
    tx: &Transaction<'_>,
    account: &AccountId,
    label: &GmailLabelRecord,
) -> Result<()> {
    tx.execute(
        "INSERT INTO gmail_labels (account_id, label_id, name, kind, color_background, color_text,
             list_visibility, message_visibility, tag, folder_id, sync)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT (account_id, label_id) DO UPDATE SET
             name = excluded.name, kind = excluded.kind,
             color_background = excluded.color_background, color_text = excluded.color_text,
             list_visibility = excluded.list_visibility,
             message_visibility = excluded.message_visibility, tag = excluded.tag,
             folder_id = excluded.folder_id, sync = excluded.sync",
        params![
            account.as_str(),
            label.id.as_str(),
            label.name,
            enum_str(&label.kind),
            label.color.as_ref().map(|c| c.background.as_str()),
            label.color.as_ref().map(|c| c.text.as_str()),
            enum_str(&label.list_visibility),
            enum_str(&label.message_visibility),
            label.tag,
            label.folder_id.as_ref().map(FolderId::as_str),
            enum_str(&label.sync),
        ],
    )?;
    Ok(())
}

fn label_in_tx(
    tx: &Transaction<'_>,
    account: &AccountId,
    id: &GmailLabelId,
) -> Result<GmailLabelRecord> {
    tx.query_row(
        &format!("SELECT {LABEL_COLS} FROM gmail_labels WHERE account_id = ?1 AND label_id = ?2"),
        params![account.as_str(), id.as_str()],
        label_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("Gmail label", id))
}

/// Renames (`to = Some`) or removes (`to = None`) a tag on every message
/// of an account.
fn retag_in_tx(
    tx: &Transaction<'_>,
    account: &AccountId,
    from: &str,
    to: Option<&str>,
) -> Result<()> {
    if let Some(to) = to {
        tx.execute(
            "INSERT OR IGNORE INTO message_tags (message_id, tag)
             SELECT t.message_id, ?3 FROM message_tags t
             JOIN messages m ON m.id = t.message_id
             WHERE m.account_id = ?1 AND t.tag = ?2",
            params![account.as_str(), from, to],
        )?;
    }
    tx.execute(
        "DELETE FROM message_tags WHERE tag = ?2
         AND message_id IN (SELECT id FROM messages WHERE account_id = ?1)",
        params![account.as_str(), from],
    )?;
    Ok(())
}

/// A message's Gmail data on a connection (summaries attach it).
pub(crate) fn gmail_info_in(conn: &Connection, id: &MessageId) -> Result<Option<GmailMessageInfo>> {
    let row: Option<(Option<String>, String, String, Option<String>)> = conn
        .prepare_cached(
            "SELECT m.remote_id, g.gmail_thread_id, g.label_ids, g.category
             FROM gmail_messages g JOIN messages m ON m.id = g.message_id
             WHERE g.message_id = ?1",
        )?
        .query_row([id.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?;
    row.map(|(gmail_id, thread_id, labels, category)| {
        Ok(GmailMessageInfo {
            gmail_id: gmail_id.unwrap_or_default(),
            thread_id,
            label_ids: serde_json::from_str(&labels)?,
            category: category
                .as_deref()
                .map(enum_parse::<GmailCategory>)
                .transpose()?,
        })
    })
    .transpose()
}

impl Store {
    // --- Remote ids ------------------------------------------------------

    /// Sets (or clears) a message's id on its API server.
    pub fn set_message_remote_id(&self, id: &MessageId, remote_id: Option<&str>) -> Result<()> {
        self.with_conn(|conn| set_message_remote_id_in(conn, id, remote_id))
    }

    /// Sets (or clears) a message's server thread id — Gmail `threadId`,
    /// Graph / EWS conversation id (M4c, schema v15) — and re-threads it:
    /// messages of one server thread share a Nail thread. Returns the
    /// message's thread.
    pub fn set_message_remote_thread_id(
        &self,
        id: &MessageId,
        remote_thread_id: Option<&str>,
    ) -> Result<ThreadId> {
        self.with_tx(|tx| {
            let changed = tx.execute(
                "UPDATE messages SET remote_thread_id = ?2 WHERE id = ?1",
                params![id.as_str(), remote_thread_id.filter(|t| !t.is_empty())],
            )?;
            if changed == 0 {
                return Err(Error::not_found("message", id));
            }
            Ok(ThreadId::new(crate::thread::assign_thread(
                tx,
                id.as_str(),
            )?))
        })
    }

    /// A message's server thread id (schema v15).
    pub fn message_remote_thread_id(&self, id: &MessageId) -> Result<Option<String>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT remote_thread_id FROM messages WHERE id = ?1",
                [id.as_str()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or_else(|| Error::not_found("message", id))
        })
    }

    /// The message with this remote id in an account.
    pub fn message_by_remote_id(
        &self,
        account: &AccountId,
        remote_id: &str,
    ) -> Result<Option<MessageId>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT id FROM messages WHERE account_id = ?1 AND remote_id = ?2",
                    params![account.as_str(), remote_id],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .map(MessageId))
        })
    }

    /// A message's remote id.
    pub fn message_remote_id(&self, id: &MessageId) -> Result<Option<String>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT remote_id FROM messages WHERE id = ?1",
                [id.as_str()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or_else(|| Error::not_found("message", id))
        })
    }

    // --- Sync state --------------------------------------------------------

    /// The Gmail sync state of an account (empty when never synced).
    pub fn gmail_sync(&self, account: &AccountId) -> Result<GmailSyncRecord> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT history_id, full_sync_page, full_sync_history_id, last_sync_at
                     FROM gmail_sync WHERE account_id = ?1",
                    [account.as_str()],
                    |r| {
                        Ok(GmailSyncRecord {
                            history_id: r
                                .get::<_, Option<i64>>(0)?
                                .and_then(|h| u64::try_from(h).ok()),
                            full_sync_page: r.get(1)?,
                            full_sync_history_id: r
                                .get::<_, Option<i64>>(2)?
                                .and_then(|h| u64::try_from(h).ok()),
                            last_sync_at: r.get(3)?,
                        })
                    },
                )
                .optional()?
                .unwrap_or_default())
        })
    }

    /// Replaces the Gmail sync state of an account.
    pub fn set_gmail_sync(&self, account: &AccountId, record: &GmailSyncRecord) -> Result<()> {
        let as_sql = |h: Option<u64>| h.map(|h| i64::try_from(h).unwrap_or(i64::MAX));
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO gmail_sync (account_id, history_id, full_sync_page,
                     full_sync_history_id, last_sync_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (account_id) DO UPDATE SET
                     history_id = excluded.history_id, full_sync_page = excluded.full_sync_page,
                     full_sync_history_id = excluded.full_sync_history_id,
                     last_sync_at = excluded.last_sync_at",
                params![
                    account.as_str(),
                    as_sql(record.history_id),
                    record.full_sync_page,
                    as_sql(record.full_sync_history_id),
                    record.last_sync_at,
                ],
            )?;
            Ok(())
        })
    }

    // --- Labels ------------------------------------------------------------

    /// The account's labels as stored: system labels first, then user
    /// labels by name.
    pub fn gmail_labels(&self, account: &AccountId) -> Result<Vec<GmailLabelRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {LABEL_COLS} FROM gmail_labels WHERE account_id = ?1
                 ORDER BY kind = 'user', name COLLATE NOCASE, label_id"
            ))?;
            let rows = stmt.query_map([account.as_str()], label_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The user label whose tag is `tag`.
    pub fn gmail_label_by_tag(
        &self,
        account: &AccountId,
        tag: &str,
    ) -> Result<Option<GmailLabelRecord>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {LABEL_COLS} FROM gmail_labels WHERE account_id = ?1 AND tag = ?2"
                    ),
                    params![account.as_str(), tag],
                    label_from_row,
                )
                .optional()?)
        })
    }

    /// Applies the server's label list: labels not on the server are
    /// removed (unless created locally and still queued), user labels the
    /// server renamed rename their tag on every message of the account.
    pub fn replace_gmail_labels(
        &self,
        account: &AccountId,
        labels: &[GmailLabelRecord],
    ) -> Result<()> {
        self.with_tx(|tx| {
            let mut stmt = tx.prepare(&format!(
                "SELECT {LABEL_COLS} FROM gmail_labels WHERE account_id = ?1"
            ))?;
            let current: Vec<GmailLabelRecord> = stmt
                .query_map([account.as_str()], label_from_row)?
                .collect::<rusqlite::Result<_>>()?;
            drop(stmt);
            for old in &current {
                let keep_local = old.sync == GmailLabelSync::Pending
                    && old.id.as_str().starts_with(LOCAL_LABEL_PREFIX);
                match labels.iter().find(|l| l.id == old.id) {
                    None if keep_local => {}
                    None => {
                        if let Some(tag) = &old.tag {
                            retag_in_tx(tx, account, tag, None)?;
                        }
                        tx.execute(
                            "DELETE FROM gmail_labels WHERE account_id = ?1 AND label_id = ?2",
                            params![account.as_str(), old.id.as_str()],
                        )?;
                    }
                    Some(new) => {
                        if let (Some(from), Some(to)) = (&old.tag, &new.tag)
                            && from != to
                        {
                            retag_in_tx(tx, account, from, Some(to))?;
                        }
                    }
                }
            }
            for label in labels {
                upsert_label_in_tx(tx, account, label)?;
            }
            Ok(())
        })
    }

    /// The account's labels with message counts (from the stored Gmail
    /// label ids), for [`nail_api::Request::ListGmailLabels`].
    pub fn list_gmail_labels(&self, account: &AccountId) -> Result<Vec<GmailLabel>> {
        let records = self.gmail_labels(account)?;
        let counts: std::collections::HashMap<String, (u32, u32)> = self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT j.value, COUNT(*), SUM(m.seen = 0)
                 FROM gmail_messages g
                 JOIN messages m ON m.id = g.message_id, json_each(g.label_ids) j
                 WHERE g.account_id = ?1
                 GROUP BY j.value",
            )?;
            let rows = stmt.query_map([account.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?)))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        Ok(records
            .into_iter()
            .map(|r| {
                let (total, unread) = counts.get(r.id.as_str()).copied().unwrap_or((0, 0));
                GmailLabel {
                    account_id: account.clone(),
                    id: r.id,
                    name: r.name,
                    kind: r.kind,
                    color: r.color,
                    list_visibility: r.list_visibility,
                    message_visibility: r.message_visibility,
                    tag: r.tag,
                    folder_id: r.folder_id,
                    total,
                    unread,
                    sync: r.sync,
                }
            })
            .collect())
    }

    // --- Messages ----------------------------------------------------------

    /// Stores a message's Gmail data (thread, labels, category).
    pub fn set_gmail_message(&self, id: &MessageId, info: &GmailMessageInfo) -> Result<()> {
        self.with_tx(|tx| set_gmail_message_in(tx, id, info))
    }

    /// A message's Gmail data.
    pub fn gmail_message(&self, id: &MessageId) -> Result<Option<GmailMessageInfo>> {
        self.with_conn(|conn| gmail_info_in(conn, id))
    }

    // --- Label operation queue ----------------------------------------------

    /// Applies a label change locally and queues it for the server, in one
    /// transaction. A create stores the label under a new `local-…` id
    /// (pass any id; it is ignored) and returns that id with the op; an
    /// update renames the tag on the account's messages; a delete removes
    /// the label and its tag. System labels cannot be changed.
    pub fn enqueue_gmail_label_op(
        &self,
        account: &AccountId,
        label_id: &GmailLabelId,
        op: GmailLabelOp,
    ) -> Result<(GmailLabelId, OpId)> {
        self.with_tx(|tx| {
            let (label_id, payload, kind) = match op {
                GmailLabelOp::Create(settings) => {
                    let n: i64 = tx.query_row(
                        "SELECT COALESCE(MAX(id), 0) + 1 FROM gmail_label_ops",
                        [],
                        |r| r.get(0),
                    )?;
                    let id = GmailLabelId::new(format!("{LOCAL_LABEL_PREFIX}{n}"));
                    let mut record = GmailLabelRecord::user(id.as_str(), &settings);
                    record.sync = GmailLabelSync::Pending;
                    upsert_label_in_tx(tx, account, &record)?;
                    (id, Some(serde_json::to_string(&settings)?), "create")
                }
                GmailLabelOp::Update(settings) => {
                    let old = label_in_tx(tx, account, label_id)?;
                    if old.kind == GmailLabelKind::System {
                        return Err(Error::InvalidData(format!(
                            "system label {label_id} cannot be changed"
                        )));
                    }
                    let mut record = GmailLabelRecord::user(label_id.as_str(), &settings);
                    record.sync = GmailLabelSync::Pending;
                    if let Some(from) = &old.tag
                        && from != &settings.name
                    {
                        retag_in_tx(tx, account, from, Some(&settings.name))?;
                    }
                    upsert_label_in_tx(tx, account, &record)?;
                    (
                        label_id.clone(),
                        Some(serde_json::to_string(&settings)?),
                        "update",
                    )
                }
                GmailLabelOp::Delete => {
                    let old = label_in_tx(tx, account, label_id)?;
                    if old.kind == GmailLabelKind::System {
                        return Err(Error::InvalidData(format!(
                            "system label {label_id} cannot be deleted"
                        )));
                    }
                    if let Some(tag) = &old.tag {
                        retag_in_tx(tx, account, tag, None)?;
                    }
                    tx.execute(
                        "DELETE FROM gmail_labels WHERE account_id = ?1 AND label_id = ?2",
                        params![account.as_str(), label_id.as_str()],
                    )?;
                    (label_id.clone(), None, "delete")
                }
            };
            tx.execute(
                "INSERT INTO gmail_label_ops (account_id, label_id, kind, payload)
                 VALUES (?1, ?2, ?3, ?4)",
                params![account.as_str(), label_id.as_str(), kind, payload],
            )?;
            Ok((label_id, gmail_label_op_id(tx.last_insert_rowid())))
        })
    }

    /// Pending label operations of an account, in queue order.
    pub fn pending_gmail_label_ops(&self, account: &AccountId) -> Result<Vec<QueuedGmailLabelOp>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, account_id, label_id, kind, payload, attempts FROM gmail_label_ops
                 WHERE account_id = ?1 AND state = 'pending' ORDER BY id",
            )?;
            let rows = stmt.query_map([account.as_str()], |r| {
                let kind: String = r.get(3)?;
                let payload: Option<String> = r.get(4)?;
                let settings = || -> rusqlite::Result<GmailLabelSettings> {
                    serde_json::from_str(payload.as_deref().unwrap_or("null")).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })
                };
                Ok(QueuedGmailLabelOp {
                    id: r.get(0)?,
                    account_id: AccountId(r.get(1)?),
                    label_id: GmailLabelId(r.get(2)?),
                    op: match kind.as_str() {
                        "create" => GmailLabelOp::Create(settings()?),
                        "update" => GmailLabelOp::Update(settings()?),
                        _ => GmailLabelOp::Delete,
                    },
                    attempts: r.get(5)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Records how replaying a label operation ended. A finished create
    /// moves the label (and later queued operations) from its `local-…`
    /// id to the server's; a finished create / update marks the label
    /// synced.
    pub fn finish_gmail_label_op(&self, op_id: i64, outcome: &GmailLabelOpOutcome) -> Result<()> {
        self.with_tx(|tx| {
            let (account, label_id): (String, String) = tx
                .query_row(
                    "SELECT account_id, label_id FROM gmail_label_ops WHERE id = ?1",
                    [op_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| Error::not_found("Gmail label operation", op_id))?;
            match outcome {
                GmailLabelOpOutcome::Done { server_id } => {
                    tx.execute(
                        "UPDATE gmail_label_ops SET state = 'done', last_error = NULL,
                             attempts = attempts + 1,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                         WHERE id = ?1",
                        [op_id],
                    )?;
                    let mut label = label_id.clone();
                    if let Some(server_id) = server_id
                        && server_id != &label_id
                    {
                        tx.execute(
                            "UPDATE gmail_labels SET label_id = ?3
                             WHERE account_id = ?1 AND label_id = ?2",
                            params![account, label_id, server_id],
                        )?;
                        tx.execute(
                            "UPDATE gmail_label_ops SET label_id = ?3
                             WHERE account_id = ?1 AND label_id = ?2 AND state = 'pending'",
                            params![account, label_id, server_id],
                        )?;
                        label.clone_from(server_id);
                    }
                    let still_queued: bool = tx.query_row(
                        "SELECT EXISTS (SELECT 1 FROM gmail_label_ops
                         WHERE account_id = ?1 AND label_id = ?2 AND state = 'pending')",
                        params![account, label],
                        |r| r.get(0),
                    )?;
                    if !still_queued {
                        tx.execute(
                            "UPDATE gmail_labels SET sync = 'synced'
                             WHERE account_id = ?1 AND label_id = ?2",
                            params![account, label],
                        )?;
                    }
                }
                GmailLabelOpOutcome::Retry(error) => {
                    tx.execute(
                        "UPDATE gmail_label_ops SET attempts = attempts + 1, last_error = ?2,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                         WHERE id = ?1",
                        params![op_id, error],
                    )?;
                }
                GmailLabelOpOutcome::Failed(error) => {
                    tx.execute(
                        "UPDATE gmail_label_ops SET state = 'failed', attempts = attempts + 1,
                             last_error = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                         WHERE id = ?1",
                        params![op_id, error],
                    )?;
                }
            }
            Ok(())
        })
    }
}

/// [`Store::set_message_remote_id`] on an open connection / transaction.
pub(crate) fn set_message_remote_id_in(
    conn: &Connection,
    id: &MessageId,
    remote_id: Option<&str>,
) -> Result<()> {
    let changed = conn
        .prepare_cached("UPDATE messages SET remote_id = ?2 WHERE id = ?1")?
        .execute(params![id.as_str(), remote_id])?;
    if changed == 0 {
        return Err(Error::not_found("message", id));
    }
    Ok(())
}

/// [`Store::set_gmail_message`] inside an open transaction.
pub(crate) fn set_gmail_message_in(
    conn: &Connection,
    id: &MessageId,
    info: &GmailMessageInfo,
) -> Result<()> {
    let account: String = conn
        .query_row(
            "SELECT account_id FROM messages WHERE id = ?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::not_found("message", id))?;
    conn.execute(
        "INSERT INTO gmail_messages (message_id, account_id, gmail_thread_id, label_ids,
             category)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (message_id) DO UPDATE SET
             account_id = excluded.account_id,
             gmail_thread_id = excluded.gmail_thread_id,
             label_ids = excluded.label_ids, category = excluded.category",
        params![
            id.as_str(),
            account,
            info.thread_id,
            serde_json::to_string(&info.label_ids)?,
            info.category.as_ref().map(enum_str),
        ],
    )?;
    conn.execute(
        "UPDATE messages SET remote_id = ?2 WHERE id = ?1",
        params![id.as_str(), info.gmail_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredMessage;
    use nail_api::gmail::GmailFolder;
    use nail_api::{Account, AccountKind, Flags, Folder, MessageSummary};

    fn account() -> AccountId {
        AccountId::new("gm")
    }

    fn store() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_account(&Account {
                id: account(),
                name: "Gmail".into(),
                email: "ada@gmail.example".into(),
                kind: AccountKind::Gmail,
            })
            .unwrap();
        for folder in [GmailFolder::Inbox, GmailFolder::Archive] {
            store
                .upsert_folder(&Folder {
                    id: format!("gm-{}", folder.path()).into(),
                    account_id: account(),
                    parent_id: None,
                    name: folder.path().into(),
                    path: folder.path().into(),
                    role: folder.role(),
                    unread: 0,
                    total: 0,
                })
                .unwrap();
        }
        for (id, seen, tags) in [("m1", false, vec!["Work"]), ("m2", true, vec!["Work"])] {
            store
                .upsert_message(&StoredMessage::new(MessageSummary {
                    id: id.into(),
                    folder_id: "gm-INBOX".into(),
                    account_id: account(),
                    subject: id.into(),
                    from: None,
                    to: vec![],
                    date: "2026-10-01T10:00:00Z".into(),
                    flags: Flags {
                        seen,
                        ..Flags::default()
                    },
                    has_attachments: false,
                    preview: String::new(),
                    thread_id: None,
                    tags: tags.into_iter().map(str::to_owned).collect(),
                    duplicates: vec![],
                    headers_only: false,
                    gmail: None,
                    exchange: None,
                }))
                .unwrap();
        }
        store
    }

    fn tags(store: &Store, id: &str) -> Vec<String> {
        store.get_message(&id.into()).unwrap().summary.tags
    }

    #[test]
    fn remote_ids_and_message_data() {
        let store = store();
        let info = GmailMessageInfo {
            gmail_id: "18c0".into(),
            thread_id: "t1".into(),
            label_ids: vec!["INBOX".into(), "UNREAD".into(), "Label_1".into()],
            category: Some(GmailCategory::Social),
        };
        store.set_gmail_message(&"m1".into(), &info).unwrap();
        assert_eq!(store.gmail_message(&"m1".into()).unwrap(), Some(info));
        assert_eq!(
            store.message_by_remote_id(&account(), "18c0").unwrap(),
            Some("m1".into())
        );
        assert_eq!(
            store.message_remote_id(&"m1".into()).unwrap().as_deref(),
            Some("18c0")
        );
        store
            .set_message_remote_id(&"m2".into(), Some("18c1"))
            .unwrap();
        assert_eq!(
            store.message_by_remote_id(&account(), "18c1").unwrap(),
            Some("m2".into())
        );
        // One remote id per account.
        assert!(
            store
                .set_message_remote_id(&"m2".into(), Some("18c0"))
                .is_err()
        );
        assert!(store.gmail_message(&"m2".into()).unwrap().is_none());
        assert!(
            store
                .set_message_remote_id(&"nope".into(), None)
                .unwrap_err()
                .is_not_found()
        );
    }

    #[test]
    fn queued_moves_carry_the_remote_id() {
        let store = store();
        store
            .set_message_remote_id(&"m1".into(), Some("18c0"))
            .unwrap();
        let enqueued = store
            .enqueue(&crate::Operation::MoveMessage {
                message_id: "m1".into(),
                target_folder_id: "gm-ARCHIVE".into(),
                source: None,
            })
            .unwrap();
        let source = enqueued.op.operation.source().unwrap();
        assert_eq!(source.remote_id.as_deref(), Some("18c0"));
        assert_eq!(source.folder_id.as_str(), "gm-INBOX");
        // The remote id stays with the moved message.
        assert_eq!(
            store.message_by_remote_id(&account(), "18c0").unwrap(),
            Some("m1".into())
        );
    }

    #[test]
    fn sync_state_round_trips() {
        let store = store();
        assert_eq!(
            store.gmail_sync(&account()).unwrap(),
            GmailSyncRecord::default()
        );
        let record = GmailSyncRecord {
            history_id: Some(123_456_789_012),
            full_sync_page: Some("page-2".into()),
            full_sync_history_id: Some(99),
            last_sync_at: Some(7),
        };
        store.set_gmail_sync(&account(), &record).unwrap();
        assert_eq!(store.gmail_sync(&account()).unwrap(), record);
    }

    #[test]
    fn server_labels_replace_and_rename_tags() {
        let store = store();
        let work = GmailLabelRecord::user("Label_1", &GmailLabelSettings::named("Work"));
        store
            .replace_gmail_labels(
                &account(),
                &[
                    GmailLabelRecord::system("INBOX", Some("gm-INBOX".into())),
                    work.clone(),
                ],
            )
            .unwrap();
        let info = GmailMessageInfo {
            gmail_id: "18c0".into(),
            thread_id: "t1".into(),
            label_ids: vec!["INBOX".into(), "Label_1".into()],
            category: None,
        };
        store.set_gmail_message(&"m1".into(), &info).unwrap();
        let listed = store.list_gmail_labels(&account()).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id.as_str(), "INBOX", "system labels first");
        assert_eq!((listed[1].total, listed[1].unread), (1, 1));
        assert_eq!(listed[1].tag.as_deref(), Some("Work"));

        // Renamed on the server: the tag follows.
        let renamed = GmailLabelRecord::user("Label_1", &GmailLabelSettings::named("Job"));
        store
            .replace_gmail_labels(&account(), std::slice::from_ref(&renamed))
            .unwrap();
        assert_eq!(tags(&store, "m1"), ["Job"]);
        assert_eq!(
            store
                .gmail_label_by_tag(&account(), "Job")
                .unwrap()
                .unwrap()
                .id
                .as_str(),
            "Label_1"
        );
        // Removed on the server: the tag goes.
        store.replace_gmail_labels(&account(), &[]).unwrap();
        assert_eq!(tags(&store, "m2").len(), 0);
        assert_eq!(store.gmail_labels(&account()).unwrap().len(), 0);
    }

    #[test]
    fn label_queue_applies_locally_and_takes_server_ids() {
        let store = store();
        let (local, op) = store
            .enqueue_gmail_label_op(
                &account(),
                &GmailLabelId::new(""),
                GmailLabelOp::Create(GmailLabelSettings::named("Travel")),
            )
            .unwrap();
        assert!(local.as_str().starts_with(LOCAL_LABEL_PREFIX));
        assert!(op.as_str().starts_with("gmail-label-"));
        // A server list without it keeps the pending local label.
        store.replace_gmail_labels(&account(), &[]).unwrap();
        let labels = store.gmail_labels(&account()).unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].sync, GmailLabelSync::Pending);
        // Rename before the create was replayed: both ops queue.
        store
            .enqueue_gmail_label_op(
                &account(),
                &local,
                GmailLabelOp::Update(GmailLabelSettings::named("Trips")),
            )
            .unwrap();
        let pending = store.pending_gmail_label_ops(&account()).unwrap();
        assert_eq!(pending.len(), 2);
        assert!(matches!(pending[0].op, GmailLabelOp::Create(_)));
        store
            .finish_gmail_label_op(
                pending[0].id,
                &GmailLabelOpOutcome::Done {
                    server_id: Some("Label_9".into()),
                },
            )
            .unwrap();
        let pending = store.pending_gmail_label_ops(&account()).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].label_id.as_str(), "Label_9");
        assert_eq!(
            store.gmail_labels(&account()).unwrap()[0].sync,
            GmailLabelSync::Pending,
            "the update is still queued"
        );
        store
            .finish_gmail_label_op(pending[0].id, &GmailLabelOpOutcome::Retry("offline".into()))
            .unwrap();
        let pending = store.pending_gmail_label_ops(&account()).unwrap();
        assert_eq!(pending[0].attempts, 1);
        store
            .finish_gmail_label_op(
                pending[0].id,
                &GmailLabelOpOutcome::Done { server_id: None },
            )
            .unwrap();
        let labels = store.gmail_labels(&account()).unwrap();
        assert_eq!(labels[0].id.as_str(), "Label_9");
        assert_eq!(labels[0].name, "Trips");
        assert_eq!(labels[0].sync, GmailLabelSync::Synced);
        assert!(
            store
                .pending_gmail_label_ops(&account())
                .unwrap()
                .is_empty(),
            "{:?}",
            store.pending_gmail_label_ops(&account()).unwrap()
        );
    }

    #[test]
    fn label_updates_and_deletes_change_tags() {
        let store = store();
        store
            .replace_gmail_labels(
                &account(),
                &[
                    GmailLabelRecord::system("INBOX", None),
                    GmailLabelRecord::user("Label_1", &GmailLabelSettings::named("Work")),
                ],
            )
            .unwrap();
        store
            .enqueue_gmail_label_op(
                &account(),
                &"Label_1".into(),
                GmailLabelOp::Update(GmailLabelSettings::named("Office")),
            )
            .unwrap();
        assert_eq!(tags(&store, "m1"), ["Office"]);
        store
            .enqueue_gmail_label_op(&account(), &"Label_1".into(), GmailLabelOp::Delete)
            .unwrap();
        assert_eq!(tags(&store, "m1").len(), 0);
        assert!(
            store
                .enqueue_gmail_label_op(&account(), &"INBOX".into(), GmailLabelOp::Delete)
                .is_err()
        );
        assert!(
            store
                .enqueue_gmail_label_op(&account(), &"Label_404".into(), GmailLabelOp::Delete)
                .unwrap_err()
                .is_not_found()
        );
        let pending = store.pending_gmail_label_ops(&account()).unwrap();
        assert_eq!(pending.len(), 2);
        store
            .finish_gmail_label_op(pending[1].id, &GmailLabelOpOutcome::Failed("gone".into()))
            .unwrap();
        assert_eq!(store.pending_gmail_label_ops(&account()).unwrap().len(), 1);
    }
}
