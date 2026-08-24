//! gnr8 binary entry point — the orchestrator + trusted writer (D-09).
//!
//! gnr8 is configured ONLY by code: `gnr8 init` scaffolds a `.gnr8/` Rust crate (the pipeline), and
//! every generating command runs that crate as a child process (`cargo run --manifest-path`), receives
//! its [`gnr8::runner::ArtifactBundle`], and owns writing the files (the ownership manifest, no-op
//! skip, edit protection). There is no TOML config anywhere. Each command surfaces real errors (a
//! missing `.gnr8/`, a compile error in the user's pipeline, a missing Go toolchain) through this
//! `anyhow` boundary as a clean stderr message + a deliberate non-zero exit, never a panic (RUST-04).

mod child;
mod cli;
mod doctor;
mod render;
mod watch;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Commands, GuideTopic, InspectAction, SdkPreset, SourcePreset};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

static VERIFIED_NOOP_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn main() -> Result<()> {
    let cli = Cli::parse();
    let output = Output::new(cli.json, cli.verbose);

    // `inspect` renders straight to stdout. With no path it delegates to the user's `.gnr8/` child
    // pipeline; an explicit path requests direct source analysis.
    // The remaining commands either scaffold (`init`) or delegate to the user's `.gnr8/` child crate and
    // own writing/policy.
    match &cli.command {
        Commands::Inspect { action } => run_inspect(action, output),
        Commands::Init { source, sdk } => run_init(*source, *sdk, output),
        Commands::Guide { topic } => run_guide(*topic, output),
        Commands::Generate { force } => run_generate(*force, output),
        Commands::Check => run_check(output),
        Commands::Watch { debounce_ms } => run_watch(*debounce_ms, output),
        Commands::Doctor => run_doctor(output),
    }
}

#[derive(Clone, Copy)]
struct Output {
    json: bool,
    verbose: u8,
}

impl Output {
    fn new(json: bool, verbose: u8) -> Self {
        Self { json, verbose }
    }

    fn progress(self, message: impl AsRef<str>) {
        if !self.json {
            println!("{}", message.as_ref());
        }
    }

    fn verbose(self, message: impl AsRef<str>) {
        if !self.json && self.verbose > 0 {
            println!("  {}", message.as_ref());
        }
    }

    fn verbose_paths(self, label: &str, paths: &[String]) {
        self.verbose_paths_at(2, label, paths);
    }

    /// Print a path list once verbosity reaches `level`.
    ///
    /// Actionable lists (the outputs that made `check` fail) are printed at `-v`, because the
    /// failure message tells the user that is where the paths are. Bulk lists stay at `-vv`.
    fn verbose_paths_at(self, level: u8, label: &str, paths: &[String]) {
        if self.json || self.verbose < level || paths.is_empty() {
            return;
        }
        println!("  {label}:");
        for path in paths {
            println!("    {path}");
        }
    }
}

/// The current project root, resolved against the working directory. The child runs with this as its
/// `current_dir`, and `regenerate`/`plan_only` resolve output paths against it. A `current_dir` failure
/// surfaces as `CoreError::Workspace` (clean message, never a panic).
fn project_root() -> Result<std::path::PathBuf, gnr8::CoreError> {
    std::env::current_dir().map_err(|e| gnr8::CoreError::Workspace {
        message: format!("failed to resolve the current directory: {e}"),
    })
}

/// Scaffold the mandatory `.gnr8/` generation crate in the working directory (idempotent) and summarize
/// the outcome. Re-running over an existing crate preserves the user's `src/main.rs` and reports
/// "nothing to do" (D-01). `--json` emits the created/skipped lists.
fn run_init(source: Option<SourcePreset>, sdk: Option<SdkPreset>, output: Output) -> Result<()> {
    let root = project_root()?;
    let source = source.unwrap_or(SourcePreset::GoGin);
    let sdk = sdk.unwrap_or_else(|| default_sdk_for_source(source));
    let outcome = gnr8::workspace::init_with_presets(&root, map_source(source), map_sdk(sdk))?;

    if output.json {
        #[derive(serde::Serialize)]
        struct InitReport {
            created: Vec<String>,
            skipped: Vec<String>,
            source: &'static str,
            sdk: &'static str,
        }
        let report = InitReport {
            created: outcome.created.clone(),
            skipped: outcome.skipped.clone(),
            source: source_name(source),
            sdk: sdk_name(sdk),
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    if outcome.created.is_empty() {
        output.progress(format!(
            "nothing to do — .gnr8/ already present (skipped: {})",
            outcome.skipped.join(", ")
        ));
    } else {
        if outcome.skipped.is_empty() {
            output.progress(format!(
                "initialized .gnr8/ (created: {})",
                outcome.created.join(", ")
            ));
        } else {
            output.progress(format!(
                "initialized .gnr8/ (created: {}; skipped: {})",
                outcome.created.join(", "),
                outcome.skipped.join(", ")
            ));
        }
        output.progress(
            "edit .gnr8/src/main.rs to adapt parsing + generation, then run `gnr8 generate`.",
        );
        output.progress("see .gnr8/README.md for project-local gnr8 guidance.");
    }
    Ok(())
}

fn default_sdk_for_source(source: SourcePreset) -> SdkPreset {
    match source {
        SourcePreset::GoGin => SdkPreset::Go,
        SourcePreset::Fastapi | SourcePreset::Flask => SdkPreset::Python,
        SourcePreset::Nestjs => SdkPreset::Typescript,
    }
}

fn map_source(source: SourcePreset) -> gnr8::workspace::SourcePreset {
    match source {
        SourcePreset::GoGin => gnr8::workspace::SourcePreset::GoGin,
        SourcePreset::Fastapi => gnr8::workspace::SourcePreset::FastApi,
        SourcePreset::Flask => gnr8::workspace::SourcePreset::Flask,
        SourcePreset::Nestjs => gnr8::workspace::SourcePreset::NestJs,
    }
}

fn map_sdk(sdk: SdkPreset) -> gnr8::workspace::SdkPreset {
    match sdk {
        SdkPreset::Go => gnr8::workspace::SdkPreset::Go,
        SdkPreset::Python => gnr8::workspace::SdkPreset::Python,
        SdkPreset::Typescript => gnr8::workspace::SdkPreset::TypeScript,
    }
}

fn source_name(source: SourcePreset) -> &'static str {
    match source {
        SourcePreset::GoGin => "go-gin",
        SourcePreset::Fastapi => "fastapi",
        SourcePreset::Flask => "flask",
        SourcePreset::Nestjs => "nestjs",
    }
}

fn sdk_name(sdk: SdkPreset) -> &'static str {
    match sdk {
        SdkPreset::Go => "go",
        SdkPreset::Python => "python",
        SdkPreset::Typescript => "typescript",
    }
}

const BASIC_GUIDE: &str = include_str!("../../../docs/AGENT-USAGE.md");
const GO_GIN_PY_TS_GUIDE: &str =
    include_str!("../../../docs/guides/go-gin-to-python-typescript.md");
const PYTHON_API_PY_SDK_GUIDE: &str =
    include_str!("../../../docs/guides/python-apis-to-python-sdk.md");
const NESTJS_TS_GUIDE: &str = include_str!("../../../docs/guides/nestjs-to-typescript-sdk.md");

#[derive(Clone, Copy, Debug, serde::Serialize)]
struct GuideSummary {
    id: &'static str,
    title: &'static str,
    summary: &'static str,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
struct Guide {
    id: &'static str,
    title: &'static str,
    summary: &'static str,
    markdown: &'static str,
}

fn run_guide(topic: Option<GuideTopic>, output: Output) -> Result<()> {
    let guide = guide_for(topic);
    if output.json {
        #[derive(serde::Serialize)]
        struct GuideReport {
            id: &'static str,
            title: &'static str,
            summary: &'static str,
            markdown: &'static str,
            available: Vec<GuideSummary>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&GuideReport {
                id: guide.id,
                title: guide.title,
                summary: guide.summary,
                markdown: guide.markdown,
                available: guide_summaries(),
            })?
        );
    } else {
        print!("{}", guide.markdown);
    }
    Ok(())
}

fn guide_for(topic: Option<GuideTopic>) -> Guide {
    match topic {
        None => Guide {
            id: "basic",
            title: "Basic gnr8 Agent Guide",
            summary: "Default workflow, supported source/SDK presets, common edits, recovery, and CI.",
            markdown: BASIC_GUIDE,
        },
        Some(GuideTopic::GoGinToPythonTypescript) => Guide {
            id: "go-gin-to-python-typescript",
            title: "Go/Gin Backend to Python and TypeScript SDKs",
            summary: "Complex Go/Gin setup with OpenAPI plus Python and TypeScript SDK targets.",
            markdown: GO_GIN_PY_TS_GUIDE,
        },
        Some(GuideTopic::PythonApisToPythonSdk) => Guide {
            id: "python-apis-to-python-sdk",
            title: "FastAPI or Flask Backend to Python SDK",
            summary: "Python API source extraction with typed models, diagnostics, and Python SDK output.",
            markdown: PYTHON_API_PY_SDK_GUIDE,
        },
        Some(GuideTopic::NestjsToTypescriptSdk) => Guide {
            id: "nestjs-to-typescript-sdk",
            title: "NestJS Backend to TypeScript SDK",
            summary: "NestJS controller and DTO extraction using the project TypeScript toolchain.",
            markdown: NESTJS_TS_GUIDE,
        },
    }
}

fn guide_summaries() -> Vec<GuideSummary> {
    vec![
        guide_for(Some(GuideTopic::GoGinToPythonTypescript)),
        guide_for(Some(GuideTopic::PythonApisToPythonSdk)),
        guide_for(Some(GuideTopic::NestjsToTypescriptSdk)),
    ]
    .into_iter()
    .map(|guide| GuideSummary {
        id: guide.id,
        title: guide.title,
        summary: guide.summary,
    })
    .collect()
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
    /// Per-bucket path counts.
    counts: LifecycleCounts,
    /// Timing buckets in milliseconds.
    timings_ms: LifecycleTimings,
    /// Diagnostic counts from the pipeline.
    diagnostics: DiagnosticCounts,
    /// Cache/write mode used for the run.
    cache_mode: String,
    /// Number of source/input files considered.
    source_files: usize,
    /// Number of generated artifact files considered.
    artifact_files: usize,
}

#[derive(Debug, serde::Serialize)]
struct LifecycleCounts {
    written: usize,
    unchanged: usize,
    skipped: usize,
    deleted: usize,
}

#[derive(Debug, serde::Serialize)]
struct LifecycleTimings {
    hot_noop: u128,
    pipeline: Option<u128>,
    write: Option<u128>,
    total: u128,
}

#[derive(Debug, serde::Serialize)]
struct DiagnosticCounts {
    total: usize,
    info: usize,
    warn: usize,
    error: usize,
}

