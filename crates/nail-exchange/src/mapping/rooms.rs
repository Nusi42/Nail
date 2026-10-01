//! Rooms in the free/busy picker (ADR 0072): the room finder lists an
//! account's rooms (optionally of one room list, with a minimum
//! capacity), their availability in the planned range comes from the
//! account's free/busy (`FreeBusySource::Exchange`), and a chosen room
//! joins the event as a room attendee (and location).

use nail_api::exchange::{Room, RoomAvailability};
use nail_api::pim::Attendee;
use nail_api::pim::calendar::{AttendeeKind, AttendeeRole};
use nail_api::scheduling::{AttendeeFreeBusy, BusyPeriod};

/// The rooms to show: of `room_list` (if given), with at least
/// `min_capacity` seats (rooms of unknown size are kept), sorted by name.
pub fn filter_rooms(
    rooms: &[Room],
    room_list: Option<&str>,
    min_capacity: Option<u32>,
) -> Vec<Room> {
    let mut out: Vec<Room> = rooms
        .iter()
        .filter(|r| {
            room_list.is_none_or(|l| {
                r.room_list
                    .as_deref()
                    .is_some_and(|x| x.eq_ignore_ascii_case(l))
            })
        })
        .filter(|r| match (min_capacity, r.capacity) {
            (Some(min), Some(seats)) => seats >= min,
            _ => true,
        })
        .cloned()
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.address.cmp(&b.address))
    });
    out.dedup_by(|a, b| a.address.eq_ignore_ascii_case(&b.address));
    out
}

/// Busy periods clipped to `[from, to)` (RFC 3339 UTC texts) and merged.
fn clip(periods: &[BusyPeriod], from: &str, to: &str) -> Vec<BusyPeriod> {
    BusyPeriod::merge(
        periods
            .iter()
            .filter(|p| p.end.as_str() > from && p.start.as_str() < to)
            .map(|p| BusyPeriod {
                start: if p.start.as_str() < from {
                    from.to_owned()
                } else {
                    p.start.clone()
                },
                end: if p.end.as_str() > to {
                    to.to_owned()
                } else {
                    p.end.clone()
                },
                kind: p.kind,
            })
            .collect(),
    )
}

/// Each room with its availability in `[from, to)` from the free/busy
/// answer (`free` is unknown when the room was not answered or failed).
/// Free rooms come first, then by name.
pub fn availability(
    rooms: &[Room],
    free_busy: &[AttendeeFreeBusy],
    from: &str,
    to: &str,
) -> Vec<RoomAvailability> {
    let mut out: Vec<RoomAvailability> = rooms
        .iter()
        .map(|room| {
            let answer = free_busy
                .iter()
                .find(|a| a.email.eq_ignore_ascii_case(&room.address));
            match answer {
                Some(a) if a.error.is_none() => {
                    let periods = clip(&a.periods, from, to);
                    RoomAvailability {
                        room: room.clone(),
                        free: Some(periods.is_empty()),
                        periods,
                        error: None,
                    }
                }
                Some(a) => RoomAvailability {
                    room: room.clone(),
                    free: None,
                    periods: Vec::new(),
                    error: a.error,
                },
                None => RoomAvailability {
                    room: room.clone(),
                    free: None,
                    periods: Vec::new(),
                    error: None,
                },
            }
        })
        .collect();
    let rank = |r: &RoomAvailability| match r.free {
        Some(true) => 0,
        None => 1,
        Some(false) => 2,
    };
    out.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.room.name.to_lowercase().cmp(&b.room.name.to_lowercase()))
    });
    out
}

/// The attendee that books a room (`CUTYPE=ROOM`, non-participant, answer
/// requested: the room's mailbox accepts or declines).
pub fn room_attendee(room: &Room) -> Attendee {
    Attendee {
        name: Some(room.name.clone()),
        role: AttendeeRole::NonParticipant,
        kind: AttendeeKind::Room,
        rsvp: true,
        ..Attendee::new(room.address.clone())
    }
}

/// Adds a room to an event's attendees (once) and returns the location
/// text to show when the event has none.
pub fn book_room(attendees: &mut Vec<Attendee>, room: &Room) -> String {
    if !attendees
        .iter()
        .any(|a| a.email.eq_ignore_ascii_case(&room.address))
    {
        attendees.push(room_attendee(room));
    }
    room.name.clone()
}

