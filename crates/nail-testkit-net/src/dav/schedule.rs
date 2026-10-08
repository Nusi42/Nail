//! The mock server's RFC 6638 side: free/busy `POST`s to the schedule
//! outbox and auto-scheduling on `PUT` / `DELETE` (see the module docs of
//! `dav`).

use std::fmt::Write as _;

use super::ical::{self, Line};
use super::{
    CALENDARS_PATH, Data, DavRequest, Inner, MockCollectionKind, MockScheduleMessage, NS_CALDAV,
    NS_DAV, Resource, calendar_home_of, dav_error, empty, escape, inbox_of, lock, resolve, respond,
};
use axum::response::Response;

/// Most recipients one free/busy `POST` may name.
const MAX_RECIPIENTS: usize = 100;

/// A busy stretch: start, end (Unix seconds) and `FBTYPE`.
type Busy = (i64, i64, &'static str);

pub(super) fn post(inner: &Inner, request: &DavRequest, me: &str) -> Response {
    let data = lock(&inner.data);
    let path = match resolve(inner, &data, &request.path) {
        Some(Resource::Collection(path))
            if data.collections[&path].kind == MockCollectionKind::ScheduleOutbox =>
        {
            path
        }
        Some(_) => return empty(405),
        None => return empty(404),
    };
    if data.owner_of(&path).as_deref() != Some(me) {
        return dav_error(403, NS_DAV, "need-privileges");
    }
    let is_calendar = request
        .content_type
        .as_deref()
        .is_some_and(|ct| ct.to_ascii_lowercase().starts_with("text/calendar"));
    if !is_calendar {
        return dav_error(415, NS_CALDAV, "supported-calendar-data");
    }
    let Some(question) = Question::parse(&request.body) else {
        return dav_error(400, NS_CALDAV, "valid-scheduling-message");
    };
    let Some(user) = data.users.get(me) else {
        return empty(403);
    };
    if !ical::address(&question.organizer).eq_ignore_ascii_case(&user.email) {
        return dav_error(403, NS_CALDAV, "organizer-allowed");
    }
    if question.attendees.len() > MAX_RECIPIENTS {
        return dav_error(403, NS_CALDAV, "recipient-limit");
    }
    let mut body = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<c:schedule-response xmlns:d=\"DAV:\" \
         xmlns:c=\"{NS_CALDAV}\">"
    );
    for attendee in &question.attendees {
        let _ = write!(
            body,
            "<c:response><c:recipient><d:href>{}</d:href></c:recipient>",
            escape(attendee)
        );
        match data.user_by_email(attendee) {
            None => body.push_str("<c:request-status>3.7;Invalid calendar user</c:request-status>"),
            Some(target) if data.hidden_free_busy.contains(&target.username) => {
                body.push_str("<c:request-status>3.8;No authority</c:request-status>");
            }
            Some(target) => {
                let busy = busy_of(
                    &data,
                    &target.username,
                    &target.email,
                    question.start,
                    question.end,
                );
                let reply = reply_text(&question, attendee, &busy);
                let _ = write!(
                    body,
                    "<c:request-status>2.0;Success</c:request-status>\
                     <c:calendar-data>{}</c:calendar-data>",
                    escape(&reply)
                );
            }
        }
        body.push_str("</c:response>");
    }
    body.push_str("</c:schedule-response>");
    respond(200, Some("application/xml; charset=utf-8"), body)
}

/// A `VFREEBUSY` request.
struct Question {
    uid: String,
    organizer: String,
    attendees: Vec<String>,
    start: i64,
    end: i64,
}

