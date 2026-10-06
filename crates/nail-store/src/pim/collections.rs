//! PIM sources and their collections (calendars / task lists and address
//! books), sync tokens, and the local "Collected" address book.

use nail_api::pim::contact::AddressBookRole;
use nail_api::pim::{
    AddressBook, AddressBookSettings, Calendar, CalendarSettings, PimSourceKind, PimSourceState,
};
use nail_api::{AddressBookId, ApiError, CalendarId, PimSourceId};
use rusqlite::{OptionalExtension, Row, Transaction, params};

use crate::error::{Error, Result};
use crate::store::Store;

use super::LOCAL_SOURCE_ID;

/// Id of the local "Collected" address book (made by
/// [`Store::ensure_collected_book`]).
pub const COLLECTED_BOOK_ID: &str = "local-collected";

/// Fluent id of the Collected book's name (`core-pim.ftl`); the core
/// passes the localized text to [`Store::ensure_collected_book`].
pub const COLLECTED_BOOK_NAME_ID: &str = "pim-address-book-collected";

/// A source row: what the store knows about a source besides its settings
/// (those stay in `accounts.kdl`).
#[derive(Debug, Clone, PartialEq)]
pub struct PimSourceRecord {
    /// The source.
    pub id: PimSourceId,
    /// Its kind.
    pub kind: PimSourceKind,
    /// Discovered endpoints (principal, home sets, capabilities), JSON.
    pub discovery: Option<serde_json::Value>,
    /// Sync state.
    pub state: PimSourceState,
    /// Last successful sync (Unix seconds).
    pub last_sync_at: Option<i64>,
    /// Last failure.
    pub last_error: Option<ApiError>,
    /// When the row was made (Unix seconds).
    pub created_at: i64,
}

/// What a collection is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionKind {
    /// A calendar or task list.
    Calendar,
    /// An address book.
    AddressBook,
}

impl CollectionKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::AddressBook => "address-book",
        }
    }

    fn parse(text: &str) -> Self {
        if text == "calendar" {
            Self::Calendar
        } else {
            Self::AddressBook
        }
    }
}

/// Sync bookkeeping of one collection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CollectionSync {
    /// Collection URL (DAV) or feed URL (ICS).
    pub href: Option<String>,
    /// CalDAV / CardDAV `getctag`.
    pub ctag: Option<String>,
    /// RFC 6578 sync token.
    pub sync_token: Option<String>,
    /// HTTP `ETag` of an ICS feed.
    pub etag: Option<String>,
    /// HTTP `Last-Modified` of an ICS feed.
    pub last_modified: Option<String>,
    /// Last sync (Unix seconds).
    pub last_sync_at: Option<i64>,
}

/// Facts about a collection the item code needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectionInfo {
    pub id: String,
    pub source_id: String,
    pub source_kind: PimSourceKind,
    pub kind: CollectionKind,
    pub read_only: bool,
}

pub(crate) fn source_kind_str(kind: PimSourceKind) -> &'static str {
    match kind {
        PimSourceKind::Local => "local",
        PimSourceKind::Ics => "ics",
        PimSourceKind::Caldav => "caldav",
        PimSourceKind::Carddav => "carddav",
        PimSourceKind::Exchange => "exchange",
    }
}

fn parse_source_kind(text: &str) -> PimSourceKind {
    match text {
        "ics" => PimSourceKind::Ics,
        "caldav" => PimSourceKind::Caldav,
        "carddav" => PimSourceKind::Carddav,
        "exchange" => PimSourceKind::Exchange,
        _ => PimSourceKind::Local,
    }
}

fn state_str(state: PimSourceState) -> &'static str {
    match state {
        PimSourceState::Idle => "idle",
        PimSourceState::Syncing => "syncing",
        PimSourceState::Offline => "offline",
        PimSourceState::AuthError => "auth-error",
        PimSourceState::Error => "error",
        PimSourceState::Disabled => "disabled",
    }
}

