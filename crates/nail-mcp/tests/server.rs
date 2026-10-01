//! The MCP server end to end: an rmcp client over Streamable HTTP against
//! [`nail_mcp::McpServer`] with a fake data source and an in-memory store.

mod common;

use std::time::Duration;

use common::{FakeData, connect, connect_fails, create_client, grant, start, start_with, summary};
use nail_api::mcp::{McpCallKind, McpOutcome, McpScope};
use nail_api::{Event, UiContext};
use rmcp::model::ClientConfig;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ReadResourceRequestParams, ResourceContents,
    SubscribeRequestParams,
};
use rmcp::service::{RunningService, ServiceError};
use rmcp::{ClientHandler, RoleClient};
use serde_json::{Value, json};

async fn read<H: ClientHandler>(
    client: &RunningService<RoleClient, H>,
    uri: &str,
) -> Result<ResourceContents, ServiceError> {
    let mut result = client
        .read_resource(ReadResourceRequestParams::new(uri))
        .await?;
    assert_eq!(result.contents.len(), 1);
    Ok(result.contents.remove(0))
}

fn text(contents: &ResourceContents) -> Value {
    match contents {
        ResourceContents::TextResourceContents { text, .. } => serde_json::from_str(text).unwrap(),
        other => panic!("text expected: {other:?}"),
    }
}

fn denied(err: &ServiceError) -> String {
    match err {
        ServiceError::McpError(e) => {
            assert_eq!(
                e.data.as_ref().and_then(|d| d.get("nail")),
                Some(&json!("denied")),
                "{e:?}"
            );
            e.message.to_string()
        }
        other => panic!("MCP error expected: {other:?}"),
    }
}

async fn tool<H: ClientHandler>(
    client: &RunningService<RoleClient, H>,
    name: &'static str,
    args: Value,
) -> CallToolResult {
    let params =
        CallToolRequestParams::new(name).with_arguments(serde_json::from_value(args).unwrap());
    client.call_tool(params).await.unwrap()
}

#[tokio::test]
async fn a_new_client_sees_nothing_until_granted() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "claude").await;
    let (client, _) = connect(env.server.url(), &token).await;

    // Nothing to list, nothing to read, tools refuse.
    let resources = client.list_all_resources().await.unwrap();
    assert!(resources.is_empty(), "{resources:?}");
    let templates = client.list_all_resource_templates().await.unwrap();
    assert!(
        templates
            .iter()
            .any(|t| t.uri_template == "nail://messages/{message_id}")
    );
    let err = read(&client, "nail://messages/m1").await.unwrap_err();
    assert!(denied(&err).contains("scope `read` not granted"));
    let result = tool(&client, "get_message", json!({"message_id": "m1"})).await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(result.structured_content.unwrap()["error"], json!("denied"));

    // Granted: read for the work account.
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    let resources = client.list_all_resources().await.unwrap();
    let uris: Vec<_> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert!(uris.contains(&"nail://accounts"), "{uris:?}");
    assert!(uris.contains(&"nail://accounts/work/folders"), "{uris:?}");
    assert!(!uris.contains(&"nail://accounts/home/folders"), "{uris:?}");
    assert!(!uris.contains(&"nail://rules"), "rules need all accounts");

    let message = read(&client, "nail://messages/m1").await.unwrap();
    let value = text(&message);
    assert_eq!(
        value["untrusted_content"],
        json!(true),
        "content is marked untrusted"
    );
    assert_eq!(value["data"]["id"], json!("m1"));
    // Exchange data travels with the message (M3c).
    assert_eq!(value["data"]["exchange"]["inference"], json!("focused"));
    assert_eq!(
        value["data"]["attachments"][0]["uri"],
        json!("nail://messages/m1/attachments/0")
    );
    let ResourceContents::TextResourceContents { meta, .. } = &message else {
        unreachable!()
    };
    assert_eq!(
        meta.as_ref().unwrap().get("nail/untrusted_content"),
        Some(&json!(true))
    );

    // Other account, raw source, attachments, encrypted: still refused.
    let err = read(&client, "nail://messages/m2").await.unwrap_err();
    assert!(denied(&err).contains("account `home`"));
    let err = read(&client, "nail://messages/m1/raw").await.unwrap_err();
    assert!(denied(&err).contains("raw-source"));
    let err = read(&client, "nail://messages/m1/attachments/0")
        .await
        .unwrap_err();
    assert!(denied(&err).contains("attachments"));
    let err = read(&client, "nail://messages/m3").await.unwrap_err();
    assert!(denied(&err).contains("encrypted"));

    // Each separate grant opens exactly its part.
    grant(
        &env.store,
        &id,
        &[
            McpScope::Read,
            McpScope::RawSource,
            McpScope::Attachments,
            McpScope::Encrypted,
        ],
        &["work"],
    )
    .await;
    match read(&client, "nail://messages/m1/raw").await.unwrap() {
        ResourceContents::BlobResourceContents {
            blob,
            mime_type,
            meta,
            ..
        } => {
            use base64::Engine as _;
            let raw = base64::engine::general_purpose::STANDARD
                .decode(blob)
                .unwrap();
            assert_eq!(raw, common::RAW_M1);
            assert_eq!(mime_type.as_deref(), Some("message/rfc822"));
            assert!(meta.is_some());
        }
        other => panic!("blob expected: {other:?}"),
    }
    match read(&client, "nail://messages/m1/attachments/0")
        .await
        .unwrap()
    {
        ResourceContents::BlobResourceContents { mime_type, .. } => {
            assert_eq!(mime_type.as_deref(), Some("text/csv"));
        }
        other => panic!("blob expected: {other:?}"),
    }
    assert_eq!(
        text(&read(&client, "nail://messages/m3").await.unwrap())["data"]["id"],
        json!("m3")
    );

    let _ = client.cancel().await;
}

