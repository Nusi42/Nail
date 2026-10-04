//! `nail mcp --client <name>`: the stdio bridge to Nail's MCP server
//! (ADR 0025), starting the core headless when Nail is not running (M2c
//! WP53).
//!
//! 1. `--url` connects there.
//! 2. Otherwise the data directory's single-instance lock
//!    ([`nail_store::DataDirLock`]) tells whether Nail runs — before the
//!    endpoint file is trusted: a file left by a crashed instance could
//!    name a port another local process listens on now, which would get
//!    the client's token.
//!    * The lock is free: nobody runs. A left-over endpoint file is
//!      removed; unless `--no-start`, `nail mcp` keeps the lock, opens the
//!      core headless (engines on, no UI), starts the MCP server on the
//!      port of the MCP settings (or `--mcp-port`, else a free one), writes
//!      the endpoint file and bridges to it. It stops when the MCP client
//!      closes stdin.
//!    * The lock is held: the holder's endpoint file (written by that
//!      process) names its server; `nail mcp` waits a few seconds for it
//!      (the holder may still be starting) and bridges to it. A holder
//!      without an MCP server is reported instead of opening the data
//!      twice.
//!
//! stdout carries only JSON-RPC messages; notices and errors go to stderr.
//!
//! Confirmations (M4a, ADR 0079): the bridge is a transparent pump, so a
//! write's elicitation form (in-band for sessions, MRTR rounds for
//! stateless clients) reaches the MCP client and its answer comes back
//! unchanged. A headless core has no window to confirm in: `nail mcp`
//! prints each pending confirmation to stderr ([`confirmation_notice`]);
//! the client's form can approve what elicitation may approve, everything
//! else expires.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use nail_api::agents::Confirmation;
use nail_api::{Event, LocalizedMessage};
use nail_config::l10n::Catalog;
use nail_core::{Service, ServiceOptions};
use nail_mcp::bridge::{self, BridgeError, BridgeOptions, TOKEN_ENV};
use nail_mcp::endpoint::{endpoint_path, read_endpoint};
use nail_store::{DataDirLock, LockError};

use crate::cli::McpArgs;
use crate::messages::{self, Localized};
use crate::serve::{Dirs, resolve_dirs};

/// How long probing a running instance's endpoint may take.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
/// How long `nail mcp` waits for a starting instance's endpoint.
pub(crate) const ENDPOINT_WAIT: Duration = Duration::from_secs(10);

/// Where the bridge connects.
#[derive(Debug)]
pub(crate) enum Target {
    /// A running server.
    Url(String),
    /// Nobody runs: start the core headless (holding its lock).
    Start(DataDirLock),
}

/// The client token from `$NAIL_MCP_TOKEN` (trimmed; required).
pub(crate) fn token(value: Option<String>) -> Result<String, BridgeError> {
    value
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .ok_or(BridgeError::NoToken)
}

