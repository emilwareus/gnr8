//! RED placeholder — the full clap derive CLI surface lands in the GREEN step.
//! Intentionally incomplete so the parse tests below fail to compile (missing variants).

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "gnr8", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Generate OpenAPI + Go SDK from Go source.
    Generate,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{Cli, Commands, InspectAction};
    use clap::Parser;

    #[test]
    fn cli_parses_all_top_level_commands() {
        assert!(matches!(
            Cli::try_parse_from(["gnr8", "init"]).unwrap().command,
            Commands::Init
        ));
        assert!(matches!(
            Cli::try_parse_from(["gnr8", "generate"]).unwrap().command,
            Commands::Generate
        ));
        assert!(matches!(
            Cli::try_parse_from(["gnr8", "watch"]).unwrap().command,
            Commands::Watch
        ));
        assert!(matches!(
            Cli::try_parse_from(["gnr8", "check"]).unwrap().command,
            Commands::Check
        ));
        assert!(matches!(
            Cli::try_parse_from(["gnr8", "doctor"]).unwrap().command,
            Commands::Doctor
        ));
    }

    #[test]
    fn cli_parses_inspect_subcommands() {
        for (arg, want) in [
            ("routes", InspectAction::Routes),
            ("schemas", InspectAction::Schemas),
            ("graph", InspectAction::Graph),
        ] {
            let cli = Cli::try_parse_from(["gnr8", "inspect", arg]).unwrap();
            match cli.command {
                Commands::Inspect { action } => assert_eq!(
                    std::mem::discriminant(&action),
                    std::mem::discriminant(&want)
                ),
                other => panic!("expected Inspect, got {other:?}"),
            }
        }
    }

    #[test]
    fn cli_global_json_flag() {
        let cli = Cli::try_parse_from(["gnr8", "--json", "doctor"]).unwrap();
        assert!(cli.json);
        let cli = Cli::try_parse_from(["gnr8", "-v", "doctor"]).unwrap();
        assert!(cli.verbose >= 1);
    }

    #[test]
    fn cli_rejects_unknown_command() {
        assert!(Cli::try_parse_from(["gnr8", "bogus"]).is_err());
    }
}
