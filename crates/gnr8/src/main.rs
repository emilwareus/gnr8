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

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Commands, CompatAction, GuideTopic, InspectAction, SdkPreset, SourcePreset};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, UNIX_EPOCH};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let output = Output::new(cli.json, cli.verbose);

    // `inspect` renders straight to stdout. In initialized projects it delegates to the user's `.gnr8/`
    // child pipeline, while uninitialized/direct use still analyzes the requested source path.
    // The remaining commands either scaffold (`init`) or delegate to the user's `.gnr8/` child crate and
    // own writing/policy.
    match &cli.command {
        Commands::Inspect { action } => run_inspect(action, output),
        Commands::Init { source, sdk } => run_init(*source, *sdk, output),
        Commands::Guide { topic } => run_guide(*topic, output),
        Commands::Generate {
            force,
            accept_generated_baseline,
        } => run_generate(*force, *accept_generated_baseline, output),
        Commands::Check => run_check(output),
        Commands::Watch { debounce_ms } => run_watch(*debounce_ms, output),
        Commands::Doctor => run_doctor(output),
        Commands::Compat { action } => run_compat(action, output),
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
        if self.json || self.verbose < 2 || paths.is_empty() {
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

#[expect(
    clippy::too_many_lines,
    reason = "language-specific compat rendering stays together for CLI readability"
)]
fn run_compat(action: &CompatAction, output: Output) -> Result<()> {
    match action {
        CompatAction::Typescript {
            old,
            new,
            contract,
            suggest,
        } => {
            let contract_path = contract.as_deref();
            let contract = load_compat_contract(contract_path)?;
            let old_surface = gnr8::sdk::compat::extract_typescript_surface(old)?;
            let new_surface = gnr8::sdk::compat::extract_typescript_surface(new)?;
            let diff = gnr8::sdk::compat::diff_typescript_surfaces(&old_surface, &new_surface);
            let evaluation = contract.as_ref().map(|contract| {
                gnr8::sdk::compat::evaluate_typescript_contract(
                    &contract.typescript,
                    &diff,
                    &new_surface,
                )
                .with_global_docs_allowance(&contract.allow)
            });
            let effective_diff = evaluation
                .as_ref()
                .map_or(&diff, |evaluation| &evaluation.unapproved_diff);
            let suggestions = if *suggest {
                gnr8::sdk::compat::suggest_typescript_compat(effective_diff)
            } else {
                Vec::new()
            };
            let breaking = evaluation
                .as_ref()
                .map_or_else(|| diff.is_breaking(), |evaluation| evaluation.breaking);
            let allowed_missing_docs =
                allowed_missing_docs(&diff.missing_docs, &effective_diff.missing_docs);
            if output.json {
                #[derive(serde::Serialize)]
                struct CompatReport<'a> {
                    language: &'static str,
                    old: &'a str,
                    new: &'a str,
                    contract: Option<&'a str>,
                    breaking: bool,
                    docs_breaking: bool,
                    allowed_missing_docs: &'a [String],
                    diff: &'a gnr8::sdk::compat::TypeScriptSurfaceDiff,
                    contract_evaluation:
                        Option<&'a gnr8::sdk::compat::TypeScriptContractEvaluation>,
                    suggestions: &'a [String],
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&CompatReport {
                        language: "typescript",
                        old,
                        new,
                        contract: contract_path,
                        breaking,
                        docs_breaking: effective_diff.has_doc_breaks(),
                        allowed_missing_docs: &allowed_missing_docs,
                        diff: &diff,
                        contract_evaluation: evaluation.as_ref(),
                        suggestions: &suggestions,
                    })?
                );
            } else if breaking {
                output.progress("compat typescript: breaking changes detected");
                if let Some(evaluation) = &evaluation {
                    print_compat_list(
                        "missing required contract symbols",
                        &evaluation.missing_required,
                    );
                }
                print_typescript_compat_diff(effective_diff);
            } else {
                output.progress("compat typescript: compatible");
            }
            if !output.json {
                print_compat_suggestions(&suggestions);
            }
            if breaking {
                std::process::exit(1);
            }
            Ok(())
        }
        CompatAction::Go {
            old,
            new,
            contract,
            suggest,
        } => {
            let contract_path = contract.as_deref();
            let contract = load_compat_contract(contract_path)?;
            let old_surface = gnr8::sdk::compat::extract_go_surface(old)?;
            let new_surface = gnr8::sdk::compat::extract_go_surface(new)?;
            let diff = gnr8::sdk::compat::diff_go_surfaces(&old_surface, &new_surface);
            let evaluation = contract.as_ref().map(|contract| {
                gnr8::sdk::compat::evaluate_go_contract(&contract.go, &diff, &new_surface)
                    .with_global_docs_allowance(&contract.allow)
            });
            let effective_diff = evaluation
                .as_ref()
                .map_or(&diff, |evaluation| &evaluation.unapproved_diff);
            let suggestions = if *suggest {
                gnr8::sdk::compat::suggest_go_compat(effective_diff)
            } else {
                Vec::new()
            };
            let breaking = evaluation
                .as_ref()
                .map_or_else(|| diff.is_breaking(), |evaluation| evaluation.breaking);
            let allowed_missing_docs =
                allowed_missing_docs(&diff.missing_docs, &effective_diff.missing_docs);
            if output.json {
                #[derive(serde::Serialize)]
                struct CompatReport<'a> {
                    language: &'static str,
                    old: &'a str,
                    new: &'a str,
                    contract: Option<&'a str>,
                    breaking: bool,
                    docs_breaking: bool,
                    allowed_missing_docs: &'a [String],
                    diff: &'a gnr8::sdk::compat::GoSurfaceDiff,
                    contract_evaluation: Option<&'a gnr8::sdk::compat::GoContractEvaluation>,
                    suggestions: &'a [String],
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&CompatReport {
                        language: "go",
                        old,
                        new,
                        contract: contract_path,
                        breaking,
                        docs_breaking: effective_diff.has_doc_breaks(),
                        allowed_missing_docs: &allowed_missing_docs,
                        diff: &diff,
                        contract_evaluation: evaluation.as_ref(),
                        suggestions: &suggestions,
                    })?
                );
            } else if breaking {
                output.progress("compat go: breaking changes detected");
                if let Some(evaluation) = &evaluation {
                    print_compat_list(
                        "missing required contract symbols",
                        &evaluation.missing_required,
                    );
                }
                print_go_compat_diff(effective_diff);
            } else {
                output.progress("compat go: compatible");
            }
            if !output.json {
                print_compat_suggestions(&suggestions);
            }
            if breaking {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

fn load_compat_contract(
    path: Option<&str>,
) -> Result<Option<gnr8::sdk::compat::CompatibilityContract>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let contract_path = Path::new(path);
    if !contract_path.exists() {
        anyhow::bail!("compat contract does not exist: {path}");
    }
    let text = std::fs::read_to_string(contract_path)
        .with_context(|| format!("failed to read compat contract: {path}"))?;
    let contract = toml::from_str(&text)
        .with_context(|| format!("failed to parse compat contract TOML: {path}"))?;
    Ok(Some(contract))
}

