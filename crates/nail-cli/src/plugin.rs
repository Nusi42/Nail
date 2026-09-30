//! `nail plugin …` (M2d, ADR 0050): `nail plugin dev <folder>` loads an
//! unpacked plug-in into Nail with hot reload and prints its log (errors,
//! API calls) as a console.
//!
//! **Slot (WP60).** WP61 owns this file (and the arguments below): how the
//! command reaches the core (e.g. a running Nail, or a headless one like
//! `nail mcp`), `Request::LoadDevPlugin` with the manifest's permissions
//! granted, and tailing `Event::PluginLogLine`. Until then it reports that
//! plug-in development is not available yet.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use nail_api::LocalizedMessage;

use crate::messages;

/// `nail plugin …`
#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Load an unpacked plug-in folder (with manifest.json) into Nail,
    /// reload it when its files change and show its log.
    Dev(PluginDevArgs),
}

/// `nail plugin dev …`
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct PluginDevArgs {
    /// The plug-in folder (holding manifest.json).
    #[arg(value_name = "FOLDER")]
    pub folder: PathBuf,
}

/// Runs a `nail plugin` command.
pub async fn run(command: PluginCommand) -> anyhow::Result<()> {
    match command {
        PluginCommand::Dev(args) => Err(messages::error(
            LocalizedMessage::new("cli-plugin-dev-unavailable")
                .arg("folder", args.folder.display().to_string()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn dev_takes_a_folder() {
        let cli =
            crate::cli::Cli::try_parse_from(["nail", "plugin", "dev", "C:/dev/follow-up"]).unwrap();
        match cli.command {
            crate::cli::Command::Plugin {
                command: PluginCommand::Dev(args),
            } => assert_eq!(args.folder, PathBuf::from("C:/dev/follow-up")),
            other => panic!("{other:?}"),
        }
        assert!(crate::cli::Cli::try_parse_from(["nail", "plugin", "dev"]).is_err());
    }

    #[tokio::test]
    async fn dev_is_not_available_yet() {
        let err = run(PluginCommand::Dev(PluginDevArgs {
            folder: "C:/x".into(),
        }))
        .await
        .unwrap_err();
        let text = messages::render_error(&err, &nail_config::l10n::Catalog::english());
        assert!(text.contains("C:/x"), "{text}");
    }
}
