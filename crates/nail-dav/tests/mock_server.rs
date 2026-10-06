//! nail-dav against the mock CalDAV / CardDAV server of nail-testkit-net
//! (loopback only): discovery, queries, multiget, sync (sync-collection,
//! paging, token resets, ctag / ETag fallback), conditional writes and
//! conflicts, collection management, authentication, TLS and failures.
#![cfg(test)]

use std::sync::Arc;
use std::sync::Mutex;

use nail_api::ErrorCode;
use nail_api::accounts::SecretRef;
use nail_api::engine::{AccessTokenProvider, BoxFuture, EngineError};
use nail_api::pim::calendar::CalendarComponent;
use nail_dav::{
    CollectionKind, CollectionState, DavAuth, DavClient, DavError, DavService, DeleteOutcome,
    DiscoveryInput, NewCollection, Precondition, ServerVersion, SyncMethod, SyncOptions, TextMatch,
    TimeRange, WriteOutcome, delete_object, discover, href_key, member_url, sync_collection,
    write_object,
};
use nail_discovery::{DnsLookup, MxRecord, NetError, SrvRecord};
use nail_testkit_net::{DnsZone, MockDavConfig, MockDavServer, ZoneAnswer};
use pretty_assertions::assert_eq;
use url::Url;

fn basic() -> DavAuth {
    DavAuth::Basic {
        username: "ada".into(),
        password: "secret".into(),
    }
}

fn client(auth: DavAuth) -> DavClient {
    DavClient::new(auth).unwrap()
}

async fn server() -> MockDavServer {
    MockDavServer::start(MockDavConfig::default())
        .await
        .unwrap()
}

fn url(server: &MockDavServer, path: &str) -> Url {
    Url::parse(&server.url(path)).unwrap()
}

fn event(uid: &str, start: &str, end: &str, extra: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nail//Test//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n\
         DTSTAMP:20261001T000000Z\r\nDTSTART:{start}\r\nDTEND:{end}\r\n{extra}SUMMARY:{uid}\r\n\
         END:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

fn todo(uid: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nail//Test//EN\r\nBEGIN:VTODO\r\nUID:{uid}\r\n\
         DTSTAMP:20261001T000000Z\r\nDUE:20261005T120000Z\r\nSUMMARY:{uid}\r\nEND:VTODO\r\nEND:VCALENDAR\r\n"
    )
}

fn card(uid: &str, name: &str, email: &str) -> String {
    format!(
        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:{uid}\r\nFN:{name}\r\nEMAIL:{email}\r\nEND:VCARD\r\n"
    )
}

/// [`DnsLookup`] over an in-memory [`DnsZone`].
#[derive(Debug)]
struct ZoneDns(DnsZone, Mutex<Vec<String>>);

impl DnsLookup for ZoneDns {
    fn srv<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<SrvRecord>, NetError>> {
        Box::pin(async move {
            self.1.lock().unwrap().push(name.to_owned());
            match self.0.srv(name) {
                ZoneAnswer::Records(records) => Ok(records
                    .into_iter()
                    .map(|r| SrvRecord {
                        priority: r.priority,
                        weight: r.weight,
                        port: r.port,
                        target: r.target.trim_end_matches('.').to_owned(),
                    })
                    .collect()),
                ZoneAnswer::NxDomain => Err(NetError("NXDOMAIN".into())),
                ZoneAnswer::ServFail => Err(NetError("SERVFAIL".into())),
            }
        })
    }