trait GoContractEvaluationExt {
    fn with_global_docs_allowance(
        self,
        allow: &gnr8::sdk::compat::CompatibilityAllow,
    ) -> gnr8::sdk::compat::GoContractEvaluation;
}

impl GoContractEvaluationExt for gnr8::sdk::compat::GoContractEvaluation {
    fn with_global_docs_allowance(
        mut self,
        allow: &gnr8::sdk::compat::CompatibilityAllow,
    ) -> gnr8::sdk::compat::GoContractEvaluation {
        apply_global_docs_stale_allowances(
            "allow.missing_docs",
            allow,
            &self.unapproved_diff.missing_docs,
            &mut self.stale_allowances,
        );
        self.unapproved_diff.missing_docs =
            unallowed_missing_docs(&self.unapproved_diff.missing_docs, allow);
        self.breaking = !self.missing_required.is_empty() || self.unapproved_diff.is_breaking();
        self
    }
}

trait TypeScriptContractEvaluationExt {
    fn with_global_docs_allowance(
        self,
        allow: &gnr8::sdk::compat::CompatibilityAllow,
    ) -> gnr8::sdk::compat::TypeScriptContractEvaluation;
}

impl TypeScriptContractEvaluationExt for gnr8::sdk::compat::TypeScriptContractEvaluation {
    fn with_global_docs_allowance(
        mut self,
        allow: &gnr8::sdk::compat::CompatibilityAllow,
    ) -> gnr8::sdk::compat::TypeScriptContractEvaluation {
        apply_global_docs_stale_allowances(
            "allow.missing_docs",
            allow,
            &self.unapproved_diff.missing_docs,
            &mut self.stale_allowances,
        );
        self.unapproved_diff.missing_docs =
            unallowed_missing_docs(&self.unapproved_diff.missing_docs, allow);
        self.breaking = !self.missing_required.is_empty() || self.unapproved_diff.is_breaking();
        self
    }
}

