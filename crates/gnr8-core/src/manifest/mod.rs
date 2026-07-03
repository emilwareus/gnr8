//! The ownership manifest — a blake3-hashed record of every file gnr8 generated, plus the
//! content-hashing primitive the lifecycle uses for no-op detection (WS-04, D-04, D-05).
//!
//! The manifest maps each generated output path → the blake3 content hash gnr8 last wrote there,
//! with a `source` provenance tag (currently always `"generated"` — the host owns all artifacts
//! uniformly post-pivot). It is persisted as
//! `.gnr8/cache/manifest.json` (git-ignored), with `files` sorted by path so the JSON is a
//! deterministic, reviewable diff (mirrors the graph's sorted-collection policy, GRAPH-02).
//!
//! ## Why blake3 (not `std::hash::DefaultHasher`)
//!
//! The manifest is *persisted state*. `DefaultHasher` is a hashmap hasher whose algorithm/seed are
//! NOT guaranteed stable across Rust releases, so a manifest written by one toolchain could
//! mis-compare under another (false "user edited" warnings or false no-ops). blake3 is a fast,
//! collision-resistant, toolchain-stable content fingerprint (RESEARCH Pitfall 4). It is used here
//! purely as a non-secret integrity-by-comparison fingerprint, NOT as a security primitive
//! (T-04-02-SC).
//!
//! ## Graceful degradation (DoS hardening, T-04-02-03)
//!
//! An ABSENT manifest loads as the empty default (first run ⇒ every output is fresh). A CORRUPT or
//! unparseable manifest ALSO loads as the empty default (regenerate-from-scratch) rather than
//! crashing — a tampered/garbage cache file must never panic or mask a destructive write. Only a
//! genuine read I/O error (e.g. permission denied on an existing file) becomes a typed
//! [`crate::CoreError::Manifest`]. No production `unwrap`/`expect`/`panic` (RUST-04).

// These docs are user-facing prose dense with proper nouns/acronyms (blake3, DefaultHasher, DoS,
// JSON, ...); backticking them all would hurt readability. Allow `doc_markdown` module-wide
// (skill ch.2.4, mirrors the scoped allow in workspace/mod.rs).
#![allow(clippy::doc_markdown)]

use std::path::Path;

/// The current on-disk manifest schema version written by [`Manifest::save`].
const MANIFEST_VERSION: u32 = 1;

/// The manifest path relative to the `.gnr8/` workspace dir.
const MANIFEST_REL: &str = "cache/manifest.json";

/// Hash `bytes` into a stable 64-char lowercase hex blake3 digest.
///
/// Same input ⇒ same digest across runs and toolchains (the property that makes no-op detection
/// and user-edit detection correct rather than heuristic). NOT a security primitive — a non-secret
/// content fingerprint used for integrity-by-comparison only.
#[must_use]
pub fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// One generated file's ownership record: its project-relative path, the blake3 content hash gnr8
/// last wrote there, and a `source` provenance tag (currently always `"generated"`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManifestEntry {
    /// The project-relative output path (e.g. `"sdk/client.go"`, `"openapi.yaml"`).
    pub path: String,
    /// The blake3 hex digest of the bytes gnr8 last wrote to `path`.
    pub hash: String,
    /// Generator provenance tag. The host writes a single `"generated"` tag for every artifact (it
    /// owns the child's whole bundle uniformly); reserved for future per-target attribution.
    pub source: String,
}

/// The ownership manifest: a version tag plus the per-file records, sorted by path on save.
///
/// `Default` yields an empty manifest; [`save`](Manifest::save) always writes the current schema
/// version regardless of how the in-memory value was constructed.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    /// The on-disk schema version (written as [`MANIFEST_VERSION`] on save).
    #[serde(default)]
    pub version: u32,
    /// The generated-file records, kept sorted by path for deterministic diffs.
    #[serde(default)]
    pub files: Vec<ManifestEntry>,
}

impl Manifest {
    /// The blake3 hash gnr8 last recorded for `path`, or `None` if `path` is not tracked.
    #[must_use]
    pub fn recorded_hash(&self, path: &str) -> Option<&str> {
        self.files
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
            .ok()
            .map(|idx| self.files[idx].hash.as_str())
    }

    /// Insert or update the record for `path` (hash + provenance), keeping `files` sorted by path.
    ///
    /// An existing entry for `path` is updated in place; a new entry is inserted and the vector is
    /// re-sorted so the manifest stays a deterministic, byte-stable diff.
    pub fn record(&mut self, path: &str, hash: &str, source: &str) {
        match self
            .files
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
        {
            Ok(idx) => {
                let entry = &mut self.files[idx];
                entry.hash = hash.to_string();
                entry.source = source.to_string();
            }
            Err(idx) => self.files.insert(
                idx,
                ManifestEntry {
                    path: path.to_string(),
                    hash: hash.to_string(),
                    source: source.to_string(),
                },
            ),
        }
    }