/// Run `gnr8 generate` (+ `--force`): run the user's `.gnr8/` pipeline (child process), then write only
/// changed files and report counts. Every protected (user-edited) file is named in a stderr warning so
/// the "no silent clobbering" protection is VISIBLE (T-04-02-04). Pipeline diagnostics the child carried
/// are surfaced too. `--json` serializes the counts. A missing `.gnr8/` (run `gnr8 init`), a compile
/// error in the user's pipeline, or a missing Go toolchain surface via the anyhow boundary, never a panic.
fn run_generate(force: bool, output: Output) -> Result<()> {
    let root = project_root()?;
    gnr8::lifecycle::with_generation_state_lock(&root, || {
        cleanup_verified_noop_temporary_files(&root)
    })??;
    let total_start = Instant::now();
    let hot_start = Instant::now();
    let hot_noop = if force {
        None
    } else {
        pre_child_generate_verified_noop(&root)?
    };
    let hot_elapsed = hot_start.elapsed();
    let mut pipeline_elapsed = None;
    let mut write_elapsed = None;
    let (outcome, diagnostics, cache_label, source_files, artifact_files) =
        if let Some(noop) = hot_noop {
            (
                noop.outcome,
                noop.diagnostics,
                "verified hot no-op",
                noop.source_files,
                noop.artifact_files,
            )
        } else {
            output.progress("generate: running pipeline");
            let pipeline_start = Instant::now();
            let mut bundle = child::run_child(&root, "__emit")?;
            pipeline_elapsed = Some(pipeline_start.elapsed());
            let source_files = bundle.cache_input_stamps.len();
            let mut artifact_files = bundle.artifacts.len();
            output.progress("generate: writing outputs");
            let write_start = Instant::now();
            let outcome = regenerate_bundle(&root, &mut bundle, force)?;
            write_elapsed = Some(write_start.elapsed());
            if artifact_files == 0 {
                artifact_files =
                    outcome.written.len() + outcome.unchanged.len() + outcome.skipped.len();
            }
            (
                outcome,
                bundle.diagnostics.clone(),
                "pipeline",
                source_files,
                artifact_files,
            )
        };

    print_diagnostics(output, &diagnostics);
    // Warn (stderr) for every protected file so the user SEES which outputs were not clobbered.
    for path in &outcome.skipped {
        eprintln!(
            "warning: {path} was hand-edited since gnr8 last wrote it — skipped (use --force to overwrite)"
        );
    }

    let skipped_count = outcome.skipped.len();
    if output.json {
        let counts = LifecycleCounts {
            written: outcome.written.len(),
            unchanged: outcome.unchanged.len(),
            skipped: outcome.skipped.len(),
            deleted: outcome.deleted.len(),
        };
        let report = LifecycleReport {
            written: outcome.written,
            unchanged: outcome.unchanged,
            skipped: outcome.skipped,
            deleted: outcome.deleted,
            counts,
            timings_ms: LifecycleTimings {
                hot_noop: duration_ms(hot_elapsed),
                pipeline: pipeline_elapsed.map(duration_ms),
                write: write_elapsed.map(duration_ms),
                total: duration_ms(total_start.elapsed()),
            },
            diagnostics: diagnostic_counts(&diagnostics),
            cache_mode: cache_label.to_string(),
            source_files,
            artifact_files,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        let summary = lifecycle_summary(&outcome);
        output.progress(format!("generate: done ({summary})"));
        output.verbose(format!("mode: {cache_label}"));
        output.verbose(format!("parsed/input files: {source_files}"));
        output.verbose(format!("artifacts: {artifact_files}"));
        output.verbose(format!("hot no-op check: {}", fmt_duration(hot_elapsed)));
        if let Some(elapsed) = pipeline_elapsed {
            output.verbose(format!("pipeline: {}", fmt_duration(elapsed)));
        }
        if let Some(elapsed) = write_elapsed {
            output.verbose(format!("write plan: {}", fmt_duration(elapsed)));
        }
        output.verbose(format!("total: {}", fmt_duration(total_start.elapsed())));
        output.verbose_paths("written", &outcome.written);
        output.verbose_paths("deleted", &outcome.deleted);
        output.verbose_paths("skipped", &outcome.skipped);
    }
    if skipped_count > 0 {
        bail!("generation incomplete: {skipped_count} protected output(s) were skipped");
    }
    Ok(())
}

/// Run `gnr8 check`: run the user's `.gnr8/` pipeline, then DRY-RUN the same `plan_writes` decision (no
/// writes, no manifest save). Exits NON-ZERO (code 1) if any output is stale (`Write`) or drifted
/// (`UserEdited`); exits 0 when every output is `Unchanged`. Reuses the exact pure decision function —
/// zero new policy. `--json` emits the stale/drifted path lists. Pipeline errors flow through the anyhow
/// boundary, never a panic.
#[allow(clippy::too_many_lines)]
fn run_check(output: Output) -> Result<()> {
    let root = project_root()?;
    let total_start = Instant::now();
    let hot_start = Instant::now();
    let hot_noop = pre_child_verified_noop(&root);
    let hot_elapsed = hot_start.elapsed();
    let mut pipeline_elapsed = None;
    let mut plan_elapsed = None;
    let (plan, diagnostics, cache_label, source_files, artifact_files) =
        if let Some(noop) = hot_noop {
            let artifact_files = noop.artifact_files;
            (
                clean_plan_from_paths(noop.outcome.unchanged),
                noop.diagnostics,
                "verified hot no-op",
                noop.source_files,
                artifact_files,
            )
        } else {
            output.progress("check: running pipeline");
            let pipeline_start = Instant::now();
            let mut bundle = child::run_child(&root, "__emit")?;
            pipeline_elapsed = Some(pipeline_start.elapsed());
            let source_files = bundle.cache_input_stamps.len();
            let mut artifact_files = bundle.artifacts.len();
            let diagnostics = bundle.diagnostics.clone();
            output.progress("check: planning writes");
            let plan_start = Instant::now();
            let plan = plan_bundle(&root, &mut bundle)?;
            plan_elapsed = Some(plan_start.elapsed());
            if artifact_files == 0 {
                artifact_files = plan.files.len();
            }
            (plan, diagnostics, "pipeline", source_files, artifact_files)
        };

    // `check` is the CI gate, so it is the run whose diagnostics a reader most needs. It printed
    // none: a gate that failed on drift reported a count of stale paths and nothing about WHY the
    // outputs changed, which is what made the toolchain failure in issue #67 take a manual diff to
    // find. `generate` has always printed these; `check` now says the same thing.
    print_diagnostics(output, &diagnostics);

    // Partition the plan into stale (would be written) vs drifted (user-edited) vs clean (unchanged).
    let mut stale: Vec<String> = Vec::new();
    let mut drifted: Vec<String> = Vec::new();
    let mut clean: Vec<String> = Vec::new();
    for file in &plan.files {
        match file.action {
            gnr8::lifecycle::WriteAction::Write => stale.push(file.path.clone()),
            gnr8::lifecycle::WriteAction::UserEdited => drifted.push(file.path.clone()),
            gnr8::lifecycle::WriteAction::Unchanged => clean.push(file.path.clone()),
        }
    }
    let has_drift = plan.has_drift();

    if output.json {
        #[derive(serde::Serialize)]
        struct CheckReport {
            up_to_date: bool,
            stale: Vec<String>,
            drifted: Vec<String>,
            unchanged: Vec<String>,
            counts: CheckCounts,
            timings_ms: LifecycleTimings,
            diagnostics: DiagnosticCounts,
            cache_mode: String,
            source_files: usize,
            artifact_files: usize,
        }
        #[derive(serde::Serialize)]
        struct CheckCounts {
            stale: usize,
            drifted: usize,
            unchanged: usize,
        }
        let report = CheckReport {
            up_to_date: !has_drift,
            stale: stale.clone(),
            drifted: drifted.clone(),
            unchanged: clean.clone(),
            counts: CheckCounts {
                stale: stale.len(),
                drifted: drifted.len(),
                unchanged: clean.len(),
            },
            timings_ms: LifecycleTimings {
                hot_noop: duration_ms(hot_elapsed),
                pipeline: pipeline_elapsed.map(duration_ms),
                write: plan_elapsed.map(duration_ms),
                total: duration_ms(total_start.elapsed()),
            },
            diagnostics: diagnostic_counts(&diagnostics),
            cache_mode: cache_label.to_string(),
            source_files,
            artifact_files,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if has_drift {
        output.progress(format!(
            "check: not up to date ({} stale, {} drifted; run `gnr8 generate`, or `gnr8 check -v` for paths)",
            stale.len(),
            drifted.len()
        ));
    } else {
        output.progress(format!("check: up to date ({} unchanged)", clean.len()));
    }
    output.verbose(format!("parsed/input files: {source_files}"));
    output.verbose(format!("outputs checked: {}", plan.files.len()));
    output.verbose(format!("hot no-op check: {}", fmt_duration(hot_elapsed)));
    if let Some(elapsed) = pipeline_elapsed {
        output.verbose(format!("pipeline: {}", fmt_duration(elapsed)));
    }
    if let Some(elapsed) = plan_elapsed {
        output.verbose(format!("write plan: {}", fmt_duration(elapsed)));
    }
    output.verbose(format!("total: {}", fmt_duration(total_start.elapsed())));
    // The failure message promises these paths at `-v`, so print them at `-v`.
    output.verbose_paths_at(1, "stale", &stale);
    output.verbose_paths_at(1, "drifted", &drifted);

    if has_drift {
        // Deliberate non-zero exit so `gnr8 check` is a usable CI gate (RESEARCH Open Q 3).
        std::process::exit(1);
    }
    Ok(())
}

fn clean_plan_from_paths(paths: Vec<String>) -> gnr8::lifecycle::WritePlan {
    gnr8::lifecycle::WritePlan {
        files: paths
            .into_iter()
            .map(|path| gnr8::lifecycle::PlannedFile {
                path,
                action: gnr8::lifecycle::WriteAction::Unchanged,
                new_bytes: Vec::new(),
                new_hash: String::new(),
                source: "generated".to_string(),
            })
            .collect(),
    }
}

pub(crate) fn regenerate_bundle(
    root: &std::path::Path,
    bundle: &mut gnr8::runner::ArtifactBundle,
    force: bool,
) -> Result<gnr8::lifecycle::GenerateOutcome, gnr8::CoreError> {
    if let Some(metadata) = cached_artifact_metadata(root, bundle) {
        gnr8::lifecycle::recover_cached_output_transactions(root, &metadata)?;
        if let Some(outcome) = verified_noop_outcome(root, bundle, &metadata) {
            save_verified_noop_stamp(root, bundle, &metadata, &outcome);
            return Ok(outcome);
        }
        if let Some(outcome) = gnr8::lifecycle::regenerate_cached_with_anchors(
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
    let outcome = gnr8::lifecycle::regenerate_with_anchors(
        root,
        &bundle.artifacts,
        &bundle.output_anchors,
        force,
    )?;
    save_verified_noop_stamp_from_artifacts(root, bundle, &outcome);
    Ok(outcome)
}

fn plan_bundle(
    root: &std::path::Path,
    bundle: &mut gnr8::runner::ArtifactBundle,
) -> Result<gnr8::lifecycle::WritePlan, gnr8::CoreError> {
    if let Some(metadata) = cached_artifact_metadata(root, bundle) {
        return gnr8::lifecycle::plan_only_cached(root, &metadata);
    }
    ensure_bundle_artifacts(root, bundle)?;
    gnr8::lifecycle::plan_only(root, &bundle.artifacts)
}

fn cached_artifact_metadata(
    root: &std::path::Path,
    bundle: &gnr8::runner::ArtifactBundle,
) -> Option<Vec<gnr8::sdk::ArtifactMetadata>> {
    let key = bundle.artifact_cache_key.as_deref()?;
    gnr8::sdk::load_artifact_cache_metadata(root, key)
}

fn ensure_bundle_artifacts(
    root: &std::path::Path,
    bundle: &mut gnr8::runner::ArtifactBundle,
) -> Result<(), gnr8::CoreError> {
    if !bundle.artifacts.is_empty() {
        return Ok(());
    }
    let Some(key) = bundle.artifact_cache_key.as_deref() else {
        return Ok(());
    };
    bundle.artifacts =
        gnr8::sdk::load_artifact_cache_files(root, key).ok_or_else(|| {
            gnr8::CoreError::ChildRun {
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
    #[serde(default)]
    cli_version: String,
    #[serde(default)]
    core_version: String,
    #[serde(default)]
    capability_fingerprint: String,
    artifact_cache_key: String,
    output_anchors: Vec<String>,
    artifact_paths: Vec<String>,
    input_roots: Vec<String>,
    #[serde(default)]
    input_fast_files: Vec<FastFileStamp>,
    #[serde(default)]
    output_artifact_fast_files: Vec<FastFileStamp>,
    #[serde(default)]
    output_dir_fast_stamps: Vec<FastDirStamp>,
    #[serde(default)]
    input_files: Vec<gnr8::sdk::FileStamp>,
    #[serde(default)]
    source_files: Vec<gnr8::sdk::FileStamp>,
    #[serde(default)]
    config_files: Vec<gnr8::sdk::FileStamp>,
    #[serde(default)]
    tool_files: Vec<gnr8::sdk::FileStamp>,
    #[serde(default)]
    pipeline_files: Vec<gnr8::sdk::FileStamp>,
    #[serde(default)]
    pipeline_roots: Vec<String>,
    #[serde(default)]
    output_files: Vec<gnr8::sdk::FileStamp>,
    diagnostics: Vec<gnr8::graph::Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
struct FastFileStamp {
    path: String,
    len: u64,
    modified_ns: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
struct FastDirStamp {
    path: String,
    modified_ns: u128,
}

struct FastOutputStamps {
    artifact_files: Vec<FastFileStamp>,
    dirs: Vec<FastDirStamp>,
}

struct VerifiedInputStamps {
    fast: Vec<FastFileStamp>,
    all: Vec<gnr8::sdk::FileStamp>,
    source: Vec<gnr8::sdk::FileStamp>,
    config: Vec<gnr8::sdk::FileStamp>,
    tools: Vec<gnr8::sdk::FileStamp>,
    pipeline: Vec<gnr8::sdk::FileStamp>,
}

struct CachedNoop {
    outcome: gnr8::lifecycle::GenerateOutcome,
    diagnostics: Vec<gnr8::graph::Diagnostic>,
    source_files: usize,
    artifact_files: usize,
}

fn pre_child_verified_noop(root: &std::path::Path) -> Option<CachedNoop> {
    if !gnr8::runner::PRE_CHILD_NOOP_SUPPORTED {
        return None;
    }
    let stamp = load_verified_noop_stamp(root)?;
    if stamp.cli_version != env!("CARGO_PKG_VERSION")
        || stamp.core_version != env!("CARGO_PKG_VERSION")
        || stamp.capability_fingerprint != gnr8::runner::capability_fingerprint()
        || stamp.source_files.is_empty()
        || stamp.config_files.is_empty()
        || stamp.tool_files.is_empty()
        || stamp.output_files.len() != stamp.artifact_paths.len()
        || !manifest_covers_output_files(root, &stamp.output_files)
    {
        return None;
    }
    let source_fast = collect_declared_input_fast_stamps(root, &stamp.input_roots)?;
    let config_fast = collect_required_config_fast_stamps(root)?;
    let tool_fast = fast_stamps_for_expected_files(root, &stamp.tool_files)?;
    let pipeline_fast =
        collect_pipeline_fast_stamps(root, &stamp.pipeline_roots, &stamp.pipeline_files)?;
    if content_stamps_from_fast(root, &source_fast)? != stamp.source_files
        || content_stamps_from_fast(root, &config_fast)? != stamp.config_files
        || content_stamps_from_fast(root, &tool_fast)? != stamp.tool_files
        || content_stamps_from_fast(root, &pipeline_fast)? != stamp.pipeline_files
    {
        return None;
    }
    let mut current_inputs = source_fast;
    current_inputs.extend(config_fast);
    current_inputs.extend(tool_fast);
    current_inputs.extend(pipeline_fast);
    current_inputs.sort();
    if current_inputs != stamp.input_fast_files {
        return None;
    }
    if content_stamps_from_fast(root, &current_inputs)? != stamp.input_files {
        return None;
    }
    let current_outputs =
        collect_verified_fast_output_stamps(root, &stamp.output_anchors, &stamp.artifact_paths)?;
    if current_outputs.artifact_files != stamp.output_artifact_fast_files
        || current_outputs.dirs != stamp.output_dir_fast_stamps
    {
        return None;
    }
    if content_stamps_from_fast(root, &current_outputs.artifact_files)? != stamp.output_files {
        return None;
    }
    let source_files = stamp.source_files.len();
    let artifact_files = stamp.artifact_paths.len();
    Some(CachedNoop {
        outcome: gnr8::lifecycle::GenerateOutcome {
            written: Vec::new(),
            unchanged: stamp.artifact_paths,
            skipped: Vec::new(),
            deleted: Vec::new(),
        },
        diagnostics: stamp.diagnostics,
        source_files,
        artifact_files,
    })
}

fn pre_child_generate_verified_noop(root: &std::path::Path) -> Result<Option<CachedNoop>> {
    if !gnr8::runner::PRE_CHILD_NOOP_SUPPORTED {
        return Ok(None);
    }
    let Some(stamp) = load_verified_noop_stamp(root) else {
        return Ok(None);
    };
    let metadata = stamp
        .output_files
        .iter()
        .map(|file| gnr8::sdk::ArtifactMetadata {
            path: file.path.clone(),
            hash: file.hash.clone(),
        })
        .collect::<Vec<_>>();
    gnr8::lifecycle::recover_cached_output_transactions(root, &metadata)?;
    Ok(pre_child_verified_noop(root))
}

fn verified_noop_outcome(
    root: &std::path::Path,
    bundle: &gnr8::runner::ArtifactBundle,
    metadata: &[gnr8::sdk::ArtifactMetadata],
) -> Option<gnr8::lifecycle::GenerateOutcome> {
    let key = bundle.artifact_cache_key.as_deref()?;
    let stamp = load_verified_noop_stamp(root)?;
    if stamp.artifact_cache_key != key || stamp.output_anchors != bundle.output_anchors {
        return None;
    }
    if !manifest_covers_metadata(root, metadata) {
        return None;
    }
    let artifact_paths = artifact_paths(metadata);
    if stamp.artifact_paths != artifact_paths {
        return None;
    }
    let current =
        collect_verified_fast_output_stamps(root, &bundle.output_anchors, &artifact_paths)?;
    if current.artifact_files != stamp.output_artifact_fast_files
        || current.dirs != stamp.output_dir_fast_stamps
    {
        return None;
    }
    let current_files = content_stamps_from_fast(root, &current.artifact_files)?;
    if current_files != stamp.output_files
        || metadata.iter().any(|artifact| {
            current_files
                .iter()
                .find(|file| file.path == artifact.path)
                .is_none_or(|file| file.hash != artifact.hash)
        })
    {
        return None;
    }
    Some(gnr8::lifecycle::GenerateOutcome {
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
    bundle: &gnr8::runner::ArtifactBundle,
    metadata: &[gnr8::sdk::ArtifactMetadata],
    outcome: &gnr8::lifecycle::GenerateOutcome,
) {
    save_verified_noop_stamp_for_metadata(root, bundle, metadata, outcome);
}

fn save_verified_noop_stamp_from_artifacts(
    root: &std::path::Path,
    bundle: &gnr8::runner::ArtifactBundle,
    outcome: &gnr8::lifecycle::GenerateOutcome,
) {
    let metadata: Vec<gnr8::sdk::ArtifactMetadata> = bundle
        .artifacts
        .iter()
        .map(|artifact| gnr8::sdk::ArtifactMetadata {
            path: artifact.path.clone(),
            hash: gnr8::manifest::blake3_hex(artifact.text.as_bytes()),
        })
        .collect();
    save_verified_noop_stamp_for_metadata(root, bundle, &metadata, outcome);
}

fn save_verified_noop_stamp_for_metadata(
    root: &std::path::Path,
    bundle: &gnr8::runner::ArtifactBundle,
    metadata: &[gnr8::sdk::ArtifactMetadata],
    outcome: &gnr8::lifecycle::GenerateOutcome,
) {
    let _ = gnr8::lifecycle::with_generation_state_lock(root, || {
        save_verified_noop_stamp_locked(root, bundle, metadata, outcome);
    });
}

fn save_verified_noop_stamp_locked(
    root: &std::path::Path,
    bundle: &gnr8::runner::ArtifactBundle,
    metadata: &[gnr8::sdk::ArtifactMetadata],
    outcome: &gnr8::lifecycle::GenerateOutcome,
) {
    if !gnr8::runner::PRE_CHILD_NOOP_SUPPORTED {
        return;
    }
    if cleanup_verified_noop_temporary_files(root).is_err() {
        return;
    }
    if !outcome.written.is_empty() || !outcome.skipped.is_empty() || !outcome.deleted.is_empty() {
        return;
    }
    let Some(key) = bundle.artifact_cache_key.as_deref() else {
        return;
    };
    if bundle.cache_input_roots.is_empty()
        || bundle.cache_input_stamps.is_empty()
        || bundle.cache_config_stamps.is_empty()
        || !bundle.cache_config_complete
        || !bundle.cache_pipeline_complete
        || bundle.cache_tool_stamps.is_empty()
    {
        return;
    }
    let artifact_paths = artifact_paths(metadata);
    let Some(output_fast) =
        collect_verified_fast_output_stamps(root, &bundle.output_anchors, &artifact_paths)
    else {
        return;
    };
    let Some(output_files) = content_stamps_from_fast(root, &output_fast.artifact_files) else {
        return;
    };
    if output_files.len() != artifact_paths.len()
        || !manifest_covers_output_files(root, &output_files)
        || metadata.iter().any(|artifact| {
            output_files
                .iter()
                .find(|file| file.path == artifact.path)
                .is_none_or(|file| file.hash != artifact.hash)
        })
    {
        return;
    }
    let Some(inputs) = collect_verified_input_stamps(root, bundle) else {
        return;
    };
    let stamp = VerifiedNoopStamp {
        cli_version: bundle.cli_version.clone(),
        core_version: bundle.core_version.clone(),
        capability_fingerprint: bundle.capability_fingerprint.clone(),
        artifact_cache_key: key.to_string(),
        output_anchors: bundle.output_anchors.clone(),
        artifact_paths,
        input_roots: bundle.cache_input_roots.clone(),
        input_fast_files: inputs.fast,
        output_artifact_fast_files: output_fast.artifact_files,
        output_dir_fast_stamps: output_fast.dirs,
        input_files: inputs.all,
        source_files: inputs.source,
        config_files: inputs.config,
        tool_files: inputs.tools,
        pipeline_files: inputs.pipeline,
        pipeline_roots: bundle.cache_pipeline_roots.clone(),
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
    let _ = publish_verified_noop_stamp(&path, &bytes);
}

fn collect_verified_input_stamps(
    root: &std::path::Path,
    bundle: &gnr8::runner::ArtifactBundle,
) -> Option<VerifiedInputStamps> {
    let source_fast = collect_declared_input_fast_stamps(root, &bundle.cache_input_roots)?;
    let source = content_stamps_from_fast(root, &source_fast)?;
    if source != bundle.cache_input_stamps {
        return None;
    }
    let config_fast = collect_required_config_fast_stamps(root)?;
    let config = content_stamps_from_fast(root, &config_fast)?;
    if config != bundle.cache_config_stamps {
        return None;
    }
    let tool_fast = fast_stamps_for_expected_files(root, &bundle.cache_tool_stamps)?;
    let tools = content_stamps_from_fast(root, &tool_fast)?;
    if tools != bundle.cache_tool_stamps {
        return None;
    }
    let pipeline_fast = collect_pipeline_fast_stamps(
        root,
        &bundle.cache_pipeline_roots,
        &bundle.cache_pipeline_stamps,
    )?;
    let pipeline = content_stamps_from_fast(root, &pipeline_fast)?;
    if pipeline != bundle.cache_pipeline_stamps {
        return None;
    }
    let mut fast = source_fast;
    fast.extend(config_fast);
    fast.extend(tool_fast);
    fast.extend(pipeline_fast);
    fast.sort();
    let all = content_stamps_from_fast(root, &fast)?;
    Some(VerifiedInputStamps {
        fast,
        all,
        source,
        config,
        tools,
        pipeline,
    })
}

fn publish_verified_noop_stamp(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "verified no-op stamp has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let sequence = VERIFIED_NOOP_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".verified-noop-{}-{sequence}.tmp",
        std::process::id()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&temporary)?;
    let published = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            replace_verified_noop_stamp(&temporary, path)
        });
    if published.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    published
}

fn replace_verified_noop_stamp(
    from: &std::path::Path,
    to: &std::path::Path,
) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        atomicwrites::replace_atomic(from, to)
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(from, to)
    }
}

fn cleanup_verified_noop_temporary_files(root: &std::path::Path) -> std::io::Result<()> {
    let dir = root.join(".gnr8/cache");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if is_verified_noop_temporary_name(name) {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn is_verified_noop_temporary_name(name: &str) -> bool {
    let Some(token) = name
        .strip_prefix(".verified-noop-")
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    let Some((pid, sequence)) = token.split_once('-') else {
        return false;
    };
    !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !sequence.is_empty()
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

fn content_stamps_from_fast(
    root: &std::path::Path,
    fast_files: &[FastFileStamp],
) -> Option<Vec<gnr8::sdk::FileStamp>> {
    let mut stamps = Vec::with_capacity(fast_files.len());
    for fast in fast_files {
        let path = root.join(&fast.path);
        let metadata = path.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        stamps.push(gnr8::sdk::FileStamp {
            path: fast.path.clone(),
            len: metadata.len(),
            modified_ns: fast_modified_ns(&metadata),
            hash: gnr8::manifest::blake3_hex(&bytes),
        });
    }
    stamps.sort();
    Some(stamps)
}

fn manifest_covers_output_files(root: &std::path::Path, files: &[gnr8::sdk::FileStamp]) -> bool {
    let Ok(manifest) = gnr8::manifest::load(&root.join(".gnr8")) else {
        return false;
    };
    manifest.files.len() == files.len()
        && files
            .iter()
            .all(|file| manifest.recorded_hash(&file.path) == Some(file.hash.as_str()))
}

fn manifest_covers_metadata(
    root: &std::path::Path,
    artifacts: &[gnr8::sdk::ArtifactMetadata],
) -> bool {
    let Ok(manifest) = gnr8::manifest::load(&root.join(".gnr8")) else {
        return false;
    };
    manifest.files.len() == artifacts.len()
        && artifacts
            .iter()
            .all(|artifact| manifest.recorded_hash(&artifact.path) == Some(artifact.hash.as_str()))
}

fn collect_verified_fast_output_stamps(
    root: &std::path::Path,
    output_anchors: &[String],
    artifact_paths: &[String],
) -> Option<FastOutputStamps> {
    let artifact_paths: Vec<std::path::PathBuf> =
        artifact_paths.iter().map(|path| root.join(path)).collect();
    let artifact_files = stamp_fast_project_files(root, &artifact_paths)?;
    let mut dirs = std::collections::BTreeSet::new();
    for anchor in output_anchors {
        collect_anchor_dir_stamp_paths(root, anchor, &mut dirs)?;
    }
    let dirs: Vec<std::path::PathBuf> = dirs.into_iter().collect();
    let dirs = stamp_fast_project_dirs(root, &dirs)?;
    Some(FastOutputStamps {
        artifact_files,
        dirs,
    })
}

fn collect_anchor_dir_stamp_paths(
    root: &std::path::Path,
    anchor: &str,
    paths: &mut std::collections::BTreeSet<std::path::PathBuf>,
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
        if let Some(parent) = anchor_path.parent() {
            paths.insert(parent.to_path_buf());
        }
        return Some(());
    }
    if !anchor_path.is_dir() {
        return Some(());
    }
    paths.insert(anchor_path.clone());
    let mut stack = vec![anchor_path];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries {
            let entry = entry.ok()?;
            let path = entry.path();
            let kind = entry.file_type().ok()?;
            if kind.is_dir() {
                paths.insert(path.clone());
                stack.push(path);
            }
        }
    }
    Some(())
}

fn artifact_paths(metadata: &[gnr8::sdk::ArtifactMetadata]) -> Vec<String> {
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

fn collect_declared_input_fast_stamps(
    root: &std::path::Path,
    input_roots: &[String],
) -> Option<Vec<FastFileStamp>> {
    if input_roots.is_empty() {
        return None;
    }
    let mut stamps = Vec::new();
    for input_root in input_roots {
        let path = root.join(input_root);
        if path.is_file() {
            push_fast_file_stamp(root, &path, &mut stamps)?;
        } else {
            collect_hot_input_file_stamps(root, &path, &mut stamps)?;
        }
    }
    stamps.sort();
    Some(stamps)
}

fn collect_required_config_fast_stamps(root: &std::path::Path) -> Option<Vec<FastFileStamp>> {
    let paths = gnr8::sdk::cache_config_input_paths(root)?;
    stamp_fast_project_files(root, &paths)
}

fn fast_stamps_for_expected_files(
    root: &std::path::Path,
    expected: &[gnr8::sdk::FileStamp],
) -> Option<Vec<FastFileStamp>> {
    if expected.is_empty() {
        return Some(Vec::new());
    }
    let paths = expected
        .iter()
        .map(|stamp| root.join(&stamp.path))
        .collect::<Vec<_>>();
    stamp_fast_project_files(root, &paths)
}

fn collect_pipeline_fast_stamps(
    root: &std::path::Path,
    input_roots: &[String],
    expected: &[gnr8::sdk::FileStamp],
) -> Option<Vec<FastFileStamp>> {
    let mut stamps = fast_stamps_for_expected_files(root, expected)?;
    if !input_roots.is_empty() {
        stamps.extend(collect_declared_input_fast_stamps(root, input_roots)?);
    }
    stamps.sort();
    stamps.dedup();
    Some(stamps)
}

fn collect_hot_input_file_stamps(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<FastFileStamp>,
) -> Option<()> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        let name = path.file_name().and_then(|name| name.to_str())?;
        let kind = entry.file_type().ok()?;
        if kind.is_dir() {
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
            collect_hot_input_file_stamps(root, &path, out)?;
        } else if kind.is_file() {
            push_fast_file_stamp(root, &path, out)?;
        }
    }
    Some(())
}

fn push_fast_file_stamp(
    root: &std::path::Path,
    path: &std::path::Path,
    out: &mut Vec<FastFileStamp>,
) -> Option<()> {
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    out.push(FastFileStamp {
        path: fast_project_relative_path(root, path),
        len: metadata.len(),
        modified_ns: fast_modified_ns(&metadata),
    });
    Some(())
}

fn stamp_fast_project_files(
    root: &Path,
    paths: &[std::path::PathBuf],
) -> Option<Vec<FastFileStamp>> {
    if paths.is_empty() {
        return Some(Vec::new());
    }
    let workers = std::thread::available_parallelism().map_or(4, usize::from);
    let workers = workers.clamp(1, paths.len());
    if workers == 1 || paths.len() < 512 {
        return stamp_fast_project_files_serial(root, paths);
    }
    let chunk_size = paths.len().div_ceil(workers);

    let mut stamps = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in paths.chunks(chunk_size) {
            handles.push(scope.spawn(move || stamp_fast_project_files_serial(root, chunk)));
        }

        let mut stamps = Vec::with_capacity(paths.len());
        for handle in handles {
            let chunk = handle.join().ok()??;
            stamps.extend(chunk);
        }
        Some(stamps)
    })?;
    stamps.sort();
    Some(stamps)
}

fn stamp_fast_project_files_serial(
    root: &Path,
    paths: &[std::path::PathBuf],
) -> Option<Vec<FastFileStamp>> {
    let mut stamps = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = path.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }
        stamps.push(FastFileStamp {
            path: fast_project_relative_path(root, path),
            len: metadata.len(),
            modified_ns: fast_modified_ns(&metadata),
        });
    }
    stamps.sort();
    Some(stamps)
}

fn stamp_fast_project_dirs(root: &Path, paths: &[std::path::PathBuf]) -> Option<Vec<FastDirStamp>> {
    let mut stamps = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = path.metadata().ok()?;
        if !metadata.is_dir() {
            return None;
        }
        stamps.push(FastDirStamp {
            path: fast_project_relative_path(root, path),
            modified_ns: fast_modified_ns(&metadata),
        });
    }
    stamps.sort();
    Some(stamps)
}

fn fast_project_relative_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

fn fast_modified_ns(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn verified_noop_stamp_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".gnr8").join("cache").join("verified-noop.json")
}

/// Probe whether the DETECTED source language's toolchain is ACTUALLY ready, returning `(language,
/// present)`.
///
/// One `gnr8::analyze::source_toolchain` decision over the project root picks the language (the
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
    let Ok(toolchain) = gnr8::analyze::source_toolchain(&root.to_string_lossy()) else {
        return ("unknown".to_string(), false);
    };
    let present = if toolchain == gnr8::analyze::SourceToolchain::TypeScript {
        // TypeScript's real toolchain is `node` + a resolvable `typescript`; the core probe verifies
        // BOTH via the same resolution `generate` uses (WR-02 — one source of truth, no fallback).
        gnr8::analyze::typescript_toolchain_present(&root.to_string_lossy())
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

fn probe_source_lang_toolchain_from_roots(
    project_root: &Path,
    input_roots: &[String],
) -> Option<(String, bool)> {
    let mut resolved: Option<(String, bool)> = None;
    for input_root in input_roots {
        let (language, present) = probe_source_lang_toolchain(&project_root.join(input_root));
        if language == "unknown" {
            continue;
        }
        match &mut resolved {
            None => resolved = Some((language, present)),
            Some((existing_language, existing_present)) if existing_language == &language => {
                *existing_present = *existing_present && present;
            }
            Some(_) => return None,
        }
    }
    resolved
}

fn reconcile_doctor_source_probe(
    project_root: &Path,
    initial: (String, bool),
    pipeline_ran: bool,
    input_roots: &[String],
) -> (String, bool) {
    if !pipeline_ran {
        return initial;
    }

    if let Some((language, present)) =
        probe_source_lang_toolchain_from_roots(project_root, input_roots)
    {
        return (language, present || pipeline_ran);
    }

    if !initial.1 {
        return ("configured".to_string(), true);
    }

    initial
}

fn collect_sdk_readiness(
    root: &Path,
    bundle: &mut gnr8::runner::ArtifactBundle,
) -> Vec<doctor::SdkReadiness> {
    if let Err(err) = ensure_bundle_artifacts(root, bundle) {
        return vec![doctor::SdkReadiness::not_ready(
            "artifacts",
            "generated",
            "artifact cache",
            err.to_string(),
        )];
    }

    bundle
        .readiness_targets
        .iter()
        .map(|target| {
            let artifacts = artifacts_for_readiness(bundle, target);
            readiness_for_target(target, &artifacts)
        })
        .collect()
}

fn artifacts_for_readiness(
    bundle: &gnr8::runner::ArtifactBundle,
    target: &gnr8::sdk::ReadinessTarget,
) -> Vec<gnr8::sdk::Artifact> {
    let output_path = target.output_path.trim_end_matches('/');
    let prefix = format!("{output_path}/");
    bundle
        .artifacts
        .iter()
        .filter(|artifact| artifact.path == output_path || artifact.path.starts_with(&prefix))
        .cloned()
        .collect()
}

fn readiness_for_target(
    target: &gnr8::sdk::ReadinessTarget,
    artifacts: &[gnr8::sdk::Artifact],
) -> doctor::SdkReadiness {
    use gnr8::sdk::ReadinessKind;

    let output_path = target.output_path.as_str();
    match target.kind {
        ReadinessKind::OpenApi => artifacts
            .iter()
            .find(|artifact| artifact.path == output_path)
            .map_or_else(
                || {
                    doctor::SdkReadiness::not_ready(
                        "openapi",
                        output_path,
                        "built-in OpenAPI parser",
                        "declared OpenAPI target did not emit its artifact",
                    )
                },
                |artifact| validate_openapi_target(&artifact.path, &artifact.text),
            ),
        ReadinessKind::Go => validate_go_target(output_path, artifacts),
        ReadinessKind::Python => validate_python_target(output_path, artifacts),
        ReadinessKind::TypeScript => validate_typescript_target(output_path, artifacts),
    }
}

fn path_extension_is(path: &str, ext: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|actual| actual.eq_ignore_ascii_case(ext))
}

fn validate_openapi_target(path: &str, text: &str) -> doctor::SdkReadiness {
    match gnr8::sdk::validate_openapi_artifact(text, Path::new(path)) {
        Ok(()) => doctor::SdkReadiness::ready("openapi", path, "built-in OpenAPI parser"),
        Err(err) => doctor::SdkReadiness::not_ready(
            "openapi",
            path,
            "built-in OpenAPI parser",
            err.to_string(),
        ),
    }
}

fn validate_go_target(anchor: &str, artifacts: &[gnr8::sdk::Artifact]) -> doctor::SdkReadiness {
    const TOOLCHAIN: &str = "go test ./...; go vet ./...";
    if let Err(reason) = command_available("go", &["version"]) {
        return doctor::SdkReadiness::not_ready("go", anchor, TOOLCHAIN, reason);
    }
    let Ok(materialized) = materialize_artifact_group(anchor, artifacts, "go") else {
        return doctor::SdkReadiness::not_ready(
            "go",
            anchor,
            TOOLCHAIN,
            "failed to materialize generated Go SDK for readiness",
        );
    };
    if !materialized.target_dir.join("go.mod").is_file() {
        return doctor::SdkReadiness::not_ready(
            "go",
            anchor,
            TOOLCHAIN,
            "generated Go SDK is missing go.mod package metadata",
        );
    }
    if let Err(reason) = command_success_in(
        "go",
        &["test", "./..."],
        &materialized.target_dir,
        &[("GOPROXY", "off")],
    ) {
        return doctor::SdkReadiness::not_ready("go", anchor, TOOLCHAIN, reason);
    }
    if let Err(reason) = command_success_in(
        "go",
        &["vet", "./..."],
        &materialized.target_dir,
        &[("GOPROXY", "off")],
    ) {
        return doctor::SdkReadiness::not_ready("go", anchor, TOOLCHAIN, reason);
    }
    doctor::SdkReadiness::ready("go", anchor, TOOLCHAIN)
}

fn validate_python_target(anchor: &str, artifacts: &[gnr8::sdk::Artifact]) -> doctor::SdkReadiness {
    const TOOLCHAIN: &str = "python3 -m py_compile; import package";
    if let Err(reason) = command_available("python3", &["--version"]) {
        return doctor::SdkReadiness::not_ready("python", anchor, TOOLCHAIN, reason);
    }
    let Ok(materialized) = materialize_artifact_group(anchor, artifacts, "python") else {
        return doctor::SdkReadiness::not_ready(
            "python",
            anchor,
            TOOLCHAIN,
            "failed to materialize generated Python SDK for readiness",
        );
    };
    let package_dir = python_package_root(&materialized.target_dir, &materialized.root);
    let py_files = artifacts
        .iter()
        .filter(|artifact| path_extension_is(&artifact.path, "py"))
        .map(|artifact| materialized.root.join(&artifact.path))
        .collect::<Vec<_>>();
    if py_files.is_empty() {
        return doctor::SdkReadiness::not_ready(
            "python",
            anchor,
            TOOLCHAIN,
            "generated Python SDK contains no .py files",
        );
    }
    if let Err(reason) = python_compile(&py_files) {
        return doctor::SdkReadiness::not_ready("python", anchor, TOOLCHAIN, reason);
    }
    match python_import_package_result(&package_dir) {
        Ok(warnings) if warnings.is_empty() => doctor::SdkReadiness::ready(
            "python",
            package_dir_display(anchor, &package_dir, &materialized),
            TOOLCHAIN,
        ),
        Ok(warnings) => doctor::SdkReadiness::ready_with_warnings(
            "python",
            package_dir_display(anchor, &package_dir, &materialized),
            TOOLCHAIN,
            warnings,
        ),
        Err(reason) => doctor::SdkReadiness::not_ready("python", anchor, TOOLCHAIN, reason),
    }
}

fn package_dir_display(
    anchor: &str,
    package_dir: &Path,
    materialized: &MaterializedTarget,
) -> String {
    package_dir.strip_prefix(&materialized.root).map_or_else(
        |_| anchor.to_string(),
        |rel| rel.to_string_lossy().replace('\\', "/"),
    )
}

/// Prefer the package root (directory with `__init__.py` or `pyproject.toml`) over a nested anchor.
///
/// The walk is bounded by `root` — the materialized tree — so it can never escape into the ambient
/// filesystem and adopt an unrelated `__init__.py` as the package root.
fn python_package_root(target_dir: &Path, root: &Path) -> PathBuf {
    let is_package =
        |dir: &Path| dir.join("pyproject.toml").is_file() || dir.join("__init__.py").is_file();
    if is_package(target_dir) {
        return target_dir.to_path_buf();
    }
    let mut current = target_dir;
    while let Some(parent) = current.parent() {
        if !parent.starts_with(root) {
            break;
        }
        if is_package(parent) {
            return parent.to_path_buf();
        }
        current = parent;
    }
    target_dir.to_path_buf()
}

fn validate_typescript_target(
    anchor: &str,
    artifacts: &[gnr8::sdk::Artifact],
) -> doctor::SdkReadiness {
    const TOOLCHAIN: &str = "node + typescript --noEmit --strict";
    if let Err(reason) = command_available("node", &["--version"]) {
        return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
    }
    let Ok(project_root) = std::env::current_dir() else {
        return doctor::SdkReadiness::not_ready(
            "typescript",
            anchor,
            TOOLCHAIN,
            "failed to resolve the project directory",
        );
    };
    let Some(tsc) = typescript_compiler(&project_root, anchor) else {
        return doctor::SdkReadiness::not_ready(
            "typescript",
            anchor,
            TOOLCHAIN,
            "typescript compiler not found; install it in the project with `npm install --save-dev typescript` or provide `tsc` on PATH",
        );
    };
    let Ok(materialized) = materialize_artifact_group(anchor, artifacts, "typescript") else {
        return doctor::SdkReadiness::not_ready(
            "typescript",
            anchor,
            TOOLCHAIN,
            "failed to materialize generated TypeScript SDK for readiness",
        );
    };
    if let Err(reason) =
        link_typescript_node_modules(&project_root, anchor, &materialized.target_dir)
    {
        return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
    }
    let ts_files = artifacts
        .iter()
        .filter(|artifact| path_extension_is(&artifact.path, "ts"))
        .map(|artifact| materialized.root.join(&artifact.path))
        .collect::<Vec<_>>();
    if ts_files.is_empty() {
        return doctor::SdkReadiness::not_ready(
            "typescript",
            anchor,
            TOOLCHAIN,
            "generated TypeScript SDK contains no .ts files",
        );
    }
    if materialized.target_dir.join("package.json").is_file() {
        if !materialized.target_dir.join("tsconfig.json").is_file() {
            return doctor::SdkReadiness::not_ready(
                "typescript",
                anchor,
                TOOLCHAIN,
                "generated TypeScript package is missing tsconfig.json",
            );
        }
        // Prefer the package project build so doctor honors tsconfig paths/rootDir.
        if let Err(reason) = typescript_build(&tsc, &materialized.target_dir) {
            return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
        }
        if materialized.target_dir.join("tsconfig.esm.json").is_file() {
            if let Err(reason) =
                typescript_build_config(&tsc, &materialized.target_dir, "tsconfig.esm.json")
            {
                return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
            }
        }
        if let Err(reason) = validate_typescript_package_entrypoints(&materialized.target_dir) {
            return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
        }
        if command_available("npm", &["--version"]).is_ok() {
            if let Err(reason) = command_success_in(
                "npm",
                &["pack", "--dry-run", "--ignore-scripts"],
                &materialized.target_dir,
                &[],
            ) {
                return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
            }
        }
        return doctor::SdkReadiness::ready("typescript", anchor, TOOLCHAIN);
    }
    if let Err(reason) = typescript_typecheck(&tsc, &ts_files, &materialized.target_dir) {
        return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
    }
    doctor::SdkReadiness::ready("typescript", anchor, TOOLCHAIN)
}

struct MaterializedTarget {
    root: PathBuf,
    target_dir: PathBuf,
}

impl Drop for MaterializedTarget {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn materialize_artifact_group(
    anchor: &str,
    artifacts: &[gnr8::sdk::Artifact],
    label: &str,
) -> Result<MaterializedTarget, String> {
    let root = unique_doctor_temp_dir(label)?;
    let result = (|| {
        for artifact in artifacts {
            let path = safe_temp_artifact_path(&root, &artifact.path)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "failed to create readiness temp dir '{}': {err}",
                        parent.display()
                    )
                })?;
            }
            std::fs::write(&path, &artifact.text).map_err(|err| {
                format!(
                    "failed to write readiness temp file '{}': {err}",
                    path.display()
                )
            })?;
        }
        let target_dir = safe_temp_artifact_path(&root, anchor)?;
        Ok(MaterializedTarget {
            root: root.clone(),
            target_dir,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&root);
    }
    result
}

fn unique_doctor_temp_dir(label: &str) -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("system clock before Unix epoch: {err}"))?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "gnr8-doctor-readiness-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).map_err(|err| {
        format!(
            "failed to create readiness temp dir '{}': {err}",
            dir.display()
        )
    })?;
    Ok(dir)
}

