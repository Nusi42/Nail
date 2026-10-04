//! The directory side: room lists and rooms (`places`, needs
//! `Place.Read.All`), access checks of other mailboxes (`/users/{upn}`),
//! free/busy of people and rooms (`calendar/getSchedule`) and meeting time
//! suggestions (`findMeetingTimes`).

use nail_api::AccountId;
use nail_api::ErrorCode;
use nail_api::exchange::{Room, RoomList};
use nail_api::scheduling::{AttendeeFreeBusy, BusyKind, BusyPeriod, FreeBusyQuery, FreeBusySource};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::batch::BatchRequest;
use crate::engine::GraphEngine;
use crate::error::GraphError;
use crate::wire::{Page, Place, ScheduleInformation, User};
use crate::{path_segment, text};

/// Addresses per `getSchedule` call.
pub const SCHEDULES_PER_CALL: usize = 20;

/// Pages of a place listing read at most (100 places each).
const MAX_PLACE_PAGES: usize = 50;

/// A meeting time Graph suggests (`findMeetingTimes`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingSuggestion {
    /// Start (RFC 3339 UTC).
    pub start: String,
    /// End (RFC 3339 UTC).
    pub end: String,
    /// Confidence in percent.
    pub confidence: Option<u32>,
    /// Graph's reason text (server language), if any.
    pub reason: Option<String>,
    /// Each attendee's availability (`free`, `tentative`, `busy`, …).
    pub attendees: Vec<(String, String)>,
    /// Rooms suggested for it (addresses).
    pub rooms: Vec<String>,
}

/// A request for meeting suggestions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingTimesQuery {
    /// Required attendees.
    pub attendees: Vec<String>,
    /// Rooms (resources) to consider.
    pub rooms: Vec<String>,
    /// Window start (RFC 3339 UTC).
    pub from: String,
    /// Window end (RFC 3339 UTC).
    pub to: String,
    /// Meeting length in minutes.
    pub duration_minutes: u32,
    /// Suggestions wanted at most.
    pub max_candidates: u32,
}

fn room_of(place: Place, room_list: Option<&str>) -> Option<Room> {
    let address = place.email_address.filter(|a| !a.is_empty())?;
    let mut features: Vec<String> = [
        place.audio_device_name,
        place.video_device_name,
        place.display_device_name,
    ]
    .into_iter()
    .flatten()
    .filter(|d| !d.trim().is_empty())
    .collect();
    features.extend(place.tags.into_iter().filter(|t| !t.trim().is_empty()));
    Some(Room {
        name: place.display_name.unwrap_or_else(|| address.clone()),
        address: address.to_ascii_lowercase(),
        capacity: place.capacity,
        building: place.building.filter(|b| !b.is_empty()),
        floor: place
            .floor_label
            .filter(|f| !f.is_empty())
            .or_else(|| place.floor_number.map(|n| n.to_string())),
        accessible: place.is_wheel_chair_accessible,
        features,
        room_list: room_list.map(str::to_ascii_lowercase),
    })
}

/// The busy kind of a `scheduleItem` status (`None` = free).
pub fn busy_kind(status: &str) -> Option<BusyKind> {
    match status {
        "busy" => Some(BusyKind::Busy),
        "tentative" => Some(BusyKind::Tentative),
        "oof" => Some(BusyKind::Unavailable),
        _ => None,
    }
}

impl GraphEngine {
    async fn places(&self, path: &str) -> Result<Vec<Place>, GraphError> {
        let mut out = Vec::new();
        let mut page: Page<Place> = self
            .client
            .get_json(path, &[("$top", "100".to_owned())], &[], "GET places")
            .await?;
        for _ in 0..MAX_PLACE_PAGES {
            out.append(&mut page.value);
            match page.next_link.take() {
                Some(next) => {
                    page = self.client.get_link_json(&next, &[], "GET places").await?;
                }
                None => break,
            }
        }
        Ok(out)
    }

