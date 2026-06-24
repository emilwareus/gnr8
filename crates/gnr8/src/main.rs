//! gnr8 binary entry point — the SOLE `anyhow` boundary in the project (D-09).
//!
//! Parses the CLI, dispatches each command to a `gnr8-core` seam, and renders the result. Every
//! command is skeletal in Phase 1: the seam returns `CoreError::NotYetImplemented`, which we surface
//! as a clean, non-panicking message and a deliberate non-zero exit code (2) rather than a panic
//! backtrace (D-12 / RUST-04).

mod cli;
mod render;

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

    // `inspect` is implemented (Phase 2): it builds the graph and renders directly to stdout, with a
    // real analysis error (e.g. the Go toolchain missing) flowing through this anyhow boundary as a
    // clean stderr message + exit 1, never a panic (GO-06 / D-09).
    if let Commands::Inspect { action } = &cli.command {
        return run_inspect(action, cli.json);
    }

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
        #[allow(unreachable_patterns)]
        Err(e) => Err(e.into()),
    }
}

/// Map each non-inspect command to its `gnr8-core` seam. These still return a typed
/// `NotYetImplemented` naming the phase that will implement them (rendered via the [`Report`] path).
fn dispatch(cli: &Cli) -> Result<Report, gnr8_core::CoreError> {
    match &cli.command {
        Commands::Init => gnr8_core::not_yet("init", 4),
        Commands::Generate => gnr8_core::not_yet("generate", 3),
        Commands::Watch => gnr8_core::not_yet("watch", 4),
        Commands::Check => gnr8_core::not_yet("check", 4),
        Commands::Doctor => gnr8_core::not_yet("doctor", 5),
        // Handled in `main` before this dispatch; kept exhaustive for the compiler.
        Commands::Inspect { .. } => gnr8_core::not_yet("inspect", 2),
    }
}

/// Build the API graph for an `inspect` subcommand's target dir, render it (table or `--json`), and
/// print it. The `build_graph` `CoreError` and any `serde_json` render error both convert into the
/// anyhow boundary via `?` (clean message + exit 1, never a panic).
fn run_inspect(action: &InspectAction, json: bool) -> Result<()> {
    let output = match action {
        InspectAction::Routes { path } => {
            let graph = gnr8_core::analyze::build_graph(path)?;
            render::render_routes(&graph, json)?
        }
        InspectAction::Schemas { path } => {
            let graph = gnr8_core::analyze::build_graph(path)?;
            render::render_schemas(&graph, json)?
        }
        InspectAction::Graph { path } => {
            let graph = gnr8_core::analyze::build_graph(path)?;
            render::render_graph(&graph, json)?
        }
    };
    print!("{output}");
    Ok(())
}