    /// Drop every entry whose path is not in `current_paths` (D-04: deleting a file from config
    /// drops its manifest entry, so a stale recorded hash never protects a no-longer-generated file).
    pub fn prune_to(&mut self, current_paths: &[String]) {
        let keep: std::collections::HashSet<&str> =
            current_paths.iter().map(String::as_str).collect();
        self.files
            .retain(|entry| keep.contains(entry.path.as_str()));
    }

    fn normalized_files(mut files: Vec<ManifestEntry>) -> Vec<ManifestEntry> {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files
    }

    fn normalized(self) -> Self {
        Self {
            version: MANIFEST_VERSION,
            files: Self::normalized_files(self.files),
        }
    }

    fn empty_current() -> Self {
        Self {
            version: MANIFEST_VERSION,
            files: Vec::new(),
        }
    }

    /// Persist the manifest to `<gnr8_dir>/cache/manifest.json`, creating `cache/` if needed.
    ///
    /// Writes the current schema version and sorts `files` by path before serializing so the JSON
    /// is a deterministic diff (GRAPH-02). I/O and serialization failures map to
    /// [`crate::CoreError::Manifest`] — never a panic.
    ///
    /// # Errors
    ///
    /// Returns [`crate::CoreError::Manifest`] if `cache/` cannot be created, the manifest cannot be
    /// serialized, or the file cannot be written.
    pub fn save(&self, gnr8_dir: &Path) -> Result<(), crate::CoreError> {
        let path = gnr8_dir.join(MANIFEST_REL);
        let cache = path.parent().ok_or_else(|| crate::CoreError::Manifest {
            message: format!("manifest path {} has no parent directory", path.display()),
        })?;
        std::fs::create_dir_all(cache).map_err(|err| crate::CoreError::Manifest {
            message: format!("failed to create {}: {err}", cache.display()),
        })?;

        // Serialize a normalized view: current version + path-sorted entries (deterministic diff).
        let normalized = Manifest {
            version: MANIFEST_VERSION,
            files: Self::normalized_files(self.files.clone()),
        };
        let json = serde_json::to_string_pretty(&normalized).map_err(|err| {
            crate::CoreError::Manifest {
                message: format!("failed to serialize manifest: {err}"),
            }
        })?;

        std::fs::write(&path, json).map_err(|err| crate::CoreError::Manifest {
            message: format!("failed to write {}: {err}", path.display()),
        })
    }
}

/// Load the manifest from `<gnr8_dir>/cache/manifest.json`, degrading gracefully.
///
/// - File ABSENT ⇒ the empty default (version 1) — first run, every output is fresh.
/// - File PRESENT but unparseable/corrupt ⇒ ALSO the empty default (regenerate-from-scratch); a
///   garbage cache must never crash generation (T-04-02-03).
/// - A genuine read I/O error on an existing file (e.g. permission denied) ⇒
///   [`crate::CoreError::Manifest`].
///
/// # Errors
///
/// Returns [`crate::CoreError::Manifest`] only for a real read I/O error (NOT for an absent or
/// corrupt file, both of which yield the empty default). Never panics.
pub fn load(gnr8_dir: &Path) -> Result<Manifest, crate::CoreError> {
    let path = gnr8_dir.join(MANIFEST_REL);
    match std::fs::read(&path) {
        Ok(bytes) => {
            // Corrupt/unparseable cache ⇒ regenerate-from-scratch (empty default), never an error.
            let manifest = serde_json::from_slice::<Manifest>(&bytes)
                .unwrap_or_else(|_| Manifest::empty_current());
            Ok(manifest.normalized())
        }
        // Absent ⇒ graceful empty default (first run).
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::empty_current()),
        // A real I/O error (permission denied, etc.) is a typed error, not a silent empty.
        Err(err) => Err(crate::CoreError::Manifest {
            message: format!("failed to read {}: {err}", path.display()),
        }),
    }
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{blake3_hex, Manifest};

    #[test]
    fn record_inserts_then_updates_in_place_keeping_sorted() {
        let mut manifest = Manifest::default();
        manifest.record("b.go", "1", "sdk");
        manifest.record("a.go", "2", "sdk");
        // Sorted by path after inserts.
        let paths: Vec<&str> = manifest.files.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["a.go", "b.go"]);
        // Update in place (no duplicate entry).
        manifest.record("a.go", "3", "openapi");
        assert_eq!(manifest.files.len(), 2);
        assert_eq!(manifest.recorded_hash("a.go"), Some("3"));
    }

    #[test]
    fn blake3_hex_matches_the_underlying_digest() {
        assert_eq!(blake3_hex(b"x"), blake3::hash(b"x").to_hex().to_string());
    }
}
