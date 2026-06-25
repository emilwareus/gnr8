//! Go source analysis seam (Phase 2): reads a Go module and extracts HTTP route facts.
//!
//! Wave 1 (02-01) landed the Rust↔Go contract surface:
//! - [`facts`] — the serde mirror of the `goextract` JSON facts document.
//! - [`helper`] — the `std::process::Command` subprocess driver with typed errors.
//!
//! Wave 3 (02-03) wires them together: [`build_graph`] runs the helper, deserializes the facts, and
//! assembles the router-agnostic [`crate::graph::ApiGraph`] (stable ids, sorted serialization,
//! provenance on every node — GRAPH-01/02, D-07/D-08).

pub(crate) mod facts;
pub(crate) mod helper;

/// The source language of an analyzed target directory.
///
/// Picked by a SINGLE deterministic classification ([`detect_language`]) of the target's files —
/// NOT by which `Source` built-in was used, and NOT by a try-one-then-fall-back-to-the-other chain
/// (CLAUDE.md rule 3). The selected variant routes to exactly one sidecar driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    /// A Go module — routes to [`helper::run_goextract`].
    Go,
    /// A Python package/app — routes to [`helper::run_pyextract`].
    Python,
}

/// Classify the language of a resolved target directory by ONE deterministic file scan.
///
/// This is a single classification, never a fallback chain (CLAUDE.md rule 3 / RESEARCH Pitfall 1):
/// we walk the tree once, recording whether any Go marker (`go.mod` or `*.go`) and/or any Python
/// marker (`*.py`) is present, then decide from those two booleans. We do NOT "try goextract, and on
/// failure try pyextract".
///
/// Decision (deterministic, documented order):
/// - both markers present → a typed [`crate::CoreError::Config`] naming BOTH languages (WR-05): a
///   mixed tree is genuinely ambiguous, so we surface it and let the user scope the source's `inputs`
///   to a single-language subdir rather than silently pick one and extract the wrong (or nothing)
///   from the other. This is still one decision, not a fallback.
/// - only Python markers → [`Lang::Python`].
/// - only Go markers → [`Lang::Go`].
/// - neither (empty / non-existent / unrecognized) → a typed [`crate::CoreError::Config`], never a
///   guessed language (T-02-04-py).
///
/// # Errors
///
/// [`crate::CoreError::Config`] if the target holds BOTH Go and Python source (ambiguous) or no
/// recognizable Go or Python source.
pub(crate) fn detect_language(target_dir: &str) -> Result<Lang, crate::CoreError> {
    let mut has_go = false;
    let mut has_python = false;
    scan_markers(
        std::path::Path::new(target_dir),
        &mut has_go,
        &mut has_python,
    );

    // ONE decision from the two booleans — documented order, no fallback (rule 3).
    match (has_go, has_python) {
        (true, true) => Err(crate::CoreError::Config {
            message: format!(
                "ambiguous source language of {target_dir:?}: found BOTH Go (go.mod/*.go) and \
                 Python (*.py) source — scope the source's inputs to a single-language subdir so \
                 the correct sidecar runs (gnr8 will not guess per-file)"
            ),
        }),
        (true, false) => Ok(Lang::Go),
        (false, true) => Ok(Lang::Python),
        (false, false) => Err(crate::CoreError::Config {
            message: format!(
                "cannot determine source language of {target_dir:?}: no Go (go.mod/*.go) or \
                 Python (*.py) source found — point the source at a Go module or a Python app dir"
            ),
        }),
    }
}

/// Recursively record Go (`go.mod`/`*.go`) and Python (`*.py`) marker presence under `dir`.
///
/// A single tree walk feeding the one [`detect_language`] decision; a directory that cannot be read
/// (permission, non-existent) simply contributes no markers — the caller turns "no markers" into a
/// typed `Config` error, so a bad path is a typed error, not a panic (RUST-04).
fn scan_markers(dir: &std::path::Path, has_go: &mut bool, has_python: &mut bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_markers(&path, has_go, has_python);
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        // Case-insensitive extension comparison (mirrors `sdk::builtins::is_go_file`); `go.mod` is a
        // fixed file name, not an extension match.
        if name == "go.mod" || ext.eq_ignore_ascii_case("go") {
            *has_go = true;
        } else if ext.eq_ignore_ascii_case("py") {
            *has_python = true;
        }
        // Early exit is unnecessary (the trees are small) and would complicate determinism; both
        // booleans are monotonic so a full walk is correct and order-independent.
    }
}