fn parse_state(text: &str) -> PimSourceState {
    match text {
        "syncing" => PimSourceState::Syncing,
        "offline" => PimSourceState::Offline,
        "auth-error" => PimSourceState::AuthError,
        "error" => PimSourceState::Error,
        "disabled" => PimSourceState::Disabled,
        _ => PimSourceState::Idle,
    }
}

fn json_col<T: serde::de::DeserializeOwned>(row: &Row<'_>, idx: usize) -> rusqlite::Result<T> {
    let text: String = row.get(idx)?;
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn opt_json_col<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    idx: usize,
) -> rusqlite::Result<Option<T>> {
    let text: Option<String> = row.get(idx)?;
    text.map(|t| {
        serde_json::from_str(&t).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
        })
    })
    .transpose()
}

/// Column 8 is the collection's Exchange data as JSON
/// (M3c; `crate::exchange::collection_info_from_json` reads it).
const COLLECTION_COLS: &str = "c.id, c.source_id, c.settings, c.components, c.read_only, \
     c.timezone, c.role, c.kind, (SELECT json_object('mailbox', x.mailbox, 'role', x.role, \
     'todo', x.todo) FROM exchange_pim_collections x WHERE x.collection_id = c.id)";

fn calendar_from_row(row: &Row<'_>) -> rusqlite::Result<Calendar> {
    Ok(Calendar {
        id: CalendarId(row.get(0)?),
        source_id: PimSourceId(row.get(1)?),
        account_id: None,
        settings: json_col(row, 2)?,
        components: json_col(row, 3)?,
        read_only: row.get(4)?,
        timezone: row.get(5)?,
        exchange: crate::exchange::collection_info_from_json(row.get(8)?),
    })
}

fn address_book_from_row(row: &Row<'_>) -> rusqlite::Result<AddressBook> {
    let role: Option<String> = row.get(6)?;
    Ok(AddressBook {
        id: AddressBookId(row.get(0)?),
        source_id: PimSourceId(row.get(1)?),
        account_id: None,
        settings: json_col(row, 2)?,
        read_only: row.get(4)?,
        role: role
            .filter(|r| r == "collected")
            .map(|_| AddressBookRole::Collected),
        exchange: crate::exchange::collection_info_from_json(row.get(8)?),
    })
}

pub(crate) fn collection_info(
    tx: &rusqlite::Connection,
    id: &str,
) -> Result<Option<CollectionInfo>> {
    Ok(tx
        .query_row(
            "SELECT c.id, c.source_id, s.kind, c.kind, c.read_only
             FROM pim_collections c JOIN pim_sources s ON s.id = c.source_id
             WHERE c.id = ?1",
            [id],
            |r| {
                Ok(CollectionInfo {
                    id: r.get(0)?,
                    source_id: r.get(1)?,
                    source_kind: parse_source_kind(&r.get::<_, String>(2)?),
                    kind: CollectionKind::parse(&r.get::<_, String>(3)?),
                    read_only: r.get(4)?,
                })
            },
        )
        .optional()?)
}

/// A collection row to write.
#[derive(Clone, Copy)]
struct NewCollection<'a> {
    id: &'a str,
    source_id: &'a str,
    kind: CollectionKind,
    href: Option<&'a str>,
    settings: &'a str,
    components: &'a str,
    read_only: bool,
    timezone: Option<&'a str>,
    role: Option<&'a str>,
}

fn insert_collection(tx: &Transaction<'_>, c: &NewCollection<'_>) -> Result<()> {
    let NewCollection {
        id,
        source_id,
        kind,
        href,
        settings,
        components,
        read_only,
        timezone,
        role,
    } = *c;
    let source: Option<String> = tx
        .query_row(
            "SELECT id FROM pim_sources WHERE id = ?1",
            [source_id],
            |r| r.get(0),
        )
        .optional()?;
    if source.is_none() {
        return Err(Error::not_found("PIM source", source_id));
    }
    tx.execute(
        "INSERT INTO pim_collections
             (id, source_id, kind, href, settings, components, read_only, timezone, role)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (id) DO UPDATE SET
             href = excluded.href, settings = excluded.settings,
             components = excluded.components, read_only = excluded.read_only,
             timezone = excluded.timezone",
        params![
            id,
            source_id,
            kind.as_str(),
            href,
            settings,
            components,
            read_only,
            timezone,
            role
        ],
    )?;
    Ok(())
}

