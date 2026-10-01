//! PIM sync engines: one task per enabled subscription or DAV source.
//!
//! * **ICS subscriptions** (`calendar-subscription`): the feed is fetched
//!   with a conditional GET (`ETag` / `Last-Modified`), parsed with
//!   `nail-ical` (floating times in the feed's `X-WR-TIMEZONE`) into one
//!   read-only calendar; items the feed no longer has go.
//! * **CalDAV / CardDAV**: discovery once (configured URL, `/.well-known/`,
//!   SRV through the OS resolver; ADR 0060) and then, every pass, the PIM
//!   queue is replayed (conditional `PUT` / `DELETE`, `MKCALENDAR` /
//!   `MKCOL`, `PROPPATCH`; `412` = the server wins, the local version is
//!   kept as a conflict copy by the next sync), the home sets are listed
//!   and every collection is synced (`sync-collection`, else ctag + ETag
//!   listing).
//!
//! A pass runs at start, every `refresh` minutes, when the user asks
//! (`SyncPimSources`) and when a write is queued. Failures set the source
//! state (`offline` for retryable errors with a growing backoff,
//! `auth-error`, `error`).

use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::{AuthConfig, SecretValue};
use nail_api::engine::AccessTokenProvider;
use nail_api::pim::source::{DiscoveredCollection, PimSourceTestResult};
use nail_api::pim::{
    AddressBook, AddressBookSettings, Calendar, CalendarComponent, CalendarSettings, ContactGroup,
    PimItemKind, PimItemRef, PimItemSync, PimSourceConfig, PimSourceKind, PimSourceSettings,
    PimSourceState,
};
use nail_api::{
    AddressBookId, ApiError, CalendarId, ContactGroupId, ContactId, ErrorCode, Event, OpState,
    PimSourceId,
};
use nail_dav::{
    CollectionKind, CollectionState, DavAuth, DavClient, DavCollection, DavError, DavService,
    DeleteOutcome, DiscoveryInput, NewCollection, Precondition, SyncOptions, WriteOutcome,
};
use nail_ical::vcard::ParsedCards;
use nail_ical::{ParseOptions, ParsedCalendar};
use nail_store::pim::{
    CollectionSync, PimItem, PimOperation, PimReplayOutcome, PimReplayer, QueuedPimOp, ServerApply,
    ServerRemove,
};
use nail_store::{BlobStore, ReplayPolicy};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use url::Url;

use super::{derived_id, not_found};
use crate::error::{CoreError, Result};
use crate::service::{Service, lock};

/// Largest feed or DAV object accepted.
const MAX_FEED_BYTES: usize = nail_api::pim::import::MAX_PIM_IMPORT_BYTES;

/// A running source engine.
pub(crate) struct SourceHandle {
    config: PimSourceConfig,
    wake: Arc<Notify>,
    task: JoinHandle<()>,
}

impl SourceHandle {
    /// Stops the engine.
    pub(crate) fn stop(self) {
        self.task.abort();
    }
}

/// What a sync pass changed.
#[derive(Debug, Default)]
pub(crate) struct SyncChanges {
    /// Items inserted, updated or removed (with their collection).
    pub items: Vec<(PimItemRef, String)>,
    /// Items removed.
    pub removed: Vec<(PimItemRef, String)>,
    /// Collections added or removed.
    pub collections: bool,
}

// ---------------------------------------------------------------------------
// Parsing server data into store items

/// The calendar items of parsed iCalendar for a collection. Inline
/// attachment data goes to the blob store.
pub(crate) fn calendar_items(
    blobs: &BlobStore,
    parsed: ParsedCalendar,
    collection: &str,
    read_only: bool,
    keep_alarms: bool,
) -> Vec<(PimItem, String)> {
    let mut out = Vec::new();
    for event in parsed.events {
        for data in &event.inline_data {
            if let Err(err) = blobs.put(&data.bytes) {
                tracing::warn!(%err, "cannot store an inline attachment");
            }
        }
        let future = event.this_and_future.clone();
        let raw = event.raw.clone();
        let id = derived_id("ev", collection, &event.uid);
        let mut model = event.into_event(
            id.into(),
            CalendarId::new(collection),
            read_only,
            PimItemSync::Synced,
        );
        for o in &mut model.overrides {
            o.this_and_future |= future.contains(&o.recurrence_id);
        }
        if !keep_alarms {
            model.fields.alarms.clear();
            for o in &mut model.overrides {
                o.fields.alarms.clear();
            }
        }
        out.push((PimItem::Event(model), raw));
    }
    for task in parsed.tasks {
        let raw = task.raw.clone();
        let parent_uid = task.parent_uid.clone();
        let id = derived_id("task", collection, &task.uid);
        let mut model = task.into_task(
            id.into(),
            CalendarId::new(collection),
            None,
            Vec::new(),
            read_only,
            PimItemSync::Synced,
        );
        if !keep_alarms {
            model.fields.alarms.clear();
        }
        out.push((
            PimItem::Task {
                task: model,
                parent_uid,
            },
            raw,
        ));
    }
    out
}

/// The address book items of parsed vCards for a collection. Photos go to
/// the blob store.
pub(crate) fn card_items(
    blobs: &BlobStore,
    parsed: ParsedCards,
    collection: &str,
    read_only: bool,
) -> Vec<(PimItem, String)> {
    let mut out = Vec::new();
    for contact in parsed.contacts {
        if let Some(photo) = &contact.photo
            && let Err(err) = blobs.put(&photo.bytes)
        {
            tracing::warn!(%err, "cannot store a contact photo");
        }
        let id = derived_id("ct", collection, &nail_ical::vcard::uid_key(&contact.uid));
        out.push((
            PimItem::Contact(nail_api::pim::Contact {
                id: ContactId::new(id),
                address_book_id: AddressBookId::new(collection),
                uid: contact.uid,
                fields: contact.fields,
                version: contact.version,
                group_ids: Vec::new(),
                linked_ids: Vec::new(),
                updated_at: contact.updated_at,
                read_only,
                sync: PimItemSync::Synced,
            }),
            contact.raw,
        ));
    }
    for group in parsed.groups {
        let card = group.group;
        let id = derived_id("grp", collection, &nail_ical::vcard::uid_key(&card.uid));
        let members = card
            .member_uids
            .iter()
            .map(|k| format!("urn:uuid:{k}"))
            .chain(card.member_emails.iter().map(|e| format!("mailto:{e}")))
            .collect();
        out.push((
            PimItem::Group {
                group: ContactGroup {
                    id: ContactGroupId::new(id),
                    address_book_id: AddressBookId::new(collection),
                    uid: card.uid,
                    name: card.name,
                    member_ids: Vec::new(),
                    member_emails: card.member_emails,
                    read_only,
                    sync: PimItemSync::Synced,
                },
                members,
            },
            group.raw,
        ));
    }
    out
}

