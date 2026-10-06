//! Scheduling state (schema v11, M3b, ADR 0065): what the core needs to
//! process iTIP messages correctly and to show the invitation card.
//!
//! * [`ItipStateRecord`] — per event (`UID` + recurrence id) and address:
//!   the newest `SEQUENCE` / `DTSTAMP` applied and the answer.
//!   [`Store::record_itip_state`] only moves forward ([`itip_is_newer`]),
//!   so an older reply or update arriving late changes nothing.
//! * [`ItipMessageRecord`] — every received iTIP message and its outcome.
//! * [`SentItipRecord`] — every iTIP message Nail sent.
//! * The scheduling settings ([`Store::scheduling_settings`]) live in the
//!   app settings.
//!
//! Addresses are stored lowercase; recurrence ids as the JSON of the
//! `nail_api` `PimTime` (`""` = the whole event), like the instance cache.

use nail_api::pim::calendar::ParticipationStatus;
use nail_api::scheduling::{ItipMethod, SchedulingSettings, SchedulingTransport};
use nail_api::{AccountId, EventId, IdentityId, MessageId, OutboxId};
use rusqlite::{OptionalExtension, Row, params};

use crate::error::{Error, Result};
use crate::store::Store;

/// The app-settings key of [`SchedulingSettings`].
pub const SCHEDULING_SETTINGS_KEY: &str = "scheduling";

/// Whether an iTIP message with (`sequence`, `dtstamp`) supersedes one
/// with (`old_sequence`, `old_dtstamp`) (RFC 5546 §2.1.5): a higher
/// `SEQUENCE` wins; with the same `SEQUENCE` a later `DTSTAMP` wins
/// (RFC 3339 UTC strings of the same shape compare as text); a missing
/// `DTSTAMP` never wins a tie.
pub fn itip_is_newer(
    sequence: u32,
    dtstamp: Option<&str>,
    old_sequence: u32,
    old_dtstamp: Option<&str>,
) -> bool {
    match sequence.cmp(&old_sequence) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (dtstamp, old_dtstamp) {
            (Some(new), Some(old)) => new > old,
            (Some(_), None) => true,
            (None, _) => false,
        },
    }
}

/// Whose state an [`ItipStateRecord`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItipParty {
    /// The organizer.
    Organizer,
    /// An attendee.
    Attendee,
}

impl ItipParty {
    fn as_str(self) -> &'static str {
        match self {
            Self::Organizer => "organizer",
            Self::Attendee => "attendee",
        }
    }

    fn parse(text: &str) -> Self {
        if text == "organizer" {
            Self::Organizer
        } else {
            Self::Attendee
        }
    }
}

/// The scheduling state of one address in one event (`itip_state`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItipStateRecord {
    /// The event's `UID`.
    pub uid: String,
    /// The occurrence (`PimTime` JSON), `""` = the whole event.
    pub recurrence_id: String,
    /// The address (stored lowercase).
    pub address: String,
    /// Organizer or attendee.
    pub party: ItipParty,
    /// The newest `SEQUENCE` applied.
    pub sequence: u32,
    /// Its `DTSTAMP` (RFC 3339 UTC).
    pub dtstamp: Option<String>,
    /// The attendee's answer.
    pub status: Option<ParticipationStatus>,
    /// The message last applied for this address.
    pub message_id: Option<MessageId>,
    /// When it changed (Unix seconds).
    pub updated_at: i64,
}

/// What happened to a received iTIP message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItipMessageState {
    /// Not processed yet.
    New,
    /// Applied to the calendar (or answered).
    Applied,
    /// Older than what the calendar has.
    Outdated,
    /// Deliberately not applied (unusable, not addressed to the user, the
    /// user chose so).
    Ignored,
    /// Applying failed.
    Failed,
}

impl ItipMessageState {
    fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Applied => "applied",
            Self::Outdated => "outdated",
            Self::Ignored => "ignored",
            Self::Failed => "failed",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "applied" => Self::Applied,
            "outdated" => Self::Outdated,
            "ignored" => Self::Ignored,
            "failed" => Self::Failed,
            _ => Self::New,
        }
    }
}

