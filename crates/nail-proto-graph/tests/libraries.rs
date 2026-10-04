//! Smoke tests of what WP91 builds on (WP90, ADR 0072): the workspace
//! reqwest with the ring TLS configuration from nail-auth talks to the
//! loopback mock server, Graph's MIME upload encoding, and the contract's
//! Focused Inbox / category / folder mapping of Graph resources.
#![cfg(test)]

use base64::Engine as _;
use nail_api::exchange::{CategoryColor, ExchangeWellKnownFolder, InferenceClassification};
use nail_proto_graph::testing::MockGraphServer;

#[tokio::test]
async fn mock_server_answers_me_over_loopback() {
    let server = MockGraphServer::start("ada@contoso.example").await.unwrap();
    assert!(server.addr().ip().is_loopback());
    let tls = nail_auth::tls_config().expect("ring TLS configuration");
    let client = nail_auth::http_client_builder(tls).build().unwrap();
    let me: serde_json::Value = client
        .get(format!("{}/me", server.api_base()))
        .bearer_auth("token")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(me["mail"], "ada@contoso.example");
}

#[test]
fn mime_uploads_are_standard_base64() {
    // `POST /me/sendMail` with `Content-Type: text/plain` takes the MIME
    // message as standard base64.
    let raw = b"Subject: =?utf-8?q?Gr=C3=BC=C3=9Fe?=\r\n\r\n>>>???\r\n";
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    assert_eq!(decoded, raw);
}

#[test]
fn listed_resources_map_to_the_contract() {
    // A `messages/delta` entry and a master category as Graph sends them.
    let message: serde_json::Value = serde_json::from_str(
        r#"{"id": "AAMkAGI2", "changeKey": "CQAAABYAAA", "conversationId": "AAQkAGI2",
            "isRead": false, "flag": {"flagStatus": "flagged"},
            "categories": ["Red category"], "inferenceClassification": "other",
            "parentFolderId": "AAMkInbox"}"#,
    )
    .unwrap();
    assert_eq!(
        InferenceClassification::parse(message["inferenceClassification"].as_str().unwrap()),
        Some(InferenceClassification::Other)
    );
    let category: serde_json::Value =
        serde_json::from_str(r#"{"id": "c1", "displayName": "Red category", "color": "preset0"}"#)
            .unwrap();
    assert_eq!(
        CategoryColor::parse(category["color"].as_str().unwrap()),
        Some(CategoryColor::Preset0)
    );
    assert_eq!(
        ExchangeWellKnownFolder::parse("junkemail"),
        Some(ExchangeWellKnownFolder::JunkEmail)
    );
}