/// Parses a stored raw item again (a kept conflict copy) as the item
/// `item` (its id is kept).
pub(crate) fn parse_item(
    service: &Service,
    raw: &str,
    collection: &str,
    item: &PimItemRef,
) -> Result<PimItem> {
    let blobs = service.blobs();
    let mut items = match item.kind() {
        PimItemKind::Event | PimItemKind::Task => calendar_items(
            blobs,
            nail_ical::parse_calendar(raw, ParseOptions::default()),
            collection,
            false,
            true,
        ),
        PimItemKind::Contact | PimItemKind::ContactGroup => {
            card_items(blobs, nail_ical::vcard::parse_cards(raw), collection, false)
        }
    };
    items.retain(|(i, _)| i.kind() == item.kind());
    let (mut parsed, _) = items
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::invalid_request("the stored copy cannot be read"))?;
    let id = match item {
        PimItemRef::Event { event_id } => event_id.as_str(),
        PimItemRef::Task { task_id } => task_id.as_str(),
        PimItemRef::Contact { contact_id } => contact_id.as_str(),
        PimItemRef::ContactGroup { group_id } => group_id.as_str(),
    };
    match &mut parsed {
        PimItem::Event(e) => e.id = id.into(),
        PimItem::Task { task, .. } => task.id = id.into(),
        PimItem::Contact(c) => c.id = id.into(),
        PimItem::Group { group, .. } => group.id = id.into(),
    }
    Ok(parsed)
}

// ---------------------------------------------------------------------------
// Engines

impl Service {
    /// Starts, restarts and stops source engines to match the
    /// configuration.
    pub(crate) fn reconcile_pim_engines(&self, sources: &[PimSourceConfig]) {
        let mut engines = lock(&self.inner.pim.engines);
        let wanted: Vec<&PimSourceConfig> = sources.iter().filter(|s| s.enabled).collect();
        let stale: Vec<PimSourceId> = engines
            .iter()
            .filter(|(id, handle)| !wanted.iter().any(|s| &s.id == *id && **s == handle.config))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(handle) = engines.remove(&id) {
                handle.stop();
            }
        }
        for config in wanted {
            if engines.contains_key(&config.id) {
                continue;
            }
            let wake = Arc::new(Notify::new());
            let task = tokio::spawn(run_engine(self.clone(), config.clone(), Arc::clone(&wake)));
            engines.insert(
                config.id.clone(),
                SourceHandle {
                    config: config.clone(),
                    wake,
                    task,
                },
            );
        }
        // Disabled sources show as such.
        for config in sources.iter().filter(|s| !s.enabled) {
            let id = config.id.clone();
            let store = self.inner.store.clone();
            tokio::spawn(async move {
                let _ = store
                    .call(move |s| {
                        s.set_pim_source_state(&id, PimSourceState::Disabled, None, None)
                    })
                    .await;
            });
        }
    }

    /// Wakes a source's engine (a write was queued, or the user asked).
    pub(crate) fn wake_pim_source(&self, source: &PimSourceId) {
        if let Some(handle) = lock(&self.inner.pim.engines).get(source) {
            handle.wake.notify_one();
        }
    }

    /// `SyncPimSources`: one source or all.
    pub(crate) fn sync_pim_now(&self, source: Option<&PimSourceId>) {
        let engines = lock(&self.inner.pim.engines);
        for (id, handle) in engines.iter() {
            if source.is_none_or(|s| s == id) {
                handle.wake.notify_one();
            }
        }
    }

    async fn set_source_state(
        &self,
        id: &PimSourceId,
        state: PimSourceState,
        error: Option<ApiError>,
        synced_at: Option<i64>,
    ) {
        let id = id.clone();
        let result = self
            .inner
            .store
            .call(move |s| s.set_pim_source_state(&id, state, error.as_ref(), synced_at))
            .await;
        if let Err(err) = result {
            tracing::debug!(%err, "cannot record a PIM source state");
        }
        self.broadcast(Event::PimSourcesChanged);
    }

    /// After a pass: index, reminders and change events.
    async fn after_sync(&self, changes: SyncChanges) {
        if changes.collections {
            self.pim_events([Event::CalendarsChanged, Event::AddressBooksChanged]);
        }
        if changes.items.is_empty() && changes.removed.is_empty() {
            return;
        }
        let mut calendars: Vec<CalendarId> = Vec::new();
        let mut task_lists: Vec<CalendarId> = Vec::new();
        let mut books: Vec<AddressBookId> = Vec::new();
        for (item, collection) in changes.items.iter().chain(&changes.removed) {
            match item.kind() {
                PimItemKind::Event => calendars.push(CalendarId::new(collection.as_str())),
                PimItemKind::Task => task_lists.push(CalendarId::new(collection.as_str())),
                _ => books.push(AddressBookId::new(collection.as_str())),
            }
        }
        for list in [&mut calendars, &mut task_lists] {
            list.sort();
            list.dedup();
        }
        books.sort();
        books.dedup();
        let items: Vec<PimItemRef> = changes.items.iter().map(|(i, _)| i.clone()).collect();
        let removed: Vec<PimItemRef> = changes.removed.iter().map(|(i, _)| i.clone()).collect();
        self.pim_index_apply(&items, &removed).await;
        if !calendars.is_empty() || !task_lists.is_empty() {
            self.replan_all_reminders().await;
        }
        if !calendars.is_empty() {
            self.broadcast(Event::EventsChanged {
                calendar_ids: calendars,
            });
        }
        if !task_lists.is_empty() {
            self.broadcast(Event::TasksChanged {
                calendar_ids: task_lists,
            });
        }
        if !books.is_empty() {
            self.broadcast(Event::ContactsChanged {
                address_book_ids: books,
            });
        }
    }
}