impl Question {
    fn parse(text: &str) -> Option<Self> {
        let mut method = None;
        let mut inside = false;
        let mut uid = String::new();
        let mut organizer = None;
        let mut attendees = Vec::new();
        let (mut start, mut end) = (None, None);
        for raw in ical::unfold(text) {
            let line = ical::parse_line(&raw)?;
            let value = line.value.trim();
            match line.name.as_str() {
                "METHOD" => method = Some(value.to_ascii_uppercase()),
                "BEGIN" if value.eq_ignore_ascii_case("VFREEBUSY") => inside = true,
                "END" if value.eq_ignore_ascii_case("VFREEBUSY") => inside = false,
                "UID" if inside => value.clone_into(&mut uid),
                "ORGANIZER" if inside => organizer = Some(value.to_owned()),
                "ATTENDEE" if inside => attendees.push(value.to_owned()),
                "DTSTART" if inside => start = ical::seconds(value),
                "DTEND" if inside => end = ical::seconds(value),
                _ => {}
            }
        }
        let (start, end) = (start?, end?);
        (method.as_deref() == Some("REQUEST") && end > start && !attendees.is_empty()).then(|| {
            Self {
                uid,
                organizer: organizer.unwrap_or_default(),
                attendees,
                start,
                end,
            }
        })
    }
}

/// The busy periods of `user` (address `email`) in `start..end`.
fn busy_of(data: &Data, user: &str, email: &str, start: i64, end: i64) -> Vec<Busy> {
    let home = calendar_home_of(user);
    let mut out: Vec<Busy> = Vec::new();
    for (path, coll) in &data.collections {
        if !path.starts_with(&home) || !matches!(coll.kind, MockCollectionKind::Calendar(_)) {
            continue;
        }
        for object in coll.objects.values() {
            for event in ical::events(&object.data) {
                let (Some(s), Some(e)) = (event.start, event.end) else {
                    continue;
                };
                if event.transparent || event.status == "CANCELLED" {
                    continue;
                }
                let mine = event
                    .attendees
                    .iter()
                    .find(|a| a.email.eq_ignore_ascii_case(email))
                    .map(|a| a.partstat.as_str());
                let kind = match mine {
                    Some("DECLINED" | "DELEGATED") => continue,
                    Some("NEEDS-ACTION" | "TENTATIVE") => "BUSY-TENTATIVE",
                    _ if event.status == "TENTATIVE" => "BUSY-TENTATIVE",
                    _ => "BUSY",
                };
                let (s, e) = (s.max(start), e.min(end));
                if e > s {
                    out.push((s, e, kind));
                }
            }
        }
    }
    out.sort_unstable();
    out
}

fn reply_text(question: &Question, attendee: &str, busy: &[Busy]) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        "PRODID:-//Nail//Mock DAV//EN".to_owned(),
        "METHOD:REPLY".to_owned(),
        "BEGIN:VFREEBUSY".to_owned(),
        format!("UID:{}", question.uid),
        format!("DTSTAMP:{}", ical::stamp(question.start)),
        format!("DTSTART:{}", ical::stamp(question.start)),
        format!("DTEND:{}", ical::stamp(question.end)),
        format!("ORGANIZER:{}", question.organizer),
        format!("ATTENDEE:{attendee}"),
    ];
    for (s, e, kind) in busy {
        lines.push(format!(
            "FREEBUSY;FBTYPE={kind}:{}/{}",
            ical::stamp(*s),
            ical::stamp(*e)
        ));
    }
    lines.push("END:VFREEBUSY".to_owned());
    lines.push("END:VCALENDAR".to_owned());
    lines.join("\r\n") + "\r\n"
}

// ---------------------------------------------------------------------------
// Auto-scheduling

/// A file name for `uid` (letters, digits, `-`, `_`, `.`).
fn object_name(uid: &str) -> String {
    let name: String = uid
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{name}.ics")
}

/// Delivers an iTIP message: into the inbox of a local user (returns
/// `1.2`), else recorded as mailed (`1.1`).
fn deliver(
    data: &mut Data,
    from: &str,
    to: &str,
    method: &str,
    uid: &str,
    text: &str,
) -> &'static str {
    let local = data.user_by_email(to).map(|u| u.username.clone());
    if let Some(user) = &local {
        let n = data.next();
        let name = format!("{}-{n}.ics", object_name(uid).trim_end_matches(".ics"));
        data.put(&inbox_of(user), &name, text);
    }
    data.scheduled.push(MockScheduleMessage {
        method: method.to_owned(),
        from: from.to_owned(),
        to: to.to_owned(),
        uid: uid.to_owned(),
        local: local.is_some(),
        data: text.to_owned(),
    });
    if local.is_some() { "1.2" } else { "1.1" }
}

