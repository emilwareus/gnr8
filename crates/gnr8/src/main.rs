//! gnr8 binary entry point — the SOLE `anyhow` boundary in the project (D-09).
//!
//! Parses the CLI, dispatches each command to a `gnr8-core` seam, and renders the result. Every
//! command is skeletal in Phase 1: the seam returns `CoreError::NotYetImplemented`, which we surface
//! as a clean, non-panicking message and a deliberate non-zero exit code (2) rather than a panic
//! backtrace (D-12 / RUST-04).

mod cli;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, InspectAction};

/// Placeholder command result. Real reports (routes, schemas, diagnostics, ...) arrive in later
/// phases; this exists so the render path compiles today.
#[derive(Debug, serde::Serialize)]
struct Report {
    /// Human-readable summary line for the executed command.
    message: String,
}

impl Report {
    /// Render the report to stdout as a plain human-readable line (Phase 1; tables come later).
    fn render_human(&self) {
        println!("{}", self.message);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match dispatch(&cli) {
        Ok(report) => {
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                report.render_human();
            }
            Ok(())
        }
        // NotYetImplemented is an expected, non-panicking exit: clear message, deliberate code 2.
        Err(e @ gnr8_core::CoreError::NotYetImplemented { .. }) => {
            eprintln!("gnr8: {e}");
            std::process::exit(2);
        }
        // Any future real error bubbles through anyhow → stderr + exit 1.
        // (Unreachable today; kept so adding real CoreError variants needs no boundary change.)
        #[allow(unreachable_patterns)]
        Err(e) => Err(e.into()),
    }
}

/// Map each parsed command to its `gnr8-core` seam. In Phase 1 every arm returns a typed
/// `NotYetImplemented` naming the phase that will implement it.
fn dispatch(cli: &Cli) -> Result<Report, gnr8_core::CoreError> {
    match &cli.command {
        Commands::Init => gnr8_core::not_yet("init", 4),
        Commands::Generate => gnr8_core::not_yet("generate", 3),
        Commands::Watch => gnr8_core::not_yet("watch", 4),
        Commands::Check => gnr8_core::not_yet("check", 4),
        Commands::Doctor => gnr8_core::not_yet("doctor", 5),
        Commands::Inspect { action } => match action {
            InspectAction::Routes => gnr8_core::not_yet("inspect routes", 2),
            InspectAction::Schemas => gnr8_core::not_yet("inspect schemas", 2),
            InspectAction::Graph => gnr8_core::not_yet("inspect graph", 2),
        },
    }
}