fn apply_global_docs_stale_allowances(
    label: &str,
    allow: &gnr8::sdk::compat::CompatibilityAllow,
    current: &[String],
    stale: &mut Vec<String>,
) {
    if allow.docs_layout_migration {
        return;
    }
    let current: BTreeSet<&str> = current.iter().map(String::as_str).collect();
    for doc in &allow.missing_docs {
        if !current.contains(doc.as_str()) {
            stale.push(format!("{label}: {doc}"));
        }
    }
}

fn unallowed_missing_docs(
    missing_docs: &[String],
    allow: &gnr8::sdk::compat::CompatibilityAllow,
) -> Vec<String> {
    if allow.docs_layout_migration {
        return Vec::new();
    }
    missing_docs
        .iter()
        .filter(|doc| !allow.missing_docs.iter().any(|allowed| allowed == *doc))
        .cloned()
        .collect()
}

fn allowed_missing_docs(missing_docs: &[String], unallowed: &[String]) -> Vec<String> {
    missing_docs
        .iter()
        .filter(|doc| !unallowed.iter().any(|unallowed| unallowed == *doc))
        .cloned()
        .collect()
}

fn print_typescript_compat_diff(diff: &gnr8::sdk::compat::TypeScriptSurfaceDiff) {
    print_compat_list("missing root exports", &diff.missing_root_exports);
    print_compat_list("missing model exports", &diff.missing_model_exports);
    print_compat_list("missing API classes", &diff.missing_api_classes);
    print_compat_list("missing API factories", &diff.missing_api_factories);
    print_compat_list("missing operation methods", &diff.missing_operation_methods);
    print_compat_list("missing request aliases", &diff.missing_request_aliases);
    print_compat_list("package entry changes", &diff.package_entry_point_changes);
    print_compat_list("missing docs", &diff.missing_docs);
    for missing in &diff.missing_interface_properties {
        println!(
            "  missing interface property: {}.{}",
            missing.interface, missing.property
        );
    }
    for change in &diff.interface_property_changes {
        println!(
            "  interface property changed: {}.{} (optional {} -> {}, nullable {} -> {}, type {} -> {})",
            change.interface,
            change.property,
            change.old.optional,
            change.new.optional,
            change.old.nullable,
            change.new.nullable,
            change.old.ty,
            change.new.ty
        );
    }
    for change in &diff.operation_return_type_changes {
        println!(
            "  operation return changed: {} ({} -> {})",
            change.operation, change.old, change.new
        );
    }
    for change in &diff.operation_signature_changes {
        println!(
            "  operation signature changed: {} ({} -> {})",
            change.operation, change.old, change.new
        );
    }
    for mismatch in &diff.export_kind_mismatches {
        println!(
            "  export kind mismatch: {} ({:?} -> {:?})",
            mismatch.symbol, mismatch.old, mismatch.new
        );
    }
}

fn print_go_compat_diff(diff: &gnr8::sdk::compat::GoSurfaceDiff) {
    print_compat_list("missing exported types", &diff.missing_exported_types);
    print_compat_list(
        "missing exported functions",
        &diff.missing_exported_functions,
    );
    print_compat_list("missing exported methods", &diff.missing_exported_methods);
    print_compat_list("missing docs", &diff.missing_docs);
    print_compat_list("package metadata changes", &diff.package_metadata_changes);
    for change in &diff.exported_function_signature_changes {
        println!(
            "  exported function signature changed: {} ({} -> {})",
            change.symbol, change.old, change.new
        );
    }
    for change in &diff.exported_method_signature_changes {
        println!(
            "  exported method signature changed: {} ({} -> {})",
            change.symbol, change.old, change.new
        );
    }
}

fn print_compat_suggestions(suggestions: &[String]) {
    print_compat_list("suggestions", suggestions);
}