/// Finds `user`'s copy of the event `uid` in their calendars.
fn find_copy(data: &Data, user: &str, uid: &str) -> Option<(String, String, String)> {
    let home = calendar_home_of(user);
    data.collections
        .iter()
        .filter(|(p, c)| p.starts_with(&home) && matches!(c.kind, MockCollectionKind::Calendar(_)))
        .find_map(|(path, coll)| {
            coll.objects.iter().find_map(|(name, object)| {
                ical::events(&object.data)
                    .iter()
                    .any(|e| e.uid == uid)
                    .then(|| (path.clone(), name.clone(), object.data.clone()))
            })
        })
}

/// Sets the `SCHEDULE-STATUS` (or `PARTSTAT`) parameter on the attendee /
/// organizer lines of `text` whose address is in `values`.
fn set_params(text: &str, prop: &str, param: &str, values: &[(String, String)]) -> String {
    ical::rewrite(text, |component, line| {
        if component == "VCALENDAR" || line.name != prop {
            return None;
        }
        let address = ical::address(&line.value);
        let (_, value) = values.iter().find(|(a, _)| *a == address)?;
        let mut new = line.clone();
        new.set_param(param, value);
        Some(Some(new))
    })
}

/// A `REPLY` from `me`: the object with only `me` as attendee.
fn reply_from(text: &str, me: &str) -> String {
    let itip = ical::as_itip(text, "REPLY");
    ical::rewrite(&itip, |_, line| {
        (line.name == "ATTENDEE" && ical::address(&line.value) != me).then_some(None)
    })
}

/// After a `PUT` to a calendar: schedules as the module docs say. Returns
/// whether the stored object was rewritten (then no `ETag` is sent).
pub(super) fn after_put(
    data: &mut Data,
    (parent, name): (&str, &str),
    me: &str,
    previous: Option<&str>,
    request: &DavRequest,
) -> bool {
    if !parent.starts_with(CALENDARS_PATH) {
        return false;
    }
    let Some(my_email) = data.users.get(me).map(|u| u.email.to_ascii_lowercase()) else {
        return false;
    };
    let stored = request.body.clone();
    let events = ical::events(&stored);
    let Some(master) = events.iter().find(|e| e.organizer.is_some()) else {
        return false;
    };
    let uid = master.uid.clone();
    let organizer = master.organizer.clone().unwrap_or_default();
    if organizer == my_email {
        let mut statuses: Vec<(String, String)> = Vec::new();
        let request_text = ical::as_itip(&stored, "REQUEST");
        for attendee in server_attendees(&events, &my_email) {
            let status = deliver(
                data,
                &my_email,
                &attendee.email,
                "REQUEST",
                &uid,
                &request_text,
            );
            statuses.push((attendee.email.clone(), status.to_owned()));
            place_invitation(data, &attendee.email, &uid, &stored);
        }
        // Attendees taken off the meeting get a CANCEL.
        let mut seen: Vec<String> = Vec::new();
        if let Some(previous) = previous {
            let cancel = cancel_text(previous);
            for event in ical::events(previous) {
                for gone in event.attendees {
                    if gone.email != my_email
                        && gone.server
                        && !seen.contains(&gone.email)
                        && !events
                            .iter()
                            .any(|e| e.attendees.iter().any(|a| a.email == gone.email))
                    {
                        seen.push(gone.email.clone());
                        deliver(data, &my_email, &gone.email, "CANCEL", &uid, &cancel);
                        cancel_copy(data, &gone.email, &uid);
                    }
                }
            }
        }
        if statuses.is_empty() {
            return false;
        }
        let rewritten = set_params(&stored, "ATTENDEE", "SCHEDULE-STATUS", &statuses);
        data.put(parent, name, &rewritten);
        return true;
    }
    // The user answers as an attendee.
    let Some(mine) = events
        .iter()
        .flat_map(|e| e.attendees.iter())
        .find(|a| a.email == my_email)
    else {
        return false;
    };
    let before = previous.and_then(|p| {
        ical::events(p)
            .into_iter()
            .flat_map(|e| e.attendees)
            .find(|a| a.email == my_email)
            .map(|a| a.partstat)
    });
    let suppressed = request
        .schedule_reply
        .as_deref()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("F"));
    if !master.organizer_server || suppressed || before.as_deref() == Some(mine.partstat.as_str()) {
        return false;
    }
    if before.is_none() && mine.partstat == "NEEDS-ACTION" {
        return false;
    }
    let partstat = mine.partstat.clone();
    let status = send_reply(
        data,
        &my_email,
        &organizer,
        &uid,
        &reply_from(&stored, &my_email),
        &partstat,
    );
    let rewritten = set_params(
        &stored,
        "ORGANIZER",
        "SCHEDULE-STATUS",
        &[(organizer, status.to_owned())],
    );
    data.put(parent, name, &rewritten);
    true
}