impl Store {
    // ---------------------------------------------------------------- sources

    /// Makes sure a source row exists with this kind. A row of another kind
    /// (the user replaced a `caldav` node with a `carddav` one of the same
    /// name) is removed with everything of it first. Returns whether the
    /// row was created (or recreated).
    pub fn ensure_pim_source(
        &self,
        id: &PimSourceId,
        kind: PimSourceKind,
        now: i64,
    ) -> Result<bool> {
        if id.as_str() == LOCAL_SOURCE_ID && kind != PimSourceKind::Local {
            return Err(Error::InvalidData(
                "the local PIM source is built in".into(),
            ));
        }
        self.with_tx(|tx| {
            let existing: Option<String> = tx
                .query_row(
                    "SELECT kind FROM pim_sources WHERE id = ?1",
                    [id.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            match existing {
                Some(k) if k == source_kind_str(kind) => return Ok(false),
                Some(_) => {
                    tx.execute("DELETE FROM pim_sources WHERE id = ?1", [id.as_str()])?;
                }
                None => {}
            }
            tx.execute(
                "INSERT INTO pim_sources (id, kind, created_at) VALUES (?1, ?2, ?3)",
                params![id.as_str(), source_kind_str(kind), now],
            )?;
            Ok(true)
        })
    }

    /// Every source row, the local one first.
    pub fn pim_sources(&self) -> Result<Vec<PimSourceRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, kind, discovery, state, last_sync_at, last_error, created_at
                 FROM pim_sources ORDER BY id <> 'local', id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(PimSourceRecord {
                    id: PimSourceId(r.get(0)?),
                    kind: parse_source_kind(&r.get::<_, String>(1)?),
                    discovery: opt_json_col(r, 2)?,
                    state: parse_state(&r.get::<_, String>(3)?),
                    last_sync_at: r.get(4)?,
                    last_error: opt_json_col(r, 5)?,
                    created_at: r.get(6)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// One source row.
    pub fn pim_source(&self, id: &PimSourceId) -> Result<Option<PimSourceRecord>> {
        Ok(self.pim_sources()?.into_iter().find(|s| &s.id == id))
    }

    /// Records a source's sync state; `last_sync_at` is kept when `None`.
    pub fn set_pim_source_state(
        &self,
        id: &PimSourceId,
        state: PimSourceState,
        error: Option<&ApiError>,
        last_sync_at: Option<i64>,
    ) -> Result<()> {
        let error = error.map(serde_json::to_string).transpose()?;
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE pim_sources SET state = ?2, last_error = ?3,
                     last_sync_at = COALESCE(?4, last_sync_at)
                 WHERE id = ?1",
                params![id.as_str(), state_str(state), error, last_sync_at],
            )?)
        })?;
        if changed == 0 {
            return Err(Error::not_found("PIM source", id));
        }
        Ok(())
    }

