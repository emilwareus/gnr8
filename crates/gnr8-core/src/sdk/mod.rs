//! The code-as-config SDK — the composition surface a user's `.gnr8/` Rust lifecycle drives.
//!
//! This is the framework face of `gnr8-core`: instead of a declarative TOML file, the user writes a
//! tiny Rust binary that builds a [`Pipeline`] out of four kinds of stage and hands it to
//! [`crate::runner::run`]. The four seams decouple **N sources** from **M targets** through the one
//! stable IR ([`crate::graph::ApiGraph`]):
//!
//! - [`Source`] — source code → IR (built-in: [`builtins::GoGin`]).
//! - [`Transform`] — IR → IR; where everything that used to be TOML lives, as code
//!   (built-ins: [`builtins::SetBasePath`], [`builtins::SetTitle`], [`builtins::ApplySecurity`],
//!   [`builtins::RenameOperation`], [`builtins::RenameType`]).
//! - [`Target`] — frozen IR → [`Artifacts`] (built-ins: [`builtins::OpenApi31`], [`builtins::GoSdk`]).
//! - [`PostProcess`] — [`Artifacts`] → [`Artifacts`], after all targets (built-in:
//!   [`builtins::Header`]).
//!
//! Determinism (the standing invariant): [`Artifacts`] keeps its files sorted by path, the IR is
//! already sorted, and the built-in targets wrap the existing deterministic
//! [`crate::lower::to_openapi`] / [`crate::gosdk::generate`] functions — so identical input ⇒
//! byte-identical output. No production `unwrap`/`expect`/`panic`; every fallible boundary returns a
//! typed [`crate::CoreError`].
//!
//! The built-in stages are thin wrappers — they NEVER re-implement extraction, lowering, or SDK
//! emission; they read the graph metadata a transform set and call the existing core functions
//! (CLAUDE.md: one deterministic path per fact, no fallbacks).

// User-facing prose dense with proper nouns/acronyms (IR, OpenAPI, SDK, TOML, Gin, ...); backticking
// them all would hurt readability. Allow `doc_markdown` module-wide (mirrors config/lifecycle).
#![allow(clippy::doc_markdown)]

pub mod builtins;
pub mod bundle;
pub(crate) mod emit_common;
pub mod layout;
pub mod model_style;

use std::path::PathBuf;

use crate::graph::{ApiGraph, Diagnostic};
use crate::CoreError;

/// The execution context handed to every stage.
///
/// Carries the project root every relative path (a source's input dir, a target's output path) is
/// resolved against — for the child process this is `std::env::current_dir()` (set by the runner).
/// Deliberately small for now; richer context (a diagnostics sink, a subprocess runner, a facts
/// cache — see `docs/extensibility.md`) is added in later stages without breaking this shape.
#[derive(Debug, Clone)]
pub struct Cx {
    /// The project root all relative paths resolve against.
    pub project_root: PathBuf,
}

impl Cx {
    /// Build a context rooted at `project_root`.
    #[must_use]
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
        }
    }
}

/// One generated file: a project-relative path and its UTF-8 text contents.
///
/// Generated artifacts are text (OpenAPI YAML, Go source) — modeled as `String`, not bytes, because
/// every generator gnr8 ships emits UTF-8. Derives serde so it crosses the host↔child JSON boundary
/// (inside [`crate::runner::ArtifactBundle`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Artifact {
    /// The project-relative output path (e.g. `"generated/openapi.yaml"`, `"sdk/client.go"`).
    pub path: String,
    /// The file's full UTF-8 text contents.
    pub text: String,
}

/// The accumulating set of generated files, kept **sorted by path** for determinism.
///
/// Targets and post-processors call [`Artifacts::write`]; the set keeps itself ordered so two runs
/// over unchanged input serialize byte-identically regardless of the order stages ran (the standing
/// determinism invariant). Writing the same path twice replaces the prior contents (last write wins
/// for a given path) — a post-processor rewriting a file in place is the intended use.
#[derive(Debug, Clone, Default)]
pub struct Artifacts {
    /// The files, maintained in ascending `path` order (no map → deterministic iteration).
    files: Vec<Artifact>,
}

