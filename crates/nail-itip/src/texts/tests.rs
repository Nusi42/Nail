use super::*;
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use nail_api::localized::MessageArg;
use nail_api::pim::calendar::ConferenceLink;
use pretty_assertions::assert_eq;
use std::collections::BTreeSet;

const EN: &str = include_str!("../../../../locales/en/itip.ftl");
const DE: &str = include_str!("../../../../locales/de/itip.ftl");

/// Renders `itip.ftl` like nail-config's catalog (no isolation marks).
struct English(FluentBundle<FluentResource>);

impl English {
    fn new() -> Self {
        let lang: unic_langid::LanguageIdentifier = "en".parse().unwrap();
        let mut bundle = FluentBundle::new(vec![lang]);
        bundle.set_use_isolating(false);
        bundle
            .add_resource(FluentResource::try_new(EN.to_owned()).expect("itip.ftl parses"))
            .unwrap();
        Self(bundle)
    }
}

impl Localizer for English {
    fn render(&self, message: &LocalizedMessage) -> String {
        let pattern = self
            .0
            .get_message(&message.id)
            .and_then(|m| m.value())
            .unwrap_or_else(|| panic!("missing {}", message.id));
        let mut args = FluentArgs::new();
        for (name, value) in &message.args {
            match value {
                MessageArg::Number(n) => args.set(name.as_str(), FluentValue::from(*n)),
                MessageArg::Text(t) => args.set(name.as_str(), FluentValue::from(t.as_str())),
            }
        }
        let mut errors = Vec::new();
        let text = self.0.format_pattern(pattern, Some(&args), &mut errors);
        assert!(errors.is_empty(), "{}: {errors:?}", message.id);
        text.into_owned()
    }
}

fn ids(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|l| l.split_once(" ="))
        .map(|(id, _)| id.trim().to_owned())
        .filter(|id| nail_api::localized::is_message_id(id))
        .collect()
}

#[test]
fn the_catalog_matches_the_code() {
    let defined = ids(EN);
    let used: BTreeSet<String> = MESSAGE_IDS.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(defined, used);
    assert!(ids(DE).is_empty(), "the German catalog stays empty for now");
    // Every id written in the module is listed.
    let source = include_str!("../texts.rs");
    for id in source
        .split('"')
        .filter(|s| s.starts_with("itip-") && nail_api::localized::is_message_id(s))
    {
        assert!(used.contains(id), "{id} is not in MESSAGE_IDS");
    }
}

fn fields() -> EventFields {
    let mut f = EventFields::new(
        "Planning <Q4>",
        PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin"),
        PimTime::zoned("2026-10-05T10:00:00", "Europe/Berlin"),
    );
    f.location = Some("Room 1".into());
    f.description = Some("Agenda:\n1. Budget".into());
    f.organizer = Some(Organizer {
        email: "ada@example.org".into(),
        name: Some("Ada".into()),
    });
    let mut bob = Attendee::new("bob@example.org");
    bob.name = Some("Bob".into());
    let mut carol = Attendee::new("carol@example.org");
    carol.status = ParticipationStatus::Accepted;
    f.attendees = vec![bob, carol];
    f.conferences = vec![ConferenceLink {
        uri: "https://meet.example.org/abc".into(),
        label: None,
        features: Vec::new(),
    }];
    f
}

#[test]
fn an_invitation() {
    let f = fields();
    let mail = compose(
        &Summary {
            method: Some(ItipMethod::Request),
            fields: Some(&f),
            recurring: true,
            ..Summary::default()
        },
        &English::new(),
    );
    assert_eq!(mail.subject, "Invitation: Planning <Q4>");
    assert_eq!(
        mail.text,
        "Ada invites you to “Planning <Q4>”.\n\n\
When: 2026-10-05 09:00 – 10:00 (Europe/Berlin), repeating\n\
Where: Room 1\n\
Join: https://meet.example.org/abc\n\
Organizer: Ada <ada@example.org>\n\
Participants: Bob <bob@example.org> (no answer yet)\n    carol@example.org (accepted)\n\
Details: Agenda:\n    1. Budget\n\n\
This message contains calendar data. Calendar programs can add the event to your calendar and send your answer.\n"
    );
    assert!(
        mail.html
            .contains("<p>Ada invites you to “Planning &lt;Q4&gt;”.</p>")
    );
    assert!(mail.html.contains("<th align=\"left\" valign=\"top\">Participants</th><td>Bob &lt;bob@example.org&gt; (no answer yet)<br>\ncarol@example.org (accepted)</td>"));
    assert!(!mail.html.contains("<Q4>"));
}

