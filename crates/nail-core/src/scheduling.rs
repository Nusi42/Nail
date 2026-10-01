//! Invitations, scheduling and free/busy (M3b, contract ADR 0065,
//! integration ADR 0070) on `nail-itip` (ADR 0066) and `nail-dav`'s RFC
//! 6638 support (ADR 0068).
//!
//! * **Cards**: a message with an iTIP part (`nail_itip::find_itip`) gets
//!   [`nail_api::scheduling::Invitation`] on `GetMessage` / `GetInvitation`:
//!   the user's addresses (the receiving account's identities first), the
//!   calendar copy found by `UID`, the stored scheduling state (`DTSTAMP`
//!   of the applied version, the last reply of an attendee), conflicts with
//!   the instance cache and the transport (iMIP, or the CalDAV server when
//!   it schedules: RFC 6638 `SCHEDULE-AGENT`).
//! * **Arrival**: new iTIP mail is recorded and processed per
//!   [`SchedulingSettings`] — never when the card has an issue that
//!   [`ItipIssue::blocks_automatic`] (sender checks, `SENT-BY` claims,
//!   another organizer than the copy's, uninvited attendees), never
//!   `PUBLISH` or observer messages, never junk.
//! * **Writes** go through the PIM queue (`save_item` / `delete_item`;
//!   `nail-ical` patches the stored text); **iMIP mail** goes through the
//!   outbox as a prepared message (`AutomaticMail::Itip`, undo send
//!   applies) from the identity the card names, unless the CalDAV server
//!   delivers (then only the event is written — never both).
//! * **Free/busy**: the user's own calendars (`nail_dav::own_busy_periods`
//!   over the instance cache) for the user's addresses, the CalDAV schedule
//!   outbox of every source that has one for the others, `unknown` for the
//!   rest.
//! * **Schedule inbox** ([`process_schedule_inbox`]): items the server
//!   delivered are read with each CalDAV pass; the server already changed
//!   the calendar copies, so Nail records the versions and removes the
//!   items.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nail_api::pim::calendar::{
    Attendee, Calendar, CalendarEvent, EventFields, ParticipationStatus,
};
use nail_api::pim::{CalendarComponent, PimItemRef, PimItemSync, PimSourceConfig, PimTime};
use nail_api::scheduling::{
    AttendeeFreeBusy, CancelScope, CounterProposal, FreeBusyQuery, FreeBusyResult, FreeBusySource,
    Invitation, InvitationAction, InvitationResponse, InvitationRole, InvitationState,
    InvitationTarget, ItipIssue, ItipMethod, SchedulingResult, SchedulingSettings,
    SchedulingTransport, SentItip,
};
use nail_api::{
    AccountId, Address, ApiError, CalendarId, Draft, DraftAttachment, ErrorCode, Event, EventId,
    IdentityId, LocalizedMessage, MessageId, Request, Response, SendCrypto,
};
use nail_itip::address::{UserAddress, normalize, same};
use nail_itip::build::{
    CancelOptions, CancelWhat, CounterOptions, MinimalOptions, ReplyOptions, RequestOptions,
};
use nail_itip::card::{CardInput, StoredCopy};
use nail_itip::imip::{MailAddress, OutgoingImip};
use nail_itip::server::CalendarScheduling;
use nail_itip::{ItipObject, ReceivedImip, UserAddresses, Version};
use nail_store::pim::PimItem;
use nail_store::scheduling::{
    ItipMessageRecord, ItipMessageState, ItipParty, ItipStateRecord, SentItipRecord,
};

