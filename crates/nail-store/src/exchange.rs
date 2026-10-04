//! Exchange accounts in the store (schema v12, M3c, ADR 0072).
//!
//! * Sync state per account ([`ExchangeSyncRecord`]) and the
//!   folder-hierarchy cursor per mailbox
//!   ([`Store::exchange_hierarchy_cursor`]).
//! * Mail folders ([`ExchangeFolderRecord`]): remote id, change key,
//!   parent, well-known name and the items cursor per folder; messages
//!   keep their remote id in `messages.remote_id`
//!   ([`Store::set_message_remote_id`]) and the Exchange extras in
//!   `exchange_items` ([`Store::set_exchange_item`],
//!   [`Store::exchange_message_info`]).
//! * PIM: one source per account ([`Store::ensure_exchange_pim_source`]),
//!   remote id / change key / cursor per collection
//!   ([`ExchangePimCollectionRecord`]; [`nail_api::pim::Calendar::exchange`]
//!   is filled from it) and remote id / change key per item.
//! * Shared and delegate mailboxes ([`Store::exchange_mailboxes`]).
//! * The master category list ([`Store::list_exchange_categories`] with tag
//!   counts), automatic replies (sealed), inbox rules with the Nail rule
//!   each was placed from, cached rooms and room lists.
//! * The Exchange operation queue: [`Store::enqueue_exchange_op`] applies
//!   a change locally **and** queues it in one transaction; the engine
//!   replays [`Store::pending_exchange_ops`] and reports with
//!   [`Store::finish_exchange_op`].

use std::collections::HashMap;

use nail_api::exchange::rules::{ExchangeInboxRule, ExchangeRulesState};
use nail_api::exchange::{
    AutomaticReplies, AutomaticRepliesSettings, CategoryColor, ExchangeCategory,
    ExchangeCategorySettings, ExchangeCollectionInfo, ExchangeCollectionRole, ExchangeEngine,
    ExchangeMailbox, ExchangeMailboxKind, ExchangeMailboxOrigin, ExchangeMailboxSettings,
    ExchangeMailboxState, ExchangeMessageInfo, ExchangeSync, ExchangeWellKnownFolder,
    InferenceClassification, LOCAL_CATEGORY_PREFIX, Room, RoomList, exchange_pim_source_id,
};
use nail_api::pim::source::PimSourceKind;
use nail_api::{
    AccountId, AddressBookId, ApiError, CalendarId, ExchangeCategoryId, FolderId, MessageId, OpId,
    PimSourceId, RuleId,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::Store;

/// The store's mailbox key: `''` for the account's own mailbox, else the
/// lowercase address.
fn mailbox_key(mailbox: Option<&str>) -> String {
    mailbox.map(str::to_ascii_lowercase).unwrap_or_default()
}

fn mailbox_of(key: String) -> Option<String> {
    (!key.is_empty()).then_some(key)
}

fn json_error(idx: usize, err: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(err))
}

fn kebab<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_value(value)?
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

fn from_kebab<T: serde::de::DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
}

// ---------------------------------------------------------------------------
// Records

/// Exchange sync state of an account (`exchange_sync`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExchangeSyncRecord {
    /// Graph or EWS.
    pub engine: ExchangeEngine,
    /// The server's version (EWS).
    pub server_version: Option<String>,
    /// EWS streaming subscription.
    pub subscription_id: Option<String>,
    /// … and its watermark.
    pub watermark: Option<String>,
    /// Throttled until (Unix seconds).
    pub throttled_until: Option<i64>,
    /// Last complete sync (Unix seconds).
    pub last_sync_at: Option<i64>,
}

/// A mail folder of an Exchange account (`exchange_folders`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeFolderRecord {
    /// The local folder.
    pub folder_id: FolderId,
    /// The shared / delegate mailbox (lowercase), `None` = own.
    pub mailbox: Option<String>,
    /// The server's id.
    pub remote_id: String,
    /// The version.
    pub change_key: Option<String>,
    /// The parent's remote id.
    pub parent_remote_id: Option<String>,
    /// A well-known folder.
    pub well_known: Option<ExchangeWellKnownFolder>,
    /// The items cursor (`None` = full sync needed).
    pub cursor: Option<String>,
    /// An interrupted first sync.
    pub full_sync_page: Option<String>,
    /// Last sync (Unix seconds).
    pub last_sync_at: Option<i64>,
}

impl ExchangeFolderRecord {
    /// A new folder without a cursor.
    pub fn new(folder_id: impl Into<FolderId>, remote_id: impl Into<String>) -> Self {
        Self {
            folder_id: folder_id.into(),
            mailbox: None,
            remote_id: remote_id.into(),
            change_key: None,
            parent_remote_id: None,
            well_known: None,
            cursor: None,
            full_sync_page: None,
            last_sync_at: None,
        }
    }
}

/// Exchange extras of a stored message (`exchange_items`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExchangeItemRecord {
    /// The shared / delegate mailbox, `None` = own.
    pub mailbox: Option<String>,
    /// The version.
    pub change_key: Option<String>,
    /// The conversation.
    pub conversation_id: Option<String>,
    /// Focused Inbox.
    pub inference: Option<InferenceClassification>,
}

/// A calendar / address book / task list of an Exchange account
/// (`exchange_pim_collections`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangePimCollectionRecord {
    /// The PIM collection (calendar or address book id).
    pub collection_id: String,
    /// The shared / delegate mailbox, `None` = own.
    pub mailbox: Option<String>,
    /// The server's id.
    pub remote_id: String,
    /// The version.
    pub change_key: Option<String>,
    /// A special role.
    pub role: Option<ExchangeCollectionRole>,
    /// A Microsoft To Do list.
    pub todo: bool,
    /// The items cursor.
    pub cursor: Option<String>,
    /// Last sync (Unix seconds).
    pub last_sync_at: Option<i64>,
}

/// A category as stored (`exchange_categories`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCategoryRecord {
    /// The server's id (or `local-…`).
    pub id: ExchangeCategoryId,
    /// Name = tag.
    pub name: String,
    /// Colour.
    pub color: CategoryColor,
    /// Local sync state.
    pub sync: ExchangeSync,
}

/// A queued Exchange write (`exchange_ops.payload`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ExchangeOp {
    /// Create a category (stored under a `local-…` id meanwhile).
    CreateCategory {
        /// Name and colour.
        settings: ExchangeCategorySettings,
    },
    /// Change a category (the core retags messages through the mail
    /// queue when the name changes).
    UpdateCategory {
        /// The category.
        category_id: ExchangeCategoryId,
        /// The new settings.
        settings: ExchangeCategorySettings,
    },
    /// Delete a category (tags stay on messages).
    DeleteCategory {
        /// The category.
        category_id: ExchangeCategoryId,
    },
    /// Replace the automatic replies.
    SetAutomaticReplies {
        /// The settings.
        settings: AutomaticRepliesSettings,
    },
    /// Switch an inbox rule on / off.
    SetRuleEnabled {
        /// The server rule.
        server_rule_id: String,
        /// On or off.
        enabled: bool,
    },
    /// Delete an inbox rule.
    DeleteRule {
        /// The server rule.
        server_rule_id: String,
    },
    /// Move messages between Focused and Other.
    Classify {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// Where to.
        classification: InferenceClassification,
        /// Also for future mail from their senders.
        always: bool,
    },
}

impl ExchangeOp {
    fn kind(&self) -> &'static str {
        match self {
            Self::CreateCategory { .. } => "create-category",
            Self::UpdateCategory { .. } => "update-category",
            Self::DeleteCategory { .. } => "delete-category",
            Self::SetAutomaticReplies { .. } => "set-automatic-replies",
            Self::SetRuleEnabled { .. } => "set-rule-enabled",
            Self::DeleteRule { .. } => "delete-rule",
            Self::Classify { .. } => "classify",
        }
    }

    fn target(&self) -> Option<&str> {
        match self {
            Self::UpdateCategory { category_id, .. } | Self::DeleteCategory { category_id } => {
                Some(category_id.as_str())
            }
            Self::SetRuleEnabled { server_rule_id, .. } | Self::DeleteRule { server_rule_id } => {
                Some(server_rule_id)
            }
            Self::CreateCategory { .. }
            | Self::SetAutomaticReplies { .. }
            | Self::Classify { .. } => None,
        }
    }
}

/// A pending Exchange operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedExchangeOp {
    /// Queue id.
    pub id: i64,
    /// The account.
    pub account_id: AccountId,
    /// The category a create made (its `local-…` id), else the op's target.
    pub target: Option<String>,
    /// What to do.
    pub op: ExchangeOp,
    /// Tries so far.
    pub attempts: u32,
}

impl QueuedExchangeOp {
    /// The op id clients see.
    pub fn op_id(&self) -> OpId {
        exchange_op_id(self.id)
    }
}

/// The op id of a queued Exchange operation (`exchange-<n>`).
pub fn exchange_op_id(id: i64) -> OpId {
    OpId::new(format!("exchange-{id}"))
}

/// How replaying an Exchange operation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeOpOutcome {
    /// Done; a created (or replaced) category reports the server's id.
    Done {
        /// The category's server id (create, or an update that changed it).
        server_id: Option<String>,
    },
    /// Try again later (the error, for display).
    Retry(String),
    /// Gave up; the local change is rolled back where possible (a created
    /// category disappears, other records are marked synced and corrected
    /// by the next sync).
    Failed(String),
}

// ---------------------------------------------------------------------------

const FOLDER_COLS: &str = "folder_id, mailbox, remote_id, change_key, parent_remote_id, \
     well_known, cursor, full_sync_page, last_sync_at";

