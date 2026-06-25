//! The host↔child process boundary (`docs/code-as-config.md`): run the user's `.gnr8/` generation
//! crate and parse the artifact bundle it prints.
//!
//! The installed `gnr8` binary is the orchestrator + trusted writer; the `.gnr8/` crate is the pure
//! generator. The boundary is `cargo run --manifest-path .gnr8/Cargo.toml -- <subcommand>` +
//! JSON-on-stdout + an exit code — no FFI, no plugin ABI (mirrors the polint model). This module owns
//! the HOST side: it requires the `.gnr8/` workspace, spawns the child with `current_dir = project
//! root` (so the child's relative inputs resolve against the project), and on success parses the
//! child's stdout as a versioned [`ArtifactBundle`].
//!
//! ## Error categorization (never a panic — RUST-04 / D-09)
//!
//! Every failure surfaces as a typed [`CoreError::ChildRun`] with an ACTIONABLE message:
//! - the `.gnr8/Cargo.toml` is missing ⇒ "run `gnr8 init`";
//! - `cargo` cannot be spawned ⇒ "install Rust/cargo";
//! - the child exited non-zero ⇒ the child's stderr is surfaced verbatim (a compile error in the
//!   user's pipeline, or a runtime/toolchain error from the pipeline itself);
//! - the child's stdout is not a parseable bundle ⇒ a parse error with a hint;
//! - the bundle's schema `version` differs from this gnr8's ⇒ an actionable "realign gnr8-core" error
//!   (the `.gnr8/` crate links a skewed `gnr8-core`).
//!
//! `cargo` itself is the build cache: incremental rebuilds of `.gnr8/` are fast after the first build.

// Acronym-dense prose (cargo, OpenAPI, FFI, JSON, ...); allow `doc_markdown` module-wide (mirrors the
// scoped allows across the binary).
#![allow(clippy::doc_markdown)]

use std::path::Path;
use std::process::Command;

use gnr8_core::runner::ArtifactBundle;
use gnr8_core::CoreError;

/// The env var that overrides the cargo binary used to build/run the child (checked before `CARGO`).
const GNR8_CARGO_ENV: &str = "GNR8_CARGO";
/// The standard cargo-set env var naming the cargo binary (checked after `GNR8_CARGO`).
const CARGO_ENV: &str = "CARGO";
/// The default cargo binary when neither override is set.
const DEFAULT_CARGO: &str = "cargo";

/// Run the user's `.gnr8/` generation crate with `subcommand` (`__emit` / `__inspect`) and return the
/// parsed [`ArtifactBundle`] it printed on stdout.
///
/// Requires `<project_root>/.gnr8/Cargo.toml` (a missing one is the "run `gnr8 init`" error). Spawns
/// `cargo run --quiet --manifest-path <root>/.gnr8/Cargo.toml -- <subcommand>` with
/// `current_dir = project_root`, so the child inherits cwd = project root and analyzes the project. The
/// cargo binary is `$GNR8_CARGO`, else `$CARGO`, else `cargo`.
///
/// With `--quiet`, cargo's build progress goes to stderr and ONLY the child program's output reaches
/// stdout, so stdout is parsed directly as the bundle JSON. On any failure the child's stderr is folded
/// into the returned error so the user sees the underlying compiler/runtime message.
///
/// # Errors
///
/// Returns [`CoreError::ChildRun`] for a missing workspace, a cargo spawn failure, a non-zero child
/// exit (surfacing the child's stderr), an unparseable bundle, or a bundle whose schema `version` this
/// gnr8 does not support. Never panics.
pub(crate) fn run_child(
    project_root: &Path,
    subcommand: &str,
) -> Result<ArtifactBundle, CoreError> {
    let manifest = gnr8_core::workspace::manifest_path(project_root);
    if !manifest.is_file() {
        return Err(CoreError::ChildRun {
            message: format!(
                "no .gnr8/ workspace at {} — run `gnr8 init` to scaffold the generation crate",
                manifest.display()
            ),
        });
    }

    let cargo = cargo_binary();
    let output = Command::new(&cargo)
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--")
        .arg(subcommand)
        .current_dir(project_root)
        .output()
        .map_err(|err| CoreError::ChildRun {
            message: format!(
                "failed to run the .gnr8 generation crate via `{cargo} run` ({err}) — is Rust/cargo \
                 installed and on PATH? (override the cargo binary with $GNR8_CARGO if needed)"
            ),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoreError::ChildRun {
            message: format!(
                "the .gnr8 generation crate failed (`{cargo} run -- {subcommand}` exited with {}).\n\
                 This is usually a compile error in your .gnr8/src/main.rs pipeline, or a generation \
                 error from it (e.g. the Go toolchain is missing). cargo/child output:\n{}",
                describe_status(output.status),
                stderr.trim_end()
            ),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let bundle = parse_bundle(stdout.trim(), &output.stderr)?;

    // Reject a bundle this host does not understand: the `.gnr8/` crate links its own `gnr8-core`, so a
    // version skew (e.g. a pinned published `gnr8-core` vs a newer host) must fail with an actionable
    // message rather than a confusing parse error or silently-wrong output.
    if bundle.version != gnr8_core::runner::BUNDLE_VERSION {
        return Err(CoreError::ChildRun {
            message: format!(
                "the .gnr8 generation crate emitted artifact-bundle schema version {}, but this gnr8 \
                 supports version {}. Realign the gnr8-core your .gnr8/ crate depends on with this \
                 gnr8 binary (update .gnr8/Cargo.toml), then re-run.",
                bundle.version,
                gnr8_core::runner::BUNDLE_VERSION
            ),
        });
    }
    Ok(bundle)
}

/// Parse the child's stdout as an [`ArtifactBundle`], folding the child's stderr into the error message
/// on failure so a non-bundle stdout (e.g. an unexpected panic message) is debuggable.
fn parse_bundle(stdout: &str, stderr: &[u8]) -> Result<ArtifactBundle, CoreError> {
    serde_json::from_str::<ArtifactBundle>(stdout).map_err(|err| {
        let stderr = String::from_utf8_lossy(stderr);
        CoreError::ChildRun {
            message: format!(
                "the .gnr8 generation crate did not emit a parseable artifact bundle on stdout \
                 ({err}). Got {} byte(s) of stdout.{}",
                stdout.len(),
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" Child stderr:\n{}", stderr.trim_end())
                }
            ),
        }
    })
}

/// The cargo binary to invoke: `$GNR8_CARGO`, else `$CARGO`, else `cargo` (the documented override
/// order). A non-UTF-8 / empty value is ignored in favor of the next source.
fn cargo_binary() -> String {
    for var in [GNR8_CARGO_ENV, CARGO_ENV] {
        if let Ok(value) = std::env::var(var) {
            if !value.is_empty() {
                return value;
            }
        }
    }
    DEFAULT_CARGO.to_string()
}

/// Render an [`std::process::ExitStatus`] as a short string for the error message (the numeric code,
/// or a signal note on Unix when the process was killed by a signal). `ExitStatus` is `Copy`, so it is
/// taken by value.
fn describe_status(status: std::process::ExitStatus) -> String {
    status.code().map_or_else(
        || "no exit code (terminated by signal)".to_string(),
        |c| format!("code {c}"),
    )
}