use crate::error::{CoreError, Result};
use crate::pim::{Expander, new_id};
use crate::service::Service;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetInvitation { message_id } => Ok(Response::Invitation {
            invitation: service.invitation(&message_id).await?.map(Box::new),
        }),
        Request::RespondToInvitation {
            target,
            response,
            comment,
            send,
            calendar_id,
        } => scheduled(
            service
                .respond(target, response, comment, send, calendar_id)
                .await?,
        ),
        Request::ProposeNewTime {
            target,
            start,
            end,
            comment,
        } => scheduled(service.propose(target, start, end, comment).await?),
        Request::RefreshInvitation { target } => scheduled(service.refresh(target).await?),
        Request::ForwardInvitation {
            target,
            to,
            comment,
        } => scheduled(service.forward(target, to, comment).await?),
        Request::ApplyInvitation {
            message_id,
            calendar_id,
        } => {
            let loaded = service.loaded(&message_id).await?;
            scheduled(service.apply_message(&loaded, calendar_id).await?)
        }
        Request::SendInvitations {
            event_id,
            recurrence_id,
            attendees,
            comment,
        } => scheduled(
            service
                .send_invitations(&event_id, recurrence_id.as_ref(), &attendees, comment)
                .await?,
        ),
        Request::CancelMeeting {
            event_id,
            scope,
            recurrence_id,
            attendees,
            comment,
        } => scheduled(
            service
                .cancel_meeting(&event_id, scope, recurrence_id, &attendees, comment)
                .await?,
        ),
        Request::RespondToCounter {
            message_id,
            accept,
            comment,
        } => scheduled(
            service
                .respond_to_counter(&message_id, accept, comment)
                .await?,
        ),
        Request::QueryFreeBusy {
            attendees,
            from,
            to,
            identity_id,
            ignore_event_id,
        } => Ok(Response::FreeBusy {
            result: Box::new(
                service
                    .free_busy(attendees, from, to, identity_id, ignore_event_id)
                    .await?,
            ),
        }),
        Request::GetSchedulingSettings => Ok(Response::SchedulingSettings {
            settings: service.scheduling_settings().await?,
        }),
        Request::SetSchedulingSettings { settings } => {
            if let Some(id) = &settings.calendar_id {
                let calendar = service.calendar(id).await?;
                if calendar.read_only || !calendar.components.contains(&CalendarComponent::Event) {
                    return Err(ApiError::invalid_request(format!(
                        "calendar {id} cannot take invitations"
                    ))
                    .into());
                }
            }
            let stored = settings.clone();
            service
                .store()
                .call(move |s| s.set_scheduling_settings(&stored))
                .await?;
            service.broadcast(Event::SchedulingSettingsChanged);
            Ok(Response::SchedulingSettings { settings })
        }
        other => Err(ApiError::internal(format!(
            "{} routed to scheduling",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}

fn scheduled(result: SchedulingResult) -> Result<Response> {
    Ok(Response::Scheduled {
        result: Box::new(result),
    })
}

/// `invalid_request` with an `itip-issue-*` text.
fn issue_error(issue: ItipIssue) -> CoreError {
    ApiError::invalid_request(format!("the invitation cannot be used: {issue:?}"))
        .with_localized(LocalizedMessage::new(issue.message_id()))
        .into()
}

/// `invalid_request` with a free text (no user-facing id: the UI only
/// offers what the card allows, so these are programming errors).
fn refused(message: impl Into<String>) -> CoreError {
    ApiError::invalid_request(message).into()
}

/// The store key of a recurrence id (`""` = the whole event).
fn rid_key(rid: Option<&PimTime>) -> String {
    rid.and_then(|r| serde_json::to_string(r).ok())
        .unwrap_or_default()
}

/// A received iTIP message.
#[derive(Debug, Clone)]
pub(crate) struct Loaded {
    message_id: MessageId,
    account_id: AccountId,
    imip: ReceivedImip,
    object: ItipObject,
    senders: Vec<String>,
}

/// The user's copy of an event.
#[derive(Debug, Clone)]
struct Copy {
    event: CalendarEvent,
    calendar: Calendar,
    raw: Option<String>,
}

impl Service {
    // --- Reading ---------------------------------------------------------

    /// The iTIP part of a stored message (`None` = it has none).
    async fn load_itip(&self, id: &MessageId) -> Result<Option<Loaded>> {
        let key = id.clone();
        let blobs = self.blobs().clone();
        let found = self
            .store()
            .call(move |s| {
                let message = s.get_message(&key)?;
                let raw = match message.blob_hash {
                    Some(hash) => blobs.get(&hash).ok(),
                    None => None,
                };
                Ok((message.summary.account_id, raw))
            })
            .await?;
        let (account_id, Some(raw)) = found else {
            return Ok(None);
        };
        let parsed = tokio::task::spawn_blocking(move || {
            nail_itip::find_itip(&raw).map(|imip| {
                let object = imip.parse();
                (imip, object)
            })
        })
        .await
        .map_err(|e| ApiError::internal(format!("iTIP parsing: {e}")))?;
        Ok(parsed.map(|(imip, object)| Loaded {
            message_id: id.clone(),
            account_id,
            senders: imip.senders(),
            imip,
            object,
        }))
    }

    /// Whether the message carries an iTIP `REFRESH` (applying it mails
    /// the current version; remote sessions need `send` for that).
    pub(crate) async fn is_itip_refresh(&self, id: &MessageId) -> bool {
        matches!(
            self.load_itip(id).await,
            Ok(Some(loaded)) if loaded.object.method == Some(ItipMethod::Refresh)
        )
    }

    /// Like [`Self::load_itip`], `invalid_request` without an iTIP part.
    pub(crate) async fn loaded(&self, id: &MessageId) -> Result<Loaded> {
        self.load_itip(id)
            .await?
            .ok_or_else(|| issue_error(ItipIssue::NoEvent))
    }

    /// The user's addresses: the identities of `account` first, then every
    /// other identity, then the accounts' own addresses.
    pub(crate) fn me(&self, account: Option<&AccountId>) -> UserAddresses {
        let config = self.inner.accounts.config();
        let mut accounts: Vec<&nail_api::AccountConfig> = config.accounts.iter().collect();
        accounts.sort_by_key(|a| Some(&a.id) != account);
        let mut entries = Vec::new();
        for a in &accounts {
            for identity in &a.identities {
                entries.push(UserAddress {
                    identity_id: Some(identity.id.clone()),
                    email: identity.email.clone(),
                });
            }
        }
        for a in &accounts {
            entries.push(UserAddress {
                identity_id: a.default_identity().map(|i| i.id.clone()),
                email: a.email.clone(),
            });
        }
        UserAddresses::new(entries)
    }

    /// The identity with `email` (or the first identity of the account the
    /// address belongs to).
    fn identity_for(&self, email: &str) -> Option<IdentityId> {
        self.me(None)
            .find(email)
            .and_then(|e| e.identity_id.clone())
    }

    /// The user's copy of the event with `uid` (writable calendars first).
    async fn copy_of(&self, uid: &str) -> Result<Option<Copy>> {
        let wanted = uid.to_owned();
        let events = self
            .store()
            .call(move |s| {
                Ok(s.events(None)?
                    .into_iter()
                    .filter(|e| e.uid == wanted)
                    .collect::<Vec<_>>())
            })
            .await?;
        let mut copies = Vec::new();
        for event in events {
            let Ok(calendar) = self.calendar(&event.calendar_id).await else {
                continue;
            };
            copies.push((event, calendar));
        }
        copies.sort_by_key(|(e, c)| (c.read_only || e.read_only, e.id.0.clone()));
        let Some((event, calendar)) = copies.into_iter().next() else {
            return Ok(None);
        };
        let raw = self
            .item_raw(&PimItemRef::Event {
                event_id: event.id.clone(),
            })
            .await?;
        Ok(Some(Copy {
            event,
            calendar,
            raw,
        }))
    }

    /// The copy behind an event id.
    async fn copy_by_id(&self, id: &EventId) -> Result<Copy> {
        let event = self.event(id).await?;
        let calendar = self.calendar(&event.calendar_id).await?;
        let raw = self
            .item_raw(&PimItemRef::Event {
                event_id: event.id.clone(),
            })
            .await?;
        Ok(Copy {
            event,
            calendar,
            raw,
        })
    }

    /// Whether the server schedules for `calendar` (RFC 6638).
    async fn calendar_scheduling(&self, calendar: &Calendar) -> CalendarScheduling {
        // M3c: Exchange schedules its meetings itself (the engines' meeting
        // APIs, ADR 0077): never iMIP for an Exchange calendar.
        let auto_schedule = calendar.exchange.is_some()
            || crate::pim::engine::source_scheduling(self, &calendar.source_id)
                .await
                .is_some_and(|i| i.auto_schedule);
        CalendarScheduling {
            auto_schedule,
            writable: !calendar.read_only,
        }
    }

    pub(crate) async fn scheduling_settings(&self) -> Result<SchedulingSettings> {
        Ok(self.store().call(|s| s.scheduling_settings()).await?)
    }

    /// `GetInvitation` / `MessageBody.invitation`.
    pub(crate) async fn invitation(&self, id: &MessageId) -> Result<Option<Invitation>> {
        match self.load_itip(id).await? {
            Some(loaded) => Ok(Some(self.card(&loaded).await?)),
            None => Ok(None),
        }
    }

    /// The invitation card of a loaded message.
    async fn card(&self, loaded: &Loaded) -> Result<Invitation> {
        let object = &loaded.object;
        let me = self.me(Some(&loaded.account_id));
        let uid = object.uid.clone().unwrap_or_default();
        let copy = if uid.is_empty() {
            None
        } else {
            self.copy_of(&uid).await?
        };
        let analysis = nail_itip::card::analyze(object, &loaded.senders, &me);
        // The version the calendar has is keyed on the copy's organizer: a
        // message naming another organizer must not compare against "no
        // version" (and so always look newer).
        let organizer = copy
            .as_ref()
            .and_then(|c| c.event.fields.organizer.as_ref())
            .or_else(|| object.organizer())
            .map(|o| normalize(&o.email));
        let from = analysis
            .from_attendee
            .as_ref()
            .map(|a| normalize(&a.attendee.email));
        let rid = rid_key(object.main().and_then(|c| c.recurrence_id.as_ref()));
        let (dtstamp, last_reply, applied) = {
            let (uid, message) = (uid.clone(), loaded.message_id.clone());
            self.store()
                .call(move |s| {
                    let dtstamp = match &organizer {
                        Some(o) => s.itip_state(&uid, "", o)?.and_then(|r| r.dtstamp),
                        None => None,
                    };
                    let last_reply = match &from {
                        Some(a) => s
                            .itip_state(&uid, &rid, a)?
                            .map(|r| Version::new(r.sequence, r.dtstamp)),
                        None => None,
                    };
                    let applied = s
                        .itip_message(&message)?
                        .is_some_and(|r| r.state == ItipMessageState::Applied);
                    Ok((dtstamp, last_reply, applied))
                })
                .await?
        };
        let method = object.method.unwrap_or(ItipMethod::Publish);
        let transport = match &copy {
            Some(c) => {
                let calendar = self.calendar_scheduling(&c.calendar).await;
                match analysis.role {
                    InvitationRole::Organizer => {
                        let recipients: Vec<String> = c
                            .event
                            .fields
                            .attendees
                            .iter()
                            .map(|a| a.email.clone())
                            .filter(|a| !me.contains(a))
                            .collect();
                        nail_itip::server::plan_organizer_delivery(
                            ItipMethod::Request,
                            c.raw.as_deref().unwrap_or_default(),
                            &uid,
                            &recipients,
                            calendar,
                        )
                        .transport()
                    }
                    _ => nail_itip::server::attendee_transport(
                        ItipMethod::Reply,
                        c.raw.as_deref(),
                        &uid,
                        calendar,
                    ),
                }
            }
            None if analysis.identity_id.is_some() => SchedulingTransport::Imip,
            None => SchedulingTransport::None,
        };
        let conflicts = if matches!(
            method,
            ItipMethod::Request | ItipMethod::Add | ItipMethod::Publish | ItipMethod::Counter
        ) {
            self.conflicts(object, copy.as_ref().map(|c| &c.event.id))
                .await
        } else {
            Vec::new()
        };
        Ok(nail_itip::build_card(CardInput {
            message_id: loaded.message_id.clone(),
            object,
            senders: &loaded.senders,
            me: &me,
            stored: copy.as_ref().map(|c| StoredCopy {
                event: &c.event,
                dtstamp: dtstamp.as_deref(),
                read_only: c.calendar.read_only || c.event.read_only,
            }),
            last_reply,
            applied,
            conflicts,
            transport,
        }))
    }

    /// Events of the user's calendars overlapping the invited times.
    async fn conflicts(
        &self,
        object: &ItipObject,
        own: Option<&EventId>,
    ) -> Vec<nail_api::scheduling::InvitationConflict> {
        let viewer = crate::pim::local_zone();
        let tz = crate::pim::local_zone_name();
        let (from, to) = nail_itip::conflicts::window(object, self.pim_utc_now(), viewer);
        let spans = nail_itip::conflicts::invited_spans(object, from, to, viewer);
        if spans.is_empty() {
            return Vec::new();
        }
        let (start, end) = (from.timestamp(), to.timestamp());
        let instances = self
            .store()
            .call(move |s| s.list_event_instances(start, end, None, &tz, &Expander::default()))
            .await
            .unwrap_or_default();
        nail_itip::conflicts::find_conflicts(&spans, &instances, own)
    }

    // --- Writing ------------------------------------------------------------

    /// Saves the user's copy (PIM queue) and returns it with the op ids.
    async fn write_copy(
        &self,
        mut event: CalendarEvent,
        original: Option<&str>,
    ) -> Result<(CalendarEvent, Vec<nail_api::OpId>)> {
        event.updated_at = Some(nail_store::pim::time::utc_string(self.pim_now()));
        let raw = self.event_text(&event, original, None);
        let id = event.id.clone();
        let write = self.save_item(PimItem::Event(event), raw).await?;
        Ok((self.event(&id).await?, crate::pim::op_ids(&write.op_ids)))
    }

    /// Removes the user's copy.
    async fn delete_copy(&self, copy: &Copy) -> Result<Vec<nail_api::OpId>> {
        let write = self
            .delete_item(
                PimItemRef::Event {
                    event_id: copy.event.id.clone(),
                },
                copy.calendar.id.as_str(),
            )
            .await?;
        Ok(crate::pim::op_ids(&write.op_ids))
    }

    /// Where a new event from an invitation goes: the request's calendar,
    /// the settings', else the default one.
    async fn invitation_calendar(&self, asked: Option<CalendarId>) -> Result<CalendarId> {
        let settings = self.scheduling_settings().await?;
        let asked = asked.or(settings.calendar_id);
        self.import_target(asked.as_ref(), CalendarComponent::Event)
            .await
    }

    /// A new copy from a `REQUEST` / `PUBLISH`.
    async fn create_copy(
        &self,
        object: &ItipObject,
        calendar: Option<CalendarId>,
    ) -> Result<(CalendarEvent, Vec<nail_api::OpId>)> {
        let new =
            nail_itip::apply::new_event(object).map_err(|_| issue_error(ItipIssue::NoEvent))?;
        let calendar_id = self.invitation_calendar(calendar).await?;
        let mut event = new.event.into_event(
            EventId::new(new_id("ev")),
            calendar_id,
            false,
            PimItemSync::Pending,
        );
        let now = nail_store::pim::time::utc_string(self.pim_now());
        event.created_at = Some(now);
        self.write_copy(event, Some(&new.text)).await
    }

    /// Records the organizer's version the copy now has.
    ///
    /// The version is keyed on the organizer of the user's copy (the
    /// message's only when no copy names one), the same key the card reads.
    async fn record_version(&self, object: &ItipObject, message: Option<&MessageId>) {
        let Some(uid) = object.uid.clone() else {
            return;
        };
        let stored = match self.copy_of(&uid).await {
            Ok(copy) => copy.and_then(|c| c.event.fields.organizer.map(|o| o.email)),
            Err(_) => None,
        };
        let Some(organizer) = stored.or_else(|| object.organizer().map(|o| o.email.clone())) else {
            return;
        };
        let version = object.version();
        let record = ItipStateRecord {
            uid,
            recurrence_id: String::new(),
            address: normalize(&organizer),
            party: ItipParty::Organizer,
            sequence: version.sequence,
            dtstamp: version.dtstamp,
            status: None,
            message_id: message.cloned(),
            updated_at: self.pim_now(),
        };
        if let Err(err) = self
            .store()
            .call(move |s| s.record_itip_state(&record))
            .await
        {
            tracing::debug!(%err, "cannot record the iTIP version");
        }
    }

    /// Records an attendee's answer (organizer side) or the user's own.
    async fn record_answer(
        &self,
        uid: &str,
        rid: Option<&PimTime>,
        attendee: &Attendee,
        version: &Version,
        message: Option<&MessageId>,
    ) -> bool {
        let record = ItipStateRecord {
            uid: uid.to_owned(),
            recurrence_id: rid_key(rid),
            address: normalize(&attendee.email),
            party: ItipParty::Attendee,
            sequence: version.sequence,
            dtstamp: version.dtstamp.clone(),
            status: Some(attendee.status),
            message_id: message.cloned(),
            updated_at: self.pim_now(),
        };
        self.store()
            .call(move |s| s.record_itip_state(&record))
            .await
            .unwrap_or(false)
    }

    async fn mark_message(
        &self,
        loaded: &Loaded,
        state: ItipMessageState,
        event: Option<&EventId>,
    ) {
        let main = loaded.object.main();
        let record = ItipMessageRecord {
            message_id: loaded.message_id.clone(),
            account_id: loaded.account_id.clone(),
            method: loaded.object.method.unwrap_or(ItipMethod::Publish),
            uid: loaded.object.uid.clone().unwrap_or_default(),
            recurrence_id: rid_key(main.and_then(|c| c.recurrence_id.as_ref())),
            sequence: main.map_or(0, |c| c.sequence),
            dtstamp: main.and_then(|c| c.dtstamp.clone()),
            sender: loaded.senders.first().cloned(),
            state,
            event_id: event.cloned(),
            processed_at: (state != ItipMessageState::New).then(|| self.pim_now()),
        };
        if let Err(err) = self
            .store()
            .call(move |s| s.record_itip_message(&record))
            .await
        {
            tracing::debug!(%err, "cannot record the iTIP message");
        }
    }

    // --- Sending ----------------------------------------------------------

    /// Queues an iMIP mail in the outbox of `identity` (undo send applies)
    /// and records it.
    #[allow(clippy::too_many_arguments)]
    async fn send_imip(
        &self,
        identity_id: &IdentityId,
        method: ItipMethod,
        to: &[(String, Option<String>)],
        calendar: String,
        summary: &nail_itip::Summary<'_>,
        in_reply_to: Option<&str>,
        record: (&str, Option<&PimTime>, u32, Option<ParticipationStatus>),
    ) -> Result<SentItip> {
        let (account, identity) = self.identity(identity_id)?;
        let to: Vec<(String, Option<String>)> = to
            .iter()
            .filter(|(email, _)| !normalize(email).is_empty())
            .cloned()
            .collect();
        if to.is_empty() {
            return Err(refused("nobody to send the message to"));
        }
        let locale = self.inner.locale.clone();
        let localizer = move |m: &LocalizedMessage| crate::strings::render(locale.as_deref(), m);
        let texts = nail_itip::texts::compose(summary, &localizer);
        let mut mail = OutgoingImip::new(
            method,
            MailAddress {
                name: identity.name.clone(),
                email: identity.email.clone(),
            },
            to.iter()
                .map(|(email, name)| MailAddress {
                    name: name.clone(),
                    email: email.clone(),
                })
                .collect(),
            texts.clone(),
            calendar.clone(),
        );
        mail.in_reply_to = in_reply_to.map(str::to_owned);
        mail.date = Some(self.pim_now());
        let raw = mail.to_mime();
        let size = raw.len() as u64;
        let blobs = self.blobs().clone();
        let hash = tokio::task::spawn_blocking(move || blobs.put(&raw)).await??;
        let mut draft = Draft::new(identity.id.clone());
        draft.to = to
            .iter()
            .map(|(email, name)| Address {
                name: name.clone(),
                email: email.clone(),
            })
            .collect();
        draft.subject = texts.subject;
        draft.attachments = vec![DraftAttachment {
            filename: "invitation.eml".to_owned(),
            mime: "message/rfc822".to_owned(),
            size,
            blob: hash.as_str().to_owned(),
            content_id: None,
        }];
        draft.automatic = Some(nail_api::AutomaticMail::Itip);
        let draft_id = self.store_draft(draft, false).await?;
        let entry = self
            .send_draft(draft_id, None, None, Some(SendCrypto::none()))
            .await?;
        let recipients: Vec<String> = to.iter().map(|(e, _)| normalize(e)).collect();
        let (uid, rid, sequence, status) = record;
        let sent = SentItipRecord {
            id: 0,
            account_id: account.id.clone(),
            identity_id: identity.id.clone(),
            method,
            uid: uid.to_owned(),
            recurrence_id: rid_key(rid),
            sequence,
            dtstamp: nail_store::pim::time::utc_string(self.pim_now()),
            status,
            recipients: recipients.clone(),
            transport: SchedulingTransport::Imip,
            outbox_id: Some(entry.id.clone()),
            created_at: self.pim_now(),
        };
        if let Err(err) = self.store().call(move |s| s.record_sent_itip(&sent)).await {
            tracing::debug!(%err, "cannot record the sent iTIP message");
        }
        Ok(SentItip {
            method,
            recipients,
            transport: SchedulingTransport::Imip,
            outbox_id: Some(entry.id),
        })
    }

    /// Records a message the CalDAV server delivers (nothing is mailed).
    async fn record_server_delivery(
        &self,
        identity_id: Option<&IdentityId>,
        method: ItipMethod,
        recipients: Vec<String>,
        record: (&str, Option<&PimTime>, u32, Option<ParticipationStatus>),
    ) -> SentItip {
        if let Some(identity) = identity_id
            && let Ok((account, _)) = self.identity(identity)
        {
            let (uid, rid, sequence, status) = record;
            let sent = SentItipRecord {
                id: 0,
                account_id: account.id,
                identity_id: identity.clone(),
                method,
                uid: uid.to_owned(),
                recurrence_id: rid_key(rid),
                sequence,
                dtstamp: nail_store::pim::time::utc_string(self.pim_now()),
                status,
                recipients: recipients.clone(),
                transport: SchedulingTransport::Server,
                outbox_id: None,
                created_at: self.pim_now(),
            };
            let _ = self.store().call(move |s| s.record_sent_itip(&sent)).await;
        }
        SentItip {
            method,
            recipients,
            transport: SchedulingTransport::Server,
            outbox_id: None,
        }
    }

    fn now_utc(&self) -> DateTime<Utc> {
        self.pim_utc_now()
    }

    // --- Attendee side ------------------------------------------------------

    /// What an answer acts on: the source text, the event's fields, the
    /// user's address and identity, the organizer, the occurrence.
    async fn answer_context(&self, target: &InvitationTarget) -> Result<AnswerContext> {
        match target {
            InvitationTarget::Message { message_id } => {
                let loaded = self.loaded(message_id).await?;
                if let Some(fatal) = loaded.object.issues.iter().find(|i| i.is_fatal()) {
                    return Err(issue_error(*fatal));
                }
                let card = self.card(&loaded).await?;
                let address = card
                    .my_address
                    .clone()
                    .or_else(|| {
                        card.identity_id
                            .as_ref()
                            .and_then(|id| self.identity(id).ok())
                            .map(|(_, i)| i.email)
                    })
                    .ok_or_else(|| issue_error(ItipIssue::NotAddressedToMe))?;
                let identity = card
                    .identity_id
                    .clone()
                    .or_else(|| self.identity_for(&address))
                    .ok_or_else(|| issue_error(ItipIssue::NotAddressedToMe))?;
                let uid = loaded.object.uid.clone().unwrap_or_default();
                let copy = self.copy_of(&uid).await?;
                Ok(AnswerContext {
                    source: loaded.object.text.clone(),
                    uid,
                    recurrence_id: loaded.object.main().and_then(|c| c.recurrence_id.clone()),
                    address,
                    identity,
                    organizer: card.organizer.clone(),
                    fields: card.fields.as_deref().cloned(),
                    recurring: card.recurrence.is_some(),
                    copy,
                    loaded: Some(loaded),
                    card: Some(card),
                })
            }
            InvitationTarget::Event {
                event_id,
                recurrence_id,
            } => {
                let copy = self.copy_by_id(event_id).await?;
                let me = self.me(None);
                let fields = match recurrence_id {
                    Some(rid) => nail_itip::apply::override_for(&copy.event, rid)
                        .map_or(&copy.event.fields, |o| &o.fields)
                        .clone(),
                    None => copy.event.fields.clone(),
                };
                let mine = copy
                    .event
                    .fields
                    .attendees
                    .iter()
                    .filter_map(|a| me.rank(&a.email).map(|r| (r, a.email.clone())))
                    .min_by_key(|(r, _)| *r)
                    .map(|(_, e)| e)
                    .ok_or_else(|| issue_error(ItipIssue::NotAddressedToMe))?;
                let identity = self
                    .identity_for(&mine)
                    .ok_or_else(|| issue_error(ItipIssue::NotAddressedToMe))?;
                let source = copy
                    .raw
                    .clone()
                    .unwrap_or_else(|| self.event_text(&copy.event, None, None));
                Ok(AnswerContext {
                    source,
                    uid: copy.event.uid.clone(),
                    recurrence_id: recurrence_id.clone(),
                    address: mine,
                    identity,
                    organizer: copy.event.fields.organizer.clone(),
                    recurring: copy.event.is_recurring(),
                    fields: Some(fields),
                    copy: Some(copy),
                    loaded: None,
                    card: None,
                })
            }
        }
    }

    /// `RespondToInvitation`.
    async fn respond(
        &self,
        target: InvitationTarget,
        response: InvitationResponse,
        comment: Option<String>,
        send: bool,
        calendar_id: Option<CalendarId>,
    ) -> Result<SchedulingResult> {
        check_comment(comment.as_deref())?;
        let ctx = self.answer_context(&target).await?;
        if let Some(card) = &ctx.card {
            let action = match response {
                InvitationResponse::Accept => InvitationAction::Accept,
                InvitationResponse::Tentative => InvitationAction::Tentative,
                InvitationResponse::Decline => InvitationAction::Decline,
            };
            if !card.allows(action) {
                return Err(refused(format!(
                    "the invitation cannot be answered now ({:?})",
                    card.state
                )));
            }
        }
        let status = response.status();
        let mut op_ids = Vec::new();
        // The copy: new, updated to the invitation, then the answer.
        let (copy_event, original) = match (&ctx.copy, &ctx.loaded) {
            (Some(copy), Some(loaded)) => {
                let newer = ctx
                    .card
                    .as_ref()
                    .is_some_and(|c| c.state == InvitationState::Update);
                let event = if newer {
                    nail_itip::apply::apply_request(&copy.event, &loaded.object)
                        .map_err(|_| issue_error(ItipIssue::NoEvent))?
                } else {
                    copy.event.clone()
                };
                (event, copy.raw.clone())
            }
            (Some(copy), None) => (copy.event.clone(), copy.raw.clone()),
            (None, Some(loaded)) => {
                let (event, ops) = self.create_copy(&loaded.object, calendar_id).await?;
                op_ids.extend(ops);
                let raw = self
                    .item_raw(&PimItemRef::Event {
                        event_id: event.id.clone(),
                    })
                    .await?;
                (event, raw)
            }
            (None, None) => return Err(issue_error(ItipIssue::NoEvent)),
        };
        let name = self.identity(&ctx.identity).ok().and_then(|(_, i)| i.name);
        let answered = nail_itip::apply::set_my_status(
            &copy_event,
            &ctx.address,
            name.as_deref(),
            status,
            ctx.recurrence_id.as_ref(),
        )
        .map_err(|_| refused("not an occurrence of the event"))?;
        let calendar = self.calendar(&answered.calendar_id).await?;
        let scheduling = self.calendar_scheduling(&calendar).await;
        let transport = nail_itip::server::attendee_transport(
            ItipMethod::Reply,
            original.as_deref(),
            &ctx.uid,
            scheduling,
        );
        let (event, ops) = self.write_copy(answered, original.as_deref()).await?;
        op_ids.extend(ops);
        if let Some(loaded) = &ctx.loaded {
            self.record_version(&loaded.object, Some(&loaded.message_id))
                .await;
        }
        let mut me = Attendee::new(normalize(&ctx.address));
        me.name.clone_from(&name);
        me.status = status;
        let version = ctx.loaded.as_ref().map_or_else(
            || Version::new(event.sequence, None),
            |l| l.object.version(),
        );
        self.record_answer(
            &ctx.uid,
            ctx.recurrence_id.as_ref(),
            &me,
            &version,
            ctx.loaded.as_ref().map(|l| &l.message_id),
        )
        .await;
        let mut sent = Vec::new();
        if send && let Some(organizer) = ctx.organizer.as_ref() {
            match transport {
                SchedulingTransport::Imip => {
                    let reply = nail_itip::build::build_reply(
                        &ctx.source,
                        &ReplyOptions {
                            uid: &ctx.uid,
                            recurrence_id: ctx.recurrence_id.as_ref(),
                            attendee: &me,
                            delegates: &[],
                            comment: comment.as_deref(),
                            now: self.now_utc(),
                        },
                    )
                    .map_err(|e| refused(format!("cannot build the reply: {e}")))?;
                    let summary = nail_itip::Summary {
                        method: Some(ItipMethod::Reply),
                        fields: ctx.fields.as_ref(),
                        recurrence_id: ctx.recurrence_id.as_ref(),
                        recurring: ctx.recurring,
                        organizer: Some(organizer),
                        attendee: Some(&me),
                        comment: comment.as_deref(),
                        ..Default::default()
                    };
                    sent.push(
                        self.send_imip(
                            &ctx.identity,
                            ItipMethod::Reply,
                            &[(organizer.email.clone(), organizer.name.clone())],
                            reply,
                            &summary,
                            ctx.loaded
                                .as_ref()
                                .and_then(|l| l.imip.message_id.as_deref()),
                            (
                                &ctx.uid,
                                ctx.recurrence_id.as_ref(),
                                version.sequence,
                                Some(status),
                            ),
                        )
                        .await?,
                    );
                }
                SchedulingTransport::Server => {
                    sent.push(
                        self.record_server_delivery(
                            Some(&ctx.identity),
                            ItipMethod::Reply,
                            vec![normalize(&organizer.email)],
                            (
                                &ctx.uid,
                                ctx.recurrence_id.as_ref(),
                                version.sequence,
                                Some(status),
                            ),
                        )
                        .await,
                    );
                }
                SchedulingTransport::None => {}
            }
        }
        let event_id = event.id.clone();
        self.finish(
            ctx.loaded.as_ref(),
            Some(&event_id),
            op_ids,
            sent,
            Some(event),
        )
        .await
    }

    /// The result of a scheduling write: the message is marked applied,
    /// cards are refreshed.
    async fn finish(
        &self,
        loaded: Option<&Loaded>,
        event_id: Option<&EventId>,
        op_ids: Vec<nail_api::OpId>,
        sent: Vec<SentItip>,
        event: Option<CalendarEvent>,
    ) -> Result<SchedulingResult> {
        let mut invitation = None;
        if let Some(loaded) = loaded {
            self.mark_message(loaded, ItipMessageState::Applied, event_id)
                .await;
            invitation = Some(Box::new(self.card(loaded).await?));
        }
        let uid = event
            .as_ref()
            .map(|e| e.uid.clone())
            .or_else(|| loaded.and_then(|l| l.object.uid.clone()));
        self.cards_changed(uid.as_deref(), loaded.map(|l| &l.message_id))
            .await;
        Ok(SchedulingResult {
            event: event.map(Box::new),
            removed_event_id: None,
            sent,
            op_ids,
            invitation,
        })
    }

    /// Tells clients that the cards of an event's messages changed.
    async fn cards_changed(&self, uid: Option<&str>, also: Option<&MessageId>) {
        let mut ids: Vec<MessageId> = also.into_iter().cloned().collect();
        if let Some(uid) = uid {
            let key = uid.to_owned();
            if let Ok(records) = self.store().call(move |s| s.itip_messages_for(&key)).await {
                for r in records {
                    if !ids.contains(&r.message_id) {
                        ids.push(r.message_id);
                    }
                }
            }
        }
        if !ids.is_empty() {
            self.broadcast(Event::InvitationsChanged { message_ids: ids });
        }
    }

    /// `ProposeNewTime`: a `COUNTER` to the organizer.
    async fn propose(
        &self,
        target: InvitationTarget,
        start: PimTime,
        end: PimTime,
        comment: Option<String>,
    ) -> Result<SchedulingResult> {
        check_comment(comment.as_deref())?;
        let ctx = self.answer_context(&target).await?;
        if let Some(card) = &ctx.card
            && !card.allows(InvitationAction::ProposeNewTime)
        {
            return Err(refused("a new time cannot be proposed for this invitation"));
        }
        let issues = crate::pim::event_issues(
            &EventFields {
                start: start.clone(),
                end: end.clone(),
                ..ctx
                    .fields
                    .clone()
                    .unwrap_or_else(|| EventFields::new("", start.clone(), end.clone()))
            },
            None,
        );
        if !issues.is_empty() {
            return Err(crate::pim::invalid(&issues));
        }
        let organizer = ctx
            .organizer
            .clone()
            .ok_or_else(|| issue_error(ItipIssue::MissingOrganizer))?;
        let proposal = CounterProposal {
            start,
            end,
            location: None,
            comment: comment.clone(),
        };
        let text = nail_itip::build::build_counter(
            &ctx.source,
            &CounterOptions {
                uid: &ctx.uid,
                recurrence_id: ctx.recurrence_id.as_ref(),
                attendee: &ctx.address,
                proposal: &proposal,
                now: self.now_utc(),
            },
        )
        .map_err(|e| refused(format!("cannot build the proposal: {e}")))?;
        let me = Attendee::new(normalize(&ctx.address));
        let summary = nail_itip::Summary {
            method: Some(ItipMethod::Counter),
            fields: ctx.fields.as_ref(),
            recurrence_id: ctx.recurrence_id.as_ref(),
            recurring: ctx.recurring,
            organizer: Some(&organizer),
            attendee: Some(&me),
            comment: comment.as_deref(),
            proposal: Some(&proposal),
            ..Default::default()
        };
        let sequence = ctx
            .loaded
            .as_ref()
            .map_or(0, |l| l.object.version().sequence);
        let sent = self
            .send_imip(
                &ctx.identity,
                ItipMethod::Counter,
                &[(organizer.email.clone(), organizer.name.clone())],
                text,
                &summary,
                ctx.loaded
                    .as_ref()
                    .and_then(|l| l.imip.message_id.as_deref()),
                (&ctx.uid, ctx.recurrence_id.as_ref(), sequence, None),
            )
            .await?;
        let invitation = match &ctx.loaded {
            Some(l) => Some(Box::new(self.card(l).await?)),
            None => None,
        };
        Ok(SchedulingResult {
            event: ctx.copy.map(|c| Box::new(c.event)),
            removed_event_id: None,
            sent: vec![sent],
            op_ids: Vec::new(),
            invitation,
        })
    }

    /// `RefreshInvitation`: a `REFRESH` to the organizer.
    async fn refresh(&self, target: InvitationTarget) -> Result<SchedulingResult> {
        let ctx = self.answer_context(&target).await?;
        let organizer = ctx
            .organizer
            .clone()
            .ok_or_else(|| issue_error(ItipIssue::MissingOrganizer))?;
        let text = nail_itip::build::build_refresh(
            &ctx.source,
            &MinimalOptions {
                uid: &ctx.uid,
                recurrence_id: ctx.recurrence_id.as_ref(),
                attendee: &ctx.address,
                comment: None,
                sequence: None,
                now: self.now_utc(),
            },
        )
        .map_err(|e| refused(format!("cannot build the request: {e}")))?;
        let me = Attendee::new(normalize(&ctx.address));
        let summary = nail_itip::Summary {
            method: Some(ItipMethod::Refresh),
            fields: ctx.fields.as_ref(),
            recurrence_id: ctx.recurrence_id.as_ref(),
            recurring: ctx.recurring,
            organizer: Some(&organizer),
            attendee: Some(&me),
            ..Default::default()
        };
        let sent = self
            .send_imip(
                &ctx.identity,
                ItipMethod::Refresh,
                &[(organizer.email.clone(), organizer.name.clone())],
                text,
                &summary,
                ctx.loaded
                    .as_ref()
                    .and_then(|l| l.imip.message_id.as_deref()),
                (&ctx.uid, ctx.recurrence_id.as_ref(), 0, None),
            )
            .await?;
        Ok(SchedulingResult {
            event: ctx.copy.map(|c| Box::new(c.event)),
            removed_event_id: None,
            sent: vec![sent],
            op_ids: Vec::new(),
            invitation: None,
        })
    }

    /// `ForwardInvitation`: the original `REQUEST` unchanged (RFC 5546
    /// §2.1.5) to new people.
    async fn forward(
        &self,
        target: InvitationTarget,
        to: Vec<Address>,
        comment: Option<String>,
    ) -> Result<SchedulingResult> {
        check_comment(comment.as_deref())?;
        if !nail_api::scheduling::are_valid_recipients(&to) {
            return Err(refused("the recipients are not valid addresses"));
        }
        let ctx = self.answer_context(&target).await?;
        let organizer = ctx.organizer.clone();
        let text = match &ctx.loaded {
            Some(l) => l.object.text.clone(),
            None => nail_itip::build::build_request(
                &ctx.source,
                &RequestOptions {
                    method: ItipMethod::Request,
                    uid: &ctx.uid,
                    recurrence_id: None,
                    sequence: None,
                    now: self.now_utc(),
                },
            )
            .map_err(|e| refused(format!("cannot build the invitation: {e}")))?,
        };
        let summary = nail_itip::Summary {
            method: Some(ItipMethod::Request),
            fields: ctx.fields.as_ref(),
            recurring: ctx.recurring,
            organizer: organizer.as_ref(),
            comment: comment.as_deref(),
            ..Default::default()
        };
        let recipients: Vec<(String, Option<String>)> = to
            .iter()
            .map(|a| (a.email.clone(), a.name.clone()))
            .collect();
        let sent = self
            .send_imip(
                &ctx.identity,
                ItipMethod::Request,
                &recipients,
                text,
                &summary,
                None,
                (&ctx.uid, None, 0, None),
            )
            .await?;
        Ok(SchedulingResult {
            event: ctx.copy.map(|c| Box::new(c.event)),
            removed_event_id: None,
            sent: vec![sent],
            op_ids: Vec::new(),
            invitation: None,
        })
    }

    // --- Applying received messages ------------------------------------------

    /// `ApplyInvitation` (and automatic processing on arrival).
    pub(crate) async fn apply_message(
        &self,
        loaded: &Loaded,
        calendar_id: Option<CalendarId>,
    ) -> Result<SchedulingResult> {
        if let Some(fatal) = loaded.object.issues.iter().find(|i| i.is_fatal()) {
            return Err(issue_error(*fatal));
        }
        let card = self.card(loaded).await?;
        if card.state == InvitationState::Outdated {
            self.mark_message(loaded, ItipMessageState::Outdated, None)
                .await;
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "the calendar has a newer version of this event",
            )
            .into());
        }
        let object = &loaded.object;
        let uid = object.uid.clone().unwrap_or_default();
        let copy = self.copy_of(&uid).await?;
        let mut op_ids = Vec::new();
        let mut sent = Vec::new();
        let mut removed = None;
        let mut event = None;
        match card.method {
            ItipMethod::Request | ItipMethod::Publish => {
                if !(card.allows(InvitationAction::AddToCalendar)
                    || card.allows(InvitationAction::UpdateCalendar))
                {
                    return Err(refused("nothing to add or update"));
                }
                let (e, ops) = match &copy {
                    None => self.create_copy(object, calendar_id).await?,
                    Some(copy) => {
                        let updated = nail_itip::apply::apply_request(&copy.event, object)
                            .map_err(|_| issue_error(ItipIssue::NoEvent))?;
                        self.write_copy(updated, copy.raw.as_deref()).await?
                    }
                };
                self.record_version(object, Some(&loaded.message_id)).await;
                op_ids.extend(ops);
                event = Some(e);
            }
            ItipMethod::Add => {
                let copy = copy.ok_or_else(|| refused("the event is not in a calendar"))?;
                let updated = nail_itip::apply::apply_add(&copy.event, object)
                    .map_err(|_| issue_error(ItipIssue::NoEvent))?;
                let (e, ops) = self.write_copy(updated, copy.raw.as_deref()).await?;
                self.record_version(object, Some(&loaded.message_id)).await;
                op_ids.extend(ops);
                event = Some(e);
            }
            ItipMethod::Cancel => {
                let copy = copy.ok_or_else(|| refused("the event is not in a calendar"))?;
                match nail_itip::apply::apply_cancel(
                    &copy.event,
                    object,
                    card.my_address.as_deref(),
                )
                .map_err(|_| issue_error(ItipIssue::NoEvent))?
                {
                    nail_itip::apply::Applied::Delete => {
                        op_ids.extend(self.delete_copy(&copy).await?);
                        removed = Some(copy.event.id.clone());
                    }
                    nail_itip::apply::Applied::Updated(updated) => {
                        let (e, ops) = self.write_copy(*updated, copy.raw.as_deref()).await?;
                        op_ids.extend(ops);
                        event = Some(e);
                    }
                    nail_itip::apply::Applied::Unchanged => {}
                }
                self.record_version(object, Some(&loaded.message_id)).await;
            }
            ItipMethod::Reply => {
                let copy = copy.ok_or_else(|| refused("the event is not in a calendar"))?;
                let updated = nail_itip::apply::apply_reply(&copy.event, object)
                    .map_err(|_| refused("the reply cannot be applied"))?;
                let (e, ops) = self.write_copy(updated, copy.raw.as_deref()).await?;
                op_ids.extend(ops);
                for c in &object.components {
                    if let Some(a) = c.attendees.first() {
                        self.record_answer(
                            &uid,
                            c.recurrence_id.as_ref(),
                            &a.attendee,
                            &c.version(),
                            Some(&loaded.message_id),
                        )
                        .await;
                    }
                }
                event = Some(e);
            }
            ItipMethod::Refresh => {
                // Answer with the current version.
                let copy = copy.ok_or_else(|| refused("the event is not in a calendar"))?;
                let to = card
                    .from_attendee
                    .clone()
                    .ok_or_else(|| issue_error(ItipIssue::AttendeeCount))?;
                let identity = card
                    .identity_id
                    .clone()
                    .ok_or_else(|| issue_error(ItipIssue::NotAddressedToMe))?;
                sent.extend(
                    self.mail_request(
                        &copy,
                        &identity,
                        std::slice::from_ref(&to.email),
                        None,
                        None,
                        false,
                    )
                    .await?,
                );
                event = Some(copy.event);
            }
            ItipMethod::DeclineCounter | ItipMethod::Counter => {}
        }
        let event_id = event
            .as_ref()
            .map(|e| e.id.clone())
            .or_else(|| removed.clone());
        let mut result = self
            .finish(Some(loaded), event_id.as_ref(), op_ids, sent, event)
            .await?;
        result.removed_event_id = removed;
        Ok(result)
    }

    /// Processes new iTIP mail per the scheduling settings (arrivals;
    /// never junk, never when the sender check failed).
    pub(crate) async fn process_itip_arrivals(&self, ids: &[MessageId], junk: &[MessageId]) {
        let settings = match self.scheduling_settings().await {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(%err, "cannot read the scheduling settings");
                return;
            }
        };
        for id in ids.iter().filter(|id| !junk.contains(id)) {
            let loaded = match self.load_itip(id).await {
                Ok(Some(loaded)) => loaded,
                Ok(None) => continue,
                Err(err) => {
                    tracing::debug!(message = %id, %err, "cannot read the iTIP part");
                    continue;
                }
            };
            self.mark_message(&loaded, ItipMessageState::New, None)
                .await;
            let card = match self.card(&loaded).await {
                Ok(card) => card,
                Err(err) => {
                    tracing::debug!(message = %id, %err, "cannot build the invitation card");
                    continue;
                }
            };
            // Spoofed senders, `SENT-BY` claims, another organizer than the
            // copy's, uninvited attendees, messages about other people's
            // events: the card shows them, nothing is applied on its own.
            let blocked = card.issues.iter().any(|i| i.blocks_automatic());
            if blocked || card.state == InvitationState::Invalid {
                self.mark_message(&loaded, ItipMessageState::Ignored, None)
                    .await;
                continue;
            }
            if card.state == InvitationState::Outdated {
                self.mark_message(&loaded, ItipMessageState::Outdated, None)
                    .await;
                continue;
            }
            // PUBLISH carries no sender check and observers are not part of
            // the event: both are only ever applied by hand. Organizer
            // messages are applied for attendees only (the user's own
            // REQUEST coming back is not an update), answers for the
            // organizer only.
            let eligible = card.method != ItipMethod::Publish
                && card.role
                    == if card.method.from_organizer() {
                        InvitationRole::Attendee
                    } else {
                        InvitationRole::Organizer
                    };
            let automatic = eligible
                && ((card.allows(InvitationAction::AddToCalendar)
                    && settings.add_invitations
                    && card.role == InvitationRole::Attendee)
                    || (card.allows(InvitationAction::UpdateCalendar) && settings.apply_updates)
                    || (card.allows(InvitationAction::ApplyReply) && settings.apply_replies)
                    || (card.allows(InvitationAction::RemoveFromCalendar)
                        && settings.apply_cancellations));
            if automatic {
                match self.apply_message(&loaded, None).await {
                    Ok(_) => {
                        tracing::info!(message = %id, method = ?card.method, "invitation applied")
                    }
                    Err(err) => {
                        tracing::info!(message = %id, %err, "invitation not applied");
                        self.mark_message(&loaded, ItipMessageState::Failed, None)
                            .await;
                    }
                }
            } else {
                self.cards_changed(None, Some(id)).await;
            }
        }
    }

    // --- Organizer side -------------------------------------------------------

    /// Mails the organizer's copy as `REQUEST` to `recipients` (those Nail
    /// delivers; the server delivers to the rest).
    async fn mail_request(
        &self,
        copy: &Copy,
        identity: &IdentityId,
        recipients: &[String],
        recurrence_id: Option<&PimTime>,
        comment: Option<&str>,
        update: bool,
    ) -> Result<Vec<SentItip>> {
        let raw = copy
            .raw
            .clone()
            .unwrap_or_else(|| self.event_text(&copy.event, None, None));
        let scheduling = self.calendar_scheduling(&copy.calendar).await;
        let plan = nail_itip::server::plan_organizer_delivery(
            ItipMethod::Request,
            &raw,
            &copy.event.uid,
            recipients,
            scheduling,
        );
        let mut sent = Vec::new();
        let sequence = copy.event.sequence;
        if !plan.imip.is_empty() {
            let text = nail_itip::build::build_request(
                &raw,
                &RequestOptions {
                    method: ItipMethod::Request,
                    uid: &copy.event.uid,
                    recurrence_id,
                    sequence: Some(sequence),
                    now: self.now_utc(),
                },
            )
            .map_err(|e| refused(format!("cannot build the invitation: {e}")))?;
            let fields = match recurrence_id {
                Some(rid) => nail_itip::apply::override_for(&copy.event, rid)
                    .map_or(&copy.event.fields, |o| &o.fields),
                None => &copy.event.fields,
            };
            let summary = nail_itip::Summary {
                method: Some(ItipMethod::Request),
                update,
                fields: Some(fields),
                recurrence_id,
                recurring: copy.event.is_recurring(),
                organizer: copy.event.fields.organizer.as_ref(),
                comment,
                ..Default::default()
            };
            let to = names_of(&copy.event.fields.attendees, &plan.imip);
            sent.push(
                self.send_imip(
                    identity,
                    ItipMethod::Request,
                    &to,
                    text,
                    &summary,
                    None,
                    (&copy.event.uid, recurrence_id, sequence, None),
                )
                .await?,
            );
        }
        if !plan.server.is_empty() {
            sent.push(
                self.record_server_delivery(
                    Some(identity),
                    ItipMethod::Request,
                    plan.server.clone(),
                    (&copy.event.uid, recurrence_id, sequence, None),
                )
                .await,
            );
        }
        Ok(sent)
    }

    /// The organizer's identity of an event (`invalid_request` when the
    /// user does not organize it).
    fn organizer_identity(&self, event: &CalendarEvent) -> Result<IdentityId> {
        let organizer = event
            .fields
            .organizer
            .as_ref()
            .ok_or_else(|| refused("the event has no organizer"))?;
        self.identity_for(&organizer.email)
            .ok_or_else(|| refused("you do not organize this event"))
    }

    /// `SendInvitations`.
    async fn send_invitations(
        &self,
        event_id: &EventId,
        recurrence_id: Option<&PimTime>,
        attendees: &[String],
        comment: Option<String>,
    ) -> Result<SchedulingResult> {
        check_comment(comment.as_deref())?;
        let copy = self.copy_by_id(event_id).await?;
        let identity = self.organizer_identity(&copy.event)?;
        let me = self.me(None);
        let recipients: Vec<String> = copy
            .event
            .fields
            .attendees
            .iter()
            .map(|a| a.email.clone())
            .filter(|a| !me.contains(a))
            .filter(|a| attendees.is_empty() || attendees.iter().any(|w| same(w, a)))
            .collect();
        if recipients.is_empty() {
            return Err(refused("the event has nobody to invite"));
        }
        let uid = copy.event.uid.clone();
        let earlier = {
            let key = uid.clone();
            self.store()
                .call(move |s| s.sent_itips(&key))
                .await?
                .iter()
                .any(|r| r.method == ItipMethod::Request)
        };
        let sent = self
            .mail_request(
                &copy,
                &identity,
                &recipients,
                recurrence_id,
                comment.as_deref(),
                earlier,
            )
            .await?;
        self.cards_changed(Some(&uid), None).await;
        Ok(SchedulingResult {
            event: Some(Box::new(copy.event)),
            removed_event_id: None,
            sent,
            op_ids: Vec::new(),
            invitation: None,
        })
    }

    /// `CancelMeeting`.
    async fn cancel_meeting(
        &self,
        event_id: &EventId,
        scope: CancelScope,
        recurrence_id: Option<PimTime>,
        attendees: &[String],
        comment: Option<String>,
    ) -> Result<SchedulingResult> {
        check_comment(comment.as_deref())?;
        let copy = self.copy_by_id(event_id).await?;
        let identity = self.organizer_identity(&copy.event)?;
        let me = self.me(None);
        let raw = copy
            .raw
            .clone()
            .unwrap_or_else(|| self.event_text(&copy.event, None, None));
        let everyone: Vec<String> = copy
            .event
            .fields
            .attendees
            .iter()
            .map(|a| a.email.clone())
            .filter(|a| !me.contains(a))
            .collect();
        let sequence = copy.event.sequence + 1;
        let (what, cancelled_for): (CancelWhat<'_>, Vec<String>) = match scope {
            CancelScope::Meeting => (CancelWhat::Meeting, everyone.clone()),
            CancelScope::Occurrence => (
                CancelWhat::Occurrence(
                    recurrence_id
                        .as_ref()
                        .ok_or_else(|| refused("which occurrence?"))?,
                ),
                everyone.clone(),
            ),
            CancelScope::Attendees => {
                if attendees.is_empty() {
                    return Err(refused("which attendees?"));
                }
                (CancelWhat::Attendees(attendees), attendees.to_vec())
            }
        };
        let text = nail_itip::build::build_cancel(
            &raw,
            &CancelOptions {
                uid: &copy.event.uid,
                what: what.clone(),
                sequence,
                comment: comment.as_deref(),
                now: self.now_utc(),
            },
        )
        .map_err(|e| refused(format!("cannot build the cancellation: {e}")))?;
        let scheduling = self.calendar_scheduling(&copy.calendar).await;
        let plan = nail_itip::server::plan_organizer_delivery(
            ItipMethod::Cancel,
            &raw,
            &copy.event.uid,
            &cancelled_for,
            scheduling,
        );
        let mut sent = Vec::new();
        if !plan.imip.is_empty() {
            let summary = nail_itip::Summary {
                method: Some(ItipMethod::Cancel),
                fields: Some(&copy.event.fields),
                recurrence_id: recurrence_id
                    .as_ref()
                    .filter(|_| scope == CancelScope::Occurrence),
                recurring: copy.event.is_recurring(),
                organizer: copy.event.fields.organizer.as_ref(),
                comment: comment.as_deref(),
                ..Default::default()
            };
            let to = names_of(&copy.event.fields.attendees, &plan.imip);
            sent.push(
                self.send_imip(
                    &identity,
                    ItipMethod::Cancel,
                    &to,
                    text,
                    &summary,
                    None,
                    (&copy.event.uid, recurrence_id.as_ref(), sequence, None),
                )
                .await?,
            );
        }
        if !plan.server.is_empty() {
            sent.push(
                self.record_server_delivery(
                    Some(&identity),
                    ItipMethod::Cancel,
                    plan.server.clone(),
                    (&copy.event.uid, recurrence_id.as_ref(), sequence, None),
                )
                .await,
            );
        }
        // The organizer's copy.
        let mut op_ids = Vec::new();
        let mut removed = None;
        let mut event = None;
        match scope {
            CancelScope::Meeting => {
                op_ids.extend(self.delete_copy(&copy).await?);
                removed = Some(copy.event.id.clone());
            }
            CancelScope::Occurrence => {
                let mut updated = copy.event.clone();
                let rid = recurrence_id
                    .as_ref()
                    .ok_or_else(|| refused("which occurrence?"))?;
                nail_ical::edit::delete_occurrence(&mut updated, rid)
                    .map_err(|_| refused("not an occurrence of the event"))?;
                updated.sequence = sequence;
                let (e, ops) = self.write_copy(updated, copy.raw.as_deref()).await?;
                op_ids.extend(ops);
                event = Some(e);
            }
            CancelScope::Attendees => {
                let mut updated = copy.event.clone();
                updated
                    .fields
                    .attendees
                    .retain(|a| !attendees.iter().any(|r| same(r, &a.email)));
                for o in &mut updated.overrides {
                    o.fields
                        .attendees
                        .retain(|a| !attendees.iter().any(|r| same(r, &a.email)));
                }
                updated.sequence = sequence;
                let (e, ops) = self.write_copy(updated, copy.raw.as_deref()).await?;
                op_ids.extend(ops);
                event = Some(e);
            }
        }
        self.cards_changed(Some(&copy.event.uid), None).await;
        Ok(SchedulingResult {
            event: event.map(Box::new),
            removed_event_id: removed,
            sent,
            op_ids,
            invitation: None,
        })
    }

    /// `RespondToCounter`.
    async fn respond_to_counter(
        &self,
        message_id: &MessageId,
        accept: bool,
        comment: Option<String>,
    ) -> Result<SchedulingResult> {
        check_comment(comment.as_deref())?;
        let loaded = self.loaded(message_id).await?;
        let card = self.card(&loaded).await?;
        let action = if accept {
            InvitationAction::AcceptCounter
        } else {
            InvitationAction::DeclineCounter
        };
        if card.method != ItipMethod::Counter || !card.allows(action) {
            return Err(refused("this is not a proposal that can be answered"));
        }
        let uid = loaded.object.uid.clone().unwrap_or_default();
        let copy = self
            .copy_of(&uid)
            .await?
            .ok_or_else(|| refused("the event is not in a calendar"))?;
        let identity = self.organizer_identity(&copy.event)?;
        let proposer = card
            .from_attendee
            .clone()
            .ok_or_else(|| issue_error(ItipIssue::AttendeeCount))?;
        let rid = card.recurrence_id.clone();
        let mut op_ids = Vec::new();
        let mut sent = Vec::new();
        let mut event = Some(copy.event.clone());
        if accept {
            let proposal = card
                .counter
                .clone()
                .ok_or_else(|| issue_error(ItipIssue::NoEvent))?;
            let organizer = copy
                .event
                .fields
                .organizer
                .as_ref()
                .map(|o| o.email.clone())
                .unwrap_or_default();
            let updated =
                nail_itip::apply::accept_counter(&copy.event, &proposal, rid.as_ref(), &organizer)
                    .map_err(|_| refused("the proposal cannot be applied"))?;
            let (saved, ops) = self.write_copy(updated, copy.raw.as_deref()).await?;
            op_ids.extend(ops);
            let fresh = self.copy_by_id(&saved.id).await?;
            let me = self.me(None);
            let everyone: Vec<String> = fresh
                .event
                .fields
                .attendees
                .iter()
                .map(|a| a.email.clone())
                .filter(|a| !me.contains(a))
                .collect();
            sent.extend(
                self.mail_request(&fresh, &identity, &everyone, None, comment.as_deref(), true)
                    .await?,
            );
            event = Some(saved);
        } else {
            let raw = copy
                .raw
                .clone()
                .unwrap_or_else(|| self.event_text(&copy.event, None, None));
            let text = nail_itip::build::build_decline_counter(
                &raw,
                &MinimalOptions {
                    uid: &uid,
                    recurrence_id: rid.as_ref(),
                    attendee: &proposer.email,
                    comment: comment.as_deref(),
                    sequence: Some(copy.event.sequence),
                    now: self.now_utc(),
                },
            )
            .map_err(|e| refused(format!("cannot build the answer: {e}")))?;
            let summary = nail_itip::Summary {
                method: Some(ItipMethod::DeclineCounter),
                fields: Some(&copy.event.fields),
                recurrence_id: rid.as_ref(),
                recurring: copy.event.is_recurring(),
                organizer: copy.event.fields.organizer.as_ref(),
                attendee: Some(&proposer),
                comment: comment.as_deref(),
                ..Default::default()
            };
            sent.push(
                self.send_imip(
                    &identity,
                    ItipMethod::DeclineCounter,
                    &[(proposer.email.clone(), proposer.name.clone())],
                    text,
                    &summary,
                    loaded.imip.message_id.as_deref(),
                    (&uid, rid.as_ref(), copy.event.sequence, None),
                )
                .await?,
            );
        }
        let event_id = event.as_ref().map(|e| e.id.clone());
        self.finish(Some(&loaded), event_id.as_ref(), op_ids, sent, event)
            .await
    }

    // --- Free/busy ----------------------------------------------------------

    /// `QueryFreeBusy`.
    async fn free_busy(
        &self,
        attendees: Vec<String>,
        from: String,
        to: String,
        identity_id: Option<IdentityId>,
        ignore_event_id: Option<EventId>,
    ) -> Result<FreeBusyResult> {
        let issues = nail_api::scheduling::free_busy_issues(&attendees, &from, &to);
        if let Some(first) = issues.first() {
            return Err(
                ApiError::invalid_request(format!("invalid free/busy query: {issues:?}"))
                    .with_localized(LocalizedMessage::new(first.message_id()))
                    .into(),
            );
        }
        let me = self.me(None);
        let organizer = identity_id
            .as_ref()
            .and_then(|id| self.identity(id).ok())
            .map(|(_, i)| i.email)
            .or_else(|| me.entries().first().map(|e| e.email.clone()))
            .unwrap_or_default();
        let mut answers: HashMap<String, AttendeeFreeBusy> = HashMap::new();
        // The user's own calendars.
        let own: Vec<String> = attendees
            .iter()
            .filter(|a| me.contains(a))
            .cloned()
            .collect();
        if !own.is_empty() {
            let periods = self
                .own_busy(&from, &to, ignore_event_id.clone(), &me)
                .await?;
            for a in own {
                answers.insert(
                    normalize(&a),
                    AttendeeFreeBusy {
                        email: a,
                        source: FreeBusySource::Own,
                        periods: periods.clone(),
                        error: None,
                        localized: None,
                    },
                );
            }
        }
        // CalDAV schedule outboxes.
        let mut rest: Vec<String> = attendees
            .iter()
            .filter(|a| !answers.contains_key(&normalize(a)))
            .cloned()
            .collect();
        let sources: Vec<PimSourceConfig> = self
            .inner
            .accounts
            .config()
            .pim_sources
            .iter()
            .filter(|s| s.enabled && s.kind() == nail_api::pim::PimSourceKind::Caldav)
            .cloned()
            .collect();
        for source in sources {
            if rest.is_empty() {
                break;
            }
            let Some(info) = crate::pim::engine::source_scheduling(self, &source.id).await else {
                continue;
            };
            let client = match crate::pim::engine::dav_client(self, &source, None).await {
                Ok(c) => c,
                Err(err) => {
                    tracing::debug!(source = %source.id, %err, "no DAV client for free/busy");
                    continue;
                }
            };
            let Some(provider) = nail_dav::CalDavFreeBusy::new(client, source.id.clone(), info)
            else {
                continue;
            };
            let query = FreeBusyQuery {
                organizer: organizer.clone(),
                attendees: rest.clone(),
                from: from.clone(),
                to: to.clone(),
            };
            match nail_api::engine::FreeBusyProvider::free_busy(&provider, &query).await {
                Ok(found) => {
                    for entry in found {
                        answers.insert(normalize(&entry.email), entry);
                    }
                }
                Err(err) => {
                    // Every attendee asked there shows the failure.
                    for a in &rest {
                        answers.insert(
                            normalize(a),
                            AttendeeFreeBusy {
                                email: a.clone(),
                                source: FreeBusySource::Caldav {
                                    source_id: source.id.clone(),
                                },
                                periods: Vec::new(),
                                error: Some(err.code),
                                localized: err.localized.clone(),
                            },
                        );
                    }
                }
            }
            rest.retain(|a| !answers.contains_key(&normalize(a)));
        }
        // M3c: people and rooms of Exchange organizations.
        if !rest.is_empty() {
            let query = FreeBusyQuery {
                organizer: organizer.clone(),
                attendees: rest.clone(),
                from: from.clone(),
                to: to.clone(),
            };
            for entry in crate::exchange::free_busy(self, &query).await {
                answers.insert(normalize(&entry.email), entry);
            }
        }
        Ok(FreeBusyResult {
            attendees: attendees
                .iter()
                .map(|a| {
                    answers
                        .remove(&normalize(a))
                        .unwrap_or_else(|| AttendeeFreeBusy {
                            email: a.clone(),
                            source: FreeBusySource::Unknown,
                            periods: Vec::new(),
                            error: None,
                            localized: None,
                        })
                })
                .collect(),
            from,
            to,
        })
    }

    /// Busy periods of the user's own calendars (subscriptions and other
    /// read-only feeds do not count).
    async fn own_busy(
        &self,
        from: &str,
        to: &str,
        exclude: Option<EventId>,
        me: &UserAddresses,
    ) -> Result<Vec<nail_api::scheduling::BusyPeriod>> {
        let (Some(start), Some(end)) = (
            nail_api::scheduling::utc_seconds(from),
            nail_api::scheduling::utc_seconds(to),
        ) else {
            return Ok(Vec::new());
        };
        let calendars: Vec<CalendarId> = self
            .list_calendars(None)
            .await?
            .into_iter()
            .filter(|c| !c.read_only && c.components.contains(&CalendarComponent::Event))
            .map(|c| c.id)
            .collect();
        let tz = crate::pim::local_zone_name();
        let wanted = calendars.clone();
        let (instances, events) = self
            .store()
            .call(move |s| {
                let instances =
                    s.list_event_instances(start, end, Some(&wanted), &tz, &Expander::default())?;
                let events = s.events(Some(&wanted))?;
                Ok((instances, events))
            })
            .await?;
        let by_id: HashMap<&EventId, &CalendarEvent> = events.iter().map(|e| (&e.id, e)).collect();
        let my_status = |i: &nail_api::pim::EventInstance| -> Option<ParticipationStatus> {
            let event = by_id.get(&i.event_id)?;
            let fields = match &i.recurrence_id {
                Some(rid) => {
                    nail_itip::apply::override_for(event, rid).map_or(&event.fields, |o| &o.fields)
                }
                None => &event.fields,
            };
            if fields
                .organizer
                .as_ref()
                .is_some_and(|o| me.contains(&o.email))
            {
                return None;
            }
            fields
                .attendees
                .iter()
                .find(|a| me.contains(&a.email))
                .map(|a| a.status)
        };
        Ok(nail_dav::own_busy_periods(
            &instances,
            &nail_dav::OwnFreeBusy {
                from: from.to_owned(),
                to: to.to_owned(),
                exclude_event: exclude,
                calendars: Some(calendars),
            },
            my_status,
        ))
    }

    /// Fills `organizer` of a new event with attendees (the identity of the
    /// calendar's account, else the first identity), so the organizer
    /// flows recognise the user.
    pub(crate) fn default_organizer(&self, fields: &mut EventFields, calendar: &Calendar) {
        if fields.attendees.is_empty() || fields.organizer.is_some() {
            return;
        }
        let config = self.inner.accounts.config();
        let account = calendar
            .account_id
            .clone()
            .or_else(|| self.pim_source_account(&calendar.source_id));
        let identity = account
            .as_ref()
            .and_then(|a| config.account(a))
            .and_then(|a| a.default_identity())
            .or_else(|| config.accounts.iter().find_map(|a| a.default_identity()));
        if let Some(identity) = identity {
            fields.organizer = Some(nail_api::pim::calendar::Organizer {
                email: identity.email.clone(),
                name: identity.name.clone(),
            });
        }
    }
}

