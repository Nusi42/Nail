//! `nail mcp --client <name>`: the stdio bridge to the running app's MCP
//! server (ADR 0025).
//!
//! stdout carries only JSON-RPC messages; diagnostics go to stderr.

use std::path::PathBuf;

use anyhow::Context;
use nail_mcp::bridge::{self, BridgeOptions, TOKEN_ENV};
use nail_mcp::endpoint::{endpoint_path, read_endpoint};

use crate::cli::McpArgs;

/// Where to connect and with which token.
pub fn bridge_options(
    args: &McpArgs,
    token: Option<String>,
    default_data_dir: Option<PathBuf>,
) -> anyhow::Result<BridgeOptions> {
    let token = token
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .ok_or(bridge::BridgeError::NoToken)?;
    let url = match &args.url {
        Some(url) => url.clone(),
        None => {
            let dir = args
                .data_dir
                .clone()
                .or(default_data_dir)
                .context("no default data directory; pass --data-dir or --url")?;
            let path = endpoint_path(&dir);
            read_endpoint(&path)
                .map_err(|e| bridge::BridgeError::NotRunning(format!("{}: {e}", path.display())))?
                .url
        }
    };
    Ok(BridgeOptions {
        url,
        token,
        client_name: Some(args.client.clone()),
    })
}

/// Runs the bridge on stdin/stdout until the MCP client closes stdin.
pub async fn run(args: McpArgs) -> anyhow::Result<()> {
    let options = bridge_options(
        &args,
        std::env::var(TOKEN_ENV).ok(),
        nail_core::paths::default_data_dir(),
    )?;
    bridge::run(options, tokio::io::stdin(), tokio::io::stdout()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(url: Option<&str>, data_dir: Option<PathBuf>) -> McpArgs {
        McpArgs {
            client: "claude".into(),
            url: url.map(str::to_owned),
            data_dir,
        }
    }

    #[test]
    fn the_url_comes_from_the_flag_or_the_endpoint_file() {
        let options = bridge_options(
            &args(Some("http://127.0.0.1:5/mcp"), None),
            Some(" tok ".into()),
            None,
        )
        .unwrap();
        assert_eq!(options.url, "http://127.0.0.1:5/mcp");
        assert_eq!(options.token, "tok");
        assert_eq!(options.client_name.as_deref(), Some("claude"));

        let dir = tempfile::tempdir().unwrap();
        nail_mcp::endpoint::write_endpoint(
            &endpoint_path(dir.path()),
            &nail_mcp::endpoint::Endpoint {
                url: "http://127.0.0.1:6/mcp".into(),
                pid: 1,
            },
        )
        .unwrap();
        let options =
            bridge_options(&args(None, None), Some("t".into()), Some(dir.path().into())).unwrap();
        assert_eq!(options.url, "http://127.0.0.1:6/mcp");
    }

    #[test]
    fn missing_token_or_app_is_explained() {
        let err = bridge_options(&args(Some("http://x"), None), None, None).unwrap_err();
        assert!(err.to_string().contains(TOKEN_ENV), "{err}");
        let empty = tempfile::tempdir().unwrap();
        let err = bridge_options(
            &args(None, Some(empty.path().into())),
            Some("t".into()),
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("not running"), "{err}");
    }
}
