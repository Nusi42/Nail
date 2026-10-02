//! Calendars, tasks and contacts for imports, exports and backups (ADRs
//! 0094, 0099).
//!
//! Imports collect the parsed items on the blocking pool ([`PimBuffer`])
//! and write them afterwards through the core's PIM import path (the PIM
//! queue for server sources, same `UID` = update; [`apply`]). Exports and
//! backups read each item's stored iCalendar / vCard text
//! ([`StorePimSource`]).

use std::collections::BTreeMap;

use nail_api::pim::{AddressBook, Calendar, CalendarComponent, PimItemRef};
use nail_api::portability::{ImportCollectionKind, ItemRef, JobReport, PortabilityIssueCode};
use nail_api::{AddressBookId, CalendarId, PimSourceId};
use nail_ical::import::ImportData;
use nail_ical::vcard::{ParsedCards, ParsedContact, ParsedGroup};
use nail_ical::{ParsedCalendar, ParsedEvent, ParsedTask};
use nail_portability::SinkError;
use nail_portability::sink::{
    ExportCollection, PimExportSource, PimImportSink, SourceCollection, Visit,
};

use crate::error::Result;
use crate::service::Service;

fn store_err(err: impl std::fmt::Display) -> SinkError {
    SinkError::Fatal(err.to_string())
}

/// Where buffered items go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PimMode {
    /// An import: the options' calendar / address book (`None` = the
    /// default one).
    Import,
    /// A restore: the collection with the backed-up key, created as a local
    /// collection when it is gone.
    Restore,
}

/// Items read on the blocking pool, written by [`apply`].
pub(crate) struct PimBuffer {
    service: Service,
    mode: PimMode,
    calendar: Option<CalendarId>,
    address_book: Option<AddressBookId>,
    calendars: BTreeMap<String, (String, ParsedCalendar)>,
    books: BTreeMap<String, (String, ParsedCards)>,
}

impl PimBuffer {
    pub(crate) fn new(
        service: &Service,
        mode: PimMode,
        calendar: Option<CalendarId>,
        address_book: Option<AddressBookId>,
    ) -> Self {
        Self {
            service: service.clone(),
            mode,
            calendar,
            address_book,
            calendars: BTreeMap::new(),
            books: BTreeMap::new(),
        }
    }

    fn key(&self, collection: &SourceCollection) -> String {
        match self.mode {
            PimMode::Import => String::new(),
            PimMode::Restore => collection.key.clone(),
        }
    }

    fn calendar(&mut self, collection: &SourceCollection) -> &mut ParsedCalendar {
        let key = self.key(collection);
        &mut self
            .calendars
            .entry(key)
            .or_insert_with(|| (collection.name.clone(), ParsedCalendar::default()))
            .1
    }

    fn cards(&mut self, collection: &SourceCollection) -> &mut ParsedCards {
        let key = self.key(collection);
        &mut self
            .books
            .entry(key)
            .or_insert_with(|| {
                (
                    collection.name.clone(),
                    ParsedCards {
                        contacts: Vec::new(),
                        groups: Vec::new(),
                        issues: Vec::new(),
                    },
                )
            })
            .1
    }

    /// Whether nothing was read.
    pub(crate) fn is_empty(&self) -> bool {
        self.calendars.is_empty() && self.books.is_empty()
    }
}

impl PimImportSink for PimBuffer {
    fn event(&mut self, c: &SourceCollection, event: ParsedEvent) -> Result<(), SinkError> {
        self.calendar(c).events.push(event);
        Ok(())
    }

    fn task(&mut self, c: &SourceCollection, task: ParsedTask) -> Result<(), SinkError> {
        self.calendar(c).tasks.push(task);
        Ok(())
    }

    fn contact(&mut self, c: &SourceCollection, contact: ParsedContact) -> Result<(), SinkError> {
        self.cards(c).contacts.push(contact);
        Ok(())
    }

    fn group(&mut self, c: &SourceCollection, group: ParsedGroup) -> Result<(), SinkError> {
        self.cards(c).groups.push(group);
        Ok(())
    }

