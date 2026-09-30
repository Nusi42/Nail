//! `nail serve`.

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nail_api::LocalizedMessage;
use nail_core::demo::DemoMulti;
use nail_core::{Service, ServiceOptions, paths};
use nail_server::{
    RemoteUiHost, RemoteUiOptions, Server, ServerOptions, TestControl, TransportOptions,
};
use nail_store::DataDirLock;

use crate::cli::{Seed, ServeArgs};
use crate::messages::{self, Localized};

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
            .localized(|| LocalizedMessage::new("cli-demo-temp-dir"))?;
        tracing::info!(dir = %dir.path().display(), "demo run in a temporary directory");
        Some(dir)
    } else {
        None
    };
    // The portable zip keeps everything beside the app (`bin\nail.exe`
    // next to the `portable` marker's directory).
    let portable = std::env::current_exe()
        .ok()
        .and_then(|exe| portable_root_beside(&exe));
    let pick =
        |given: Option<PathBuf>, sub: &str, default: fn() -> Option<PathBuf>, flag: &str| match (
            given, &temp, &portable,
        ) {
            (Some(dir), _, _) => Ok(dir),
            (None, Some(temp), _) => Ok(temp.path().join(sub)),
            (None, None, Some(root)) => Ok(root.join(sub)),
            (None, None, None) => default()
                .localized(|| LocalizedMessage::new("cli-no-default-dir").arg("flag", flag)),
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

/// Name of the portable build's marker file (as the desktop app's
/// `portable::MARKER`).
pub const PORTABLE_MARKER: &str = "portable";

/// The portable build's root for `nail` at `exe`: in the portable zip the
/// CLI is `<root>\bin\nail.exe` and the marker is `<root>\portable`; data
/// and config then default to `<root>\data` and `<root>\config`, like the
/// desktop app's.
pub fn portable_root_beside(exe: &Path) -> Option<PathBuf> {
    let root = exe.parent()?.parent()?;
    root.join(PORTABLE_MARKER)
        .is_file()
        .then(|| root.to_path_buf())
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
        return Err(messages::error(
            LocalizedMessage::new("cli-demo-real-accounts").arg("path", path.display().to_string()),
        ));
    }
    std::fs::create_dir_all(config_dir)
        .and_then(|()| std::fs::write(&path, demo.accounts_kdl()))
        .localized(|| {
            LocalizedMessage::new("cli-cannot-write").arg("path", path.display().to_string())
        })
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
    // One process per data directory (a headless `nail mcp` included).
    let lock = DataDirLock::acquire(&data_dir, "serve")?;
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

    // Seeded runs keep secrets in memory, trust only the test CA and look
    // up no keys online.
    if args.seed.is_some() {
        options = nail_core::demo::demo_options(options);
    }
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
    let service = Service::open(options).localized(|| {
        LocalizedMessage::new("cli-cannot-open-data-dir").arg("dir", data_dir.display().to_string())
    })?;

    if let Some(demo) = &demo_multi {
        demo.seed_security(&service).await?;
    }

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
        let settings = service.store().call(|s| s.mcp_settings()).await?;
        let port = nail_mcp::configured_port(&settings, args.mcp_port);
        let running = service
            .start_mcp_server(port, Some(endpoint))
            .await
            .map_err(|err| messages::mcp_start_failed(port, err.into()))?;
        tracing::info!(url = %running.url(), "MCP server running");
        Some(running)
    };

    let test_control = demo_multi
        .clone()
        .map(|demo| Arc::new(DemoControl(demo)) as Arc<dyn TestControl>);
    // The local UI: loopback, no login, with the remote-content proxy.
    let server = Server::bind_with(
        service.clone(),
        ServerOptions {
            addr: SocketAddr::from((Ipv4Addr::LOCALHOST, args.port)),
            ui_dir: ui_dir.clone(),
            test_control,
        },
        TransportOptions {
            proxy: service.remote_proxy(),
            ..TransportOptions::default()
        },
    )
    .await?;
    // The remote UI (TLS + login), when enabled in the settings; the
    // desktop's settings start, restart and stop it (ADR 0041).
    let remote_ui = RemoteUiHost::start(service.clone(), RemoteUiOptions::new(ui_dir));
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
    drop(remote_ui);
    if let Some(mcp) = mcp {
        mcp.shutdown().await;
    }
    service.shutdown().await;
    drop(demo_multi);
    drop(lock);
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
/// executable, else `ui` in its parent directory (the installed and
/// portable layouts: `<Nail>\bin\nail.exe` beside `<Nail>\ui`), else the
/// repository's `ui/dist` (development builds). Candidates without an
/// `index.html` are skipped (except an explicit `--ui-dir`, which is used
/// as given).
pub fn resolve_ui_dir(
    arg: Option<PathBuf>,
    env: Option<PathBuf>,
    exe: Option<&Path>,
) -> Option<PathBuf> {
    if arg.is_some() {
        return arg;
    }
    let exe_dir = exe.and_then(Path::parent);
    let beside_exe = exe_dir.map(|dir| dir.join("ui"));
    let install_root = exe_dir.and_then(Path::parent).map(|dir| dir.join("ui"));
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    env.into_iter()
        .chain(beside_exe)
        .chain(install_root)
        .chain(std::iter::once(repo))
        .find(|dir| dir.join("index.html").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portable_zip_keeps_data_beside_the_app() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("bin").join("nail.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        assert_eq!(portable_root_beside(&exe), None);
        std::fs::write(root.path().join(PORTABLE_MARKER), "").unwrap();
        assert_eq!(portable_root_beside(&exe), Some(root.path().to_path_buf()));
    }

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

    /// `<Nail>\bin\nail.exe` of the installer and the portable zip finds
    /// `<Nail>\ui`.
    #[test]
    fn the_installed_cli_finds_the_ui_of_its_install_root() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("bin").join("nail.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::create_dir(root.path().join("ui")).unwrap();
        std::fs::write(root.path().join("ui").join("index.html"), "x").unwrap();
        assert_eq!(
            resolve_ui_dir(None, None, Some(&exe)),
            Some(root.path().join("ui"))
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