/// Build the router-agnostic [`crate::graph::ApiGraph`] from a Go OR Python fixture/source directory.
///
/// Resolves `fixture_dir` to an absolute target, classifies its language ONCE via [`detect_language`]
/// (one deterministic detector, never a try-Go-then-try-Python fallback — CLAUDE.md rule 3), runs the
/// matching sidecar driver ([`helper::run_goextract`] / [`helper::run_pyextract`]), and maps the SAME
/// neutral facts into the graph ([`crate::graph::ApiGraph::from_facts`], reused unchanged — the v2.0
/// bet). Operation ids are stable, schema ids are qualified, and every collection is sorted so two
/// runs over unchanged source are byte-identical (GRAPH-02).
///
/// # Errors
///
/// - [`crate::CoreError::Config`] if the target's language cannot be determined (empty/ambiguous).
/// - [`crate::CoreError::GoToolchainMissing`] / [`crate::CoreError::PythonToolchainMissing`] if the
///   selected toolchain cannot be spawned.
/// - [`crate::CoreError::HelperExit`] if the sidecar exits non-zero.
/// - [`crate::CoreError::FactsParse`] if the sidecar's stdout is not the expected JSON.
pub fn build_graph(fixture_dir: &str) -> Result<crate::graph::ApiGraph, crate::CoreError> {
    // Resolve to an absolute target so a relative `fixture_dir` works (the helper runs from the
    // sidecar dir) AND the graph relativizes span file paths against the same root the helper saw.
    let target = helper::resolve_target(fixture_dir);
    let facts = match detect_language(&target)? {
        Lang::Python => helper::run_pyextract(&target)?,
        Lang::Go => helper::run_goextract(&target)?,
    };
    Ok(crate::graph::ApiGraph::from_facts(facts, &target))
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // to the test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{detect_language, Lang};
    use crate::CoreError;

    const FASTAPI_FIXTURE_DIR: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/fastapi-bookstore"
    );
    const GOALSERVICE_FIXTURE_DIR: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

    /// `build_graph` against a non-existent fixture dir must return a typed `CoreError` — never a
    /// panic, never `NotYetImplemented`. With language dispatch (02-01) a non-existent dir now
    /// classifies as ambiguous (no Go/Python markers) and surfaces `Config` BEFORE any spawn; on a
    /// machine with the toolchain a real-but-bad target would surface `HelperExit`/`FactsParse`, and
    /// a missing toolchain `GoToolchainMissing`/`PythonToolchainMissing`. Accept all of these.
    #[test]
    fn build_graph_surfaces_typed_error_for_bad_target() {
        let result = super::build_graph("/gnr8-nonexistent-target-dir-xyz");
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                CoreError::Config { .. }
                    | CoreError::GoToolchainMissing { .. }
                    | CoreError::PythonToolchainMissing { .. }
                    | CoreError::HelperExit { .. }
                    | CoreError::FactsParse { .. }
            ),
            "expected a typed dispatch/subprocess error, got {err:?}"
        );
        // It must NOT be the old NotYetImplemented stub.
        assert!(
            !matches!(err, CoreError::NotYetImplemented { .. }),
            "build_graph is implemented; must not return NotYetImplemented"
        );
    }

    /// The single deterministic detector classifies the `FastAPI` fixture (a `*.py` tree) as Python and
    /// the goalservice fixture (a `go.mod`/`*.go` module) as Go — the one decision `build_graph`/
    /// `collect` route on (rule 3). Uses the same `CARGO_MANIFEST_DIR`-relative fixture-path style as
    /// the snapshot tests so it does not depend on the process cwd.
    #[test]
    fn detect_language_classifies_python_and_go_fixtures() {
        assert_eq!(
            detect_language(FASTAPI_FIXTURE_DIR).unwrap(),
            Lang::Python,
            "the fastapi-bookstore fixture is a Python tree"
        );
        assert_eq!(
            detect_language(GOALSERVICE_FIXTURE_DIR).unwrap(),
            Lang::Go,
            "the goalservice fixture is a Go module"
        );
    }

    /// An empty/ambiguous target (no Go or Python source) is a typed `Config` error — never a guessed
    /// language (T-02-04-py). A freshly-created empty temp dir holds neither marker.
    #[test]
    fn detect_language_rejects_an_empty_target_as_config_error() {
        let dir = std::env::temp_dir().join(format!(
            "gnr8-detect-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let result = detect_language(&dir.to_string_lossy());
        // Clean up before asserting so a failure does not leak the temp dir.
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            matches!(result, Err(CoreError::Config { .. })),
            "an empty target must be a typed Config error, got {result:?}"
        );
    }

    /// WR-05 regression: a tree carrying BOTH a `*.go` and a `*.py` marker is ambiguous and must be a
    /// typed `Config` error naming both languages — never a silent pick of Go. A freshly-created temp
    /// dir with one of each marker exercises the `(true, true)` arm.
    #[test]
    fn detect_language_rejects_a_mixed_go_python_tree() {
        let dir = std::env::temp_dir().join(format!(
            "gnr8-detect-mixed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.go"), b"package main\n").unwrap();
        std::fs::write(dir.join("app.py"), b"x = 1\n").unwrap();
        let result = detect_language(&dir.to_string_lossy());
        // Clean up before asserting so a failure does not leak the temp dir.
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            matches!(&result, Err(CoreError::Config { message }) if message.contains("ambiguous")),
            "a mixed Go/Python tree must be a typed Config error naming the ambiguity, got {result:?}"
        );
    }
}