impl Artifacts {
    /// An empty artifact set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) the file at `path` with `text`, keeping the set sorted by path.
    ///
    /// A binary search locates the slot: an existing path's contents are overwritten in place; a new
    /// path is inserted at its sorted position. This keeps [`Artifacts::files`] totally ordered after
    /// every write, so the emitted bundle is deterministic without a final sort pass.
    pub fn write(&mut self, path: impl Into<String>, text: impl Into<String>) {
        let path = path.into();
        let text = text.into();
        match self.files.binary_search_by(|a| a.path.cmp(&path)) {
            Ok(index) => {
                // Overwrite an existing path's contents (e.g. a post-processor rewriting a file).
                if let Some(existing) = self.files.get_mut(index) {
                    existing.text = text;
                }
            }
            Err(index) => self.files.insert(index, Artifact { path, text }),
        }
    }

    /// The generated files, sorted by path.
    #[must_use]
    pub fn files(&self) -> &[Artifact] {
        &self.files
    }

    /// Consume the set into its sorted `Vec<Artifact>` (used to build the emitted bundle).
    #[must_use]
    pub fn into_files(self) -> Vec<Artifact> {
        self.files
    }
}

/// A source: source code (or an artifact) → IR (+ diagnostics on the graph).
///
/// The first stage of a pipeline. Built-in: [`builtins::GoGin`]. A user implements this to add a
/// parser for a router/language gnr8 does not ship (see `docs/extensibility.md`).
pub trait Source {
    /// Load the API graph for this source.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] if the source cannot be loaded (e.g. the Go toolchain is missing
    /// or the source fails to parse). Never panics.
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError>;
}

/// A transform: IR → IR, run (in order) on the merged graph before it is frozen for targets.
///
/// This is where everything that used to be a TOML knob lives, as code. Built-ins: [`builtins::SetBasePath`],
/// [`builtins::SetTitle`], [`builtins::ApplySecurity`], [`builtins::RenameOperation`],
/// [`builtins::RenameType`].
pub trait Transform {
    /// Mutate `ir` in place.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] if the transform cannot be applied (e.g. a rename that would
    /// collide). Never panics.
    fn apply(&self, ir: &mut ApiGraph, cx: &Cx) -> Result<(), CoreError>;
}

/// A target: the frozen IR → [`Artifacts`]. Targets get `&ApiGraph` (read-only) — they never mutate
/// the IR, so every target sees the same post-transform model.
///
/// Built-ins: [`builtins::OpenApi31`], [`builtins::GoSdk`]. A user implements this to add an emitter
/// (a second SDK language, a Postman collection, docs — see `docs/extensibility.md`).
pub trait Target {
    /// Generate this target's files into `out`.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] if the IR carries a fact this target cannot represent (a dangling
    /// `$ref`, an unsupported scheme, …) or generation otherwise fails. Never panics.
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, cx: &Cx) -> Result<(), CoreError>;

    /// The project-relative output path(s) this target writes — its **loop-safety anchors**.
    ///
    /// The pipeline excludes any operation/schema/diagnostic whose source provenance lives under one
    /// of these from the analyzed IR, so a target never ingests gnr8's OWN previously-generated output
    /// sitting in the source tree (e.g. a committed `generated/sdk/*.go` Go package). Defaults to
    /// empty (a target that writes nothing the source could re-ingest). This is the framework twin of
    /// the host's `exclude_output_paths` — one loop-safety principle, applied wherever gnr8 generates.
    fn output_anchors(&self) -> Vec<String> {
        Vec::new()
    }
}

/// A post-processor: [`Artifacts`] → [`Artifacts`], run (in order) after all targets and before the
/// host writes. Operates on the in-memory text so the host's ownership/no-op logic still applies.
///
/// Built-in: [`builtins::Header`] (prepend a "generated by gnr8" banner).
pub trait PostProcess {
    /// Rewrite `out` in place.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] if the post-processing step fails. Never panics.
    fn run(&self, out: &mut Artifacts, cx: &Cx) -> Result<(), CoreError>;
}