/// The rooms an event books (room attendees' addresses).
pub fn booked_rooms(attendees: &[Attendee]) -> Vec<&str> {
    attendees
        .iter()
        .filter(|a| a.kind == AttendeeKind::Room)
        .map(|a| a.email.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::ErrorCode;
    use nail_api::scheduling::{BusyKind, FreeBusySource};

    fn room(name: &str, address: &str, capacity: Option<u32>, list: Option<&str>) -> Room {
        Room {
            address: address.into(),
            name: name.into(),
            capacity,
            building: Some("HQ".into()),
            floor: None,
            accessible: None,
            features: Vec::new(),
            room_list: list.map(str::to_owned),
        }
    }

    fn rooms() -> Vec<Room> {
        vec![
            room("Oak", "oak@corp.example", Some(12), Some("hq@corp.example")),
            room(
                "birch",
                "birch@corp.example",
                Some(4),
                Some("hq@corp.example"),
            ),
            room(
                "Cedar",
                "cedar@corp.example",
                None,
                Some("annex@corp.example"),
            ),
        ]
    }

    #[test]
    fn filters_and_sorts() {
        let all = filter_rooms(&rooms(), None, None);
        assert_eq!(
            all.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["birch", "Cedar", "Oak"]
        );
        let big = filter_rooms(&rooms(), Some("HQ@corp.example"), Some(6));
        assert_eq!(big.len(), 1);
        assert_eq!(big[0].name, "Oak");
        let unknown_size = filter_rooms(&rooms(), None, Some(100));
        assert_eq!(unknown_size.len(), 1, "unknown capacity is kept");
    }

    fn answer(email: &str, periods: Vec<BusyPeriod>, error: Option<ErrorCode>) -> AttendeeFreeBusy {
        AttendeeFreeBusy {
            email: email.into(),
            source: FreeBusySource::Exchange {
                account_id: "work".into(),
            },
            periods,
            error,
            localized: None,
        }
    }

    #[test]
    fn availability_in_range() {
        let busy = |start: &str, end: &str| BusyPeriod {
            start: start.into(),
            end: end.into(),
            kind: BusyKind::Busy,
        };
        let free_busy = vec![
            answer(
                "OAK@corp.example",
                vec![
                    busy("2026-10-05T06:00:00Z", "2026-10-05T07:30:00Z"),
                    busy("2026-10-05T12:00:00Z", "2026-10-05T13:00:00Z"),
                ],
                None,
            ),
            answer(
                "birch@corp.example",
                vec![busy("2026-10-05T10:00:00Z", "2026-10-05T11:00:00Z")],
                None,
            ),
            answer("cedar@corp.example", Vec::new(), Some(ErrorCode::Forbidden)),
        ];
        let found = availability(
            &rooms(),
            &free_busy,
            "2026-10-05T07:00:00Z",
            "2026-10-05T09:00:00Z",
        );
        assert_eq!(found[0].room.name, "birch");
        assert_eq!(found[0].free, Some(true));
        assert_eq!(found[1].room.name, "Cedar");
        assert_eq!(
            (found[1].free, found[1].error),
            (None, Some(ErrorCode::Forbidden))
        );
        assert_eq!(found[2].room.name, "Oak");
        assert_eq!(found[2].free, Some(false));
        assert_eq!(
            found[2].periods,
            [busy("2026-10-05T07:00:00Z", "2026-10-05T07:30:00Z")]
        );
        let unanswered = availability(&rooms()[..1], &[], "a", "b");
        assert_eq!((unanswered[0].free, unanswered[0].error), (None, None));
    }

    #[test]
    fn booking() {
        let oak = &rooms()[0];
        let attendee = room_attendee(oak);
        assert_eq!(attendee.kind, AttendeeKind::Room);
        assert_eq!(attendee.role, AttendeeRole::NonParticipant);
        assert!(attendee.rsvp);
        let mut attendees = vec![Attendee::new("bob@corp.example")];
        assert_eq!(book_room(&mut attendees, oak), "Oak");
        book_room(&mut attendees, oak);
        assert_eq!(attendees.len(), 2);
        assert_eq!(booked_rooms(&attendees), ["oak@corp.example"]);
    }
}
