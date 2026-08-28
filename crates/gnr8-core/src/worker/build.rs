//! Validating, fingerprinting and building a project's `.gnr8/` worker.
//!
//! ## Trust
//!
//! Building and running `.gnr8/` is **trusted-code execution**. `cargo build` compiles and runs that
//! crate's `build.rs`, its proc-macro dependencies, and then gnr8 executes the resulting binary —
//! all with the invoking user's privileges. gnr8 does not sandbox any of it and does not claim to.
//! What it does provide is consent and containment: [`WorkerPolicy`] can forbid building, or forbid
//! building *and* running, so an untrusted checkout can still be inspected
//! (`gnr8 inspect routes <path>` never touches `.gnr8/`).
//!
//! ## The build stamp
//!
//! There is exactly one rule for reusing a previously built worker, and it is content-addressed
//! rather than heuristic:
//!
//! > If `.gnr8/cache/worker.json` exists, its fingerprint equals the freshly computed one, and the
//! > recorded binary still hashes to the recorded value, then that binary **is** the build output of
//! > those inputs; run it. Otherwise build.
//!
//! The fingerprint covers every file under `.gnr8/` (except `target/` and `cache/`), the host
//! executable's own content hash — which is what makes an in-repo path dependency on the SDK safe —
//! and the protocol/capability constants. So an unchanged project runs `cargo` zero times.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::{blake3_file, blake3_hex};
use crate::CoreError;

/// The env var that overrides the cargo binary used to build the worker (checked before `CARGO`).
pub const GNR8_CARGO_ENV: &str = "GNR8_CARGO";
/// The standard cargo-set env var naming the cargo binary (checked after `GNR8_CARGO`).
const CARGO_ENV: &str = "CARGO";
/// The default cargo binary when neither override is set.
const DEFAULT_CARGO: &str = "cargo";
/// Set to `1` to pass `--offline` to every cargo invocation.
pub const GNR8_CARGO_OFFLINE_ENV: &str = "GNR8_CARGO_OFFLINE";

/// The cargo profile gnr8 compiles a project's worker under.
///
/// `.gnr8/target` is gnr8's own build directory, so a named profile keeps the worker's compilation
/// settings gnr8's decision and keeps them out of the way of a `cargo build` the user runs there
/// themselves. It inherits `dev`, so a project that tunes `[profile.dev]` still tunes this.
const WORKER_PROFILE: &str = "gnr8";

/// The profile definition passed to every worker build, one `--config` value per line.
///
/// The worker spends its time on two things, and an unoptimized build makes both dominate. The
/// first is the SDK's own work on a frame — `serde_json` over the API graph and the frame's blake3
/// digest: on a 4,836-artifact project one graph round trip cost 408ms of encode + decode built
/// unoptimized against 27ms optimized, and the pipeline is several of those. The second is the
/// user's own stages, which on a project whose custom targets generate content was another ~20% of
/// a warm run. So everything that RUNS IN THE WORKER is optimized, dependencies and user crate
/// alike: a 332-artifact warm generate measured 0.28s that way against 0.57s with none of it
/// optimized, 0.47s with only the SDK optimized, and 0.29s with the user's own crate left out.
///
/// `opt-level = 2` is where that stops paying: it is worth about 10% of a warm run over `1` on both
/// projects (a 332-artifact `check` at 0.228s against 0.245s, `generate` at 0.235s against 0.252s;
/// a 4,836-artifact `check` at 0.588s against 0.633s) for 3s of a from-scratch build, and `3`
/// measured within noise of `2` on both axes. Dependency debug info is dropped because it is 10x of
/// the binary gnr8 re-hashes on every run and nothing reads it; the user's own crate keeps its own,
/// which is what a panic in a stage they wrote needs.
///
/// A build script or a proc macro runs in the COMPILER, not in the worker, so optimizing one buys
/// the worker nothing and costs the build the time twice over: `syn` went from 1.6s to 4.6s and
/// `serde_derive` from 1.8s to 3.4s, both squarely on the critical path to the SDK. `build-override`
/// is where cargo names those units — and it only reaches them when `package."*"` does not also set
/// `opt-level`, because a package override is applied last and would shadow it. The profile-wide
/// setting therefore carries the optimization and `build-override` carves the compiler's own code
/// back out: a from-scratch worker build went from 20.2s to 15.8s with the warm run unchanged.
const WORKER_PROFILE_CONFIG: [&str; 5] = [
    r#"profile.gnr8.inherits="dev""#,
    "profile.gnr8.opt-level=2",
    "profile.gnr8.build-override.opt-level=0",
    "profile.gnr8.build-override.debug=false",
    r#"profile.gnr8.package."*".debug=false"#,
];