/// What answering needs.
struct AnswerContext {
    source: String,
    uid: String,
    recurrence_id: Option<PimTime>,
    address: String,
    identity: IdentityId,
    organizer: Option<nail_api::pim::calendar::Organizer>,
    fields: Option<EventFields>,
    recurring: bool,
    copy: Option<Copy>,
    loaded: Option<Loaded>,
    card: Option<Invitation>,
}

/// `(address, name)` of `recipients` with the names the event knows.
fn names_of(attendees: &[Attendee], recipients: &[String]) -> Vec<(String, Option<String>)> {
    recipients
        .iter()
        .map(|r| {
            let name = attendees
                .iter()
                .find(|a| same(&a.email, r))
                .and_then(|a| a.name.clone());
            (r.clone(), name)
        })
        .collect()
}

fn check_comment(comment: Option<&str>) -> Result<()> {
    if comment.is_some_and(|c| !nail_api::scheduling::is_valid_comment(c)) {
        return Err(refused("the comment is too long or has control characters"));
    }
    Ok(())
}

/// Reads the schedule inbox of a CalDAV source (RFC 6638): the server
/// already put the messages' effects into the calendars; Nail records
/// the versions, refreshes cards and removes the items.
pub(crate) async fn process_schedule_inbox(
    service: &Service,
    config: &PimSourceConfig,
    client: &nail_dav::DavClient,
) {
    let Some(info) = crate::pim::engine::source_scheduling(service, &config.id).await else {
        return;
    };
    let Some(inbox) = info.inbox.clone() else {
        return;
    };
    let items = match nail_dav::list_inbox(client, &inbox).await {
        Ok(items) => items,
        Err(err) => {
            tracing::debug!(source = %config.id, %err, "cannot read the schedule inbox");
            return;
        }
    };
    let mut uids: Vec<String> = Vec::new();
    for item in &items {
        let object = nail_itip::parse_itip(&item.data, None);
        if object.method.is_some_and(ItipMethod::from_organizer) {
            service.record_version(&object, None).await;
        } else if let (Some(uid), Some(main)) = (object.uid.as_deref(), object.main())
            && let Some(a) = main.attendees.first()
        {
            service
                .record_answer(
                    uid,
                    main.recurrence_id.as_ref(),
                    &a.attendee,
                    &main.version(),
                    None,
                )
                .await;
        }
        if let Err(err) = nail_dav::delete_inbox_item(client, item).await {
            tracing::debug!(source = %config.id, %err, "cannot remove a schedule inbox item");
            continue;
        }
        if let Some(uid) = &item.uid
            && !uids.contains(uid)
        {
            uids.push(uid.clone());
        }
    }
    if !items.is_empty() {
        tracing::info!(source = %config.id, items = items.len(), "schedule inbox processed");
    }
    for uid in uids {
        service.cards_changed(Some(&uid), None).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurrence_keys_match_the_store_format() {
        assert_eq!(rid_key(None), "");
        let rid = PimTime::utc("2026-10-05T09:00:00");
        assert_eq!(rid_key(Some(&rid)), serde_json::to_string(&rid).unwrap());
    }

    #[test]
    fn recipients_carry_known_names() {
        let mut bob = Attendee::new("bob@example.org");
        bob.name = Some("Bob".into());
        let out = names_of(
            &[bob],
            &["BOB@example.org".into(), "eve@example.org".into()],
        );
        assert_eq!(out[0].1.as_deref(), Some("Bob"));
        assert_eq!(out[1].1, None);
    }

    #[test]
    fn comments_are_checked() {
        assert!(check_comment(None).is_ok());
        assert!(check_comment(Some("See you")).is_ok());
        assert!(check_comment(Some("\u{7}")).is_err());
        let err = issue_error(ItipIssue::NotAddressedToMe);
        assert_eq!(err.code(), ErrorCode::InvalidRequest);
        assert_eq!(
            err.localized().map(|l| l.id),
            Some("itip-issue-not-addressed-to-me".to_owned())
        );
    }
}