    /// The organization's room lists (`forbidden` without
    /// `Place.Read.All`).
    pub async fn list_room_lists(&self) -> Result<Vec<RoomList>, GraphError> {
        let mut lists: Vec<RoomList> = self
            .places("/places/microsoft.graph.roomlist")
            .await?
            .into_iter()
            .filter_map(|p| {
                let address = p.email_address.filter(|a| !a.is_empty())?;
                Some(RoomList {
                    name: p.display_name.unwrap_or_else(|| address.clone()),
                    address: address.to_ascii_lowercase(),
                })
            })
            .collect();
        lists.sort_by_key(|l| l.name.to_lowercase());
        Ok(lists)
    }

    /// The rooms of a room list, or every room.
    pub async fn list_rooms(&self, room_list: Option<&str>) -> Result<Vec<Room>, GraphError> {
        let path = match room_list {
            Some(list) => format!(
                "/places/{}/microsoft.graph.roomlist/rooms",
                path_segment(list)
            ),
            None => "/places/microsoft.graph.room".to_owned(),
        };
        let mut rooms: Vec<Room> = self
            .places(&path)
            .await?
            .into_iter()
            .filter_map(|p| room_of(p, room_list))
            .collect();
        rooms.sort_by_key(|r| r.name.to_lowercase());
        Ok(rooms)
    }

    /// Checks that the user can open another mailbox; answers its display
    /// name (`Forbidden` = no access, `NotFound` / `MailboxUnavailable` =
    /// no such mailbox here).
    pub async fn probe_mailbox(&self, address: &str) -> Result<Option<String>, GraphError> {
        let user = format!("/users/{}", path_segment(address));
        let answers = self
            .client
            .batch(&[
                BatchRequest::get(format!("{user}?$select=displayName,mail")),
                BatchRequest::get(format!("{user}/mailFolders/inbox?$select=id")),
            ])
            .await?;
        if let Some(error) = answers[1].error("GET /users/{mailbox}/mailFolders/inbox") {
            return Err(error);
        }
        // Reading the user needs `User.ReadBasic.All`; without it the name
        // stays unknown.
        Ok(answers[0]
            .json::<User>("GET /users/{mailbox}")
            .ok()
            .and_then(|u| u.display_name))
    }

    /// Free/busy of people and rooms (`getSchedule`), one entry per
    /// attendee in query order.
    pub async fn schedules(
        &self,
        query: &FreeBusyQuery,
    ) -> Result<Vec<AttendeeFreeBusy>, GraphError> {
        let mut out = Vec::with_capacity(query.attendees.len());
        let account = self.account_id();
        for chunk in query.attendees.chunks(SCHEDULES_PER_CALL) {
            let answer = self
                .client
                .send_json(
                    Method::POST,
                    "/me/calendar/getSchedule",
                    Some(&json!({
                        "schedules": chunk,
                        "startTime": {"dateTime": text::trim_fraction(&query.from), "timeZone": "UTC"},
                        "endTime": {"dateTime": text::trim_fraction(&query.to), "timeZone": "UTC"},
                        "availabilityViewInterval": 15,
                    })),
                    &[("Prefer", "outlook.timezone=\"UTC\"".to_owned())],
                    // A read, despite the POST.
                    true,
                    "POST calendar/getSchedule",
                )
                .await?;
            let page: Page<ScheduleInformation> = serde_json::from_value(answer)
                .map_err(|e| GraphError::Protocol(format!("getSchedule: {e}")))?;
            for attendee in chunk {
                let info = page
                    .value
                    .iter()
                    .find(|s| s.schedule_id.eq_ignore_ascii_case(attendee));
                out.push(attendee_free_busy(attendee, info, &account));
            }
        }
        Ok(out)
    }

