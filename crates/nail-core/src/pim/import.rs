//! `.ics` / `.vcf` imports (files opened with Nail, uploads) and "create
//! event / task from email".

use chrono::{Duration, Timelike};
use nail_api::pim::import::{
    MAX_PIM_IMPORT_BYTES, PimImportPreview, PimImportResult, PimImportSource,
};
use nail_api::pim::{
    Attendee, CalendarComponent, EventFields, PimItemKind, PimLink, PimTime, TaskFields,
};
use nail_api::{
    AddressBookId, ApiError, CalendarId, ErrorCode, Event, MessageId, Request, Response,
};
use nail_ical::import::{self as ical_import, ImportData};
use nail_store::pim::{COLLECTED_BOOK_ID, PimItem};

use super::engine::{calendar_items, card_items};
use super::{forbidden, new_id, op_ids};
use crate::error::Result;
use crate::service::Service;

pub(super) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::PreviewPimImport { source } => {
            let data = read_source(source).await?;
            Response::PimImportPreview {
                preview: Box::new(preview(&data)?),
            }
        }
        Request::ImportPim {
            source,
            calendar_id,
            address_book_id,
        } => {
            let data = read_source(source).await?;
            preview(&data)?;
            Response::PimImported {
                result: Box::new(
                    service
                        .import_pim(data, calendar_id, address_book_id)
                        .await?,
                ),
            }
        }
        Request::EventFromMessage { message_id } => Response::EventSuggested {
            fields: Box::new(service.event_from_message(message_id).await?),
        },
        Request::TaskFromMessage { message_id } => Response::TaskSuggested {
            fields: Box::new(service.task_from_message(message_id).await?),
        },
        other => {
            return Err(ApiError::internal(format!(
                "{} routed to PIM imports",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}

/// Reads import data (a desktop file or uploaded text).
async fn read_source(source: PimImportSource) -> Result<ImportData> {
    match source {
        PimImportSource::File { path } => {
            let data = tokio::task::spawn_blocking(move || -> Result<ImportData> {
                let file = std::path::Path::new(&path);
                let meta = std::fs::metadata(file)
                    .map_err(|e| ApiError::not_found(format!("cannot read {path}: {e}")))?;
                if meta.len() > MAX_PIM_IMPORT_BYTES as u64 {
                    return Err(ApiError::new(ErrorCode::TooLarge, "the file is too large").into());
                }
                let bytes = std::fs::read(file)
                    .map_err(|e| ApiError::not_found(format!("cannot read {path}: {e}")))?;
                let name = file.file_name().and_then(|n| n.to_str()).map(str::to_owned);
                Ok(ical_import::read_bytes(&bytes, name.as_deref()))
            })
            .await??;
            Ok(data)
        }
        PimImportSource::Data { file_name, content } => {
            if content.len() > MAX_PIM_IMPORT_BYTES {
                return Err(ApiError::new(ErrorCode::TooLarge, "the data is too large").into());
            }
            Ok(ical_import::read(&content, file_name.as_deref()))
        }
    }
}

/// The preview; `invalid_request` when the data is neither iCalendar nor
/// vCard.
fn preview(data: &ImportData) -> Result<PimImportPreview> {
    if let Some(issue) = ical_import::fatal_issue(data) {
        return Err(ApiError::invalid_request(issue.diagnostic())
            .with_localized(issue.localized())
            .into());
    }
    Ok(ical_import::preview(data))
}

impl Service {
    /// The calendar an import goes to: the one asked for (it must hold
    /// `component`), else the first writable local one that does.
    pub(crate) async fn import_target(
        &self,
        asked: Option<&CalendarId>,
        component: CalendarComponent,
    ) -> Result<CalendarId> {
        let calendars = self.list_calendars(None).await?;
        if let Some(id) = asked {
            let calendar = calendars
                .iter()
                .find(|c| &c.id == id)
                .ok_or_else(|| super::not_found(format!("calendar {id}")))?;
            if calendar.read_only {
                return Err(forbidden(format!("calendar {id} is read-only")));
            }
            if calendar.components.contains(&component) {
                return Ok(id.clone());
            }
        }
        calendars
            .iter()
            .filter(|c| !c.read_only && c.components.contains(&component))
            .min_by_key(|c| {
                (
                    c.source_id.as_str() != nail_api::PimSourceId::LOCAL,
                    c.settings.order,
                )
            })
            .map(|c| c.id.clone())
            .ok_or_else(|| super::not_found("a calendar for the import"))
    }

    async fn import_pim(
        &self,
        data: ImportData,
        calendar_id: Option<CalendarId>,
        address_book_id: Option<AddressBookId>,
    ) -> Result<PimImportResult> {
        let issues: Vec<_> = match &data {
            ImportData::Calendar(c) => c.issues.iter().map(|i| i.to_import_issue()).collect(),
            ImportData::Cards(c) => c.issues.iter().map(|i| i.to_import_issue()).collect(),
        };
        let blobs = self.blobs().clone();
        let items: Vec<(PimItem, String)> = match data {
            ImportData::Calendar(calendar) => {
                let events_to = if calendar.events.is_empty() {
                    None
                } else {
                    Some(
                        self.import_target(calendar_id.as_ref(), CalendarComponent::Event)
                            .await?,
                    )
                };
                let tasks_to = if calendar.tasks.is_empty() {
                    None
                } else {
                    Some(
                        self.import_target(calendar_id.as_ref(), CalendarComponent::Task)
                            .await?,
                    )
                };
                let mut out = Vec::new();
                let mut events = calendar.clone();
                events.tasks.clear();
                if let Some(target) = &events_to {
                    out.extend(calendar_items(&blobs, events, target.as_str(), false, true));
                }
                let mut tasks = calendar;
                tasks.events.clear();
                if let Some(target) = &tasks_to {
                    out.extend(calendar_items(&blobs, tasks, target.as_str(), false, true));
                }
                out
            }
            ImportData::Cards(cards) => {
                let book = match address_book_id {
                    Some(id) => {
                        let book = self.address_book(&id).await?;
                        if book.read_only {
                            return Err(forbidden(format!("address book {id} is read-only")));
                        }
                        id
                    }
                    None => self
                        .list_address_books(None)
                        .await?
                        .into_iter()
                        .filter(|b| !b.read_only && b.id.as_str() != COLLECTED_BOOK_ID)
                        .min_by_key(|b| {
                            (
                                b.source_id.as_str() != nail_api::PimSourceId::LOCAL,
                                b.settings.order,
                            )
                        })
                        .map(|b| b.id)
                        .ok_or_else(|| super::not_found("an address book for the import"))?,
                };
                card_items(&blobs, cards, book.as_str(), false)
            }
        };
        let now = self.pim_now();
        let (created, updated, skipped, ops, changed) = self
            .inner
            .store
            .call(move |s| {
                let (mut created, mut updated, mut skipped) = (0u32, 0u32, 0u32);
                let mut ops = Vec::new();
                let mut changed = Vec::new();
                for (mut item, raw) in items {
                    // An item whose UID is there already is updated.
                    let collection = item.collection_id().to_owned();
                    let existing: Option<String> = match item.kind() {
                        PimItemKind::Event => s
                            .events(Some(&[CalendarId::new(collection.clone())]))?
                            .into_iter()
                            .find(|e| e.uid == item.uid())
                            .map(|e| e.id.0),
                        PimItemKind::Task => s
                            .list_tasks(Some(&[CalendarId::new(collection.clone())]), true)?
                            .into_iter()
                            .find(|t| t.uid == item.uid())
                            .map(|t| t.id.0),
                        PimItemKind::Contact => s
                            .contacts(Some(&[AddressBookId::new(collection.clone())]))?
                            .into_iter()
                            .find(|c| c.uid == item.uid())
                            .map(|c| c.id.0),
                        PimItemKind::ContactGroup => s
                            .contact_groups(Some(&[AddressBookId::new(collection.clone())]))?
                            .into_iter()
                            .find(|g| g.uid == item.uid())
                            .map(|g| g.id.0),
                    };
                    let is_update = existing.is_some();
                    let id = existing.unwrap_or_else(|| {
                        new_id(match item.kind() {
                            PimItemKind::Event => "ev",
                            PimItemKind::Task => "task",
                            PimItemKind::Contact => "ct",
                            PimItemKind::ContactGroup => "grp",
                        })
                    });
                    match &mut item {
                        PimItem::Event(e) => e.id = id.into(),
                        PimItem::Task { task, .. } => task.id = id.into(),
                        PimItem::Contact(c) => c.id = id.into(),
                        PimItem::Group { group, .. } => group.id = id.into(),
                    }
                    match s.save_pim_item(&item, &raw, now) {
                        Ok(write) => {
                            if is_update {
                                updated += 1;
                            } else {
                                created += 1;
                            }
                            ops.extend(write.op_ids);
                            changed.push((write.item, collection));
                        }
                        Err(err) => {
                            tracing::warn!(uid = item.uid(), %err, "cannot import an item");
                            skipped += 1;
                        }
                    }
                }
                Ok((created, updated, skipped, ops, changed))
            })
            .await?;
        // Queued uploads per source.
        let mut by_source: Vec<(nail_api::PimSourceId, Vec<nail_store::pim::PimOpId>)> = Vec::new();
        let mut collections: Vec<String> = changed.iter().map(|(_, c)| c.clone()).collect();
        collections.sort();
        collections.dedup();
        for collection in &collections {
            let source = self.collection_source(collection).await?;
            if !by_source.iter().any(|(s, _)| s == &source) {
                by_source.push((source, Vec::new()));
            }
        }
        if let Some((source, _)) = by_source.first() {
            // Every op of one import goes to the source(s) of its targets.
            let source = source.clone();
            self.report_queued(&source, &ops);
        }
        let items: Vec<_> = changed.iter().map(|(i, _)| i.clone()).collect();
        self.pim_index_apply(&items, &[]).await;
        self.replan_all_reminders().await;
        for collection in collections {
            self.pim_events([
                Event::EventsChanged {
                    calendar_ids: vec![CalendarId::new(collection.clone())],
                },
                Event::TasksChanged {
                    calendar_ids: vec![CalendarId::new(collection.clone())],
                },
                Event::ContactsChanged {
                    address_book_ids: vec![AddressBookId::new(collection)],
                },
            ]);
        }
        tracing::info!(created, updated, skipped, "PIM data imported");
        Ok(PimImportResult {
            created,
            updated,
            skipped,
            issues,
            op_ids: op_ids(&ops),
        })
    }

    async fn message_link(&self, id: &MessageId) -> Result<(nail_api::MessageBody, PimLink)> {
        let body = self.message_body(id.clone()).await?;
        let link = PimLink::Message {
            message_id: Some(id.clone()),
            rfc822_id: body
                .headers
                .message_id
                .clone()
                .map(|m| m.trim_matches(['<', '>']).to_owned()),
            subject: Some(body.headers.subject.clone()).filter(|s| !s.trim().is_empty()),
        };
        Ok((body, link))
    }

    async fn event_from_message(&self, id: MessageId) -> Result<EventFields> {
        let (body, link) = self.message_link(&id).await?;
        // An attached invitation or event: its fields.
        let encrypted = body.security.as_ref().is_some_and(|s| s.is_encrypted());
        if !encrypted
            && let Some(attachment) = body
                .attachments
                .iter()
                .find(|a| a.mime.eq_ignore_ascii_case("text/calendar"))
        {
            let message = id.clone();
            let store = self.inner.store.clone();
            let blobs = self.blobs().clone();
            let index = attachment.index;
            let text = tokio::task::spawn_blocking(move || -> Result<Option<String>> {
                let stored = store.get_message(&message)?;
                let Some(hash) = stored.blob_hash else {
                    return Ok(None);
                };
                let raw = blobs.get(&hash)?;
                Ok(nail_mime::attachment_content(&raw, index)
                    .map(|c| String::from_utf8_lossy(&c.data).into_owned()))
            })
            .await??;
            if let Some(text) = text
                && let Some(event) = nail_ical::parse_calendar(&text, Default::default())
                    .events
                    .into_iter()
                    .next()
            {
                let mut fields = event.fields;
                fields.links.push(link);
                return Ok(fields);
            }
        }
        let own: Vec<String> = self
            .own_addresses()
            .iter()
            .map(|a| a.to_lowercase())
            .collect();
        let mut attendees: Vec<Attendee> = Vec::new();
        for address in body
            .headers
            .from
            .iter()
            .chain(&body.headers.to)
            .chain(&body.headers.cc)
        {
            let email = address.email.trim().to_owned();
            if !nail_api::pim::is_plausible_email(&email)
                || own.contains(&email.to_lowercase())
                || attendees
                    .iter()
                    .any(|a| a.email.eq_ignore_ascii_case(&email))
            {
                continue;
            }
            let mut attendee = Attendee::new(email);
            attendee.name = address.name.clone().filter(|n| !n.trim().is_empty());
            attendees.push(attendee);
        }
        // The next full hour, one hour long, in this machine's zone.
        let zone = super::local_zone();
        let now = self.pim_utc_now().with_timezone(&zone);
        let start = now
            .naive_local()
            .with_minute(0)
            .and_then(|t| t.with_second(0))
            .unwrap_or(now.naive_local())
            + Duration::hours(1);
        let end = start + Duration::hours(1);
        let format = |t: chrono::NaiveDateTime| t.format("%Y-%m-%dT%H:%M:%S").to_string();
        let mut fields = EventFields::new(
            body.headers.subject.clone(),
            PimTime::zoned(format(start), zone.name()),
            PimTime::zoned(format(end), zone.name()),
        );
        fields.attendees = attendees;
        fields.links.push(link);
        Ok(fields)
    }

    async fn task_from_message(&self, id: MessageId) -> Result<TaskFields> {
        let (body, link) = self.message_link(&id).await?;
        let mut fields = TaskFields::titled(body.headers.subject.clone());
        fields.links.push(link);
        Ok(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn previews_and_refusals() {
        let data = read_source(PimImportSource::Data {
            file_name: Some("a.ics".into()),
            content: "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:1\r\nDTSTART:20261001T090000Z\r\nSUMMARY:Hi\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n".into(),
        })
        .await
        .unwrap();
        let shown = preview(&data).unwrap();
        assert_eq!(shown.events, 1);
        assert_eq!(shown.sample, ["Hi"]);
        let junk = read_source(PimImportSource::Data {
            file_name: Some("a.txt".into()),
            content: "hello".into(),
        })
        .await
        .unwrap();
        let err: ApiError = preview(&junk).unwrap_err().into();
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        assert!(err.localized.is_some());
        let missing = read_source(PimImportSource::File {
            path: "Z:/no/such/file.ics".into(),
        })
        .await;
        assert!(missing.is_err());
    }
}