    /// Records what discovery found for a source.
    pub fn set_pim_source_discovery(
        &self,
        id: &PimSourceId,
        discovery: Option<&serde_json::Value>,
    ) -> Result<()> {
        let text = discovery.map(serde_json::to_string).transpose()?;
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE pim_sources SET discovery = ?2 WHERE id = ?1",
                params![id.as_str(), text],
            )?;
            Ok(())
        })
    }

    /// Removes a source with all its collections, items, reminders and
    /// queued operations. The local source cannot be removed.
    pub fn remove_pim_source(&self, id: &PimSourceId) -> Result<bool> {
        if id.as_str() == LOCAL_SOURCE_ID {
            return Err(Error::InvalidData(
                "the local PIM source is built in".into(),
            ));
        }
        self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM pim_sources WHERE id = ?1", [id.as_str()])? > 0)
        })
    }

    /// Removes every source that is not in `configured` (the local source
    /// always stays). Returns the removed ids.
    pub fn retain_pim_sources(&self, configured: &[PimSourceId]) -> Result<Vec<PimSourceId>> {
        let gone: Vec<PimSourceId> = self
            .pim_source_ids()?
            .into_iter()
            .filter(|id| id != LOCAL_SOURCE_ID && !configured.iter().any(|c| c.as_str() == id))
            .map(PimSourceId)
            .collect();
        for id in &gone {
            self.remove_pim_source(id)?;
        }
        Ok(gone)
    }

    // ------------------------------------------------------------ collections

    /// Creates (or replaces the settings of) a calendar / task list.
    /// `account_id` of the argument is not stored (it comes from the
    /// source's configuration).
    pub fn save_calendar(&self, calendar: &Calendar, href: Option<&str>) -> Result<()> {
        let settings = serde_json::to_string(&calendar.settings)?;
        let mut components = calendar.components.clone();
        components.sort();
        components.dedup();
        let components = serde_json::to_string(&components)?;
        let timezone_changed = self.with_tx(|tx| {
            let before: Option<Option<String>> = tx
                .query_row(
                    "SELECT timezone FROM pim_collections WHERE id = ?1",
                    [calendar.id.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            insert_collection(
                tx,
                &NewCollection {
                    id: calendar.id.as_str(),
                    source_id: calendar.source_id.as_str(),
                    kind: CollectionKind::Calendar,
                    href,
                    settings: &settings,
                    components: &components,
                    read_only: calendar.read_only,
                    timezone: calendar.timezone.as_deref(),
                    role: None,
                },
            )?;
            Ok(before.is_some_and(|b| b != calendar.timezone))
        })?;
        if timezone_changed {
            self.invalidate_instances(&super::InstanceScope::Calendar(calendar.id.clone()))?;
        }
        Ok(())
    }

    /// Creates (or replaces the settings of) an address book.
    pub fn save_address_book(&self, book: &AddressBook, href: Option<&str>) -> Result<()> {
        let settings = serde_json::to_string(&book.settings)?;
        let role = book.role.map(|_| "collected");
        self.with_tx(|tx| {
            insert_collection(
                tx,
                &NewCollection {
                    id: book.id.as_str(),
                    source_id: book.source_id.as_str(),
                    kind: CollectionKind::AddressBook,
                    href,
                    settings: &settings,
                    components: "[]",
                    read_only: book.read_only,
                    timezone: None,
                    role,
                },
            )
        })
    }

    /// Calendars and task lists, of one source or all, by their order
    /// setting and name. `account_id` is `None` (the core fills it in).
    pub fn calendars(&self, source: Option<&PimSourceId>) -> Result<Vec<Calendar>> {
        let mut out: Vec<Calendar> = self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLLECTION_COLS} FROM pim_collections c
                 WHERE c.kind = 'calendar' AND (?1 IS NULL OR c.source_id = ?1)"
            ))?;
            let rows = stmt.query_map([source.map(PimSourceId::as_str)], calendar_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        out.sort_by(|a, b| {
            (a.settings.order, a.settings.name.to_lowercase(), &a.id).cmp(&(
                b.settings.order,
                b.settings.name.to_lowercase(),
                &b.id,
            ))
        });
        Ok(out)
    }

    /// One calendar.
    pub fn calendar(&self, id: &CalendarId) -> Result<Option<Calendar>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {COLLECTION_COLS} FROM pim_collections c
                         WHERE c.id = ?1 AND c.kind = 'calendar'"
                    ),
                    [id.as_str()],
                    calendar_from_row,
                )
                .optional()?)
        })
    }

    /// Address books, of one source or all, by order and name.
    pub fn address_books(&self, source: Option<&PimSourceId>) -> Result<Vec<AddressBook>> {
        let mut out: Vec<AddressBook> = self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLLECTION_COLS} FROM pim_collections c
                 WHERE c.kind = 'address-book' AND (?1 IS NULL OR c.source_id = ?1)"
            ))?;
            let rows = stmt.query_map([source.map(PimSourceId::as_str)], address_book_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        out.sort_by(|a, b| {
            (a.settings.order, a.settings.name.to_lowercase(), &a.id).cmp(&(
                b.settings.order,
                b.settings.name.to_lowercase(),
                &b.id,
            ))
        });
        Ok(out)
    }

    /// One address book.
    pub fn address_book(&self, id: &AddressBookId) -> Result<Option<AddressBook>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {COLLECTION_COLS} FROM pim_collections c
                         WHERE c.id = ?1 AND c.kind = 'address-book'"
                    ),
                    [id.as_str()],
                    address_book_from_row,
                )
                .optional()?)
        })
    }

    /// Changes a calendar's settings.
    pub fn update_calendar_settings(
        &self,
        id: &CalendarId,
        settings: &CalendarSettings,
    ) -> Result<()> {
        self.update_settings(id.as_str(), "calendar", &serde_json::to_string(settings)?)
    }

    /// Changes an address book's settings.
    pub fn update_address_book_settings(
        &self,
        id: &AddressBookId,
        settings: &AddressBookSettings,
    ) -> Result<()> {
        self.update_settings(
            id.as_str(),
            "address-book",
            &serde_json::to_string(settings)?,
        )
    }

    fn update_settings(&self, id: &str, kind: &str, settings: &str) -> Result<()> {
        let n = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE pim_collections SET settings = ?3 WHERE id = ?1 AND kind = ?2",
                params![id, kind, settings],
            )?)
        })?;
        if n == 0 {
            return Err(Error::not_found("PIM collection", id));
        }
        Ok(())
    }

    /// Changes a calendar's zone (CalDAV `calendar-timezone`); its
    /// instance cache is dropped.
    pub fn set_calendar_timezone(&self, id: &CalendarId, timezone: Option<&str>) -> Result<()> {
        let n = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE pim_collections SET timezone = ?2 WHERE id = ?1 AND kind = 'calendar'
                 AND timezone IS NOT ?2",
                params![id.as_str(), timezone],
            )?)
        })?;
        if n > 0 {
            self.invalidate_instances(&super::InstanceScope::Calendar(id.clone()))?;
        }
        Ok(())
    }

    /// Removes a collection with its items (no server write; the core
    /// queues [`super::PimOperation::DeleteCollection`] itself when it
    /// deletes on the server).
    pub fn delete_collection(&self, id: &str) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM pim_collections WHERE id = ?1", [id])? > 0)
        })
    }

    /// A collection's sync bookkeeping.
    pub fn collection_sync(&self, id: &str) -> Result<CollectionSync> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT href, ctag, sync_token, etag, last_modified, last_sync_at
                 FROM pim_collections WHERE id = ?1",
                [id],
                |r| {
                    Ok(CollectionSync {
                        href: r.get(0)?,
                        ctag: r.get(1)?,
                        sync_token: r.get(2)?,
                        etag: r.get(3)?,
                        last_modified: r.get(4)?,
                        last_sync_at: r.get(5)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| Error::not_found("PIM collection", id))
        })
    }

    /// Records a collection's sync bookkeeping after a sync.
    pub fn set_collection_sync(&self, id: &str, sync: &CollectionSync) -> Result<()> {
        let n = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE pim_collections SET href = ?2, ctag = ?3, sync_token = ?4, etag = ?5,
                     last_modified = ?6, last_sync_at = ?7
                 WHERE id = ?1",
                params![
                    id,
                    sync.href,
                    sync.ctag,
                    sync.sync_token,
                    sync.etag,
                    sync.last_modified,
                    sync.last_sync_at
                ],
            )?)
        })?;
        if n == 0 {
            return Err(Error::not_found("PIM collection", id));
        }
        Ok(())
    }

    /// The collection of a source with this URL.
    pub fn collection_by_href(&self, source: &PimSourceId, href: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT id FROM pim_collections WHERE source_id = ?1 AND href = ?2",
                    params![source.as_str(), href],
                    |r| r.get(0),
                )
                .optional()?)
        })
    }

    /// The local "Collected" address book, made on first use with `name`
    /// (the localized [`COLLECTED_BOOK_NAME_ID`]). Its addresses never
    /// pollute the real books: autocomplete ranks them below contacts and
    /// duplicate detection leaves them out.
    pub fn ensure_collected_book(&self, name: &str) -> Result<AddressBook> {
        if let Some(book) = self.address_book(&AddressBookId::new(COLLECTED_BOOK_ID))? {
            return Ok(book);
        }
        let mut settings = AddressBookSettings::named(name);
        // Ranked by autocomplete through the collected statistics instead.
        settings.order = i32::MAX;
        let book = AddressBook {
            id: AddressBookId::new(COLLECTED_BOOK_ID),
            source_id: PimSourceId::local(),
            account_id: None,
            settings,
            read_only: false,
            role: Some(AddressBookRole::Collected),
            exchange: None,
        };
        self.save_address_book(&book, None)?;
        Ok(book)
    }
}

