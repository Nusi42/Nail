//! nail-dav against real CalDAV / CardDAV servers (plan M3a WP72):
//! Radicale in the CI job "interop" (`NAIL_INTEROP_DAV`, see `src/lib.rs`).
//! The same checks also run against the mock server of nail-testkit-net
//! (over TLS), so they are known to work before CI reaches a real server.

use nail_api::ErrorCode;
use nail_api::engine::BoxFuture;
use nail_api::pim::calendar::CalendarComponent;
use nail_dav::{
    CollectionKind, CollectionState, DavAuth, DavClient, DavError, DavService, DeleteOutcome,
    DiscoveryInput, NewCollection, Precondition, ServerVersion, SyncOptions, TextMatch, TimeRange,
    WriteOutcome, delete_object, discover, member_url, sync_collection, write_object,
};
use nail_discovery::{DnsLookup, MxRecord, NetError, SrvRecord};
use nail_interop::{DavServer, Interop, from_env, unique};

/// No DNS: the tests start from the configured URL.
#[derive(Debug)]
struct NoDns;

impl DnsLookup for NoDns {
    fn srv<'a>(&'a self, _name: &'a str) -> BoxFuture<'a, Result<Vec<SrvRecord>, NetError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn mx<'a>(&'a self, _domain: &'a str) -> BoxFuture<'a, Result<Vec<MxRecord>, NetError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

fn client(interop: &Interop, password: &str) -> DavClient {
    DavClient::with_roots(
        &interop.roots,
        DavAuth::Basic {
            username: interop.user.clone(),
            password: password.to_owned(),
        },
    )
    .expect("DAV client")
}

fn event(uid: &str, start: &str, summary: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nail//Interop//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n\
         DTSTAMP:20261001T000000Z\r\nDTSTART:{start}\r\nDURATION:PT1H\r\nSUMMARY:{summary}\r\n\
         END:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

fn card(uid: &str, name: &str) -> String {
    format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid}\r\nFN:{name}\r\nN:{name};;;;\r\n\
         EMAIL;TYPE=INTERNET:{uid}@example.org\r\nEND:VCARD\r\n"
    )
}