fn print_compat_list(label: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    println!("  {label}:");
    for value in values {
        println!("    {value}");
    }
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
            summary: "Complex Go/Gin setup with OpenAPI plus two SDK targets and compatibility transforms.",
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
    /// Whether `--accept-generated-baseline` was used.
    baseline_adopted: bool,
    /// Migration cleanup classification for reviewing stale/generated-looking files.
    cleanup: CleanupReport,
}

#[derive(Debug, serde::Serialize)]
struct CleanupReport {
    /// Files currently owned by this generation run.
    owned_files: Vec<String>,
    /// Stale generated files deleted during this generation.
    stale_generated_files: Vec<String>,
    /// Generated-looking files that are not owned by this run.
    generated_looking_unowned_files: Vec<String>,
    /// Hand-edited generated files protected from overwrite.
    protected_hand_edited_files: Vec<String>,
    /// Package/config files likely left from `OpenAPI` Generator-era output.
    legacy_package_files: Vec<String>,
    /// Package dependencies likely no longer needed after replacing `OpenAPI` Generator output.
    obsolete_package_dependencies: Vec<String>,
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
    warn: usize,
    error: usize,
}

/// Run `gnr8 generate` (+ `--force`): run the user's `.gnr8/` pipeline (child process), then write only
/// changed files and report counts. Every protected (user-edited) file is named in a stderr warning so
/// the "no silent clobbering" protection is VISIBLE (T-04-02-04). Pipeline diagnostics the child carried
/// are surfaced too. `--json` serializes the counts. A missing `.gnr8/` (run `gnr8 init`), a compile
/// error in the user's pipeline, or a missing Go toolchain surface via the anyhow boundary, never a panic.
fn run_generate(force: bool, accept_generated_baseline: bool, output: Output) -> Result<()> {
    let root = project_root()?;
    let total_start = Instant::now();
    let hot_start = Instant::now();
    let hot_noop = pre_child_verified_noop(&root);
    let hot_elapsed = hot_start.elapsed();
    let mut pipeline_elapsed = None;
    let mut write_elapsed = None;
    let (outcome, diagnostics, cache_label, source_files, artifact_files) = if let Some(noop) =
        hot_noop
    {
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
        let outcome = regenerate_bundle(&root, &mut bundle, force || accept_generated_baseline)?;
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

    if output.json {
        let cleanup = migration_cleanup_report(&root, &outcome);
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
            baseline_adopted: accept_generated_baseline,
            cleanup,
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
    Ok(())
}

fn migration_cleanup_report(
    root: &Path,
    outcome: &gnr8::lifecycle::GenerateOutcome,
) -> CleanupReport {
    let mut owned_files = Vec::new();
    owned_files.extend(outcome.written.iter().cloned());
    owned_files.extend(outcome.unchanged.iter().cloned());
    owned_files.extend(outcome.skipped.iter().cloned());
    owned_files.sort();
    owned_files.dedup();

    let owned: BTreeSet<String> = owned_files.iter().cloned().collect();
    let legacy_package_files = legacy_openapi_generator_files(root);
    let generated_looking_unowned_files = legacy_package_files
        .iter()
        .filter(|path| !owned.contains(*path))
        .cloned()
        .collect();

    CleanupReport {
        owned_files,
        stale_generated_files: outcome.deleted.clone(),
        generated_looking_unowned_files,
        protected_hand_edited_files: outcome.skipped.clone(),
        obsolete_package_dependencies: legacy_openapi_generator_dependencies(root),
        legacy_package_files,
    }
}

fn legacy_openapi_generator_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_legacy_openapi_generator_files(root, root, 0, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect_legacy_openapi_generator_files(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<String>,
) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if should_skip_cleanup_scan_dir(&path) {
                continue;
            }
            collect_legacy_openapi_generator_files(root, &path, depth + 1, out);
            continue;
        }
        let Some(rel) = path.strip_prefix(root).ok().map(path_to_slash_string) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if legacy_openapi_generator_filename(name)
            || legacy_openapi_generator_package_file(name, &path)
        {
            out.push(rel);
        }
    }
}

fn should_skip_cleanup_scan_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".git" | ".gnr8" | "node_modules" | "target" | "vendor")
    )
}

fn legacy_openapi_generator_filename(name: &str) -> bool {
    matches!(
        name,
        ".openapi-generator-ignore" | "git_push.sh" | "openapitools.json"
    ) || name == ".openapi-generator"
}

