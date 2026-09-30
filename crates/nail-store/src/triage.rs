//! Schema v7 (M2c, ADR 0043): message body state (sync policies), snooze
//! wake times, the junk classifier's data, notification settings and the
//! Sieve state per account.
//!
//! Times are Unix seconds (UTC) in and out of the store; the core converts
//! to RFC 3339 for the API.

use std::collections::HashMap;

use nail_api::sieve::VacationSettings;
use nail_api::sync::BodyState;
use nail_api::triage::{
    JunkSettings, JunkSource, JunkVerdict, NotificationOverride, NotificationScope,
    NotificationSettings,
};
use nail_api::{AccountId, ApiError, FolderId, MessageId};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::store::Store;

/// `app_settings` key of the global notification settings (the overrides
/// live in `notify_overrides`).
pub const NOTIFICATION_SETTINGS_KEY: &str = "notifications";
/// `app_settings` key of the MCP settings (`nail_api::mcp::McpSettings`).
pub const MCP_SETTINGS_KEY: &str = "mcp";

/// The stored form of a junk token: the first 64 bits of its SHA-256, in
/// hex. The classifier's tables never hold the words themselves.
pub fn hash_junk_token(token: &str) -> String {
    hex::encode(&Sha256::digest(token.as_bytes())[..8])
}

fn is_token_hash(text: &str) -> bool {
    text.len() == 16
        && text
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

// ---------------------------------------------------------------------------
// Snooze

/// A snoozed message as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnoozeRecord {
    /// The message.
    pub message_id: MessageId,
    /// Its account.
    pub account_id: AccountId,
    /// Wake time (Unix seconds).
    pub until: i64,
    /// Where it returns to.
    pub wake_folder_id: FolderId,
    /// When it was snoozed (Unix seconds).
    pub snoozed_at: i64,
    /// Its `Message-ID` header, to find it again after a resync.
    pub rfc_message_id: Option<String>,
}

const SNOOZE_COLS: &str =
    "message_id, account_id, until, wake_folder_id, snoozed_at, rfc_message_id";

fn snooze_from_row(row: &Row<'_>) -> rusqlite::Result<SnoozeRecord> {
    Ok(SnoozeRecord {
        message_id: MessageId(row.get(0)?),
        account_id: AccountId(row.get(1)?),
        until: row.get(2)?,
        wake_folder_id: FolderId(row.get(3)?),
        snoozed_at: row.get(4)?,
        rfc_message_id: row.get(5)?,
    })
}

// ---------------------------------------------------------------------------
// Junk

/// Classifier counters of one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JunkStats {
    /// Messages trained as junk.
    pub trained_junk: u32,
    /// Messages trained as not junk.
    pub trained_ham: u32,
    /// Distinct (hashed) tokens.
    pub tokens: u32,
    /// Last training (Unix seconds).
    pub last_trained_at: Option<i64>,
}

/// What [`Store::train_junk`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainOutcome {
    /// First training of this message.
    Trained,
    /// The message was trained as the other class before; it moved.
    Retrained,
    /// Already trained as this class; nothing changed.
    Unchanged,
}

/// Token counts: `(junk, ham)` messages a hashed token occurred in.
pub type TokenCounts = HashMap<String, (u32, u32)>;

fn source_str(source: JunkSource) -> &'static str {
    match source {
        JunkSource::Server => "server",
        JunkSource::Classifier => "classifier",
        JunkSource::User => "user",
    }
}

fn parse_source(text: &str) -> JunkSource {
    match text {
        "server" => JunkSource::Server,
        "classifier" => JunkSource::Classifier,
        _ => JunkSource::User,
    }
}

// ---------------------------------------------------------------------------
// Sieve

/// The Sieve state of one account as stored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SieveRecord {
    /// SHA-256 (hex) of the uploaded script, `None` = never uploaded.
    pub script_hash: Option<String>,
    /// When it was uploaded (Unix seconds).
    pub uploaded_at: Option<i64>,
    /// The server's Sieve extensions, as last seen.
    pub extensions: Vec<String>,
    /// Other scripts on the server, as last seen.
    pub other_scripts: Vec<String>,
    /// The vacation settings.
    pub vacation: VacationSettings,
    /// The last failure, if the last attempt failed.
    pub last_error: Option<ApiError>,
}