fn folder_from_row(row: &Row<'_>) -> rusqlite::Result<ExchangeFolderRecord> {
    let well_known: Option<String> = row.get(5)?;
    Ok(ExchangeFolderRecord {
        folder_id: FolderId(row.get(0)?),
        mailbox: mailbox_of(row.get(1)?),
        remote_id: row.get(2)?,
        change_key: row.get(3)?,
        parent_remote_id: row.get(4)?,
        well_known: well_known.as_deref().and_then(from_kebab),
        cursor: row.get(6)?,
        full_sync_page: row.get(7)?,
        last_sync_at: row.get(8)?,
    })
}

const PIM_COLS: &str =
    "collection_id, mailbox, remote_id, change_key, role, todo, cursor, last_sync_at";

fn pim_collection_from_row(row: &Row<'_>) -> rusqlite::Result<ExchangePimCollectionRecord> {
    let role: Option<String> = row.get(4)?;
    Ok(ExchangePimCollectionRecord {
        collection_id: row.get(0)?,
        mailbox: mailbox_of(row.get(1)?),
        remote_id: row.get(2)?,
        change_key: row.get(3)?,
        role: role.as_deref().and_then(ExchangeCollectionRole::parse),
        todo: row.get(5)?,
        cursor: row.get(6)?,
        last_sync_at: row.get(7)?,
    })
}

fn category_from_row(row: &Row<'_>) -> rusqlite::Result<ExchangeCategoryRecord> {
    let color: String = row.get(2)?;
    let sync: String = row.get(3)?;
    Ok(ExchangeCategoryRecord {
        id: ExchangeCategoryId(row.get(0)?),
        name: row.get(1)?,
        color: CategoryColor::parse(&color).unwrap_or_default(),
        sync: ExchangeSync::parse(&sync),
    })
}

fn api_error_json(error: Option<&ApiError>) -> Result<Option<String>> {
    Ok(error.map(serde_json::to_string).transpose()?)
}

fn parse_api_error(text: Option<String>) -> Option<ApiError> {
    text.and_then(|t| serde_json::from_str(&t).ok())
}

/// `messages.remote_id`, mailbox, conversation id, Focused Inbox.
type InfoRow = (Option<String>, String, Option<String>, Option<String>);