    /// Meeting times that suit the attendees (and a room), best first.
    pub async fn find_meeting_times(
        &self,
        query: &MeetingTimesQuery,
    ) -> Result<Vec<MeetingSuggestion>, GraphError> {
        let mut attendees: Vec<Value> = query
            .attendees
            .iter()
            .map(|a| json!({"type": "required", "emailAddress": {"address": a}}))
            .collect();
        attendees.extend(
            query
                .rooms
                .iter()
                .map(|r| json!({"type": "resource", "emailAddress": {"address": r}})),
        );
        let mut body = json!({
            "attendees": attendees,
            "timeConstraint": {
                "activityDomain": "work",
                "timeSlots": [{
                    "start": {"dateTime": text::trim_fraction(&query.from), "timeZone": "UTC"},
                    "end": {"dateTime": text::trim_fraction(&query.to), "timeZone": "UTC"},
                }],
            },
            "meetingDuration": format!("PT{}M", query.duration_minutes.max(1)),
            "maxCandidates": query.max_candidates.max(1),
            "returnSuggestionReasons": true,
            "isOrganizerOptional": false,
        });
        if !query.rooms.is_empty() {
            body["locationConstraint"] = json!({
                "isRequired": false,
                "suggestLocation": false,
                "locations": query.rooms.iter().map(|r| json!({"locationEmailAddress": r, "resolveAvailability": true, "displayName": r})).collect::<Vec<_>>(),
            });
        }
        let answer = self
            .client
            .send_json(
                Method::POST,
                "/me/findMeetingTimes",
                Some(&body),
                &[("Prefer", "outlook.timezone=\"UTC\"".to_owned())],
                true,
                "POST findMeetingTimes",
            )
            .await?;
        let result: WireSuggestions = serde_json::from_value(answer)
            .map_err(|e| GraphError::Protocol(format!("findMeetingTimes: {e}")))?;
        Ok(result
            .meeting_time_suggestions
            .iter()
            .filter_map(suggestion)
            .collect())
    }
}