/// SHA-256 (hex) of a Sieve script, as recorded by
/// [`Store::record_sieve_upload`].
pub fn hash_sieve_script(script: &str) -> String {
    hex::encode(Sha256::digest(script.as_bytes()))
}

#[derive(Serialize, Deserialize)]
struct StoredNotificationSettings {
    enabled: bool,
    show_preview: bool,
    sound: bool,
    folders: nail_api::triage::NotifyFolders,
    quiet_hours: Option<nail_api::triage::QuietHours>,
}

impl Store {
    // --- Body state --------------------------------------------------------

    /// Where a message's body is.
    pub fn body_state(&self, id: &MessageId) -> Result<BodyState> {
        self.with_conn(|conn| {
            let text: Option<String> = conn
                .query_row(
                    "SELECT body_state FROM messages WHERE id = ?1",
                    [id.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            let text = text.ok_or_else(|| Error::not_found("message", id))?;
            BodyState::parse(&text)
                .ok_or_else(|| Error::InvalidData(format!("unknown body state {text:?}")))
        })
    }

    /// Sets the body state of messages (unknown ids are skipped). Returns
    /// how many rows changed.
    pub fn set_body_state(&self, ids: &[MessageId], state: BodyState) -> Result<usize> {
        self.with_tx(|tx| {
            let mut stmt =
                tx.prepare_cached("UPDATE messages SET body_state = ?2 WHERE id = ?1")?;
            let mut changed = 0;
            for id in ids {
                changed += stmt.execute(params![id.as_str(), state.as_str()])?;
            }
            Ok(changed)
        })
    }

    /// Messages of a folder whose body is not stored (`headers-only`),
    /// newest first.
    pub fn headers_only_messages(&self, folder: &FolderId, limit: u32) -> Result<Vec<MessageId>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id FROM messages WHERE folder_id = ?1 AND body_state = 'headers-only'
                 ORDER BY date_sort DESC, id DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![folder.as_str(), limit], |r| {
                r.get::<_, String>(0).map(MessageId)
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    // --- Snooze --------------------------------------------------------------

    /// Records (or replaces) a snooze. The message must exist; its account
    /// and `Message-ID` are taken from its row.
    pub fn snooze(
        &self,
        message: &MessageId,
        until: i64,
        wake_folder: &FolderId,
        now: i64,
    ) -> Result<SnoozeRecord> {
        if until <= now {
            return Err(Error::InvalidData(
                "a snooze must end in the future".to_owned(),
            ));
        }
        self.with_tx(|tx| {
            let row: Option<(String, Option<String>)> = tx
                .query_row(
                    "SELECT account_id, message_id FROM messages WHERE id = ?1",
                    [message.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let (account, rfc) = row.ok_or_else(|| Error::not_found("message", message))?;
            let folder_account: Option<String> = tx
                .query_row(
                    "SELECT account_id FROM folders WHERE id = ?1",
                    [wake_folder.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            if folder_account.as_deref() != Some(account.as_str()) {
                return Err(Error::InvalidData(format!(
                    "wake folder {wake_folder} is not a folder of account {account}"
                )));
            }
            tx.execute(
                "INSERT INTO snoozed_messages (message_id, account_id, until, wake_folder_id,
                     snoozed_at, rfc_message_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (message_id) DO UPDATE SET until = excluded.until,
                     wake_folder_id = excluded.wake_folder_id, snoozed_at = excluded.snoozed_at",
                params![
                    message.as_str(),
                    account,
                    until,
                    wake_folder.as_str(),
                    now,
                    rfc
                ],
            )?;
            Ok(tx.query_row(
                &format!("SELECT {SNOOZE_COLS} FROM snoozed_messages WHERE message_id = ?1"),
                [message.as_str()],
                snooze_from_row,
            )?)
        })
    }

    /// Removes snoozes (woken or cancelled). Returns the removed records.
    pub fn unsnooze(&self, messages: &[MessageId]) -> Result<Vec<SnoozeRecord>> {
        self.with_tx(|tx| {
            let mut out = Vec::new();
            for id in messages {
                if let Some(record) = tx
                    .query_row(
                        &format!(
                            "SELECT {SNOOZE_COLS} FROM snoozed_messages WHERE message_id = ?1"
                        ),
                        [id.as_str()],
                        snooze_from_row,
                    )
                    .optional()?
                {
                    tx.execute(
                        "DELETE FROM snoozed_messages WHERE message_id = ?1",
                        [id.as_str()],
                    )?;
                    out.push(record);
                }
            }
            Ok(out)
        })
    }

    /// Snoozes, soonest first; `account` = one account's only.
    pub fn list_snoozed(&self, account: Option<&AccountId>) -> Result<Vec<SnoozeRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {SNOOZE_COLS} FROM snoozed_messages
                 WHERE ?1 IS NULL OR account_id = ?1 ORDER BY until, message_id"
            ))?;
            let rows = stmt.query_map([account.map(AccountId::as_str)], snooze_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Snoozes due at `now` (wake time reached), soonest first.
    pub fn due_snoozes(&self, now: i64) -> Result<Vec<SnoozeRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {SNOOZE_COLS} FROM snoozed_messages WHERE until <= ?1
                 ORDER BY until, message_id"
            ))?;
            let rows = stmt.query_map([now], snooze_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The next wake time, for the core's timer.
    pub fn next_snooze_wake(&self) -> Result<Option<i64>> {
        self.with_conn(|conn| {
            Ok(conn.query_row("SELECT MIN(until) FROM snoozed_messages", [], |r| r.get(0))?)
        })
    }

    /// Points a snooze at a message's new row (after a resync gave it a new
    /// id). Returns whether a snooze moved.
    pub fn rebind_snooze(&self, old: &MessageId, new: &MessageId) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE snoozed_messages SET message_id = ?2 WHERE message_id = ?1",
                params![old.as_str(), new.as_str()],
            )? > 0)
        })
    }

