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
pub mod docs;
pub(crate) mod emit_common;
pub mod layout;
pub mod model;
pub mod model_style;
pub(crate) mod openapi_source;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

use crate::graph::{ApiGraph, Diagnostic};
use crate::manifest::blake3_hex;
use crate::CoreError;

static ARTIFACT_CACHE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
// Effective target/post configuration may be computed from arbitrary Rust runtime inputs. Until
// stages expose a complete deterministic behavior fingerprint, cross-run artifact reuse is unsafe.
const ARTIFACT_CACHE_SUPPORTED: bool = false;

pub(crate) fn is_internal_transaction_name(name: &str, suffix: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    let Some(token) = normalized
        .strip_prefix(".gnr8-")
        .and_then(|rest| rest.strip_suffix(suffix))
    else {
        return false;
    };
    token.len() == 24 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_reserved_transaction_name(name: &str) -> bool {
    ["-txn", "-building", "-building-lease"]
        .into_iter()
        .any(|suffix| is_internal_transaction_name(name, suffix))
}

/// Return the single portable identity for an artifact path.
///
/// Paths are required to use NFC and `/` separators, and every component must be valid on the
/// filesystems gnr8 supports. The returned identity additionally applies full Unicode case folding,
/// so paths that would alias on case-insensitive or normalization-insensitive filesystems are caught
/// before any output is written.
pub(crate) fn portable_path_identity(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("path is empty".to_string());
    }
    if path.nfc().collect::<String>() != path {
        return Err("path must use Unicode NFC normalization".to_string());
    }
    if path.starts_with('/') || path.ends_with('/') || path.contains('\\') {
        return Err("path must be relative and use canonical `/` separators".to_string());
    }

    let mut identity = Vec::new();
    for (index, component) in path.split('/').enumerate() {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err("path contains an empty, `.` or `..` component".to_string());
        }
        if component.ends_with(['.', ' ']) {
            return Err("path components may not end with a dot or space".to_string());
        }
        if component.len() > 255 || component.encode_utf16().count() > 255 {
            return Err(
                "path components may not exceed 255 UTF-8 bytes or UTF-16 code units".to_string(),
            );
        }
        if component
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        {
            return Err(
                "path contains a character that is not portable across filesystems".to_string(),
            );
        }
        if index == 0 && component.eq_ignore_ascii_case(".gnr8") {
            return Err("the `.gnr8` workspace is reserved for gnr8 state".to_string());
        }
        if is_reserved_transaction_name(component) {
            return Err(format!(
                "path component {component:?} is reserved for generated-output transactions"
            ));
        }

        let basename = component
            .split_once('.')
            .map_or(component, |(basename, _)| basename);
        let upper = basename.to_ascii_uppercase();
        let numbered_device = upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            });
        if matches!(
            upper.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || numbered_device
        {
            return Err(format!(
                "path component {component:?} is a reserved device name"
            ));
        }

        let folded = component.case_fold().collect::<String>();
        identity.push(folded.nfc().collect::<String>());
    }
    Ok(identity.join("/"))
}

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
    /// The target or post-processor that most recently took ownership of this artifact.
    pub producer: String,
    /// How the current producer obtained ownership.
    #[serde(default)]
    pub ownership: ArtifactOwnership,
    /// Every explicit overlay or rewrite, in application order.
    #[serde(default)]
    pub rewrite_chain: Vec<ArtifactRewrite>,
}

impl Artifact {
    /// Construct an artifact for lifecycle APIs outside a running pipeline.
    #[must_use]
    pub fn new(path: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
            producer: "external".to_string(),
            ownership: ArtifactOwnership::Created,
            rewrite_chain: Vec::new(),
        }
    }
}

/// The explicit operation through which an artifact's current producer obtained ownership.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactOwnership {
    /// The path was created when it did not previously exist.
    #[default]
    Created,
    /// An existing path was intentionally replaced in full.
    Overlaid,
    /// An existing path was intentionally transformed in place.
    Rewritten,
}

/// One recorded ownership transition after initial creation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArtifactRewrite {
    /// Whether the transition was an overlay or a rewrite.
    pub ownership: ArtifactOwnership,
    /// Stage that previously owned the artifact.
    pub previous_producer: String,
    /// Stage that applied this transition.
    pub producer: String,
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

/// A content-backed file identity for generation snapshot checks.
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
/// Targets call [`Artifacts::create`], while intentional replacement uses [`Artifacts::overlay`] or
/// [`Artifacts::rewrite`]. The set keeps itself ordered so two runs over unchanged input serialize
/// byte-identically regardless of the order stages ran (the standing determinism invariant).
#[derive(Debug, Clone)]
pub struct Artifacts {
    /// The files, maintained in ascending `path` order (no map → deterministic iteration).
    files: Vec<Artifact>,
    /// The pipeline stage responsible for the next ownership transition.
    current_producer: String,
}

