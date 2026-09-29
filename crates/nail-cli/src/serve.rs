//! `nail serve`.

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use nail_core::demo::DemoMulti;
use nail_core::{Service, ServiceOptions, paths};
use nail_server::{Server, ServerOptions, TestControl};

use crate::cli::{Seed, ServeArgs};

/// Environment variable naming the UI directory.
pub const UI_DIR_ENV: &str = "NAIL_UI_DIR";

/// The directories of a run. A seeded (demo) run never falls back to the
/// real data and config directories: missing ones are temporary and go
/// away with this value.
pub struct Dirs {
    pub data: PathBuf,
    pub config: PathBuf,
    _temp: Option<tempfile::TempDir>,
}

/// `--data-dir` / `--config-dir`, else temporary directories for a demo
/// seed, else the platform defaults.
pub fn resolve_dirs(
    data_dir: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    seed: Option<Seed>,
) -> anyhow::Result<Dirs> {
    let temp = if seed.is_some() && (data_dir.is_none() || config_dir.is_none()) {
        let dir = tempfile::Builder::new()
            .prefix("nail-demo-")
            .tempdir()
            .context("cannot create a temporary directory for the demo")?;
        tracing::info!(dir = %dir.path().display(), "demo run in a temporary directory");
        Some(dir)
    } else {
        None
    };
    let pick =
        |given: Option<PathBuf>, sub: &str, default: fn() -> Option<PathBuf>, flag: &str| match (
            given, &temp,
        ) {
            (Some(dir), _) => Ok(dir),
            (None, Some(temp)) => Ok(temp.path().join(sub)),
            (None, None) => {
                default().with_context(|| format!("no default {sub} directory; pass {flag}"))
            }
        };
    Ok(Dirs {
        data: pick(data_dir, "data", paths::default_data_dir, "--data-dir")?,
        config: pick(
            config_dir,
            "config",
            paths::default_config_dir,
            "--config-dir",
        )?,
        _temp: temp,
    })
}

/// Writes the demo-multi `accounts.kdl` into `config_dir`. Refuses a
/// directory whose `accounts.kdl` is not a demo one: the run would replace
/// its accounts' secrets with the in-memory demo keychain and write demo
/// state next to real accounts. An earlier demo file is replaced (its mock
/// server ports are gone).
pub fn write_demo_multi_config(demo: &DemoMulti, config_dir: &Path) -> anyhow::Result<()> {
    let path = config_dir.join("accounts.kdl");
    if let Ok(existing) = std::fs::read_to_string(&path)
        && !existing.starts_with(nail_core::demo::MULTI_CONFIG_MARKER)
    {
        anyhow::bail!(
            "{} holds real accounts; --seed demo-multi needs an empty --config-dir (or none, for a temporary one)",
            path.display()
        );
    }
    std::fs::create_dir_all(config_dir)
        .and_then(|()| std::fs::write(&path, demo.accounts_kdl()))
        .with_context(|| format!("cannot write {}", path.display()))
}

