//! `nail mcp --client <name>`: an rmcp stdio client talks through the
//! bridge to the HTTP server, with the same grants and audit.

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
    assert_eq!(tools.len(), 5);
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
    let _ = client.cancel().await;
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