async fn home(interop: &Interop, server: &DavServer, service: DavService) -> url::Url {
    let found = discover(
        &client(interop, &interop.password),
        &NoDns,
        &DiscoveryInput {
            service,
            url: Some(server.url.clone()),
            email: None,
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{} {service:?} discovery: {e}", server.name));
    found
        .home_sets
        .first()
        .cloned()
        .unwrap_or_else(|| panic!("{}: no {service:?} home set", server.name))
}

async fn check_caldav(interop: &Interop, server: &DavServer) {
    let name = &server.name;
    let ours = client(interop, &interop.password);
    let other = client(interop, &interop.password);
    let home = home(interop, server, DavService::CalDav).await;
    let calendar = home.join(&format!("{}/", unique("nail-cal"))).unwrap();
    ours.mkcalendar(
        &calendar,
        &NewCollection {
            display_name: "Nail interop".into(),
            description: None,
            color: Some("#3366ccff".into()),
            components: vec![CalendarComponent::Event, CalendarComponent::Task],
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{name} MKCALENDAR: {e}"));
    let created = ours.collection(&calendar).await.unwrap();
    assert_eq!(created.name(), "Nail interop", "{name}");
    assert!(
        created.components.contains(&CalendarComponent::Event),
        "{name}"
    );
    assert!(!created.read_only, "{name}");

    let initial = sync_collection(
        &ours,
        &calendar,
        CollectionKind::Calendar,
        &CollectionState::default(),
        SyncOptions::default(),
    )
    .await
    .unwrap_or_else(|e| panic!("{name} initial sync: {e}"));
    assert!(initial.changed.is_empty(), "{name}");

    let uid = unique("event");
    let item = member_url(&calendar, &uid, CollectionKind::Calendar);
    let WriteOutcome::Written { etag, .. } = write_object(
        &ours,
        &item,
        CollectionKind::Calendar,
        &event(&uid, "20261005T100000Z", "first"),
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("{name} PUT: {e}")) else {
        panic!("{name}: the new event conflicted");
    };
    let etag = etag.unwrap_or_else(|| panic!("{name}: no ETag after PUT"));

    let october = TimeRange::new("20261001T000000Z", "20261101T000000Z").unwrap();
    let hits = ours
        .calendar_query(&calendar, CalendarComponent::Event, Some(&october), true)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1, "{name}: {hits:?}");
    assert!(hits[0].data.as_deref().unwrap().contains(&uid), "{name}");
    let november = TimeRange::new("20261101T000000Z", "20261201T000000Z").unwrap();
    assert!(
        ours.calendar_query(&calendar, CalendarComponent::Event, Some(&november), false)
            .await
            .unwrap()
            .is_empty(),
        "{name}"
    );

    // Another client changes the event; our sync sees it.
    let after_create = sync_collection(
        &ours,
        &calendar,
        CollectionKind::Calendar,
        &initial.state,
        SyncOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(after_create.changed.len(), 1, "{name}");
    other
        .put(
            &item,
            CollectionKind::Calendar,
            &event(&uid, "20261006T100000Z", "theirs"),
            &Precondition::IfMatch(etag.clone()),
        )
        .await
        .unwrap_or_else(|e| panic!("{name} second client's PUT: {e}"));
    let after_change = sync_collection(
        &ours,
        &calendar,
        CollectionKind::Calendar,
        &after_create.state,
        SyncOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(after_change.changed.len(), 1, "{name}");
    assert!(
        after_change.changed[0]
            .data
            .as_deref()
            .unwrap()
            .contains("theirs"),
        "{name}"
    );

    // Our stale update loses: the server's version comes back.
    let stale = write_object(
        &ours,
        &item,
        CollectionKind::Calendar,
        &event(&uid, "20261007T100000Z", "mine"),
        Some(&etag),
    )
    .await
    .unwrap();
    let WriteOutcome::Conflict(ServerVersion::Current(server_copy)) = stale else {
        panic!("{name}: expected a conflict, got {stale:?}");
    };
    assert!(
        server_copy.data.as_deref().unwrap().contains("theirs"),
        "{name}"
    );

    // Delete with the current ETag; the next sync reports it removed.
    let current = server_copy
        .etag
        .clone()
        .or_else(|| after_change.state.etags.values().next().cloned());
    assert_eq!(
        delete_object(&ours, &item, current.as_deref())
            .await
            .unwrap(),
        DeleteOutcome::Deleted,
        "{name}"
    );
    let after_delete = sync_collection(
        &ours,
        &calendar,
        CollectionKind::Calendar,
        &after_change.state,
        SyncOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(after_delete.removed.len(), 1, "{name}");
    assert!(after_delete.state.etags.is_empty(), "{name}");

    // A token the server does not know restarts from an initial sync.
    let unknown = CollectionState {
        sync_token: Some(format!(
            "{}-bogus",
            after_delete.state.sync_token.clone().unwrap_or_default()
        )),
        ..after_delete.state.clone()
    };
    let restarted = sync_collection(
        &ours,
        &calendar,
        CollectionKind::Calendar,
        &unknown,
        SyncOptions::default(),
    )
    .await
    .unwrap_or_else(|e| panic!("{name} sync with an unknown token: {e}"));
    assert!(restarted.token_reset, "{name}");
    assert!(
        restarted.changed.is_empty() && restarted.removed.is_empty(),
        "{name}"
    );

    ours.delete(&calendar, &Precondition::None)
        .await
        .unwrap_or_else(|e| panic!("{name} DELETE calendar: {e}"));
}

async fn check_carddav(interop: &Interop, server: &DavServer) {
    let name = &server.name;
    let ours = client(interop, &interop.password);
    let home = home(interop, server, DavService::CardDav).await;
    let book = home.join(&format!("{}/", unique("nail-book"))).unwrap();
    ours.mkaddressbook(
        &book,
        &NewCollection {
            display_name: "Nail interop contacts".into(),
            ..NewCollection::default()
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{name} MKCOL: {e}"));
    let created = ours.collection(&book).await.unwrap();
    assert_eq!(created.kind, CollectionKind::AddressBook, "{name}");

    let mut hrefs = Vec::new();
    for (uid, full) in [
        (unique("ada"), "Ada Lovelace"),
        (unique("bob"), "Bob Builder"),
    ] {
        let item = member_url(&book, &uid, CollectionKind::AddressBook);
        let outcome = write_object(
            &ours,
            &item,
            CollectionKind::AddressBook,
            &card(&uid, full),
            None,
        )
        .await
        .unwrap_or_else(|e| panic!("{name} PUT vCard: {e}"));
        assert!(matches!(outcome, WriteOutcome::Written { .. }), "{name}");
        hrefs.push(item.path().to_owned());
    }
    let found = ours
        .addressbook_query(
            &book,
            Some(&TextMatch {
                property: "FN".into(),
                text: "lovelace".into(),
            }),
            true,
        )
        .await
        .unwrap();
    assert_eq!(found.len(), 1, "{name}: {found:?}");
    assert!(
        found[0].data.as_deref().unwrap().contains("Ada Lovelace"),
        "{name}"
    );

    let mut wanted = hrefs.clone();
    wanted.push(format!("{}missing.vcf", book.path()));
    let got = ours
        .multiget(&book, CollectionKind::AddressBook, &wanted)
        .await
        .unwrap();
    assert_eq!(got.objects.len(), 2, "{name}");

    let synced = sync_collection(
        &ours,
        &book,
        CollectionKind::AddressBook,
        &CollectionState::default(),
        SyncOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(synced.changed.len(), 2, "{name}");
    // The ctag / ETag path works on the same server.
    let by_etags = sync_collection(
        &ours,
        &book,
        CollectionKind::AddressBook,
        &CollectionState::default(),
        SyncOptions {
            use_sync_collection: false,
            ..SyncOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(by_etags.changed.len(), 2, "{name}");

    ours.delete(&book, &Precondition::None)
        .await
        .unwrap_or_else(|e| panic!("{name} DELETE address book: {e}"));
}

async fn check_wrong_password(interop: &Interop, server: &DavServer) {
    let err = discover(
        &client(interop, "definitely-wrong"),
        &NoDns,
        &DiscoveryInput {
            service: DavService::CalDav,
            url: Some(server.url.clone()),
            email: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err, DavError::AuthFailed, "{}", server.name);
    assert_eq!(err.code(), ErrorCode::AuthFailed);
}

#[tokio::test]
async fn caldav_events_sync_and_conflicts() {
    let Some(interop) = from_env() else { return };
    for server in &interop.dav {
        check_caldav(&interop, server).await;
    }
}

#[tokio::test]
async fn carddav_contacts_query_and_sync() {
    let Some(interop) = from_env() else { return };
    for server in &interop.dav {
        check_carddav(&interop, server).await;
    }
}

#[tokio::test]
async fn dav_rejects_a_wrong_password() {
    let Some(interop) = from_env() else { return };
    for server in &interop.dav {
        check_wrong_password(&interop, server).await;
    }
}

#[tokio::test]
async fn dav_checks_pass_against_the_mock_server() {
    let mock = nail_testkit_net::MockDavServer::start(nail_testkit_net::MockDavConfig {
        tls: true,
        username: "ada@example.org".into(),
        password: "nail-interop".into(),
        ..nail_testkit_net::MockDavConfig::default()
    })
    .await
    .unwrap();
    let interop = Interop {
        servers: Vec::new(),
        dav: vec![DavServer {
            name: "mock".into(),
            url: mock.base_url(),
        }],
        roots: vec![mock.ca_der().unwrap().to_vec()],
        user: "ada@example.org".into(),
        password: "nail-interop".into(),
    };
    let server = interop.dav[0].clone();
    check_caldav(&interop, &server).await;
    check_carddav(&interop, &server).await;
    check_wrong_password(&interop, &server).await;
}
