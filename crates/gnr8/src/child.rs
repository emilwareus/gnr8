//! The host↔child process boundary (`docs/code-as-config.md`): run the user's `.gnr8/` generation
//! crate and parse the artifact bundle it prints.
//!
//! The installed `gnr8` binary is the orchestrator + trusted writer; the `.gnr8/` crate is the pure
//! generator. The boundary is `cargo run --manifest-path .gnr8/Cargo.toml -- <subcommand>` +
//! JSON-on-stdout + an exit code — no FFI, no plugin ABI (mirrors the polint model). This module owns
//! the HOST side: it requires the `.gnr8/` workspace, spawns the child with `current_dir = project
//! root` (so the child's relative inputs resolve against the project), and on success parses the
//! child's stdout as either a versioned [`ArtifactBundle`] or an inspect [`gnr8::graph::ApiGraph`].
//!
//! ## Error categorization (never a panic — RUST-04 / D-09)
//!
//! Every failure surfaces as a typed [`CoreError::ChildRun`] with an ACTIONABLE message:
//! - the `.gnr8/Cargo.toml` is missing ⇒ "run `gnr8 init`";
//! - `cargo` cannot be spawned ⇒ "install Rust/cargo";
//! - the child exited non-zero ⇒ the child's stderr is surfaced verbatim (a compile error in the
//!   user's pipeline, or a runtime/toolchain error from the pipeline itself);
//! - the child's stdout is not a parseable bundle ⇒ a parse error with a hint;
//! - the bundle's schema `version` differs from this gnr8's ⇒ an actionable "realign gnr8" error
//!   (the `.gnr8/` crate links a skewed `gnr8` library).
//!
//! `cargo` itself is the build cache: incremental rebuilds of `.gnr8/` are fast after the first build.

// Acronym-dense prose (cargo, OpenAPI, FFI, JSON, ...); allow `doc_markdown` module-wide (mirrors the
// scoped allows across the binary).
#![allow(clippy::doc_markdown)]

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use gnr8::runner::ArtifactBundle;
use gnr8::CoreError;

/// The env var that overrides the cargo binary used to build/run the child (checked before `CARGO`).
const GNR8_CARGO_ENV: &str = "GNR8_CARGO";
/// The standard cargo-set env var naming the cargo binary (checked after `GNR8_CARGO`).
const CARGO_ENV: &str = "CARGO";
/// The default cargo binary when neither override is set.
const DEFAULT_CARGO: &str = "cargo";
/// The generation crate directory under the project root.
const WORKSPACE_DIR: &str = ".gnr8";

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
    let (stdout, stderr) = run_child_stdout(project_root, subcommand)?;
    let bundle = parse_bundle(stdout.trim(), &stderr)?;

    // Reject a bundle this host does not understand: the `.gnr8/` crate links its own `gnr8`, so a
    // version skew (e.g. a pinned published `gnr8` vs a newer host) must fail with an actionable
    // message rather than a confusing parse error or silently-wrong output.
    if bundle.protocol_version != gnr8::runner::PROTOCOL_VERSION {
        return Err(CoreError::ChildRun {
            message: format!(
                "the .gnr8 generation crate emitted protocol version {}, but this gnr8 supports \
                 version {}. Realign the installed CLI with .gnr8/Cargo.lock, then re-run.",
                bundle.protocol_version,
                gnr8::runner::PROTOCOL_VERSION
            ),
        });
    }
    let host_version = env!("CARGO_PKG_VERSION");
    if bundle.cli_version != host_version || bundle.core_version != host_version {
        return Err(CoreError::ChildRun {
            message: format!(
                "gnr8 version mismatch: host CLI {host_version}, bundle CLI {}, child gnr8-core {}. \
                 Install the exact version pinned in .gnr8/Cargo.lock.",
                bundle.cli_version, bundle.core_version
            ),
        });
    }
    let expected_fingerprint = gnr8::runner::capability_fingerprint();
    if bundle.capability_fingerprint != expected_fingerprint {
        return Err(CoreError::ChildRun {
            message: format!(
                "gnr8 capability mismatch: host {expected_fingerprint}, child {}. Rebuild the CLI \
                 and generation crate at one exact version.",
                bundle.capability_fingerprint
            ),
        });
    }
    Ok(bundle)
}

/// Run the user's `.gnr8/` generation crate in inspect mode and parse the transformed graph.
pub(crate) fn inspect_child(project_root: &Path) -> Result<gnr8::graph::ApiGraph, CoreError> {
    let (stdout, stderr) = run_child_stdout(project_root, "__inspect")?;
    parse_graph(stdout.trim(), &stderr)
}

fn run_child_stdout(project_root: &Path, subcommand: &str) -> Result<(String, Vec<u8>), CoreError> {
    let manifest = gnr8::workspace::manifest_path(project_root);
    if !manifest.is_file() {
        return Err(CoreError::ChildRun {
            message: format!(
                "no .gnr8/ workspace at {} — run `gnr8 init` to scaffold the generation crate",
                manifest.display()
            ),
        });
    }

    let invocation = child_invocation(project_root, &manifest, subcommand);
    let output = invocation.command().output().map_err(|err| {
        let cargo = cargo_binary();
        CoreError::ChildRun {
            message: format!(
                "failed to run the .gnr8 generation crate via `{}` ({err}) — is Rust/cargo \
                 installed and on PATH? (override the cargo binary with $GNR8_CARGO if needed)",
                invocation.description(&cargo)
            ),
        }
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoreError::ChildRun {
            message: format!(
                "the .gnr8 generation crate failed (`{}` exited with {}).\n\
                 This is usually a compile error in your .gnr8/src/main.rs pipeline, or a generation \
                 error from it (e.g. the Go toolchain is missing). cargo/child output:\n{}",
                invocation.description(&cargo_binary()),
                describe_status(output.status),
                stderr.trim_end()
            ),
        });
    }

    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        output.stderr,
    ))
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