    fn mx<'a>(&'a self, _domain: &'a str) -> BoxFuture<'a, Result<Vec<MxRecord>, NetError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

fn no_dns() -> ZoneDns {
    ZoneDns(DnsZone::new(), Mutex::new(Vec::new()))
}

// ---------------------------------------------------------------------------
// Discovery

#[tokio::test]
async fn discovers_calendars_from_the_server_url() {
    let server = server().await;
    let found = discover(
        &client(basic()),
        &no_dns(),
        &DiscoveryInput {
            service: DavService::CalDav,
            url: Some(server.base_url()),
            email: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(found.principal.path(), server.principal_path());
    assert_eq!(found.home_sets.len(), 1);
    assert_eq!(found.home_sets[0].path(), server.calendar_home());
    let mut names: Vec<String> = found
        .collections
        .iter()
        .map(nail_dav::DavCollection::name)
        .collect();
    names.sort();
    assert_eq!(names, ["Holidays", "Personal", "Tasks"]);
    let personal = found
        .collections
        .iter()
        .find(|c| c.name() == "Personal")
        .unwrap();
    assert_eq!(
        personal.components,
        [CalendarComponent::Event, CalendarComponent::Task]
    );
    assert!(!personal.read_only && personal.supports_sync);
    assert!(personal.ctag.is_some() && personal.sync_token.is_some());
    let shown = personal.to_discovered();
    assert_eq!(shown.color.as_deref(), Some("#3366ccff"));
    let tasks = found
        .collections
        .iter()
        .find(|c| c.name() == "Tasks")
        .unwrap();
    assert_eq!(tasks.components, [CalendarComponent::Task]);
    let holidays = found
        .collections
        .iter()
        .find(|c| c.name() == "Holidays")
        .unwrap();
    assert!(holidays.read_only);
    // The root has no principal of its own; the first PROPFIND went there
    // with depth 0 and credentials.
    let first = &server.requests()[0];
    assert_eq!(first.method, "PROPFIND");
    assert_eq!(first.depth.as_deref(), Some("0"));
    assert!(
        first
            .authorization
            .as_deref()
            .unwrap()
            .starts_with("Basic ")
    );
}

#[tokio::test]
async fn discovers_address_books_through_srv_and_well_known() {
    let server = server().await;
    let mut zone = DnsZone::new();
    zone.add_srv("_carddavs._tcp.example.org", 0, 1, 443, "dav.example.org");
    let dns = ZoneDns(zone, Mutex::new(Vec::new()));
    let client = client(basic()).route_all_to(server.addr());
    let found = discover(
        &client,
        &dns,
        &DiscoveryInput {
            service: DavService::CardDav,
            url: None,
            email: Some("ada@example.org".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        dns.1.lock().unwrap().as_slice(),
        ["_carddavs._tcp.example.org"]
    );
    assert_eq!(
        found.context_url.as_str(),
        "https://dav.example.org/dav/",
        "the well-known redirect was followed on the logical host"
    );
    assert_eq!(found.collections.len(), 1);
    assert_eq!(found.collections[0].kind, CollectionKind::AddressBook);
    assert_eq!(found.collections[0].name(), "Contacts");
    let requests = server.requests();
    assert_eq!(requests[0].path, "/.well-known/carddav");
    assert_eq!(requests[1].path, "/dav/");
}

#[tokio::test]
async fn discovery_falls_back_to_the_domain_well_known() {
    let server = server().await;
    let client = client(basic()).route_all_to(server.addr());
    let found = discover(
        &client,
        &no_dns(),
        &DiscoveryInput {
            service: DavService::CalDav,
            url: None,
            email: Some("ada@example.org".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(found.context_url.as_str(), "https://example.org/dav/");
    assert_eq!(found.collections.len(), 3);
}

#[tokio::test]
async fn discovery_accepts_a_collection_url() {
    let server = server().await;
    let found = discover(
        &client(basic()),
        &no_dns(),
        &DiscoveryInput {
            service: DavService::CalDav,
            url: Some(server.url(&server.calendar_path("tasks"))),
            email: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(found.collections.len(), 1);
    assert_eq!(found.collections[0].name(), "Tasks");
    assert_eq!(found.principal.path(), server.principal_path());
}

#[tokio::test]
async fn discovery_stops_at_a_refused_login() {
    let server = server().await;
    let wrong = DavAuth::Basic {
        username: "ada".into(),
        password: "wrong".into(),
    };
    let err = discover(
        &client(wrong),
        &no_dns(),
        &DiscoveryInput {
            service: DavService::CalDav,
            url: Some(server.base_url()),
            email: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err, DavError::AuthFailed);
    assert_eq!(err.code(), ErrorCode::AuthFailed);
    assert_eq!(
        server.requests().len(),
        1,
        "no further candidates after a 401"
    );
}

#[tokio::test]
async fn discovery_reports_no_service() {
    let server = MockDavServer::start(MockDavConfig {
        well_known: false,
        ..MockDavConfig::default()
    })
    .await
    .unwrap();
    let err = discover(
        &client(basic()),
        &no_dns(),
        &DiscoveryInput {
            service: DavService::CalDav,
            url: Some(server.url("/nothing/here/")),
            email: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DavError::NoService(_)), "{err:?}");
    assert_eq!(err.localized().id, "dav-not-found");
    let paths: Vec<String> = server.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(paths, ["/nothing/here/", "/.well-known/caldav"]);

    let none = discover(
        &client(basic()),
        &no_dns(),
        &DiscoveryInput {
            service: DavService::CalDav,
            url: None,
            email: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(none, DavError::NoService(_)));
}

#[tokio::test]
async fn insecure_urls_are_refused() {
    let err = discover(
        &client(basic()),
        &no_dns(),
        &DiscoveryInput {
            service: DavService::CalDav,
            url: Some("http://dav.example.org/".into()),
            email: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DavError::InsecureUrl(_)));
    assert_eq!(err.code(), ErrorCode::Config);
    // Even when routed to a mock, a logical http: URL to a remote host
    // never carries credentials.
    let server = server().await;
    let routed = client(basic()).route_all_to(server.addr());
    let err = routed
        .collection(&Url::parse("http://dav.example.org/x/").unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::InsecureUrl(_)));
    assert_eq!(server.requests().len(), 0);
}

// ---------------------------------------------------------------------------
// Authentication and transport

#[tokio::test]
async fn tls_with_basic_auth() {
    let server = MockDavServer::start(MockDavConfig {
        tls: true,
        ..MockDavConfig::default()
    })
    .await
    .unwrap();
    assert!(server.base_url().starts_with("https://localhost:"));
    let roots = vec![server.ca_der().unwrap().to_vec()];
    let client = DavClient::with_roots(&roots, basic()).unwrap();
    let personal = client
        .collection(&url(&server, &server.calendar_path("personal")))
        .await
        .unwrap();
    assert_eq!(personal.name(), "Personal");

    // A client that does not trust the test CA fails with a TLS error.
    let untrusted = DavClient::new(basic()).unwrap();
    let err = untrusted
        .collection(&url(&server, &server.calendar_path("personal")))
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Tls(_)), "{err:?}");
    assert_eq!(err.code(), ErrorCode::Tls);
}

#[derive(Debug, Default)]
struct Tokens {
    calls: Mutex<Vec<bool>>,
}

impl AccessTokenProvider for Tokens {
    fn access_token<'a>(
        &'a self,
        provider: &'a str,
        secret: &'a SecretRef,
        force_refresh: bool,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move {
            assert_eq!(provider, "google");
            assert_eq!(secret.as_str(), "keychain:nail/pim/g/oauth2");
            self.calls.lock().unwrap().push(force_refresh);
            if provider == "google" && force_refresh {
                Ok("fresh-token".to_owned())
            } else {
                Ok("stale-token".to_owned())
            }
        })
    }
}

#[tokio::test]
async fn oauth2_bearer_is_refreshed_once_after_401() {
    let server = MockDavServer::start(MockDavConfig {
        bearer_token: Some("fresh-token".into()),
        ..MockDavConfig::default()
    })
    .await
    .unwrap();
    let tokens = Arc::new(Tokens::default());
    let client = client(DavAuth::OAuth2 {
        tokens: tokens.clone(),
        provider: "google".into(),
        secret: SecretRef("keychain:nail/pim/g/oauth2".into()),
    });
    let book = client
        .collection(&url(&server, &server.address_book_path("contacts")))
        .await
        .unwrap();
    assert_eq!(book.kind, CollectionKind::AddressBook);
    assert_eq!(tokens.calls.lock().unwrap().as_slice(), [false, true]);
    let auths: Vec<Option<String>> = server
        .requests()
        .into_iter()
        .map(|r| r.authorization)
        .collect();
    assert_eq!(
        auths,
        [
            Some("Bearer stale-token".to_owned()),
            Some("Bearer fresh-token".to_owned())
        ]
    );

    // A fixed bearer token that is refused is an auth failure (no loop).
    let fixed = DavClient::new(DavAuth::Bearer("nope".into())).unwrap();
    let err = fixed
        .collection(&url(&server, &server.address_book_path("contacts")))
        .await
        .unwrap_err();
    assert_eq!(err, DavError::AuthFailed);
}

#[derive(Debug)]
struct RevokedTokens;

impl AccessTokenProvider for RevokedTokens {
    fn access_token<'a>(
        &'a self,
        _provider: &'a str,
        _secret: &'a SecretRef,
        _force_refresh: bool,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async { Err(EngineError::permanent(ErrorCode::AuthFailed, "revoked")) })
    }
}

#[tokio::test]
async fn token_errors_pass_through() {
    let server = server().await;
    let client = client(DavAuth::OAuth2 {
        tokens: Arc::new(RevokedTokens),
        provider: "google".into(),
        secret: SecretRef("keychain:nail/pim/g/oauth2".into()),
    });
    let err = client
        .collection(&url(&server, &server.calendar_path("personal")))
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Token(_)));
    let engine: EngineError = err.into();
    assert_eq!(engine.code, ErrorCode::AuthFailed);
    assert!(!engine.retryable);
    assert_eq!(server.requests().len(), 0);
}

#[tokio::test]
async fn server_failures_map_to_errors() {
    let server = server().await;
    let client = client(basic());
    let personal = url(&server, &server.calendar_path("personal"));
    server.fail_next("PROPFIND", 503);
    let err = client.collection(&personal).await.unwrap_err();
    assert_eq!(err, DavError::RateLimited(Some(1)));
    assert!(err.retryable());
    server.fail_next("*", 500);
    let err = client.collection(&personal).await.unwrap_err();
    assert!(matches!(err, DavError::Status { status: 500, .. }));
    assert!(err.retryable());
    let err = client
        .collection(&url(&server, "/dav/calendars/ada/missing/"))
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::NotFound(_)));
    // And the server works again.
    assert!(client.collection(&personal).await.is_ok());

    // A closed port is a network error, retryable.
    let addr = server.addr();
    drop(server);
    let err = client
        .collection(&Url::parse(&format!("http://{addr}/dav/")).unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Network(_)), "{err:?}");
    assert!(err.retryable());
}