/// Whether a loopback MCP URL (`http://127.0.0.1:<port>/mcp`) accepts
/// connections. Other hosts count as unreachable: the endpoint file only
/// ever names loopback, and `nail mcp` connects nowhere else on its own.
pub(crate) fn loopback_reachable(url: &str) -> bool {
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
/// endpoint URL; `wait` is how long a held lock's endpoint is waited for.
pub(crate) fn choose_target(
    url: Option<&str>,
    data_dir: &Path,
    no_start: bool,
    wait: Duration,
    reachable: impl Fn(&str) -> bool,
) -> anyhow::Result<Target> {
    if let Some(url) = url {
        return Ok(Target::Url(url.to_owned()));
    }
    let endpoint_file = endpoint_path(data_dir);
    match DataDirLock::acquire(data_dir, "mcp") {
        Ok(lock) => {
            // Nobody runs: an endpoint file is a crashed instance's.
            if endpoint_file.exists() {
                drop(std::fs::remove_file(&endpoint_file));
            }
            if no_start {
                drop(lock);
                return Err(BridgeError::NotRunning(format!(
                    "no running instance for {}",
                    data_dir.display()
                ))
                .into());
            }
            Ok(Target::Start(lock))
        }
        Err(LockError::Held(holder)) => {
            let deadline = std::time::Instant::now() + wait;
            loop {
                // Only an endpoint of the lock holder counts (when it says
                // who it is): an older file may name another process.
                if let Ok(endpoint) = read_endpoint(&endpoint_file)
                    && holder.as_ref().is_none_or(|h| h.pid == endpoint.pid)
                    && reachable(&endpoint.url)
                {
                    return Ok(Target::Url(endpoint.url));
                }
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200).min(wait));
            }
            if no_start {
                return Err(BridgeError::NotRunning(format!(
                    "{} does not answer",
                    endpoint_file.display()
                ))
                .into());
            }
            Err(messages::error(match holder {
                Some(h) => LocalizedMessage::new("cli-mcp-instance-without-mcp")
                    .arg("owner", h.owner.as_str())
                    .arg("pid", h.pid),
                None => LocalizedMessage::new("cli-mcp-instance-without-mcp-unknown"),
            }))
        }
        Err(err) => Err(err.into()),
    }
}

/// The stderr notice of a confirmation a headless `nail mcp` waits for.
pub(crate) fn confirmation_notice(catalog: &Catalog, confirmation: &Confirmation) -> String {
    catalog.render(
        &LocalizedMessage::new("mcp-tool-bridge-confirmation-pending")
            .arg("actor", confirmation.requested_by.name.as_str())
            .arg("summary", catalog.render(&confirmation.summary)),
    )
}

