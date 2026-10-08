//! PIM sources (`accounts.kdl`), calendars and address books, the local
//! source's default collections, and conflict resolution.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use nail_api::accounts::SecretValue;
use nail_api::pim::contact::AddressBookRole;
use nail_api::pim::source::{PimSourceIssue, PimSourceTestResult};
use nail_api::pim::{
    AddressBook, AddressBookSettings, Calendar, CalendarComponent, CalendarSettings,
    ConflictChoice, PimItemRef, PimSource, PimSourceConfig, PimSourceKind, PimSourceState,
};
use nail_api::{
    AddressBookId, ApiError, CalendarId, ErrorCode, Event, LocalizedMessage, PimSourceId, Request,
    Response, SecretRef,
};
use nail_store::Store;
use nail_store::pim::{COLLECTED_BOOK_ID, COLLECTED_BOOK_NAME_ID, PimOperation, PimSourceRecord};

use super::{check, forbidden, new_id, not_found, op_ids};
use crate::accounts::edit_error;
use crate::error::Result;
use crate::service::Service;

/// Ids of the local source's default collections.
pub(crate) const LOCAL_CALENDAR_ID: &str = "local-calendar";
/// The default local task list.
pub(crate) const LOCAL_TASKS_ID: &str = "local-tasks";
/// The default local address book.
pub(crate) const LOCAL_CONTACTS_ID: &str = "local-contacts";

pub(super) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::ListPimSources => Response::PimSources {
            sources: service.list_pim_sources().await?,
        },
        Request::AddPimSource { config, secret } => Response::PimSourceSaved {
            source: Box::new(service.save_pim_source(*config, secret, true).await?),
        },
        Request::UpdatePimSource { config, secret } => Response::PimSourceSaved {
            source: Box::new(service.save_pim_source(*config, secret, false).await?),
        },
        Request::RemovePimSource { source_id } => {
            service.remove_pim_source(&source_id).await?;
            Response::Done
        }
        Request::TestPimSource { config, secret } => Response::PimSourceTested {
            result: Box::new(service.test_pim_source(&config, secret).await),
        },
        Request::SyncPimSources { source_id } => {
            service.sync_pim_now(source_id.as_ref());
            Response::Done
        }
        Request::ListCalendars { source_id } => Response::Calendars {
            calendars: service.list_calendars(source_id).await?,
        },
        Request::CreateCalendar {
            source_id,
            settings,
            components,
        } => {
            let (calendar, ops) = service
                .create_calendar(source_id, settings, components)
                .await?;
            Response::CalendarSaved {
                calendar: Box::new(calendar),
                op_ids: ops,
            }
        }
        Request::UpdateCalendar {
            calendar_id,
            settings,
        } => {
            let (calendar, ops) = service.update_calendar(calendar_id, settings).await?;
            Response::CalendarSaved {
                calendar: Box::new(calendar),
                op_ids: ops,
            }
        }
        Request::DeleteCalendar { calendar_id } => Response::PimDeleted {
            op_ids: service.delete_collection(calendar_id.0, true).await?,
        },
        Request::ListAddressBooks { source_id } => Response::AddressBooks {
            address_books: service.list_address_books(source_id).await?,
        },
        Request::CreateAddressBook {
            source_id,
            settings,
        } => {
            let (book, ops) = service.create_address_book(source_id, settings).await?;
            Response::AddressBookSaved {
                address_book: Box::new(book),
                op_ids: ops,
            }
        }
        Request::UpdateAddressBook {
            address_book_id,
            settings,
        } => {
            let (book, ops) = service
                .update_address_book(address_book_id, settings)
                .await?;
            Response::AddressBookSaved {
                address_book: Box::new(book),
                op_ids: ops,
            }
        }
        Request::DeleteAddressBook { address_book_id } => Response::PimDeleted {
            op_ids: service.delete_collection(address_book_id.0, false).await?,
        },
        Request::ResolvePimConflict { item, keep } => {
            service.resolve_pim_conflict(item, keep).await?;
            Response::Done
        }
        other => {
            return Err(
                ApiError::internal(format!("{} routed to PIM sources", other.wire_type())).into(),
            );
        }
    })
}

