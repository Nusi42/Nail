//! Calendars, tasks and contacts: `.ics` / `.vcf` files in (through
//! `nail-ical`) and out (one iCalendar file per calendar, one vCard file
//! per address book).

use std::path::Path;

use nail_api::portability::{ImportCollectionKind, ItemRef, PortabilityIssueCode};
use nail_ical::content::{self, Component, Property};
use nail_ical::import::ImportData;
use nail_ical::{Issue, IssueKind};

use crate::error::{PortabilityError, Result, SinkError};
use crate::job::JobContext;
use crate::sink::{PimImportSink, SourceCollection};

/// `PRODID` of the calendar files Nail exports.
pub const EXPORT_PRODID: &str = "-//Nail//Nail portability//EN";

/// Reads an `.ics` / `.vcf` file.
pub fn read_file(path: &Path) -> Result<ImportData> {
    let bytes =
        std::fs::read(path).map_err(|e| PortabilityError::io(path.display().to_string(), e))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    Ok(nail_ical::import::read_bytes(&bytes, name.as_deref()))
}

/// What a file holds, as a collection of the preview.
pub fn collection_of(key: &str, fallback_name: &str, data: &ImportData) -> (SourceCollection, u64) {
    match data {
        ImportData::Calendar(calendar) => {
            let kind = if calendar.events.is_empty() && !calendar.tasks.is_empty() {
                ImportCollectionKind::TaskList
            } else {
                ImportCollectionKind::Calendar
            };
            (
                SourceCollection {
                    key: key.to_owned(),
                    name: calendar
                        .name
                        .clone()
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| fallback_name.to_owned()),
                    kind,
                },
                (calendar.events.len() + calendar.tasks.len()) as u64,
            )
        }
        ImportData::Cards(cards) => (
            SourceCollection {
                key: key.to_owned(),
                name: fallback_name.to_owned(),
                kind: ImportCollectionKind::AddressBook,
            },
            (cards.contacts.len() + cards.groups.len()) as u64,
        ),
    }
}

/// The issue code of a `nail-ical` issue.
fn issue_code(issue: &Issue, calendar: bool) -> PortabilityIssueCode {
    match issue.kind {
        IssueKind::NotICalendar | IssueKind::NotVCard => PortabilityIssueCode::UnsupportedFormat,
        _ if calendar => PortabilityIssueCode::InvalidCalendarData,
        _ => PortabilityIssueCode::InvalidContactData,
    }
}

/// Reports `nail-ical`'s issues (skipped or changed items) for a file.
fn report_issues(
    ctx: &mut JobContext<'_>,
    collection: &SourceCollection,
    file: Option<&str>,
    issues: &[Issue],
    calendar: bool,
) {
    for issue in issues {
        // "No items" is not a problem of an item.
        if matches!(issue.kind, IssueKind::NoItems | IssueKind::NoContacts) {
            continue;
        }
        let item = ItemRef {
            file: file.map(str::to_owned),
            folder: Some(collection.name.clone()),
            index: issue.line.map(u64::from),
            label: issue.item.clone(),
            ..ItemRef::default()
        };
        let detail = format!("{:?} ({})", issue.kind, issue.message_id());
        let code = issue_code(issue, calendar);
        if issue.skipped {
            ctx.skipped(code, item, detail);
        } else {
            ctx.warning(code, item, detail);
        }
    }
}

