//! The stdio bridge behind `nail mcp --client <name>` (ADR 0025).
//!
//! MCP clients that only speak stdio start `nail mcp`; the bridge forwards
//! every JSON-RPC message between its stdin/stdout and the running app's
//! Streamable HTTP endpoint on 127.0.0.1, authenticated with the client's
//! bearer token. It is a transparent pump: the app does all protocol work,
//! grant checks and auditing, exactly as for HTTP clients.

use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, Transport};
use rmcp::{RoleClient, RoleServer};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::http::{MCP_PATH, WHOAMI_PATH};

/// Environment variable holding the client's bearer token.
pub const TOKEN_ENV: &str = "NAIL_MCP_TOKEN";

/// Why the bridge stopped with an error. Messages are English: they go to
/// stderr of a process an MCP client started (developer tooling).
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// No token was given.
    #[error(
        "no MCP client token: set {TOKEN_ENV} to the token Nail showed when the client was created"
    )]
    NoToken,
    /// The app is not running (no endpoint file, or nothing listens).
    #[error("Nail is not running or its MCP server is off ({0})")]
    NotRunning(String),
    /// The token was refused.
    #[error("the MCP client token was refused (unknown or revoked); create a new client in Nail")]
    Unauthorized,
    /// The token belongs to another client.
    #[error("the token belongs to the MCP client {actual:?}, not {expected:?}")]
    WrongClient {
        /// `--client` as given.
        expected: String,
        /// The token's client.
        actual: String,
    },
    /// A transport failed while forwarding.
    #[error("MCP bridge transport failed: {0}")]
    Transport(String),
}

impl BridgeError {
    /// The user-facing text (`mcp-core-bridge-*` in `core.ftl`), which
    /// `nail mcp` prints; the English `Display` stays for logs.
    pub fn localized(&self) -> nail_api::LocalizedMessage {
        use nail_api::LocalizedMessage as M;
        match self {
            Self::NoToken => M::new("mcp-core-bridge-no-token").arg("variable", TOKEN_ENV),
            Self::NotRunning(detail) => {
                M::new("mcp-core-bridge-not-running").arg("detail", detail.as_str())
            }
            Self::Unauthorized => M::new("mcp-core-bridge-token-refused"),
            Self::WrongClient { expected, actual } => M::new("mcp-core-bridge-wrong-client")
                .arg("expected", expected.as_str())
                .arg("actual", actual.as_str()),
            Self::Transport(detail) => {
                M::new("mcp-core-bridge-transport").arg("detail", detail.as_str())
            }
        }
    }
}

/// Where and as whom to connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeOptions {
    /// Streamable HTTP URL (`http://127.0.0.1:<port>/mcp`).
    pub url: String,
    /// The client's bearer token.
    pub token: String,
    /// The client name the user expects (`--client`); checked against the
    /// token's client when given.
    pub client_name: Option<String>,
}

/// An HTTP client for loopback: no proxy (a system proxy must never see
/// the token), no overall timeout (the notification stream is long-lived).
pub fn loopback_http_client() -> Result<reqwest::Client, BridgeError> {
    let tls = nail_auth::tls_config().map_err(|e| BridgeError::Transport(e.to_string()))?;
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .no_proxy()
        .connect_timeout(std::time::Duration::from_secs(5))
        .user_agent(concat!("nail-mcp-bridge/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| BridgeError::Transport(e.to_string()))
}

/// Checks the token (and the client name) with `/whoami`.
pub async fn check_client(
    http: &reqwest::Client,
    options: &BridgeOptions,
) -> Result<String, BridgeError> {
    let base = options.url.strip_suffix(MCP_PATH).unwrap_or(&options.url);
    let response = http
        .get(format!("{base}{WHOAMI_PATH}"))
        .bearer_auth(&options.token)
        .send()
        .await
        .map_err(|e| BridgeError::NotRunning(e.without_url().to_string()))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(BridgeError::Unauthorized);
    }
    let body: serde_json::Value = response
        .error_for_status()
        .map_err(|e| BridgeError::Transport(e.without_url().to_string()))?
        .json()
        .await
        .map_err(|e| BridgeError::Transport(e.without_url().to_string()))?;
    let actual = body["name"].as_str().unwrap_or_default().to_owned();
    match &options.client_name {
        Some(expected) if expected != &actual => Err(BridgeError::WrongClient {
            expected: expected.clone(),
            actual,
        }),
        _ => Ok(actual),
    }
}

/// Forwards between `reader`/`writer` (the stdio client) and the app until
/// either side closes.
pub async fn run<R, W>(options: BridgeOptions, reader: R, writer: W) -> Result<(), BridgeError>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let http = loopback_http_client()?;
    check_client(&http, &options).await?;
    let mut upstream: StreamableHttpClientTransport<reqwest::Client> =
        StreamableHttpClientTransport::with_client(
            http,
            StreamableHttpClientTransportConfig::with_uri(options.url.clone())
                .auth_header(options.token.clone()),
        );
    let mut downstream: AsyncRwTransport<RoleServer, R, W> =
        AsyncRwTransport::new_server(reader, writer);
    let result = pump(&mut downstream, &mut upstream).await;
    drop(Transport::<RoleClient>::close(&mut upstream).await);
    drop(Transport::<RoleServer>::close(&mut downstream).await);
    result
}

async fn pump<Down, Up>(downstream: &mut Down, upstream: &mut Up) -> Result<(), BridgeError>
where
    Down: Transport<RoleServer>,
    Up: Transport<RoleClient>,
{
    loop {
        tokio::select! {
            from_client = downstream.receive() => {
                let Some(message) = from_client else { return Ok(()) };
                upstream
                    .send(message)
                    .await
                    .map_err(|e| BridgeError::Transport(e.to_string()))?;
            }
            from_app = upstream.receive() => {
                let Some(message) = from_app else {
                    return Err(BridgeError::Transport("the app closed the connection".into()));
                };
                downstream
                    .send(message)
                    .await
                    .map_err(|e| BridgeError::Transport(e.to_string()))?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_have_user_facing_texts() {
        let cases = [
            (BridgeError::NoToken, "mcp-core-bridge-no-token"),
            (
                BridgeError::NotRunning("refused".into()),
                "mcp-core-bridge-not-running",
            ),
            (BridgeError::Unauthorized, "mcp-core-bridge-token-refused"),
            (
                BridgeError::WrongClient {
                    expected: "a".into(),
                    actual: "b".into(),
                },
                "mcp-core-bridge-wrong-client",
            ),
            (
                BridgeError::Transport("reset".into()),
                "mcp-core-bridge-transport",
            ),
        ];
        for (error, id) in cases {
            let localized = error.localized();
            assert_eq!(localized.id, id);
            assert!(localized.is_valid());
        }
        assert_eq!(
            BridgeError::NoToken.localized().args["variable"],
            TOKEN_ENV.into()
        );
    }
}