impl Default for Artifacts {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            current_producer: "direct".to_string(),
        }
    }
}

impl Artifacts {
    /// An empty artifact set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new artifact with one portable, project-relative path.
    ///
    /// Paths use NFC-normalized UTF-8 and `/` separators. Each component is at most 255 UTF-8 bytes
    /// and UTF-16 code units, excludes control/Windows-invalid characters, trailing dots/spaces,
    /// Windows device names, and gnr8's state/transaction namespace. Unicode case-fold-equivalent
    /// paths collide so one bundle behaves identically on case-sensitive and insensitive filesystems.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ArtifactOwnership`] when `path` is non-portable or another stage already
    /// owns its portable identity.
    pub fn create(
        &mut self,
        path: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<(), CoreError> {
        let path = path.into();
        let text = text.into();
        let identity = portable_path_identity(&path).map_err(|reason| {
            self.ownership_error(
                "artifact.path_invalid",
                path.clone(),
                format!("artifact path is not portable: {reason}"),
            )
        })?;
        if let Some(existing) = self.files.iter().find(|artifact| {
            portable_path_identity(&artifact.path).is_ok_and(|candidate| candidate == identity)
        }) {
            return Err(self.ownership_error(
                "artifact.path_collision",
                path,
                format!(
                    "path resolves to the same portable output identity as {:?}, owned by {}; use one canonical path",
                    existing.path, existing.producer
                ),
            ));
        }
        match self.files.binary_search_by(|a| a.path.cmp(&path)) {
            Ok(index) => {
                let owner = self
                    .files
                    .get(index)
                    .map_or("unknown", |artifact| artifact.producer.as_str());
                Err(self.ownership_error(
                    "artifact.path_collision",
                    path,
                    format!("path is already owned by {owner}; use overlay or rewrite explicitly"),
                ))
            }
            Err(index) => {
                self.files.insert(
                    index,
                    Artifact {
                        path,
                        text,
                        producer: self.current_producer.clone(),
                        ownership: ArtifactOwnership::Created,
                        rewrite_chain: Vec::new(),
                    },
                );
                Ok(())
            }
        }
    }

    /// Intentionally replace an existing artifact in full and record the ownership transition.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ArtifactOwnership`] when `path` does not exist.
    pub fn overlay(
        &mut self,
        path: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<(), CoreError> {
        let path = path.into();
        let text = text.into();
        let index = self
            .files
            .binary_search_by(|a| a.path.cmp(&path))
            .map_err(|_| {
                self.ownership_error(
                    "artifact.overlay_missing",
                    path.clone(),
                    "overlay requires an existing artifact".to_string(),
                )
            })?;
        self.replace_at(index, text, ArtifactOwnership::Overlaid);
        Ok(())
    }

    /// Transform an existing artifact and record the ownership transition.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ArtifactOwnership`] when `path` does not exist.
    pub fn rewrite<F>(&mut self, path: impl Into<String>, rewrite: F) -> Result<(), CoreError>
    where
        F: FnOnce(&str) -> String,
    {
        let path = path.into();
        let index = self
            .files
            .binary_search_by(|a| a.path.cmp(&path))
            .map_err(|_| {
                self.ownership_error(
                    "artifact.rewrite_missing",
                    path.clone(),
                    "rewrite requires an existing artifact".to_string(),
                )
            })?;
        let text = self
            .files
            .get(index)
            .map(|artifact| rewrite(&artifact.text))
            .ok_or_else(|| {
                self.ownership_error(
                    "artifact.rewrite_missing",
                    path,
                    "rewrite requires an existing artifact".to_string(),
                )
            })?;
        self.replace_at(index, text, ArtifactOwnership::Rewritten);
        Ok(())
    }

    fn replace_at(&mut self, index: usize, text: String, ownership: ArtifactOwnership) {
        if let Some(existing) = self.files.get_mut(index) {
            let previous_producer = existing.producer.clone();
            existing.text = text;
            existing.ownership = ownership;
            existing.producer.clone_from(&self.current_producer);
            existing.rewrite_chain.push(ArtifactRewrite {
                ownership,
                previous_producer,
                producer: self.current_producer.clone(),
            });
        }
    }

    fn ownership_error(&self, code: &str, path: String, message: String) -> CoreError {
        CoreError::ArtifactOwnership {
            code: code.to_string(),
            path,
            producer: self.current_producer.clone(),
            message,
        }
    }

    fn begin_stage(&mut self, producer: String) {
        self.current_producer = producer;
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
        Self {
            files,
            current_producer: "restored-cache".to_string(),
        }
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

    /// Roots that define this source's input surface for in-child snapshot checks.
    ///
    /// Built-in sources with explicit input directories implement this; custom sources are
    /// conservative by default.
    fn cache_input_roots(&self, _cx: &Cx) -> Option<Vec<PathBuf>> {
        None
    }

    /// External tool or project files that complete this source's host-verifiable input surface.
    ///
    /// Toolchain-backed and custom sources must either return every resolved dependency here or
    /// conservatively use the default.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] when the source cannot enumerate its declared dependencies.
    fn verified_noop_input_files(&self, _cx: &Cx) -> Result<Option<Vec<PathBuf>>, CoreError> {
        Ok(None)
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

    /// Project files this transform reads outside the frozen source graph.
    ///
    /// `Some(files)` declares a complete file-backed input surface. Custom transforms are
    /// conservative by default.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] when the transform cannot enumerate its declared inputs.
    fn verified_noop_input_files(&self, _cx: &Cx) -> Result<Option<Vec<PathBuf>>, CoreError> {
        Ok(None)
    }

    /// Project directories whose complete recursive membership this transform reads.
    ///
    /// These roots are rescanned by the host, so additions and removals invalidate a no-op stamp.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] when the transform cannot enumerate its declared roots.
    fn verified_noop_input_roots(&self, _cx: &Cx) -> Result<Option<Vec<PathBuf>>, CoreError> {
        Ok(Some(Vec::new()))
    }
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

    /// Stable producer label recorded on artifacts created by this target.
    fn producer(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

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

    /// Complete project-file input surface for rendering snapshot checks.
    ///
    /// This is separate from [`Target::cache_input_files`] because a custom target must explicitly
    /// opt into being skipped before its code runs. `None` disables that optimization.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] when the target cannot enumerate its declared inputs.
    fn verified_noop_input_files(&self, _cx: &Cx) -> Result<Option<Vec<PathBuf>>, CoreError> {
        Ok(None)
    }

    /// Project directories whose complete recursive membership this target reads.
    ///
    /// The host rescans these roots to catch files added after the child published its bundle.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] when the target cannot enumerate its declared roots.
    fn verified_noop_input_roots(&self, _cx: &Cx) -> Result<Option<Vec<PathBuf>>, CoreError> {
        Ok(Some(Vec::new()))
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

    /// Generated targets that `gnr8 doctor` can validate with a built-in readiness check.
    ///
    /// Output anchors describe ownership and loop safety; they are not necessarily standalone
    /// packages. Targets opt into readiness explicitly so static overlays and support files are not
    /// misclassified from their file extensions.
    fn readiness_targets(&self) -> Vec<ReadinessTarget> {
        Vec::new()
    }
}

/// A generated target that `gnr8 doctor` can validate after the pipeline runs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ReadinessTarget {
    /// The validator to run.
    pub kind: ReadinessKind,
    /// Project-relative artifact path or package directory.
    pub output_path: String,
}

impl ReadinessTarget {
    /// Declare a generated target readiness check.
    #[must_use]
    pub fn new(kind: ReadinessKind, output_path: impl Into<String>) -> Self {
        Self {
            kind,
            output_path: output_path.into(),
        }
    }
}

/// Built-in readiness validators available to generated targets.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessKind {
    /// Parse and validate an OpenAPI artifact.
    #[serde(rename = "openapi")]
    OpenApi,
    /// Compile and vet a generated Go package.
    Go,
    /// Compile and import a generated Python package.
    Python,
    /// Type-check and validate a generated TypeScript package.
    #[serde(rename = "typescript")]
    TypeScript,
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

    /// Stable producer label recorded on artifacts changed by this post-processor.
    fn producer(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

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

    /// Complete project-file input surface for rendering snapshot checks.
    ///
    /// `None` disables the optimization. This is the safe default for custom command-backed or
    /// environment-sensitive post-processors whose behavior cannot be verified from project files.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] when the post-processor cannot enumerate its declared inputs.
    fn verified_noop_input_files(&self, _cx: &Cx) -> Result<Option<Vec<PathBuf>>, CoreError> {
        Ok(None)
    }

    /// Project directories whose complete recursive membership this post-processor reads.
    ///
    /// The host rescans these roots to catch membership changes between generations.
    ///
    /// # Errors
    ///
    /// Returns a typed [`CoreError`] when the post-processor cannot enumerate its declared roots.
    fn verified_noop_input_roots(&self, _cx: &Cx) -> Result<Option<Vec<PathBuf>>, CoreError> {
        Ok(Some(Vec::new()))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtifactCacheInputs {
    pub(crate) complete: bool,
    pub(crate) roots: Vec<String>,
    pub(crate) stamps: Vec<FileStamp>,
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

    /// Readiness checks declared by every target in this pipeline.
    #[must_use]
    pub fn readiness_targets(&self) -> Vec<ReadinessTarget> {
        let mut targets = self
            .targets
            .iter()
            .flat_map(|target| target.readiness_targets())
            .collect::<Vec<_>>();
        targets.sort();
        targets.dedup();
        targets
    }

    /// Source input roots that can be bracketed before and after a child generation run.
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
            let path = cx.project_root.join(root);
            if path.is_file() {
                paths.push(path);
            } else {
                collect_cache_input_files(&path, &mut paths);
            }
        }
        stamp_project_paths(&cx.project_root, &paths).unwrap_or_default()
    }

    /// Rescannable roots and content stamps for all declared pipeline inputs.
    ///
    /// The returned completeness bit is false when any stage lacks a complete file-backed surface;
    /// known inputs are still returned so generation can reject changes during a run.
    ///
    /// # Errors
    ///
    /// Propagates a stage's typed error when its declared inputs cannot be enumerated.
    pub(crate) fn artifact_cache_inputs(&self, cx: &Cx) -> Result<ArtifactCacheInputs, CoreError> {
        let mut complete = true;
        let mut paths = Vec::new();
        let mut roots = Vec::new();
        for source in &self.sources {
            match source.verified_noop_input_files(cx)? {
                Some(inputs) => paths.extend(inputs),
                None => complete = false,
            }
        }
        for transform in &self.transforms {
            match transform.verified_noop_input_files(cx)? {
                Some(inputs) => paths.extend(inputs),
                None => complete = false,
            }
            match transform.verified_noop_input_roots(cx)? {
                Some(input_roots) => roots.extend(input_roots),
                None => complete = false,
            }
        }
        for target in &self.targets {
            match target.verified_noop_input_files(cx)? {
                Some(inputs) => paths.extend(inputs),
                None => complete = false,
            }
            match target.verified_noop_input_roots(cx)? {
                Some(input_roots) => roots.extend(input_roots),
                None => complete = false,
            }
        }
        for post in &self.posts {
            match post.verified_noop_input_files(cx)? {
                Some(inputs) => paths.extend(inputs),
                None => complete = false,
            }
            match post.verified_noop_input_roots(cx)? {
                Some(input_roots) => roots.extend(input_roots),
                None => complete = false,
            }
        }
        roots.sort();
        roots.dedup();
        for root in &roots {
            let mut root_paths = Vec::new();
            if collect_verified_root_files_strict(root, &mut root_paths).is_some() {
                paths.extend(root_paths);
            } else {
                complete = false;
            }
        }
        paths.sort();
        paths.dedup();
        let mut stamps = Vec::new();
        for path in paths {
            if let Some(mut stamp) = stamp_project_paths(&cx.project_root, &[path]) {
                stamps.append(&mut stamp);
            } else {
                complete = false;
            }
        }
        stamps.sort();
        let roots = roots
            .iter()
            .map(|root| project_relative_path(&cx.project_root, root))
            .collect();
        Ok(ArtifactCacheInputs {
            complete,
            roots,
            stamps,
        })
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
        let cache_key = if ARTIFACT_CACHE_SUPPORTED && self.artifact_cache_inputs(cx)?.complete {
            let target_inputs = self.target_cache_input_files(cx)?;
            let post_cache_key = self.post_cache_key(cx)?;
            Some(artifact_cache_key(
                &ir,
                cx,
                &target_inputs,
                &post_cache_key,
            )?)
        } else {
            None
        };
        if let Some(cache_key) = cache_key.as_deref() {
            if compact_cache_hit && artifact_cache_exists(cx, cache_key) {
                return Ok(RunOutcome {
                    artifacts: Artifacts::new(),
                    diagnostics,
                    artifact_cache_key: Some(cache_key.to_string()),
                    artifact_cache_hit: true,
                });
            }
            if let Some(cached) = load_artifact_cache(cx, cache_key) {
                if let Some(metadata) = artifact_metadata(&cached.artifacts) {
                    save_artifact_metadata_cache(cx, cache_key, &metadata);
                }
                return Ok(RunOutcome {
                    artifacts: Artifacts::from_files(cached.artifacts),
                    diagnostics,
                    artifact_cache_key: Some(cache_key.to_string()),
                    artifact_cache_hit: true,
                });
            }
        }

        let mut artifacts = Artifacts::new();
        for (index, target) in self.targets.iter().enumerate() {
            artifacts.begin_stage(format!("target[{index}]:{}", target.producer()));
            target.generate(&ir, &mut artifacts, cx)?;
        }
        for (index, post) in self.posts.iter().enumerate() {
            artifacts.begin_stage(format!("post[{index}]:{}", post.producer()));
            post.run(&mut artifacts, cx)?;
        }
        if let Some(cache_key) = cache_key.as_deref() {
            save_artifact_cache(cx, cache_key, artifacts.files());
        }
        Ok(RunOutcome {
            artifacts,
            diagnostics,
            artifact_cache_key: cache_key,
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
    let cache: ArtifactCache = serde_json::from_slice(&bytes).ok()?;
    artifact_metadata(&cache.artifacts)?;
    Some(cache)
}

fn artifact_cache_exists(cx: &Cx, key: &str) -> bool {
    let Some(cache) = load_artifact_cache(cx, key) else {
        return false;
    };
    let Some(metadata) = load_artifact_metadata_cache(cx, key) else {
        return false;
    };
    artifact_metadata(&cache.artifacts).is_some_and(|expected| expected == metadata.artifacts)
}

/// Load cached artifacts for a child-emitted artifact-cache reference.
#[must_use]
pub fn load_artifact_cache_files(project_root: &Path, key: &str) -> Option<Vec<Artifact>> {
    load_artifact_cache(&Cx::new(project_root.to_path_buf()), key).map(|cache| cache.artifacts)
}

/// Remove one exact disposable artifact-cache entry after a generation snapshot changed.
///
/// This is an internal host/runner seam. The key is validated before it is used as a file name.
///
/// # Errors
///
/// Returns a typed I/O error if a matching cache file exists but cannot be removed.
#[doc(hidden)]
pub fn discard_artifact_cache(root: &Path, key: &str) -> Result<(), CoreError> {
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CoreError::SdkGen {
            message: format!("refusing to discard invalid artifact cache key {key:?}"),
        });
    }
    let cx = Cx::new(root);
    for path in [
        artifact_cache_path(&cx, key),
        artifact_metadata_cache_path(&cx, key),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(CoreError::Io {
                    message: format!("failed to discard artifact cache {}: {err}", path.display()),
                });
            }
        }
    }
    Ok(())
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
    let Some(metadata) = artifact_metadata(artifacts) else {
        return;
    };
    let cache = ArtifactCache {
        artifacts: artifacts.to_vec(),
    };
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        return;
    };
    if publish_artifact_cache_file(&path, &bytes).is_err() {
        return;
    }
    save_artifact_metadata_cache(cx, key, &metadata);
}

fn artifact_metadata(artifacts: &[Artifact]) -> Option<Vec<ArtifactMetadata>> {
    let mut seen = BTreeSet::new();
    let mut previous_path: Option<&str> = None;
    let mut metadata = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        if previous_path.is_some_and(|previous| previous >= artifact.path.as_str()) {
            return None;
        }
        previous_path = Some(&artifact.path);
        let identity = portable_path_identity(&artifact.path).ok()?;
        if !seen.insert(identity) {
            return None;
        }
        metadata.push(ArtifactMetadata {
            path: artifact.path.clone(),
            hash: blake3_hex(artifact.text.as_bytes()),
        });
    }
    Some(metadata)
}

fn publish_artifact_cache_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use fs2::FileExt;

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "artifact cache path has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "artifact cache path has no UTF-8 file name",
            )
        })?;
    let (temporary, mut file) = loop {
        let sequence = ARTIFACT_CACHE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{file_name}.{}-{sequence}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    };
    let published = file
        .lock_exclusive()
        .and_then(|()| file.write_all(bytes))
        .and_then(|()| file.sync_all())
        .and_then(|()| replace_atomic_file(&temporary, path));
    drop(file);
    if published.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    published
}