/// The wait after a pass: the refresh interval (or the timing override),
/// a growing backoff after retryable failures.
fn next_wait(service: &Service, config: &PimSourceConfig, failures: u32) -> Duration {
    let interval =
        service.inner.timing.poll_override.unwrap_or_else(|| {
            Duration::from_secs(u64::from(config.refresh_minutes().max(1)) * 60)
        });
    if failures == 0 {
        return interval;
    }
    let timing = &service.inner.timing;
    let backoff = timing
        .reconnect_min
        .saturating_mul(1 << failures.min(16))
        .min(timing.reconnect_max);
    backoff.min(interval)
}

async fn run_engine(service: Service, config: PimSourceConfig, wake: Arc<Notify>) {
    let mut failures = 0u32;
    loop {
        service
            .set_source_state(&config.id, PimSourceState::Syncing, None, None)
            .await;
        match sync_once(&service, &config).await {
            Ok(changes) => {
                failures = 0;
                let now = service.pim_now();
                service
                    .set_source_state(&config.id, PimSourceState::Idle, None, Some(now))
                    .await;
                service.after_sync(changes).await;
            }
            Err(err) => {
                let api: ApiError = err.into();
                let retryable = matches!(
                    api.code,
                    ErrorCode::Network | ErrorCode::Unavailable | ErrorCode::RateLimited
                );
                let state = match api.code {
                    ErrorCode::AuthFailed => PimSourceState::AuthError,
                    _ if retryable => PimSourceState::Offline,
                    _ => PimSourceState::Error,
                };
                tracing::warn!(source = %config.id, error = %api.message, "PIM sync failed");
                failures = if retryable { failures + 1 } else { 0 };
                service
                    .set_source_state(&config.id, state, Some(api), None)
                    .await;
            }
        }
        let wait = next_wait(&service, &config, failures);
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = wake.notified() => {}
        }
    }
}

/// One pass over a source.
pub(crate) async fn sync_once(service: &Service, config: &PimSourceConfig) -> Result<SyncChanges> {
    match config.kind() {
        PimSourceKind::Ics => sync_ics(service, config).await,
        PimSourceKind::Caldav | PimSourceKind::Carddav => {
            let client = dav_client(service, config, None).await?;
            replay_dav(service, config, &client).await?;
            sync_dav(service, config, &client).await
        }
        PimSourceKind::Local => Ok(SyncChanges::default()),
    }
}

// ---------------------------------------------------------------------------
// ICS subscriptions

/// The calendar id of a subscription.
pub(crate) fn feed_calendar_id(source: &PimSourceId) -> CalendarId {
    CalendarId::new(format!("{source}-feed"))
}

/// Fetches a feed; `None` when it did not change (`304`).
async fn fetch_feed(
    service: &Service,
    config: &PimSourceConfig,
    known: Option<&CollectionSync>,
    secret: Option<&SecretValue>,
) -> Result<Option<(Vec<u8>, Option<String>, Option<String>, String)>> {
    let PimSourceSettings::Ics { url, auth, .. } = &config.settings else {
        return Err(ApiError::internal("not a subscription").into());
    };
    let fetch = nail_ical::feed::fetch_url(url).ok_or_else(|| {
        ApiError::invalid_request(format!("not a subscription URL: {url}")).with_localized(
            nail_api::LocalizedMessage::new(
                nail_api::pim::source::PimSourceIssue::InvalidUrl.message_id(),
            ),
        )
    })?;
    let http = nail_auth::http_client()
        .map_err(|e| ApiError::internal(format!("cannot build the HTTP client: {e}")))?;
    let mut request = http.get(&fetch).header(
        reqwest::header::ACCEPT,
        "text/calendar, text/plain;q=0.5, */*;q=0.1",
    );
    if let Some(known) = known {
        if let Some(etag) = &known.etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if let Some(modified) = &known.last_modified {
            request = request.header(reqwest::header::IF_MODIFIED_SINCE, modified);
        }
    }
    if let Some(AuthConfig::Password { username, .. }) = auth {
        let password = match secret {
            Some(SecretValue::Password { password }) => Some(password.clone()),
            _ => match Service::pim_secret_ref(config) {
                Some(reference) => {
                    let store = Arc::clone(&service.inner.secrets);
                    tokio::task::spawn_blocking(move || store.get(&reference)).await??
                }
                None => None,
            },
        };
        request = request.basic_auth(username, password);
    }
    let response = tokio::time::timeout(Duration::from_secs(60), request.send())
        .await
        .map_err(|_| ApiError::new(ErrorCode::Network, "the feed did not answer in time"))?
        .map_err(|e| ApiError::new(ErrorCode::Network, format!("cannot fetch the feed: {e}")))?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(
            ApiError::new(ErrorCode::AuthFailed, format!("the feed answered {status}")).into(),
        );
    }
    if !status.is_success() {
        let code = if status.is_server_error() {
            ErrorCode::Unavailable
        } else {
            ErrorCode::NotFound
        };
        return Err(ApiError::new(code, format!("the feed answered {status}")).into());
    }
    let header = |name: reqwest::header::HeaderName| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let etag = header(reqwest::header::ETAG);
    let modified = header(reqwest::header::LAST_MODIFIED);
    let bytes = response
        .bytes()
        .await
        .map_err(|e| ApiError::new(ErrorCode::Network, format!("cannot read the feed: {e}")))?;
    if bytes.len() > MAX_FEED_BYTES {
        return Err(ApiError::new(ErrorCode::TooLarge, "the feed is too large").into());
    }
    Ok(Some((bytes.to_vec(), etag, modified, fetch)))
}

