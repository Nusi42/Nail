//! `nail mcp --client <name>`: the stdio bridge to Nail's MCP server
//! (ADR 0025), starting the core headless when Nail is not running (M2c
//! WP53).
//!
//! 1. `--url` connects there.
//! 2. Otherwise the endpoint file of the data directory names the running
//!    instance's server; when it answers, the bridge connects to it.
//! 3. Otherwise (and unless `--no-start`) `nail mcp` takes the data
//!    directory's single-instance lock ([`nail_store::DataDirLock`]), opens
//!    the core headless (engines on, no UI), starts the MCP server on the
//!    port of the MCP settings (or `--mcp-port`, else a free one), writes
//!    the endpoint file and bridges to it. It stops when the MCP client
//!    closes stdin. When another process holds the lock without an MCP
//!    server, `nail mcp` says so instead of opening the data twice.
//!
//! stdout carries only JSON-RPC messages; notices and errors go to stderr.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use nail_api::LocalizedMessage;
use nail_core::{Service, ServiceOptions};
use nail_mcp::bridge::{self, BridgeError, BridgeOptions, TOKEN_ENV};
use nail_mcp::endpoint::{endpoint_path, read_endpoint};
use nail_store::{DataDirLock, LockError};

use crate::cli::McpArgs;
use crate::messages::{self, Localized};
use crate::serve::{Dirs, resolve_dirs};

/// How long probing a running instance's endpoint may take.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Where the bridge connects.
#[derive(Debug)]
pub enum Target {
    /// A running server.
    Url(String),
    /// Nobody runs: start the core headless (holding its lock).
    Start(DataDirLock),
}

/// The client token from `$NAIL_MCP_TOKEN` (trimmed; required).
pub fn token(value: Option<String>) -> Result<String, BridgeError> {
    value
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .ok_or(BridgeError::NoToken)
}

/// Whether a loopback MCP URL (`http://127.0.0.1:<port>/mcp`) accepts
/// connections. Other hosts count as unreachable: the endpoint file only
/// ever names loopback, and `nail mcp` connects nowhere else on its own.
pub fn loopback_reachable(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let Ok(mut addrs) = authority.to_socket_addrs() else {
        return false;
    };
    addrs.any(|addr: SocketAddr| {
        addr.ip().is_loopback() && TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok()
    })
}

/// Decides where to connect (see the module docs). `reachable` probes an
/// endpoint URL.
pub fn choose_target(
    url: Option<&str>,
    data_dir: &Path,
    no_start: bool,
    reachable: impl Fn(&str) -> bool,
) -> anyhow::Result<Target> {
    if let Some(url) = url {
        return Ok(Target::Url(url.to_owned()));
    }
    let endpoint_file = endpoint_path(data_dir);
    let endpoint = read_endpoint(&endpoint_file).ok();
    if let Some(endpoint) = &endpoint
        && reachable(&endpoint.url)
    {
        return Ok(Target::Url(endpoint.url.clone()));
    }
    if no_start {
        return Err(BridgeError::NotRunning(match &endpoint {
            Some(e) => format!("{} does not answer", e.url),
            None => format!("no {}", endpoint_file.display()),
        })
        .into());
    }
    match DataDirLock::acquire(data_dir, "mcp") {
        Ok(lock) => Ok(Target::Start(lock)),
        Err(LockError::Held(holder)) => Err(messages::error(match holder {
            Some(h) => LocalizedMessage::new("cli-mcp-instance-without-mcp")
                .arg("owner", h.owner.as_str())
                .arg("pid", h.pid),
            None => LocalizedMessage::new("cli-mcp-instance-without-mcp-unknown"),
        })),
        Err(err) => Err(err.into()),
    }
}

fn bridge_options(url: String, token: String, args: &McpArgs) -> BridgeOptions {
    BridgeOptions {
        url,
        token,
        client_name: Some(args.client.clone()),
    }
}

/// Runs the bridge on stdin/stdout until the MCP client closes stdin.
pub async fn run(args: McpArgs) -> anyhow::Result<()> {
    let token = token(std::env::var(TOKEN_ENV).ok())?;
    let dirs = if args.url.is_some() {
        None
    } else {
        Some(resolve_dirs(
            args.data_dir.clone(),
            args.config_dir.clone(),
            None,
        )?)
    };
    let data_dir = dirs.as_ref().map(|d| d.data.clone()).unwrap_or_default();
    let target = choose_target(args.url.as_deref(), &data_dir, args.no_start, |url| {
        loopback_reachable(url)
    })?;
    match (target, dirs) {
        (Target::Url(url), _) => {
            bridge::run(
                bridge_options(url, token, &args),
                tokio::io::stdin(),
                tokio::io::stdout(),
            )
            .await?;
            Ok(())
        }
        (Target::Start(lock), Some(dirs)) => run_headless(&args, dirs, lock, token).await,
        (Target::Start(_), None) => unreachable!("a URL never starts the core"),
    }
}

