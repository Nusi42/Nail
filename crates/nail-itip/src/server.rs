//! CalDAV auto-scheduling (RFC 6638): when the user's server delivers
//! iTIP messages, so Nail must **not** send iMIP mail (duplicates).
//!
//! A calendar's source schedules when it advertises
//! `calendar-auto-schedule` in its `DAV:` capabilities (RFC 6638 §2) and
//! the calendar is writable (the server acts on what Nail writes). The
//! `SCHEDULE-AGENT` parameter then decides per calendar user (§7.1):
//!
//! * organizer → attendee messages (`REQUEST`, `CANCEL`, `ADD`,
//!   `DECLINECOUNTER`): the **attendee's** `SCHEDULE-AGENT` in the
//!   organizer's copy;
//! * attendee → organizer messages (`REPLY`, `COUNTER`, `REFRESH`): the
//!   **organizer's** `SCHEDULE-AGENT` in the attendee's copy.
//!
//! `SERVER` (or absent, or unknown) = the server delivers; Nail only
//! writes the event. `CLIENT` = Nail sends iMIP (the server has been
//! told to keep out). `NONE` = nobody sends. Without auto-scheduling
//! every message is iMIP. Messages that are not the result of writing
//! the event (`COUNTER`, `REFRESH`, `DECLINECOUNTER`, forwarding) have no
//! implicit-scheduling equivalent and always go as iMIP; `REPLY` is
//! implicit when the attendee writes their new `PARTSTAT`.
//!
//! When Nail sends iMIP itself while the calendar's server schedules (the
//! user forced client delivery), it marks the party `SCHEDULE-AGENT=CLIENT`
//! in the stored copy first ([`mark_client_scheduled`]), so the server does
//! not deliver a second copy.

use nail_api::scheduling::{ItipMethod, SchedulingTransport};
use nail_ical::content::{self, Component};

use crate::address::{normalize, same};
use crate::object::{ItipComponent, ItipObject, ScheduleAgent, parse_itip};

/// What the calendar a scheduling message is about can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CalendarScheduling {
    /// The source advertises `calendar-auto-schedule` (RFC 6638 §2).
    pub auto_schedule: bool,
    /// The calendar can be written.
    pub writable: bool,
}

impl CalendarScheduling {
    /// Whether the server schedules for this calendar.
    pub fn server_schedules(self) -> bool {
        self.auto_schedule && self.writable
    }
}

/// Whether a method is delivered by implicit scheduling when the event
/// is written (RFC 6638 §3.2).
pub fn is_implicit(method: ItipMethod) -> bool {
    matches!(
        method,
        ItipMethod::Request | ItipMethod::Cancel | ItipMethod::Add | ItipMethod::Reply
    )
}

/// Who delivers to each recipient.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryPlan {
    /// The server delivers (Nail writes the event only).
    pub server: Vec<String>,
    /// Nail sends iMIP mail.
    pub imip: Vec<String>,
    /// Nobody (`SCHEDULE-AGENT=NONE`).
    pub nobody: Vec<String>,
}

impl DeliveryPlan {
    /// The transport the card / result reports: `imip` when Nail mails
    /// anyone, else `server` when the server delivers, else `none`.
    pub fn transport(&self) -> SchedulingTransport {
        if !self.imip.is_empty() {
            SchedulingTransport::Imip
        } else if !self.server.is_empty() {
            SchedulingTransport::Server
        } else {
            SchedulingTransport::None
        }
    }
}

/// The main component of the event in a stored copy (`None` = another
/// event or unreadable: the defaults apply).
fn main_component(object: &ItipObject, uid: &str) -> Option<ItipComponent> {
    if object.uid.as_deref() != Some(uid) {
        return None;
    }
    object.main().cloned()
}

