//! Printing and saving as PDF (M4c, ADR 0094).
//!
//! Printing renders in the UI, with the theme: a **print job**
//! ([`crate::Request::CreatePrintJob`]) records what to print
//! ([`PrintTarget`]) and how ([`PrintOptions`]); the UI's print view
//! (route [`print_view_route`]) reads it back
//! ([`crate::Request::GetPrintJob`]), renders it with the theme's `print.*`
//! parts and, once laid out, reports [`crate::Request::PrintViewReady`].
//!
//! * **Printer**: the UI opens the print view and calls the browser's /
//!   WebView2's print dialog (`window.print()`); this also offers "Save as
//!   PDF" in web mode.
//! * **PDF file** (desktop): [`crate::Request::SavePrintPdf`] — the shell
//!   loads the print view in a hidden window, waits for the ready signal
//!   and writes the PDF with WebView2's `PrintToPdf`
//!   ([`crate::desktop::DesktopHost::print_to_pdf`]).
//!
//! Jobs live in memory for [`PRINT_JOB_TTL_SECS`]. Mail bodies are shown as
//! everywhere else (sanitized, remote content only where allowed).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{CalendarId, EventId, MessageId, PrintJobId, ThreadId};

/// How long a print job stays readable (seconds).
pub const PRINT_JOB_TTL_SECS: u64 = 600;

/// Longest agenda printed (days).
pub const MAX_AGENDA_DAYS: u32 = 366;

/// The UI route (hash) of the print view for a job: `#/print/<job id>`.
pub fn print_view_route(job_id: &PrintJobId) -> String {
    format!("#/print/{}", job_id.as_str())
}

labeled_enum! {
    /// How an agenda is laid out (`print-agenda-layout-*`).
    #[derive(Default)]
    pub enum AgendaLayout : "print-agenda-layout" {
        /// One page per day with times.
        Day = "day",
        /// A week per page.
        Week = "week",
        /// A month grid.
        Month = "month",
        /// A list of events by day (the default).
        #[default]
        List = "list",
    }
}

/// What to print.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PrintTarget {
    /// One message.
    Message {
        /// The message.
        message_id: MessageId,
    },
    /// A whole conversation, oldest first.
    Thread {
        /// The thread.
        thread_id: ThreadId,
    },
    /// One event (a series' occurrence when `occurrence` is set).
    Event {
        /// The event.
        event_id: EventId,
        /// The occurrence's original start (`RECURRENCE-ID`, as the event
        /// instance lists it); `null` = the event / series itself.
        #[serde(default)]
        occurrence: Option<String>,
    },
    /// The calendars' events in a date range.
    Agenda {
        /// First day (`YYYY-MM-DD`, local).
        start: String,
        /// Last day, inclusive (`YYYY-MM-DD`).
        end: String,
        /// The calendars; empty = the ones shown.
        #[serde(default)]
        calendar_ids: Vec<CalendarId>,
        /// The layout.
        #[serde(default)]
        layout: AgendaLayout,
    },
}

