//! `accounts.kdl` PIM sources (M3a, ADR 0058): `calendar-subscription`,
//! `caldav` and `carddav` nodes — reading, errors, and the
//! format-preserving writer.

use nail_api::accounts::{AuthConfig, SecretRef};
use nail_api::pim::{PimSourceConfig, PimSourceKind, PimSourceSettings};
use nail_api::{AccountId, PimSourceId};
use nail_config::{AccountsDocument, parse_accounts};
use pretty_assertions::assert_eq;

const FILE: &str = r##"account "work" email="ada@work.example" {
    imap host="imap.work.example"
}

// Public holidays
calendar-subscription "holidays" name="Holidays" url="webcal://example.org/holidays.ics" color="#d33" refresh=1440

caldav "work-dav" account="work"
carddav "work-contacts" account="work" url="https://dav.work.example/" refresh=30

// A Radicale on this computer, without a mail account
caldav "radicale" url="http://127.0.0.1:5232/" enabled=#false {
    auth "password" username="ada" secret="keychain:nail/pim/radicale/password"
}
"##;

fn source(config: &nail_api::AccountsConfig, id: &str) -> PimSourceConfig {
    config.pim_source(&PimSourceId::new(id)).unwrap().clone()
}

#[test]
fn sources_are_read_in_file_order() {
    let config = parse_accounts(FILE, "accounts.kdl").unwrap();
    let ids: Vec<&str> = config.pim_sources.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["holidays", "work-dav", "work-contacts", "radicale"]);

    let holidays = source(&config, "holidays");
    assert_eq!(holidays.kind(), PimSourceKind::Ics);
    assert_eq!(holidays.name.as_deref(), Some("Holidays"));
    assert!(holidays.enabled);
    assert_eq!(
        holidays.settings,
        PimSourceSettings::Ics {
            url: "webcal://example.org/holidays.ics".into(),
            color: Some("#d33".into()),
            alarms: false,
            refresh_minutes: Some(1440),
            auth: None,
        }
    );

    let work = source(&config, "work-dav");
    assert_eq!(work.account(), Some(&AccountId::new("work")));
    assert_eq!(work.url(), None);
    assert_eq!(work.auth(), None);
    assert_eq!(work.refresh_minutes(), 15);

    let contacts = source(&config, "work-contacts");
    assert_eq!(contacts.kind(), PimSourceKind::Carddav);
    assert_eq!(contacts.refresh_minutes(), 30);

    let radicale = source(&config, "radicale");
    assert!(!radicale.enabled);
    assert_eq!(radicale.account(), None);
    assert_eq!(
        radicale.auth(),
        Some(&AuthConfig::Password {
            username: "ada".into(),
            secret: Some(SecretRef::parse("keychain:nail/pim/radicale/password").unwrap()),
            mechanism: None,
        })
    );
    for source in &config.pim_sources {
        assert!(source.issues().is_empty(), "{source:?}");
    }
}

#[test]
fn an_account_linked_login_defaults_to_the_account_address() {
    let text = r#"account "work" email="ada@work.example" {
    imap host="imap.work.example"
}
caldav "work-dav" account="work" {
    auth "password"
}
"#;
    let config = parse_accounts(text, "accounts.kdl").unwrap();
    assert_eq!(
        source(&config, "work-dav").auth(),
        Some(&AuthConfig::Password {
            username: "ada@work.example".into(),
            secret: None,
            mechanism: None,
        })
    );
}