fn legacy_openapi_generator_package_file(name: &str, path: &Path) -> bool {
    if !matches!(
        name,
        "package.json" | "go.mod" | "pom.xml" | "build.gradle" | "build.gradle.kts"
    ) {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.contains("openapi-generator")
        || text.contains("@openapitools")
        || text.contains("github.com/antihax/optional")
}

fn legacy_openapi_generator_dependencies(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_legacy_openapi_generator_dependencies(root, root, 0, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect_legacy_openapi_generator_dependencies(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<String>,
) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if should_skip_cleanup_scan_dir(&path) {
                continue;
            }
            collect_legacy_openapi_generator_dependencies(root, &path, depth + 1, out);
            continue;
        }
        let Some(rel) = path.strip_prefix(root).ok().map(path_to_slash_string) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match name {
            "package.json" => {
                for dep in [
                    "@openapitools/openapi-generator-cli",
                    "openapi-generator",
                    "typescript-axios",
                    "typescript-fetch",
                ] {
                    if text.contains(dep) {
                        out.push(format!("{rel}: {dep}"));
                    }
                }
            }
            "go.mod" => {
                for dep in ["github.com/antihax/optional"] {
                    if text.contains(dep) {
                        out.push(format!("{rel}: {dep}"));
                    }
                }
            }
            "pom.xml" | "build.gradle" | "build.gradle.kts" => {
                for dep in ["org.openapitools", "openapi-generator"] {
                    if text.contains(dep) {
                        out.push(format!("{rel}: {dep}"));
                    }
                }
            }
            _ => {}
        }
    }
}

fn path_to_slash_string(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
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
            let plan = plan_check_bundle(&root, &mut bundle)?;
            plan_elapsed = Some(plan_start.elapsed());
            if artifact_files == 0 {
                artifact_files = plan.files.len();
            }
            (plan, diagnostics, "pipeline", source_files, artifact_files)
        };

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
    output.verbose_paths("stale", &stale);
    output.verbose_paths("drifted", &drifted);

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