async fn sync_ics(service: &Service, config: &PimSourceConfig) -> Result<SyncChanges> {
    let PimSourceSettings::Ics { color, alarms, .. } = &config.settings else {
        return Ok(SyncChanges::default());
    };
    let calendar_id = feed_calendar_id(&config.id);
    let key = calendar_id.clone();
    let known = service
        .inner
        .store
        .call(move |s| {
            Ok(match s.calendar(&key)? {
                Some(_) => Some(s.collection_sync(key.as_str())?),
                None => None,
            })
        })
        .await?;
    let Some((bytes, etag, modified, fetch)) =
        fetch_feed(service, config, known.as_ref(), None).await?
    else {
        return Ok(SyncChanges::default());
    };
    let feed = nail_ical::feed::parse_feed_bytes(&bytes);
    if let Some(issue) = feed
        .issues
        .iter()
        .find(|i| matches!(i.kind, nail_ical::IssueKind::NotICalendar))
    {
        return Err(ApiError::new(ErrorCode::Rejected, issue.diagnostic())
            .with_localized(issue.localized())
            .into());
    }
    let mut changes = SyncChanges::default();
    if known.is_none() {
        let mut settings = CalendarSettings::named(
            config
                .name
                .clone()
                .or_else(|| feed.name.clone())
                .unwrap_or_else(|| config.id.to_string()),
        );
        settings.color = color
            .clone()
            .or_else(|| feed.color.clone())
            .filter(|c| nail_api::pim::is_color(c));
        settings.reminders = *alarms;
        let mut components = vec![CalendarComponent::Event];
        if !feed.tasks.is_empty() {
            components.push(CalendarComponent::Task);
        }
        let calendar = Calendar {
            id: calendar_id.clone(),
            source_id: config.id.clone(),
            account_id: None,
            settings,
            components,
            read_only: true,
            timezone: feed.time_zone.clone(),
        };
        service
            .inner
            .store
            .call(move |s| s.save_calendar(&calendar, None))
            .await?;
        changes.collections = true;
    }
    let items = calendar_items(service.blobs(), feed, calendar_id.as_str(), true, *alarms);
    let uids: Vec<String> = items.iter().map(|(i, _)| i.uid().to_owned()).collect();
    let now = service.pim_now();
    let collection = calendar_id.0.clone();
    let (applied, removed) = service
        .inner
        .store
        .call(move |s| {
            let mut applied = Vec::new();
            for (item, raw) in &items {
                match s.apply_server_item(item, raw, None, None, now) {
                    Ok(result) if result.changed() => applied.push(result.item().clone()),
                    Ok(_) => {}
                    Err(err) => tracing::warn!(uid = item.uid(), %err, "cannot store a feed item"),
                }
            }
            let removed = s.retain_server_uids(&collection, &uids)?;
            s.set_collection_sync(
                &collection,
                &CollectionSync {
                    href: Some(fetch),
                    ctag: None,
                    sync_token: None,
                    etag,
                    last_modified: modified,
                    last_sync_at: Some(now),
                },
            )?;
            Ok((applied, removed))
        })
        .await?;
    changes.items = applied
        .into_iter()
        .map(|i| (i, calendar_id.0.clone()))
        .collect();
    changes.removed = removed
        .into_iter()
        .filter_map(|r| match r {
            ServerRemove::Removed(i) | ServerRemove::KeptAsConflict(i) => {
                Some((i, calendar_id.0.clone()))
            }
            ServerRemove::Missing => None,
        })
        .collect();
    Ok(changes)
}

// ---------------------------------------------------------------------------
// CalDAV / CardDAV

fn dav_service(config: &PimSourceConfig) -> DavService {
    match config.kind() {
        PimSourceKind::Carddav => DavService::CardDav,
        _ => DavService::CalDav,
    }
}

/// The DAV client of a source: the account's login (`account=`) or the
/// source's own (`auth`), with `secret` instead of the stored one (tests
/// of an unsaved configuration).
pub(crate) async fn dav_client(
    service: &Service,
    config: &PimSourceConfig,
    secret: Option<&SecretValue>,
) -> Result<DavClient> {
    let accounts = service.inner.accounts.config();
    let (auth, reference) = match config.account() {
        Some(account) => {
            let account_config = accounts
                .account(account)
                .ok_or_else(|| not_found(format!("account {account}")))?;
            let auth = account_config.incoming.server().auth.clone();
            let reference = auth.secret_ref(account);
            (auth, reference)
        }
        None => {
            let auth = config.auth().cloned().unwrap_or(AuthConfig::None);
            (auth, Service::pim_secret_ref(config))
        }
    };
    let dav_auth = match auth {
        AuthConfig::None => DavAuth::None,
        AuthConfig::Password { username, .. } => {
            let password = match secret {
                Some(SecretValue::Password { password }) => password.clone(),
                _ => {
                    let reference = reference
                        .ok_or_else(|| ApiError::new(ErrorCode::Config, "no secret reference"))?;
                    let store = Arc::clone(&service.inner.secrets);
                    let r = reference.clone();
                    tokio::task::spawn_blocking(move || store.get(&r))
                        .await??
                        .ok_or_else(|| {
                            ApiError::new(
                                ErrorCode::AuthFailed,
                                format!("no password stored under {reference}"),
                            )
                        })?
                }
            };
            DavAuth::Basic { username, password }
        }
        AuthConfig::OAuth2 { provider, .. } => {
            if let Some(SecretValue::OAuth2 { flow_id }) = secret {
                let (_, tokens) = service.inner.flows.peek(flow_id)?;
                DavAuth::Bearer(tokens.access_token.clone())
            } else {
                let reference = reference
                    .ok_or_else(|| ApiError::new(ErrorCode::Config, "no secret reference"))?;
                DavAuth::OAuth2 {
                    tokens: Arc::clone(&service.inner.tokens) as Arc<dyn AccessTokenProvider>,
                    provider,
                    secret: reference,
                }
            }
        }
        AuthConfig::ClientCertificate { .. } => {
            return Err(ApiError::unsupported("client certificates are not supported yet").into());
        }
    };
    DavClient::new(dav_auth).map_err(dav_error)
}

/// A DAV error as a core error (code, retry hint, `dav-*` text).
pub(crate) fn dav_error(err: DavError) -> CoreError {
    let engine: nail_api::engine::EngineError = err.into();
    engine.into()
}