/// The first gnr8 version whose `.gnr8/` crate links the thin SDK instead of the whole engine.
///
/// A `gnr8` dependency pinned below this is the previous contract, and it cannot work: that crate
/// calls `gnr8::runner::run` and prints a JSON bundle, which is not this protocol. Rejecting it here
/// costs milliseconds; letting it compile costs a full build before the same conclusion.
const FIRST_SDK_VERSION: (u64, u64, u64) = (0, 9, 0);

/// The package names of the host-only engine crates. A `.gnr8/` crate that depends on one of them
/// has pulled the whole generator into the project build, which is the thing this split exists to
/// prevent.
const HOST_ONLY_PACKAGES: [&str; 2] = ["gnr8-engine", "gnr8-core"];

/// What the host is permitted to do with a project's `.gnr8/` crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerPolicy {
    /// Whether `cargo` may be invoked.
    pub allow_build: bool,
    /// Whether the worker binary may be executed.
    pub allow_execute: bool,
}

impl Default for WorkerPolicy {
    fn default() -> Self {
        Self {
            allow_build: true,
            allow_execute: true,
        }
    }
}

impl WorkerPolicy {
    /// Never invoke `cargo`; an existing, matching worker binary may still be run.
    #[must_use]
    pub const fn no_build() -> Self {
        Self {
            allow_build: false,
            allow_execute: true,
        }
    }

    /// Never build and never run anything from `.gnr8/`.
    #[must_use]
    pub const fn no_execute() -> Self {
        Self {
            allow_build: false,
            allow_execute: false,
        }
    }
}

/// A validated `.gnr8/` workspace: where its manifest is and what its worker binary is called.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// The project root.
    pub project_root: PathBuf,
    /// `<project_root>/.gnr8`.
    pub dir: PathBuf,
    /// `<project_root>/.gnr8/Cargo.toml`.
    pub manifest: PathBuf,
    /// The `[package] name` the manifest declares.
    pub package: String,
}

impl Workspace {
    /// The cargo target directory gnr8 pins for the worker build.
    #[must_use]
    pub fn target_dir(&self) -> PathBuf {
        self.dir.join("target")
    }

    /// The profile directory the worker binary lands in.
    fn profile_dir(&self) -> PathBuf {
        self.target_dir().join(WORKER_PROFILE)
    }

    /// Where the built worker binary lands.
    #[must_use]
    pub fn binary_path(&self) -> PathBuf {
        self.profile_dir().join(binary_file_name(&self.package))
    }

    /// The build stamp path.
    #[must_use]
    pub fn stamp_path(&self) -> PathBuf {
        stamp_path(&self.project_root)
    }
}

/// Where a project's worker build stamp lives — the one definition of that path.
///
/// Taken by project root rather than by [`Workspace`] so callers that must reach the stamp *before*
/// a workspace validates — `gnr8 init --upgrade`, whose whole job is a manifest the host currently
/// rejects — still go through this function instead of rebuilding the path by hand.
#[must_use]
pub fn stamp_path(project_root: &Path) -> PathBuf {
    project_root
        .join(crate::lifecycle::WORKSPACE_DIR)
        .join("cache")
        .join("worker.json")
}

#[cfg(windows)]
fn binary_file_name(package: &str) -> String {
    format!("{package}.exe")
}

#[cfg(not(windows))]
fn binary_file_name(package: &str) -> String {
    package.to_string()
}

