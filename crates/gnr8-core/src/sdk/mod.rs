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
pub mod compat;
pub mod docs;
pub(crate) mod emit_common;
pub mod go;
pub mod layout;
pub mod model;
pub mod model_style;
pub(crate) mod openapi_source;
pub mod profile;
pub mod surface;
pub mod typescript;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::graph::{ApiGraph, Diagnostic};
use crate::manifest::blake3_hex;
use crate::CoreError;

/// Validate a generated OpenAPI artifact enough for `gnr8 doctor` readiness.
///
/// This reuses the OpenAPI source parser's JSON/YAML parsing and version detection, then checks local
/// `$ref`s plus operation/schema naming facts that make an emitted document consumable.
///
/// # Errors
///
/// Returns [`CoreError::Config`] when the artifact is not parseable OpenAPI 3.x or has broken local
/// references / unstable names.
pub fn validate_openapi_artifact(text: &str, path: &Path) -> Result<(), CoreError> {
    openapi_source::validate_openapi_artifact(text, path)
}

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

/// A generated file's identity without its full text payload.
///
/// Stored beside the artifact cache so no-op host runs can classify files by path/hash without
/// deserializing megabytes of generated SDK text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArtifactMetadata {
    /// The project-relative output path.
    pub path: String,
    /// The blake3 hash of the generated UTF-8 text bytes.
    pub hash: String,
}