/// A received iTIP message (`itip_messages`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItipMessageRecord {
    /// The message.
    pub message_id: MessageId,
    /// Its account.
    pub account_id: AccountId,
    /// The method.
    pub method: ItipMethod,
    /// The event's `UID`.
    pub uid: String,
    /// The occurrence (`PimTime` JSON), `""` = the whole event.
    pub recurrence_id: String,
    /// `SEQUENCE`.
    pub sequence: u32,
    /// `DTSTAMP` (RFC 3339 UTC).
    pub dtstamp: Option<String>,
    /// The organizer / attendee it came from (stored lowercase).
    pub sender: Option<String>,
    /// What happened to it.
    pub state: ItipMessageState,
    /// The user's event it was applied to.
    pub event_id: Option<EventId>,
    /// When it was processed (Unix seconds).
    pub processed_at: Option<i64>,
}

/// An iTIP message Nail sent (`itip_sent`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentItipRecord {
    /// Row id (ignored when recording).
    pub id: i64,
    /// The sending account.
    pub account_id: AccountId,
    /// The sending identity.
    pub identity_id: IdentityId,
    /// The method.
    pub method: ItipMethod,
    /// The event's `UID`.
    pub uid: String,
    /// The occurrence (`PimTime` JSON), `""` = the whole event.
    pub recurrence_id: String,
    /// `SEQUENCE` sent.
    pub sequence: u32,
    /// `DTSTAMP` sent (RFC 3339 UTC).
    pub dtstamp: String,
    /// The answer sent (replies).
    pub status: Option<ParticipationStatus>,
    /// Recipients.
    pub recipients: Vec<String>,
    /// How it travelled ([`SchedulingTransport::None`] is refused).
    pub transport: SchedulingTransport,
    /// The outbox entry of an iMIP mail.
    pub outbox_id: Option<OutboxId>,
    /// When (Unix seconds).
    pub created_at: i64,
}

fn method_str(method: ItipMethod) -> String {
    enum_str(&method)
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

const STATE_COLS: &str =
    "uid, recurrence_id, address, role, sequence, dtstamp, partstat, message_id, updated_at";

fn state_from_row(row: &Row<'_>) -> rusqlite::Result<ItipStateRecord> {
    let role: String = row.get(3)?;
    let status: Option<String> = row.get(6)?;
    Ok(ItipStateRecord {
        uid: row.get(0)?,
        recurrence_id: row.get(1)?,
        address: row.get(2)?,
        party: ItipParty::parse(&role),
        sequence: row.get(4)?,
        dtstamp: row.get(5)?,
        status: status.as_deref().map(enum_parse).transpose()?,
        message_id: row.get::<_, Option<String>>(7)?.map(MessageId),
        updated_at: row.get(8)?,
    })
}

const MESSAGE_COLS: &str = "message_id, account_id, method, uid, recurrence_id, sequence, dtstamp, \
                            sender, state, event_id, processed_at";

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<ItipMessageRecord> {
    let method: String = row.get(2)?;
    let state: String = row.get(8)?;
    Ok(ItipMessageRecord {
        message_id: MessageId(row.get(0)?),
        account_id: AccountId(row.get(1)?),
        method: enum_parse(&method)?,
        uid: row.get(3)?,
        recurrence_id: row.get(4)?,
        sequence: row.get(5)?,
        dtstamp: row.get(6)?,
        sender: row.get(7)?,
        state: ItipMessageState::parse(&state),
        event_id: row.get::<_, Option<String>>(9)?.map(EventId),
        processed_at: row.get(10)?,
    })
}

const SENT_COLS: &str = "id, account_id, identity_id, method, uid, recurrence_id, sequence, \
                         dtstamp, partstat, recipients, transport, outbox_id, created_at";

fn sent_from_row(row: &Row<'_>) -> rusqlite::Result<SentItipRecord> {
    let method: String = row.get(3)?;
    let status: Option<String> = row.get(8)?;
    let recipients: String = row.get(9)?;
    let transport: String = row.get(10)?;
    Ok(SentItipRecord {
        id: row.get(0)?,
        account_id: AccountId(row.get(1)?),
        identity_id: IdentityId(row.get(2)?),
        method: enum_parse(&method)?,
        uid: row.get(4)?,
        recurrence_id: row.get(5)?,
        sequence: row.get(6)?,
        dtstamp: row.get(7)?,
        status: status.as_deref().map(enum_parse).transpose()?,
        recipients: serde_json::from_str(&recipients).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e))
        })?,
        transport: enum_parse(&transport)?,
        outbox_id: row.get::<_, Option<String>>(11)?.map(OutboxId),
        created_at: row.get(12)?,
    })
}

