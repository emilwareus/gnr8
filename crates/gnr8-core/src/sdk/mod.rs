//! Host-side SDK surface: the composition types re-exported from the thin `gnr8` crate, plus the
//! filesystem-facing rules only the writer needs.
//!
//! The four stage traits, `Pipeline`, `Cx`, `Artifacts` and every built-in declaration live in the
//! published `gnr8` SDK, so a user's `.gnr8/` crate and the host share one definition. Re-exporting
//! them here keeps `crate::sdk::…` valid throughout the engine.
//!
//! What is genuinely host-only lives below: artifact-path portability (a property of the filesystem
//! the host writes to, not of the stage that produced the path), the content-stamp helpers the
//! source caches key on, and OpenAPI artifact validation for `gnr8 doctor`.

// User-facing prose dense with proper nouns/acronyms (IR, OpenAPI, SDK, TOML, Gin, ...); backticking
// them all would hurt readability. Allow `doc_markdown` module-wide.
#![allow(clippy::doc_markdown)]

pub mod builtins;
pub use builtins::{PostExec, SourceExec, TargetExec, TransformExec};
pub mod bundle;
pub mod docs;
pub(crate) mod emit_common;
pub mod model;
pub(crate) mod openapi_source;

pub use gnr8::sdk::{
    layout, model_style, prelude, stage, Artifact, ArtifactMetadata, ArtifactOwnership,
    ArtifactRewrite, Artifacts, BuiltinPost, BuiltinSource, BuiltinTarget, BuiltinTransform,
    Custom, Cx, FileStamp, Pipeline, PostProcess, PostStage, ReadinessKind, ReadinessTarget,
    Source, SourceStage, StagePlan, TargetStage, TransformStage,
};
pub use gnr8::sdk::{Target, Transform};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

use crate::manifest::blake3_hex;
use crate::CoreError;

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

/// Validate every artifact path in a completed set against the filesystems gnr8 supports.
///
/// A stage — built-in or user-written, in this process or in a worker — proposes paths. Whether a
/// path is *writable everywhere* is a property of the host's filesystem, so the rule lives here and
/// runs once, over the finished set, before anything reaches the writer. Paths must use NFC and `/`
/// separators; each component is at most 255 UTF-8 bytes and UTF-16 code units and excludes
/// control/Windows-invalid characters, trailing dots/spaces, Windows device names, and gnr8's
/// state/transaction namespace. Unicode case-fold-equivalent paths collide, so one bundle behaves
/// identically on case-sensitive and case-insensitive filesystems.
///
/// # Errors
///
/// Returns [`CoreError::ArtifactOwnership`] naming the offending path and its producer.
pub fn validate_artifact_paths(artifacts: &[Artifact]) -> Result<(), CoreError> {
    let mut seen: std::collections::BTreeMap<String, &str> = std::collections::BTreeMap::new();
    for artifact in artifacts {
        let identity = portable_path_identity(&artifact.path).map_err(|reason| {
            CoreError::ArtifactOwnership {
                code: "artifact.path_invalid".to_string(),
                path: artifact.path.clone(),
                producer: artifact.producer.clone(),
                message: format!("artifact path is not portable: {reason}"),
            }
        })?;
        if let Some(owner) = seen.insert(identity, artifact.producer.as_str()) {
            return Err(CoreError::ArtifactOwnership {
                code: "artifact.path_collision".to_string(),
                path: artifact.path.clone(),
                producer: artifact.producer.clone(),
                message: format!(
                    "path resolves to the same portable output identity as another artifact owned \
                     by {owner}; use one canonical path"
                ),
            });
        }
    }
    Ok(())
}