    fn exists(&mut self, collection: &SourceCollection, uid: &str) -> bool {
        let store = self.service.store();
        let key = collection.key.clone();
        match collection.kind {
            ImportCollectionKind::AddressBook => {
                let books = [AddressBookId::new(key)];
                store
                    .contacts(Some(&books))
                    .is_ok_and(|all| all.iter().any(|c| c.uid == uid))
                    || store
                        .contact_groups(Some(&books))
                        .is_ok_and(|all| all.iter().any(|g| g.uid == uid))
            }
            _ => {
                let cals = [CalendarId::new(key)];
                store
                    .events(Some(&cals))
                    .is_ok_and(|all| all.iter().any(|e| e.uid == uid))
                    || store
                        .list_tasks(Some(&cals), true)
                        .is_ok_and(|all| all.iter().any(|t| t.uid == uid))
            }
        }
    }
}

/// Writes the buffered items (after the blocking part of a job) and adds
/// what could not be written to the report.
pub(crate) async fn apply(service: &Service, buffer: PimBuffer, report: &mut JobReport) {
    // The local defaults (made by the first PIM request otherwise).
    if let Err(err) = service.ensure_local_pim().await {
        tracing::warn!(%err, "cannot make the local calendars and address books");
    }
    let PimBuffer {
        mode,
        calendar,
        address_book,
        calendars,
        books,
        ..
    } = buffer;
    for (key, (name, parsed)) in calendars {
        let target = match mode {
            PimMode::Import => Ok(calendar.clone()),
            PimMode::Restore => ensure_calendar(service, &key, &name).await.map(Some),
        };
        let written = match target {
            Ok(target) => {
                service
                    .import_pim(ImportData::Calendar(parsed), target, None)
                    .await
            }
            Err(err) => Err(err),
        };
        record(report, &name, written);
    }
    for (key, (name, parsed)) in books {
        let target = match mode {
            PimMode::Import => Ok(address_book.clone()),
            PimMode::Restore => ensure_address_book(service, &key, &name).await.map(Some),
        };
        let written = match target {
            Ok(target) => {
                service
                    .import_pim(ImportData::Cards(parsed), None, target)
                    .await
            }
            Err(err) => Err(err),
        };
        record(report, &name, written);
    }
}

fn record(
    report: &mut JobReport,
    collection: &str,
    written: Result<nail_api::pim::PimImportResult>,
) {
    let item = ItemRef {
        folder: Some(collection.to_owned()),
        ..ItemRef::default()
    };
    match written {
        Ok(result) if result.skipped > 0 => {
            report.push_issue(nail_api::portability::ItemIssue::new(
                PortabilityIssueCode::WriteFailed,
                nail_api::portability::IssueSeverity::Skipped,
                item,
                format!("{} items could not be saved", result.skipped),
            ));
        }
        Ok(_) => {}
        Err(err) => {
            let err = nail_api::ApiError::from(err);
            report.push_issue(nail_api::portability::ItemIssue::new(
                PortabilityIssueCode::WriteFailed,
                nail_api::portability::IssueSeverity::Skipped,
                item,
                err.message,
            ));
        }
    }
}

/// The calendar a restored collection goes to: the one with its id, else
/// a new local calendar with that id and name.
async fn ensure_calendar(service: &Service, key: &str, name: &str) -> Result<CalendarId> {
    let id = CalendarId::new(key.to_owned());
    let found = {
        let id = id.clone();
        service.store().call(move |s| s.calendar(&id)).await?
    };
    if found.is_none() {
        let calendar = Calendar {
            id: id.clone(),
            source_id: PimSourceId::new(PimSourceId::LOCAL),
            account_id: None,
            settings: nail_api::pim::CalendarSettings::named(name),
            components: vec![CalendarComponent::Event, CalendarComponent::Task],
            read_only: false,
            timezone: None,
            exchange: None,
        };
        service
            .store()
            .call(move |s| s.save_calendar(&calendar, None))
            .await?;
        service.broadcast(nail_api::Event::CalendarsChanged);
    }
    Ok(id)
}

/// The address book a restored collection goes to (see
/// [`ensure_calendar`]).
async fn ensure_address_book(service: &Service, key: &str, name: &str) -> Result<AddressBookId> {
    let id = AddressBookId::new(key.to_owned());
    let found = {
        let id = id.clone();
        service.store().call(move |s| s.address_book(&id)).await?
    };
    if found.is_none() {
        let book = AddressBook {
            id: id.clone(),
            source_id: PimSourceId::new(PimSourceId::LOCAL),
            account_id: None,
            settings: nail_api::pim::AddressBookSettings::named(name),
            read_only: false,
            role: None,
            exchange: None,
        };
        service
            .store()
            .call(move |s| s.save_address_book(&book, None))
            .await?;
        service.broadcast(nail_api::Event::AddressBooksChanged);
    }
    Ok(id)
}