fn source_issues(issues: &[PimSourceIssue]) -> crate::error::CoreError {
    let ids: Vec<&str> = issues.iter().map(|i| i.message_id()).collect();
    let mut error = ApiError::invalid_request(format!("invalid source: {}", ids.join(", ")));
    if let Some(first) = issues.first() {
        error = error.with_localized(LocalizedMessage::new(first.message_id()));
    }
    error.into()
}

/// The status view of a source.
fn source_view(
    id: &PimSourceId,
    kind: PimSourceKind,
    config: Option<&PimSourceConfig>,
    record: Option<&PimSourceRecord>,
) -> PimSource {
    let enabled = config.is_none_or(|c| c.enabled);
    let state = if enabled {
        record.map_or(PimSourceState::Idle, |r| r.state)
    } else {
        PimSourceState::Disabled
    };
    let error = record.and_then(|r| r.last_error.clone());
    PimSource {
        id: id.clone(),
        kind,
        name: config
            .and_then(|c| c.name.clone())
            .unwrap_or_else(|| match kind {
                PimSourceKind::Local => String::new(),
                _ => id.to_string(),
            }),
        account_id: config.and_then(|c| c.account().cloned()),
        enabled,
        config: config.cloned(),
        state,
        last_sync_at: record
            .and_then(|r| r.last_sync_at)
            .map(nail_store::pim::time::utc_string),
        error: error.as_ref().map(|e| e.code),
        message: error.as_ref().map(|e| e.message.clone()),
        localized: error.and_then(|e| e.localized.map(|l| *l)),
    }
}

/// Makes sure the local source has its collections: the Collected book,
/// and a calendar, a task list and an address book when it has none.
fn ensure_local_collections(store: &Store, locale: Option<&str>) -> Result<bool> {
    let text = |id: &str| crate::strings::message(locale, id);
    store.ensure_collected_book(&text(COLLECTED_BOOK_NAME_ID))?;
    let local = PimSourceId::local();
    let mut created = false;
    let calendars = store.calendars(Some(&local))?;
    if !calendars.iter().any(Calendar::has_events) {
        store.save_calendar(
            &Calendar {
                id: CalendarId::new(LOCAL_CALENDAR_ID),
                source_id: local.clone(),
                account_id: None,
                settings: CalendarSettings::named(text("pim-calendar-default-name")),
                components: vec![CalendarComponent::Event],
                read_only: false,
                timezone: None,
                exchange: None,
            },
            None,
        )?;
        created = true;
    }
    if !calendars.iter().any(Calendar::has_tasks) {
        let mut settings = CalendarSettings::named(text("pim-task-list-default-name"));
        settings.order = 1;
        store.save_calendar(
            &Calendar {
                id: CalendarId::new(LOCAL_TASKS_ID),
                source_id: local.clone(),
                account_id: None,
                settings,
                components: vec![CalendarComponent::Task],
                read_only: false,
                timezone: None,
                exchange: None,
            },
            None,
        )?;
        created = true;
    }
    let books = store.address_books(Some(&local))?;
    if !books.iter().any(|b| b.role.is_none()) {
        store.save_address_book(
            &AddressBook {
                id: AddressBookId::new(LOCAL_CONTACTS_ID),
                source_id: local,
                account_id: None,
                settings: AddressBookSettings::named(text("pim-address-book-default-name")),
                read_only: false,
                role: None,
                exchange: None,
            },
            None,
        )?;
        created = true;
    }
    Ok(created)
}