/// The email address discovery may search the domain of.
fn discovery_email(service: &Service, config: &PimSourceConfig) -> Option<String> {
    let accounts = service.inner.accounts.config();
    match config.account() {
        Some(account) => accounts.account(account).map(|a| a.email.clone()),
        None => {
            match config.auth() {
                Some(
                    AuthConfig::Password { username, .. } | AuthConfig::OAuth2 { username, .. },
                ) if username.contains('@') => Some(username.clone()),
                _ => None,
            }
        }
    }
}

/// What discovery found, as stored with the source.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct StoredDiscovery {
    context_url: String,
    principal: String,
    home_sets: Vec<String>,
}

/// The source's collections on the server (discovering first when needed).
async fn server_collections(
    service: &Service,
    config: &PimSourceConfig,
    client: &DavClient,
) -> Result<(StoredDiscovery, Vec<DavCollection>)> {
    let id = config.id.clone();
    let stored: Option<StoredDiscovery> = service
        .inner
        .store
        .call(move |s| Ok(s.pim_source(&id)?.and_then(|r| r.discovery)))
        .await?
        .and_then(|v| serde_json::from_value(v).ok());
    let kind = dav_service(config);
    if let Some(found) = stored {
        let homes: Vec<Url> = found
            .home_sets
            .iter()
            .filter_map(|h| Url::parse(h).ok())
            .collect();
        let collections = if homes.is_empty() {
            let url = Url::parse(&found.context_url)
                .map_err(|e| ApiError::internal(format!("stored discovery: {e}")))?;
            vec![client.collection(&url).await.map_err(dav_error)?]
        } else {
            nail_dav::list_collections(client, kind, &homes)
                .await
                .map_err(dav_error)?
        };
        return Ok((found, collections));
    }
    let dns = nail_discovery::SystemDns::new()
        .map_err(|e| ApiError::new(ErrorCode::Network, format!("DNS: {e}")))?;
    let discovery = nail_dav::discover(
        client,
        &dns,
        &DiscoveryInput {
            service: kind,
            url: config.url().map(str::to_owned),
            email: discovery_email(service, config),
        },
    )
    .await
    .map_err(dav_error)?;
    let found = StoredDiscovery {
        context_url: discovery.context_url.to_string(),
        principal: discovery.principal.to_string(),
        home_sets: discovery.home_sets.iter().map(Url::to_string).collect(),
    };
    let value = serde_json::to_value(&found).ok();
    let id = config.id.clone();
    service
        .inner
        .store
        .call(move |s| s.set_pim_source_discovery(&id, value.as_ref()))
        .await?;
    Ok((found, discovery.collections))
}

/// Tries a configuration: subscriptions are fetched, DAV sources
/// discovered (nothing is stored).
pub(crate) async fn test_source(
    service: &Service,
    config: &PimSourceConfig,
    secret: Option<SecretValue>,
) -> Result<PimSourceTestResult> {
    match config.kind() {
        PimSourceKind::Ics => {
            let (bytes, _, _, fetch) = fetch_feed(service, config, None, secret.as_ref())
                .await?
                .ok_or_else(|| ApiError::internal("unexpected 304"))?;
            let feed = nail_ical::feed::parse_feed_bytes(&bytes);
            if let Some(issue) = nail_ical::import::fatal_issue(
                &nail_ical::import::ImportData::Calendar(feed.clone()),
            ) {
                return Err(ApiError::new(ErrorCode::Rejected, issue.diagnostic())
                    .with_localized(issue.localized())
                    .into());
            }
            Ok(PimSourceTestResult {
                ok: true,
                discovered_url: Some(fetch.clone()),
                collections: vec![DiscoveredCollection {
                    url: fetch,
                    name: feed
                        .name
                        .clone()
                        .or_else(|| config.name.clone())
                        .unwrap_or_else(|| config.id.to_string()),
                    components: vec![CalendarComponent::Event],
                    address_book: false,
                    read_only: true,
                    color: feed.color.filter(|c| nail_api::pim::is_color(c)),
                }],
                error: None,
                message: None,
                localized: None,
            })
        }
        PimSourceKind::Caldav | PimSourceKind::Carddav => {
            let client = dav_client(service, config, secret.as_ref()).await?;
            let dns = nail_discovery::SystemDns::new()
                .map_err(|e| ApiError::new(ErrorCode::Network, format!("DNS: {e}")))?;
            let discovery = nail_dav::discover(
                &client,
                &dns,
                &DiscoveryInput {
                    service: dav_service(config),
                    url: config.url().map(str::to_owned),
                    email: discovery_email(service, config),
                },
            )
            .await
            .map_err(dav_error)?;
            Ok(PimSourceTestResult {
                ok: true,
                discovered_url: Some(discovery.context_url.to_string()),
                collections: discovery
                    .collections
                    .iter()
                    .map(DavCollection::to_discovered)
                    .collect(),
                error: None,
                message: None,
                localized: None,
            })
        }
        PimSourceKind::Local => Ok(PimSourceTestResult {
            ok: true,
            discovered_url: None,
            collections: Vec::new(),
            error: None,
            message: None,
            localized: None,
        }),
    }
}