/// The Exchange data of a message on a connection (summaries attach it).
pub(crate) fn exchange_info_in(
    conn: &Connection,
    id: &MessageId,
) -> Result<Option<ExchangeMessageInfo>> {
    let row: Option<InfoRow> = conn
        .prepare_cached(
            "SELECT m.remote_id, x.mailbox, x.conversation_id, x.inference
             FROM exchange_items x JOIN messages m ON m.id = x.message_id
             WHERE x.message_id = ?1",
        )?
        .query_row([id.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?;
    Ok(row.map(
        |(item_id, mailbox, conversation_id, inference)| ExchangeMessageInfo {
            item_id: item_id.unwrap_or_default(),
            conversation_id,
            inference: inference
                .as_deref()
                .and_then(InferenceClassification::parse),
            mailbox: mailbox_of(mailbox),
        },
    ))
}

fn upsert_category_in_tx(
    tx: &Transaction<'_>,
    account: &AccountId,
    record: &ExchangeCategoryRecord,
) -> Result<()> {
    tx.execute(
        "INSERT INTO exchange_categories (account_id, category_id, name, color, sync)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (account_id, category_id) DO UPDATE SET
             name = excluded.name, color = excluded.color, sync = excluded.sync",
        params![
            account.as_str(),
            record.id.as_str(),
            record.name,
            record.color.as_str(),
            record.sync.as_str()
        ],
    )?;
    Ok(())
}

fn mailbox_from_row(
    account: &AccountId,
    row: &Row<'_>,
) -> rusqlite::Result<(ExchangeMailbox, String)> {
    let kind: String = row.get(2)?;
    let origin: String = row.get(3)?;
    let state: String = row.get(8)?;
    let error = parse_api_error(row.get(10)?);
    let address: String = row.get(0)?;
    Ok((
        ExchangeMailbox {
            account_id: account.clone(),
            settings: ExchangeMailboxSettings {
                address: address.clone(),
                name: row.get(1)?,
                kind: if kind == "delegate" {
                    ExchangeMailboxKind::Delegate
                } else {
                    ExchangeMailboxKind::Shared
                },
                mail: row.get(4)?,
                calendar: row.get(5)?,
                contacts: row.get(6)?,
                tasks: row.get(7)?,
            },
            origin: if origin == "automapped" {
                ExchangeMailboxOrigin::Automapped
            } else {
                ExchangeMailboxOrigin::User
            },
            state: ExchangeMailboxState::parse(&state).unwrap_or_default(),
            root_folder_id: row.get::<_, Option<String>>(9)?.map(FolderId),
            calendar_ids: Vec::new(),
            address_book_ids: Vec::new(),
            error: error.as_ref().map(|e| e.code),
            localized: error.and_then(|e| e.localized.map(|l| *l)),
        },
        address,
    ))
}

const MAILBOX_COLS: &str = "address, name, kind, origin, mail, calendar, contacts, tasks, state, \
     root_folder_id, last_error";

impl Store {
    // --- Sync state --------------------------------------------------------

    /// The Exchange sync state of an account (`None` = never synced).
    pub fn exchange_sync(&self, account: &AccountId) -> Result<Option<ExchangeSyncRecord>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT engine, server_version, subscription_id, watermark, throttled_until,
                         last_sync_at
                     FROM exchange_sync WHERE account_id = ?1",
                    [account.as_str()],
                    |r| {
                        let engine: String = r.get(0)?;
                        Ok(ExchangeSyncRecord {
                            engine: ExchangeEngine::parse(&engine).unwrap_or_default(),
                            server_version: r.get(1)?,
                            subscription_id: r.get(2)?,
                            watermark: r.get(3)?,
                            throttled_until: r.get(4)?,
                            last_sync_at: r.get(5)?,
                        })
                    },
                )
                .optional()?)
        })
    }

    /// Replaces the Exchange sync state of an account (the rules state is
    /// kept).
    pub fn set_exchange_sync(
        &self,
        account: &AccountId,
        record: &ExchangeSyncRecord,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_sync (account_id, engine, server_version, subscription_id,
                     watermark, throttled_until, last_sync_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (account_id) DO UPDATE SET
                     engine = excluded.engine, server_version = excluded.server_version,
                     subscription_id = excluded.subscription_id,
                     watermark = excluded.watermark, throttled_until = excluded.throttled_until,
                     last_sync_at = excluded.last_sync_at",
                params![
                    account.as_str(),
                    record.engine.as_str(),
                    record.server_version,
                    record.subscription_id,
                    record.watermark,
                    record.throttled_until,
                    record.last_sync_at,
                ],
            )?;
            Ok(())
        })
    }

    /// The folder-hierarchy cursor of a mailbox (`None` = list every
    /// folder).
    pub fn exchange_hierarchy_cursor(
        &self,
        account: &AccountId,
        mailbox: Option<&str>,
    ) -> Result<Option<String>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT hierarchy_cursor FROM exchange_mailbox_sync
                     WHERE account_id = ?1 AND mailbox = ?2",
                    params![account.as_str(), mailbox_key(mailbox)],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten())
        })
    }

    /// Stores (or clears) the folder-hierarchy cursor of a mailbox.
    pub fn set_exchange_hierarchy_cursor(
        &self,
        account: &AccountId,
        mailbox: Option<&str>,
        cursor: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_mailbox_sync (account_id, mailbox, hierarchy_cursor,
                     last_sync_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (account_id, mailbox) DO UPDATE SET
                     hierarchy_cursor = excluded.hierarchy_cursor,
                     last_sync_at = excluded.last_sync_at",
                params![account.as_str(), mailbox_key(mailbox), cursor, now],
            )?;
            Ok(())
        })
    }

    // --- Mail folders and items ---------------------------------------------

    /// Stores a folder's Exchange data (the local folder must exist).
    pub fn upsert_exchange_folder(
        &self,
        account: &AccountId,
        record: &ExchangeFolderRecord,
    ) -> Result<()> {
        let well_known = record.well_known.as_ref().map(kebab).transpose()?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_folders (folder_id, account_id, mailbox, remote_id,
                     change_key, parent_remote_id, well_known, cursor, full_sync_page,
                     last_sync_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (folder_id) DO UPDATE SET
                     mailbox = excluded.mailbox, remote_id = excluded.remote_id,
                     change_key = excluded.change_key,
                     parent_remote_id = excluded.parent_remote_id,
                     well_known = excluded.well_known, cursor = excluded.cursor,
                     full_sync_page = excluded.full_sync_page,
                     last_sync_at = excluded.last_sync_at",
                params![
                    record.folder_id.as_str(),
                    account.as_str(),
                    mailbox_key(record.mailbox.as_deref()),
                    record.remote_id,
                    record.change_key,
                    record.parent_remote_id,
                    well_known,
                    record.cursor,
                    record.full_sync_page,
                    record.last_sync_at,
                ],
            )?;
            Ok(())
        })
    }

    /// Every Exchange folder of an account (own mailbox first).
    pub fn exchange_folders(&self, account: &AccountId) -> Result<Vec<ExchangeFolderRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {FOLDER_COLS} FROM exchange_folders WHERE account_id = ?1
                 ORDER BY mailbox, folder_id"
            ))?;
            let rows = stmt.query_map([account.as_str()], folder_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The Exchange data of a local folder.
    pub fn exchange_folder(&self, folder: &FolderId) -> Result<Option<ExchangeFolderRecord>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {FOLDER_COLS} FROM exchange_folders WHERE folder_id = ?1"),
                    [folder.as_str()],
                    folder_from_row,
                )
                .optional()?)
        })
    }

    /// The folder with this remote id in a mailbox.
    pub fn exchange_folder_by_remote(
        &self,
        account: &AccountId,
        mailbox: Option<&str>,
        remote_id: &str,
    ) -> Result<Option<ExchangeFolderRecord>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {FOLDER_COLS} FROM exchange_folders
                         WHERE account_id = ?1 AND mailbox = ?2 AND remote_id = ?3"
                    ),
                    params![account.as_str(), mailbox_key(mailbox), remote_id],
                    folder_from_row,
                )
                .optional()?)
        })
    }

    /// Stores a folder's items cursor (and an interrupted first sync).
    pub fn set_exchange_folder_cursor(
        &self,
        folder: &FolderId,
        cursor: Option<&str>,
        full_sync_page: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE exchange_folders SET cursor = ?2, full_sync_page = ?3, last_sync_at = ?4
                 WHERE folder_id = ?1",
                params![folder.as_str(), cursor, full_sync_page, now],
            )?;
            if changed == 0 {
                return Err(Error::not_found("Exchange folder", folder));
            }
            Ok(())
        })
    }

    /// Stores a message's Exchange extras (the message must exist).
    pub fn set_exchange_item(&self, id: &MessageId, record: &ExchangeItemRecord) -> Result<()> {
        self.with_conn(|conn| set_exchange_item_in(conn, id, record))
    }

    /// A message's Exchange extras as stored.
    pub fn exchange_item(&self, id: &MessageId) -> Result<Option<ExchangeItemRecord>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT mailbox, change_key, conversation_id, inference
                     FROM exchange_items WHERE message_id = ?1",
                    [id.as_str()],
                    |r| {
                        let inference: Option<String> = r.get(3)?;
                        Ok(ExchangeItemRecord {
                            mailbox: mailbox_of(r.get(0)?),
                            change_key: r.get(1)?,
                            conversation_id: r.get(2)?,
                            inference: inference
                                .as_deref()
                                .and_then(InferenceClassification::parse),
                        })
                    },
                )
                .optional()?)
        })
    }

    /// A message's Exchange data as summaries carry it
    /// ([`nail_api::MessageSummary::exchange`]).
    pub fn exchange_message_info(&self, id: &MessageId) -> Result<Option<ExchangeMessageInfo>> {
        self.with_conn(|conn| exchange_info_in(conn, id))
    }

    // --- PIM ------------------------------------------------------------------

    /// The account's Exchange PIM source, created on first use. Answers
    /// its id (`exchange:<account>`).
    pub fn ensure_exchange_pim_source(&self, account: &AccountId, now: i64) -> Result<PimSourceId> {
        let id = exchange_pim_source_id(account);
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO pim_sources (id, kind, account_id, created_at)
                 VALUES (?1, 'exchange', ?2, ?3)
                 ON CONFLICT (id) DO NOTHING",
                params![id.as_str(), account.as_str(), now],
            )?;
            Ok(())
        })?;
        debug_assert_eq!(PimSourceKind::Exchange.node_name(), None);
        Ok(id)
    }

    /// Stores a collection's Exchange data (the PIM collection must exist).
    pub fn upsert_exchange_pim_collection(
        &self,
        account: &AccountId,
        record: &ExchangePimCollectionRecord,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_pim_collections (collection_id, account_id, mailbox,
                     remote_id, change_key, role, todo, cursor, last_sync_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (collection_id) DO UPDATE SET
                     mailbox = excluded.mailbox, remote_id = excluded.remote_id,
                     change_key = excluded.change_key, role = excluded.role,
                     todo = excluded.todo, cursor = excluded.cursor,
                     last_sync_at = excluded.last_sync_at",
                params![
                    record.collection_id,
                    account.as_str(),
                    mailbox_key(record.mailbox.as_deref()),
                    record.remote_id,
                    record.change_key,
                    record.role.map(ExchangeCollectionRole::as_str),
                    record.todo,
                    record.cursor,
                    record.last_sync_at,
                ],
            )?;
            Ok(())
        })
    }

    /// Every Exchange collection of an account.
    pub fn exchange_pim_collections(
        &self,
        account: &AccountId,
    ) -> Result<Vec<ExchangePimCollectionRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {PIM_COLS} FROM exchange_pim_collections WHERE account_id = ?1
                 ORDER BY mailbox, collection_id"
            ))?;
            let rows = stmt.query_map([account.as_str()], pim_collection_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Stores a collection's items cursor.
    pub fn set_exchange_pim_cursor(
        &self,
        collection_id: &str,
        cursor: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE exchange_pim_collections SET cursor = ?2, last_sync_at = ?3
                 WHERE collection_id = ?1",
                params![collection_id, cursor, now],
            )?;
            if changed == 0 {
                return Err(Error::not_found("Exchange collection", collection_id));
            }
            Ok(())
        })
    }

    /// Stores the remote id and version of a PIM object (`pim_objects.id`).
    pub fn set_exchange_pim_item(
        &self,
        object_id: i64,
        remote_id: &str,
        change_key: Option<&str>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "INSERT INTO exchange_pim_items (object_id, collection_id, remote_id, change_key)
                 SELECT id, collection_id, ?2, ?3 FROM pim_objects WHERE id = ?1
                 ON CONFLICT (object_id) DO UPDATE SET
                     remote_id = excluded.remote_id, change_key = excluded.change_key",
                params![object_id, remote_id, change_key],
            )?;
            if changed == 0 {
                return Err(Error::not_found("PIM object", object_id));
            }
            Ok(())
        })
    }

    /// The PIM object with this remote id in a collection, with its
    /// version.
    pub fn exchange_pim_item_by_remote(
        &self,
        collection_id: &str,
        remote_id: &str,
    ) -> Result<Option<(i64, Option<String>)>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT object_id, change_key FROM exchange_pim_items
                     WHERE collection_id = ?1 AND remote_id = ?2",
                    params![collection_id, remote_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?)
        })
    }

    /// The remote id and version of a PIM object.
    pub fn exchange_pim_item(&self, object_id: i64) -> Result<Option<(String, Option<String>)>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT remote_id, change_key FROM exchange_pim_items WHERE object_id = ?1",
                    [object_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?)
        })
    }

    // --- Shared and delegate mailboxes ---------------------------------------

    /// The account's other mailboxes by name (hidden automapped ones only
    /// with `include_hidden`), with the calendars / address books shown.
    pub fn exchange_mailboxes(
        &self,
        account: &AccountId,
        include_hidden: bool,
    ) -> Result<Vec<ExchangeMailbox>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {MAILBOX_COLS} FROM exchange_mailboxes
                 WHERE account_id = ?1 AND (hidden = 0 OR ?2)
                 ORDER BY COALESCE(name, address) COLLATE NOCASE, address"
            ))?;
            let rows = stmt.query_map(params![account.as_str(), include_hidden], |r| {
                mailbox_from_row(account, r)
            })?;
            let mut mailboxes: Vec<(ExchangeMailbox, String)> =
                rows.collect::<rusqlite::Result<_>>()?;
            let mut stmt = conn.prepare_cached(
                "SELECT x.mailbox, c.id, c.kind FROM exchange_pim_collections x
                 JOIN pim_collections c ON c.id = x.collection_id
                 WHERE x.account_id = ?1 AND x.mailbox <> ''
                 ORDER BY c.id",
            )?;
            let collections = stmt.query_map([account.as_str()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            let mut by_mailbox: HashMap<String, Vec<(String, String)>> = HashMap::new();
            for row in collections {
                let (mailbox, id, kind) = row?;
                by_mailbox.entry(mailbox).or_default().push((id, kind));
            }
            for (mailbox, address) in &mut mailboxes {
                for (id, kind) in by_mailbox.remove(address.as_str()).unwrap_or_default() {
                    if kind == "address-book" {
                        mailbox.address_book_ids.push(AddressBookId(id));
                    } else {
                        mailbox.calendar_ids.push(CalendarId(id));
                    }
                }
            }
            Ok(mailboxes.into_iter().map(|(m, _)| m).collect())
        })
    }

    /// Adds or changes a mailbox (the address is stored lowercase; a hidden
    /// automapped one becomes visible again with these settings). Answers
    /// the stored mailbox.
    pub fn save_exchange_mailbox(
        &self,
        account: &AccountId,
        settings: &ExchangeMailboxSettings,
        origin: ExchangeMailboxOrigin,
        now: i64,
    ) -> Result<ExchangeMailbox> {
        let address = settings.address.trim().to_ascii_lowercase();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_mailboxes (account_id, address, name, kind, origin, mail,
                     calendar, contacts, tasks, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (account_id, address) DO UPDATE SET
                     name = excluded.name, kind = excluded.kind, mail = excluded.mail,
                     calendar = excluded.calendar, contacts = excluded.contacts,
                     tasks = excluded.tasks, hidden = 0",
                params![
                    account.as_str(),
                    address,
                    settings.name,
                    match settings.kind {
                        ExchangeMailboxKind::Shared => "shared",
                        ExchangeMailboxKind::Delegate => "delegate",
                    },
                    match origin {
                        ExchangeMailboxOrigin::User => "user",
                        ExchangeMailboxOrigin::Automapped => "automapped",
                    },
                    settings.mail,
                    settings.calendar,
                    settings.contacts,
                    settings.tasks,
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.exchange_mailboxes(account, true)?
            .into_iter()
            .find(|m| m.settings.address == address)
            .ok_or_else(|| Error::not_found("Exchange mailbox", address))
    }

    /// Removes a mailbox: an automapped one is hidden (Autodiscover would
    /// add it again), a user's is deleted. `false` when there is none.
    pub fn remove_exchange_mailbox(&self, account: &AccountId, address: &str) -> Result<bool> {
        let address = address.trim().to_ascii_lowercase();
        self.with_conn(|conn| {
            let hidden = conn.execute(
                "UPDATE exchange_mailboxes SET hidden = 1, state = 'pending',
                     root_folder_id = NULL
                 WHERE account_id = ?1 AND address = ?2 AND origin = 'automapped'",
                params![account.as_str(), address],
            )?;
            let deleted = conn.execute(
                "DELETE FROM exchange_mailboxes
                 WHERE account_id = ?1 AND address = ?2 AND origin = 'user'",
                params![account.as_str(), address],
            )?;
            Ok(hidden + deleted > 0)
        })
    }

    /// Adds automapped mailboxes Autodiscover listed that are not stored
    /// yet (hidden ones stay hidden). Answers the addresses added.
    pub fn merge_automapped_mailboxes(
        &self,
        account: &AccountId,
        found: &[ExchangeMailboxSettings],
        now: i64,
    ) -> Result<Vec<String>> {
        let mut added = Vec::new();
        self.with_tx(|tx| {
            for settings in found {
                let address = settings.address.trim().to_ascii_lowercase();
                let changed = tx.execute(
                    "INSERT INTO exchange_mailboxes (account_id, address, name, kind, origin,
                         mail, calendar, contacts, tasks, created_at)
                     VALUES (?1, ?2, ?3, ?4, 'automapped', ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT (account_id, address) DO NOTHING",
                    params![
                        account.as_str(),
                        address,
                        settings.name,
                        match settings.kind {
                            ExchangeMailboxKind::Shared => "shared",
                            ExchangeMailboxKind::Delegate => "delegate",
                        },
                        settings.mail,
                        settings.calendar,
                        settings.contacts,
                        settings.tasks,
                        now,
                    ],
                )?;
                if changed > 0 {
                    added.push(address);
                }
            }
            Ok(())
        })?;
        Ok(added)
    }

    /// Records a mailbox's reachability after a sync.
    pub fn set_exchange_mailbox_state(
        &self,
        account: &AccountId,
        address: &str,
        state: ExchangeMailboxState,
        root_folder: Option<&FolderId>,
        error: Option<&ApiError>,
    ) -> Result<()> {
        let error = api_error_json(error)?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE exchange_mailboxes SET state = ?3, root_folder_id = ?4, last_error = ?5
                 WHERE account_id = ?1 AND address = ?2",
                params![
                    account.as_str(),
                    address.trim().to_ascii_lowercase(),
                    state.as_str(),
                    root_folder.map(FolderId::as_str),
                    error,
                ],
            )?;
            if changed == 0 {
                return Err(Error::not_found("Exchange mailbox", address));
            }
            Ok(())
        })
    }

    // --- Categories -------------------------------------------------------------

    /// The account's categories as stored, by name.
    pub fn exchange_category_records(
        &self,
        account: &AccountId,
    ) -> Result<Vec<ExchangeCategoryRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT category_id, name, color, sync FROM exchange_categories
                 WHERE account_id = ?1 ORDER BY name COLLATE NOCASE, category_id",
            )?;
            let rows = stmt.query_map([account.as_str()], category_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Applies the server's master list: categories with local changes
    /// still queued (`pending`) are kept as they are; the rest is
    /// replaced.
    pub fn replace_exchange_categories(
        &self,
        account: &AccountId,
        server: &[ExchangeCategoryRecord],
    ) -> Result<()> {
        self.with_tx(|tx| {
            let pending: Vec<String> = {
                let mut stmt = tx.prepare(
                    "SELECT category_id FROM exchange_categories
                     WHERE account_id = ?1 AND sync = 'pending'",
                )?;
                let rows = stmt.query_map([account.as_str()], |r| r.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            tx.execute(
                "DELETE FROM exchange_categories WHERE account_id = ?1 AND sync = 'synced'",
                [account.as_str()],
            )?;
            for record in server {
                if pending.iter().any(|p| p == record.id.as_str()) {
                    continue;
                }
                let mut record = record.clone();
                record.sync = ExchangeSync::Synced;
                // A queued category with the same name wins over the
                // server's (it is being created / renamed right now).
                let clash: bool = tx.query_row(
                    "SELECT EXISTS (SELECT 1 FROM exchange_categories
                     WHERE account_id = ?1 AND name = ?2 COLLATE NOCASE)",
                    params![account.as_str(), record.name],
                    |r| r.get(0),
                )?;
                if !clash {
                    upsert_category_in_tx(tx, account, &record)?;
                }
            }
            Ok(())
        })
    }

    /// The category named `name` (case-insensitive).
    pub fn exchange_category_by_name(
        &self,
        account: &AccountId,
        name: &str,
    ) -> Result<Option<ExchangeCategoryRecord>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT category_id, name, color, sync FROM exchange_categories
                     WHERE account_id = ?1 AND name = ?2 COLLATE NOCASE",
                    params![account.as_str(), name],
                    category_from_row,
                )
                .optional()?)
        })
    }

    /// The master list with message counts of each category's tag, for
    /// [`nail_api::Request::ListExchangeCategories`].
    pub fn list_exchange_categories(&self, account: &AccountId) -> Result<Vec<ExchangeCategory>> {
        let records = self.exchange_category_records(account)?;
        let counts: HashMap<String, (u32, u32)> = self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT lower(t.tag), COUNT(*), SUM(m.seen = 0)
                 FROM message_tags t JOIN messages m ON m.id = t.message_id
                 WHERE m.account_id = ?1
                 GROUP BY lower(t.tag)",
            )?;
            let rows = stmt.query_map([account.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?)))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        Ok(records
            .into_iter()
            .map(|r| {
                let (total, unread) = counts
                    .get(&r.name.to_lowercase())
                    .copied()
                    .unwrap_or((0, 0));
                ExchangeCategory {
                    account_id: account.clone(),
                    id: r.id,
                    name: r.name,
                    color: r.color,
                    total,
                    unread,
                    sync: r.sync,
                }
            })
            .collect())
    }

    // --- Automatic replies ------------------------------------------------------

    /// The automatic replies as stored (defaults, never fetched, when
    /// there are none).
    pub fn exchange_automatic_replies(&self, account: &AccountId) -> Result<AutomaticReplies> {
        let row: Option<(String, String, Option<i64>)> = self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT nail_open('exchange_automatic_replies.settings', settings), sync,
                         fetched_at
                     FROM exchange_automatic_replies WHERE account_id = ?1",
                    [account.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?)
        })?;
        Ok(match row {
            None => AutomaticReplies {
                account_id: account.clone(),
                settings: AutomaticRepliesSettings::default(),
                sync: ExchangeSync::Synced,
                fetched_at: None,
            },
            Some((settings, sync, fetched_at)) => AutomaticReplies {
                account_id: account.clone(),
                settings: serde_json::from_str(&settings)?,
                sync: ExchangeSync::parse(&sync),
                fetched_at: fetched_at.and_then(rfc3339),
            },
        })
    }

    /// Stores the automatic replies read from the server (ignored while a
    /// local change is queued).
    pub fn set_exchange_automatic_replies(
        &self,
        account: &AccountId,
        settings: &AutomaticRepliesSettings,
        fetched_at: i64,
    ) -> Result<()> {
        let json = serde_json::to_string(settings)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_automatic_replies (account_id, settings, sync, fetched_at)
                 VALUES (?1, nail_seal('exchange_automatic_replies.settings', ?2), 'synced', ?3)
                 ON CONFLICT (account_id) DO UPDATE SET
                     settings = excluded.settings, fetched_at = excluded.fetched_at
                 WHERE exchange_automatic_replies.sync = 'synced'",
                params![account.as_str(), json, fetched_at],
            )?;
            Ok(())
        })
    }

    // --- Inbox rules --------------------------------------------------------------

    /// Applies the server's inbox rules (by sequence), keeping which Nail
    /// rule each was placed from; rules no longer on the server go.
    pub fn replace_exchange_rules(
        &self,
        account: &AccountId,
        rules: &[ExchangeInboxRule],
    ) -> Result<()> {
        self.with_tx(|tx| {
            let placed: HashMap<String, (Option<String>, Option<String>)> = {
                let mut stmt = tx.prepare(
                    "SELECT server_rule_id, rule_id, digest FROM exchange_rules
                     WHERE account_id = ?1",
                )?;
                let rows = stmt.query_map([account.as_str()], |r| {
                    Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?)))
                })?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            tx.execute(
                "DELETE FROM exchange_rules WHERE account_id = ?1",
                [account.as_str()],
            )?;
            for rule in rules {
                let (rule_id, digest) = placed.get(&rule.id).cloned().unwrap_or((
                    rule.nail_rule_id.as_ref().map(|r| r.as_str().to_owned()),
                    None,
                ));
                let mut stored = rule.clone();
                stored.nail_rule_id = None;
                tx.execute(
                    "INSERT INTO exchange_rules (account_id, server_rule_id, rule_id, digest,
                         sequence, data)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        account.as_str(),
                        rule.id,
                        rule_id,
                        digest,
                        rule.sequence,
                        serde_json::to_string(&stored)?,
                    ],
                )?;
            }
            Ok(())
        })
    }

    /// The stored inbox rules by sequence, with the Nail rule each was
    /// placed from.
    pub fn exchange_rules(&self, account: &AccountId) -> Result<Vec<ExchangeInboxRule>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT data, rule_id FROM exchange_rules WHERE account_id = ?1
                 ORDER BY sequence, server_rule_id",
            )?;
            let rows = stmt.query_map([account.as_str()], |r| {
                let data: String = r.get(0)?;
                let rule_id: Option<String> = r.get(1)?;
                let mut rule: ExchangeInboxRule =
                    serde_json::from_str(&data).map_err(|e| json_error(0, e))?;
                rule.nail_rule_id = rule_id.map(RuleId);
                Ok(rule)
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Records that a server rule was placed from a Nail rule (with the
    /// digest of what was placed), or clears it (`None`).
    pub fn set_exchange_rule_placement(
        &self,
        account: &AccountId,
        server_rule_id: &str,
        rule: Option<(&RuleId, &str)>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE exchange_rules SET rule_id = ?3, digest = ?4
                 WHERE account_id = ?1 AND server_rule_id = ?2",
                params![
                    account.as_str(),
                    server_rule_id,
                    rule.map(|(r, _)| r.as_str()),
                    rule.map(|(_, d)| d),
                ],
            )?;
            if changed == 0 {
                return Err(Error::not_found("Exchange inbox rule", server_rule_id));
            }
            Ok(())
        })
    }

    /// The digest of what was placed for a Nail rule, with the server
    /// rule's id.
    pub fn exchange_rule_placement(
        &self,
        account: &AccountId,
        rule: &RuleId,
    ) -> Result<Option<(String, Option<String>)>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT server_rule_id, digest FROM exchange_rules
                     WHERE account_id = ?1 AND rule_id = ?2",
                    params![account.as_str(), rule.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?)
        })
    }

    /// The rules state: state, last read (Unix seconds), last failure.
    pub fn exchange_rules_state(
        &self,
        account: &AccountId,
    ) -> Result<(ExchangeRulesState, Option<i64>, Option<ApiError>)> {
        self.with_conn(|conn| {
            let row: Option<(String, Option<i64>, Option<String>)> = conn
                .query_row(
                    "SELECT rules_state, rules_synced_at, rules_error FROM exchange_sync
                     WHERE account_id = ?1",
                    [account.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            Ok(match row {
                None => (ExchangeRulesState::Pending, None, None),
                Some((state, at, error)) => (
                    ExchangeRulesState::parse(&state).unwrap_or(ExchangeRulesState::Pending),
                    at,
                    parse_api_error(error),
                ),
            })
        })
    }

    /// Records the rules state (creating the sync row for `engine` when
    /// there is none).
    pub fn set_exchange_rules_state(
        &self,
        account: &AccountId,
        engine: ExchangeEngine,
        state: ExchangeRulesState,
        synced_at: Option<i64>,
        error: Option<&ApiError>,
    ) -> Result<()> {
        let error = api_error_json(error)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exchange_sync (account_id, engine, rules_state, rules_synced_at,
                     rules_error)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (account_id) DO UPDATE SET
                     rules_state = excluded.rules_state,
                     rules_synced_at = COALESCE(excluded.rules_synced_at, rules_synced_at),
                     rules_error = excluded.rules_error",
                params![
                    account.as_str(),
                    engine.as_str(),
                    state.as_str(),
                    synced_at,
                    error
                ],
            )?;
            Ok(())
        })
    }

    // --- Rooms ----------------------------------------------------------------------

    /// Replaces the cached room lists.
    pub fn replace_exchange_room_lists(
        &self,
        account: &AccountId,
        lists: &[RoomList],
        now: i64,
    ) -> Result<()> {
        self.with_tx(|tx| {
            tx.execute(
                "DELETE FROM exchange_rooms WHERE account_id = ?1 AND kind = 'room-list'",
                [account.as_str()],
            )?;
            for list in lists {
                tx.execute(
                    "INSERT OR REPLACE INTO exchange_rooms (account_id, kind, room_list, address,
                         data, fetched_at)
                     VALUES (?1, 'room-list', '', ?2, ?3, ?4)",
                    params![
                        account.as_str(),
                        list.address.to_ascii_lowercase(),
                        serde_json::to_string(list)?,
                        now
                    ],
                )?;
            }
            Ok(())
        })
    }

    /// The cached room lists by name, with when they were fetched (`None`
    /// = never).
    pub fn exchange_room_lists(&self, account: &AccountId) -> Result<(Vec<RoomList>, Option<i64>)> {
        self.cached_rooms(account, "room-list", "")
    }

    /// Replaces the cached rooms of a room list (`None` = every room).
    pub fn replace_exchange_rooms(
        &self,
        account: &AccountId,
        room_list: Option<&str>,
        rooms: &[Room],
        now: i64,
    ) -> Result<()> {
        let list = mailbox_key(room_list);
        self.with_tx(|tx| {
            tx.execute(
                "DELETE FROM exchange_rooms
                 WHERE account_id = ?1 AND kind = 'room' AND room_list = ?2",
                params![account.as_str(), list],
            )?;
            for room in rooms {
                tx.execute(
                    "INSERT OR REPLACE INTO exchange_rooms (account_id, kind, room_list, address,
                         data, fetched_at)
                     VALUES (?1, 'room', ?2, ?3, ?4, ?5)",
                    params![
                        account.as_str(),
                        list,
                        room.address.to_ascii_lowercase(),
                        serde_json::to_string(room)?,
                        now
                    ],
                )?;
            }
            Ok(())
        })
    }

    /// The cached rooms of a room list (`None` = every room) by name, with
    /// when they were fetched.
    pub fn exchange_rooms(
        &self,
        account: &AccountId,
        room_list: Option<&str>,
    ) -> Result<(Vec<Room>, Option<i64>)> {
        self.cached_rooms(account, "room", &mailbox_key(room_list))
    }

    fn cached_rooms<T: serde::de::DeserializeOwned + HasName>(
        &self,
        account: &AccountId,
        kind: &str,
        list: &str,
    ) -> Result<(Vec<T>, Option<i64>)> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT data, fetched_at FROM exchange_rooms
                 WHERE account_id = ?1 AND kind = ?2 AND room_list = ?3",
            )?;
            let rows = stmt.query_map(params![account.as_str(), kind, list], |r| {
                let data: String = r.get(0)?;
                let item: T = serde_json::from_str(&data).map_err(|e| json_error(0, e))?;
                Ok((item, r.get::<_, i64>(1)?))
            })?;
            let mut items: Vec<(T, i64)> = rows.collect::<rusqlite::Result<_>>()?;
            items.sort_by_key(|(item, _)| item.sort_key());
            let fetched = items.iter().map(|(_, at)| *at).min();
            Ok((items.into_iter().map(|(i, _)| i).collect(), fetched))
        })
    }

    // --- Operation queue ------------------------------------------------------------

    /// Applies an Exchange change locally and queues it for the server, in
    /// one transaction. A category create is stored under a new `local-…`
    /// id (answered as the op's target); a category update / delete
    /// changes the master list (message tags are left to the mail queue);
    /// automatic replies are stored as `pending`; rule changes update the
    /// stored rule; a classification updates the messages' Focused Inbox
    /// value.
    pub fn enqueue_exchange_op(
        &self,
        account: &AccountId,
        op: &ExchangeOp,
    ) -> Result<(Option<String>, OpId)> {
        self.with_tx(|tx| {
            let mut target = op.target().map(str::to_owned);
            match op {
                ExchangeOp::CreateCategory { settings } => {
                    let n: i64 = tx.query_row(
                        "SELECT COALESCE(MAX(id), 0) + 1 FROM exchange_ops",
                        [],
                        |r| r.get(0),
                    )?;
                    let id = format!("{LOCAL_CATEGORY_PREFIX}{n}");
                    upsert_category_in_tx(
                        tx,
                        account,
                        &ExchangeCategoryRecord {
                            id: ExchangeCategoryId::new(id.clone()),
                            name: settings.name.trim().to_owned(),
                            color: settings.color,
                            sync: ExchangeSync::Pending,
                        },
                    )?;
                    target = Some(id);
                }
                ExchangeOp::UpdateCategory {
                    category_id,
                    settings,
                } => {
                    let changed = tx.execute(
                        "UPDATE exchange_categories SET name = ?3, color = ?4, sync = 'pending'
                         WHERE account_id = ?1 AND category_id = ?2",
                        params![
                            account.as_str(),
                            category_id.as_str(),
                            settings.name.trim(),
                            settings.color.as_str()
                        ],
                    )?;
                    if changed == 0 {
                        return Err(Error::not_found("Exchange category", category_id));
                    }
                }
                ExchangeOp::DeleteCategory { category_id } => {
                    tx.execute(
                        "DELETE FROM exchange_categories WHERE account_id = ?1 AND category_id = ?2",
                        params![account.as_str(), category_id.as_str()],
                    )?;
                }
                ExchangeOp::SetAutomaticReplies { settings } => {
                    tx.execute(
                        "INSERT INTO exchange_automatic_replies (account_id, settings, sync)
                         VALUES (?1, nail_seal('exchange_automatic_replies.settings', ?2),
                             'pending')
                         ON CONFLICT (account_id) DO UPDATE SET
                             settings = excluded.settings, sync = 'pending'",
                        params![account.as_str(), serde_json::to_string(settings)?],
                    )?;
                }
                ExchangeOp::SetRuleEnabled {
                    server_rule_id,
                    enabled,
                } => {
                    let changed = tx.execute(
                        "UPDATE exchange_rules SET data = json_set(data, '$.enabled', json(?3))
                         WHERE account_id = ?1 AND server_rule_id = ?2",
                        params![
                            account.as_str(),
                            server_rule_id,
                            if *enabled { "true" } else { "false" }
                        ],
                    )?;
                    if changed == 0 {
                        return Err(Error::not_found("Exchange inbox rule", server_rule_id));
                    }
                }
                ExchangeOp::DeleteRule { server_rule_id } => {
                    tx.execute(
                        "DELETE FROM exchange_rules WHERE account_id = ?1 AND server_rule_id = ?2",
                        params![account.as_str(), server_rule_id],
                    )?;
                }
                ExchangeOp::Classify {
                    message_ids,
                    classification,
                    ..
                } => {
                    for id in message_ids {
                        let changed = tx.execute(
                            "INSERT INTO exchange_items (message_id, account_id, inference)
                             SELECT id, account_id, ?3 FROM messages
                             WHERE id = ?1 AND account_id = ?2
                             ON CONFLICT (message_id) DO UPDATE SET
                                 inference = excluded.inference",
                            params![id.as_str(), account.as_str(), classification.as_str()],
                        )?;
                        if changed == 0 {
                            return Err(Error::not_found("message", id));
                        }
                    }
                }
            }
            tx.execute(
                "INSERT INTO exchange_ops (account_id, kind, target, payload)
                 VALUES (?1, ?2, ?3, nail_seal('exchange_ops.payload', ?4))",
                params![
                    account.as_str(),
                    op.kind(),
                    target,
                    serde_json::to_string(op)?
                ],
            )?;
            Ok((target, exchange_op_id(tx.last_insert_rowid())))
        })
    }

    /// Pending Exchange operations of an account, in queue order.
    pub fn pending_exchange_ops(&self, account: &AccountId) -> Result<Vec<QueuedExchangeOp>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, account_id, target, nail_open('exchange_ops.payload', payload),
                     attempts
                 FROM exchange_ops WHERE account_id = ?1 AND state = 'pending' ORDER BY id",
            )?;
            let rows = stmt.query_map([account.as_str()], |r| {
                let payload: String = r.get(3)?;
                Ok(QueuedExchangeOp {
                    id: r.get(0)?,
                    account_id: AccountId(r.get(1)?),
                    target: r.get(2)?,
                    op: serde_json::from_str(&payload).map_err(|e| json_error(3, e))?,
                    attempts: r.get(4)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Number of pending Exchange operations of an account.
    pub fn pending_exchange_op_count(&self, account: &AccountId) -> Result<u32> {
        self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM exchange_ops WHERE account_id = ?1 AND state = 'pending'",
                [account.as_str()],
                |r| r.get(0),
            )?)
        })
    }

    /// Records how replaying an Exchange operation ended (see
    /// [`ExchangeOpOutcome`]). A finished category create — or an update
    /// that gave the category a new id (Graph renames by replacement) —
    /// moves the category (and later queued operations on it) to the
    /// server's id.
    pub fn finish_exchange_op(&self, op_id: i64, outcome: &ExchangeOpOutcome) -> Result<()> {
        self.with_tx(|tx| {
            let (account, kind, target): (String, String, Option<String>) = tx
                .query_row(
                    "SELECT account_id, kind, target FROM exchange_ops WHERE id = ?1",
                    [op_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| Error::not_found("Exchange operation", op_id))?;
            let (state, error) = match outcome {
                ExchangeOpOutcome::Done { .. } => ("done", None),
                ExchangeOpOutcome::Retry(e) => ("pending", Some(e.as_str())),
                ExchangeOpOutcome::Failed(e) => ("failed", Some(e.as_str())),
            };
            tx.execute(
                "UPDATE exchange_ops SET state = ?2, last_error = ?3, attempts = attempts + 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![op_id, state, error],
            )?;
            if matches!(outcome, ExchangeOpOutcome::Retry(_)) {
                return Ok(());
            }
            let mut target = target;
            if let (
                ExchangeOpOutcome::Done {
                    server_id: Some(server),
                },
                "create-category" | "update-category",
                Some(local),
            ) = (outcome, kind.as_str(), &target)
                && server != local
            {
                tx.execute(
                    "UPDATE exchange_categories SET category_id = ?3
                     WHERE account_id = ?1 AND category_id = ?2",
                    params![account, local, server],
                )?;
                tx.execute(
                    "UPDATE exchange_ops SET target = ?3
                     WHERE account_id = ?1 AND target = ?2 AND state = 'pending'",
                    params![account, local, server],
                )?;
                target = Some(server.clone());
            }
            if matches!(outcome, ExchangeOpOutcome::Failed(_)) && kind == "create-category" {
                tx.execute(
                    "DELETE FROM exchange_categories WHERE account_id = ?1 AND category_id = ?2",
                    params![account, target],
                )?;
                return Ok(());
            }
            match kind.as_str() {
                "create-category" | "update-category" => {
                    let queued: bool = tx.query_row(
                        "SELECT EXISTS (SELECT 1 FROM exchange_ops WHERE account_id = ?1
                         AND target = ?2 AND state = 'pending')",
                        params![account, target],
                        |r| r.get(0),
                    )?;
                    if !queued {
                        tx.execute(
                            "UPDATE exchange_categories SET sync = 'synced'
                             WHERE account_id = ?1 AND category_id = ?2",
                            params![account, target],
                        )?;
                    }
                }
                "set-automatic-replies" => {
                    let queued: bool = tx.query_row(
                        "SELECT EXISTS (SELECT 1 FROM exchange_ops WHERE account_id = ?1
                         AND kind = 'set-automatic-replies' AND state = 'pending')",
                        [&account],
                        |r| r.get(0),
                    )?;
                    if !queued {
                        tx.execute(
                            "UPDATE exchange_automatic_replies SET sync = 'synced'
                             WHERE account_id = ?1",
                            [&account],
                        )?;
                    }
                }
                _ => {}
            }
            Ok(())
        })
    }
}

