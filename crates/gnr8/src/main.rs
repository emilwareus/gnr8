//! gnr8 binary entry point — the SOLE `anyhow` boundary in the project (D-09).
//!
//! Parses the CLI, dispatches each command to a `gnr8-core` seam, and renders the result. Every
//! command is skeletal in Phase 1: the seam returns `CoreError::NotYetImplemented`, which we surface
//! as a clean, non-panicking message and a deliberate non-zero exit code (2) rather than a panic
//! backtrace (D-12 / RUST-04).

mod cli;
mod render;
mod watch;

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

    // `generate` + `check` (Phase 4) run the real deterministic pipeline through the lifecycle core,
    // rendering counts (human or --json) and, for `check`, exiting non-zero on drift. Both flow real
    // pipeline errors (Go toolchain missing, lowering, sdkgen, I/O) through this anyhow boundary as a
    // clean stderr message + exit 1, never a panic (D-09 / RUST-04).
    //
    // `watch` (Phase 4 / WATCH-02/03) is also dispatched OUTSIDE the `dispatch -> Report` path: it loops
    // and prints latency lines directly rather than returning a single Report. Ctrl-C exits with code 0;
    // a config/regenerate error exits via the anyhow boundary (clean stderr, exit 1, no panic).
    match &cli.command {
        Commands::Generate { force } => return run_generate(*force, cli.json),
        Commands::Check => return run_check(cli.json),
        Commands::Watch { debounce_ms } => return run_watch(*debounce_ms, cli.json),
        _ => {}
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

/// Map each non-inspect command to its `gnr8-core` seam. `Init` runs the real `.gnr8/` scaffold
/// (Phase 4 / WS-01); the remaining lifecycle arms still return a typed `NotYetImplemented` naming
/// the phase that will implement them (04-02/04-03), rendered via the [`Report`] path.
fn dispatch(cli: &Cli) -> Result<Report, gnr8_core::CoreError> {
    match &cli.command {
        Commands::Init => run_init(),
        Commands::Doctor => gnr8_core::not_yet("doctor", 5),
        // Handled in `main` before this dispatch; kept exhaustive for the compiler.
        Commands::Generate { .. } => gnr8_core::not_yet("generate", 4),
        Commands::Check => gnr8_core::not_yet("check", 4),
        Commands::Watch { .. } => gnr8_core::not_yet("watch", 4),
        Commands::Inspect { .. } => gnr8_core::not_yet("inspect", 2),
    }
}

/// Scaffold the `.gnr8/` workspace in the current working directory (idempotent) and summarize the
/// outcome. Re-running over an existing workspace reports "nothing to do" and preserves user edits
/// (D-01). The `current_dir` I/O failure surfaces as `CoreError::Workspace`; any scaffold failure
/// flows through the same typed path — never a panic (RUST-04 / D-09).
fn run_init() -> Result<Report, gnr8_core::CoreError> {
    let cwd = std::env::current_dir().map_err(|e| gnr8_core::CoreError::Workspace {
        message: format!("failed to resolve the current directory: {e}"),
    })?;
    let outcome = gnr8_core::workspace::init(&cwd)?;

    let message = if outcome.created.is_empty() {
        format!(
            "nothing to do — .gnr8/ already present (skipped: {})",
            outcome.skipped.join(", ")
        )
    } else if outcome.skipped.is_empty() {
        format!(
            "initialized .gnr8/ (created: {})",
            outcome.created.join(", ")
        )
    } else {
        format!(
            "initialized .gnr8/ (created: {}; skipped: {})",
            outcome.created.join(", "),
            outcome.skipped.join(", ")
        )
    };
    Ok(Report { message })
}

/// The current project root + its `.gnr8/` dir, resolved against the working directory.
///
/// `regenerate`/`plan_only` take the project root; `config::load` reads `<root>/.gnr8/config.toml`.
/// A `current_dir` failure surfaces as `CoreError::Workspace` (clean message, never a panic).
fn project_paths() -> Result<(std::path::PathBuf, std::path::PathBuf), gnr8_core::CoreError> {
    let root = std::env::current_dir().map_err(|e| gnr8_core::CoreError::Workspace {
        message: format!("failed to resolve the current directory: {e}"),
    })?;
    let gnr8_dir = root.join(".gnr8");
    Ok((root, gnr8_dir))
}

/// A serializable generate/check report: the per-bucket counts + paths (the `--json` shape 04-03
/// extends with latency). The human render summarizes the counts; `--json` serializes this struct.
#[derive(Debug, serde::Serialize)]
struct LifecycleReport {
    /// Paths written (new or changed; under `--force`, overwritten user edits).
    written: Vec<String>,
    /// Paths byte-identical and therefore not rewritten (no-op).
    unchanged: Vec<String>,
    /// Paths protected (user-edited / pre-existing) and skipped — overwrite with `--force`.
    skipped: Vec<String>,
}

/// Run `gnr8 generate` (+ `--force`): load the config, regenerate through the real pipeline, write
/// only changed files, and report counts. Every protected (user-edited) file is named in a stderr
/// warning line so the "no silent clobbering" protection is VISIBLE (T-04-02-04). `--json` serializes
/// the counts. A missing `.gnr8/config.toml` surfaces a clean `CoreError::Config` (run `gnr8 init`);
/// a missing Go toolchain surfaces `GoToolchainMissing` — both via the anyhow boundary, never a panic.
fn run_generate(force: bool, json: bool) -> Result<()> {
    let (root, gnr8_dir) = project_paths()?;
    let config = gnr8_core::config::load(&gnr8_dir)?;
    let outcome = gnr8_core::lifecycle::regenerate(&root, &config, force)?;

    // Warn (stderr) for every protected file so the user SEES which outputs were not clobbered.
    for path in &outcome.skipped {
        eprintln!(
            "warning: {path} was hand-edited since gnr8 last wrote it — skipped (use --force to overwrite)"
        );
    }

    if json {
        let report = LifecycleReport {
            written: outcome.written,
            unchanged: outcome.unchanged,
            skipped: outcome.skipped,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{} written, {} unchanged, {} skipped (user-edited; use --force to overwrite)",
            outcome.written.len(),
            outcome.unchanged.len(),
            outcome.skipped.len()
        );
    }
    Ok(())
}

/// Run `gnr8 check`: a DRY-RUN of the same `plan_writes` decision (no writes, no manifest save). Exits
/// NON-ZERO (code 1) if any output is stale (`Write`) or drifted (`UserEdited`); exits 0 when every
/// output is `Unchanged`. Reuses the exact pure decision function — zero new policy. `--json` emits the
/// stale/drifted path lists. Pipeline errors flow through the anyhow boundary, never a panic.
fn run_check(json: bool) -> Result<()> {
    let (root, gnr8_dir) = project_paths()?;
    let config = gnr8_core::config::load(&gnr8_dir)?;
    let plan = gnr8_core::lifecycle::plan_only(&root, &config)?;

    // Partition the plan into stale (would be written) vs drifted (user-edited) vs clean (unchanged).
    let mut stale: Vec<String> = Vec::new();
    let mut drifted: Vec<String> = Vec::new();
    let mut clean: Vec<String> = Vec::new();
    for file in &plan.files {
        match file.action {
            gnr8_core::lifecycle::WriteAction::Write => stale.push(file.path.clone()),
            gnr8_core::lifecycle::WriteAction::UserEdited => drifted.push(file.path.clone()),
            gnr8_core::lifecycle::WriteAction::Unchanged => clean.push(file.path.clone()),
        }
    }
    let has_drift = plan.has_drift();

    if json {
        #[derive(serde::Serialize)]
        struct CheckReport {
            up_to_date: bool,
            stale: Vec<String>,
            drifted: Vec<String>,
            unchanged: Vec<String>,
        }
        let report = CheckReport {
            up_to_date: !has_drift,
            stale,
            drifted,
            unchanged: clean,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if has_drift {
        for path in &stale {
            eprintln!("stale: {path} is out of date (run `gnr8 generate`)");
        }
        for path in &drifted {
            eprintln!("drifted: {path} was hand-edited and differs from the generated output");
        }
        println!(
            "outputs are NOT up to date: {} stale, {} drifted",
            stale.len(),
            drifted.len()
        );
    } else {
        println!("outputs are up to date ({} unchanged)", clean.len());
    }

    if has_drift {
        // Deliberate non-zero exit so `gnr8 check` is a usable CI gate (RESEARCH Open Q 3).
        std::process::exit(1);
    }
    Ok(())
}

/// Run `gnr8 watch [--debounce-ms N]`: load the config, run an initial COLD regeneration (so the
/// cold-latency scenario is measured and the outputs are current), print a startup line, then enter the
/// debounced, loop-safe watch loop (WATCH-02/03). The loop watches the configured SOURCE dirs only,
/// filters out gnr8's own output writes (no self-loop), and times each regeneration. Ctrl-C exits with
/// code 0; a missing `.gnr8/config.toml` surfaces a clean `CoreError::Config` (run `gnr8 init`) and a
/// pipeline error (Go toolchain missing, lowering, sdkgen) flows through the anyhow boundary — never a
/// panic (D-09 / RUST-04).
fn run_watch(debounce_ms: u64, json: bool) -> Result<()> {
    let (root, gnr8_dir) = project_paths()?;
    let config = gnr8_core::config::load(&gnr8_dir)?;

    // Startup line: name the watched source dir(s) so the user sees the scope; Ctrl-C stops the loop.
    if !json {
        println!(
            "watching {} — press Ctrl-C to stop",
            config.inputs.join(", ")
        );
    }

    // The COLD scenario: an initial regeneration ensures outputs are current and measures cold latency.
    watch::cold_regenerate(&root, &config, json)?;

    // Enter the debounced watch loop (blocks until Ctrl-C / channel disconnect, then returns Ok).
    watch::run(
        &root,
        &config,
        std::time::Duration::from_millis(debounce_ms),
        json,
    )
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