/// Syncs every collection of a DAV source.
async fn sync_dav(
    service: &Service,
    config: &PimSourceConfig,
    client: &DavClient,
) -> Result<SyncChanges> {
    let (_, collections) = server_collections(service, config, client).await?;
    let kind = dav_service(config).kind();
    let source = config.id.clone();
    // Match server collections to store rows (by URL), make new ones.
    let found: Vec<DavCollection> = collections.into_iter().filter(|c| c.kind == kind).collect();
    let listing = found.clone();
    let (ids, added, gone) = service
        .inner
        .store
        .call(move |s| {
            let mut ids = Vec::new();
            let mut added = false;
            for collection in &listing {
                let href = collection.url.to_string();
                let existing = s.collection_by_href(&source, &href)?;
                let id = existing.clone().unwrap_or_else(|| {
                    derived_id(
                        if kind == CollectionKind::Calendar {
                            "cal"
                        } else {
                            "book"
                        },
                        source.as_str(),
                        &href,
                    )
                });
                match kind {
                    CollectionKind::Calendar => {
                        let before = s.calendar(&CalendarId::new(id.clone()))?;
                        let settings = before.as_ref().map_or_else(
                            || {
                                let mut settings = CalendarSettings::named(collection.name());
                                settings.description = collection.description.clone();
                                settings.color = collection
                                    .color
                                    .clone()
                                    .filter(|c| nail_api::pim::is_color(c));
                                settings
                            },
                            |c| c.settings.clone(),
                        );
                        let components = if collection.components.is_empty() {
                            vec![CalendarComponent::Event, CalendarComponent::Task]
                        } else {
                            collection.components.clone()
                        };
                        s.save_calendar(
                            &Calendar {
                                id: CalendarId::new(id.clone()),
                                source_id: source.clone(),
                                account_id: None,
                                settings,
                                components,
                                read_only: collection.read_only,
                                timezone: before.and_then(|c| c.timezone),
                            },
                            Some(&href),
                        )?;
                    }
                    CollectionKind::AddressBook => {
                        let before = s.address_book(&AddressBookId::new(id.clone()))?;
                        let settings = before.map_or_else(
                            || {
                                let mut settings = AddressBookSettings::named(collection.name());
                                settings.description = collection.description.clone();
                                settings
                            },
                            |b| b.settings,
                        );
                        s.save_address_book(
                            &AddressBook {
                                id: AddressBookId::new(id.clone()),
                                source_id: source.clone(),
                                account_id: None,
                                settings,
                                read_only: collection.read_only,
                                role: None,
                            },
                            Some(&href),
                        )?;
                    }
                }
                added |= existing.is_none();
                ids.push(id);
            }
            // Collections the server no longer has (made here and not
            // created yet have no URL and stay).
            let known: Vec<String> = match kind {
                CollectionKind::Calendar => s
                    .calendars(Some(&source))?
                    .into_iter()
                    .map(|c| c.id.0)
                    .collect(),
                CollectionKind::AddressBook => s
                    .address_books(Some(&source))?
                    .into_iter()
                    .map(|b| b.id.0)
                    .collect(),
            };
            let mut gone = false;
            for id in known {
                if ids.contains(&id) {
                    continue;
                }
                if s.collection_sync(&id)?.href.is_some() {
                    s.delete_collection(&id)?;
                    gone = true;
                }
            }
            Ok((ids, added, gone))
        })
        .await?;
    let mut changes = SyncChanges {
        collections: added || gone,
        ..SyncChanges::default()
    };
    for (collection, id) in found.iter().zip(ids) {
        sync_dav_collection(service, client, collection, &id, &mut changes).await?;
    }
    Ok(changes)
}

async fn sync_dav_collection(
    service: &Service,
    client: &DavClient,
    collection: &DavCollection,
    id: &str,
    changes: &mut SyncChanges,
) -> Result<()> {
    let key = id.to_owned();
    let (sync, objects) = service
        .inner
        .store
        .call(move |s| Ok((s.collection_sync(&key)?, s.server_objects(&key)?)))
        .await?;
    let mut etags = BTreeMap::new();
    for object in &objects {
        if let Some(href) = &object.href {
            etags.insert(href.clone(), object.etag.clone().unwrap_or_default());
        }
    }
    let previous = CollectionState {
        sync_token: sync.sync_token.clone(),
        ctag: sync.ctag.clone(),
        etags,
    };
    let outcome = nail_dav::sync_collection(
        client,
        &collection.url,
        collection.kind,
        &previous,
        SyncOptions::default(),
    )
    .await
    .map_err(dav_error)?;
    let read_only = collection.read_only;
    let blobs = service.blobs().clone();
    let now = service.pim_now();
    let key = id.to_owned();
    let kind = collection.kind;
    let (applied, removed) = service
        .inner
        .store
        .call(move |s| {
            let mut applied = Vec::new();
            let mut removed = Vec::new();
            for object in &outcome.changed {
                let Some(data) = &object.data else { continue };
                let items = match kind {
                    CollectionKind::Calendar => calendar_items(
                        &blobs,
                        nail_ical::parse_calendar(data, ParseOptions::default()),
                        &key,
                        read_only,
                        true,
                    ),
                    CollectionKind::AddressBook => {
                        card_items(&blobs, nail_ical::vcard::parse_cards(data), &key, read_only)
                    }
                };
                for (item, _) in &items {
                    match s.apply_server_item(
                        item,
                        data,
                        Some(&object.href),
                        object.etag.as_deref(),
                        now,
                    ) {
                        Ok(
                            result @ (ServerApply::Inserted(_)
                            | ServerApply::Updated(_)
                            | ServerApply::Conflict(_)),
                        ) => applied.push(result.item().clone()),
                        Ok(_) => {}
                        Err(err) => {
                            tracing::warn!(href = %object.href, %err, "cannot store a DAV item");
                        }
                    }
                }
            }
            for href in &outcome.removed {
                match s.remove_server_item(&key, Some(href), None)? {
                    ServerRemove::Removed(i) | ServerRemove::KeptAsConflict(i) => removed.push(i),
                    ServerRemove::Missing => {}
                }
            }
            let mut sync = s.collection_sync(&key)?;
            sync.sync_token = outcome.state.sync_token.clone();
            sync.ctag = outcome.state.ctag.clone();
            sync.last_sync_at = Some(now);
            s.set_collection_sync(&key, &sync)?;
            Ok((applied, removed))
        })
        .await?;
    changes
        .items
        .extend(applied.into_iter().map(|i| (i, id.to_owned())));
    changes
        .removed
        .extend(removed.into_iter().map(|i| (i, id.to_owned())));
    Ok(())
}