/// Prints every new confirmation to stderr until the event stream ends.
async fn print_confirmations(
    mut events: tokio::sync::broadcast::Receiver<Event>,
    catalog: Catalog,
) {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        match events.recv().await {
            Ok(Event::ConfirmationRequested { confirmation }) => {
                eprintln!("{}", confirmation_notice(&catalog, &confirmation));
            }
            Ok(_) | Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => return,
        }
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
pub(crate) async fn run(args: McpArgs) -> anyhow::Result<()> {
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
    let target = choose_target(
        args.url.as_deref(),
        &data_dir,
        args.no_start,
        ENDPOINT_WAIT,
        loopback_reachable,
    )?;
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
    let settings = service
        .store()
        .call(nail_store::Store::mcp_settings)
        .await?;
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
    let notices = tokio::spawn(print_confirmations(
        service.subscribe(),
        messages::catalog(),
    ));
    let result = bridge::run(
        bridge_options(running.url(), token, args),
        tokio::io::stdin(),
        tokio::io::stdout(),
    )
    .await;
    notices.abort();
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
        write_as(dir, url, std::process::id());
    }

    fn write_as(dir: &Path, url: &str, pid: u32) {
        write_endpoint(
            &endpoint_path(dir),
            &Endpoint {
                url: url.into(),
                pid,
            },
        )
        .unwrap();
    }

    const NO_WAIT: Duration = Duration::ZERO;

    #[test]
    fn pending_confirmations_are_announced_on_stderr() {
        use nail_api::agents::{ActionActor, ActionAttribution, ActionKind, ConfirmationState};
        let confirmation = Confirmation {
            id: "1".into(),
            requested_by: ActionAttribution {
                actor: ActionActor::mcp_client("c1"),
                name: "Claude".into(),
                on_behalf_of: None,
            },
            kind: ActionKind::Send,
            operation: "send_message".into(),
            summary: LocalizedMessage::new("confirmation-summary-send")
                .arg("actor", "Claude")
                .arg("subject", "Plan")
                .arg("count", 2),
            targets: vec![],
            more_targets: 0,
            changes: vec![],
            account_ids: vec![],
            created_at: String::new(),
            expires_at: String::new(),
            state: ConfirmationState::Pending,
            decided_at: None,
            decided_via: None,
            can_always_allow: true,
            error: None,
        };
        let text = confirmation_notice(&Catalog::english(), &confirmation);
        assert!(
            text.starts_with("Claude is waiting for your confirmation"),
            "{text}"
        );
        assert!(text.contains("2 recipients"), "{text}");
        assert!(!text.contains("mcp-tool-"), "{text}");
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
    fn a_url_or_the_running_instances_endpoint_is_used_as_is() {
        let dir = tempfile::tempdir().unwrap();
        match choose_target(
            Some("http://127.0.0.1:5/mcp"),
            dir.path(),
            false,
            NO_WAIT,
            |_| false,
        ) {
            Ok(Target::Url(url)) => assert_eq!(url, "http://127.0.0.1:5/mcp"),
            other => panic!("{other:?}"),
        }
        // A running instance (it holds the lock) wrote its endpoint.
        let running = DataDirLock::acquire(dir.path(), "desktop").unwrap();
        write(dir.path(), "http://127.0.0.1:6/mcp");
        match choose_target(None, dir.path(), false, NO_WAIT, |_| true) {
            Ok(Target::Url(url)) => assert_eq!(url, "http://127.0.0.1:6/mcp"),
            other => panic!("{other:?}"),
        }
        drop(running);
    }

    #[test]
    fn a_stale_endpoint_is_never_used_even_when_something_answers() {
        let dir = tempfile::tempdir().unwrap();
        // A crashed instance left its file; another process now listens on
        // that port (`reachable` says yes). The free lock shows nobody
        // runs: the file goes, and `nail mcp` starts its own core.
        write(dir.path(), "http://127.0.0.1:6/mcp");
        let Ok(Target::Start(lock)) = choose_target(None, dir.path(), false, NO_WAIT, |_| true)
        else {
            panic!("expected a headless start");
        };
        assert!(!endpoint_path(dir.path()).exists(), "stale file removed");
        drop(lock);
        // A running instance with an endpoint file of another process.
        let running = DataDirLock::acquire(dir.path(), "desktop").unwrap();
        write_as(dir.path(), "http://127.0.0.1:7/mcp", std::process::id() + 1);
        assert!(choose_target(None, dir.path(), false, NO_WAIT, |_| true).is_err());
        drop(running);
    }

    #[test]
    fn a_starting_instances_endpoint_is_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let running = DataDirLock::acquire(dir.path(), "mcp").unwrap();
        let path = dir.path().to_owned();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            write(&path, "http://127.0.0.1:8/mcp");
        });
        match choose_target(None, dir.path(), false, Duration::from_secs(10), |_| true) {
            Ok(Target::Url(url)) => assert_eq!(url, "http://127.0.0.1:8/mcp"),
            other => panic!("{other:?}"),
        }
        writer.join().unwrap();
        drop(running);
    }

    #[test]
    fn nobody_running_starts_headless_once() {
        let dir = tempfile::tempdir().unwrap();
        let Ok(Target::Start(lock)) = choose_target(None, dir.path(), false, NO_WAIT, |_| false)
        else {
            panic!("expected a headless start");
        };
        assert_eq!(lock.info().owner, "mcp");
        // A second `nail mcp` while the first runs without an endpoint.
        let err = choose_target(None, dir.path(), false, NO_WAIT, |_| false).unwrap_err();
        let text = messages::render_error(&err, &Catalog::english());
        assert!(text.contains("mcp") && text.contains("MCP"), "{text}");
        drop(lock);
        assert!(matches!(
            choose_target(None, dir.path(), false, NO_WAIT, |_| false),
            Ok(Target::Start(_))
        ));
    }

    #[test]
    fn no_start_only_connects() {
        let dir = tempfile::tempdir().unwrap();
        let err = choose_target(None, dir.path(), true, NO_WAIT, |_| false).unwrap_err();
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