fn parse_graph(stdout: &str, stderr: &[u8]) -> Result<gnr8::graph::ApiGraph, CoreError> {
    serde_json::from_str::<gnr8::graph::ApiGraph>(stdout).map_err(|err| {
        let stderr = String::from_utf8_lossy(stderr);
        CoreError::ChildRun {
            message: format!(
                "the .gnr8 generation crate did not emit a parseable API graph on stdout \
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

enum ChildInvocation {
    Direct {
        binary: PathBuf,
        project_root: PathBuf,
        subcommand: String,
    },
    CargoRun {
        cargo: String,
        manifest: PathBuf,
        project_root: PathBuf,
        subcommand: String,
    },
}

impl ChildInvocation {
    fn command(&self) -> Command {
        let mut command = match self {
            Self::Direct {
                binary,
                project_root,
                subcommand,
            } => {
                let mut command = Command::new(binary);
                command.arg(subcommand).current_dir(project_root);
                command
            }
            Self::CargoRun {
                cargo,
                manifest,
                project_root,
                subcommand,
            } => {
                let mut command = Command::new(cargo);
                command
                    .arg("run")
                    .arg("--quiet")
                    .arg("--manifest-path")
                    .arg(manifest)
                    .arg("--")
                    .arg(subcommand)
                    .current_dir(project_root);
                command
            }
        };
        if let Some(resource_dir) = gnr8::resource::resource_dir() {
            command.env(gnr8::resource::GNR8_RESOURCE_DIR_ENV, resource_dir);
        }
        command
            .env(
                gnr8::runner::HOST_PROTOCOL_ENV,
                gnr8::runner::PROTOCOL_VERSION.to_string(),
            )
            .env(gnr8::runner::HOST_VERSION_ENV, env!("CARGO_PKG_VERSION"))
            .env(
                gnr8::runner::HOST_CAPABILITY_ENV,
                gnr8::runner::capability_fingerprint(),
            );
        command
    }

    fn description(&self, fallback_cargo: &str) -> String {
        match self {
            Self::Direct {
                binary, subcommand, ..
            } => {
                format!("{} {subcommand}", binary.display())
            }
            Self::CargoRun {
                cargo, subcommand, ..
            } => format!(
                "{} run --quiet --manifest-path .gnr8/Cargo.toml -- {subcommand}",
                if cargo.is_empty() {
                    fallback_cargo
                } else {
                    cargo
                }
            ),
        }
    }
}

fn child_invocation(project_root: &Path, manifest: &Path, subcommand: &str) -> ChildInvocation {
    if let Some(binary) = fresh_child_binary(project_root, manifest) {
        return ChildInvocation::Direct {
            binary,
            project_root: project_root.to_path_buf(),
            subcommand: subcommand.to_string(),
        };
    }
    ChildInvocation::CargoRun {
        cargo: cargo_binary(),
        manifest: manifest.to_path_buf(),
        project_root: project_root.to_path_buf(),
        subcommand: subcommand.to_string(),
    }
}

fn fresh_child_binary(project_root: &Path, manifest: &Path) -> Option<PathBuf> {
    let package = package_name(manifest)?;
    for profile in ["release", "debug"] {
        let binary = project_root
            .join(WORKSPACE_DIR)
            .join("target")
            .join(profile)
            .join(&package);
        if binary.is_file() && is_executable_fresh(&binary, project_root, manifest) {
            return Some(binary);
        }
    }
    None
}

fn is_executable_fresh(binary: &Path, project_root: &Path, manifest: &Path) -> bool {
    let Ok(binary_modified) = binary.metadata().and_then(|metadata| metadata.modified()) else {
        return false;
    };
    generation_workspace_inputs(project_root, manifest)
        .into_iter()
        .filter_map(|path| {
            path.metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
        })
        .all(|modified| modified <= binary_modified)
}

fn generation_workspace_inputs(project_root: &Path, manifest: &Path) -> Vec<PathBuf> {
    let mut inputs = vec![manifest.to_path_buf()];
    let lock = manifest.with_file_name("Cargo.lock");
    if lock.is_file() {
        inputs.push(lock);
    }
    collect_files(&project_root.join(WORKSPACE_DIR).join("src"), &mut inputs);
    if let Ok(exe) = std::env::current_exe() {
        inputs.push(exe);
    }
    inputs
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn package_name(manifest: &Path) -> Option<String> {
    let body = std::fs::read_to_string(manifest).ok()?;
    let parsed: toml::Value = toml::from_str(&body).ok()?;
    parsed
        .get("package")?
        .get("name")?
        .as_str()
        .map(ToString::to_string)
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::package_name;

    fn temp_manifest(body: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "gnr8-child-manifest-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("Cargo.toml");
        std::fs::write(&manifest, body).unwrap();
        manifest
    }

    #[test]
    fn package_name_reads_toml_package_name() {
        let manifest = temp_manifest(
            r#"
[package]
name = 'quoted-child' # comments are valid TOML
version = "0.1.0"
"#,
        );

        assert_eq!(package_name(&manifest), Some("quoted-child".to_string()));
    }
}