/// The headless core with its MCP server, bridged to stdio.
async fn run_headless(
    args: &McpArgs,
    dirs: Dirs,
    lock: DataDirLock,
    token: String,
) -> anyhow::Result<()> {
    let catalog = messages::catalog();
    let options = ServiceOptions::new(&dirs.data, &dirs.config);
    let service = Service::open(options).localized(|| {
        LocalizedMessage::new("cli-cannot-open-data-dir")
            .arg("dir", dirs.data.display().to_string())
    })?;
    let settings = service.store().call(|s| s.mcp_settings()).await?;
    let port = nail_mcp::configured_port(&settings, args.mcp_port);
    let running = match service
        .start_mcp_server(port, Some(endpoint_path(&dirs.data)))
        .await
    {
        Ok(running) => running,
        Err(err) => {
            service.shutdown().await;
            return Err(messages::mcp_start_failed(port, err.into()));
        }
    };
    eprintln!(
        "{}",
        catalog.render(
            &LocalizedMessage::new("cli-mcp-headless-started")
                .arg("dir", dirs.data.display().to_string())
                .arg("url", running.url())
        )
    );
    let result = bridge::run(
        bridge_options(running.url(), token, args),
        tokio::io::stdin(),
        tokio::io::stdout(),
    )
    .await;
    running.shutdown().await;
    service.shutdown().await;
    drop(lock);
    drop(dirs);
    result.map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_mcp::endpoint::{Endpoint, write_endpoint};

    fn write(dir: &Path, url: &str) {
        write_endpoint(
            &endpoint_path(dir),
            &Endpoint {
                url: url.into(),
                pid: 1,
            },
        )
        .unwrap();
    }

    #[test]
    fn the_token_is_required() {
        assert_eq!(token(Some(" tok ".into())).unwrap(), "tok");
        assert!(matches!(token(None), Err(BridgeError::NoToken)));
        assert!(matches!(
            token(Some("  ".into())),
            Err(BridgeError::NoToken)
        ));
    }

    #[test]
    fn a_url_or_a_live_endpoint_is_used_as_is() {
        let dir = tempfile::tempdir().unwrap();
        match choose_target(Some("http://127.0.0.1:5/mcp"), dir.path(), false, |_| false) {
            Ok(Target::Url(url)) => assert_eq!(url, "http://127.0.0.1:5/mcp"),
            other => panic!("{other:?}"),
        }
        write(dir.path(), "http://127.0.0.1:6/mcp");
        match choose_target(None, dir.path(), false, |_| true) {
            Ok(Target::Url(url)) => assert_eq!(url, "http://127.0.0.1:6/mcp"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nobody_running_starts_headless_once() {
        let dir = tempfile::tempdir().unwrap();
        // A stale endpoint file of a crashed instance does not count.
        write(dir.path(), "http://127.0.0.1:6/mcp");
        let Ok(Target::Start(lock)) = choose_target(None, dir.path(), false, |_| false) else {
            panic!("expected a headless start");
        };
        assert_eq!(lock.info().owner, "mcp");
        // A second `nail mcp` while the first starts: the lock is held.
        let err = choose_target(None, dir.path(), false, |_| false).unwrap_err();
        let text = messages::render_error(&err, &nail_config::l10n::Catalog::english());
        assert!(text.contains("mcp") && text.contains("MCP"), "{text}");
        drop(lock);
        assert!(matches!(
            choose_target(None, dir.path(), false, |_| false),
            Ok(Target::Start(_))
        ));
    }

    #[test]
    fn no_start_only_connects() {
        let dir = tempfile::tempdir().unwrap();
        let err = choose_target(None, dir.path(), true, |_| false).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<BridgeError>(),
            Some(BridgeError::NotRunning(_))
        ));
        assert!(err.to_string().contains("not running"), "{err}");
    }

    #[test]
    fn only_loopback_endpoints_are_probed() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(loopback_reachable(&format!("http://127.0.0.1:{port}/mcp")));
        drop(listener);
        assert!(!loopback_reachable("https://127.0.0.1:1/mcp"));
        assert!(!loopback_reachable("http://192.0.2.1:80/mcp"));
        assert!(!loopback_reachable("not a url"));
    }
}