/// Imports what a file / address book holds into the sink, counting events,
/// tasks, contacts and groups.
pub fn import_data(
    ctx: &mut JobContext<'_>,
    sink: &mut dyn PimImportSink,
    collection: &SourceCollection,
    file: Option<&str>,
    data: ImportData,
) -> Result<()> {
    if let Some(fatal) = nail_ical::import::fatal_issue(&data) {
        let calendar = matches!(data, ImportData::Calendar(_));
        ctx.skipped(
            issue_code(fatal, calendar),
            ItemRef {
                file: file.map(str::to_owned),
                folder: Some(collection.name.clone()),
                ..ItemRef::default()
            },
            fatal.message_id(),
        );
        return Ok(());
    }
    match data {
        ImportData::Calendar(calendar) => {
            report_issues(ctx, collection, file, &calendar.issues, true);
            for event in calendar.events {
                ctx.check_cancelled()?;
                let item = item_ref(file, collection, Some(event.line), &event.fields.title);
                match sink.event(collection, event) {
                    Ok(()) => ctx.report.counts.events += 1,
                    Err(e) => write_failed(ctx, item, e)?,
                }
                ctx.advance(1, 0);
            }
            for task in calendar.tasks {
                ctx.check_cancelled()?;
                let item = item_ref(file, collection, Some(task.line), &task.fields.title);
                match sink.task(collection, task) {
                    Ok(()) => ctx.report.counts.tasks += 1,
                    Err(e) => write_failed(ctx, item, e)?,
                }
                ctx.advance(1, 0);
            }
        }
        ImportData::Cards(cards) => {
            report_issues(ctx, collection, file, &cards.issues, false);
            for contact in cards.contacts {
                ctx.check_cancelled()?;
                let item = item_ref(
                    file,
                    collection,
                    Some(contact.line),
                    &contact.fields.display_name,
                );
                match sink.contact(collection, contact) {
                    Ok(()) => ctx.report.counts.contacts += 1,
                    Err(e) => write_failed(ctx, item, e)?,
                }
                ctx.advance(1, 0);
            }
            for group in cards.groups {
                ctx.check_cancelled()?;
                let item = item_ref(file, collection, Some(group.line), &group.group.name);
                match sink.group(collection, group) {
                    Ok(()) => ctx.report.counts.groups += 1,
                    Err(e) => write_failed(ctx, item, e)?,
                }
                ctx.advance(1, 0);
            }
        }
    }
    Ok(())
}

fn item_ref(
    file: Option<&str>,
    collection: &SourceCollection,
    line: Option<u32>,
    label: &str,
) -> ItemRef {
    ItemRef {
        file: file.map(str::to_owned),
        folder: Some(collection.name.clone()),
        index: line.map(u64::from),
        label: (!label.trim().is_empty()).then(|| label.to_owned()),
        ..ItemRef::default()
    }
}

/// Records a sink failure: an item failure is a skipped item, a fatal one
/// ends the job.
pub(crate) fn write_failed(
    ctx: &mut JobContext<'_>,
    item: ItemRef,
    error: SinkError,
) -> Result<()> {
    match error {
        SinkError::Item(detail) => {
            ctx.skipped(PortabilityIssueCode::WriteFailed, item, detail);
            Ok(())
        }
        SinkError::Fatal(detail) => Err(PortabilityError::Sink(detail)),
    }
}

// ---------------------------------------------------------------------------
// Writing

/// Builds one iCalendar file from items that are each a whole `VCALENDAR`:
/// their `VEVENT` / `VTODO` / `VJOURNAL` components go in, `VTIMEZONE`s
/// once per `TZID`.
#[derive(Debug)]
pub struct IcsBuilder {
    calendar: Component,
    zones: Vec<String>,
    items: u64,
    events: u64,
    tasks: u64,
}

impl IcsBuilder {
    /// A calendar named `name` (`X-WR-CALNAME`).
    pub fn new(name: &str) -> Self {
        let mut calendar = Component::new("VCALENDAR");
        calendar.properties.push(Property::new("VERSION", "2.0"));
        calendar
            .properties
            .push(Property::new("PRODID", EXPORT_PRODID));
        calendar
            .properties
            .push(Property::new("CALSCALE", "GREGORIAN"));
        if !name.trim().is_empty() {
            calendar
                .properties
                .push(Property::text_value("X-WR-CALNAME", name));
        }
        Self {
            calendar,
            zones: Vec::new(),
            items: 0,
            events: 0,
            tasks: 0,
        }
    }

    /// Adds an item's text; returns how many components it added (0 = not
    /// iCalendar or empty).
    pub fn add(&mut self, text: &str) -> usize {
        let document = content::parse(text);
        let mut added = 0;
        for top in document.components {
            let children = if top.is("VCALENDAR") {
                top.components
            } else {
                vec![top]
            };
            for component in children {
                if component.is("VTIMEZONE") {
                    let tzid = component.text("TZID").unwrap_or_default();
                    if !self.zones.contains(&tzid) {
                        self.zones.push(tzid);
                        // Zones go before the items.
                        let at = self
                            .calendar
                            .components
                            .iter()
                            .position(|c| !c.is("VTIMEZONE"))
                            .unwrap_or(self.calendar.components.len());
                        self.calendar.components.insert(at, component);
                    }
                } else if component.is("VEVENT")
                    || component.is("VTODO")
                    || component.is("VJOURNAL")
                {
                    // Overrides share the series' UID: count series once.
                    let is_override = component.property("RECURRENCE-ID").is_some();
                    if component.is("VEVENT") && !is_override {
                        self.events += 1;
                    } else if component.is("VTODO") && !is_override {
                        self.tasks += 1;
                    }
                    self.calendar.components.push(component);
                    added += 1;
                }
            }
        }
        if added > 0 {
            self.items += 1;
        }
        added
    }

