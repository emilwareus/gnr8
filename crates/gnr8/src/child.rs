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
    ensure_cargo_lock(project_root)?;
    // Bracket the whole cargo build + child run. The child brackets its own execution too, but a
    // config edit after cargo compiled the binary and before that binary started would otherwise
    // pair old pipeline code with new `.gnr8/src` stamps.
    let config_before = host_config_snapshot(project_root);
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
    let config_after = host_config_snapshot(project_root);
    validate_host_config_snapshot(
        project_root,
        bundle.artifact_cache_key.as_deref(),
        &bundle.cache_config_stamps,
        config_before.as_deref(),
        config_after.as_deref(),
    )?;
    Ok(bundle)
}

fn validate_host_config_snapshot(
    project_root: &Path,
    artifact_cache_key: Option<&str>,
    child_snapshot: &[gnr8::sdk::FileStamp],
    before: Option<&[gnr8::sdk::FileStamp]>,
    after: Option<&[gnr8::sdk::FileStamp]>,
) -> Result<(), CoreError> {
    let config_changed = before != after;
    let child_snapshot_disagrees = after.is_some_and(|after| after != child_snapshot);
    if config_changed || child_snapshot_disagrees {
        if let Some(key) = artifact_cache_key {
            gnr8::sdk::discard_artifact_cache(project_root, key)?;
        }
        return Err(CoreError::ChildRun {
            message: "the .gnr8 configuration changed while cargo built or ran the generation crate; no outputs were accepted — rerun generate"
                .to_string(),
        });
    }
    Ok(())
}

fn ensure_cargo_lock(project_root: &Path) -> Result<(), CoreError> {
    let manifest = gnr8::workspace::manifest_path(project_root);
    let lock = manifest.with_file_name("Cargo.lock");
    if lock.is_file() || !manifest.is_file() {
        return Ok(());
    }
    let cargo = cargo_binary();
    let output = Command::new(&cargo)
        .args(["generate-lockfile", "--manifest-path"])
        .arg(&manifest)
        .current_dir(project_root)
        .output()
        .map_err(|err| CoreError::ChildRun {
            message: format!("failed to create .gnr8/Cargo.lock with {cargo:?}: {err}"),
        })?;
    if !output.status.success() {
        return Err(CoreError::ChildRun {
            message: format!(
                "failed to create .gnr8/Cargo.lock before building the generation crate:\n{}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(())
}

fn host_config_snapshot(project_root: &Path) -> Option<Vec<gnr8::sdk::FileStamp>> {
    let fast = crate::collect_required_config_fast_stamps(project_root)?;
    crate::content_stamps_from_fast(project_root, &fast)
}

/// Run the user's `.gnr8/` generation crate in inspect mode and parse the transformed graph.
pub(crate) fn inspect_child(project_root: &Path) -> Result<gnr8::graph::ApiGraph, CoreError> {
    ensure_cargo_lock(project_root)?;
    let config_before = host_config_snapshot(project_root);
    let (stdout, stderr) = run_child_stdout(project_root, "__inspect")?;
    let config_after = host_config_snapshot(project_root);
    if config_before != config_after {
        return Err(CoreError::ChildRun {
            message: "the .gnr8 configuration changed while cargo built or ran the inspection crate; no graph was accepted — rerun inspect"
                .to_string(),
        });
    }
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
    let output = invocation.command()?.output().map_err(|err| {
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
    CargoRun {
        cargo: String,
        manifest: PathBuf,
        project_root: PathBuf,
        subcommand: String,
    },
}

impl ChildInvocation {
    fn command(&self) -> Result<Command, CoreError> {
        let mut command = match self {
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
        let resource_dir = gnr8::resource::resource_dir()?;
        configure_child_environment(&mut command, &resource_dir);
        Ok(command)
    }

    fn description(&self, fallback_cargo: &str) -> String {
        match self {
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

fn configure_child_environment(command: &mut Command, resource_dir: &Path) {
    command
        .env(gnr8::resource::GNR8_RESOURCE_DIR_ENV, resource_dir)
        .env(
            gnr8::runner::HOST_PROTOCOL_ENV,
            gnr8::runner::PROTOCOL_VERSION.to_string(),
        )
        .env(gnr8::runner::HOST_VERSION_ENV, env!("CARGO_PKG_VERSION"))
        .env(
            gnr8::runner::HOST_CAPABILITY_ENV,
            gnr8::runner::capability_fingerprint(),
        );
}

fn child_invocation(project_root: &Path, manifest: &Path, subcommand: &str) -> ChildInvocation {
    ChildInvocation::CargoRun {
        cargo: cargo_binary(),
        manifest: manifest.to_path_buf(),
        project_root: project_root.to_path_buf(),
        subcommand: subcommand.to_string(),
    }
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

    use super::{configure_child_environment, validate_host_config_snapshot};
    use std::ffi::OsStr;
    use std::process::Command;

    fn env_value<'a>(command: &'a Command, key: &str) -> Option<&'a OsStr> {
        command
            .get_envs()
            .find(|(name, _)| *name == OsStr::new(key))
            .and_then(|(_, value)| value)
    }

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
    fn child_environment_includes_complete_compatibility_handshake() {
        let mut command = Command::new("child");
        configure_child_environment(&mut command, std::path::Path::new("/resources"));

        assert_eq!(
            env_value(&command, gnr8::runner::HOST_PROTOCOL_ENV),
            Some(OsStr::new(&gnr8::runner::PROTOCOL_VERSION.to_string()))
        );
        assert_eq!(
            env_value(&command, gnr8::runner::HOST_VERSION_ENV),
            Some(OsStr::new(env!("CARGO_PKG_VERSION")))
        );
        assert_eq!(
            env_value(&command, gnr8::runner::HOST_CAPABILITY_ENV),
            Some(OsStr::new(&gnr8::runner::capability_fingerprint()))
        );
    }

    #[test]
    fn changed_outer_config_snapshot_rejects_the_bundle_and_discards_its_cache() {
        let manifest = temp_manifest("[package]\nname = 'snapshot-test'\nversion = '0.1.0'\n");
        let root = manifest.parent().unwrap();
        let key = "a".repeat(64);
        let cache = root.join(".gnr8/cache/artifacts");
        std::fs::create_dir_all(&cache).unwrap();
        let full = cache.join(format!("{key}.json"));
        let metadata = cache.join(format!("{key}.meta.json"));
        std::fs::write(&full, b"poisoned").unwrap();
        std::fs::write(&metadata, b"poisoned").unwrap();
        let before = vec![gnr8::sdk::FileStamp {
            path: ".gnr8/src/main.rs".to_string(),
            len: 1,
            modified_ns: 1,
            hash: "b".repeat(64),
        }];
        let after = vec![gnr8::sdk::FileStamp {
            hash: "c".repeat(64),
            ..before[0].clone()
        }];

        let err =
            validate_host_config_snapshot(root, Some(&key), &after, Some(&before), Some(&after))
                .unwrap_err();

        assert!(err.to_string().contains("configuration changed"));
        assert!(!full.exists());
        assert!(!metadata.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
