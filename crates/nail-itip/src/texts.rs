//! The readable part of iMIP mail: subject, plain text and HTML
//! (RFC 6047 §2.4 asks for a human-readable alternative to the calendar
//! data). Every word comes from `locales/<lang>/itip.ftl` through a
//! [`Localizer`]; Rust only assembles lines. Times are written
//! numerically (`2026-10-05 09:00 (Europe/Berlin)`) and wrapped by Fluent
//! patterns, so translations decide the surrounding words.

use nail_api::localized::LocalizedMessage;
use nail_api::pim::PimTime;
use nail_api::pim::calendar::ParticipationStatus;
use nail_api::pim::calendar::{Attendee, EventFields, Organizer};
use nail_api::scheduling::{CounterProposal, ItipMethod};

/// Renders Fluent messages (the core passes `nail_config::l10n::Catalog`
/// for the user's language; tests render `itip.ftl` directly).
pub trait Localizer {
    /// The text of a message.
    fn render(&self, message: &LocalizedMessage) -> String;
}

impl<F: Fn(&LocalizedMessage) -> String> Localizer for F {
    fn render(&self, message: &LocalizedMessage) -> String {
        self(message)
    }
}

/// Every Fluent id this module uses (all in `itip.ftl`).
pub const MESSAGE_IDS: &[&str] = &[
    "itip-subject-request",
    "itip-subject-update",
    "itip-subject-publish",
    "itip-subject-add",
    "itip-subject-cancel",
    "itip-subject-cancel-occurrence",
    "itip-subject-reply",
    "itip-subject-counter",
    "itip-subject-decline-counter",
    "itip-subject-refresh",
    "itip-body-request",
    "itip-body-update",
    "itip-body-publish",
    "itip-body-add",
    "itip-body-cancel",
    "itip-body-cancel-occurrence",
    "itip-body-reply",
    "itip-body-reply-delegated",
    "itip-body-counter",
    "itip-body-decline-counter",
    "itip-body-refresh",
    "itip-label-when",
    "itip-label-where",
    "itip-label-join",
    "itip-label-organizer",
    "itip-label-attendees",
    "itip-label-proposed",
    "itip-label-comment",
    "itip-label-description",
    "itip-line",
    "itip-when-range",
    "itip-when-zone",
    "itip-when-all-day",
    "itip-when-all-day-range",
    "itip-when-recurring",
    "itip-status-accepted",
    "itip-status-tentative",
    "itip-status-declined",
    "itip-status-delegated",
    "itip-status-needs-action",
    "itip-untitled",
    "itip-footer",
];

/// What the mail is about.
#[derive(Debug, Clone, Default)]
pub struct Summary<'a> {
    /// The method (`None` = `REQUEST`).
    pub method: Option<ItipMethod>,
    /// A `REQUEST` that updates an invitation sent before.
    pub update: bool,
    /// The event (or occurrence) fields.
    pub fields: Option<&'a EventFields>,
    /// The occurrence the message is about.
    pub recurrence_id: Option<&'a PimTime>,
    /// The event recurs.
    pub recurring: bool,
    /// The organizer (falls back to `fields.organizer`).
    pub organizer: Option<&'a Organizer>,
    /// For `REPLY` / `COUNTER` / `REFRESH` / `DECLINECOUNTER`: the attendee.
    pub attendee: Option<&'a Attendee>,
    /// The comment sent along.
    pub comment: Option<&'a str>,
    /// For `COUNTER`: the proposal.
    pub proposal: Option<&'a CounterProposal>,
}

/// Subject, plain text and HTML of an iMIP mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailText {
    /// `Subject`.
    pub subject: String,
    /// `text/plain` body.
    pub text: String,
    /// `text/html` body.
    pub html: String,
}

fn msg(id: &str) -> LocalizedMessage {
    LocalizedMessage::new(id)
}

/// `2026-10-05 09:00`.
fn wall(local: &str) -> String {
    let mut out = local.replacen('T', " ", 1);
    // Drop the seconds when they are zero.
    if out.len() == 19 && out.ends_with(":00") {
        out.truncate(16);
    }
    out
}

/// A time as text: the date for all-day values, else date and time with
/// the zone.
pub fn format_time(t: &PimTime, loc: &dyn Localizer) -> String {
    match t {
        PimTime::Date { date } => date.clone(),
        PimTime::Floating { local } => wall(local),
        PimTime::Zoned { local, tz } => loc.render(
            &msg("itip-when-zone")
                .arg("time", wall(local))
                .arg("zone", tz.as_str()),
        ),
    }
}