    /// Items added.
    pub fn items(&self) -> u64 {
        self.items
    }

    /// Event series added.
    pub fn events(&self) -> u64 {
        self.events
    }

    /// Tasks added.
    pub fn tasks(&self) -> u64 {
        self.tasks
    }

    /// The file's text.
    pub fn finish(self) -> String {
        self.calendar.to_text()
    }
}

/// Builds one vCard file from card texts (each one or more `VCARD`s).
#[derive(Debug, Default)]
pub struct VcfBuilder {
    out: String,
    cards: u64,
    groups: u64,
}

impl VcfBuilder {
    /// An empty file.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a card's text; returns how many cards it added.
    pub fn add(&mut self, text: &str) -> usize {
        let document = content::parse(text);
        let mut added = 0;
        for card in document.components.iter().filter(|c| c.is("VCARD")) {
            card.write(&mut self.out);
            added += 1;
            let group = card
                .text("KIND")
                .or_else(|| card.text("X-ADDRESSBOOKSERVER-KIND"))
                .is_some_and(|k| k.eq_ignore_ascii_case("group"));
            if group {
                self.groups += 1;
            }
        }
        self.cards += added as u64;
        added
    }

    /// Cards added (contacts and groups).
    pub fn cards(&self) -> u64 {
        self.cards
    }

    /// Group cards added.
    pub fn groups(&self) -> u64 {
        self.groups
    }

    /// The file's text.
    pub fn finish(self) -> String {
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::NullSink;
    use nail_api::portability::{JobKind, JobReport};
    use nail_ical::vcard::{ParsedContact, ParsedGroup};
    use nail_ical::{ParsedEvent, ParsedTask};

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\nX-WR-CALNAME:Family\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:e1\r\nDTSTAMP:20260101T000000Z\r\nDTSTART;TZID=Europe/Berlin:20261001T100000\r\nSUMMARY:Dinner\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:e2\r\nDTSTAMP:20260101T000000Z\r\nSUMMARY:No start\r\nEND:VEVENT\r\nBEGIN:VTODO\r\nUID:t1\r\nDTSTAMP:20260101T000000Z\r\nSUMMARY:Shop\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    const VCF: &str = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Ada Lovelace\r\nEMAIL:ada@example.org\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:4.0\r\nUID:g1\r\nKIND:group\r\nFN:Friends\r\nMEMBER:urn:uuid:c1\r\nEND:VCARD\r\n";

    #[derive(Default)]
    struct Recorder {
        titles: Vec<String>,
        fail_tasks: bool,
    }

    impl PimImportSink for Recorder {
        fn event(
            &mut self,
            _c: &SourceCollection,
            e: ParsedEvent,
        ) -> std::result::Result<(), SinkError> {
            self.titles.push(e.fields.title);
            Ok(())
        }
        fn task(
            &mut self,
            _c: &SourceCollection,
            t: ParsedTask,
        ) -> std::result::Result<(), SinkError> {
            if self.fail_tasks {
                return Err(SinkError::Item("disk full".into()));
            }
            self.titles.push(t.fields.title);
            Ok(())
        }
        fn contact(
            &mut self,
            _c: &SourceCollection,
            c: ParsedContact,
        ) -> std::result::Result<(), SinkError> {
            self.titles.push(c.fields.display_name);
            Ok(())
        }
        fn group(
            &mut self,
            _c: &SourceCollection,
            g: ParsedGroup,
        ) -> std::result::Result<(), SinkError> {
            self.titles.push(g.group.name);
            Ok(())
        }
    }

    fn ctx() -> JobContext<'static> {
        JobContext::silent(JobReport::started("j".into(), JobKind::Import, "t"))
    }