#[tokio::test]
async fn every_call_is_audited() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "auditor").await;
    let (client, _) = connect(env.server.url(), &token).await;
    let _ = read(&client, "nail://messages/m1").await;
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    read(&client, "nail://messages/m1").await.unwrap();
    tool(&client, "search_messages", json!({"query": "plan"})).await;
    let _ = read(&client, "nail://nothing").await;
    // Malformed arguments and unknown tools leave a trace too.
    let bad_args = CallToolRequestParams::new("get_message")
        .with_arguments(json!({"id": 1}).as_object().unwrap().clone());
    assert!(client.call_tool(bad_args).await.is_err());
    assert!(
        client
            .call_tool(CallToolRequestParams::new("send_message"))
            .await
            .is_err()
    );

    let entries = env.store.list_mcp_audit(Some(&id), None, 50).unwrap();
    let seen: Vec<_> = entries
        .iter()
        .rev()
        .map(|e| (e.kind, e.name.as_str(), e.outcome))
        .collect();
    assert_eq!(
        seen,
        [
            (
                McpCallKind::Resource,
                "nail://messages/m1",
                McpOutcome::Denied
            ),
            (
                McpCallKind::Resource,
                "nail://messages/m1",
                McpOutcome::Allowed
            ),
            (McpCallKind::Tool, "search_messages", McpOutcome::Allowed),
            (McpCallKind::Resource, "nail://nothing", McpOutcome::Error),
            (McpCallKind::Tool, "get_message", McpOutcome::Error),
            (McpCallKind::Tool, "send_message", McpOutcome::Error),
        ]
    );
    assert_eq!(
        entries.last().unwrap().detail.as_deref(),
        Some("scope `read` not granted")
    );
    assert!(
        entries[0]
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("unknown tool")),
        "{:?}",
        entries[0]
    );
    assert!(entries.iter().all(|e| e.client_name == "auditor"));
    assert!(
        env.store
            .get_mcp_client(&id)
            .unwrap()
            .last_used_at
            .is_some(),
        "use is recorded"
    );
    let _ = client.cancel().await;
}

#[tokio::test]
async fn bad_and_revoked_tokens_are_refused() {
    let env = start().await;
    assert!(connect_fails(env.server.url(), "nail_mcp_not-a-token").await);
    let entries = env.store.list_mcp_audit(None, None, 10).unwrap();
    assert!(!entries.is_empty());
    assert!(entries.iter().all(|e| e.kind == McpCallKind::Connect
        && e.outcome == McpOutcome::Denied
        && e.client_id.is_none()));

    let (id, token) = create_client(&env.store, "gone").await;
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    let (client, _) = connect(env.server.url(), &token).await;
    read(&client, "nail://accounts").await.unwrap();
    nail_mcp::admin::handle(
        &env.store,
        None,
        nail_api::Request::RevokeMcpClient {
            client_id: id.clone(),
        },
        |_| {},
    )
    .await
    .unwrap();
    // The very next request of the open session is refused.
    assert!(read(&client, "nail://accounts").await.is_err());
    assert!(connect_fails(env.server.url(), &token).await);
    let _ = client.cancel().await;
}

