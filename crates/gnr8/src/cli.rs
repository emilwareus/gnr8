//! The gnr8 command-line surface (RUST-02 / D-10..D-12), defined with the clap derive API.
//!
//! Six top-level commands plus a nested `inspect` subcommand, with a global `--json` flag and a
//! repeatable `-v/--verbose` count. Execution lives in the binary modules; core library failures stay
//! typed as `gnr8::CoreError` until the final CLI reporting boundary.

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
    Generate {
        /// Overwrite generated files a user has hand-edited (D-04 / A4 override verb).
        #[arg(long)]
        force: bool,
    },
    /// Watch source and regenerate on change.
    Watch {
        /// Debounce window in milliseconds — coalesce a burst of rapid file events into one
        /// regeneration (RESEARCH Open Q 1: ship a 200ms default, expose a knob without overconfiguring).
        #[arg(long, default_value_t = 200)]
        debounce_ms: u64,
    },
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

/// The default analysis target when `inspect` is run without an explicit path: the goalservice Gin
/// fixture, resolved relative to this crate's manifest dir (mirrors how the contract tests resolve
/// `FIXTURE_DIR`). Keeps `gnr8 inspect routes` working out of the box this phase (D-09).
pub(crate) const DEFAULT_INSPECT_TARGET: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

/// `inspect` subcommands (D-11). Each takes an optional target directory (the Go module to analyze),
/// defaulting to the goalservice fixture so the command works with no argument and with an explicit
/// path (`gnr8 inspect routes <dir>`).
#[derive(Debug, Subcommand)]
pub(crate) enum InspectAction {
    /// Show discovered routes.
    Routes {
        /// The Go module directory to analyze (defaults to the goalservice fixture).
        #[arg(default_value = DEFAULT_INSPECT_TARGET)]
        path: String,
    },
    /// Show discovered schemas.
    Schemas {
        /// The Go module directory to analyze (defaults to the goalservice fixture).
        #[arg(default_value = DEFAULT_INSPECT_TARGET)]
        path: String,
    },
    /// Show the raw API graph.
    Graph {
        /// The Go module directory to analyze (defaults to the goalservice fixture).
        #[arg(default_value = DEFAULT_INSPECT_TARGET)]
        path: String,
    },
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
        // `generate` defaults `--force` to false.
        assert!(matches!(
            Cli::try_parse_from(["gnr8", "generate"]).unwrap().command,
            Commands::Generate { force: false }
        ));
        // `generate --force` sets the flag.
        assert!(matches!(
            Cli::try_parse_from(["gnr8", "generate", "--force"])
                .unwrap()
                .command,
            Commands::Generate { force: true }
        ));
        // `watch` defaults `--debounce-ms` to 200.
        assert!(matches!(
            Cli::try_parse_from(["gnr8", "watch"]).unwrap().command,
            Commands::Watch { debounce_ms: 200 }
        ));
        // `watch --debounce-ms N` overrides the default window.
        assert!(matches!(
            Cli::try_parse_from(["gnr8", "watch", "--debounce-ms", "100"])
                .unwrap()
                .command,
            Commands::Watch { debounce_ms: 100 }
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
        // Each variant now carries a `path` (defaulted to the fixture); discriminant compares the
        // variant only, so the `want` path is irrelevant.
        for (arg, want) in [
            (
                "routes",
                InspectAction::Routes {
                    path: String::new(),
                },
            ),
            (
                "schemas",
                InspectAction::Schemas {
                    path: String::new(),
                },
            ),
            (
                "graph",
                InspectAction::Graph {
                    path: String::new(),
                },
            ),
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
    fn cli_inspect_defaults_target_and_accepts_explicit_path() {
        // No path → the fixture default.
        let cli = Cli::try_parse_from(["gnr8", "inspect", "routes"]).unwrap();
        let Commands::Inspect {
            action: InspectAction::Routes { path },
        } = cli.command
        else {
            panic!("expected inspect routes");
        };
        assert!(
            path.ends_with("fixtures/goalservice"),
            "default target: {path}"
        );

        // Explicit path wins.
        let cli = Cli::try_parse_from(["gnr8", "inspect", "schemas", "/some/dir"]).unwrap();
        let Commands::Inspect {
            action: InspectAction::Schemas { path },
        } = cli.command
        else {
            panic!("expected inspect schemas");
        };
        assert_eq!(path, "/some/dir");
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
