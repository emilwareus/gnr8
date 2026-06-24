//! Subprocess driver for the `goextract` Go helper (CONTEXT D-02/D-03).
//!
//! Runs `go run . <target_dir>` from the `goextract` module directory, capturing
//! stdout/stderr/exit status, and deserializes stdout into [`facts::GoFacts`].
//! Every failure mode maps to a typed [`CoreError`] and is propagated with `?` —
//! there is no `unwrap`/`expect`/`panic` here, so a missing toolchain or malformed
//! output never crashes the library (GO-06 / RUST-04 / Pitfall 6).
//!
//! Security (threat T-02-01): `target_dir` is passed as a DISCRETE `Command`
//! argument, never interpolated into a shell string — there is no `sh -c`.

// The driver is the Rust↔Go contract surface for 02-01. Its production consumer is
// `analyze::build_graph`, which 02-03 implements; until then `run_goextract` and
// `goextract_dir` are exercised only by the unit tests below. Allow dead_code so the
// clippy `-D warnings` gate stays green this wave without masking a real signal.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

use crate::analyze::facts;
use crate::CoreError;

/// The directory of the `goextract` Go module, resolved relative to this crate's
/// manifest dir (single source of truth for the path). Mirrors how the contract
/// tests resolve `FIXTURE_DIR` (see `crates/gnr8-core/tests/snapshot_graph.rs`).
pub(crate) fn goextract_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../goextract"))
}

/// Resolve `target_dir` to a CANONICAL absolute path.
///
/// Two reasons (both load-bearing for correctness + determinism, GRAPH-02):
/// 1. The helper subprocess runs with `current_dir(goextract_dir())`, so a RELATIVE `target_dir`
///    (e.g. `fixtures/goalservice` typed at the repo root) would otherwise be interpreted relative to
///    `goextract/` and fail. Absolutizing against the caller's cwd makes relative inspect paths work.
/// 2. The helper emits CANONICAL absolute file paths in spans/diagnostics (Go resolves `..` and
///    symlinks). For `from_facts`/`collect` to strip that prefix, the module root we hand them must be
///    canonical too — otherwise a root like `<manifest>/../../fixtures/goalservice` (the contract
///    tests') would not prefix-match and the machine-absolute path would leak into the snapshot.
///
/// Falls back to the lexical join (or the raw input) if canonicalization fails — e.g. a non-existent
/// target, which the helper then reports as a typed error rather than this function panicking
/// (RUST-04). Canonicalizing a path that exists is the common case (the fixture + any real target).
pub(crate) fn resolve_target(target_dir: &str) -> String {
    let path = std::path::Path::new(target_dir);
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical.to_string_lossy().into_owned();
    }
    if path.is_absolute() {
        return target_dir.to_string();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path).to_string_lossy().into_owned(),
        Err(_) => target_dir.to_string(),
    }
}

/// Run the `goextract` helper against `target_dir` and return the parsed facts.
///
/// # Errors
///
/// - [`CoreError::GoToolchainMissing`] if the `go` binary cannot be spawned.
/// - [`CoreError::HelperExit`] if the helper exits non-zero (carries stderr).
/// - [`CoreError::FactsParse`] if stdout is not the expected JSON facts document.
pub(crate) fn run_goextract(target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    run_goextract_with("go", target_dir)
}

/// Inner driver parameterized on the Go binary name so tests can force a missing
/// binary (toolchain-missing path) without mutating the process `PATH`.
fn run_goextract_with(go_bin: &str, target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    let output = Command::new(go_bin)
        // `run`, `.`, and the target dir are DISCRETE args (no shell, no interpolation).
        .args(["run", ".", target_dir])
        .current_dir(goextract_dir())
        .output()
        .map_err(|source| CoreError::GoToolchainMissing { source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(CoreError::HelperExit {
            code: output.status.code(),
            stderr,
        });
    }

    let parsed: facts::GoFacts = serde_json::from_slice(&output.stdout)
        .map_err(|source| CoreError::FactsParse { source })?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5);
    // scope the allow so the workspace-wide RUST-04 deny stays intact for prod code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{goextract_dir, run_goextract_with};
    use crate::CoreError;

    mod goextract_dir {
        use super::goextract_dir;

        #[test]
        fn resolves_a_path_ending_in_goextract() {
            let dir = goextract_dir();
            assert!(
                dir.ends_with("goextract"),
                "expected the resolved dir to end in 'goextract', got {dir:?}"
            );
        }
    }

    mod run_goextract {
        use super::{run_goextract_with, CoreError};

        #[test]
        fn returns_go_toolchain_missing_when_binary_absent() {
            // A binary name that cannot exist on PATH forces the spawn to fail with
            // an io::Error -> GoToolchainMissing, NOT a panic (GO-06).
            let result = run_goextract_with("gnr8-nonexistent-go-binary-xyz", "/some/target/dir");
            let err = result.unwrap_err();
            assert!(
                matches!(err, CoreError::GoToolchainMissing { .. }),
                "expected GoToolchainMissing, got {err:?}"
            );
            // Display must render without panic and mention the toolchain.
            assert!(err.to_string().contains("Go toolchain"));
        }
    }
}