/// Validate a project's `.gnr8/` workspace **before** anything is compiled or run.
///
/// Checks, in order: the directory and manifest exist and are real (non-symlink) entries; the
/// manifest parses and declares a package name; it depends on `gnr8`; it does not depend on a
/// host-only engine crate; and its `gnr8` pin is not the pre-SDK contract.
///
/// # Errors
///
/// Returns [`CoreError::WorkerBuild`] naming the exact problem and the action that fixes it.
pub fn validate_workspace(project_root: &Path) -> Result<Workspace, CoreError> {
    let dir = project_root.join(crate::lifecycle::WORKSPACE_DIR);
    let manifest = dir.join("Cargo.toml");
    require_real_entry(&dir, EntryKind::Directory)?;
    require_real_entry(&manifest, EntryKind::File)?;

    let text = std::fs::read_to_string(&manifest).map_err(|err| CoreError::WorkerBuild {
        message: format!("failed to read {}: {err}", manifest.display()),
    })?;
    let value: toml::Value = toml::from_str(&text).map_err(|err| CoreError::WorkerBuild {
        message: format!("{} is not valid TOML: {err}", manifest.display()),
    })?;
    let package = value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| CoreError::WorkerBuild {
            message: format!(
                "{} has no [package] name; gnr8 needs it to locate the built worker",
                manifest.display()
            ),
        })?
        .to_string();
    if package.is_empty() || !package.bytes().all(is_cargo_name_byte) {
        return Err(CoreError::WorkerBuild {
            message: format!(
                "{} declares the package name {package:?}, which is not a valid Cargo package name",
                manifest.display()
            ),
        });
    }

    let dependencies = value.get("dependencies");
    for host_only in HOST_ONLY_PACKAGES {
        if dependencies.and_then(|deps| deps.get(host_only)).is_some() {
            return Err(CoreError::WorkerBuild {
                message: format!(
                    "{} depends on `{host_only}`, the gnr8 host engine. A .gnr8 crate links only the \
                     thin `gnr8` SDK — the engine lives in the installed CLI. Remove that dependency \
                     and run `gnr8 init --upgrade`.",
                    manifest.display()
                ),
            });
        }
    }
    let gnr8_dep = dependencies
        .and_then(|deps| deps.get("gnr8"))
        .ok_or_else(|| CoreError::WorkerBuild {
            message: format!(
                "{} has no `gnr8` dependency; a .gnr8 crate must depend on the gnr8 SDK. Run \
                 `gnr8 init --upgrade`.",
                manifest.display()
            ),
        })?;
    if let Some(version) = exact_pin(gnr8_dep) {
        if version < FIRST_SDK_VERSION {
            let (major, minor, patch) = FIRST_SDK_VERSION;
            return Err(CoreError::WorkerBuild {
                message: format!(
                    "{} pins gnr8 {}.{}.{}, which is the previous .gnr8 contract: that crate linked \
                     the whole generation engine and printed a JSON bundle. gnr8 {major}.{minor}.{patch} \
                     and later ship a thin SDK and a framed worker protocol. Run `gnr8 init --upgrade` \
                     to repoint the manifest, then wrap your own stages in `Custom(...)`.",
                    manifest.display(),
                    version.0,
                    version.1,
                    version.2
                ),
            });
        }
    }

    Ok(Workspace {
        project_root: project_root.to_path_buf(),
        dir,
        manifest,
        package,
    })
}

/// What a workspace entry must be for gnr8 to build through it.
#[derive(Debug, Clone, Copy)]
enum EntryKind {
    Directory,
    File,
}

/// Require `path` to exist as a real (non-symlink) entry of `kind`.
///
/// A symlinked `.gnr8/` or manifest would let a checkout point the build somewhere the fingerprint
/// cannot see, so it is refused rather than followed.
fn require_real_entry(path: &Path, kind: EntryKind) -> Result<(), CoreError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| CoreError::WorkerBuild {
        message: format!(
            "no .gnr8 workspace entry at {} — run `gnr8 init` to scaffold the generation crate",
            path.display()
        ),
    })?;
    let ok = match kind {
        EntryKind::Directory => metadata.is_dir(),
        EntryKind::File => metadata.is_file(),
    };
    if metadata.file_type().is_symlink() || !ok {
        let noun = match kind {
            EntryKind::Directory => "directory",
            EntryKind::File => "file",
        };
        return Err(CoreError::WorkerBuild {
            message: format!(
                "{} must be a real {noun}; gnr8 refuses to build through a symlinked .gnr8 workspace",
                path.display()
            ),
        });
    }
    Ok(())
}