impl Store {
    /// The scheduling state of `address` in an event (occurrence).
    pub fn itip_state(
        &self,
        uid: &str,
        recurrence_id: &str,
        address: &str,
    ) -> Result<Option<ItipStateRecord>> {
        let address = address.to_lowercase();
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {STATE_COLS} FROM itip_state
                         WHERE uid = ?1 AND recurrence_id = ?2 AND address = ?3"
                    ),
                    params![uid, recurrence_id, address],
                    state_from_row,
                )
                .optional()?)
        })
    }

    /// Every address's state in an event, all occurrences, by recurrence
    /// id then address.
    pub fn itip_states(&self, uid: &str) -> Result<Vec<ItipStateRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {STATE_COLS} FROM itip_state WHERE uid = ?1
                 ORDER BY recurrence_id, address"
            ))?;
            let rows = stmt.query_map([uid], state_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Records `record` when it is newer than the stored state
    /// ([`itip_is_newer`]) or there is none. Returns whether it was
    /// stored (`false` = the message is outdated).
    pub fn record_itip_state(&self, record: &ItipStateRecord) -> Result<bool> {
        let address = record.address.to_lowercase();
        self.with_tx(|tx| {
            let current: Option<(u32, Option<String>)> = tx
                .query_row(
                    "SELECT sequence, dtstamp FROM itip_state
                     WHERE uid = ?1 AND recurrence_id = ?2 AND address = ?3",
                    params![record.uid, record.recurrence_id, address],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((sequence, dtstamp)) = &current
                && !itip_is_newer(
                    record.sequence,
                    record.dtstamp.as_deref(),
                    *sequence,
                    dtstamp.as_deref(),
                )
            {
                return Ok(false);
            }
            tx.execute(
                "INSERT INTO itip_state (uid, recurrence_id, address, role, sequence, dtstamp,
                     partstat, message_id, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (uid, recurrence_id, address) DO UPDATE SET
                     role = excluded.role, sequence = excluded.sequence,
                     dtstamp = excluded.dtstamp, partstat = excluded.partstat,
                     message_id = excluded.message_id, updated_at = excluded.updated_at",
                params![
                    record.uid,
                    record.recurrence_id,
                    address,
                    record.party.as_str(),
                    record.sequence,
                    record.dtstamp,
                    record.status.as_ref().map(enum_str),
                    record.message_id.as_ref().map(MessageId::as_str),
                    record.updated_at,
                ],
            )?;
            Ok(true)
        })
    }

    /// Forgets the scheduling state of an event (it was deleted for good).
    pub fn forget_itip_state(&self, uid: &str) -> Result<usize> {
        self.with_conn(|conn| Ok(conn.execute("DELETE FROM itip_state WHERE uid = ?1", [uid])?))
    }

    /// Records a received iTIP message (replacing an earlier record of the
    /// same message).
    pub fn record_itip_message(&self, record: &ItipMessageRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO itip_messages (message_id, account_id, method, uid, recurrence_id,
                     sequence, dtstamp, sender, state, event_id, processed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT (message_id) DO UPDATE SET
                     account_id = excluded.account_id, method = excluded.method,
                     uid = excluded.uid, recurrence_id = excluded.recurrence_id,
                     sequence = excluded.sequence, dtstamp = excluded.dtstamp,
                     sender = excluded.sender, state = excluded.state,
                     event_id = excluded.event_id, processed_at = excluded.processed_at",
                params![
                    record.message_id.as_str(),
                    record.account_id.as_str(),
                    method_str(record.method),
                    record.uid,
                    record.recurrence_id,
                    record.sequence,
                    record.dtstamp,
                    record.sender.as_ref().map(|s| s.to_lowercase()),
                    record.state.as_str(),
                    record.event_id.as_ref().map(EventId::as_str),
                    record.processed_at,
                ],
            )?;
            Ok(())
        })
    }

    /// The record of a received iTIP message.
    pub fn itip_message(&self, message_id: &MessageId) -> Result<Option<ItipMessageRecord>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {MESSAGE_COLS} FROM itip_messages WHERE message_id = ?1"),
                    [message_id.as_str()],
                    message_from_row,
                )
                .optional()?)
        })
    }

    /// Received iTIP messages of an event, oldest `SEQUENCE` / `DTSTAMP`
    /// first.
    pub fn itip_messages_for(&self, uid: &str) -> Result<Vec<ItipMessageRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {MESSAGE_COLS} FROM itip_messages WHERE uid = ?1
                 ORDER BY sequence, dtstamp, message_id"
            ))?;
            let rows = stmt.query_map([uid], message_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Sets what happened to a received iTIP message.
    pub fn set_itip_message_state(
        &self,
        message_id: &MessageId,
        state: ItipMessageState,
        event_id: Option<&EventId>,
        processed_at: i64,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE itip_messages SET state = ?2, event_id = ?3, processed_at = ?4
                 WHERE message_id = ?1",
                params![
                    message_id.as_str(),
                    state.as_str(),
                    event_id.map(EventId::as_str),
                    processed_at
                ],
            )?;
            if changed == 0 {
                return Err(Error::not_found("iTIP message", message_id));
            }
            Ok(())
        })
    }

    /// Records an iTIP message Nail sent; returns its row id.
    pub fn record_sent_itip(&self, record: &SentItipRecord) -> Result<i64> {
        if record.transport == SchedulingTransport::None {
            return Err(Error::InvalidData(
                "a sent iTIP message needs a transport".into(),
            ));
        }
        let recipients: Vec<String> = record.recipients.iter().map(|r| r.to_lowercase()).collect();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO itip_sent (account_id, identity_id, method, uid, recurrence_id,
                     sequence, dtstamp, partstat, recipients, transport, outbox_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    record.account_id.as_str(),
                    record.identity_id.as_str(),
                    method_str(record.method),
                    record.uid,
                    record.recurrence_id,
                    record.sequence,
                    record.dtstamp,
                    record.status.as_ref().map(enum_str),
                    serde_json::to_string(&recipients)?,
                    enum_str(&record.transport),
                    record.outbox_id.as_ref().map(OutboxId::as_str),
                    record.created_at,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// iTIP messages sent for an event, oldest first.
    pub fn sent_itips(&self, uid: &str) -> Result<Vec<SentItipRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {SENT_COLS} FROM itip_sent WHERE uid = ?1 ORDER BY id"
            ))?;
            let rows = stmt.query_map([uid], sent_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The scheduling settings (defaults when never set).
    pub fn scheduling_settings(&self) -> Result<SchedulingSettings> {
        Ok(self
            .setting::<SchedulingSettings>(SCHEDULING_SETTINGS_KEY)?
            .unwrap_or_default())
    }

    /// Replaces the scheduling settings.
    pub fn set_scheduling_settings(&self, settings: &SchedulingSettings) -> Result<()> {
        self.set_setting(SCHEDULING_SETTINGS_KEY, settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::{Account, AccountKind, Flags, Folder, FolderRole, MessageSummary};

    fn store() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_account(&Account {
                id: "a".into(),
                name: "A".into(),
                email: "ada@example.org".into(),
                kind: AccountKind::Imap,
            })
            .unwrap();
        store
            .upsert_folder(&Folder {
                id: "inbox".into(),
                account_id: "a".into(),
                parent_id: None,
                name: "INBOX".into(),
                path: "INBOX".into(),
                role: FolderRole::Inbox,
                unread: 0,
                total: 0,
            })
            .unwrap();
        store
            .upsert_message(&crate::StoredMessage::new(MessageSummary {
                id: "m1".into(),
                folder_id: "inbox".into(),
                account_id: "a".into(),
                subject: "Invitation".into(),
                from: None,
                to: vec![],
                date: "2026-10-01T10:00:00Z".into(),
                flags: Flags::default(),
                has_attachments: false,
                preview: String::new(),
                thread_id: None,
                tags: vec![],
                duplicates: vec![],
                headers_only: false,
                gmail: None,
                exchange: None,
            }))
            .unwrap();
        store
    }

    fn state(sequence: u32, dtstamp: &str, status: ParticipationStatus) -> ItipStateRecord {
        ItipStateRecord {
            uid: "uid-1".into(),
            recurrence_id: String::new(),
            address: "Bob@Example.org".into(),
            party: ItipParty::Attendee,
            sequence,
            dtstamp: Some(dtstamp.into()),
            status: Some(status),
            message_id: None,
            updated_at: 1,
        }
    }

    #[test]
    fn newer_means_higher_sequence_then_later_stamp() {
        assert!(itip_is_newer(2, None, 1, Some("2026-10-02T00:00:00Z")));
        assert!(!itip_is_newer(1, Some("2026-10-09T00:00:00Z"), 2, None));
        assert!(itip_is_newer(
            1,
            Some("2026-10-02T00:00:00Z"),
            1,
            Some("2026-10-01T00:00:00Z")
        ));
        assert!(!itip_is_newer(
            1,
            Some("2026-10-01T00:00:00Z"),
            1,
            Some("2026-10-01T00:00:00Z")
        ));
        assert!(itip_is_newer(0, Some("2026-10-01T00:00:00Z"), 0, None));
        assert!(!itip_is_newer(0, None, 0, None));
    }

    #[test]
    fn state_only_moves_forward() {
        let store = store();
        let first = state(0, "2026-10-01T10:00:00Z", ParticipationStatus::Accepted);
        assert!(store.record_itip_state(&first).unwrap());
        // A late, older reply is outdated.
        let older = state(0, "2026-10-01T09:00:00Z", ParticipationStatus::Declined);
        assert!(!store.record_itip_state(&older).unwrap());
        let stored = store
            .itip_state("uid-1", "", "BOB@example.org")
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, Some(ParticipationStatus::Accepted));
        assert_eq!(stored.address, "bob@example.org");
        // A newer one replaces it.
        let newer = state(1, "2026-10-01T08:00:00Z", ParticipationStatus::Tentative);
        assert!(store.record_itip_state(&newer).unwrap());
        let all = store.itip_states("uid-1").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].sequence, 1);
        assert_eq!(all[0].status, Some(ParticipationStatus::Tentative));
        assert_eq!(store.forget_itip_state("uid-1").unwrap(), 1);
        assert_eq!(store.itip_states("uid-1").unwrap().len(), 0);
    }

    #[test]
    fn received_messages_are_tracked() {
        let store = store();
        let record = ItipMessageRecord {
            message_id: "m1".into(),
            account_id: "a".into(),
            method: ItipMethod::DeclineCounter,
            uid: "uid-1".into(),
            recurrence_id: String::new(),
            sequence: 3,
            dtstamp: Some("2026-10-01T10:00:00Z".into()),
            sender: Some("Organizer@Example.org".into()),
            state: ItipMessageState::New,
            event_id: None,
            processed_at: None,
        };
        store.record_itip_message(&record).unwrap();
        store
            .set_itip_message_state(
                &"m1".into(),
                ItipMessageState::Applied,
                Some(&"ev-1".into()),
                42,
            )
            .unwrap();
        let back = store.itip_message(&"m1".into()).unwrap().unwrap();
        assert_eq!(back.method, ItipMethod::DeclineCounter);
        assert_eq!(back.state, ItipMessageState::Applied);
        assert_eq!(back.event_id, Some("ev-1".into()));
        assert_eq!(back.sender.as_deref(), Some("organizer@example.org"));
        assert_eq!(store.itip_messages_for("uid-1").unwrap().len(), 1);
        assert!(
            store
                .set_itip_message_state(&"nope".into(), ItipMessageState::Failed, None, 1)
                .unwrap_err()
                .is_not_found()
        );
        // The record goes with its message.
        store.delete_message(&"m1".into()).unwrap();
        assert!(store.itip_message(&"m1".into()).unwrap().is_none());
    }

    #[test]
    fn sent_messages_and_settings() {
        let store = store();
        let mut sent = SentItipRecord {
            id: 0,
            account_id: "a".into(),
            identity_id: "ada@example.org".into(),
            method: ItipMethod::Reply,
            uid: "uid-1".into(),
            recurrence_id: String::new(),
            sequence: 0,
            dtstamp: "2026-10-01T10:00:00Z".into(),
            status: Some(ParticipationStatus::Accepted),
            recipients: vec!["Organizer@Example.org".into()],
            transport: SchedulingTransport::Imip,
            outbox_id: Some("7".into()),
            created_at: 5,
        };
        let id = store.record_sent_itip(&sent).unwrap();
        let back = store.sent_itips("uid-1").unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, id);
        assert_eq!(back[0].recipients, ["organizer@example.org"]);
        assert_eq!(back[0].transport, SchedulingTransport::Imip);
        assert_eq!(back[0].status, Some(ParticipationStatus::Accepted));
        sent.transport = SchedulingTransport::None;
        assert!(store.record_sent_itip(&sent).is_err());

        assert_eq!(
            store.scheduling_settings().unwrap(),
            SchedulingSettings::default()
        );
        let settings = SchedulingSettings {
            apply_cancellations: true,
            ..SchedulingSettings::default()
        };
        store.set_scheduling_settings(&settings).unwrap();
        assert_eq!(store.scheduling_settings().unwrap(), settings);
    }
}