fn attendee_free_busy(
    attendee: &str,
    info: Option<&ScheduleInformation>,
    account: &AccountId,
) -> AttendeeFreeBusy {
    let source = FreeBusySource::Exchange {
        account_id: account.clone(),
    };
    let failed = |code: ErrorCode, message: &str| {
        let error = if code == ErrorCode::NotFound {
            GraphError::NotFound(message.to_owned())
        } else {
            GraphError::Unavailable {
                status: 503,
                retry_after: None,
            }
        };
        AttendeeFreeBusy {
            email: attendee.to_owned(),
            source: source.clone(),
            periods: Vec::new(),
            error: Some(code),
            localized: Some(Box::new(error.localized())),
        }
    };
    let Some(info) = info else {
        return failed(ErrorCode::NotFound, attendee);
    };
    if let Some(error) = &info.error {
        let message = error.message.clone().unwrap_or_default();
        let not_found = error.response_code.as_deref().is_some_and(|c| {
            c.contains("NotFound") || c.contains("InvalidSmtpAddress") || c.contains("Unresolved")
        });
        return failed(
            if not_found {
                ErrorCode::NotFound
            } else {
                ErrorCode::Unavailable
            },
            &message,
        );
    }
    let periods = info
        .schedule_items
        .iter()
        .filter_map(|item| {
            Some(BusyPeriod {
                kind: busy_kind(&item.status)?,
                start: text::to_utc(&item.start.date_time)?,
                end: text::to_utc(&item.end.date_time)?,
            })
        })
        .collect();
    AttendeeFreeBusy {
        email: attendee.to_owned(),
        source,
        periods: BusyPeriod::merge(periods),
        error: None,
        localized: None,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSuggestions {
    #[serde(default)]
    meeting_time_suggestions: Vec<Value>,
}

fn suggestion(value: &Value) -> Option<MeetingSuggestion> {
    let slot = value.get("meetingTimeSlot")?;
    let start = text::to_utc(slot.pointer("/start/dateTime")?.as_str()?)?;
    let end = text::to_utc(slot.pointer("/end/dateTime")?.as_str()?)?;
    let confidence = value
        .get("confidence")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite() && *c >= 0.0)
        .map(|c| {
            // At most 100 after the clamp, so the cast cannot truncate.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0..=100 above, so the cast is exact"
            )]
            let percent = c.min(100.0).round() as u32;
            percent
        });
    let attendees = value
        .get("attendeeAvailability")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x.pointer("/attendee/emailAddress/address")?
                            .as_str()?
                            .to_ascii_lowercase(),
                        x.get("availability")?.as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let rooms = value
        .get("locations")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|l| l.get("locationEmailAddress")?.as_str())
                .filter(|a| !a.is_empty())
                .map(str::to_ascii_lowercase)
                .collect()
        })
        .unwrap_or_default();
    Some(MeetingSuggestion {
        start,
        end,
        confidence,
        reason: value
            .get("suggestionReason")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        attendees,
        rooms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn recorded_schedule_maps_to_busy_periods() {
        let page: Page<ScheduleInformation> =
            serde_json::from_str(include_str!("../fixtures/responses/get-schedule.json")).unwrap();
        let account = AccountId::new("work");
        let bob = attendee_free_busy(
            "BOB@contoso.example",
            page.value
                .iter()
                .find(|s| s.schedule_id.eq_ignore_ascii_case("bob@contoso.example")),
            &account,
        );
        assert_eq!(bob.error, None);
        assert_eq!(
            bob.periods,
            [
                BusyPeriod {
                    start: "2026-10-05T08:00:00Z".into(),
                    end: "2026-10-05T09:30:00Z".into(),
                    kind: BusyKind::Busy
                },
                BusyPeriod {
                    start: "2026-10-05T13:00:00Z".into(),
                    end: "2026-10-05T14:00:00Z".into(),
                    kind: BusyKind::Tentative
                },
            ]
        );
        assert_eq!(bob.email, "BOB@contoso.example");
        assert_eq!(
            bob.source,
            FreeBusySource::Exchange {
                account_id: account.clone()
            }
        );
        let unknown = attendee_free_busy(
            "nobody@contoso.example",
            page.value
                .iter()
                .find(|s| s.schedule_id == "nobody@contoso.example"),
            &account,
        );
        assert_eq!(unknown.error, Some(ErrorCode::NotFound));
        assert!(unknown.localized.is_some());
        let missing = attendee_free_busy("x@y.example", None, &account);
        assert_eq!(missing.error, Some(ErrorCode::NotFound));
        assert_eq!(busy_kind("workingElsewhere"), None);
        assert_eq!(busy_kind("oof"), Some(BusyKind::Unavailable));
    }

    #[test]
    fn places_map_to_rooms() {
        let place: Place = serde_json::from_value(json!({
            "id": "p1", "displayName": "Conf Room Adams", "emailAddress": "Adams@contoso.example",
            "capacity": 12, "building": "1", "floorNumber": 2, "floorLabel": null,
            "isWheelChairAccessible": true, "audioDeviceName": null, "videoDeviceName": "Surface Hub",
            "displayDeviceName": "", "tags": ["whiteboard"]
        }))
        .unwrap();
        let room = room_of(place, Some("Building1@contoso.example")).unwrap();
        assert_eq!(room.address, "adams@contoso.example");
        assert_eq!(room.floor.as_deref(), Some("2"));
        assert_eq!(room.features, ["Surface Hub", "whiteboard"]);
        assert_eq!(room.room_list.as_deref(), Some("building1@contoso.example"));
        assert_eq!(room.accessible, Some(true));
        assert!(room_of(Place::default(), None).is_none());
    }

    #[test]
    fn suggestions_parse() {
        let s = suggestion(&json!({
            "confidence": 87.5,
            "suggestionReason": "Suggested because it is one of the nearest times when all attendees are available.",
            "meetingTimeSlot": {
                "start": {"dateTime": "2026-10-05T10:00:00.0000000", "timeZone": "UTC"},
                "end": {"dateTime": "2026-10-05T10:30:00.0000000", "timeZone": "UTC"}
            },
            "attendeeAvailability": [
                {"attendee": {"type": "required", "emailAddress": {"address": "Bob@contoso.example"}}, "availability": "free"}
            ],
            "locations": [{"displayName": "Adams", "locationEmailAddress": "adams@contoso.example"}]
        }))
        .unwrap();
        assert_eq!(s.start, "2026-10-05T10:00:00Z");
        assert_eq!(s.confidence, Some(88));
        assert_eq!(
            s.attendees,
            [("bob@contoso.example".to_owned(), "free".to_owned())]
        );
        assert_eq!(s.rooms, ["adams@contoso.example"]);
        assert!(suggestion(&json!({})).is_none());
    }
}