fn is_cargo_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

/// The exact version an `=X.Y.Z` / `X.Y.Z` dependency requirement pins, if it pins one.
///
/// Anything else — a caret range, a git or path dependency — is left to the handshake, which is the
/// authoritative version check. This only rejects what is provably the old contract.
fn exact_pin(dependency: &toml::Value) -> Option<(u64, u64, u64)> {
    let requirement = match dependency {
        toml::Value::String(text) => text.as_str(),
        toml::Value::Table(table) => {
            if table.contains_key("path") || table.contains_key("git") {
                return None;
            }
            table.get("version")?.as_str()?
        }
        _ => return None,
    };
    let trimmed = requirement.trim();
    let body = trimmed.strip_prefix('=').unwrap_or(trimmed).trim();
    let mut parts = body.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0");
    let patch = patch
        .split(['-', '+'])
        .next()
        .and_then(|patch| patch.parse().ok())?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// The recorded identity of a previously built worker.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WorkerStamp {
    /// Fingerprint of every input the build consumed.
    fingerprint: String,
    /// Project-relative path of the built binary.
    binary: String,
    /// Length of the built binary in bytes.
    binary_len: u64,
    /// blake3 of the built binary's bytes.
    binary_hash: String,
}

/// Every file whose content the worker build depends on, sorted by project-relative path.
///
/// `target/` and `cache/` are gnr8's own outputs and are excluded. Anything that is not a regular
/// file or directory — a symlink, a fifo — makes the set unrepresentable, which is reported rather
/// than silently skipped.
fn workspace_input_files(dir: &Path) -> Result<Vec<PathBuf>, CoreError> {
    fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CoreError> {
        let entries = std::fs::read_dir(dir).map_err(|err| CoreError::WorkerBuild {
            message: format!("failed to read {}: {err}", dir.display()),
        })?;
        for entry in entries {
            let entry = entry.map_err(|err| CoreError::WorkerBuild {
                message: format!("failed to read an entry under {}: {err}", dir.display()),
            })?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|err| CoreError::WorkerBuild {
                message: format!("failed to stat {}: {err}", path.display()),
            })?;
            let name = path.file_name().and_then(|name| name.to_str());
            if dir == root && matches!(name, Some("target" | "cache")) {
                continue;
            }
            if kind.is_symlink() {
                return Err(CoreError::WorkerBuild {
                    message: format!(
                        "{} is a symlink; gnr8 refuses to build a .gnr8 workspace whose contents it \
                         cannot fingerprint by content",
                        path.display()
                    ),
                });
            }
            if kind.is_dir() {
                collect(root, &path, out)?;
            } else if kind.is_file() {
                out.push(path);
            } else {
                return Err(CoreError::WorkerBuild {
                    message: format!(
                        "{} is neither a regular file nor a directory; gnr8 cannot fingerprint it",
                        path.display()
                    ),
                });
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();
    Ok(files)
}

/// Hash every input once, folding each file into both accumulators in a single pass.
///
/// `skip` names the one relative path the second accumulator leaves out — `Cargo.lock`, which cargo
/// itself may rewrite during the build and which therefore cannot be part of the concurrent-edit
/// bracket.
fn hash_paths(root: &Path, paths: &[PathBuf], skip: &str) -> Result<(String, String), CoreError> {
    // The reads are spread across the machine's cores; `map_ordered` keeps them in `paths` order, so
    // both accumulators fold the same sequence at any thread count.
    let entries = crate::parallel::map_ordered(paths, |path| {
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel = rel.to_string_lossy().replace('\\', "/");
        let bytes = std::fs::read(path).map_err(|err| CoreError::WorkerBuild {
            message: format!("failed to read {}: {err}", path.display()),
        })?;
        Ok((rel, format!("\0{}\n", blake3_hex(&bytes))))
    })?;
    let mut complete = blake3::Hasher::new();
    let mut authored = blake3::Hasher::new();
    complete.update(b"gnr8-worker-v1\n");
    authored.update(b"gnr8-worker-v1\n");
    for (rel, digest) in &entries {
        complete.update(rel.as_bytes());
        complete.update(digest.as_bytes());
        if rel != skip {
            authored.update(rel.as_bytes());
            authored.update(digest.as_bytes());
        }
    }
    Ok((
        complete.finalize().to_hex().to_string(),
        authored.finalize().to_hex().to_string(),
    ))
}

/// The content hash of the running `gnr8` executable.
///
/// This is what makes the build stamp safe when `.gnr8/Cargo.toml` uses a path dependency on an
/// in-repo SDK: changing the SDK forces a host rebuild, which changes this hash, which invalidates
/// every worker stamp.
fn host_executable_hash() -> Result<String, CoreError> {
    let exe = std::env::current_exe().map_err(|err| CoreError::WorkerBuild {
        message: format!("failed to resolve the gnr8 executable: {err}"),
    })?;
    let (_, hash) = blake3_file(&exe).map_err(|err| CoreError::WorkerBuild {
        message: format!("failed to read {}: {err}", exe.display()),
    })?;
    Ok(hash)
}

/// The two fingerprints of a `.gnr8/` workspace.
struct Fingerprints {
    /// Every input, including `Cargo.lock` — the identity the stamp records.
    complete: String,
    /// Every input except `Cargo.lock` — the user-authored surface, bracketed across the build so a
    /// concurrent edit cannot be recorded as if it had been compiled.
    authored: String,
}

fn fingerprints(workspace: &Workspace, host_hash: &str) -> Result<Fingerprints, CoreError> {
    let files = workspace_input_files(&workspace.dir)?;
    fingerprints_of(workspace, host_hash, &files)
}

/// The two fingerprints over an already-enumerated input set.
fn fingerprints_of(
    workspace: &Workspace,
    host_hash: &str,
    files: &[PathBuf],
) -> Result<Fingerprints, CoreError> {
    let prefix = format!(
        "{host_hash}\n{}\n{}\n",
        gnr8::protocol::PROTOCOL_VERSION,
        gnr8::protocol::capability_digest(gnr8::protocol::sdk_version())
    );
    let (complete, authored) = hash_paths(&workspace.dir, files, "Cargo.lock")?;
    Ok(Fingerprints {
        complete: blake3_hex(format!("{prefix}{complete}").as_bytes()),
        authored: blake3_hex(format!("{prefix}{authored}").as_bytes()),
    })
}

fn read_stamp(workspace: &Workspace) -> Option<WorkerStamp> {
    let bytes = std::fs::read(workspace.stamp_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn binary_identity(path: &Path) -> Option<(u64, String)> {
    blake3_file(path).ok()
}

/// Whether a recorded stamp still describes the binary on disk.
///
/// `on_disk` is that binary's already-read identity, or `None` if there is no binary there.
fn stamp_matches(
    workspace: &Workspace,
    stamp: &WorkerStamp,
    fingerprint: &str,
    on_disk: Option<&(u64, String)>,
) -> bool {
    if stamp.fingerprint != fingerprint {
        return false;
    }
    let binary = workspace.binary_path();
    if project_relative(&workspace.project_root, &binary) != stamp.binary {
        return false;
    }
    on_disk.is_some_and(|(len, hash)| *len == stamp.binary_len && *hash == stamp.binary_hash)
}

fn project_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Confirm the worker binary the host is about to execute really lives inside `.gnr8/target`.
///
/// The path is composed from the manifest's package name, which is already restricted to Cargo's
/// name charset, so it cannot contain a traversal. What this adds is the symlink case: `.gnr8/target`
/// is deliberately excluded from the build fingerprint — it is gnr8's own output — so a symlinked
/// `target/`, the profile directory under it, or the binary would otherwise redirect execution without ever appearing
/// as a changed input. Canonicalizing would not catch it, because both sides resolve through the
/// same link; each level is therefore checked with `symlink_metadata`.
///
/// The cost is that gnr8 will not run a worker through a symlinked build directory at all. That is
/// intentional: gnr8 pins `--target-dir` itself and treats that subtree as its own.
///
/// # Errors
///
/// Returns [`CoreError::WorkerBuild`] when any level of the path is not a real entry.
fn confirm_binary_is_inside_the_workspace(
    workspace: &Workspace,
    binary: &Path,
) -> Result<(), CoreError> {
    require_real_worker_path(&workspace.target_dir(), EntryKind::Directory)?;
    require_real_worker_path(&workspace.profile_dir(), EntryKind::Directory)?;
    require_real_worker_path(binary, EntryKind::File)
}

fn require_real_worker_path(path: &Path, kind: EntryKind) -> Result<(), CoreError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|err| CoreError::WorkerBuild {
        message: format!(
            "failed to stat the built worker path {}: {err}",
            path.display()
        ),
    })?;
    let ok = match kind {
        EntryKind::Directory => metadata.is_dir(),
        EntryKind::File => metadata.is_file(),
    };
    if metadata.file_type().is_symlink() || !ok {
        return Err(CoreError::WorkerBuild {
            message: format!(
                "{} is a symlink or not a plain build output; gnr8 refuses to run it. \
                 .gnr8/target is gnr8's own build directory and is excluded from the worker \
                 fingerprint, so it must not be redirected.",
                path.display()
            ),
        });
    }
    Ok(())
}

/// The outcome of making a worker binary available.
#[derive(Debug, Clone)]
pub struct WorkerBinary {
    /// The executable to run.
    pub path: PathBuf,
    /// Whether `cargo` was invoked to produce it.
    pub built: bool,
}

/// Ensure a runnable worker binary exists for `workspace`, building it only when its inputs changed.
///
/// # Errors
///
/// Returns [`CoreError::WorkerBuild`] when the policy forbids building but no matching binary
/// exists, when `cargo` cannot be spawned or fails, when the built binary is missing, or when the
/// `.gnr8/` sources changed while cargo was running.
pub fn ensure_worker(
    workspace: &Workspace,
    policy: WorkerPolicy,
) -> Result<WorkerBinary, CoreError> {
    // Deciding whether the recorded worker is still current means reading the host executable, every
    // `.gnr8/` input, and the built worker — tens of megabytes of unrelated files. They are read at
    // the same time rather than one after another.
    let ((host_hash, workspace_files), recorded_binary) = crate::parallel::join(
        || {
            let host_hash = host_executable_hash()?;
            let files = workspace_input_files(&workspace.dir)?;
            Ok((host_hash, files))
        },
        || Ok(binary_identity(&workspace.binary_path())),
    )?;
    let before = fingerprints_of(workspace, &host_hash, &workspace_files)?;
    if let Some(stamp) = read_stamp(workspace) {
        if stamp_matches(
            workspace,
            &stamp,
            &before.complete,
            recorded_binary.as_ref(),
        ) {
            let binary = workspace.binary_path();
            confirm_binary_is_inside_the_workspace(workspace, &binary)?;
            return Ok(WorkerBinary {
                path: binary,
                built: false,
            });
        }
    }
    if !policy.allow_build {
        return Err(CoreError::WorkerBuild {
            message: format!(
                "the .gnr8 worker for {} is missing or out of date, and building was refused. \
                 Building .gnr8/ compiles and runs Rust from this repository; re-run without \
                 --no-build to allow it.",
                workspace.project_root.display()
            ),
        });
    }

    ensure_lockfile(workspace)?;
    cargo_build(workspace)?;

    let binary = workspace.binary_path();
    let (binary_len, binary_hash) =
        binary_identity(&binary).ok_or_else(|| CoreError::WorkerBuild {
            message: format!(
                "cargo reported success but no worker binary is at {}. Check that .gnr8/Cargo.toml \
                 declares a [[bin]] or src/main.rs.",
                binary.display()
            ),
        })?;

    confirm_binary_is_inside_the_workspace(workspace, &binary)?;

    let after = fingerprints(workspace, &host_hash)?;
    if after.authored != before.authored {
        return Err(CoreError::WorkerBuild {
            message: "the .gnr8 sources changed while cargo was building them; no worker was \
                      accepted — rerun"
                .to_string(),
        });
    }
    write_stamp(
        workspace,
        &WorkerStamp {
            fingerprint: after.complete,
            binary: project_relative(&workspace.project_root, &binary),
            binary_len,
            binary_hash,
        },
    );
    Ok(WorkerBinary {
        path: binary,
        built: true,
    })
}

/// Publish the build stamp atomically.
///
/// Two `gnr8` processes in one project can reach this at the same time. A torn stamp would only cost
/// an extra rebuild — a partial JSON does not parse, and `read_stamp` treats that as "no stamp" — but
/// write-then-rename removes the window entirely, and the temporary name carries the writer's pid so
/// two concurrent publishes never collide.
fn write_stamp(workspace: &Workspace, stamp: &WorkerStamp) {
    let path = workspace.stamp_path();
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(stamp) else {
        return;
    };
    let temporary = parent.join(format!(".worker.json.{}.tmp", std::process::id()));
    if std::fs::write(&temporary, bytes).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return;
    }
    if std::fs::rename(&temporary, &path).is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
}

fn ensure_lockfile(workspace: &Workspace) -> Result<(), CoreError> {
    if workspace.dir.join("Cargo.lock").is_file() {
        return Ok(());
    }
    let mut command = cargo_command();
    command
        .arg("generate-lockfile")
        .arg("--manifest-path")
        .arg(&workspace.manifest)
        .current_dir(&workspace.project_root);
    let output = command.output().map_err(|err| CoreError::WorkerBuild {
        message: format!(
            "failed to create .gnr8/Cargo.lock with {:?}: {err} — is Rust/cargo installed and on \
             PATH? (override the cargo binary with ${GNR8_CARGO_ENV} if needed)",
            cargo_binary()
        ),
    })?;
    if !output.status.success() {
        return Err(CoreError::WorkerBuild {
            message: format!(
                "failed to create .gnr8/Cargo.lock before building the worker:\n{}",
                String::from_utf8_lossy(&output.stderr).trim_end()
            ),
        });
    }
    Ok(())
}

fn cargo_build(workspace: &Workspace) -> Result<(), CoreError> {
    let mut command = cargo_command();
    command.arg("build").arg("--quiet");
    for setting in WORKER_PROFILE_CONFIG {
        command.arg("--config").arg(setting);
    }
    command
        .arg("--profile")
        .arg(WORKER_PROFILE)
        .arg("--manifest-path")
        .arg(&workspace.manifest)
        .arg("--target-dir")
        .arg(workspace.target_dir())
        .current_dir(&workspace.project_root);
    let output = command.output().map_err(|err| CoreError::WorkerBuild {
        message: format!(
            "failed to build the .gnr8 worker with {:?} ({err}) — is Rust/cargo installed and on \
             PATH? (override the cargo binary with ${GNR8_CARGO_ENV} if needed)",
            cargo_binary()
        ),
    })?;
    if !output.status.success() {
        return Err(CoreError::WorkerBuild {
            message: format!(
                "the .gnr8 worker did not compile ({}). This is usually an error in your \
                 .gnr8/src/main.rs pipeline. cargo output:\n{}",
                describe_status(output.status),
                String::from_utf8_lossy(&output.stderr).trim_end()
            ),
        });
    }
    Ok(())
}

fn cargo_command() -> Command {
    let mut command = Command::new(cargo_binary());
    if std::env::var(GNR8_CARGO_OFFLINE_ENV).is_ok_and(|value| value == "1") {
        command.arg("--offline");
    }
    command
}

/// The cargo binary to invoke: `$GNR8_CARGO`, else `$CARGO`, else `cargo`.
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

fn describe_status(status: std::process::ExitStatus) -> String {
    status.code().map_or_else(
        || "no exit code (terminated by signal)".to_string(),
        |code| format!("exit code {code}"),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{exact_pin, validate_workspace, workspace_input_files, WorkerPolicy};
    use std::path::PathBuf;

    fn temp_project(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "gnr8-worker-build-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join(".gnr8/src")).unwrap();
        dir
    }

    fn write_manifest(root: &std::path::Path, body: &str) {
        std::fs::write(root.join(".gnr8/Cargo.toml"), body).unwrap();
        std::fs::write(root.join(".gnr8/src/main.rs"), "fn main() {}\n").unwrap();
    }

    const CURRENT: &str = "[package]\nname = \"x-gnr8-gen\"\nversion = \"0.1.0\"\n\n[dependencies]\ngnr8 = \"=0.9.0\"\n";

    #[test]
    fn a_current_manifest_validates() {
        let root = temp_project("ok");
        write_manifest(&root, CURRENT);
        let workspace = validate_workspace(&root).unwrap();
        assert_eq!(workspace.package, "x-gnr8-gen");
        assert!(workspace.binary_path().starts_with(workspace.target_dir()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_previous_contract_is_rejected_before_anything_is_built() {
        let root = temp_project("old-pin");
        write_manifest(
            &root,
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[dependencies]\ngnr8 = \"=0.8.0\"\n",
        );
        let err = validate_workspace(&root).unwrap_err();
        assert!(err.to_string().contains("previous .gnr8 contract"), "{err}");
        assert!(err.to_string().contains("gnr8 init --upgrade"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_dependency_on_the_host_engine_is_rejected() {
        for package in ["gnr8-engine", "gnr8-core"] {
            let root = temp_project("engine-dep");
            write_manifest(
                &root,
                &format!(
                    "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[dependencies]\ngnr8 = \"=0.9.0\"\n{package} = \"0.9.0\"\n"
                ),
            );
            let err = validate_workspace(&root).unwrap_err();
            assert!(err.to_string().contains("host engine"), "{err}");
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn a_manifest_without_the_sdk_dependency_is_rejected() {
        let root = temp_project("no-dep");
        write_manifest(&root, "[package]\nname = \"x\"\nversion = \"0.1.0\"\n");
        let err = validate_workspace(&root).unwrap_err();
        assert!(err.to_string().contains("no `gnr8` dependency"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_workspace_names_gnr8_init() {
        let root = temp_project("missing");
        std::fs::remove_dir_all(root.join(".gnr8")).unwrap();
        let err = validate_workspace(&root).unwrap_err();
        assert!(err.to_string().contains("gnr8 init"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_workspace_is_refused() {
        let root = temp_project("symlink");
        write_manifest(&root, CURRENT);
        let real = root.join("real-gnr8");
        std::fs::rename(root.join(".gnr8"), &real).unwrap();
        std::os::unix::fs::symlink(&real, root.join(".gnr8")).unwrap();
        let err = validate_workspace(&root).unwrap_err();
        assert!(err.to_string().contains("real directory"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_source_file_cannot_be_fingerprinted() {
        let root = temp_project("symlink-src");
        write_manifest(&root, CURRENT);
        let outside = root.join("outside.rs");
        std::fs::write(&outside, "fn main() {}\n").unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".gnr8/src/extra.rs")).unwrap();
        let err = workspace_input_files(&root.join(".gnr8")).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_build_directories_are_excluded_from_the_fingerprint() {
        let root = temp_project("exclude");
        write_manifest(&root, CURRENT);
        std::fs::create_dir_all(root.join(".gnr8/target/debug")).unwrap();
        std::fs::write(root.join(".gnr8/target/debug/x"), "binary").unwrap();
        std::fs::create_dir_all(root.join(".gnr8/cache")).unwrap();
        std::fs::write(root.join(".gnr8/cache/worker.json"), "{}").unwrap();
        let files = workspace_input_files(&root.join(".gnr8")).unwrap();
        assert!(files
            .iter()
            .all(|path| !path.to_string_lossy().contains("/target/")));
        assert!(files
            .iter()
            .all(|path| !path.to_string_lossy().contains("/cache/")));
        assert_eq!(files.len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_build_and_no_execute_are_distinct_policies() {
        assert!(!WorkerPolicy::no_build().allow_build);
        assert!(WorkerPolicy::no_build().allow_execute);
        assert!(!WorkerPolicy::no_execute().allow_build);
        assert!(!WorkerPolicy::no_execute().allow_execute);
        assert!(WorkerPolicy::default().allow_build);
    }

    #[test]
    fn exact_pins_are_recognized_and_ranges_are_left_to_the_handshake() {
        assert_eq!(
            exact_pin(&toml::Value::String("=0.8.0".to_string())),
            Some((0, 8, 0))
        );
        assert_eq!(
            exact_pin(&toml::Value::String("0.9.1".to_string())),
            Some((0, 9, 1))
        );
        assert_eq!(exact_pin(&toml::Value::String("^0.9".to_string())), None);
        assert_eq!(exact_pin(&toml::Value::String(">=0.9".to_string())), None);
        let mut table = toml::map::Map::new();
        table.insert(
            "path".to_string(),
            toml::Value::String("../sdk".to_string()),
        );
        assert_eq!(exact_pin(&toml::Value::Table(table)), None);
    }
}