#[test]
fn subjects_for_every_method() {
    let en = English::new();
    let f = fields();
    let mut bob = Attendee::new("bob@example.org");
    bob.name = Some("Bob".into());
    let subject = |method, update, status: ParticipationStatus, rid: Option<&PimTime>| {
        let mut a = bob.clone();
        a.status = status;
        a.delegated_to = vec!["dan@example.org".into()];
        compose(
            &Summary {
                method: Some(method),
                update,
                fields: Some(&f),
                recurrence_id: rid,
                attendee: Some(&a),
                ..Summary::default()
            },
            &en,
        )
    };
    use ParticipationStatus as P;
    let s = |m, u, st, rid| subject(m, u, st, rid).subject;
    assert_eq!(
        s(ItipMethod::Request, true, P::NeedsAction, None),
        "Updated invitation: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::Publish, false, P::NeedsAction, None),
        "Event: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::Add, false, P::NeedsAction, None),
        "More dates: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::Cancel, false, P::NeedsAction, None),
        "Cancelled: Planning <Q4>"
    );
    let rid = PimTime::date("2026-10-12");
    assert_eq!(
        s(ItipMethod::Cancel, false, P::NeedsAction, Some(&rid)),
        "Cancelled: Planning <Q4> on 2026-10-12"
    );
    assert_eq!(
        s(ItipMethod::Reply, false, P::Accepted, None),
        "Accepted: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::Reply, false, P::Tentative, None),
        "Tentatively accepted: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::Reply, false, P::Declined, None),
        "Declined: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::Reply, false, P::Delegated, None),
        "Delegated: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::Reply, false, P::NeedsAction, None),
        "Answer: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::Counter, false, P::Tentative, None),
        "New time proposed: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::DeclineCounter, false, P::Tentative, None),
        "Proposal declined: Planning <Q4>"
    );
    assert_eq!(
        s(ItipMethod::Refresh, false, P::Tentative, None),
        "Please send the latest version: Planning <Q4>"
    );

    let delegated = subject(ItipMethod::Reply, false, P::Delegated, None);
    assert!(
        delegated
            .text
            .starts_with("Bob has passed the invitation to “Planning <Q4>” on to dan@example.org.")
    );
    let declined = subject(ItipMethod::Reply, false, P::Declined, None);
    assert!(declined.text.starts_with("Bob has declined the invitation"));
    // Replies carry no description.
    assert!(!declined.text.contains("Budget"));
}

#[test]
fn counters_show_the_proposal_and_comment() {
    let f = fields();
    let proposal = CounterProposal {
        start: PimTime::utc("2026-10-05T13:00:00"),
        end: PimTime::utc("2026-10-06T14:30:00"),
        location: Some("Café".into()),
        comment: Some("Afternoon works better".into()),
    };
    let mut bob = Attendee::new("bob@example.org");
    bob.status = ParticipationStatus::Tentative;
    let mail = compose(
        &Summary {
            method: Some(ItipMethod::Counter),
            fields: Some(&f),
            attendee: Some(&bob),
            proposal: Some(&proposal),
            ..Summary::default()
        },
        &English::new(),
    );
    assert!(
        mail.text
            .starts_with("bob@example.org proposes a new time for “Planning <Q4>”.")
    );
    assert!(
        mail.text.contains(
            "Proposed time: 2026-10-05 13:00 (UTC) – 2026-10-06 14:30 (UTC)\nWhere: Café\n"
        )
    );
    assert!(mail.text.contains("Comment: Afternoon works better\n"));
}

#[test]
fn times() {
    let en = English::new();
    let d = |s: &str| PimTime::date(s);
    assert_eq!(
        format_when(&d("2026-10-05"), &d("2026-10-06"), false, &en),
        "2026-10-05 (all day)"
    );
    assert_eq!(
        format_when(&d("2026-10-05"), &d("2026-10-08"), false, &en),
        "2026-10-05 – 2026-10-07 (all day)"
    );
    assert_eq!(
        format_when(
            &PimTime::floating("2026-10-05T09:30:00"),
            &PimTime::floating("2026-10-05T10:00:00"),
            false,
            &en
        ),
        "2026-10-05 09:30 – 10:00"
    );
    assert_eq!(
        format_when(
            &PimTime::zoned("2026-10-05T23:00:00", "Europe/Berlin"),
            &PimTime::zoned("2026-10-06T01:00:00", "Europe/Berlin"),
            false,
            &en
        ),
        "2026-10-05 23:00 (Europe/Berlin) – 2026-10-06 01:00 (Europe/Berlin)"
    );
    assert_eq!(
        format_time(&PimTime::floating("2026-10-05T09:30:15"), &en),
        "2026-10-05 09:30:15"
    );
    // No title.
    let mut f = fields();
    f.title = "  ".into();
    let mail = compose(
        &Summary {
            fields: Some(&f),
            ..Summary::default()
        },
        &en,
    );
    assert_eq!(mail.subject, "Invitation: (No title)");
    assert_eq!(
        html_escape("<a href='x'>&\""),
        "&lt;a href=&#39;x&#39;&gt;&amp;&quot;"
    );
}

#[test]
fn closures_are_localizers() {
    let ids_only = |m: &LocalizedMessage| m.id.clone();
    let mail = compose(&Summary::default(), &ids_only);
    assert_eq!(mail.subject, "itip-subject-request");
    assert_eq!(mail.text, "itip-body-request\n\n\nitip-footer\n");
}