#[tokio::test]
async fn tools_filter_by_grant_and_mark_content() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "tools").await;
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    let (client, _) = connect(env.server.url(), &token).await;

    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), nail_mcp::tools::TOOL_NAMES.len());

    let result = tool(&client, "search_messages", json!({"query": "plan"})).await;
    assert_ne!(result.is_error, Some(true));
    let value = result.structured_content.unwrap();
    assert_eq!(value["untrusted_content"], json!(true));
    let ids: Vec<_> = value["data"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["m1", "m3"], "home is not granted");
    assert!(
        value["data"].get("hidden").is_none(),
        "nothing counts what the grant hides"
    );
    assert_eq!(
        value["data"]["total"],
        Value::Null,
        "totals are inexact when filtered"
    );

    let result = tool(
        &client,
        "search_messages",
        json!({"query": "x", "account_id": "home"}),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    let result = tool(&client, "search_messages", json!({"query": "("})).await;
    assert_eq!(
        result.structured_content.unwrap()["error"],
        json!("invalid_query")
    );

    let result = tool(&client, "get_thread", json!({"thread_id": "t1"})).await;
    let thread = &result.structured_content.unwrap()["data"];
    assert!(thread.get("hidden").is_none());
    assert_eq!(thread["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        thread["subject"],
        json!("Subject of m1"),
        "the aggregate of the visible messages only"
    );
    assert_eq!(thread["participants"][0]["email"], json!("ada@example.org"));

    let result = tool(&client, "list_folders", json!({"account_id": "work"})).await;
    let folders = result.structured_content.unwrap();
    assert_eq!(folders["folders"].as_array().unwrap().len(), 2);
    let result = tool(&client, "list_folders", json!({"account_id": "home"})).await;
    assert_eq!(result.is_error, Some(true));

    // Folder exclusions hide a folder of a granted account.
    nail_mcp::admin::handle(
        &env.store,
        None,
        nail_api::Request::UpdateMcpGrants {
            client_id: id.clone(),
            grant: nail_api::McpGrant {
                scopes: vec![McpScope::Read],
                accounts: vec!["both".into()],
                excluded_folders: vec!["work-hr".into()],
                ..Default::default()
            },
        },
        |_| {},
    )
    .await
    .unwrap();
    let result = tool(&client, "list_folders", json!({"account_id": "work"})).await;
    let folders = result.structured_content.unwrap();
    assert_eq!(folders["folders"].as_array().unwrap().len(), 1);
    let messages = text(
        &read(&client, "nail://folders/home-inbox/messages")
            .await
            .unwrap(),
    );
    assert_eq!(
        messages["data"]["messages"][0]["id"],
        json!("m2"),
        "virtual account grants its members"
    );
    let err = read(&client, "nail://folders/work-hr/messages")
        .await
        .unwrap_err();
    assert!(denied(&err).contains("work-hr"));

    let err = client
        .call_tool(CallToolRequestParams::new("send_message"))
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::McpError(_)));
    let _ = client.cancel().await;
}