fn regenerate_bundle(
    root: &std::path::Path,
    bundle: &mut gnr8::runner::ArtifactBundle,
    force: bool,
) -> Result<gnr8::lifecycle::GenerateOutcome, gnr8::CoreError> {
    if let Some(metadata) = cached_artifact_metadata(root, bundle) {
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

fn plan_check_bundle(
    root: &std::path::Path,
    bundle: &mut gnr8::runner::ArtifactBundle,
) -> Result<gnr8::lifecycle::WritePlan, gnr8::CoreError> {
    let mut plan = plan_bundle(root, bundle)?;
    normalize_unowned_identical_outputs_for_check(root, &mut plan);
    if !plan.has_drift() {
        save_verified_noop_stamp_from_plan(root, bundle, &plan);
    }
    Ok(plan)
}

fn save_verified_noop_stamp_from_plan(
    root: &std::path::Path,
    bundle: &gnr8::runner::ArtifactBundle,
    plan: &gnr8::lifecycle::WritePlan,
) {
    let paths = plan.files.iter().map(|file| file.path.clone()).collect();
    let outcome = gnr8::lifecycle::GenerateOutcome {
        written: Vec::new(),
        unchanged: plan.files.iter().map(|file| file.path.clone()).collect(),
        skipped: Vec::new(),
        deleted: Vec::new(),
    };
    save_verified_noop_stamp_for_paths(root, bundle, paths, &outcome);
}

fn normalize_unowned_identical_outputs_for_check(
    root: &std::path::Path,
    plan: &mut gnr8::lifecycle::WritePlan,
) {
    for file in &mut plan.files {
        if file.action != gnr8::lifecycle::WriteAction::UserEdited {
            continue;
        }
        let Ok(bytes) = std::fs::read(root.join(&file.path)) else {
            continue;
        };
        if gnr8::manifest::blake3_hex(&bytes) == file.new_hash {
            file.action = gnr8::lifecycle::WriteAction::Unchanged;
        }
    }
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

struct CachedNoop {
    outcome: gnr8::lifecycle::GenerateOutcome,
    diagnostics: Vec<gnr8::graph::Diagnostic>,
    source_files: usize,
    artifact_files: usize,
}

fn pre_child_verified_noop(root: &std::path::Path) -> Option<CachedNoop> {
    let stamp = load_verified_noop_stamp(root)?;
    let current_inputs = collect_hot_input_fast_stamps(root, &stamp.input_roots)?;
    if current_inputs != stamp.input_fast_files {
        return None;
    }
    let current_outputs =
        collect_verified_fast_output_stamps(root, &stamp.output_anchors, &stamp.artifact_paths)?;
    if current_outputs.artifact_files != stamp.output_artifact_fast_files
        || current_outputs.dirs != stamp.output_dir_fast_stamps
    {
        return None;
    }
    let source_files = stamp.input_fast_files.len();
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
    let artifact_paths = artifact_paths(metadata);
    let current =
        collect_verified_fast_output_stamps(root, &bundle.output_anchors, &artifact_paths)?;
    if current.artifact_files != stamp.output_artifact_fast_files
        || current.dirs != stamp.output_dir_fast_stamps
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
    save_verified_noop_stamp_for_paths(root, bundle, artifact_paths(metadata), outcome);
}

fn save_verified_noop_stamp_from_artifacts(
    root: &std::path::Path,
    bundle: &gnr8::runner::ArtifactBundle,
    outcome: &gnr8::lifecycle::GenerateOutcome,
) {
    let paths = bundle
        .artifacts
        .iter()
        .map(|artifact| artifact.path.clone())
        .collect();
    save_verified_noop_stamp_for_paths(root, bundle, paths, outcome);
}

fn save_verified_noop_stamp_for_paths(
    root: &std::path::Path,
    bundle: &gnr8::runner::ArtifactBundle,
    artifact_paths: Vec<String>,
    outcome: &gnr8::lifecycle::GenerateOutcome,
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
    let Some(output_fast) =
        collect_verified_fast_output_stamps(root, &bundle.output_anchors, &artifact_paths)
    else {
        return;
    };
    let Some(input_fast_files) = collect_hot_input_fast_stamps(root, &bundle.cache_input_roots)
    else {
        return;
    };
    let stamp = VerifiedNoopStamp {
        artifact_cache_key: key.to_string(),
        output_anchors: bundle.output_anchors.clone(),
        artifact_paths,
        input_roots: bundle.cache_input_roots.clone(),
        input_fast_files,
        output_artifact_fast_files: output_fast.artifact_files,
        output_dir_fast_stamps: output_fast.dirs,
        input_files: Vec::new(),
        output_files: Vec::new(),
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

fn collect_hot_input_fast_stamps(
    root: &std::path::Path,
    input_roots: &[String],
) -> Option<Vec<FastFileStamp>> {
    if input_roots.is_empty() {
        return None;
    }
    let mut stamps = Vec::new();
    for input_root in input_roots {
        collect_hot_input_file_stamps(root, &root.join(input_root), &mut stamps)?;
    }
    collect_host_config_fast_stamps(root, &mut stamps)?;
    stamps.sort();
    Some(stamps)
}

fn collect_host_config_fast_stamps(
    root: &std::path::Path,
    out: &mut Vec<FastFileStamp>,
) -> Option<()> {
    let gnr8_dir = root.join(".gnr8");
    let _ = collect_hot_input_file_stamps(root, &gnr8_dir.join("src"), out);
    for name in ["Cargo.toml", "Cargo.lock"] {
        let path = gnr8_dir.join(name);
        if path.is_file() {
            push_fast_file_stamp(root, &path, out)?;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        push_fast_file_stamp(root, &exe, out)?;
    }
    Some(())
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

    let groups = artifact_groups_by_anchor(bundle);
    groups
        .into_iter()
        .filter_map(|(anchor, artifacts)| readiness_for_artifact_group(&anchor, &artifacts))
        .collect()
}

fn artifact_groups_by_anchor(
    bundle: &gnr8::runner::ArtifactBundle,
) -> BTreeMap<String, Vec<gnr8::sdk::Artifact>> {
    let mut groups: BTreeMap<String, Vec<gnr8::sdk::Artifact>> = BTreeMap::new();
    for anchor in &bundle.output_anchors {
        let normalized = anchor.trim_end_matches('/').to_string();
        if normalized.is_empty() {
            continue;
        }
        let prefix = format!("{normalized}/");
        let artifacts = bundle
            .artifacts
            .iter()
            .filter(|artifact| artifact.path == normalized || artifact.path.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        if !artifacts.is_empty() {
            groups.insert(normalized, artifacts);
        }
    }
    groups
}

fn readiness_for_artifact_group(
    anchor: &str,
    artifacts: &[gnr8::sdk::Artifact],
) -> Option<doctor::SdkReadiness> {
    if let Some(openapi) = artifacts
        .iter()
        .find(|artifact| is_openapi_artifact(&artifact.path, &artifact.text))
    {
        return Some(validate_openapi_target(&openapi.path, &openapi.text));
    }
    if artifacts
        .iter()
        .any(|artifact| path_extension_is(&artifact.path, "go"))
    {
        return Some(validate_go_target(anchor, artifacts));
    }
    if artifacts
        .iter()
        .any(|artifact| path_extension_is(&artifact.path, "py"))
    {
        return Some(validate_python_target(anchor, artifacts));
    }
    if artifacts
        .iter()
        .any(|artifact| path_extension_is(&artifact.path, "ts"))
    {
        return Some(validate_typescript_target(anchor, artifacts));
    }
    None
}

fn is_openapi_artifact(path: &str, text: &str) -> bool {
    let openapi_like = text.contains("openapi:")
        || text.contains("\"openapi\"")
        || text.contains("swagger:")
        || text.contains("\"swagger\"");
    (path_extension_is(path, "yaml")
        || path_extension_is(path, "yml")
        || path_extension_is(path, "json"))
        && openapi_like
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
    if let Err(reason) = python_import_package(&materialized.target_dir) {
        return doctor::SdkReadiness::not_ready("python", anchor, TOOLCHAIN, reason);
    }
    doctor::SdkReadiness::ready("python", anchor, TOOLCHAIN)
}

fn validate_typescript_target(
    anchor: &str,
    artifacts: &[gnr8::sdk::Artifact],
) -> doctor::SdkReadiness {
    const TOOLCHAIN: &str = "node + typescript --noEmit --strict";
    if let Err(reason) = command_available("node", &["--version"]) {
        return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
    }
    let Some(tsc) = typescript_compiler_path() else {
        return doctor::SdkReadiness::not_ready(
            "typescript",
            anchor,
            TOOLCHAIN,
            "typescript compiler not found; run `make tsextract-deps` or install the TypeScript toolchain",
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
    if materialized.target_dir.join("package.json").is_file()
        && command_available("npm", &["--version"]).is_ok()
    {
        if let Err(reason) =
            command_success_in("npm", &["pack", "--dry-run"], &materialized.target_dir, &[])
        {
            return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
        }
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
    if let Err(reason) = typescript_typecheck(&tsc, &ts_files, &materialized.target_dir) {
        return doctor::SdkReadiness::not_ready("typescript", anchor, TOOLCHAIN, reason);
    }
    doctor::SdkReadiness::ready("typescript", anchor, TOOLCHAIN)
}

struct MaterializedTarget {
    root: PathBuf,
    target_dir: PathBuf,
}

fn materialize_artifact_group(
    anchor: &str,
    artifacts: &[gnr8::sdk::Artifact],
    label: &str,
) -> Result<MaterializedTarget, String> {
    let root = unique_doctor_temp_dir(label)?;
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
    Ok(MaterializedTarget { root, target_dir })
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
    stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("command exited non-zero without output")
        .to_string()
}

fn python_compile(files: &[PathBuf]) -> Result<(), String> {
    let args = std::iter::once("-m".to_string())
        .chain(std::iter::once("py_compile".to_string()))
        .chain(files.iter().map(|path| path.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    command_success_in("python3", &arg_refs, Path::new("."), &[])
}

fn python_import_package(package_dir: &Path) -> Result<(), String> {
    let init = package_dir.join("__init__.py");
    if !init.is_file() {
        return Err("generated Python SDK is missing __init__.py".to_string());
    }
    let code = "\
import importlib.util
import sys
init_path = sys.argv[1]
package_dir = sys.argv[2]
spec = importlib.util.spec_from_file_location(
    'gnr8_sdk_check',
    init_path,
    submodule_search_locations=[package_dir],
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
";
    let init_arg = init.to_string_lossy().into_owned();
    let dir_arg = package_dir.to_string_lossy().into_owned();
    command_success_in(
        "python3",
        &["-c", code, &init_arg, &dir_arg],
        package_dir.parent().unwrap_or_else(|| Path::new(".")),
        &[],
    )
}

fn typescript_compiler_path() -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tsextract")
        .join("node_modules")
        .join("typescript")
        .join("lib")
        .join("tsc.js");
    path.is_file().then_some(path)
}

fn typescript_typecheck(tsc: &Path, files: &[PathBuf], cwd: &Path) -> Result<(), String> {
    let mut args = vec![
        tsc.to_string_lossy().into_owned(),
        "--noEmit".to_string(),
        "--strict".to_string(),
        "--lib".to_string(),
        "es2022,dom".to_string(),
    ];
    args.extend(files.iter().map(|path| path.to_string_lossy().into_owned()));
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    command_success_in("node", &arg_refs, cwd, &[])
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
    let mut bundle = if initialized {
        output.progress("doctor: running pipeline");
        child::run_child(&root, "__emit").ok()
    } else {
        None
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
    let drift = bundle.as_mut().and_then(|b| plan_bundle(&root, b).ok());

    let report = doctor::DoctorReport::assemble(
        initialized,
        source_present,
        &language,
        pipeline_ran,
        diagnostics,
        drift.as_ref(),
    )
    .with_sdk_readiness(sdk_readiness)
    .with_runtime(
        doctor::DoctorRuntime {
            binary_path: std::env::current_exe()
                .ok()
                .map(|path| path.to_string_lossy().into_owned()),
            resource_dir: gnr8::resource::resource_dir()
                .map(|path| path.to_string_lossy().into_owned()),
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
/// In a project with `.gnr8/`, inspect uses the same child `__inspect` pipeline as generation so source
/// package filters, transforms, and resource/toolchain resolution match `generate`/`check`. Without a
/// local `.gnr8/` workspace it falls back to direct source inspection of the provided path.
fn run_inspect(action: &InspectAction, output: Output) -> Result<()> {
    let total_start = Instant::now();
    let rendered = match action {
        InspectAction::Routes { path } => {
            let graph = inspect_graph(path, output)?;
            render::render_routes(&graph, output.json)?
        }
        InspectAction::Schemas { path } => {
            let graph = inspect_graph(path, output)?;
            render::render_schemas(&graph, output.json)?
        }
        InspectAction::Graph { path } => {
            let graph = inspect_graph(path, output)?;
            render::render_graph(&graph, output.json)?
        }
    };
    print!("{rendered}");
    output.verbose(format!("total: {}", fmt_duration(total_start.elapsed())));
    Ok(())
}

fn inspect_graph(path: &str, output: Output) -> Result<gnr8::graph::ApiGraph> {
    let root = project_root()?;
    if gnr8::workspace::manifest_path(&root).is_file() {
        output.verbose(format!(
            "inspect: using .gnr8 pipeline at {}",
            root.display()
        ));
        return Ok(child::inspect_child(&root)?);
    }
    output.verbose(format!("inspect: analyzing source path directly: {path}"));
    Ok(gnr8::analyze::build_graph(path)?)
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
        eprintln!(
            "warning: {} pipeline diagnostics (run with -v for details)",
            diagnostics.len()
        );
        return;
    }
    for diag in diagnostics {
        eprintln!(
            "{}: {} ({}:{})",
            diag.severity, diag.message, diag.file, diag.line
        );
    }
}

fn diagnostic_counts(diagnostics: &[gnr8::graph::Diagnostic]) -> DiagnosticCounts {
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

    use super::{allowed_missing_docs, reconcile_doctor_source_probe, unallowed_missing_docs};
    use gnr8::sdk::compat::CompatibilityAllow;
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

    #[test]
    fn compat_contract_can_allow_all_docs_layout_migration() {
        let allow = CompatibilityAllow {
            docs_layout_migration: true,
            missing_docs: Vec::new(),
        };
        let missing = vec!["docs/BooksApi.md".to_string(), "docs/Book.md".to_string()];

        assert!(unallowed_missing_docs(&missing, &allow).is_empty());
    }

    #[test]
    fn compat_contract_can_allow_selected_missing_docs_only() {
        let allow = CompatibilityAllow {
            docs_layout_migration: false,
            missing_docs: vec!["docs/Book.md".to_string()],
        };
        let missing = vec!["docs/BooksApi.md".to_string(), "docs/Book.md".to_string()];
        let unallowed = unallowed_missing_docs(&missing, &allow);

        assert_eq!(unallowed, vec!["docs/BooksApi.md".to_string()]);
        assert_eq!(
            allowed_missing_docs(&missing, &unallowed),
            vec!["docs/Book.md".to_string()]
        );
    }
}
