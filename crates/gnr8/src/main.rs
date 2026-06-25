//! gnr8 binary entry point — the orchestrator + trusted writer (D-09).
//!
//! gnr8 is configured ONLY by code: `gnr8 init` scaffolds a `.gnr8/` Rust crate (the pipeline), and
//! every generating command runs that crate as a child process (`cargo run --manifest-path`), receives
//! its [`gnr8_core::runner::ArtifactBundle`], and owns writing the files (the ownership manifest, no-op
//! skip, edit protection). There is no TOML config anywhere. Each command surfaces real errors (a
//! missing `.gnr8/`, a compile error in the user's pipeline, a missing Go toolchain) through this
//! `anyhow` boundary as a clean stderr message + a deliberate non-zero exit, never a panic (RUST-04).

mod child;
mod cli;
mod doctor;
mod render;
mod watch;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, InspectAction};

fn main() -> Result<()> {
    let cli = Cli::parse();

    // `inspect` analyzes a target dir directly (a dev/debug tool over `analyze::build_graph`); it is
    // dispatched first and renders straight to stdout. The remaining commands either scaffold (`init`)
    // or delegate to the user's `.gnr8/` child crate and own writing/policy.
    match &cli.command {
        Commands::Inspect { action } => run_inspect(action, cli.json),
        Commands::Init => run_init(cli.json),
        Commands::Generate { force } => run_generate(*force, cli.json),
        Commands::Check => run_check(cli.json),
        Commands::Watch { debounce_ms } => run_watch(*debounce_ms, cli.json),
        Commands::Doctor => run_doctor(cli.json),
    }
}

/// The current project root, resolved against the working directory. The child runs with this as its
/// `current_dir`, and `regenerate`/`plan_only` resolve output paths against it. A `current_dir` failure
/// surfaces as `CoreError::Workspace` (clean message, never a panic).
fn project_root() -> Result<std::path::PathBuf, gnr8_core::CoreError> {
    std::env::current_dir().map_err(|e| gnr8_core::CoreError::Workspace {
        message: format!("failed to resolve the current directory: {e}"),
    })
}