fn replace_atomic_file(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        atomicwrites::replace_atomic(from, to)
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(from, to)
    }
}

pub(crate) fn cleanup_artifact_cache_temporary_files(project_root: &Path) -> std::io::Result<()> {
    use fs2::FileExt;

    let dir = project_root
        .join(crate::lifecycle::WORKSPACE_DIR)
        .join("cache")
        .join("artifacts");
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
        if !is_artifact_cache_temporary_name(name) || !entry.file_type()?.is_file() {
            continue;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(entry.path())?;
        match file.try_lock_exclusive() {
            Ok(()) => std::fs::remove_file(entry.path())?,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn is_artifact_cache_temporary_name(name: &str) -> bool {
    let Some(body) = name
        .strip_prefix('.')
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    let Some((cache_name, token)) = body.rsplit_once('.') else {
        return false;
    };
    let key = cache_name
        .strip_suffix(".meta.json")
        .or_else(|| cache_name.strip_suffix(".json"));
    let Some(key) = key else {
        return false;
    };
    let Some((pid, sequence)) = token.split_once('-') else {
        return false;
    };
    key.len() == 64
        && key.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !sequence.is_empty()
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
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
    let cache: ArtifactMetadataCache = serde_json::from_slice(&bytes).ok()?;
    let mut seen = BTreeSet::new();
    let mut previous_path: Option<&str> = None;
    for artifact in &cache.artifacts {
        if previous_path.is_some_and(|previous| previous >= artifact.path.as_str())
            || artifact.hash.len() != 64
            || !artifact.hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        previous_path = Some(&artifact.path);
        let identity = portable_path_identity(&artifact.path).ok()?;
        if !seen.insert(identity) {
            return None;
        }
    }
    Some(cache)
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
    let _ = publish_artifact_cache_file(&path, &bytes);
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

fn collect_cache_input_files_strict(dir: &Path, out: &mut Vec<PathBuf>) -> Option<()> {
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
            collect_cache_input_files_strict(&path, out)?;
        } else if kind.is_file() {
            out.push(path);
        } else {
            return None;
        }
    }
    out.sort();
    Some(())
}

fn collect_verified_root_files_strict(dir: &Path, out: &mut Vec<PathBuf>) -> Option<()> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        let kind = entry.file_type().ok()?;
        if kind.is_dir() {
            collect_verified_root_files_strict(&path, out)?;
        } else if kind.is_file() {
            out.push(path);
        } else {
            // The stage may legitimately ignore symlinks or special entries. Such a tree remains
            // generatable, but its membership is not safe for a host-side skip.
            return None;
        }
    }
    out.sort();
    Some(())
}

