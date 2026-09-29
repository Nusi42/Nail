//! The `nail` command-line tool.
//!
//! Stub created by M0 WP0. Subcommands (`serve`, later `mcp` and `plugin`)
//! land in WP5.

use clap::Parser;

/// Command-line arguments of the `nail` binary.
#[derive(Debug, Parser)]
#[command(name = "nail", version, about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