/// What a `nail serve` session may start on this machine. Seeded (demo,
/// test) runs open no browser. No run opens an editor: the UI runs in a
/// browser that may be on another machine, so `OpenConfigFile` answers
/// `unsupported` there (ADR 0026, userDocs "customization").
fn browser_mode(options: &mut ServiceOptions, seed: Option<Seed>) {
    options.open_browser = seed.is_none();
    options.open_editor = false;
}

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    let dirs = resolve_dirs(args.data_dir, args.config_dir, args.seed)?;
    let (data_dir, config_dir) = (dirs.data.clone(), dirs.config.clone());
    let ui_dir = resolve_ui_dir(
        args.ui_dir,
        std::env::var_os(UI_DIR_ENV).map(PathBuf::from),
        std::env::current_exe().ok().as_deref(),
    );
    tracing::info!(data = %data_dir.display(), config = %config_dir.display(), ui = ?ui_dir, "starting");

    let mut options = ServiceOptions::new(&data_dir, &config_dir);
    options.locale = args.locale;
    options.watch_config = !args.no_watch;
    browser_mode(&mut options, args.seed);

    // demo-multi: mock servers first, the service is pointed at them.
    let mut demo_multi = None;
    if args.seed == Some(Seed::DemoMulti) {
        let demo = nail_core::demo::start_demo_multi().await?;
        write_demo_multi_config(&demo, &config_dir)?;
        options = demo.service_options(options);
        tracing::info!(
            imap = %demo.work.addr(),
            pop3 = %demo.home.addr(),
            smtp = %demo.smtp.addr(),
            "demo-multi mock servers running"
        );
        demo_multi = Some(Arc::new(demo));
    }
    let service = Service::open(options)
        .with_context(|| format!("cannot open the data directory {}", data_dir.display()))?;

    // Keeps the mock IMAP server alive for the lifetime of `serve`.
    let mut _demo_imap = None;
    match args.seed {
        Some(Seed::Demo) => {
            let seed = {
                let service = service.clone();
                tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                    let seed = nail_core::demo::seed_demo(service.store(), service.blobs())?;
                    // The seed goes straight into the store: index it too.
                    service.rebuild_search_index()?;
                    Ok(seed)
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
        Some(Seed::DemoMulti) | None => {}
    }

    // The MCP server (ADR 0025), with the core as its data provider; agents
    // find it through the endpoint file in the data directory.
    let mcp = if args.no_mcp {
        None
    } else {
        let endpoint = nail_mcp::endpoint::endpoint_path(&data_dir);
        let running = service
            .start_mcp_server(args.mcp_port, Some(endpoint))
            .await
            .context("cannot start the MCP server")?;
        tracing::info!(url = %running.url(), "MCP server running");
        Some(running)
    };

    let test_control = demo_multi
        .clone()
        .map(|demo| Arc::new(DemoControl(demo)) as Arc<dyn TestControl>);
    let server = Server::bind(
        service.clone(),
        ServerOptions {
            addr: SocketAddr::from((Ipv4Addr::LOCALHOST, args.port)),
            ui_dir,
            test_control,
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
    if let Some(mcp) = mcp {
        mcp.shutdown().await;
    }
    service.shutdown().await;
    drop(demo_multi);
    drop(dirs);
    Ok(())
}

/// The demo-multi mock servers as the server's test hooks.
struct DemoControl(Arc<DemoMulti>);

impl TestControl for DemoControl {
    fn control(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.0.control(command, args)
    }
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

    /// Review M1a: a seeded run without directories must not touch the
    /// real data and config directories.
    #[test]
    fn seeded_runs_default_to_temporary_directories() {
        for seed in [Seed::Demo, Seed::DemoImap, Seed::DemoMulti] {
            let dirs = resolve_dirs(None, None, Some(seed)).unwrap();
            assert_ne!(Some(&dirs.data), paths::default_data_dir().as_ref());
            assert_ne!(Some(&dirs.config), paths::default_config_dir().as_ref());
            assert!(
                dirs.data.starts_with(std::env::temp_dir()),
                "{:?}",
                dirs.data
            );
            let root = dirs.data.parent().unwrap().to_path_buf();
            assert!(root.is_dir());
            drop(dirs);
            assert!(!root.exists(), "the temporary directory is removed");
        }
        // Explicit directories are used as given; only the missing one is
        // temporary.
        let dirs = resolve_dirs(Some("d".into()), None, Some(Seed::Demo)).unwrap();
        assert_eq!(dirs.data, PathBuf::from("d"));
        assert!(dirs.config.starts_with(std::env::temp_dir()));
        let dirs = resolve_dirs(Some("d".into()), Some("c".into()), Some(Seed::Demo)).unwrap();
        assert_eq!((dirs.data, dirs.config), ("d".into(), "c".into()));
        // Without a seed: the platform defaults.
        let dirs = resolve_dirs(None, None, None).unwrap();
        assert_eq!(Some(dirs.data), paths::default_data_dir());
    }

    #[tokio::test]
    async fn demo_multi_refuses_real_accounts() {
        let demo = nail_core::demo::start_demo_multi().await.unwrap();
        let config = tempfile::tempdir().unwrap();
        let path = config.path().join("accounts.kdl");
        // Empty directory: written.
        write_demo_multi_config(&demo, config.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), demo.accounts_kdl());
        // An earlier demo file: replaced (new ports).
        std::fs::write(
            &path,
            format!("{}\nold", nail_core::demo::MULTI_CONFIG_MARKER),
        )
        .unwrap();
        write_demo_multi_config(&demo, config.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), demo.accounts_kdl());
        // Real accounts: refused and left alone.
        let real = "account \"me\" email=\"me@example.org\" { imap host=\"imap.example.org\" }\n";
        std::fs::write(&path, real).unwrap();
        let err = write_demo_multi_config(&demo, config.path()).unwrap_err();
        assert!(err.to_string().contains("real accounts"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), real);
    }

    #[test]
    fn serve_never_opens_an_editor() {
        for seed in [None, Some(Seed::Demo)] {
            let mut options = ServiceOptions::new("d", "c");
            options.open_editor = true;
            browser_mode(&mut options, seed);
            assert!(!options.open_editor, "{seed:?}");
            assert_eq!(options.open_browser, seed.is_none());
        }
    }

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