/// Return the conventional Cargo inputs the host can safely monitor for `.gnr8` no-op decisions.
#[doc(hidden)]
#[must_use]
pub fn cache_config_input_paths(project_root: &Path) -> Option<Vec<PathBuf>> {
    let gnr8_dir = project_root.join(crate::lifecycle::WORKSPACE_DIR);
    let cargo_toml = gnr8_dir.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return None;
    }
    let mut paths = vec![cargo_toml];
    collect_cache_input_files_strict(&gnr8_dir.join("src"), &mut paths)?;
    let cargo_lock = gnr8_dir.join("Cargo.lock");
    if cargo_lock.exists() {
        if !cargo_lock.is_file() {
            return None;
        }
        paths.push(cargo_lock);
    }
    let build_rs = gnr8_dir.join("build.rs");
    if build_rs.exists() {
        if !build_rs.is_file() {
            return None;
        }
        paths.push(build_rs);
    }
    for name in ["rust-toolchain", "rust-toolchain.toml"] {
        let path = project_root.join(name);
        if path.exists() {
            if !path.is_file() {
                return None;
            }
            paths.push(path);
        }
    }
    for name in ["config", "config.toml"] {
        let path = project_root.join(".cargo").join(name);
        if path.exists() {
            if !path.is_file() {
                return None;
            }
            paths.push(path);
        }
    }
    paths.sort();
    Some(paths)
}

