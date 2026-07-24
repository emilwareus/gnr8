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
use crate::manifest::blake3_hex;
use crate::CoreError;

/// The directory of the `goextract` Go module, resolved relative to this crate's
/// manifest dir (single source of truth for the path). Mirrors how the contract
/// tests resolve `FIXTURE_DIR` (see `crates/gnr8-core/tests/snapshot_graph.rs`).
pub(crate) fn goextract_dir() -> Result<PathBuf, CoreError> {
    Ok(sidecar_root()?.join("goextract"))
}

/// The repo root that HOLDS the `pyextract/` Python package, resolved relative to this crate's
/// manifest dir (single source of truth for the path). The invocation is `python3 -m pyextract`, so
/// the subprocess runs from the dir that CONTAINS `pyextract/` (the repo root), not from inside it —
/// this is the deliberate analog of [`goextract_dir`] one level up. Carries the v1 compile-time-path
/// debt forward without deepening it (CONTEXT decision; RESEARCH A6).
pub(crate) fn pyextract_dir() -> Result<PathBuf, CoreError> {
    sidecar_root()
}

/// The directory of the `tsextract` Node sidecar (it HOLDS `index.js` + `node_modules`), resolved
/// relative to this crate's manifest dir (single source of truth for the path). The invocation is
/// `node index.js <target_dir>`, so the subprocess runs from inside `tsextract/` — exactly the
/// goextract analog one level down (`<root>/tsextract`), NOT the repo root used by [`pyextract_dir`]
/// (which runs `python3 -m pyextract`). Carries the v1 compile-time-path debt forward without
/// deepening it (CONTEXT decision; RESEARCH A6).
pub(crate) fn tsextract_dir() -> Result<PathBuf, CoreError> {
    Ok(sidecar_root()?.join("tsextract"))
}