/// The composed generation pipeline: the user builds this and hands it to [`crate::runner::run`].
///
/// Stages are stored as `Box<dyn …>` (object-safe traits) so a heterogeneous set of built-in and
/// user stages share one type in each ordered vector. The builder methods take `self` by value and
/// return it so calls chain (`Pipeline::new().source(...).transform(...).target(...)`).
#[derive(Default)]
pub struct Pipeline {
    sources: Vec<Box<dyn Source>>,
    transforms: Vec<Box<dyn Transform>>,
    targets: Vec<Box<dyn Target>>,
    posts: Vec<Box<dyn PostProcess>>,
}

impl Pipeline {
    /// An empty pipeline.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a [`Source`] (one is required; multi-source merge is a later stage).
    #[must_use]
    pub fn source(mut self, s: impl Source + 'static) -> Self {
        self.sources.push(Box::new(s));
        self
    }

    /// Append a [`Transform`] (applied in call order).
    #[must_use]
    pub fn transform(mut self, t: impl Transform + 'static) -> Self {
        self.transforms.push(Box::new(t));
        self
    }

    /// Append a [`Target`] (each generates from the same frozen IR).
    #[must_use]
    pub fn target(mut self, t: impl Target + 'static) -> Self {
        self.targets.push(Box::new(t));
        self
    }

    /// Append a [`PostProcess`] (applied in call order, after all targets).
    #[must_use]
    pub fn post(mut self, p: impl PostProcess + 'static) -> Self {
        self.posts.push(Box::new(p));
        self
    }

    /// Run the pipeline through transforms only and return the frozen IR (no targets, no posts).
    ///
    /// The shared front half of [`Pipeline::run`] and the runner's `__inspect` mode: load the single
    /// source, apply every transform in order, and hand back the graph. Kept separate so `__inspect`
    /// can render the post-transform IR without generating artifacts.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] if not exactly one source is configured (multi-source merge is a
    /// documented later stage), or propagates a source/transform error. Never panics.
    pub fn build_ir(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        // Exactly one source for now. >1 is rejected with a clear typed error (not silently merged or
        // first-wins) so the contract is honest until the merge stage lands (docs/extensibility.md §3).
        let source = match self.sources.as_slice() {
            [single] => single,
            [] => {
                return Err(CoreError::Config {
                    message: "pipeline has no source — add exactly one `.source(...)` (e.g. \
                              GoGin::new().inputs([\".\"]))"
                        .to_string(),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "pipeline has {} sources, but merging multiple sources is not yet supported \
                         — configure exactly one `.source(...)`",
                        many.len()
                    ),
                });
            }
        };

        let mut ir = source.load(cx)?;

        // Loop safety (the architectural invariant the host path enforces via `exclude_output_paths`):
        // drop any operation/schema/diagnostic whose source lives under one of THIS pipeline's own
        // target outputs — or under the `.gnr8/` workspace dir — so a target never re-ingests gnr8's
        // own previously-generated output (e.g. a committed `generated/sdk/*.go` Go package in the
        // analyzed module). Anchors are gathered from the targets' declared output paths; the same
        // `crate::lifecycle` exclusion is reused so there is ONE definition, no divergence.
        let mut anchors: Vec<String> = self
            .targets
            .iter()
            .flat_map(|t| t.output_anchors())
            .collect();
        anchors.push(crate::lifecycle::WORKSPACE_DIR.to_string());
        let anchor_refs: Vec<&str> = anchors.iter().map(String::as_str).collect();
        crate::lifecycle::exclude_output_anchors(&mut ir, &anchor_refs);