#[tokio::test]
async fn ui_context_needs_its_own_grant_and_shows_the_indicator() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "context").await;
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    env.data.set_context(UiContext {
        message_id: Some("m1".into()),
        selected_text: Some("the plan".into()),
        draft_id: Some("draft-1".into()),
        draft_cursor: Some(4),
        selected_message_ids: vec!["m1".into(), "m2".into()],
        ..UiContext::default()
    });
    let (client, _) = connect(env.server.url(), &token).await;
    let err = read(&client, "nail://ui/context").await.unwrap_err();
    assert!(denied(&err).contains("context"));
    let result = tool(&client, "get_ui_context", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    assert!(
        env.data.events().is_empty(),
        "no indicator for a refused read"
    );

    grant(
        &env.store,
        &id,
        &[McpScope::Read, McpScope::Context],
        &["work"],
    )
    .await;
    let value = text(&read(&client, "nail://ui/context").await.unwrap());
    assert_eq!(value["untrusted_content"], json!(true));
    let data = &value["data"];
    assert_eq!(data["message"]["id"], json!("m1"));
    assert_eq!(data["message"]["selected_text"], json!("the plan"));
    assert_eq!(data["selected_message_ids"], json!(["m1"]), "m2 is in home");
    assert_eq!(data["draft"]["draft"]["subject"], json!("Re: Plan"));
    assert_eq!(data["draft"]["cursor"], json!(4));
    assert_eq!(
        env.data.events(),
        [Event::McpContextAccess {
            client_id: id.clone(),
            client_name: "context".into(),
            edit: false,
        }]
    );

    // Where the user is, only as far as the grant covers it.
    env.data.set_context(UiContext {
        account_id: Some("home".into()),
        folder_id: Some("home-inbox".into()),
        thread_id: Some("t3".into()),
        draft_id: Some("draft-home".into()),
        ..UiContext::default()
    });
    let data = tool(&client, "get_ui_context", json!({}))
        .await
        .structured_content
        .unwrap()["data"]
        .clone();
    assert_eq!(data["account_id"], Value::Null);
    assert_eq!(data["folder_id"], Value::Null);
    assert_eq!(data["thread_id"], json!("t3"), "t3 is in work");
    // A draft from a non-granted account, or quoting a message outside the
    // grant, is named, not shown.
    assert_eq!(data["draft"], json!({"id": "draft-home", "hidden": true}));
    env.data.set_context(UiContext {
        account_id: Some("work".into()),
        folder_id: Some("work-inbox".into()),
        thread_id: Some("t2-unknown".into()),
        draft_id: Some("draft-reply-m2".into()),
        ..UiContext::default()
    });
    let data = tool(&client, "get_ui_context", json!({}))
        .await
        .structured_content
        .unwrap()["data"]
        .clone();
    assert_eq!(data["account_id"], json!("work"));
    assert_eq!(data["folder_id"], json!("work-inbox"));
    assert_eq!(data["thread_id"], Value::Null);
    assert_eq!(
        data["draft"],
        json!({"id": "draft-reply-m2", "hidden": true})
    );

    // A message outside the account grant is named, not shown.
    env.data.set_context(UiContext {
        message_id: Some("m2".into()),
        ..UiContext::default()
    });
    let value = tool(&client, "get_ui_context", json!({}))
        .await
        .structured_content
        .unwrap();
    assert_eq!(
        value["data"]["message"],
        json!({"id": "m2", "hidden": true})
    );
    let _ = client.cancel().await;
}

async fn next_update(rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>) -> Option<String> {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .ok()
        .flatten()
}

#[tokio::test]
#[allow(deprecated)]
async fn subscriptions_notify_new_mail_and_ui_context() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "watcher").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Read, McpScope::Context],
        &["work"],
    )
    .await;
    let (client, mut updates) = connect(env.server.url(), &token).await;

    for uri in [
        "nail://accounts/work/folders",
        "nail://folders/work-inbox/messages",
        "nail://ui/context",
    ] {
        client
            .subscribe(SubscribeRequestParams::new(uri))
            .await
            .unwrap_or_else(|e| panic!("{uri}: {e}"));
    }
    // Not granted, not subscribable.
    let err = client
        .subscribe(SubscribeRequestParams::new("nail://accounts/home/folders"))
        .await
        .unwrap_err();
    assert!(denied(&err).contains("home"));
    assert!(
        client
            .subscribe(SubscribeRequestParams::new("nail://messages/m1"))
            .await
            .is_err()
    );
    assert_eq!(env.server.subscriber_count(), 1);

    env.server
        .on_event(&Event::NewMail {
            account_id: "work".into(),
            folder_id: "work-inbox".into(),
            message_ids: vec!["m9".into()],
        })
        .await;
    let mut got = vec![
        next_update(&mut updates).await.unwrap(),
        next_update(&mut updates).await.unwrap(),
    ];
    got.sort();
    assert_eq!(
        got,
        [
            "nail://accounts/work/folders",
            "nail://folders/work-inbox/messages"
        ]
    );

    env.server.ui_context_changed().await;
    assert_eq!(
        next_update(&mut updates).await.as_deref(),
        Some("nail://ui/context")
    );

    // Mail in an account without a subscription sends nothing.
    env.server
        .on_event(&Event::NewMail {
            account_id: "home".into(),
            folder_id: "home-inbox".into(),
            message_ids: vec![],
        })
        .await;

    // A narrowed grant stops the context notifications at once.
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    env.server.ui_context_changed().await;
    // Unsubscribing stops the folder updates.
    client
        .unsubscribe(rmcp::model::UnsubscribeRequestParams::new(
            "nail://folders/work-inbox/messages",
        ))
        .await
        .unwrap();
    env.server
        .on_event(&Event::MailChanged {
            folder_ids: vec!["work-inbox".into()],
        })
        .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(300), updates.recv())
            .await
            .is_err(),
        "no further notifications"
    );

    let entries = env.store.list_mcp_audit(Some(&id), None, 50).unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.kind == McpCallKind::Subscribe && e.outcome == McpOutcome::Allowed)
            .count(),
        3
    );
    let _ = client.cancel().await;
}

