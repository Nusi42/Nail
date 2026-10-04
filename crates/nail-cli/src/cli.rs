//! Command-line arguments.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Default port of `nail serve`.
pub(crate) const DEFAULT_PORT: u16 = 4780;

/// Nail: email and PIM client.
#[derive(Debug, Parser)]
#[command(name = "nail", version, about)]
pub(crate) struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Run the core headless and serve the web UI on localhost.
    Serve(ServeArgs),
    /// Connect a stdio MCP client to Nail (forwards to its MCP server on
    /// 127.0.0.1); starts Nail headless when it is not running. The
    /// client's token is read from $`NAIL_MCP_TOKEN`.
    Mcp(McpArgs),
    /// Plug-in tools (M2d).
    Plugin {
        /// The plug-in tool.
        #[command(subcommand)]
        command: crate::plugin::PluginCommand,
    },
    /// Configuration tools.
    Config {
        /// The config tool.
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

/// `nail config …`
#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Print the JSON Schema of theme.kdl, layout.kdl or accounts.kdl (JSON
    /// form of the file model, for editor tooling).
    Schema {
        /// Which file.
        #[arg(value_enum, default_value_t = SchemaKind::Theme)]
        kind: SchemaKind,
    },
}

/// The configuration file a schema describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SchemaKind {
    /// theme.kdl
    Theme,
    /// layout.kdl
    Layout,
    /// accounts.kdl
    Accounts,
}

/// Demo data for `nail serve --seed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Seed {
    /// Load the demo mail corpus as a local account (no network).
    Demo,
    /// Serve the demo corpus from an in-process mock IMAP server and sync
    /// it like a real account.
    DemoImap,
    /// Two configured accounts (IMAP with IDLE, POP3) and SMTP on
    /// in-process mock servers, plus a discoverable third account; writes
    /// its accounts.kdl (refusing a config directory with other accounts),
    /// keeps secrets in memory, and exposes the test controls at
    /// /api/test/… (for end-to-end tests).
    DemoMulti,
}

