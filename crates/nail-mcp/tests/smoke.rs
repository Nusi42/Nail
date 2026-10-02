//! WP20 smoke test: an rmcp server served over Streamable HTTP on
//! 127.0.0.1 answers an rmcp client (the setup WP23 builds on).

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolRequestParams, ClientConfig, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
    transport::{
        StreamableHttpClientTransport,
        streamable_http_client::StreamableHttpClientTransportConfig,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct EchoRequest {
    text: String,
}

#[derive(Debug, Clone)]
struct EchoServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl EchoServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Echo the text")]
    fn echo(&self, Parameters(EchoRequest { text }): Parameters<EchoRequest>) -> String {
        text
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EchoServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
}

#[tokio::test]
async fn streamable_http_round_trip() {
    let service: StreamableHttpService<EchoServer, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(EchoServer::new()),
            Default::default(),
            StreamableHttpServerConfig::default().with_sse_keep_alive(None),
        );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    // The workspace builds reqwest with `rustls-no-provider` (ADR 0014), so
    // the client must come with its TLS configuration: nail-auth's.
    let http = nail_auth::http_client().unwrap();
    let transport = StreamableHttpClientTransport::with_client(
        http,
        StreamableHttpClientTransportConfig::with_uri(format!("http://{addr}/mcp")),
    );
    let client = ClientConfig::default().serve(transport).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(
        tools.iter().map(|t| t.name.as_ref()).collect::<Vec<_>>(),
        ["echo"]
    );
    let args = serde_json::from_value(serde_json::json!({"text": "hi"})).unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("echo").with_arguments(args))
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true));
    let text = serde_json::to_value(&result.content).unwrap().to_string();
    assert!(text.contains("hi"), "{text}");

    let _ = client.cancel().await;
    server.abort();
}

/// M4a (ADR 0079): the write tools' argument types from nail-api derive
/// their JSON Schemas with rmcp's own schemars (one version in the build),
/// so the server can publish them as tool input schemas.
#[test]
fn contract_argument_types_have_rmcp_schemas() {
    use nail_api::mcp::prompts::DraftReplyArgs;
    use nail_api::mcp::tools::{CreateDraftArgs, DeleteMessagesArgs, SendMessageArgs};
    use rmcp::handler::server::common::schema_for_type;

    let send = schema_for_type::<SendMessageArgs>();
    assert_eq!(send.get("type").and_then(|t| t.as_str()), Some("object"));
    let required = send.get("required").and_then(|r| r.as_array()).unwrap();
    assert_eq!(required, &[serde_json::json!("draft_id")]);
    let draft = schema_for_type::<CreateDraftArgs>();
    assert!(draft["properties"]["attachments"].is_object());
    let delete = schema_for_type::<DeleteMessagesArgs>();
    assert!(delete["properties"]["permanent"].is_object());
    let prompt = schema_for_type::<DraftReplyArgs>();
    assert!(prompt["properties"]["message_id"].is_object());
}