// ---------------------------------------------------------------------------
// Queries and multiget

#[tokio::test]
async fn calendar_query_filters_by_component_and_time_range() {
    let server = server().await;
    let personal = server.calendar_path("personal");
    server.put_object(
        &personal,
        "in.ics",
        &event("in", "20261005T100000Z", "20261005T110000Z", ""),
    );
    server.put_object(
        &personal,
        "out.ics",
        &event("out", "20261105T100000Z", "20261105T110000Z", ""),
    );
    server.put_object(
        &personal,
        "weekly.ics",
        &event(
            "weekly",
            "20250105T100000Z",
            "20250105T110000Z",
            "RRULE:FREQ=WEEKLY\r\n",
        ),
    );
    server.put_object(
        &personal,
        "allday.ics",
        &event("allday", "20261010", "20261011", ""),
    );
    server.put_object(&personal, "task.ics", &todo("task"));
    let client = client(basic());
    let calendar = url(&server, &personal);
    let range = TimeRange::new("20261001T000000Z", "20261101T000000Z").unwrap();
    let mut found = client
        .calendar_query(&calendar, CalendarComponent::Event, Some(&range), true)
        .await
        .unwrap();
    found.sort_by(|a, b| a.href.cmp(&b.href));
    let names: Vec<&str> = found
        .iter()
        .map(|o| o.href.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(names, ["allday.ics", "in.ics", "weekly.ics"]);
    assert!(found.iter().all(|o| o.etag.is_some()));
    assert!(found[1].data.as_deref().unwrap().contains("UID:in"));

    let without_data = client
        .calendar_query(&calendar, CalendarComponent::Task, None, false)
        .await
        .unwrap();
    assert_eq!(without_data.len(), 1);
    assert_eq!(without_data[0].href, format!("{personal}task.ics"));
    assert_eq!(without_data[0].data, None);

    let report = server
        .requests()
        .into_iter()
        .find(|r| r.method == "REPORT")
        .unwrap();
    assert_eq!(report.depth.as_deref(), Some("1"));
    assert!(report.body.contains("start=\"20261001T000000Z\""));
}

#[tokio::test]
async fn addressbook_query_and_multiget() {
    let server = server().await;
    let contacts = server.address_book_path("contacts");
    server.put_object(
        &contacts,
        "ada.vcf",
        &card("ada", "Ada Lovelace", "ada@example.org"),
    );
    server.put_object(
        &contacts,
        "bob.vcf",
        &card("bob", "Bob & Co", "bob@example.org"),
    );
    let client = client(basic());
    let book = url(&server, &contacts);
    assert_eq!(
        client
            .addressbook_query(&book, None, false)
            .await
            .unwrap()
            .len(),
        2
    );
    let found = client
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
    assert_eq!(found.len(), 1);
    assert!(
        found[0]
            .data
            .as_deref()
            .unwrap()
            .contains("FN:Ada Lovelace")
    );

    let hrefs = vec![
        format!("{contacts}bob.vcf"),
        format!("{contacts}gone.vcf"),
        format!("{contacts}ada.vcf"),
    ];
    let got = client
        .multiget(&book, CollectionKind::AddressBook, &hrefs)
        .await
        .unwrap();
    assert_eq!(got.objects.len(), 2);
    let bob = got
        .objects
        .iter()
        .find(|o| o.href.ends_with("bob.vcf"))
        .unwrap();
    assert_eq!(
        bob.data.as_deref(),
        Some(card("bob", "Bob & Co", "bob@example.org").as_str())
    );
    assert_eq!(
        bob.etag.as_deref(),
        Some(server.object(&contacts, "bob.vcf").unwrap().etag.as_str())
    );
    assert_eq!(got.missing, [format!("{contacts}gone.vcf")]);
}

#[tokio::test]
async fn multiget_runs_in_batches() {
    let server = server().await;
    let personal = server.calendar_path("personal");
    let mut hrefs = Vec::new();
    for i in 0..(nail_dav::ops::MULTIGET_BATCH + 7) {
        let name = format!("e{i}.ics");
        server.put_object(
            &personal,
            &name,
            &event(&format!("e{i}"), "20261005T100000Z", "20261005T110000Z", ""),
        );
        hrefs.push(format!("{personal}{name}"));
    }
    let got = client(basic())
        .multiget(&url(&server, &personal), CollectionKind::Calendar, &hrefs)
        .await
        .unwrap();
    assert_eq!(got.objects.len(), hrefs.len());
    assert!(got.missing.is_empty(), "{:?}", got.missing);
    let reports = server
        .requests()
        .into_iter()
        .filter(|r| r.method == "REPORT")
        .count();
    assert_eq!(reports, 2);
}

// ---------------------------------------------------------------------------
// Sync

fn names(hrefs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = hrefs
        .iter()
        .map(|h| h.rsplit('/').next().unwrap().to_owned())
        .collect();
    out.sort();
    out
}

#[tokio::test]
async fn sync_collection_initial_incremental_and_reset() {
    let server = server().await;
    let personal = server.calendar_path("personal");
    server.put_object(
        &personal,
        "a.ics",
        &event("a", "20261005T100000Z", "20261005T110000Z", ""),
    );
    server.put_object(
        &personal,
        "b.ics",
        &event("b", "20261006T100000Z", "20261006T110000Z", ""),
    );
    let client = client(basic());
    let calendar = url(&server, &personal);

    let first = sync_collection(
        &client,
        &calendar,
        CollectionKind::Calendar,
        &CollectionState::default(),
        SyncOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(first.method, SyncMethod::SyncCollection);
    assert!(!first.token_reset);
    let changed: Vec<String> = first.changed.iter().map(|o| o.href.clone()).collect();
    assert_eq!(names(&changed), ["a.ics", "b.ics"]);
    assert!(first.removed.is_empty(), "{:?}", first.removed);
    assert!(first.state.sync_token.is_some());
    assert_eq!(first.state.etags.len(), 2);

    // Nothing changed: no multiget.
    server.clear_requests();
    let quiet = sync_collection(
        &client,
        &calendar,
        CollectionKind::Calendar,
        &first.state,
        SyncOptions::default(),
    )
    .await
    .unwrap();
    assert!(quiet.changed.is_empty() && quiet.removed.is_empty());
    assert_eq!(server.requests().len(), 1);

    // Another client changes b, deletes a, adds c (and adds and deletes d).
    server.put_object(
        &personal,
        "b.ics",
        &event("b", "20261007T100000Z", "20261007T110000Z", ""),
    );
    server.delete_object(&personal, "a.ics");
    server.put_object(&personal, "c.ics", &todo("c"));
    server.put_object(&personal, "d.ics", &todo("d"));
    server.delete_object(&personal, "d.ics");
    let second = sync_collection(
        &client,
        &calendar,
        CollectionKind::Calendar,
        &quiet.state,
        SyncOptions::default(),
    )
    .await
    .unwrap();
    let changed: Vec<String> = second.changed.iter().map(|o| o.href.clone()).collect();
    assert_eq!(names(&changed), ["b.ics", "c.ics"]);
    assert_eq!(names(&second.removed), ["a.ics"], "d was never known");
    assert!(
        second
            .changed
            .iter()
            .any(|o| o.data.as_deref().unwrap().contains("20261007T100000Z"))
    );
    assert_eq!(
        second.state.etags.keys().cloned().collect::<Vec<_>>(),
        [format!("{personal}b.ics"), format!("{personal}c.ics")]
    );

    // The server loses its history: the old token is refused, the client
    // lists everything again and only fetches what changed.
    server.forget_sync_history();
    server.delete_object(&personal, "c.ics");
    server.put_object(&personal, "e.ics", &todo("e"));
    server.clear_requests();
    let third = sync_collection(
        &client,
        &calendar,
        CollectionKind::Calendar,
        &second.state,
        SyncOptions::default(),
    )
    .await
    .unwrap();
    assert!(third.token_reset);
    let changed: Vec<String> = third.changed.iter().map(|o| o.href.clone()).collect();
    assert_eq!(
        names(&changed),
        ["e.ics"],
        "b is unchanged and not fetched again"
    );
    assert_eq!(names(&third.removed), ["c.ics"]);
    assert_eq!(third.state.etags.len(), 2);
    let reports: Vec<String> = server
        .requests()
        .into_iter()
        .filter(|r| r.method == "REPORT")
        .map(|r| r.body)
        .collect();
    assert_eq!(reports.len(), 3, "refused sync, initial sync, multiget");
    assert!(reports[1].contains("<d:sync-token></d:sync-token>"));
}

#[tokio::test]
async fn sync_collection_pages_through_truncated_answers() {
    let server = MockDavServer::start(MockDavConfig {
        sync_page_size: Some(2),
        ..MockDavConfig::default()
    })
    .await
    .unwrap();
    let contacts = server.address_book_path("contacts");
    for i in 0..5 {
        server.put_object(
            &contacts,
            &format!("c{i}.vcf"),
            &card(&format!("c{i}"), "X", "x@example.org"),
        );
    }
    let client = client(basic());
    let outcome = sync_collection(
        &client,
        &url(&server, &contacts),
        CollectionKind::AddressBook,
        &CollectionState::default(),
        SyncOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.changed.len(), 5);
    let syncs = server
        .requests()
        .into_iter()
        .filter(|r| r.body.contains("sync-collection"))
        .count();
    assert_eq!(syncs, 3);

    // A client-side limit works the same way.
    server.put_object(&contacts, "c9.vcf", &card("c9", "Y", "y@example.org"));
    let next = sync_collection(
        &client,
        &url(&server, &contacts),
        CollectionKind::AddressBook,
        &outcome.state,
        SyncOptions {
            page_limit: Some(1),
            ..SyncOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        names(
            &next
                .changed
                .iter()
                .map(|o| o.href.clone())
                .collect::<Vec<_>>()
        ),
        ["c9.vcf"]
    );
}

#[tokio::test]
async fn ctag_and_etag_fallback_without_sync_collection() {
    let server = MockDavServer::start(MockDavConfig {
        sync_collection: false,
        ..MockDavConfig::default()
    })
    .await
    .unwrap();
    let tasks = server.calendar_path("tasks");
    server.put_object(&tasks, "t1.ics", &todo("t1"));
    server.put_object(&tasks, "t2.ics", &todo("t2"));
    let client = client(basic());
    let calendar = url(&server, &tasks);
    let found = client.collection(&calendar).await.unwrap();
    assert!(!found.supports_sync && found.sync_token.is_none());

    // The client tries sync-collection, the server refuses, ETags take over.
    let first = sync_collection(
        &client,
        &calendar,
        CollectionKind::Calendar,
        &CollectionState::default(),
        SyncOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(first.method, SyncMethod::Etags);
    assert_eq!(first.changed.len(), 2);
    assert!(first.state.ctag.is_some() && first.state.sync_token.is_none());

    // Unchanged ctag: one PROPFIND, nothing else.
    server.clear_requests();
    let options = SyncOptions {
        use_sync_collection: false,
        ..SyncOptions::default()
    };
    let same = sync_collection(
        &client,
        &calendar,
        CollectionKind::Calendar,
        &first.state,
        options,
    )
    .await
    .unwrap();
    assert_eq!(same.method, SyncMethod::Unchanged);
    assert_eq!(same.state, first.state);
    assert_eq!(server.requests().len(), 1);

    server.put_object(&tasks, "t2.ics", &todo("t2-changed"));
    server.delete_object(&tasks, "t1.ics");
    server.put_object(&tasks, "t3.ics", &todo("t3"));
    let next = sync_collection(
        &client,
        &calendar,
        CollectionKind::Calendar,
        &same.state,
        options,
    )
    .await
    .unwrap();
    assert_eq!(next.method, SyncMethod::Etags);
    let changed: Vec<String> = next.changed.iter().map(|o| o.href.clone()).collect();
    assert_eq!(names(&changed), ["t2.ics", "t3.ics"]);
    assert_eq!(names(&next.removed), ["t1.ics"]);
    assert_eq!(next.state.etags.len(), 2);
    assert_ne!(next.state.ctag, same.state.ctag);
}

// ---------------------------------------------------------------------------
// Writes and conflicts

#[tokio::test]
async fn conditional_writes_and_server_wins_conflicts() {
    let server = server().await;
    let personal = server.calendar_path("personal");
    let client = client(basic());
    let calendar = url(&server, &personal);
    let item = member_url(&calendar, "meeting-1@nail", CollectionKind::Calendar);
    assert_eq!(item.path(), format!("{personal}meeting-1@nail.ics"));
    let v1 = event("meeting-1@nail", "20261005T100000Z", "20261005T110000Z", "");

    // Create.
    let WriteOutcome::Written {
        etag: Some(etag1),
        data: None,
    } = write_object(&client, &item, CollectionKind::Calendar, &v1, None)
        .await
        .unwrap()
    else {
        panic!("not written");
    };
    let put = server
        .requests()
        .into_iter()
        .rfind(|r| r.method == "PUT")
        .unwrap();
    assert_eq!(put.if_none_match.as_deref(), Some("*"));
    assert!(put.content_type.unwrap().starts_with("text/calendar"));
    assert_eq!(
        server.object(&personal, "meeting-1@nail.ics").unwrap().data,
        v1
    );

    // Creating it again conflicts with the existing item.
    let again = write_object(&client, &item, CollectionKind::Calendar, &v1, None)
        .await
        .unwrap();
    assert!(matches!(
        again,
        WriteOutcome::Conflict(ServerVersion::Current(_))
    ));

    // Update with the right ETag.
    let v2 = event("meeting-1@nail", "20261005T120000Z", "20261005T130000Z", "");
    let WriteOutcome::Written {
        etag: Some(etag2), ..
    } = write_object(&client, &item, CollectionKind::Calendar, &v2, Some(&etag1))
        .await
        .unwrap()
    else {
        panic!("not written");
    };
    assert_ne!(etag1, etag2);
    let put = server
        .requests()
        .into_iter()
        .rfind(|r| r.method == "PUT")
        .unwrap();
    assert_eq!(put.if_match.as_deref(), Some(etag1.as_str()));

    // Another client changes it; our stale update loses (server wins).
    let theirs = event("meeting-1@nail", "20261005T150000Z", "20261005T160000Z", "");
    let their_etag = server
        .put_object(&personal, "meeting-1@nail.ics", &theirs)
        .unwrap();
    let v3 = event("meeting-1@nail", "20261005T080000Z", "20261005T090000Z", "");
    let outcome = write_object(&client, &item, CollectionKind::Calendar, &v3, Some(&etag2))
        .await
        .unwrap();
    let WriteOutcome::Conflict(ServerVersion::Current(server_copy)) = outcome else {
        panic!("expected a conflict, got {outcome:?}");
    };
    assert_eq!(server_copy.data.as_deref(), Some(theirs.as_str()));
    assert_eq!(server_copy.etag.as_deref(), Some(their_etag.as_str()));
    assert_eq!(
        server.object(&personal, "meeting-1@nail.ics").unwrap().data,
        theirs
    );

    // A stale delete loses too; the server's version stays.
    let outcome = delete_object(&client, &item, Some(&etag2)).await.unwrap();
    assert!(
        matches!(outcome, DeleteOutcome::Conflict(ref o) if o.etag.as_deref() == Some(their_etag.as_str()))
    );
    assert!(server.object(&personal, "meeting-1@nail.ics").is_some());

    // Deleted on the server while we edited: the deletion wins.
    server.delete_object(&personal, "meeting-1@nail.ics");
    let outcome = write_object(
        &client,
        &item,
        CollectionKind::Calendar,
        &v3,
        Some(&their_etag),
    )
    .await
    .unwrap();
    assert_eq!(outcome, WriteOutcome::Conflict(ServerVersion::Deleted));
    assert_eq!(
        delete_object(&client, &item, Some(&their_etag))
            .await
            .unwrap(),
        DeleteOutcome::AlreadyGone
    );
    assert_eq!(
        delete_object(&client, &item, None).await.unwrap(),
        DeleteOutcome::AlreadyGone
    );

    // A current delete works.
    let etag = server.put_object(&personal, "x.ics", &todo("x")).unwrap();
    let x = url(&server, &format!("{personal}x.ics"));
    assert_eq!(
        delete_object(&client, &x, Some(&etag)).await.unwrap(),
        DeleteOutcome::Deleted
    );
    assert!(server.object(&personal, "x.ics").is_none());
}

#[tokio::test]
async fn writes_without_etag_read_the_item_back() {
    let server = MockDavServer::start(MockDavConfig {
        etag_on_put: false,
        ..MockDavConfig::default()
    })
    .await
    .unwrap();
    let contacts = server.address_book_path("contacts");
    let client = client(basic());
    let item = member_url(&url(&server, &contacts), "ada", CollectionKind::AddressBook);
    let vcard = card("ada", "Ada", "ada@example.org");
    let outcome = write_object(&client, &item, CollectionKind::AddressBook, &vcard, None)
        .await
        .unwrap();
    let stored = server.object(&contacts, "ada.vcf").unwrap();
    assert_eq!(
        outcome,
        WriteOutcome::Written {
            etag: Some(stored.etag),
            data: Some(vcard)
        }
    );
    let put = server
        .requests()
        .into_iter()
        .find(|r| r.method == "PUT")
        .unwrap();
    assert!(put.content_type.unwrap().starts_with("text/vcard"));
}

#[tokio::test]
async fn read_only_and_invalid_writes_are_refused() {
    let server = server().await;
    let client = client(basic());
    let holidays = url(&server, &server.calendar_path("holidays"));
    let item = member_url(&holidays, "h1", CollectionKind::Calendar);
    let err = write_object(&client, &item, CollectionKind::Calendar, &todo("h1"), None)
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Forbidden(_)));
    assert_eq!(err.code(), ErrorCode::Forbidden);
    // A vCard into a calendar is refused by the server.
    let personal = url(&server, &server.calendar_path("personal"));
    let wrong = member_url(&personal, "c", CollectionKind::Calendar);
    let err = client
        .put(
            &wrong,
            CollectionKind::AddressBook,
            &card("c", "C", "c@example.org"),
            &Precondition::None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Forbidden(_)));
}

// ---------------------------------------------------------------------------
// Collections

#[tokio::test]
async fn create_rename_and_delete_collections() {
    let server = server().await;
    let client = client(basic());
    let home = url(&server, &server.calendar_home());
    let work = home.join("work/").unwrap();
    client
        .mkcalendar(
            &work,
            &NewCollection {
                display_name: "Work & Co".into(),
                description: Some("Office".into()),
                color: Some("#00ff00".into()),
                components: vec![CalendarComponent::Event],
            },
        )
        .await
        .unwrap();
    let created = client.collection(&work).await.unwrap();
    assert_eq!(created.name(), "Work & Co");
    assert_eq!(created.color.as_deref(), Some("#00ff00"));
    assert_eq!(created.components, [CalendarComponent::Event]);
    // Again: the server refuses.
    let err = client
        .mkcalendar(&work, &NewCollection::default())
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Status { status: 405, .. }));

    client
        .proppatch(&work, Some("Job"), Some("#123456"))
        .await
        .unwrap();
    let renamed = client.collection(&work).await.unwrap();
    assert_eq!(
        (renamed.name().as_str(), renamed.color.as_deref()),
        ("Job", Some("#123456"))
    );
    client.proppatch(&work, None, None).await.unwrap();

    let books = url(&server, &server.addressbook_home());
    let family = books.join("family/").unwrap();
    client
        .mkaddressbook(
            &family,
            &NewCollection {
                display_name: "Family".into(),
                ..NewCollection::default()
            },
        )
        .await
        .unwrap();
    let listed = client
        .collections(&books, CollectionKind::AddressBook)
        .await
        .unwrap();
    let mut listed: Vec<String> = listed.iter().map(nail_dav::DavCollection::name).collect();
    listed.sort();
    assert_eq!(listed, ["Contacts", "Family"]);

    client.delete(&work, &Precondition::None).await.unwrap();
    assert!(!server.collection_paths().contains(&href_key(&work)));
    let err = client.proppatch(&work, Some("x"), None).await.unwrap_err();
    assert!(matches!(err, DavError::NotFound(_)));
}