/// Delivers a `REPLY` and updates a local organizer's copy.
fn send_reply(
    data: &mut Data,
    me: &str,
    organizer: &str,
    uid: &str,
    text: &str,
    partstat: &str,
) -> &'static str {
    let status = deliver(data, me, organizer, "REPLY", uid, text);
    if let Some(user) = data.user_by_email(organizer).map(|u| u.username.clone())
        && let Some((path, name, copy)) = find_copy(data, &user, uid)
    {
        let updated = set_params(
            &copy,
            "ATTENDEE",
            "PARTSTAT",
            &[(me.to_owned(), partstat.to_owned())],
        );
        data.put(&path, &name, &updated);
    }
    status
}

/// Puts (or updates) a local attendee's copy of an invitation in their
/// default calendar, keeping their own answer.
fn place_invitation(data: &mut Data, email: &str, uid: &str, organizer_copy: &str) {
    let Some(user) = data.user_by_email(email).map(|u| u.username.clone()) else {
        return;
    };
    let clean = ical::rewrite(organizer_copy, |_, line| {
        if !matches!(line.name.as_str(), "ATTENDEE" | "ORGANIZER") {
            return None;
        }
        let mut new: Line = line.clone();
        new.remove_param("SCHEDULE-STATUS");
        Some(Some(new))
    });
    match find_copy(data, &user, uid) {
        Some((path, name, existing)) => {
            let answer = ical::events(&existing)
                .into_iter()
                .flat_map(|e| e.attendees)
                .find(|a| a.email == email)
                .map(|a| a.partstat);
            let text = match answer {
                Some(partstat) => set_params(
                    &clean,
                    "ATTENDEE",
                    "PARTSTAT",
                    &[(email.to_owned(), partstat)],
                ),
                None => clean,
            };
            data.put(&path, &name, &text);
        }
        None => {
            if let Some(calendar) = data.default_calendar(&user) {
                data.put(&calendar, &object_name(uid), &clean);
            }
        }
    }
}

/// A `CANCEL` of the whole event.
fn cancel_text(text: &str) -> String {
    let itip = ical::as_itip(text, "CANCEL");
    with_status_cancelled(&itip)
}

fn with_status_cancelled(text: &str) -> String {
    ical::rewrite(text, |component, line| {
        if component != "VEVENT" {
            return None;
        }
        match line.name.as_str() {
            "STATUS" => Some(None),
            "UID" => Some(Some(Line::new(
                "UID",
                &format!("{}\r\nSTATUS:CANCELLED", line.value.trim()),
            ))),
            _ => None,
        }
    })
}

/// Marks a local attendee's copy as cancelled.
fn cancel_copy(data: &mut Data, email: &str, uid: &str) {
    if let Some(user) = data.user_by_email(email).map(|u| u.username.clone())
        && let Some((path, name, copy)) = find_copy(data, &user, uid)
    {
        data.put(&path, &name, &with_status_cancelled(&copy));
    }
}