/// Scaffold the mandatory `.gnr8/` generation crate in the working directory (idempotent) and summarize
/// the outcome. Re-running over an existing crate preserves the user's `src/main.rs` and reports
/// "nothing to do" (D-01). `--json` emits the created/skipped lists.
fn run_init(json: bool) -> Result<()> {
    let root = project_root()?;
    let outcome = gnr8_core::workspace::init(&root)?;

    if json {
        #[derive(serde::Serialize)]
        struct InitReport {
            created: Vec<String>,
            skipped: Vec<String>,
        }
        let report = InitReport {
            created: outcome.created.clone(),
            skipped: outcome.skipped.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    if outcome.created.is_empty() {
        println!(
            "nothing to do — .gnr8/ already present (skipped: {})",
            outcome.skipped.join(", ")
        );
    } else {
        if outcome.skipped.is_empty() {
            println!(
                "initialized .gnr8/ (created: {})",
                outcome.created.join(", ")
            );
        } else {
            println!(
                "initialized .gnr8/ (created: {}; skipped: {})",
                outcome.created.join(", "),
                outcome.skipped.join(", ")
            );
        }
        println!("edit .gnr8/src/main.rs to adapt parsing + generation, then run `gnr8 generate`.");
    }
    Ok(())
}

/// A serializable generate/check report: the per-bucket counts + paths. The human render summarizes the
/// counts; `--json` serializes this struct.
#[derive(Debug, serde::Serialize)]
struct LifecycleReport {
    /// Paths written (new or changed; under `--force`, overwritten user edits).
    written: Vec<String>,
    /// Paths byte-identical and therefore not rewritten (no-op).
    unchanged: Vec<String>,
    /// Paths protected (user-edited / pre-existing) and skipped — overwrite with `--force`.
    skipped: Vec<String>,
}

/// Run `gnr8 generate` (+ `--force`): run the user's `.gnr8/` pipeline (child process), then write only
/// changed files and report counts. Every protected (user-edited) file is named in a stderr warning so
/// the "no silent clobbering" protection is VISIBLE (T-04-02-04). Pipeline diagnostics the child carried
/// are surfaced too. `--json` serializes the counts. A missing `.gnr8/` (run `gnr8 init`), a compile
/// error in the user's pipeline, or a missing Go toolchain surface via the anyhow boundary, never a panic.
fn run_generate(force: bool, json: bool) -> Result<()> {
    let root = project_root()?;
    let bundle = child::run_child(&root, "__emit")?;
    let outcome = gnr8_core::lifecycle::regenerate(&root, &bundle.artifacts, force)?;

    // Surface the pipeline's diagnostics (lossy/unsupported source patterns the child reported).
    for diag in &bundle.diagnostics {
        eprintln!(
            "{}: {} ({}:{})",
            diag.severity, diag.message, diag.file, diag.line
        );
    }
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

/// Run `gnr8 check`: run the user's `.gnr8/` pipeline, then DRY-RUN the same `plan_writes` decision (no
/// writes, no manifest save). Exits NON-ZERO (code 1) if any output is stale (`Write`) or drifted
/// (`UserEdited`); exits 0 when every output is `Unchanged`. Reuses the exact pure decision function —
/// zero new policy. `--json` emits the stale/drifted path lists. Pipeline errors flow through the anyhow
/// boundary, never a panic.
fn run_check(json: bool) -> Result<()> {
    let root = project_root()?;
    let bundle = child::run_child(&root, "__emit")?;
    let plan = gnr8_core::lifecycle::plan_only(&root, &bundle.artifacts)?;

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

/// Probe whether the Go toolchain is present by attempting to spawn `go version`.
///
/// `.is_ok()` means the `go` binary SPAWNED (a non-zero exit still means `go` exists); a spawn
/// `io::Error` (binary not found) means it is ABSENT. `go`/`version` are passed as DISCRETE `Command`
/// args (never `sh -c`), so no shell metacharacter can be injected (T-05-01-01).
fn probe_go() -> bool {
    std::process::Command::new("go")
        .arg("version")
        .output()
        .is_ok()
}

/// Run `gnr8 doctor`: a health aggregator that runs the user's `.gnr8/` pipeline once and reports its
/// diagnostics + drift (HARD-01 / D-01, D-02). Mirrors `run_check`'s shell-vs-decision split (this is
/// the impure shell; the pure grouping + exit policy lives in [`doctor::DoctorReport`]).
///
/// Collects three lifecycle facts (`.gnr8/` present, Go toolchain present, the pipeline runs), and —
/// when the pipeline runs cleanly — its diagnostics and the dry-run drift plan. A pipeline failure (a
/// compile error, a missing toolchain) is REPORTED as a finding, never `?`/unwrap'd into a crash
/// (Pitfall 4 / D-02). Prints the human report or `--json`, then exits non-zero ONLY on an actionable
/// problem (mirrors `run_check`).
fn run_doctor(json: bool) -> Result<()> {
    let root = project_root()?;
    let initialized = gnr8_core::workspace::manifest_path(&root).is_file();
    let go_present = probe_go();

    // Run the pipeline once. Its `Err` IS the "pipeline broken" finding (do NOT `?`); on success we get
    // the child's diagnostics and can compute drift from its artifacts. Both degrade gracefully.
    let bundle = if initialized {
        child::run_child(&root, "__emit").ok()
    } else {
        None
    };
    let pipeline_ran = bundle.is_some();
    let diagnostics = bundle.as_ref().map(|b| b.diagnostics.clone());
    let drift = bundle
        .as_ref()
        .and_then(|b| gnr8_core::lifecycle::plan_only(&root, &b.artifacts).ok());

    let report = doctor::DoctorReport::assemble(
        initialized,
        go_present,
        pipeline_ran,
        diagnostics,
        drift.as_ref(),
    );

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", report.render_human());
    }

    if report.has_actionable_problem() {
        // Deliberate non-zero exit so `gnr8 doctor` is a usable CI gate (mirrors run_check). The
        // informational analysis WARNs do NOT contribute to this (Pitfall 1).
        std::process::exit(1);
    }
    Ok(())
}

/// Run `gnr8 watch [--debounce-ms N]`: run an initial COLD regeneration (so the cold-latency scenario is
/// measured and the outputs are current), print a startup line, then enter the debounced watch loop
/// (WATCH-02/03). The loop watches the project's Go sources AND `.gnr8/src/` (so editing the pipeline
/// re-runs it), filters out gnr8's own output writes (no self-loop), and times each regeneration. Ctrl-C
/// exits with code 0; a missing `.gnr8/` or a pipeline error flows through the anyhow boundary — never a
/// panic (D-09 / RUST-04).
fn run_watch(debounce_ms: u64, json: bool) -> Result<()> {
    // Floor the debounce window at a small minimum (IN-04): `--debounce-ms 0` would create a
    // zero-window debouncer that defeats burst-coalescing and amplifies the delete/rename edge case.
    const MIN_DEBOUNCE_MS: u64 = 10;

    let root = project_root()?;

    if !json {
        println!(
            "watching {} (sources + .gnr8/src) — press Ctrl-C to stop",
            root.display()
        );
    }

    // The COLD scenario: an initial regeneration ensures outputs are current and measures cold latency.
    watch::cold_regenerate(&root, json)?;

    let debounce = std::time::Duration::from_millis(debounce_ms.max(MIN_DEBOUNCE_MS));
    watch::run(&root, debounce, json)
}

/// Build the API graph for an `inspect` subcommand's target dir, render it (table or `--json`), and
/// print it. A dev/debug tool over `analyze::build_graph` (it analyzes a directory directly, NOT through
/// the child pipeline, since the renderers take an `ApiGraph` and the IR carries no transforms yet). The
/// `build_graph` `CoreError` and any render error both flow through the anyhow boundary (clean message +
/// exit 1, never a panic).
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