labeled_enum! {
    /// Why a print target is refused (`print-target-issue-*`).
    pub enum PrintTargetIssue : "print-target-issue" {
        /// A date is not `YYYY-MM-DD`.
        InvalidDate = "invalid-date",
        /// The end is before the start.
        EndBeforeStart = "end-before-start",
        /// More than [`MAX_AGENDA_DAYS`].
        RangeTooLong = "range-too-long",
    }
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` date (proleptic Gregorian).
fn day_number(date: &str) -> Option<i64> {
    let b = date.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| date.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m) || d < 1 || d > days_in[(m - 1) as usize] {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

impl PrintTarget {
    /// Problems (agenda dates; other targets are checked when read).
    pub fn issues(&self) -> Vec<PrintTargetIssue> {
        let Self::Agenda { start, end, .. } = self else {
            return Vec::new();
        };
        match (day_number(start), day_number(end)) {
            (Some(s), Some(e)) if e < s => vec![PrintTargetIssue::EndBeforeStart],
            (Some(s), Some(e)) if e - s + 1 > i64::from(MAX_AGENDA_DAYS) => {
                vec![PrintTargetIssue::RangeTooLong]
            }
            (Some(_), Some(_)) => Vec::new(),
            _ => vec![PrintTargetIssue::InvalidDate],
        }
    }
}

labeled_enum! {
    /// Paper size (`print-page-size-*`).
    #[derive(Default)]
    pub enum PageSize : "print-page-size" {
        /// ISO A4 (the default).
        #[default]
        A4 = "a4",
        /// ISO A5.
        A5 = "a5",
        /// US Letter.
        Letter = "letter",
        /// US Legal.
        Legal = "legal",
    }
}

impl PageSize {
    /// Width and height in millimetres (portrait).
    pub fn millimetres(self) -> (f64, f64) {
        match self {
            Self::A4 => (210.0, 297.0),
            Self::A5 => (148.0, 210.0),
            Self::Letter => (215.9, 279.4),
            Self::Legal => (215.9, 355.6),
        }
    }
}

labeled_enum! {
    /// Page orientation (`print-orientation-*`).
    #[derive(Default)]
    pub enum PageOrientation : "print-orientation" {
        /// Portrait (the default).
        #[default]
        Portrait = "portrait",
        /// Landscape.
        Landscape = "landscape",
    }
}

labeled_enum! {
    /// Which mail headers are printed (`print-headers-*`).
    #[derive(Default)]
    pub enum HeaderDetail : "print-headers" {
        /// From, To, Cc, Date, Subject (the default).
        #[default]
        Basic = "basic",
        /// Every header.
        Full = "full",
    }
}

/// How to print.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct PrintOptions {
    /// Paper size.
    pub page_size: PageSize,
    /// Orientation.
    pub orientation: PageOrientation,
    /// Mail headers.
    pub headers: HeaderDetail,
    /// Include quoted text of replies (else collapsed to a note).
    pub quoted_text: bool,
    /// List attachments (names and sizes).
    pub attachment_list: bool,
    /// Load remote images (only where remote content is allowed anyway).
    pub remote_images: bool,
    /// Print background colours and images.
    pub backgrounds: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            page_size: PageSize::A4,
            orientation: PageOrientation::Portrait,
            headers: HeaderDetail::Basic,
            quoted_text: true,
            attachment_list: true,
            remote_images: false,
            backgrounds: false,
        }
    }
}

/// A print job ([`crate::Request::CreatePrintJob`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PrintJob {
    /// The job.
    pub job_id: PrintJobId,
    /// What to print.
    pub target: PrintTarget,
    /// How.
    pub options: PrintOptions,
    /// When it was made (RFC 3339).
    pub created_at: String,
    /// The print view's route ([`print_view_route`]).
    pub view: String,
    /// Whether the print view reported it is laid out.
    #[serde(default)]
    pub ready: bool,
}

/// A written PDF ([`crate::Request::SavePrintPdf`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PdfFile {
    /// Where it was written.
    pub path: String,
    /// Its size.
    pub size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::assert_labels;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn labels_match_the_wire() {
        assert_labels(
            AgendaLayout::ALL,
            AgendaLayout::as_str,
            AgendaLayout::message_id,
        );
        assert_labels(
            PrintTargetIssue::ALL,
            PrintTargetIssue::as_str,
            PrintTargetIssue::message_id,
        );
        assert_labels(PageSize::ALL, PageSize::as_str, PageSize::message_id);
        assert_labels(
            PageOrientation::ALL,
            PageOrientation::as_str,
            PageOrientation::message_id,
        );
        assert_labels(
            HeaderDetail::ALL,
            HeaderDetail::as_str,
            HeaderDetail::message_id,
        );
    }

    #[test]
    fn dates() {
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(day_number("2026-10-02"), Some(20_728));
        assert_eq!(day_number("2024-02-29"), Some(19_782));
        assert_eq!(day_number("2023-02-29"), None);
        assert_eq!(day_number("2026-13-01"), None);
        assert_eq!(day_number("2026-1-01"), None);
    }

    #[test]
    fn agenda_checks() {
        let agenda = |start: &str, end: &str| PrintTarget::Agenda {
            start: start.into(),
            end: end.into(),
            calendar_ids: vec![],
            layout: AgendaLayout::Week,
        };
        assert!(agenda("2026-10-01", "2026-10-07").issues().is_empty());
        assert!(agenda("2026-01-01", "2026-12-31").issues().is_empty());
        assert_eq!(
            agenda("2026-10-07", "2026-10-01").issues(),
            vec![PrintTargetIssue::EndBeforeStart]
        );
        assert_eq!(
            agenda("2025-01-01", "2026-12-31").issues(),
            vec![PrintTargetIssue::RangeTooLong]
        );
        assert_eq!(
            agenda("tomorrow", "2026-12-31").issues(),
            vec![PrintTargetIssue::InvalidDate]
        );
        let message = PrintTarget::Message {
            message_id: "m1".into(),
        };
        assert!(message.issues().is_empty());
    }

    #[test]
    fn wire_shapes() {
        assert_eq!(
            serde_json::to_value(PrintTarget::Event {
                event_id: "e1".into(),
                occurrence: None
            })
            .unwrap(),
            json!({"type": "event", "event_id": "e1", "occurrence": null})
        );
        let options: PrintOptions = serde_json::from_value(json!({"page_size": "letter"})).unwrap();
        assert_eq!(options.page_size, PageSize::Letter);
        assert!(options.quoted_text);
        assert_eq!(print_view_route(&"p1".into()), "#/print/p1");
        assert_eq!(PageSize::A4.millimetres(), (210.0, 297.0));
    }
}