impl Service {
    /// The local source's default collections (once per run).
    pub(crate) async fn ensure_local_pim(&self) -> Result<()> {
        if self.inner.pim.local_ready.load(Ordering::Acquire) {
            return Ok(());
        }
        let locale = self.inner.locale.clone();
        let created = self
            .inner
            .store
            .call(move |s| {
                ensure_local_collections(s, locale.as_deref())
                    .map_err(|e| nail_store::Error::InvalidData(e.to_string()))
            })
            .await?;
        self.inner.pim.local_ready.store(true, Ordering::Release);
        // Made before any client asked for them (first start): no event.
        if created {
            tracing::info!("local calendar, task list and address book created");
        }
        Ok(())
    }

    /// Mirrors the configured sources into the store (rows of new sources,
    /// removed sources' data goes) and (re)starts their engines.
    pub(crate) async fn reconcile_pim_sources(&self) {
        let config = self.inner.accounts.config();
        let now = self.pim_now();
        let configured: Vec<(PimSourceId, PimSourceKind)> = config
            .pim_sources
            .iter()
            .map(|s| (s.id.clone(), s.kind()))
            .collect();
        // M3c: the sources of Exchange accounts follow their accounts.
        let exchange: Vec<nail_api::AccountId> = config
            .accounts
            .iter()
            .filter(|a| matches!(a.incoming, nail_api::IncomingServer::Exchange { .. }))
            .map(|a| a.id.clone())
            .collect();
        let result = self
            .inner
            .store
            .call(move |s| {
                for (id, kind) in &configured {
                    s.ensure_pim_source(id, *kind, now)?;
                }
                let mut ids: Vec<PimSourceId> =
                    configured.iter().map(|(id, _)| id.clone()).collect();
                ids.extend(
                    exchange
                        .iter()
                        .map(nail_api::exchange::exchange_pim_source_id),
                );
                s.retain_pim_sources(&ids)
            })
            .await;
        match result {
            Ok(gone) if !gone.is_empty() => {
                tracing::info!(?gone, "PIM sources removed with their data");
                self.reindex_pim();
                self.replan_all_reminders().await;
                self.pim_events([
                    Event::CalendarsChanged,
                    Event::AddressBooksChanged,
                    Event::RemindersChanged,
                ]);
            }
            Ok(_) => {}
            Err(err) => tracing::warn!(%err, "cannot mirror the PIM sources into the store"),
        }
        if self.inner.started.load(Ordering::Relaxed) {
            self.reconcile_pim_engines(&config.pim_sources);
        }
        let changed = {
            let mut last = crate::service::lock(&self.inner.pim.last_sources);
            let changed = *last != config.pim_sources;
            last.clone_from(&config.pim_sources);
            changed
        };
        if changed {
            self.broadcast(Event::PimSourcesChanged);
        }
    }

    async fn list_pim_sources(&self) -> Result<Vec<PimSource>> {
        let config = self.inner.accounts.config();
        let records = self.inner.store.call(Store::pim_sources).await?;
        let local = PimSourceId::local();
        let mut out = vec![source_view(
            &local,
            PimSourceKind::Local,
            None,
            records.iter().find(|r| r.id == local),
        )];
        for source in &config.pim_sources {
            out.push(source_view(
                &source.id,
                source.kind(),
                Some(source),
                records.iter().find(|r| r.id == source.id),
            ));
        }
        Ok(out)
    }

    /// Where a source's own secret lives (`None` when it uses an account's
    /// login or needs none).
    pub(crate) fn pim_secret_ref(config: &PimSourceConfig) -> Option<SecretRef> {
        if config.account().is_some() {
            return None;
        }
        let auth = config.auth()?;
        let kind = auth.secret_kind()?;
        Some(
            auth.explicit_secret()
                .cloned()
                .unwrap_or_else(|| config.default_secret(kind)),
        )
    }

    /// Wakes the source's engine when operations were queued for it.
    /// Saves a collection change and, when `queue`, queues `op` for the
    /// server and wakes the source's engine.
    async fn write_collection(
        &self,
        source: PimSourceId,
        queue: bool,
        op: PimOperation,
        save: impl FnOnce(&Store) -> nail_store::Result<()> + Send + 'static,
    ) -> Result<Vec<nail_store::pim::PimOpId>> {
        let key = source.clone();
        let ops = self
            .inner
            .store
            .call(move |s| {
                save(s)?;
                Ok(if queue {
                    vec![s.enqueue_pim_op(&key, &op)?]
                } else {
                    Vec::new()
                })
            })
            .await?;
        self.wake_for(&ops, &source);
        Ok(ops)
    }

