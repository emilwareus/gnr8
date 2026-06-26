//! The child-process runner: the entry point a user's `.gnr8/` binary calls from `main()`.
//!
//! The `.gnr8/` crate is a pure generator; the installed `gnr8` host is the orchestrator + trusted
//! writer (see `docs/code-as-config.md`). The boundary is `cargo run --manifest-path` +
//! JSON-on-stdout + an exit code — no FFI, no plugin ABI. This module owns the child side:
//!
//! - `__emit`    → run the full pipeline, print the [`ArtifactBundle`] JSON to stdout, exit 0 (1 on
//!   error). The host deserializes the bundle and owns the actual file writes next stage.
//! - `__inspect` → run the pipeline through transforms only, print the frozen IR as pretty JSON to
//!   stdout, exit 0 (1 on error).
//! - unknown / no subcommand → print usage to stderr, exit 2.
//!
//! [`run`] NEVER panics: argv is parsed with [`std::env::args`] (no clap), and every pipeline error is
//! caught and rendered to stderr with a non-zero exit code (RUST-04).

// User-facing prose dense with proper nouns (IR, JSON, stdout, argv, FFI, ...); allow doc_markdown
// module-wide (mirrors the rest of the framework surface).
#![allow(clippy::doc_markdown)]

use std::process::ExitCode;

use crate::graph::Diagnostic;
use crate::sdk::{Artifact, Cx, FileStamp, Pipeline};
use crate::CoreError;

/// The current artifact-bundle wire-schema version. Bumped on any breaking change to the JSON shape;
/// the host (the `gnr8` binary) rejects a bundle whose `version` differs from this, so a `.gnr8/`
/// crate built against a skewed `gnr8-core` fails with an actionable error instead of a confusing
/// parse error or silently-wrong output (forward/back-compat across the boundary).
pub const BUNDLE_VERSION: u32 = 2;

/// The exit code for a usage error (unknown / missing subcommand). `0` = success, `1` = run error,
/// `2` = usage, mirroring conventional CLI exit semantics.
const EXIT_USAGE: u8 = 2;

/// The versioned artifact bundle the child prints on stdout for `__emit` and the host deserializes.
///
/// Derives both `Serialize` (the child writes it) and `Deserialize` (the host reads it next stage),
/// so it is the single shared wire type across the process boundary. Lives in `runner` because the
/// runner owns the protocol; re-exported from there for the host to consume.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtifactBundle {
    /// The wire-schema version ([`BUNDLE_VERSION`]).
    pub version: u32,
    /// The generated files, sorted by path (the pipeline keeps them ordered).
    pub artifacts: Vec<Artifact>,
    /// Diagnostics the IR carried after transforms (lossy/unsupported source patterns).
    pub diagnostics: Vec<Diagnostic>,
    /// Project-relative target output anchors, used by the host to prune stale generated files.
    #[serde(default)]
    pub output_anchors: Vec<String>,
    /// Optional key for artifacts stored under `.gnr8/cache/artifacts/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_cache_key: Option<String>,
    /// Source input roots that can be rescanned by the host before a hot no-op child skip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache_input_roots: Vec<String>,
    /// Source input file stamps captured by the child.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache_input_stamps: Vec<FileStamp>,
}

impl ArtifactBundle {
    /// Wrap a pipeline's artifacts + diagnostics in the current versioned envelope.
    #[must_use]
    fn new(
        artifacts: Vec<Artifact>,
        diagnostics: Vec<Diagnostic>,
        output_anchors: Vec<String>,
        artifact_cache_key: Option<String>,
        cache_input_roots: Vec<String>,
        cache_input_stamps: Vec<FileStamp>,
    ) -> Self {
        Self {
            version: BUNDLE_VERSION,
            artifacts,
            diagnostics,
            output_anchors,
            artifact_cache_key,
            cache_input_roots,
            cache_input_stamps,
        }
    }
}

/// The recognized child subcommands.
enum Mode {
    /// `__emit` — run the full pipeline, print the [`ArtifactBundle`] JSON.
    Emit,
    /// `__inspect` — run through transforms, print the frozen IR JSON.
    Inspect,
}