/// Sort key of cached directory entries.
trait HasName {
    fn sort_key(&self) -> (String, String);
}

impl HasName for Room {
    fn sort_key(&self) -> (String, String) {
        (self.name.to_lowercase(), self.address.clone())
    }
}

impl HasName for RoomList {
    fn sort_key(&self) -> (String, String) {
        (self.name.to_lowercase(), self.address.clone())
    }
}

fn rfc3339(secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs, 0).map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// The Exchange data of a collection as [`nail_api::pim::Calendar::exchange`]
/// carries it, from the JSON the collection queries select.
pub(crate) fn collection_info_from_json(text: Option<String>) -> Option<ExchangeCollectionInfo> {
    let value: serde_json::Value = serde_json::from_str(&text?).ok()?;
    Some(ExchangeCollectionInfo {
        mailbox: value["mailbox"]
            .as_str()
            .filter(|m| !m.is_empty())
            .map(str::to_owned),
        role: value["role"]
            .as_str()
            .and_then(ExchangeCollectionRole::parse),
        todo: value["todo"].as_i64() == Some(1),
    })
}

/// [`Store::set_exchange_item`] on an open connection / transaction.
pub(crate) fn set_exchange_item_in(
    conn: &Connection,
    id: &MessageId,
    record: &ExchangeItemRecord,
) -> Result<()> {
    let inserted = conn
        .prepare_cached(
            "INSERT INTO exchange_items (message_id, account_id, mailbox, change_key,
                 conversation_id, inference)
             SELECT id, account_id, ?2, ?3, ?4, ?5 FROM messages WHERE id = ?1
             ON CONFLICT (message_id) DO UPDATE SET
                 mailbox = excluded.mailbox, change_key = excluded.change_key,
                 conversation_id = excluded.conversation_id,
                 inference = excluded.inference",
        )?
        .execute(params![
            id.as_str(),
            mailbox_key(record.mailbox.as_deref()),
            record.change_key,
            record.conversation_id,
            record.inference.map(InferenceClassification::as_str),
        ])?;
    if inserted == 0 {
        return Err(Error::not_found("message", id));
    }
    Ok(())
}