#[tokio::test]
async fn rules_need_a_grant_for_all_accounts() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "rules").await;
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;
    let (client, _) = connect(env.server.url(), &token).await;
    let err = read(&client, "nail://rules").await.unwrap_err();
    assert!(denied(&err).contains("all accounts"));
    grant(&env.store, &id, &[McpScope::Read], &["all"]).await;
    match read(&client, "nail://rules").await.unwrap() {
        ResourceContents::TextResourceContents { text, .. } => assert!(text.contains("Receipts")),
        other => panic!("text expected: {other:?}"),
    }
    let accounts = text(&read(&client, "nail://accounts").await.unwrap());
    assert_eq!(accounts["accounts"].as_array().unwrap().len(), 3);
    let _ = client.cancel().await;
}

#[tokio::test]
async fn the_endpoint_file_follows_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let path = nail_mcp::endpoint::endpoint_path(dir.path());
    let server = nail_mcp::McpServer::start(
        nail_store::Store::open_in_memory().unwrap(),
        std::sync::Arc::new(FakeData::default()),
        nail_mcp::ServerOptions {
            port: 0,
            endpoint_file: Some(path.clone()),
        },
    )
    .await
    .unwrap();
    let endpoint = nail_mcp::endpoint::read_endpoint(&path).unwrap();
    assert_eq!(endpoint.url, server.url());
    assert!(endpoint.url.starts_with("http://127.0.0.1:"));
    assert_eq!(server.info().url.as_deref(), Some(server.url()));
    server.shutdown().await;
    assert!(!path.exists());
}

#[tokio::test]
async fn listen_streams_of_protocol_2026_07_28_get_updates() {
    use rmcp::model::{ProtocolVersion, ServerNotification, SubscriptionFilter};
    use rmcp::service::{ClientLifecycleMode, ClientServiceExt};

    let env = start().await;
    let (id, token) = create_client(&env.store, "modern").await;
    grant(
        &env.store,
        &id,
        &[McpScope::Read, McpScope::Context],
        &["work"],
    )
    .await;
    let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
        nail_mcp::bridge::loopback_http_client().unwrap(),
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            env.server.url().to_owned(),
        )
        .auth_header(token),
    );
    let client = ClientConfig::default()
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap();
    // Stateless requests work as well.
    let value = text(&read(&client, "nail://accounts").await.unwrap());
    assert_eq!(value["accounts"].as_array().unwrap().len(), 1, "work only");

    let mut subscription = client
        .peer()
        .listen(
            SubscriptionFilter::builder()
                .resource_subscriptions(["nail://ui/context", "nail://accounts/home/folders"])
                .build(),
        )
        .await
        .unwrap();
    // Wait until the stream is registered.
    for _ in 0..50 {
        if env.server.subscriber_count() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(env.server.subscriber_count(), 1);
    env.server.ui_context_changed().await;
    let notification = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    match notification {
        ServerNotification::ResourceUpdatedNotification(n) => {
            assert_eq!(n.params.uri, "nail://ui/context");
        }
        other => panic!("update expected: {other:?}"),
    }
    let entries = env.store.list_mcp_audit(Some(&id), None, 50).unwrap();
    let subscribe: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == McpCallKind::Subscribe)
        .map(|e| (e.name.as_str(), e.outcome))
        .collect();
    assert!(
        subscribe.contains(&("nail://ui/context", McpOutcome::Allowed)),
        "{subscribe:?}"
    );
    assert!(
        subscribe.contains(&("nail://accounts/home/folders", McpOutcome::Denied)),
        "{subscribe:?}"
    );
    drop(subscription);
    let _ = client.cancel().await;
}

/// The data of a user with more mail than the client may see: `m4` in the
/// excluded folder `work-hr`, `m2` in the non-granted account `home`.
fn with_hidden_mail() -> FakeData {
    let mut data = FakeData::default();
    for n in 4..40 {
        data.messages
            .push(summary(&format!("m{n}"), "work", "work-hr", "t9"));
    }
    data
}

