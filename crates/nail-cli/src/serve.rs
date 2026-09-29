//! `nail serve`.

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use anyhow::Context;
use nail_core::{Service, ServiceOptions, paths};
use nail_server::{Server, ServerOptions};

use crate::cli::{Seed, ServeArgs};

/// Environment variable naming the UI directory.
pub const UI_DIR_ENV: &str = "NAIL_UI_DIR";

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    let data_dir = match args.data_dir {
        Some(dir) => dir,
        None => paths::default_data_dir().context("no default data directory; pass --data-dir")?,
    };
    let config_dir = match args.config_dir {
        Some(dir) => dir,
        None => {
            paths::default_config_dir().context("no default config directory; pass --config-dir")?
        }
    };
    let ui_dir = resolve_ui_dir(
        args.ui_dir,
        std::env::var_os(UI_DIR_ENV).map(PathBuf::from),
        std::env::current_exe().ok().as_deref(),
    );
    tracing::info!(data = %data_dir.display(), config = %config_dir.display(), ui = ?ui_dir, "starting");

    let mut options = ServiceOptions::new(&data_dir, &config_dir);
    options.locale = args.locale;
    options.watch_config = !args.no_watch;
    let service = Service::open(options)
        .with_context(|| format!("cannot open the data directory {}", data_dir.display()))?;

    // Keeps the mock IMAP server alive for the lifetime of `serve`.
    let mut _demo_imap = None;
    match args.seed {
        Some(Seed::Demo) => {
            let seed = {
                let service = service.clone();
                tokio::task::spawn_blocking(move || {
                    nail_core::demo::seed_demo(service.store(), service.blobs())
                })
                .await??
            };
            tracing::info!(messages = seed.messages, "demo data loaded");
        }
        Some(Seed::DemoImap) => {
            let demo = nail_core::demo::start_demo_imap().await?;
            tracing::info!(addr = %demo.server.addr(), "demo IMAP server running");
            let id = demo.account.id.clone();
            service
                .add_imap_account(demo.account.clone(), demo.settings.clone())
                .await?;
            service.start_sync(Some(id)).await?;
            _demo_imap = Some(demo);
        }
        None => {}
    }

    let server = Server::bind(
        service,
        ServerOptions {
            addr: SocketAddr::from((Ipv4Addr::LOCALHOST, args.port)),
            ui_dir,
        },
    )
    .await?;
    let url = server.url();
    // The URL is the only thing written to stdout, so scripts (and the
    // Playwright harness) can read it.
    {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{url}")?;
        out.flush()?;
    }
    if args.open
        && let Err(err) = open::that_detached(&url)
    {
        tracing::warn!(%err, "cannot open the browser");
    }

    server
        .run_until(async {
            match tokio::signal::ctrl_c().await {
                Ok(()) => tracing::info!("shutting down"),
                Err(err) => tracing::error!(%err, "cannot listen for Ctrl+C"),
            }
        })
        .await?;
    Ok(())
}

/// The UI directory: `--ui-dir`, else `$NAIL_UI_DIR`, else `ui` next to the
/// executable, else the repository's `ui/dist` (development builds).
/// Candidates without an `index.html` are skipped (except an explicit
/// `--ui-dir`, which is used as given).
pub fn resolve_ui_dir(
    arg: Option<PathBuf>,
    env: Option<PathBuf>,
    exe: Option<&Path>,
) -> Option<PathBuf> {
    if arg.is_some() {
        return arg;
    }
    let beside_exe = exe.and_then(Path::parent).map(|dir| dir.join("ui"));
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    env.into_iter()
        .chain(beside_exe)
        .chain(std::iter::once(repo))
        .find(|dir| dir.join("index.html").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_ui_dir_wins() {
        assert_eq!(
            resolve_ui_dir(Some("x".into()), Some("y".into()), None),
            Some(PathBuf::from("x"))
        );
    }

    #[test]
    fn env_and_exe_dirs_need_an_index() {
        let env = tempfile::tempdir().unwrap();
        let exe_dir = tempfile::tempdir().unwrap();
        let exe = exe_dir.path().join("nail.exe");
        std::fs::create_dir(exe_dir.path().join("ui")).unwrap();
        std::fs::write(exe_dir.path().join("ui/index.html"), "x").unwrap();

        // The env dir has no index.html: the one beside the executable wins.
        assert_eq!(
            resolve_ui_dir(None, Some(env.path().into()), Some(&exe)),
            Some(exe_dir.path().join("ui"))
        );
        std::fs::write(env.path().join("index.html"), "x").unwrap();
        assert_eq!(
            resolve_ui_dir(None, Some(env.path().into()), Some(&exe)),
            Some(env.path().to_path_buf())
        );
    }
}
