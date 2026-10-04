//! `nail mcp --client <name>`: an rmcp stdio client talks through the
//! bridge to the HTTP server, with the same grants and audit.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use common::{create_client, grant, start};
use nail_api::mcp::{McpCallKind, McpScope};
use nail_mcp::bridge::{self, BridgeError, BridgeOptions};
use rmcp::ServiceExt;
use rmcp::model::{ClientConfig, ReadResourceRequestParams};

#[tokio::test]
async fn the_bridge_forwards_stdio_to_the_app() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "desktop").await;
    grant(&env.store, &id, &[McpScope::Read], &["work"]).await;

    // client stdout → bridge stdin, bridge stdout → client stdin
    let (client_out, bridge_in) = tokio::io::duplex(64 * 1024);
    let (bridge_out, client_in) = tokio::io::duplex(64 * 1024);
    let options = BridgeOptions {
        url: env.server.url().to_owned(),
        token,
        client_name: Some("desktop".into()),
    };
    let bridge = tokio::spawn(bridge::run(options, bridge_in, bridge_out));

    let client = ClientConfig::default()
        .serve((client_in, client_out))
        .await
        .unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), nail_mcp::tools::TOOL_NAMES.len());
    let result = client
        .read_resource(ReadResourceRequestParams::new("nail://messages/m1"))
        .await
        .unwrap();
    assert_eq!(result.contents.len(), 1);
    assert!(
        client
            .read_resource(ReadResourceRequestParams::new("nail://messages/m2"))
            .await
            .is_err(),
        "the grant applies through the bridge"
    );
    drop(client.cancel().await);
    // Closing stdin ends the bridge.
    let ended = tokio::time::timeout(std::time::Duration::from_secs(10), bridge)
        .await
        .expect("bridge ends when stdin closes")
        .unwrap();
    assert!(ended.is_ok(), "{ended:?}");

    let entries = env.store.list_mcp_audit(Some(&id), None, 10).unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.kind == McpCallKind::Resource)
            .count(),
        2
    );
}

#[tokio::test]
async fn the_bridge_checks_token_and_client_name() {
    let env = start().await;
    let (_, token) = create_client(&env.store, "desktop").await;
    let (_, sink) = tokio::io::duplex(64);
    let (source, _) = tokio::io::duplex(64);

    let wrong_name = BridgeOptions {
        url: env.server.url().to_owned(),
        token: token.clone(),
        client_name: Some("laptop".into()),
    };
    let http = bridge::loopback_http_client().unwrap();
    match bridge::check_client(&http, &wrong_name).await {
        Err(BridgeError::WrongClient { expected, actual }) => {
            assert_eq!((expected.as_str(), actual.as_str()), ("laptop", "desktop"));
        }
        other => panic!("wrong client expected: {other:?}"),
    }
    let bad_token = BridgeOptions {
        token: "nail_mcp_wrong".into(),
        client_name: None,
        ..wrong_name.clone()
    };
    assert!(matches!(
        bridge::run(bad_token, source, sink).await,
        Err(BridgeError::Unauthorized)
    ));

    let url = env.server.url().to_owned();
    drop(env);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let (_, sink) = tokio::io::duplex(64);
    let (source, _) = tokio::io::duplex(64);
    let not_running = BridgeOptions {
        url,
        token,
        client_name: None,
    };
    assert!(matches!(
        bridge::run(not_running, source, sink).await,
        Err(BridgeError::NotRunning(_))
    ));
}

/// A stdio client that approves every elicitation form.
#[derive(Clone, Default)]
struct Approving(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl rmcp::ClientHandler for Approving {
    fn get_info(&self) -> ClientConfig {
        let mut config = ClientConfig::default();
        config.capabilities.elicitation = Some(
            rmcp::model::ElicitationCapability::new()
                .with_form(rmcp::model::FormElicitationCapability::new()),
        );
        config
    }

    async fn create_elicitation(
        &self,
        request: rmcp::model::ElicitRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleClient>,
    ) -> Result<rmcp::model::ElicitResult, rmcp::model::ErrorData> {
        if let rmcp::model::ElicitRequestParams::FormElicitationParams { message, .. } = request {
            self.0.lock().unwrap().push(message);
        }
        Ok(
            rmcp::model::ElicitResult::new(rmcp::model::ElicitationAction::Accept)
                .with_content(serde_json::json!({"approve": true})),
        )
    }
}

/// M4a: a confirmation's elicitation form reaches the stdio client through
/// the bridge, and its answer comes back.
#[tokio::test]
async fn the_bridge_carries_confirmations_both_ways() {
    let env = start().await;
    let (id, token) = create_client(&env.store, "desktop").await;
    grant(&env.store, &id, &[McpScope::Send], &["work"]).await;
    let (client_out, bridge_in) = tokio::io::duplex(64 * 1024);
    let (bridge_out, client_in) = tokio::io::duplex(64 * 1024);
    let options = BridgeOptions {
        url: env.server.url().to_owned(),
        token,
        client_name: None,
    };
    let bridge = tokio::spawn(bridge::run(options, bridge_in, bridge_out));
    let approving = Approving::default();
    let client = approving
        .clone()
        .serve((client_in, client_out))
        .await
        .unwrap();
    let params = rmcp::model::CallToolRequestParams::new("send_message").with_arguments(
        serde_json::json!({"draft_id": "d1"})
            .as_object()
            .unwrap()
            .clone(),
    );
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), client.call_tool(params))
        .await
        .expect("answered through the bridge")
        .unwrap();
    let value = result.structured_content.unwrap();
    assert_eq!(value["status"], "done", "{value}");
    assert_eq!(approving.0.lock().unwrap().len(), 1, "the form was shown");
    assert_eq!(env.data.write_names(), ["SendDraft"]);
    drop(client.cancel().await);
    drop(tokio::time::timeout(std::time::Duration::from_secs(10), bridge).await);
}