fn sidecar_root() -> Result<PathBuf, CoreError> {
    crate::resource::resource_dir()
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
/// A missing or unreadable target is rejected here with the original path in the diagnostic. It is
/// never reinterpreted relative to a helper working directory.
pub(crate) fn resolve_target(target_dir: &str) -> Result<String, CoreError> {
    let path = std::path::Path::new(target_dir);
    let canonical = std::fs::canonicalize(path).map_err(|source| CoreError::Config {
        message: format!(
            "target directory '{}' is missing or unreadable: {source}",
            path.display()
        ),
    })?;
    if !canonical.is_dir() {
        return Err(CoreError::Config {
            message: format!("target directory '{}' is not a directory", path.display()),
        });
    }
    Ok(canonical.to_string_lossy().into_owned())
}

/// Run the `goextract` helper against `target_dir` and return the parsed facts.
///
/// # Errors
///
/// - [`CoreError::GoToolchainMissing`] if the `go` binary cannot be spawned.
/// - [`CoreError::HelperExit`] if the helper exits non-zero (carries stderr).
/// - [`CoreError::FactsParse`] if stdout is not the expected JSON facts document.
pub(crate) fn run_goextract(target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    run_goextract_with("go", target_dir, &[], &[])
}

/// Run the `goextract` helper against `target_dir`, with separate route and schema scopes.
pub(crate) fn run_goextract_package_scopes(
    target_dir: &str,
    route_patterns: &[String],
    schema_patterns: &[String],
) -> Result<facts::GoFacts, CoreError> {
    run_goextract_with("go", target_dir, route_patterns, schema_patterns)
}

/// Inner driver parameterized on the Go binary name so tests can force a missing
/// binary (toolchain-missing path) without mutating the process `PATH`.
fn run_goextract_with(
    go_bin: &str,
    target_dir: &str,
    route_patterns: &[String],
    schema_patterns: &[String],
) -> Result<facts::GoFacts, CoreError> {
    let mut cmd = if go_bin == "go" {
        Command::new(goextract_binary(go_bin)?)
    } else {
        let mut cmd = Command::new(go_bin);
        // Tests that pass a fake Go binary exercise the legacy `go run` shape so missing-toolchain
        // categorization stays simple and explicit.
        cmd.args(["run", "."]);
        cmd
    };
    let dir = checked_sidecar_dir("goextract", goextract_dir()?)?;
    cmd.arg(target_dir);
    if route_patterns == schema_patterns {
        cmd.args(route_patterns);
    } else {
        for pattern in route_patterns {
            cmd.args(["--route-package", pattern]);
        }
        for pattern in schema_patterns {
            cmd.args(["--schema-package", pattern]);
        }
    }
    cmd.current_dir(dir);
    let output = cmd
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

fn goextract_binary(go_bin: &str) -> Result<PathBuf, CoreError> {
    let root = checked_sidecar_dir("goextract", goextract_dir()?)?;
    let source_hash = goextract_source_hash(&root)?;
    let dir = std::env::temp_dir()
        .join("gnr8-goextract")
        .join(source_hash);
    let binary = dir.join(if cfg!(windows) {
        "goextract.exe"
    } else {
        "goextract"
    });
    if binary.is_file() {
        return Ok(binary);
    }

    std::fs::create_dir_all(&dir).map_err(|source| CoreError::Io {
        message: format!(
            "failed to create goextract cache dir {}: {source}",
            dir.display()
        ),
    })?;
    let output = Command::new(go_bin)
        .args(["build", "-o"])
        .arg(&binary)
        .arg(".")
        .current_dir(&root)
        .output()
        .map_err(|source| CoreError::GoToolchainMissing { source })?;
    if !output.status.success() {
        return Err(CoreError::HelperExit {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(binary)
}

fn checked_sidecar_dir(label: &str, path: PathBuf) -> Result<PathBuf, CoreError> {
    if path.is_dir() {
        return Ok(path);
    }
    Err(CoreError::Io {
        message: format!(
            "{label} resource directory is missing or unreadable at {} — reinstall gnr8 or set {} to the release resource root",
            path.display(),
            crate::resource::GNR8_RESOURCE_DIR_ENV
        ),
    })
}

pub(crate) fn goextract_source_hash(root: &std::path::Path) -> Result<String, CoreError> {
    let mut files = Vec::new();
    collect_goextract_source_files(root, &mut files)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gnr8-goextract-binary-cache-v1\n");
    for path in files {
        let rel = path.strip_prefix(root).map_err(|source| CoreError::Io {
            message: format!(
                "goextract source {} is outside declared helper root {}: {source}",
                path.display(),
                root.display()
            ),
        })?;
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        let bytes = std::fs::read(&path).map_err(|source| CoreError::Io {
            message: format!(
                "failed to read goextract source {} for helper cache key: {source}",
                path.display()
            ),
        })?;
        hasher.update(blake3_hex(&bytes).as_bytes());
        hasher.update(b"\0");
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn collect_goextract_source_files(
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), CoreError> {
    let entries = std::fs::read_dir(dir).map_err(|source| CoreError::Io {
        message: format!(
            "failed to read declared goextract source directory {}: {source}",
            dir.display()
        ),
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CoreError::Io {
            message: format!(
                "failed to enumerate declared goextract source directory {}: {source}",
                dir.display()
            ),
        })?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.is_dir() {
            if matches!(name, ".git" | "target" | "vendor") {
                continue;
            }
            collect_goextract_source_files(&path, out)?;
            continue;
        }
        if name == "go.mod"
            || name == "go.sum"
            || path.extension().and_then(|ext| ext.to_str()) == Some("go")
        {
            out.push(path);
        }
    }
    out.sort();
    Ok(())
}

/// Run the `pyextract` Python helper against `target_dir` and return the parsed facts.
///
/// The Python twin of [`run_goextract`]: spawns `python3 -m pyextract <target_dir>` from
/// [`pyextract_dir`] (the repo root that holds the `pyextract/` package), capturing
/// stdout/stderr/exit status, and deserializes stdout into the SAME neutral [`facts::GoFacts`] DTO
/// (the contract is language-agnostic; the `Go` in the type name is historical). Every failure mode
/// maps to a typed [`CoreError`] and is propagated with `?` — never a panic (RUST-04 / T-02-02-py).
///
/// # Errors
///
/// - [`CoreError::PythonToolchainMissing`] if the `python3` binary cannot be spawned.
/// - [`CoreError::HelperExit`] if the helper exits non-zero (carries stderr).
/// - [`CoreError::FactsParse`] if stdout is not the expected JSON facts document.
pub(crate) fn run_pyextract(target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    run_pyextract_with("python3", target_dir)
}

/// Inner driver parameterized on the Python binary name so tests can force a missing binary
/// (toolchain-missing path) without mutating the process `PATH` — mirrors [`run_goextract_with`].
fn run_pyextract_with(py_bin: &str, target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    let dir = checked_sidecar_dir("pyextract", pyextract_dir()?)?;
    let output = Command::new(py_bin)
        // `-m`, `pyextract`, and the target dir are DISCRETE args (no shell, no interpolation of
        // `target_dir` into a single string) — threat T-02-01-py, mirroring the goextract control.
        .args(["-m", "pyextract", target_dir])
        .current_dir(dir)
        .output()
        .map_err(|source| CoreError::PythonToolchainMissing { source })?;

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

/// Run the `tsextract` Node helper against `target_dir` and return the parsed facts.
///
/// The TypeScript twin of [`run_goextract`]/[`run_pyextract`]: spawns `node index.js <target_dir>`
/// from [`tsextract_dir`] (the dir that holds `index.js` + `node_modules`), capturing
/// stdout/stderr/exit status, and deserializes stdout into the SAME neutral [`facts::GoFacts`] DTO
/// (the contract is language-agnostic; the `Go` in the type name is historical). Every failure mode
/// maps to a typed [`CoreError`] and is propagated with `?` — never a panic (RUST-04 / T-04-02).
///
/// # Errors
///
/// - [`CoreError::TypeScriptToolchainMissing`] if the `node` binary cannot be spawned.
/// - [`CoreError::HelperExit`] if the helper exits non-zero (carries stderr).
/// - [`CoreError::FactsParse`] if stdout is not the expected JSON facts document.
pub(crate) fn run_tsextract(target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    run_tsextract_with("node", target_dir)
}

/// Inner driver parameterized on the Node binary name so tests can force a missing binary
/// (toolchain-missing path) without mutating the process `PATH` — mirrors [`run_pyextract_with`].
fn run_tsextract_with(node_bin: &str, target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    let dir = checked_sidecar_dir("tsextract", tsextract_dir()?)?;
    let output = Command::new(node_bin)
        // `index.js` and the target dir are DISCRETE args (no shell, no interpolation of
        // `target_dir` into a single string) — threat T-04-01, mirroring the goextract control.
        .args(["index.js", target_dir])
        .current_dir(dir)
        .output()
        .map_err(|source| CoreError::TypeScriptToolchainMissing { source })?;

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

/// Health-probe whether the TypeScript toolchain is ACTUALLY ready for `target_dir` (WR-02): both
/// `node` runs AND the user's `typescript` is resolvable, using the EXACT resolution `run_tsextract`
/// uses at generate time (`tsextract/probe.js` calls the SAME `ts.resolveTypescript`, so there is one
/// source of truth — no second detector, no fallback; CLAUDE.md rule 3). Returns `true` iff the probe
/// exits 0.
///
/// `gnr8 doctor` calls this so a TS project with `node` but no `typescript` reports UNHEALTHY up front,
/// rather than passing doctor and failing at `generate`. A spawn error (no `node`) or a non-zero exit
/// (typescript absent) both mean "not ready" → `false`; never a panic (the doctor renders it as a
/// finding). Spawned with DISCRETE args from `tsextract_dir`, never `sh -c` (T-06-01).
pub(crate) fn typescript_toolchain_present(target_dir: &str) -> Result<bool, CoreError> {
    let dir = checked_sidecar_dir("tsextract", tsextract_dir()?)?;
    Ok(Command::new("node")
        .args(["probe.js", target_dir])
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success()))
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5);
    // scope the allow so the workspace-wide RUST-04 deny stays intact for prod code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{
        goextract_dir, pyextract_dir, resolve_target, run_goextract_with, run_pyextract_with,
        run_tsextract_with, tsextract_dir, typescript_toolchain_present,
    };
    use crate::CoreError;

    #[test]
    fn missing_target_is_an_explicit_error() {
        let missing =
            std::env::temp_dir().join(format!("gnr8-missing-target-{}", std::process::id()));
        let error = resolve_target(&missing.to_string_lossy()).unwrap_err();
        assert!(
            error.to_string().contains("target directory"),
            "unexpected diagnostic: {error}"
        );
    }

    mod goextract_dir {
        use super::goextract_dir;

        #[test]
        fn resolves_a_path_ending_in_goextract() {
            let dir = goextract_dir().unwrap();
            assert!(
                dir.ends_with("goextract"),
                "expected the resolved dir to end in 'goextract', got {dir:?}"
            );
        }
    }

    mod pyextract_dir {
        use super::{goextract_dir, pyextract_dir};

        #[test]
        fn resolves_to_the_repo_root_that_holds_pyextract() {
            // `pyextract_dir` is the repo root that CONTAINS `pyextract/` (invocation is
            // `python3 -m pyextract`). It is exactly the parent of `goextract_dir` (which points
            // one level deeper, at `<root>/goextract`). Canonicalize both so the `/../..` lexical
            // segments resolve, then assert the parent relationship holds.
            let py_root = std::fs::canonicalize(pyextract_dir().unwrap())
                .expect("pyextract_dir should resolve to an existing repo root");
            let go_dir = std::fs::canonicalize(goextract_dir().unwrap())
                .expect("goextract_dir should resolve to an existing dir");
            assert_eq!(
                go_dir.parent(),
                Some(py_root.as_path()),
                "pyextract_dir ({py_root:?}) must be the parent of goextract_dir ({go_dir:?})"
            );
            // And it actually holds the `pyextract/` package dir once that lands.
            // (Asserted lazily: the path string must end at the repo root, not inside goextract.)
            assert!(
                !py_root.ends_with("goextract"),
                "pyextract_dir must be the repo root, not the goextract dir: {py_root:?}"
            );
        }
    }

    mod tsextract_dir {
        use super::{goextract_dir, tsextract_dir};

        #[test]
        fn resolves_a_sibling_of_goextract_ending_in_tsextract() {
            // `tsextract_dir` points one level down at `<root>/tsextract` (the dir holding
            // `index.js` + `node_modules`), exactly like `goextract_dir` points at `<root>/goextract`
            // — they are siblings. Compare lexically (the dir need not exist yet for this assertion).
            let ts_dir = tsextract_dir().unwrap();
            assert!(
                ts_dir.ends_with("tsextract"),
                "expected the resolved dir to end in 'tsextract', got {ts_dir:?}"
            );
            assert_eq!(
                ts_dir.parent(),
                goextract_dir().unwrap().parent(),
                "tsextract_dir and goextract_dir must be siblings under the repo root"
            );
        }
    }

    mod run_tsextract {
        use super::{run_tsextract_with, CoreError};

        #[test]
        fn returns_typescript_toolchain_missing_when_binary_absent() {
            // A binary name that cannot exist on PATH forces the spawn to fail with an io::Error
            // -> TypeScriptToolchainMissing, NOT a panic (T-04-02). Forced via the `_with` split so
            // we never mutate the process PATH.
            let result = run_tsextract_with("gnr8-nonexistent-node-binary-xyz", "/some/target/dir");
            let err = result.unwrap_err();
            assert!(
                matches!(err, CoreError::TypeScriptToolchainMissing { .. }),
                "expected TypeScriptToolchainMissing, got {err:?}"
            );
            // Display must render without panic and mention the toolchain.
            assert!(err.to_string().contains("TypeScript toolchain"));
        }
    }

    mod typescript_toolchain_probe {
        use super::{tsextract_dir, typescript_toolchain_present};

        /// WR-02: `typescript_toolchain_present` returns `true` when `typescript` IS resolvable — here
        /// from the sidecar's own dev `node_modules` (restored by `make tsextract-deps`, exactly the
        /// gnr8 test-suite contract). Skips gracefully if those dev deps are not installed so the unit
        /// run never fails on a machine without `npm ci` (the `examples-check` gate covers the wired
        /// end-to-end path). The nestjs fixture is a valid TS target dir to point the probe at.
        #[test]
        fn reports_present_when_typescript_resolves_from_the_sidecar() {
            if !tsextract_dir()
                .unwrap()
                .join("node_modules")
                .join("typescript")
                .is_dir()
            {
                eprintln!(
                    "skipping: tsextract/node_modules/typescript absent (run `make tsextract-deps`)"
                );
                return;
            }
            let nestjs = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/nestjs-bookstore"
            );
            assert!(
                typescript_toolchain_present(nestjs).unwrap(),
                "with the sidecar's dev typescript installed, the TS toolchain probe must report present"
            );
        }

        /// WR-02: the probe reports ABSENT (no panic) when `typescript` cannot resolve from EITHER the
        /// target or the sidecar. Forced deterministically by pointing the probe at a target dir that
        /// is `node`-resolvable but holds no `typescript`, while the sidecar's own `node_modules` is the
        /// only other search root — so this asserts the not-found exit path maps to `false` rather than
        /// a spawn-success masking a missing toolchain. (A bogus target dir with no `node_modules`; the
        /// sidecar may still resolve it, so this test only asserts the call never panics and returns a
        /// bool — the negative wiring is exercised end-to-end by `examples-check`/`probe.js`.)
        #[test]
        fn never_panics_and_returns_a_bool_for_a_bare_target() {
            let dir = std::env::temp_dir().join(format!(
                "gnr8-ts-probe-bare-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            ));
            std::fs::create_dir_all(&dir).unwrap();
            // Just assert it returns without panic; the value depends on whether the sidecar has the
            // dev typescript installed (resolvable) or not (absent) — both are valid environments.
            let _present: bool = typescript_toolchain_present(&dir.to_string_lossy()).unwrap();
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    mod run_goextract {
        use super::{run_goextract_with, CoreError};

        #[test]
        fn returns_go_toolchain_missing_when_binary_absent() {
            // A binary name that cannot exist on PATH forces the spawn to fail with
            // an io::Error -> GoToolchainMissing, NOT a panic (GO-06).
            let result = run_goextract_with(
                "gnr8-nonexistent-go-binary-xyz",
                "/some/target/dir",
                &[],
                &[],
            );
            let err = result.unwrap_err();
            assert!(
                matches!(err, CoreError::GoToolchainMissing { .. }),
                "expected GoToolchainMissing, got {err:?}"
            );
            // Display must render without panic and mention the toolchain.
            assert!(err.to_string().contains("Go toolchain"));
        }
    }

    mod run_pyextract {
        use super::{run_pyextract_with, CoreError};

        #[test]
        fn returns_python_toolchain_missing_when_binary_absent() {
            // A binary name that cannot exist on PATH forces the spawn to fail with an io::Error
            // -> PythonToolchainMissing, NOT a panic (T-02-02-py). Forced via the `_with` split so
            // we never mutate the process PATH.
            let result =
                run_pyextract_with("gnr8-nonexistent-python-binary-xyz", "/some/target/dir");
            let err = result.unwrap_err();
            assert!(
                matches!(err, CoreError::PythonToolchainMissing { .. }),
                "expected PythonToolchainMissing, got {err:?}"
            );
            // Display must render without panic and mention the toolchain.
            assert!(err.to_string().contains("Python toolchain"));
        }
    }
}