/// Plans delivery of an organizer message (`REQUEST`, `CANCEL`, `ADD`,
/// `DECLINECOUNTER`, `PUBLISH`) to `recipients`, from the organizer's
/// stored copy `raw`.
pub fn plan_organizer_delivery(
    method: ItipMethod,
    raw: &str,
    uid: &str,
    recipients: &[String],
    calendar: CalendarScheduling,
) -> DeliveryPlan {
    let mut plan = DeliveryPlan::default();
    let main = (calendar.server_schedules() && is_implicit(method))
        .then(|| main_component(&parse_itip(raw, Some("PUBLISH")), uid))
        .flatten();
    for r in recipients {
        let r = normalize(r);
        if r.is_empty()
            || plan.server.contains(&r)
            || plan.imip.contains(&r)
            || plan.nobody.contains(&r)
        {
            continue;
        }
        if !calendar.server_schedules() || !is_implicit(method) {
            plan.imip.push(r);
            continue;
        }
        let agent = main
            .as_ref()
            .and_then(|m| m.attendee(&r))
            .map_or(ScheduleAgent::Server, |a| a.schedule_agent);
        match agent {
            ScheduleAgent::Server => plan.server.push(r),
            ScheduleAgent::Client => plan.imip.push(r),
            ScheduleAgent::None => plan.nobody.push(r),
        }
    }
    plan
}

/// The transport of an attendee message (`REPLY`, `COUNTER`, `REFRESH`)
/// from the attendee's stored copy `raw` (`None` = not in a calendar:
/// always iMIP).
pub fn attendee_transport(
    method: ItipMethod,
    raw: Option<&str>,
    uid: &str,
    calendar: CalendarScheduling,
) -> SchedulingTransport {
    let Some(raw) = raw else {
        return SchedulingTransport::Imip;
    };
    if !calendar.server_schedules() || !is_implicit(method) {
        return SchedulingTransport::Imip;
    }
    let agent = main_component(&parse_itip(raw, Some("PUBLISH")), uid)
        .map_or(ScheduleAgent::Server, |m| m.organizer_agent);
    match agent {
        ScheduleAgent::Server => SchedulingTransport::Server,
        ScheduleAgent::Client => SchedulingTransport::Imip,
        ScheduleAgent::None => SchedulingTransport::None,
    }
}