    #[test]
    fn imports_calendars_with_item_issues() {
        let data = nail_ical::import::read(ICS, Some("family.ics"));
        let (collection, items) = collection_of("family.ics", "family", &data);
        assert_eq!(collection.name, "Family");
        assert_eq!(collection.kind, ImportCollectionKind::Calendar);
        assert_eq!(items, 2);
        let mut sink = Recorder {
            fail_tasks: true,
            ..Recorder::default()
        };
        let mut ctx = ctx();
        import_data(&mut ctx, &mut sink, &collection, Some("family.ics"), data).unwrap();
        assert_eq!(sink.titles, vec!["Dinner"]);
        assert_eq!(ctx.report.counts.events, 1);
        assert_eq!(ctx.report.counts.tasks, 0);
        // The event without a start (skipped by nail-ical) and the failed task.
        let codes: Vec<_> = ctx.report.issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&PortabilityIssueCode::InvalidCalendarData));
        assert!(codes.contains(&PortabilityIssueCode::WriteFailed));
        assert_eq!(ctx.report.counts.skipped, 2);
    }

    #[test]
    fn imports_cards() {
        let data = nail_ical::import::read(VCF, Some("x.vcf"));
        let (collection, items) = collection_of("x.vcf", "x", &data);
        assert_eq!(collection.kind, ImportCollectionKind::AddressBook);
        assert_eq!(items, 2);
        let mut sink = Recorder::default();
        let mut ctx = ctx();
        import_data(&mut ctx, &mut sink, &collection, None, data).unwrap();
        assert_eq!(sink.titles, vec!["Ada Lovelace", "Friends"]);
        assert_eq!(ctx.report.counts.contacts, 1);
        assert_eq!(ctx.report.counts.groups, 1);
        assert!(ctx.report.issues.is_empty(), "{:?}", ctx.report.issues);
    }

    #[test]
    fn not_icalendar_is_one_skipped_item() {
        let data = nail_ical::import::read("hello", Some("x.ics"));
        let (collection, _) = collection_of("x.ics", "x", &data);
        let mut ctx = ctx();
        import_data(&mut ctx, &mut NullSink, &collection, None, data).unwrap();
        assert_eq!(ctx.report.issues.len(), 1);
        assert_eq!(
            ctx.report.issues[0].code,
            PortabilityIssueCode::UnsupportedFormat
        );
    }

    #[test]
    fn fatal_sink_errors_stop() {
        struct Fatal;
        impl PimImportSink for Fatal {
            fn event(
                &mut self,
                _c: &SourceCollection,
                _e: ParsedEvent,
            ) -> std::result::Result<(), SinkError> {
                Err(SinkError::Fatal("store closed".into()))
            }
            fn task(
                &mut self,
                _c: &SourceCollection,
                _t: ParsedTask,
            ) -> std::result::Result<(), SinkError> {
                Ok(())
            }
            fn contact(
                &mut self,
                _c: &SourceCollection,
                _p: ParsedContact,
            ) -> std::result::Result<(), SinkError> {
                Ok(())
            }
            fn group(
                &mut self,
                _c: &SourceCollection,
                _g: ParsedGroup,
            ) -> std::result::Result<(), SinkError> {
                Ok(())
            }
        }
        let data = nail_ical::import::read(ICS, None);
        let (collection, _) = collection_of("k", "n", &data);
        let result = import_data(&mut ctx(), &mut Fatal, &collection, None, data);
        assert!(matches!(result, Err(PortabilityError::Sink(_))));
    }

    #[test]
    fn ics_builder_merges_items_and_zones() {
        let mut builder = IcsBuilder::new("Family");
        assert_eq!(builder.add(ICS), 3);
        // The same zone again is not repeated.
        assert_eq!(builder.add(ICS), 3);
        assert_eq!(builder.add("not ical"), 0);
        assert_eq!(builder.items(), 2);
        assert_eq!((builder.events(), builder.tasks()), (4, 2));
        let text = builder.finish();
        assert_eq!(text.matches("BEGIN:VTIMEZONE").count(), 1);
        assert_eq!(text.matches("BEGIN:VEVENT").count(), 4);
        assert!(text.find("BEGIN:VTIMEZONE").unwrap() < text.find("BEGIN:VEVENT").unwrap());
        let back = nail_ical::import::read(&text, None);
        let ImportData::Calendar(calendar) = back else {
            panic!("not a calendar");
        };
        assert_eq!(calendar.name.as_deref(), Some("Family"));
        assert_eq!(calendar.tasks.len(), 1);
    }

    #[test]
    fn vcf_builder_concatenates_cards() {
        let mut builder = VcfBuilder::new();
        assert_eq!(builder.add(VCF), 2);
        assert_eq!(builder.add("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n"), 0);
        assert_eq!(builder.cards(), 2);
        assert_eq!(builder.groups(), 1);
        let text = builder.finish();
        let ImportData::Cards(cards) = nail_ical::import::read(&text, None) else {
            panic!("not cards");
        };
        assert_eq!(cards.contacts.len(), 1);
        assert_eq!(cards.groups.len(), 1);
    }
}