fn safe_temp_artifact_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err(format!("artifact path {rel:?} must be project-relative"));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!(
            "artifact path {rel:?} must not escape the project root"
        ));
    }
    Ok(root.join(path))
}

fn command_available(program: &str, args: &[&str]) -> Result<(), String> {
    command_success_in(program, args, Path::new("."), &[])
}

fn command_success_in(
    program: &str,
    args: &[&str],
    cwd: &Path,
    envs: &[(&str, &str)],
) -> Result<(), String> {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .output()
        .map_err(|err| format!("failed to run `{}`: {err}", command_label(program, args)))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "`{}` failed: {}",
        command_label(program, args),
        command_output_excerpt(&output)
    ))
}

fn command_label(program: &str, args: &[&str]) -> String {
    if args.is_empty() {
        program.to_string()
    } else {
        format!("{program} {}", args.join(" "))
    }
}

fn command_output_excerpt(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    text_output_excerpt(&stderr, &stdout)
}

fn text_output_excerpt(stderr: &str, stdout: &str) -> String {
    const MAX_LINES: usize = 6;
    const HEAD_LINES: usize = 3;
    const TAIL_LINES: usize = 2;

    let lines = stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return "command exited non-zero without output".to_string();
    }
    if lines.len() <= MAX_LINES {
        return lines.join(" | ");
    }

    lines
        .iter()
        .take(HEAD_LINES)
        .copied()
        .chain(std::iter::once("…"))
        .chain(
            lines
                .iter()
                .skip(lines.len().saturating_sub(TAIL_LINES))
                .copied(),
        )
        .collect::<Vec<_>>()
        .join(" | ")
}