/// After a `DELETE` of a calendar object: an organizer cancels, an
/// attendee declines.
pub(super) fn after_delete(data: &mut Data, me: &str, previous: &str, request: &DavRequest) {
    let Some(my_email) = data.users.get(me).map(|u| u.email.to_ascii_lowercase()) else {
        return;
    };
    let events = ical::events(previous);
    let Some(master) = events.iter().find(|e| e.organizer.is_some()) else {
        return;
    };
    let uid = master.uid.clone();
    let organizer = master.organizer.clone().unwrap_or_default();
    if organizer == my_email {
        let cancel = cancel_text(previous);
        for attendee in server_attendees(&events, &my_email) {
            deliver(data, &my_email, &attendee.email, "CANCEL", &uid, &cancel);
            cancel_copy(data, &attendee.email, &uid);
        }
        return;
    }
    let suppressed = request
        .schedule_reply
        .as_deref()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("F"));
    let attending = events
        .iter()
        .flat_map(|e| e.attendees.iter())
        .any(|a| a.email == my_email && a.partstat != "DECLINED");
    if !master.organizer_server || suppressed || !attending {
        return;
    }
    let reply = set_params(
        &reply_from(previous, &my_email),
        "ATTENDEE",
        "PARTSTAT",
        &[(my_email.clone(), "DECLINED".to_owned())],
    );
    send_reply(data, &my_email, &organizer, &uid, &reply, "DECLINED");
}

/// The attendees the server schedules for `me` (the organizer): those
/// with `SCHEDULE-AGENT=SERVER`, once each, without `me`.
fn server_attendees<'a>(events: &'a [ical::Event], me: &str) -> Vec<&'a ical::Attendee> {
    let mut out: Vec<&ical::Attendee> = Vec::new();
    for attendee in events.iter().flat_map(|e| e.attendees.iter()) {
        if attendee.email != me && attendee.server && !out.iter().any(|a| a.email == attendee.email)
        {
            out.push(attendee);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn questions_and_replies() {
        let text = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VFREEBUSY\r\nUID:q\r\nDTSTART:20261001T000000Z\r\n\
                    DTEND:20261002T000000Z\r\nORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\n\
                    END:VFREEBUSY\r\nEND:VCALENDAR\r\n";
        let q = Question::parse(text).unwrap();
        assert_eq!(q.attendees, ["mailto:bob@example.org"]);
        assert_eq!(q.end - q.start, 86_400);
        let reply = reply_text(
            &q,
            "mailto:bob@example.org",
            &[(q.start, q.start + 60, "BUSY")],
        );
        assert!(reply.contains("FREEBUSY;FBTYPE=BUSY:20261001T000000Z/20261001T000100Z\r\n"));
        assert!(Question::parse(&text.replace("METHOD:REQUEST", "METHOD:REPLY")).is_none());
        assert!(Question::parse(&text.replace("DTEND:20261002", "DTEND:20250101")).is_none());
        assert_eq!(object_name("a/b c@d"), "a_b_c_d.ics");
    }

    #[test]
    fn cancel_and_reply_texts() {
        let event = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:m\r\nSTATUS:CONFIRMED\r\nORGANIZER:mailto:ada@example.org\r\n\
                     ATTENDEE;PARTSTAT=ACCEPTED:mailto:bob@example.org\r\nATTENDEE:mailto:carol@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let cancel = cancel_text(event);
        assert!(cancel.contains("METHOD:CANCEL\r\n"));
        assert!(cancel.contains("UID:m\r\nSTATUS:CANCELLED\r\n"));
        assert_eq!(cancel.matches("STATUS:").count(), 1);
        let reply = reply_from(event, "bob@example.org");
        assert!(reply.contains("METHOD:REPLY"));
        assert!(reply.contains("bob@example.org"));
        assert!(!reply.contains("carol@example.org"));
        let statused = set_params(
            event,
            "ATTENDEE",
            "SCHEDULE-STATUS",
            &[("carol@example.org".into(), "1.2".into())],
        );
        assert!(statused.contains("ATTENDEE;SCHEDULE-STATUS=1.2:mailto:carol@example.org"));
    }
}