    /// Saves changed collection settings; a server-side change (`queue`)
    /// is queued for the server.
    async fn update_collection(
        &self,
        source: &PimSourceId,
        queue: bool,
        collection: &impl std::fmt::Display,
        save: impl FnOnce(&Store) -> nail_store::Result<()> + Send + 'static,
    ) -> Result<Vec<nail_store::pim::PimOpId>> {
        let op = PimOperation::UpdateCollection {
            collection_id: collection.to_string(),
        };
        self.write_collection(source.clone(), queue, op, save).await
    }

    fn wake_for<T>(&self, ops: &[T], source: &PimSourceId) {
        if !ops.is_empty() {
            self.wake_pim_source(source);
        }
    }

    async fn store_pim_secret(&self, config: &PimSourceConfig, secret: &SecretValue) -> Result<()> {
        let Some(reference) = Self::pim_secret_ref(config) else {
            return Ok(());
        };
        self.store_secret(reference, secret).await?;
        if let SecretValue::OAuth2 { flow_id } = secret {
            drop(self.inner.flows.take(flow_id));
        }
        Ok(())
    }

    async fn save_pim_source(
        &self,
        mut config: PimSourceConfig,
        secret: Option<SecretValue>,
        add: bool,
    ) -> Result<PimSource> {
        let current = self.inner.accounts.config();
        if add {
            if config.id.as_str().is_empty() {
                let hint = config
                    .name
                    .clone()
                    .or_else(|| {
                        config.url().and_then(|u| {
                            url::Url::parse(&nail_api::pim::source::ics_fetch_url(u))
                                .ok()
                                .and_then(|u| u.host_str().map(str::to_owned))
                        })
                    })
                    .or_else(|| config.account().map(ToString::to_string))
                    .unwrap_or_else(|| config.kind().node_name().unwrap_or("source").to_owned());
                let doc_taken = |id: &str| {
                    id == PimSourceId::LOCAL
                        || current.pim_sources.iter().any(|s| s.id.as_str() == id)
                };
                config.id = PimSourceId::new(nail_config::derive_account_id(&hint, doc_taken).0);
            } else if current.pim_source(&config.id).is_some() {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    format!("the name {} is taken", config.id),
                )
                .into());
            }
        } else if current.pim_source(&config.id).is_none() {
            return Err(not_found(format!("PIM source {}", config.id)));
        }
        let issues = config.issues();
        if !issues.is_empty() {
            return Err(source_issues(&issues));
        }
        if let Some(account) = config.account()
            && current.account(account).is_none()
        {
            return Err(ApiError::invalid_localized(
                format!("no account {account}"),
                PimSourceIssue::MissingAuth.message_id(),
            )
            .into());
        }
        let id = config.id.clone();
        let saved = config.clone();
        self.inner
            .accounts
            .edit(|doc| doc.upsert_pim_source(saved).map_err(edit_error))?;
        if let Some(secret) = &secret {
            self.store_pim_secret(&config, secret).await?;
        }
        // Discovery is redone for a changed configuration.
        let reset = id.clone();
        self.inner
            .store
            .call(move |s| {
                if s.pim_source(&reset)?.is_some() {
                    s.set_pim_source_discovery(&reset, None)?;
                }
                Ok(())
            })
            .await?;
        self.reconcile_pim_sources().await;
        self.pim_events([Event::PimSourcesChanged, Event::AccountsChanged]);
        tracing::info!(source = %id, add, "PIM source saved");
        self.list_pim_sources()
            .await?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| not_found(format!("PIM source {id}")))
    }

    async fn remove_pim_source(&self, id: &PimSourceId) -> Result<()> {
        if id.as_str() == PimSourceId::LOCAL {
            return Err(ApiError::invalid_request("the local source cannot be removed").into());
        }
        let config = self
            .inner
            .accounts
            .config()
            .pim_source(id)
            .cloned()
            .ok_or_else(|| not_found(format!("PIM source {id}")))?;
        self.inner
            .accounts
            .edit(|doc| doc.remove_pim_source(id).map_err(edit_error))?;
        if let Some(reference) = Self::pim_secret_ref(&config) {
            self.inner.tokens.manager().forget(&reference);
            let store = Arc::clone(&self.inner.secrets);
            if let Err(err) = tokio::task::spawn_blocking(move || store.delete(&reference)).await? {
                tracing::warn!(%err, "cannot delete a keychain entry");
            }
        }
        self.reconcile_pim_sources().await;
        self.pim_events([
            Event::PimSourcesChanged,
            Event::AccountsChanged,
            Event::CalendarsChanged,
            Event::AddressBooksChanged,
            Event::EventsChanged {
                calendar_ids: Vec::new(),
            },
            Event::ContactsChanged {
                address_book_ids: Vec::new(),
            },
        ]);
        Ok(())
    }

    async fn test_pim_source(
        &self,
        config: &PimSourceConfig,
        secret: Option<SecretValue>,
    ) -> PimSourceTestResult {
        let issues = config.issues();
        if let Some(first) = issues.first() {
            return PimSourceTestResult {
                ok: false,
                discovered_url: None,
                collections: Vec::new(),
                error: Some(ErrorCode::InvalidRequest),
                message: Some(format!("invalid source: {}", first.message_id())),
                localized: Some(LocalizedMessage::new(first.message_id())),
            };
        }
        match super::engine::test_source(self, config, secret).await {
            Ok(result) => result,
            Err(err) => {
                let api: ApiError = err.into();
                PimSourceTestResult {
                    ok: false,
                    discovered_url: None,
                    collections: Vec::new(),
                    error: Some(api.code),
                    message: Some(api.message),
                    localized: api.localized.map(|l| *l),
                }
            }
        }
    }

    /// Fills the mail account of collections from the configuration.
    fn with_accounts<T>(
        &self,
        list: &mut [T],
        parts: impl Fn(&mut T) -> (&PimSourceId, &mut Option<nail_api::AccountId>),
    ) {
        let config = self.inner.accounts.config();
        for item in list {
            let (source, account) = parts(item);
            // M3c: an Exchange account's source names its account.
            *account = nail_api::exchange::exchange_pim_source_account(source)
                .or_else(|| config.pim_source(source).and_then(|s| s.account().cloned()));
        }
    }

    pub(crate) async fn list_calendars(
        &self,
        source: Option<PimSourceId>,
    ) -> Result<Vec<Calendar>> {
        let mut calendars = self
            .inner
            .store
            .call(move |s| s.calendars(source.as_ref()))
            .await?;
        self.with_accounts(&mut calendars, |c| (&c.source_id, &mut c.account_id));
        Ok(calendars)
    }

    pub(crate) async fn calendar(&self, id: &CalendarId) -> Result<Calendar> {
        let key = id.clone();
        let mut calendar = self
            .inner
            .store
            .call(move |s| s.calendar(&key))
            .await?
            .ok_or_else(|| not_found(format!("calendar {id}")))?;
        calendar.account_id = self.pim_source_account(&calendar.source_id);
        Ok(calendar)
    }

    pub(crate) async fn list_address_books(
        &self,
        source: Option<PimSourceId>,
    ) -> Result<Vec<AddressBook>> {
        let mut books = self
            .inner
            .store
            .call(move |s| s.address_books(source.as_ref()))
            .await?;
        self.with_accounts(&mut books, |b| (&b.source_id, &mut b.account_id));
        Ok(books)
    }

    pub(crate) async fn address_book(&self, id: &AddressBookId) -> Result<AddressBook> {
        let key = id.clone();
        let mut book = self
            .inner
            .store
            .call(move |s| s.address_book(&key))
            .await?
            .ok_or_else(|| not_found(format!("address book {id}")))?;
        book.account_id = self.pim_source_account(&book.source_id);
        Ok(book)
    }

    fn source_kind(&self, source: &PimSourceId) -> Result<PimSourceKind> {
        if source.as_str() == PimSourceId::LOCAL {
            return Ok(PimSourceKind::Local);
        }
        self.inner
            .accounts
            .config()
            .pim_source(source)
            .map(PimSourceConfig::kind)
            .ok_or_else(|| not_found(format!("PIM source {source}")))
    }

    async fn create_calendar(
        &self,
        source: PimSourceId,
        settings: CalendarSettings,
        mut components: Vec<CalendarComponent>,
    ) -> Result<(Calendar, Vec<nail_api::OpId>)> {
        check(&settings.issues())?;
        components.sort();
        components.dedup();
        if components.is_empty() {
            return Err(ApiError::invalid_request("a calendar holds events or tasks").into());
        }
        let kind = self.source_kind(&source)?;
        if !matches!(kind, PimSourceKind::Local | PimSourceKind::Caldav) {
            return Err(ApiError::invalid_request("this source cannot hold new calendars").into());
        }
        let calendar = Calendar {
            id: CalendarId::new(new_id("cal")),
            source_id: source.clone(),
            account_id: None,
            settings,
            components,
            read_only: false,
            timezone: None,
            exchange: None,
        };
        let saved = calendar.clone();
        let collection_id = saved.id.0.clone();
        let ops = self
            .write_collection(
                source.clone(),
                kind == PimSourceKind::Caldav,
                PimOperation::CreateCollection { collection_id },
                move |s| s.save_calendar(&saved, None),
            )
            .await?;
        self.broadcast(Event::CalendarsChanged);
        Ok((self.calendar(&calendar.id).await?, op_ids(&ops)))
    }

    async fn update_calendar(
        &self,
        id: CalendarId,
        settings: CalendarSettings,
    ) -> Result<(Calendar, Vec<nail_api::OpId>)> {
        check(&settings.issues())?;
        let before = self.calendar(&id).await?;
        let server_change = self.source_kind(&before.source_id)? == PimSourceKind::Caldav
            && !before.read_only
            && (before.settings.name != settings.name || before.settings.color != settings.color);
        let reminders_changed = before.settings.reminders != settings.reminders;
        let key = id.clone();
        let ops = self
            .update_collection(&before.source_id, server_change, &id, move |s| {
                s.update_calendar_settings(&key, &settings)
            })
            .await?;
        if reminders_changed {
            self.replan_all_reminders().await;
        }
        self.pim_events([
            Event::CalendarsChanged,
            Event::EventsChanged {
                calendar_ids: vec![id.clone()],
            },
            Event::TasksChanged {
                calendar_ids: vec![id.clone()],
            },
        ]);
        Ok((self.calendar(&id).await?, op_ids(&ops)))
    }

    async fn create_address_book(
        &self,
        source: PimSourceId,
        settings: AddressBookSettings,
    ) -> Result<(AddressBook, Vec<nail_api::OpId>)> {
        check(&settings.issues())?;
        let kind = self.source_kind(&source)?;
        if !matches!(kind, PimSourceKind::Local | PimSourceKind::Carddav) {
            return Err(ApiError::invalid_request("this source cannot hold address books").into());
        }
        let book = AddressBook {
            id: AddressBookId::new(new_id("book")),
            source_id: source.clone(),
            account_id: None,
            settings,
            read_only: false,
            role: None,
            exchange: None,
        };
        let saved = book.clone();
        let collection_id = saved.id.0.clone();
        let ops = self
            .write_collection(
                source.clone(),
                kind == PimSourceKind::Carddav,
                PimOperation::CreateCollection { collection_id },
                move |s| s.save_address_book(&saved, None),
            )
            .await?;
        self.broadcast(Event::AddressBooksChanged);
        Ok((self.address_book(&book.id).await?, op_ids(&ops)))
    }

    async fn update_address_book(
        &self,
        id: AddressBookId,
        settings: AddressBookSettings,
    ) -> Result<(AddressBook, Vec<nail_api::OpId>)> {
        check(&settings.issues())?;
        let before = self.address_book(&id).await?;
        let server_change = self.source_kind(&before.source_id)? == PimSourceKind::Carddav
            && !before.read_only
            && before.settings.name != settings.name;
        let key = id.clone();
        let ops = self
            .update_collection(&before.source_id, server_change, &id, move |s| {
                s.update_address_book_settings(&key, &settings)
            })
            .await?;
        self.pim_events([
            Event::AddressBooksChanged,
            Event::ContactsChanged {
                address_book_ids: vec![id.clone()],
            },
        ]);
        Ok((self.address_book(&id).await?, op_ids(&ops)))
    }

    /// `DeleteCalendar` / `DeleteAddressBook`.
    async fn delete_collection(&self, id: String, calendar: bool) -> Result<Vec<nail_api::OpId>> {
        let (source, read_only) = if calendar {
            let c = self.calendar(&CalendarId::new(id.clone())).await?;
            (c.source_id, c.read_only)
        } else {
            let b = self.address_book(&AddressBookId::new(id.clone())).await?;
            if b.role == Some(AddressBookRole::Collected) || b.id.as_str() == COLLECTED_BOOK_ID {
                return Err(forbidden("the Collected address book cannot be deleted"));
            }
            (b.source_id, b.read_only)
        };
        let kind = self.source_kind(&source)?;
        if kind == PimSourceKind::Ics {
            return Err(forbidden(
                "a subscription's calendar goes with the subscription",
            ));
        }
        let remote = matches!(kind, PimSourceKind::Caldav | PimSourceKind::Carddav) && !read_only;
        let key = id.clone();
        let source_id = source.clone();
        let ops = self
            .inner
            .store
            .call(move |s| {
                let href = s.collection_sync(&key)?.href;
                let mut ops = Vec::new();
                if remote && let Some(href) = href {
                    ops.push(
                        s.enqueue_pim_op(&source_id, &PimOperation::DeleteCollection { href })?,
                    );
                }
                s.delete_collection(&key)?;
                Ok(ops)
            })
            .await?;
        self.wake_for(&ops, &source);
        self.pim_index_remove_collection(&id);
        self.replan_all_reminders().await;
        if calendar {
            self.pim_events([
                Event::CalendarsChanged,
                Event::EventsChanged {
                    calendar_ids: vec![CalendarId::new(id.clone())],
                },
                Event::TasksChanged {
                    calendar_ids: vec![CalendarId::new(id)],
                },
                Event::RemindersChanged,
            ]);
        } else {
            self.pim_events([
                Event::AddressBooksChanged,
                Event::ContactsChanged {
                    address_book_ids: vec![AddressBookId::new(id)],
                },
            ]);
        }
        Ok(op_ids(&ops))
    }

    async fn resolve_pim_conflict(&self, item: PimItemRef, keep: ConflictChoice) -> Result<()> {
        let reference = item.clone();
        let object = self
            .inner
            .store
            .call(move |s| s.pim_object(&reference))
            .await?
            .ok_or_else(|| not_found("PIM item"))?;
        match keep {
            ConflictChoice::Server => {
                let reference = item.clone();
                self.inner
                    .store
                    .call(move |s| s.resolve_conflict_keep_server(&reference))
                    .await?;
            }
            ConflictChoice::Local => {
                let Some(local) = object.conflict_raw.clone() else {
                    return Err(ApiError::invalid_request("the item has no conflict").into());
                };
                let parsed = super::engine::parse_item(self, &local, &object.collection_id, &item)?;
                let now = self.pim_now();
                let write = self
                    .inner
                    .store
                    .call(move |s| s.save_pim_item(&parsed, &local, now))
                    .await?;
                let source = self.collection_source(&object.collection_id).await?;
                self.report_queued(&source, &write.op_ids);
            }
        }
        self.after_item_change(&item, &object.collection_id).await;
        Ok(())
    }

    /// The source of a calendar or address book.
    pub(crate) async fn collection_source(&self, collection: &str) -> Result<PimSourceId> {
        let id = collection.to_owned();
        let found = self
            .inner
            .store
            .call(move |s| {
                Ok(match s.calendar(&CalendarId::new(id.clone()))? {
                    Some(c) => Some(c.source_id),
                    None => s
                        .address_book(&AddressBookId::new(id))?
                        .map(|b| b.source_id),
                })
            })
            .await?;
        found.ok_or_else(|| not_found(format!("collection {collection}")))
    }

    /// After an item changed (any way): search index, reminders, events.
    pub(crate) async fn after_item_change(&self, item: &PimItemRef, collection: &str) {
        self.pim_index_update(item).await;
        match item {
            PimItemRef::Event { .. } | PimItemRef::Task { .. } => {
                self.replan_item_reminders(item).await;
                let id = CalendarId::new(collection);
                self.broadcast(match item {
                    PimItemRef::Event { .. } => Event::EventsChanged {
                        calendar_ids: vec![id],
                    },
                    _ => Event::TasksChanged {
                        calendar_ids: vec![id],
                    },
                });
            }
            PimItemRef::Contact { .. } | PimItemRef::ContactGroup { .. } => {
                self.broadcast(Event::ContactsChanged {
                    address_book_ids: vec![AddressBookId::new(collection)],
                });
            }
        }
    }

    /// Reports newly queued operations as pending and wakes the source's
    /// engine to replay them.
    pub(crate) fn report_queued(&self, source: &PimSourceId, ids: &[nail_store::pim::PimOpId]) {
        if ids.is_empty() {
            return;
        }
        for id in ids {
            self.broadcast(Event::PimOpStatus {
                status: nail_api::pim::PimOpStatus {
                    op_id: nail_api::OpId::new(id.to_string()),
                    source_id: source.clone(),
                    state: nail_api::OpState::Pending,
                    error: None,
                    message: None,
                    localized: None,
                },
            });
        }
        self.wake_pim_source(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_defaults_are_made_once() {
        let store = Store::open_in_memory().unwrap();
        assert!(ensure_local_collections(&store, None).unwrap());
        assert!(!ensure_local_collections(&store, None).unwrap());
        let calendars = store.calendars(Some(&PimSourceId::local())).unwrap();
        assert!(calendars.iter().any(Calendar::has_events));
        assert!(calendars.iter().any(Calendar::has_tasks));
        let books = store.address_books(None).unwrap();
        assert_eq!(books.len(), 2, "{books:?}");
        assert!(
            books
                .iter()
                .any(|b| b.role == Some(AddressBookRole::Collected))
        );
        // Names come from the catalog (never Fluent ids).
        assert!(
            calendars
                .iter()
                .all(|c| !c.settings.name.starts_with("pim-"))
        );
    }

    #[test]
    fn source_views() {
        let id = PimSourceId::local();
        let view = source_view(&id, PimSourceKind::Local, None, None);
        assert_eq!(view.state, PimSourceState::Idle);
        assert!(view.name.is_empty() && view.enabled);
        let config = PimSourceConfig {
            id: PimSourceId::new("hol"),
            name: None,
            enabled: false,
            settings: nail_api::pim::PimSourceSettings::Ics {
                url: "https://example.org/h.ics".into(),
                color: None,
                alarms: false,
                refresh_minutes: None,
                auth: None,
            },
        };
        let view = source_view(&config.id, config.kind(), Some(&config), None);
        assert_eq!(view.state, PimSourceState::Disabled);
        assert_eq!(view.name, "hol");
        assert!(Service::pim_secret_ref(&config).is_none());
    }
}