/// The components a calendar holds, parsed from a stored row (tests).
#[cfg(test)]
pub(crate) fn components_of(store: &Store, id: &str) -> Vec<nail_api::pim::CalendarComponent> {
    store
        .with_conn(|conn| {
            let text: String = conn.query_row(
                "SELECT components FROM pim_collections WHERE id = ?1",
                [id],
                |r| r.get(0),
            )?;
            Ok(serde_json::from_str(&text)?)
        })
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::pim::CalendarComponent;

    fn calendar(id: &str, source: &str, name: &str) -> Calendar {
        Calendar {
            id: CalendarId::new(id),
            source_id: PimSourceId::new(source),
            account_id: None,
            settings: CalendarSettings::named(name),
            components: vec![CalendarComponent::Task, CalendarComponent::Event],
            read_only: false,
            timezone: None,
            exchange: None,
        }
    }

    #[test]
    fn sources_are_created_recreated_and_removed() {
        let store = Store::open_in_memory().unwrap();
        let dav = PimSourceId::new("work");
        assert!(
            store
                .ensure_pim_source(&dav, PimSourceKind::Caldav, 5)
                .unwrap()
        );
        assert!(
            !store
                .ensure_pim_source(&dav, PimSourceKind::Caldav, 6)
                .unwrap()
        );
        store
            .save_calendar(&calendar("w1", "work", "Work"), Some("/w1/"))
            .unwrap();
        // Another kind under the same name starts from scratch.
        assert!(
            store
                .ensure_pim_source(&dav, PimSourceKind::Carddav, 7)
                .unwrap()
        );
        assert_eq!(store.calendars(Some(&dav)).unwrap().len(), 0);
        let sources = store.pim_sources().unwrap();
        assert_eq!(sources[0].id.as_str(), LOCAL_SOURCE_ID);
        assert_eq!(sources[1].kind, PimSourceKind::Carddav);
        assert_eq!(sources[1].created_at, 7);
        // State and discovery.
        let error = ApiError::new(nail_api::ErrorCode::AuthFailed, "denied");
        store
            .set_pim_source_state(&dav, PimSourceState::AuthError, Some(&error), Some(99))
            .unwrap();
        store
            .set_pim_source_discovery(&dav, Some(&serde_json::json!({"principal": "/p/"})))
            .unwrap();
        let record = store.pim_source(&dav).unwrap().unwrap();
        assert_eq!(record.state, PimSourceState::AuthError);
        assert_eq!(record.last_error, Some(error));
        assert_eq!(record.last_sync_at, Some(99));
        assert_eq!(record.discovery.unwrap()["principal"], "/p/");
        store
            .set_pim_source_state(&dav, PimSourceState::Idle, None, None)
            .unwrap();
        assert_eq!(
            store.pim_source(&dav).unwrap().unwrap().last_sync_at,
            Some(99)
        );
        assert!(
            store
                .set_pim_source_state(&PimSourceId::new("nope"), PimSourceState::Idle, None, None)
                .is_err()
        );
        // The local source is built in.
        assert!(store.remove_pim_source(&PimSourceId::local()).is_err());
        assert!(
            store
                .ensure_pim_source(&PimSourceId::local(), PimSourceKind::Ics, 1)
                .is_err()
        );
        store
            .ensure_pim_source(&PimSourceId::new("holidays"), PimSourceKind::Ics, 1)
            .unwrap();
        let gone = store
            .retain_pim_sources(&[PimSourceId::new("holidays")])
            .unwrap();
        assert_eq!(gone, [dav]);
        assert_eq!(store.pim_source_ids().unwrap(), ["holidays", "local"]);
    }

    #[test]
    fn calendars_and_address_books() {
        let store = Store::open_in_memory().unwrap();
        let mut b = calendar("b", LOCAL_SOURCE_ID, "beta");
        b.settings.order = 1;
        store.save_calendar(&b, None).unwrap();
        store
            .save_calendar(&calendar("a", LOCAL_SOURCE_ID, "Alpha"), None)
            .unwrap();
        let ids: Vec<String> = store
            .calendars(None)
            .unwrap()
            .into_iter()
            .map(|c| c.id.0)
            .collect();
        assert_eq!(ids, ["a", "b"]);
        assert_eq!(
            components_of(&store, "a"),
            [CalendarComponent::Event, CalendarComponent::Task]
        );
        let mut settings = CalendarSettings::named("Renamed");
        settings.color = Some("#123456".into());
        store
            .update_calendar_settings(&CalendarId::new("a"), &settings)
            .unwrap();
        assert_eq!(
            store
                .calendar(&CalendarId::new("a"))
                .unwrap()
                .unwrap()
                .settings,
            settings
        );
        assert!(
            store
                .calendar(&CalendarId::new("missing"))
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .save_calendar(&calendar("x", "no-such-source", "X"), None)
                .is_err()
        );
        // Address books and the Collected book.
        let book = AddressBook {
            id: AddressBookId::new("people"),
            source_id: PimSourceId::local(),
            account_id: None,
            settings: AddressBookSettings::named("People"),
            read_only: false,
            role: None,
            exchange: None,
        };
        store.save_address_book(&book, None).unwrap();
        let collected = store.ensure_collected_book("Collected").unwrap();
        assert_eq!(collected.role, Some(AddressBookRole::Collected));
        assert_eq!(store.ensure_collected_book("Other").unwrap(), collected);
        let books = store.address_books(None).unwrap();
        assert_eq!(books.len(), 2);
        assert_eq!(books[0], book);
        // A calendar id is not an address book.
        assert!(
            store
                .address_book(&AddressBookId::new("a"))
                .unwrap()
                .is_none()
        );
        assert!(store.delete_collection("people").unwrap());
        assert!(!store.delete_collection("people").unwrap());
    }

    #[test]
    fn sync_bookkeeping() {
        let store = Store::open_in_memory().unwrap();
        let src = PimSourceId::new("dav");
        store
            .ensure_pim_source(&src, PimSourceKind::Caldav, 1)
            .unwrap();
        store
            .save_calendar(&calendar("c", "dav", "C"), Some("/c/"))
            .unwrap();
        assert_eq!(
            store.collection_by_href(&src, "/c/").unwrap().as_deref(),
            Some("c")
        );
        let sync = CollectionSync {
            href: Some("/c/".into()),
            ctag: Some("ct1".into()),
            sync_token: Some("tok".into()),
            etag: None,
            last_modified: None,
            last_sync_at: Some(10),
        };
        store.set_collection_sync("c", &sync).unwrap();
        assert_eq!(store.collection_sync("c").unwrap(), sync);
        assert!(store.collection_sync("zz").is_err());
        assert!(store.set_collection_sync("zz", &sync).is_err());
    }

    #[test]
    fn the_collected_name_is_in_the_catalog() {
        let en = include_str!("../../../../locales/en/core-pim.ftl");
        assert!(
            en.lines()
                .any(|l| l.starts_with(&format!("{COLLECTED_BOOK_NAME_ID} =")))
        );
    }
}
