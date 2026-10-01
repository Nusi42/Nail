//! Smoke tests of what WP82 builds on (WP80, ADR 0065): the workspace
//! reqwest with the ring TLS configuration from nail-auth talks to the
//! loopback mock server, Gmail's base64url `raw` encoding, and the label
//! mapping of the contract.

use base64::Engine as _;
use nail_api::gmail::{GmailCategory, GmailFolder};
use nail_proto_gmail::testing::MockGmailServer;

#[tokio::test]
async fn mock_server_answers_the_profile_over_loopback() {
    let server = MockGmailServer::start("ada@gmail.example").await.unwrap();
    assert!(server.addr().ip().is_loopback());
    let tls = nail_auth::tls_config().expect("ring TLS configuration");
    let client = nail_auth::http_client_builder(tls).build().unwrap();
    let profile: serde_json::Value = client
        .get(format!("{}/profile", server.api_base()))
        .bearer_auth("token")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(profile["emailAddress"], "ada@gmail.example");
    assert_eq!(profile["historyId"], "1");
}

#[test]
fn raw_messages_are_base64url_without_padding() {
    let raw = b"Subject: =?utf-8?q?Gr=C3=BC=C3=9Fe?=\r\n\r\n>>>???\r\n";
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
    assert!(!encoded.contains('+') && !encoded.contains('/') && !encoded.ends_with('='));
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .unwrap();
    assert_eq!(decoded, raw);
}

#[test]
fn listed_labels_map_to_folders_and_categories() {
    // A `messages.get?format=minimal` answer as Gmail sends it.
    let message: serde_json::Value = serde_json::from_str(
        r#"{"id": "18c0", "threadId": "18bf", "labelIds": ["UNREAD", "CATEGORY_UPDATES", "INBOX"],
            "historyId": "4211", "internalDate": "1790856000000", "sizeEstimate": 2048}"#,
    )
    .unwrap();
    let labels: Vec<String> = serde_json::from_value(message["labelIds"].clone()).unwrap();
    assert_eq!(GmailFolder::for_labels(&labels), GmailFolder::Inbox);
    assert_eq!(
        GmailCategory::for_labels(&labels),
        Some(GmailCategory::Updates)
    );
    assert!(labels.iter().any(|l| l == "UNREAD"), "unread = not seen");
}