#[tokio::test]
async fn server_side_collection_changes_show_in_listings() {
    let server = server().await;
    let client = client(basic());
    let home = url(&server, &server.calendar_home());
    server.add_calendar(
        &format!("{}shared/", server.calendar_home()),
        "Shared",
        &["VEVENT"],
    );
    server.set_read_only(&format!("{}shared/", server.calendar_home()), true);
    server.rename_collection(&server.calendar_path("personal"), "Mine");
    server.remove_collection(&server.calendar_path("tasks"));
    let found = nail_dav::list_collections(&client, DavService::CalDav, &[home])
        .await
        .unwrap();
    let mut shown: Vec<(String, bool)> = found.iter().map(|c| (c.name(), c.read_only)).collect();
    shown.sort();
    assert_eq!(
        shown,
        [
            ("Holidays".to_owned(), true),
            ("Mine".to_owned(), false),
            ("Shared".to_owned(), true)
        ]
    );
}

#[tokio::test]
async fn too_many_redirects_are_an_error() {
    let http = nail_testkit_net::MockHttpServer::start().await.unwrap();
    http.route(
        None,
        None,
        "/loop",
        nail_testkit_net::MockResponse::status(301).with_header("location", "/loop"),
    );
    let client = client(basic());
    let err = client
        .collection(&Url::parse(&format!("{}/loop", http.base_url())).unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Protocol(_)), "{err:?}");
    assert_eq!(http.requests().len(), nail_dav::client::MAX_REDIRECTS + 1);

    // A redirect from https to plain http is refused.
    http.route(
        None,
        None,
        "/down",
        nail_testkit_net::MockResponse::status(302).with_header("location", "http://example.org/x"),
    );
    let routed = client.route_all_to(http.addr());
    let err = routed
        .collection(&Url::parse("https://example.org/down").unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::InsecureUrl(_)), "{err:?}");
}