fn without_hidden_mail() -> FakeData {
    let mut data = FakeData::default();
    data.messages
        .retain(|m| m.folder_id.as_str() == "work-inbox");
    data
}

/// M2a: with decrypted indexing on, a search can match the decrypted text
/// of encrypted mail; a client without the `encrypted` scope never sees
/// those hits.
#[tokio::test]
async fn searches_leave_out_encrypted_mail_without_the_encrypted_grant() {
    let data = FakeData {
        decrypted_index: true,
        ..FakeData::default()
    };
    let env = start_with(data).await;
    let (id, token) = create_client(&env.store, "search").await;
    grant(&env.store, &id, &[McpScope::Read], &["work", "home"]).await;
    let (client, _) = connect(env.server.url(), &token).await;
    let ids = |value: &Value| -> Vec<String> {
        value["data"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_owned())
            .collect()
    };
    let result = tool(&client, "search_messages", json!({"query": "plan"})).await;
    assert_eq!(ids(&result.structured_content.unwrap()), ["m1", "m2"]);
    let _ = client.cancel().await;

    grant(
        &env.store,
        &id,
        &[McpScope::Read, McpScope::Encrypted],
        &["work", "home"],
    )
    .await;
    let (client, _) = connect(env.server.url(), &token).await;
    let result = tool(&client, "search_messages", json!({"query": "plan"})).await;
    assert_eq!(ids(&result.structured_content.unwrap()), ["m1", "m2", "m3"]);
    let _ = client.cancel().await;
}

#[tokio::test]
async fn restricted_searches_reveal_nothing_about_hidden_mail() {
    let mut answers = Vec::new();
    for data in [with_hidden_mail(), without_hidden_mail()] {
        let env = start_with(data).await;
        let (id, token) = create_client(&env.store, "search").await;
        nail_mcp::admin::handle(
            &env.store,
            None,
            nail_api::Request::UpdateMcpGrants {
                client_id: id.clone(),
                grant: nail_api::McpGrant {
                    scopes: vec![McpScope::Read],
                    accounts: vec!["work".into()],
                    excluded_folders: vec!["work-hr".into()],
                    ..Default::default()
                },
            },
            |_| {},
        )
        .await
        .unwrap();
        let (client, _) = connect(env.server.url(), &token).await;
        let mut pages = Vec::new();
        for args in [
            json!({"query": "folder:work-hr subject:salary"}),
            json!({"query": "account:home from:x"}),
            json!({"query": "x", "limit": 1}),
            json!({"query": "x", "limit": 1, "offset": 1}),
            json!({"query": "x", "limit": 1, "offset": 2}),
        ] {
            let result = tool(&client, "search_messages", args).await;
            pages.push(result.structured_content.unwrap()["data"].clone());
        }
        let _ = client.cancel().await;
        answers.push(pages);
    }
    assert_eq!(answers[0], answers[1], "hidden mail changes nothing");
    let pages = &answers[0];
    // Paging counts only readable messages.
    assert_eq!(pages[2]["messages"][0]["id"], json!("m1"));
    assert_eq!(pages[2]["next_offset"], json!(1));
    assert_eq!(pages[3]["messages"][0]["id"], json!("m3"));
    assert_eq!(pages[3]["next_offset"], Value::Null);
    assert_eq!(pages[4]["messages"], json!([]));
    assert_eq!(pages[0]["total"], Value::Null);
    assert!(pages[0].get("hidden").is_none());
}

#[tokio::test]
async fn refused_connections_cannot_flood_the_audit_log() {
    let env = start().await;
    let http = nail_mcp::bridge::loopback_http_client().unwrap();
    let long_path = env
        .server
        .url()
        .replace("/mcp", &format!("/{}", "x".repeat(500)));
    for n in 0..60 {
        let url = if n % 2 == 0 {
            env.server.url().to_owned()
        } else {
            long_path.clone()
        };
        let status = http
            .post(url)
            .bearer_auth("nail_mcp_wrong")
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status.as_u16(), 401);
    }
    let entries = env.store.list_mcp_audit(None, None, 500).unwrap();
    assert_eq!(entries.len(), 20, "at most 20 refusals a minute are logged");
    assert!(entries.iter().all(|e| e.name.len() <= 70), "{entries:?}");
}