/// What an API engine (Gmail, Exchange) stores with a synced message in
/// the same transaction as the message itself
/// ([`Store::merge_with_remote`]), so no reader ever sees the message
/// without its server id or Exchange extras (M5c, ADR 0120).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteExtras {
    /// The message's id on its API server (`None` = leave it).
    pub remote_id: Option<String>,
    /// The Exchange extras (`None` = leave them).
    pub exchange: Option<ExchangeItemRecord>,
    /// The Gmail data (`None` = leave it; sets the server id too).
    pub gmail: Option<nail_api::gmail::GmailMessageInfo>,
}

/// Applies `extras` to the stored message `id` inside a transaction.
pub(crate) fn apply_remote_extras_in(
    conn: &Connection,
    id: &MessageId,
    extras: &RemoteExtras,
) -> Result<()> {
    if let Some(remote) = &extras.remote_id {
        crate::gmail::set_message_remote_id_in(conn, id, Some(remote))?;
    }
    if let Some(record) = &extras.exchange {
        set_exchange_item_in(conn, id, record)?;
    }
    if let Some(info) = &extras.gmail {
        crate::gmail::set_gmail_message_in(conn, id, info)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredMessage;
    use nail_api::exchange::AutomaticRepliesStatus;
    use nail_api::exchange::rules::{ExchangeRuleActions, ExchangeRuleConditions};
    use nail_api::pim::{CalendarComponent, CalendarSettings};
    use nail_api::{Account, AccountKind, Calendar, ErrorCode, Flags, Folder, MessageSummary};

    fn account() -> AccountId {
        AccountId::new("work")
    }

    fn store() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_account(&Account {
                id: account(),
                name: "Work".into(),
                email: "ada@contoso.example".into(),
                kind: AccountKind::Exchange,
            })
            .unwrap();
        store
            .upsert_folder(&Folder {
                id: "x-inbox".into(),
                account_id: account(),
                parent_id: None,
                name: "Inbox".into(),
                path: "Inbox".into(),
                role: nail_api::FolderRole::Inbox,
                unread: 0,
                total: 0,
            })
            .unwrap();
        for (id, seen, tags) in [
            ("m1", false, vec!["Red"]),
            ("m2", true, vec!["red", "Blue"]),
        ] {
            store
                .upsert_message(&StoredMessage::new(MessageSummary {
                    id: id.into(),
                    folder_id: "x-inbox".into(),
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

    #[test]
    fn sync_state_and_hierarchy_cursors() {
        let store = store();
        assert_eq!(store.exchange_sync(&account()).unwrap(), None);
        let record = ExchangeSyncRecord {
            engine: ExchangeEngine::Ews,
            server_version: Some("15.2.1544".into()),
            subscription_id: Some("sub".into()),
            watermark: Some("wm".into()),
            throttled_until: None,
            last_sync_at: Some(10),
        };
        store.set_exchange_sync(&account(), &record).unwrap();
        assert_eq!(store.exchange_sync(&account()).unwrap(), Some(record));
        assert_eq!(
            store.exchange_hierarchy_cursor(&account(), None).unwrap(),
            None
        );
        store
            .set_exchange_hierarchy_cursor(&account(), None, Some("h1"), 5)
            .unwrap();
        store
            .set_exchange_hierarchy_cursor(
                &account(),
                Some("Support@contoso.example"),
                Some("h2"),
                5,
            )
            .unwrap();
        assert_eq!(
            store
                .exchange_hierarchy_cursor(&account(), Some("support@contoso.example"))
                .unwrap()
                .as_deref(),
            Some("h2")
        );
        assert_eq!(
            store
                .exchange_hierarchy_cursor(&account(), None)
                .unwrap()
                .as_deref(),
            Some("h1")
        );
    }

    #[test]
    fn folders_items_and_focused_inbox() {
        let store = store();
        let mut folder = ExchangeFolderRecord::new("x-inbox", "AAMkInbox");
        folder.well_known = Some(ExchangeWellKnownFolder::Inbox);
        store.upsert_exchange_folder(&account(), &folder).unwrap();
        store
            .set_exchange_folder_cursor(&"x-inbox".into(), Some("delta-1"), None, 7)
            .unwrap();
        let stored = store
            .exchange_folder_by_remote(&account(), None, "AAMkInbox")
            .unwrap()
            .unwrap();
        assert_eq!(stored.cursor.as_deref(), Some("delta-1"));
        assert_eq!(stored.well_known, Some(ExchangeWellKnownFolder::Inbox));
        assert_eq!(store.exchange_folders(&account()).unwrap().len(), 1);
        assert!(
            store
                .set_exchange_folder_cursor(&"nope".into(), None, None, 1)
                .is_err()
        );

        store
            .set_message_remote_id(&"m1".into(), Some("AAMkM1"))
            .unwrap();
        store
            .set_exchange_item(
                &"m1".into(),
                &ExchangeItemRecord {
                    mailbox: None,
                    change_key: Some("CQAAA".into()),
                    conversation_id: Some("conv".into()),
                    inference: Some(InferenceClassification::Focused),
                },
            )
            .unwrap();
        let info = store.exchange_message_info(&"m1".into()).unwrap().unwrap();
        assert_eq!(info.item_id, "AAMkM1");
        assert_eq!(info.inference, Some(InferenceClassification::Focused));
        assert_eq!(info.mailbox, None);
        assert!(
            store
                .set_exchange_item(&"zz".into(), &ExchangeItemRecord::default())
                .is_err()
        );

        // Moving to Other is applied locally and queued.
        let (_, op) = store
            .enqueue_exchange_op(
                &account(),
                &ExchangeOp::Classify {
                    message_ids: vec!["m1".into(), "m2".into()],
                    classification: InferenceClassification::Other,
                    always: true,
                },
            )
            .unwrap();
        assert!(op.as_str().starts_with("exchange-"));
        for id in ["m1", "m2"] {
            assert_eq!(
                store.exchange_item(&id.into()).unwrap().unwrap().inference,
                Some(InferenceClassification::Other)
            );
        }
        let pending = store.pending_exchange_ops(&account()).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].op_id(), op);
        store
            .finish_exchange_op(pending[0].id, &ExchangeOpOutcome::Done { server_id: None })
            .unwrap();
        assert_eq!(store.pending_exchange_op_count(&account()).unwrap(), 0);

        // Deleting the message removes its Exchange row.
        store
            .with_conn(|c| Ok(c.execute("DELETE FROM messages WHERE id = 'm1'", [])?))
            .unwrap();
        assert_eq!(store.exchange_item(&"m1".into()).unwrap(), None);
    }

    #[test]
    fn pim_source_collections_and_items() {
        let store = store();
        let source = store.ensure_exchange_pim_source(&account(), 1).unwrap();
        assert_eq!(source.as_str(), "exchange:work");
        assert_eq!(
            store.ensure_exchange_pim_source(&account(), 2).unwrap(),
            source
        );
        let calendar = Calendar {
            id: CalendarId::new("x-tasks"),
            source_id: source.clone(),
            account_id: None,
            settings: CalendarSettings::named("Tasks"),
            components: vec![CalendarComponent::Task],
            read_only: false,
            timezone: None,
            exchange: None,
        };
        store.save_calendar(&calendar, None).unwrap();
        store
            .upsert_exchange_pim_collection(
                &account(),
                &ExchangePimCollectionRecord {
                    collection_id: "x-tasks".into(),
                    mailbox: None,
                    remote_id: "AQMkTasks".into(),
                    change_key: None,
                    role: Some(ExchangeCollectionRole::DefaultTasks),
                    todo: true,
                    cursor: None,
                    last_sync_at: None,
                },
            )
            .unwrap();
        // Calendars carry the Exchange data.
        let listed = store
            .calendar(&CalendarId::new("x-tasks"))
            .unwrap()
            .unwrap();
        let info = listed.exchange.unwrap();
        assert!(info.todo && info.mailbox.is_none());
        assert_eq!(info.role, Some(ExchangeCollectionRole::DefaultTasks));
        store
            .set_exchange_pim_cursor("x-tasks", Some("d1"), 3)
            .unwrap();
        assert_eq!(
            store.exchange_pim_collections(&account()).unwrap()[0]
                .cursor
                .as_deref(),
            Some("d1")
        );
        // Items keep their remote id and change key.
        let object: i64 = store
            .with_conn(|c| {
                c.execute(
                    "INSERT INTO pim_objects (collection_id, uid, kind, raw, updated_at)
                     VALUES ('x-tasks', 'u1', 'task', 'BEGIN:VCALENDAR', 1)",
                    [],
                )?;
                Ok(c.last_insert_rowid())
            })
            .unwrap();
        store
            .set_exchange_pim_item(object, "AAkTask1", Some("ck1"))
            .unwrap();
        assert_eq!(
            store
                .exchange_pim_item_by_remote("x-tasks", "AAkTask1")
                .unwrap(),
            Some((object, Some("ck1".into())))
        );
        assert_eq!(
            store.exchange_pim_item(object).unwrap(),
            Some(("AAkTask1".into(), Some("ck1".into())))
        );
        // Deleting the account removes the source and its collections.
        store
            .with_conn(|c| Ok(c.execute("DELETE FROM accounts WHERE id = 'work'", [])?))
            .unwrap();
        assert_eq!(store.calendar(&CalendarId::new("x-tasks")).unwrap(), None);
    }

    #[test]
    fn mailboxes_automapping_and_state() {
        let store = store();
        let mut support = ExchangeMailboxSettings::shared("Support@contoso.example");
        support.name = Some("Support".into());
        let saved = store
            .save_exchange_mailbox(&account(), &support, ExchangeMailboxOrigin::User, 1)
            .unwrap();
        assert_eq!(saved.settings.address, "support@contoso.example");
        assert_eq!(saved.state, ExchangeMailboxState::Pending);
        let boss = ExchangeMailboxSettings {
            kind: ExchangeMailboxKind::Delegate,
            mail: false,
            ..ExchangeMailboxSettings::shared("boss@contoso.example")
        };
        let added = store
            .merge_automapped_mailboxes(&account(), &[boss.clone(), support.clone()], 2)
            .unwrap();
        assert_eq!(added, ["boss@contoso.example"]);
        let listed = store.exchange_mailboxes(&account(), false).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].settings.address, "boss@contoso.example");
        assert_eq!(listed[0].origin, ExchangeMailboxOrigin::Automapped);
        // Removing an automapped mailbox hides it; merging does not bring
        // it back, adding it by hand does.
        assert!(
            store
                .remove_exchange_mailbox(&account(), "BOSS@contoso.example")
                .unwrap()
        );
        assert_eq!(
            store.exchange_mailboxes(&account(), false).unwrap().len(),
            1
        );
        assert!(
            store
                .merge_automapped_mailboxes(&account(), std::slice::from_ref(&boss), 3)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.exchange_mailboxes(&account(), true).unwrap().len(), 2);
        store
            .save_exchange_mailbox(&account(), &boss, ExchangeMailboxOrigin::User, 4)
            .unwrap();
        assert_eq!(
            store.exchange_mailboxes(&account(), false).unwrap().len(),
            2
        );
        // State and errors.
        let error = ApiError::new(ErrorCode::Forbidden, "no access");
        store
            .set_exchange_mailbox_state(
                &account(),
                "support@contoso.example",
                ExchangeMailboxState::AccessDenied,
                None,
                Some(&error),
            )
            .unwrap();
        let support = store
            .exchange_mailboxes(&account(), false)
            .unwrap()
            .into_iter()
            .find(|m| m.settings.address == "support@contoso.example")
            .unwrap();
        assert_eq!(support.state, ExchangeMailboxState::AccessDenied);
        assert_eq!(support.error, Some(ErrorCode::Forbidden));
        assert!(
            store
                .remove_exchange_mailbox(&account(), "support@contoso.example")
                .unwrap()
        );
        assert!(
            !store
                .remove_exchange_mailbox(&account(), "support@contoso.example")
                .unwrap()
        );
    }

    #[test]
    fn categories_with_counts_and_queue() {
        let store = store();
        store
            .replace_exchange_categories(
                &account(),
                &[ExchangeCategoryRecord {
                    id: "c-red".into(),
                    name: "Red".into(),
                    color: CategoryColor::Preset0,
                    sync: ExchangeSync::Synced,
                }],
            )
            .unwrap();
        let listed = store.list_exchange_categories(&account()).unwrap();
        assert_eq!(listed.len(), 1);
        // Tags compare case-insensitively like Outlook's categories.
        assert_eq!((listed[0].total, listed[0].unread), (2, 1));

        // Create: a local id, pending, then the server's id.
        let (local, op) = store
            .enqueue_exchange_op(
                &account(),
                &ExchangeOp::CreateCategory {
                    settings: ExchangeCategorySettings {
                        name: "Blue".into(),
                        color: CategoryColor::Preset7,
                    },
                },
            )
            .unwrap();
        let local = local.unwrap();
        assert!(local.starts_with(LOCAL_CATEGORY_PREFIX));
        assert!(op.as_str().starts_with("exchange-"));
        let blue = store
            .exchange_category_by_name(&account(), "blue")
            .unwrap()
            .unwrap();
        assert_eq!(blue.sync, ExchangeSync::Pending);
        // A server list without it keeps the pending one.
        store.replace_exchange_categories(&account(), &[]).unwrap();
        assert_eq!(
            store.exchange_category_records(&account()).unwrap().len(),
            1
        );
        let queued = store.pending_exchange_ops(&account()).unwrap();
        assert_eq!(queued[0].target.as_deref(), Some(local.as_str()));
        store
            .finish_exchange_op(
                queued[0].id,
                &ExchangeOpOutcome::Done {
                    server_id: Some("c-blue".into()),
                },
            )
            .unwrap();
        let blue = store
            .exchange_category_by_name(&account(), "Blue")
            .unwrap()
            .unwrap();
        assert_eq!(blue.id.as_str(), "c-blue");
        assert_eq!(blue.sync, ExchangeSync::Synced);

        // A failed create disappears.
        store
            .enqueue_exchange_op(
                &account(),
                &ExchangeOp::CreateCategory {
                    settings: ExchangeCategorySettings::named("Green"),
                },
            )
            .unwrap();
        let queued = store.pending_exchange_ops(&account()).unwrap();
        store
            .finish_exchange_op(queued[0].id, &ExchangeOpOutcome::Retry("busy".into()))
            .unwrap();
        assert_eq!(
            store.pending_exchange_ops(&account()).unwrap()[0].attempts,
            1
        );
        store
            .finish_exchange_op(queued[0].id, &ExchangeOpOutcome::Failed("no".into()))
            .unwrap();
        assert!(
            store
                .exchange_category_by_name(&account(), "Green")
                .unwrap()
                .is_none()
        );

        // Update and delete.
        store
            .enqueue_exchange_op(
                &account(),
                &ExchangeOp::UpdateCategory {
                    category_id: "c-blue".into(),
                    settings: ExchangeCategorySettings::named("Navy"),
                },
            )
            .unwrap();
        assert_eq!(
            store
                .exchange_category_by_name(&account(), "navy")
                .unwrap()
                .unwrap()
                .sync,
            ExchangeSync::Pending
        );
        // Graph renames by replacement: the update answers the new id,
        // which the category (and later queued ops on it) moves to.
        let rename = store.pending_exchange_ops(&account()).unwrap();
        assert_eq!(rename[0].target.as_deref(), Some("c-blue"));
        store
            .finish_exchange_op(
                rename[0].id,
                &ExchangeOpOutcome::Done {
                    server_id: Some("c-navy".into()),
                },
            )
            .unwrap();
        let navy = store
            .exchange_category_by_name(&account(), "Navy")
            .unwrap()
            .unwrap();
        assert_eq!(
            (navy.id.as_str(), navy.sync),
            ("c-navy", ExchangeSync::Synced)
        );
        store
            .enqueue_exchange_op(
                &account(),
                &ExchangeOp::DeleteCategory {
                    category_id: "c-navy".into(),
                },
            )
            .unwrap();
        assert!(
            store
                .exchange_category_by_name(&account(), "navy")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .enqueue_exchange_op(
                    &account(),
                    &ExchangeOp::UpdateCategory {
                        category_id: "gone".into(),
                        settings: ExchangeCategorySettings::named("X"),
                    },
                )
                .is_err()
        );
    }

    #[test]
    fn automatic_replies_are_queued_and_kept() {
        let store = store();
        let replies = store.exchange_automatic_replies(&account()).unwrap();
        assert_eq!(replies.fetched_at, None);
        assert_eq!(replies.settings.status, AutomaticRepliesStatus::Disabled);
        let server = AutomaticRepliesSettings {
            status: AutomaticRepliesStatus::AlwaysEnabled,
            internal_reply: "Out".into(),
            external_reply: "Out".into(),
            ..AutomaticRepliesSettings::default()
        };
        store
            .set_exchange_automatic_replies(&account(), &server, 1_790_000_000)
            .unwrap();
        let stored = store.exchange_automatic_replies(&account()).unwrap();
        assert_eq!(stored.settings, server);
        assert!(stored.fetched_at.unwrap().ends_with('Z'));
        // A local change is pending and wins over a server read until done.
        let local = AutomaticRepliesSettings::default();
        store
            .enqueue_exchange_op(
                &account(),
                &ExchangeOp::SetAutomaticReplies {
                    settings: local.clone(),
                },
            )
            .unwrap();
        store
            .set_exchange_automatic_replies(&account(), &server, 1_790_000_100)
            .unwrap();
        let stored = store.exchange_automatic_replies(&account()).unwrap();
        assert_eq!(stored.settings, local);
        assert_eq!(stored.sync, ExchangeSync::Pending);
        let op = store.pending_exchange_ops(&account()).unwrap().remove(0);
        store
            .finish_exchange_op(op.id, &ExchangeOpOutcome::Done { server_id: None })
            .unwrap();
        assert_eq!(
            store.exchange_automatic_replies(&account()).unwrap().sync,
            ExchangeSync::Synced
        );
    }

    #[test]
    fn inbox_rules_keep_their_placement() {
        let store = store();
        let rule = |id: &str, sequence: u32| ExchangeInboxRule {
            id: id.into(),
            name: id.into(),
            sequence,
            enabled: true,
            conditions: ExchangeRuleConditions::default(),
            exceptions: ExchangeRuleConditions::default(),
            actions: ExchangeRuleActions {
                mark_as_read: true,
                ..ExchangeRuleActions::default()
            },
            has_unsupported: false,
            in_error: false,
            nail_rule_id: None,
        };
        store
            .replace_exchange_rules(&account(), &[rule("r2", 2), rule("r1", 1)])
            .unwrap();
        store
            .set_exchange_rule_placement(&account(), "r2", Some((&"newsletters".into(), "d1")))
            .unwrap();
        // A re-read keeps the placement.
        store
            .replace_exchange_rules(&account(), &[rule("r1", 1), rule("r2", 2)])
            .unwrap();
        let rules = store.exchange_rules(&account()).unwrap();
        assert_eq!(rules[0].id, "r1");
        assert_eq!(rules[1].nail_rule_id, Some("newsletters".into()));
        assert_eq!(
            store
                .exchange_rule_placement(&account(), &"newsletters".into())
                .unwrap(),
            Some(("r2".into(), Some("d1".into())))
        );
        // Switching off / deleting is applied locally and queued.
        store
            .enqueue_exchange_op(
                &account(),
                &ExchangeOp::SetRuleEnabled {
                    server_rule_id: "r1".into(),
                    enabled: false,
                },
            )
            .unwrap();
        assert!(!store.exchange_rules(&account()).unwrap()[0].enabled);
        store
            .enqueue_exchange_op(
                &account(),
                &ExchangeOp::DeleteRule {
                    server_rule_id: "r1".into(),
                },
            )
            .unwrap();
        assert_eq!(store.exchange_rules(&account()).unwrap().len(), 1);
        assert_eq!(store.pending_exchange_op_count(&account()).unwrap(), 2);
        // The rules state.
        assert_eq!(
            store.exchange_rules_state(&account()).unwrap().0,
            ExchangeRulesState::Pending
        );
        store
            .set_exchange_rules_state(
                &account(),
                ExchangeEngine::Graph,
                ExchangeRulesState::InSync,
                Some(9),
                None,
            )
            .unwrap();
        let (state, at, error) = store.exchange_rules_state(&account()).unwrap();
        assert_eq!(
            (state, at, error),
            (ExchangeRulesState::InSync, Some(9), None)
        );
    }

    #[test]
    fn rooms_are_cached_per_list() {
        let store = store();
        assert_eq!(
            store.exchange_room_lists(&account()).unwrap(),
            (vec![], None)
        );
        store
            .replace_exchange_room_lists(
                &account(),
                &[RoomList {
                    address: "b1@contoso.example".into(),
                    name: "Building 1".into(),
                }],
                5,
            )
            .unwrap();
        let room = |name: &str| Room {
            address: format!("{}@contoso.example", name.to_lowercase()),
            name: name.into(),
            capacity: Some(8),
            building: Some("1".into()),
            floor: None,
            accessible: None,
            features: vec![],
            room_list: Some("b1@contoso.example".into()),
        };
        store
            .replace_exchange_rooms(
                &account(),
                Some("b1@contoso.example"),
                &[room("Zeta"), room("Alpha")],
                6,
            )
            .unwrap();
        let (rooms, at) = store
            .exchange_rooms(&account(), Some("B1@contoso.example"))
            .unwrap();
        assert_eq!(
            rooms.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Alpha", "Zeta"]
        );
        assert_eq!(at, Some(6));
        assert!(store.exchange_rooms(&account(), None).unwrap().0.is_empty());
        assert_eq!(store.exchange_room_lists(&account()).unwrap().0.len(), 1);
    }
}