/// The time span of an event as text.
pub fn format_when(start: &PimTime, end: &PimTime, recurring: bool, loc: &dyn Localizer) -> String {
    let when = match (start, end) {
        (PimTime::Date { date: s }, PimTime::Date { date: e }) => {
            let last = nail_ical::time::parse_date(e)
                .and_then(|d| d.pred_opt())
                .map(nail_ical::time::format_date)
                .filter(|last| last.as_str() > s.as_str());
            match last {
                Some(last) => loc.render(
                    &msg("itip-when-all-day-range")
                        .arg("start", s.as_str())
                        .arg("end", last),
                ),
                None => loc.render(&msg("itip-when-all-day").arg("date", s.as_str())),
            }
        }
        (PimTime::Zoned { local: a, tz: x }, PimTime::Zoned { local: b, tz: y })
            if x == y && a.get(..10) == b.get(..10) =>
        {
            // `2026-10-05 09:00 to 10:00 (zone)`: the zone once.
            let range = range(wall(a), time_of(b), loc);
            loc.render(
                &msg("itip-when-zone")
                    .arg("time", range)
                    .arg("zone", x.as_str()),
            )
        }
        (PimTime::Floating { local: a }, PimTime::Floating { local: b })
            if a.get(..10) == b.get(..10) =>
        {
            range(wall(a), time_of(b), loc)
        }
        _ => range(format_time(start, loc), format_time(end, loc), loc),
    };
    wrap_recurring(when, recurring, loc)
}

fn range(start: String, end: String, loc: &dyn Localizer) -> String {
    loc.render(&msg("itip-when-range").arg("start", start).arg("end", end))
}

/// `10:00` of `2026-10-05T10:00:00`.
fn time_of(local: &str) -> String {
    wall(local).get(11..).unwrap_or_default().to_owned()
}

fn wrap_recurring(when: String, recurring: bool, loc: &dyn Localizer) -> String {
    if recurring {
        loc.render(&msg("itip-when-recurring").arg("when", when))
    } else {
        when
    }
}

fn status_text(status: ParticipationStatus, loc: &dyn Localizer) -> String {
    loc.render(&msg(match status {
        ParticipationStatus::Accepted => "itip-status-accepted",
        ParticipationStatus::Tentative => "itip-status-tentative",
        ParticipationStatus::Declined => "itip-status-declined",
        ParticipationStatus::Delegated => "itip-status-delegated",
        ParticipationStatus::NeedsAction => "itip-status-needs-action",
    }))
}

fn status_key(status: ParticipationStatus) -> &'static str {
    match status {
        ParticipationStatus::Accepted => "accepted",
        ParticipationStatus::Tentative => "tentative",
        ParticipationStatus::Declined => "declined",
        ParticipationStatus::Delegated => "delegated",
        ParticipationStatus::NeedsAction => "needs-action",
    }
}

fn person(name: Option<&String>, email: &str) -> String {
    match name.map(|n| n.trim()).filter(|n| !n.is_empty()) {
        Some(name) => format!("{name} <{email}>"),
        None => email.to_owned(),
    }
}

fn short_person(name: Option<&String>, email: &str) -> String {
    name.map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .unwrap_or(email)
        .to_owned()
}

/// Escapes text for HTML element content and attribute values.
pub fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn html_text(text: &str) -> String {
    html_escape(text).replace('\n', "<br>\n")
}

