//! Smoke tests of what WP93 builds on (WP90, ADR 0072): Autodiscover v2
//! against the loopback mock server through the nail-auth HTTP client, a
//! POX answer read with quick-xml, Windows zone names through nail-ical,
//! and the contract's mailbox / category mapping helpers.

use nail_api::exchange::{CategoryColor, ExchangeMailboxSettings, split_mailbox_folder_path};
use nail_exchange::testing::MockAutodiscoverServer;
use quick_xml::Reader;
use quick_xml::events::Event;

#[tokio::test]
async fn autodiscover_v2_over_loopback() {
    let server = MockAutodiscoverServer::start("https://mail.corp.example/EWS/Exchange.asmx")
        .await
        .unwrap();
    assert!(server.addr().ip().is_loopback());
    let tls = nail_auth::tls_config().expect("ring TLS configuration");
    let client = nail_auth::http_client_builder(tls).build().unwrap();
    let url = nail_exchange::autodiscover_v2_url(&server.host(), "ada@corp.example", "EWS", false);
    let answer: serde_json::Value = client
        .get(url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(answer["Protocol"], "EWS");
    assert_eq!(answer["Url"], "https://mail.corp.example/EWS/Exchange.asmx");
}

#[test]
fn pox_answers_are_read_with_quick_xml() {
    let pox = r#"<?xml version="1.0" encoding="utf-8"?>
<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/responseschema/2006">
  <Response xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a">
    <Account>
      <Protocol><Type>EXCH</Type><EwsUrl>https://mail.corp.example/EWS/Exchange.asmx</EwsUrl></Protocol>
      <AlternativeMailbox><Type>Delegate</Type><DisplayName>Support</DisplayName><SmtpAddress>support@corp.example</SmtpAddress></AlternativeMailbox>
    </Account>
  </Response>
</Autodiscover>"#;
    let mut reader = Reader::from_str(pox);
    let mut current = String::new();
    let mut ews_url = None;
    let mut alternative = None;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(start) => {
                current = AsRef::<str>::as_ref(&start.local_name()).to_owned();
            }
            Event::Text(text) => {
                let text = text.xml10_content().into_owned();
                match current.as_str() {
                    "EwsUrl" => ews_url = Some(text),
                    "SmtpAddress" => alternative = Some(text),
                    _ => {}
                }
            }
            Event::End(_) => current.clear(),
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(
        ews_url.as_deref(),
        Some("https://mail.corp.example/EWS/Exchange.asmx")
    );
    let mailbox = ExchangeMailboxSettings::shared(alternative.unwrap());
    assert!(mailbox.issues("ada@corp.example").is_empty());
}

#[test]
fn windows_zone_names_map_to_iana() {
    let zone = nail_ical::tz::iana_for_name("W. Europe Standard Time").unwrap();
    assert_eq!(zone.name(), "Europe/Berlin");
    assert!(nail_ical::tz::iana_for_name("Pacific Standard Time").is_some());
}

#[test]
fn contract_helpers() {
    assert_eq!(
        split_mailbox_folder_path("@support@corp.example/Inbox"),
        (Some("support@corp.example"), "Inbox")
    );
    assert_eq!(CategoryColor::from_ews_index(14), CategoryColor::Preset14);
}
