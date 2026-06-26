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
    /// Stale generated-output files deleted during this generation.
    deleted: Vec<String>,
}

/// Run `gnr8 generate` (+ `--force`): run the user's `.gnr8/` pipeline (child process), then write only
/// changed files and report counts. Every protected (user-edited) file is named in a stderr warning so
/// the "no silent clobbering" protection is VISIBLE (T-04-02-04). Pipeline diagnostics the child carried
/// are surfaced too. `--json` serializes the counts. A missing `.gnr8/` (run `gnr8 init`), a compile
/// error in the user's pipeline, or a missing Go toolchain surface via the anyhow boundary, never a panic.
fn run_generate(force: bool, json: bool) -> Result<()> {
    let root = project_root()?;
    let (outcome, diagnostics) = if let Some(noop) = pre_child_verified_noop(&root) {
        (noop.outcome, noop.diagnostics)
    } else {
        let mut bundle = child::run_child(&root, "__emit")?;
        let outcome = regenerate_bundle(&root, &mut bundle, force)?;
        (outcome, bundle.diagnostics.clone())
    };

    // Surface the pipeline's diagnostics (lossy/unsupported source patterns the child reported).
    for diag in &diagnostics {
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
            deleted: outcome.deleted,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{} written, {} unchanged, {} deleted, {} skipped (user-edited; use --force to overwrite)",
            outcome.written.len(),
            outcome.unchanged.len(),
            outcome.deleted.len(),
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
    let plan = if let Some(noop) = pre_child_verified_noop(&root) {
        clean_plan_from_paths(noop.outcome.unchanged)
    } else {
        let mut bundle = child::run_child(&root, "__emit")?;
        plan_bundle(&root, &mut bundle)?
    };

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

fn clean_plan_from_paths(paths: Vec<String>) -> gnr8_core::lifecycle::WritePlan {
    gnr8_core::lifecycle::WritePlan {
        files: paths
            .into_iter()
            .map(|path| gnr8_core::lifecycle::PlannedFile {
                path,
                action: gnr8_core::lifecycle::WriteAction::Unchanged,
                new_bytes: Vec::new(),
                new_hash: String::new(),
                source: "generated".to_string(),
            })
            .collect(),
    }
}

fn regenerate_bundle(
    root: &std::path::Path,
    bundle: &mut gnr8_core::runner::ArtifactBundle,
    force: bool,
) -> Result<gnr8_core::lifecycle::GenerateOutcome, gnr8_core::CoreError> {
    if let Some(metadata) = cached_artifact_metadata(root, bundle) {
        if let Some(outcome) = verified_noop_outcome(root, bundle, &metadata) {
            save_verified_noop_stamp(root, bundle, &metadata, &outcome);
            return Ok(outcome);
        }
        if let Some(outcome) = gnr8_core::lifecycle::regenerate_cached_with_anchors(
            root,
            &metadata,
            &bundle.output_anchors,
            force,
        )? {
            save_verified_noop_stamp(root, bundle, &metadata, &outcome);
            return Ok(outcome);
        }
    }
    ensure_bundle_artifacts(root, bundle)?;
    let outcome = gnr8_core::lifecycle::regenerate_with_anchors(
        root,
        &bundle.artifacts,
        &bundle.output_anchors,
        force,
    )?;
    Ok(outcome)
}

fn plan_bundle(
    root: &std::path::Path,
    bundle: &mut gnr8_core::runner::ArtifactBundle,
) -> Result<gnr8_core::lifecycle::WritePlan, gnr8_core::CoreError> {
    if let Some(metadata) = cached_artifact_metadata(root, bundle) {
        return gnr8_core::lifecycle::plan_only_cached(root, &metadata);
    }
    ensure_bundle_artifacts(root, bundle)?;
    gnr8_core::lifecycle::plan_only(root, &bundle.artifacts)
}

fn cached_artifact_metadata(
    root: &std::path::Path,
    bundle: &gnr8_core::runner::ArtifactBundle,
) -> Option<Vec<gnr8_core::sdk::ArtifactMetadata>> {
    if !bundle.artifacts.is_empty() {
        return None;
    }
    let key = bundle.artifact_cache_key.as_deref()?;
    gnr8_core::sdk::load_artifact_cache_metadata(root, key)
}

fn ensure_bundle_artifacts(
    root: &std::path::Path,
    bundle: &mut gnr8_core::runner::ArtifactBundle,
) -> Result<(), gnr8_core::CoreError> {
    if !bundle.artifacts.is_empty() {
        return Ok(());
    }
    let Some(key) = bundle.artifact_cache_key.as_deref() else {
        return Ok(());
    };
    bundle.artifacts =
        gnr8_core::sdk::load_artifact_cache_files(root, key).ok_or_else(|| {
            gnr8_core::CoreError::ChildRun {
                message: format!(
                    "the .gnr8 generation crate emitted artifact cache key {key}, but the host \
                     could not read the corresponding cache file. Re-run generation to rebuild the cache."
                ),
            }
        })?;
    Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct VerifiedNoopStamp {
    artifact_cache_key: String,
    output_anchors: Vec<String>,
    artifact_paths: Vec<String>,
    input_roots: Vec<String>,
    input_files: Vec<gnr8_core::sdk::FileStamp>,
    output_files: Vec<gnr8_core::sdk::FileStamp>,
    diagnostics: Vec<gnr8_core::graph::Diagnostic>,
}

struct CachedNoop {
    outcome: gnr8_core::lifecycle::GenerateOutcome,
    diagnostics: Vec<gnr8_core::graph::Diagnostic>,
}

fn pre_child_verified_noop(root: &std::path::Path) -> Option<CachedNoop> {
    let stamp = load_verified_noop_stamp(root)?;
    let current_inputs = collect_hot_input_stamps(root, &stamp.input_roots)?;
    if current_inputs != stamp.input_files {
        return None;
    }
    let current_outputs =
        collect_verified_file_stamps(root, &stamp.output_anchors, &stamp.artifact_paths)?;
    if current_outputs != stamp.output_files {
        return None;
    }
    Some(CachedNoop {
        outcome: gnr8_core::lifecycle::GenerateOutcome {
            written: Vec::new(),
            unchanged: stamp.artifact_paths,
            skipped: Vec::new(),
            deleted: Vec::new(),
        },
        diagnostics: stamp.diagnostics,
    })
}

fn verified_noop_outcome(
    root: &std::path::Path,
    bundle: &gnr8_core::runner::ArtifactBundle,
    metadata: &[gnr8_core::sdk::ArtifactMetadata],
) -> Option<gnr8_core::lifecycle::GenerateOutcome> {
    let key = bundle.artifact_cache_key.as_deref()?;
    let stamp = load_verified_noop_stamp(root)?;
    if stamp.artifact_cache_key != key || stamp.output_anchors != bundle.output_anchors {
        return None;
    }
    let artifact_paths = artifact_paths(metadata);
    let current = collect_verified_file_stamps(root, &bundle.output_anchors, &artifact_paths)?;
    if current != stamp.output_files {
        return None;
    }
    Some(gnr8_core::lifecycle::GenerateOutcome {
        written: Vec::new(),
        unchanged: metadata
            .iter()
            .map(|artifact| artifact.path.clone())
            .collect(),
        skipped: Vec::new(),
        deleted: Vec::new(),
    })
}

fn save_verified_noop_stamp(
    root: &std::path::Path,
    bundle: &gnr8_core::runner::ArtifactBundle,
    metadata: &[gnr8_core::sdk::ArtifactMetadata],
    outcome: &gnr8_core::lifecycle::GenerateOutcome,
) {
    if !outcome.written.is_empty() || !outcome.skipped.is_empty() || !outcome.deleted.is_empty() {
        return;
    }
    let Some(key) = bundle.artifact_cache_key.as_deref() else {
        return;
    };
    if bundle.cache_input_roots.is_empty() || bundle.cache_input_stamps.is_empty() {
        return;
    }
    let artifact_paths = artifact_paths(metadata);
    let Some(output_files) =
        collect_verified_file_stamps(root, &bundle.output_anchors, &artifact_paths)
    else {
        return;
    };
    let Some(input_files) = combine_input_stamps(root, &bundle.cache_input_stamps) else {
        return;
    };
    let stamp = VerifiedNoopStamp {
        artifact_cache_key: key.to_string(),
        output_anchors: bundle.output_anchors.clone(),
        artifact_paths,
        input_roots: bundle.cache_input_roots.clone(),
        input_files,
        output_files,
        diagnostics: bundle.diagnostics.clone(),
    };
    let path = verified_noop_stamp_path(root);
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(&stamp) else {
        return;
    };
    let _ = std::fs::write(path, bytes);
}

fn collect_verified_file_stamps(
    root: &std::path::Path,
    output_anchors: &[String],
    artifact_paths: &[String],
) -> Option<Vec<gnr8_core::sdk::FileStamp>> {
    let mut paths = std::collections::BTreeSet::new();
    for path in artifact_paths {
        paths.insert(path.clone());
    }
    for anchor in output_anchors {
        collect_anchor_stamp_paths(root, anchor, &mut paths)?;
    }
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(|path| root.join(path)).collect();
    gnr8_core::sdk::stamp_project_paths(root, &paths)
}

fn artifact_paths(metadata: &[gnr8_core::sdk::ArtifactMetadata]) -> Vec<String> {
    metadata
        .iter()
        .map(|artifact| artifact.path.clone())
        .collect()
}

fn load_verified_noop_stamp(root: &std::path::Path) -> Option<VerifiedNoopStamp> {
    std::fs::read(verified_noop_stamp_path(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn combine_input_stamps(
    root: &std::path::Path,
    source_stamps: &[gnr8_core::sdk::FileStamp],
) -> Option<Vec<gnr8_core::sdk::FileStamp>> {
    let mut stamps = source_stamps.to_vec();
    stamps.extend(host_config_stamps(root)?);
    stamps.sort();
    Some(stamps)
}

fn collect_hot_input_stamps(
    root: &std::path::Path,
    input_roots: &[String],
) -> Option<Vec<gnr8_core::sdk::FileStamp>> {
    if input_roots.is_empty() {
        return None;
    }
    let mut paths = Vec::new();
    for input_root in input_roots {
        collect_hot_input_files(&root.join(input_root), &mut paths)?;
    }
    paths.extend(host_config_paths(root));
    gnr8_core::sdk::stamp_project_paths(root, &paths)
}

fn host_config_stamps(root: &std::path::Path) -> Option<Vec<gnr8_core::sdk::FileStamp>> {
    gnr8_core::sdk::stamp_project_paths(root, &host_config_paths(root))
}

fn host_config_paths(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    let gnr8_dir = root.join(".gnr8");
    collect_hot_input_files(&gnr8_dir.join("src"), &mut paths);
    for name in ["Cargo.toml", "Cargo.lock"] {
        let path = gnr8_dir.join(name);
        if path.is_file() {
            paths.push(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        paths.push(exe);
    }
    paths
}

fn collect_hot_input_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> Option<()> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        let name = path.file_name().and_then(|name| name.to_str())?;
        if path.is_dir() {
            if matches!(
                name,
                ".context"
                    | ".git"
                    | ".gnr8"
                    | "node_modules"
                    | "target"
                    | "vendor"
                    | "__pycache__"
            ) {
                continue;
            }
            collect_hot_input_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    out.sort();
    Some(())
}

fn collect_anchor_stamp_paths(
    root: &std::path::Path,
    anchor: &str,
    paths: &mut std::collections::BTreeSet<String>,
) -> Option<()> {
    if anchor.is_empty()
        || std::path::Path::new(anchor).components().any(|component| {
            !matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return None;
    }
    let anchor_path = root.join(anchor);
    if anchor_path.is_file() {
        paths.insert(anchor.to_string());
        return Some(());
    }
    if !anchor_path.is_dir() {
        return Some(());
    }
    let mut stack = vec![anchor_path];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries {
            let entry = entry.ok()?;
            let path = entry.path();
            let kind = entry.file_type().ok()?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .ok()?
                    .to_string_lossy()
                    .replace('\\', "/");
                paths.insert(rel);
            }
        }
    }
    Some(())
}

fn verified_noop_stamp_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".gnr8").join("cache").join("verified-noop.json")
}

/// Probe whether the DETECTED source language's toolchain is ACTUALLY ready, returning `(language,
/// present)`.
///
/// One `gnr8_core::analyze::source_toolchain` decision over the project root picks the language (the
/// `.gnr8/` crate is excluded from that scan in core, so it does not spoof detection — Open Q2). That
/// SINGLE decision then routes to exactly one readiness check (no try-go-then-python fallback — CLAUDE.md
/// rule 3):
/// - Go/Python: spawn the discrete probe binary (`go version` / `python3 --version`) and require it to
///   EXIT SUCCESSFULLY (WR-05). `.output().map(|o| o.status.success())` — a spawn `io::Error` (binary not
///   found) OR a non-zero exit (a broken/stub binary that cannot even run `--version`) both mean NOT
///   ready, so doctor no longer reports a non-functional shim as healthy.
/// - TypeScript: the real toolchain is `node` AND the user's `typescript`. Delegate to the core probe
///   (`tsextract/probe.js`, which runs the SAME `ts.resolveTypescript` `generate` uses) so a project
///   with `node` but no `typescript` reports UNHEALTHY up front instead of passing doctor then failing
///   at generate (WR-02). Still one source of truth — the probe reuses the extractor's resolution.
///
/// On `Err` (empty/ambiguous source) the language is `"unknown"` and the toolchain is reported absent —
/// surfaced as a doctor finding, never a panic. The binary name is one of three compile-time
/// `&'static str` arms and the args are literals, never `sh -c` (T-06-01).
fn probe_source_lang_toolchain(root: &std::path::Path) -> (String, bool) {
    let Ok(toolchain) = gnr8_core::analyze::source_toolchain(&root.to_string_lossy()) else {
        return ("unknown".to_string(), false);
    };
    let present = if toolchain == gnr8_core::analyze::SourceToolchain::TypeScript {
        // TypeScript's real toolchain is `node` + a resolvable `typescript`; the core probe verifies
        // BOTH via the same resolution `generate` uses (WR-02 — one source of truth, no fallback).
        gnr8_core::analyze::typescript_toolchain_present(&root.to_string_lossy())
    } else {
        // Go/Python are wholly `go`/`python3`: spawn the discrete probe binary and require a SUCCESSFUL
        // exit (WR-05) — spawn-success alone masked a broken binary that exits non-zero. `go` uses the
        // bare `version` subcommand; `python3` uses the `--version` flag.
        let version_arg = if toolchain.probe_binary() == "go" {
            "version"
        } else {
            "--version"
        };
        std::process::Command::new(toolchain.probe_binary())
            .arg(version_arg)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    (toolchain.language().to_string(), present)
}

/// Run `gnr8 doctor`: a health aggregator that runs the user's `.gnr8/` pipeline once and reports its
/// diagnostics + drift (HARD-01 / D-01, D-02). Mirrors `run_check`'s shell-vs-decision split (this is
/// the impure shell; the pure grouping + exit policy lives in [`doctor::DoctorReport`]).
///
/// Collects three lifecycle facts (`.gnr8/` present, the DETECTED source-language toolchain present via
/// one `analyze::source_toolchain` decision, the pipeline runs), and —
/// when the pipeline runs cleanly — its diagnostics and the dry-run drift plan. A pipeline failure (a
/// compile error, a missing toolchain) is REPORTED as a finding, never `?`/unwrap'd into a crash
/// (Pitfall 4 / D-02). Prints the human report or `--json`, then exits non-zero ONLY on an actionable
/// problem (mirrors `run_check`).
fn run_doctor(json: bool) -> Result<()> {
    let root = project_root()?;
    let initialized = gnr8_core::workspace::manifest_path(&root).is_file();
    let (language, source_present) = probe_source_lang_toolchain(&root);

    // Run the pipeline once. Its `Err` IS the "pipeline broken" finding (do NOT `?`); on success we get
    // the child's diagnostics and can compute drift from its artifacts. Both degrade gracefully.
    let mut bundle = if initialized {
        child::run_child(&root, "__emit").ok()
    } else {
        None
    };
    let pipeline_ran = bundle.is_some();
    let diagnostics = bundle.as_ref().map(|b| b.diagnostics.clone());
    let drift = bundle.as_mut().and_then(|b| plan_bundle(&root, b).ok());

    let report = doctor::DoctorReport::assemble(
        initialized,
        source_present,
        &language,
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