/// A metadata-only file identity for hot no-op checks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct FileStamp {
    /// Project-relative file path.
    pub path: String,
    /// File length in bytes.
    pub len: u64,
    /// File modification timestamp as nanoseconds since the Unix epoch.
    pub modified_ns: u128,
    /// The blake3 hash of the file bytes.
    pub hash: String,
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

    /// Build an artifact set from an already-sorted or unsorted file list, normalizing path order.
    #[must_use]
    pub fn from_files(mut files: Vec<Artifact>) -> Self {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Self { files }
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

    /// Roots that define this source's input surface for host-side hot no-op checks.
    ///
    /// Returning `None` disables the pre-child fast path for this source. Built-in sources with
    /// explicit input directories implement this; custom sources are conservative by default.
    fn cache_input_roots(&self, _cx: &Cx) -> Option<Vec<PathBuf>> {
        None
    }
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

    /// Project files this target reads while generating artifacts.
    ///
    /// These files are folded into the artifact-cache key. Targets that only depend on the frozen IR
    /// and `.gnr8/` config can use the empty default; targets that copy or template project files must
    /// return every concrete source file so cache hits cannot hide changed target inputs.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] when a configured input path is invalid or cannot be enumerated.
    fn cache_input_files(&self, _cx: &Cx) -> Result<Vec<PathBuf>, CoreError> {
        Ok(Vec::new())
    }

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

    /// Extra cache key material for this post-processor.
    ///
    /// Command-backed post-processors include their command line and executable metadata here so
    /// artifact-cache hits cannot hide formatter changes.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] when the post-processor cannot compute stable cache-key input.
    fn cache_key_fragment(&self, _cx: &Cx) -> Result<Vec<u8>, CoreError> {
        Ok(Vec::new())
    }
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

    /// Append a post-write command/hook that rewrites generated artifacts before the host owns them.
    #[must_use]
    pub fn post_write(self, p: impl PostProcess + 'static) -> Self {
        self.post(p)
    }

    /// Project-relative output anchors declared by every target in this pipeline.
    ///
    /// The child includes these in the artifact bundle so the host writer can prune files that used to
    /// be produced by a generated output directory after a split-layout or naming change.
    #[must_use]
    pub fn output_anchors(&self) -> Vec<String> {
        self.targets
            .iter()
            .flat_map(|target| target.output_anchors())
            .collect()
    }

    /// Source input roots that are safe for the host to rescan before a hot no-op child skip.
    #[must_use]
    pub fn cache_input_roots(&self, cx: &Cx) -> Vec<String> {
        let mut roots = Vec::new();
        for source in &self.sources {
            let Some(source_roots) = source.cache_input_roots(cx) else {
                return Vec::new();
            };
            roots.extend(
                source_roots
                    .into_iter()
                    .map(|root| project_relative_path(&cx.project_root, &root)),
            );
        }
        roots.sort();
        roots.dedup();
        roots
    }

    /// File stamps for this pipeline's declared source input roots.
    #[must_use]
    pub fn cache_input_stamps(&self, cx: &Cx) -> Vec<FileStamp> {
        let roots = self.cache_input_roots(cx);
        if roots.is_empty() {
            return Vec::new();
        }
        let mut paths = Vec::new();
        for root in roots {
            collect_cache_input_files(&cx.project_root.join(root), &mut paths);
        }
        stamp_project_paths(&cx.project_root, &paths).unwrap_or_default()
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
        let mut anchors: Vec<String> = self.output_anchors();
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
        self.run_with_cache(cx, false)
    }

    pub(crate) fn run_for_emit(&self, cx: &Cx) -> Result<RunOutcome, CoreError> {
        self.run_with_cache(cx, true)
    }

    fn run_with_cache(&self, cx: &Cx, compact_cache_hit: bool) -> Result<RunOutcome, CoreError> {
        let ir = self.build_ir(cx)?;
        // Collect diagnostics off the frozen IR (clone so the borrow ends before targets read `ir`).
        let diagnostics: Vec<Diagnostic> = ir.diagnostics.clone();
        let target_inputs = self.target_cache_input_files(cx)?;
        let post_cache_key = self.post_cache_key(cx)?;
        let cache_key = artifact_cache_key(&ir, cx, &target_inputs, &post_cache_key)?;
        if compact_cache_hit && artifact_cache_exists(cx, &cache_key) {
            return Ok(RunOutcome {
                artifacts: Artifacts::new(),
                diagnostics,
                artifact_cache_key: Some(cache_key),
                artifact_cache_hit: true,
            });
        }
        if let Some(cached) = load_artifact_cache(cx, &cache_key) {
            return Ok(RunOutcome {
                artifacts: Artifacts::from_files(cached.artifacts),
                diagnostics,
                artifact_cache_key: Some(cache_key),
                artifact_cache_hit: true,
            });
        }

        let mut artifacts = Artifacts::new();
        for target in &self.targets {
            target.generate(&ir, &mut artifacts, cx)?;
        }
        for post in &self.posts {
            post.run(&mut artifacts, cx)?;
        }
        save_artifact_cache(cx, &cache_key, artifacts.files());
        Ok(RunOutcome {
            artifacts,
            diagnostics,
            artifact_cache_key: Some(cache_key),
            artifact_cache_hit: false,
        })
    }

    fn target_cache_input_files(&self, cx: &Cx) -> Result<Vec<PathBuf>, CoreError> {
        let mut files = Vec::new();
        for target in &self.targets {
            files.extend(target.cache_input_files(cx)?);
        }
        files.sort();
        files.dedup();
        Ok(files)
    }

    fn post_cache_key(&self, cx: &Cx) -> Result<Vec<u8>, CoreError> {
        let mut out = Vec::new();
        for post in &self.posts {
            out.extend(post.cache_key_fragment(cx)?);
            out.push(b'\n');
        }
        Ok(out)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ArtifactCache {
    artifacts: Vec<Artifact>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ArtifactMetadataCache {
    artifacts: Vec<ArtifactMetadata>,
}

fn artifact_cache_key(
    ir: &ApiGraph,
    cx: &Cx,
    target_inputs: &[PathBuf],
    post_cache_key: &[u8],
) -> Result<String, CoreError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gnr8-artifact-cache-v3\n");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\n");
    let ir_json = serde_json::to_vec(ir).map_err(|source| CoreError::SdkGen {
        message: format!("failed to serialize IR for artifact cache key: {source}"),
    })?;
    hasher.update(blake3_hex(&ir_json).as_bytes());
    hasher.update(b"\n");
    hasher.update(config_surface_fingerprint(cx).as_bytes());
    hasher.update(b"\n");
    hasher.update(hash_files(target_inputs, &cx.project_root).as_bytes());
    hasher.update(b"\n");
    hasher.update(post_cache_key);
    Ok(hasher.finalize().to_hex().to_string())
}

fn config_surface_fingerprint(cx: &Cx) -> String {
    let mut inputs = Vec::new();
    let gnr8_dir = cx.project_root.join(crate::lifecycle::WORKSPACE_DIR);
    collect_cache_input_files(&gnr8_dir.join("src"), &mut inputs);
    for name in ["Cargo.toml", "Cargo.lock"] {
        let path = gnr8_dir.join(name);
        if path.is_file() {
            inputs.push(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        inputs.push(exe);
    }
    hash_files(&inputs, &cx.project_root)
}

fn load_artifact_cache(cx: &Cx, key: &str) -> Option<ArtifactCache> {
    let path = artifact_cache_path(cx, key);
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn artifact_cache_exists(cx: &Cx, key: &str) -> bool {
    artifact_cache_path(cx, key).is_file()
}

/// Load cached artifacts for a child-emitted artifact-cache reference.
#[must_use]
pub fn load_artifact_cache_files(project_root: &Path, key: &str) -> Option<Vec<Artifact>> {
    load_artifact_cache(&Cx::new(project_root.to_path_buf()), key).map(|cache| cache.artifacts)
}

/// Load cached artifact path/hash metadata for a child-emitted artifact-cache reference.
#[must_use]
pub fn load_artifact_cache_metadata(
    project_root: &Path,
    key: &str,
) -> Option<Vec<ArtifactMetadata>> {
    load_artifact_metadata_cache(&Cx::new(project_root.to_path_buf()), key)
        .map(|cache| cache.artifacts)
}

fn save_artifact_cache(cx: &Cx, key: &str, artifacts: &[Artifact]) {
    let path = artifact_cache_path(cx, key);
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let cache = ArtifactCache {
        artifacts: artifacts.to_vec(),
    };
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        return;
    };
    let _ = std::fs::write(path, bytes);

    let metadata: Vec<ArtifactMetadata> = artifacts
        .iter()
        .map(|artifact| ArtifactMetadata {
            path: artifact.path.clone(),
            hash: blake3_hex(artifact.text.as_bytes()),
        })
        .collect();
    save_artifact_metadata_cache(cx, key, &metadata);
}

fn artifact_cache_path(cx: &Cx, key: &str) -> PathBuf {
    cx.project_root
        .join(crate::lifecycle::WORKSPACE_DIR)
        .join("cache")
        .join("artifacts")
        .join(format!("{key}.json"))
}

fn load_artifact_metadata_cache(cx: &Cx, key: &str) -> Option<ArtifactMetadataCache> {
    let path = artifact_metadata_cache_path(cx, key);
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_artifact_metadata_cache(cx: &Cx, key: &str, artifacts: &[ArtifactMetadata]) {
    let path = artifact_metadata_cache_path(cx, key);
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let cache = ArtifactMetadataCache {
        artifacts: artifacts.to_vec(),
    };
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        return;
    };
    let _ = std::fs::write(path, bytes);
}

fn artifact_metadata_cache_path(cx: &Cx, key: &str) -> PathBuf {
    cx.project_root
        .join(crate::lifecycle::WORKSPACE_DIR)
        .join("cache")
        .join("artifacts")
        .join(format!("{key}.meta.json"))
}

pub(crate) fn collect_cache_input_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
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
            collect_cache_input_files(&path, out);
        } else {
            out.push(path);
        }
    }
    out.sort();
}