/// The store's calendars or address books, read for an export or a
/// backup.
pub(crate) struct StorePimSource {
    service: Service,
    collections: Vec<(ExportCollection, ImportCollectionKind)>,
}

impl StorePimSource {
    /// Chosen calendars (blocking).
    pub(crate) fn calendars(service: &Service, ids: &[CalendarId]) -> Result<Self> {
        let store = service.store();
        let mut collections = Vec::new();
        for id in ids {
            let calendar = store
                .calendar(id)?
                .ok_or_else(|| nail_api::ApiError::not_found(format!("calendar {id}")))?;
            collections.push((
                ExportCollection {
                    key: id.to_string(),
                    name: calendar.settings.name,
                    items: None,
                },
                ImportCollectionKind::Calendar,
            ));
        }
        Ok(Self {
            service: service.clone(),
            collections,
        })
    }

    /// Chosen address books (blocking).
    pub(crate) fn address_books(service: &Service, ids: &[AddressBookId]) -> Result<Self> {
        let store = service.store();
        let mut collections = Vec::new();
        for id in ids {
            let book = store
                .address_book(id)?
                .ok_or_else(|| nail_api::ApiError::not_found(format!("address book {id}")))?;
            collections.push((
                ExportCollection {
                    key: id.to_string(),
                    name: book.settings.name,
                    items: None,
                },
                ImportCollectionKind::AddressBook,
            ));
        }
        Ok(Self {
            service: service.clone(),
            collections,
        })
    }

    /// How many collections the export writes.
    pub(crate) fn collection_count(&self) -> usize {
        self.collections.len()
    }

    /// The local source's calendars (backups).
    pub(crate) fn local_calendars(service: &Service) -> Result<Self> {
        let ids: Vec<CalendarId> = service
            .store()
            .calendars(Some(&PimSourceId::new(PimSourceId::LOCAL)))?
            .into_iter()
            .map(|c| c.id)
            .collect();
        Self::calendars(service, &ids)
    }

    /// The local source's address books (backups).
    pub(crate) fn local_address_books(service: &Service) -> Result<Self> {
        let ids: Vec<AddressBookId> = service
            .store()
            .address_books(Some(&PimSourceId::new(PimSourceId::LOCAL)))?
            .into_iter()
            .map(|b| b.id)
            .collect();
        Self::address_books(service, &ids)
    }
}

impl PimExportSource for StorePimSource {
    fn collections(&mut self) -> Result<Vec<ExportCollection>, SinkError> {
        Ok(self.collections.iter().map(|(c, _)| c.clone()).collect())
    }

    fn items(
        &mut self,
        collection: &ExportCollection,
        visit: &mut dyn FnMut(Result<String, SinkError>) -> Visit,
    ) -> Result<(), SinkError> {
        let Some((_, kind)) = self
            .collections
            .iter()
            .find(|(c, _)| c.key == collection.key)
        else {
            return Ok(());
        };
        let store = self.service.store();
        let refs: Vec<PimItemRef> = match kind {
            ImportCollectionKind::AddressBook => {
                let books = [AddressBookId::new(collection.key.clone())];
                let mut refs: Vec<PimItemRef> = store
                    .contacts(Some(&books))
                    .map_err(store_err)?
                    .into_iter()
                    .map(|c| PimItemRef::Contact { contact_id: c.id })
                    .collect();
                refs.extend(
                    store
                        .contact_groups(Some(&books))
                        .map_err(store_err)?
                        .into_iter()
                        .map(|g| PimItemRef::ContactGroup { group_id: g.id }),
                );
                refs
            }
            _ => {
                let cals = [CalendarId::new(collection.key.clone())];
                let mut refs: Vec<PimItemRef> = store
                    .events(Some(&cals))
                    .map_err(store_err)?
                    .into_iter()
                    .map(|e| PimItemRef::Event { event_id: e.id })
                    .collect();
                refs.extend(
                    store
                        .list_tasks(Some(&cals), true)
                        .map_err(store_err)?
                        .into_iter()
                        .map(|t| PimItemRef::Task { task_id: t.id }),
                );
                refs
            }
        };
        for item in refs {
            let text = match store.pim_object(&item) {
                Ok(Some(object)) => Ok(object.raw),
                Ok(None) => Err(SinkError::Item(format!("{item:?} has no stored text"))),
                Err(err) => Err(SinkError::Item(err.to_string())),
            };
            if visit(text) == Visit::Stop {
                break;
            }
        }
        Ok(())
    }
}
