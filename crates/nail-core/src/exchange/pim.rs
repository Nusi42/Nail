//! Calendars, contacts and tasks of an Exchange account: its
//! `exchange:<account>` PIM source (ADR 0077).
//!
//! * **Collections**: every pass lists the calendars, contact folders and
//!   task lists (Graph: Microsoft To Do lists) of the own mailbox and of
//!   the shown parts of shared / delegate mailboxes; each is a calendar /
//!   task list / address book of the source with
//!   `ExchangeCollectionInfo` and its Exchange record (remote id, cursor).
//! * **Items**: changes per collection since its cursor (an expired one
//!   means a full listing merged with the store), mapped to Nail's model
//!   by `nail_exchange::mapping` and stored with the iCalendar / vCard
//!   text `nail-ical` writes. The server id is the object's `href`, the
//!   change key its `etag`; checklist items of a To Do task are its
//!   subtasks (`href` `<task id>#<checklist id>`).
//! * **Writes** go through the PIM queue ([`ExchangeReplayer`]):
//!   `write_pim` creates / replaces / deletes items (a stale change key is
//!   a conflict: the server wins, the local version is kept as a conflict
//!   copy); meetings the user organizes are sent to the attendees by the
//!   server (`MeetingSend::ToAll`), and an attendee's answer goes through
//!   the meeting API (`respond_to_meeting`) — never iMIP. EWS writes use
//!   UTC wall times (EWS wants Windows zone ids).

use std::collections::HashSet;
use std::future::Future;

use nail_api::exchange::engine::{
    ExchangeEngine, ExchangePimWrite, ExchangePimWriteOutcome, MeetingSend, PimDeltaOutcome,
};
use nail_api::exchange::pim::{
    ExchangeDateTime, ExchangeEvent, ExchangePimFolder, ExchangePimItem, PimFolderKind,
};
use nail_api::exchange::{
    ExchangeCollectionInfo, ExchangeEngine as EngineKind, ExchangeMailboxSettings,
};
use nail_api::pim::calendar::ParticipationStatus;
use nail_api::pim::{
    AddressBook, AddressBookSettings, Calendar, CalendarComponent, CalendarEvent, CalendarSettings,
    ContactGroup, PimItemKind, PimItemSync, Task,
};
use nail_api::scheduling::InvitationResponse;
use nail_api::{
    AccountConfig, AccountId, AddressBookId, CalendarId, ContactGroupId, ContactId, EventId,
    PimSourceId, TaskId,
};
use nail_exchange::mapping::contacts::MappedContact;
use nail_exchange::mapping::events::WriteContext;
use nail_exchange::mapping::tasks::TaskWriteContext;
use nail_store::ReplayPolicy;
use nail_store::exchange::ExchangePimCollectionRecord;
use nail_store::pim::{
    PimItem, PimOperation, PimReplayOutcome, PimReplayer, QueuedPimOp, ServerApply, ServerRemove,
};

use super::sync::Listing;
use crate::error::{CoreError, Result};
use crate::pim::derived_id;
use crate::pim::engine::SyncChanges;
use crate::service::Service;

/// Most pages of one collection in one pass.
const MAX_PAGES: u32 = 1_000;

/// The id of an Exchange collection.
fn collection_id(
    source: &PimSourceId,
    mailbox: Option<&str>,
    folder: &ExchangePimFolder,
) -> String {
    let prefix = match folder.kind {
        PimFolderKind::Calendar => "exch-cal",
        PimFolderKind::Tasks => "exch-tasks",
        PimFolderKind::Contacts => "exch-book",
    };
    derived_id(
        prefix,
        source.as_str(),
        &format!("{}\0{}", mailbox.unwrap_or(""), folder.remote_id),
    )
}

/// Whether a mailbox shows a kind of folder.
fn shows(settings: Option<&ExchangeMailboxSettings>, kind: PimFolderKind) -> bool {
    settings.is_none_or(|s| match kind {
        PimFolderKind::Calendar => s.calendar,
        PimFolderKind::Contacts => s.contacts,
        PimFolderKind::Tasks => s.tasks,
    })
}