#[tokio::test]
async fn logins_never_follow_a_redirect_to_another_server() {
    let dav = server().await;
    let front = nail_testkit_net::MockHttpServer::start().await.unwrap();
    front.route(
        None,
        None,
        "/moved",
        nail_testkit_net::MockResponse::status(301)
            .with_header("location", &format!("{}/", dav.base_url())),
    );
    let start = Url::parse(&format!("{}/moved", front.base_url())).unwrap();
    // The other server asks for the login: an error naming it, and the
    // login was not sent there.
    let err = client(basic()).collection(&start).await.unwrap_err();
    assert_eq!(err, DavError::RedirectNeedsLogin("127.0.0.1".into()));
    assert_eq!(err.code(), ErrorCode::Config);
    assert_ne!(dav.requests().len(), 0);
    assert!(dav.requests().iter().all(|r| r.authorization.is_none()));
    // Discovery reports it rather than "no service".
    let found = discover(
        &client(basic()),
        &no_dns(),
        &DiscoveryInput {
            service: DavService::CalDav,
            url: Some(start.to_string()),
            email: None,
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(found, DavError::RedirectNeedsLogin(_)),
        "{found:?}"
    );
    assert!(dav.requests().iter().all(|r| r.authorization.is_none()));
    // Entering the other server's address signs in there.
    assert!(
        discover(
            &client(basic()),
            &no_dns(),
            &DiscoveryInput {
                service: DavService::CalDav,
                url: Some(dav.base_url()),
                email: None,
            },
        )
        .await
        .is_ok()
    );
    assert!(dav.requests().iter().any(|r| r.authorization.is_some()));
}
