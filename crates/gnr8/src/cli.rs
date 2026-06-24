//! The gnr8 command-line surface (RUST-02 / D-10..D-12), defined with the clap derive API.
//!
//! Six top-level commands plus a nested `inspect` subcommand, with a global `--json` flag and a
//! repeatable `-v/--verbose` count. Phase 1 only parses the surface; every command dispatches to a
//! `gnr8-core` seam that returns a typed `NotYetImplemented` error (see `main.rs`).

use clap::{Parser, Subcommand};

// `doc_markdown` flags "OpenAPI" (a proper noun, not a code item); backticks would leak into clap
// help text, so allow it locally on the doc comments that double as user-facing help (skill ch.2.4).
/// Code-first OpenAPI + Go SDK generator.
#[allow(clippy::doc_markdown)]
#[derive(Debug, Parser)]
#[command(
    name = "gnr8",
    version,
    about = "Code-first OpenAPI + Go SDK generator"
)]
pub(crate) struct Cli {
    /// Emit machine-readable JSON instead of human-readable output.
    #[arg(long, global = true)]
    pub(crate) json: bool,

    /// Increase output detail (-v, -vv).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub(crate) verbose: u8,

    /// The command to run.
    #[command(subcommand)]
    pub(crate) command: Commands,
}

/// Top-level gnr8 commands (D-11).
#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    /// Scaffold a project-local .gnr8/ workspace.
    Init,
    /// Generate OpenAPI + Go SDK from Go source.
    #[allow(clippy::doc_markdown)] // "OpenAPI" is a proper noun; keep clap help text clean.
    Generate,
    /// Watch source and regenerate on change.
    Watch,
    /// Verify generated outputs are up to date.
    Check,
    /// Explain inferred API facts and diagnostics.
    Inspect {
        /// What to inspect.
        #[command(subcommand)]
        action: InspectAction,
    },
    /// Summarize unsupported patterns and lifecycle issues.
    Doctor,
}

/// `inspect` subcommands (D-11).
#[derive(Debug, Subcommand)]
pub(crate) enum InspectAction {
    /// Show discovered routes.
    Routes,
    /// Show discovered schemas.
    Schemas,
    /// Show the raw API graph.
    Graph,
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4); scope the allow to
    // the test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
