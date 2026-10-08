//! Smoke tests of what WP93 builds on (WP90, ADR 0072): Autodiscover v2
//! against the loopback mock server through the nail-auth HTTP client, a
//! POX answer read with nail-discovery's XML tree, Windows zone names through nail-ical,
//! and the contract's mailbox / category mapping helpers.
#![cfg(test)]

use nail_api::exchange::{CategoryColor, ExchangeMailboxSettings, split_mailbox_folder_path};
use nail_exchange::testing::MockAutodiscoverServer;

#[tokio::test]
async fn autodiscover_v2_over_loopback() {
    let server = MockAutodiscoverServer::start("https://mail.corp.example/EWS/Exchange.asmx")
        .await
        .unwrap();
    assert!(server.addr().ip().is_loopback());
    let client = nail_auth::http_client().unwrap();
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
fn pox_answers_are_read_as_a_tree() {
    let pox = r#"<?xml version="1.0" encoding="utf-8"?>
<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/responseschema/2006">
  <Response xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a">
    <Account>
      <Protocol><Type>EXCH</Type><EwsUrl>https://mail.corp.example/EWS/Exchange.asmx</EwsUrl></Protocol>
      <AlternativeMailbox><Type>Delegate</Type><DisplayName>Support</DisplayName><SmtpAddress>support@corp.example</SmtpAddress></AlternativeMailbox>
    </Account>
  </Response>
</Autodiscover>"#;
    let root = nail_discovery::xml::parse(pox.as_bytes()).unwrap();
    let account = root.child("Response").unwrap().child("Account").unwrap();
    let ews_url = account.child("Protocol").unwrap().text_of("EwsUrl");
    let alternative = account
        .child("AlternativeMailbox")
        .unwrap()
        .text_of("SmtpAddress");
    assert_eq!(
        ews_url.as_deref(),
        Some("https://mail.corp.example/EWS/Exchange.asmx")
    );
    let mailbox = ExchangeMailboxSettings::shared(alternative.unwrap());
    assert_eq!(mailbox.issues("ada@corp.example").len(), 0);
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