#[test]
fn invalid_sources_are_refused_with_positions() {
    let account =
        "account \"work\" email=\"ada@work.example\" {\n    imap host=\"imap.work.example\"\n}\n";
    let cases: &[(&str, &str)] = &[
        ("caldav \"local\" account=\"work\"\n", "reserved"),
        ("caldav \"Work\" account=\"work\"\n", "invalid source name"),
        ("caldav \"x\" account=\"nope\"\n", "unknown account"),
        ("caldav \"x\"\n", "needs url"),
        (
            "caldav \"x\" url=\"https://dav.example.org/\"\n",
            "needs an `auth` node",
        ),
        (
            "caldav \"x\" url=\"https://dav.example.org/\" {\n    auth \"password\"\n}\n",
            "needs username",
        ),
        (
            "caldav \"x\" account=\"work\" url=\"http://dav.example.org/\"\n",
            "url",
        ),
        ("caldav \"x\" account=\"work\" refresh=1\n", "refresh"),
        ("calendar-subscription \"h\"\n", "needs url"),
        (
            "calendar-subscription \"h\" url=\"https://e.example/h.ics\" color=\"light blue\"\n",
            "color",
        ),
        (
            "calendar-subscription \"h\" url=\"https://e.example/h.ics\" account=\"work\"\n",
            "account",
        ),
        (
            "caldav \"x\" account=\"work\" password=\"hunter2\"\n",
            "keychain",
        ),
        (
            "caldav \"x\" account=\"work\"\ncarddav \"x\" account=\"work\"\n",
            "already defined",
        ),
        ("caldav \"x\" account=\"work\" {\n    sync\n}\n", "sync"),
    ];
    for (snippet, needle) in cases {
        let text = format!("{account}{snippet}");
        let err = parse_accounts(&text, "accounts.kdl").expect_err(snippet);
        let shown = err.to_string();
        assert!(
            shown.to_lowercase().contains(&needle.to_lowercase()),
            "{snippet}: {shown}"
        );
        assert!(err.location().line >= 4, "{snippet}: {:?}", err.location());
    }
}

#[test]
fn the_writer_adds_changes_and_removes_sources_in_place() {
    let mut doc = AccountsDocument::parse(FILE, "accounts.kdl").unwrap();

    // Unchanged sources keep their formatting and comments.
    let mut changed = source(doc.config(), "work-contacts");
    if let PimSourceSettings::Carddav {
        refresh_minutes, ..
    } = &mut changed.settings
    {
        *refresh_minutes = Some(60);
    }
    doc.upsert_pim_source(changed).unwrap();
    assert!(
        doc.text()
            .contains("// Public holidays\ncalendar-subscription \"holidays\"")
    );
    assert!(
        doc.text()
            .contains("carddav \"work-contacts\" account=\"work\" url=\"https://dav.work.example/\" refresh=60\n"),
        "{}",
        doc.text()
    );

    // A new source is appended.
    let new = PimSourceConfig {
        id: doc.derive_pim_source_id("Team Calendar"),
        name: Some("Team".into()),
        enabled: true,
        settings: PimSourceSettings::Ics {
            url: "https://example.org/team.ics".into(),
            color: None,
            alarms: true,
            refresh_minutes: None,
            auth: None,
        },
    };
    assert_eq!(new.id.as_str(), "team-calendar");
    doc.upsert_pim_source(new.clone()).unwrap();
    assert!(
        doc.text().ends_with(
            "calendar-subscription \"team-calendar\" name=\"Team\" url=\"https://example.org/team.ics\" alarms=#true\n"
        ),
        "{}",
        doc.text()
    );
    assert_eq!(source(doc.config(), "team-calendar"), new);

    // Removing takes the attached comment along.
    assert!(
        doc.remove_pim_source(&PimSourceId::new("radicale"))
            .unwrap()
    );
    assert!(
        !doc.remove_pim_source(&PimSourceId::new("radicale"))
            .unwrap()
    );
    assert!(!doc.text().contains("Radicale"), "{}", doc.text());
    assert!(!doc.text().contains("caldav \"radicale\""));

    // The result reads back to the same configuration.
    let reread = parse_accounts(doc.text(), "accounts.kdl").unwrap();
    assert_eq!(&reread, doc.config());
}

#[test]
fn removing_an_account_removes_its_dav_sources() {
    let mut doc = AccountsDocument::parse(FILE, "accounts.kdl").unwrap();
    assert!(doc.remove_account(&AccountId::new("work")).unwrap());
    let ids: Vec<&str> = doc
        .config()
        .pim_sources
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(ids, ["holidays", "radicale"]);
    parse_accounts(doc.text(), "accounts.kdl").unwrap();
}

#[test]
fn derived_ids_avoid_local_and_taken_names() {
    let doc = AccountsDocument::parse(FILE, "accounts.kdl").unwrap();
    assert_eq!(doc.derive_pim_source_id("Local").as_str(), "local-2");
    assert_eq!(doc.derive_pim_source_id("holidays").as_str(), "holidays-2");
}

#[test]
fn the_schema_describes_the_source_nodes() {
    let schema = serde_json::to_string(&nail_config::accounts_schema()).unwrap();
    for needle in [
        "calendar-subscription",
        "caldav",
        "carddav",
        "\"refresh\"",
        "\"alarms\"",
    ] {
        assert!(schema.contains(needle), "schema lacks {needle}");
    }
}
