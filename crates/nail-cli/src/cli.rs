//! Command-line arguments.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Default port of `nail serve`.
pub const DEFAULT_PORT: u16 = 4780;

/// Nail: email and PIM client.
#[derive(Debug, Parser)]
#[command(name = "nail", version, about)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the core headless and serve the web UI on localhost.
    Serve(ServeArgs),
    /// Configuration tools.
    Config {
        /// The config tool.
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

/// `nail config …`
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
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
pub enum SchemaKind {
    /// theme.kdl
    Theme,
    /// layout.kdl
    Layout,
    /// accounts.kdl
    Accounts,
}

/// Demo data for `nail serve --seed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Seed {
    /// Load the demo mail corpus as a local account (no network).
    Demo,
    /// Serve the demo corpus from an in-process mock IMAP server and sync
    /// it like a real account.
    DemoImap,
    /// Two configured accounts (IMAP with IDLE, POP3) and SMTP on
    /// in-process mock servers, plus a discoverable third account; writes
    /// accounts.kdl if missing, keeps secrets in memory, and exposes the
    /// test controls at /api/test/… (for end-to-end tests).
    DemoMulti,
}

/// `nail serve …`
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ServeArgs {
    /// Port on 127.0.0.1 (0 picks a free port; the URL is printed).
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
    /// Data directory (store and message blobs). Default: %LOCALAPPDATA%\Nail.
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
    /// Config directory (theme.kdl, layout.kdl). Default: %APPDATA%\Nail.
    #[arg(long, value_name = "DIR")]
    pub config_dir: Option<PathBuf>,
    /// Directory of the built UI (ui/dist). Default: $NAIL_UI_DIR, `ui`
    /// next to the executable, or the repository's ui/dist.
    #[arg(long, value_name = "DIR")]
    pub ui_dir: Option<PathBuf>,
    /// Load demo data before serving.
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
    /// More log output (-v debug, -vv trace). NAIL_LOG overrides.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
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
                port: DEFAULT_PORT,
                data_dir: None,
                config_dir: None,
                ui_dir: None,
                seed: None,
                locale: None,
                open: false,
                no_watch: false,
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
                "-vv",
            ]),
            ServeArgs {
                port: 0,
                data_dir: Some("d".into()),
                config_dir: Some("c".into()),
                ui_dir: Some("u".into()),
                seed: Some(Seed::Demo),
                locale: Some("de".into()),
                open: true,
                no_watch: true,
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