/// Run the user's pipeline as the `.gnr8/` child process.
///
/// Parses `argv[1]` (via [`std::env::args`], no clap): `__emit` / `__inspect` dispatch to the two
/// modes; anything else (including no subcommand) prints usage to stderr and exits `2`. On a pipeline
/// error the message is printed to stderr and the process exits `1`. On success the requested JSON is
/// printed to stdout and the process exits `0`. Never panics (RUST-04).
///
/// `cx.project_root` is [`std::env::current_dir`] — when the host runs the child with
/// `current_dir = repo root`, a source's relative input resolves against the repo root.
// The by-value `Pipeline` is the public contract the user's `main()` calls (`runner::run(Pipeline)`)
// — it takes ownership of the composed pipeline so the user hands it over wholesale. We only borrow
// it internally, so clippy flags `needless_pass_by_value`; the owned signature is intentional.
#[allow(clippy::needless_pass_by_value)]
#[must_use]
pub fn run(pipeline: Pipeline) -> ExitCode {
    // argv[1] is the subcommand; ignore the program name (argv[0]) and any trailing args for now.
    let mode = match std::env::args().nth(1).as_deref() {
        Some("__emit") => Mode::Emit,
        Some("__inspect") => Mode::Inspect,
        other => {
            print_usage(other);
            return ExitCode::from(EXIT_USAGE);
        }
    };

    // project_root = the process cwd (the host sets current_dir = repo root). A failure to read it is
    // a typed error, never a panic.
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("gnr8: cannot determine the current directory: {err}");
            return ExitCode::FAILURE;
        }
    };
    let cx = Cx::new(cwd);

    let result = match mode {
        Mode::Emit => emit(&pipeline, &cx),
        Mode::Inspect => inspect(&pipeline, &cx),
    };
    match result {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("gnr8: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Run the full pipeline and serialize the [`ArtifactBundle`] as compact JSON.
///
/// # Errors
///
/// Propagates any pipeline error, or a [`CoreError::SdkGen`] if the bundle cannot be serialized (it
/// is plain owned data, so this is effectively unreachable, but it stays a typed error, never a panic).
fn emit(pipeline: &Pipeline, cx: &Cx) -> Result<String, CoreError> {
    let outcome = pipeline.run_for_emit(cx)?;
    let bundle = ArtifactBundle::new(
        outcome.artifacts.into_files(),
        outcome.diagnostics,
        pipeline.output_anchors(),
        outcome
            .artifact_cache_hit
            .then_some(outcome.artifact_cache_key)
            .flatten(),
        pipeline.cache_input_roots(cx),
        pipeline.cache_input_stamps(cx),
    );
    serde_json::to_string(&bundle).map_err(|err| CoreError::SdkGen {
        message: format!("failed to serialize the artifact bundle: {err}"),
    })
}

/// Run the pipeline through transforms only and serialize the frozen IR as pretty JSON.
///
/// # Errors
///
/// Propagates any source/transform error, or a [`CoreError::SdkGen`] if the IR cannot be serialized
/// (plain owned data — effectively unreachable). Never panics.
fn inspect(pipeline: &Pipeline, cx: &Cx) -> Result<String, CoreError> {
    let ir = pipeline.build_ir(cx)?;
    serde_json::to_string_pretty(&ir).map_err(|err| CoreError::SdkGen {
        message: format!("failed to serialize the IR: {err}"),
    })
}

/// Print the child usage to stderr (for an unknown or missing subcommand).
fn print_usage(got: Option<&str>) {
    if let Some(arg) = got {
        eprintln!("gnr8: unknown subcommand {arg:?}");
    } else {
        eprintln!("gnr8: missing subcommand");
    }
    eprintln!("usage: <gnr8-gen> <__emit|__inspect>");
    eprintln!("  __emit     run the pipeline and print the artifact bundle JSON to stdout");
    eprintln!("  __inspect  run the pipeline through transforms and print the IR JSON to stdout");
    eprintln!(
        "this binary is the gnr8 generation child; it is normally invoked by the gnr8 host via \
         `cargo run --manifest-path .gnr8/Cargo.toml -- __emit`."
    );
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{emit, inspect, ArtifactBundle, BUNDLE_VERSION};
    use crate::graph::ApiGraph;
    use crate::sdk::{Cx, Pipeline, Source};
    use crate::CoreError;

    /// A source yielding a fixed graph with one diagnostic, so emit/inspect run without a toolchain.
    struct StubSource;
    impl Source for StubSource {
        fn load(&self, _cx: &Cx) -> Result<ApiGraph, CoreError> {
            Ok(ApiGraph {
                title: "Stub API".to_string(),
                diagnostics: vec![crate::graph::Diagnostic {
                    severity: "WARN".to_string(),
                    message: "stub diagnostic".to_string(),
                    file: "x.go".to_string(),
                    line: 1,
                }],
                ..ApiGraph::default()
            })
        }
    }

    fn cx() -> Cx {
        Cx::new(std::env::temp_dir())
    }

    #[test]
    fn emit_produces_a_versioned_bundle_that_round_trips() {
        // A pipeline with no targets still emits a valid (empty-artifacts) bundle carrying diagnostics.
        let json = emit(&Pipeline::new().source(StubSource), &cx()).unwrap();
        let bundle: ArtifactBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(bundle.version, BUNDLE_VERSION);
        assert!(bundle.artifacts.is_empty());
        assert_eq!(bundle.diagnostics.len(), 1);
        assert_eq!(bundle.diagnostics[0].message, "stub diagnostic");
    }

    #[test]
    fn inspect_serializes_the_frozen_ir() {
        let json = inspect(&Pipeline::new().source(StubSource), &cx()).unwrap();
        // The IR JSON carries the title the source set.
        assert!(json.contains("\"title\": \"Stub API\""), "{json}");
    }

    #[test]
    fn emit_propagates_a_missing_source_as_typed_error() {
        let err = emit(&Pipeline::new(), &cx()).unwrap_err();
        assert!(matches!(err, CoreError::Config { .. }), "{err:?}");
    }
}