pub(crate) fn cache_config_input_stamps(cx: &Cx) -> Option<Vec<FileStamp>> {
    let paths = cache_config_input_paths(&cx.project_root)?;
    stamp_project_paths(&cx.project_root, &paths)
}

pub(crate) fn cache_config_inputs_complete(cx: &Cx) -> bool {
    let gnr8_dir = cx.project_root.join(crate::lifecycle::WORKSPACE_DIR);
    if gnr8_dir.join("build.rs").exists() {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(gnr8_dir.join("Cargo.toml")) else {
        return false;
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return false;
    };
    !toml_contains_key(&value, "path")
        && value
            .get("package")
            .and_then(|package| package.get("build"))
            .is_none_or(|build| build.as_bool() == Some(false))
}

fn toml_contains_key(value: &toml::Value, needle: &str) -> bool {
    match value {
        toml::Value::Table(table) => table
            .iter()
            .any(|(key, value)| key == needle || toml_contains_key(value, needle)),
        toml::Value::Array(values) => values.iter().any(|value| toml_contains_key(value, needle)),
        _ => false,
    }
}

pub(crate) fn cache_tool_input_stamps(cx: &Cx) -> Option<Vec<FileStamp>> {
    let mut paths = vec![std::env::current_exe().ok()?];
    let resource_root = crate::resource::resource_dir().ok()?;
    for sidecar in ["goextract", "pyextract", "tsextract"] {
        collect_cache_input_files_strict(&resource_root.join(sidecar), &mut paths)?;
    }
    paths.sort();
    stamp_project_paths(&cx.project_root, &paths)
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
        ApiOverrides, ApplySecurity, ConfigurePagination, ConfigureSdkRuntime, DiagnosticPolicy,
        DocumentOperation, EnumOrder, FastApi, Flask, FormatCommand, GoGin, GoSdk, GroupOperations,
        Header, MarkIdempotent, NestJs, OpenApi, OpenApi31, OpenApi31Json, OpenApiFieldPatch,
        OpenApiMetadata, OpenApiSchemaPatch, OperationSelector, ParameterOverride, PySdk,
        RenameOperation, RenameType, RequestParameter, RequireOperationDocs, ResponseOverride,
        SdkPackageMetadata, SecurityOverride, SetBasePath, SetEnumOrder,
        SetOperationSuccessResponse, SetSchemaFieldType, SetTitle, StaticFiles, TsSdk,
    };
    pub use super::docs::SdkDocs;
    pub use super::layout::{OperationFileSplit, SdkFileLayout};
    pub use super::model::SdkModel;
    pub use super::model_style::PyModelStyle;
    pub use super::{
        Artifact, ArtifactMetadata, Artifacts, Cx, FileStamp, Pipeline, PostProcess, ReadinessKind,
        ReadinessTarget, Source, Target, Transform,
    };
    pub use crate::graph::{
        DiagnosticCategory, OpenApiContact, OpenApiLicense, OpenApiServer, PaginationMode,
        PaginationTermination, RuntimeHookKind, SchemaUse, SecurityScheme, Type,
    };
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        stamp_project_paths, Artifacts, Cx, Pipeline, ReadinessKind, ReadinessTarget, Source,
        Target, Transform,
    };
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
            out.create(self.dest, text)?;
            Ok(())
        }

        fn cache_input_files(&self, cx: &Cx) -> Result<Vec<std::path::PathBuf>, CoreError> {
            Ok(vec![cx.project_root.join(self.source)])
        }

        fn verified_noop_input_files(
            &self,
            cx: &Cx,
        ) -> Result<Option<Vec<std::path::PathBuf>>, CoreError> {
            self.cache_input_files(cx).map(Some)
        }
    }

    struct ReadinessOnlyTarget {
        kind: ReadinessKind,
        path: &'static str,
    }

    impl Target for ReadinessOnlyTarget {
        fn generate(
            &self,
            _ir: &ApiGraph,
            _out: &mut Artifacts,
            _cx: &Cx,
        ) -> Result<(), CoreError> {
            Ok(())
        }

        fn readiness_targets(&self) -> Vec<ReadinessTarget> {
            vec![ReadinessTarget::new(self.kind, self.path)]
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
    fn artifact_ownership_is_explicit_and_sorted() {
        let mut a = Artifacts::new();
        a.create("b.go", "B1").unwrap();
        a.create("a.go", "A").unwrap();
        a.create("c.go", "C").unwrap();
        let collision = a.create("b.go", "B2").unwrap_err();
        assert!(
            matches!(collision, CoreError::ArtifactOwnership { ref code, .. } if code == "artifact.path_collision")
        );
        a.overlay("b.go", "B2").unwrap();
        let paths: Vec<&str> = a.files().iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["a.go", "b.go", "c.go"], "sorted by path");
        let b = a.files().iter().find(|f| f.path == "b.go").unwrap();
        assert_eq!(b.text, "B2");
        assert_eq!(b.ownership, super::ArtifactOwnership::Overlaid);
        assert_eq!(b.rewrite_chain.len(), 1);
    }

    #[test]
    fn artifact_creation_rejects_portable_path_aliases_and_invalid_names() {
        let mut artifacts = Artifacts::new();
        artifacts.create("models/Straße.ts", "first").unwrap();
        let folded = artifacts.create("models/STRASSE.ts", "second").unwrap_err();
        assert!(
            matches!(folded, CoreError::ArtifactOwnership { ref code, .. } if code == "artifact.path_collision")
        );

        for path in [
            "models/e\u{301}.ts",
            "models/con.ts",
            "models/com0.ts",
            "models/LPT0.ts",
            "models/conin$.ts",
            "models/CONOUT$.ts",
            "models/model.ts.",
            "models/model.ts:stream",
            "models/.gnr8-0123456789abcdef01234567-building-lease",
            ".GNR8-0123456789ABCDEF01234567-TXN/client.ts",
            ".gnr8/cache/manifest.json",
            ".GNR8/cache/artifacts/file.json",
        ] {
            let err = artifacts.create(path, "invalid").unwrap_err();
            assert!(
                matches!(err, CoreError::ArtifactOwnership { ref code, .. } if code == "artifact.path_invalid"),
                "{path}: {err:?}"
            );
        }

        let long_path = format!("models/{}", "a".repeat(256));
        let err = artifacts.create(&long_path, "invalid").unwrap_err();
        assert!(
            matches!(err, CoreError::ArtifactOwnership { ref code, .. } if code == "artifact.path_invalid"),
            "{long_path}: {err:?}"
        );
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
    fn readiness_targets_are_sorted_and_deduplicated() {
        let pipeline = Pipeline::new()
            .target(ReadinessOnlyTarget {
                kind: ReadinessKind::TypeScript,
                path: "generated/z",
            })
            .target(ReadinessOnlyTarget {
                kind: ReadinessKind::OpenApi,
                path: "generated/openapi.yaml",
            })
            .target(ReadinessOnlyTarget {
                kind: ReadinessKind::TypeScript,
                path: "generated/z",
            });

        assert_eq!(
            pipeline.readiness_targets(),
            vec![
                ReadinessTarget::new(ReadinessKind::OpenApi, "generated/openapi.yaml"),
                ReadinessTarget::new(ReadinessKind::TypeScript, "generated/z"),
            ]
        );
    }

    #[test]
    fn readiness_kind_uses_canonical_wire_names() {
        assert_eq!(
            serde_json::to_string(&ReadinessKind::OpenApi).unwrap(),
            "\"openapi\""
        );
        assert_eq!(
            serde_json::to_string(&ReadinessKind::TypeScript).unwrap(),
            "\"typescript\""
        );
    }

    #[test]
    fn artifact_cache_stays_disabled_without_a_complete_behavior_fingerprint() {
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
        assert!(first.artifact_cache_key.is_none());
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
            "pipelines without a complete behavior fingerprint must render normally"
        );
        assert!(second.artifact_cache_key.is_none());
        assert_eq!(second.artifacts.files()[0].text, "two\n");
    }

    #[test]
    fn runtime_derived_target_configuration_cannot_reuse_another_targets_artifacts() {
        let root = temp_project("target-config-cache-disabled");
        std::fs::create_dir_all(root.join("static")).unwrap();
        std::fs::write(root.join("static/runtime.txt"), "stable\n").unwrap();
        let cx = Cx::new(&root);

        let first = Pipeline::new()
            .source(StubSource)
            .target(CopyFileTarget {
                source: "static/runtime.txt",
                dest: "out-a/runtime.txt",
            })
            .run(&cx)
            .unwrap();
        let second = Pipeline::new()
            .source(StubSource)
            .target(CopyFileTarget {
                source: "static/runtime.txt",
                dest: "out-b/runtime.txt",
            })
            .run(&cx)
            .unwrap();

        assert_eq!(first.artifacts.files()[0].path, "out-a/runtime.txt");
        assert_eq!(second.artifacts.files()[0].path, "out-b/runtime.txt");
        assert!(first.artifact_cache_key.is_none());
        assert!(second.artifact_cache_key.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn artifact_cache_temp_cleanup_removes_only_exact_private_names() {
        use fs2::FileExt;

        let root = temp_project("artifact-temp-cleanup");
        let cache = root.join(".gnr8/cache/artifacts");
        std::fs::create_dir_all(&cache).unwrap();
        let key = "a".repeat(64);
        let stale_full = cache.join(format!(".{key}.json.123-4.tmp"));
        let stale_meta = cache.join(format!(".{key}.meta.json.123-5.tmp"));
        let live = cache.join(format!(".{key}.json.123-6.tmp"));
        let unrelated = cache.join(format!(".{key}.json.backup.tmp"));
        std::fs::write(&stale_full, b"stale").unwrap();
        std::fs::write(&stale_meta, b"stale").unwrap();
        std::fs::write(&live, b"live").unwrap();
        std::fs::write(&unrelated, b"keep").unwrap();
        let live_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&live)
            .unwrap();
        live_file.lock_exclusive().unwrap();

        super::cleanup_artifact_cache_temporary_files(&root).unwrap();

        assert!(!stale_full.exists());
        assert!(!stale_meta.exists());
        assert_eq!(std::fs::read(&live).unwrap(), b"live");
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep");
        drop(live_file);
        super::cleanup_artifact_cache_temporary_files(&root).unwrap();
        assert!(!live.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn config_snapshot_tracks_build_rs_but_disables_hot_skip_for_unbounded_cargo_inputs() {
        let root = temp_project("config-input-policy");
        std::fs::create_dir_all(root.join(".gnr8/src")).unwrap();
        std::fs::write(root.join(".gnr8/src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(
            root.join(".gnr8/Cargo.toml"),
            "[package]\nname = \"config-test\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let cx = Cx::new(&root);
        assert!(super::cache_config_inputs_complete(&cx));

        std::fs::write(root.join(".gnr8/build.rs"), "fn main() {}\n").unwrap();
        let paths = super::cache_config_input_paths(&root).unwrap();
        assert!(paths.contains(&root.join(".gnr8/build.rs")));
        assert!(!super::cache_config_inputs_complete(&cx));

        std::fs::remove_file(root.join(".gnr8/build.rs")).unwrap();
        std::fs::write(
            root.join(".gnr8/Cargo.toml"),
            "[package]\nname = \"config-test\"\nversion = \"0.1.0\"\n\n[dependencies]\nlocal = { path = \"../local\" }\n",
        )
        .unwrap();
        assert!(!super::cache_config_inputs_complete(&cx));
        let _ = std::fs::remove_dir_all(root);
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