/// `nail serve …`
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct ServeArgs {
    /// Port on 127.0.0.1 (0 picks a free port; the URL is printed).
    /// Default: 4780, or a free port when 4780 is taken. An explicit port
    /// that is taken is an error.
    #[arg(long)]
    pub port: Option<u16>,
    /// Data directory (store and message blobs). Default: %LOCALAPPDATA%\Nail.
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
    /// Config directory (theme.kdl, layout.kdl). Default: %APPDATA%\Nail.
    #[arg(long, value_name = "DIR")]
    pub config_dir: Option<PathBuf>,
    /// Directory of the built UI (ui/dist). Default: $`NAIL_UI_DIR`, `ui`
    /// next to the executable, or the repository's ui/dist.
    #[arg(long, value_name = "DIR")]
    pub ui_dir: Option<PathBuf>,
    /// Load demo data before serving. Without --data-dir / --config-dir,
    /// a seeded run uses temporary directories (removed on exit), never
    /// the real ones.
    #[arg(long, value_enum)]
    pub seed: Option<Seed>,
    /// UI language override (BCP 47, e.g. "de"); default: the browser's.
    #[arg(long, value_name = "TAG")]
    pub locale: Option<String>,
    /// Open the UI in the default browser.
    #[arg(long)]
    pub open: bool,
    /// Do not watch theme.kdl / layout.kdl for changes.
    #[arg(long)]
    pub no_watch: bool,
    /// Port of the MCP server on 127.0.0.1 (0 picks a free port; agents
    /// find it in mcp-endpoint.json of the data directory). Default: the fixed port of
    /// the MCP settings, else a free port.
    #[arg(long, value_name = "PORT")]
    pub mcp_port: Option<u16>,
    /// Do not start the MCP server.
    #[arg(long)]
    pub no_mcp: bool,
    /// What to open, as the desktop app does when Windows starts it with a
    /// file or link: an `.ics`, `.vcf` or `.eml` file, a `mailto:` or
    /// `webcal:` link (repeatable). The UI takes them at start.
    #[arg(long, value_name = "FILE_OR_LINK")]
    pub launch: Vec<String>,
    /// More log output (-v debug, -vv trace). `NAIL_LOG` overrides.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

/// `nail mcp …`
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct McpArgs {
    /// Name of the MCP client as created in Settings → MCP clients; checked
    /// against the token.
    #[arg(long, value_name = "NAME")]
    pub client: String,
    /// MCP endpoint URL. Default: read from the running app's data
    /// directory (mcp-endpoint.json).
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    /// Data directory. Default: %LOCALAPPDATA%\Nail.
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
    /// Config directory for a headless start. Default: %APPDATA%\Nail.
    #[arg(long, value_name = "DIR")]
    pub config_dir: Option<PathBuf>,
    /// Only connect to a running Nail; never start it headless.
    #[arg(long)]
    pub no_start: bool,
    /// Port of the MCP server of a headless start (0 = any free port).
    /// Default: the fixed port of the MCP settings, else any free port.
    #[arg(long, value_name = "PORT")]
    pub mcp_port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use pretty_assertions::assert_eq;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("nail").chain(args.iter().copied()))
    }

    fn serve(args: &[&str]) -> ServeArgs {
        match parse(args).unwrap().command {
            Command::Serve(serve) => serve,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn serve_defaults() {
        assert_eq!(
            serve(&["serve"]),
            ServeArgs {
                port: None,
                data_dir: None,
                config_dir: None,
                ui_dir: None,
                seed: None,
                locale: None,
                open: false,
                no_watch: false,
                mcp_port: None,
                no_mcp: false,
                launch: Vec::new(),
                verbose: 0,
            }
        );
    }

    #[test]
    fn serve_with_everything() {
        assert_eq!(
            serve(&[
                "serve",
                "--port",
                "0",
                "--data-dir",
                "d",
                "--config-dir",
                "c",
                "--ui-dir",
                "u",
                "--seed",
                "demo",
                "--locale",
                "de",
                "--open",
                "--no-watch",
                "--mcp-port",
                "4799",
                "--no-mcp",
                "--launch",
                "C:/cal.ics",
                "--launch",
                "webcal://example.org/h.ics",
                "-vv",
            ]),
            ServeArgs {
                port: Some(0),
                data_dir: Some("d".into()),
                config_dir: Some("c".into()),
                ui_dir: Some("u".into()),
                seed: Some(Seed::Demo),
                locale: Some("de".into()),
                open: true,
                no_watch: true,
                mcp_port: Some(4799),
                no_mcp: true,
                launch: vec!["C:/cal.ics".into(), "webcal://example.org/h.ics".into()],
                verbose: 2,
            }
        );
        assert_eq!(
            serve(&["serve", "--seed", "demo-imap"]).seed,
            Some(Seed::DemoImap)
        );
        assert_eq!(
            serve(&["serve", "--seed", "demo-multi"]).seed,
            Some(Seed::DemoMulti)
        );
    }

    #[test]
    fn serve_rejects_bad_values() {
        assert!(parse(&["serve", "--port", "70000"]).is_err());
        assert!(parse(&["serve", "--seed", "lots"]).is_err());
        assert!(parse(&["serve", "--bogus"]).is_err());
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn mcp_arguments() {
        match parse(&["mcp", "--client", "claude"]).unwrap().command {
            Command::Mcp(args) => assert_eq!(
                args,
                McpArgs {
                    client: "claude".into(),
                    url: None,
                    data_dir: None,
                    config_dir: None,
                    no_start: false,
                    mcp_port: None,
                }
            ),
            other => panic!("unexpected {other:?}"),
        }
        match parse(&[
            "mcp",
            "--client",
            "c",
            "--url",
            "http://127.0.0.1:1/mcp",
            "--data-dir",
            "d",
            "--config-dir",
            "c",
            "--no-start",
            "--mcp-port",
            "4799",
        ])
        .unwrap()
        .command
        {
            Command::Mcp(args) => {
                assert_eq!(args.url.as_deref(), Some("http://127.0.0.1:1/mcp"));
                assert_eq!(args.data_dir, Some("d".into()));
                assert_eq!(args.config_dir, Some("c".into()));
                assert!(args.no_start);
                assert_eq!(args.mcp_port, Some(4799));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(parse(&["mcp"]).is_err(), "--client is required");
    }

    #[test]
    fn config_schema_kinds() {
        for (args, expected) in [
            (&["config", "schema"][..], SchemaKind::Theme),
            (&["config", "schema", "theme"][..], SchemaKind::Theme),
            (&["config", "schema", "layout"][..], SchemaKind::Layout),
            (&["config", "schema", "accounts"][..], SchemaKind::Accounts),
        ] {
            match parse(args).unwrap().command {
                Command::Config {
                    command: ConfigCommand::Schema { kind },
                } => assert_eq!(kind, expected),
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(parse(&["config", "schema", "rules"]).is_err());
    }
}