pub(crate) fn hash_files(files: &[PathBuf], root: &Path) -> String {
    let mut hasher = blake3::Hasher::new();
    let mut sorted = files.to_vec();
    sorted.sort();
    let mut cache = FileHashCacheState::load(root, FileHashCacheScope::Inputs);
    for path in sorted {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(cache.hash_path(&path).as_bytes());
        hasher.update(b"\0");
    }
    cache.save();
    hasher.finalize().to_hex().to_string()
}

pub(crate) fn hash_project_files(root: &Path, paths: &[String]) -> HashMap<String, Option<String>> {
    let mut cache = FileHashCacheState::load(root, FileHashCacheScope::Outputs);
    let mut out = HashMap::with_capacity(paths.len());
    for path in paths {
        let absolute = root.join(path);
        let hash = absolute.is_file().then(|| cache.hash_path(&absolute));
        out.insert(path.clone(), hash);
    }
    cache.save();
    out
}

/// Build metadata-only stamps for project files.
#[must_use]
pub fn stamp_project_paths(root: &Path, paths: &[PathBuf]) -> Option<Vec<FileStamp>> {
    stamp_project_paths_with_scope(root, paths, FileHashCacheScope::Inputs)
}

/// Build content-hashed stamps for generated output files.
#[must_use]
pub fn stamp_project_output_paths(root: &Path, paths: &[PathBuf]) -> Option<Vec<FileStamp>> {
    stamp_project_paths_with_scope(root, paths, FileHashCacheScope::Outputs)
}