        for transform in &self.transforms {
            transform.apply(&mut ir, cx)?;
        }
        Ok(ir)
    }

    /// Run the full pipeline: source.load → transforms → freeze → each target.generate → posts.
    ///
    /// Returns the accumulated [`Artifacts`] (sorted by path) plus the diagnostics the IR carried
    /// after transforms. Targets receive the frozen IR by shared reference so none can mutate what a
    /// later target sees.
    ///
    /// # Errors
    ///
    /// Propagates any source/transform/target/post error as its typed [`CoreError`]. Never panics.
    pub fn run(&self, cx: &Cx) -> Result<RunOutcome, CoreError> {
        let ir = self.build_ir(cx)?;
        // Collect diagnostics off the frozen IR (clone so the borrow ends before targets read `ir`).
        let diagnostics: Vec<Diagnostic> = ir.diagnostics.clone();

        let mut artifacts = Artifacts::new();
        for target in &self.targets {
            target.generate(&ir, &mut artifacts, cx)?;
        }
        for post in &self.posts {
            post.run(&mut artifacts, cx)?;
        }
        Ok(RunOutcome {
            artifacts,
            diagnostics,
        })
    }
}

/// The result of a [`Pipeline::run`]: the generated artifacts + the diagnostics collected from the IR.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// The generated files, sorted by path.
    pub artifacts: Artifacts,
    /// Diagnostics carried by the IR after transforms (lossy/unsupported source patterns).
    pub diagnostics: Vec<Diagnostic>,
}

/// The composition surface a `.gnr8/` lifecycle imports: `use gnr8_core::sdk::prelude::*;`.
///
/// Re-exports everything a user composes — [`Pipeline`], the four traits, [`Cx`], [`Artifacts`]/
/// [`Artifact`], every built-in stage, and the public [`crate::graph::SecurityScheme`].
pub mod prelude {
    pub use super::builtins::{
        ApplySecurity, FastApi, Flask, GoGin, GoSdk, Header, NestJs, OpenApi31, OpenApi31Json,
        PySdk, RenameOperation, RenameType, SetBasePath, SetTitle, TsSdk,
    };
    pub use super::layout::SdkFileLayout;
    pub use super::model_style::PyModelStyle;
    pub use super::{Artifact, Artifacts, Cx, Pipeline, PostProcess, Source, Target, Transform};
    pub use crate::graph::SecurityScheme;
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{Artifacts, Cx, Pipeline, Source, Transform};
    use crate::graph::ApiGraph;
    use crate::CoreError;

    /// A source that yields a fixed, empty-but-titled graph without touching Go (no toolchain needed).
    struct StubSource;
    impl Source for StubSource {
        fn load(&self, _cx: &Cx) -> Result<ApiGraph, CoreError> {
            Ok(ApiGraph::default())
        }
    }

    /// A transform that sets the title, to prove transforms run in order on the loaded graph.
    struct StubTitle(&'static str);
    impl Transform for StubTitle {
        fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
            ir.title = self.0.to_string();
            Ok(())
        }
    }

    #[test]
    fn artifacts_stay_sorted_and_overwrite_in_place() {
        let mut a = Artifacts::new();
        a.write("b.go", "B1");
        a.write("a.go", "A");
        a.write("c.go", "C");
        a.write("b.go", "B2"); // overwrite
        let paths: Vec<&str> = a.files().iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["a.go", "b.go", "c.go"], "sorted by path");
        let b = a.files().iter().find(|f| f.path == "b.go").unwrap();
        assert_eq!(b.text, "B2", "last write wins for a given path");
    }

    #[test]
    fn pipeline_requires_exactly_one_source() {
        let cx = Cx::new(std::env::temp_dir());
        // Zero sources → typed Config error.
        let zero = Pipeline::new().run(&cx);
        assert!(matches!(zero, Err(CoreError::Config { .. })), "{zero:?}");
        // Two sources → typed Config error (no silent merge).
        let two = Pipeline::new()
            .source(StubSource)
            .source(StubSource)
            .build_ir(&cx);
        assert!(matches!(two, Err(CoreError::Config { .. })), "{two:?}");
    }

    #[test]
    fn build_ir_runs_transforms_in_order() {
        let cx = Cx::new(std::env::temp_dir());
        let ir = Pipeline::new()
            .source(StubSource)
            .transform(StubTitle("First"))
            .transform(StubTitle("Second"))
            .build_ir(&cx)
            .unwrap();
        // The later transform wins → ordered application.
        assert_eq!(ir.title, "Second");
    }
}