/// Builds subject, text and HTML of an iMIP mail.
pub fn compose(summary: &Summary<'_>, loc: &dyn Localizer) -> MailText {
    let method = summary.method.unwrap_or(ItipMethod::Request);
    let fields = summary.fields;
    let title = fields
        .map(|f| f.title.trim())
        .filter(|t| !t.is_empty())
        .map_or_else(|| loc.render(&msg("itip-untitled")), str::to_owned);
    let organizer = summary
        .organizer
        .or_else(|| fields.and_then(|f| f.organizer.as_ref()));
    let organizer_name =
        organizer.map_or_else(String::new, |o| short_person(o.name.as_ref(), &o.email));
    let attendee_name = summary
        .attendee
        .map_or_else(String::new, |a| short_person(a.name.as_ref(), &a.email));
    let status = summary
        .attendee
        .map_or(ParticipationStatus::NeedsAction, |a| a.status);
    let occurrence = summary
        .recurrence_id
        .map(|rid| format_time(rid, loc))
        .unwrap_or_default();

    let with_people = |id: &str| {
        msg(id)
            .arg("title", title.as_str())
            .arg("organizer", organizer_name.as_str())
            .arg("attendee", attendee_name.as_str())
            .arg("status", status_key(status))
            .arg("date", occurrence.as_str())
    };
    let (subject_id, body_id) = match method {
        ItipMethod::Request if summary.update => ("itip-subject-update", "itip-body-update"),
        ItipMethod::Request => ("itip-subject-request", "itip-body-request"),
        ItipMethod::Publish => ("itip-subject-publish", "itip-body-publish"),
        ItipMethod::Add => ("itip-subject-add", "itip-body-add"),
        ItipMethod::Cancel if summary.recurrence_id.is_some() => (
            "itip-subject-cancel-occurrence",
            "itip-body-cancel-occurrence",
        ),
        ItipMethod::Cancel => ("itip-subject-cancel", "itip-body-cancel"),
        ItipMethod::Reply if status == ParticipationStatus::Delegated => {
            ("itip-subject-reply", "itip-body-reply-delegated")
        }
        ItipMethod::Reply => ("itip-subject-reply", "itip-body-reply"),
        ItipMethod::Counter => ("itip-subject-counter", "itip-body-counter"),
        ItipMethod::DeclineCounter => ("itip-subject-decline-counter", "itip-body-decline-counter"),
        ItipMethod::Refresh => ("itip-subject-refresh", "itip-body-refresh"),
    };
    let subject = loc.render(&with_people(subject_id));
    let delegate = summary
        .attendee
        .and_then(|a| a.delegated_to.first())
        .cloned()
        .unwrap_or_default();
    let intro = loc.render(&with_people(body_id).arg("delegate", delegate));

    // Label / value rows.
    let mut rows: Vec<(String, String)> = Vec::new();
    let label = |id: &str| loc.render(&msg(id));
    if let Some(f) = fields {
        let recurring = summary.recurring && summary.recurrence_id.is_none();
        rows.push((
            label("itip-label-when"),
            format_when(&f.start, &f.end, recurring, loc),
        ));
        if let Some(location) = f
            .location
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            rows.push((label("itip-label-where"), location.to_owned()));
        }
        for link in &f.conferences {
            rows.push((label("itip-label-join"), link.uri.clone()));
        }
    }
    if let Some(p) = summary.proposal {
        rows.push((
            label("itip-label-proposed"),
            format_when(&p.start, &p.end, false, loc),
        ));
        if let Some(location) = p
            .location
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            rows.push((label("itip-label-where"), location.to_owned()));
        }
    }
    if let Some(o) = organizer {
        rows.push((
            label("itip-label-organizer"),
            person(o.name.as_ref(), &o.email),
        ));
    }
    if let Some(f) = fields.filter(|f| !f.attendees.is_empty()) {
        let people: Vec<String> = f
            .attendees
            .iter()
            .map(|a| {
                format!(
                    "{} ({})",
                    person(a.name.as_ref(), &a.email),
                    status_text(a.status, loc)
                )
            })
            .collect();
        rows.push((label("itip-label-attendees"), people.join("\n")));
    }
    let comment = summary
        .comment
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .or_else(|| summary.proposal.and_then(|p| p.comment.as_deref()));
    if let Some(comment) = comment {
        rows.push((label("itip-label-comment"), comment.to_owned()));
    }
    if matches!(
        method,
        ItipMethod::Request | ItipMethod::Publish | ItipMethod::Add
    ) && let Some(description) = fields
        .and_then(|f| f.description.as_deref())
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        rows.push((label("itip-label-description"), description.to_owned()));
    }
    let footer = loc.render(&msg("itip-footer"));

    let mut text = String::new();
    text.push_str(&intro);
    text.push_str("\n\n");
    for (label, value) in &rows {
        let value = value.replace('\n', "\n    ");
        text.push_str(
            &loc.render(
                &msg("itip-line")
                    .arg("label", label.as_str())
                    .arg("value", value),
            ),
        );
        text.push('\n');
    }
    text.push('\n');
    text.push_str(&footer);
    text.push('\n');

    let mut html =
        String::from("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"></head><body>\n");
    html.push_str(&format!("<p>{}</p>\n<table>\n", html_text(&intro)));
    for (label, value) in &rows {
        html.push_str(&format!(
            "<tr><th align=\"left\" valign=\"top\">{}</th><td>{}</td></tr>\n",
            html_escape(label),
            html_text(value)
        ));
    }
    html.push_str(&format!(
        "</table>\n<p><small>{}</small></p>\n</body></html>\n",
        html_text(&footer)
    ));
    MailText {
        subject,
        text,
        html,
    }
}

#[cfg(test)]
mod tests;