fn stamp_project_paths_with_scope(
    root: &Path,
    paths: &[PathBuf],
    scope: FileHashCacheScope,
) -> Option<Vec<FileStamp>> {
    let mut stamps = Vec::with_capacity(paths.len());
    let mut cache = FileHashCacheState::load(root, scope);
    for path in paths {
        let metadata = path.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }
        let hash = cache.hash_path(path);
        stamps.push(FileStamp {
            path: project_relative_path(root, path),
            len: metadata.len(),
            modified_ns: modified_ns(&metadata),
            hash,
        });
    }
    cache.save();
    stamps.sort();
    Some(stamps)
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct FileHashCache {
    entries: BTreeMap<String, FileHashCacheEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FileHashCacheEntry {
    len: u64,
    modified_ns: u128,
    hash: String,
}

struct FileHashCacheState {
    path: Option<PathBuf>,
    root: PathBuf,
    cache: FileHashCache,
    dirty: bool,
}

#[derive(Clone, Copy)]
enum FileHashCacheScope {
    Inputs,
    Outputs,
}

impl FileHashCacheState {
    fn load(root: &Path, scope: FileHashCacheScope) -> Self {
        let path = file_hash_cache_path(root, scope);
        let cache = path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            root: root.to_path_buf(),
            cache,
            dirty: false,
        }
    }

    fn hash_path(&mut self, path: &Path) -> String {
        let key = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(metadata) = std::fs::metadata(path) else {
            if self.cache.entries.remove(&key).is_some() {
                self.dirty = true;
            }
            return "<missing>".to_string();
        };
        let fingerprint = FileHashFingerprint::from_metadata(&metadata);
        if let Some(entry) = self.cache.entries.get(&key) {
            if entry.len == fingerprint.len && entry.modified_ns == fingerprint.modified_ns {
                return entry.hash.clone();
            }
        }
        let hash = match std::fs::read(path) {
            Ok(bytes) => blake3_hex(&bytes),
            Err(_) => "<missing>".to_string(),
        };
        self.cache.entries.insert(
            key,
            FileHashCacheEntry {
                len: fingerprint.len,
                modified_ns: fingerprint.modified_ns,
                hash: hash.clone(),
            },
        );
        self.dirty = true;
        hash
    }

    fn save(&self) {
        if !self.dirty {
            return;
        }
        let Some(path) = &self.path else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(&self.cache) else {
            return;
        };
        let _ = std::fs::write(path, bytes);
    }
}

struct FileHashFingerprint {
    len: u64,
    modified_ns: u128,
}

impl FileHashFingerprint {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified_ns: modified_ns(metadata),
        }
    }
}

fn modified_ns(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn project_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn file_hash_cache_path(root: &Path, scope: FileHashCacheScope) -> Option<PathBuf> {
    let gnr8_dir = root.join(crate::lifecycle::WORKSPACE_DIR);
    let file_name = match scope {
        FileHashCacheScope::Inputs => "input-file-hashes.json",
        FileHashCacheScope::Outputs => "output-file-hashes.json",
    };
    gnr8_dir
        .is_dir()
        .then(|| gnr8_dir.join("cache").join(file_name))
}

/// The result of a [`Pipeline::run`]: the generated artifacts + the diagnostics collected from the IR.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// The generated files, sorted by path.
    pub artifacts: Artifacts,
    /// Diagnostics carried by the IR after transforms (lossy/unsupported source patterns).
    pub diagnostics: Vec<Diagnostic>,
    /// Artifact cache key for this run, when available.
    pub artifact_cache_key: Option<String>,
    /// Whether target generation was skipped because the artifact cache was already warm.
    pub artifact_cache_hit: bool,
}