fn python_compile(files: &[PathBuf]) -> Result<(), String> {
    let args = std::iter::once("-m".to_string())
        .chain(std::iter::once("py_compile".to_string()))
        .chain(files.iter().map(|path| path.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    command_success_in("python3", &arg_refs, Path::new("."), &[])
}

fn python_import_package_result(package_dir: &Path) -> Result<Vec<String>, String> {
    let init = package_dir.join("__init__.py");
    if !init.is_file() {
        return Err("generated Python SDK is missing __init__.py".to_string());
    }
    let code = "\
import importlib.util
import sys
import warnings
init_path = sys.argv[1]
package_dir = sys.argv[2]
with warnings.catch_warnings(record=True) as caught:
    warnings.simplefilter('always')
    spec = importlib.util.spec_from_file_location(
        'gnr8_sdk_check',
        init_path,
        submodule_search_locations=[package_dir],
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    assert spec.loader is not None
    spec.loader.exec_module(module)
    for item in caught:
        print(f'WARNING:{item.category.__name__}:{item.message}', file=sys.stderr)
";
    let init_arg = init.to_string_lossy().into_owned();
    let dir_arg = package_dir.to_string_lossy().into_owned();
    let mut command = Command::new("python3");
    command
        .args(["-c", code, &init_arg, &dir_arg])
        .env("PYTHONWARNINGS", "default")
        .current_dir(package_dir.parent().unwrap_or_else(|| Path::new(".")));
    let output = command
        .output()
        .map_err(|err| format!("failed to import generated Python SDK: {err}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!(
            "importing generated Python SDK failed: {}",
            command_output_excerpt(&output)
        ));
    }
    let warnings = stderr
        .lines()
        .filter_map(|line| line.strip_prefix("WARNING:"))
        .map(|line| {
            if line.contains("allow_population_by_field_name") {
                "Pydantic emitted configuration deprecation warnings".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>();
    let mut unique = Vec::new();
    for warning in warnings {
        if !unique.contains(&warning) {
            unique.push(warning);
        }
    }
    Ok(unique)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeScriptCompiler {
    NodeScript(PathBuf),
    Executable(String),
}

fn typescript_compiler(project_root: &Path, anchor: &str) -> Option<TypeScriptCompiler> {
    let output_dir = safe_temp_artifact_path(project_root, anchor).ok()?;
    if let Some(path) = local_typescript_compiler(&output_dir) {
        return Some(TypeScriptCompiler::NodeScript(path));
    }
    if command_available("tsc", &["--version"]).is_ok() {
        return Some(TypeScriptCompiler::Executable("tsc".to_string()));
    }
    let development_sidecar = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tsextract")
        .join("node_modules")
        .join("typescript")
        .join("lib")
        .join("tsc.js");
    development_sidecar
        .is_file()
        .then_some(TypeScriptCompiler::NodeScript(development_sidecar))
}

fn link_typescript_node_modules(
    project_root: &Path,
    anchor: &str,
    materialized_target: &Path,
) -> Result<(), String> {
    let output_dir = safe_temp_artifact_path(project_root, anchor)?;
    let Some(source) = local_node_modules(&output_dir) else {
        return Ok(());
    };
    let destination = materialized_target.join("node_modules");
    if destination.exists() {
        return Ok(());
    }
    symlink_directory(&source, &destination).map_err(|err| {
        format!(
            "failed to make installed TypeScript dependencies available to readiness checks: {err}"
        )
    })
}

fn local_node_modules(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .map(|root| root.join("node_modules"))
        .find(|path| path.is_dir())
}

#[cfg(unix)]
fn symlink_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn symlink_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(source, destination)
}

fn local_typescript_compiler(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .map(|root| {
            root.join("node_modules")
                .join("typescript")
                .join("lib")
                .join("tsc.js")
        })
        .find(|path| path.is_file())
}

fn run_typescript_compiler(
    compiler: &TypeScriptCompiler,
    args: &[String],
    cwd: &Path,
) -> Result<(), String> {
    match compiler {
        TypeScriptCompiler::NodeScript(path) => {
            let mut node_args = vec![path.to_string_lossy().into_owned()];
            node_args.extend_from_slice(args);
            let arg_refs = node_args.iter().map(String::as_str).collect::<Vec<_>>();
            command_success_in("node", &arg_refs, cwd, &[])
        }
        TypeScriptCompiler::Executable(program) => {
            let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            command_success_in(program, &arg_refs, cwd, &[])
        }
    }
}

fn typescript_typecheck(
    compiler: &TypeScriptCompiler,
    files: &[PathBuf],
    cwd: &Path,
) -> Result<(), String> {
    let mut args = vec![
        "--noEmit".to_string(),
        "--strict".to_string(),
        "--lib".to_string(),
        "es2022,dom".to_string(),
    ];
    args.extend(files.iter().map(|path| path.to_string_lossy().into_owned()));
    run_typescript_compiler(compiler, &args, cwd)
}

fn typescript_build(compiler: &TypeScriptCompiler, cwd: &Path) -> Result<(), String> {
    typescript_build_config(compiler, cwd, "tsconfig.json")
}

fn typescript_build_config(
    compiler: &TypeScriptCompiler,
    cwd: &Path,
    config: &str,
) -> Result<(), String> {
    run_typescript_compiler(
        compiler,
        &["--project".to_string(), config.to_string()],
        cwd,
    )
}

fn validate_typescript_package_entrypoints(package_dir: &Path) -> Result<(), String> {
    let package_path = package_dir.join("package.json");
    let text = std::fs::read_to_string(&package_path)
        .map_err(|err| format!("failed to read '{}': {err}", package_path.display()))?;
    let package: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| format!("invalid generated package.json: {err}"))?;
    let main = package
        .get("main")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "generated package.json is missing string entrypoint main".to_string())?;
    let declarations = if let Some(types) = package.get("types") {
        (
            "types",
            types.as_str().ok_or_else(|| {
                "generated package.json entrypoint types must be a string".to_string()
            })?,
        )
    } else {
        let typings = package.get("typings").ok_or_else(|| {
            "generated package.json is missing string entrypoint types or typings".to_string()
        })?;
        (
            "typings",
            typings.as_str().ok_or_else(|| {
                "generated package.json entrypoint typings must be a string".to_string()
            })?,
        )
    };
    let mut entrypoints = vec![
        ("main".to_string(), main),
        (declarations.0.to_string(), declarations.1),
    ];
    if let Some(module) = package.get("module") {
        entrypoints.push((
            "module".to_string(),
            module.as_str().ok_or_else(|| {
                "generated package.json entrypoint module must be a string".to_string()
            })?,
        ));
    }
    if let Some(exports_root) = package.get("exports") {
        let (exports, label_prefix) = match exports_root {
            serde_json::Value::Object(root) if root.contains_key(".") => (&root["."], "exports[.]"),
            serde_json::Value::Object(_) | serde_json::Value::String(_) => {
                (exports_root, "exports")
            }
            _ => {
                return Err(
                    "generated package.json entrypoint exports must be a string or object"
                        .to_string(),
                );
            }
        };
        if let Some(relative) = exports.as_str() {
            entrypoints.push((label_prefix.to_string(), relative));
        } else if let Some(exports) = exports.as_object() {
            let mut recognized = false;
            for key in ["types", "import", "require", "default"] {
                if let Some(value) = exports.get(key) {
                    let label = format!("{label_prefix}.{key}");
                    let relative = value.as_str().ok_or_else(|| {
                        format!("generated package.json entrypoint {label} must be a string")
                    })?;
                    entrypoints.push((label, relative));
                    recognized = true;
                }
            }
            if !recognized {
                return Err(format!(
                    "generated package.json {label_prefix} has no supported string entrypoints"
                ));
            }
        } else {
            return Err(format!(
                "generated package.json entrypoint {label_prefix} must be a string or object"
            ));
        }
    }

    for (label, relative) in entrypoints {
        let relative = Path::new(relative.strip_prefix("./").unwrap_or(relative));
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!(
                "generated package.json entrypoint {label} is not a safe relative path: {}",
                relative.display()
            ));
        }
        if !package_dir.join(relative).is_file() {
            return Err(format!(
                "generated package.json entrypoint {label} does not exist after build: {}",
                relative.display()
            ));
        }
    }
    Ok(())
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
fn run_doctor(output: Output) -> Result<()> {
    let root = project_root()?;
    let initialized = gnr8::workspace::manifest_path(&root).is_file();
    let initial_source_probe = probe_source_lang_toolchain(&root);

    // Run the pipeline once. Its `Err` IS the "pipeline broken" finding (do NOT `?`); on success we get
    // the child's diagnostics and can compute drift from its artifacts. Both degrade gracefully.
    let total_start = Instant::now();
    let (mut bundle, mut pipeline_error) = if initialized {
        output.progress("doctor: running pipeline");
        match child::run_child(&root, "__emit") {
            Ok(bundle) => (Some(bundle), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        }
    } else {
        (None, None)
    };
    let pipeline_ran = bundle.is_some();
    let cache_input_roots = bundle
        .as_ref()
        .map(|bundle| bundle.cache_input_roots.clone())
        .unwrap_or_default();
    let (language, source_present) = reconcile_doctor_source_probe(
        &root,
        initial_source_probe,
        pipeline_ran,
        &cache_input_roots,
    );
    let diagnostics = bundle.as_ref().map(|b| b.diagnostics.clone());
    let output_anchors = bundle
        .as_ref()
        .map(|bundle| bundle.output_anchors.clone())
        .unwrap_or_default();
    let sdk_readiness = bundle
        .as_mut()
        .map(|bundle| collect_sdk_readiness(&root, bundle))
        .unwrap_or_default();
    let drift = match bundle.as_mut() {
        Some(bundle) => match plan_bundle(&root, bundle) {
            Ok(plan) => Some(plan),
            Err(error) => {
                pipeline_error = Some(format!("output drift planning failed: {error:#}"));
                None
            }
        },
        None => None,
    };

    let report = doctor::DoctorReport::assemble(
        initialized,
        source_present,
        &language,
        pipeline_ran,
        diagnostics,
        drift.as_ref(),
    )
    .with_pipeline_error(pipeline_error)
    .with_sdk_readiness(sdk_readiness)
    .with_runtime(
        doctor::DoctorRuntime {
            binary_path: std::env::current_exe()
                .ok()
                .map(|path| path.to_string_lossy().into_owned()),
            resource_dir: Some(
                gnr8::resource::resource_dir()?
                    .to_string_lossy()
                    .into_owned(),
            ),
            output_anchors,
        },
        doctor::DoctorTimings {
            total: duration_ms(total_start.elapsed()),
        },
    );

    if output.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", report.render_human());
        output.verbose(format!("total: {}", fmt_duration(total_start.elapsed())));
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
fn run_watch(debounce_ms: u64, output: Output) -> Result<()> {
    // Floor the debounce window at a small minimum (IN-04): `--debounce-ms 0` would create a
    // zero-window debouncer that defeats burst-coalescing and amplifies the delete/rename edge case.
    const MIN_DEBOUNCE_MS: u64 = 10;

    let root = project_root()?;

    if !output.json {
        output.progress(format!(
            "watch: {} (sources + .gnr8/src, Ctrl-C to stop)",
            root.display()
        ));
    }

    // The COLD scenario: an initial regeneration ensures outputs are current and measures cold latency.
    watch::cold_regenerate(&root, output.json, output.verbose)?;

    let debounce = std::time::Duration::from_millis(debounce_ms.max(MIN_DEBOUNCE_MS));
    watch::run(&root, debounce, output.json, output.verbose)
}

/// Build the API graph for an `inspect` subcommand, render it (table or `--json`), and print it.
///
/// With no path, inspect uses the same child `__inspect` pipeline as generation so source package
/// filters, transforms, and resource/toolchain resolution match `generate`/`check`. An explicit path
/// requests direct source inspection.
fn run_inspect(action: &InspectAction, output: Output) -> Result<()> {
    let total_start = Instant::now();
    let rendered = match action {
        InspectAction::Routes { path } => {
            let graph = inspect_graph(path.as_deref(), output)?;
            render::render_routes(&graph, output.json)?
        }
        InspectAction::Schemas { path } => {
            let graph = inspect_graph(path.as_deref(), output)?;
            render::render_schemas(&graph, output.json)?
        }
        InspectAction::Graph { path } => {
            let graph = inspect_graph(path.as_deref(), output)?;
            render::render_graph(&graph, output.json)?
        }
    };
    print!("{rendered}");
    output.verbose(format!("total: {}", fmt_duration(total_start.elapsed())));
    Ok(())
}

fn inspect_graph(path: Option<&str>, output: Output) -> Result<gnr8::graph::ApiGraph> {
    if let Some(path) = path {
        output.verbose(format!("inspect: analyzing source path directly: {path}"));
        return Ok(gnr8::analyze::build_graph(path)?);
    }

    let root = project_root()?;
    if gnr8::workspace::manifest_path(&root).is_file() {
        output.verbose(format!(
            "inspect: using .gnr8 pipeline at {}",
            root.display()
        ));
        return Ok(child::inspect_child(&root)?);
    }
    bail!("no .gnr8 pipeline found; run `gnr8 init` or pass a source path to `gnr8 inspect`")
}

fn lifecycle_summary(outcome: &gnr8::lifecycle::GenerateOutcome) -> String {
    format!(
        "{} written, {} unchanged, {} deleted, {} skipped",
        outcome.written.len(),
        outcome.unchanged.len(),
        outcome.deleted.len(),
        outcome.skipped.len()
    )
}

fn print_diagnostics(output: Output, diagnostics: &[gnr8::graph::Diagnostic]) {
    if diagnostics.is_empty() || output.json {
        return;
    }
    if output.verbose == 0 {
        eprintln!("{}", diagnostic_summary(&diagnostic_counts(diagnostics)));
        return;
    }
    for diag in diagnostics {
        eprintln!(
            "{} [{}]: {} ({}:{})",
            diag.severity, diag.code, diag.message, diag.file, diag.line
        );
    }
}

/// The one-line diagnostic summary printed without `-v`.
///
/// Error-severity diagnostics get their own count and their own verdict. A run that reports
/// hundreds of them is not a run with "some diagnostics" — extraction did not describe the API,
/// and the reader has to be told so by the line they actually see. Pure, so the wording is
/// testable without running a pipeline.
fn diagnostic_summary(counts: &DiagnosticCounts) -> String {
    let total = counts.total;
    if counts.error > 0 {
        return format!(
            "error: {total} pipeline diagnostics, {} of them ERROR — extraction is incomplete; \
             run with -v for the list, or `gnr8 doctor` to fail on them",
            counts.error
        );
    }
    if counts.warn > 0 {
        return format!(
            "warning: {total} pipeline diagnostics, {} of them WARN (run with -v for details)",
            counts.warn
        );
    }
    format!("info: {total} pipeline diagnostics (run with -v for details)")
}

fn diagnostic_counts(diagnostics: &[gnr8::graph::Diagnostic]) -> DiagnosticCounts {
    let info = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity.eq_ignore_ascii_case("INFO"))
        .count();
    let warn = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity.eq_ignore_ascii_case("WARN"))
        .count();
    let error = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity.eq_ignore_ascii_case("ERROR"))
        .count();
    DiagnosticCounts {
        total: diagnostics.len(),
        info,
        warn,
        error,
    }
}

fn duration_ms(duration: Duration) -> u128 {
    duration.as_millis()
}

fn fmt_duration(duration: Duration) -> String {
    let millis = duration.as_secs_f64() * 1000.0;
    if millis < 10.0 {
        format!("{millis:.1} ms")
    } else {
        format!("{millis:.0} ms")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{
        content_stamps_from_fast, diagnostic_counts, diagnostic_summary,
        link_typescript_node_modules, local_node_modules, local_typescript_compiler,
        pre_child_verified_noop, readiness_for_target, reconcile_doctor_source_probe,
        text_output_excerpt, typescript_compiler, validate_typescript_package_entrypoints,
        FastFileStamp, MaterializedTarget, TypeScriptCompiler, VerifiedNoopStamp,
    };
    use gnr8::graph::{Diagnostic, DiagnosticCategory, SourceSpan};
    use gnr8::sdk::{Artifact, ReadinessKind, ReadinessTarget};
    use std::path::PathBuf;

    fn temp_root(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("gnr8-doctor-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn cache_snapshots(
        root: &std::path::Path,
    ) -> (Vec<gnr8::sdk::FileStamp>, Vec<gnr8::sdk::FileStamp>) {
        let config_fast = super::collect_required_config_fast_stamps(root).unwrap();
        let config = super::content_stamps_from_fast(root, &config_fast).unwrap();
        let tool = root.join(".gnr8/test-generation-tool");
        std::fs::write(&tool, "tool-v1").unwrap();
        let tools = gnr8::sdk::stamp_project_paths(root, std::slice::from_ref(&tool)).unwrap();
        (config, tools)
    }

    #[test]
    fn old_verified_noop_stamp_cannot_bypass_missing_ownership() {
        let root = temp_root("orphaned-noop-stamp");
        let cache = root.join(".gnr8/cache");
        std::fs::create_dir_all(&cache).unwrap();
        let stamp = VerifiedNoopStamp {
            cli_version: String::new(),
            core_version: String::new(),
            capability_fingerprint: String::new(),
            artifact_cache_key: "old-key".to_string(),
            output_anchors: vec!["openapi.yaml".to_string()],
            artifact_paths: vec!["openapi.yaml".to_string()],
            input_roots: vec!["src".to_string()],
            input_fast_files: Vec::new(),
            output_artifact_fast_files: Vec::new(),
            output_dir_fast_stamps: Vec::new(),
            input_files: Vec::new(),
            source_files: Vec::new(),
            config_files: Vec::new(),
            tool_files: Vec::new(),
            pipeline_files: Vec::new(),
            pipeline_roots: Vec::new(),
            output_files: Vec::new(),
            diagnostics: Vec::new(),
        };
        std::fs::write(
            cache.join("verified-noop.json"),
            serde_json::to_vec(&stamp).unwrap(),
        )
        .unwrap();

        assert!(pre_child_verified_noop(&root).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn content_stamps_detect_same_length_file_edits() {
        let root = temp_root("content-stamp");
        let path = root.join("source.go");
        std::fs::write(&path, "AAAA").unwrap();
        let metadata = path.metadata().unwrap();
        let fast = vec![FastFileStamp {
            path: "source.go".to_string(),
            len: metadata.len(),
            modified_ns: super::fast_modified_ns(&metadata),
        }];
        let before = content_stamps_from_fast(&root, &fast).unwrap();

        std::fs::write(&path, "BBBB").unwrap();
        let after = content_stamps_from_fast(&root, &fast).unwrap();

        assert_ne!(before[0].hash, after[0].hash);
        assert_eq!(before[0].len, after[0].len);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_noop_temp_cleanup_removes_only_exact_private_names() {
        let root = temp_root("verified-noop-temp-cleanup");
        let cache = root.join(".gnr8/cache");
        std::fs::create_dir_all(&cache).unwrap();
        let stale = cache.join(".verified-noop-123-4.tmp");
        let unrelated = cache.join(".verified-noop-not-ours.tmp");
        std::fs::write(&stale, b"stale").unwrap();
        std::fs::write(&unrelated, b"keep").unwrap();

        super::cleanup_verified_noop_temporary_files(&root).unwrap();

        assert!(!stale.exists());
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_noop_publication_atomically_replaces_an_existing_stamp() {
        let root = temp_root("verified-noop-replace");
        let stamp = root.join(".gnr8/cache/verified-noop.json");
        std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();
        std::fs::write(&stamp, b"old").unwrap();

        super::publish_verified_noop_stamp(&stamp, b"new").unwrap();

        assert_eq!(std::fs::read(&stamp).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hot_noop_config_scan_fails_closed_when_required_sources_are_missing() {
        let root = temp_root("missing-config-source");
        gnr8::workspace::init(&root).unwrap();
        std::fs::remove_dir_all(root.join(".gnr8/src")).unwrap();

        assert!(super::collect_required_config_fast_stamps(&root).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_noop_stamp_is_not_published_for_a_different_source_snapshot() {
        let root = temp_root("source-snapshot-changed");
        gnr8::workspace::init(&root).unwrap();
        std::fs::create_dir_all(root.join("service")).unwrap();
        let input = root.join("service/input.go");
        std::fs::write(&input, "version-a").unwrap();
        let child_stamps =
            gnr8::sdk::stamp_project_paths(&root, std::slice::from_ref(&input)).unwrap();
        let (config_stamps, tool_stamps) = cache_snapshots(&root);
        std::fs::create_dir_all(root.join("generated")).unwrap();
        let output = root.join("generated/client.go");
        std::fs::write(&output, "generated").unwrap();
        let output_hash = gnr8::manifest::blake3_hex(b"generated");
        let mut manifest = gnr8::manifest::Manifest::default();
        manifest.record("generated/client.go", &output_hash, "generated");
        manifest.save(&root.join(".gnr8")).unwrap();
        let bundle = gnr8::runner::ArtifactBundle {
            protocol_version: gnr8::runner::PROTOCOL_VERSION,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            core_version: env!("CARGO_PKG_VERSION").to_string(),
            capability_fingerprint: String::new(),
            artifacts: vec![Artifact::new("generated/client.go", "generated")],
            diagnostics: Vec::new(),
            output_anchors: vec!["generated".to_string()],
            readiness_targets: Vec::new(),
            artifact_cache_key: Some("snapshot-key".to_string()),
            cache_input_roots: vec!["service".to_string()],
            cache_input_stamps: child_stamps,
            cache_config_stamps: config_stamps,
            cache_config_complete: true,
            cache_pipeline_stamps: Vec::new(),
            cache_pipeline_roots: Vec::new(),
            cache_pipeline_complete: true,
            cache_tool_stamps: tool_stamps,
        };
        let outcome = gnr8::lifecycle::GenerateOutcome {
            written: Vec::new(),
            unchanged: vec!["generated/client.go".to_string()],
            skipped: Vec::new(),
            deleted: Vec::new(),
        };

        std::fs::write(&input, "version-b").unwrap();
        super::save_verified_noop_stamp_from_artifacts(&root, &bundle, &outcome);

        assert!(!super::verified_noop_stamp_path(&root).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn arbitrary_runtime_inputs_keep_the_pre_child_noop_disabled() {
        let root = temp_root("static-input-invalidates-noop");
        gnr8::workspace::init(&root).unwrap();
        std::fs::create_dir_all(root.join("service")).unwrap();
        let source = root.join("service/input.go");
        std::fs::write(&source, "stable-source").unwrap();
        let source_stamps =
            gnr8::sdk::stamp_project_paths(&root, std::slice::from_ref(&source)).unwrap();
        let (config_stamps, tool_stamps) = cache_snapshots(&root);
        std::fs::create_dir_all(root.join("assets")).unwrap();
        let static_input = root.join("assets/template.txt");
        std::fs::write(&static_input, "version-a").unwrap();
        let pipeline_stamps =
            gnr8::sdk::stamp_project_paths(&root, std::slice::from_ref(&static_input)).unwrap();
        std::fs::create_dir_all(root.join("generated")).unwrap();
        std::fs::write(root.join("generated/template.txt"), "version-a").unwrap();
        let output_hash = gnr8::manifest::blake3_hex(b"version-a");
        let mut manifest = gnr8::manifest::Manifest::default();
        manifest.record("generated/template.txt", &output_hash, "generated");
        manifest.save(&root.join(".gnr8")).unwrap();
        let bundle = gnr8::runner::ArtifactBundle {
            protocol_version: gnr8::runner::PROTOCOL_VERSION,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            core_version: env!("CARGO_PKG_VERSION").to_string(),
            capability_fingerprint: gnr8::runner::capability_fingerprint(),
            artifacts: vec![Artifact::new("generated/template.txt", "version-a")],
            diagnostics: Vec::new(),
            output_anchors: vec!["generated".to_string()],
            readiness_targets: Vec::new(),
            artifact_cache_key: Some("static-input-key".to_string()),
            cache_input_roots: vec!["service".to_string()],
            cache_input_stamps: source_stamps,
            cache_config_stamps: config_stamps,
            cache_config_complete: true,
            cache_pipeline_stamps: pipeline_stamps,
            cache_pipeline_roots: vec!["assets".to_string()],
            cache_pipeline_complete: true,
            cache_tool_stamps: tool_stamps,
        };
        let outcome = gnr8::lifecycle::GenerateOutcome {
            written: Vec::new(),
            unchanged: vec!["generated/template.txt".to_string()],
            skipped: Vec::new(),
            deleted: Vec::new(),
        };

        super::save_verified_noop_stamp_from_artifacts(&root, &bundle, &outcome);
        assert!(super::pre_child_verified_noop(&root).is_none());

        std::fs::write(root.join("assets/added.txt"), "new-static-input").unwrap();
        assert!(super::pre_child_verified_noop(&root).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pure_file_backed_pipeline_still_runs_child_without_whole_config_contract() {
        let root = temp_root("empty-pipeline-inputs-noop");
        gnr8::workspace::init(&root).unwrap();
        std::fs::create_dir_all(root.join("service")).unwrap();
        let source = root.join("service/input.yaml");
        std::fs::write(&source, "openapi: 3.1.0\n").unwrap();
        let source_stamps =
            gnr8::sdk::stamp_project_paths(&root, std::slice::from_ref(&source)).unwrap();
        let (config_stamps, tool_stamps) = cache_snapshots(&root);
        std::fs::create_dir_all(root.join("generated")).unwrap();
        std::fs::write(root.join("generated/openapi.yaml"), "openapi: 3.1.0\n").unwrap();
        let output_hash = gnr8::manifest::blake3_hex(b"openapi: 3.1.0\n");
        let mut manifest = gnr8::manifest::Manifest::default();
        manifest.record("generated/openapi.yaml", &output_hash, "generated");
        manifest.save(&root.join(".gnr8")).unwrap();
        let bundle = gnr8::runner::ArtifactBundle {
            protocol_version: gnr8::runner::PROTOCOL_VERSION,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            core_version: env!("CARGO_PKG_VERSION").to_string(),
            capability_fingerprint: gnr8::runner::capability_fingerprint(),
            artifacts: vec![Artifact::new("generated/openapi.yaml", "openapi: 3.1.0\n")],
            diagnostics: Vec::new(),
            output_anchors: vec!["generated".to_string()],
            readiness_targets: Vec::new(),
            artifact_cache_key: Some("empty-pipeline-key".to_string()),
            cache_input_roots: vec!["service".to_string()],
            cache_input_stamps: source_stamps,
            cache_config_stamps: config_stamps,
            cache_config_complete: true,
            cache_pipeline_stamps: Vec::new(),
            cache_pipeline_roots: Vec::new(),
            cache_pipeline_complete: true,
            cache_tool_stamps: tool_stamps,
        };
        let outcome = gnr8::lifecycle::GenerateOutcome {
            written: Vec::new(),
            unchanged: vec!["generated/openapi.yaml".to_string()],
            skipped: Vec::new(),
            deleted: Vec::new(),
        };

        super::save_verified_noop_stamp_from_artifacts(&root, &bundle, &outcome);
        assert!(super::pre_child_verified_noop(&root).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_noop_stamp_is_not_published_for_a_different_output_snapshot() {
        let root = temp_root("output-snapshot-changed");
        gnr8::workspace::init(&root).unwrap();
        std::fs::create_dir_all(root.join("service")).unwrap();
        let input = root.join("service/input.go");
        std::fs::write(&input, "stable-source").unwrap();
        let child_stamps =
            gnr8::sdk::stamp_project_paths(&root, std::slice::from_ref(&input)).unwrap();
        let (config_stamps, tool_stamps) = cache_snapshots(&root);
        std::fs::create_dir_all(root.join("generated")).unwrap();
        std::fs::write(root.join("generated/client.go"), "newer-output").unwrap();
        let newer_hash = gnr8::manifest::blake3_hex(b"newer-output");
        let mut manifest = gnr8::manifest::Manifest::default();
        manifest.record("generated/client.go", &newer_hash, "generated");
        manifest.save(&root.join(".gnr8")).unwrap();
        let bundle = gnr8::runner::ArtifactBundle {
            protocol_version: gnr8::runner::PROTOCOL_VERSION,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            core_version: env!("CARGO_PKG_VERSION").to_string(),
            capability_fingerprint: String::new(),
            artifacts: vec![Artifact::new("generated/client.go", "older-output")],
            diagnostics: Vec::new(),
            output_anchors: vec!["generated".to_string()],
            readiness_targets: Vec::new(),
            artifact_cache_key: Some("snapshot-key".to_string()),
            cache_input_roots: vec!["service".to_string()],
            cache_input_stamps: child_stamps,
            cache_config_stamps: config_stamps,
            cache_config_complete: true,
            cache_pipeline_stamps: Vec::new(),
            cache_pipeline_roots: Vec::new(),
            cache_pipeline_complete: true,
            cache_tool_stamps: tool_stamps,
        };
        let outcome = gnr8::lifecycle::GenerateOutcome {
            written: Vec::new(),
            unchanged: vec!["generated/client.go".to_string()],
            skipped: Vec::new(),
            deleted: Vec::new(),
        };

        super::save_verified_noop_stamp_from_artifacts(&root, &bundle, &outcome);

        assert!(!super::verified_noop_stamp_path(&root).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_noop_stamp_is_not_published_for_a_different_config_snapshot() {
        let root = temp_root("config-snapshot-changed");
        gnr8::workspace::init(&root).unwrap();
        std::fs::create_dir_all(root.join("service")).unwrap();
        let input = root.join("service/input.go");
        std::fs::write(&input, "stable-source").unwrap();
        let child_stamps =
            gnr8::sdk::stamp_project_paths(&root, std::slice::from_ref(&input)).unwrap();
        let (config_stamps, tool_stamps) = cache_snapshots(&root);
        std::fs::create_dir_all(root.join("generated")).unwrap();
        std::fs::write(root.join("generated/client.go"), "generated").unwrap();
        let output_hash = gnr8::manifest::blake3_hex(b"generated");
        let mut manifest = gnr8::manifest::Manifest::default();
        manifest.record("generated/client.go", &output_hash, "generated");
        manifest.save(&root.join(".gnr8")).unwrap();
        let bundle = gnr8::runner::ArtifactBundle {
            protocol_version: gnr8::runner::PROTOCOL_VERSION,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            core_version: env!("CARGO_PKG_VERSION").to_string(),
            capability_fingerprint: gnr8::runner::capability_fingerprint(),
            artifacts: vec![Artifact::new("generated/client.go", "generated")],
            diagnostics: Vec::new(),
            output_anchors: vec!["generated".to_string()],
            readiness_targets: Vec::new(),
            artifact_cache_key: Some("snapshot-key".to_string()),
            cache_input_roots: vec!["service".to_string()],
            cache_input_stamps: child_stamps,
            cache_config_stamps: config_stamps,
            cache_config_complete: true,
            cache_pipeline_stamps: Vec::new(),
            cache_pipeline_roots: Vec::new(),
            cache_pipeline_complete: true,
            cache_tool_stamps: tool_stamps,
        };
        let outcome = gnr8::lifecycle::GenerateOutcome {
            written: Vec::new(),
            unchanged: vec!["generated/client.go".to_string()],
            skipped: Vec::new(),
            deleted: Vec::new(),
        };

        std::fs::write(root.join(".gnr8/src/main.rs"), "fn main() {}\n").unwrap();
        super::save_verified_noop_stamp_from_artifacts(&root, &bundle, &outcome);

        assert!(!super::verified_noop_stamp_path(&root).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn doctor_source_probe_uses_pipeline_input_roots_when_pipeline_runs() {
        let root = temp_root("input-roots");
        let src = root.join("service");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("app.py"), "def app():\n    pass\n").unwrap();

        let (language, present) = reconcile_doctor_source_probe(
            &root,
            ("unknown".to_string(), false),
            true,
            &["service".to_string()],
        );

        assert_eq!(language, "python");
        assert!(present);
    }

    #[test]
    fn doctor_source_probe_treats_successful_pipeline_as_configured_source() {
        let root = temp_root("configured");
        let (language, present) =
            reconcile_doctor_source_probe(&root, ("unknown".to_string(), false), true, &[]);

        assert_eq!(language, "configured");
        assert!(present);
    }

    /// The line a reader actually sees without `-v` must say when extraction failed.
    ///
    /// It used to say "warning: 240 pipeline diagnostics" whether those were 240 lossy-pattern
    /// notes or 240 packages that did not load, so a run that described nothing read exactly like
    /// a healthy one. Issue #67 took a manual diff of the committed output to discover which it
    /// had been.
    #[test]
    fn the_summary_line_calls_out_error_diagnostics() {
        let summary = diagnostic_summary(&super::DiagnosticCounts {
            total: 240,
            info: 0,
            warn: 0,
            error: 240,
        });
        assert!(summary.starts_with("error: "), "{summary}");
        assert!(summary.contains("240 pipeline diagnostics"), "{summary}");
        assert!(summary.contains("240 of them ERROR"), "{summary}");
        assert!(summary.contains("extraction is incomplete"), "{summary}");
        assert!(summary.contains("gnr8 doctor"), "{summary}");
    }

    /// An error among warnings still sets the verdict: one package that did not load makes the
    /// whole extraction incomplete, however many ordinary notes accompany it.
    #[test]
    fn one_error_among_warnings_sets_the_verdict() {
        let summary = diagnostic_summary(&super::DiagnosticCounts {
            total: 12,
            info: 3,
            warn: 8,
            error: 1,
        });
        assert!(summary.starts_with("error: "), "{summary}");
        assert!(summary.contains("1 of them ERROR"), "{summary}");
    }

    /// Warnings and notes keep their own, quieter verdicts — an INFO-only run is not a warning.
    #[test]
    fn warnings_and_notes_keep_their_own_verdict() {
        let warned = diagnostic_summary(&super::DiagnosticCounts {
            total: 4,
            info: 1,
            warn: 3,
            error: 0,
        });
        assert!(warned.starts_with("warning: "), "{warned}");
        assert!(warned.contains("3 of them WARN"), "{warned}");

        let noted = diagnostic_summary(&super::DiagnosticCounts {
            total: 2,
            info: 2,
            warn: 0,
            error: 0,
        });
        assert!(noted.starts_with("info: "), "{noted}");
    }

    #[test]
    fn diagnostic_counts_report_all_supported_severities() {
        let diagnostic = |severity| {
            Diagnostic::new(
                "source.test",
                DiagnosticCategory::Source,
                severity,
                "test",
                SourceSpan {
                    file: "src/service.go".to_string(),
                    start_line: 1,
                    end_line: 1,
                },
            )
        };
        let counts =
            diagnostic_counts(&[diagnostic("INFO"), diagnostic("WARN"), diagnostic("ERROR")]);

        assert_eq!(
            (counts.total, counts.info, counts.warn, counts.error),
            (3, 1, 1, 1)
        );
    }

    #[test]
    fn declared_openapi_readiness_requires_the_exact_artifact() {
        let readiness = readiness_for_target(
            &ReadinessTarget::new(ReadinessKind::OpenApi, "generated/openapi.yaml"),
            &[Artifact::new(
                "generated/other.yaml",
                "openapi: 3.1.0\ninfo:\n  title: Other\n  version: 1.0.0\npaths: {}\n",
            )],
        );

        assert_eq!(readiness.status, "not_ready");
        assert!(readiness.reason.contains("did not emit its artifact"));
    }

    #[test]
    fn typescript_compiler_resolves_from_project_node_modules() {
        let root = temp_root("project-tsc");
        let compiler = root.join("node_modules/typescript/lib/tsc.js");
        std::fs::create_dir_all(compiler.parent().unwrap()).unwrap();
        std::fs::write(&compiler, "// test compiler").unwrap();
        let nested = root.join("packages/sdk");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(local_typescript_compiler(&nested), Some(compiler));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typescript_compiler_prefers_the_generated_package_install() {
        let root = temp_root("output-tsc");
        let compiler = root.join("generated/sdk/node_modules/typescript/lib/tsc.js");
        std::fs::create_dir_all(compiler.parent().unwrap()).unwrap();
        std::fs::write(&compiler, "// test compiler").unwrap();

        assert_eq!(
            typescript_compiler(&root, "generated/sdk"),
            Some(TypeScriptCompiler::NodeScript(compiler))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typescript_readiness_reuses_installed_package_dependencies() {
        let root = temp_root("output-dependencies");
        let dependency = root.join("node_modules/example-package/index.d.ts");
        std::fs::create_dir_all(dependency.parent().unwrap()).unwrap();
        std::fs::write(&dependency, "export {};\n").unwrap();
        let materialized = root.join("materialized");
        std::fs::create_dir_all(&materialized).unwrap();

        assert_eq!(
            local_node_modules(&root.join("generated/sdk")),
            Some(root.join("node_modules"))
        );
        link_typescript_node_modules(&root, "generated/sdk", &materialized).unwrap();
        assert!(materialized
            .join("node_modules/example-package/index.d.ts")
            .is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typescript_package_entrypoints_must_exist_after_build() {
        let root = temp_root("package-entrypoints");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/index.js"), "exports.answer = 42;\n").unwrap();
        std::fs::write(
            root.join("dist/index.d.ts"),
            "export declare const answer: number;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "exports": {
    ".": {
      "types": "./dist/index.d.ts",
      "import": "./dist/index.js",
      "require": "./dist/index.js",
      "default": "./dist/index.js"
    }
  }
}"#,
        )
        .unwrap();

        assert!(validate_typescript_package_entrypoints(&root).is_ok());
        std::fs::remove_file(root.join("dist/index.js")).unwrap();
        let err = validate_typescript_package_entrypoints(&root).unwrap_err();
        assert!(err.contains("does not exist after build"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typescript_package_accepts_typings_without_exports() {
        let root = temp_root("package-typings");
        std::fs::create_dir_all(root.join("dist/esm")).unwrap();
        std::fs::write(root.join("dist/index.js"), "exports.answer = 42;\n").unwrap();
        std::fs::write(
            root.join("dist/esm/index.js"),
            "export const answer = 42;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("dist/index.d.ts"),
            "export declare const answer: number;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{
  "main": "./dist/index.js",
  "typings": "./dist/index.d.ts",
  "module": "./dist/esm/index.js"
}"#,
        )
        .unwrap();

        assert!(validate_typescript_package_entrypoints(&root).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typescript_package_rejects_non_string_optional_entrypoint() {
        let root = temp_root("package-invalid-module");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/index.js"), "exports.answer = 42;\n").unwrap();
        std::fs::write(
            root.join("dist/index.d.ts"),
            "export declare const answer: number;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "module": false
}"#,
        )
        .unwrap();

        let err = validate_typescript_package_entrypoints(&root).unwrap_err();

        assert!(err.contains("module must be a string"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typescript_package_rejects_unsupported_exports_shape() {
        let root = temp_root("package-invalid-exports");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/index.js"), "exports.answer = 42;\n").unwrap();
        std::fs::write(
            root.join("dist/index.d.ts"),
            "export declare const answer: number;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "exports": {
    ".": {
      "browser": "./dist/index.js"
    }
  }
}"#,
        )
        .unwrap();

        let err = validate_typescript_package_entrypoints(&root).unwrap_err();

        assert!(err.contains("no supported string entrypoints"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typescript_package_rejects_invalid_top_level_exports() {
        let root = temp_root("package-invalid-root-exports");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/index.js"), "exports.answer = 42;\n").unwrap();
        std::fs::write(
            root.join("dist/index.d.ts"),
            "export declare const answer: number;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "exports": false
}"#,
        )
        .unwrap();

        let err = validate_typescript_package_entrypoints(&root).unwrap_err();

        assert!(err.contains("exports must be a string or object"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn materialized_readiness_target_cleans_its_temp_tree() {
        let root = temp_root("materialized-cleanup");
        let target_dir = root.join("sdk");
        std::fs::create_dir_all(&target_dir).unwrap();
        drop(MaterializedTarget {
            root: root.clone(),
            target_dir,
        });

        assert!(!root.exists());
    }

    #[test]
    fn command_output_excerpt_keeps_the_actionable_end_of_a_traceback() {
        let stderr = "\
Traceback (most recent call last):
  File \"<string>\", line 1, in <module>
  File \"generated/sdk/models.py\", line 5, in <module>
    from pydantic import BaseModel
ModuleNotFoundError: No module named 'pydantic'
";

        assert_eq!(
            text_output_excerpt(stderr, ""),
            "Traceback (most recent call last): | File \"<string>\", line 1, in <module> | \
File \"generated/sdk/models.py\", line 5, in <module> | from pydantic import BaseModel | \
ModuleNotFoundError: No module named 'pydantic'"
        );
    }

    #[test]
    fn command_output_excerpt_bounds_long_subprocess_output() {
        let stderr = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n";

        assert_eq!(
            text_output_excerpt(stderr, ""),
            "one | two | three | … | seven | eight"
        );
    }
}