/// Marks calendar users `SCHEDULE-AGENT=CLIENT` in a stored copy: the
/// organizer (`organizer: true`, attendee side) or the given attendees
/// (organizer side), in every component of the event. Unchanged lines
/// keep their text.
pub fn mark_client_scheduled(
    raw: &str,
    uid: &str,
    organizer: bool,
    attendees: &[String],
) -> String {
    fn visit(c: &mut Component, uid: &str, organizer: bool, attendees: &[String]) {
        let ours = c.is("VEVENT") && c.property("UID").is_some_and(|p| p.value.trim() == uid);
        if ours {
            for p in &mut c.properties {
                let hit = (organizer && p.is("ORGANIZER"))
                    || (p.is("ATTENDEE") && attendees.iter().any(|a| same(a, &p.value)));
                if hit && !p.has_param_value("SCHEDULE-AGENT", "CLIENT") {
                    p.set_param("SCHEDULE-AGENT", vec!["CLIENT".into()]);
                }
            }
        }
        for sub in &mut c.components {
            visit(sub, uid, organizer, attendees);
        }
    }

    let mut document = content::parse(raw);
    for c in &mut document.components {
        visit(c, uid, organizer, attendees);
    }
    document.to_text()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\nBEGIN:VEVENT\r\n\
UID:m1\r\nDTSTART:20261005T090000Z\r\nSUMMARY:Planning\r\n\
ORGANIZER;SCHEDULE-AGENT=CLIENT:mailto:ada@example.org\r\n\
ATTENDEE:mailto:bob@example.org\r\nATTENDEE;SCHEDULE-AGENT=CLIENT:mailto:carol@example.org\r\n\
ATTENDEE;SCHEDULE-AGENT=NONE:mailto:dan@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const SERVER: CalendarScheduling = CalendarScheduling {
        auto_schedule: true,
        writable: true,
    };

    fn people() -> Vec<String> {
        [
            "bob@example.org",
            "MAILTO:Carol@example.org",
            "dan@example.org",
            "bob@example.org",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn organizer_delivery_follows_schedule_agent() {
        let plan = plan_organizer_delivery(ItipMethod::Request, RAW, "m1", &people(), SERVER);
        assert_eq!(plan.server, ["bob@example.org"]);
        assert_eq!(plan.imip, ["carol@example.org"]);
        assert_eq!(plan.nobody, ["dan@example.org"]);
        assert_eq!(plan.transport(), SchedulingTransport::Imip);

        let only_server = plan_organizer_delivery(
            ItipMethod::Cancel,
            RAW,
            "m1",
            &["bob@example.org".into()],
            SERVER,
        );
        assert_eq!(only_server.transport(), SchedulingTransport::Server);
        let nobody = plan_organizer_delivery(
            ItipMethod::Request,
            RAW,
            "m1",
            &["dan@example.org".into()],
            SERVER,
        );
        assert_eq!(nobody.transport(), SchedulingTransport::None);
        assert_eq!(
            DeliveryPlan::default().transport(),
            SchedulingTransport::None
        );
    }

    #[test]
    fn without_auto_scheduling_everything_is_imip() {
        for calendar in [
            CalendarScheduling::default(),
            CalendarScheduling {
                auto_schedule: true,
                writable: false,
            },
            CalendarScheduling {
                auto_schedule: false,
                writable: true,
            },
        ] {
            let plan = plan_organizer_delivery(ItipMethod::Request, RAW, "m1", &people(), calendar);
            assert_eq!(plan.imip.len(), 3);
            assert!(plan.server.is_empty() && plan.nobody.is_empty());
        }
        // Methods without an implicit equivalent are always mailed.
        let plan =
            plan_organizer_delivery(ItipMethod::DeclineCounter, RAW, "m1", &people(), SERVER);
        assert_eq!(plan.imip.len(), 3);
    }

    #[test]
    fn attendee_side_uses_the_organizer_agent() {
        assert_eq!(
            attendee_transport(ItipMethod::Reply, Some(RAW), "m1", SERVER),
            SchedulingTransport::Imip
        );
        let server_raw = RAW.replace("ORGANIZER;SCHEDULE-AGENT=CLIENT:", "ORGANIZER:");
        assert_eq!(
            attendee_transport(ItipMethod::Reply, Some(&server_raw), "m1", SERVER),
            SchedulingTransport::Server
        );
        let none_raw = RAW.replace(
            "SCHEDULE-AGENT=CLIENT:mailto:ada",
            "SCHEDULE-AGENT=NONE:mailto:ada",
        );
        assert_eq!(
            attendee_transport(ItipMethod::Reply, Some(&none_raw), "m1", SERVER),
            SchedulingTransport::None
        );
        assert_eq!(
            attendee_transport(ItipMethod::Counter, Some(&server_raw), "m1", SERVER),
            SchedulingTransport::Imip
        );
        assert_eq!(
            attendee_transport(ItipMethod::Reply, None, "m1", SERVER),
            SchedulingTransport::Imip
        );
        assert_eq!(
            attendee_transport(
                ItipMethod::Reply,
                Some(&server_raw),
                "m1",
                CalendarScheduling::default()
            ),
            SchedulingTransport::Imip
        );
        // Another event's text: the default (server) applies.
        assert_eq!(
            attendee_transport(ItipMethod::Reply, Some(&server_raw), "other", SERVER),
            SchedulingTransport::Server
        );
    }

    #[test]
    fn marking_client_delivery_keeps_other_lines() {
        let raw = RAW.replace("ORGANIZER;SCHEDULE-AGENT=CLIENT:", "ORGANIZER:");
        let marked = mark_client_scheduled(&raw, "m1", true, &["bob@example.org".into()]);
        assert!(marked.contains("ORGANIZER;SCHEDULE-AGENT=CLIENT:mailto:ada@example.org"));
        assert!(marked.contains("ATTENDEE;SCHEDULE-AGENT=CLIENT:mailto:bob@example.org"));
        assert!(marked.contains("ATTENDEE;SCHEDULE-AGENT=NONE:mailto:dan@example.org"));
        assert!(marked.contains("SUMMARY:Planning\r\n"));
        assert_eq!(
            attendee_transport(ItipMethod::Reply, Some(&marked), "m1", SERVER),
            SchedulingTransport::Imip
        );
        // Another UID is left alone.
        assert_eq!(mark_client_scheduled(&raw, "other", true, &[]), raw);
    }
}
