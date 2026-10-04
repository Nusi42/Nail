//! The `nail` command-line tool.
//!
//! * `nail serve` runs the core headless and serves the web UI and the API
//!   on localhost (see `nail-server`).
//! * `nail mcp --client <name>` bridges a stdio MCP client to Nail's MCP
//!   server (see `nail-mcp`), starting the core headless when Nail is not
//!   running (M2c).
//! * `nail plugin dev <folder>` loads an unpacked plug-in with hot reload
//!   and shows its log (M2d; WP61).
//! * `nail config schema` prints the JSON Schema of `theme.kdl` / `layout.kdl`.
//!
//! `serve` and a headless `mcp` hold the data directory's single-instance
//! lock (`nail_store::DataDirLock`).
//!
//! Command-line help and log output are English developer tooling (see
//! `docs/adr/0009-core-service.md`); errors and notices a user reads are
//! localized (`messages`, M2c ADR 0043).

mod cli;
mod mcp;
mod messages;
mod plugin;
mod serve;

use std::io::Write;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command, ConfigCommand, SchemaKind};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::debug!(?err, "nail failed");
            eprintln!("{}", messages::render_error(&err, &messages::catalog()));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve(args) => {
            init_tracing(args.verbose);
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            runtime.block_on(serve::run(args))
        }
        Command::Mcp(args) => {
            // stdout is the MCP channel: logs only to stderr, and quiet.
            init_tracing_quiet();
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            runtime.block_on(mcp::run(args))
        }
        Command::Plugin { command } => {
            init_tracing(0);
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            runtime.block_on(plugin::run(command))
        }
        Command::Config {
            command: ConfigCommand::Schema { kind },
        } => {
            let json = schema_json(kind)?;
            let mut out = std::io::stdout().lock();
            writeln!(out, "{json}")?;
            Ok(())
        }
    }
}

/// The pretty-printed JSON Schema for `kind`.
fn schema_json(kind: SchemaKind) -> serde_json::Result<String> {
    match kind {
        SchemaKind::Theme => serde_json::to_string_pretty(&nail_config::theme_schema()),
        SchemaKind::Layout => serde_json::to_string_pretty(&nail_config::layout_schema()),
        SchemaKind::Accounts => serde_json::to_string_pretty(&nail_config::accounts_schema()),
    }
}

/// Warnings only (`NAIL_LOG` overrides), on stderr.
fn init_tracing_quiet() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("NAIL_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(|| nail_core::logs::TeeWriter)
        .init();
}

fn init_tracing(verbose: u8) {
    use tracing_subscriber::EnvFilter;
    // Tantivy logs every index commit at info level; keep it quiet unless
    // everything is asked for.
    let default = match verbose {
        0 => "info,tantivy=warn",
        1 => "debug,tantivy=warn",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_env("NAIL_LOG").unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(|| nail_core::logs::TeeWriter)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_are_json_schema_documents() {
        for kind in [SchemaKind::Theme, SchemaKind::Layout, SchemaKind::Accounts] {
            let json: serde_json::Value =
                serde_json::from_str(&schema_json(kind).unwrap()).unwrap();
            assert_eq!(
                json["$schema"],
                "https://json-schema.org/draft/2020-12/schema"
            );
        }
    }

    #[test]
    fn schemas_match_the_checked_in_snapshots() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../nail-config/schema");
        for (kind, file) in [
            (SchemaKind::Theme, "theme.schema.json"),
            (SchemaKind::Layout, "layout.schema.json"),
            (SchemaKind::Accounts, "accounts.schema.json"),
        ] {
            let printed: serde_json::Value =
                serde_json::from_str(&schema_json(kind).unwrap()).unwrap();
            let snapshot: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join(file)).unwrap()).unwrap();
            assert_eq!(printed, snapshot, "{file}");
        }
    }
}