/// Replays a DAV source's queued writes and reports their outcome.
async fn replay_dav(service: &Service, config: &PimSourceConfig, client: &DavClient) -> Result<()> {
    let replayer = DavReplayer {
        service: service.clone(),
        client: client.clone(),
        config: config.clone(),
    };
    let report = service
        .inner
        .store
        .replay_pim_source(&config.id, &replayer, ReplayPolicy::default())
        .await?;
    for (ids, state) in [
        (&report.done, OpState::Done),
        (&report.failed, OpState::Failed),
    ] {
        for id in ids {
            let op = *id;
            let error = service
                .inner
                .store
                .call(move |s| Ok(s.pim_op(op)?.last_error))
                .await
                .ok()
                .flatten();
            service.broadcast(Event::PimOpStatus {
                status: nail_api::pim::PimOpStatus {
                    op_id: nail_api::OpId::new(id.to_string()),
                    source_id: config.id.clone(),
                    state,
                    error: (state == OpState::Failed).then_some(ErrorCode::Rejected),
                    message: error,
                    localized: None,
                },
            });
        }
    }
    if !report.done.is_empty() || !report.failed.is_empty() {
        // Sync states changed (pending → synced).
        service.pim_events([
            Event::EventsChanged {
                calendar_ids: Vec::new(),
            },
            Event::TasksChanged {
                calendar_ids: Vec::new(),
            },
            Event::ContactsChanged {
                address_book_ids: Vec::new(),
            },
        ]);
    }
    let _ = service.inner.store.call(|s| s.purge_done_pim_ops()).await;
    Ok(())
}

struct DavReplayer {
    service: Service,
    client: DavClient,
    config: PimSourceConfig,
}

fn outcome_of(err: &DavError) -> PimReplayOutcome {
    match err {
        DavError::PreconditionFailed => PimReplayOutcome::Conflict(err.to_string()),
        e if e.retryable() => PimReplayOutcome::Retry(e.to_string()),
        e => PimReplayOutcome::Fail(e.to_string()),
    }
}

impl DavReplayer {
    async fn collection_url(&self, collection: &str) -> std::result::Result<Option<Url>, String> {
        let key = collection.to_owned();
        let sync = self
            .service
            .inner
            .store
            .call(move |s| s.collection_sync(&key))
            .await
            .map_err(|e| e.to_string())?;
        Ok(sync.href.and_then(|h| Url::parse(&h).ok()))
    }

