//! Smoke tests of what WP92 builds on (WP90, ADR 0072): the workspace
//! reqwest with the ring TLS configuration from nail-auth posts a SOAP
//! envelope to the loopback mock server, the crate's XML tree reads the answer,
//! `MimeContent` is base64, and the contract maps EWS colour indexes and
//! distinguished folders.
#![cfg(test)]

use base64::Engine as _;
use nail_api::exchange::{CategoryColor, ExchangeWellKnownFolder};
use nail_proto_ews::testing::MockEwsServer;
/// The text of the first element named `local` (any namespace), the
/// element itself included.
fn first_text(xml: &str, local: &str) -> Option<String> {
    let root = nail_proto_ews::xml::parse(xml).ok()?;
    let found = if root.name == local {
        Some(&root)
    } else {
        root.find(local)
    };
    found.map(|e| e.text.clone())
}

#[tokio::test]
async fn mock_server_answers_soap_over_loopback() {
    let server = MockEwsServer::start().await.unwrap();
    assert!(server.addr().ip().is_loopback());
    let client = nail_auth::http_client().unwrap();
    let body = nail_proto_ews::envelope(
        nail_proto_ews::DEFAULT_SERVER_VERSION,
        r#"<m:GetFolder><m:FolderShape><t:BaseShape>Default</t:BaseShape></m:FolderShape><m:FolderIds><t:DistinguishedFolderId Id="inbox"/></m:FolderIds></m:GetFolder>"#,
    );
    let answer = client
        .post(server.url())
        .header("content-type", "text/xml; charset=utf-8")
        .body(body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(
        first_text(&answer, "ResponseCode").as_deref(),
        Some("NoError")
    );
    assert_eq!(first_text(&answer, "DisplayName").as_deref(), Some("Inbox"));
}

#[test]
fn mime_content_is_base64() {
    let raw = b"From: ada@corp.example\r\nSubject: Hi\r\n\r\nHello\r\n";
    let xml = format!(
        r#"<t:MimeContent CharacterSet="UTF-8">{}</t:MimeContent>"#,
        base64::engine::general_purpose::STANDARD.encode(raw)
    );
    let text = first_text(&xml, "MimeContent").unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(text)
        .unwrap();
    assert_eq!(decoded, raw);
}

#[test]
fn ews_values_map_to_the_contract() {
    // The category list stores colours as indexes, -1 = none.
    assert_eq!(CategoryColor::from_ews_index(7), CategoryColor::Preset7);
    assert_eq!(CategoryColor::from_ews_index(-1), CategoryColor::None);
    assert_eq!(
        ExchangeWellKnownFolder::parse("deleteditems"),
        Some(ExchangeWellKnownFolder::DeletedItems)
    );
    assert_eq!(ExchangeWellKnownFolder::Tasks.ews_name(), Some("tasks"));
}