    // --- Junk ----------------------------------------------------------------

    /// An account's junk settings (the defaults when never saved).
    pub fn junk_settings(&self, account: &AccountId) -> Result<JunkSettings> {
        self.with_conn(|conn| {
            let text: Option<String> = conn
                .query_row(
                    "SELECT settings FROM junk_settings WHERE account_id = ?1",
                    [account.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(text
                .map(|t| serde_json::from_str(&t))
                .transpose()?
                .unwrap_or_default())
        })
    }

    /// Saves an account's junk settings (refused when invalid).
    pub fn set_junk_settings(&self, account: &AccountId, settings: &JunkSettings) -> Result<()> {
        if let Some(issue) = settings.issues().first() {
            return Err(Error::InvalidData(format!(
                "invalid junk settings: {}",
                issue.message_id()
            )));
        }
        let text = serde_json::to_string(settings)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO junk_settings (account_id, settings) VALUES (?1, ?2)
                 ON CONFLICT (account_id) DO UPDATE SET settings = excluded.settings",
                params![account.as_str(), text],
            )?;
            Ok(())
        })
    }

    /// Counts of (plain) tokens in an account's classifier; tokens it has
    /// never seen are absent. Keys are the plain tokens as given.
    pub fn junk_token_counts(&self, account: &AccountId, tokens: &[String]) -> Result<TokenCounts> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT junk, ham FROM junk_tokens WHERE account_id = ?1 AND token = ?2",
            )?;
            let mut out = HashMap::new();
            for token in tokens {
                if out.contains_key(token) {
                    continue;
                }
                let hashed = hash_junk_token(token);
                if let Some(counts) = stmt
                    .query_row(params![account.as_str(), hashed], |r| {
                        Ok((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?))
                    })
                    .optional()?
                {
                    out.insert(token.clone(), counts);
                }
            }
            Ok(out)
        })
    }

    /// Trains the classifier with one message (identified by `message_key`:
    /// its `Message-ID`, else `local:<id>`) as junk or not junk. Each
    /// distinct token counts once per message. A message trained as the
    /// other class before is untrained from it first.
    pub fn train_junk(
        &self,
        account: &AccountId,
        message_key: &str,
        junk: bool,
        tokens: &[String],
        now: i64,
    ) -> Result<TrainOutcome> {
        if message_key.is_empty() {
            return Err(Error::InvalidData("empty message key".to_owned()));
        }
        let mut hashed: Vec<String> = tokens.iter().map(|t| hash_junk_token(t)).collect();
        hashed.sort_unstable();
        hashed.dedup();
        self.with_tx(|tx| {
            let previous: Option<(bool, String)> = tx
                .query_row(
                    "SELECT junk, tokens FROM junk_training
                     WHERE account_id = ?1 AND message_key = ?2",
                    params![account.as_str(), message_key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let outcome = match &previous {
                Some((was, _)) if *was == junk => return Ok(TrainOutcome::Unchanged),
                Some(_) => TrainOutcome::Retrained,
                None => TrainOutcome::Trained,
            };
            if let Some((was_junk, old)) = previous {
                let old: Vec<String> = serde_json::from_str(&old)?;
                let column = if was_junk { "junk" } else { "ham" };
                let mut stmt = tx.prepare_cached(&format!(
                    "UPDATE junk_tokens SET {column} = MAX({column} - 1, 0)
                     WHERE account_id = ?1 AND token = ?2"
                ))?;
                for token in old.iter().filter(|t| is_token_hash(t)) {
                    stmt.execute(params![account.as_str(), token])?;
                }
                tx.execute(
                    "DELETE FROM junk_tokens WHERE account_id = ?1 AND junk = 0 AND ham = 0",
                    [account.as_str()],
                )?;
            }
            let (junk_inc, ham_inc) = if junk { (1, 0) } else { (0, 1) };
            let mut stmt = tx.prepare_cached(
                "INSERT INTO junk_tokens (account_id, token, junk, ham) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (account_id, token) DO UPDATE SET junk = junk + excluded.junk,
                     ham = ham + excluded.ham",
            )?;
            for token in &hashed {
                stmt.execute(params![account.as_str(), token, junk_inc, ham_inc])?;
            }
            tx.execute(
                "INSERT INTO junk_training (account_id, message_key, junk, tokens, trained_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (account_id, message_key) DO UPDATE SET junk = excluded.junk,
                     tokens = excluded.tokens, trained_at = excluded.trained_at",
                params![
                    account.as_str(),
                    message_key,
                    junk,
                    serde_json::to_string(&hashed)?,
                    now
                ],
            )?;
            Ok(outcome)
        })
    }

    /// Forgets the training of one message (undoing a first "Mark as junk /
    /// not junk"): removes its training row and lowers each of its tokens'
    /// counts of that class by one (never below zero), dropping tokens left
    /// at 0 / 0, in one transaction. Returns whether it was trained.
    pub fn untrain_junk(&self, account: &AccountId, message_key: &str) -> Result<bool> {
        self.with_tx(|tx| {
            let previous: Option<(bool, String)> = tx
                .query_row(
                    "SELECT junk, tokens FROM junk_training
                     WHERE account_id = ?1 AND message_key = ?2",
                    params![account.as_str(), message_key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((was_junk, old)) = previous else {
                return Ok(false);
            };
            let old: Vec<String> = serde_json::from_str(&old)?;
            let column = if was_junk { "junk" } else { "ham" };
            let mut stmt = tx.prepare_cached(&format!(
                "UPDATE junk_tokens SET {column} = MAX({column} - 1, 0)
                 WHERE account_id = ?1 AND token = ?2"
            ))?;
            for token in old.iter().filter(|t| is_token_hash(t)) {
                stmt.execute(params![account.as_str(), token])?;
            }
            tx.execute(
                "DELETE FROM junk_tokens WHERE account_id = ?1 AND junk = 0 AND ham = 0",
                [account.as_str()],
            )?;
            tx.execute(
                "DELETE FROM junk_training WHERE account_id = ?1 AND message_key = ?2",
                params![account.as_str(), message_key],
            )?;
            Ok(true)
        })
    }

    /// The classifier's counters for an account.
    pub fn junk_stats(&self, account: &AccountId) -> Result<JunkStats> {
        self.with_conn(|conn| {
            let (trained_junk, trained_ham, last): (u32, u32, Option<i64>) = conn.query_row(
                "SELECT COALESCE(SUM(junk), 0), COALESCE(SUM(1 - junk), 0), MAX(trained_at)
                 FROM junk_training WHERE account_id = ?1",
                [account.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let tokens: u32 = conn.query_row(
                "SELECT COUNT(*) FROM junk_tokens WHERE account_id = ?1",
                [account.as_str()],
                |r| r.get(0),
            )?;
            Ok(JunkStats {
                trained_junk,
                trained_ham,
                tokens,
                last_trained_at: last,
            })
        })
    }

    /// Forgets everything an account's classifier learned (settings and
    /// verdicts stay).
    pub fn reset_junk(&self, account: &AccountId) -> Result<()> {
        self.with_tx(|tx| {
            tx.execute(
                "DELETE FROM junk_tokens WHERE account_id = ?1",
                [account.as_str()],
            )?;
            tx.execute(
                "DELETE FROM junk_training WHERE account_id = ?1",
                [account.as_str()],
            )?;
            Ok(())
        })
    }

    /// Records a message's junk verdict (replacing an earlier one).
    pub fn set_junk_verdict(
        &self,
        message: &MessageId,
        verdict: &JunkVerdict,
        now: i64,
    ) -> Result<()> {
        if verdict.score.is_some_and(|s| s > 100) {
            return Err(Error::InvalidData("a junk score is at most 100".to_owned()));
        }
        self.with_conn(|conn| {
            let changed = conn.execute(
                "INSERT INTO junk_verdicts (message_id, junk, score, source, decided_at)
                 SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM messages WHERE id = ?1)
                 ON CONFLICT (message_id) DO UPDATE SET junk = excluded.junk,
                     score = excluded.score, source = excluded.source,
                     decided_at = excluded.decided_at",
                params![
                    message.as_str(),
                    verdict.junk,
                    verdict.score,
                    source_str(verdict.source),
                    now
                ],
            )?;
            if changed == 0 {
                return Err(Error::not_found("message", message));
            }
            Ok(())
        })
    }

    /// A message's junk verdict, if any.
    pub fn junk_verdict(&self, message: &MessageId) -> Result<Option<JunkVerdict>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT junk, score, source FROM junk_verdicts WHERE message_id = ?1",
                    [message.as_str()],
                    |r| {
                        Ok(JunkVerdict {
                            junk: r.get(0)?,
                            score: r.get(1)?,
                            source: parse_source(&r.get::<_, String>(2)?),
                        })
                    },
                )
                .optional()?)
        })
    }

    // --- Notifications -------------------------------------------------------

    /// The notification settings (the defaults when never saved).
    pub fn notification_settings(&self) -> Result<NotificationSettings> {
        let stored: Option<StoredNotificationSettings> = self.setting(NOTIFICATION_SETTINGS_KEY)?;
        let overrides = self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT scope, target_id, notify FROM notify_overrides ORDER BY position",
            )?;
            let rows = stmt.query_map([], |r| {
                let scope: String = r.get(0)?;
                let target: String = r.get(1)?;
                Ok(NotificationOverride {
                    scope: if scope == "account" {
                        NotificationScope::Account {
                            account_id: AccountId(target),
                        }
                    } else {
                        NotificationScope::Folder {
                            folder_id: FolderId(target),
                        }
                    },
                    notify: r.get(2)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        let mut settings = NotificationSettings::default();
        if let Some(s) = stored {
            settings.enabled = s.enabled;
            settings.show_preview = s.show_preview;
            settings.sound = s.sound;
            settings.folders = s.folders;
            settings.quiet_hours = s.quiet_hours;
        }
        settings.overrides = overrides;
        Ok(settings)
    }

    /// Saves the notification settings (refused when invalid). Overrides
    /// for accounts or folders that do not exist are refused too.
    pub fn set_notification_settings(&self, settings: &NotificationSettings) -> Result<()> {
        if let Some(issue) = settings.issues().first() {
            return Err(Error::InvalidData(format!(
                "invalid notification settings: {}",
                issue.message_id()
            )));
        }
        let stored = serde_json::to_string(&StoredNotificationSettings {
            enabled: settings.enabled,
            show_preview: settings.show_preview,
            sound: settings.sound,
            folders: settings.folders,
            quiet_hours: settings.quiet_hours.clone(),
        })?;
        self.with_tx(|tx| {
            tx.execute("DELETE FROM notify_overrides", [])?;
            for (position, o) in settings.overrides.iter().enumerate() {
                let (scope, target, table) = match &o.scope {
                    NotificationScope::Account { account_id } => {
                        ("account", account_id.as_str(), "accounts")
                    }
                    NotificationScope::Folder { folder_id } => {
                        ("folder", folder_id.as_str(), "folders")
                    }
                };
                let exists: bool = tx.query_row(
                    &format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE id = ?1)"),
                    [target],
                    |r| r.get(0),
                )?;
                if !exists {
                    return Err(Error::not_found(
                        if scope == "account" {
                            "account"
                        } else {
                            "folder"
                        },
                        target,
                    ));
                }
                tx.execute(
                    "INSERT INTO notify_overrides (scope, target_id, notify, position)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![scope, target, o.notify, position as i64],
                )?;
            }
            tx.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![NOTIFICATION_SETTINGS_KEY, stored],
            )?;
            Ok(())
        })
    }

    // --- Sieve ---------------------------------------------------------------

    /// The Sieve state of an account (defaults when nothing was recorded).
    pub fn sieve_record(&self, account: &AccountId) -> Result<SieveRecord> {
        self.with_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT script_hash, uploaded_at, extensions, other_scripts, vacation,
                         last_error
                     FROM sieve_state WHERE account_id = ?1",
                    [account.as_str()],
                    |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<i64>>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, Option<String>>(4)?,
                            r.get::<_, Option<String>>(5)?,
                        ))
                    },
                )
                .optional()?;
            let Some((hash, uploaded, ext, other, vacation, error)) = row else {
                return Ok(SieveRecord::default());
            };
            Ok(SieveRecord {
                script_hash: hash,
                uploaded_at: uploaded,
                extensions: serde_json::from_str(&ext)?,
                other_scripts: serde_json::from_str(&other)?,
                vacation: vacation
                    .map(|v| serde_json::from_str(&v))
                    .transpose()?
                    .unwrap_or_default(),
                last_error: error.map(|e| serde_json::from_str(&e)).transpose()?,
            })
        })
    }

    fn ensure_sieve_row(tx: &rusqlite::Transaction<'_>, account: &AccountId) -> Result<()> {
        let exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM accounts WHERE id = ?1)",
            [account.as_str()],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(Error::not_found("account", account));
        }
        tx.execute(
            "INSERT INTO sieve_state (account_id) VALUES (?1) ON CONFLICT DO NOTHING",
            [account.as_str()],
        )?;
        Ok(())
    }

    /// Saves an account's vacation settings (refused when invalid).
    pub fn set_vacation(&self, account: &AccountId, vacation: &VacationSettings) -> Result<()> {
        if let Some(issue) = vacation.issues().first() {
            return Err(Error::InvalidData(format!(
                "invalid vacation settings: {}",
                issue.message_id()
            )));
        }
        let text = serde_json::to_string(vacation)?;
        self.with_tx(|tx| {
            Self::ensure_sieve_row(tx, account)?;
            tx.execute(
                "UPDATE sieve_state SET vacation = ?2 WHERE account_id = ?1",
                params![account.as_str(), text],
            )?;
            Ok(())
        })
    }

    /// Records a successful upload of `script` (clears the last error).
    pub fn record_sieve_upload(
        &self,
        account: &AccountId,
        script: &str,
        extensions: &[String],
        other_scripts: &[String],
        now: i64,
    ) -> Result<()> {
        let hash = hash_sieve_script(script);
        let ext = serde_json::to_string(extensions)?;
        let other = serde_json::to_string(other_scripts)?;
        self.with_tx(|tx| {
            Self::ensure_sieve_row(tx, account)?;
            tx.execute(
                "UPDATE sieve_state SET script_hash = ?2, uploaded_at = ?3, extensions = ?4,
                     other_scripts = ?5, last_error = NULL
                 WHERE account_id = ?1",
                params![account.as_str(), hash, now, ext, other],
            )?;
            Ok(())
        })
    }

    /// Forgets the recorded upload (Nail's script was removed from the
    /// server, or ManageSieve was switched off); keeps the vacation
    /// settings.
    pub fn clear_sieve_upload(&self, account: &AccountId) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE sieve_state SET script_hash = NULL, uploaded_at = NULL, last_error = NULL
                 WHERE account_id = ?1",
                [account.as_str()],
            )?;
            Ok(())
        })
    }

    /// Records a failed upload or connection (`None` clears it).
    pub fn record_sieve_error(&self, account: &AccountId, error: Option<&ApiError>) -> Result<()> {
        let text = error.map(serde_json::to_string).transpose()?;
        self.with_tx(|tx| {
            Self::ensure_sieve_row(tx, account)?;
            tx.execute(
                "UPDATE sieve_state SET last_error = ?2 WHERE account_id = ?1",
                params![account.as_str(), text],
            )?;
            Ok(())
        })
    }
}