    async fn run(&self, op: &QueuedPimOp) -> std::result::Result<PimReplayOutcome, String> {
        let store = &self.service.inner.store;
        match &op.operation {
            PimOperation::Put { object_id } => {
                let id = *object_id;
                let Some(object) = store
                    .call(move |s| s.pim_object_by_id(id))
                    .await
                    .map_err(|e| e.to_string())?
                else {
                    return Ok(PimReplayOutcome::Done {
                        href: None,
                        etag: None,
                    });
                };
                let Some(base) = self.collection_url(&object.collection_id).await? else {
                    return Ok(PimReplayOutcome::Retry(
                        "the collection is not on the server yet".into(),
                    ));
                };
                let kind = match object.kind {
                    PimItemKind::Event | PimItemKind::Task => CollectionKind::Calendar,
                    _ => CollectionKind::AddressBook,
                };
                let url = match &object.href {
                    Some(href) => base.join(href).map_err(|e| e.to_string())?,
                    None => nail_dav::member_url(&base, &object.uid, kind),
                };
                let etag = object.href.as_ref().and(object.etag.as_deref());
                match nail_dav::write_object(&self.client, &url, kind, &object.raw, etag).await {
                    Ok(WriteOutcome::Written { etag, .. }) => Ok(PimReplayOutcome::Done {
                        href: Some(nail_dav::href_key(&url)),
                        etag,
                    }),
                    Ok(WriteOutcome::Conflict(_)) => Ok(PimReplayOutcome::Conflict(
                        "the item changed on the server".into(),
                    )),
                    Err(e) => Ok(outcome_of(&e)),
                }
            }
            PimOperation::Delete {
                object_id,
                collection_id,
                href,
                etag,
            } => {
                let mut href = href.clone();
                let mut etag = etag.clone();
                if href.is_none()
                    && let Some(id) = *object_id
                    && let Some(object) = store
                        .call(move |s| s.pim_object_by_id(id))
                        .await
                        .map_err(|e| e.to_string())?
                {
                    href = object.href;
                    etag = object.etag;
                }
                let Some(href) = href else {
                    return Ok(PimReplayOutcome::Done {
                        href: None,
                        etag: None,
                    });
                };
                let Some(base) = self.collection_url(collection_id).await? else {
                    return Ok(PimReplayOutcome::Done {
                        href: None,
                        etag: None,
                    });
                };
                let url = base.join(&href).map_err(|e| e.to_string())?;
                match nail_dav::delete_object(&self.client, &url, etag.as_deref()).await {
                    Ok(DeleteOutcome::Deleted | DeleteOutcome::AlreadyGone) => {
                        Ok(PimReplayOutcome::Done {
                            href: None,
                            etag: None,
                        })
                    }
                    Ok(DeleteOutcome::Conflict(_)) => Ok(PimReplayOutcome::Conflict(
                        "the item changed on the server".into(),
                    )),
                    Err(e) => Ok(outcome_of(&e)),
                }
            }
            PimOperation::CreateCollection { collection_id } => {
                self.create_collection(collection_id).await
            }
            PimOperation::UpdateCollection { collection_id } => {
                let Some(url) = self.collection_url(collection_id).await? else {
                    return Ok(PimReplayOutcome::Retry(
                        "the collection is not on the server yet".into(),
                    ));
                };
                let key = collection_id.clone();
                let (name, color) = store
                    .call(move |s| {
                        Ok(match s.calendar(&CalendarId::new(key.clone()))? {
                            Some(c) => (c.settings.name, c.settings.color),
                            None => (
                                s.address_book(&AddressBookId::new(key))?
                                    .map(|b| b.settings.name)
                                    .unwrap_or_default(),
                                None,
                            ),
                        })
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                match self
                    .client
                    .proppatch(&url, Some(&name), color.as_deref())
                    .await
                {
                    Ok(()) => Ok(PimReplayOutcome::Done {
                        href: None,
                        etag: None,
                    }),
                    Err(e) => Ok(outcome_of(&e)),
                }
            }
            PimOperation::DeleteCollection { href } => {
                let url = Url::parse(href).map_err(|e| e.to_string())?;
                match self.client.delete(&url, &Precondition::None).await {
                    Ok(()) | Err(DavError::NotFound(_)) => Ok(PimReplayOutcome::Done {
                        href: None,
                        etag: None,
                    }),
                    Err(e) => Ok(outcome_of(&e)),
                }
            }
        }
    }

    async fn create_collection(
        &self,
        collection_id: &str,
    ) -> std::result::Result<PimReplayOutcome, String> {
        let store = &self.service.inner.store;
        let (_, _) = (store, collection_id);
        let found = server_collections(&self.service, &self.config, &self.client).await;
        let discovery = match found {
            Ok((discovery, _)) => discovery,
            Err(e) => {
                let api: ApiError = e.into();
                return Ok(PimReplayOutcome::Retry(api.message));
            }
        };
        let Some(home) = discovery.home_sets.first().and_then(|h| Url::parse(h).ok()) else {
            return Ok(PimReplayOutcome::Fail(
                "the server has no place for new collections".into(),
            ));
        };
        let key = collection_id.to_owned();
        let found = store
            .call(move |s| {
                Ok((
                    s.calendar(&CalendarId::new(key.clone()))?,
                    s.address_book(&AddressBookId::new(key))?,
                ))
            })
            .await
            .map_err(|e| e.to_string())?;
        let slug = super::new_uuid();
        let url = home.join(&format!("{slug}/")).map_err(|e| e.to_string())?;
        let result = match found {
            (Some(calendar), _) => {
                let new = NewCollection {
                    display_name: calendar.settings.name.clone(),
                    description: calendar.settings.description.clone(),
                    color: calendar.settings.color.clone(),
                    components: calendar.components.clone(),
                };
                self.client.mkcalendar(&url, &new).await
            }
            (None, Some(book)) => {
                let new = NewCollection {
                    display_name: book.settings.name.clone(),
                    description: book.settings.description.clone(),
                    color: None,
                    components: Vec::new(),
                };
                self.client.mkaddressbook(&url, &new).await
            }
            (None, None) => {
                return Ok(PimReplayOutcome::Done {
                    href: None,
                    etag: None,
                });
            }
        };
        if let Err(e) = result {
            return Ok(outcome_of(&e));
        }
        let key = collection_id.to_owned();
        let href = url.to_string();
        store
            .call(move |s| {
                let mut sync = s.collection_sync(&key)?;
                sync.href = Some(href);
                s.set_collection_sync(&key, &sync)
            })
            .await
            .map_err(|e| e.to_string())?;
        Ok(PimReplayOutcome::Done {
            href: None,
            etag: None,
        })
    }
}

impl PimReplayer for DavReplayer {
    fn replay(&self, op: &QueuedPimOp) -> impl Future<Output = PimReplayOutcome> + Send {
        let op = op.clone();
        async move {
            match self.run(&op).await {
                Ok(outcome) => outcome,
                Err(err) => PimReplayOutcome::Retry(err),
            }
        }
    }
}

/// Source ids of the DAV sources configured now (for tests).
#[allow(dead_code)]
pub(crate) fn dav_source_ids(sources: &[PimSourceConfig]) -> HashSet<PimSourceId> {
    sources
        .iter()
        .filter(|s| matches!(s.kind(), PimSourceKind::Caldav | PimSourceKind::Carddav))
        .map(|s| s.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//T//EN\r\n\
BEGIN:VEVENT\r\nUID:a@x\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261005T090000Z\r\nDTEND:20261005T100000Z\r\nSUMMARY:A\r\n\
BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nDESCRIPTION:A\r\nEND:VALARM\r\nEND:VEVENT\r\n\
BEGIN:VTODO\r\nUID:t@x\r\nDTSTAMP:20261001T000000Z\r\nSUMMARY:T\r\nRELATED-TO;RELTYPE=PARENT:p@x\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    #[test]
    fn calendar_items_have_derived_ids_and_optional_alarms() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(dir.path()).unwrap();
        let parsed = nail_ical::parse_calendar(ICS, ParseOptions::default());
        let items = calendar_items(&blobs, parsed.clone(), "c1", true, false);
        assert_eq!(items.len(), 2);
        let PimItem::Event(event) = &items[0].0 else {
            panic!("an event first")
        };
        assert!(event.read_only && event.fields.alarms.is_empty());
        assert_eq!(event.id.as_str(), derived_id("ev", "c1", "a@x"));
        let PimItem::Task { parent_uid, .. } = &items[1].0 else {
            panic!("then a task")
        };
        assert_eq!(parent_uid.as_deref(), Some("p@x"));
        let again = calendar_items(&blobs, parsed, "c1", false, true);
        let PimItem::Event(event) = &again[0].0 else {
            panic!()
        };
        assert_eq!(event.fields.alarms.len(), 1);
    }

    #[test]
    fn card_items_keep_members() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(dir.path()).unwrap();
        let vcf = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:AAA\r\nFN:Ada\r\nEMAIL:ada@x.example\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:4.0\r\nKIND:group\r\nUID:urn:uuid:G1\r\nFN:Team\r\nMEMBER:urn:uuid:aaa\r\nMEMBER:mailto:bob@x.example\r\nEND:VCARD\r\n";
        let items = card_items(&blobs, nail_ical::vcard::parse_cards(vcf), "b1", false);
        assert_eq!(items.len(), 2);
        let PimItem::Group { members, .. } = &items[1].0 else {
            panic!("a group")
        };
        assert_eq!(members, &["urn:uuid:aaa", "mailto:bob@x.example"]);
    }

    #[test]
    fn replay_outcomes() {
        assert!(matches!(
            outcome_of(&DavError::PreconditionFailed),
            PimReplayOutcome::Conflict(_)
        ));
        assert!(matches!(
            outcome_of(&DavError::Timeout),
            PimReplayOutcome::Retry(_)
        ));
        assert!(matches!(
            outcome_of(&DavError::AuthFailed),
            PimReplayOutcome::Fail(_)
        ));
        assert_eq!(feed_calendar_id(&PimSourceId::new("h")).as_str(), "h-feed");
    }
}
