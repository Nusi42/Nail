//! Directory lookups: room lists and rooms (`GetRoomLists`, `GetRooms`),
//! free/busy of people and rooms (`GetUserAvailability`), names
//! (`ResolveNames`) and delegates (`GetDelegate`).

use nail_api::exchange::{Room, RoomList};
use nail_api::scheduling::{BusyKind, BusyPeriod};

use crate::client::{EwsClient, Operation};
use crate::error::EwsError;
use crate::soap::{self, ResponseMessage};
use crate::types::{format_utc, parse_time};
use crate::xml::{Element, el};

/// Most mailboxes one `GetUserAvailability` call may ask about.
pub const MAX_AVAILABILITY_MAILBOXES: usize = 100;

/// The free/busy of one mailbox, or why it could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Availability {
    /// The address asked.
    pub address: String,
    /// Busy periods (UTC, merged), or the error for this mailbox.
    pub result: Result<Vec<BusyPeriod>, EwsError>,
}

/// A delegate of the mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegate {
    /// The delegate's address.
    pub address: String,
    /// Display name.
    pub name: Option<String>,
    /// Permission levels per folder (`Calendar` → `Editor`, …).
    pub permissions: Vec<(String, String)>,
    /// Receives copies of meeting messages.
    pub receives_meeting_messages: bool,
    /// May see private items.
    pub view_private_items: bool,
}

fn busy_kind(text: &str) -> Option<BusyKind> {
    match text {
        "Tentative" => Some(BusyKind::Tentative),
        "Busy" => Some(BusyKind::Busy),
        "OOF" => Some(BusyKind::Unavailable),
        _ => None,
    }
}

/// The `t:TimeZone` of `GetUserAvailabilityRequest` for UTC (no bias, no
/// daylight saving), so every time in the answer is UTC.
const UTC_ZONE: &str = concat!(
    "<t:TimeZone><t:Bias>0</t:Bias>",
    "<t:StandardTime><t:Bias>0</t:Bias><t:Time>00:00:00</t:Time><t:DayOrder>1</t:DayOrder>",
    "<t:Month>1</t:Month><t:DayOfWeek>Sunday</t:DayOfWeek></t:StandardTime>",
    "<t:DaylightTime><t:Bias>0</t:Bias><t:Time>00:00:00</t:Time><t:DayOrder>1</t:DayOrder>",
    "<t:Month>1</t:Month><t:DayOfWeek>Sunday</t:DayOfWeek></t:DaylightTime></t:TimeZone>"
);

/// Reads one `FreeBusyResponse`.
fn parse_free_busy(response: &Element) -> Result<Vec<BusyPeriod>, EwsError> {
    if let Some(message) = response.child("ResponseMessage") {
        ResponseMessage::read(message).check()?;
    }
    let events = response
        .path(&["FreeBusyView", "CalendarEventArray"])
        .map(|a| a.children_named("CalendarEvent").collect::<Vec<_>>())
        .unwrap_or_default();
    let periods = events
        .into_iter()
        .filter_map(|event| {
            Some(BusyPeriod {
                start: format_utc(parse_time(event.child_text("StartTime")?)?),
                end: format_utc(parse_time(event.child_text("EndTime")?)?),
                kind: busy_kind(event.child_text("BusyType")?)?,
            })
        })
        .collect();
    Ok(BusyPeriod::merge(periods))
}

fn address_entry(element: &Element) -> Option<(String, String)> {
    Some((
        element.child_value("EmailAddress")?,
        element.child_value("Name").unwrap_or_default(),
    ))
}