/// The addresses that are "me" for meetings of the account.
fn own_addresses(account: &AccountConfig) -> Vec<String> {
    let mut out = vec![account.email.to_ascii_lowercase()];
    for identity in &account.identities {
        let e = identity.email.to_ascii_lowercase();
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

/// One pass: replay the source's PIM queue, list collections, sync items.
/// `true` when something changed.
pub(crate) async fn sync(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
) -> Result<bool> {
    let id = account.id.clone();
    let now = service.pim_now();
    let source = service
        .store()
        .call(move |s| s.ensure_exchange_pim_source(&id, now))
        .await?;
    replay(service, account, engine, &source).await?;
    tracing::debug!(account = %account.id, "Exchange PIM: replayed");
    let id = account.id.clone();
    let mailboxes = service
        .store()
        .call(move |s| s.exchange_mailboxes(&id, false))
        .await?;
    let mut targets: Vec<(Option<String>, Option<ExchangeMailboxSettings>)> = vec![(None, None)];
    for m in mailboxes {
        if m.settings.calendar || m.settings.contacts || m.settings.tasks {
            targets.push((Some(m.settings.address.clone()), Some(m.settings)));
        }
    }
    let mut changes = SyncChanges::default();
    let mut kept: HashSet<String> = HashSet::new();
    let mut listed_all = true;
    for (mailbox, settings) in &targets {
        let folders = match engine.pim_folders(mailbox.as_deref()).await {
            Ok(folders) => folders,
            Err(err) if mailbox.is_some() => {
                tracing::info!(account = %account.id, mailbox = ?mailbox, %err, "cannot list the mailbox's calendars");
                listed_all = false;
                // Keep what is stored for it.
                let (id, key) = (account.id.clone(), mailbox.clone());
                let stored = service
                    .store()
                    .call(move |s| s.exchange_pim_collections(&id))
                    .await?;
                kept.extend(
                    stored
                        .into_iter()
                        .filter(|c| c.mailbox == key)
                        .map(|c| c.collection_id),
                );
                continue;
            }
            Err(err) => return Err(err.into()),
        };
        for folder in folders
            .into_iter()
            .filter(|f| shows(settings.as_ref(), f.kind))
        {
            let key = collection_id(&source, mailbox.as_deref(), &folder);
            changes.collections |=
                save_collection(service, account, &source, mailbox.as_deref(), &folder, &key)
                    .await?;
            kept.insert(key.clone());
            sync_collection(
                service,
                account,
                engine,
                mailbox.as_deref(),
                &folder,
                &key,
                &mut changes,
            )
            .await?;
        }
    }
    // Collections the server no longer lists (or no longer shown).
    if listed_all || !kept.is_empty() {
        let (id, src) = (account.id.clone(), source.clone());
        let gone = service
            .store()
            .call(move |s| {
                let mut gone = false;
                let known: Vec<String> = s
                    .calendars(Some(&src))?
                    .into_iter()
                    .map(|c| c.id.0)
                    .chain(s.address_books(Some(&src))?.into_iter().map(|b| b.id.0))
                    .collect();
                let _ = &id;
                for key in known {
                    if !kept.contains(&key) {
                        s.delete_collection(&key)?;
                        gone = true;
                    }
                }
                Ok(gone)
            })
            .await?;
        changes.collections |= gone;
    }
    let any = changes.collections || !changes.items.is_empty() || !changes.removed.is_empty();
    tracing::debug!(account = %account.id, items = changes.items.len(), "Exchange PIM: synced");
    service.after_sync(changes).await;
    Ok(any)
}

/// Creates / updates a collection row and its Exchange record; `true`
/// when it is new or changed.
async fn save_collection(
    service: &Service,
    account: &AccountConfig,
    source: &PimSourceId,
    mailbox: Option<&str>,
    folder: &ExchangePimFolder,
    key: &str,
) -> Result<bool> {
    let (id, src, folder, key, mailbox) = (
        account.id.clone(),
        source.clone(),
        folder.clone(),
        key.to_owned(),
        mailbox.map(str::to_owned),
    );
    service
        .store()
        .call(move |s| {
            let info = ExchangeCollectionInfo {
                mailbox: mailbox.clone(),
                role: folder.role,
                todo: folder.todo,
            };
            let records = s.exchange_pim_collections(&id)?;
            let record = records.iter().find(|r| r.collection_id == key);
            let mut changed = record.is_none();
            match folder.kind {
                PimFolderKind::Calendar | PimFolderKind::Tasks => {
                    let before = s.calendar(&CalendarId::new(key.clone()))?;
                    let mut settings = before.as_ref().map_or_else(
                        || {
                            let mut settings = CalendarSettings::named(&folder.name);
                            settings.color = folder.color.and_then(|c| c.rgb()).map(str::to_owned);
                            settings
                        },
                        |c| c.settings.clone(),
                    );
                    if settings.name != folder.name {
                        settings.name.clone_from(&folder.name);
                        changed = true;
                    }
                    let calendar = Calendar {
                        id: CalendarId::new(key.clone()),
                        source_id: src.clone(),
                        account_id: Some(id.clone()),
                        settings,
                        components: if folder.kind == PimFolderKind::Tasks {
                            vec![CalendarComponent::Task]
                        } else {
                            vec![CalendarComponent::Event]
                        },
                        read_only: folder.read_only,
                        timezone: before.as_ref().and_then(|c| c.timezone.clone()),
                        exchange: Some(info),
                    };
                    changed |= before
                        .as_ref()
                        .is_none_or(|b| b.read_only != calendar.read_only);
                    s.save_calendar(&calendar, Some(&folder.remote_id))?;
                }
                PimFolderKind::Contacts => {
                    let before = s.address_book(&AddressBookId::new(key.clone()))?;
                    let mut settings = before.as_ref().map_or_else(
                        || AddressBookSettings::named(&folder.name),
                        |b| b.settings.clone(),
                    );
                    if settings.name != folder.name {
                        settings.name.clone_from(&folder.name);
                        changed = true;
                    }
                    s.save_address_book(
                        &AddressBook {
                            id: AddressBookId::new(key.clone()),
                            source_id: src.clone(),
                            account_id: Some(id.clone()),
                            settings,
                            read_only: folder.read_only,
                            role: None,
                            exchange: Some(info),
                        },
                        Some(&folder.remote_id),
                    )?;
                }
            }
            s.upsert_exchange_pim_collection(
                &id,
                &ExchangePimCollectionRecord {
                    collection_id: key.clone(),
                    mailbox: mailbox.clone(),
                    remote_id: folder.remote_id.clone(),
                    change_key: folder.change_key.clone(),
                    role: folder.role,
                    todo: folder.todo,
                    cursor: record.and_then(|r| r.cursor.clone()),
                    last_sync_at: record.and_then(|r| r.last_sync_at),
                },
            )?;
            Ok(changed)
        })
        .await
        .map_err(CoreError::from)
}

/// The store items of one Exchange item (a To Do task with its checklist
/// gives several), with their `href` / `etag`.
fn items_of(
    service: &Service,
    item: &ExchangePimItem,
    key: &str,
    read_only: bool,
) -> Vec<(PimItem, String, String, Option<String>)> {
    let href = item.remote_id().to_owned();
    let etag = item.change_key().map(str::to_owned);
    match item {
        ExchangePimItem::Event(e) => {
            let mapped = nail_exchange::mapping::events::event_from_exchange(e);
            let id = EventId::new(derived_id("ev", key, &mapped.uid));
            let event = mapped.into_event(id, CalendarId::new(key), read_only);
            let raw = service.event_text(&event, None, None);
            vec![(PimItem::Event(event), raw, href, etag)]
        }
        ExchangePimItem::Task(t) => {
            let mapped = nail_exchange::mapping::tasks::task_from_exchange(t);
            let parent = Task {
                id: TaskId::new(derived_id("task", key, &mapped.uid)),
                calendar_id: CalendarId::new(key),
                uid: mapped.uid.clone(),
                fields: mapped.fields,
                recurrence: mapped.recurrence,
                subtask_ids: Vec::new(),
                created_at: mapped.created_at,
                updated_at: mapped.updated_at,
                read_only,
                sync: PimItemSync::Synced,
            };
            let raw = service.task_text(&parent, None, None);
            let mut out = vec![(
                PimItem::Task {
                    task: parent,
                    parent_uid: None,
                },
                raw,
                href.clone(),
                etag.clone(),
            )];
            for sub in mapped.subtasks {
                let task = Task {
                    id: TaskId::new(derived_id("task", key, &sub.uid)),
                    calendar_id: CalendarId::new(key),
                    uid: sub.uid.clone(),
                    fields: sub.fields,
                    recurrence: None,
                    subtask_ids: Vec::new(),
                    created_at: None,
                    updated_at: None,
                    read_only,
                    sync: PimItemSync::Synced,
                };
                let raw = service.task_text(&task, Some(&mapped.uid), None);
                out.push((
                    PimItem::Task {
                        task,
                        parent_uid: Some(mapped.uid.clone()),
                    },
                    raw,
                    format!("{href}#{}", sub.checklist_id),
                    etag.clone(),
                ));
            }
            out
        }
        ExchangePimItem::Contact(c) => {
            match nail_exchange::mapping::contacts::contact_from_exchange(c) {
                MappedContact::Contact { uid, fields } => {
                    let contact = nail_api::pim::Contact {
                        id: ContactId::new(derived_id("ct", key, &uid)),
                        address_book_id: AddressBookId::new(key),
                        uid,
                        fields: *fields,
                        version: nail_api::pim::contact::VCardVersion::V4,
                        group_ids: Vec::new(),
                        linked_ids: Vec::new(),
                        updated_at: None,
                        read_only,
                        sync: PimItemSync::Synced,
                    };
                    let raw = service.contact_text(&contact, None);
                    vec![(PimItem::Contact(contact), raw, href, etag)]
                }
                MappedContact::Group {
                    uid,
                    name,
                    member_emails,
                } => {
                    let card = nail_ical::vcard::GroupCard {
                        uid: uid.clone(),
                        name: name.clone(),
                        member_uids: Vec::new(),
                        member_emails: member_emails.clone(),
                        version: nail_api::pim::contact::VCardVersion::V4,
                        style: nail_ical::vcard::GroupStyle::Kind,
                    };
                    let raw = service.group_text(&card, None);
                    let group = ContactGroup {
                        id: ContactGroupId::new(derived_id("grp", key, &uid)),
                        address_book_id: AddressBookId::new(key),
                        uid,
                        name,
                        member_ids: Vec::new(),
                        member_emails,
                        read_only,
                        sync: PimItemSync::Synced,
                    };
                    vec![(PimItem::group(group), raw, href, etag)]
                }
            }
        }
    }
}

/// Syncs one collection's items. A full listing (no cursor, or an expired
/// one) is kept across passes (`full_sync_page`, the ids it saw in the
/// store) and merged only after its last page (ADR 0078).
async fn sync_collection(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
    mailbox: Option<&str>,
    folder: &ExchangePimFolder,
    key: &str,
    changes: &mut SyncChanges,
) -> Result<()> {
    let (id, k) = (account.id.clone(), key.to_owned());
    let (stored_cursor, page) = service
        .store()
        .call(move |s| {
            let cursor = s
                .exchange_pim_collections(&id)?
                .into_iter()
                .find(|c| c.collection_id == k)
                .and_then(|c| c.cursor);
            Ok((cursor, s.exchange_pim_listing(&k)?))
        })
        .await?;
    // PIM listings are never "first": there are no arrivals.
    let mut listing = Listing::of(stored_cursor.as_deref(), page.as_deref(), Some(0));
    let mut cursor = match &listing {
        Some(l) => l.resume.clone(),
        None => stored_cursor,
    };
    if listing.as_ref().is_some_and(|l| l.resume.is_none()) {
        save_listing(service, key, None, listing.as_ref(), true).await?;
    }
    let mut restarted = false;
    let mut pages = 0u32;
    loop {
        pages += 1;
        if pages > MAX_PAGES {
            // Goes on in the next pass; a partial listing is never merged.
            break;
        }
        let outcome = engine
            .pim_changes(mailbox, folder, cursor.as_deref())
            .await
            .map_err(CoreError::from)?;
        let delta = match outcome {
            PimDeltaOutcome::CursorExpired => {
                listing = Listing::after_expiry(listing, &mut restarted);
                if listing.is_none() {
                    return Err(nail_api::ApiError::new(
                        nail_api::ErrorCode::Internal,
                        "the server refused a full listing",
                    )
                    .into());
                }
                cursor = None;
                save_listing(service, key, None, listing.as_ref(), true).await?;
                continue;
            }
            PimDeltaOutcome::Delta(delta) => delta,
        };
        let own = own_addresses(account);
        let mut stored: Vec<(PimItem, String, String, Option<String>)> = Vec::new();
        for item in &delta.upserted {
            stored.extend(items_of(service, item, key, folder.read_only));
        }
        // The own answer to each meeting as the server has it (a later
        // local edit answers again only when it changes; ADR 0078).
        let answers: Vec<(String, Option<String>)> = stored
            .iter()
            .filter_map(|(item, _, _, _)| match item {
                PimItem::Event(event) if !event.fields.attendees.is_empty() => {
                    Some((event.uid.clone(), own_status(event, &own)))
                }
                _ => None,
            })
            .collect();
        let seen: Vec<String> = stored.iter().map(|(_, _, href, _)| href.clone()).collect();
        let (k, removed, next) = (key.to_owned(), delta.removed.clone(), delta.cursor.clone());
        let now = service.pim_now();
        let parents: Vec<String> = delta
            .upserted
            .iter()
            .map(|i| i.remote_id().to_owned())
            .collect();
        let hrefs: HashSet<String> = stored.iter().map(|(_, _, h, _)| h.clone()).collect();
        let (account_id, more) = (account.id.clone(), delta.more);
        let progress = listing.clone().map(|mut l| {
            l.resume = Some(delta.cursor.clone());
            l
        });
        let (applied, gone) = service
            .store()
            .call(move |s| {
                let mut applied = Vec::new();
                let mut gone = Vec::new();
                for (item, raw, href, etag) in &stored {
                    match s.apply_server_item(item, raw, Some(href), etag.as_deref(), now) {
                        Ok(
                            result @ (ServerApply::Inserted(_)
                            | ServerApply::Updated(_)
                            | ServerApply::Conflict(_)),
                        ) => applied.push(result.item().clone()),
                        Ok(_) => {}
                        Err(err) => tracing::warn!(%href, %err, "cannot store an Exchange item"),
                    }
                }
                for (uid, status) in &answers {
                    s.set_exchange_synced_answer(&k, uid, status.as_deref())?;
                }
                // Checklist items a task no longer has.
                for object in s.server_objects(&k)? {
                    let Some(href) = object.href else { continue };
                    if let Some((parent, _)) = href.split_once('#')
                        && parents.iter().any(|p| p == parent)
                        && !hrefs.contains(&href)
                        && let ServerRemove::Removed(i) | ServerRemove::KeptAsConflict(i) =
                            s.remove_server_item(&k, Some(&href), None)?
                    {
                        gone.push(i);
                    }
                }
                for remote in &removed {
                    for object in s.server_objects(&k)? {
                        let Some(href) = object.href else { continue };
                        if (href == *remote || href.starts_with(&format!("{remote}#")))
                            && let ServerRemove::Removed(i) | ServerRemove::KeptAsConflict(i) =
                                s.remove_server_item(&k, Some(&href), None)?
                        {
                            gone.push(i);
                        }
                    }
                }
                match progress {
                    None => s.set_exchange_pim_listing(&k, Some(&next), None, now)?,
                    Some(listing) => {
                        s.add_exchange_listed(&account_id, &k, &seen)?;
                        if more {
                            s.set_exchange_pim_listing(&k, None, Some(&listing.encode()), now)?;
                        } else {
                            // The last page: items the server no longer
                            // has go (local ones not uploaded yet stay).
                            let listed = s.exchange_listed(&k)?;
                            for object in s.server_objects(&k)? {
                                let Some(href) = object.href else { continue };
                                if !listed.contains(&href)
                                    && let ServerRemove::Removed(i)
                                    | ServerRemove::KeptAsConflict(i) =
                                        s.remove_server_item(&k, Some(&href), None)?
                                {
                                    gone.push(i);
                                }
                            }
                            s.clear_exchange_listed(&k)?;
                            s.set_exchange_pim_listing(&k, Some(&next), None, now)?;
                        }
                    }
                }
                Ok((applied, gone))
            })
            .await?;
        if let Some(l) = &mut listing {
            l.resume = Some(delta.cursor.clone());
        }
        changes
            .items
            .extend(applied.into_iter().map(|i| (i, key.to_owned())));
        changes
            .removed
            .extend(gone.into_iter().map(|i| (i, key.to_owned())));
        if !delta.more {
            break;
        }
        cursor = Some(delta.cursor);
    }
    Ok(())
}

/// Stores a collection's cursor and listing in progress (`clear`: forget
/// what a listing saw).
async fn save_listing(
    service: &Service,
    key: &str,
    cursor: Option<String>,
    listing: Option<&Listing>,
    clear: bool,
) -> Result<()> {
    let (k, page, now) = (
        key.to_owned(),
        listing.map(Listing::encode),
        service.pim_now(),
    );
    service
        .store()
        .call(move |s| {
            if clear {
                s.clear_exchange_listed(&k)?;
            }
            s.set_exchange_pim_listing(&k, cursor.as_deref(), page.as_deref(), now)
        })
        .await?;
    Ok(())
}

/// The user's own participation status in a meeting (kebab-case), if the
/// user is an attendee.
fn own_status(event: &CalendarEvent, own: &[String]) -> Option<String> {
    event
        .fields
        .attendees
        .iter()
        .find(|a| own.contains(&a.email.to_ascii_lowercase()))
        .map(|a| status_text(a.status))
}

/// A participation status as stored.
fn status_text(status: ParticipationStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{status:?}").to_ascii_lowercase())
}

/// Forgets the collections of another mailbox (`RemoveExchangeMailbox`).
pub(crate) async fn forget_mailbox(
    service: &Service,
    account: &AccountId,
    address: &str,
) -> Result<()> {
    forget(service, account, address, |_| true).await
}

/// Forgets the collections of parts a mailbox no longer shows.
pub(crate) async fn forget_unshown(
    service: &Service,
    account: &AccountId,
    settings: &ExchangeMailboxSettings,
) -> Result<()> {
    let s = settings.clone();
    forget(
        service,
        account,
        &settings.address,
        move |kind| match kind {
            Some(PimFolderKind::Calendar) => !s.calendar,
            Some(PimFolderKind::Tasks) => !s.tasks,
            Some(PimFolderKind::Contacts) => !s.contacts,
            None => false,
        },
    )
    .await
}

async fn forget(
    service: &Service,
    account: &AccountId,
    address: &str,
    remove: impl Fn(Option<PimFolderKind>) -> bool + Send + 'static,
) -> Result<()> {
    let (id, key) = (account.clone(), address.trim().to_ascii_lowercase());
    let gone = service
        .store()
        .call(move |s| {
            let mut gone = false;
            for record in s.exchange_pim_collections(&id)? {
                if record.mailbox.as_deref() != Some(key.as_str()) {
                    continue;
                }
                let kind = match s.calendar(&CalendarId::new(record.collection_id.clone()))? {
                    Some(c) if c.components.contains(&CalendarComponent::Task) => {
                        Some(PimFolderKind::Tasks)
                    }
                    Some(_) => Some(PimFolderKind::Calendar),
                    None => s
                        .address_book(&AddressBookId::new(record.collection_id.clone()))?
                        .map(|_| PimFolderKind::Contacts),
                };
                if remove(kind) {
                    s.delete_collection(&record.collection_id)?;
                    gone = true;
                }
            }
            Ok(gone)
        })
        .await?;
    if gone {
        service.reindex_pim().await;
        service.pim_events([
            nail_api::Event::CalendarsChanged,
            nail_api::Event::AddressBooksChanged,
        ]);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Replay

/// Replays the source's queued PIM writes at once (a write was queued;
/// failures stay queued for the next pass).
pub(crate) async fn replay_writes(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
) {
    let source = nail_api::exchange::exchange_pim_source_id(&account.id);
    tracing::debug!(account = %account.id, "Exchange PIM: replaying writes");
    if let Err(err) = replay(service, account, engine, &source).await {
        tracing::info!(account = %account.id, %err, "Exchange PIM writes deferred");
    }
}

async fn replay(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
    source: &PimSourceId,
) -> Result<()> {
    let id = account.id.clone();
    let collections = service
        .store()
        .call(move |s| s.exchange_pim_collections(&id))
        .await?;
    let replayer = ExchangeReplayer {
        service,
        account,
        engine,
        collections,
    };
    let report = service
        .inner
        .store
        .replay_pim_source(source, &replayer, ReplayPolicy::default())
        .await?;
    crate::pim::engine::report_replay(service, source, &report).await;
    Ok(())
}

/// Replays PIM writes of an Exchange source through the engine.
struct ExchangeReplayer<'a> {
    service: &'a Service,
    account: &'a AccountConfig,
    engine: &'a dyn ExchangeEngine,
    collections: Vec<ExchangePimCollectionRecord>,
}

fn outcome_of(err: &nail_api::engine::EngineError) -> PimReplayOutcome {
    if err.code == nail_api::ErrorCode::Conflict {
        PimReplayOutcome::Conflict(err.message.clone())
    } else if err.retryable {
        PimReplayOutcome::Retry(err.message.clone())
    } else {
        PimReplayOutcome::Fail(err.message.clone())
    }
}

/// Times as EWS wants them (ADR 0078): in a Windows zone, so the server
/// expands a series in its zone across daylight-saving changes; all-day
/// items in the user's zone (`local`, IANA). A timed item whose zone has
/// no Windows equivalent goes out in UTC — a series is refused instead
/// (it would move by an hour at each change).
fn ews_zones(event: &mut ExchangeEvent, local: &str) -> std::result::Result<(), String> {
    use nail_exchange::mapping::time::{instant, zone_of};
    use nail_exchange::mapping::windows_zones::windows_zone;
    let year = event
        .start
        .date_time
        .get(..4)
        .and_then(|y| y.parse().ok())
        .unwrap_or(2026);
    let to_utc = |t: &mut ExchangeDateTime| {
        if let Some(utc) = nail_exchange::mapping::time::utc_text(t) {
            *t = ExchangeDateTime::utc(utc.trim_end_matches('Z').to_owned());
        }
    };
    if event.is_all_day {
        let zone = zone_of(local)
            .filter(|tz| *tz != chrono_tz::Tz::UTC)
            .and_then(|tz| windows_zone(tz, year))
            .unwrap_or("UTC");
        event.start.time_zone = zone.to_owned();
        event.end.time_zone = zone.to_owned();
        if let Some(recurrence) = &mut event.recurrence {
            recurrence.time_zone = Some(zone.to_owned());
        }
    } else {
        let tz = zone_of(&event.start.time_zone).filter(|tz| *tz != chrono_tz::Tz::UTC);
        match tz.map(|tz| (tz, windows_zone(tz, year))) {
            Some((tz, Some(windows))) => {
                // The end as a wall time in the start's zone.
                if zone_of(&event.end.time_zone) != Some(tz)
                    && let Some(end) = instant(&event.end)
                {
                    event.end.date_time =
                        chrono::TimeZone::from_utc_datetime(&tz, &end.naive_utc())
                            .format("%Y-%m-%dT%H:%M:%S")
                            .to_string();
                }
                event.start.time_zone = windows.to_owned();
                event.end.time_zone = windows.to_owned();
                if let Some(recurrence) = &mut event.recurrence {
                    recurrence.time_zone = Some(windows.to_owned());
                }
            }
            Some((tz, None)) if event.recurrence.is_some() => {
                return Err(format!(
                    "Exchange Server has no time zone like {} for this series",
                    tz.name()
                ));
            }
            _ => {
                to_utc(&mut event.start);
                to_utc(&mut event.end);
                if let Some(recurrence) = &mut event.recurrence {
                    recurrence.time_zone = Some("UTC".to_owned());
                }
            }
        }
    }
    for x in &mut event.exceptions {
        ews_zones(x, local)?;
    }
    Ok(())
}

impl ExchangeReplayer<'_> {
    fn collection(&self, key: &str) -> Option<&ExchangePimCollectionRecord> {
        self.collections.iter().find(|c| c.collection_id == key)
    }

    /// The zone of dates and floating times of events (EWS: mapped to a
    /// Windows id by [`ews_zones`]).
    fn zone(&self) -> String {
        crate::pim::local_zone_name()
    }

    /// The zone of task dates (EWS writes them in UTC).
    fn task_zone(&self) -> String {
        if self.engine.kind() == EngineKind::Ews {
            "UTC".to_owned()
        } else {
            crate::pim::local_zone_name()
        }
    }

    /// The parsed store item of an object.
    fn parse(&self, raw: &str, kind: PimItemKind, key: &str) -> Option<PimItem> {
        let blobs = self.service.blobs();
        let items = match kind {
            PimItemKind::Event | PimItemKind::Task => crate::pim::engine::calendar_items(
                blobs,
                nail_ical::parse_calendar(raw, nail_ical::ParseOptions::default()),
                key,
                false,
                true,
            ),
            PimItemKind::Contact | PimItemKind::ContactGroup => crate::pim::engine::card_items(
                blobs,
                nail_ical::vcard::parse_cards(raw),
                key,
                false,
            ),
        };
        items.into_iter().map(|(i, _)| i).find(|i| i.kind() == kind)
    }

    async fn run(&self, op: &QueuedPimOp) -> std::result::Result<PimReplayOutcome, String> {
        tracing::debug!(account = %self.account.id, op = ?op.operation, "replaying an Exchange PIM write");
        let store = &self.service.inner.store;
        let done =
            |href: Option<String>, etag: Option<String>| PimReplayOutcome::Done { href, etag };
        match &op.operation {
            PimOperation::Put { object_id } => {
                let id = *object_id;
                let Some(object) = store
                    .call(move |s| s.pim_object_by_id(id))
                    .await
                    .map_err(|e| e.to_string())?
                else {
                    return Ok(done(None, None));
                };
                let Some(collection) = self.collection(&object.collection_id).cloned() else {
                    return Ok(PimReplayOutcome::Retry(
                        "the collection is not on the server yet".into(),
                    ));
                };
                let Some(item) = self.parse(&object.raw, object.kind, &object.collection_id) else {
                    return Ok(PimReplayOutcome::Fail("the item cannot be read".into()));
                };
                let mailbox = collection.mailbox.as_deref();
                let remote = object
                    .href
                    .as_deref()
                    .map(|h| h.split('#').next().unwrap_or(h).to_owned());
                match item {
                    PimItem::Event(event) => {
                        self.put_event(
                            mailbox,
                            &collection,
                            &event,
                            remote.as_deref(),
                            object.etag.as_deref(),
                        )
                        .await
                    }
                    PimItem::Task { task, parent_uid } => {
                        self.put_task(
                            &collection,
                            &object.collection_id,
                            &task,
                            parent_uid.as_deref(),
                            object.href.as_deref(),
                            object.etag.as_deref(),
                        )
                        .await
                    }
                    PimItem::Contact(contact) => {
                        let remote_pair = remote.as_deref().map(|r| (r, object.etag.as_deref()));
                        let c = nail_exchange::mapping::contacts::contact_to_exchange(
                            &contact.fields,
                            remote_pair,
                        );
                        self.write(
                            mailbox,
                            &collection,
                            ExchangePimItem::Contact(Box::new(c)),
                            remote.is_none(),
                            MeetingSend::None,
                        )
                        .await
                    }
                    PimItem::Group { group, .. } => {
                        let remote_pair = remote.as_deref().map(|r| (r, object.etag.as_deref()));
                        let c = nail_exchange::mapping::contacts::group_to_exchange(
                            &group.name,
                            &group.member_emails,
                            remote_pair,
                        );
                        self.write(
                            mailbox,
                            &collection,
                            ExchangePimItem::Contact(Box::new(c)),
                            remote.is_none(),
                            MeetingSend::None,
                        )
                        .await
                    }
                }
            }
            PimOperation::Delete {
                object_id,
                collection_id,
                href,
                ..
            } => {
                let mut href = href.clone();
                let mut raw = None;
                if let Some(id) = *object_id
                    && let Some(object) = store
                        .call(move |s| s.pim_object_by_id(id))
                        .await
                        .map_err(|e| e.to_string())?
                {
                    if href.is_none() {
                        href = object.href.clone();
                    }
                    raw = Some(object.raw);
                }
                let Some(href) = href else {
                    return Ok(done(None, None));
                };
                let Some(collection) = self.collection(collection_id).cloned() else {
                    return Ok(done(None, None));
                };
                if let Some((parent, _)) = href.split_once('#') {
                    // A checklist item: the parent task is written without it.
                    let _ = parent;
                    return Ok(self
                        .rewrite_parent_of(&collection, collection_id, &href)
                        .await);
                }
                let send = match raw
                    .as_deref()
                    .and_then(|r| self.parse(r, PimItemKind::Event, collection_id))
                {
                    Some(PimItem::Event(event)) if self.organizes(&event) => MeetingSend::ToAll,
                    _ => MeetingSend::None,
                };
                let write = ExchangePimWrite::Delete {
                    remote_id: href.clone(),
                    send,
                };
                match self
                    .engine
                    .write_pim(collection.mailbox.as_deref(), &write)
                    .await
                {
                    Ok(_) => Ok(done(None, None)),
                    Err(err) if err.code == nail_api::ErrorCode::NotFound => Ok(done(None, None)),
                    Err(err) => Ok(outcome_of(&err)),
                }
            }
            PimOperation::CreateCollection { .. }
            | PimOperation::UpdateCollection { .. }
            | PimOperation::DeleteCollection { .. } => Ok(PimReplayOutcome::Fail(
                "calendars and address books of Exchange accounts are managed in Exchange".into(),
            )),
        }
    }

    fn organizes(&self, event: &CalendarEvent) -> bool {
        let own = own_addresses(self.account);
        !event.fields.attendees.is_empty()
            && event
                .fields
                .organizer
                .as_ref()
                .is_none_or(|o| own.contains(&o.email.to_ascii_lowercase()))
    }

    async fn put_event(
        &self,
        mailbox: Option<&str>,
        collection: &ExchangePimCollectionRecord,
        event: &CalendarEvent,
        remote: Option<&str>,
        etag: Option<&str>,
    ) -> std::result::Result<PimReplayOutcome, String> {
        let own = own_addresses(self.account);
        // An invitation the user answers: the meeting API, not an update —
        // but only when the answer changed (or Nail queued one with its
        // comment); other edits of the user's copy (reminder, categories,
        // notes, show-as) are written as an update nobody is told about
        // (ADR 0078).
        if let Some(remote) = remote
            && !event.fields.attendees.is_empty()
            && !self.organizes(event)
        {
            let status = own_status(event, &own);
            let response = match event
                .fields
                .attendees
                .iter()
                .find(|a| own.contains(&a.email.to_ascii_lowercase()))
                .map(|a| a.status)
            {
                Some(ParticipationStatus::Accepted) => Some(InvitationResponse::Accept),
                Some(ParticipationStatus::Tentative) => Some(InvitationResponse::Tentative),
                Some(ParticipationStatus::Declined) => Some(InvitationResponse::Decline),
                _ => None,
            };
            let (k, uid) = (collection.collection_id.clone(), event.uid.clone());
            let answer = self
                .service
                .inner
                .store
                .call(move |s| s.exchange_meeting_answer(&k, &uid))
                .await
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            let changed = status != answer.synced_status;
            if let Some(response) = response
                && (changed || answer.queued.is_some())
            {
                let (comment, send) = answer.queued.map_or((None, true), |q| (q.comment, q.send));
                return match self
                    .engine
                    .respond_to_meeting(mailbox, remote, response, comment.as_deref(), send)
                    .await
                {
                    Ok(()) => {
                        let (k, uid) = (collection.collection_id.clone(), event.uid.clone());
                        let _ = self
                            .service
                            .inner
                            .store
                            .call(move |s| {
                                s.finish_exchange_meeting_answer(&k, &uid, status.as_deref())
                            })
                            .await;
                        Ok(PimReplayOutcome::Done {
                            href: Some(remote.to_owned()),
                            etag: None,
                        })
                    }
                    Err(err) => Ok(outcome_of(&err)),
                };
            }
        }
        let zone = self.zone();
        let ctx = WriteContext {
            remote: remote.map(|r| (r, etag)),
            default_zone: &zone,
            own: &own,
        };
        let mut mapped = match nail_exchange::mapping::events::event_to_exchange(event, &ctx) {
            Ok((mapped, _)) => mapped,
            Err(not) => {
                return Ok(PimReplayOutcome::Fail(format!(
                    "Exchange cannot store this event ({})",
                    not.0.id
                )));
            }
        };
        if self.engine.kind() == EngineKind::Ews
            && let Err(reason) = ews_zones(&mut mapped, &zone)
        {
            return Ok(PimReplayOutcome::Fail(reason));
        }
        let send = if self.organizes(event) {
            MeetingSend::ToAll
        } else {
            MeetingSend::None
        };
        self.write(
            mailbox,
            collection,
            ExchangePimItem::Event(Box::new(mapped)),
            remote.is_none(),
            send,
        )
        .await
    }

    /// A task (or a checklist item: its parent) with its checklist.
    async fn put_task(
        &self,
        collection: &ExchangePimCollectionRecord,
        key: &str,
        task: &Task,
        parent_uid: Option<&str>,
        href: Option<&str>,
        etag: Option<&str>,
    ) -> std::result::Result<PimReplayOutcome, String> {
        if let Some(parent) = parent_uid {
            // A checklist item: write its parent; a new item's local copy
            // goes (the next sync brings the server's).
            let outcome = self.write_task_tree(collection, key, parent).await?;
            if href.is_none() && matches!(outcome, PimReplayOutcome::Done { .. }) {
                let (k, uid) = (key.to_owned(), task.uid.clone());
                let _ = self
                    .service
                    .inner
                    .store
                    .call(move |s| s.remove_server_item(&k, None, Some(&uid)))
                    .await;
            }
            return Ok(match outcome {
                PimReplayOutcome::Done { .. } => PimReplayOutcome::Done {
                    href: href.map(str::to_owned),
                    etag: etag.map(str::to_owned),
                },
                other => other,
            });
        }
        self.write_task_tree(collection, key, &task.uid).await
    }

    /// Writes the task `uid` with its subtasks as checklist items.
    async fn write_task_tree(
        &self,
        collection: &ExchangePimCollectionRecord,
        key: &str,
        uid: &str,
    ) -> std::result::Result<PimReplayOutcome, String> {
        let k = key.to_owned();
        let objects = self
            .service
            .inner
            .store
            .call(move |s| {
                let mut out = Vec::new();
                for info in s.server_objects(&k)? {
                    if info.deleted {
                        continue;
                    }
                    if let Some(object) = s.pim_object_by_id(info.object_id)? {
                        out.push(object);
                    }
                }
                Ok(out)
            })
            .await
            .map_err(|e| e.to_string())?;
        let mut parent: Option<(Task, Option<String>, Option<String>)> = None;
        let mut children: Vec<(Option<String>, Task)> = Vec::new();
        for object in &objects {
            if object.kind != PimItemKind::Task {
                continue;
            }
            let Some(PimItem::Task { task, parent_uid }) =
                self.parse(&object.raw, PimItemKind::Task, key)
            else {
                continue;
            };
            if task.uid == uid {
                parent = Some((task, object.href.clone(), object.etag.clone()));
            } else if parent_uid.as_deref() == Some(uid) {
                let checklist = object
                    .href
                    .as_deref()
                    .and_then(|h| h.split_once('#'))
                    .map(|(_, c)| c.to_owned());
                children.push((checklist, task));
            }
        }
        let Some((task, href, etag)) = parent else {
            return Ok(PimReplayOutcome::Done {
                href: None,
                etag: None,
            });
        };
        children.sort_by_key(|(_, t)| t.fields.order.unwrap_or(i64::MAX));
        let subtasks: Vec<(Option<String>, &Task)> =
            children.iter().map(|(c, t)| (c.clone(), t)).collect();
        let zone = self.task_zone();
        let ctx = TaskWriteContext {
            remote: href.as_deref().map(|h| (h, etag.as_deref())),
            default_zone: &zone,
            linked_resources: &[],
        };
        let mapped = match nail_exchange::mapping::tasks::task_to_exchange(&task, &subtasks, &ctx) {
            Ok((mapped, _)) => mapped,
            Err(not) => {
                return Ok(PimReplayOutcome::Fail(format!(
                    "Exchange cannot store this task ({})",
                    not.0.id
                )));
            }
        };
        self.write(
            collection.mailbox.as_deref(),
            collection,
            ExchangePimItem::Task(Box::new(mapped)),
            href.is_none(),
            MeetingSend::None,
        )
        .await
    }

    /// The parent of a deleted checklist item, written without it.
    async fn rewrite_parent_of(
        &self,
        collection: &ExchangePimCollectionRecord,
        key: &str,
        href: &str,
    ) -> PimReplayOutcome {
        let parent_href = href.split('#').next().unwrap_or(href).to_owned();
        let k = key.to_owned();
        let parent_uid = self
            .service
            .inner
            .store
            .call(move |s| {
                Ok(s.server_objects(&k)?
                    .into_iter()
                    .find(|o| o.href.as_deref() == Some(parent_href.as_str()))
                    .map(|o| o.uid))
            })
            .await
            .ok()
            .flatten();
        match parent_uid {
            Some(uid) => match self.write_task_tree(collection, key, &uid).await {
                Ok(PimReplayOutcome::Done { .. }) => PimReplayOutcome::Done {
                    href: None,
                    etag: None,
                },
                Ok(other) => other,
                Err(err) => PimReplayOutcome::Retry(err),
            },
            None => PimReplayOutcome::Done {
                href: None,
                etag: None,
            },
        }
    }

    async fn write(
        &self,
        mailbox: Option<&str>,
        collection: &ExchangePimCollectionRecord,
        item: ExchangePimItem,
        create: bool,
        send: MeetingSend,
    ) -> std::result::Result<PimReplayOutcome, String> {
        let write = if create {
            ExchangePimWrite::Create {
                folder: collection.remote_id.clone(),
                item,
                send,
            }
        } else {
            ExchangePimWrite::Update { item, send }
        };
        Ok(match self.engine.write_pim(mailbox, &write).await {
            Ok(ExchangePimWriteOutcome::Done {
                remote_id,
                change_key,
            }) => PimReplayOutcome::Done {
                href: Some(remote_id),
                etag: change_key,
            },
            Ok(ExchangePimWriteOutcome::Conflict) => {
                PimReplayOutcome::Conflict("the item changed on the server".into())
            }
            Ok(ExchangePimWriteOutcome::Gone) => {
                PimReplayOutcome::Conflict("the item is gone on the server".into())
            }
            Err(err) => outcome_of(&err),
        })
    }
}

impl PimReplayer for ExchangeReplayer<'_> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(kind: PimFolderKind, remote: &str) -> ExchangePimFolder {
        ExchangePimFolder {
            remote_id: remote.into(),
            change_key: None,
            name: "Calendar".into(),
            kind,
            role: None,
            todo: false,
            read_only: false,
            color: None,
        }
    }

    #[test]
    fn collection_ids_are_per_mailbox_and_kind() {
        let source = PimSourceId::new("exchange:work");
        let cal = folder(PimFolderKind::Calendar, "AAMkCal");
        let a = collection_id(&source, None, &cal);
        assert!(a.starts_with("exch-cal-"));
        assert_eq!(a, collection_id(&source, None, &cal));
        assert_ne!(
            a,
            collection_id(&source, Some("boss@contoso.example"), &cal)
        );
        assert!(
            collection_id(&source, None, &folder(PimFolderKind::Tasks, "AAMkCal"))
                .starts_with("exch-tasks-")
        );
        assert!(
            collection_id(&source, None, &folder(PimFolderKind::Contacts, "x"))
                .starts_with("exch-book-")
        );
    }

    #[test]
    fn shown_parts_follow_the_mailbox_settings() {
        let mut settings = ExchangeMailboxSettings::shared("support@contoso.example");
        assert!(shows(None, PimFolderKind::Tasks));
        assert!(shows(Some(&settings), PimFolderKind::Calendar));
        assert!(!shows(Some(&settings), PimFolderKind::Contacts));
        settings.tasks = true;
        assert!(shows(Some(&settings), PimFolderKind::Tasks));
    }

    fn event(start: &str, end: &str, zone: &str, all_day: bool) -> ExchangeEvent {
        serde_json::from_value(serde_json::json!({
            "remote_id": "AAMk1",
            "start": {"date_time": start, "time_zone": zone},
            "end": {"date_time": end, "time_zone": zone},
            "is_all_day": all_day,
        }))
        .unwrap()
    }

    #[test]
    fn ews_writes_keep_the_zone_as_a_windows_id() {
        // A weekly 09:00 Berlin series stays at 09:00 Berlin (no drift at
        // the daylight-saving change): W. Europe Standard Time, not UTC.
        let mut series = event(
            "2026-10-05T09:00:00",
            "2026-10-05T10:00:00",
            "Europe/Berlin",
            false,
        );
        series.recurrence = Some(
            serde_json::from_value(serde_json::json!({
                "pattern": {"kind": "weekly", "interval": 1, "days_of_week": ["monday"]},
                "range": {"kind": "noEnd", "start_date": "2026-10-05"},
                "time_zone": "Europe/Berlin",
            }))
            .unwrap(),
        );
        ews_zones(&mut series, "Europe/Berlin").unwrap();
        assert_eq!(series.start.time_zone, "W. Europe Standard Time");
        assert_eq!(series.start.date_time, "2026-10-05T09:00:00");
        assert_eq!(series.end.time_zone, "W. Europe Standard Time");
        assert_eq!(
            series.recurrence.unwrap().time_zone.as_deref(),
            Some("W. Europe Standard Time")
        );
        // The end in another zone becomes a wall time in the start's.
        let mut flight = event(
            "2026-10-05T09:00:00",
            "2026-10-05T12:00:00",
            "Europe/Berlin",
            false,
        );
        flight.end.time_zone = "UTC".into();
        ews_zones(&mut flight, "Europe/Berlin").unwrap();
        assert_eq!(flight.end.date_time, "2026-10-05T14:00:00");
        assert_eq!(flight.end.time_zone, "W. Europe Standard Time");
        // All-day items: the user's zone, dates stay dates.
        let mut all_day = event(
            "2026-10-05T00:00:00",
            "2026-10-06T00:00:00",
            "Europe/Berlin",
            true,
        );
        ews_zones(&mut all_day, "America/New_York").unwrap();
        assert_eq!(all_day.start.date_time, "2026-10-05T00:00:00");
        assert_eq!(all_day.start.time_zone, "Eastern Standard Time");
        // UTC stays UTC.
        let mut utc = event("2026-10-05T08:00:00", "2026-10-05T09:00:00", "UTC", false);
        ews_zones(&mut utc, "Europe/Berlin").unwrap();
        assert_eq!(utc.start.time_zone, "UTC");
        assert_eq!(utc.start.date_time, "2026-10-05T08:00:00");
    }
}