/// The composition surface a `.gnr8/` lifecycle imports: `use gnr8::sdk::prelude::*;`.
///
/// Re-exports everything a user composes — [`Pipeline`], the four traits, [`Cx`], [`Artifacts`]/
/// [`Artifact`], every built-in stage, and the public [`crate::graph::SecurityScheme`].
pub mod prelude {
    pub use super::builtins::{
        ApiOverrides, ApplySecurity, EnumOrder, FastApi, Flask, FormatCommand, GoGin, GoSdk,
        GroupOperations, Header, NestJs, OpenApi, OpenApi31, OpenApi31Json, OpenApiFieldPatch,
        OpenApiSchemaAliases, OpenApiSchemaPatch, OperationSelector, PySdk, QueryParam,
        RenameOperation, RenameType, SdkOperationAliases, SdkPackageMetadata, SetBasePath,
        SetEnumOrder, SetOperationSuccessResponse, SetSchemaFieldType, SetTitle, StaticFiles,
        TsSdk,
    };
    pub use super::docs::SdkDocs;
    pub use super::go::{
        GoExecuteCompatibility, GoQuerySetterArgumentPolicy, GoRequestBuilderAliases,
        GoRequestBuilderOperationAliases, GoRequestBuilderScope, QueryTimeFormat,
        RequiredPointerConstructorPolicy,
    };
    pub use super::layout::{OperationFileSplit, SdkFileLayout};
    pub use super::model::SdkModel;
    pub use super::model_style::PyModelStyle;
    pub use super::profile::SdkProfile;
    pub use super::surface::SdkTypeAliases;
    pub use super::typescript::{
        TsBarrelExports, TsCompatibility, TsModelPropertyPolicy, TsNullablePolicy, TsResponsePolicy,
    };
    pub use super::{
        Artifact, ArtifactMetadata, Artifacts, Cx, FileStamp, Pipeline, PostProcess, Source,
        Target, Transform,
    };
    pub use crate::graph::SecurityScheme;
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{stamp_project_paths, Artifacts, Cx, Pipeline, Source, Target, Transform};
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

    struct CopyFileTarget {
        source: &'static str,
        dest: &'static str,
    }

    impl Target for CopyFileTarget {
        fn generate(&self, _ir: &ApiGraph, out: &mut Artifacts, cx: &Cx) -> Result<(), CoreError> {
            let path = cx.project_root.join(self.source);
            let text = std::fs::read_to_string(&path).map_err(|err| CoreError::Io {
                message: format!("failed to read {}: {err}", path.display()),
            })?;
            out.write(self.dest, text);
            Ok(())
        }

        fn cache_input_files(&self, cx: &Cx) -> Result<Vec<std::path::PathBuf>, CoreError> {
            Ok(vec![cx.project_root.join(self.source)])
        }
    }

    fn temp_project(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("gnr8-sdk-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

    #[test]
    fn artifact_cache_key_includes_target_cache_inputs() {
        let root = temp_project("target-input-cache");
        std::fs::create_dir_all(root.join("static")).unwrap();
        std::fs::write(root.join("static/runtime.txt"), "one\n").unwrap();
        let cx = Cx::new(&root);

        let first = Pipeline::new()
            .source(StubSource)
            .target(CopyFileTarget {
                source: "static/runtime.txt",
                dest: "generated/runtime.txt",
            })
            .run(&cx)
            .unwrap();
        assert_eq!(first.artifacts.files()[0].text, "one\n");

        std::fs::write(root.join("static/runtime.txt"), "two\n").unwrap();
        let second = Pipeline::new()
            .source(StubSource)
            .target(CopyFileTarget {
                source: "static/runtime.txt",
                dest: "generated/runtime.txt",
            })
            .run(&cx)
            .unwrap();

        assert!(
            !second.artifact_cache_hit,
            "target input changes must invalidate artifact cache"
        );
        assert_eq!(second.artifacts.files()[0].text, "two\n");
    }

    #[test]
    fn file_stamps_include_content_hashes() {
        let root = temp_project("file-stamp-hash");
        let path = root.join("input.txt");
        std::fs::write(&path, "aaaa").unwrap();
        let first = stamp_project_paths(&root, std::slice::from_ref(&path)).unwrap();

        std::fs::write(&path, "bbbb").unwrap();
        let second = stamp_project_paths(&root, &[path]).unwrap();

        assert_ne!(first[0].hash, second[0].hash);
    }
}