impl EwsClient {
    /// `GetRoomLists`: the organization's room lists.
    pub async fn get_room_lists(&self) -> Result<Vec<RoomList>, EwsError> {
        let response = self
            .call(&Operation::read(
                "GetRoomLists",
                "<m:GetRoomLists/>".to_owned(),
            ))
            .await?;
        ResponseMessage::read(&response.body).check()?;
        Ok(response
            .body
            .child("RoomLists")
            .map(|lists| {
                lists
                    .children_named("Address")
                    .filter_map(address_entry)
                    .map(|(address, name)| RoomList { address, name })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `GetRooms`: the rooms of a room list.
    pub async fn get_rooms(&self, room_list: &str) -> Result<Vec<Room>, EwsError> {
        let body = format!(
            "<m:GetRooms><m:RoomList>{}</m:RoomList></m:GetRooms>",
            el("t:EmailAddress", room_list)
        );
        let response = self.call(&Operation::read("GetRooms", body)).await?;
        ResponseMessage::read(&response.body).check()?;
        Ok(response
            .body
            .child("Rooms")
            .map(|rooms| {
                rooms
                    .children_named("Room")
                    .filter_map(|r| r.child("Id").and_then(address_entry))
                    .map(|(address, name)| Room {
                        address,
                        name,
                        capacity: None,
                        building: None,
                        floor: None,
                        accessible: None,
                        features: Vec::new(),
                        room_list: Some(room_list.to_owned()),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `GetUserAvailability` (free/busy view) of mailboxes between `from`
    /// and `to` (RFC 3339): one entry per address in order. `rooms` are
    /// asked as rooms.
    pub async fn get_availability(
        &self,
        addresses: &[String],
        rooms: &[String],
        from: &str,
        to: &str,
    ) -> Result<Vec<Availability>, EwsError> {
        let (Some(from), Some(to)) = (parse_time(from), parse_time(to)) else {
            return Err(EwsError::Rejected {
                code: "InvalidRange".into(),
                message: "free/busy needs RFC 3339 times".into(),
            });
        };
        let mut out = Vec::with_capacity(addresses.len());
        for chunk in addresses.chunks(MAX_AVAILABILITY_MAILBOXES) {
            let mailboxes: String = chunk
                .iter()
                .map(|a| {
                    let kind = if rooms.iter().any(|r| r.eq_ignore_ascii_case(a)) {
                        "Room"
                    } else {
                        "Required"
                    };
                    format!(
                        "<t:MailboxData><t:Email>{}</t:Email>{}<t:ExcludeConflicts>false</t:ExcludeConflicts></t:MailboxData>",
                        el("t:Address", a),
                        el("t:AttendeeType", kind)
                    )
                })
                .collect();
            let body = format!(
                concat!(
                    "<m:GetUserAvailabilityRequest>{zone}<m:MailboxDataArray>{mailboxes}</m:MailboxDataArray>",
                    "<t:FreeBusyViewOptions><t:TimeWindow><t:StartTime>{from}</t:StartTime><t:EndTime>{to}</t:EndTime>",
                    "</t:TimeWindow><t:MergedFreeBusyIntervalInMinutes>30</t:MergedFreeBusyIntervalInMinutes>",
                    "<t:RequestedView>FreeBusy</t:RequestedView></t:FreeBusyViewOptions></m:GetUserAvailabilityRequest>"
                ),
                zone = UTC_ZONE,
                mailboxes = mailboxes,
                from = from.format("%Y-%m-%dT%H:%M:%S"),
                to = to.format("%Y-%m-%dT%H:%M:%S"),
            );
            let response = self
                .call(&Operation::read("GetUserAvailability", body))
                .await?;
            let answers: Vec<&Element> = response
                .body
                .child("FreeBusyResponseArray")
                .map(|a| a.children_named("FreeBusyResponse").collect())
                .unwrap_or_default();
            if answers.len() != chunk.len() {
                return Err(EwsError::Protocol(format!(
                    "GetUserAvailability answered {} of {} mailboxes",
                    answers.len(),
                    chunk.len()
                )));
            }
            for (address, answer) in chunk.iter().zip(answers) {
                out.push(Availability {
                    address: address.clone(),
                    result: parse_free_busy(answer),
                });
            }
        }
        Ok(out)
    }

    /// `ResolveNames` in the directory: the mailbox's display name and
    /// address, or [`EwsError::NotFound`].
    pub async fn resolve_name(&self, entry: &str) -> Result<(String, Option<String>), EwsError> {
        let body = format!(
            r#"<m:ResolveNames ReturnFullContactData="false" SearchScope="ActiveDirectory">{}</m:ResolveNames>"#,
            el("m:UnresolvedEntry", entry)
        );
        let response = self.call(&Operation::read("ResolveNames", body)).await?;
        let messages = soap::response_messages(&response.body);
        let message = messages
            .first()
            .ok_or_else(|| EwsError::Protocol("ResolveNames without an answer".into()))?;
        // `ErrorNameResolutionMultipleResults` is a warning with results.
        if message.is_error() && message.code != "ErrorNameResolutionMultipleResults" {
            return Err(message.error());
        }
        let mailboxes: Vec<&Element> = message
            .element
            .child("ResolutionSet")
            .map(|set| {
                set.children_named("Resolution")
                    .filter_map(|r| r.child("Mailbox"))
                    .collect()
            })
            .unwrap_or_default();
        let best = mailboxes
            .iter()
            .find(|m| {
                m.child_text("EmailAddress")
                    .is_some_and(|a| a.eq_ignore_ascii_case(entry))
            })
            .or_else(|| mailboxes.first())
            .ok_or_else(|| EwsError::NotFound(format!("no mailbox for {entry}")))?;
        Ok((
            best.child_value("EmailAddress")
                .unwrap_or_else(|| entry.to_owned()),
            best.child_value("Name"),
        ))
    }

    /// `GetDelegate`: the delegates of the own mailbox.
    pub async fn get_delegates(&self) -> Result<Vec<Delegate>, EwsError> {
        let own = self
            .own_address()
            .ok_or_else(|| EwsError::Config("the account's address is not known".into()))?
            .to_owned();
        let body = format!(
            r#"<m:GetDelegate IncludePermissions="true"><m:Mailbox>{}</m:Mailbox></m:GetDelegate>"#,
            el("t:EmailAddress", &own)
        );
        let response = self.call(&Operation::read("GetDelegate", body)).await?;
        ResponseMessage::read(&response.body).check()?;
        Ok(soap::response_messages(&response.body)
            .into_iter()
            .filter(|m| !m.is_error())
            .filter_map(|m| {
                let user = m.element.child("DelegateUser")?;
                let id = user.child("UserId")?;
                Some(Delegate {
                    address: id.child_value("PrimarySmtpAddress")?,
                    name: id.child_value("DisplayName"),
                    permissions: user
                        .child("DelegatePermissions")
                        .map(|p| {
                            p.children
                                .iter()
                                .map(|level| {
                                    (
                                        level
                                            .name
                                            .trim_end_matches("FolderPermissionLevel")
                                            .to_owned(),
                                        level.text.trim().to_owned(),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    receives_meeting_messages: user
                        .child_bool("ReceiveCopiesOfMeetingMessages")
                        .unwrap_or(false),
                    view_private_items: user.child_bool("ViewPrivateItems").unwrap_or(false),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;
    use pretty_assertions::assert_eq;

    #[test]
    fn free_busy_views() {
        let answer = parse(concat!(
            r#"<FreeBusyResponse><ResponseMessage ResponseClass="Success"><ResponseCode>NoError</ResponseCode></ResponseMessage>"#,
            r#"<FreeBusyView><FreeBusyViewType>Detailed</FreeBusyViewType><CalendarEventArray>"#,
            r#"<CalendarEvent><StartTime>2026-10-05T09:00:00</StartTime><EndTime>2026-10-05T10:00:00</EndTime><BusyType>Busy</BusyType></CalendarEvent>"#,
            r#"<CalendarEvent><StartTime>2026-10-05T09:30:00</StartTime><EndTime>2026-10-05T11:00:00</EndTime><BusyType>Busy</BusyType></CalendarEvent>"#,
            r#"<CalendarEvent><StartTime>2026-10-05T12:00:00</StartTime><EndTime>2026-10-05T13:00:00</EndTime><BusyType>Free</BusyType></CalendarEvent>"#,
            r#"<CalendarEvent><StartTime>2026-10-06T00:00:00</StartTime><EndTime>2026-10-07T00:00:00</EndTime><BusyType>OOF</BusyType></CalendarEvent>"#,
            r#"</CalendarEventArray></FreeBusyView></FreeBusyResponse>"#
        ))
        .unwrap();
        let periods = parse_free_busy(&answer).unwrap();
        assert_eq!(
            periods,
            [
                BusyPeriod {
                    start: "2026-10-05T09:00:00Z".into(),
                    end: "2026-10-05T11:00:00Z".into(),
                    kind: BusyKind::Busy
                },
                BusyPeriod {
                    start: "2026-10-06T00:00:00Z".into(),
                    end: "2026-10-07T00:00:00Z".into(),
                    kind: BusyKind::Unavailable
                }
            ]
        );
        let failed = parse(
            r#"<FreeBusyResponse><ResponseMessage ResponseClass="Error"><MessageText>no</MessageText><ResponseCode>ErrorMailRecipientNotFound</ResponseCode></ResponseMessage></FreeBusyResponse>"#,
        )
        .unwrap();
        assert!(matches!(
            parse_free_busy(&failed),
            Err(EwsError::NotFound(_))
        ));
    }
}
